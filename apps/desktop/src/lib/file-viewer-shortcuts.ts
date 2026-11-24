export const SIDE_PANEL_SELECTOR = '[data-orbit-side-panel="true"]';
export const CODE_EDITOR_SELECTOR = '[data-orbit-code-editor="true"]';
export const COMPOSER_INPUT_SELECTOR = '[data-orbit-composer-input="true"]';
export const SETTINGS_ESCAPE_OVERLAY_SELECTOR =
  '[data-slot="dialog-content"], [role="dialog"], [role="alertdialog"], [data-slot="popover-content"], [role="menu"], [role="listbox"]';

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

interface ClosestCapableLike {
  closest?: (selector: string) => unknown;
  parentElement?: ClosestCapableLike | null;
}

function matchesClosestSelector(target: unknown, selector: string): boolean {
  if (!target || typeof target !== "object") return false;
  const node = target as ClosestCapableLike;
  if (typeof node.closest === "function") {
    return Boolean(node.closest(selector));
  }
  if (node.parentElement && typeof node.parentElement.closest === "function") {
    return Boolean(node.parentElement.closest(selector));
  }
  return false;
}

export interface ShortcutDocumentLike {
  body?: unknown;
  documentElement?: unknown;
  querySelector?: (selector: string) => unknown;
}

/**
 * Returns true when focus (or event target when no specific element is focused)
 * resides inside the Right Side Panel (`[data-orbit-side-panel="true"]`) or a
 * mounted File Viewer pane, so global `⌘F` yields to Find-in-File.
 */
export function isFocusInsideSidePanel(
  eventTarget: EventTarget | null,
  activeElement: Element | null,
  doc: ShortcutDocumentLike | null = typeof document !== "undefined" ? document : null,
): boolean {
  const isSpecificActive =
    activeElement !== null &&
    activeElement !== doc?.body &&
    activeElement !== doc?.documentElement;

  const candidate = isSpecificActive ? activeElement : eventTarget;
  if (matchesClosestSelector(candidate, SIDE_PANEL_SELECTOR)) {
    return true;
  }

  const sidePanelEl = (doc?.querySelector?.(SIDE_PANEL_SELECTOR) as HTMLElement | null) ?? null;
  if (sidePanelEl && typeof Node !== "undefined") {
    return isFocusInsideFileViewerPane(sidePanelEl, eventTarget, activeElement);
  }
  return false;
}

/**
 * Returns true when focus (or event target) resides inside the mounted CodeMirror 6
 * editor surface (`[data-orbit-code-editor="true"]`), so editor chords (`⌘K` inline AI edit,
 * `⌘D` multi-cursor select next occurrence) yield cleanly from global window handlers.
 */
export function isFocusInsideCodeEditor(
  eventTarget: EventTarget | null,
  activeElement: Element | null,
  doc: ShortcutDocumentLike | null = typeof document !== "undefined" ? document : null,
): boolean {
  const isSpecificActive =
    activeElement !== null &&
    activeElement !== doc?.body &&
    activeElement !== doc?.documentElement;

  const candidate = isSpecificActive ? activeElement : eventTarget;
  return matchesClosestSelector(candidate, CODE_EDITOR_SELECTOR);
}

/**
 * Returns true if any modal dialog, alertdialog, popover, menu, or listbox
 * overlay is currently open in the DOM and should consume `Escape` before
 * Settings exits.
 */
export function hasOpenSettingsBlockingOverlay(
  doc: Pick<ShortcutDocumentLike, "querySelector"> | null =
    typeof document !== "undefined" ? document : null,
): boolean {
  return Boolean(doc?.querySelector?.(SETTINGS_ESCAPE_OVERLAY_SELECTOR));
}

export interface SettingsEscapeContext {
  key: string;
  defaultPrevented?: boolean;
  activeView: "workspace" | "settings";
  commandPaletteOpen: boolean;
  targetTagName?: string | null;
  hasBlockingOverlay: boolean;
}

/**
 * Pure guardrail deciding whether `Escape` should close the Settings view.
 * Bails out whenever an inner dialog, popover, menu, listbox, command palette,
 * or text input owns the `Escape` keystroke.
 */
export function shouldCloseSettingsOnEscape(ctx: SettingsEscapeContext): boolean {
  if (ctx.defaultPrevented || ctx.key !== "Escape") {
    return false;
  }
  if (ctx.activeView !== "settings" || ctx.commandPaletteOpen || ctx.hasBlockingOverlay) {
    return false;
  }
  const tag = (ctx.targetTagName ?? "").toUpperCase();
  if (tag === "INPUT" || tag === "TEXTAREA") {
    return false;
  }
  return true;
}

export interface QuestionDockModEnterContext {
  registerGlobalShortcuts: boolean;
  canSubmit: boolean;
  submitting: boolean;
  confirmDialogOpen: boolean;
  modalDialogOpen: boolean;
  event: {
    key: string;
    metaKey: boolean;
    ctrlKey: boolean;
    shiftKey: boolean;
    altKey: boolean;
    repeat?: boolean;
    defaultPrevented?: boolean;
    isComposing?: boolean;
  };
  focusInsideQuestionCard: boolean;
  focusInsideComposer: boolean;
  focusInsideSidePanel: boolean;
  focusInsideOtherEditable: boolean;
}

/**
 * Pure guardrail deciding whether a window-level `⌘↵` / `Ctrl+Enter` should
 * submit the active `StickyQuestionDockCard`. Allows `1–9` → `⌘↵` even when
 * focus is outside the card, without ever stealing `⌘↵` from OmniComposer,
 * Side Panel panes, modal dialogs, or other editable fields.
 */
export function shouldSubmitQuestionDockOnModEnter(ctx: QuestionDockModEnterContext): boolean {
  if (!ctx.registerGlobalShortcuts || !ctx.canSubmit || ctx.submitting) {
    return false;
  }
  if (ctx.confirmDialogOpen || ctx.modalDialogOpen) {
    return false;
  }
  const { event } = ctx;
  if (event.defaultPrevented || event.repeat || event.isComposing) {
    return false;
  }
  if (event.key !== "Enter" || !(event.metaKey || event.ctrlKey) || event.shiftKey || event.altKey) {
    return false;
  }
  if (ctx.focusInsideQuestionCard) {
    return true;
  }
  if (ctx.focusInsideComposer || ctx.focusInsideSidePanel || ctx.focusInsideOtherEditable) {
    return false;
  }
  return true;
}
