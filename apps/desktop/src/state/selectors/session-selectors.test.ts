import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type {
  AgentCardView,
  ConditionalTimerRecord,
  GoalRunRecord,
  HostCardView,
  HostDetailView,
  PendingPermissionSummary,
  SessionBackgroundTaskView,
  SessionTouchedFileView,
} from "../../lib/orbit-ipc.ts";
import {
  buildOverviewLinkItems,
  doesTouchedFileMatchOpenPath,
  doesUrlMatchOpenBrowserUrl,
  findGitReviewForCwd,
  formatTouchedFilesForOverview,
  hasCrossHostChildAgents,
  hasLocalSubagentRecords,
  mergeSessionPullRequests,
  selectFleetHealth,
  selectPendingDecisions,
  selectScopedTerminalCount,
  selectSessionRuntimeActivity,
  selectSessionSubagents,
  selectWorkspaceArtifactsList,
  selectWorkspaceToolSummary,
  splitFormattedTouchedFiles,
} from "./session-selectors.ts";

function makeHostCard(overrides: Partial<HostCardView> = {}): HostCardView {
  return {
    host_id: "local",
    display_name: "Local Mac",
    connection_state: "online_direct",
    daemon_version: "0.9.7",
    daemon_protocol_version: 1,
    client_protocol_version: 1,
    available_providers: ["codex"],
    pending_approvals: 0,
    running_agents: 0,
    is_local: true,
    ...overrides,
  } as HostCardView;
}

function makeAgent(overrides: Partial<AgentCardView> = {}): AgentCardView {
  return {
    agent_id: "parent-1",
    workspace_id: "ws-1",
    title: "Main Thread",
    provider: "codex",
    model: "gpt-5",
    status_label: "Idle",
    is_running: false,
    is_awaiting_permission: false,
    has_background_tasks: false,
    latest_seq: 10,
    updated_at_rfc3339: "2026-11-23T12:00:00Z",
    ...overrides,
  } as AgentCardView;
}

function makeHostDetail(
  host: HostCardView,
  agents: AgentCardView[],
  pendingPermissions: PendingPermissionSummary[] = [],
): HostDetailView {
  return {
    revision: 1,
    host,
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
    pending_permissions: pendingPermissions,
  } as HostDetailView;
}

describe("session-selectors", () => {
  it("resolves session subagents with unique OneWorks characters and aggregates pending decisions", () => {
    const host = makeHostCard();
    const parent = makeAgent({ agent_id: "parent-1" });
    const childA = makeAgent({
      agent_id: "child-a",
      parent_agent_id: "parent-1",
      parent_host_id: "local",
      title: "Codebase Researcher",
      is_running: true,
    });
    const childB = makeAgent({
      agent_id: "child-b",
      parent_agent_id: "parent-1",
      parent_host_id: "local",
      title: "Migration Auditor",
      is_awaiting_permission: true,
    });

    const childPerm: PendingPermissionSummary = {
      agent_id: "child-b",
      agent_title: "Migration Auditor",
      request_id: "req-42",
      title: "Run cargo test",
      detail: {
        kind: "shell",
        command: "cargo test --workspace",
        cwd: null,
        output_preview: null,
        exit_code: null,
      },
      allowed_decisions: ["allow", "allow_always", "deny"],
      created_at: "2026-11-23T12:01:00Z",
    };

    const state = {
      fleet: {
        revision: 1,
        total_pending_approvals: 1,
        hosts: [host],
        projects: [],
      },
      hostDetails: {
        local: makeHostDetail(host, [parent, childA, childB], [childPerm]),
      },
      stages: {},
      archivedSubagents: {},
    };

    const summary = selectSessionSubagents(state, "local", "parent-1");
    assert.equal(summary.visible.length, 2);
    assert.equal(summary.activeCount, 2);
    assert.equal(summary.awaitingCount, 1);
    assert.notEqual(summary.visible[0]!.character.id, summary.visible[1]!.character.id);

    const decisions = selectPendingDecisions(state, "local", "parent-1");
    assert.equal(decisions.totalCount, 1);
    assert.equal(decisions.subagentPermissions.length, 1);
    assert.equal(decisions.subagentPermissions[0]!.permission.request_id, "req-42");
    assert.equal(decisions.subagentPermissions[0]!.subagent.title, "Migration Auditor");

    const runtime = selectSessionRuntimeActivity(state, "local", "parent-1");
    assert.equal(runtime.overviewIndicatorTone, "approval");
    assert.equal(runtime.overviewIndicatorReason, "approval");

    const health = selectFleetHealth(state);
    assert.equal(health.onlineDirectCount, 1);
    assert.equal(health.totalPendingApprovals, 1);
  });

  it("distinguishes overview indicator reasons across subagent, background task, goal, and timer", () => {
    const host = makeHostCard();
    const parent = makeAgent({ agent_id: "parent-1" });
    const baseFleet = {
      revision: 1,
      total_pending_approvals: 0,
      hosts: [host],
      projects: [],
    };

    const bgTask: SessionBackgroundTaskView = {
      id: "task-1",
      kind: "shell",
      title: "npm run tauri dev",
      subtitle: "",
      status: "running",
      command: "npm run tauri dev",
      log_path: null,
      output_preview: null,
      started_at_ms: 1_000,
    };
    const runningBg = selectSessionRuntimeActivity(
      {
        fleet: baseFleet,
        hostDetails: { local: makeHostDetail(host, [parent]) },
        archivedSubagents: {},
      },
      "local",
      "parent-1",
      { backgroundTasks: [bgTask] },
    );
    assert.equal(runningBg.overviewIndicatorTone, "running");
    assert.equal(runningBg.overviewIndicatorReason, "background_task");

    const goal = {
      goal_id: "goal-1",
      agent_id: "parent-1",
      objective: "Ship release",
      verify_command: "cargo test",
      status: "running",
      current_iteration: 1,
      max_iterations: 5,
    } as GoalRunRecord;
    const activeGoalState = selectSessionRuntimeActivity(
      {
        fleet: baseFleet,
        hostDetails: {
          local: {
            ...makeHostDetail(host, [parent]),
            goal_runs: [goal],
          },
        },
        archivedSubagents: {},
      },
      "local",
      "parent-1",
    );
    assert.equal(activeGoalState.overviewIndicatorTone, "goal");
    assert.equal(activeGoalState.overviewIndicatorReason, "goal");

    const timer = {
      timer_id: "timer-1",
      agent_id: "parent-1",
      state: "armed",
      prompt: "Check CI",
      fire_at_ms: 999_999,
      condition: { kind: "never" },
    } as unknown as ConditionalTimerRecord;
    const armedTimerState = selectSessionRuntimeActivity(
      {
        fleet: baseFleet,
        hostDetails: {
          local: {
            ...makeHostDetail(host, [parent]),
            conditional_timers: [timer],
          },
        },
        archivedSubagents: {},
      },
      "local",
      "parent-1",
    );
    assert.equal(armedTimerState.overviewIndicatorTone, "goal");
    assert.equal(armedTimerState.overviewIndicatorReason, "timer");
  });

  it("scopes terminal counts in selectWorkspaceToolSummary to the active project cwd", () => {
    const host = makeHostCard();
    const parent = makeAgent({ agent_id: "parent-1", cwd: "/Users/dev/orbit" });
    const detail: HostDetailView = {
      ...makeHostDetail(host, [parent]),
      terminals: [
        {
          terminal_id: "term-1",
          cwd: "/Users/dev/orbit",
          title: "zsh",
          cols: 120,
          rows: 30,
          created_at: "2026-11-23T12:00:00Z",
        },
        {
          terminal_id: "term-2",
          cwd: "/Users/dev/orbit/apps/desktop",
          title: "vite",
          cols: 120,
          rows: 30,
          created_at: "2026-11-23T12:00:00Z",
        },
        {
          terminal_id: "term-other",
          cwd: "/Users/dev/orbit-unrelated",
          title: "other",
          cols: 120,
          rows: 30,
          created_at: "2026-11-23T12:00:00Z",
        },
      ],
    };

    const summary = selectWorkspaceToolSummary(
      {
        hostDetails: { local: detail },
        gitReviews: {},
        terminalTabs: [
          {
            key: "local::0",
            hostId: "local",
            terminalId: "term-1",
            slot: 0,
            cwd: "/Users/dev/orbit",
            title: "zsh",
          },
          {
            key: "local::1",
            hostId: "local",
            terminalId: "term-other",
            slot: 1,
            cwd: "/Users/dev/orbit-unrelated",
            title: "other",
          },
        ],
      },
      "local",
      "parent-1",
      "/Users/dev/orbit",
      [],
      parent,
      [],
    );

    // term-1 (open tab in /Users/dev/orbit) + term-2 (detached in /Users/dev/orbit/apps/desktop) = 2;
    // term-other (/Users/dev/orbit-unrelated) must be excluded.
    assert.equal(summary.terminalCount, 2);
  });

  it("orders modified touched files ahead of reads and disambiguates duplicate basenames", () => {
    const files: SessionTouchedFileView[] = [
      {
        path: "/repo/README.md",
        display_path: "README.md",
        action: "read",
        additions: 0,
        deletions: 0,
        last_seq: 50,
      },
      {
        path: "/repo/crates/orbit-cli/src/lib.rs",
        display_path: "crates/orbit-cli/src/lib.rs",
        action: "edited",
        additions: 12,
        deletions: 3,
        last_seq: 20,
      },
      {
        path: "/repo/crates/orbit-daemon/src/lib.rs",
        display_path: "crates/orbit-daemon/src/lib.rs",
        action: "created",
        additions: 40,
        deletions: 0,
        last_seq: 30,
      },
      {
        path: "/repo/apps/desktop/src/stage/panel.tsx",
        display_path: "apps/desktop/src/stage/panel.tsx",
        action: "read",
        additions: 0,
        deletions: 0,
        last_seq: 40,
      },
    ];

    const formatted = formatTouchedFilesForOverview(files);
    assert.deepEqual(
      formatted.map((item) => item.file.display_path),
      [
        "crates/orbit-daemon/src/lib.rs",
        "crates/orbit-cli/src/lib.rs",
        "README.md",
        "apps/desktop/src/stage/panel.tsx",
      ],
    );
    assert.equal(formatted[0]!.basename, "lib.rs");
    assert.equal(formatted[0]!.dirHint, "orbit-daemon/src");
    assert.equal(formatted[1]!.basename, "lib.rs");
    assert.equal(formatted[1]!.dirHint, "orbit-cli/src");
    assert.equal(formatted[2]!.dirHint, null);
    assert.equal(formatted[3]!.dirHint, null);
  });

  it("supports narrow git review lookup, scoped terminal counting, and cross-host child detection", () => {
    const localHost = makeHostCard({ host_id: "local" });
    const remoteHost = makeHostCard({ host_id: "gpu-1" });
    const parent = makeAgent({ agent_id: "parent-1" });
    const unrelatedRemote = makeAgent({ agent_id: "remote-solo" });
    const delegatedRemote = makeAgent({
      agent_id: "remote-child",
      parent_agent_id: "parent-1",
      parent_host_id: "local",
    });

    assert.equal(
      hasCrossHostChildAgents(
        {
          local: makeHostDetail(localHost, [parent]),
          "gpu-1": makeHostDetail(remoteHost, [unrelatedRemote]),
        },
        "local",
        "parent-1",
      ),
      false,
    );

    assert.equal(
      hasCrossHostChildAgents(
        {
          local: makeHostDetail(localHost, [parent]),
          "gpu-1": makeHostDetail(remoteHost, [delegatedRemote]),
        },
        "local",
        "parent-1",
      ),
      true,
    );

    const reviewA = { cwd: "/Users/dev/orbit", diff_files: [], status: null } as unknown as NonNullable<
      ReturnType<typeof findGitReviewForCwd>
    >;
    const gitReviews = { "local::/Users/dev/orbit/": reviewA };
    assert.equal(findGitReviewForCwd(gitReviews, "local", "/Users/dev/orbit"), reviewA);
    assert.equal(findGitReviewForCwd(gitReviews, "local", "/Users/dev/other"), null);

    assert.equal(
      selectScopedTerminalCount(
        [
          {
            key: "local::0",
            hostId: "local",
            terminalId: "t1",
            slot: 0,
            cwd: "/Users/dev/orbit",
            title: "zsh",
          },
        ],
        [
          {
            terminal_id: "t1",
            cwd: "/Users/dev/orbit",
            title: "zsh",
            cols: 80,
            rows: 24,
            created_at: "2026-11-23T12:00:00Z",
          },
          {
            terminal_id: "t2",
            cwd: "/Users/dev/other",
            title: "zsh",
            cols: 80,
            rows: 24,
            created_at: "2026-11-23T12:00:00Z",
          },
        ],
        "local",
        "/Users/dev/orbit",
      ),
      1,
    );

    assert.equal(
      doesTouchedFileMatchOpenPath(
        "apps/desktop/src/App.tsx",
        {
          path: "/Users/dev/orbit/apps/desktop/src/App.tsx",
          display_path: "apps/desktop/src/App.tsx",
        },
        "/Users/dev/orbit",
      ),
      true,
    );
    assert.equal(
      doesTouchedFileMatchOpenPath(
        "/Users/dev/orbit/apps/desktop/src/App.tsx",
        {
          path: "apps/desktop/src/App.tsx",
          display_path: "apps/desktop/src/App.tsx",
        },
        "/Users/dev/orbit/",
      ),
      true,
    );
    assert.equal(
      doesTouchedFileMatchOpenPath(
        "apps/desktop/src/Other.tsx",
        {
          path: "/Users/dev/orbit/apps/desktop/src/App.tsx",
          display_path: "apps/desktop/src/App.tsx",
        },
        "/Users/dev/orbit",
      ),
      false,
    );

    assert.equal(
      hasLocalSubagentRecords(makeHostDetail(localHost, [parent]), "local", "parent-1"),
      false,
    );
    assert.equal(
      hasLocalSubagentRecords(
        makeHostDetail(localHost, [
          parent,
          makeAgent({ agent_id: "child-1", parent_agent_id: "parent-1" }),
        ]),
        "local",
        "parent-1",
      ),
      true,
    );
  });

  it("merges pull requests, combines local/external links, and splits modified vs read touched files", () => {
    const mergedPrs = mergeSessionPullRequests(
      [
        {
          url: "https://github.com/danielgaoyy/orbit/pull/43",
          number: 43,
          repo: "danielgaoyy/orbit",
          title: null,
          last_seq: 12,
        },
      ],
      [
        {
          url: "https://github.com/danielgaoyy/orbit/pull/43/",
          number: 43,
          repo: "danielgaoyy/orbit",
          title: "Polish Overview HUD",
          last_seq: 99,
        },
        {
          url: "https://github.com/danielgaoyy/orbit/pull/44",
          number: 44,
          repo: "danielgaoyy/orbit",
          title: "Fix Rust daemon",
          last_seq: 50,
        },
      ],
    );
    assert.equal(mergedPrs.length, 2);
    assert.equal(mergedPrs[0]!.number, 43);
    assert.equal(mergedPrs[0]!.title, "Polish Overview HUD");
    assert.equal(mergedPrs[1]!.number, 44);

    const links = buildOverviewLinkItems(
      ["http://localhost:5173/", "http://localhost:5173"],
      [
        {
          url: "https://docs.rs/tokio",
          label: "Tokio Docs",
          host: "docs.rs",
          last_seq: 10,
        },
      ],
    );
    assert.equal(links.length, 2);
    assert.equal(links[0]!.kind, "local");
    assert.equal(links[0]!.label, "localhost:5173");
    assert.equal(links[1]!.kind, "external");
    assert.equal(links[1]!.label, "Tokio Docs");

    const formatted = formatTouchedFilesForOverview([
      {
        path: "/repo/a.ts",
        display_path: "a.ts",
        action: "edited",
        additions: 4,
        deletions: 1,
        last_seq: 10,
      },
      {
        path: "/repo/b.ts",
        display_path: "b.ts",
        action: "read",
        additions: 0,
        deletions: 0,
        last_seq: 20,
      },
    ]);
    const split = splitFormattedTouchedFiles(formatted);
    assert.equal(split.modified.length, 1);
    assert.equal(split.modified[0]!.basename, "a.ts");
    assert.equal(split.read.length, 1);
    assert.equal(split.read[0]!.basename, "b.ts");

    assert.equal(
      doesUrlMatchOpenBrowserUrl("http://localhost:5173", "http://localhost:5173/"),
      true,
    );
    assert.equal(
      doesUrlMatchOpenBrowserUrl("http://localhost:5173/other", "http://localhost:5173/"),
      false,
    );

    const sessionArtifacts = selectWorkspaceArtifactsList(
      [
        {
          artifact_id: "art-1",
          workspace_id: "ws-1",
          agent_id: "parent-1",
          title: "Current Session Plan",
          kind: "markdown",
          version: 2,
          byte_len: 100,
          created_at: "2026-11-23T12:00:00Z",
          updated_at: "2026-11-23T12:05:00Z",
        },
        {
          artifact_id: "art-other",
          workspace_id: "ws-1",
          agent_id: "other-thread",
          title: "Other Thread Artifact",
          kind: "code",
          version: 1,
          byte_len: 50,
          created_at: "2026-11-23T12:00:00Z",
          updated_at: "2026-11-23T12:06:00Z",
        },
        {
          artifact_id: "art-shared",
          workspace_id: "ws-1",
          agent_id: null,
          title: "Shared Workspace Spec",
          kind: "markdown",
          version: 1,
          byte_len: 80,
          created_at: "2026-11-23T12:00:00Z",
          updated_at: "2026-11-23T12:01:00Z",
        },
      ],
      "ws-1",
      "parent-1",
    );
    assert.deepEqual(
      sessionArtifacts.map((a) => a.artifact_id),
      ["art-1", "art-shared"],
    );
  });
});
