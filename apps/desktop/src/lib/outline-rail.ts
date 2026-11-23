export interface OutlinePromptItem {
  seq: number;
  preview: string;
}

/**
 * Maximum number of prompts rendered directly as individual ticks before earlier turns aggregate.
 * Using a threshold of 12 with 10 recent individual ticks guarantees an aggregated cluster always
 * holds at least 3 earlier prompts (never producing a noisy "1 earlier" group).
 */
export const OUTLINE_AGGREGATION_THRESHOLD = 12;
export const OUTLINE_RECENT_INDIVIDUAL_COUNT = 10;

export interface PartitionedOutlinePrompts {
  /** Earlier prompts collapsed into the top aggregated cluster (chronological, oldest first). */
  earlier: OutlinePromptItem[];
  /** Most recent prompts rendered as individual clickable ticks on the rail. */
  recent: OutlinePromptItem[];
  /** True when the currently active/visible prompt lives inside `earlier`. */
  isActiveInEarlier: boolean;
}

export function partitionOutlinePrompts(
  prompts: readonly OutlinePromptItem[],
  activeSeq: number | null,
): PartitionedOutlinePrompts {
  if (prompts.length <= OUTLINE_AGGREGATION_THRESHOLD) {
    return {
      earlier: [],
      recent: prompts as OutlinePromptItem[],
      isActiveInEarlier: false,
    };
  }

  const splitIdx = prompts.length - OUTLINE_RECENT_INDIVIDUAL_COUNT;
  const earlier = prompts.slice(0, splitIdx);
  const recent = prompts.slice(splitIdx);
  const isActiveInEarlier =
    activeSeq !== null && earlier.some((item) => item.seq === activeSeq);

  return {
    earlier,
    recent,
    isActiveInEarlier,
  };
}
