import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { Children, createElement, isValidElement, useState, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { StageTurnView, TimelineItemView } from "../../lib/orbit-ipc.ts";

let server: ViteDevServer;
before(async () => {
  server = await createServer({
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [
      {
        name: "test-step-timeline-rail-store-mock",
        enforce: "pre",
        load(id) {
          if (id.endsWith("/state/orbit-store.ts")) {
            return `
              export const orbitActions = {
                openSubagentTab: () => {},
                selectAgent: () => {},
                openInspectorForFile: () => {},
                openFileInSidePanel: () => {},
                prefetchWorkspaceFile: () => {},
                openArtifactsTab: () => {},
              };
              const state = { hostDetails: {}, archivedThreads: {}, stages: {} };
              export const useOrbitSelector = (selector) => selector(state);
              export const getOrbitState = () => state;
            `;
          }
        },
      },
    ],
  });
});

after(async () => {
  await server?.close();
});

function makeReadTool(seq: number, path = `/repo/file_${seq}.ts`): TimelineItemView {
  return {
    item_type: "tool_call",
    seq,
    call_id: `read-${seq}`,
    status: "completed",
    summary_title: `Read ${path}`,
    detail: { kind: "file_read", path, start_line: null, end_line: null },
    diff_files: [],
  };
}

function makeShellTool(
  seq: number,
  command: string,
  status: "running" | "completed" | "failed" | "denied",
  outputPreview = `output-${seq}`,
): TimelineItemView {
  return {
    item_type: "tool_call",
    seq,
    call_id: `shell-${seq}`,
    status,
    summary_title: command,
    detail: {
      kind: "shell",
      command,
      cwd: null,
      exit_code: status === "failed" || status === "denied" ? 1 : 0,
      output_preview: outputPreview,
    },
    diff_files: [],
  };
}

function renderWithActions(
  renderFn: (step: number) => ReactNode,
  stepActions: Array<((actions: Array<() => void>) => void) | null>,
): string {
  const actions: Array<() => void> = [];
  function collect(node: ReactNode) {
    Children.forEach(node, (child) => {
      if (
        !isValidElement<{
          onClick?: () => void;
          open?: boolean;
          onOpenChange?: (open: boolean) => void;
          children?: ReactNode;
        }>(child)
      ) {
        return;
      }
      const { open, onOpenChange, onClick } = child.props;
      if (onOpenChange) actions.push(() => onOpenChange(!open));
      if (onClick) actions.push(onClick);
      collect(child.props.children);
    });
  }

  let currentStep = 0;
  function Harness() {
    const [, setTick] = useState(0);
    actions.length = 0;
    const tree = renderFn(currentStep);
    collect(tree);
    if (currentStep < stepActions.length) {
      const fn = stepActions[currentStep++];
      if (fn) {
        fn(actions);
      } else {
        setTick((n) => n + 1);
      }
    }
    return tree;
  }

  return renderToStaticMarkup(createElement(Harness));
}

describe("StepTimelineRail deterministic earlier-step folding", () => {
  it("keeps earlier steps folded above threshold unless explicitly revealed by user click", async () => {
    const {
      LONG_STEP_LIST_THRESHOLD,
      RECENT_VISIBLE_STEP_COUNT,
      buildStepRailScopeKey,
      isEarlierStepsRevealedForScope,
      resolveStepTimelineRailVisibility,
      findTrailingUnrecoveredFailureStartIndex,
    } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");

    assert.equal(LONG_STEP_LIST_THRESHOLD, 12);
    assert.equal(RECENT_VISIBLE_STEP_COUNT, 8);

    // At or below threshold: nothing hidden
    assert.deepEqual(resolveStepTimelineRailVisibility(12, false), {
      hasOverflow: false,
      hiddenCount: 0,
      visibleStartIndex: 0,
    });

    // Above threshold without explicit click: strictly folds earlier steps
    assert.deepEqual(resolveStepTimelineRailVisibility(13, false), {
      hasOverflow: true,
      hiddenCount: 5,
      visibleStartIndex: 5,
    });
    assert.deepEqual(resolveStepTimelineRailVisibility(25, false), {
      hasOverflow: true,
      hiddenCount: 17,
      visibleStartIndex: 17,
    });

    // Explicit click reveals all steps
    assert.deepEqual(resolveStepTimelineRailVisibility(25, true), {
      hasOverflow: true,
      hiddenCount: 0,
      visibleStartIndex: 0,
    });

    // Scope isolation: optimistic 0 -> confirmed 42 preserves reveal; rewind/turn/agent change resets
    const scopeA = buildStepRailScopeKey("ag_1", 0, 0);
    const scopeB = buildStepRailScopeKey("ag_1", 1, 0);
    assert.equal(isEarlierStepsRevealedForScope(null, scopeA, 42), false);
    assert.equal(isEarlierStepsRevealedForScope({ scopeKey: scopeA, anchorSeq: 0 }, scopeA, 42), true);
    assert.equal(isEarlierStepsRevealedForScope({ scopeKey: scopeA, anchorSeq: 42 }, scopeA, 42), true);
    assert.equal(isEarlierStepsRevealedForScope({ scopeKey: scopeA, anchorSeq: 42 }, scopeA, 55), false);
    assert.equal(isEarlierStepsRevealedForScope({ scopeKey: scopeA, anchorSeq: 42 }, scopeB, 42), false);

    // Trailing unrecovered failure detection ignores earlier recovered failures
    const recoveredItems = [
      makeReadTool(1),
      makeShellTool(2, "rg needle", "failed"),
      makeReadTool(3),
    ];
    assert.equal(findTrailingUnrecoveredFailureStartIndex(recoveredItems), recoveredItems.length);

    const trailingFailedItems = [
      makeShellTool(1, "old fail", "failed"),
      makeReadTool(2),
      makeShellTool(3, "tail fail", "failed"),
      { item_type: "assistant_thinking" as const, seq: 4, item_id: "th-4", text: "checking" },
    ];
    assert.equal(findTrailingUnrecoveredFailureStartIndex(trailingFailedItems), 2);
  });

  it("never auto-expands earlier folded steps when a live group settles with earlier recovered failures", async () => {
    const { StepTimelineRail, TurnActivityDisclosure } = await server.ssrLoadModule(
      "/src/components/stage/stage-step-group.tsx",
    );

    const items: TimelineItemView[] = Array.from({ length: 16 }, (_, idx) => {
      const seq = idx + 1;
      if (seq === 2) {
        return makeShellTool(seq, "rg missing", "failed", "recovered-early-error-output");
      }
      return makeReadTool(seq, `/repo/step_${seq}.ts`);
    });

    // Simulate live streaming (step 0: open disclosure while active) -> settled (step 1: isWorkActive becomes false)
    // Even though step #2 failed (failed_count: 1, unrecovered_failure_count: 0), earlier 8 steps MUST stay folded!
    const htmlAfterSettle = renderWithActions(
      (step) =>
        TurnActivityDisclosure.type({
          items,
          isWorkActive: step === 0,
          liveLabel: "Reading step_16.ts…",
          settledLabel: "Read files, ran a command",
          summaryCounts: {
            action_steps: 16,
            read_file_count: 15,
            command_count: 1,
            failed_count: 1,
            denied_count: 0,
            unrecovered_failure_count: 0,
          },
          labelIncludesFailure: false,
          defaultCollapsed: true,
          autoOpenForError: false,
          liveStartedAtMs: Date.now() - 5000,
          cwd: "/repo",
          agentId: "ag-live-settle",
          turnIndex: 0,
          turnAnchorSeq: 10,
          workOrdinal: 0,
          thinkingSignature: "",
          getThinkingDurationMs: () => null,
          holdViewport: (_a: unknown, apply: () => void) => apply(),
        }),
      [
        // Step 0: user opens outer disclosure while live
        (actions) => actions[0](),
        // Step 1: turn/group settles (isWorkActive: true -> false) with NO user click on earlier steps
        null,
      ],
    );

    assert.match(htmlAfterSettle, /aria-expanded="true"/);
    assert.match(htmlAfterSettle, /Show 8 earlier steps/);
    assert.doesNotMatch(htmlAfterSettle, /step_1\.ts/);
    assert.doesNotMatch(htmlAfterSettle, /recovered-early-error-output/);
    assert.match(htmlAfterSettle, /step_16\.ts/);

    // Direct StepTimelineRail check: even when autoExpandErrors flips false -> true AND items grow
    // pushing a failed item into the hidden prefix, earlier steps MUST stay folded until clicked.
    const railHtmlWithoutClick = renderWithActions(
      (step) =>
        StepTimelineRail.type({
          items: step === 0 ? items.slice(0, 13) : items,
          cwd: "/repo",
          holdViewport: (_a: unknown, apply: () => void) => apply(),
          isLiveActive: step === 0,
          autoExpandErrors: step > 0,
          agentId: "ag-rail-growth",
          turnIndex: 0,
          turnAnchorSeq: 20,
          workOrdinal: 0,
          thinkingSignature: "",
          getThinkingDurationMs: () => null,
        }),
      [null],
    );
    assert.match(railHtmlWithoutClick, /Show 8 earlier steps/);
    assert.doesNotMatch(railHtmlWithoutClick, /step_1\.ts/);
    assert.match(railHtmlWithoutClick, /step_16\.ts/);
  });

  it("keeps earlier steps folded when opening a completed TurnWorkedDisclosure with historical recovered errors", async () => {
    const { TurnStageSection } = await server.ssrLoadModule(
      "/src/components/stage/stage-turn-card.tsx",
    );

    const items: TimelineItemView[] = Array.from({ length: 18 }, (_, idx) => {
      const seq = idx + 1;
      if (seq === 3) {
        return makeShellTool(seq, "cargo test", "failed", "early-recovered-test-failure");
      }
      return makeReadTool(seq, `/repo/mod_${seq}.ts`);
    });

    const counts = {
      edited_file_count: 0,
      created_file_count: 0,
      command_count: 1,
      read_file_count: 17,
      search_count: 0,
      subagent_count: 0,
      mcp_server_count: 0,
      other_tool_count: 0,
      thought_count: 0,
      narration_count: 0,
      action_steps: 18,
      total_steps: 18,
      failed_count: 1,
      denied_count: 0,
      unrecovered_failure_count: 0,
      additions: 0,
      deletions: 0,
    };

    const turn: StageTurnView = {
      key: "turn-worked-overflow",
      turn_index: 0,
      anchor_seq: 100,
      is_compaction_command: false,
      user_prompt: null,
      agent_message: null,
      folded_steps: [],
      work_counts: counts,
      work_summary_label: "Read files, ran a command",
      work_summary_live_label: "Reading mod_18.ts…",
      work_summary_includes_failure: false,
      thought_duration_ms: null,
      terminal_reply: null,
      notices: [],
      compactions: [],
      turn_diffs: [],
      work_duration_ms: 24_000,
      turn_duration_ms: 24_000,
      has_error: true,
      has_unrecovered_error: false,
      has_active_item: false,
      started_at_ms: 1_000,
      completed_at_ms: 25_000,
      live_segments: [
        {
          kind: "step_group",
          key: "sg-1",
          first_seq: 1,
          items,
          summary: {
            counts,
            label: "Read files, ran a command",
            live_label: "Reading mod_18.ts…",
            live_detail: null,
            label_includes_failure: false,
          },
          has_following_content: true,
          has_running_step: false,
          work_duration_ms: 24_000,
        },
        {
          kind: "assistant_message",
          key: "msg-19",
          item: {
            item_type: "assistant_message",
            seq: 19,
            item_id: "m19",
            text: "All fixed.",
            streaming: false,
            blocks: [
              {
                kind: "paragraph",
                spans: [
                  {
                    text: "All fixed.",
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    inline_code: false,
                    link_url: null,
                  },
                ],
              },
            ],
          },
        },
      ],
    };

    // Open outer "Worked for 24s" disclosure (autoFoldCompleted: false simulates open state)
    const html = renderToStaticMarkup(
      createElement(TurnStageSection, {
        turn,
        isTurnActive: false,
        autoFoldCompleted: false,
        isNetworkOffline: false,
        hostId: "local",
        cwd: "/repo",
        agentId: "ag-worked-fold",
        hostArtifacts: [],
        thinkingSignature: "",
        getThinkingDurationMs: () => null,
        holdViewport: (_a: unknown, apply: () => void) => apply(),
        onRequestRewind: () => {},
        onPreviewImage: () => {},
      }),
    );

    assert.match(html, /Worked for 24s/);
    assert.match(html, /Show 10 earlier steps/);
    assert.doesNotMatch(html, /mod_1\.ts/);
    assert.doesNotMatch(html, /early-recovered-test-failure/);
    assert.match(html, /mod_18\.ts/);
  });

  it("auto-expands only the visible unrecovered trailing failure while keeping earlier steps folded, and expands earlier steps only on explicit click", async () => {
    const { StepTimelineRail } = await server.ssrLoadModule(
      "/src/components/stage/stage-step-group.tsx",
    );

    const items: TimelineItemView[] = Array.from({ length: 16 }, (_, idx) => {
      const seq = idx + 1;
      if (seq === 2) {
        return makeShellTool(seq, "early cmd", "failed", "early-recovered-stderr");
      }
      if (seq === 16) {
        return makeShellTool(seq, "final cmd", "failed", "final-unrecovered-stderr");
      }
      return makeReadTool(seq, `/repo/item_${seq}.ts`);
    });

    const baseProps = {
      items,
      cwd: "/repo",
      holdViewport: (_a: unknown, apply: () => void) => apply(),
      isLiveActive: false,
      autoExpandErrors: true,
      agentId: "ag-unrecovered",
      turnIndex: 1,
      turnAnchorSeq: 42,
      workOrdinal: 0,
      thinkingSignature: "",
      getThinkingDurationMs: () => null,
    };

    // Before clicking "Show 8 earlier steps": earlier 8 stay folded, trailing unrecovered failure IS visible & expanded
    const initialHtml = renderToStaticMarkup(createElement(StepTimelineRail, baseProps));
    assert.match(initialHtml, /Show 8 earlier steps/);
    assert.doesNotMatch(initialHtml, /item_1\.ts/);
    assert.doesNotMatch(initialHtml, /early-recovered-stderr/);
    assert.match(initialHtml, /final-unrecovered-stderr/);

    // After explicitly clicking "Show 8 earlier steps": earlier steps appear, recovered early failure stays collapsed
    const revealedHtml = renderWithActions(
      () => StepTimelineRail.type(baseProps),
      [(actions) => actions[0]()],
    );
    assert.doesNotMatch(revealedHtml, /Show \d+ earlier step/);
    assert.match(revealedHtml, /item_1\.ts/);
    assert.match(revealedHtml, /early cmd/);
    assert.doesNotMatch(revealedHtml, /early-recovered-stderr/);
    assert.match(revealedHtml, /final-unrecovered-stderr/);

    // After explicit reveal, if the turn is rewound (turnAnchorSeq changes 42 -> 55), earlier steps reset to folded
    const rewoundHtml = renderWithActions(
      (step) =>
        StepTimelineRail.type({
          ...baseProps,
          turnAnchorSeq: step === 0 ? 42 : 55,
        }),
      [(actions) => actions[0](), null],
    );
    assert.match(rewoundHtml, /Show 8 earlier steps/);
    assert.doesNotMatch(rewoundHtml, /item_1\.ts/);
  });

  it("keeps earlier steps folded even when all 15 steps failed in a row", async () => {
    const { StepTimelineRail } = await server.ssrLoadModule(
      "/src/components/stage/stage-step-group.tsx",
    );

    const items: TimelineItemView[] = Array.from({ length: 15 }, (_, idx) =>
      makeShellTool(idx + 1, `cmd_${idx + 1}`, "failed", `err_${idx + 1}`),
    );

    const html = renderToStaticMarkup(
      createElement(StepTimelineRail, {
        items,
        cwd: "/repo",
        holdViewport: (_a: unknown, apply: () => void) => apply(),
        isLiveActive: false,
        autoExpandErrors: true,
        agentId: "ag-all-failed",
        turnIndex: 0,
        turnAnchorSeq: 7,
        workOrdinal: 0,
        thinkingSignature: "",
        getThinkingDurationMs: () => null,
      }),
    );

    assert.match(html, /Show 7 earlier steps/);
    assert.doesNotMatch(html, /cmd_1\b/);
    assert.match(html, /cmd_15\b/);
  });
});
