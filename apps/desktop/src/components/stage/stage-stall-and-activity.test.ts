import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { StageTurnView } from "../../lib/orbit-ipc.ts";

let server: ViteDevServer;

before(async () => {
  server = await createServer({
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [
      {
        name: "test-stage-stall-orbit-store",
        enforce: "pre",
        load(id) {
          if (id.endsWith("/state/orbit-store.ts")) {
            return `
              export const orbitActions = {
                openArtifactsTab: () => {},
                openSubagentTab: () => {},
              };
              const state = { hostDetails: {}, stages: {} };
              export const useOrbitSelector = (selector) => selector(state);
              export const getOrbitState = () => state;
            `;
          }
          return null;
        },
      },
    ],
  });
});

after(async () => {
  await server?.close();
});

function makeStreamingAssistantTurn(latestActivityMs: number, startedAtMs: number): StageTurnView {
  const span = (text: string) => ({
    text,
    bold: false,
    italic: false,
    strikethrough: false,
    inline_code: false,
    link_url: null,
  });
  return {
    key: "turn-streaming-silent",
    turn_index: 0,
    anchor_seq: 1,
    user_prompt: {
      item_type: "user_prompt",
      seq: 1,
      message_id: "u1",
      text: "Write a large module",
      badges: [],
      blocks: [{ kind: "paragraph", spans: [span("Write a large module")] }],
      timestamp_ms: startedAtMs,
    },
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
    work_summary_includes_failure: false,
    thought_duration_ms: null,
    turn_duration_ms: null,
    work_duration_ms: null,
    terminal_reply: null,
    notices: [],
    compactions: [],
    live_segments: [
      {
        kind: "assistant_message",
        key: "msg-1",
        item: {
          item_type: "assistant_message",
          seq: 2,
          item_id: "agy_msg_1",
          text: "Let me create the full file now.",
          streaming: true,
          blocks: [
            {
              kind: "paragraph",
              spans: [span("Let me create the full file now.")],
            },
          ],
          timestamp_ms: latestActivityMs,
        },
      },
    ],
    turn_diffs: [],
    has_error: false,
    has_unrecovered_error: false,
    has_active_item: true,
    started_at_ms: startedAtMs,
    completed_at_ms: null,
    latest_activity_ms: latestActivityMs,
    latest_visible_progress_ms: latestActivityMs,
  };
}

describe("Stage stall detection and streaming-tail trailing activity", () => {
  it("never triggers Interrupt & continue when queuedNotificationCount === 0, and triggers on queued stalls", async () => {
    const {
      computeLiveStallState,
      SILENT_STALL_THRESHOLD_MS,
      THINKING_STALL_THRESHOLD_MS,
      QUEUED_NO_PROGRESS_THRESHOLD_MS,
    } = await server.ssrLoadModule("/src/components/stage/stage-tool-renderers.tsx");

    assert.equal(SILENT_STALL_THRESHOLD_MS, 90_000);
    assert.equal(THINKING_STALL_THRESHOLD_MS, 90_000);
    assert.equal(QUEUED_NO_PROGRESS_THRESHOLD_MS, 30_000);

    // 45s silent, empty queue -> NOT stalled yet and NEVER showInterruptContinue
    const s45 = computeLiveStallState({
      nowMs: 45_000,
      startedAtMs: 0,
      lastActivityMs: 0,
      lastVisibleProgressMs: 0,
      queuedNotificationCount: 0,
    });
    assert.equal(s45.isSilentStall, false);
    assert.equal(s45.showInterruptContinue, false);

    // 180s silent or continuous thinking, empty queue -> calm silent/thinking indicator ONLY, no Interrupt & continue
    const s180Silent = computeLiveStallState({
      nowMs: 180_000,
      startedAtMs: 0,
      lastActivityMs: 0,
      lastVisibleProgressMs: 0,
      queuedNotificationCount: 0,
    });
    assert.equal(s180Silent.isSilentStall, true);
    assert.equal(s180Silent.showInterruptContinue, false);

    const s180Thinking = computeLiveStallState({
      nowMs: 180_000,
      startedAtMs: 0,
      lastActivityMs: 175_000,
      lastVisibleProgressMs: 0,
      queuedNotificationCount: 0,
    });
    assert.equal(s180Thinking.isThinkingStall, true);
    assert.equal(s180Thinking.showInterruptContinue, false);

    // Queued notification + 30s without visible progress -> showInterruptContinue === true
    const sQueued = computeLiveStallState({
      nowMs: 30_000,
      startedAtMs: 0,
      lastActivityMs: 25_000,
      lastVisibleProgressMs: 0,
      queuedNotificationCount: 1,
    });
    assert.equal(sQueued.showInterruptContinue, true);
  });

  it("shows trailing working activity below silent streaming assistant prose without empty-queue Interrupt & continue", async () => {
    const {
      STREAMING_TAIL_SILENCE_THRESHOLD_MS,
      TurnStageSection,
      shouldShowStreamingTailActivity,
    } = await server.ssrLoadModule("/src/components/stage/stage-turn-card.tsx");

    assert.equal(STREAMING_TAIL_SILENCE_THRESHOLD_MS, 10_000);
    assert.equal(shouldShowStreamingTailActivity(10_000, null), false);
    assert.equal(shouldShowStreamingTailActivity(10_000, 0), false);
    assert.equal(shouldShowStreamingTailActivity(19_999, 10_000), false);
    assert.equal(shouldShowStreamingTailActivity(20_000, 10_000), true);

    const now = Date.now();
    // Active streaming message currently emitting (< 10s silent): no extra trailing indicator clutter
    const briskTurn = makeStreamingAssistantTurn(now - 2_000, now - 15_000);
    const briskHtml = renderToStaticMarkup(
      createElement(TurnStageSection, {
        turn: briskTurn,
        isTurnActive: true,
        isLastTurn: true,
        isNetworkOffline: false,
        autoFoldCompleted: true,
        hostId: "local",
        cwd: "/tmp/repo",
        agentId: "ag_1",
        thinkingSignature: "",
        getThinkingDurationMs: () => null,
        holdViewport: () => {},
        onRequestRewind: () => {},
        onPreviewImage: () => {},
        queuedNotificationCount: 0,
        onInterruptAndContinue: () => {},
      }),
    );
    assert.match(briskHtml, /Let me create the full file now/);
    assert.doesNotMatch(briskHtml, /Working…|Working\.\.\./);
    assert.doesNotMatch(briskHtml, /Interrupt &amp; continue/);

    // Silent for 50s while buffering a large write_file tool call (empty queue):
    // trailing working disclosure MUST be visible, and empty-queue Interrupt & continue MUST remain hidden.
    const silentToolBufferingTurn = makeStreamingAssistantTurn(now - 50_000, now - 100_000);
    const silentEmptyQueueHtml = renderToStaticMarkup(
      createElement(TurnStageSection, {
        turn: silentToolBufferingTurn,
        isTurnActive: true,
        isLastTurn: true,
        isNetworkOffline: false,
        autoFoldCompleted: true,
        hostId: "local",
        cwd: "/tmp/repo",
        agentId: "ag_1",
        thinkingSignature: "",
        getThinkingDurationMs: () => null,
        holdViewport: () => {},
        onRequestRewind: () => {},
        onPreviewImage: () => {},
        queuedNotificationCount: 0,
        onInterruptAndContinue: () => {},
      }),
    );
    assert.match(silentEmptyQueueHtml, /Working/);
    assert.doesNotMatch(silentEmptyQueueHtml, /Interrupt &amp; continue/);

    // Same 50s silent streaming tail, but with 2 queued follow-ups: shows both trailing activity and Interrupt & continue
    const silentQueuedHtml = renderToStaticMarkup(
      createElement(TurnStageSection, {
        turn: silentToolBufferingTurn,
        isTurnActive: true,
        isLastTurn: true,
        isNetworkOffline: false,
        autoFoldCompleted: true,
        hostId: "local",
        cwd: "/tmp/repo",
        agentId: "ag_1",
        thinkingSignature: "",
        getThinkingDurationMs: () => null,
        holdViewport: () => {},
        onRequestRewind: () => {},
        onPreviewImage: () => {},
        queuedNotificationCount: 2,
        onInterruptAndContinue: () => {},
      }),
    );
    assert.match(silentQueuedHtml, /Working/);
    assert.match(silentQueuedHtml, /2 queued/);
    assert.match(silentQueuedHtml, /Interrupt &amp; continue/);
  });

  it("treats Waiting for 1 timer and Waiting for N background tasks as calm background waits after settled reply", async () => {
    const { TurnStageSection } = await server.ssrLoadModule(
      "/src/components/stage/stage-turn-card.tsx",
    );

    const now = Date.now();
    const baseTurn = makeStreamingAssistantTurn(now - 120_000, now - 150_000);
    const span = {
      text: "Timer set, waiting cleanly.",
      bold: false,
      italic: false,
      strikethrough: false,
      inline_code: false,
      link_url: null,
    };
    const settledTimerTurn: StageTurnView = {
      ...baseTurn,
      has_active_item: false,
      live_segments: [
        {
          kind: "assistant_message",
          key: "msg-1",
          item: {
            item_type: "assistant_message",
            seq: 2,
            item_id: "agy_msg_1",
            text: "Timer set, waiting cleanly.",
            streaming: false,
            blocks: [{ kind: "paragraph", spans: [span] }],
            timestamp_ms: now - 120_000,
          },
        },
      ],
    };

    for (const statusLabel of ["Waiting for 1 timer", "Waiting for 2 background tasks"]) {
      const html = renderToStaticMarkup(
        createElement(TurnStageSection, {
          turn: settledTimerTurn,
          isTurnActive: true,
          isLastTurn: true,
          hasLiveBackgroundTasks: true,
          agentStatusLabel: statusLabel,
          isNetworkOffline: false,
          autoFoldCompleted: true,
          hostId: "local",
          cwd: "/tmp/repo",
          agentId: "ag_1",
          thinkingSignature: "",
          getThinkingDurationMs: () => null,
          holdViewport: () => {},
          onRequestRewind: () => {},
          onPreviewImage: () => {},
          queuedNotificationCount: 0,
          onInterruptAndContinue: () => {},
        }),
      );
      assert.match(html, /Timer set, waiting cleanly\./);
      assert.doesNotMatch(html, /Thinking…|Thinking\.\.\./);
      assert.doesNotMatch(html, /Working…|Working\.\.\./);
      assert.doesNotMatch(html, /waiting for response/i);
      assert.doesNotMatch(html, /Interrupt &amp; continue/);
    }

    // Contrast: when the agent is genuinely actively thinking (`Working`) with no waiting label,
    // TurnStageSection DOES render the trailing thinking disclosure (`Thinking…`).
    const workingHtml = renderToStaticMarkup(
      createElement(TurnStageSection, {
        turn: settledTimerTurn,
        isTurnActive: true,
        isLastTurn: true,
        hasLiveBackgroundTasks: true,
        agentStatusLabel: "Working",
        isNetworkOffline: false,
        autoFoldCompleted: true,
        hostId: "local",
        cwd: "/tmp/repo",
        agentId: "ag_1",
        thinkingSignature: "",
        getThinkingDurationMs: () => null,
        holdViewport: () => {},
        onRequestRewind: () => {},
        onPreviewImage: () => {},
        queuedNotificationCount: 0,
        onInterruptAndContinue: () => {},
      }),
    );
    assert.match(workingHtml, /Thinking/);
  });
});
