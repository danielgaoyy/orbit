import { memo, useState } from "react";
import { cn } from "@/lib/utils";

export interface OutlinePromptItem {
  seq: number;
  preview: string;
}

/**
 * Peripheral left-margin Turn Outline Rail:
 * Appears only on wide viewports (`>= 940px` stage width) when `>= 2` user prompts exist,
 * allowing zero-clutter jump-to-turn navigation in long sessions.
 */
export const ChatOutlineRail = memo(
  function ChatOutlineRail({
    prompts,
    activeSeq,
    onJumpToSeq,
  }: {
    prompts: OutlinePromptItem[];
    activeSeq: number | null;
    onJumpToSeq: (seq: number) => void;
  }) {
    const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

    if (prompts.length < 2) return null;

    return (
      <nav
        aria-label="Conversation outline"
        onMouseLeave={() => setHoveredIdx(null)}
        className="pointer-events-auto absolute top-[14%] bottom-[16%] left-2 z-20 flex w-7 flex-col items-start justify-center"
      >
        {prompts.map((p, idx) => {
          const isActive = p.seq === activeSeq;
          const dist = hoveredIdx === null ? 99 : Math.abs(idx - hoveredIdx);
          const mag = dist === 0 ? 1 : dist === 1 ? 0.48 : dist === 2 ? 0.18 : 0;
          const baseW = isActive ? 14 : 8;
          const pillW = Math.round(baseW + mag * (22 - baseW));
          const pillH = dist === 0 ? 3 : 2;

          return (
            <div
              key={p.seq}
              onMouseEnter={() => setHoveredIdx(idx)}
              className="relative flex max-h-3.5 min-h-1.5 w-full flex-1 items-center pl-1"
            >
              <button
                type="button"
                aria-label={`Jump to prompt ${idx + 1}: ${p.preview}`}
                onClick={() => onJumpToSeq(p.seq)}
                className="flex h-full w-full cursor-pointer items-center justify-start outline-none"
              >
                <span
                  style={{ width: `${pillW}px`, height: `${pillH}px` }}
                  className={cn(
                    "rounded-full transition-all duration-150 ease-out",
                    dist === 0
                      ? "bg-foreground"
                      : isActive
                        ? "bg-foreground/45"
                        : "bg-foreground/18",
                  )}
                />
              </button>

              {hoveredIdx === idx && (
                <div className="bg-popover text-popover-foreground border-border floating-surface orbit-inline-flyout-side pointer-events-none absolute top-1/2 left-8 z-30 w-56 -translate-y-1/2 rounded-xl border px-3 py-2 text-[12px] leading-snug">
                  <div className="line-clamp-2 break-words">{p.preview}</div>
                </div>
              )}
            </div>
          );
        })}
      </nav>
    );
  },
  (prev, next) =>
    prev.activeSeq === next.activeSeq &&
    prev.onJumpToSeq === next.onJumpToSeq &&
    prev.prompts.length === next.prompts.length &&
    prev.prompts[prev.prompts.length - 1]?.seq === next.prompts[next.prompts.length - 1]?.seq,
);
