import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  type XtermBufferLike,
  type XtermSearchShortcutContext,
  type XtermSearchShortcutEventLike,
  type XtermSearchTarget,
  resolveXtermSearchShortcut,
  revealXtermMatch,
  searchXtermBuffer,
  stepXtermMatchIndex,
} from "./xterm-search.ts";

function makeBuffer(lines: string[]): XtermBufferLike {
  return {
    length: lines.length,
    getLine(y: number) {
      const text = lines[y];
      if (text === undefined) return undefined;
      return {
        translateToString(trimRight?: boolean) {
          return trimRight ? text.replace(/\s+$/, "") : text;
        },
      };
    },
  };
}

function makeKey(
  overrides: Partial<XtermSearchShortcutEventLike> = {},
): XtermSearchShortcutEventLike {
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

function makeCtx(
  overrides: Partial<XtermSearchShortcutContext> = {},
): XtermSearchShortcutContext {
  return {
    isMac: true,
    terminalHasFocus: true,
    findOpen: false,
    inSearchInput: false,
    ...overrides,
  };
}

describe("xterm-search buffer engine", () => {
  it("finds multiple plain-text matches per line across scrollback case-insensitively by default", () => {
    const buf = makeBuffer([
      "Compiling orbit-client-core v0.9.4",
      "warning: unused variable foo; Foo appears twice",
      "Finished dev target in 1.2s",
    ]);

    const res = searchXtermBuffer(buf, "foo");
    assert.equal(res.error, null);
    assert.equal(res.truncated, false);
    assert.deepEqual(res.matches, [
      { row: 1, col: 25, length: 3 },
      { row: 1, col: 30, length: 3 },
    ]);
  });

  it("respects caseSensitive option for plain text and regex modes", () => {
    const buf = makeBuffer(["Alpha alpha ALPHA", "alpha-2"]);

    const plainSensitive = searchXtermBuffer(buf, "Alpha", { caseSensitive: true });
    assert.deepEqual(plainSensitive.matches, [{ row: 0, col: 0, length: 5 }]);

    const regexSensitive = searchXtermBuffer(buf, "ALPHA|alpha-\\d", {
      caseSensitive: true,
      regex: true,
    });
    assert.equal(regexSensitive.error, null);
    assert.deepEqual(regexSensitive.matches, [
      { row: 0, col: 12, length: 5 },
      { row: 1, col: 0, length: 7 },
    ]);
  });

  it("handles invalid regex safely without throwing and avoids infinite loops on zero-width patterns", () => {
    const buf = makeBuffer(["error[E0308]: mismatched types", "ok"]);

    const invalid = searchXtermBuffer(buf, "[unclosed(", { regex: true });
    assert.equal(invalid.matches.length, 0);
    assert.ok(invalid.error && invalid.error.length > 0);

    const zeroWidth = searchXtermBuffer(buf, "a*", { regex: true });
    assert.equal(zeroWidth.error, null);
    assert.deepEqual(zeroWidth.matches, [{ row: 0, col: 18, length: 1 }]);

    const anchorOnly = searchXtermBuffer(buf, "^", { regex: true });
    assert.equal(anchorOnly.error, null);
    assert.equal(anchorOnly.matches.length, 0);
  });

  it("bounds results at maxMatches and sets truncated flag", () => {
    const buf = makeBuffer(["a a a a a", "a a a a a"]);
    const res = searchXtermBuffer(buf, "a", { maxMatches: 4 });
    assert.equal(res.matches.length, 4);
    assert.equal(res.truncated, true);
    assert.equal(res.error, null);
  });

  it("steps match indices with clean wrap-around and reveals/selects match in xterm", () => {
    assert.equal(stepXtermMatchIndex(0, 0, 1), -1);
    assert.equal(stepXtermMatchIndex(0, 3, 1), 1);
    assert.equal(stepXtermMatchIndex(2, 3, 1), 0);
    assert.equal(stepXtermMatchIndex(0, 3, -1), 2);
    assert.equal(stepXtermMatchIndex(-1, 3, -1), 2);

    const calls: string[] = [];
    const fakeTerm: XtermSearchTarget = {
      cols: 100,
      rows: 30,
      buffer: { active: makeBuffer([]) },
      scrollToLine(line) {
        calls.push(`scroll:${line}`);
      },
      select(col, row, length) {
        calls.push(`select:${col},${row},${length}`);
      },
      clearSelection() {
        calls.push("clear");
      },
    };

    revealXtermMatch(fakeTerm, { row: 42, col: 7, length: 5 });
    revealXtermMatch(fakeTerm, null);
    assert.deepEqual(calls, ["scroll:32", "select:7,42,5", "clear"]);
  });

  it("scopes Cmd/Ctrl+F, Cmd/Ctrl+G, Enter, and Esc strictly to focused terminal pane", () => {
    // Focused macOS terminal: Cmd+F opens search
    assert.equal(resolveXtermSearchShortcut(makeKey({ key: "f", metaKey: true }), makeCtx()), "open");
    // Unfocused terminal: NEVER hijacks global or file viewer Cmd+F
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "f", metaKey: true }),
        makeCtx({ terminalHasFocus: false }),
      ),
      null,
    );
    // Windows/Linux: Ctrl+F opens, Meta+F ignored
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "f", metaKey: false, ctrlKey: true }),
        makeCtx({ isMac: false }),
      ),
      "open",
    );
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "f", metaKey: true, ctrlKey: false }),
        makeCtx({ isMac: false }),
      ),
      null,
    );
    // Cmd+G / Shift+Cmd+G navigate when search is open
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "g", metaKey: true }),
        makeCtx({ findOpen: true }),
      ),
      "next",
    );
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "G", metaKey: true, shiftKey: true }),
        makeCtx({ findOpen: true }),
      ),
      "prev",
    );
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "g", metaKey: true }),
        makeCtx({ findOpen: false }),
      ),
      null,
    );
    // Enter / Shift+Enter only inside search input; Escape closes when open
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "Enter", metaKey: false }),
        makeCtx({ findOpen: true, inSearchInput: true }),
      ),
      "next",
    );
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "Enter", metaKey: false, shiftKey: true }),
        makeCtx({ findOpen: true, inSearchInput: true }),
      ),
      "prev",
    );
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "Enter", metaKey: false }),
        makeCtx({ findOpen: true, inSearchInput: false }),
      ),
      null,
    );
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "Escape", metaKey: false }),
        makeCtx({ findOpen: true }),
      ),
      "close",
    );
    // Never fires during IME composition
    assert.equal(
      resolveXtermSearchShortcut(
        makeKey({ key: "Enter", metaKey: false, isComposing: true }),
        makeCtx({ findOpen: true, inSearchInput: true }),
      ),
      null,
    );
  });
});
