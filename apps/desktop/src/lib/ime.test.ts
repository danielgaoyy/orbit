import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  POST_COMPOSITION_ENTER_GUARD_MS,
  createImeCompositionState,
  handleImeCompositionEnd,
  handleImeCompositionStart,
  resetImeCompositionState,
  shouldIgnoreImeKeyDown,
  type ImeKeyEventLike,
} from "./ime.ts";

function makeKey(overrides: Partial<ImeKeyEventLike>): ImeKeyEventLike {
  return {
    key: "Enter",
    code: "Enter",
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    isComposing: false,
    keyCode: 13,
    which: 13,
    ...overrides,
  };
}

describe("ime composition guard", () => {
  it("allows immediate Enter submission right after Space or Digit candidate selection", () => {
    const state = createImeCompositionState();
    handleImeCompositionStart(state);
    // Pressing Space during composition commits the Chinese candidate
    assert.equal(
      shouldIgnoreImeKeyDown(
        state,
        makeKey({ key: "Process", code: "Space", isComposing: true, keyCode: 229, which: 229 }),
        1000,
      ),
      true,
    );
    handleImeCompositionEnd(state, 1002);

    // Fast typist hits Enter just 10ms later -> must NOT be swallowed into a newline
    assert.equal(shouldIgnoreImeKeyDown(state, makeKey({ key: "Enter", code: "Enter" }), 1012), false);

    // Same for Digit1 candidate selection
    handleImeCompositionStart(state);
    assert.equal(
      shouldIgnoreImeKeyDown(
        state,
        makeKey({ key: "Process", code: "Digit1", isComposing: true, keyCode: 229, which: 229 }),
        2000,
      ),
      true,
    );
    handleImeCompositionEnd(state, 2001);
    assert.equal(shouldIgnoreImeKeyDown(state, makeKey({ key: "Enter", code: "Enter" }), 2015), false);
  });

  it("absorbs WebKit synchronous Enter commit echo within tight guard window, then allows real Enter", () => {
    const state = createImeCompositionState();
    handleImeCompositionStart(state);
    assert.equal(
      shouldIgnoreImeKeyDown(
        state,
        makeKey({ key: "Process", code: "KeyO", isComposing: true, keyCode: 229, which: 229 }),
        1000,
      ),
      true,
    );
    // WebKit fires compositionend then synthetic keydown("Enter") ~4ms later
    handleImeCompositionEnd(state, 1050);
    assert.equal(
      shouldIgnoreImeKeyDown(state, makeKey({ key: "Enter", code: "Enter" }), 1054),
      true,
    );
    // Subsequent Enter submits cleanly
    assert.equal(
      shouldIgnoreImeKeyDown(state, makeKey({ key: "Enter", code: "Enter" }), 1070),
      false,
    );

    // Even without Space/Digit tracking, an Enter pressed after the tight guard window submits
    handleImeCompositionStart(state);
    handleImeCompositionEnd(state, 2000);
    assert.equal(
      shouldIgnoreImeKeyDown(
        state,
        makeKey({ key: "Enter", code: "Enter" }),
        2000 + POST_COMPOSITION_ENTER_GUARD_MS + 10,
      ),
      false,
    );
  });

  it("self-heals stuck composition state on blur reset or normal non-IME keydown", () => {
    const state = createImeCompositionState();
    handleImeCompositionStart(state);
    // Simulate dropped compositionend on blur
    resetImeCompositionState(state);
    assert.equal(shouldIgnoreImeKeyDown(state, makeKey({ key: "Enter", code: "Enter" }), 1000), false);

    // Simulate dropped compositionend followed by normal typing then Enter
    handleImeCompositionStart(state);
    assert.equal(
      shouldIgnoreImeKeyDown(
        state,
        makeKey({ key: "a", code: "KeyA", isComposing: false, keyCode: 65, which: 65 }),
        1100,
      ),
      false,
    );
    assert.equal(shouldIgnoreImeKeyDown(state, makeKey({ key: "Enter", code: "Enter" }), 1150), false);
  });
});
