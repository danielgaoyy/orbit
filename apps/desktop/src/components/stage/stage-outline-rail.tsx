import { memo, useState, type CSSProperties } from "react";
import { cn } from "@/lib/utils";

export interface OutlinePromptItem {
  seq: number;
  preview: string;
}

/**
 * Peripheral left-margin Turn Outline Rail:
 * Appears when effective conversation width grants a clean left gutter (`>= 800px` open / `< 776px` close)
 * and `>= 2` user prompts exist, synchronizing sidebar slides and visibility transitions strictly on the GPU.
 */
export const ChatOutlineRail = memo(
  function ChatOutlineRail({
    prompts,
    activeSeq,
    onJumpToSeq,
    visible = true,
    offsetPx = 0,
    offsetTransition = "none",
    isAnimating = false,
  }: {
    prompts: OutlinePromptItem[];
    activeSeq: number | null;
    onJumpToSeq: (seq: number) => void;
    visible?: boolean;
    offsetPx?: number;
    offsetTransition?: string;
    isAnimating?: boolean;
  }) {
    const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

    if (prompts.length < 2) return null;

    const wrapperStyle: CSSProperties | undefined =
      offsetPx !== 0 || offsetTransition !== "none" || isAnimating
        ? {
            transform: offsetPx !== 0 ? `translate3d(${offsetPx}px, 0, 0)` : undefined,
            transition: offsetTransition,
            willChange: isAnimating ? "transform" : undefined,
          }
        : undefined;

    return (
      <div
        style={wrapperStyle}
        className="pointer-events-none absolute top-[14%] bottom-[16%] left-[6px] z-20 w-6"
      >
        <nav
          aria-label="Conversation outline"
          aria-hidden={!visible}
          onMouseLeave={() => setHoveredIdx(null)}
          className={cn(
            "flex h-full w-full flex-col items-start justify-center transition-[opacity,transform,visibility] duration-200 ease-[var(--ease-panel)]",
            visible
              ? "pointer-events-auto visible translate-x-0 opacity-100"
              : "pointer-events-none invisible -translate-x-1.5 opacity-0",
          )}
        >
          {prompts.map((p, idx) => {
            const isActive = p.seq === activeSeq;
            const dist = !visible || hoveredIdx === null ? 99 : Math.abs(idx - hoveredIdx);
            const mag = dist === 0 ? 1 : dist === 1 ? 0.48 : dist === 2 ? 0.18 : 0;
            const baseW = isActive ? 11 : 6;
            const pillW = Math.round(baseW + mag * (16 - baseW));
            const pillH = dist === 0 ? 3 : 2;

            return (
              <div
                key={p.seq}
                onMouseEnter={() => {
                  if (visible) setHoveredIdx(idx);
                }}
                className="relative flex max-h-3.5 min-h-1.5 w-full flex-1 items-center"
              >
                <button
                  type="button"
                  tabIndex={visible ? 0 : -1}
                  aria-label={`Jump to prompt ${idx + 1}: ${p.preview}`}
                  onClick={() => {
                    if (visible) onJumpToSeq(p.seq);
                  }}
                  className="flex h-full w-full cursor-pointer items-center justify-start outline-none"
                >
                  <span
                    style={{
                      transform: `scale3d(${(pillW / 16).toFixed(3)}, ${(pillH / 3).toFixed(3)}, 1)`,
                    }}
                    className={cn(
                      "h-[3px] w-[16px] origin-left rounded-full transition-[transform,background-color] duration-150 ease-out will-change-transform",
                      dist === 0
                        ? "bg-foreground"
                        : isActive
                          ? "bg-foreground/45"
                          : "bg-foreground/18",
                    )}
                  />
                </button>

                {visible && hoveredIdx === idx && (
                  <div className="bg-popover text-popover-foreground border-border floating-surface orbit-inline-flyout-side pointer-events-none absolute top-1/2 left-6 z-30 w-56 -translate-y-1/2 rounded-xl border px-3 py-2 text-[12px] leading-snug">
                    <div className="line-clamp-2 break-words">{p.preview}</div>
                  </div>
                )}
              </div>
            );
          })}
        </nav>
      </div>
    );
  },
  (prev, next) =>
    prev.activeSeq === next.activeSeq &&
    prev.onJumpToSeq === next.onJumpToSeq &&
    prev.visible === next.visible &&
    prev.offsetPx === next.offsetPx &&
    prev.offsetTransition === next.offsetTransition &&
    prev.isAnimating === next.isAnimating &&
    prev.prompts.length === next.prompts.length &&
    prev.prompts[prev.prompts.length - 1]?.seq === next.prompts[next.prompts.length - 1]?.seq,
);
