export const DEFAULT_MAX_PENDING_WRITE_BYTES = 256 * 1024;
export const HIDDEN_FLUSH_DELAY_MS = 16;

export interface XtermWriteBatcherEnv {
  isDocumentHidden: () => boolean;
  requestAnimationFrame: (cb: () => void) => number;
  cancelAnimationFrame: (id: number) => void;
  setTimeout: (cb: () => void, ms: number) => number;
  clearTimeout: (id: number) => void;
  maxPendingBytes?: number;
  hiddenFlushDelayMs?: number;
}

export interface XtermWriteBatcher {
  enqueue(chunk: Uint8Array): void;
  flushNow(): void;
  onVisibilityChange(): void;
  pendingByteCount(): number;
  dispose(): void;
}

/**
 * Bounded frame-coalesced binary write batcher for xterm.js:
 * - Visible window under byte cap: coalesces bursts into a single `requestAnimationFrame` write.
 * - Hidden/minimized window (`document.hidden`): drains via `setTimeout(16ms)` instead of
 *   waiting on a suspended `requestAnimationFrame`.
 * - Byte cap reached (`>= maxPendingBytes`): flushes synchronously immediately so pending
 *   buffers remain strictly bounded even under sustained build output while backgrounded.
 */
export function createXtermWriteBatcher(
  onWrite: (bytes: Uint8Array) => void,
  env: XtermWriteBatcherEnv,
): XtermWriteBatcher {
  const maxPendingBytes = Math.max(1, env.maxPendingBytes ?? DEFAULT_MAX_PENDING_WRITE_BYTES);
  const hiddenFlushDelayMs = Math.max(0, env.hiddenFlushDelayMs ?? HIDDEN_FLUSH_DELAY_MS);

  let disposed = false;
  let pendingChunks: Uint8Array[] = [];
  let pendingBytes = 0;
  let rafId: number | null = null;
  let timeoutId: number | null = null;

  const cancelScheduled = () => {
    if (rafId !== null) {
      env.cancelAnimationFrame(rafId);
      rafId = null;
    }
    if (timeoutId !== null) {
      env.clearTimeout(timeoutId);
      timeoutId = null;
    }
  };

  const flushNow = () => {
    cancelScheduled();
    if (disposed || pendingChunks.length === 0) return;

    const toWrite =
      pendingChunks.length === 1
        ? pendingChunks[0]
        : (() => {
            const merged = new Uint8Array(pendingBytes);
            let offset = 0;
            for (const part of pendingChunks) {
              merged.set(part, offset);
              offset += part.byteLength;
            }
            return merged;
          })();

    pendingChunks = [];
    pendingBytes = 0;
    onWrite(toWrite);
  };

  const scheduleFlush = () => {
    if (disposed || pendingChunks.length === 0) return;
    if (pendingBytes >= maxPendingBytes) {
      flushNow();
      return;
    }
    if (env.isDocumentHidden()) {
      if (rafId !== null) {
        env.cancelAnimationFrame(rafId);
        rafId = null;
      }
      if (timeoutId === null) {
        timeoutId = env.setTimeout(() => {
          timeoutId = null;
          flushNow();
        }, hiddenFlushDelayMs);
      }
      return;
    }
    if (rafId === null && timeoutId === null) {
      rafId = env.requestAnimationFrame(() => {
        rafId = null;
        flushNow();
      });
    }
  };

  return {
    enqueue(chunk: Uint8Array) {
      if (disposed || chunk.byteLength === 0) return;
      pendingChunks.push(chunk);
      pendingBytes += chunk.byteLength;
      scheduleFlush();
    },

    flushNow,

    onVisibilityChange() {
      if (disposed || pendingChunks.length === 0) return;
      if (env.isDocumentHidden() && rafId !== null) {
        // Transitioned to hidden while an rAF was pending — switch immediately to timer drain
        scheduleFlush();
      }
    },

    pendingByteCount() {
      return pendingBytes;
    },

    dispose() {
      disposed = true;
      cancelScheduled();
      pendingChunks = [];
      pendingBytes = 0;
    },
  };
}
