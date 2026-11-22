import { memo, useCallback, useEffect, useRef, useState } from "react";
import { FoldVertical, Loader2, RefreshCw } from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/overlays";
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
      return "bg-foreground/90";
    case "rules":
      return "bg-foreground/72";
    case "skills":
      return "bg-foreground/56";
    case "history":
      return "bg-foreground/42";
    case "tool_outputs":
      return "bg-foreground/28";
    case "reasoning":
      return "bg-foreground/18";
    default:
      return "bg-transparent";
  }
}

function segmentDotTone(category: string): string {
  switch (category) {
    case "instructions":
      return "bg-foreground/90";
    case "rules":
      return "bg-foreground/70";
    case "skills":
      return "bg-foreground/55";
    case "history":
      return "bg-foreground/40";
    case "tool_outputs":
      return "bg-foreground/25";
    case "reasoning":
      return "bg-foreground/16";
    default:
      return "bg-muted-foreground/25";
  }
}

type SettledPhase = "running" | "idle" | "awaiting_permission" | "error" | "none";

/**
 * Subtle compact Context Occupancy micro-pill in the active session composer.
 * Displays live percentage occupancy and opens a crisp stacked-bar breakdown popover
 * with a quick `/compact` trigger when occupancy reaches >= 75%.
 */
export const ContextOccupancyPill = memo(function ContextOccupancyPill({
  compact = false,
}: {
  compact?: boolean;
}) {
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
  const [loading, setLoading] = useState(false);
  const [breakdown, setBreakdown] = useState<ContextOccupancyBreakdown | null>(null);

  const reqSeqRef = useRef(0);
  const activeScopeRef = useRef<{ hostId: string | null; agentId: string | null }>({
    hostId,
    agentId,
  });
  activeScopeRef.current = { hostId, agentId };
  const prevPhaseRef = useRef<SettledPhase>(agentPhase);

  const fetchBreakdown = useCallback(
    async (targetHostId: string | null, targetAgentId: string | null) => {
      if (!targetHostId || !targetAgentId) return;
      const seq = ++reqSeqRef.current;
      setLoading(true);
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
        // Quietly ignore transient disconnects in ambient pill
      } finally {
        if (reqSeqRef.current === seq) {
          setLoading(false);
        }
      }
    },
    [],
  );

  // Reset immediately and fetch once when switching active session `(hostId, agentId)`.
  useEffect(() => {
    reqSeqRef.current += 1;
    setBreakdown(null);
    setLoading(false);
    prevPhaseRef.current = agentPhase;
    if (isDraft || !hostId || !agentId) {
      return;
    }
    void fetchBreakdown(hostId, agentId);
    // Intentionally scoped to session identity changes; settled transitions are handled below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isDraft, hostId, agentId, fetchBreakdown]);

  // Refresh ONLY when an agent transitions into a settled state (`idle` / `awaiting_permission` / `error`).
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

  if (isDraft || !hostId || !agentId) {
    return null;
  }

  const pct = breakdown ? Math.min(100, Math.max(0, breakdown.occupancy_percent)) : 0;
  const roundedPct = Math.round(pct);
  const isWarning = pct >= 75 && pct < 90;
  const isCritical = pct >= 90;

  // SVG circular micro-ring metrics
  const radius = 5.25;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference * (1 - pct / 100);

  return (
    <Popover
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (nextOpen) {
          void fetchBreakdown(hostId, agentId);
        }
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          title={
            breakdown
              ? `Context occupancy: ${roundedPct}% (${formatTokenShort(breakdown.total_used_tokens)} / ${formatTokenShort(breakdown.context_window_tokens)} tokens)`
              : "Inspect context occupancy"
          }
          className={cn(
            "inline-flex h-8.5 shrink-0 cursor-pointer items-center justify-center gap-1.5 rounded-full border border-transparent text-[12px] transition-colors",
            compact ? "w-8.5 px-0" : "px-2",
            isCritical
              ? "text-rose-600 hover:bg-rose-500/10 dark:text-rose-400"
              : isWarning
                ? "text-amber-600 hover:bg-amber-500/10 dark:text-amber-400"
                : "text-foreground/60 hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
          )}
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 14 14"
            className="shrink-0 -rotate-90"
            aria-hidden="true"
          >
            <circle
              cx="7"
              cy="7"
              r={radius}
              fill="none"
              stroke="currentColor"
              strokeOpacity="0.2"
              strokeWidth="1.6"
            />
            <circle
              cx="7"
              cy="7"
              r={radius}
              fill="none"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeDasharray={circumference}
              strokeDashoffset={dashOffset}
              strokeLinecap="round"
            />
          </svg>
          {!compact && (
            <span className="tabular-nums font-mono text-[11.5px]">
              {breakdown ? `${roundedPct}% ctx` : "ctx"}
            </span>
          )}
        </button>
      </PopoverTrigger>

      <PopoverContent
        side="top"
        align="end"
        sideOffset={8}
        finalFocus={false}
        className="w-[292px] p-3"
      >
        <div className="space-y-3">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="text-foreground text-[12.5px] font-medium">
                Context Window
              </div>
              <div className="text-muted-foreground truncate font-mono text-[11px]">
                {breakdown?.model ?? "Loading…"}
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              <span
                className={cn(
                  "tabular-nums font-mono text-[12px] font-medium",
                  isCritical
                    ? "text-rose-600 dark:text-rose-400"
                    : isWarning
                      ? "text-amber-600 dark:text-amber-400"
                      : "text-foreground",
                )}
              >
                {breakdown ? `${pct.toFixed(1)}%` : "—"}
              </span>
              <button
                type="button"
                aria-label="Refresh context breakdown"
                disabled={loading}
                onClick={() => void fetchBreakdown(hostId, agentId)}
                className="text-muted-foreground hover:text-foreground inline-flex size-5 cursor-pointer items-center justify-center rounded transition-colors"
              >
                {loading ? (
                  <Loader2 className="size-3 animate-spin" />
                ) : (
                  <RefreshCw className="size-3" />
                )}
              </button>
            </div>
          </div>

          {breakdown ? (
            <>
              {/* Stacked monochrome occupancy bar */}
              <div className="space-y-1">
                <div className="bg-muted flex h-2 w-full overflow-hidden rounded-full">
                  {breakdown.segments
                    .filter((seg) => seg.category !== "free" && seg.percent > 0)
                    .map((seg) => (
                      <div
                        key={seg.category}
                        style={{ width: `${Math.max(0.5, seg.percent)}%` }}
                        className={cn("h-full transition-all duration-200", segmentBarTone(seg.category))}
                        title={`${seg.label}: ${formatTokenShort(seg.tokens)} (${seg.percent.toFixed(1)}%)`}
                      />
                    ))}
                </div>
                <div className="text-muted-foreground tabular-nums flex items-center justify-between font-mono text-[11px]">
                  <span>{formatTokenShort(breakdown.total_used_tokens)} used</span>
                  <span>{formatTokenShort(breakdown.context_window_tokens)} max</span>
                </div>
              </div>

              {/* Hairline segment rows */}
              <div className="divide-border/50 divide-y">
                {breakdown.segments
                  .filter((seg) => seg.tokens > 0 || seg.category === "free")
                  .map((seg) => (
                    <div
                      key={seg.category}
                      className="flex items-center justify-between gap-2 py-1.5 text-[12px]"
                    >
                      <span className="flex min-w-0 items-center gap-2">
                        <span
                          className={cn(
                            "size-2 shrink-0 rounded-xs",
                            segmentDotTone(seg.category),
                          )}
                        />
                        <span
                          className={cn(
                            "truncate",
                            seg.category === "free"
                              ? "text-muted-foreground"
                              : "text-foreground/90",
                          )}
                        >
                          {seg.label}
                        </span>
                      </span>
                      <span className="text-muted-foreground tabular-nums shrink-0 font-mono text-[11.5px]">
                        {formatTokenShort(seg.tokens)}{" "}
                        <span className="text-muted-foreground/70">
                          ({seg.percent.toFixed(1)}%)
                        </span>
                      </span>
                    </div>
                  ))}
              </div>

              {/* Quick /compact trigger when occupancy >= 75% */}
              {pct >= 75 && (
                <div className="border-border/60 border-t pt-2.5">
                  <button
                    type="button"
                    onClick={() => {
                      setOpen(false);
                      void orbitActions.submitSlashCompact();
                    }}
                    className="border-border hover:bg-accent text-foreground inline-flex h-[30px] w-full cursor-pointer items-center justify-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors"
                  >
                    <FoldVertical className="size-3.5 stroke-[1.8]" />
                    <span>Compact Context (/compact)</span>
                  </button>
                </div>
              )}
            </>
          ) : (
            <div className="text-muted-foreground flex items-center justify-center gap-2 py-4 text-[12px]">
              <Loader2 className="size-3.5 animate-spin" />
              <span>Measuring context…</span>
            </div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
});
