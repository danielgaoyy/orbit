import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import {
  ALL_TOPICS_MASK,
  STATE_KEY_TOPIC,
  STORE_TOPICS,
  TOPIC_BIT,
  TopicNotificationHub,
  computeChangedTopicMask,
  createTopicStoreKernel,
  evaluateSelectorWithTopicTracking,
  topicsToMask,
} from "./store-topics.ts";
import type { OrbitUiState } from "./store-types.ts";

function createMinimalTestState(): OrbitUiState {
  const base: Record<string, unknown> = {};
  for (const key of Object.keys(STATE_KEY_TOPIC)) {
    base[key] = null;
  }
  return {
    ...(base as unknown as OrbitUiState),
    fleet: { revision: 1, total_pending_approvals: 0, hosts: [], projects: [] },
    fleetHydrated: true,
    bootstrapError: null,
    hostDetails: {},
    selectedHostId: "host_1",
    selectedAgentId: "agent_1",
    selectedWorkspaceCwd: "/repo",
    isDraftNewSession: false,
    stages: {},
    rewoundSessions: {},
    sessionScrollNonce: 0,
    pendingJumpTarget: null,
    slashCommands: [],
    composerDrafts: {},
    terminalTabs: [],
    terminalSplits: [],
    activeTerminalTabKey: null,
    terminalFocused: false,
    sidebarOpen: true,
    sidebarWidth: 256,
    theme: "system",
    resolvedTheme: "light",
    locale: "en",
    enterToSend: true,
    activeTurnSendBehavior: "queue",
    terminalFontSize: 13,
    activeView: "workspace",
    workspacePanel: "conversation",
    settingsSection: "general",
    toastNotice: null,
  };
}

describe("Orbit store topic notification channels (B2-07)", () => {
  it("maps every StoreTopic to a unique power-of-two bit covering ALL_TOPICS_MASK", () => {
    let union = 0;
    const seenBits = new Set<number>();
    for (const topic of STORE_TOPICS) {
      const bit = TOPIC_BIT[topic];
      assert.ok(bit > 0 && (bit & (bit - 1)) === 0, `topic ${topic} bit must be a power of 2`);
      assert.ok(!seenBits.has(bit), `topic ${topic} bit must be unique`);
      seenBits.add(bit);
      union |= bit;
    }
    assert.strictEqual(union, ALL_TOPICS_MASK);

    const validTopics = new Set<string>(STORE_TOPICS);
    for (const [field, topic] of Object.entries(STATE_KEY_TOPIC)) {
      assert.ok(validTopics.has(topic), `field ${field} mapped to unknown topic ${topic}`);
    }
  });

  it("isolates high-frequency stage streaming deltas from unrelated domain subscribers", () => {
    const kernel = createTopicStoreKernel(createMinimalTestState());

    const counts = {
      stage: 0,
      preferences: 0,
      terminal: 0,
      fleet: 0,
      sidebar: 0,
      updater: 0,
      automations: 0,
    };

    const unsubs = [
      kernel.subscribeTopics("stage", () => {
        counts.stage += 1;
      }),
      kernel.subscribeTopics("preferences", () => {
        counts.preferences += 1;
      }),
      kernel.subscribeTopics("terminal", () => {
        counts.terminal += 1;
      }),
      kernel.subscribeTopics("fleet", () => {
        counts.fleet += 1;
      }),
      kernel.subscribeTopics("sidebar", () => {
        counts.sidebar += 1;
      }),
      kernel.subscribeTopics("updater", () => {
        counts.updater += 1;
      }),
      kernel.subscribeTopics("automations", () => {
        counts.automations += 1;
      }),
    ];

    // Simulate 50 rapid streaming token deltas mutating only `stages`
    for (let rev = 1; rev <= 50; rev += 1) {
      const { dirtyMask } = kernel.setState((s) => ({
        ...s,
        stages: {
          ...s.stages,
          "host_1::agent_1": {
            revision: rev,
            host_id: "host_1",
            agent: null,
            turns: [],
            pending_permissions: [],
            pending_questions: [],
            session_turn_diffs: [],
            insights: {
              touched_files: [],
              background_tasks: [],
              subagents: [],
              detected_local_urls: [],
            },
            latest_seq: rev,
          },
        },
      }));
      assert.strictEqual(dirtyMask, TOPIC_BIT.stage);
    }

    assert.strictEqual(counts.stage, 50);
    assert.strictEqual(counts.preferences, 0);
    assert.strictEqual(counts.terminal, 0);
    assert.strictEqual(counts.fleet, 0);
    assert.strictEqual(counts.sidebar, 0);
    assert.strictEqual(counts.updater, 0);
    assert.strictEqual(counts.automations, 0);
    assert.strictEqual(kernel.getState().stages["host_1::agent_1"]?.revision, 50);

    // Mutate preferences: only preferences listener wakes, with accurate snapshot
    kernel.setState((s) => ({ ...s, theme: "dark", resolvedTheme: "dark" }));
    assert.strictEqual(counts.preferences, 1);
    assert.strictEqual(counts.stage, 50);
    assert.strictEqual(counts.terminal, 0);
    assert.strictEqual(counts.fleet, 0);
    assert.strictEqual(kernel.getState().theme, "dark");

    // Mutate terminal: only terminal listener wakes
    kernel.setState((s) => ({
      ...s,
      terminalTabs: [
        {
          key: "host_1::0",
          hostId: "host_1",
          terminalId: "term_1",
          slot: 0,
          cwd: "/repo",
          title: "zsh",
        },
      ],
      activeTerminalTabKey: "host_1::0",
    }));
    assert.strictEqual(counts.terminal, 1);
    assert.strictEqual(counts.preferences, 1);
    assert.strictEqual(counts.stage, 50);
    assert.strictEqual(kernel.getState().activeTerminalTabKey, "host_1::0");

    // No-op shallow clone with unchanged field values emits 0 notifications
    const noOp = kernel.setState((s) => ({ ...s }));
    assert.strictEqual(noOp.dirtyMask, 0);
    assert.strictEqual(counts.stage, 50);
    assert.strictEqual(counts.preferences, 1);
    assert.strictEqual(counts.terminal, 1);

    for (const unsub of unsubs) unsub();
    assert.strictEqual(kernel.getBucketSize("stage"), 0);
    assert.strictEqual(kernel.getBucketSize("preferences"), 0);
  });

  it("deduplicates multi-topic subscribers when a single setState touches multiple domains", () => {
    const hub = new TopicNotificationHub();
    let multiCalls = 0;
    let fleetCalls = 0;
    let prefCalls = 0;

    const h1 = hub.subscribe(topicsToMask(["fleet", "stage"]), () => {
      multiCalls += 1;
    });
    const h2 = hub.subscribe(TOPIC_BIT.fleet, () => {
      fleetCalls += 1;
    });
    const h3 = hub.subscribe(TOPIC_BIT.preferences, () => {
      prefCalls += 1;
    });

    const notified = hub.emit(TOPIC_BIT.fleet | TOPIC_BIT.stage);
    assert.strictEqual(notified, 2);
    assert.strictEqual(multiCalls, 1);
    assert.strictEqual(fleetCalls, 1);
    assert.strictEqual(prefCalls, 0);

    h1.unsubscribe();
    h2.unsubscribe();
    h3.unsubscribe();
  });

  it("automatically infers exact topic masks for 1-arg selectors and updates dynamically across branches", () => {
    const initial = createMinimalTestState();
    initial.isDraftNewSession = true;
    const kernel = createTopicStoreKernel(initial);

    // 1. Preferences selector tracks only preferences and preserves nested reference identity
    const prefRes = evaluateSelectorWithTopicTracking(kernel.getState(), (s) => s.theme);
    assert.strictEqual(prefRes.value, "system");
    assert.strictEqual(prefRes.mask, TOPIC_BIT.preferences);

    const hostsRes = evaluateSelectorWithTopicTracking(kernel.getState(), (s) => s.fleet.hosts);
    assert.strictEqual(hostsRes.value, kernel.getState().fleet.hosts);
    assert.strictEqual(hostsRes.mask, TOPIC_BIT.fleet);

    // 2. Identity selector `(s) => s` returns raw state (never proxy) and subscribes to ALL_TOPICS_MASK
    const fullRes = evaluateSelectorWithTopicTracking(kernel.getState(), (s) => s);
    assert.strictEqual(fullRes.value, kernel.getState());
    assert.strictEqual(fullRes.mask, ALL_TOPICS_MASK);

    // 3. Cross-topic conditional selector dynamically expands subscription mask when branch flips
    const conditionalSelector = (s: OrbitUiState) =>
      s.isDraftNewSession ? null : (s.stages["host_1::agent_1"] ?? null);

    let wakeups = 0;
    const first = evaluateSelectorWithTopicTracking(kernel.getState(), conditionalSelector);
    assert.strictEqual(first.value, null);
    assert.strictEqual(first.mask, TOPIC_BIT.fleet);

    const handle = kernel.subscribeMask(first.mask, () => {
      wakeups += 1;
      const evaluated = evaluateSelectorWithTopicTracking(kernel.getState(), conditionalSelector);
      handle.updateMask(evaluated.mask);
    });

    // While in draft mode, stage deltas do NOT wake the conditional selector
    kernel.setState((s) => ({
      ...s,
      stages: {
        ...s.stages,
        "host_1::agent_1": {
          revision: 1,
          host_id: "host_1",
          agent: null,
          turns: [],
          pending_permissions: [],
          pending_questions: [],
          session_turn_diffs: [],
          insights: {
            touched_files: [],
            background_tasks: [],
            subagents: [],
            detected_local_urls: [],
          },
          latest_seq: 1,
        },
      },
    }));
    assert.strictEqual(wakeups, 0);

    // Leaving draft mode flips `fleet` -> wakes subscriber -> expands mask to `fleet | stage`
    kernel.setState((s) => ({ ...s, isDraftNewSession: false }));
    assert.strictEqual(wakeups, 1);
    assert.strictEqual(handle.getMask(), TOPIC_BIT.fleet | TOPIC_BIT.stage);

    // Subsequent stage delta NOW wakes the subscriber immediately
    kernel.setState((s) => ({
      ...s,
      stages: {
        ...s.stages,
        "host_1::agent_1": {
          ...s.stages["host_1::agent_1"],
          revision: 2,
          latest_seq: 2,
        },
      },
    }));
    assert.strictEqual(wakeups, 2);

    handle.unsubscribe();
  });

  it("computes multi-domain dirty masks accurately across state transitions", () => {
    const prev = createMinimalTestState();
    const next: OrbitUiState = {
      ...prev,
      sidebarOpen: false,
      toastNotice: "Saved",
    };
    assert.strictEqual(
      computeChangedTopicMask(prev, next),
      TOPIC_BIT.sidebar | TOPIC_BIT.ui,
    );
  });
});

describe("Modular store slices & stage extraction contract (B2-07)", () => {
  const stateDir = path.dirname(fileURLToPath(import.meta.url));
  const stageDir = path.resolve(stateDir, "..", "components", "stage");

  it("preserves all backwards-compatible public exports on orbit-store.ts and wires all 7 domain slices", () => {
    const storeSource = readFileSync(path.join(stateDir, "orbit-store.ts"), "utf8");
    const storeLines = storeSource.split("\n").length;
    assert.ok(
      storeLines < 1500,
      `Expected modular orbit-store.ts kernel to be compact (<1500 LOC), got ${storeLines}`,
    );

    const requiredExports = [
      "orbitActions",
      "useOrbitSelector",
      "getOrbitState",
      "subscribeOrbitTopics",
      "useComposerDraft",
      "useHasThreadDraft",
      "threadScopeKey",
      "subagentArchiveKey",
      "isSubagentArchived",
      "fileCacheKey",
      "sideQuestionScopeKey",
      "composerScopeKey",
      "resolveFleetTargetVersion",
    ];
    for (const sym of requiredExports) {
      assert.ok(
        storeSource.includes(sym),
        `Expected orbit-store.ts to keep public symbol '${sym}'`,
      );
    }

    const expectedSlices = [
      "fleet-slice.ts",
      "updater-slice.ts",
      "stage-stream-slice.ts",
      "workspace-slice.ts",
      "terminal-slice.ts",
      "automations-slice.ts",
      "preferences-slice.ts",
    ];
    for (const sliceFile of expectedSlices) {
      const fullPath = path.join(stateDir, "slices", sliceFile);
      assert.ok(existsSync(fullPath), `Expected slice file ${sliceFile} to exist`);
    }
  });

  it("extracts cohesive stage modules and preserves StreamStage / TurnStageSection / PromptImageLightbox exports", () => {
    const streamStageSource = readFileSync(path.join(stageDir, "stream-stage.tsx"), "utf8");
    const stageLines = streamStageSource.split("\n").length;
    assert.ok(
      stageLines < 2500,
      `Expected modular stream-stage.tsx to be <2500 LOC (was 5914), got ${stageLines}`,
    );

    assert.match(streamStageSource, /export\s+function\s+StreamStage\b/);
    assert.match(streamStageSource, /export\s*\{[^}]*\bTurnStageSection\b[^}]*\}/);
    assert.match(streamStageSource, /export\s*\{[^}]*\bPromptImageLightbox\b[^}]*\}/);
    assert.match(streamStageSource, /export\s+type\s*\{[^}]*\bPromptImagePreviewTarget\b[^}]*\}/);

    const expectedStageModules = [
      "stage-helpers.ts",
      "stage-tool-renderers.tsx",
      "stage-step-group.tsx",
      "stage-prompt-controls.tsx",
      "stage-turn-card.tsx",
    ];
    for (const modFile of expectedStageModules) {
      assert.ok(
        existsSync(path.join(stageDir, modFile)),
        `Expected stage module ${modFile} to exist`,
      );
    }
  });
});
