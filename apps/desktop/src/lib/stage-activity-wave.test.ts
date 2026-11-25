import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { Children, createElement, isValidElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import {
  summarizeActiveSubagents,
  summarizeStageLiveActivity,
} from "../components/stage/stage-activity-wave-model.ts";
import type { StageTurnView } from "./orbit-ipc.ts";

let server: ViteDevServer;

before(async () => {
  server = await createServer({
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [
      {
        name: "test-stage-activity-wave-store",
        enforce: "pre",
        load(id) {
          if (id.endsWith("/state/orbit-store.ts")) {
            return `
              export const actionsLog = [];
              export const orbitActions = {
                openArtifactsTab: () => {},
                openSubagentTab: (record) => actionsLog.push({ type: "openSubagentTab", record }),
                toggleOverviewPanel: (open) => {
                  actionsLog.push({ type: "toggleOverviewPanel", open });
                  if (state.stageCanDockOverview) state.overviewDesktopOpenPref = open ?? !state.overviewDesktopOpenPref;
                  else state.overviewPopoverOpen = open ?? !state.overviewPopoverOpen;
                },
                toggleOverviewSection: (section) => {
                  actionsLog.push({ type: "toggleOverviewSection", section });
                  state.overviewCollapsedSections[section] = !state.overviewCollapsedSections[section];
                },
              };
              export const state = {
                hostDetails: { local: { host: { host_id: "local" }, agents: [{ agent_id: "ag_child" }], conditional_timers: [], goal_runs: [] } },
                stages: {},
                stageCanDockOverview: true,
                overviewDesktopOpenPref: false,
                overviewPopoverOpen: false,
                overviewCollapsedSections: { subagents: true, tasks: false, goals: false },
              };
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

function renderWave(component: { type: (props: object) => ReactNode }, props: object) {
  const clicks: Array<() => void> = [];
  function collectClicks(node: ReactNode) {
    Children.forEach(node, (child) => {
      if (!isValidElement<{ onClick?: () => void; children?: ReactNode }>(child)) return;
      if (child.props.onClick) clicks.push(child.props.onClick);
      collectClicks(child.props.children);
    });
  }
  function Harness() {
    clicks.length = 0;
    const tree = component.type(props);
    collectClicks(tree);
    return tree;
  }
  return { html: renderToStaticMarkup(createElement(Harness)), clicks };
}

describe("summarizeStageLiveActivity", () => {
  it("returns null when idle", () => {
    assert.equal(
      summarizeStageLiveActivity({
        runningSubagentsCount: 0,
        awaitingSubagentsCount: 0,
        runningBackgroundTasks: [],
        armedTimersCount: 0,
        hasActiveGoal: false,
      }),
      null,
    );
  });

  it("summarizes singular and plural running subagents across en, zh-CN, and ja", () => {
    const enOne = summarizeStageLiveActivity(
      {
        runningSubagentsCount: 1,
        awaitingSubagentsCount: 0,
        runningBackgroundTasks: [],
        armedTimersCount: 0,
        hasActiveGoal: false,
      },
      "en",
    );
    assert.equal(enOne?.text, "1 subagent running");
    assert.equal(enOne?.tone, "normal");
    assert.equal(enOne?.primarySection, "subagents");

    const enMany = summarizeStageLiveActivity(
      {
        runningSubagentsCount: 3,
        awaitingSubagentsCount: 0,
        runningBackgroundTasks: [],
        armedTimersCount: 0,
        hasActiveGoal: false,
      },
      "en",
    );
    assert.equal(enMany?.text, "3 subagents running");

    assert.equal(
      summarizeStageLiveActivity(
        {
          runningSubagentsCount: 2,
          awaitingSubagentsCount: 0,
          runningBackgroundTasks: [],
          armedTimersCount: 0,
          hasActiveGoal: false,
        },
        "zh-CN",
      )?.text,
      "2 个子代理运行中",
    );

    assert.equal(
      summarizeStageLiveActivity(
        {
          runningSubagentsCount: 2,
          awaitingSubagentsCount: 0,
          runningBackgroundTasks: [],
          armedTimersCount: 0,
          hasActiveGoal: false,
        },
        "ja",
      )?.text,
      "サブエージェント 2 件実行中",
    );
  });

  it("prioritizes awaiting approval subagents first with warning tone", () => {
    const summary = summarizeStageLiveActivity({
      runningSubagentsCount: 2,
      awaitingSubagentsCount: 1,
      runningBackgroundTasks: [{ id: "bg-1", title: "cargo test" }],
      armedTimersCount: 0,
      hasActiveGoal: false,
    });
    assert.ok(summary);
    assert.equal(summary.tone, "warning");
    assert.equal(summary.primarySection, "subagents");
    assert.equal(
      summary.text,
      "1 subagent awaiting approval · 2 subagents running · 1 background task running",
    );
    assert.equal(summary.parts[0]?.kind, "awaiting_subagents");
    assert.equal(summary.parts[0]?.tone, "warning");
    assert.equal(summary.secondaryHint, null);

    const manyAwaiting = summarizeStageLiveActivity({
      runningSubagentsCount: 0,
      awaitingSubagentsCount: 2,
      runningBackgroundTasks: [],
      armedTimersCount: 0,
      hasActiveGoal: false,
    });
    assert.equal(manyAwaiting?.text, "2 subagents awaiting approval");
    assert.equal(manyAwaiting?.tone, "warning");
  });

  it("shows a concise secondary hint only for a single background task without active subagents", () => {
    const singleWithTitle = summarizeStageLiveActivity({
      runningSubagentsCount: 0,
      awaitingSubagentsCount: 0,
      runningBackgroundTasks: [{ id: "bg-1", title: "Run test suite", command: "cargo test" }],
      armedTimersCount: 0,
      hasActiveGoal: false,
    });
    assert.ok(singleWithTitle);
    assert.equal(singleWithTitle.text, "1 background task running");
    assert.equal(singleWithTitle.secondaryHint, "Run test suite");
    assert.equal(singleWithTitle.primarySection, "tasks");

    const singleWithCommandFallback = summarizeStageLiveActivity({
      runningSubagentsCount: 0,
      awaitingSubagentsCount: 0,
      runningBackgroundTasks: [{ id: "bg-2", title: "", command: "npm   run   build" }],
      armedTimersCount: 0,
      hasActiveGoal: false,
    });
    assert.equal(singleWithCommandFallback?.secondaryHint, "npm run build");

    const pluralTasks = summarizeStageLiveActivity({
      runningSubagentsCount: 0,
      awaitingSubagentsCount: 0,
      runningBackgroundTasks: [
        { id: "bg-1", title: "Build client" },
        { id: "bg-2", title: "Watch logs" },
      ],
      armedTimersCount: 0,
      hasActiveGoal: false,
    });
    assert.equal(pluralTasks?.text, "2 background tasks running");
    assert.equal(pluralTasks?.secondaryHint, null);
  });

  it("shows armed timers and standalone active goal only when foreground is idle", () => {
    const timerIdle = summarizeStageLiveActivity({
      runningSubagentsCount: 0,
      awaitingSubagentsCount: 0,
      runningBackgroundTasks: [],
      armedTimersCount: 1,
      hasActiveGoal: true,
      isForegroundRunning: false,
    });
    assert.equal(timerIdle?.text, "1 timer waiting");
    assert.equal(timerIdle?.primarySection, "goals");

    const timersMany = summarizeStageLiveActivity({
      runningSubagentsCount: 0,
      awaitingSubagentsCount: 0,
      runningBackgroundTasks: [],
      armedTimersCount: 2,
      hasActiveGoal: false,
      isForegroundRunning: false,
    });
    assert.equal(timersMany?.text, "2 timers waiting");

    const goalOnlyIdle = summarizeStageLiveActivity({
      runningSubagentsCount: 0,
      awaitingSubagentsCount: 0,
      runningBackgroundTasks: [],
      armedTimersCount: 0,
      hasActiveGoal: true,
      isForegroundRunning: false,
    });
    assert.equal(goalOnlyIdle?.text, "Goal active");
    assert.equal(goalOnlyIdle?.primarySection, "goals");

    // During foreground streaming, timers & goals stay suppressed to avoid duplicating Thinking…
    assert.equal(
      summarizeStageLiveActivity({
        runningSubagentsCount: 0,
        awaitingSubagentsCount: 0,
        runningBackgroundTasks: [],
        armedTimersCount: 2,
        hasActiveGoal: true,
        isForegroundRunning: true,
      }),
      null,
    );

    // Concurrent subagents/bg tasks STILL surface while foreground is running.
    const concurrentDuringStreaming = summarizeStageLiveActivity({
      runningSubagentsCount: 1,
      awaitingSubagentsCount: 0,
      runningBackgroundTasks: [{ id: "bg-1", title: "dev server" }],
      armedTimersCount: 2,
      hasActiveGoal: true,
      isForegroundRunning: true,
    });
    assert.equal(
      concurrentDuringStreaming?.text,
      "1 subagent running · 1 background task running",
    );
  });

  it("summarizes active subagents prioritizing awaiting_permission for firstActive and tracking soleActive", () => {
    const running = { id: "a", status: "running" };
    const awaiting = { id: "b", status: "awaiting_permission" };
    const done = { id: "c", status: "completed" };

    const one = summarizeActiveSubagents([done, running]);
    assert.equal(one.runningCount, 1);
    assert.equal(one.awaitingCount, 0);
    assert.equal(one.soleActive, running);
    assert.equal(one.firstActive, running);

    const two = summarizeActiveSubagents([running, awaiting, done]);
    assert.equal(two.runningCount, 1);
    assert.equal(two.awaitingCount, 1);
    assert.equal(two.soleActive, null);
    assert.equal(two.firstActive, awaiting);
  });
});

describe("StageActivityWave SSR rendering and interactions", () => {
  it("renders nothing when idle and calm ripple-wave markup with status accessibility when active", async () => {
    const { StageActivityWave } = await server.ssrLoadModule(
      "/src/components/stage/stage-activity-wave.tsx",
    );

    const idle = renderWave(StageActivityWave, {
      runningSubagentsCount: 0,
      awaitingSubagentsCount: 0,
      runningBackgroundTasks: [],
      armedTimersCount: 0,
      hasActiveGoal: false,
    });
    assert.equal(idle.html, "");

    let clicked = 0;
    const active = renderWave(StageActivityWave, {
      runningSubagentsCount: 1,
      awaitingSubagentsCount: 0,
      runningBackgroundTasks: [],
      armedTimersCount: 0,
      hasActiveGoal: false,
      overviewTrigger: true,
      onClick: () => {
        clicked += 1;
      },
    });
    assert.match(active.html, /data-slot="stage-activity-wave"/);
    assert.match(active.html, /role="status"/);
    assert.match(active.html, /aria-live="polite"/);
    assert.match(active.html, /data-slot="stage-activity-wave-orb"/);
    assert.match(active.html, /orbit-activity-ripple-ring/);
    assert.match(active.html, /orbit-activity-ripple-ring-delayed/);
    assert.match(active.html, /data-overview-trigger="true"/);
    assert.match(active.html, /shimmer/);
    assert.match(active.html, /1 subagent running/);
    assert.match(active.html, /Overview/);
    assert.equal(active.html.match(/<button\b/g)?.length, 1);
    assert.equal(active.clicks.length, 1);
    active.clicks[0]();
    assert.equal(clicked, 1);

    // Warning state shifts orb & awaiting text to warning tone without a clickable button when onClick omitted.
    const awaiting = renderWave(StageActivityWave, {
      runningSubagentsCount: 0,
      awaitingSubagentsCount: 1,
      runningBackgroundTasks: [{ id: "bg-1", title: "cargo build" }],
      armedTimersCount: 0,
      hasActiveGoal: false,
    });
    assert.match(awaiting.html, /data-tone="warning"/);
    assert.match(awaiting.html, /text-signal-warning/);
    assert.match(awaiting.html, /1 subagent awaiting approval/);
    assert.match(awaiting.html, /1 background task running/);
    assert.doesNotMatch(awaiting.html, /<button\b|Overview/);
  });

  it("routes StreamStageActivityWave clicks to open Overview, expand collapsed section, or open sole active subagent", async () => {
    const { StreamStageActivityWave } = await server.ssrLoadModule(
      "/src/components/stage/stage-activity-wave.tsx",
    );
    const { actionsLog: rawLog, state } = await server.ssrLoadModule("/src/state/orbit-store.ts");
    const actionsLog = rawLog as Array<{
      type: string;
      open?: boolean;
      section?: string;
      record?: { childAgentId?: string };
    }>;
    const soleChild = {
      key: "managed:local:ag_child",
      callId: "call-1",
      title: "Reviewer",
      promptPreview: "Review PR",
      childAgentId: "ag_child",
      childHostId: "local",
      providerSubagentId: null,
      status: "running",
      isManaged: true,
    };

    // 1. Overview closed + subagents collapsed -> opens Overview AND expands subagents
    actionsLog.length = 0;
    state.stageCanDockOverview = true;
    state.overviewDesktopOpenPref = false;
    state.overviewCollapsedSections.subagents = true;
    const closedWave = renderWave(StreamStageActivityWave, {
      hostId: "local",
      agentId: "parent",
      sessionSubagents: [soleChild],
      runningBackgroundTasks: [],
      isForegroundRunning: false,
    });
    assert.equal(closedWave.clicks.length, 1);
    closedWave.clicks[0]();
    assert.deepEqual([...actionsLog], [
      { type: "toggleOverviewPanel", open: true },
      { type: "toggleOverviewSection", section: "subagents" },
    ]);

    // 2. Overview open + section expanded + sole active subagent -> opens child subagent tab directly
    actionsLog.length = 0;
    const openWave = renderWave(StreamStageActivityWave, {
      hostId: "local",
      agentId: "parent",
      sessionSubagents: [soleChild],
      runningBackgroundTasks: [],
      isForegroundRunning: false,
    });
    openWave.clicks[0]();
    assert.equal(actionsLog.length, 1);
    assert.equal(actionsLog[0]?.type, "openSubagentTab");
    assert.equal(actionsLog[0]?.record?.childAgentId, "ag_child");

    // 3. Idle + foreground running renders bottom cushion spacer only
    const spacer = renderWave(StreamStageActivityWave, {
      hostId: "local",
      agentId: "parent",
      sessionSubagents: [],
      runningBackgroundTasks: [],
      isForegroundRunning: true,
    });
    assert.match(spacer.html, /h-\[32px\]/);
    assert.doesNotMatch(spacer.html, /data-slot="stage-activity-wave"/);
  });

  it("suppresses duplicate background-wait trailing activity inside TurnStageSection", async () => {
    const { TurnStageSection } = await server.ssrLoadModule(
      "/src/components/stage/stage-turn-card.tsx",
    );
    const turn: StageTurnView = {
      key: "turn-bg-wait",
      turn_index: 0,
      anchor_seq: 1,
      user_prompt: null,
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
            item_id: "msg-1",
            text: "Started the background build.",
            streaming: false,
            blocks: [
              {
                kind: "paragraph",
                spans: [
                  {
                    text: "Started the background build.",
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    inline_code: false,
                    link_url: null,
                  },
                ],
              },
            ],
            timestamp_ms: 1000,
          },
        },
        {
          kind: "wakeup",
          key: "wakeup-1",
          label: "Waiting for background task",
        },
      ],
      turn_diffs: [],
      has_error: false,
      has_unrecovered_error: false,
      has_active_item: true,
      started_at_ms: 1000,
      completed_at_ms: null,
      latest_activity_ms: 1000,
      latest_visible_progress_ms: 1000,
    };

    const html = renderToStaticMarkup(
      createElement(TurnStageSection, {
        turn,
        isTurnActive: true,
        isLastTurn: true,
        agentStatusLabel: "Running (background task)",
        hasLiveBackgroundTasks: true,
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
      }),
    );
    assert.match(html, /Started the background build/);
    assert.doesNotMatch(html, /Thinking…|Waiting for background task|Working…/);
  });
});
