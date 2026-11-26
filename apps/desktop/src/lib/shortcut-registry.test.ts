import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  eventToChord,
  findShortcutConflicts,
  formatChordForDisplay,
  getEffectiveChord,
  isOsReservedChord,
  matchesActionChord,
  normalizeChord,
  sanitizeCustomKeybindings,
  swapShortcutBinding,
} from "./shortcut-registry.ts";

describe("ShortcutRegistry", () => {
  it("normalizes chords and blocks OS-reserved chords", () => {
    assert.equal(normalizeChord("cmd+shift+a"), "Mod+Shift+A");
    assert.equal(normalizeChord("Mod+alt+o"), "Mod+Alt+O");
    assert.equal(normalizeChord("Shift+Tab"), "Shift+Tab");
    assert.equal(normalizeChord("a"), null); // bare key rejected
    assert.equal(normalizeChord("Meta"), null); // modifier-only rejected

    for (const reserved of [
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
    ]) {
      assert.equal(isOsReservedChord(reserved), true, `expected reserved: ${reserved}`);
    }
    assert.equal(isOsReservedChord("Mod+K"), false);
    assert.equal(isOsReservedChord("Mod+Shift+O"), false);
  });

  it("parses KeyboardEvent chords including macOS Option+O dead characters via e.code", () => {
    assert.equal(
      eventToChord(
        { key: "ø", code: "KeyO", metaKey: true, ctrlKey: false, altKey: true, shiftKey: false },
        true,
      ),
      "Mod+Alt+O",
    );
    assert.equal(
      eventToChord(
        { key: "k", code: "KeyK", metaKey: false, ctrlKey: true, altKey: false, shiftKey: false },
        false,
      ),
      "Mod+K",
    );
    assert.equal(
      matchesActionChord(
        { key: "ø", code: "KeyO", metaKey: true, ctrlKey: false, altKey: true, shiftKey: false },
        "toggle_overview_hud",
        {},
        true,
      ),
      true,
    );
  });

  it("detects same-scope / global conflicts and swaps bindings cleanly", () => {
    const conflicts = findShortcutConflicts("toggle_sidebar", "Mod+K", {});
    assert.equal(conflicts.length, 1);
    assert.equal(conflicts[0].id, "command_palette");

    // Non-colliding scopes (approvals vs panels) do NOT conflict with each other
    const crossScope = findShortcutConflicts("approve_allow", "Mod+D", {});
    assert.equal(crossScope.length, 0);

    // 1-click swap exchanges chords cleanly
    const swapped = swapShortcutBinding("toggle_sidebar", "Mod+K", "command_palette", {});
    assert.equal(getEffectiveChord("toggle_sidebar", swapped), "Mod+K");
    assert.equal(getEffectiveChord("command_palette", swapped), "Mod+B");
  });

  it("formats display glyphs for macOS and Windows/Linux and sanitizes synced overrides", () => {
    assert.equal(formatChordForDisplay("Mod+Shift+O", true), "⌘⇧O");
    assert.equal(formatChordForDisplay("Mod+Shift+O", false), "Ctrl+Shift+O");
    assert.equal(formatChordForDisplay("Mod+Backspace", true), "⌘⌫");
    assert.equal(formatChordForDisplay("Mod+Backspace", false), "Ctrl+Backspace");

    const sanitized = sanitizeCustomKeybindings({
      command_palette: "cmd+q", // OS-reserved -> dropped
      toggle_sidebar: "mod+shift+b", // valid -> kept normalized
      unknown_action: "Mod+L", // unknown -> dropped
    });
    assert.deepEqual(sanitized, { toggle_sidebar: "Mod+Shift+B" });
  });
});
