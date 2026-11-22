import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { GitReviewView, StageTurnView } from "./orbit-ipc.ts";
import {
  MAX_CACHED_STAGE_VIEWS,
  collectPinnedStageKeys,
  computeGitReviewSignature,
  computeSubagentTurnsFingerprint,
  evictInactiveStageEntries,
  mergeGitReviewSnapshot,
  normalizeCwdForCompare,
  recordStageAccess,
  removeStageAccessKeys,
  resolveSubagentTabId,
  upsertSubagentSidePanelTab,
  type StageCachePinTab,
} from "./stage-cache.ts";

function makeBaseTurn(overrides?: Partial<StageTurnView>): StageTurnView {
  return {
    key: "turn-1",
    turn_index: 0,
    anchor_seq: 1,
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
    work_summary_label: "",
    work_summary_live_label: "",
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
    ...overrides,
  };
}

describe("stage-cache LRU eviction", () => {
  it("evicts oldest unpinned stage views beyond the cap while preserving LRU ordering", () => {
    let stages: Record<string, { rev: number }> = {};
    let order: string[] = [];
    const pinned = new Set<string>();

    for (let i = 1; i <= 5; i += 1) {
      const k = `h1::a${i}`;
      stages[k] = { rev: i };
      order = recordStageAccess(order, k);
    }

    // Revisit a1 so a2 becomes the oldest unpinned entry
    order = recordStageAccess(order, "h1::a1");

    const res = evictInactiveStageEntries(stages, order, pinned, 3);
    assert.deepEqual(res.evictedKeys, ["h1::a2", "h1::a3"]);
    assert.deepEqual(Object.keys(res.stages).sort(), ["h1::a1", "h1::a4", "h1::a5"]);
    assert.deepEqual(res.accessOrder, ["h1::a4", "h1::a5", "h1::a1"]);
  });

  it("never evicts pinned selected thread, managed subagent, peer split, or provider subagent parent", () => {
    const pinned = collectPinnedStageKeys({
      selectedHostId: "host-a",
      selectedAgentId: "selected-1",
      isDraftNewSession: false,
      sidePanelTabs: [
        { kind: "subagent", hostId: "host-b", childAgentId: "child-1", parentAgentId: "selected-1" },
        { kind: "peer", hostId: "host-a", peerAgentId: "peer-1" },
        { kind: "subagent", providerSubagentId: "ps-1", parentAgentId: "parent-1" },
      ],
    });

    assert.ok(pinned.has("host-a::selected-1"));
    assert.ok(pinned.has("host-b::child-1"));
    assert.ok(pinned.has("host-a::peer-1"));
    assert.ok(pinned.has("host-a::parent-1"));

    const stages: Record<string, number> = {
      "host-a::selected-1": 1,
      "host-b::child-1": 2,
      "host-a::peer-1": 3,
      "host-a::parent-1": 4,
      "host-a::stale-1": 5,
      "host-a::stale-2": 6,
      "host-a::recent-1": 7,
    };
    const order = [
      "host-a::selected-1",
      "host-b::child-1",
      "host-a::peer-1",
      "host-a::parent-1",
      "host-a::stale-1",
      "host-a::stale-2",
      "host-a::recent-1",
    ];

    const res = evictInactiveStageEntries(stages, order, pinned, 5);
    assert.deepEqual(res.evictedKeys, ["host-a::stale-1", "host-a::stale-2"]);
    assert.ok("host-a::selected-1" in res.stages);
    assert.ok("host-b::child-1" in res.stages);
    assert.ok("host-a::peer-1" in res.stages);
    assert.ok("host-a::parent-1" in res.stages);
    assert.ok("host-a::recent-1" in res.stages);
  });

  it("handles default MAX_CACHED_STAGE_VIEWS cap, untracked keys, and removal helper cleanly", () => {
    const stages: Record<string, number> = {};
    let order: string[] = [];
    for (let i = 0; i < MAX_CACHED_STAGE_VIEWS + 4; i += 1) {
      const k = `h::agent-${i}`;
      stages[k] = i;
      if (i >= 2) {
        // Leave agent-0 and agent-1 untracked to verify untracked keys are treated as oldest
        order = recordStageAccess(order, k);
      }
    }

    const res = evictInactiveStageEntries(stages, order, new Set(["h::agent-0"]));
    assert.equal(Object.keys(res.stages).length, MAX_CACHED_STAGE_VIEWS);
    assert.ok("h::agent-0" in res.stages, "pinned untracked key must survive");
    assert.deepEqual(res.evictedKeys, ["h::agent-1", "h::agent-2", "h::agent-3", "h::agent-4"]);

    const cleaned = removeStageAccessKeys(res.accessOrder, new Set(["h::agent-5", "h::agent-6"]));
    assert.ok(!cleaned.includes("h::agent-5"));
    assert.ok(!cleaned.includes("h::agent-6"));
  });

  it("keeps all entries when every cached stage is pinned above cap", () => {
    const stages = { "h::1": 1, "h::2": 2, "h::3": 3 };
    const order = ["h::1", "h::2", "h::3"];
    const pinned = new Set(order);
    const res = evictInactiveStageEntries(stages, order, pinned, 2);
    assert.deepEqual(res.evictedKeys, []);
    assert.equal(res.stages, stages);
  });
});

describe("computeSubagentTurnsFingerprint", () => {
  it("changes on live streaming text deltas inside an existing assistant_message segment", () => {
    const turnA = makeBaseTurn({
      has_active_item: true,
      live_segments: [
        {
          kind: "assistant_message",
          key: "seg-msg-1",
          item: {
            item_type: "assistant_message",
            seq: 2,
            item_id: "m1",
            text: "Hello",
            streaming: true,
            blocks: [],
          },
        },
      ],
    });
    const turnB = makeBaseTurn({
      has_active_item: true,
      live_segments: [
        {
          kind: "assistant_message",
          key: "seg-msg-1",
          item: {
            item_type: "assistant_message",
            seq: 2,
            item_id: "m1",
            text: "Hello world, streaming delta",
            streaming: true,
            blocks: [],
          },
        },
      ],
    });
    const turnC = makeBaseTurn({
      has_active_item: false,
      live_segments: [
        {
          kind: "assistant_message",
          key: "seg-msg-1",
          item: {
            item_type: "assistant_message",
            seq: 2,
            item_id: "m1",
            text: "Hello world, streaming delta",
            streaming: false,
            blocks: [],
          },
        },
      ],
    });

    const fpA = computeSubagentTurnsFingerprint([turnA]);
    const fpB = computeSubagentTurnsFingerprint([turnB]);
    const fpC = computeSubagentTurnsFingerprint([turnC]);
    assert.notEqual(fpA, fpB);
    assert.notEqual(fpB, fpC);
  });

  it("changes on terminal_reply streaming deltas, thinking deltas, tool output/status updates, notices, and diffs", () => {
    const baseReply = makeBaseTurn({
      terminal_reply: {
        item_type: "assistant_message",
        seq: 10,
        item_id: "r1",
        text: "Draft",
        streaming: true,
        blocks: [],
      },
    });
    const expandedReply = makeBaseTurn({
      terminal_reply: {
        item_type: "assistant_message",
        seq: 10,
        item_id: "r1",
        text: "Draft answer updated",
        streaming: true,
        blocks: [],
      },
    });
    const settledReply = makeBaseTurn({
      terminal_reply: {
        item_type: "assistant_message",
        seq: 10,
        item_id: "r1",
        text: "Draft answer updated",
        streaming: false,
        blocks: [],
      },
    });
    assert.notEqual(
      computeSubagentTurnsFingerprint([baseReply]),
      computeSubagentTurnsFingerprint([expandedReply]),
    );
    assert.notEqual(
      computeSubagentTurnsFingerprint([expandedReply]),
      computeSubagentTurnsFingerprint([settledReply]),
    );

    const stepGroupBase = makeBaseTurn({
      live_segments: [
        {
          kind: "step_group",
          key: "sg-1",
          first_seq: 1,
          has_following_content: false,
          has_running_step: true,
          summary: {
            counts: makeBaseTurn().work_counts,
            label: "1 step",
            live_label: "Running",
            label_includes_failure: false,
          },
          items: [
            { item_type: "assistant_thinking", seq: 1, item_id: "th-1", text: "Checking" },
            {
              item_type: "tool_call",
              seq: 2,
              call_id: "c1",
              status: "running",
              summary_title: "cargo test",
              detail: {
                kind: "shell",
                command: "cargo test",
                cwd: null,
                exit_code: null,
                output_preview: "running 1 test",
              },
              diff_files: [],
            },
          ],
        },
      ],
    });

    const thinkingDelta = structuredClone(stepGroupBase);
    if (thinkingDelta.live_segments[0]?.kind === "step_group") {
      const th = thinkingDelta.live_segments[0].items[0];
      if (th?.item_type === "assistant_thinking") th.text += " more thoughts";
    }
    assert.notEqual(
      computeSubagentTurnsFingerprint([stepGroupBase]),
      computeSubagentTurnsFingerprint([thinkingDelta]),
    );

    const toolOutputDelta = structuredClone(stepGroupBase);
    if (toolOutputDelta.live_segments[0]?.kind === "step_group") {
      const tc = toolOutputDelta.live_segments[0].items[1];
      if (tc?.item_type === "tool_call" && tc.detail.kind === "shell") {
        tc.detail.output_preview = "running 1 test\nok";
      }
    }
    assert.notEqual(
      computeSubagentTurnsFingerprint([stepGroupBase]),
      computeSubagentTurnsFingerprint([toolOutputDelta]),
    );

    const toolStatusDelta = structuredClone(toolOutputDelta);
    if (toolStatusDelta.live_segments[0]?.kind === "step_group") {
      const tc = toolStatusDelta.live_segments[0].items[1];
      if (tc?.item_type === "tool_call") tc.status = "completed";
    }
    assert.notEqual(
      computeSubagentTurnsFingerprint([toolOutputDelta]),
      computeSubagentTurnsFingerprint([toolStatusDelta]),
    );

    const withNotice = makeBaseTurn({
      notices: [{ item_type: "notice", seq: 3, level: "info", message: "Done" }],
    });
    const withDiff = makeBaseTurn({
      turn_diffs: [{ path: "src/app.ts", additions: 5, deletions: 1 }],
    });
    assert.notEqual(
      computeSubagentTurnsFingerprint([makeBaseTurn()]),
      computeSubagentTurnsFingerprint([withNotice]),
    );
    assert.notEqual(
      computeSubagentTurnsFingerprint([makeBaseTurn()]),
      computeSubagentTurnsFingerprint([withDiff]),
    );

    // Earlier-segment tool completion & same-file diff line count update also invalidate fingerprint
    const multiSegTurn = makeBaseTurn({
      live_segments: [
        {
          kind: "step_group",
          key: "sg-early",
          first_seq: 1,
          has_following_content: true,
          has_running_step: true,
          summary: {
            counts: makeBaseTurn().work_counts,
            label: "1 step",
            live_label: "Running",
            label_includes_failure: false,
          },
          items: [
            {
              item_type: "tool_call",
              seq: 1,
              call_id: "c-early",
              status: "running",
              summary_title: "npm test",
              detail: {
                kind: "shell",
                command: "npm test",
                cwd: null,
                exit_code: null,
                output_preview: "running",
              },
              diff_files: [],
            },
          ],
        },
        {
          kind: "assistant_message",
          key: "seg-later",
          item: {
            item_type: "assistant_message",
            seq: 2,
            item_id: "m-later",
            text: "Working on it",
            streaming: false,
            blocks: [],
          },
        },
      ],
      turn_diffs: [{ path: "src/app.ts", additions: 5, deletions: 1 }],
    });
    const earlyToolFinished = structuredClone(multiSegTurn);
    if (earlyToolFinished.live_segments[0]?.kind === "step_group") {
      const tc = earlyToolFinished.live_segments[0].items[0];
      if (tc?.item_type === "tool_call") tc.status = "completed";
    }
    const diffCountUpdated = structuredClone(multiSegTurn);
    diffCountUpdated.turn_diffs[0].additions = 12;

    assert.notEqual(
      computeSubagentTurnsFingerprint([multiSegTurn]),
      computeSubagentTurnsFingerprint([earlyToolFinished]),
    );
    assert.notEqual(
      computeSubagentTurnsFingerprint([multiSegTurn]),
      computeSubagentTurnsFingerprint([diffCountUpdated]),
    );
  });
});

describe("upsertSubagentSidePanelTab (multi-subagent tabs)", () => {
  type TestSidePanelTab = StageCachePinTab & { id: string; title: string };

  it("opens distinct tabs for Subagent A and Subagent B without overwriting A, and focuses A without duplicating on re-open", () => {
    const initialTabs: TestSidePanelTab[] = [
      { id: "tab:review", kind: "review", title: "Review" },
    ];

    // 1. Open provider-native Subagent A
    const specA = {
      callId: "call-a",
      role: "Codebase Researcher",
      promptPreview: "Audit parser",
      tabHostId: "host-1",
      parentHostId: "host-1",
      parentAgentId: "parent-1",
      effectiveChildAgentId: null,
      effectiveProviderSubagentId: "prov-sub-a",
    };
    const res1 = upsertSubagentSidePanelTab(initialTabs, specA);
    assert.equal(res1.created, true);
    assert.equal(res1.activeTabId, resolveSubagentTabId(specA));
    assert.equal(res1.activeTabId, "tab:subagent:provider:host-1:parent-1:prov-sub-a");
    assert.equal(res1.tabs.length, 2);

    // 2. Open managed child Subagent B -> appends 2nd distinct subagent tab without overwriting A
    const specB = {
      callId: "call-b",
      role: "Rust Core Engineer",
      promptPreview: "Fix shim race",
      tabHostId: "host-2",
      parentHostId: "host-1",
      parentAgentId: "parent-1",
      effectiveChildAgentId: "child-agent-b",
      effectiveProviderSubagentId: null,
    };
    const res2 = upsertSubagentSidePanelTab(res1.tabs, specB);
    assert.equal(res2.created, true);
    assert.equal(res2.activeTabId, "tab:subagent:agent:host-2:child-agent-b");
    assert.equal(res2.tabs.length, 3);
    assert.equal(res2.tabs[1].id, res1.activeTabId);
    assert.equal(res2.tabs[2].id, res2.activeTabId);

    // 3. Re-opening Subagent A focuses A and updates metadata in place without creating a 3rd subagent tab
    const res3 = upsertSubagentSidePanelTab(res2.tabs, {
      ...specA,
      role: "Codebase Researcher (Updated)",
      promptPreview: "Audit parser & tests",
    });
    assert.equal(res3.created, false);
    assert.equal(res3.activeTabId, res1.activeTabId);
    assert.equal(res3.tabs.length, 3);
    assert.equal(res3.tabs[1].title, "Codebase Researcher (Updated)");
    assert.equal(res3.tabs[1].subagentPrompt, "Audit parser & tests");

    // 4. Fallback callId-only subagent gets a distinct call-scoped tab
    const res4 = upsertSubagentSidePanelTab(res3.tabs, {
      callId: "call-c",
      role: "Quick Helper",
      promptPreview: "Summarize diff",
      tabHostId: "host-1",
      parentHostId: "host-1",
      parentAgentId: "parent-1",
      effectiveChildAgentId: null,
      effectiveProviderSubagentId: null,
    });
    assert.equal(res4.created, true);
    assert.equal(res4.activeTabId, "tab:subagent:call:call-c");
    assert.equal(res4.tabs.length, 4);

    // 5. Late ID binding for call-c upgrades the fallback tab in place without duplicating it
    const res5 = upsertSubagentSidePanelTab(res4.tabs, {
      callId: "call-c",
      role: "Quick Helper",
      promptPreview: "Summarize diff",
      tabHostId: "host-1",
      parentHostId: "host-1",
      parentAgentId: "parent-1",
      effectiveChildAgentId: null,
      effectiveProviderSubagentId: "prov-sub-c",
    });
    assert.equal(res5.created, false);
    assert.equal(res5.tabs.length, 4);
    assert.equal(res5.activeTabId, "tab:subagent:call:call-c");
    assert.equal(res5.tabs[3].providerSubagentId, "prov-sub-c");

    // Subsequent open by bound providerSubagentId matches upgraded tab with zero duplicates
    const res6 = upsertSubagentSidePanelTab(res5.tabs, {
      callId: "provider:prov-sub-c",
      role: "Quick Helper",
      promptPreview: "Summarize diff",
      tabHostId: "host-1",
      parentHostId: "host-1",
      parentAgentId: "parent-1",
      effectiveChildAgentId: null,
      effectiveProviderSubagentId: "prov-sub-c",
    });
    assert.equal(res6.created, false);
    assert.equal(res6.tabs.length, 4);
    assert.equal(res6.activeTabId, "tab:subagent:call:call-c");
  });
});

describe("git review snapshot merge & CWD normalization", () => {
  it("normalizes trailing slashes without corrupting root paths", () => {
    assert.equal(normalizeCwdForCompare("/Users/jess/repo///"), "/Users/jess/repo");
    assert.equal(normalizeCwdForCompare("  /Users/jess/repo/ "), "/Users/jess/repo");
    assert.equal(normalizeCwdForCompare("/Users/jess/repo"), "/Users/jess/repo");
    assert.equal(normalizeCwdForCompare("/"), "/");
    assert.equal(normalizeCwdForCompare("C:\\"), "C:\\");
    assert.equal(normalizeCwdForCompare("c:\\"), "C:\\");
    assert.equal(normalizeCwdForCompare("c:/repo/"), "C:/repo");
    assert.equal(normalizeCwdForCompare("C:/repo/"), "C:/repo");
    assert.equal(normalizeCwdForCompare("\\\\?\\c:\\Users\\jess\\orbit\\"), "C:\\Users\\jess\\orbit");
    assert.equal(normalizeCwdForCompare("\\\\?\\C:\\Users\\jess\\orbit\\"), "C:\\Users\\jess\\orbit");
    assert.equal(normalizeCwdForCompare("\\\\?\\C:\\"), "C:\\");
    assert.equal(normalizeCwdForCompare("\\\\?\\UNC\\server\\share\\repo\\"), "\\\\server\\share\\repo");
    assert.equal(normalizeCwdForCompare(""), "");
    assert.equal(normalizeCwdForCompare(null), "");
  });

  it("preserves workspace-wide diff_files totals, prunes stale files on shrink/clean, and detects real signature changes", () => {
    const prevFull: GitReviewView = {
      revision: 1,
      host_id: "h1",
      cwd: "/repo",
      selected_path: null,
      raw_diff: "full",
      status: {
        cwd: "/repo",
        is_git_repo: true,
        branch: "main",
        upstream: null,
        ahead: 0,
        behind: 0,
        files: [
          { path: "src/a.ts", status: "M", staged: false, untracked: false },
          { path: "src/b.ts", status: "M", staged: false, untracked: false },
        ],
      },
      diff_files: [
        { path: "src/a.ts", additions: 10, deletions: 2, lines: [] },
        { path: "src/b.ts", additions: 25, deletions: 4, lines: [] },
      ],
    };

    // Identical content produces identical signature (breaks WorkspaceFilesResponse -> git_changed loops)
    assert.equal(
      computeGitReviewSignature(prevFull),
      computeGitReviewSignature({ ...prevFull, revision: 99 }),
    );

    // Incoming single-file refresh for src/a.ts only (e.g. selectedDiffPath = "src/a.ts")
    const singleFileIncoming: GitReviewView = {
      ...prevFull,
      revision: 2,
      selected_path: "src/a.ts",
      raw_diff: "single-a",
      diff_files: [{ path: "src/a.ts", additions: 14, deletions: 3, lines: [] }],
    };

    const merged = mergeGitReviewSnapshot(prevFull, singleFileIncoming);
    assert.equal(merged.selected_path, null);
    assert.equal(merged.diff_files.length, 2);
    assert.deepEqual(
      merged.diff_files.map((f) => [f.path, f.additions, f.deletions]),
      [
        ["src/a.ts", 14, 3],
        ["src/b.ts", 25, 4],
      ],
    );
    assert.notEqual(computeGitReviewSignature(prevFull), computeGitReviewSignature(merged));

    // Partial shrink ([a, b] -> [a]) via single-file merge prunes reverted b.ts
    const partialShrinkIncoming: GitReviewView = {
      ...prevFull,
      revision: 3,
      selected_path: "src/a.ts",
      status: {
        ...prevFull.status!,
        files: [{ path: "src/a.ts", status: "M", staged: false, untracked: false }],
      },
      diff_files: [{ path: "src/a.ts", additions: 14, deletions: 3, lines: [] }],
    };
    const shrunk = mergeGitReviewSnapshot(merged, partialShrinkIncoming);
    assert.deepEqual(
      shrunk.diff_files.map((f) => f.path),
      ["src/a.ts"],
    );

    // Full clean ([a, b] -> []) via single-file merge prunes all stale diff_files entries
    const fullCleanSingleFile: GitReviewView = {
      ...prevFull,
      revision: 4,
      selected_path: "src/a.ts",
      raw_diff: "",
      status: { ...prevFull.status!, files: [] },
      diff_files: [],
    };
    const cleanedViaSingle = mergeGitReviewSnapshot(merged, fullCleanSingleFile);
    assert.equal(cleanedViaSingle.diff_files.length, 0);
    assert.equal(cleanedViaSingle.selected_path, null);

    // Full workspace snapshot (selected_path: null) replaces cleanly
    const cleanWorkspace: GitReviewView = {
      ...prevFull,
      revision: 5,
      selected_path: null,
      raw_diff: "",
      status: { ...prevFull.status!, files: [] },
      diff_files: [],
    };
    const cleared = mergeGitReviewSnapshot(merged, cleanWorkspace);
    assert.equal(cleared.diff_files.length, 0);
    assert.equal(cleared.selected_path, null);
  });
});


