import { memo, useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/overlays";
import type { ContextOccupancyBreakdown } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { type MessageKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

function formatTokenShort(tokens: number): string {
  if (!Number.isFinite(tokens) || tokens <= 0) return "0";
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(2)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
  return String(Math.round(tokens));
}

function resolveSegmentLabel(
  category: string,
  fallbackLabel: string,
  t: (key: MessageKey) => string,
): string {
  switch (category) {
    case "system_tools":
      return t("composer.ctxSegSystemTools");
    case "instructions":
      return t("composer.ctxSegInstructions");
    case "rules":
      return t("composer.ctxSegRules");
    case "skills":
      return t("composer.ctxSegSkills");
    case "history":
      return t("composer.ctxSegHistory");
    case "tool_outputs":
      return t("composer.ctxSegToolOutputs");
    case "reasoning":
      return t("composer.ctxSegReasoning");
    case "free":
      return t("composer.ctxSegFree");
    default:
      return fallbackLabel;
  }
}

function segmentBarTone(category: string): string {
  switch (category) {
    case "system_tools":
    case "instructions":
      return "bg-foreground/82";
    case "rules":
      return "bg-foreground/68";
    case "skills":
      return "bg-foreground/54";
    case "history":
      return "bg-foreground/44";
    case "tool_outputs":
      return "bg-foreground/28";
    case "reasoning":
      return "bg-foreground/18";
    case "free":
      return "bg-transparent";
    default:
      return "bg-foreground/30";
  }
}

function segmentDotTone(category: string): string {
  switch (category) {
    case "system_tools":
    case "instructions":
      return "bg-foreground/82";
    case "rules":
      return "bg-foreground/68";
    case "skills":
      return "bg-foreground/54";
    case "history":
      return "bg-foreground/44";
    case "tool_outputs":
      return "bg-foreground/28";
    case "reasoning":
      return "bg-foreground/18";
    default:
      return "bg-ink-faint";
  }
}

type SettledPhase = "running" | "idle" | "awaiting_permission" | "error" | "none";

const OPEN_DELAY_MS = 140;
const CLOSE_DELAY_MS = 180;
const RUNNING_REFRESH_MS = 20_000;

/**
 * Minimalist context window ring in the active session composer (`34px` uniform rhythm).
 * - Uses Base UI `<Popover>` with separate hover-preview vs click-pinned state so clicking right
 *   after hovering never accidentally dismisses the popover.
 * - Refreshes on session change, model switch, turn settle, and throttled `20s` running ticks.
 * - Localizes all breakdown categories, shows interactive segment hover highlighting, model window
 *   scale, high-occupancy numeric hint, and recoverable error retry state.
 */
export const ContextOccupancyPill = memo(function ContextOccupancyPill() {
  const { t } = useI18n();
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const hostId = useOrbitSelector((s) => s.selectedHostId);
  const agentId = useOrbitSelector((s) => s.selectedAgentId);
  const agentContextKey = useOrbitSelector((s): string => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return "none\0";
    const ag = s.hostDetails[s.selectedHostId]?.agents.find(
      (a) => a.agent_id === s.selectedAgentId,
    );
    if (!ag) return "none\0";
    let phase: SettledPhase = "idle";
    if (ag.is_running) phase = "running";
    else if (ag.is_awaiting_permission) phase = "awaiting_permission";
    else {
      const lower = ag.status_label.toLowerCase();
      if (lower.includes("error") || lower.includes("failed")) phase = "error";
    }
    return `${phase}\0${ag.model ?? ""}`;
  });

  const [agentPhaseRaw, activeModelRaw] = agentContextKey.split("\0");
  const agentPhase = (agentPhaseRaw || "none") as SettledPhase;

  const [hoverOpen, setHoverOpen] = useState(false);
  const [pinnedOpen, setPinnedOpen] = useState(false);
  const [breakdown, setBreakdown] = useState<ContextOccupancyBreakdown | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [hoveredCategory, setHoveredCategory] = useState<string | null>(null);

  const open = pinnedOpen || hoverOpen;
  const openTimerRef = useRef<number | null>(null);
  const closeTimerRef = useRef<number | null>(null);
  const reqSeqRef = useRef(0);
  const activeScopeRef = useRef<{ hostId: string | null; agentId: string | null }>({
    hostId,
    agentId,
  });
  activeScopeRef.current = { hostId, agentId };
  const prevPhaseRef = useRef<SettledPhase>(agentPhase);
  const prevModelRef = useRef<string>(activeModelRaw ?? "");

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
        setLoadFailed(false);
      } catch {
        if (
          reqSeqRef.current === seq &&
          activeScopeRef.current.hostId === targetHostId &&
          activeScopeRef.current.agentId === targetAgentId
        ) {
          setLoadFailed(true);
        }
      }
    },
    [],
  );

  useEffect(() => clearTimers, [clearTimers]);

  // Reset immediately and fetch once when switching active session `(hostId, agentId)`.
  useEffect(() => {
    reqSeqRef.current += 1;
    clearTimers();
    setBreakdown(null);
    setLoadFailed(false);
    setHoverOpen(false);
    setPinnedOpen(false);
    setHoveredCategory(null);
    prevPhaseRef.current = agentPhase;
    prevModelRef.current = activeModelRaw ?? "";
    if (isDraft || !hostId || !agentId) {
      return;
    }
    void fetchBreakdown(hostId, agentId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isDraft, hostId, agentId, fetchBreakdown, clearTimers]);

  // Refresh quietly when an agent settles after a turn OR switches models mid-session.
  useEffect(() => {
    const prevPhase = prevPhaseRef.current;
    const prevModel = prevModelRef.current;
    prevPhaseRef.current = agentPhase;
    prevModelRef.current = activeModelRaw ?? "";
    if (isDraft || !hostId || !agentId) return;
    const isSettled =
      agentPhase === "idle" ||
      agentPhase === "awaiting_permission" ||
      agentPhase === "error";
    const modelChanged = prevModel !== (activeModelRaw ?? "");
    if ((prevPhase !== agentPhase && isSettled) || modelChanged) {
      void fetchBreakdown(hostId, agentId);
    }
  }, [agentPhase, activeModelRaw, isDraft, hostId, agentId, fetchBreakdown]);

  // Gentle throttled refresh while a long multi-step turn is running so high occupancy surfaces early.
  useEffect(() => {
    if (isDraft || !hostId || !agentId || agentPhase !== "running") return;
    const id = window.setInterval(() => {
      if (typeof document !== "undefined" && document.hidden) return;
      void fetchBreakdown(hostId, agentId);
    }, RUNNING_REFRESH_MS);
    return () => window.clearInterval(id);
  }, [agentPhase, isDraft, hostId, agentId, fetchBreakdown]);

  const handlePointerEnter = useCallback(() => {
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
    if (hoverOpen || pinnedOpen) return;
    if (openTimerRef.current !== null) return;
    openTimerRef.current = window.setTimeout(() => {
      openTimerRef.current = null;
      setHoverOpen(true);
      if (!isDraft && hostId && agentId) {
        void fetchBreakdown(hostId, agentId);
      }
    }, OPEN_DELAY_MS);
  }, [hoverOpen, pinnedOpen, isDraft, hostId, agentId, fetchBreakdown]);

  const handlePointerLeave = useCallback(() => {
    if (openTimerRef.current !== null) {
      window.clearTimeout(openTimerRef.current);
      openTimerRef.current = null;
    }
    if (pinnedOpen || !hoverOpen) return;
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
    }
    closeTimerRef.current = window.setTimeout(() => {
      closeTimerRef.current = null;
      setHoverOpen(false);
    }, CLOSE_DELAY_MS);
  }, [pinnedOpen, hoverOpen]);

  if (isDraft || !hostId || !agentId) {
    return null;
  }

  // Reserve exact `size-8.5` ring slot while loading so toolbar never jumps horizontally
  if (!breakdown && !loadFailed) {
    return (
      <span
        aria-label={t("composer.contextLoadingAria")}
        className="text-ink-faint inline-flex size-8.5 shrink-0 items-center justify-center rounded-full opacity-55"
      >
        <svg width="15" height="15" viewBox="0 0 15 15" className="shrink-0" aria-hidden="true">
          <circle
            cx="7.5"
            cy="7.5"
            r="5.6"
            fill="none"
            stroke="currentColor"
            strokeOpacity="0.28"
            strokeWidth="1.65"
          />
        </svg>
      </span>
    );
  }

  if (!breakdown && loadFailed) {
    return (
      <Popover
        open={pinnedOpen}
        onOpenChange={(nextOpen) => {
          setPinnedOpen(nextOpen);
          if (nextOpen) void fetchBreakdown(hostId, agentId);
        }}
      >
        <PopoverTrigger asChild>
          <button
            type="button"
            aria-label={t("composer.contextLoadFailed")}
            className="text-ink-faint hover:text-ink-primary hover:bg-accent inline-flex size-8.5 shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors"
          >
            <AlertCircle className="size-3.5 stroke-[1.75]" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          side="top"
          align="end"
          sideOffset={8}
          collisionPadding={12}
          initialFocus={false}
          finalFocus={false}
          className="w-[228px] rounded-[12px] p-3"
        >
          <div className="flex flex-col gap-2">
            <span className="text-ink-secondary text-[12px]">{t("composer.contextLoadFailed")}</span>
            <Button
              type="button"
              variant="outline"
              size="xs"
              onClick={() => {
                setLoadFailed(false);
                void fetchBreakdown(hostId, agentId);
              }}
              className="h-[28px] w-full gap-1.5 text-[11.5px]"
            >
              <RefreshCw className="size-3 stroke-[1.8]" />
              <span>{t("composer.contextRetry")}</span>
            </Button>
          </div>
        </PopoverContent>
      </Popover>
    );
  }

  // TypeScript narrow guard (`breakdown` is guaranteed non-null below)
  if (!breakdown) return null;

  const pct = Math.min(100, Math.max(0, breakdown.occupancy_percent));
  const roundedPct = Math.round(pct);
  const isWarning = pct >= 70 && pct < 90;
  const isCritical = pct >= 90;
  const showCompactPct = pct >= 75;
  const remainingTokens = Math.max(0, breakdown.context_window_tokens - breakdown.total_used_tokens);
  const visibleSegments = breakdown.segments.filter(
    (seg) => seg.category !== "free" && seg.tokens > 0,
  );

  // Minimalist 15px circular progress ring
  const radius = 5.6;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference * (1 - pct / 100);

  return (
    <Popover
      open={open}
      onOpenChange={(nextOpen) => {
        clearTimers();
        if (!nextOpen) {
          setPinnedOpen(false);
          setHoverOpen(false);
          setHoveredCategory(null);
        }
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-expanded={open}
          aria-label={t("composer.contextWindowAria", { percent: roundedPct })}
          onMouseEnter={handlePointerEnter}
          onMouseLeave={handlePointerLeave}
          onClick={(e) => {
            e.preventDefault();
            clearTimers();
            setPinnedOpen((prev) => {
              const next = !prev;
              if (next) {
                setHoverOpen(false);
                void fetchBreakdown(hostId, agentId);
              } else {
                setHoverOpen(false);
              }
              return next;
            });
          }}
          className={cn(
            "inline-flex h-8.5 shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors",
            showCompactPct ? "gap-1 px-2" : "w-8.5",
            open ? "bg-accent" : "hover:bg-accent",
            isCritical
              ? "text-signal-danger"
              : isWarning
                ? "text-signal-warning"
                : open
                  ? "text-ink-primary"
                  : "text-ink-muted hover:text-ink-primary",
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
          {showCompactPct && (
            <span className="tabular-nums font-mono text-[11px] font-medium leading-none">
              {roundedPct}%
            </span>
          )}
        </button>
      </PopoverTrigger>

      <PopoverContent
        side="top"
        align="end"
        sideOffset={8}
        collisionPadding={12}
        initialFocus={false}
        finalFocus={false}
        aria-label={t("composer.contextWindowUsage")}
        onMouseEnter={handlePointerEnter}
        onMouseLeave={handlePointerLeave}
        className="w-[264px] rounded-[12px] p-3"
      >
        <div className="flex flex-col gap-2">
          {/* Header: quiet label + crisp percentage */}
          <div className="flex items-baseline justify-between gap-2">
            <span className="text-ink-primary truncate text-[12px] font-medium tracking-[-0.01em]">
              {t("composer.contextWindow")}
            </span>
            <span
              className={cn(
                "tabular-nums shrink-0 font-mono text-[11.5px] font-medium",
                isCritical
                  ? "text-signal-danger"
                  : isWarning
                    ? "text-signal-warning"
                    : "text-ink-primary",
              )}
            >
              {pct >= 10 || pct === 0 ? `${roundedPct}%` : `${pct.toFixed(1)}%`}
            </span>
          </div>

          {/* Ultra-fine 4px monochrome stacked bar with subtle 1.5px segment cuts & hover highlight */}
          <div className="flex flex-col gap-1">
            <div className="bg-surface-recessed flex h-[4px] w-full gap-[1.5px] overflow-hidden rounded-full">
              {visibleSegments.map((seg) => {
                const dimmed = hoveredCategory !== null && hoveredCategory !== seg.category;
                return (
                  <div
                    key={seg.category}
                    style={{ width: `${Math.max(1, seg.percent)}%` }}
                    className={cn(
                      "h-full transition-opacity duration-150",
                      segmentBarTone(seg.category),
                      dimmed && "opacity-25",
                    )}
                  />
                );
              })}
            </div>
            <div className="text-ink-muted tabular-nums flex items-center justify-between font-mono text-[10.5px]">
              <span>
                {formatTokenShort(breakdown.total_used_tokens)} /{" "}
                {formatTokenShort(breakdown.context_window_tokens)}
              </span>
              <span>
                {t("composer.contextLeft", { remaining: formatTokenShort(remainingTokens) })}
              </span>
            </div>
          </div>

          {/* Non-zero category breakdown (localized, scannable, hover-linked to progress bar) */}
          {visibleSegments.length > 0 && (
            <div
              className="border-border-subtle flex flex-col gap-0.5 border-t pt-1.5"
              onMouseLeave={() => setHoveredCategory(null)}
            >
              {visibleSegments.map((seg) => {
                const segPct = seg.percent >= 1 ? `${Math.round(seg.percent)}%` : "<1%";
                const active = hoveredCategory === seg.category;
                return (
                  <div
                    key={seg.category}
                    onMouseEnter={() => setHoveredCategory(seg.category)}
                    className={cn(
                      "flex items-center justify-between gap-2 rounded-[6px] px-1 py-0.5 text-[11.5px] leading-[16px] transition-colors",
                      active && "bg-accent/65",
                    )}
                  >
                    <span className="flex min-w-0 items-center gap-1.5">
                      <span
                        className={cn(
                          "size-1.5 shrink-0 rounded-full",
                          segmentDotTone(seg.category),
                        )}
                      />
                      <span className="text-ink-secondary truncate">
                        {resolveSegmentLabel(seg.category, seg.label, t)}
                      </span>
                    </span>
                    <span className="text-ink-muted tabular-nums flex shrink-0 items-baseline gap-1.5 font-mono text-[11px]">
                      <span>{formatTokenShort(seg.tokens)}</span>
                      <span className="text-ink-faint w-[28px] text-right text-[10px]">
                        {segPct}
                      </span>
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
});
