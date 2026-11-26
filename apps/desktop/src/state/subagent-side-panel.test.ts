import assert from "node:assert/strict";
import { after, before, it } from "node:test";
import { setImmediate } from "node:timers/promises";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { CoreNotification } from "../lib/orbit-ipc.ts";
import type { SidePanelTab } from "./store-types.ts";

let server: ViteDevServer;
before(async () => {
  server = await createServer({
    server: { middlewareMode: true, hmr: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [
      {
        name: "orbit-test-ssr-get-server-snapshot",
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
});
after(async () => { await server?.close(); });

it("routes catchup and streaming notifications to open child timelines by host and agent", async (t) => {
  const { orbitActions, getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");
  let emit!: (event: CoreNotification) => void;
  t.mock.method(orbitIpc, "subscribe", async (cb: typeof emit) => { emit = cb; });
  t.mock.method(orbitActions, "reloadFleetAndHosts", async () => {});
  t.mock.method(orbitActions, "refreshSidebarView", () => {});
  const pull = t.mock.method(orbitActions, "pullStageDelta", async () => {});
  await orbitActions.retryBootstrap();

  const tabs: SidePanelTab[] = [
    { id: "visible", title: "Files", kind: "files" },
    { id: "child", title: "Child", kind: "subagent", hostId: "remote", childAgentId: "a" },
    { id: "legacy", title: "Legacy", kind: "subagent", childAgentId: "b" },
    { id: "peer", title: "Peer", kind: "peer", hostId: "remote", peerAgentId: "p" },
    { id: "native", title: "Native", kind: "subagent", hostId: "remote", providerSubagentId: "n" },
  ];
  Object.assign(getOrbitState(), {
    sidePanelOpen: true, sidePanelTabs: tabs, activeSidePanelTabId: "visible",
    selectedHostId: "local", selectedAgentId: "main", selectedWorkspaceCwd: null, isDraftNewSession: false,
  });
  let revision = 0;
  const notify = (host_id: string, agent_id: string) =>
    emit({ kind: "timeline_changed", revision: ++revision, host_id, agent_id });
  notify("remote", "a");
  notify("remote", "a");
  notify("local", "a");
  notify("local", "b");
  notify("remote", "b");
  notify("remote", "p");
  notify("remote", "n");
  orbitActions.toggleSidePanel(false);
  notify("remote", "a");
  notify("local", "main");
  assert.deepEqual(pull.mock.calls.map((call: { arguments: unknown[] }) => call.arguments), [
    ["remote", "a"], ["remote", "a"], ["local", "b"], ["remote", "p"], ["local", "main"],
  ]);
});

it("pulls a cold child immediately and waits for open before catching up on both open and focus", async (t) => {
  const { orbitActions, getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");
  const pulls = t.mock.method(orbitActions, "pullStageDelta", async () => {});
  let finishOpen!: () => void;
  const opens = t.mock.method(orbitIpc, "openAgentTimeline", () =>
    new Promise<void>((resolve) => { finishOpen = resolve; }),
  );
  Object.assign(getOrbitState(), {
    selectedHostId: "local", selectedAgentId: "parent", sidePanelTabs: [],
    hostDetails: { remote: { agents: [{ agent_id: "child" }] } },
    stages: { "remote::child": { turns: [] } },
  });
  orbitActions.openSubagentTab({
    callId: "call", role: "Reviewer", promptPreview: "Inspect", childHostId: "remote", childAgentId: "child",
  });
  const tabId = getOrbitState().activeSidePanelTabId;
  assert.equal(getOrbitState().sidePanelTabs.find((tab: SidePanelTab) => tab.id === tabId)?.childAgentId, "child");
  assert.deepEqual(pulls.mock.calls.map((call: { arguments: unknown[] }) => call.arguments), [["remote", "child", true]]);
  finishOpen();
  await setImmediate();
  assert.deepEqual(pulls.mock.calls[1].arguments, ["remote", "child", false]);

  const cached = [{ key: "already-visible" }];
  getOrbitState().stages["remote::child"] = { turns: cached };
  orbitActions.focusSidePanelTab(tabId);
  assert.strictEqual(getOrbitState().stages["remote::child"].turns, cached);
  assert.equal(pulls.mock.callCount(), 2);
  finishOpen();
  await setImmediate();
  assert.deepEqual(pulls.mock.calls[2].arguments, ["remote", "child", false]);
  assert.deepEqual(opens.mock.calls.map((call: { arguments: unknown[] }) => call.arguments), [["remote", "child"], ["remote", "child"]]);
});

it("gives the handle priority while keeping PeerMessageRow collapsible with an iconless Open action and Markdown body", async () => {
  const { PeerMessageRow, LazyMarkdownMessageBody } = await server.ssrLoadModule("/src/components/stage/subagents-track.tsx");
  const { SubagentPanelAction } = await server.ssrLoadModule("/src/components/stage/subagent-crew-dock.tsx");
  let opens = 0;
  let toggled: boolean | undefined;
  const onOpen = () => { opens++; };
  const action = createElement(SubagentPanelAction, { name: "@timeline-verification", onClick: onOpen });
  const actionHtml = renderToStaticMarkup(action);
  assert.match(actionHtml, />Open<\/button>/);
  assert.doesNotMatch(actionHtml, /<svg\b/i);

  const precomputedBlocks = [
    {
      kind: "paragraph",
      spans: [
        { text: "Markdown", bold: true, italic: false, inline_code: false, strikethrough: false, link_url: null },
        { text: " body", bold: false, italic: false, inline_code: false, strikethrough: false, link_url: null },
      ],
    },
  ];
  const row = createElement(
    PeerMessageRow,
    {
      open: true,
      onOpenChange: (next: boolean) => { toggled = next; },
      avatar: "R",
      label: "@timeline-verification",
      statusLabel: "finished",
      preview: "A much longer secondary preview",
      action,
    },
    createElement(LazyMarkdownMessageBody, { text: "**Markdown** body", precomputedBlocks }),
  );
  const html = renderToStaticMarkup(row);
  assert.equal(html.match(/<button\b/g)?.length, 2);
  assert.match(html, /aria-expanded="true"/);
  assert.match(html, /<span class="[^"]*shrink-0[^"]*max-w-\[62%\][^"]*truncate[^"]*">@timeline-verification<\/span>/);
  assert.match(html, /<span class="[^"]*font-semibold[^"]*">Markdown<\/span><span> body<\/span>/);
  assert.doesNotMatch(html, /<span class="min-w-0 flex-1 truncate text-ink-tertiary">A much longer secondary preview<\/span>/);

  const collapsedHtml = renderToStaticMarkup(
    createElement(
      PeerMessageRow,
      {
        open: false,
        onOpenChange: (next: boolean) => { toggled = next; },
        avatar: "R",
        label: "@timeline-verification",
        statusLabel: "finished",
        preview: "A much longer secondary preview",
        action,
      },
      createElement(LazyMarkdownMessageBody, { text: "**Markdown** body", precomputedBlocks }),
    ),
  );
  assert.match(collapsedHtml, /aria-expanded="false"/);
  assert.match(collapsedHtml, /<span class="min-w-0 flex-1 truncate text-ink-tertiary">A much longer secondary preview<\/span>/);
  assert.equal(toggled, undefined);
  assert.equal(opens, 0);
});

it("routes selectAgent on a child subagent to its parent thread and opens the child in the right side panel", async (t) => {
  const { orbitActions, getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");
  t.mock.method(orbitIpc, "openAgentTimeline", async () => {});
  t.mock.method(orbitIpc, "markAgentRead", async () => {});
  t.mock.method(orbitIpc, "listProviderSubagents", async () => {});
  t.mock.method(orbitActions, "pullStageDelta", async () => {});
  t.mock.method(orbitActions, "refreshSidebarView", () => {});

  Object.assign(getOrbitState(), {
    activeView: "workspace",
    workspacePanel: "conversation",
    isDraftNewSession: false,
    selectedHostId: "local",
    selectedAgentId: "other-root",
    sidePanelOpen: false,
    sidePanelTabs: [],
    activeSidePanelTabId: null,
    hostDetails: {
      local: {
        agents: [
          { agent_id: "parent-root", parent_agent_id: null, title: "Main Thread", cwd: "/repo" },
          {
            agent_id: "child-sub",
            parent_agent_id: "parent-root",
            title: "Scroll Overhaul",
            task_summary: "Fix scroll behavior",
            cwd: "/repo",
          },
        ],
        workspaces: [],
      },
    },
  });

  orbitActions.selectAgent("local", "child-sub");
  const state = getOrbitState();
  assert.equal(state.selectedHostId, "local");
  assert.equal(state.selectedAgentId, "parent-root");
  assert.equal(state.sidePanelOpen, true);
  const activeTab = state.sidePanelTabs.find((tab: SidePanelTab) => tab.id === state.activeSidePanelTabId);
  assert.equal(activeTab?.kind, "subagent");
  assert.equal(activeTab?.childAgentId, "child-sub");
  assert.equal(activeTab?.parentAgentId, "parent-root");
});

it("preserves unsaved WorkspaceBoardTab drafts across Preview toggles and background updates", async () => {
  const { reconcileBoardDraft, WorkspaceBoardTab } = await server.ssrLoadModule(
    "/src/components/side-panel/workspace-board-tab.tsx",
  );
  const { getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");

  // 1. Clean initial load adopts server content
  assert.deepEqual(
    reconcileBoardDraft({
      prevScopeKey: "local::ws-1",
      nextScopeKey: "local::ws-1",
      prevServerContent: "",
      nextServerContent: "## Tasks\n- Ship Wave 1C",
      currentDraft: "",
    }),
    { nextDraft: "## Tasks\n- Ship Wave 1C", shouldExitEdit: false },
  );

  // 2. Toggling Edit -> Preview (same server content) preserves unsaved draft
  assert.deepEqual(
    reconcileBoardDraft({
      prevScopeKey: "local::ws-1",
      nextScopeKey: "local::ws-1",
      prevServerContent: "## Tasks\n- Ship Wave 1C",
      nextServerContent: "## Tasks\n- Ship Wave 1C",
      currentDraft: "## Tasks\n- Ship Wave 1C\n- Unsaved local note",
    }),
    { nextDraft: "## Tasks\n- Ship Wave 1C\n- Unsaved local note", shouldExitEdit: false },
  );

  // 3. Background server revision advancing while dirty does NOT wipe unsaved edits
  assert.deepEqual(
    reconcileBoardDraft({
      prevScopeKey: "local::ws-1",
      nextScopeKey: "local::ws-1",
      prevServerContent: "## Tasks\n- Ship Wave 1C",
      nextServerContent: "## Tasks\n- Peer updated item",
      currentDraft: "## Tasks\n- Unsaved local note",
    }),
    { nextDraft: "## Tasks\n- Unsaved local note", shouldExitEdit: false },
  );

  // 4. Background server revision advancing while clean adopts server content
  assert.deepEqual(
    reconcileBoardDraft({
      prevScopeKey: "local::ws-1",
      nextScopeKey: "local::ws-1",
      prevServerContent: "## Tasks\n- Ship Wave 1C",
      nextServerContent: "## Tasks\n- Peer updated item",
      currentDraft: "## Tasks\n- Ship Wave 1C",
    }),
    { nextDraft: "## Tasks\n- Peer updated item", shouldExitEdit: false },
  );

  // 5. Switching workspace resets draft and exits edit mode
  assert.deepEqual(
    reconcileBoardDraft({
      prevScopeKey: "local::ws-1",
      nextScopeKey: "local::ws-2",
      prevServerContent: "## Tasks\n- Ship Wave 1C",
      nextServerContent: "## Findings\n- Workspace 2 notes",
      currentDraft: "## Tasks\n- Unsaved local note",
    }),
    { nextDraft: "## Findings\n- Workspace 2 notes", shouldExitEdit: true },
  );

  // 6. Keep-alive visibility toggles root `hidden` class without unmounting
  Object.assign(getOrbitState(), {
    selectedHostId: "local",
    selectedAgentId: "agent-1",
    hostDetails: {
      local: {
        agents: [],
        workspaces: [{ workspace_id: "ws-1", title: "Mission Board", cwd: "/repo", mission_goal: "" }],
        workspace_boards: { "ws-1": { content: "## Tasks\n- Persisted note", revision: 2, updated_by: "user" } },
        workspace_events: { "ws-1": [] },
      },
    },
  });
  const boardTab: SidePanelTab = { id: "tab:board:local::ws-1", kind: "board", title: "Notes", hostId: "local", workspaceId: "ws-1" };
  const hiddenHtml = renderToStaticMarkup(createElement(WorkspaceBoardTab, { tab: boardTab, visible: false }));
  const hiddenRootClasses = hiddenHtml.match(/^<div class="([^"]+)"/)?.[1]?.split(/\s+/) ?? [];
  assert.ok(hiddenRootClasses.includes("hidden"));
  assert.match(hiddenHtml, /Persisted note/);
  const visibleHtml = renderToStaticMarkup(createElement(WorkspaceBoardTab, { tab: boardTab, visible: true }));
  const visibleRootClasses = visibleHtml.match(/^<div class="([^"]+)"/)?.[1]?.split(/\s+/) ?? [];
  assert.ok(!visibleRootClasses.includes("hidden"));
  assert.match(visibleHtml, /Persisted note/);
});

it("keeps ReviewTab and WorkspaceBoardTab mounted-hidden in WorkspaceSidePanel while another tab is active", async () => {
  const { WorkspaceSidePanel } = await server.ssrLoadModule(
    "/src/components/side-panel/workspace-side-panel.tsx",
  );
  const { WorkspaceMotionProvider } = await server.ssrLoadModule(
    "/src/components/layout/workspace-motion.tsx",
  );
  const { getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");

  Object.assign(getOrbitState(), {
    selectedHostId: "local",
    selectedAgentId: "agent-1",
    selectedWorkspaceCwd: "/repo",
    sidePanelOpen: true,
    activeSidePanelTabId: "tab:file:readme",
    sidePanelTabs: [
      { id: "tab:review", kind: "review", title: "Review" },
      { id: "tab:board:local::ws-1", kind: "board", title: "Notes", hostId: "local", workspaceId: "ws-1" },
      { id: "tab:file:readme", kind: "file", title: "README.md", hostId: "local", cwd: "/repo", filePath: "/repo/README.md" },
    ],
    terminalTabs: [],
    fileEditorDrafts: {},
    fileViewCache: {},
    gitReviews: {},
    hostDetails: {
      local: {
        agents: [],
        artifacts: [],
        workspaces: [{ workspace_id: "ws-1", title: "Mission Board", cwd: "/repo", mission_goal: "" }],
        workspace_boards: { "ws-1": { content: "## Tasks\n- Keep alive board", revision: 1, updated_by: "user" } },
        workspace_events: { "ws-1": [] },
      },
    },
    fleet: { hosts: [] },
  });

  const html = renderToStaticMarkup(
    createElement(WorkspaceMotionProvider, null, createElement(WorkspaceSidePanel)),
  );
  // Both ReviewTab and WorkspaceBoardTab stay mounted (with exact `hidden` token) while the FileViewerTab is active
  assert.match(html, /class="[^"]*\borbit-stage-enter\b[^"]*\shidden(?:\s|")/);
  assert.match(html, /Working Tree/);
  assert.match(html, /Keep alive board/);
});

it("mounts actionable DecisionDock in SubagentTab and PeerSplitTab and recovers PeerSplitTab draft on send error", async () => {
  const { SubagentTab, mergeAgentPendingPermissions } = await server.ssrLoadModule(
    "/src/components/side-panel/subagent-tab.tsx",
  );
  const { PeerSplitTab, recoverReplyDraftAfterSendError } = await server.ssrLoadModule(
    "/src/components/side-panel/peer-split-tab.tsx",
  );
  const { getOrbitState } = await server.ssrLoadModule("/src/state/orbit-store.ts");

  const pendingPerm = {
    item_type: "permission_request" as const,
    seq: 10,
    request_id: "req-shell-1",
    title: "Run tests",
    detail: { kind: "shell" as const, command: "cargo test", cwd: "/repo" },
    allowed_decisions: ["allow", "allow_always", "deny"],
    resolved_decision: null,
    guardian_assessment: null,
    timestamp_ms: 1700000000000,
  };
  const pendingQuestion = {
    item_type: "question_request" as const,
    seq: 11,
    request_id: "req-q-1",
    questions: [
      {
        id: "q1",
        header: "Target",
        question: "Which target should run?",
        options: ["Desktop", "All"],
        is_multi_select: false,
      },
    ],
    resolved_answers: null,
    timestamp_ms: 1700000001000,
  };

  // Helper deduplication & resolved-filtering check
  const merged = mergeAgentPendingPermissions(
    "child-1",
    [pendingPerm],
    [
      pendingPerm,
      { ...pendingPerm, request_id: "req-resolved", resolved_decision: "allow" },
    ],
    [
      {
        agent_id: "child-1",
        agent_title: "Child",
        request_id: "req-shell-1",
        title: "Run tests",
        detail: pendingPerm.detail,
        allowed_decisions: ["allow", "deny"],
        created_at: "2026-11-24T00:00:00Z",
      },
      {
        agent_id: "child-1",
        agent_title: "Child",
        request_id: "req-resolved",
        title: "Already resolved",
        detail: pendingPerm.detail,
        allowed_decisions: ["allow", "deny"],
        created_at: "2026-11-24T00:00:01Z",
      },
      {
        agent_id: "child-1",
        agent_title: "Child",
        request_id: "req-host-only",
        title: "Host fallback gate",
        detail: pendingPerm.detail,
        allowed_decisions: ["allow", "deny"],
        created_at: "2026-11-24T00:00:02Z",
      },
    ],
  );
  assert.deepEqual(
    merged.map((p: { request_id: string }) => p.request_id),
    ["req-shell-1", "req-host-only"],
  );

  // Draft recovery helper check
  assert.equal(recoverReplyDraftAfterSendError("", "Retry me"), "Retry me");
  assert.equal(recoverReplyDraftAfterSendError("   ", "Retry me"), "Retry me");
  assert.equal(recoverReplyDraftAfterSendError("Already typing next", "Retry me"), "Already typing next");

  Object.assign(getOrbitState(), {
    selectedHostId: "local",
    selectedAgentId: "parent-1",
    selectedWorkspaceCwd: "/repo",
    sidePanelOpen: true,
    coalesceExploration: false,
    archivedSubagents: {},
    fleet: { hosts: [{ host_id: "local", display_name: "Local", connection_state: "online_direct" }] },
    hostDetails: {
      local: {
        host: { host_id: "local", display_name: "Local", connection_state: "online_direct" },
        agents: [
          {
            agent_id: "parent-1",
            parent_agent_id: null,
            workspace_id: "ws-1",
            title: "Main",
            cwd: "/repo",
            provider: "claude",
            is_running: true,
            is_awaiting_permission: false,
            status_label: "Running",
          },
          {
            agent_id: "child-1",
            parent_agent_id: "parent-1",
            workspace_id: "ws-1",
            title: "Verifier",
            cwd: "/repo",
            provider: "claude",
            is_running: false,
            is_awaiting_permission: true,
            status_label: "Waiting for approval",
          },
          {
            agent_id: "peer-1",
            parent_agent_id: null,
            workspace_id: "ws-1",
            handle: "reviewer",
            title: "Peer Reviewer",
            cwd: "/repo",
            provider: "codex",
            is_running: false,
            is_awaiting_permission: true,
            status_label: "Waiting for approval",
          },
        ],
        workspaces: [{ workspace_id: "ws-1", title: "Group", cwd: "/repo" }],
        pending_permissions: [],
      },
    },
    stages: {
      "local::child-1": {
        revision: 1,
        latest_seq: 10,
        turns: [],
        pending_permissions: [pendingPerm],
        pending_questions: [],
        insights: { touched_files: [], background_tasks: [], subagents: [], detected_local_urls: [] },
      },
      "local::peer-1": {
        revision: 1,
        latest_seq: 11,
        turns: [],
        pending_permissions: [],
        pending_questions: [pendingQuestion],
        insights: { touched_files: [], background_tasks: [], subagents: [], detected_local_urls: [] },
      },
    },
  });

  // Managed SubagentTab renders real actionable DecisionDock (no passive "Approval required" banner)
  const subagentHtml = renderToStaticMarkup(
    createElement(SubagentTab, {
      tab: {
        id: "tab:sub:child-1",
        kind: "subagent",
        title: "Verifier",
        hostId: "local",
        childAgentId: "child-1",
        parentAgentId: "parent-1",
      },
    }),
  );
  assert.match(subagentHtml, /data-slot="subagent-context-header"/);
  assert.match(subagentHtml, /approval-dock-surface/);
  assert.match(subagentHtml, /cargo test/);
  assert.match(subagentHtml, />Allow</);
  assert.match(subagentHtml, />Deny</);
  assert.doesNotMatch(subagentHtml, /data-slot="context-banner"/);
  assert.doesNotMatch(subagentHtml, />Refresh</);
  assert.doesNotMatch(subagentHtml, /aria-label="Open in main stage"|aria-label="Focus session"/);

  // Top-level worker opened from Personal Agent Briefing reuses the exact same SubagentTab
  // header + DecisionDock, while exposing a quiet Focus session jump button
  const paWorkerSubagentHtml = renderToStaticMarkup(
    createElement(SubagentTab, {
      tab: {
        id: "tab:subagent:call:peer-1",
        kind: "subagent",
        title: "Peer Reviewer",
        hostId: "local",
        parentHostId: "local",
        childAgentId: "peer-1",
        parentAgentId: "parent-1",
      },
    }),
  );
  assert.match(paWorkerSubagentHtml, /data-slot="subagent-context-header"/);
  assert.match(paWorkerSubagentHtml, /approval-dock-surface/);
  assert.match(paWorkerSubagentHtml, /Clarification needed/);
  assert.match(paWorkerSubagentHtml, /aria-label="Focus session"/);

  // PeerSplitTab renders real actionable DecisionDock for pending clarification/approval
  const peerHtml = renderToStaticMarkup(
    createElement(PeerSplitTab, {
      tab: {
        id: "tab:peer:local::peer-1",
        kind: "peer",
        title: "@reviewer",
        hostId: "local",
        peerAgentId: "peer-1",
      },
    }),
  );
  assert.match(peerHtml, /approval-dock-surface/);
  assert.match(peerHtml, /Clarification needed/);
  assert.match(peerHtml, /Which target should run\?/);
  assert.match(peerHtml, />Submit</);
});
