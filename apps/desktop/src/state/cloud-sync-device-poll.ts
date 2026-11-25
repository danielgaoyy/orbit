export interface DevicePollWindowLike {
  addEventListener: (type: "focus", listener: () => void) => void;
  removeEventListener: (type: "focus", listener: () => void) => void;
  setTimeout: (handler: () => void, timeoutMs: number) => ReturnType<typeof setTimeout>;
  clearTimeout: (timerId: ReturnType<typeof setTimeout>) => void;
}

export interface DevicePollDocumentLike {
  visibilityState: DocumentVisibilityState;
  addEventListener: (type: "visibilitychange", listener: () => void) => void;
  removeEventListener: (type: "visibilitychange", listener: () => void) => void;
}

export interface DevicePollWaitOptions {
  /** Minimum RFC 8628 interval between polls, including any `slow_down` backoff. */
  minIntervalMs: number;
  /** Timestamp (ms) of the previous poll completion (or flow start before the first poll). */
  lastPollAtMs: number;
  /** Absolute timestamp (ms) when the GitHub device code expires. */
  deadlineMs: number;
  /** AbortSignal cancelled when the user cancels sign-in, signs out, or restarts login. */
  signal?: AbortSignal;
  /** Clock override for deterministic tests. */
  now?: () => number;
  /** Window override for deterministic tests (defaults to global `window` when present). */
  win?: DevicePollWindowLike | null;
  /** Document override for deterministic tests (defaults to global `document` when present). */
  doc?: DevicePollDocumentLike | null;
}

export type DevicePollWakeReason = "timer" | "focus" | "expired" | "aborted";

/**
 * Formats remaining milliseconds for the GitHub Device Flow countdown as `mm:ss`.
 */
export function formatDeviceFlowCountdown(remainingMs: number): string {
  const totalSeconds = Math.max(0, Math.ceil(remainingMs / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

/**
 * Waits until the next legal GitHub Device Flow poll window.
 *
 * - Strictly enforces RFC 8628 `minIntervalMs` (including `slow_down` cooldowns)
 *   measured from `lastPollAtMs` so Orbit never triggers GitHub rate limits.
 * - Listens to window `focus` and document `visibilitychange` (`visible`) so
 *   returning from the browser immediately clears any OS background timer
 *   throttling: waking right away if cooldown already elapsed, or rescheduling
 *   for the exact remaining cooldown millisecond.
 * - Cleans up all timers and listeners immediately on wake, expiry, or abort.
 */
export function waitForNextDevicePoll(
  options: DevicePollWaitOptions,
): Promise<DevicePollWakeReason> {
  const {
    minIntervalMs,
    lastPollAtMs,
    deadlineMs,
    signal,
    now = Date.now,
    win = typeof window !== "undefined" ? (window as unknown as DevicePollWindowLike) : null,
    doc = typeof document !== "undefined" ? (document as unknown as DevicePollDocumentLike) : null,
  } = options;

  if (signal?.aborted) {
    return Promise.resolve("aborted");
  }

  const initialNow = now();
  if (initialNow >= deadlineMs) {
    return Promise.resolve("expired");
  }

  const safeIntervalMs = Math.max(0, minIntervalMs);
  const earliestLegalMs = lastPollAtMs + safeIntervalMs;

  const scheduleTimeout = (cb: () => void, ms: number): ReturnType<typeof setTimeout> =>
    win ? win.setTimeout(cb, ms) : setTimeout(cb, ms);
  const cancelTimeout = (id: ReturnType<typeof setTimeout>): void => {
    if (win) {
      win.clearTimeout(id);
    } else {
      clearTimeout(id);
    }
  };

  return new Promise<DevicePollWakeReason>((resolve) => {
    let settled = false;
    let timerId: ReturnType<typeof setTimeout> | null = null;

    const cleanup = () => {
      if (timerId !== null) {
        cancelTimeout(timerId);
        timerId = null;
      }
      if (win) {
        win.removeEventListener("focus", handleFocusWake);
      }
      if (doc) {
        doc.removeEventListener("visibilitychange", handleVisibilityChange);
      }
      if (signal) {
        signal.removeEventListener("abort", handleAbort);
      }
    };

    const finish = (reason: DevicePollWakeReason) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(reason);
    };

    const armTimer = (delayMs: number, reason: "timer" | "focus" | "expired") => {
      if (timerId !== null) {
        cancelTimeout(timerId);
        timerId = null;
      }
      timerId = scheduleTimeout(() => {
        timerId = null;
        if (now() >= deadlineMs) {
          finish("expired");
        } else {
          finish(reason);
        }
      }, Math.max(0, delayMs));
    };

    const handleAbort = () => {
      finish("aborted");
    };

    const handleFocusWake = () => {
      if (settled) return;
      const currentNow = now();
      if (currentNow >= deadlineMs) {
        finish("expired");
        return;
      }
      if (earliestLegalMs >= deadlineMs) {
        armTimer(deadlineMs - currentNow, "expired");
        return;
      }
      const remainingCooldownMs = earliestLegalMs - currentNow;
      if (remainingCooldownMs <= 0) {
        finish("focus");
        return;
      }
      armTimer(remainingCooldownMs, "focus");
    };

    const handleVisibilityChange = () => {
      if (doc?.visibilityState === "visible") {
        handleFocusWake();
      }
    };

    if (signal) {
      signal.addEventListener("abort", handleAbort, { once: true });
    }
    if (win) {
      win.addEventListener("focus", handleFocusWake);
    }
    if (doc) {
      doc.addEventListener("visibilitychange", handleVisibilityChange);
    }

    if (earliestLegalMs >= deadlineMs) {
      armTimer(deadlineMs - initialNow, "expired");
    } else {
      armTimer(Math.max(0, earliestLegalMs - initialNow), "timer");
    }
  });
}
