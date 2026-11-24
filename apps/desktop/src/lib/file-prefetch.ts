/**
 * Low-overhead intent-based file prefetch helpers for Orbit Desktop.
 *
 * Design constraints:
 * - Zero idle background work and zero per-row timers or React state.
 * - Hover prefetch uses a single shared dwell timer (`65ms`, cancelled on pointer leave)
 *   and skips heavy media/binary paths so cursor sweeps never trigger disk/IPC churn.
 * - Primary-button `pointerdown` triggers immediate prefetch (`~80–120ms` head start before `click`
 *   completes) with a ~100% hit rate for the exact file the user is opening.
 */

export type FilePrefetchIntent = "hover" | "press";

export const HOVER_PREFETCH_DELAY_MS = 65;
export const QUICK_OPEN_PREFETCH_DELAY_MS = 80;
export const FILE_LOADING_GRACE_MS = 120;
export const MAX_CONCURRENT_FILE_PREFETCHES = 2;

const HEAVY_HOVER_PREFETCH_EXT_RE =
  /\.(?:pdf|mp4|mov|m4v|webm|mkv|avi|mp3|wav|m4a|ogg|flac|aac|png|jpe?g|gif|webp|avif|bmp|ico|tiff?|zip|tar|gz|tgz|bz2|xz|7z|rar|dmg|pkg|exe|dll|so|dylib|wasm|bin|sqlite|db)$/i;

/**
 * Heavy media, image, archive, and binary files are skipped during passive hover prefetch
 * so cursor sweeps never allocate multi-megabyte buffers; explicit press (`pointerdown`) still
 * warms them immediately.
 */
export function isHeavyHoverPrefetchPath(rawPath: string): boolean {
  const clean = rawPath.trim().split(/[?#]/, 1)[0] ?? "";
  return HEAVY_HOVER_PREFETCH_EXT_RE.test(clean);
}

/**
 * Only prefetch on unmodified primary-button press so context menus, middle-clicks,
 * and modified clicks never trigger spurious reads.
 */
export function shouldPrefetchOnPointerDown(event: {
  button: number;
  metaKey?: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
}): boolean {
  return (
    event.button === 0 &&
    !event.metaKey &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.shiftKey
  );
}

let activeHoverTimer: ReturnType<typeof setTimeout> | null = null;

export function cancelHoverPrefetch(): void {
  if (activeHoverTimer !== null) {
    clearTimeout(activeHoverTimer);
    activeHoverTimer = null;
  }
}

export function scheduleHoverPrefetch(
  callback: () => void,
  delayMs: number = HOVER_PREFETCH_DELAY_MS,
): void {
  cancelHoverPrefetch();
  activeHoverTimer = setTimeout(() => {
    activeHoverTimer = null;
    callback();
  }, delayMs);
}
