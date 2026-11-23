import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  type OutlinePromptItem,
  partitionOutlinePrompts,
} from "@/lib/outline-rail";
import { cn } from "@/lib/utils";

export type { OutlinePromptItem };

const CLUSTER_CLOSE_DELAY_MS = 160;

/**
 * Peripheral left-margin Turn Outline Rail (Antigravity-style chronological aggregation):
 * - Up to 12 prompts: renders each prompt as an individual tick with comfortable `14px` hit height.
 * - Beyond 12 prompts: keeps the latest 10 prompts as crisp individual ticks and aggregates all
 *   earlier turns into a calm 3-dot vertical cluster at the top of the rail.
 * - Hovering or clicking the top cluster opens a hover-bridged scrollable popover listing every
 *   earlier prompt in chronological order (auto-scrolling to the active prompt when scrolled up).
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
    const [hoveredTickIdx, setHoveredTickIdx] = useState<number | null>(null);
    const [clusterOpen, setClusterOpen] = useState(false);
    const closeTimerRef = useRef<number | null>(null);
    const clusterContainerRef = useRef<HTMLDivElement | null>(null);
    const activeEarlierRowRef = useRef<HTMLButtonElement | null>(null);

    const clearCloseTimer = useCallback(() => {
      if (closeTimerRef.current !== null) {
        window.clearTimeout(closeTimerRef.current);
        closeTimerRef.current = null;
      }
    }, []);

    useEffect(() => clearCloseTimer, [clearCloseTimer]);

    useEffect(() => {
      if (!visible) {
        clearCloseTimer();
        setHoveredTickIdx(null);
        setClusterOpen(false);
      }
    }, [visible, clearCloseTimer]);

    const { earlier, recent, isActiveInEarlier } = useMemo(
      () => partitionOutlinePrompts(prompts, activeSeq),
      [prompts, activeSeq],
    );

    useEffect(() => {
      if (clusterOpen && isActiveInEarlier && activeEarlierRowRef.current) {
        activeEarlierRowRef.current.scrollIntoView({ block: "nearest" });
      }
    }, [clusterOpen, isActiveInEarlier]);

    useEffect(() => {
      if (!clusterOpen) return;
      const onKeyDown = (e: KeyboardEvent) => {
        if (e.key === "Escape") {
          clearCloseTimer();
          setClusterOpen(false);
        }
      };
      const onPointerDown = (e: PointerEvent) => {
        if (
          clusterContainerRef.current &&
          e.target instanceof Node &&
          !clusterContainerRef.current.contains(e.target)
        ) {
          clearCloseTimer();
          setClusterOpen(false);
        }
      };
      window.addEventListener("keydown", onKeyDown);
      document.addEventListener("pointerdown", onPointerDown, true);
      return () => {
        window.removeEventListener("keydown", onKeyDown);
        document.removeEventListener("pointerdown", onPointerDown, true);
      };
    }, [clusterOpen, clearCloseTimer]);

    const openCluster = useCallback(() => {
      if (!visible) return;
      clearCloseTimer();
      setHoveredTickIdx(null);
      setClusterOpen(true);
    }, [visible, clearCloseTimer]);

    const scheduleCloseCluster = useCallback(() => {
      clearCloseTimer();
      closeTimerRef.current = window.setTimeout(() => {
        closeTimerRef.current = null;
        setClusterOpen(false);
      }, CLUSTER_CLOSE_DELAY_MS);
    }, [clearCloseTimer]);

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
        className="pointer-events-none absolute top-[14%] bottom-[16%] left-[6px] z-30 flex w-6 items-center"
      >
        <nav
          aria-label="Conversation outline"
          aria-hidden={!visible}
          onMouseLeave={() => {
            setHoveredTickIdx(null);
            if (clusterOpen) scheduleCloseCluster();
          }}
          className={cn(
            "flex w-full flex-col items-start justify-center transition-[opacity,transform,visibility] duration-200 ease-[var(--ease-panel)]",
            visible
              ? "pointer-events-auto visible translate-x-0 opacity-100"
              : "pointer-events-none invisible -translate-x-1.5 opacity-0",
          )}
        >
          {/* Aggregated Earlier Prompts Anchor (shown only when > 12 total prompts) */}
          {earlier.length > 0 && (
            <div
              ref={clusterContainerRef}
              className="relative mb-1.5 flex h-[22px] w-full items-center"
              onMouseEnter={openCluster}
              onMouseLeave={scheduleCloseCluster}
            >
              <button
                type="button"
                tabIndex={visible ? 0 : -1}
                aria-expanded={clusterOpen}
                aria-label={`${earlier.length} earlier prompts`}
                onClick={() => {
                  if (!visible) return;
                  clearCloseTimer();
                  setHoveredTickIdx(null);
                  setClusterOpen((prev) => !prev);
                }}
                className="group/cluster flex h-full w-full cursor-pointer flex-col items-start justify-center gap-[2.5px] pl-[1.5px] outline-none"
              >
                {[0, 1, 2].map((dotIdx) => (
                  <span
                    key={dotIdx}
                    className={cn(
                      "rounded-full transition-[transform,background-color,width] duration-150 ease-out",
                      clusterOpen
                        ? "bg-foreground h-[2.5px] w-[7px]"
                        : isActiveInEarlier
                          ? "bg-foreground/55 group-hover/cluster:bg-foreground h-[2.5px] w-[5px]"
                          : "bg-foreground/26 group-hover/cluster:bg-foreground/75 h-[2.5px] w-[2.5px]",
                    )}
                  />
                ))}
              </button>

              {visible && clusterOpen && (
                /* Transparent `pl-3` hover bridge connects the 3-dot cluster to the scrollable popover */
                <div
                  className="absolute top-1/2 left-3.5 z-30 -translate-y-1/2 pl-3"
                  onMouseEnter={openCluster}
                  onMouseLeave={scheduleCloseCluster}
                >
                  <div
                    role="region"
                    aria-label="Earlier prompts"
                    className="bg-popover text-popover-foreground border-border floating-surface orbit-inline-flyout-side w-[264px] overflow-hidden rounded-xl border"
                  >
                    <div className="border-border/60 flex items-center justify-between gap-2 border-b px-3 py-1.5">
                      <span className="text-muted-foreground text-[11px] font-medium">
                        Earlier prompts
                      </span>
                      <span className="text-muted-foreground/75 tabular-nums font-mono text-[10.5px]">
                        1–{earlier.length}
                      </span>
                    </div>
                    <ScrollArea
                      className="w-full"
                      viewportClassName="max-h-[256px] overscroll-contain py-1 pl-1 pr-2.5"
                      contentClassName="space-y-0.5"
                    >
                      {earlier.map((item, idx) => {
                        const isItemActive = item.seq === activeSeq;
                        return (
                          <button
                            key={item.seq}
                            ref={isItemActive ? activeEarlierRowRef : undefined}
                            type="button"
                            title={item.preview}
                            onClick={() => {
                              clearCloseTimer();
                              setClusterOpen(false);
                              onJumpToSeq(item.seq);
                            }}
                            className={cn(
                              "flex w-full cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-left text-[12px] leading-snug transition-colors",
                              isItemActive
                                ? "bg-accent/85 text-foreground font-medium"
                                : "text-foreground/82 hover:bg-accent/60 hover:text-foreground font-normal",
                            )}
                          >
                            <span
                              className={cn(
                                "tabular-nums w-5 shrink-0 text-right font-mono text-[10.5px]",
                                isItemActive
                                  ? "text-foreground/75"
                                  : "text-muted-foreground/65",
                              )}
                            >
                              {idx + 1}
                            </span>
                            <span className="min-w-0 flex-1 truncate">{item.preview}</span>
                          </button>
                        );
                      })}
                    </ScrollArea>
                  </div>
                </div>
              )}
            </div>
          )}

          {/* Individual Recent Prompt Ticks (always crisp, fixed 14px vertical hit slot) */}
          {recent.map((p, idx) => {
            const isActive = p.seq === activeSeq;
            const dist =
              !visible || hoveredTickIdx === null ? 99 : Math.abs(idx - hoveredTickIdx);
            const mag = dist === 0 ? 1 : dist === 1 ? 0.48 : dist === 2 ? 0.18 : 0;
            const baseW = isActive ? 11 : 6;
            const pillW = Math.round(baseW + mag * (16 - baseW));
            const pillH = dist === 0 ? 3 : 2;
            const promptNumber = earlier.length + idx + 1;

            return (
              <div
                key={p.seq}
                onMouseEnter={() => {
                  if (!visible) return;
                  if (clusterOpen) {
                    clearCloseTimer();
                    setClusterOpen(false);
                  }
                  setHoveredTickIdx(idx);
                }}
                className="relative flex h-[14px] w-full shrink-0 items-center"
              >
                <button
                  type="button"
                  tabIndex={visible ? 0 : -1}
                  aria-label={`Jump to prompt ${promptNumber}: ${p.preview}`}
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

                {visible && hoveredTickIdx === idx && (
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
    prev.prompts[0]?.seq === next.prompts[0]?.seq &&
    prev.prompts[0]?.preview === next.prompts[0]?.preview &&
    prev.prompts[prev.prompts.length - 1]?.seq === next.prompts[next.prompts.length - 1]?.seq &&
    prev.prompts[prev.prompts.length - 1]?.preview === next.prompts[next.prompts.length - 1]?.preview,
);
