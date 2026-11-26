import type { MessageKey } from "./i18n/locales/en.ts";

function detectMacPlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent || "");
}
const isMacDesktop = detectMacPlatform;

export type ShortcutScope = "global" | "stage" | "approvals" | "panels" | "composer";

export type ShortcutActionId =
  | "command_palette"
  | "find"
  | "sidebar_filter"
  | "open_settings"
  | "new_conversation"
  | "open_global_overview"
  | "next_attention"
  | "toggle_sidebar"
  | "toggle_side_panel"
  | "new_side_tab"
  | "reopen_closed_side_tab"
  | "toggle_overview_hud"
  | "quick_open_file"
  | "toggle_diff"
  | "toggle_terminal"
  | "toggle_side_question"
  | "open_external_editor"
  | "cycle_mode"
  | "approve_allow"
  | "approve_always"
  | "approve_deny"
  | "approval_prev"
  | "approval_next";

export interface ShortcutActionDefinition {
  id: ShortcutActionId;
  scope: ShortcutScope;
  groupKey: MessageKey;
  labelKey: MessageKey;
  defaultChord: string;
  /** Optional secondary built-in chord (e.g. `Ctrl+\`` for terminal toggle). */
  altDefaultChord?: string;
}

export const SHORTCUT_ACTIONS: readonly ShortcutActionDefinition[] = [
  // Global / Navigation
  {
    id: "command_palette",
    scope: "global",
    groupKey: "settings.groupNavigation",
    labelKey: "settings.scCommandPalette",
    defaultChord: "Mod+K",
  },
  {
    id: "find",
    scope: "stage",
    groupKey: "settings.groupNavigation",
    labelKey: "settings.scFilterConversations",
    defaultChord: "Mod+F",
  },
  {
    id: "sidebar_filter",
    scope: "global",
    groupKey: "settings.groupNavigation",
    labelKey: "settings.scFilterSidebar",
    defaultChord: "Mod+Shift+F",
  },
  {
    id: "open_settings",
    scope: "global",
    groupKey: "settings.groupNavigation",
    labelKey: "settings.scSettings",
    defaultChord: "Mod+,",
  },
  {
    id: "new_conversation",
    scope: "global",
    groupKey: "settings.groupNavigation",
    labelKey: "settings.scNewConversation",
    defaultChord: "Mod+N",
  },
  {
    id: "open_global_overview",
    scope: "global",
    groupKey: "settings.groupNavigation",
    labelKey: "settings.scGlobalOverview",
    defaultChord: "Mod+Shift+O",
  },
  {
    id: "next_attention",
    scope: "global",
    groupKey: "settings.groupNavigation",
    labelKey: "settings.scNextApproval",
    defaultChord: "Mod+Shift+A",
  },

  // Panels
  {
    id: "toggle_sidebar",
    scope: "panels",
    groupKey: "settings.groupPanels",
    labelKey: "settings.scToggleSidebar",
    defaultChord: "Mod+B",
  },
  {
    id: "toggle_side_panel",
    scope: "panels",
    groupKey: "settings.groupPanels",
    labelKey: "settings.scToggleSidePanel",
    defaultChord: "Mod+\\",
  },
  {
    id: "new_side_tab",
    scope: "panels",
    groupKey: "settings.groupPanels",
    labelKey: "settings.scNewSideTab",
    defaultChord: "Mod+T",
  },
  {
    id: "reopen_closed_side_tab",
    scope: "panels",
    groupKey: "settings.groupPanels",
    labelKey: "settings.scReopenClosedSideTab",
    defaultChord: "Mod+Shift+T",
  },
  {
    id: "toggle_overview_hud",
    scope: "panels",
    groupKey: "settings.groupPanels",
    labelKey: "settings.scToggleOverview",
    defaultChord: "Mod+Alt+O",
  },
  {
    id: "quick_open_file",
    scope: "panels",
    groupKey: "settings.groupPanels",
    labelKey: "settings.scQuickOpenFile",
    defaultChord: "Mod+P",
  },
  {
    id: "toggle_diff",
    scope: "panels",
    groupKey: "settings.groupPanels",
    labelKey: "settings.scToggleDiff",
    defaultChord: "Mod+D",
  },
  {
    id: "toggle_terminal",
    scope: "panels",
    groupKey: "settings.groupPanels",
    labelKey: "settings.scToggleTerminal",
    defaultChord: "Mod+J",
    altDefaultChord: "Ctrl+`",
  },
  {
    id: "toggle_side_question",
    scope: "panels",
    groupKey: "settings.groupPanels",
    labelKey: "settings.scSideQuestion",
    defaultChord: "Mod+;",
  },
  {
    id: "open_external_editor",
    scope: "panels",
    groupKey: "settings.groupPanels",
    labelKey: "settings.scExternalEditor",
    defaultChord: "Mod+O",
  },

  // Composer
  {
    id: "cycle_mode",
    scope: "composer",
    groupKey: "settings.groupComposer",
    labelKey: "settings.scCyclePermissions",
    defaultChord: "Shift+Tab",
  },

  // Approvals
  {
    id: "approve_allow",
    scope: "approvals",
    groupKey: "settings.groupApprovals",
    labelKey: "settings.scAllow",
    defaultChord: "Mod+Y",
  },
  {
    id: "approve_always",
    scope: "approvals",
    groupKey: "settings.groupApprovals",
    labelKey: "settings.scAlwaysAllow",
    defaultChord: "Mod+Shift+Y",
  },
  {
    id: "approve_deny",
    scope: "approvals",
    groupKey: "settings.groupApprovals",
    labelKey: "settings.scDeny",
    defaultChord: "Mod+Backspace",
  },
  {
    id: "approval_prev",
    scope: "approvals",
    groupKey: "settings.groupApprovals",
    labelKey: "settings.scPrevApproval",
    defaultChord: "Mod+[",
  },
  {
    id: "approval_next",
    scope: "approvals",
    groupKey: "settings.groupApprovals",
    labelKey: "settings.scNextApprovalInQueue",
    defaultChord: "Mod+]",
  },
] as const;

export const SHORTCUT_ACTION_MAP: Readonly<Record<ShortcutActionId, ShortcutActionDefinition>> =
  Object.fromEntries(SHORTCUT_ACTIONS.map((a) => [a.id, a])) as Record<
    ShortcutActionId,
    ShortcutActionDefinition
  >;

export const OS_RESERVED_CHORDS: ReadonlySet<string> = new Set([
  "Mod+Q",
  "Mod+W",
  "Mod+C",
  "Mod+V",
  "Mod+X",
  "Mod+A",
  "Mod+Z",
  "Mod+=",
  "Mod+-",
  "Mod+0",
]);

const MODIFIER_ONLY_KEYS = new Set(["Meta", "Control", "Alt", "Shift", "OS", "AltGraph"]);

const CODE_TO_KEY_FALLBACK: Readonly<Record<string, string>> = {
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Space: "Space",
};

function normalizeBaseKey(rawKey: string, rawCode?: string): string | null {
  if (!rawKey || MODIFIER_ONLY_KEYS.has(rawKey)) return null;

  // On macOS, Option+Key produces dead/Unicode glyphs (e.g. `ø` for Alt+O).
  // Prefer `e.code` letter/digit/punctuation when `rawKey` isn't standard ASCII.
  if (rawCode) {
    if (/^Key[A-Z]$/.test(rawCode) && !/^[a-zA-Z]$/.test(rawKey)) {
      return rawCode.slice(3).toUpperCase();
    }
    if (/^Digit[0-9]$/.test(rawCode) && !/^[0-9]$/.test(rawKey)) {
      return rawCode.slice(5);
    }
    if (rawCode in CODE_TO_KEY_FALLBACK && !/^[\x20-\x7E]$/.test(rawKey)) {
      return CODE_TO_KEY_FALLBACK[rawCode];
    }
  }

  const trimmed = rawKey.trim();
  if (!trimmed) {
    return rawKey === " " ? "Space" : null;
  }

  const lower = trimmed.toLowerCase();
  if (lower === "space" || lower === "spacebar") return "Space";
  if (lower === "esc" || lower === "escape") return "Escape";
  if (lower === "return" || lower === "enter") return "Enter";
  if (lower === "backspace" || lower === "⌫") return "Backspace";
  if (lower === "delete" || lower === "del") return "Delete";
  if (lower === "tab") return "Tab";
  if (lower === "arrowup" || lower === "up") return "ArrowUp";
  if (lower === "arrowdown" || lower === "down") return "ArrowDown";
  if (lower === "arrowleft" || lower === "left") return "ArrowLeft";
  if (lower === "arrowright" || lower === "right") return "ArrowRight";
  if (/^f([1-9]|1[0-2])$/i.test(trimmed)) return trimmed.toUpperCase();
  if (trimmed.length === 1) {
    return /^[a-z]$/i.test(trimmed) ? trimmed.toUpperCase() : trimmed;
  }
  return trimmed;
}

/**
 * Normalizes a chord string such as `"cmd+shift+a"` or `"Mod+Alt+o"` into canonical form
 * (`Mod+Alt+Shift+Key` ordering). Returns `null` if invalid or modifier-only.
 */
export function normalizeChord(raw: string): string | null {
  if (!raw || typeof raw !== "string") return null;
  const parts = raw
    .split("+")
    .map((p) => p.trim())
    .filter((p) => p.length > 0);
  if (parts.length === 0) return null;

  let hasMod = false;
  let hasCtrl = false;
  let hasAlt = false;
  let hasShift = false;
  let baseKey: string | null = null;

  for (const part of parts) {
    const low = part.toLowerCase();
    if (low === "mod" || low === "cmd" || low === "command" || low === "meta" || low === "⌘") {
      hasMod = true;
    } else if (low === "ctrl" || low === "control" || low === "⌃") {
      hasCtrl = true;
    } else if (low === "alt" || low === "opt" || low === "option" || low === "⌥") {
      hasAlt = true;
    } else if (low === "shift" || low === "⇧") {
      hasShift = true;
    } else {
      const normalized = normalizeBaseKey(part);
      if (!normalized) return null;
      baseKey = normalized;
    }
  }

  if (!baseKey) return null;
  // Require at least one modifier unless it is a function key (F1..F12).
  const isFunctionKey = /^F([1-9]|1[0-2])$/.test(baseKey);
  if (!hasMod && !hasCtrl && !hasAlt && !hasShift && !isFunctionKey) {
    return null;
  }

  const tokens: string[] = [];
  if (hasMod) tokens.push("Mod");
  if (hasCtrl) tokens.push("Ctrl");
  if (hasAlt) tokens.push("Alt");
  if (hasShift) tokens.push("Shift");
  tokens.push(baseKey);
  return tokens.join("+");
}

export interface KeyboardEventLike {
  key: string;
  code?: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/**
 * Converts a browser `KeyboardEvent` into a canonical normalized chord string.
 */
export function eventToChord(
  e: KeyboardEventLike,
  isMac = isMacDesktop(),
): string | null {
  const baseKey = normalizeBaseKey(e.key, e.code);
  if (!baseKey) return null;

  const hasMod = isMac ? e.metaKey : e.ctrlKey;
  const hasCtrl = isMac ? e.ctrlKey : false;
  const hasAlt = e.altKey;
  const hasShift = e.shiftKey;

  const isFunctionKey = /^F([1-9]|1[0-2])$/.test(baseKey);
  if (!hasMod && !hasCtrl && !hasAlt && !hasShift && !isFunctionKey) {
    return null;
  }

  const tokens: string[] = [];
  if (hasMod) tokens.push("Mod");
  if (hasCtrl) tokens.push("Ctrl");
  if (hasAlt) tokens.push("Alt");
  if (hasShift) tokens.push("Shift");
  tokens.push(baseKey);
  return tokens.join("+");
}

export function isOsReservedChord(chord: string): boolean {
  const norm = normalizeChord(chord);
  return norm !== null && OS_RESERVED_CHORDS.has(norm);
}

export function sanitizeCustomKeybindings(
  raw: unknown,
): Record<string, string> {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return {};
  const out: Record<string, string> = {};
  for (const [key, val] of Object.entries(raw as Record<string, unknown>)) {
    if (!(key in SHORTCUT_ACTION_MAP) || typeof val !== "string") continue;
    const norm = normalizeChord(val);
    if (!norm || OS_RESERVED_CHORDS.has(norm)) continue;
    const def = SHORTCUT_ACTION_MAP[key as ShortcutActionId];
    if (norm !== def.defaultChord) {
      out[key] = norm;
    }
  }
  return out;
}

export function getEffectiveChord(
  actionId: ShortcutActionId,
  customKeybindings?: Readonly<Record<string, string>>,
): string {
  const def = SHORTCUT_ACTION_MAP[actionId];
  const override = customKeybindings?.[actionId];
  if (override) {
    const norm = normalizeChord(override);
    if (norm && !OS_RESERVED_CHORDS.has(norm)) {
      return norm;
    }
  }
  return def.defaultChord;
}

export function matchesActionChord(
  e: KeyboardEventLike,
  actionId: ShortcutActionId,
  customKeybindings?: Readonly<Record<string, string>>,
  isMac = isMacDesktop(),
): boolean {
  const chord = eventToChord(e, isMac);
  if (!chord) return false;
  const effective = getEffectiveChord(actionId, customKeybindings);
  if (chord === effective) return true;
  const def = SHORTCUT_ACTION_MAP[actionId];
  if (!customKeybindings?.[actionId] && def.altDefaultChord && chord === def.altDefaultChord) {
    return true;
  }
  return false;
}

export function formatChordForDisplay(
  chord: string,
  isMac = isMacDesktop(),
): string {
  const norm = normalizeChord(chord) ?? chord;
  const parts = norm.split("+");
  const mod = isMac ? "⌘" : "Ctrl+";
  const ctrl = isMac ? "⌃" : "Ctrl+";
  const alt = isMac ? "⌥" : "Alt+";
  const shift = isMac ? "⇧" : "Shift+";

  const formatted = parts.map((p) => {
    switch (p) {
      case "Mod":
        return mod;
      case "Ctrl":
        return ctrl;
      case "Alt":
        return alt;
      case "Shift":
        return shift;
      case "Backspace":
        return isMac ? "⌫" : "Backspace";
      case "Delete":
        return isMac ? "⌦" : "Del";
      case "Enter":
        return isMac ? "↵" : "Enter";
      case "Escape":
        return "Esc";
      case "ArrowUp":
        return "↑";
      case "ArrowDown":
        return "↓";
      case "ArrowLeft":
        return "←";
      case "ArrowRight":
        return "→";
      default:
        return p;
    }
  });

  if (isMac) {
    return formatted.join("");
  }
  // On Windows/Linux, modifier parts already end with `+`
  let out = "";
  for (let i = 0; i < formatted.length; i += 1) {
    out += formatted[i];
    if (i < formatted.length - 1 && !formatted[i].endsWith("+")) {
      out += "+";
    }
  }
  return out;
}

export function getActionShortcutDisplay(
  actionId: ShortcutActionId,
  customKeybindings?: Readonly<Record<string, string>>,
  isMac = isMacDesktop(),
): string {
  const chord = getEffectiveChord(actionId, customKeybindings);
  return formatChordForDisplay(chord, isMac);
}

function scopesCanCollide(a: ShortcutScope, b: ShortcutScope): boolean {
  return a === b || a === "global" || b === "global";
}

/**
 * Finds any other registered shortcut in the same (or colliding global) scope that is
 * currently bound to `candidateChord`.
 */
export function findShortcutConflicts(
  actionId: ShortcutActionId,
  candidateChord: string,
  customKeybindings?: Readonly<Record<string, string>>,
): ShortcutActionDefinition[] {
  const norm = normalizeChord(candidateChord);
  if (!norm) return [];
  const targetDef = SHORTCUT_ACTION_MAP[actionId];
  const conflicts: ShortcutActionDefinition[] = [];
  for (const def of SHORTCUT_ACTIONS) {
    if (def.id === actionId) continue;
    if (!scopesCanCollide(targetDef.scope, def.scope)) continue;
    const eff = getEffectiveChord(def.id, customKeybindings);
    if (eff === norm) {
      conflicts.push(def);
    }
  }
  return conflicts;
}

/**
 * Produces a new `customKeybindings` map swapping `actionId` and `conflictActionId` cleanly.
 */
export function swapShortcutBinding(
  actionId: ShortcutActionId,
  candidateChord: string,
  conflictActionId: ShortcutActionId,
  customKeybindings?: Readonly<Record<string, string>>,
): Record<string, string> {
  const norm = normalizeChord(candidateChord);
  if (!norm || OS_RESERVED_CHORDS.has(norm)) {
    return { ...(customKeybindings ?? {}) };
  }
  const prevChordForTarget = getEffectiveChord(actionId, customKeybindings);
  const next: Record<string, string> = { ...(customKeybindings ?? {}) };

  const targetDef = SHORTCUT_ACTION_MAP[actionId];
  if (norm === targetDef.defaultChord) {
    delete next[actionId];
  } else {
    next[actionId] = norm;
  }

  const conflictDef = SHORTCUT_ACTION_MAP[conflictActionId];
  if (prevChordForTarget === conflictDef.defaultChord) {
    delete next[conflictActionId];
  } else {
    next[conflictActionId] = prevChordForTarget;
  }

  return next;
}
