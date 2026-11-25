import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { after, before, describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { createServer, type ViteDevServer } from "vite";
import type { HostDetailView } from "../lib/orbit-ipc.ts";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const desktopSrcDir = path.resolve(__dirname, "..");

function makeHostDetail(overrides: Partial<HostDetailView> = {}): HostDetailView {
  return {
    revision: 1,
    host: {
      host_id: "local",
      display_name: "Local Mac",
      connection_state: "online_direct",
      daemon_version: "0.9.9",
      daemon_protocol_version: 1,
      client_protocol_version: 1,
      available_providers: ["claude_code", "codex"],
      is_local: true,
      agent_count: 1,
      running_count: 0,
      pending_approval_count: 0,
      endpoint_summary: "127.0.0.1:6868",
    } as unknown as HostDetailView["host"],
    discovered_projects: [{ path: "/repo", name: "repo" } as HostDetailView["discovered_projects"][number]],
    directory_listing: null,
    models_by_provider: {},
    workspaces: [
      {
        workspace_id: "ws-1",
        title: "Main",
        cwd: "/repo",
        updated_at: "2026-11-25T00:00:00Z",
        is_worktree: false,
      } as HostDetailView["workspaces"][number],
    ],
    agents: [
      {
        agent_id: "ag-1",
        workspace_id: "ws-1",
        title: "Session 1",
        cwd: "/repo",
        provider: "claude_code",
        provider_label: "Claude Code",
        status_label: "Idle",
        is_running: false,
        is_awaiting_permission: false,
        latest_seq: 10,
        updated_at_rfc3339: "2026-11-25T00:00:00Z",
      } as HostDetailView["agents"][number],
    ],
    last_created_agent_id: null,
    replaced_optimistic_agents: {},
    quotas: [],
    provider_catalogs: [{ provider: "claude_code", models: [] } as unknown as NonNullable<HostDetailView["provider_catalogs"]>[number]],
    terminals: [{ terminal_id: "t-1", cwd: "/repo", title: "zsh" } as HostDetailView["terminals"][number]],
    active_terminal_id: "t-1",
    active_terminal_slot: 0,
    profiles: [{ id: "p-1", name: "Default" } as unknown as NonNullable<HostDetailView["profiles"]>[number]],
    managed_skills: [{ slug: "skill-a" } as NonNullable<HostDetailView["managed_skills"]>[number]],
    acp_agents: [{ id: "acp-1" } as unknown as NonNullable<HostDetailView["acp_agents"]>[number]],
    artifacts: [{ artifact_id: "art-1", workspace_id: "ws-1" } as NonNullable<HostDetailView["artifacts"]>[number]],
    workspace_scripts: { "/repo": [{ name: "dev", command: "npm run dev" } as NonNullable<HostDetailView["workspace_scripts"]>[string][number]] },
    workspace_boards: {
      "ws-1": { workspace_id: "ws-1", revision: 1, content: "# Board 1" } as NonNullable<HostDetailView["workspace_boards"]>[string],
      "ws-2": { workspace_id: "ws-2", revision: 2, content: "# Board 2" } as NonNullable<HostDetailView["workspace_boards"]>[string],
    },
    git_repos_by_cwd: {
      "/repo": { is_git_repo: true, nested_git_repos: [] } as unknown as NonNullable<HostDetailView["git_repos_by_cwd"]>[string],
    },
    ...overrides,
  };
}

let server: ViteDevServer;
before(async () => {
  server = await createServer({
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
  });
});
after(async () => {
  await server?.close();
});

describe("stabilizeHostDetail structural sharing", () => {
  it("returns prev directly when deserialized heartbeat is structurally identical", async () => {
    const { stabilizeHostDetail } = await server.ssrLoadModule("/src/state/slices/fleet-slice.ts");
    const prev = makeHostDetail();
    const cloned: HostDetailView = JSON.parse(JSON.stringify(prev));
    const stabilized = stabilizeHostDetail(prev, cloned);
    assert.strictEqual(stabilized, prev);
  });

  it("reuses unchanged sub-field references across heartbeats when agents/revision change", async () => {
    const { stabilizeHostDetail } = await server.ssrLoadModule("/src/state/slices/fleet-slice.ts");
    const prev = makeHostDetail();
    const next: HostDetailView = JSON.parse(JSON.stringify(prev));
    next.revision = 2;
    next.agents[0].latest_seq = 11;
    next.host.last_ping_ms = 12;
    next.workspace_boards!["ws-1"].content = "# Board 1 updated";

    const stabilized = stabilizeHostDetail(prev, next);
    assert.notStrictEqual(stabilized, prev);
    assert.notStrictEqual(stabilized.agents, prev.agents);
    assert.strictEqual(stabilized.agents[0].latest_seq, 11);

    // Unchanged sub-fields preserve referential equality
    assert.strictEqual(stabilized.host.available_providers, prev.host.available_providers);
    assert.strictEqual(stabilized.provider_catalogs, prev.provider_catalogs);
    assert.strictEqual(stabilized.managed_skills, prev.managed_skills);
    assert.strictEqual(stabilized.profiles, prev.profiles);
    assert.strictEqual(stabilized.acp_agents, prev.acp_agents);
    assert.strictEqual(stabilized.artifacts, prev.artifacts);
    assert.strictEqual(stabilized.terminals, prev.terminals);
    assert.strictEqual(stabilized.workspace_scripts, prev.workspace_scripts);
    assert.strictEqual(stabilized.git_repos_by_cwd, prev.git_repos_by_cwd);

    // Record-level structural sharing keeps untouched keys referentially identical
    assert.notStrictEqual(stabilized.workspace_boards, prev.workspace_boards);
    assert.notStrictEqual(stabilized.workspace_boards!["ws-1"], prev.workspace_boards!["ws-1"]);
    assert.strictEqual(stabilized.workspace_boards!["ws-2"], prev.workspace_boards!["ws-2"]);
  });
});

describe("Non-settings bug fix & DOM cleanliness guardrails", () => {
  it("guards measureTurn against stale/shrunk rows and wires observeOffsetWithBottomSeed in SidePanelTurnList", () => {
    const src = readFileSync(path.join(desktopSrcDir, "components/stage/side-turn-list.tsx"), "utf8");
    assert.match(src, /const row = rows\[instance\.indexFromElement\(el\)\];\s*if \(!row\) return 0;/);
    assert.match(src, /observeElementOffset:\s*observeOffsetWithBottomSeed/);
  });

  it("uses a native non-passive wheel listener in ImageLightbox", () => {
    const src = readFileSync(path.join(desktopSrcDir, "components/viewers/image-lightbox.tsx"), "utf8");
    assert.match(src, /addEventListener\("wheel",\s*handleWheel,\s*\{\s*passive:\s*false\s*\}\)/);
    assert.doesNotMatch(src, /onWheel=/);
  });

  it("renders common.connecting instead of common.sync for connecting single host in FleetSidebar", () => {
    const src = readFileSync(path.join(desktopSrcDir, "components/sidebar/fleet-sidebar.tsx"), "utf8");
    assert.match(src, /fleetHealth\.connecting > 0\s*\?\s*t\("common\.connecting"\)/);
  });

  it("reuses precomputed subagentsOverride in selectPendingDecisions without recomputing", async () => {
    const { selectPendingDecisions, selectSessionSubagents } = await server.ssrLoadModule(
      "/src/state/selectors/session-selectors.ts",
    );
    const detail = makeHostDetail({
      agents: [
        {
          agent_id: "parent-1",
          workspace_id: "ws-1",
          title: "Parent",
          cwd: "/repo",
          provider: "claude_code",
          provider_label: "Claude Code",
          status_label: "Idle",
          is_running: false,
          is_awaiting_permission: false,
          latest_seq: 1,
          updated_at_rfc3339: "2026-11-25T00:00:00Z",
        } as HostDetailView["agents"][number],
        {
          agent_id: "child-1",
          parent_agent_id: "parent-1",
          workspace_id: "ws-1",
          title: "Child",
          cwd: "/repo",
          provider: "claude_code",
          provider_label: "Claude Code",
          status_label: "Waiting",
          is_running: false,
          is_awaiting_permission: true,
          latest_seq: 2,
          updated_at_rfc3339: "2026-11-25T00:00:01Z",
        } as HostDetailView["agents"][number],
      ],
      pending_permissions: [
        {
          agent_id: "child-1",
          agent_title: "Child",
          request_id: "req-1",
          title: "Run tests",
          detail: { kind: "shell", command: "npm test", cwd: "/repo", exit_code: null, output_preview: null },
          allowed_decisions: ["allow", "deny"],
          created_at: "2026-11-25T00:00:01Z",
        },
      ],
    });
    const state = {
      stages: {},
      hostDetails: { local: detail },
      fleet: { revision: 1, total_pending_approvals: 1, hosts: [detail.host], projects: [] },
      archivedSubagents: {},
    };
    const summary = selectSessionSubagents(state, "local", "parent-1");
    const decisions = selectPendingDecisions(state, "local", "parent-1", summary);
    assert.equal(decisions.totalCount, 1);
    assert.equal(decisions.subagentPermissions.length, 1);
    assert.equal(decisions.subagentPermissions[0].permission.request_id, "req-1");
  });
});
