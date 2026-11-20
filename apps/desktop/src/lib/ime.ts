import { useCallback, useRef } from "react";
import type React from "react";

const POST_COMPOSITION_ENTER_GUARD_MS = 120;

/**
 * Synchronous IME check for React or native KeyboardEvents.
 * Covers standard `isComposing`, `key === "Process"`, and legacy WebKit/Chromium `keyCode === 229`.
 */
export function isImeKeyEvent(e: React.KeyboardEvent | KeyboardEvent): boolean {
  const native = "nativeEvent" in e ? e.nativeEvent : e;
  return (
    native.isComposing ||
    e.key === "Process" ||
    native.keyCode === 229 ||
    native.which === 229
  );
}

/**
 * Cross-platform IME composition guard for inputs and textareas.
 *
 * In WebKit / WKWebView (macOS Tauri) and WebKitGTK (Linux Tauri), pressing `Enter`
 * inside a Chinese/Japanese/Korean IME to commit raw Latin text fires `compositionend`
 * immediately *before* `keydown("Enter")`, leaving `e.nativeEvent.isComposing === false`.
 *
 * Tracking `compositionstart` / `compositionend` with a brief one-shot post-end window
 * absorbs that committing `Enter` while letting normal sequences (`Space` -> `Enter`)
 * submit immediately because any non-Enter keydown clears the window.
 */
export function useImeCompositionGuard() {
  const isComposingRef = useRef(false);
  const compositionEndAtRef = useRef(0);

  const onCompositionStart = useCallback(() => {
    isComposingRef.current = true;
    compositionEndAtRef.current = 0;
  }, []);

  const onCompositionEnd = useCallback(() => {
    isComposingRef.current = false;
    compositionEndAtRef.current = performance.now();
  }, []);

  const shouldIgnoreKeyDown = useCallback((e: React.KeyboardEvent): boolean => {
    if (isComposingRef.current || isImeKeyEvent(e)) {
      return true;
    }

    if (
      e.key === "Enter" &&
      !e.metaKey &&
      !e.ctrlKey &&
      !e.altKey &&
      compositionEndAtRef.current > 0
    ) {
      const elapsed = performance.now() - compositionEndAtRef.current;
      compositionEndAtRef.current = 0;
      if (elapsed >= 0 && elapsed <= POST_COMPOSITION_ENTER_GUARD_MS) {
        return true;
      }
    } else if (
      e.key !== "Shift" &&
      e.key !== "Meta" &&
      e.key !== "Control" &&
      e.key !== "Alt"
    ) {
      compositionEndAtRef.current = 0;
    }

    return false;
  }, []);

  return {
    compositionProps: {
      onCompositionStart,
      onCompositionEnd,
    },
    shouldIgnoreKeyDown,
  };
}
