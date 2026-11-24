export interface OutlinePromptItem {
  seq: number;
  preview: string;
}

export const OUTLINE_RECENT_INDIVIDUAL_COUNT = 17;
export const OUTLINE_AGGREGATION_THRESHOLD = 17;

export interface PartitionedOutlinePrompts {
  earlier: readonly OutlinePromptItem[];
  recent: readonly OutlinePromptItem[];
  isActiveInEarlier: boolean;
}

/** Prompt sequences are chronological and can have gaps between turns. */
export function isActiveInEarlier(
  earlier: readonly OutlinePromptItem[],
  seq: number | null,
): boolean {
  if (seq === null) return false;
  let lo = 0;
  let hi = earlier.length;
  while (lo < hi) {
    const mid = lo + Math.floor((hi - lo) / 2);
    if (earlier[mid].seq < seq) lo = mid + 1;
    else hi = mid;
  }
  return earlier[lo]?.seq === seq;
}

export function partitionOutlinePrompts(
  prompts: readonly OutlinePromptItem[],
  activeSeq: number | null = null,
): PartitionedOutlinePrompts {
  if (prompts.length <= OUTLINE_AGGREGATION_THRESHOLD) {
    return { earlier: [], recent: prompts, isActiveInEarlier: false };
  }
  const split = prompts.length - OUTLINE_RECENT_INDIVIDUAL_COUNT;
  const earlier = prompts.slice(0, split);
  return {
    earlier,
    recent: prompts.slice(split),
    isActiveInEarlier: isActiveInEarlier(earlier, activeSeq),
  };
}

export function outlinePromptsEqual(
  a: readonly OutlinePromptItem[],
  b: readonly OutlinePromptItem[],
): boolean {
  return (
    a === b ||
    (a.length === b.length && a.every((p, i) => p.seq === b[i].seq && p.preview === b[i].preview))
  );
}

function hasPromptSeq(sortedPrompts: readonly OutlinePromptItem[], seq: number): boolean {
  let lo = 0;
  let hi = sortedPrompts.length;
  while (lo < hi) {
    const mid = lo + Math.floor((hi - lo) / 2);
    if (sortedPrompts[mid].seq < seq) lo = mid + 1;
    else hi = mid;
  }
  return sortedPrompts[lo]?.seq === seq;
}

/**
 * Resolves the authoritative outline anchors for the stage rail:
 * - Prefers Rust core's full-history `outline_prompts` (preserving reference stability on the fast path).
 * - Falls back to loaded turn anchors when `coreOutlinePrompts` is absent or empty.
 * - Ensures any confirmed (`seq > 0`) loaded turn prompt is never dropped even if core's list is partial,
 *   and deduplicates optimistic `seq === 0` prompts so React keys and counts remain exact.
 */
export function resolveOutlinePrompts(
  coreOutlinePrompts: readonly OutlinePromptItem[] | undefined,
  loadedOutlineAnchors: readonly OutlinePromptItem[],
  promptFallback: string,
): readonly OutlinePromptItem[] {
  if (!coreOutlinePrompts || coreOutlinePrompts.length === 0) {
    if (loadedOutlineAnchors.length === 0) {
      return coreOutlinePrompts ?? loadedOutlineAnchors;
    }
    const seen = new Set<number>();
    let needsTransform = false;
    for (let i = 0; i < loadedOutlineAnchors.length; i += 1) {
      const p = loadedOutlineAnchors[i];
      if (!p.preview || seen.has(p.seq)) {
        needsTransform = true;
        break;
      }
      seen.add(p.seq);
    }
    if (!needsTransform) return loadedOutlineAnchors;
    const deduped: OutlinePromptItem[] = [];
    const seenSeqs = new Set<number>();
    for (let i = 0; i < loadedOutlineAnchors.length; i += 1) {
      const p = loadedOutlineAnchors[i];
      if (seenSeqs.has(p.seq)) continue;
      seenSeqs.add(p.seq);
      deduped.push(p.preview ? p : { seq: p.seq, preview: promptFallback });
    }
    return deduped;
  }

  let needsFallbackPreview = false;
  for (let i = 0; i < coreOutlinePrompts.length; i += 1) {
    if (!coreOutlinePrompts[i].preview) {
      needsFallbackPreview = true;
      break;
    }
  }

  let missingLoaded: OutlinePromptItem[] | null = null;
  for (let i = 0; i < loadedOutlineAnchors.length; i += 1) {
    const loaded = loadedOutlineAnchors[i];
    if (loaded.seq <= 0) continue;
    if (!hasPromptSeq(coreOutlinePrompts, loaded.seq)) {
      if (!missingLoaded) missingLoaded = [];
      missingLoaded.push(loaded.preview ? loaded : { seq: loaded.seq, preview: promptFallback });
    }
  }

  if (!needsFallbackPreview && !missingLoaded) {
    return coreOutlinePrompts;
  }

  const base = needsFallbackPreview
    ? coreOutlinePrompts.map((p) => (p.preview ? p : { seq: p.seq, preview: promptFallback }))
    : coreOutlinePrompts;

  if (!missingLoaded) {
    return base;
  }

  const merged = [...base, ...missingLoaded].sort((a, b) => a.seq - b.seq);
  const out: OutlinePromptItem[] = [];
  for (let i = 0; i < merged.length; i += 1) {
    if (out.length === 0 || out[out.length - 1].seq !== merged[i].seq) {
      out.push(merged[i]);
    }
  }
  return out;
}
