import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type {
  AgentCardView,
  HostCardView,
  HostDetailView,
  PendingPermissionSummary,
} from "../../lib/orbit-ipc.ts";
import {
  selectFleetHealth,
  selectPendingDecisions,
  selectSessionRuntimeActivity,
  selectSessionSubagents,
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

    const health = selectFleetHealth(state);
    assert.equal(health.onlineDirectCount, 1);
    assert.equal(health.totalPendingApprovals, 1);
  });
});
