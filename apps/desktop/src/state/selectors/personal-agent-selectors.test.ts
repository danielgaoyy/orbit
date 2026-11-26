import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { AgentCardView, HostCardView, HostDetailView } from "../../lib/orbit-ipc.ts";
import {
  isDefaultPersonalAgentTitle,
  personalAgentMatchRank,
  selectIsSelectedAgentPersonalAgent,
  selectPersonalAgentRef,
} from "./personal-agent-selectors.ts";

function makeHost(host_id: string, display_name: string): HostCardView {
  return {
    host_id,
    display_name,
    hostname: host_id,
    custom_name: null,
    ssh_alias: null,
    route_mode: "auto",
    remote_daemon_port: 6868,
    custom_direct_endpoint: null,
    via_ssh_tunnel: false,
    last_ping_ms: 10,
    daemon_version: "0.11.0",
    os_arch: "darwin/arm64",
    daemon_pubkey_b64: "",
    direct_endpoints: [],
    relay_url: null,
    connection_state: "online_direct",
    last_error: null,
    available_providers: ["claude_code"],
    workspace_count: 1,
    agent_count: 1,
    pending_approval_count: 0,
  };
}

function makeAgent(overrides: Partial<AgentCardView> & { agent_id: string }): AgentCardView {
  return {
    workspace_id: "ws_1",
    cwd: "/repo",
    title: "Session",
    provider: "claude_code",
    provider_label: "Claude Code",
    model: null,
    effort: null,
    mode: "default",
    status_label: "Idle",
    is_running: false,
    is_awaiting_permission: false,
    parent_agent_id: null,
    latest_seq: 1,
    updated_at_rfc3339: "2026-11-26T00:00:00Z",
    is_pinned: false,
    seen_seq: 1,
    is_unread: false,
    ...overrides,
  };
}

function makeDetail(host: HostCardView, agents: AgentCardView[]): HostDetailView {
  return {
    revision: 1,
    host,
    discovered_projects: [],
    directory_listing: null,
    models_by_provider: {},
    workspaces: [],
    agents,
    quotas: [],
    terminals: [],
    profiles: [],
    schedules: [],
    connected_hosts: [],
    orchestration_config: null,
    orchestration_skills: [],
  } as unknown as HostDetailView;
}

describe("selectPersonalAgentRef", () => {
  it("prioritizes orbit.role=personal_router > handle=router > title containing Personal Router", () => {
    const hostA = makeHost("h_a", "Mac Studio");
    const titleAgent = makeAgent({ agent_id: "a_title", title: "Personal Router Helper" });
    const handleAgent = makeAgent({ agent_id: "a_handle", title: "General", handle: "router" });
    const roleAgent = makeAgent({
      agent_id: "a_role",
      title: "Chief of Staff",
      labels: { "orbit.role": "personal_router" },
    });

    assert.equal(personalAgentMatchRank(roleAgent), 0);
    assert.equal(personalAgentMatchRank(handleAgent), 1);
    assert.equal(personalAgentMatchRank(titleAgent), 2);

    const state = {
      fleet: { revision: 1, total_pending_approvals: 0, hosts: [hostA] },
      hostDetails: { h_a: makeDetail(hostA, [titleAgent, handleAgent, roleAgent]) },
      archivedThreads: {},
      primaryRouterHostId: null,
    };

    const ref = selectPersonalAgentRef(state);
    assert.equal(ref?.agentId, "a_role");

    const withoutRole = {
      ...state,
      hostDetails: { h_a: makeDetail(hostA, [titleAgent, handleAgent]) },
    };
    assert.equal(selectPersonalAgentRef(withoutRole)?.agentId, "a_handle");

    const titleOnly = {
      ...state,
      hostDetails: { h_a: makeDetail(hostA, [titleAgent]) },
    };
    assert.equal(selectPersonalAgentRef(titleOnly)?.agentId, "a_title");
  });

  it("prefers primary host on equal rank while letting role label beat weaker matches", () => {
    const hostA = makeHost("h_a", "Mac Studio");
    const hostB = makeHost("h_b", "Linux Build");
    const paA = makeAgent({
      agent_id: "pa_a",
      title: "PA A",
      labels: { "orbit.role": "personal_router" },
    });
    const paB = makeAgent({
      agent_id: "pa_b",
      title: "PA B",
      labels: { "orbit.role": "personal_router" },
    });

    const state = {
      fleet: { revision: 1, total_pending_approvals: 0, hosts: [hostA, hostB] },
      hostDetails: {
        h_a: makeDetail(hostA, [paA]),
        h_b: makeDetail(hostB, [paB]),
      },
      archivedThreads: {},
      primaryRouterHostId: "h_b",
    };

    const ref = selectPersonalAgentRef(state);
    assert.equal(ref?.hostId, "h_b");
    assert.equal(ref?.agentId, "pa_b");
    assert.equal(ref?.hostName, "Linux Build");

    // Referential stability for React 19 useSyncExternalStore
    assert.strictEqual(selectPersonalAgentRef(state), ref);

    // Online host beats offline host when no primary host is set
    const hostOffline = { ...makeHost("h_off", "Offline Box"), connection_state: "disconnected" as const };
    const paOff = makeAgent({
      agent_id: "pa_off",
      labels: { "orbit.role": "personal_router" },
      updated_at_rfc3339: "2026-11-26T12:00:00Z",
    });
    const paOn = makeAgent({
      agent_id: "pa_on",
      labels: { "orbit.role": "chief_of_staff" },
      updated_at_rfc3339: "2026-11-26T01:00:00Z",
    });
    const onlineCheck = selectPersonalAgentRef({
      fleet: { revision: 1, total_pending_approvals: 0, hosts: [hostOffline, hostA] },
      hostDetails: {
        h_off: makeDetail(hostOffline, [paOff]),
        h_a: makeDetail(hostA, [paOn]),
      },
      archivedThreads: {},
      primaryRouterHostId: null,
    });
    assert.equal(onlineCheck?.agentId, "pa_on");
  });

  it("excludes daemon-archived and locally archived sessions", () => {
    const hostA = makeHost("h_a", "Mac Studio");
    const archivedDaemon = makeAgent({
      agent_id: "pa_archived",
      labels: { "orbit.role": "personal_router" },
      archived_at_rfc3339: "2026-11-25T00:00:00Z",
    });
    const archivedLocal = makeAgent({
      agent_id: "pa_local_archived",
      handle: "router",
    });
    const fallback = makeAgent({
      agent_id: "pa_live",
      title: "Personal Router",
    });

    const state = {
      fleet: { revision: 1, total_pending_approvals: 0, hosts: [hostA] },
      hostDetails: { h_a: makeDetail(hostA, [archivedDaemon, archivedLocal, fallback]) },
      archivedThreads: {
        "h_a::pa_local_archived": {
          key: "h_a::pa_local_archived",
          hostId: "h_a",
          agentId: "pa_local_archived",
          archivedAtMs: 1,
        },
      },
      primaryRouterHostId: "h_a",
    };

    const ref = selectPersonalAgentRef(state);
    assert.equal(ref?.agentId, "pa_live");

    assert.equal(
      selectIsSelectedAgentPersonalAgent({
        ...state,
        workspacePanel: "conversation",
        isDraftNewSession: false,
        selectedHostId: "h_a",
        selectedAgentId: "pa_live",
      }),
      true,
    );
    assert.equal(
      selectIsSelectedAgentPersonalAgent({
        ...state,
        workspacePanel: "conversation",
        isDraftNewSession: true,
        selectedHostId: "h_a",
        selectedAgentId: "pa_live",
      }),
      false,
    );
  });

  it("identifies legacy/default Personal Agent titles for localized header display while preserving custom titles", () => {
    for (const raw of [
      "",
      "  ",
      null,
      undefined,
      "Personal Router",
      "personal router",
      "Personal Agent",
      "router",
      "@router",
      "个人代理",
      "パーソナルエージェント",
    ]) {
      assert.equal(
        isDefaultPersonalAgentTitle(raw),
        true,
        `expected ${JSON.stringify(raw)} to be treated as default PA title`,
      );
    }

    for (const custom of [
      "Personal Agent · Fleet Release Coordinator",
      "My Chief of Staff",
      "Release Ops",
    ]) {
      assert.equal(
        isDefaultPersonalAgentTitle(custom),
        false,
        `expected ${JSON.stringify(custom)} to preserve custom title`,
      );
    }
  });
});
