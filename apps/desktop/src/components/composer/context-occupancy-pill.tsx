import { memo, useCallback, useEffect, useRef, useState } from "react";
import { FoldVertical } from "lucide-react";
import type { ContextOccupancyBreakdown } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function formatTokenShort(tokens: number): string {
  if (!Number.isFinite(tokens) || tokens <= 0) return "0";
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(2)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
  return String(Math.round(tokens));
}

function segmentBarTone(category: string): string {
  switch (category) {
    case "instructions":
      return "bg-foreground/85";
    case "rules":
      return "bg-foreground/68";
    case "skills":
      return "bg-foreground/54";
    case "history":
      return "bg-foreground/40";
    case "tool_outputs":
      return "bg-foreground/26";
    case "reasoning":
      return "bg-foreground/16";
    default:
      return "bg-transparent";
  }
}

function segmentDotTone(category: string): string {
  switch (category) {
    case "instructions":
      return "bg-foreground/85";
    case "rules":
      return "bg-foreground/68";
    case "skills":
      return "bg-foreground/54";
    case "history":
      return "bg-foreground/40";
    case "tool_outputs":
      return "bg-foreground/26";
    case "reasoning":
      return "bg-foreground/16";
    default:
      return "bg-muted-foreground/25";
  }
}

type SettledPhase = "running" | "idle" | "awaiting_permission" | "error" | "none";

const OPEN_DELAY_MS = 120;
const CLOSE_DELAY_MS = 150;

/**
 * Minimalist context window ring in the active session composer.
 * - Hover opens an interactive breakdown card with a seamless hover bridge (`pb-2`) and
 *   a 150ms close grace period so moving the cursor from the ring onto the card never dismisses it.
 * - Also supports direct click/keyboard toggle for quick access to `/compact`.
 */
export const ContextOccupancyPill = memo(function ContextOccupancyPill() {
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const hostId = useOrbitSelector((s) => s.selectedHostId);
  const agentId = useOrbitSelector((s) => s.selectedAgentId);
  const agentPhase = useOrbitSelector((s): SettledPhase => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return "none";
    const ag = s.hostDetails[s.selectedHostId]?.agents.find(
      (a) => a.agent_id === s.selectedAgentId,
    );
    if (!ag) return "none";
    if (ag.is_running) return "running";
    if (ag.is_awaiting_permission) return "awaiting_permission";
    const lower = ag.status_label.toLowerCase();
    if (lower.includes("error") || lower.includes("failed")) return "error";
    return "idle";
  });

  const [open, setOpen] = useState(false);
  const [breakdown, setBreakdown] = useState<ContextOccupancyBreakdown | null>(null);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const openTimerRef = useRef<number | null>(null);
  const closeTimerRef = useRef<number | null>(null);
  const reqSeqRef = useRef(0);
  const activeScopeRef = useRef<{ hostId: string | null; agentId: string | null }>({
    hostId,
    agentId,
  });
  activeScopeRef.current = { hostId, agentId };
  const prevPhaseRef = useRef<SettledPhase>(agentPhase);

  const clearTimers = useCallback(() => {
    if (openTimerRef.current !== null) {
      window.clearTimeout(openTimerRef.current);
      openTimerRef.current = null;
    }
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
  }, []);

  const fetchBreakdown = useCallback(
    async (targetHostId: string | null, targetAgentId: string | null) => {
      if (!targetHostId || !targetAgentId) return;
      const seq = ++reqSeqRef.current;
      try {
        const next = await orbitActions.getContextOccupancy(targetHostId, targetAgentId);
        if (
          reqSeqRef.current !== seq ||
          activeScopeRef.current.hostId !== targetHostId ||
          activeScopeRef.current.agentId !== targetAgentId ||
          next.agent_id !== targetAgentId
        ) {
          return;
        }
        setBreakdown(next);
      } catch {
        // Quietly ignore transient disconnects in ambient meter
      }
    },
    [],
  );

  useEffect(() => clearTimers, [clearTimers]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      if (
        containerRef.current &&
        e.target instanceof Node &&
        !containerRef.current.contains(e.target)
      ) {
        clearTimers();
        setOpen(false);
      }
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [open, clearTimers]);

  // Reset immediately and fetch once when switching active session `(hostId, agentId)`.
  useEffect(() => {
    reqSeqRef.current += 1;
    clearTimers();
    setBreakdown(null);
    setOpen(false);
    prevPhaseRef.current = agentPhase;
    if (isDraft || !hostId || !agentId) {
      return;
    }
    void fetchBreakdown(hostId, agentId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isDraft, hostId, agentId, fetchBreakdown, clearTimers]);

  // Refresh quietly when an agent settles after a turn.
  useEffect(() => {
    const prev = prevPhaseRef.current;
    prevPhaseRef.current = agentPhase;
    if (isDraft || !hostId || !agentId) return;
    const isSettled =
      agentPhase === "idle" ||
      agentPhase === "awaiting_permission" ||
      agentPhase === "error";
    if (prev !== agentPhase && isSettled) {
      void fetchBreakdown(hostId, agentId);
    }
  }, [agentPhase, isDraft, hostId, agentId, fetchBreakdown]);

  const handlePointerEnter = useCallback(() => {
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
    if (open) return;
    if (openTimerRef.current !== null) return;
    openTimerRef.current = window.setTimeout(() => {
      openTimerRef.current = null;
      setOpen(true);
      if (!isDraft && hostId && agentId) {
        void fetchBreakdown(hostId, agentId);
      }
    }, OPEN_DELAY_MS);
  }, [open, isDraft, hostId, agentId, fetchBreakdown]);

  const handlePointerLeave = useCallback(() => {
    if (openTimerRef.current !== null) {
      window.clearTimeout(openTimerRef.current);
      openTimerRef.current = null;
    }
    if (!open) return;
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
    }
    closeTimerRef.current = window.setTimeout(() => {
      closeTimerRef.current = null;
      setOpen(false);
    }, CLOSE_DELAY_MS);
  }, [open]);

  if (isDraft || !hostId || !agentId || !breakdown) {
    return null;
  }

  const pct = Math.min(100, Math.max(0, breakdown.occupancy_percent));
  const roundedPct = Math.round(pct);
  const isWarning = pct >= 70 && pct < 90;
  const isCritical = pct >= 90;
  const remainingTokens = Math.max(0, breakdown.context_window_tokens - breakdown.total_used_tokens);
  const visibleSegments = breakdown.segments.filter(
    (seg) => seg.category !== "free" && seg.tokens > 0,
  );

  // Minimalist 15px circular progress ring
  const radius = 5.6;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference * (1 - pct / 100);

  return (
    <div
      ref={containerRef}
      className="relative inline-flex size-8 shrink-0 items-center justify-center"
      onMouseEnter={handlePointerEnter}
      onMouseLeave={handlePointerLeave}
      onBlur={(e) => {
        if (open && !e.currentTarget.contains(e.relatedTarget as Node | null)) {
          clearTimers();
          setOpen(false);
        }
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          clearTimers();
          setOpen(false);
        }
      }}
    >
      <button
        type="button"
        aria-expanded={open}
        aria-label={`Context window ${roundedPct}% used`}
        onClick={() => {
          clearTimers();
          setOpen((prev) => {
            const next = !prev;
            if (next && !isDraft && hostId && agentId) {
              void fetchBreakdown(hostId, agentId);
            }
            return next;
          });
        }}
        className={cn(
          "inline-flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors",
          open
            ? "bg-black/[0.06] dark:bg-white/[0.08]"
            : "hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
          isCritical
            ? "text-rose-600/90 dark:text-rose-400/90"
            : isWarning
              ? "text-amber-600/85 dark:text-amber-400/85"
              : open
                ? "text-foreground/80"
                : "text-foreground/48 hover:text-foreground/78",
        )}
      >
        <svg
          width="15"
          height="15"
          viewBox="0 0 15 15"
          className="shrink-0 -rotate-90"
          aria-hidden="true"
        >
          <circle
            cx="7.5"
            cy="7.5"
            r={radius}
            fill="none"
            stroke="currentColor"
            strokeOpacity="0.22"
            strokeWidth="1.65"
          />
          <circle
            cx="7.5"
            cy="7.5"
            r={radius}
            fill="none"
            stroke="currentColor"
            strokeWidth="1.65"
            strokeDasharray={circumference}
            strokeDashoffset={dashOffset}
            strokeLinecap="round"
          />
        </svg>
      </button>

      {open && (
        /* Transparent `pb-2` hover bridge connects the trigger and card with zero dead gap */
        <div
          className="absolute right-0 bottom-full z-50 pb-2"
          onMouseEnter={handlePointerEnter}
          onMouseLeave={handlePointerLeave}
        >
          <div
            role="dialog"
            aria-label="Context window usage"
            className="bg-popover text-popover-foreground floating-surface orbit-inline-menu-up w-[244px] rounded-[12px] border border-black/[0.08] p-3 dark:border-white/[0.10]"
          >
            <div className="space-y-2">
              {/* Header: quiet label + crisp percentage */}
              <div className="flex items-baseline justify-between gap-2">
                <span className="text-foreground/90 text-[12px] font-medium tracking-[-0.01em]">
                  Context window
                </span>
                <span
                  className={cn(
                    "tabular-nums font-mono text-[11.5px] font-medium",
                    isCritical
                      ? "text-rose-600 dark:text-rose-400"
                      : isWarning
                        ? "text-amber-600 dark:text-amber-400"
                        : "text-foreground/85",
                  )}
                >
                  {pct >= 10 || pct === 0 ? `${roundedPct}%` : `${pct.toFixed(1)}%`}
                </span>
              </div>

              {/* Ultra-fine 3px monochrome stacked bar + token balance */}
              <div className="space-y-1">
                <div className="bg-muted flex h-[3px] w-full overflow-hidden rounded-full">
                  {visibleSegments.map((seg) => (
                    <div
                      key={seg.category}
                      style={{ width: `${Math.max(0.8, seg.percent)}%` }}
                      className={cn("h-full", segmentBarTone(seg.category))}
                    />
                  ))}
                </div>
                <div className="text-muted-foreground/80 tabular-nums flex items-center justify-between font-mono text-[10.5px]">
                  <span>
                    {formatTokenShort(breakdown.total_used_tokens)} /{" "}
                    {formatTokenShort(breakdown.context_window_tokens)}
                  </span>
                  <span>{formatTokenShort(remainingTokens)} left</span>
                </div>
              </div>

              {/* Non-zero category breakdown (quiet, scannable, zero clutter) */}
              {visibleSegments.length > 0 && (
                <div className="border-border/50 space-y-1 border-t pt-1.5">
                  {visibleSegments.map((seg) => (
                    <div
                      key={seg.category}
                      className="flex items-center justify-between gap-2 text-[11.5px] leading-[16px]"
                    >
                      <span className="flex min-w-0 items-center gap-1.5">
                        <span
                          className={cn(
                            "size-1.5 shrink-0 rounded-full",
                            segmentDotTone(seg.category),
                          )}
                        />
                        <span className="text-foreground/78 truncate">{seg.label}</span>
                      </span>
                      <span className="text-muted-foreground/85 tabular-nums shrink-0 font-mono text-[11px]">
                        {formatTokenShort(seg.tokens)}
                      </span>
                    </div>
                  ))}
                </div>
              )}

              {/* Quick /compact trigger when context is tight */}
              {pct >= 75 && (
                <div className="border-border/50 border-t pt-2">
                  <button
                    type="button"
                    onClick={() => {
                      clearTimers();
                      setOpen(false);
                      void orbitActions.submitSlashCompact();
                    }}
                    className="border-border hover:bg-accent text-foreground inline-flex h-[28px] w-full cursor-pointer items-center justify-center gap-1.5 rounded-lg border px-2.5 text-[11.5px] font-medium transition-colors"
                  >
                    <FoldVertical className="size-3.5 stroke-[1.8]" />
                    <span>Compact context (/compact)</span>
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
});
