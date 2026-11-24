import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  MAX_OSC133_BLOCKS,
  adjustSplitPaneRatios,
  createOsc133Tracker,
  extractOsc133BlockOutput,
  findAdjacentOsc133Block,
  parseOsc133Payload,
  resolveTerminalPaneShortcut,
} from "./xterm-osc133.ts";

describe("xterm-osc133", () => {
  it("parses FinalTerm / iTerm2 / VSCode OSC 133 payloads accurately", () => {
    assert.deepEqual(parseOsc133Payload("A"), { kind: "A", exitCode: null });
    assert.deepEqual(parseOsc133Payload("B"), { kind: "B", exitCode: null });
    assert.deepEqual(parseOsc133Payload("C"), { kind: "C", exitCode: null });
    assert.deepEqual(parseOsc133Payload("D"), { kind: "D", exitCode: null });
    assert.deepEqual(parseOsc133Payload("D;0"), { kind: "D", exitCode: 0 });
    assert.deepEqual(parseOsc133Payload("D;127;err=1"), { kind: "D", exitCode: 127 });
    assert.deepEqual(parseOsc133Payload("P;k=i"), { kind: "P", exitCode: null });
    assert.equal(parseOsc133Payload(""), null);
    assert.equal(parseOsc133Payload("Z;99"), null);
    assert.deepEqual(parseOsc133Payload("D;notanumber"), { kind: "D", exitCode: null });
  });

  it("tracks multi-command lifecycles, closes unclosed commands on next prompt, and enforces bounded ring buffer", () => {
    let clock = 1000;
    const bufferLines = [
      "user@mac repo % cargo test",
      "running 4 tests",
      "test result: ok.",
      "user@mac repo % false",
      "user@mac repo % long-cmd",
      "partial output",
      "user@mac repo % ",
    ];
    const readLine = (row: number) => bufferLines[row] ?? "";

    const tracker = createOsc133Tracker({ maxBlocks: 3, now: () => (clock += 10) });

    // Command 1: A -> B -> C -> D;0
    assert.equal(tracker.handleOsc("A", { row: 0, col: 0 }, readLine), null);
    assert.equal(tracker.handleOsc("B", { row: 0, col: 16 }, readLine), null);
    const afterC1 = tracker.handleOsc("C", { row: 1, col: 0 }, readLine);
    assert.ok(afterC1);
    assert.equal(afterC1.length, 1);
    assert.equal(afterC1[0].commandLine, "cargo test");
    assert.equal(afterC1[0].promptRow, 0);
    assert.equal(afterC1[0].outputStartRow, 1);
    assert.equal(afterC1[0].exitCode, null);

    const afterD1 = tracker.handleOsc("D;0", { row: 2, col: 0 }, readLine);
    assert.ok(afterD1);
    assert.equal(afterD1[0].exitCode, 0);
    assert.equal(afterD1[0].endRow, 2);
    assert.ok(afterD1[0].finishedAt !== null);

    // Command 2: fails with exit 1
    tracker.handleOsc("A", { row: 3, col: 0 }, readLine);
    tracker.handleOsc("B", { row: 3, col: 16 }, readLine);
    tracker.handleOsc("C", { row: 4, col: 0 }, readLine);
    const afterD2 = tracker.handleOsc("D;1", { row: 4, col: 0 }, readLine);
    assert.ok(afterD2);
    assert.equal(afterD2.length, 2);
    assert.equal(afterD2[1].commandLine, "false");
    assert.equal(afterD2[1].exitCode, 1);

    // Command 3: never sends D, closed cleanly by next A
    tracker.handleOsc("A", { row: 4, col: 0 }, readLine);
    tracker.handleOsc("B", { row: 4, col: 16 }, readLine);
    tracker.handleOsc("C", { row: 5, col: 0 }, readLine);
    const afterNextA = tracker.handleOsc("A", { row: 6, col: 0 }, readLine);
    assert.ok(afterNextA);
    assert.equal(afterNextA.length, 3);
    assert.equal(afterNextA[2].endRow, 5);
    assert.ok(afterNextA[2].finishedAt !== null);

    // Command 4 evicts command 1 (maxBlocks = 3)
    tracker.handleOsc("B", { row: 6, col: 16 }, readLine);
    const afterC4 = tracker.handleOsc("C", { row: 7, col: 0 }, readLine);
    assert.ok(afterC4);
    assert.equal(afterC4.length, 3);
    assert.equal(afterC4[0].commandLine, "false");

    // Reset clears everything cleanly
    tracker.reset();
    assert.equal(tracker.getBlocks().length, 0);
  });

  it("caps default ring buffer at MAX_OSC133_BLOCKS (200)", () => {
    const tracker = createOsc133Tracker();
    for (let i = 0; i < MAX_OSC133_BLOCKS + 25; i += 1) {
      tracker.handleOsc("A", { row: i * 2, col: 0 });
      tracker.handleOsc("C", { row: i * 2 + 1, col: 0 });
      tracker.handleOsc("D;0", { row: i * 2 + 1, col: 0 });
    }
    const blocks = tracker.getBlocks();
    assert.equal(blocks.length, MAX_OSC133_BLOCKS);
    assert.equal(blocks[0].id, 26);
    assert.equal(blocks[blocks.length - 1].id, MAX_OSC133_BLOCKS + 25);
  });

  it("jumps between blocks and extracts clean trimmed block output", () => {
    const lines = [
      "$ echo hi",
      "",
      "echo hi",
      "hi there",
      "second line   ",
      "",
      "$ pwd",
      "/workspace",
    ];
    const readLine = (r: number) => lines[r] ?? "";
    const tracker = createOsc133Tracker();
    tracker.handleOsc("A", { row: 0, col: 0 }, readLine);
    tracker.handleOsc("B", { row: 0, col: 2 }, readLine);
    tracker.handleOsc("C", { row: 1, col: 0 }, readLine);
    tracker.handleOsc("D;0", { row: 5, col: 0 }, readLine);

    tracker.handleOsc("A", { row: 6, col: 0 }, readLine);
    tracker.handleOsc("B", { row: 6, col: 2 }, readLine);
    tracker.handleOsc("C", { row: 7, col: 0 }, readLine);
    tracker.handleOsc("D;0", { row: 7, col: 0 }, readLine);

    const blocks = tracker.getBlocks();
    assert.equal(blocks.length, 2);

    // Output extraction trims leading/trailing blanks, trailing whitespace, and duplicate echoed command
    assert.equal(
      extractOsc133BlockOutput(blocks[0], readLine, lines.length),
      "hi there\nsecond line",
    );
    assert.equal(extractOsc133BlockOutput(blocks[1], readLine, lines.length), "/workspace");

    // Adjacent block lookup by selectedBlockId and by viewport row
    assert.equal(findAdjacentOsc133Block(blocks, 7, "prev", blocks[1].id)?.id, blocks[0].id);
    assert.equal(findAdjacentOsc133Block(blocks, 0, "next", blocks[0].id)?.id, blocks[1].id);
    assert.equal(findAdjacentOsc133Block(blocks, 6, "prev", null)?.id, blocks[0].id);
    assert.equal(findAdjacentOsc133Block(blocks, 1, "next", null)?.id, blocks[1].id);
  });

  it("resolves split and command-jump shortcuts cleanly including macOS Option+D code fallback", () => {
    const baseCtx = {
      isMac: true,
      terminalHasFocus: true,
      findOpen: false,
      hasCommandBlocks: true,
      canSplit: true,
    };

    assert.equal(
      resolveTerminalPaneShortcut(
        { key: "D", code: "KeyD", metaKey: true, ctrlKey: false, shiftKey: true, altKey: false },
        baseCtx,
      ),
      "split_horizontal",
    );
    // macOS Option+D emits "∂" in event.key, so code === "KeyD" must match
    assert.equal(
      resolveTerminalPaneShortcut(
        { key: "∂", code: "KeyD", metaKey: true, ctrlKey: false, shiftKey: false, altKey: true },
        baseCtx,
      ),
      "split_vertical",
    );
    assert.equal(
      resolveTerminalPaneShortcut(
        { key: "ArrowUp", metaKey: true, ctrlKey: false, shiftKey: false, altKey: false },
        baseCtx,
      ),
      "prev_command",
    );
    assert.equal(
      resolveTerminalPaneShortcut(
        { key: "ArrowDown", metaKey: true, ctrlKey: false, shiftKey: false, altKey: false },
        baseCtx,
      ),
      "next_command",
    );

    // Blocked when unfocused, find bar open, or no command blocks
    assert.equal(
      resolveTerminalPaneShortcut(
        { key: "D", code: "KeyD", metaKey: true, ctrlKey: false, shiftKey: true, altKey: false },
        { ...baseCtx, terminalHasFocus: false },
      ),
      null,
    );
    assert.equal(
      resolveTerminalPaneShortcut(
        { key: "ArrowUp", metaKey: true, ctrlKey: false, shiftKey: false, altKey: false },
        { ...baseCtx, findOpen: true },
      ),
      null,
    );
    assert.equal(
      resolveTerminalPaneShortcut(
        { key: "ArrowUp", metaKey: true, ctrlKey: false, shiftKey: false, altKey: false },
        { ...baseCtx, hasCommandBlocks: false },
      ),
      null,
    );
  });

  it("clamps and redistributes split pane ratios cleanly while keeping sum === 1", () => {
    const adjusted = adjustSplitPaneRatios([0.5, 0.5], 0, 0.25, 0.18);
    assert.ok(Math.abs(adjusted[0] - 0.75) < 1e-6);
    assert.ok(Math.abs(adjusted[1] - 0.25) < 1e-6);

    const clamped = adjustSplitPaneRatios([0.5, 0.5], 0, 0.9, 0.18);
    assert.ok(Math.abs(clamped[0] - 0.82) < 1e-6);
    assert.ok(Math.abs(clamped[1] - 0.18) < 1e-6);

    const threeWay = adjustSplitPaneRatios([0.34, 0.33, 0.33], 1, -0.1, 0.18);
    assert.ok(Math.abs(threeWay[0] - 0.34) < 1e-6);
    assert.ok(Math.abs(threeWay[1] - 0.23) < 1e-6);
    assert.ok(Math.abs(threeWay[2] - 0.43) < 1e-6);
  });
});
