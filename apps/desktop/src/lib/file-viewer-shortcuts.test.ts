import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  type FileViewerShortcutContext,
  type FileViewerShortcutEventLike,
  resolveFileViewerShortcut,
} from "./file-viewer-shortcuts.ts";

function makeCtx(overrides: Partial<FileViewerShortcutContext> = {}): FileViewerShortcutContext {
  return {
    activeView: "workspace",
    sidePanelOpen: true,
    activeSidePanelTabId: "file-1",
    tabId: "file-1",
    terminalFocused: false,
    commandPaletteOpen: false,
    quickOpenFilesOpen: false,
    mediaPreviewOpen: false,
    focusInsideSidePanel: true,
    targetInsideModalDialog: false,
    canFindInFile: true,
    isEditable: true,
    isEditMode: false,
    findOpen: false,
    ...overrides,
  };
}

function makeKey(
  overrides: Partial<FileViewerShortcutEventLike> = {},
): FileViewerShortcutEventLike {
  return {
    key: "f",
    metaKey: true,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    isComposing: false,
    defaultPrevented: false,
    ...overrides,
  };
}

describe("file-viewer-shortcuts scope isolation", () => {
  it("handles Cmd/Ctrl+F and Cmd/Ctrl+E when workspace is active and focus is inside side panel", () => {
    assert.equal(resolveFileViewerShortcut(makeCtx(), makeKey({ key: "f", metaKey: true })), "find");
    assert.equal(
      resolveFileViewerShortcut(makeCtx(), makeKey({ key: "F", metaKey: false, ctrlKey: true })),
      "find",
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx(), makeKey({ key: "e", metaKey: true })),
      "toggle_edit",
    );
  });

  it("never hijacks Cmd+F or Cmd+E when focus is outside the File Viewer / Side Panel (Composer, StreamStage, Sidebar, Automations)", () => {
    const outsideFocus = makeCtx({ focusInsideSidePanel: false });
    assert.equal(resolveFileViewerShortcut(outsideFocus, makeKey({ key: "f" })), null);
    assert.equal(resolveFileViewerShortcut(outsideFocus, makeKey({ key: "e" })), null);
    assert.equal(
      resolveFileViewerShortcut(
        makeCtx({ focusInsideSidePanel: false, isEditMode: true }),
        makeKey({ key: "s" }),
      ),
      null,
    );
    assert.equal(
      resolveFileViewerShortcut(
        makeCtx({ focusInsideSidePanel: false, findOpen: true }),
        makeKey({ key: "Escape", metaKey: false }),
      ),
      null,
    );
  });

  it("never hijacks shortcuts while Settings view or any overlay/modal dialog is active", () => {
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ activeView: "settings" }), makeKey({ key: "f" })),
      null,
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ activeView: "settings" }), makeKey({ key: "e" })),
      null,
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ targetInsideModalDialog: true }), makeKey({ key: "f" })),
      null,
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ commandPaletteOpen: true }), makeKey({ key: "f" })),
      null,
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ quickOpenFilesOpen: true }), makeKey({ key: "e" })),
      null,
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ mediaPreviewOpen: true }), makeKey({ key: "f" })),
      null,
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ terminalFocused: true }), makeKey({ key: "f" })),
      null,
    );
  });

  it("respects edit mode, read-only files, modifiers, IME composition, and Escape for find bar", () => {
    // In edit mode, Cmd+S saves and Cmd+F is left to the code editor
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ isEditMode: true }), makeKey({ key: "f" })),
      null,
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ isEditMode: true }), makeKey({ key: "s" })),
      "save",
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ isEditMode: true }), makeKey({ key: "e" })),
      "toggle_edit",
    );

    // Read-only files ignore Cmd+E
    assert.equal(
      resolveFileViewerShortcut(makeCtx({ isEditable: false }), makeKey({ key: "e" })),
      null,
    );

    // Shift/Alt modifiers or IME composition are ignored
    assert.equal(
      resolveFileViewerShortcut(makeCtx(), makeKey({ key: "f", shiftKey: true })),
      null,
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx(), makeKey({ key: "e", altKey: true })),
      null,
    );
    assert.equal(
      resolveFileViewerShortcut(makeCtx(), makeKey({ key: "f", isComposing: true })),
      null,
    );

    // Escape closes find bar only when open
    assert.equal(
      resolveFileViewerShortcut(
        makeCtx({ findOpen: true }),
        makeKey({ key: "Escape", metaKey: false }),
      ),
      "close_find",
    );
    assert.equal(
      resolveFileViewerShortcut(
        makeCtx({ findOpen: false }),
        makeKey({ key: "Escape", metaKey: false }),
      ),
      null,
    );
  });
});
