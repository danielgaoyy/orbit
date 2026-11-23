export type FileViewerShortcutAction = "find" | "toggle_edit" | "save" | "close_find" | null;

export interface FileViewerShortcutEventLike {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  isComposing?: boolean;
  defaultPrevented?: boolean;
}

export interface FileViewerShortcutContext {
  activeView: "workspace" | "settings";
  sidePanelOpen: boolean;
  activeSidePanelTabId: string | null;
  tabId: string;
  terminalFocused: boolean;
  commandPaletteOpen: boolean;
  quickOpenFilesOpen: boolean;
  mediaPreviewOpen: boolean;
  focusInsideSidePanel: boolean;
  targetInsideModalDialog: boolean;
  canFindInFile: boolean;
  isEditable: boolean;
  isEditMode: boolean;
  findOpen: boolean;
}

/**
 * Pure shortcut scope predicate for FileViewerTab (`Cmd/Ctrl+F`, `Cmd/Ctrl+E`,
 * `Cmd/Ctrl+S`, `Escape`). Strictly requires `activeView === "workspace"` AND
 * active focus inside the File Viewer / Side Panel container so shortcuts never
 * hijack Composer, StreamStage, Sidebar, Settings, Automations, or modal dialogs.
 */
export function resolveFileViewerShortcut(
  ctx: FileViewerShortcutContext,
  event: FileViewerShortcutEventLike,
): FileViewerShortcutAction {
  if (event.defaultPrevented || event.isComposing) {
    return null;
  }
  if (
    ctx.activeView !== "workspace" ||
    !ctx.sidePanelOpen ||
    ctx.activeSidePanelTabId !== ctx.tabId ||
    ctx.terminalFocused ||
    ctx.commandPaletteOpen ||
    ctx.quickOpenFilesOpen ||
    ctx.mediaPreviewOpen ||
    ctx.targetInsideModalDialog ||
    !ctx.focusInsideSidePanel
  ) {
    return null;
  }

  const mod = event.metaKey || event.ctrlKey;
  const key = event.key.toLowerCase();

  if (mod && !event.shiftKey && !event.altKey) {
    if (key === "f" && ctx.canFindInFile && !ctx.isEditMode) {
      return "find";
    }
    if (key === "e" && ctx.isEditable) {
      return "toggle_edit";
    }
    if (key === "s" && ctx.isEditMode) {
      return "save";
    }
    return null;
  }

  if (!mod && !event.shiftKey && !event.altKey && event.key === "Escape" && ctx.findOpen) {
    return "close_find";
  }

  return null;
}

function nodeInsideElement(container: HTMLElement | null, node: Node | null): boolean {
  if (!container || !node) return false;
  return container === node || container.contains(node);
}

function elementClosestSelector(node: Node | null, selector: string): Element | null {
  if (!node) return null;
  if (node instanceof Element) return node.closest(selector);
  return node.parentElement?.closest(selector) ?? null;
}

/**
 * Returns true ONLY when the active focus (or keyboard event target when activeElement
 * is inside the container) belongs to the File Viewer or its enclosing Side Panel.
 */
export function isFocusInsideFileViewerPane(
  fileViewerContainer: HTMLElement | null,
  eventTarget: EventTarget | null,
  activeElement: Element | null,
): boolean {
  const targetNode = eventTarget instanceof Node ? eventTarget : null;
  const activeNode = activeElement instanceof Node ? activeElement : null;

  // Prefer the real activeElement when focused on a specific non-body element so an event
  // dispatched on window/body never leaks across panes.
  const isSpecificActive =
    activeElement !== null &&
    activeElement !== document.body &&
    activeElement !== document.documentElement;

  const candidate = isSpecificActive ? activeNode : targetNode;
  if (!candidate) return false;

  if (nodeInsideElement(fileViewerContainer, candidate)) {
    return true;
  }

  const sidePanelEl =
    fileViewerContainer?.closest<HTMLElement>('[data-orbit-side-panel="true"]') ?? null;
  return nodeInsideElement(sidePanelEl, candidate);
}

/**
 * Guards against hijacking shortcuts while a modal dialog/alertdialog is open.
 */
export function isFocusInsideModalDialog(
  eventTarget: EventTarget | null,
  activeElement: Element | null,
): boolean {
  const targetNode = eventTarget instanceof Node ? eventTarget : null;
  const activeNode = activeElement instanceof Node ? activeElement : null;
  return Boolean(
    elementClosestSelector(activeNode, '[role="dialog"], [role="alertdialog"]') ||
      elementClosestSelector(targetNode, '[role="dialog"], [role="alertdialog"]'),
  );
}
