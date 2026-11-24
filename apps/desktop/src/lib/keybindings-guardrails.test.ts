import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { after, before, describe, it } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import {
  hasOpenSettingsBlockingOverlay,
  isFocusInsideSidePanel,
  SETTINGS_ESCAPE_OVERLAY_SELECTOR,
  shouldCloseSettingsOnEscape,
  shouldSubmitQuestionDockOnModEnter,
  SIDE_PANEL_SELECTOR,
  type QuestionDockModEnterContext,
} from "./file-viewer-shortcuts.ts";

const DESKTOP_SRC = resolve(import.meta.dirname, "..");

let server: ViteDevServer;

before(async () => {
  server = await createServer({
    server: { middlewareMode: true, hmr: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [
      {
        name: "test-guardrails-mocks",
        enforce: "pre",
        load(id) {
          if (id.endsWith("/state/orbit-store.ts")) {
            return `
              export const orbitActions = {
                showToast: () => {},
                openFileInSidePanel: () => {},
                openSubagentTab: () => {},
              };
              export const useOrbitSelector = () => null;
            `;
          }
          if (id.endsWith("/lib/orbit-ipc.ts")) {
            return `
              export const isMacDesktop = () => true;
              export const orbitIpc = {
                resolvePermission: async () => {},
                answerQuestion: async () => {},
              };
            `;
          }
        },
      },
    ],
  });
});

after(async () => {
  await server?.close();
});

function makeModEnterCtx(
  overrides: Partial<QuestionDockModEnterContext> = {},
): QuestionDockModEnterContext {
  return {
    registerGlobalShortcuts: true,
    canSubmit: true,
    submitting: false,
    confirmDialogOpen: false,
    modalDialogOpen: false,
    event: {
      key: "Enter",
      metaKey: true,
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
      repeat: false,
      defaultPrevented: false,
      isComposing: false,
    },
    focusInsideQuestionCard: false,
    focusInsideComposer: false,
    focusInsideSidePanel: false,
    focusInsideOtherEditable: false,
    ...overrides,
  };
}

describe("P0-3: ConfirmDialog destructive default focus & migrations", () => {
  it("defaults tone='danger' to cancel focus while keeping tone='default' on confirm", async () => {
    const mod = await server.ssrLoadModule("/src/components/ui/confirm-dialog.tsx");
    assert.equal(mod.resolveConfirmDefaultAction("danger"), "cancel");
    assert.equal(mod.resolveConfirmDefaultAction("default"), "confirm");
    assert.equal(mod.resolveConfirmDefaultAction("danger", "secondary"), "secondary");
    assert.equal(mod.resolveConfirmDefaultAction("danger", "confirm"), "confirm");
    assert.equal(mod.resolveConfirmDefaultAction("default", "cancel"), "cancel");
  });

  it("migrates sidebar delete/group destructive actions and Empty Archive onto ConfirmDialog tone='danger'", () => {
    const sidebarMenusSrc = readFileSync(
      resolve(DESKTOP_SRC, "components/sidebar/sidebar-menus.tsx"),
      "utf8",
    );
    const archivedSectionSrc = readFileSync(
      resolve(DESKTOP_SRC, "components/settings/archived-section.tsx"),
      "utf8",
    );

    // DeleteConversationDialog, DeleteProjectDialog, and SharedWorkspaceMenu all use ConfirmDialog tone="danger"
    const confirmMatches = sidebarMenusSrc.match(/<ConfirmDialog\b[\s\S]*?tone="danger"/g) ?? [];
    assert.ok(
      confirmMatches.length >= 3,
      `Expected at least 3 destructive ConfirmDialog instances in sidebar-menus.tsx, found ${confirmMatches.length}`,
    );
    assert.doesNotMatch(sidebarMenusSrc, /variant="destructive"[\s\S]{0,80}autoFocus/);
    assert.match(sidebarMenusSrc, /kind:\s*"archive_all"/);
    assert.match(sidebarMenusSrc, /kind:\s*"merge"/);
    assert.match(sidebarMenusSrc, /kind:\s*"delete"/);

    // ArchivedSection Empty Archive uses ConfirmDialog tone="danger" without autoFocus on destructive button
    assert.match(archivedSectionSrc, /<ConfirmDialog\b[\s\S]*?tone="danger"/);
    assert.doesNotMatch(archivedSectionSrc, /variant="destructive"[\s\S]{0,80}autoFocus/);
  });
});

describe("P0-6: Global Cmd+F yield & Settings Escape overlay guardrails", () => {
  it("yields global Cmd+F whenever focus or target is inside the Right Side Panel / File Viewer", () => {
    const body = { tagName: "BODY" } as unknown as Element;
    const html = { tagName: "HTML" } as unknown as Element;
    const sidePanelChild = {
      closest: (sel: string) => (sel === SIDE_PANEL_SELECTOR ? sidePanelChild : null),
    } as unknown as Element;
    const mainStageEl = {
      closest: () => null,
    } as unknown as Element;
    const doc = { body, documentElement: html, querySelector: () => null };

    assert.equal(isFocusInsideSidePanel(mainStageEl, sidePanelChild, doc), true);
    assert.equal(isFocusInsideSidePanel(sidePanelChild, body, doc), true);
    assert.equal(isFocusInsideSidePanel(mainStageEl, mainStageEl, doc), false);
    assert.equal(isFocusInsideSidePanel(body, body, doc), false);

    const keybindingsSrc = readFileSync(resolve(DESKTOP_SRC, "lib/keybindings.ts"), "utf8");
    const yieldIdx = keybindingsSrc.indexOf("isFocusInsideSidePanel");
    const focusFilterIdx = keybindingsSrc.indexOf("focusSidebarFilter");
    assert.ok(yieldIdx > 0 && focusFilterIdx > yieldIdx);
  });

  it("blocks Settings Escape whenever any dialog, alertdialog, popover, menu, or listbox is open", () => {
    for (const selector of [
      '[data-slot="dialog-content"]',
      '[role="dialog"]',
      '[role="alertdialog"]',
      '[data-slot="popover-content"]',
      '[role="menu"]',
      '[role="listbox"]',
    ]) {
      assert.ok(
        SETTINGS_ESCAPE_OVERLAY_SELECTOR.includes(selector),
        `Expected SETTINGS_ESCAPE_OVERLAY_SELECTOR to include ${selector}`,
      );
    }

    assert.equal(
      hasOpenSettingsBlockingOverlay({
        querySelector: (sel) => (sel === SETTINGS_ESCAPE_OVERLAY_SELECTOR ? {} : null),
      }),
      true,
    );
    assert.equal(hasOpenSettingsBlockingOverlay({ querySelector: () => null }), false);

    // Bail out when an inner overlay is open
    assert.equal(
      shouldCloseSettingsOnEscape({
        key: "Escape",
        activeView: "settings",
        commandPaletteOpen: false,
        targetTagName: "DIV",
        hasBlockingOverlay: true,
      }),
      false,
    );

    // Bail out when target is input/textarea, command palette open, or event already handled
    assert.equal(
      shouldCloseSettingsOnEscape({
        key: "Escape",
        activeView: "settings",
        commandPaletteOpen: false,
        targetTagName: "INPUT",
        hasBlockingOverlay: false,
      }),
      false,
    );
    assert.equal(
      shouldCloseSettingsOnEscape({
        key: "Escape",
        activeView: "settings",
        commandPaletteOpen: true,
        targetTagName: "DIV",
        hasBlockingOverlay: false,
      }),
      false,
    );
    assert.equal(
      shouldCloseSettingsOnEscape({
        key: "Escape",
        defaultPrevented: true,
        activeView: "settings",
        commandPaletteOpen: false,
        targetTagName: "DIV",
        hasBlockingOverlay: false,
      }),
      false,
    );

    // Close Settings cleanly when no inner overlay or input owns Escape
    assert.equal(
      shouldCloseSettingsOnEscape({
        key: "Escape",
        activeView: "settings",
        commandPaletteOpen: false,
        targetTagName: "DIV",
        hasBlockingOverlay: false,
      }),
      true,
    );
  });
});

describe("P0-6 + P0-7 Prep: DecisionDock question submit, custom answer preservation & Side Panel props", () => {
  it("submits active StickyQuestionDockCard on Cmd/Ctrl+Enter without stealing from OmniComposer or Side Panel", () => {
    // 1–9 → Cmd+Enter works when focus is outside inputs (e.g. body/stage) or inside the question card
    assert.equal(shouldSubmitQuestionDockOnModEnter(makeModEnterCtx()), true);
    assert.equal(
      shouldSubmitQuestionDockOnModEnter(
        makeModEnterCtx({
          event: { key: "Enter", metaKey: false, ctrlKey: true, shiftKey: false, altKey: false },
        }),
      ),
      true,
    );
    assert.equal(
      shouldSubmitQuestionDockOnModEnter(makeModEnterCtx({ focusInsideQuestionCard: true })),
      true,
    );

    // Never steals Cmd+Enter from OmniComposer, Side Panel, other inputs, or dialogs
    assert.equal(
      shouldSubmitQuestionDockOnModEnter(makeModEnterCtx({ focusInsideComposer: true })),
      false,
    );
    assert.equal(
      shouldSubmitQuestionDockOnModEnter(makeModEnterCtx({ focusInsideSidePanel: true })),
      false,
    );
    assert.equal(
      shouldSubmitQuestionDockOnModEnter(makeModEnterCtx({ focusInsideOtherEditable: true })),
      false,
    );
    assert.equal(
      shouldSubmitQuestionDockOnModEnter(makeModEnterCtx({ confirmDialogOpen: true })),
      false,
    );
    assert.equal(
      shouldSubmitQuestionDockOnModEnter(makeModEnterCtx({ modalDialogOpen: true })),
      false,
    );

    // Disabled completely when DecisionDock is mounted with registerGlobalShortcuts={false}
    assert.equal(
      shouldSubmitQuestionDockOnModEnter(makeModEnterCtx({ registerGlobalShortcuts: false })),
      false,
    );
  });

  it("preserves custom typed answer when clicking a preset option on a single-select question", async () => {
    const mod = await server.ssrLoadModule("/src/components/stage/stage-approval-dock.tsx");
    const singleQ = [{ is_multi_select: false }];

    // 1. User types custom answer
    let draft = mod.applyQuestionCustomChange(
      mod.EMPTY_QUESTION_DRAFT,
      0,
      "Use SQLite WAL mode",
      false,
    );
    assert.equal(draft.customAnswers[0], "Use SQLite WAL mode");
    assert.equal(draft.useCustom[0], true);
    assert.deepEqual(mod.resolveQuestionAnswers(singleQ, draft), [["Use SQLite WAL mode"]]);

    // 2. User clicks a preset option — preset becomes active, custom text stays intact!
    draft = mod.applyQuestionOptionToggle(draft, 0, "Postgres", false);
    assert.equal(draft.customAnswers[0], "Use SQLite WAL mode");
    assert.equal(draft.useCustom[0], false);
    assert.deepEqual(draft.selections[0], ["Postgres"]);
    assert.deepEqual(mod.resolveQuestionAnswers(singleQ, draft), [["Postgres"]]);

    // 3. User clicks back into the custom input — preserved custom answer is immediately reactivated!
    draft = mod.activateQuestionCustomAnswer(draft, 0, false);
    assert.equal(draft.customAnswers[0], "Use SQLite WAL mode");
    assert.equal(draft.useCustom[0], true);
    assert.deepEqual(draft.selections[0], []);
    assert.deepEqual(mod.resolveQuestionAnswers(singleQ, draft), [["Use SQLite WAL mode"]]);

    // 4. Multi-select combines preset selections with custom answer cleanly
    const multiQ = [{ is_multi_select: true }];
    let multiDraft = mod.applyQuestionOptionToggle(mod.EMPTY_QUESTION_DRAFT, 0, "Unit", true);
    multiDraft = mod.applyQuestionCustomChange(multiDraft, 0, "Smoke", true);
    assert.deepEqual(mod.resolveQuestionAnswers(multiQ, multiDraft), [["Unit", "Smoke"]]);
  });

  it("supports compact spacing and registerGlobalShortcuts={false} for Side Panel DecisionDock", async () => {
    const mod = await server.ssrLoadModule("/src/components/stage/stage-approval-dock.tsx");
    const samplePermissions = [
      {
        item_type: "permission_request",
        seq: 1,
        request_id: "req-1",
        title: "Run tests",
        detail: { kind: "shell", command: "npm test", cwd: "/repo" },
        allowed_decisions: ["allow", "allow_always", "deny"],
        resolved_decision: null,
        guardian_assessment: null,
      },
    ];

    const defaultHtml = renderToStaticMarkup(
      createElement(mod.DecisionDock, {
        hostId: "local",
        agentId: "ag-1",
        cwd: "/repo",
        permissions: samplePermissions,
        questions: [],
      }),
    );
    assert.match(defaultHtml, /px-\[24px\]/);
    assert.match(defaultHtml, /data-slot="kbd-group"/);
    assert.match(defaultHtml, />⌘<\/kbd><kbd[^>]*>Y<\/kbd>/);

    const compactSidePanelHtml = renderToStaticMarkup(
      createElement(mod.DecisionDock, {
        hostId: "local",
        agentId: "ag-1",
        cwd: "/repo",
        permissions: samplePermissions,
        questions: [],
        compact: true,
        registerGlobalShortcuts: false,
      }),
    );
    assert.match(compactSidePanelHtml, /\bpx-3\b/);
    assert.doesNotMatch(compactSidePanelHtml, /px-\[24px\]/);
    assert.doesNotMatch(compactSidePanelHtml, /data-slot="kbd-group"/);

    // Also verify StickyQuestionDockCard suppresses digit shortcut hints when registerGlobalShortcuts={false}
    const sampleQuestions = [
      {
        item_type: "question_request",
        seq: 2,
        request_id: "q-1",
        questions: [
          {
            id: "q0",
            header: "Storage",
            question: "Which engine?",
            options: ["SQLite", "Postgres"],
            is_multi_select: false,
          },
        ],
      },
    ];
    const mainQuestionHtml = renderToStaticMarkup(
      createElement(mod.DecisionDock, {
        hostId: "local",
        agentId: "ag-1",
        cwd: "/repo",
        permissions: [],
        questions: sampleQuestions,
      }),
    );
    assert.match(mainQuestionHtml, /Press 1–2 to select/);

    const sideQuestionHtml = renderToStaticMarkup(
      createElement(mod.DecisionDock, {
        hostId: "local",
        agentId: "ag-1",
        cwd: "/repo",
        permissions: [],
        questions: sampleQuestions,
        compact: true,
        registerGlobalShortcuts: false,
      }),
    );
    assert.match(sideQuestionHtml, /\bpx-3\b/);
    assert.doesNotMatch(sideQuestionHtml, /Press 1–2 to select/);
  });
});
