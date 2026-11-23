import { useSyncExternalStore } from "react";

/**
 * Shared 1-second singleton clock for live elapsed durations and timer countdowns.
 * Automatically pauses while the document is hidden and resyncs immediately when visible,
 * so multiple running tasks/timers stay synchronized on a single interval.
 */
let liveSecondNowMs = Date.now();
const liveSecondListeners = new Set<() => void>();
let liveSecondIntervalId: number | null = null;
let liveSecondVisibilityAttached = false;

function tickLiveSecondClock(): void {
  if (typeof document !== "undefined" && document.hidden) return;
  liveSecondNowMs = Date.now();
  for (const listener of liveSecondListeners) {
    listener();
  }
}

function handleLiveSecondVisibilityChange(): void {
  if (typeof document !== "undefined" && !document.hidden) {
    tickLiveSecondClock();
  }
}

function subscribeLiveSecondClock(listener: () => void): () => void {
  liveSecondListeners.add(listener);
  if (liveSecondIntervalId === null && typeof window !== "undefined") {
    liveSecondNowMs = Date.now();
    liveSecondIntervalId = window.setInterval(tickLiveSecondClock, 1000);
    if (!liveSecondVisibilityAttached && typeof document !== "undefined") {
      document.addEventListener("visibilitychange", handleLiveSecondVisibilityChange);
      liveSecondVisibilityAttached = true;
    }
  }
  return () => {
    liveSecondListeners.delete(listener);
    if (
      liveSecondListeners.size === 0 &&
      liveSecondIntervalId !== null &&
      typeof window !== "undefined"
    ) {
      window.clearInterval(liveSecondIntervalId);
      liveSecondIntervalId = null;
      if (liveSecondVisibilityAttached && typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", handleLiveSecondVisibilityChange);
        liveSecondVisibilityAttached = false;
      }
    }
  };
}

function getLiveSecondClockSnapshot(): number {
  return liveSecondNowMs;
}

const noopSubscribe = () => () => {};

export function useLiveSecondClock(enabled: boolean): number {
  return useSyncExternalStore(
    enabled ? subscribeLiveSecondClock : noopSubscribe,
    getLiveSecondClockSnapshot,
    getLiveSecondClockSnapshot,
  );
}
