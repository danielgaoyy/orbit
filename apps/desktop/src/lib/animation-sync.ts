/**
 * App-wide GPU-composited phase lock for ambient loading animations.
 *
 * Every infinite loading animation (`OrbitSpinner`'s `spin`, Tailwind `animate-spin` / `animate-pulse`,
 * and `.agent-text-shimmer`'s `orbit-status-pulse`) is locked onto the single global
 * `document.timeline` origin (`startTime = 0`) via the standard Web Animations API.
 *
 * Benefits:
 * - Zero `requestAnimationFrame` polling loop and zero React re-renders.
 * - Sub-millisecond phase alignment across Sidebar, Overview, Stage, Peer Strip, and Automations.
 * - Cleanly respects `html.orbit-window-hidden` pausing and automatically resyncs on window resume.
 */

export const ORBIT_SPINNER_DURATION_MS = 850;

const SYNCED_ANIMATION_NAMES: ReadonlySet<string> = new Set([
  "spin",
  "pulse",
  "orbit-status-pulse",
]);

export interface SyncableAnimationLike {
  animationName?: string;
  playState?: string;
  startTime: number | CSSNumericValue | null;
  effect?: {
    getTiming?: () => { iterations?: number };
  } | null;
}

export function isSyncedAmbientAnimation(anim: SyncableAnimationLike): boolean {
  if (!anim.animationName || !SYNCED_ANIMATION_NAMES.has(anim.animationName)) {
    return false;
  }
  const iterations = anim.effect?.getTiming?.().iterations;
  if (iterations !== undefined && iterations !== Infinity) {
    return false;
  }
  return anim.playState === "running";
}

function isWindowHidden(): boolean {
  if (typeof document === "undefined") return false;
  return (
    document.visibilityState !== "visible" ||
    document.documentElement.classList.contains("orbit-window-hidden")
  );
}

function currentTimelineMs(): number {
  if (typeof document !== "undefined") {
    const t = document.timeline?.currentTime;
    if (typeof t === "number" && Number.isFinite(t)) {
      return t;
    }
  }
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

export function computeNegativePhaseDelayMs(nowMs: number, durationMs: number): number {
  if (!Number.isFinite(nowMs) || !Number.isFinite(durationMs) || durationMs <= 0) {
    return 0;
  }
  const phase = ((nowMs % durationMs) + durationMs) % durationMs;
  return phase === 0 ? 0 : -phase;
}

function clearInlineDelay(el: Element): void {
  if (el instanceof HTMLElement || el instanceof SVGElement) {
    if (el.dataset.orbitPhaseFallback === "1") {
      el.style.animationDelay = "";
      delete el.dataset.orbitPhaseFallback;
    }
  }
}

/**
 * Lock any running ambient animations on `el` to `document.timeline` origin `0`.
 * If WebKit has not instantiated the CSSAnimation yet (e.g. during pre-paint `useLayoutEffect`),
 * applies an exact equivalent negative `animation-delay` so Frame 0 is already in phase.
 */
export function syncElementAmbientAnimations(
  el: Element | null,
  fallbackDurationMs?: number,
): boolean {
  if (!el || isWindowHidden()) return false;

  if (typeof el.getAnimations === "function") {
    const anims = el.getAnimations();
    let synced = false;
    for (const anim of anims) {
      if (isSyncedAmbientAnimation(anim as unknown as SyncableAnimationLike)) {
        clearInlineDelay(el);
        anim.startTime = 0;
        synced = true;
      }
    }
    if (synced) return true;
  }

  if (
    fallbackDurationMs &&
    fallbackDurationMs > 0 &&
    (el instanceof HTMLElement || el instanceof SVGElement)
  ) {
    const delayMs = computeNegativePhaseDelayMs(currentTimelineMs(), fallbackDurationMs);
    el.style.animationDelay = `${delayMs.toFixed(2)}ms`;
    el.dataset.orbitPhaseFallback = "1";
  }
  return false;
}

/**
 * Re-lock every currently running ambient loading animation across the entire document
 * (e.g. right after the desktop window returns from hidden/paused state).
 */
export function resyncAllAmbientAnimations(): number {
  if (typeof document === "undefined" || isWindowHidden()) return 0;
  if (typeof document.getAnimations !== "function") return 0;

  let count = 0;
  for (const anim of document.getAnimations()) {
    if (!isSyncedAmbientAnimation(anim as unknown as SyncableAnimationLike)) {
      continue;
    }
    const target = (anim.effect as KeyframeEffect | null)?.target;
    if (target instanceof Element) {
      clearInlineDelay(target);
    }
    anim.startTime = 0;
    count += 1;
  }
  return count;
}

let installed = false;

/**
 * Install a single passive document-level `animationstart` listener so every infinite
 * spinner, shimmer, and status pulse in the app locks onto the shared clock automatically.
 */
export function installGlobalAnimationSync(): () => void {
  if (installed || typeof document === "undefined") {
    return () => {};
  }
  installed = true;

  const onAnimationStart = (ev: AnimationEvent) => {
    if (!SYNCED_ANIMATION_NAMES.has(ev.animationName)) return;
    if (ev.target instanceof Element) {
      syncElementAmbientAnimations(ev.target);
    }
  };

  let resyncRaf = 0;
  const scheduleResync = () => {
    if (document.visibilityState !== "visible") return;
    if (resyncRaf) cancelAnimationFrame(resyncRaf);
    resyncRaf = requestAnimationFrame(() => {
      resyncRaf = 0;
      if (isWindowHidden()) return;
      resyncAllAmbientAnimations();
    });
  };

  document.addEventListener("animationstart", onAnimationStart, true);
  document.addEventListener("visibilitychange", scheduleResync);

  return () => {
    installed = false;
    if (resyncRaf) cancelAnimationFrame(resyncRaf);
    document.removeEventListener("animationstart", onAnimationStart, true);
    document.removeEventListener("visibilitychange", scheduleResync);
  };
}
