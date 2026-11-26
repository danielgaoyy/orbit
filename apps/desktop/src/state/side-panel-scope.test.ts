import assert from "node:assert/strict";
import { after, before, it } from "node:test";
import { fileURLToPath } from "node:url";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import {
  SIDE_PANEL_MAX_CLOSED_TABS,
  SIDE_PANEL_MAX_SCOPES,
  SIDE_PANEL_STORAGE_KEY,
  applyOpenSidePanelTab,
  applyReopenClosedSidePanelTab,
  loadPersistedSidePanelSnapshots,
  normalizeSidePanelSplit,
  promoteDraftSidePanelScope,
  remapOptimisticSidePanelScopes,
  rebindDraftSidePanelCwd,
  removeSubagentTabsFromScopes,
  resolveSidePanelScopeKey,
  savePersistedSidePanelSnapshots,
  serializeSidePanelPersistence,
  sidePanelTabScopeKey,
  switchSidePanelScope,
} from "./side-panel-scope.ts";
import type { OrbitUiState, SidePanelTab, SideQuestionEntry } from "./store-types.ts";
import {
  clampEffectiveSidePanelWidth,
  resolveWorkspacePaneWidths,
} from "../lib/workspace-layout.ts";

let server: ViteDevServer;
let createDefaultInitialStateFn: typeof import("./store-helpers.ts").createDefaultInitialState;

before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL("../..", import.meta.url)),
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [
      {
        name: "orbit-side-panel-test-ssr-get-server-snapshot",
        transform(code, id) {
          if (id.endsWith("/src/state/orbit-store.ts")) {
            return code.replace(
              "return useSyncExternalStore(subscribe, getSnapshot);",
              "return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);",
            );
          }
          return null;
        },
      },
    ],
  });
  const helpers = await server.ssrLoadModule("/src/state/store-helpers.ts");
  createDefaultInitialStateFn = helpers.createDefaultInitialState;
});

after(async () => {
  await server?.close();
});

function makeHostDetail(hostId: string, agents: Array<Record<string, unknown>>) {
  return {
    revision: 1,
    host: {
      host_id: hostId,
      display_name: hostId,
      connection_state: "online_direct",
      available_providers: ["claude"],
      supports_server_queue: false,
    },
    agents: agents.map((a) => ({
      agent_id: a.agent_id,
      title: a.title ?? a.agent_id,
      provider: a.provider ?? "claude",
      provider_label: a.provider_label ?? "Claude",
      mode: a.mode ?? "default",
      cwd: a.cwd ?? "/repo",
      latest_seq: a.latest_seq ?? 1,
      workspace_id: a.workspace_id ?? "ws-1",
      parent_agent_id: a.parent_agent_id ?? null,
      parent_host_id: a.parent_host_id ?? null,
      archived_at_rfc3339: a.archived_at_rfc3339 ?? null,
      background: false,
      is_unread: false,
      is_awaiting_permission: false,
      status: "idle",
      ...a,
    })),
    workspaces: [
      { workspace_id: "ws-1", title: "Repo", cwd: "/repo", mission_goal: "", archived_at: null },
      { workspace_id: "ws-2", title: "Other", cwd: "/other", mission_goal: "", archived_at: null },
    ],
    discovered_projects: [],
    terminals: [],
    artifacts: [],
    pending_permissions: [],
    provider_subagents: {},
    workspace_boards: {},
    workspace_events: {},
    acp_agents: [],
  };
}

function makeBaseState(overrides: Partial<OrbitUiState> = {}): OrbitUiState {
  const base = createDefaultInitialStateFn(
    {},
    "system",
    "light",
    {
      composerDrafts: {},
      slashDirectives: {},
      inlineMentions: {},
      contextItems: {},
      pastedAttachments: {},
      diffQuotes: {},
      sideQuestionDrafts: {},
    },
  );
  return {
    ...base,
    selectedHostId: "host-a",
    selectedAgentId: "agent-a",
    selectedWorkspaceCwd: "/repo-a",
    isDraftNewSession: false,
    sidePanelScopeKey: "host-a::agent-a",
    hostDetails: {
      "host-a": makeHostDetail("host-a", [
        { agent_id: "agent-a", cwd: "/repo-a", workspace_id: "ws-a" },
        { agent_id: "agent-b", cwd: "/repo-b", workspace_id: "ws-b" },
        {
          agent_id: "child-a1",
          cwd: "/repo-a",
          parent_agent_id: "agent-a",
          parent_host_id: "host-a",
        },
      ]) as never,
      "host-b": makeHostDetail("host-b", [
        { agent_id: "agent-c", cwd: "/repo-c", workspace_id: "ws-c" },
      ]) as never,
    },
    ...overrides,
  };
}

it("isolates per-session side panel state across A -> B -> A with split & active tab preserved", () => {
  let state = makeBaseState();

  // Open Review and a File tab in session A, then split them
  state = applyOpenSidePanelTab(state, {
    id: "tab:review",
    kind: "review",
    title: "Review",
    hostId: "host-a",
    agentId: "agent-a",
    cwd: "/repo-a",
  });
  state = applyOpenSidePanelTab(state, {
    id: "tab:file:host-a::/repo-a::src/a.ts",
    kind: "file",
    title: "a.ts",
    filePath: "src/a.ts",
    hostId: "host-a",
    agentId: "agent-a",
    cwd: "/repo-a",
  });
  state = {
    ...state,
    sidePanelSplit: { secondaryTabId: "tab:review", ratio: 0.4 },
  };
  assert.equal(state.sidePanelOpen, true);
  assert.equal(state.sidePanelTabs.length, 2);
  assert.equal(state.activeSidePanelTabId, "tab:file:host-a::/repo-a::src/a.ts");
  assert.deepEqual(state.sidePanelSplit, { secondaryTabId: "tab:review", ratio: 0.4 });

  // Switch to session B (fresh session) -> starts closed with empty tabs
  const nonceBeforeB = state.sidePanelScopeSwitchNonce;
  state = switchSidePanelScope(state, "host-a::agent-b");
  assert.equal(state.sidePanelScopeKey, "host-a::agent-b");
  assert.equal(state.sidePanelOpen, false);
  assert.equal(state.sidePanelTabs.length, 0);
  assert.equal(state.activeSidePanelTabId, null);
  assert.equal(state.sidePanelSplit, null);
  assert.equal(state.sidePanelScopeSwitchNonce, nonceBeforeB + 1);

  // Open an independent Files tab in session B
  state = applyOpenSidePanelTab(state, {
    id: "tab:files",
    kind: "files",
    title: "Files",
    hostId: "host-a",
    agentId: "agent-b",
    cwd: "/repo-b",
  });
  assert.equal(state.sidePanelOpen, true);
  assert.deepEqual(state.sidePanelTabs.map((t) => t.id), ["tab:files"]);

  // Switch back to A -> exact tabs, active tab, and split are restored
  state = switchSidePanelScope(state, "host-a::agent-a");
  assert.equal(state.sidePanelOpen, true);
  assert.deepEqual(state.sidePanelTabs.map((t) => t.id), [
    "tab:review",
    "tab:file:host-a::/repo-a::src/a.ts",
  ]);
  assert.equal(state.activeSidePanelTabId, "tab:file:host-a::/repo-a::src/a.ts");
  assert.deepEqual(state.sidePanelSplit, { secondaryTabId: "tab:review", ratio: 0.4 });

  // Switch back to B -> B's own panel is still intact
  state = switchSidePanelScope(state, "host-a::agent-b");
  assert.equal(state.sidePanelOpen, true);
  assert.deepEqual(state.sidePanelTabs.map((t) => t.id), ["tab:files"]);
});

it("resolves child subagents to their root parent scope and validates tabs on scope load", () => {
  const state = makeBaseState();
  const childScope = resolveSidePanelScopeKey({
    selectedHostId: "host-a",
    selectedAgentId: "child-a1",
    isDraftNewSession: false,
    hostDetails: state.hostDetails,
  });
  assert.equal(childScope, "host-a::agent-a");

  // Inject an invalid foreign subagent tab and foreign-host peer tab into A's cached snapshot
  const poisoned = makeBaseState({
    sidePanelScopeKey: "host-a::agent-b",
    selectedAgentId: "agent-b",
    sidePanelByScope: {
      "host-a::agent-a": {
        open: true,
        tabs: [
          {
            id: "tab:subagent:foreign",
            kind: "subagent",
            title: "Foreign Child",
            hostId: "host-a",
            parentHostId: "host-a",
            parentAgentId: "agent-b",
            childAgentId: "child-b1",
          },
          {
            id: "tab:peer:host-b::agent-c",
            kind: "peer",
            title: "Other Host Peer",
            hostId: "host-b",
            peerAgentId: "agent-c",
          },
          {
            id: "tab:review",
            kind: "review",
            title: "Review",
            hostId: "host-a",
            agentId: "agent-a",
            cwd: "/repo-a",
          },
        ],
        activeTabId: "tab:subagent:foreign",
        split: { secondaryTabId: "tab:review", ratio: 0.5 },
        userDismissed: false,
        closedTabs: [],
        unseenTabIds: [],
        lastUsedAt: 1000,
      },
    },
  });

  const loaded = switchSidePanelScope(poisoned, "host-a::agent-a");
  assert.deepEqual(loaded.sidePanelTabs.map((t) => t.id), ["tab:review"]);
  assert.equal(loaded.activeSidePanelTabId, "tab:review");
  assert.equal(loaded.sidePanelSplit, null);
  assert.equal(loaded.sidePanelOpen, true);
});

it("promotes draft side panel to the created session and remaps optimistic agent IDs", () => {
  let state = makeBaseState({
    selectedAgentId: null,
    isDraftNewSession: true,
    sidePanelScopeKey: "draft::host-a",
    sideQuestionsByScope: {
      "draft::host-a": [
        {
          id: "sq-1",
          question: "Draft Q",
          answer: "Draft A",
          blocks: [],
          status: "done",
          error: null,
          createdAtMs: 1,
          providerLabel: "Claude",
        },
      ],
    },
  });

  state = applyOpenSidePanelTab(state, {
    id: "tab:side-question",
    kind: "side_question",
    title: "Side Question",
    hostId: "host-a",
    agentId: null,
    cwd: "~",
  });
  assert.equal(state.sidePanelOpen, true);

  // Promote draft -> optimistic agent opt_1
  state = promoteDraftSidePanelScope(state, "host-a", "opt_1", "/repo-new");
  assert.equal(state.sidePanelScopeKey, "host-a::opt_1");
  assert.equal(state.sidePanelOpen, true);
  assert.equal(state.sidePanelTabs[0].agentId, "opt_1");
  assert.equal(state.sidePanelTabs[0].cwd, "/repo-new");
  assert.equal(state.sidePanelByScope["draft::host-a"], undefined);
  assert.equal(state.sideQuestionsByScope["host-a::opt_1"]?.length, 1);

  // Remap optimistic opt_1 -> confirmed ag_real
  state = remapOptimisticSidePanelScopes(state, "host-a", { opt_1: "ag_real" });
  assert.equal(state.sidePanelScopeKey, "host-a::ag_real");
  assert.equal(state.sidePanelTabs[0].agentId, "ag_real");

  // Next new draft starts cleanly closed
  const nextDraft = switchSidePanelScope(state, "draft::host-a");
  assert.equal(nextDraft.sidePanelOpen, false);
  assert.equal(nextDraft.sidePanelTabs.length, 0);
});

it("enforces auto-open policy, userDismissed per-scope isolation, and unseen activity tracking", () => {
  let state = makeBaseState({ sidePanelOpen: false, sidePanelUserDismissed: false });

  // 1. Auto-open when closed & not dismissed -> opens and activates
  state = applyOpenSidePanelTab(
    state,
    { id: "tab:side-question", kind: "side_question", title: "Side Question", hostId: "host-a", agentId: "agent-a" },
    { source: "auto" },
  );
  assert.equal(state.sidePanelOpen, true);
  assert.equal(state.activeSidePanelTabId, "tab:side-question");
  assert.equal(state.sidePanelUnseenTabIds.length, 0);

  // 2. Auto-open while panel already open on another tab -> adds in background without stealing focus
  state = applyOpenSidePanelTab(
    state,
    { id: "tab:file:host-a::/repo-a::foo.ts", kind: "file", title: "foo.ts", filePath: "foo.ts", hostId: "host-a", agentId: "agent-a", cwd: "/repo-a" },
    { source: "auto", activate: true },
  );
  assert.equal(state.activeSidePanelTabId, "tab:side-question");
  assert.equal(state.sidePanelTabs.length, 2);

  // 3. Auto-open while panel is open on Launchpad (activeTabId === null) & activate=true -> activates
  state = { ...state, activeSidePanelTabId: null };
  state = applyOpenSidePanelTab(
    state,
    { id: "tab:terminal", kind: "terminal", title: "Terminal", hostId: "host-a", agentId: "agent-a", cwd: "/repo-a" },
    { source: "auto", activate: true, terminalFocused: true },
  );
  assert.equal(state.activeSidePanelTabId, "tab:terminal");
  assert.equal(state.terminalFocused, true);

  // 4. Explicit close sets userDismissed=true -> auto-open stays closed & records unseen activity
  state = { ...state, sidePanelOpen: false, sidePanelUserDismissed: true, terminalFocused: false };
  state = applyOpenSidePanelTab(
    state,
    { id: "tab:review", kind: "review", title: "Review", hostId: "host-a", agentId: "agent-a", cwd: "/repo-a" },
    { source: "auto" },
  );
  assert.equal(state.sidePanelOpen, false);
  assert.equal(state.sidePanelUserDismissed, true);
  assert.ok(state.sidePanelTabs.some((t) => t.id === "tab:review"));
  assert.deepEqual(state.sidePanelUnseenTabIds, ["tab:review"]);

  // 5. userDismissed is per-session: session B is NOT dismissed
  const stateB = switchSidePanelScope(state, "host-a::agent-b");
  assert.equal(stateB.sidePanelUserDismissed, false);
  assert.equal(stateB.sidePanelUnseenTabIds.length, 0);

  // 6. User-triggered open clears userDismissed and unseenTabIds
  const reopenedA = applyOpenSidePanelTab(
    state,
    { id: "tab:review", kind: "review", title: "Review", hostId: "host-a", agentId: "agent-a", cwd: "/repo-a" },
    { source: "user" },
  );
  assert.equal(reopenedA.sidePanelOpen, true);
  assert.equal(reopenedA.sidePanelUserDismissed, false);
  assert.equal(reopenedA.sidePanelUnseenTabIds.length, 0);
});

it("reopens closed tabs in LIFO order up to cap and normalizes invalid splits", () => {
  const tab1: SidePanelTab = { id: "tab:review", kind: "review", title: "Review", hostId: "host-a", agentId: "agent-a", cwd: "/repo-a" };
  const tab2: SidePanelTab = { id: "tab:files", kind: "files", title: "Files", hostId: "host-a", agentId: "agent-a", cwd: "/repo-a" };
  const closedStack: SidePanelTab[] = [tab2, tab1];

  let state = makeBaseState({
    sidePanelOpen: false,
    sidePanelTabs: [],
    activeSidePanelTabId: null,
    sidePanelClosedTabs: closedStack,
    sidePanelUserDismissed: true,
  });

  const first = applyReopenClosedSidePanelTab(state);
  assert.equal(first.reopenedTab?.id, "tab:files");
  assert.equal(first.state.sidePanelOpen, true);
  assert.equal(first.state.sidePanelUserDismissed, false);
  assert.equal(first.state.activeSidePanelTabId, "tab:files");
  assert.deepEqual(first.state.sidePanelClosedTabs.map((t) => t.id), ["tab:review"]);

  const second = applyReopenClosedSidePanelTab(first.state);
  assert.equal(second.reopenedTab?.id, "tab:review");
  assert.equal(second.state.activeSidePanelTabId, "tab:review");
  assert.equal(second.state.sidePanelClosedTabs.length, 0);

  // Split normalization checks
  assert.equal(normalizeSidePanelSplit([tab1], "tab:review", { secondaryTabId: "tab:files", ratio: 0.5 }), null);
  assert.equal(normalizeSidePanelSplit([tab1, tab2], "tab:review", { secondaryTabId: "tab:review", ratio: 0.5 }), null);
  assert.deepEqual(
    normalizeSidePanelSplit([tab1, tab2], "tab:review", { secondaryTabId: "tab:files", ratio: 0.95 }),
    { secondaryTabId: "tab:files", ratio: 0.75 },
  );
  assert.equal(SIDE_PANEL_MAX_CLOSED_TABS, 10);
});

it("clamps effective side panel width without mutating stored preference", () => {
  // Stored 700px on 1000px viewport with 260px left sidebar -> avail 740px -> max(320, min(700, 380)) = 380px (center = 360px)
  assert.equal(clampEffectiveSidePanelWidth(700, 1000, 260), 380);
  // Very narrow viewport -> floor at 320px
  assert.equal(clampEffectiveSidePanelWidth(700, 620, 0), 320);

  const panes = resolveWorkspacePaneWidths({
    viewportWidth: 1000,
    sidebarOpen: true,
    sidebarPrefWidth: 260,
    sidePanelOpen: true,
    sidePanelPrefWidth: 700,
  });
  assert.equal(panes.leftWidthPx, 260);
  assert.equal(panes.rightWidthPx, 380);
  assert.equal(panes.centerWidthPx, 360);
});

it("persists to orbit.desktop.sidePanel.v1, strips transient fields, enforces LRU cap, and survives corrupt JSON", () => {
  const storageMap = new Map<string, string>();
  const mockStorage: Pick<Storage, "getItem" | "setItem"> = {
    getItem: (k) => storageMap.get(k) ?? null,
    setItem: (k, v) => {
      storageMap.set(k, v);
    },
  };

  let state = makeBaseState();
  state = applyOpenSidePanelTab(state, {
    id: "tab:file:host-a::/repo-a::src/index.ts",
    kind: "file",
    title: "index.ts",
    filePath: "src/index.ts",
    hostId: "host-a",
    agentId: "agent-a",
    cwd: "/repo-a",
    focusLine: 42,
    focusEndLine: 50,
    focusNonce: 9,
  });

  savePersistedSidePanelSnapshots(state, mockStorage, 5000);
  const raw = storageMap.get(SIDE_PANEL_STORAGE_KEY);
  assert.ok(raw, "expected serialized payload in localStorage");
  const parsed = JSON.parse(raw);
  assert.equal(parsed.version, 1);
  assert.equal(parsed.byScope["host-a::agent-a"].tabs[0].focusNonce, undefined);
  assert.equal(parsed.byScope["host-a::agent-a"].tabs[0].focusLine, undefined);
  assert.equal(parsed.byScope["host-a::agent-a"].tabs[0].focusEndLine, undefined);

  const hydrated = loadPersistedSidePanelSnapshots(mockStorage);
  assert.equal(hydrated["host-a::agent-a"]?.open, true);
  assert.equal(hydrated["host-a::agent-a"]?.tabs[0]?.filePath, "src/index.ts");

  // LRU cap of 100
  const manyScopes: Record<string, (typeof hydrated)[string]> = {};
  for (let i = 0; i < SIDE_PANEL_MAX_SCOPES + 15; i += 1) {
    manyScopes[`host-a::ag-${i}`] = {
      open: true,
      tabs: [{ id: "tab:review", kind: "review", title: "Review", hostId: "host-a", agentId: `ag-${i}`, cwd: "/repo" }],
      activeTabId: "tab:review",
      split: null,
      userDismissed: false,
      closedTabs: [],
      unseenTabIds: [],
      lastUsedAt: i + 1,
    };
  }
  const serializedMany = serializeSidePanelPersistence(
    makeBaseState({ sidePanelScopeKey: null, sidePanelByScope: manyScopes }),
    99999,
  );
  assert.equal(Object.keys(serializedMany.byScope).length, SIDE_PANEL_MAX_SCOPES);
  assert.equal(serializedMany.byScope["host-a::ag-0"], undefined);
  assert.ok(serializedMany.byScope[`host-a::ag-${SIDE_PANEL_MAX_SCOPES + 14}`]);

  // Corrupt JSON & wrong version safely return {}
  mockStorage.setItem(SIDE_PANEL_STORAGE_KEY, "{not valid json");
  assert.deepEqual(loadPersistedSidePanelSnapshots(mockStorage), {});
  mockStorage.setItem(SIDE_PANEL_STORAGE_KEY, JSON.stringify({ version: 99, byScope: {} }));
  assert.deepEqual(loadPersistedSidePanelSnapshots(mockStorage), {});
});

it("integrates live store actions: selectAgent, startNewThreadDraft, selectHost, archiveThread, deleteThreadPermanently, archiveSubagent, compact collapse, and dirty draft preservation", async (t) => {
  const { orbitActions, getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

  t.mock.method(orbitIpc, "openAgentTimeline", async () => {});
  t.mock.method(orbitIpc, "markAgentRead", async () => {});
  t.mock.method(orbitIpc, "markAgentSeen", async () => {});
  t.mock.method(orbitIpc, "listProviderSubagents", async () => {});
  t.mock.method(orbitIpc, "archiveAgent", async () => {});
  t.mock.method(orbitIpc, "deleteAgent", async () => {});
  t.mock.method(orbitIpc, "refreshQuotas", async () => {});
  t.mock.method(orbitActions, "refreshSidebarView", () => {});
  t.mock.method(orbitActions, "pullStageDelta", async () => {});
  t.mock.method(orbitActions, "refreshGit", async () => {});
  t.mock.method(orbitActions, "inspectGitRepos", async () => null);
  t.mock.method(orbitActions, "ensureWorkspaceFiles", async () => {});
  t.mock.method(orbitActions, "loadWorkspaceFile", async () => {});

  const detailA = makeHostDetail("host-a", [
    { agent_id: "ag-1", cwd: "/repo-1", workspace_id: "ws-1" },
    { agent_id: "ag-2", cwd: "/repo-2", workspace_id: "ws-2" },
    { agent_id: "child-1", cwd: "/repo-1", parent_agent_id: "ag-1", parent_host_id: "host-a" },
  ]);
  const detailB = makeHostDetail("host-b", [
    { agent_id: "ag-b1", cwd: "/repo-b1", workspace_id: "ws-b1" },
  ]);

  Object.assign(getOrbitState(), {
    fleet: {
      revision: 1,
      total_pending_approvals: 0,
      hosts: [detailA.host, detailB.host],
      projects: [],
    },
    hostDetails: { "host-a": detailA, "host-b": detailB },
    selectedHostId: "host-a",
    selectedAgentId: "ag-1",
    selectedWorkspaceCwd: "/repo-1",
    isDraftNewSession: false,
    isCompactViewport: false,
    isTightForDualSidebars: true,
    sidebarOpen: true,
    sidebarDesktopOpenPref: true,
    sidebarAutoYieldedForSidePanel: false,
    sidePanelOpen: false,
    sidePanelTabs: [],
    activeSidePanelTabId: null,
    sidePanelSplit: null,
    sidePanelByScope: {},
    sidePanelScopeKey: "host-a::ag-1",
    sidePanelUserDismissed: false,
    sidePanelClosedTabs: [],
    sidePanelUnseenTabIds: [],
    sidePanelCollapsedByResize: false,
    archivedThreads: {},
    archivedSubagents: {},
    fileEditorDrafts: {},
    pendingUnsavedFileAction: null,
  });

  // Open review + dirty file tab + subagent tab on ag-1; sidebar auto-yields because tight
  orbitActions.openReviewTab();
  orbitActions.openFileInSidePanel("src/dirty.ts");
  const dirtyKey = "host-a::/repo-1::src/dirty.ts";
  Object.assign(getOrbitState().fileEditorDrafts, {
    [dirtyKey]: {
      hostId: "host-a",
      cwd: "/repo-1",
      path: "src/dirty.ts",
      mode: "edit",
      content: "edited",
      baseContent: "orig",
      dirty: true,
      saving: false,
      saveError: null,
      conflict: false,
      lineEnding: "lf",
    },
  });
  orbitActions.openSubagentTab({
    callId: "child-1",
    role: "Worker",
    promptPreview: "Work",
    childHostId: "host-a",
    childAgentId: "child-1",
    parentHostId: "host-a",
    parentAgentId: "ag-1",
  });
  assert.equal(getOrbitState().sidePanelOpen, true);
  assert.equal(getOrbitState().sidebarAutoYieldedForSidePanel, true);
  assert.equal(getOrbitState().sidebarOpen, false);

  // Attempting to close dirty file tab triggers pendingUnsavedFileAction and does NOT push to closedTabs
  orbitActions.closeSidePanelTab("tab:file:host-a::/repo-1::src/dirty.ts");
  assert.ok(getOrbitState().pendingUnsavedFileAction);
  assert.equal(getOrbitState().sidePanelClosedTabs.length, 0);

  // Switching to ag-2 hides tabs, restores auto-yielded left sidebar, clears pendingUnsavedFileAction, preserves dirty draft!
  orbitActions.selectAgent("host-a", "ag-2");
  assert.equal(getOrbitState().sidePanelScopeKey, "host-a::ag-2");
  assert.equal(getOrbitState().sidePanelOpen, false);
  assert.equal(getOrbitState().sidePanelTabs.length, 0);
  assert.equal(getOrbitState().sidebarOpen, true);
  assert.equal(getOrbitState().sidebarAutoYieldedForSidePanel, false);
  assert.equal(getOrbitState().pendingUnsavedFileAction, null);
  assert.equal(getOrbitState().fileEditorDrafts[dirtyKey]?.dirty, true);

  // Archiving subagent on cached parent ag-1 removes its tab from ag-1's snapshot
  orbitActions.archiveSubagent("host-a", "ag-1", { id: "child-1", childAgentId: "child-1" });
  const cachedAg1 = getOrbitState().sidePanelByScope["host-a::ag-1"];
  assert.ok(cachedAg1);
  assert.ok(!cachedAg1.tabs.some((tab: SidePanelTab) => tab.kind === "subagent"));

  // Switch back to ag-1 -> panel reopens & sidebar re-yields
  orbitActions.selectAgent("host-a", "ag-1");
  assert.equal(getOrbitState().sidePanelOpen, true);
  assert.equal(getOrbitState().sidebarOpen, false);
  assert.equal(getOrbitState().sidebarAutoYieldedForSidePanel, true);

  // Compact resize collapses with collapsedByResize (NOT userDismissed) and restores on widen
  orbitActions.updateViewportWidth(640);
  assert.equal(getOrbitState().isCompactViewport, true);
  assert.equal(getOrbitState().sidePanelOpen, false);
  assert.equal(getOrbitState().sidePanelCollapsedByResize, true);
  assert.equal(getOrbitState().sidePanelUserDismissed, false);

  orbitActions.updateViewportWidth(1440);
  assert.equal(getOrbitState().isCompactViewport, false);
  assert.equal(getOrbitState().sidePanelOpen, true);
  assert.equal(getOrbitState().sidePanelCollapsedByResize, false);

  // Close review tab -> reopen with reopenClosedSidePanelTab
  orbitActions.closeSidePanelTab("tab:review");
  assert.ok(!getOrbitState().sidePanelTabs.some((tab: SidePanelTab) => tab.id === "tab:review"));
  orbitActions.reopenClosedSidePanelTab();
  assert.equal(getOrbitState().activeSidePanelTabId, "tab:review");

  // startNewThreadDraft -> starts closed cleanly
  orbitActions.startNewThreadDraft("host-a", "/repo-1");
  assert.equal(getOrbitState().sidePanelScopeKey, "draft::host-a");
  assert.equal(getOrbitState().sidePanelOpen, false);
  assert.equal(getOrbitState().sidePanelTabs.length, 0);

  // selectHost switches scope cleanly
  orbitActions.selectHost("host-b");
  assert.equal(getOrbitState().sidePanelScopeKey, "host-b::ag-b1");
  assert.equal(getOrbitState().sidePanelOpen, false);

  // archiveThread drops archived session snapshot (with fallback to ag-2)
  orbitActions.selectAgent("host-a", "ag-1");
  assert.equal(getOrbitState().sidePanelOpen, true);
  orbitActions.archiveThread("host-a", "ag-1");
  assert.equal(getOrbitState().sidePanelByScope["host-a::ag-1"], undefined);
  assert.equal(getOrbitState().selectedAgentId, "ag-2");

  // deleteThreadPermanently on ag-2 drops snapshot and falls back to closed draft
  orbitActions.openReviewTab();
  assert.equal(getOrbitState().sidePanelOpen, true);
  await orbitActions.deleteThreadPermanently("host-a", "ag-2");
  assert.equal(getOrbitState().sidePanelByScope["host-a::ag-2"], undefined);
  assert.equal(getOrbitState().isDraftNewSession, true);
  assert.equal(getOrbitState().sidePanelOpen, false);
  assert.equal(getOrbitState().sidePanelTabs.length, 0);
});

it("renders ArtifactsTab and SideQuestionTab from tab scope rather than current selection", async () => {
  const { ArtifactsTab } = await server.ssrLoadModule("/src/components/artifacts/artifacts-tab.tsx");
  const { SideQuestionTab } = await server.ssrLoadModule(
    "/src/components/side-panel/side-question-tab.tsx",
  );
  const { getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");

  const entryA: SideQuestionEntry = {
    id: "sq-a",
    question: "Question belonging strictly to host-a::agent-a",
    answer: "Answer for agent-a",
    blocks: [],
    status: "done",
    error: null,
    createdAtMs: 1,
    providerLabel: "Claude",
  };
  const entryB: SideQuestionEntry = {
    id: "sq-b",
    question: "Wrong session host-b::agent-b question",
    answer: "Wrong answer",
    blocks: [],
    status: "done",
    error: null,
    createdAtMs: 2,
    providerLabel: "Codex",
  };

  const detailA = makeHostDetail("host-a", [
    { agent_id: "agent-a", cwd: "/repo-a", workspace_id: "ws-a", provider_label: "Claude A" },
  ]);
  detailA.artifacts = [
    {
      artifact_id: "art-a",
      workspace_id: "ws-a",
      title: "Artifact From Host A",
      kind: "markdown",
      version: 1,
      updated_at: "2026-01-01T00:00:00Z",
    },
  ] as never;

  const detailB = makeHostDetail("host-b", [
    { agent_id: "agent-b", cwd: "/repo-b", workspace_id: "ws-b", provider_label: "Codex B" },
  ]);
  detailB.artifacts = [
    {
      artifact_id: "art-b",
      workspace_id: "ws-b",
      title: "Wrong Artifact From Host B",
      kind: "markdown",
      version: 1,
      updated_at: "2026-01-01T00:00:00Z",
    },
  ] as never;

  // Intentionally point current selection at host-b::agent-b while rendering tabs bound to host-a::agent-a!
  Object.assign(getOrbitState(), {
    selectedHostId: "host-b",
    selectedAgentId: "agent-b",
    selectedWorkspaceCwd: "/repo-b",
    isDraftNewSession: false,
    hostDetails: { "host-a": detailA, "host-b": detailB },
    sideQuestionsByScope: {
      "host-a::agent-a": [entryA],
      "host-b::agent-b": [entryB],
    },
    sideQuestionDrafts: {
      "host-a::agent-a": "draft-for-a",
      "host-b::agent-b": "draft-for-b",
    },
  });

  const artifactsTab: SidePanelTab = {
    id: "tab:artifacts",
    kind: "artifacts",
    title: "Artifacts",
    hostId: "host-a",
    agentId: "agent-a",
    workspaceId: "ws-a",
    cwd: "/repo-a",
    artifactId: "art-a",
  };
  const artifactsHtml = renderToStaticMarkup(createElement(ArtifactsTab, { tab: artifactsTab }));
  assert.match(artifactsHtml, /Artifact From Host A/);
  assert.doesNotMatch(artifactsHtml, /Wrong Artifact From Host B/);

  const sideQuestionTab: SidePanelTab = {
    id: "tab:side-question",
    kind: "side_question",
    title: "Side Question",
    hostId: "host-a",
    agentId: "agent-a",
    cwd: "/repo-a",
  };
  const sqHtml = renderToStaticMarkup(createElement(SideQuestionTab, { tab: sideQuestionTab }));
  assert.match(sqHtml, /Question belonging strictly to host-a::agent-a/);
  assert.doesNotMatch(sqHtml, /Wrong session host-b::agent-b question/);
  assert.match(sqHtml, /Claude A/);
});

it("keeps sidePanelTabScopeKey consistent for agent and draft tabs", () => {
  assert.equal(
    sidePanelTabScopeKey({ hostId: "h1", agentId: "ag1" }),
    "h1::ag1",
  );
  assert.equal(
    sidePanelTabScopeKey({ hostId: "h1", agentId: null }),
    "draft::h1",
  );
  assert.equal(
    sidePanelTabScopeKey({ hostId: "remote", parentHostId: "local", parentAgentId: "root" }),
    "local::root",
  );
});

it("prunes archived subagent tabs cleanly across live and cached parent snapshots", () => {
  const state = makeBaseState({
    sidePanelOpen: true,
    sidePanelTabs: [
      {
        id: "tab:subagent:agent:host-a:child-a1",
        kind: "subagent",
        title: "Worker",
        hostId: "host-a",
        parentHostId: "host-a",
        parentAgentId: "agent-a",
        childAgentId: "child-a1",
      },
    ],
    activeSidePanelTabId: "tab:subagent:agent:host-a:child-a1",
    sidebarAutoYieldedForSidePanel: true,
    sidebarDesktopOpenPref: true,
    sidebarOpen: false,
  });

  const next = removeSubagentTabsFromScopes(state, "host-a", "agent-a", [
    { id: "child-a1", childAgentId: "child-a1" },
  ]);
  assert.equal(next.sidePanelTabs.length, 0);
  assert.equal(next.sidePanelOpen, false);
  assert.equal(next.sidebarOpen, true);
  assert.equal(next.sidebarAutoYieldedForSidePanel, false);
});

it("rebinds draft workspace tabs to the newly picked project while leaving session and file tabs pinned", () => {
  const state = makeBaseState({
    selectedHostId: "host-a",
    selectedAgentId: null,
    isDraftNewSession: true,
    selectedWorkspaceCwd: "/repo-old",
    sidePanelScopeKey: "draft::host-a",
    sidePanelOpen: true,
    sidePanelTabs: [
      { id: "tab:files", kind: "files", title: "Files", hostId: "host-a", agentId: null, cwd: "/repo-old" },
      { id: "tab:file:x", kind: "file", title: "x.ts", hostId: "host-a", agentId: null, cwd: "/repo-old", filePath: "x.ts" },
    ],
    sidePanelClosedTabs: [
      { id: "tab:review", kind: "review", title: "Review", hostId: "host-a", agentId: null, cwd: "/repo-old" },
    ],
    activeSidePanelTabId: "tab:files",
  });

  const next = rebindDraftSidePanelCwd(state, "/repo-new");
  assert.equal(next.sidePanelTabs.find((t) => t.id === "tab:files")?.cwd, "/repo-new");
  assert.equal(next.sidePanelTabs.find((t) => t.id === "tab:file:x")?.cwd, "/repo-old");
  assert.equal(next.sidePanelClosedTabs[0]?.cwd, "/repo-new");
  assert.equal(rebindDraftSidePanelCwd(next, "/repo-new"), next);
  assert.equal(rebindDraftSidePanelCwd({ ...state, isDraftNewSession: false }, "/repo-new").sidePanelTabs, state.sidePanelTabs);
});

it("activates destination file tab on gotoLspDefinition and ignores stale results after scope switch (P0 #1)", async () => {
  const { orbitActions, getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

  const origGoto = orbitIpc.lspGotoDefinition;
  const origReadFile = orbitIpc.readWorkspaceFile;
  const origOpenTimeline = orbitIpc.openAgentTimeline;
  const origListSub = orbitIpc.listProviderSubagents;
  const origMarkSeen = orbitIpc.markAgentSeen;
  const origFetchGitStatus = orbitIpc.fetchGitStatus;
  const origFetchGitDiff = orbitIpc.fetchGitDiff;
  const origInspectGit = orbitIpc.inspectGitRepos;
  const origGetSidebar = orbitIpc.getSidebarView;

  try {
    orbitIpc.readWorkspaceFile = async () => ({
      path: "src/b.ts",
      content: "export const b = 1;\n",
      truncated: false,
      size_bytes: 20,
      is_binary: false,
      language: "typescript",
    });
    orbitIpc.openAgentTimeline = async () => undefined;
    orbitIpc.listProviderSubagents = async () => [];
    orbitIpc.markAgentSeen = async () => undefined;
    orbitIpc.fetchGitStatus = async () => undefined;
    orbitIpc.fetchGitDiff = async () => undefined;
    orbitIpc.inspectGitRepos = async () => null;
    orbitIpc.getSidebarView = async () => ({ projects: [], archived_count: 0 });

    const detailA = makeHostDetail("host-a", [
      { agent_id: "ag-1", cwd: "/repo-1" },
      { agent_id: "ag-2", cwd: "/repo-2" },
    ]);
    Object.assign(getOrbitState(), {
      ...makeBaseState(),
      hostDetails: { "host-a": detailA as never },
      selectedHostId: "host-a",
      selectedAgentId: "ag-1",
      selectedWorkspaceCwd: "/repo-1",
      isDraftNewSession: false,
      sidePanelScopeKey: "host-a::ag-1",
    });

    orbitActions.openFileInSidePanel("src/a.ts", { hostId: "host-a", cwd: "/repo-1" });
    const fileATabId = getOrbitState().activeSidePanelTabId;
    assert.ok(fileATabId && fileATabId.includes("src/a.ts"));

    orbitIpc.lspGotoDefinition = async () => [
      { path: "src/b.ts", line: 12, col: 4, end_line: 12, end_col: 10 },
    ];
    const loc = await orbitActions.gotoLspDefinition("host-a", "/repo-1", "src/a.ts", 3, 5);
    assert.ok(loc);
    assert.ok(
      getOrbitState().activeSidePanelTabId?.includes("src/b.ts"),
      "gotoLspDefinition should focus destination file tab when panel is already open",
    );

    // Mid-flight scope switch: stale LSP response must not pollute ag-2's side panel
    let resolveDeferred!: (v: unknown) => void;
    orbitIpc.lspGotoDefinition = () =>
      new Promise((resolve) => {
        resolveDeferred = resolve;
      });
    const pending = orbitActions.gotoLspDefinition("host-a", "/repo-1", "src/b.ts", 1, 1);
    orbitActions.selectAgent("host-a", "ag-2");
    orbitActions.openReviewTab();
    resolveDeferred([{ path: "src/c.ts", line: 1, col: 1, end_line: 1, end_col: 2 }]);
    const staleResult = await pending;
    assert.equal(staleResult, null);
    assert.equal(
      getOrbitState().sidePanelTabs.some((t: SidePanelTab) => t.filePath === "src/c.ts"),
      false,
    );
  } finally {
    orbitIpc.lspGotoDefinition = origGoto;
    orbitIpc.readWorkspaceFile = origReadFile;
    orbitIpc.openAgentTimeline = origOpenTimeline;
    orbitIpc.listProviderSubagents = origListSub;
    orbitIpc.markAgentSeen = origMarkSeen;
    orbitIpc.fetchGitStatus = origFetchGitStatus;
    orbitIpc.fetchGitDiff = origFetchGitDiff;
    orbitIpc.inspectGitRepos = origInspectGit;
    orbitIpc.getSidebarView = origGetSidebar;
  }
});

it("preserves cross-host peer tabs opened from Personal Agent across switches, persistence, and reopen (P0 #2)", async () => {
  const { orbitActions, getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

  const origOpenTimeline = orbitIpc.openAgentTimeline;
  const origListSub = orbitIpc.listProviderSubagents;
  const origMarkSeen = orbitIpc.markAgentSeen;
  const origFetchGitStatus = orbitIpc.fetchGitStatus;
  const origFetchGitDiff = orbitIpc.fetchGitDiff;
  const origInspectGit = orbitIpc.inspectGitRepos;
  const origGetSidebar = orbitIpc.getSidebarView;
  const origGetStage = orbitIpc.getAgentStageWindow;

  try {
    orbitIpc.openAgentTimeline = async () => undefined;
    orbitIpc.listProviderSubagents = async () => [];
    orbitIpc.markAgentSeen = async () => undefined;
    orbitIpc.fetchGitStatus = async () => undefined;
    orbitIpc.fetchGitDiff = async () => undefined;
    orbitIpc.inspectGitRepos = async () => null;
    orbitIpc.getSidebarView = async () => ({ projects: [], archived_count: 0 });
    orbitIpc.getAgentStageWindow = async () => null;

    const detailA = makeHostDetail("host-a", [
      { agent_id: "ag-personal", cwd: "/home/personal" },
      { agent_id: "ag-other", cwd: "/repo-other" },
    ]);
    const detailB = makeHostDetail("host-b", [
      { agent_id: "ag-worker", cwd: "/remote/repo", title: "Remote Worker" },
    ]);

    Object.assign(getOrbitState(), {
      ...makeBaseState(),
      hostDetails: { "host-a": detailA, "host-b": detailB },
      selectedHostId: "host-a",
      selectedAgentId: "ag-personal",
      selectedWorkspaceCwd: "/home/personal",
      isDraftNewSession: false,
      sidePanelScopeKey: "host-a::ag-personal",
    });

    orbitActions.openPeerSplitTab("host-b", "ag-worker", "Remote Worker");
    const opened = getOrbitState().sidePanelTabs.find(
      (t: SidePanelTab) => t.id === "tab:peer:host-b::ag-worker",
    );
    assert.ok(opened);
    assert.equal(opened.hostId, "host-b");
    assert.equal(opened.parentHostId, "host-a");

    // Switch away to ag-other and back to ag-personal -> cross-host peer tab survives
    orbitActions.selectAgent("host-a", "ag-other");
    assert.equal(getOrbitState().sidePanelTabs.length, 0);
    orbitActions.selectAgent("host-a", "ag-personal");
    assert.equal(
      getOrbitState().sidePanelTabs.some((t: SidePanelTab) => t.id === "tab:peer:host-b::ag-worker"),
      true,
    );

    // Persistence round-trip preserves parentHostId and keeps tab valid on reload
    const storage = new Map<string, string>();
    const mockStorage = {
      getItem: (k: string) => storage.get(k) ?? null,
      setItem: (k: string, v: string) => {
        storage.set(k, v);
      },
    };
    savePersistedSidePanelSnapshots(getOrbitState(), mockStorage, 10_000);
    const restored = loadPersistedSidePanelSnapshots(mockStorage);
    const restoredTab = restored["host-a::ag-personal"]?.tabs.find(
      (t) => t.id === "tab:peer:host-b::ag-worker",
    );
    assert.equal(restoredTab?.parentHostId, "host-a");

    // Close and reopen via ⌘⇧T
    orbitActions.closeSidePanelTab("tab:peer:host-b::ag-worker");
    assert.equal(getOrbitState().sidePanelTabs.length, 0);
    orbitActions.reopenClosedSidePanelTab();
    assert.equal(getOrbitState().activeSidePanelTabId, "tab:peer:host-b::ag-worker");
  } finally {
    orbitIpc.openAgentTimeline = origOpenTimeline;
    orbitIpc.listProviderSubagents = origListSub;
    orbitIpc.markAgentSeen = origMarkSeen;
    orbitIpc.fetchGitStatus = origFetchGitStatus;
    orbitIpc.fetchGitDiff = origFetchGitDiff;
    orbitIpc.inspectGitRepos = origInspectGit;
    orbitIpc.getSidebarView = origGetSidebar;
    orbitIpc.getAgentStageWindow = origGetStage;
  }
});

it("renders BrowserTab from tab scope and targets ReviewTab actions to tab cwd (P1 #1)", async () => {
  const { BrowserTab } = await server.ssrLoadModule("/src/components/side-panel/browser-tab.tsx");
  const { orbitActions, getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

  const origFetchGitStatus = orbitIpc.fetchGitStatus;
  const origFetchGitDiff = orbitIpc.fetchGitDiff;
  const origGetGitReview = orbitIpc.getGitReviewSnapshot;

  try {
    const fetchedGitTargets: Array<{ hostId: string; cwd: string; staged: boolean; path: string | null }> = [];
    orbitIpc.fetchGitStatus = async () => undefined;
    orbitIpc.fetchGitDiff = async (hostId: string, cwd: string, path: string | null, staged: boolean) => {
      fetchedGitTargets.push({ hostId, cwd, path, staged });
    };
    orbitIpc.getGitReviewSnapshot = async () => null;

    const detailA = makeHostDetail("host-a", [{ agent_id: "agent-a", cwd: "/repo-a" }]);
    const detailB = makeHostDetail("host-b", [{ agent_id: "agent-b", cwd: "/repo-b" }]);

    Object.assign(getOrbitState(), {
      ...makeBaseState(),
      selectedHostId: "host-b",
      selectedAgentId: "agent-b",
      selectedWorkspaceCwd: "/repo-b",
      isDraftNewSession: false,
      hostDetails: { "host-a": detailA, "host-b": detailB },
      stages: {
        "host-a::agent-a": {
          insights: { detected_local_urls: ["http://localhost:4173"] },
        },
        "host-b::agent-b": {
          insights: { detected_local_urls: ["http://localhost:9999"] },
        },
      },
      sidePanelTabs: [
        {
          id: "tab:review",
          kind: "review",
          title: "Review",
          hostId: "host-a",
          agentId: "agent-a",
          cwd: "/repo-a",
        },
      ],
      selectedDiffPath: "src/old.ts",
    });

    const browserTab: SidePanelTab = {
      id: "tab:browser",
      kind: "browser",
      title: "Browser",
      hostId: "host-a",
      agentId: "agent-a",
      cwd: "/repo-a",
      url: "",
    };
    const browserHtml = renderToStaticMarkup(createElement(BrowserTab, { tab: browserTab, visible: true }));
    assert.match(browserHtml, /localhost:4173/);
    assert.doesNotMatch(browserHtml, /localhost:9999/);

    orbitActions.setReviewScope("working_tree");
    orbitActions.setDiffStaged(true);
    orbitActions.selectDiffFile("src/a.ts");
    await new Promise((r) => setTimeout(r, 0));
    assert.equal(fetchedGitTargets.length, 3);
    assert.deepEqual(
      fetchedGitTargets.map((x) => `${x.hostId}::${x.cwd}`),
      ["host-a::/repo-a", "host-a::/repo-a", "host-a::/repo-a"],
    );

    // startNewThreadDraft & markThreadUnread clear selectedDiffPath
    Object.assign(getOrbitState(), { selectedDiffPath: "src/a.ts" });
    orbitActions.startNewThreadDraft("host-a", "/repo-a");
    assert.equal(getOrbitState().selectedDiffPath, null);
  } finally {
    orbitIpc.fetchGitStatus = origFetchGitStatus;
    orbitIpc.fetchGitDiff = origFetchGitDiff;
    orbitIpc.getGitReviewSnapshot = origGetGitReview;
  }
});

it("skips child subagents on parent archive fallback and prunes remotely archived scopes without drafts (P1 #2)", async () => {
  const { orbitActions, getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { collectRemotelyArchivedAgentScopes } = await server.ssrLoadModule(
    "/src/state/store-helpers.ts",
  );
  const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

  const origArchive = orbitIpc.archiveAgent;
  const origGetSidebar = orbitIpc.getSidebarView;
  const origOpenTimeline = orbitIpc.openAgentTimeline;
  const origListSub = orbitIpc.listProviderSubagents;
  const origMarkSeen = orbitIpc.markAgentSeen;
  const origFetchGitStatus = orbitIpc.fetchGitStatus;
  const origFetchGitDiff = orbitIpc.fetchGitDiff;
  const origInspectGit = orbitIpc.inspectGitRepos;
  const origGetStage = orbitIpc.getAgentStageWindow;
  const origGetHostDetail = orbitIpc.getHostDetail;

  try {
    orbitIpc.archiveAgent = async () => undefined;
    orbitIpc.getSidebarView = async () => ({ projects: [], archived_count: 0 });
    orbitIpc.openAgentTimeline = async () => undefined;
    orbitIpc.listProviderSubagents = async () => [];
    orbitIpc.markAgentSeen = async () => undefined;
    orbitIpc.fetchGitStatus = async () => undefined;
    orbitIpc.fetchGitDiff = async () => undefined;
    orbitIpc.inspectGitRepos = async () => null;
    orbitIpc.getAgentStageWindow = async () => null;

    const detail = makeHostDetail("host-a", [
      { agent_id: "parent-1", cwd: "/repo-1" },
      { agent_id: "child-1", cwd: "/repo-1", parent_agent_id: "parent-1", parent_host_id: "host-a" },
      { agent_id: "sibling-2", cwd: "/repo-2" },
    ]);

    Object.assign(getOrbitState(), {
      ...makeBaseState(),
      hostDetails: { "host-a": detail },
      selectedHostId: "host-a",
      selectedAgentId: "parent-1",
      selectedWorkspaceCwd: "/repo-1",
      isDraftNewSession: false,
      sidePanelScopeKey: "host-a::parent-1",
      sidePanelOpen: true,
      sidePanelTabs: [
        { id: "tab:files", kind: "files", title: "Files", hostId: "host-a", agentId: "parent-1", cwd: "/repo-1" },
      ],
      activeSidePanelTabId: "tab:files",
      sidePanelByScope: {
        "host-a::sibling-2": {
          open: true,
          tabs: [
            {
              id: "tab:peer:host-a::child-1",
              kind: "peer",
              title: "Child",
              hostId: "host-a",
              parentHostId: "host-a",
              peerAgentId: "child-1",
            },
          ],
          activeTabId: "tab:peer:host-a::child-1",
          split: null,
          userDismissed: false,
          closedTabs: [],
          unseenTabIds: [],
          lastUsedAt: 100,
        },
      },
    });

    orbitActions.archiveThread("host-a", "parent-1");
    assert.equal(getOrbitState().selectedAgentId, "sibling-2");
    assert.equal(getOrbitState().sidePanelByScope["host-a::parent-1"], undefined);
    // Cascaded child cleanup should prune peer tabs referencing child-1 in sibling-2
    assert.equal(getOrbitState().sidePanelTabs.length, 0);

    // Remotely archived agent with NO composer draft is collected & pruned on reloadHostDetail
    const remoteArchivedDetail = makeHostDetail("host-a", [
      { agent_id: "sibling-2", cwd: "/repo-2" },
      { agent_id: "ag-remote-dead", cwd: "/repo-dead", archived_at_rfc3339: "2026-01-01T00:00:00Z" },
    ]);
    const collected = collectRemotelyArchivedAgentScopes(
      [["host-a", remoteArchivedDetail]],
      "host-a::sibling-2",
    );
    assert.equal(collected.keys.has("host-a::ag-remote-dead"), true);

    Object.assign(getOrbitState(), {
      composerDrafts: {},
      sidePanelByScope: {
        ...getOrbitState().sidePanelByScope,
        "host-a::ag-remote-dead": {
          open: true,
          tabs: [
            { id: "tab:files", kind: "files", title: "Files", hostId: "host-a", agentId: "ag-remote-dead", cwd: "/repo-dead" },
          ],
          activeTabId: "tab:files",
          split: null,
          userDismissed: false,
          closedTabs: [],
          unseenTabIds: [],
          lastUsedAt: 200,
        },
      },
    });
    orbitIpc.getHostDetail = async () => remoteArchivedDetail;
    await orbitActions.reloadHostDetail("host-a", false);
    assert.equal(getOrbitState().sidePanelByScope["host-a::ag-remote-dead"], undefined);
  } finally {
    orbitIpc.archiveAgent = origArchive;
    orbitIpc.getSidebarView = origGetSidebar;
    orbitIpc.openAgentTimeline = origOpenTimeline;
    orbitIpc.listProviderSubagents = origListSub;
    orbitIpc.markAgentSeen = origMarkSeen;
    orbitIpc.fetchGitStatus = origFetchGitStatus;
    orbitIpc.fetchGitDiff = origFetchGitDiff;
    orbitIpc.inspectGitRepos = origInspectGit;
    orbitIpc.getAgentStageWindow = origGetStage;
    orbitIpc.getHostDetail = origGetHostDetail;
  }
});

it("rebinds draft workspace tabs on startNewThreadDraft and Worktree draft promotion (P1 #3)", async () => {
  const { orbitActions, getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

  const origGetSidebar = orbitIpc.getSidebarView;
  const origInspectGit = orbitIpc.inspectGitRepos;
  try {
    orbitIpc.getSidebarView = async () => ({ projects: [], archived_count: 0 });
    orbitIpc.inspectGitRepos = async () => null;

    const detail = makeHostDetail("host-a", []);
    Object.assign(getOrbitState(), {
      ...makeBaseState(),
      hostDetails: { "host-a": detail },
      selectedHostId: "host-a",
      selectedAgentId: null,
      selectedWorkspaceCwd: "/repo-1",
      isDraftNewSession: true,
      sidePanelScopeKey: "draft::host-a",
      sidePanelOpen: true,
      sidePanelTabs: [
        { id: "tab:files", kind: "files", title: "Files", hostId: "host-a", agentId: null, cwd: "/repo-1" },
        { id: "tab:browser", kind: "browser", title: "Browser", hostId: "host-a", agentId: null, cwd: "/repo-1", url: "" },
        { id: "tab:file:a", kind: "file", title: "a.ts", hostId: "host-a", agentId: null, cwd: "/repo-1", filePath: "a.ts" },
      ],
      activeSidePanelTabId: "tab:files",
    });

    orbitActions.startNewThreadDraft("host-a", "/repo-2");
    assert.equal(
      getOrbitState().sidePanelTabs.find((t: SidePanelTab) => t.id === "tab:files")?.cwd,
      "/repo-2",
    );
    assert.equal(
      getOrbitState().sidePanelTabs.find((t: SidePanelTab) => t.id === "tab:browser")?.cwd,
      "/repo-2",
    );
    assert.equal(
      getOrbitState().sidePanelTabs.find((t: SidePanelTab) => t.id === "tab:file:a")?.cwd,
      "/repo-1",
    );

    // Worktree promotion rebinds draft workspace tabs to the new worktree cwd while keeping file tabs pinned
    const promoted = promoteDraftSidePanelScope(getOrbitState(), "host-a", "ag-wt-1", "/repo-2/.worktrees/feat");
    assert.equal(
      promoted.sidePanelTabs.find((t) => t.id === "tab:files")?.cwd,
      "/repo-2/.worktrees/feat",
    );
    assert.equal(
      promoted.sidePanelTabs.find((t) => t.id === "tab:browser")?.cwd,
      "/repo-2/.worktrees/feat",
    );
    assert.equal(
      promoted.sidePanelTabs.find((t) => t.id === "tab:file:a")?.cwd,
      "/repo-1",
    );
  } finally {
    orbitIpc.getSidebarView = origGetSidebar;
    orbitIpc.inspectGitRepos = origInspectGit;
  }
});

it("treats attachExistingTerminal as user-initiated and skips archived/deleted targets on ⌘⇧T (P1 #4)", async () => {
  const { orbitActions, getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

  const origEnsureSlot = orbitIpc.ensureTerminalSlot;
  const origAttachTerm = orbitIpc.attachTerminal;
  try {
    orbitIpc.ensureTerminalSlot = async () => 1;
    orbitIpc.attachTerminal = async () => undefined;

    Object.assign(getOrbitState(), {
      ...makeBaseState(),
      selectedHostId: "host-a",
      selectedAgentId: "ag-1",
      selectedWorkspaceCwd: "/repo-1",
      isDraftNewSession: false,
      sidePanelScopeKey: "host-a::ag-1",
      sidePanelOpen: false,
      sidePanelUserDismissed: true,
      terminalTabs: [],
    });

    await orbitActions.attachExistingTerminal("host-a", "term-1", "/repo-1", "shell");
    assert.equal(getOrbitState().sidePanelOpen, true);
    assert.equal(getOrbitState().sidePanelUserDismissed, false);
    assert.equal(getOrbitState().activeSidePanelTabId, "tab:terminal");
  } finally {
    orbitIpc.ensureTerminalSlot = origEnsureSlot;
    orbitIpc.attachTerminal = origAttachTerm;
  }

  // applyReopenClosedSidePanelTab skips archived peer, archived subagent, and archived board
  const detailA = makeHostDetail("host-a", [
    { agent_id: "ag-1", cwd: "/repo-1" },
    { agent_id: "peer-dead", cwd: "/repo-1", archived_at_rfc3339: "2026-01-01T00:00:00Z" },
    { agent_id: "child-alive", cwd: "/repo-1", parent_agent_id: "ag-1" },
  ]);
  detailA.workspaces = [
    { workspace_id: "ws-dead", cwd: "/repo-1", archived_at: "2026-01-01T00:00:00Z" },
  ] as never;

  const state = makeBaseState({
    selectedHostId: "host-a",
    selectedAgentId: "ag-1",
    isDraftNewSession: false,
    sidePanelScopeKey: "host-a::ag-1",
    hostDetails: { "host-a": detailA as never },
    archivedSubagents: {
      "host-a::ag-1::child-alive": true,
    },
    sidePanelClosedTabs: [
      {
        id: "tab:peer:host-a::peer-dead",
        kind: "peer",
        title: "Dead Peer",
        hostId: "host-a",
        parentHostId: "host-a",
        peerAgentId: "peer-dead",
      },
      {
        id: "tab:subagent:agent:host-a:child-alive",
        kind: "subagent",
        title: "Archived Sub",
        hostId: "host-a",
        parentHostId: "host-a",
        parentAgentId: "ag-1",
        childAgentId: "child-alive",
      },
      {
        id: "tab:board:host-a:ws-dead",
        kind: "board",
        title: "Dead Board",
        hostId: "host-a",
        workspaceId: "ws-dead",
      },
      {
        id: "tab:files",
        kind: "files",
        title: "Files",
        hostId: "host-a",
        agentId: "ag-1",
        cwd: "/repo-1",
      },
    ],
  });

  const reopened = applyReopenClosedSidePanelTab(state);
  assert.equal(reopened.reopenedTab?.id, "tab:files");
  assert.equal(reopened.state.activeSidePanelTabId, "tab:files");
  assert.equal(reopened.state.sidePanelClosedTabs.length, 0);
});
