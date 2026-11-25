import { useCallback, useRef } from "react";
import type React from "react";

/**
 * In WebKit / WKWebView, pressing `Enter` inside a CJK IME to commit raw Latin text fires
 * `compositionend` immediately before a synthetic `keydown("Enter")` in the *same* native
 * event dispatch (typically 0-15ms apart). Keep the guard tight enough to absorb that
 * same-dispatch event without swallowing a fast human `Space -> Enter` sequence.
 */
export const POST_COMPOSITION_ENTER_GUARD_MS = 35;

/**
 * Physical key codes that commit or cancel an IME candidate without pressing `Enter`.
 * When `compositionend` immediately follows one of these keys, any subsequent `Enter` is
 * guaranteed to be a separate physical keypress meant to submit, never a WKWebView commit echo.
 */
const NON_ENTER_COMMIT_CODES: ReadonlySet<string> = new Set([
  "Space",
  "Digit1",
  "Digit2",
  "Digit3",
  "Digit4",
  "Digit5",
  "Digit6",
  "Digit7",
  "Digit8",
  "Digit9",
  "Digit0",
  "Numpad1",
  "Numpad2",
  "Numpad3",
  "Numpad4",
  "Numpad5",
  "Numpad6",
  "Numpad7",
  "Numpad8",
  "Numpad9",
  "Numpad0",
  "Comma",
  "Period",
  "Slash",
  "Semicolon",
  "Quote",
  "BracketLeft",
  "BracketRight",
  "Minus",
  "Equal",
  "Escape",
]);

const MODIFIER_KEYS: ReadonlySet<string> = new Set([
  "Shift",
  "Meta",
  "Control",
  "Alt",
  "CapsLock",
]);

export interface ImeKeyEventLike {
  key: string;
  code?: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  isComposing?: boolean;
  keyCode?: number;
  which?: number;
  nativeEvent?: {
    isComposing?: boolean;
    keyCode?: number;
    which?: number;
  };
}

/**
 * Synchronous IME check for React or native KeyboardEvents.
 * Covers standard `isComposing`, `key === "Process"`, and legacy WebKit/Chromium `keyCode === 229`.
 */
export function isImeKeyEvent(e: React.KeyboardEvent | KeyboardEvent | ImeKeyEventLike): boolean {
  const native = (
    "nativeEvent" in e && e.nativeEvent ? e.nativeEvent : e
  ) as { isComposing?: boolean; keyCode?: number; which?: number };
  return Boolean(
    native.isComposing ||
      e.key === "Process" ||
      native.keyCode === 229 ||
      native.which === 229,
  );
}

export interface ImeCompositionState {
  isComposing: boolean;
  compositionEndAt: number;
  lastCompositionCode: string;
}

export function createImeCompositionState(): ImeCompositionState {
  return {
    isComposing: false,
    compositionEndAt: 0,
    lastCompositionCode: "",
  };
}

export function handleImeCompositionStart(state: ImeCompositionState): void {
  state.isComposing = true;
  state.compositionEndAt = 0;
  state.lastCompositionCode = "";
}

export function handleImeCompositionEnd(state: ImeCompositionState, nowMs: number): void {
  const committedByNonEnter = NON_ENTER_COMMIT_CODES.has(state.lastCompositionCode);
  state.isComposing = false;
  state.lastCompositionCode = "";
  state.compositionEndAt = committedByNonEnter ? 0 : nowMs;
}

export function resetImeCompositionState(state: ImeCompositionState): void {
  state.isComposing = false;
  state.compositionEndAt = 0;
  state.lastCompositionCode = "";
}

export function shouldIgnoreImeKeyDown(
  state: ImeCompositionState,
  e: React.KeyboardEvent | KeyboardEvent | ImeKeyEventLike,
  nowMs: number,
): boolean {
  const imeActive = isImeKeyEvent(e);
  if (imeActive) {
    state.lastCompositionCode = e.code || e.key;
    return true;
  }

  if (state.isComposing) {
    // Self-heal if WKWebView dropped `compositionend` (e.g. CapsLock / input-source switch / blur):
    // a normal non-Enter, non-modifier keydown with `isComposing === false` proves composition ended.
    if (e.key !== "Enter" && !MODIFIER_KEYS.has(e.key) && e.key !== "Dead" && e.key !== "Unidentified") {
      state.isComposing = false;
      state.lastCompositionCode = "";
      state.compositionEndAt = 0;
      return false;
    }
    state.lastCompositionCode = e.code || e.key;
    return true;
  }

  if (
    e.key === "Enter" &&
    !e.metaKey &&
    !e.ctrlKey &&
    !e.altKey &&
    state.compositionEndAt > 0
  ) {
    const elapsed = nowMs - state.compositionEndAt;
    state.compositionEndAt = 0;
    if (elapsed >= 0 && elapsed <= POST_COMPOSITION_ENTER_GUARD_MS) {
      return true;
    }
  } else if (!MODIFIER_KEYS.has(e.key)) {
    state.compositionEndAt = 0;
  }

  return false;
}

/**
 * Cross-platform IME composition guard for inputs and textareas.
 *
 * - Distinguishes candidate-selection commits (`Space`, `1-9`, punctuation) from `Enter` commits
 *   so fast `Space -> Enter` sequences always submit immediately.
 * - Absorbs WKWebView's synchronous `compositionend -> keydown("Enter")` echo (`<= 35ms`).
 * - Exposes `reset()` (wire to `onBlur` where appropriate) and self-heals stuck composition state.
 */
export function useImeCompositionGuard() {
  const stateRef = useRef<ImeCompositionState>(createImeCompositionState());

  const onCompositionStart = useCallback(() => {
    handleImeCompositionStart(stateRef.current);
  }, []);

  const onCompositionEnd = useCallback(() => {
    handleImeCompositionEnd(stateRef.current, performance.now());
  }, []);

  const reset = useCallback(() => {
    resetImeCompositionState(stateRef.current);
  }, []);

  const shouldIgnoreKeyDown = useCallback((e: React.KeyboardEvent): boolean => {
    const blockedByActiveIme = isImeKeyEvent(e) || stateRef.current.isComposing;
    const ignored = shouldIgnoreImeKeyDown(stateRef.current, e, performance.now());
    // Prevent WKWebView's synthetic post-composition Enter echo from inserting stray
    // newlines in textareas or submitting forms across all callers uniformly (BUG-023a),
    // without blocking Enter during active IME composition itself.
    if (ignored && !blockedByActiveIme && e.key === "Enter") {
      e.preventDefault();
    }
    return ignored;
  }, []);

  return {
    compositionProps: {
      onCompositionStart,
      onCompositionEnd,
    },
    reset,
    shouldIgnoreKeyDown,
  };
}
