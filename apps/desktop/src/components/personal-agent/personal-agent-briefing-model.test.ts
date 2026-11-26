import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type {
  BriefingMission,
  BriefingWaitItem,
  BriefingWorker,
  HostDetailView,
  PersonalAgentActivityEntry,
  PersonalAgentBriefingView,
  RouterHostCapabilities,
} from "../../lib/orbit-ipc.ts";
import {
  type LiveBriefingAgentCard,
  buildDefaultRouterConfig,
  computeSinceLastVisitSummary,
  filterActivityForPersonalAgent,
  findLiveBriefingGoal,
  findLiveBriefingTimer,
  formatBriefingElapsed,
  partitionBriefingWorkers,
  resolveEffectiveBriefingWorkerStatus,
  selectRecommendedRouterHost,
} from "./personal-agent-briefing-model.ts";

function makeLiveAgent(overrides: Partial<LiveBriefingAgentCard> = {}): LiveBriefingAgentCard {
  return {
    agent_id: "ag_1",
    workspace_id: "ws_1",
    title: "Worker",
    provider: "codex",
    provider_label: "Codex",
    model: "gpt-5.4",
    effort: null,
    mode: "auto",
    parent_agent_id: "pa_1",
    cwd: "/repo",
    status_label: "Idle",
    is_running: false,
    has_background_tasks: false,
    has_armed_timers: false,
    has_running_subagents: false,
    is_awaiting_permission: false,
    is_pinned: false,
    seen_seq: 5,
    is_unread: false,
    latest_seq: 5,
    updated_at_rfc3339: "2026-11-26T00:00:00Z",
    ...overrides,
  };
}

function makeWorker(overrides: Partial<BriefingWorker> = {}): BriefingWorker {
  return {
    host_id: "h_local",
    host_name: "Mac Studio",
    agent_id: "ag_1",
    title: "Worker",
    provider: "codex",
    state: "running",
    step_preview: "Working",
    worktree_branch: null,
    updated_at_ms: 1000,
    ...overrides,
  };
}

function makeHostDetail(
  hostId: string,
  agents: LiveBriefingAgentCard[],
  extra?: Partial<HostDetailView>,
): HostDetailView {
  return {
    revision: 1,
    host: {
      host_id: hostId,
      display_name: hostId,
      hostname: hostId,
      custom_name: null,
      ssh_alias: null,
      route_mode: "auto",
      remote_daemon_port: 6868,
      custom_direct_endpoint: null,
      via_ssh_tunnel: false,
      last_ping_ms: 1,
      daemon_version: "0.11.0",
      os_arch: "darwin/aarch64",
      daemon_pubkey_b64: "",
      direct_endpoints: [],
      relay_url: null,
      connection_state: "online_direct",
      last_error: null,
      available_providers: ["codex"],
      workspace_count: 1,
      agent_count: agents.length,
      pending_approval_count: 0,
    },
    discovered_projects: [],
    directory_listing: null,
    models_by_provider: {},
    workspaces: [],
    agents,
    last_created_agent_id: null,
    replaced_optimistic_agents: {},
    quotas: [],
    terminals: [],
    active_terminal_id: null,
    active_terminal_slot: 0,
    ...extra,
  };
}

describe("personal-agent-briefing-model", () => {
  it("filters Guardian/rule activity entries strictly to PA and its dispatched worker subtree", () => {
    const briefing: PersonalAgentBriefingView = {
      personal_agent: {
        host_id: "h_mac",
        host_name: "Mac Studio",
        agent_id: "pa_1",
        title: "Personal Agent",
        provider: "claude_code",
        status_label: "Idle",
        is_busy: false,
        is_awaiting: false,
      },
      needs_you: [],
      missions: [
        {
          key: "m1",
          root: {
            host_id: "h_linux",
            host_name: "Linux",
            agent_id: "w_root",
            title: "Build",
            provider: "codex",
            state: "running",
            step_preview: "Running tests",
            worktree_branch: null,
            updated_at_ms: 1000,
          },
          workers: [
            {
              host_id: "h_gpu",
              host_name: "GPU",
              agent_id: "w_child",
              title: "Eval",
              provider: "antigravity",
              state: "done",
              step_preview: "Done",
              worktree_branch: null,
              updated_at_ms: 1000,
            },
          ],
          host_names: ["Linux", "GPU"],
          done: 1,
          total: 2,
          state: "running",
          updated_at_ms: 1000,
        },
      ],
      other_active: [],
      waiting: [],
      hosts: [],
      counts: { needs_you: 0, running: 1, done_recent: 1, hosts_online: 3, hosts_total: 3 },
      since_last_visit: { completed: 1, auto_handled: 0 },
    };

    const entries: PersonalAgentActivityEntry[] = [
      {
        seq: 1,
        timestamp_ms: 2000,
        actor: "personal_agent",
        decision: "allow",
        host_id: "h_other",
        agent_id: "w_any",
        agent_title: "Any Worker",
        summary: "git status",
        risk_level: "medium",
        reason: "Read-only inspection",
        request_id: "r1",
      },
      {
        seq: 2,
        timestamp_ms: 2500,
        actor: "guardian_tier1",
        decision: "allow",
        host_id: "h_linux",
        agent_id: "w_root",
        agent_title: "Build",
        summary: "cargo test",
        risk_level: "low",
        reason: "Workspace tests",
        request_id: "r2",
      },
      {
        seq: 3,
        timestamp_ms: 2600,
        actor: "permission_rule",
        decision: "allow",
        host_id: "h_mac",
        agent_id: "unrelated_session",
        agent_title: "Scratch",
        summary: "ls",
        risk_level: "low",
        reason: "Rule match",
        request_id: "r3",
      },
      {
        seq: 4,
        timestamp_ms: 2700,
        actor: "guardian_tier1",
        decision: "allow",
        host_id: "h_gpu",
        agent_id: "w_child",
        agent_title: "Eval",
        summary: "read file",
        risk_level: "low",
        reason: "Read-only",
        request_id: "r4",
      },
    ];

    const filtered = filterActivityForPersonalAgent(entries, briefing, {
      hostId: "h_mac",
      agentId: "pa_1",
    });
    assert.deepEqual(
      filtered.map((e) => e.seq),
      [1, 2, 4],
    );

    const away = computeSinceLastVisitSummary(briefing, filtered, 2200);
    assert.deepEqual(away, { completed: 1, autoHandled: 2, hasSummary: true });
  });

  it("selects recommended router host and builds sensible defaults", () => {
    const hosts: RouterHostCapabilities[] = [
      {
        host_id: "local",
        host_name: "MacBook",
        is_local: true,
        online: true,
        hermes_installed: false,
        hermes_has_existing_memory: false,
        hermes_acp_ready: false,
        hermes_gateway_running: false,
        available_providers: ["claude_code", "codex"],
        router_enabled: false,
        recommended_score: 50,
      },
      {
        host_id: "studio",
        host_name: "Mac Studio",
        is_local: false,
        online: true,
        hermes_installed: true,
        hermes_has_existing_memory: true,
        hermes_acp_ready: true,
        hermes_gateway_running: false,
        available_providers: ["codex"],
        router_enabled: false,
        recommended_score: 95,
      },
      {
        host_id: "offline_server",
        host_name: "Offline Box",
        is_local: false,
        online: false,
        hermes_installed: true,
        hermes_has_existing_memory: true,
        hermes_acp_ready: true,
        hermes_gateway_running: false,
        available_providers: ["claude_code"],
        router_enabled: false,
        recommended_score: 120,
      },
    ];

    const recommended = selectRecommendedRouterHost(hosts);
    assert.equal(recommended?.host_id, "studio");

    const cfgHermes = buildDefaultRouterConfig(recommended!);
    assert.equal(cfgHermes.enabled, true);
    assert.equal(cfgHermes.provider, "acp");
    assert.equal(cfgHermes.acp_agent_id, "hermes");

    const cfgLocal = buildDefaultRouterConfig(hosts[0]);
    assert.equal(cfgLocal.provider, "claude_code");
    assert.equal(cfgLocal.acp_agent_id, null);
  });

  it("formats compact elapsed durations", () => {
    assert.equal(formatBriefingElapsed(10_000, 35_000), "25s");
    assert.equal(formatBriefingElapsed(0, 35_000), "");
    assert.equal(formatBriefingElapsed(10_000, 10_000 + 5 * 60_000), "5m");
    assert.equal(formatBriefingElapsed(10_000, 10_000 + 3 * 3_600_000), "3h");
  });

  it("reconciles live AgentCardView states so finished, idle, stopped, and timer-waiting workers never stay running", () => {
    // Stale briefing says "running", but live card is Idle with latest_seq > 0 -> completed
    assert.equal(
      resolveEffectiveBriefingWorkerStatus(
        makeWorker({ state: "running" }),
        makeLiveAgent({ is_running: false, status: { state: "idle" }, latest_seq: 12 }),
      ),
      "completed",
    );

    // Closed / stopped card -> canceled
    assert.equal(
      resolveEffectiveBriefingWorkerStatus(
        makeWorker({ state: "running" }),
        makeLiveAgent({ status: { state: "closed" }, status_label: "Stopped" }),
      ),
      "canceled",
    );

    // AutonomousWakeup waiting:* -> idle (not running)
    assert.equal(
      resolveEffectiveBriefingWorkerStatus(
        makeWorker({ state: "idle" }),
        makeLiveAgent({
          status: { state: "autonomous_wakeup", source: "waiting:CI job" },
          has_background_tasks: true,
        }),
      ),
      "idle",
    );

    // Active AutonomousWakeup (non-waiting) and Initializing -> running
    assert.equal(
      resolveEffectiveBriefingWorkerStatus(
        makeWorker({ state: "running" }),
        makeLiveAgent({
          is_running: true,
          status: { state: "autonomous_wakeup", source: "timer" },
        }),
      ),
      "running",
    );
    assert.equal(
      resolveEffectiveBriefingWorkerStatus(
        makeWorker({ state: "running" }),
        makeLiveAgent({ is_running: true, status: { state: "initializing" } }),
      ),
      "running",
    );

    // Provider-native running subagent (no active managed child) stays running;
    // finished parent with active managed child stays completed
    assert.equal(
      resolveEffectiveBriefingWorkerStatus(
        makeWorker({ state: "running" }),
        makeLiveAgent({
          is_running: false,
          status: { state: "idle" },
          has_running_subagents: true,
        }),
        false,
      ),
      "running",
    );
    assert.equal(
      resolveEffectiveBriefingWorkerStatus(
        makeWorker({ state: "running" }),
        makeLiveAgent({
          is_running: false,
          status: { state: "idle" },
          has_running_subagents: true,
        }),
        true,
      ),
      "completed",
    );
  });

  it("partitions missions and other_active strictly by real execution status, deduplicates Needs You, and preserves lineage", () => {
    const missions: BriefingMission[] = [
      // Mixed mission: running root + done child + running child
      {
        key: "m_mixed",
        root: makeWorker({
          host_id: "h_linux",
          host_name: "linux-build",
          agent_id: "ag_root_run",
          title: "Release build",
          state: "running",
        }),
        workers: [
          makeWorker({
            host_id: "h_mac",
            host_name: "Mac Studio",
            agent_id: "ag_child_done",
            title: "Security audit",
            state: "done",
          }),
          makeWorker({
            host_id: "h_linux",
            host_name: "linux-build",
            agent_id: "ag_child_run",
            title: "E2E suite",
            state: "running",
          }),
        ],
        host_names: ["linux-build", "Mac Studio"],
        done: 1,
        total: 3,
        state: "running",
        updated_at_ms: 5000,
      },
      // Done root + running child: running child surfaces in In progress with lineage; done root moves to Completed
      {
        key: "m_done_root_run_child",
        root: makeWorker({
          host_id: "h_mac",
          host_name: "Mac Studio",
          agent_id: "ag_parent_done",
          title: "Coordinator",
          state: "done",
        }),
        workers: [
          makeWorker({
            host_id: "h_mac",
            host_name: "Mac Studio",
            agent_id: "ag_leaf_run",
            title: "Deep check",
            state: "running",
          }),
        ],
        host_names: ["Mac Studio"],
        done: 1,
        total: 2,
        state: "running",
        updated_at_ms: 4000,
      },
      // Fully settled mission: stopped root + idle timer worker (idle timer must NOT be archivable)
      {
        key: "m_settled",
        root: makeWorker({
          host_id: "h_mac",
          host_name: "Mac Studio",
          agent_id: "ag_stopped",
          title: "Stopped task",
          state: "done",
        }),
        workers: [
          makeWorker({
            host_id: "h_mac",
            host_name: "Mac Studio",
            agent_id: "ag_timer_idle",
            title: "CI timer watcher",
            state: "idle",
          }),
        ],
        host_names: ["Mac Studio"],
        done: 1,
        total: 2,
        state: "done",
        updated_at_ms: 3000,
      },
      // Already-archived completed worker must be hidden immediately
      {
        key: "m_archived",
        root: makeWorker({
          host_id: "h_mac",
          host_name: "Mac Studio",
          agent_id: "ag_archived_done",
          title: "Old finished task",
          state: "done",
        }),
        workers: [],
        host_names: ["Mac Studio"],
        done: 1,
        total: 1,
        state: "done",
        updated_at_ms: 2000,
      },
    ];

    const otherActive: BriefingWorker[] = [
      // Truly running non-PA session -> stays in runningOtherWorkers
      makeWorker({
        host_id: "h_mac",
        host_name: "Mac Studio",
        agent_id: "ag_other_run",
        title: "Ad-hoc refactor",
        state: "running",
      }),
      // Awaiting non-PA session already in Needs You -> deduplicated
      makeWorker({
        host_id: "h_linux",
        host_name: "linux-build",
        agent_id: "ag_other_await",
        title: "Deploy prod",
        state: "awaiting",
      }),
      // Idle timer-waiting non-PA session -> must NOT appear in In progress
      makeWorker({
        host_id: "h_linux",
        host_name: "linux-build",
        agent_id: "ag_other_idle",
        title: "Idle watcher",
        state: "idle",
      }),
    ];

    const hostDetails: Record<string, HostDetailView> = {
      h_mac: makeHostDetail("h_mac", [
        makeLiveAgent({ agent_id: "ag_child_done", is_running: false, status: { state: "idle" } }),
        makeLiveAgent({
          agent_id: "ag_parent_done",
          is_running: false,
          has_running_subagents: true,
          status: { state: "idle" },
        }),
        makeLiveAgent({
          agent_id: "ag_leaf_run",
          is_running: true,
          status: { state: "running" },
        }),
        makeLiveAgent({
          agent_id: "ag_stopped",
          is_running: false,
          status: { state: "closed" },
          status_label: "Stopped",
        }),
        makeLiveAgent({
          agent_id: "ag_timer_idle",
          is_running: false,
          has_armed_timers: true,
          status: { state: "idle" },
          latest_seq: 3,
        }),
        makeLiveAgent({
          agent_id: "ag_archived_done",
          is_running: false,
          status: { state: "idle" },
        }),
        makeLiveAgent({
          agent_id: "ag_other_run",
          is_running: true,
          status: { state: "running" },
        }),
      ]),
      h_linux: makeHostDetail("h_linux", [
        makeLiveAgent({
          agent_id: "ag_root_run",
          is_running: true,
          status: { state: "running" },
        }),
        makeLiveAgent({
          agent_id: "ag_child_run",
          is_running: true,
          status: { state: "running" },
        }),
        makeLiveAgent({
          agent_id: "ag_other_await",
          is_awaiting_permission: true,
          status: { state: "awaiting_permission" },
        }),
        makeLiveAgent({
          agent_id: "ag_other_idle",
          is_running: false,
          has_armed_timers: true,
          status: { state: "idle" },
        }),
      ]),
    };

    const result = partitionBriefingWorkers({
      missions,
      otherActive,
      needsYou: [
        {
          key: "perm:h_linux:ag_other_await:r1",
          kind: "permission",
          host_id: "h_linux",
          host_name: "linux-build",
          agent_id: "ag_other_await",
          agent_title: "Deploy prod",
          request_id: "r1",
          summary: "Deploy",
          risk_level: "high",
          human_only: true,
          batch_eligible: false,
          router_recommendation: null,
          dispatched_by_personal_agent: false,
          updated_at_ms: 5000,
        },
      ],
      hostDetails,
      archivedThreads: {
        "h_mac::ag_archived_done": {
          key: "h_mac::ag_archived_done",
          hostId: "h_mac",
          agentId: "ag_archived_done",
          archivedAtMs: 6000,
        },
      },
    });

    // Running PA workers: ONLY ag_root_run (indent 0), ag_child_run (indent 1), ag_leaf_run (indent 0 + lineage)
    assert.deepEqual(
      result.runningMissionWorkers.map((w) => ({
        id: w.worker.agent_id,
        status: w.status,
        indent: w.indentLevel,
        lineage: w.lineage,
      })),
      [
        { id: "ag_root_run", status: "running", indent: 0, lineage: null },
        {
          id: "ag_child_run",
          status: "running",
          indent: 1,
          lineage: "Release build → E2E suite",
        },
        {
          id: "ag_leaf_run",
          status: "running",
          indent: 0,
          lineage: "Coordinator → Deep check",
        },
      ],
    );

    // Running other threads: ONLY ag_other_run (deduped ag_other_await, excluded idle ag_other_idle)
    assert.deepEqual(
      result.runningOtherWorkers.map((w) => w.worker.agent_id),
      ["ag_other_run"],
    );
    assert.equal(result.inProgressCount, 4);

    // Completed section: ag_child_done (completed, lineage preserved), ag_parent_done (completed),
    // ag_stopped (canceled), ag_timer_idle (idle, indented under ag_stopped, NOT archivable).
    // ag_archived_done is hidden.
    assert.deepEqual(
      result.completedWorkers.map((w) => ({
        id: w.worker.agent_id,
        status: w.status,
        indent: w.indentLevel,
        archivable: w.archivable,
        lineage: w.lineage,
      })),
      [
        {
          id: "ag_child_done",
          status: "completed",
          indent: 0,
          archivable: true,
          lineage: "Release build → Security audit",
        },
        {
          id: "ag_parent_done",
          status: "completed",
          indent: 0,
          archivable: true,
          lineage: null,
        },
        {
          id: "ag_stopped",
          status: "canceled",
          indent: 0,
          archivable: true,
          lineage: null,
        },
        {
          id: "ag_timer_idle",
          status: "idle",
          indent: 1,
          archivable: false,
          lineage: "Stopped task → CI timer watcher",
        },
      ],
    );

    assert.deepEqual(
      result.archivableCompletedWorkers.map((w) => w.worker.agent_id),
      ["ag_child_done", "ag_parent_done", "ag_stopped"],
    );
  });

  it("resolves live GoalRunRecord and armed ConditionalTimerRecord for shared Overview rows", () => {
    const hostDetails: Record<string, HostDetailView> = {
      h_mac: makeHostDetail("h_mac", [], {
        goal_runs: [
          {
            goal_id: "g_active",
            agent_id: "pa_1",
            workspace_id: "ws_1",
            objective: "Keep CI green",
            criteria: [],
            verify_command: null,
            max_iterations: 10,
            current_iteration: 2,
            max_duration_secs: 0,
            status: "running",
            last_verify_exit_code: null,
            last_verify_output: null,
            consecutive_identical_failures: 0,
            pause_reason: null,
            paused_by_user: false,
            active_duration_secs: 30,
            started_at_ms: 1000,
            updated_at_ms: 2000,
            finished_at_ms: null,
          },
          {
            goal_id: "g_done",
            agent_id: "pa_2",
            workspace_id: "ws_1",
            objective: "Old goal",
            criteria: [],
            verify_command: null,
            max_iterations: 5,
            current_iteration: 5,
            max_duration_secs: 0,
            status: "succeeded",
            last_verify_exit_code: 0,
            last_verify_output: null,
            consecutive_identical_failures: 0,
            pause_reason: null,
            paused_by_user: false,
            active_duration_secs: 30,
            started_at_ms: 1000,
            updated_at_ms: 2000,
            finished_at_ms: 2000,
          },
        ],
        conditional_timers: [
          {
            timer_id: "tm_armed",
            agent_id: "pa_1",
            workspace_id: "ws_1",
            prompt: "Check deploy",
            duration_seconds: 120,
            condition: { kind: "all_subagents_settled" },
            state: "armed",
            created_at_ms: 1000,
            fire_at_ms: 121_000,
            resolved_at_ms: null,
            resolved_reason: null,
          },
          {
            timer_id: "tm_fired",
            agent_id: "pa_1",
            workspace_id: "ws_1",
            prompt: "Already fired",
            duration_seconds: 60,
            condition: { kind: "never" },
            state: "fired",
            created_at_ms: 500,
            fire_at_ms: 60_500,
            resolved_at_ms: 60_500,
            resolved_reason: "timer_elapsed",
          },
        ],
      }),
    };

    const activeGoalItem: BriefingWaitItem = {
      key: "goal:h_mac:g_active",
      kind: "goal",
      host_id: "h_mac",
      agent_id: "pa_1",
      title: "Keep CI green",
      detail: "running · 2/10",
      due_at_ms: null,
    };
    const doneGoalItem: BriefingWaitItem = {
      key: "goal:h_mac:g_done",
      kind: "goal",
      host_id: "h_mac",
      agent_id: "pa_2",
      title: "Old goal",
      detail: "succeeded · 5/5",
      due_at_ms: null,
    };
    const armedTimerItem: BriefingWaitItem = {
      key: "timer:h_mac:tm_armed",
      kind: "timer",
      host_id: "h_mac",
      agent_id: "pa_1",
      title: "Personal Agent",
      detail: "all_subagents · Check deploy",
      due_at_ms: 121_000,
    };
    const firedTimerItem: BriefingWaitItem = {
      key: "timer:h_mac:tm_fired",
      kind: "timer",
      host_id: "h_mac",
      agent_id: "pa_1",
      title: "Personal Agent",
      detail: "timer",
      due_at_ms: 60_500,
    };

    assert.equal(findLiveBriefingGoal(activeGoalItem, hostDetails)?.goal_id, "g_active");
    assert.equal(findLiveBriefingGoal(doneGoalItem, hostDetails), null);
    assert.equal(findLiveBriefingTimer(armedTimerItem, hostDetails)?.timer_id, "tm_armed");
    assert.equal(findLiveBriefingTimer(firedTimerItem, hostDetails), null);
  });
});
