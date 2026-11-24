import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type {
  AgentCardView,
  FleetSidebarView,
  HostCardView,
  HostDetailView,
  SidebarConversationView,
  SidebarProjectGroupView,
} from "../../lib/orbit-ipc.ts";
import {
  buildRunningParentThreadKeys,
  buildSidebarDisplayModel,
  collectVisibleSidebarThreads,
} from "./sidebar-model.ts";

function makeHost(id = "h1", name = "MacBook"): HostCardView {
  return {
    host_id: id,
    display_name: name,
    connection_state: "online_direct",
    daemon_version: "0.9.7",
    available_providers: ["claude_code"],
  } as unknown as HostCardView;
}

function makeAgent(id: string, title: string, overrides: Partial<AgentCardView> = {}): AgentCardView {
  return {
    agent_id: id,
    host_id: "h1",
    title,
    cwd: "/repo/orbit",
    provider: "claude_code",
    is_running: false,
    is_awaiting_permission: false,
    updated_at_rfc3339: "2026-11-23T12:00:00Z",
    status_label: "Idle",
    ...overrides,
  } as unknown as AgentCardView;
}

function makeConv(
  agent: AgentCardView,
  overrides: Partial<SidebarConversationView> = {},
): SidebarConversationView {
  return {
    host_id: "h1",
    agent,
    is_worktree: false,
    is_unread: false,
    ...overrides,
  } as unknown as SidebarConversationView;
}

describe("sidebar-model pure derivation & invariants", () => {
  it("matches curated group titles during fuzzy filtering and reveals their threads", () => {
    const a1 = makeAgent("a1", "Refactor SQLite queries");
    const a2 = makeAgent("a2", "Update README");
    const c1 = makeConv(a1);
    const c2 = makeConv(a2);

    const project: SidebarProjectGroupView = {
      id: "proj_1",
      host_id: "h1",
      name: "Orbit",
      primary_cwd: "/repo/orbit",
      directories: ["/repo/orbit"],
      conversations: [c1, c2],
      unclassified_conversations: [c2],
      workspaces: [
        {
          workspace_id: "ws_pay",
          title: "Payments Sprint",
          is_curated: true,
          conversations: [c1],
          peer_count: 1,
          has_conflict: false,
        },
      ],
    } as unknown as SidebarProjectGroupView;

    const sidebarView: FleetSidebarView = {
      needs_attention: [],
      pinned_conversations: [],
      projects: [project],
    } as unknown as FleetSidebarView;

    const model = buildSidebarDisplayModel({
      hosts: [makeHost()],
      sidebarView,
      hostDetails: {},
      archivedThreads: {},
      hiddenProjectKeys: {},
      threadCustomTitles: {},
      collapsedProjectIds: { proj_1: true },
      collapsedWorkspaceIds: { ws_pay: true },
      sidebarProjectSort: "recent",
      threadManualOrder: {},
      workspacePanel: "conversation",
      isDraftNewSession: false,
      selectedHostId: null,
      selectedAgentId: null,
      selectedWorkspaceCwd: null,
      expandedMoreProjects: {},
      filterQuery: "pay",
    });

    assert.equal(model.projectRenderModels.length, 1);
    assert.equal(model.projectRenderModels[0].collapsed, false);
    assert.equal(model.projectRenderModels[0].curatedWorkspaces.length, 1);
    assert.equal(model.projectRenderModels[0].curatedWorkspaces[0].wsCollapsed, false);
    assert.deepEqual(
      model.projectRenderModels[0].curatedWorkspaces[0].visiblePeers.map((c) => c.agent.agent_id),
      ["a1"],
    );
    assert.equal(model.projectRenderModels[0].unclassifiedConversations.length, 0);
  });

  it("deduplicates Needs Attention and Pinned threads from below and matches Cmd+1..9 visual order", () => {
    const aAttn = makeAgent("a_attn", "Needs approval", { is_awaiting_permission: true });
    const aPin = makeAgent("a_pin", "Pinned thread");
    const aGrp = makeAgent("a_grp", "Grouped thread");
    const aFlat = makeAgent("a_flat", "Flat thread");

    const cAttn = makeConv(aAttn);
    const cPin = makeConv(aPin);
    const cGrp = makeConv(aGrp);
    const cFlat = makeConv(aFlat);

    const sidebarView: FleetSidebarView = {
      needs_attention: [cAttn],
      pinned_conversations: [cAttn, cPin],
      projects: [
        {
          id: "proj_1",
          host_id: "h1",
          name: "Orbit",
          primary_cwd: "/repo/orbit",
          directories: ["/repo/orbit"],
          conversations: [cAttn, cPin, cGrp, cFlat],
          unclassified_conversations: [cAttn, cPin, cFlat],
          workspaces: [
            {
              workspace_id: "ws_1",
              title: "Group A",
              is_curated: true,
              conversations: [cGrp],
              peer_count: 1,
              has_conflict: false,
            },
          ],
        },
      ],
    } as unknown as FleetSidebarView;

    const baseInput = {
      hosts: [makeHost()],
      sidebarView,
      hostDetails: {},
      archivedThreads: {},
      hiddenProjectKeys: {},
      threadCustomTitles: {},
      collapsedProjectIds: {},
      collapsedWorkspaceIds: {},
      sidebarProjectSort: "recent" as const,
      threadManualOrder: {},
      workspacePanel: "conversation" as const,
      isDraftNewSession: false,
      selectedHostId: null,
      selectedAgentId: null,
      selectedWorkspaceCwd: null,
    };

    const model = buildSidebarDisplayModel({
      ...baseInput,
      expandedMoreProjects: {},
      filterQuery: "",
    });

    // Attention appears once; Pinned omits Attention; Projects omits both Attention & Pinned
    assert.deepEqual(
      model.filteredAttentionItems.map((i) => i.conv.agent.agent_id),
      ["a_attn"],
    );
    assert.deepEqual(
      model.filteredPinnedEntries.map((i) => i.conv.agent.agent_id),
      ["a_pin"],
    );
    assert.deepEqual(
      model.projectRenderModels[0].unclassifiedConversations.map((c) => c.agent.agent_id),
      ["a_flat"],
    );

    // Cmd+1..9 order matches exact top-to-bottom visible sidebar sequence
    const visible = collectVisibleSidebarThreads(baseInput);
    assert.deepEqual(
      visible.map((v) => v.agentId),
      ["a_attn", "a_pin", "a_grp", "a_flat"],
    );
  });

  it("resolves running parent threads from hostDetails & stageRunningSubagentMeta without touching stages", () => {
    const childRunning = makeAgent("child_1", "Subagent", {
      parent_agent_id: "parent_a",
      is_running: true,
      status_label: "Running",
    });
    const childDone = makeAgent("child_done", "Done subagent", {
      parent_agent_id: "parent_b",
      is_running: false,
      status_label: "Idle",
    });
    const childTimerWaiting = makeAgent("child_timer", "Timer subagent", {
      parent_agent_id: "parent_timer",
      is_running: false,
      has_background_tasks: true,
      has_armed_timers: true,
      status_label: "Waiting for 1 timer",
    });
    const childLegacyTimerWaiting = makeAgent("child_legacy_timer", "Legacy timer subagent", {
      parent_agent_id: "parent_legacy_timer",
      is_running: true,
      has_background_tasks: true,
      status_label: "Waiting for 1 timer",
    });

    const hostDetails: Record<string, HostDetailView> = {
      h1: {
        agents: [childRunning, childDone, childTimerWaiting, childLegacyTimerWaiting],
        provider_subagents: {
          parent_c: [{ subagent_id: "ps_1", status: "running" }],
          parent_d: [{ subagent_id: "ps_2", status: "completed" }],
        },
      } as unknown as HostDetailView,
    };

    const runningKeys = buildRunningParentThreadKeys(hostDetails, {}, {
      "h1::parent_b": [{ child_agent_id: "child_done" }],
      "h1::parent_d": [{ provider_subagent_id: "ps_2" }],
      "h1::parent_e": [{ provider_subagent_id: "ps_unknown" }],
    });

    assert.equal(runningKeys.has("h1::parent_a"), true);
    assert.equal(runningKeys.has("h1::parent_b"), false);
    assert.equal(runningKeys.has("h1::parent_c"), true);
    assert.equal(runningKeys.has("h1::parent_d"), false);
    assert.equal(runningKeys.has("h1::parent_e"), true);
    assert.equal(runningKeys.has("h1::parent_timer"), false);
    assert.equal(runningKeys.has("h1::parent_legacy_timer"), false);
  });
});
