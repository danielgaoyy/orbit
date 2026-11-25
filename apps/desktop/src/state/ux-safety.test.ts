import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { createServer, type ViteDevServer } from "vite";
import type { AgentCardView, HostCardView, SessionPlanStepView } from "../lib/orbit-ipc.ts";
import { describePlan } from "../components/composer/composer-plan-shelf-model.ts";
import type {
  ContextMentionItem,
  DiffQuoteSnippet,
  InlineMentionTarget,
  PastedAttachment,
} from "./store-types.ts";

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

function makeHostCard(hostId = "local"): HostCardView {
  return {
    host_id: hostId,
    display_name: "Local",
    connection_state: "online_direct",
    daemon_version: "1.0.0",
    daemon_protocol_version: 4,
    available_providers: ["claude_code"],
    ssh_alias: null,
    supports_server_queue: true,
    active_agents_count: 1,
    total_agents_count: 1,
    pending_permissions_count: 0,
    is_local: true,
  } as unknown as HostCardView;
}

function makeAgentCard(
  agentId: string,
  overrides: Partial<AgentCardView> = {},
): AgentCardView {
  return {
    agent_id: agentId,
    host_id: "local",
    provider: "claude_code",
    title: `Thread ${agentId}`,
    cwd: "/repo",
    status_label: "Idle",
    is_running: false,
    has_background_tasks: false,
    is_awaiting_permission: false,
    pending_permissions_count: 0,
    updated_at_rfc3339: "2026-11-24T08:00:00Z",
    ...overrides,
  } as unknown as AgentCardView;
}

function makeRichDraftFixture() {
  const inlineMentions: InlineMentionTarget[] = [
    { token: "@app.ts", kind: "file", path: "src/app.ts" },
  ];
  const contextItems: ContextMentionItem[] = [
    { id: "ctx_1", kind: "file", path: "src/app.ts", filename: "app.ts" },
  ];
  const pastedAttachments: PastedAttachment[] = [
    { id: "paste_1", label: "Pasted text · 80 lines", lineCount: 80, content: "const x = 1;" },
  ];
  const diffQuotes: DiffQuoteSnippet[] = [
    { id: "quote_1", path: "src/app.ts", lineNumber: 12, code: "return ok;" },
  ];
  return {
    draft: "Please inspect @app.ts carefully",
    slashDirective: "plan" as const,
    inlineMentions,
    contextItems,
    pastedAttachments,
    diffQuotes,
    sideQuestionDraft: "Quick side question",
  };
}

describe("Wave 1A P0 UX Safety", () => {
  it("P0-1: validates installed providers BEFORE clearing draft state on new session submit", async (t) => {
    const { orbitActions, getOrbitState, composerScopeKey } = await server.ssrLoadModule(
      "/src/state/orbit-store.ts",
    );
    const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

    t.mock.method(orbitIpc, "composePrompt", async () => ({
      wire_prompt: "Please inspect @app.ts carefully",
      display_prompt: "Please inspect @app.ts carefully",
      effective_directive: "plan",
      badges: [],
      is_empty: false,
      is_fork: false,
      is_btw: false,
      is_compact: false,
      fast_mode_action: null,
    }));
    const createMock = t.mock.method(orbitIpc, "createAgent", async () => "agent-new");

    const state = getOrbitState();
    Object.assign(state, {
      selectedHostId: "host-no-cli",
      selectedAgentId: null,
      isDraftNewSession: true,
      selectedWorkspaceCwd: "/repo",
      toastNotice: null,
      hostDetails: {
        "host-no-cli": {
          host: {
            ...makeHostCard("host-no-cli"),
            available_providers: [],
          },
          agents: [],
          acp_agents: [],
          discovered_projects: [],
          workspaces: [],
        },
      },
    });

    const scopeKey = composerScopeKey(getOrbitState());
    const fx = makeRichDraftFixture();
    Object.assign(getOrbitState(), {
      composerDrafts: { [scopeKey]: fx.draft },
      slashDirectives: { [scopeKey]: fx.slashDirective },
      inlineMentions: { [scopeKey]: fx.inlineMentions },
      contextItems: { [scopeKey]: fx.contextItems },
      pastedAttachments: { [scopeKey]: fx.pastedAttachments },
      diffQuotes: { [scopeKey]: fx.diffQuotes },
    });

    await orbitActions.submitComposer(false);

    const afterState = getOrbitState();
    assert.equal(createMock.mock.callCount(), 0);
    assert.equal(afterState.composerDrafts[scopeKey], fx.draft);
    assert.equal(afterState.slashDirectives[scopeKey], fx.slashDirective);
    assert.deepEqual(afterState.inlineMentions[scopeKey], fx.inlineMentions);
    assert.deepEqual(afterState.contextItems[scopeKey], fx.contextItems);
    assert.deepEqual(afterState.pastedAttachments[scopeKey], fx.pastedAttachments);
    assert.deepEqual(afterState.diffQuotes[scopeKey], fx.diffQuotes);
    assert.ok(afterState.toastNotice, "expected missing CLI toast");
  });

  it("P0-1: restores all cleared slices and merges new user input when sendPrompt rejects", async (t) => {
    const { orbitActions, getOrbitState, composerScopeKey } = await server.ssrLoadModule(
      "/src/state/orbit-store.ts",
    );
    const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

    t.mock.method(orbitIpc, "composePrompt", async () => ({
      wire_prompt: "Please inspect @app.ts carefully",
      display_prompt: "Please inspect @app.ts carefully",
      effective_directive: "plan",
      badges: [],
      is_empty: false,
      is_fork: false,
      is_btw: false,
      is_compact: false,
      fast_mode_action: null,
    }));

    let rejectSend!: (err: Error) => void;
    t.mock.method(
      orbitIpc,
      "sendPrompt",
      () =>
        new Promise<void>((_resolve, reject) => {
          rejectSend = reject;
        }),
    );

    Object.assign(getOrbitState(), {
      selectedHostId: "local",
      selectedAgentId: "agent-1",
      isDraftNewSession: false,
      selectedWorkspaceCwd: "/repo",
      toastNotice: null,
      hostDetails: {
        local: {
          host: makeHostCard("local"),
          agents: [makeAgentCard("agent-1")],
          acp_agents: [],
          discovered_projects: [],
          workspaces: [],
        },
      },
    });

    const scopeKey = composerScopeKey(getOrbitState());
    const fx = makeRichDraftFixture();
    Object.assign(getOrbitState(), {
      composerDrafts: { [scopeKey]: fx.draft },
      slashDirectives: { [scopeKey]: fx.slashDirective },
      inlineMentions: { [scopeKey]: fx.inlineMentions },
      contextItems: { [scopeKey]: fx.contextItems },
      pastedAttachments: { [scopeKey]: fx.pastedAttachments },
      diffQuotes: { [scopeKey]: fx.diffQuotes },
    });

    const submitPromise = orbitActions.submitComposer(false);
    // Allow composePrompt microtask to settle and clear draft before sendPrompt rejects
    await new Promise((r) => setTimeout(r, 10));
    assert.equal(getOrbitState().composerDrafts[scopeKey], "");

    // User starts typing something new while sendPrompt is still in flight
    orbitActions.setComposerDraft("Also check edge cases");

    rejectSend(new Error("daemon disconnected"));
    await submitPromise;

    const afterState = getOrbitState();
    assert.equal(
      afterState.composerDrafts[scopeKey],
      `${fx.draft}\n\nAlso check edge cases`,
    );
    assert.equal(afterState.slashDirectives[scopeKey], fx.slashDirective);
    assert.deepEqual(afterState.inlineMentions[scopeKey], fx.inlineMentions);
    assert.deepEqual(afterState.contextItems[scopeKey], fx.contextItems);
    assert.deepEqual(afterState.pastedAttachments[scopeKey], fx.pastedAttachments);
    assert.deepEqual(afterState.diffQuotes[scopeKey], fx.diffQuotes);
    assert.match(String(afterState.toastNotice), /daemon disconnected/);
  });

  it("P0-1: restores draft slices and surfaces toast when createAgent or enqueuePrompt rejects", async (t) => {
    const { orbitActions, getOrbitState, composerScopeKey } = await server.ssrLoadModule(
      "/src/state/orbit-store.ts",
    );
    const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

    t.mock.method(orbitIpc, "composePrompt", async () => ({
      wire_prompt: "Draft prompt",
      display_prompt: "Draft prompt",
      effective_directive: null,
      badges: [],
      is_empty: false,
      is_fork: false,
      is_btw: false,
      is_compact: false,
      fast_mode_action: null,
    }));
    t.mock.method(orbitIpc, "resolveModelSelection", async () => null);
    t.mock.method(orbitIpc, "createAgent", async () => {
      throw new Error("spawn failed");
    });
    t.mock.method(orbitIpc, "enqueuePrompt", async () => {
      throw new Error("queue rejected");
    });

    // 1) createAgent failure
    Object.assign(getOrbitState(), {
      selectedHostId: "local",
      selectedAgentId: null,
      isDraftNewSession: true,
      selectedWorkspaceCwd: "/repo",
      toastNotice: null,
      hostDetails: {
        local: {
          host: makeHostCard("local"),
          agents: [],
          acp_agents: [],
          discovered_projects: [],
          workspaces: [],
        },
      },
    });
    const draftScope = composerScopeKey(getOrbitState());
    const fx = makeRichDraftFixture();
    Object.assign(getOrbitState(), {
      composerDrafts: { [draftScope]: fx.draft },
      slashDirectives: { [draftScope]: fx.slashDirective },
      inlineMentions: { [draftScope]: fx.inlineMentions },
      contextItems: { [draftScope]: fx.contextItems },
      pastedAttachments: { [draftScope]: fx.pastedAttachments },
      diffQuotes: { [draftScope]: fx.diffQuotes },
    });

    await orbitActions.submitComposer(false);
    assert.equal(getOrbitState().composerDrafts[draftScope], fx.draft);
    assert.deepEqual(getOrbitState().contextItems[draftScope], fx.contextItems);
    assert.match(String(getOrbitState().toastNotice), /spawn failed/);

    // 2) enqueuePrompt failure on busy agent with server queue
    Object.assign(getOrbitState(), {
      selectedHostId: "local",
      selectedAgentId: "busy-1",
      isDraftNewSession: false,
      selectedWorkspaceCwd: "/repo",
      toastNotice: null,
      hostDetails: {
        local: {
          host: makeHostCard("local"),
          agents: [makeAgentCard("busy-1", { is_running: true, status_label: "Running" })],
          acp_agents: [],
          discovered_projects: [],
          workspaces: [],
        },
      },
    });
    const busyScope = composerScopeKey(getOrbitState());
    Object.assign(getOrbitState(), {
      composerDrafts: { [busyScope]: fx.draft },
      slashDirectives: { [busyScope]: fx.slashDirective },
      inlineMentions: { [busyScope]: fx.inlineMentions },
      contextItems: { [busyScope]: fx.contextItems },
      pastedAttachments: { [busyScope]: fx.pastedAttachments },
      diffQuotes: { [busyScope]: fx.diffQuotes },
    });

    await orbitActions.submitComposer(false);
    assert.equal(getOrbitState().composerDrafts[busyScope], fx.draft);
    assert.deepEqual(getOrbitState().pastedAttachments[busyScope], fx.pastedAttachments);
    assert.deepEqual(getOrbitState().diffQuotes[busyScope], fx.diffQuotes);
    assert.match(String(getOrbitState().toastNotice), /queue rejected/);
  });

  it("P0-4: archiveThread snapshots drafts into lastArchivedUndo and undoLastArchive restores all draft slices", async (t) => {
    const { orbitActions, getOrbitState, threadScopeKey } = await server.ssrLoadModule(
      "/src/state/orbit-store.ts",
    );
    const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

    t.mock.method(orbitIpc, "archiveAgent", async () => {});
    t.mock.method(orbitIpc, "unarchiveAgent", async () => {});
    t.mock.method(orbitIpc, "openAgentTimeline", async () => {});
    t.mock.method(orbitIpc, "markAgentRead", async () => {});
    t.mock.method(orbitIpc, "listProviderSubagents", async () => {});
    t.mock.method(orbitActions, "pullStageDelta", async () => {});
    t.mock.method(orbitActions, "refreshSidebarView", () => {});

    const tKey = threadScopeKey("local", "agent-arch");
    const fx = makeRichDraftFixture();
    Object.assign(getOrbitState(), {
      selectedHostId: "local",
      selectedAgentId: "agent-arch",
      isDraftNewSession: false,
      archivedThreads: {},
      lastArchivedUndo: null,
      hostDetails: {
        local: {
          host: makeHostCard("local"),
          agents: [makeAgentCard("agent-arch")],
          acp_agents: [],
          discovered_projects: [],
          workspaces: [],
        },
      },
      composerDrafts: { [tKey]: fx.draft },
      slashDirectives: { [tKey]: fx.slashDirective },
      inlineMentions: { [tKey]: fx.inlineMentions },
      contextItems: { [tKey]: fx.contextItems },
      pastedAttachments: { [tKey]: fx.pastedAttachments },
      diffQuotes: { [tKey]: fx.diffQuotes },
      sideQuestionDrafts: { [tKey]: fx.sideQuestionDraft },
    });

    orbitActions.archiveThread("local", "agent-arch");

    const archivedState = getOrbitState();
    assert.ok(archivedState.archivedThreads[tKey]);
    assert.equal(archivedState.composerDrafts[tKey], undefined);
    assert.equal(archivedState.slashDirectives[tKey], undefined);
    assert.equal(archivedState.inlineMentions[tKey], undefined);
    assert.equal(archivedState.contextItems[tKey], undefined);
    assert.equal(archivedState.pastedAttachments[tKey], undefined);
    assert.equal(archivedState.diffQuotes[tKey], undefined);
    assert.equal(archivedState.sideQuestionDrafts[tKey], undefined);
    assert.equal(archivedState.lastArchivedUndo?.draft, fx.draft);
    assert.equal(archivedState.lastArchivedUndo?.sideQuestionDraft, fx.sideQuestionDraft);

    orbitActions.undoLastArchive();

    const restoredState = getOrbitState();
    assert.equal(restoredState.archivedThreads[tKey], undefined);
    assert.equal(restoredState.lastArchivedUndo, null);
    assert.equal(restoredState.composerDrafts[tKey], fx.draft);
    assert.equal(restoredState.slashDirectives[tKey], fx.slashDirective);
    assert.deepEqual(restoredState.inlineMentions[tKey], fx.inlineMentions);
    assert.deepEqual(restoredState.contextItems[tKey], fx.contextItems);
    assert.deepEqual(restoredState.pastedAttachments[tKey], fx.pastedAttachments);
    assert.deepEqual(restoredState.diffQuotes[tKey], fx.diffQuotes);
    assert.equal(restoredState.sideQuestionDrafts[tKey], fx.sideQuestionDraft);
  });

  it("P0-4: SidebarConversationRow hides 1-click hover Archive on running or awaiting-approval rows", async () => {
    const { canQuickArchiveThread } = await server.ssrLoadModule(
      "/src/components/sidebar/sidebar-conversation-row.tsx",
    );

    assert.equal(canQuickArchiveThread(makeAgentCard("idle-row")), true);
    assert.equal(
      canQuickArchiveThread(
        makeAgentCard("running-row", { is_running: true, status_label: "Running" }),
      ),
      false,
    );
    assert.equal(
      canQuickArchiveThread(
        makeAgentCard("approval-row", { is_awaiting_permission: true }),
      ),
      false,
    );
    assert.equal(
      canQuickArchiveThread(
        makeAgentCard("both-row", {
          is_running: true,
          status_label: "Running",
          is_awaiting_permission: true,
        }),
      ),
      false,
    );
  });

  it("P0-8: allows harmless Side Panel tab switching while dirty, but guards destructive actions", async (t) => {
    const { orbitActions, getOrbitState, fileCacheKey } = await server.ssrLoadModule(
      "/src/state/orbit-store.ts",
    );
    const { orbitIpc } = await server.ssrLoadModule("/src/lib/orbit-ipc.ts");

    t.mock.method(orbitIpc, "readWorkspaceFile", async (_h: string, _c: string, path: string) => ({
      resolved_path: path,
      content_kind: "text",
      content: "hello",
      size_bytes: 5,
      truncated: false,
      writable: true,
      line_ending: "lf",
      etag: "etag-1",
      mtime_unix_ms: 1,
      language: "typescript",
      highlighted_lines: [],
      markdown_blocks: [],
    }));
    t.mock.method(orbitActions, "ensureWorkspaceFiles", async () => {});

    const hostId = "local";
    const cwd = "/repo";
    const dirtyPath = "src/dirty.ts";
    const dirtyKey = fileCacheKey(hostId, cwd, dirtyPath);
    const dirtyTabId = `tab:file:${hostId}::${cwd}::${dirtyPath}`;

    Object.assign(getOrbitState(), {
      selectedHostId: hostId,
      selectedWorkspaceCwd: cwd,
      sidePanelOpen: true,
      pendingUnsavedFileAction: null,
      sidePanelTabs: [
        { id: dirtyTabId, kind: "file", title: "dirty.ts", filePath: dirtyPath, hostId, cwd },
        { id: "tab:files", kind: "files", title: "Files" },
      ],
      activeSidePanelTabId: dirtyTabId,
      fileEditorDrafts: {
        [dirtyKey]: {
          hostId,
          cwd,
          path: dirtyPath,
          content: "modified content",
          baseEtag: "etag-1",
          baseContent: "original content",
          lineEnding: "lf",
          dirty: true,
          mode: "edit",
          saving: false,
          saveError: null,
          conflict: null,
        },
      },
    });

    // 1) focusSidePanelTab switches immediately and preserves dirty draft
    orbitActions.focusSidePanelTab("tab:files");
    assert.equal(getOrbitState().activeSidePanelTabId, "tab:files");
    assert.equal(getOrbitState().pendingUnsavedFileAction, null);
    assert.equal(getOrbitState().fileEditorDrafts[dirtyKey]?.dirty, true);

    // 2) openSidePanelNewTab switches to launchpad immediately and preserves dirty draft
    orbitActions.focusSidePanelTab(dirtyTabId);
    orbitActions.openSidePanelNewTab();
    assert.equal(getOrbitState().activeSidePanelTabId, null);
    assert.equal(getOrbitState().pendingUnsavedFileAction, null);
    assert.equal(getOrbitState().fileEditorDrafts[dirtyKey]?.dirty, true);

    // 3) openFileInSidePanel opens another file immediately and preserves dirty draft
    orbitActions.focusSidePanelTab(dirtyTabId);
    orbitActions.openFileInSidePanel("src/other.ts", { hostId, cwd });
    assert.equal(
      getOrbitState().activeSidePanelTabId,
      `tab:file:${hostId}::${cwd}::src/other.ts`,
    );
    assert.equal(getOrbitState().pendingUnsavedFileAction, null);
    assert.equal(getOrbitState().fileEditorDrafts[dirtyKey]?.dirty, true);

    // 4) Destructive actions STILL trigger pendingUnsavedFileAction
    orbitActions.closeSidePanelTab(dirtyTabId);
    assert.equal(getOrbitState().pendingUnsavedFileAction?.action.kind, "close_tab");

    orbitActions.requestFileViewMode(hostId, cwd, dirtyPath);
    assert.equal(getOrbitState().pendingUnsavedFileAction?.action.kind, "switch_to_view");

    orbitActions.requestFileReload(hostId, cwd, dirtyPath);
    assert.equal(getOrbitState().pendingUnsavedFileAction?.action.kind, "reload");
  });

  it("P0-6: resolveComposerEnterAction honors enterToSend and activeTurnSendBehavior (queue vs steer)", async () => {
    const { resolveComposerEnterAction } = await server.ssrLoadModule(
      "/src/components/composer/omni-composer.tsx",
    );
    const key = (overrides: Partial<KeyboardEvent> = {}) => ({
      key: "Enter",
      metaKey: false,
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
      ...overrides,
    });

    // Default / "queue" mode, enterToSend === false, busy turn
    assert.deepEqual(
      resolveComposerEnterAction(key(), false, true),
      { submit: false, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ metaKey: true }), false, true),
      { submit: true, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ ctrlKey: true }), false, true, "queue"),
      { submit: true, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ metaKey: true, shiftKey: true }), false, true),
      { submit: true, steerImmediately: true },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ ctrlKey: true, shiftKey: true }), false, true, "queue"),
      { submit: true, steerImmediately: true },
    );

    // Default / "queue" mode, enterToSend === true, busy turn
    assert.deepEqual(
      resolveComposerEnterAction(key(), true, true),
      { submit: true, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ shiftKey: true }), true, true),
      { submit: false, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ metaKey: true }), true, true),
      { submit: true, steerImmediately: true },
    );

    // "steer" mode, enterToSend === true, busy turn: primary Enter steers, Mod+Enter queues
    assert.deepEqual(
      resolveComposerEnterAction(key(), true, true, "steer"),
      { submit: true, steerImmediately: true },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ shiftKey: true }), true, true, "steer"),
      { submit: false, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ metaKey: true }), true, true, "steer"),
      { submit: true, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ ctrlKey: true }), true, true, "steer"),
      { submit: true, steerImmediately: false },
    );

    // "steer" mode, enterToSend === false, busy turn: primary Mod+Enter steers, Mod+Shift+Enter queues
    assert.deepEqual(
      resolveComposerEnterAction(key(), false, true, "steer"),
      { submit: false, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ metaKey: true }), false, true, "steer"),
      { submit: true, steerImmediately: true },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ ctrlKey: true }), false, true, "steer"),
      { submit: true, steerImmediately: true },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ metaKey: true, shiftKey: true }), false, true, "steer"),
      { submit: true, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ ctrlKey: true, shiftKey: true }), false, true, "steer"),
      { submit: true, steerImmediately: false },
    );

    // Idle turn (isTurnBusy === false): never steers in either mode
    assert.deepEqual(
      resolveComposerEnterAction(key(), true, false, "steer"),
      { submit: true, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ metaKey: true }), true, false, "queue"),
      { submit: true, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ metaKey: true }), false, false, "steer"),
      { submit: true, steerImmediately: false },
    );
    assert.deepEqual(
      resolveComposerEnterAction(key({ metaKey: true, shiftKey: true }), false, false, "queue"),
      { submit: true, steerImmediately: false },
    );
  });

  it("P1-4: describePlan allows dismissing stopped or failed current-turn plans", () => {
    const pausedSteps: SessionPlanStepView[] = [
      { id: "1", title: "Step 1", active_form: "Working", status: "in_progress", updated_seq: 5 },
      { id: "2", title: "Step 2", active_form: "Next", status: "pending", updated_seq: 5 },
    ];
    const failedSteps: SessionPlanStepView[] = [
      { id: "1", title: "Step 1", active_form: "Working", status: "failed", updated_seq: 5 },
    ];

    assert.equal(describePlan(pausedSteps, true, 1)?.canDismiss, false);
    assert.equal(describePlan(pausedSteps, false, 1)?.canDismiss, true);
    assert.equal(describePlan(failedSteps, false, 1)?.canDismiss, true);
  });
});
