import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { TimelineItemView } from "./orbit-ipc.ts";
import {
  computeWorkChunkDurationMs,
  extractFallbackDispatchCall,
  isDelegationMilestoneToolCall,
  splitStepGroupChronologically,
  summarizeWorkChunk,
} from "./turn-delegation-split.ts";

function makeReadTool(seq: number, path: string, timestampMs?: number): TimelineItemView {
  return {
    item_type: "tool_call",
    seq,
    call_id: `read-${seq}`,
    status: "completed",
    summary_title: `Read ${path}`,
    detail: { kind: "file_read", path, start_line: null, end_line: null },
    diff_files: [],
    timestamp_ms: timestampMs ?? null,
  };
}

function makeShellTool(seq: number, command: string, status: "running" | "completed" | "failed" = "completed"): TimelineItemView {
  return {
    item_type: "tool_call",
    seq,
    call_id: `shell-${seq}`,
    status,
    summary_title: command,
    detail: { kind: "shell", command, cwd: null, exit_code: status === "failed" ? 1 : 0, output_preview: null },
    diff_files: [],
  };
}

function makeSendMessageTool(
  seq: number,
  argsJson = "{}",
  status: "running" | "completed" | "failed" | "denied" = "completed",
): TimelineItemView {
  return {
    item_type: "tool_call",
    seq,
    call_id: `msg-${seq}`,
    status,
    summary_title: "Send message",
    detail: {
      kind: "generic",
      tool_name: "send_message",
      arguments_json: argsJson,
    },
    diff_files: [],
  };
}

function makeSubagentTool(
  seq: number,
  role: string,
  promptPreview = "Audit crate",
  status: "running" | "completed" | "failed" | "denied" = "completed",
): TimelineItemView {
  return {
    item_type: "tool_call",
    seq,
    call_id: `sa-${seq}`,
    status,
    summary_title: `Ran ${role}`,
    detail: {
      kind: "subagent",
      role,
      prompt_preview: promptPreview,
      child_agent_id: `child-${seq}`,
    },
    diff_files: [],
  };
}

describe("turn-delegation-split", () => {
  it("keeps step groups without delegation calls untouched on the fast path", () => {
    const items = [makeReadTool(1, "/repo/a.ts"), makeShellTool(2, "cargo test")];
    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, false);
    assert.equal(res.chunks.length, 1);
    assert.equal(res.chunks[0]?.kind, "work");
    assert.equal(res.chunks[0]?.items, items);
  });

  it("extracts trailing send_message out of reads+commands and removes 'used a tool' from summary", () => {
    const items: TimelineItemView[] = [];
    for (let i = 1; i <= 26; i += 1) {
      items.push(makeReadTool(i, `/repo/file_${i}.rs`));
    }
    for (let i = 27; i <= 45; i += 1) {
      items.push(makeShellTool(i, `git log -n ${i}`));
    }
    items.push(makeSendMessageTool(46, "{}"));

    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, true);
    assert.equal(res.chunks.length, 2);

    const [workChunk, delegationChunk] = res.chunks;
    assert.equal(workChunk.kind, "work");
    assert.equal(workChunk.items.length, 45);
    assert.equal(delegationChunk.kind, "delegation");
    assert.equal(delegationChunk.items.length, 1);
    assert.equal(delegationChunk.items[0].call_id, "msg-46");

    const summary = summarizeWorkChunk(workChunk.items);
    assert.equal(summary.label, "Read 26 files, ran 19 commands");
    assert.equal(summary.counts.other_tool_count, 0);
    assert.equal(summary.counts.read_file_count, 26);
    assert.equal(summary.counts.command_count, 19);
  });

  it("splits interleaved work -> parallel subagents -> follow-up work in strict chronological order", () => {
    const items: TimelineItemView[] = [
      makeReadTool(1, "/repo/AGENTS.md"),
      makeReadTool(2, "/repo/Cargo.toml"),
      makeSubagentTool(3, "Codebase Researcher"),
      makeSubagentTool(4, "Security Reviewer"),
      makeShellTool(5, "cargo test --workspace"),
    ];

    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, true);
    assert.equal(res.chunks.length, 3);
    assert.equal(res.chunks[0].kind, "work");
    assert.equal(res.chunks[0].items.length, 2);
    assert.equal(res.chunks[1].kind, "delegation");
    assert.equal(res.chunks[1].items.length, 2);
    assert.equal(res.chunks[2].kind, "work");
    assert.equal(res.chunks[2].items.length, 1);

    assert.equal(summarizeWorkChunk(res.chunks[0].items).label, "Read 2 files");
    assert.equal(summarizeWorkChunk(res.chunks[2].items).label, "Ran a command");
  });

  it("renders pure-delegation step groups with zero empty work chunks", () => {
    const items: TimelineItemView[] = [
      makeSubagentTool(10, "Researcher A"),
      makeSubagentTool(11, "Researcher B"),
    ];
    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, true);
    assert.equal(res.chunks.length, 1);
    assert.equal(res.chunks[0].kind, "delegation");
    assert.equal(res.chunks[0].items.length, 2);
  });

  it("preserves failed generic invoke_subagent calls and extracts fallback dispatch metadata", () => {
    const failedDispatch: TimelineItemView = {
      item_type: "tool_call",
      seq: 20,
      call_id: "inv-20",
      status: "failed",
      summary_title: "Invoke subagent",
      detail: {
        kind: "generic",
        tool_name: "invoke_subagent",
        arguments_json: JSON.stringify({
          Role: "Rust Reviewer",
          Prompt: "Review crates/orbit-cli",
        }),
      },
      diff_files: [],
    };

    assert.equal(isDelegationMilestoneToolCall(failedDispatch), true);
    const res = splitStepGroupChronologically([failedDispatch]);
    assert.equal(res.chunks.length, 1);
    assert.equal(res.chunks[0].kind, "delegation");

    const info = extractFallbackDispatchCall(res.chunks[0].items[0]);
    assert.ok(info);
    assert.equal(info.label, "Rust Reviewer");
    assert.equal(info.preview, "Review crates/orbit-cli");
    assert.equal(info.status, "failed");
  });

  it("ignores legacy empty Subagent placeholder items", () => {
    const legacy: TimelineItemView = {
      item_type: "tool_call",
      seq: 30,
      call_id: "legacy-30",
      status: "completed",
      summary_title: "Subagent",
      detail: {
        kind: "subagent",
        role: "Subagent",
        prompt_preview: "",
        child_agent_id: null,
      },
      diff_files: [],
    };
    assert.equal(isDelegationMilestoneToolCall(legacy), false);
    const res = splitStepGroupChronologically([legacy]);
    assert.equal(res.hasDelegations, false);
  });

  it("computes work chunk duration from item timestamps when spanning >= 1s", () => {
    const items = [
      makeReadTool(1, "/repo/a.ts", 10_000),
      makeReadTool(2, "/repo/b.ts", 14_500),
    ];
    assert.equal(computeWorkChunkDurationMs(items), 4500);
    assert.equal(computeWorkChunkDurationMs([makeReadTool(1, "/repo/a.ts", 10_000)]), null);
  });
});
