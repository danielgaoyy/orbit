import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Folder, RefreshCw } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { Progress } from "@/components/ui/progress";
import { ProviderIcon } from "@/components/ui/provider-icons";
import {
  PROVIDER_LIST,
  type ProviderKind,
  type QuotaDepletionForecast,
  type UsageDailyPoint,
  type UsageInsightsSnapshot,
  formatProviderLabel,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type RangeDays = 7 | 30;
type ChartMetric = "cost" | "tokens";

function formatProviderName(provider: string): string {
  return formatProviderLabel(provider);
}

function toProviderKind(provider: string): ProviderKind {
  return PROVIDER_LIST.some((p) => p.id === provider)
    ? (provider as ProviderKind)
    : "acp";
}

function formatUsd(amount: number): string {
  if (!Number.isFinite(amount) || amount <= 0) return "$0.00";
  if (amount < 0.01) return `$${amount.toFixed(4)}`;
  if (amount < 100) return `$${amount.toFixed(2)}`;
  return `$${amount.toLocaleString(undefined, { maximumFractionDigits: 2 })}`;
}

function formatCompactTokens(tokens: number): string {
  if (!Number.isFinite(tokens) || tokens <= 0) return "0";
  if (tokens >= 1_000_000_000) return `${(tokens / 1_000_000_000).toFixed(2)}B`;
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(2)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
  return String(Math.round(tokens));
}

function formatDurationShort(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return "now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${Math.max(1, mins)}m`;
  const hours = Math.floor(mins / 60);
  const remMins = mins % 60;
  if (hours < 24) {
    return remMins > 0 ? `${hours}h ${remMins}m` : `${hours}h`;
  }
  const days = Math.floor(hours / 24);
  const remHours = hours % 24;
  return remHours > 0 ? `${days}d ${remHours}h` : `${days}d`;
}

function formatResetTime(resetsAt: string | null): string | null {
  if (!resetsAt) return null;
  const ts = Date.parse(resetsAt);
  if (Number.isNaN(ts)) return resetsAt;
  const diffSecs = Math.round((ts - Date.now()) / 1000);
  if (diffSecs <= 0) return "Resets soon";
  return `Resets in ${formatDurationShort(diffSecs)}`;
}

function formatDayShort(isoDate: string): string {
  const parts = isoDate.split("-");
  if (parts.length !== 3) return isoDate;
  return `${parts[1]}/${parts[2]}`;
}

function basenameOfPath(cwd: string): string {
  const trimmed = cwd.replace(/[\\/]+$/, "");
  if (!trimmed || trimmed === "~") return "~";
  const idx = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return idx >= 0 ? trimmed.slice(idx + 1) || trimmed : trimmed;
}

function formatPercent(ratio: number): string {
  if (!Number.isFinite(ratio) || ratio <= 0) return "0%";
  const pct = ratio * 100;
  if (pct < 1) return "<1%";
  return `${Math.round(pct)}%`;
}

function DailyUsageBarChart({
  daily,
  metric,
}: {
  daily: UsageDailyPoint[];
  metric: ChartMetric;
}) {
  const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

  const values = useMemo(
    () =>
      daily.map((d) =>
        metric === "cost"
          ? d.cost_usd
          : d.input_tokens + d.output_tokens + d.cache_read_tokens,
      ),
    [daily, metric],
  );

  const { maxVal, peakIdx } = useMemo(() => {
    let best = 0;
    let bestIndex = -1;
    values.forEach((v, i) => {
      if (v > best) {
        best = v;
        bestIndex = i;
      }
    });
    return { maxVal: best, peakIdx: bestIndex };
  }, [values]);

  if (daily.length === 0) {
    return (
      <div className="text-ink-muted py-10 text-center text-[12.5px]">
        No daily usage recorded yet.
      </div>
    );
  }

  const width = 760;
  const height = 132;
  const padLeft = 4;
  const padRight = 56;
  const padTop = 14;
  const padBottom = 22;
  const plotW = width - padLeft - padRight;
  const plotH = height - padTop - padBottom;
  const n = daily.length;
  const step = plotW / n;
  const barW = Math.max(4, Math.min(28, step * 0.58));

  const hovered = hoveredIdx !== null ? daily[hoveredIdx] : null;
  const labelStep = n <= 7 ? 1 : n <= 14 ? 2 : 5;
  const formatTick = (v: number) =>
    metric === "cost" ? formatUsd(v) : formatCompactTokens(v);

  return (
    <div className="space-y-2.5">
      <div className="flex min-h-[20px] items-center justify-between gap-2 text-[11.5px]">
        {hovered ? (
          <div className="text-ink-primary tabular-nums flex flex-wrap items-center gap-x-3 gap-y-0.5">
            <span className="font-mono font-medium">{hovered.date}</span>
            <span className="font-mono font-semibold">{formatUsd(hovered.cost_usd)}</span>
            <span className="text-ink-secondary font-mono">
              {formatCompactTokens(
                hovered.input_tokens + hovered.output_tokens + hovered.cache_read_tokens,
              )}{" "}
              tok
            </span>
            <span className="text-ink-muted">
              {hovered.turns} {hovered.turns === 1 ? "turn" : "turns"}
            </span>
          </div>
        ) : (
          <span className="text-ink-muted">
            {metric === "cost"
              ? "Daily estimated spend · hover bars to inspect"
              : "Daily token volume · hover bars to inspect"}
          </span>
        )}
        <span className="text-ink-faint tabular-nums font-mono text-[11px]">
          {maxVal > 0 ? `Peak ${formatTick(maxVal)}` : "No activity"}
        </span>
      </div>

      <svg
        viewBox={`0 0 ${width} ${height}`}
        className="h-[132px] w-full overflow-visible select-none"
        role="img"
        aria-label={metric === "cost" ? "Daily spend bar chart" : "Daily token volume bar chart"}
        onMouseLeave={() => setHoveredIdx(null)}
      >
        {/* Peak hairline + label */}
        <line
          x1={padLeft}
          x2={width - padRight}
          y1={padTop}
          y2={padTop}
          className="stroke-border-subtle/80"
          strokeDasharray="2 3"
          strokeWidth={1}
        />
        {maxVal > 0 && (
          <text
            x={width - padRight + 8}
            y={padTop + 3.5}
            className="fill-ink-faint font-mono text-[9.5px]"
          >
            {formatTick(maxVal)}
          </text>
        )}

        {/* Mid hairline + label */}
        <line
          x1={padLeft}
          x2={width - padRight}
          y1={padTop + plotH / 2}
          y2={padTop + plotH / 2}
          className="stroke-border-subtle/60"
          strokeDasharray="2 3"
          strokeWidth={1}
        />
        {maxVal > 0 && (
          <text
            x={width - padRight + 8}
            y={padTop + plotH / 2 + 3.5}
            className="fill-ink-faint font-mono text-[9.5px]"
          >
            {formatTick(maxVal / 2)}
          </text>
        )}

        {/* Zero baseline */}
        <line
          x1={padLeft}
          x2={width - padRight}
          y1={padTop + plotH}
          y2={padTop + plotH}
          className="stroke-border-default"
          strokeWidth={1}
        />
        <text
          x={width - padRight + 8}
          y={padTop + plotH + 3.5}
          className="fill-ink-faint font-mono text-[9.5px]"
        >
          0
        </text>

        {daily.map((d, idx) => {
          const val = values[idx];
          const ratio = maxVal > 0 ? val / maxVal : 0;
          const barH = val > 0 ? Math.max(3, ratio * plotH) : 1.5;
          const x = padLeft + idx * step + (step - barW) / 2;
          const y = padTop + plotH - barH;
          const centerX = padLeft + idx * step + step / 2;
          const isHovered = hoveredIdx === idx;
          const isDimmed = hoveredIdx !== null && hoveredIdx !== idx;
          const isPeak = idx === peakIdx && maxVal > 0;
          const showLabel = idx === 0 || idx === n - 1 || idx % labelStep === 0;

          return (
            <g
              key={d.date}
              className="cursor-pointer"
              onMouseEnter={() => setHoveredIdx(idx)}
            >
              {/* Invisible full-column hit target */}
              <rect
                x={padLeft + idx * step}
                y={0}
                width={step}
                height={height}
                fill="transparent"
              />

              {/* Subtle hover column highlight */}
              {isHovered && (
                <rect
                  x={padLeft + idx * step + 1}
                  y={padTop - 4}
                  width={Math.max(2, step - 2)}
                  height={plotH + 4}
                  rx={4}
                  className="fill-ink-primary/[0.04]"
                />
              )}

              <rect
                x={x}
                y={y}
                width={barW}
                height={barH}
                rx={Math.min(3, barW / 2)}
                className={cn(
                  "transition-all duration-150",
                  val === 0
                    ? "fill-border-default/50"
                    : isHovered
                      ? "fill-ink-primary"
                      : isDimmed
                        ? "fill-ink-primary/25"
                        : "fill-ink-primary/70",
                )}
              />

              {/* Peak marker dot */}
              {isPeak && (
                <circle
                  cx={centerX}
                  cy={Math.max(5, y - 6)}
                  r={2.25}
                  className={cn(
                    "transition-opacity duration-150",
                    isDimmed ? "fill-ink-muted/40" : "fill-ink-primary",
                  )}
                />
              )}

              {showLabel && (
                <text
                  x={centerX}
                  y={height - 4}
                  textAnchor="middle"
                  className={cn(
                    "font-mono text-[9.5px] transition-colors",
                    isHovered ? "fill-ink-primary font-medium" : "fill-ink-faint",
                  )}
                >
                  {formatDayShort(d.date)}
                </text>
              )}
            </g>
          );
        })}
      </svg>
    </div>
  );
}

export function HostUsageSection({ hostId }: { hostId: string }) {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostCard = fleet.hosts.find((h) => h.host_id === hostId) ?? null;
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const [rangeDays, setRangeDays] = useState<RangeDays>(7);
  const [chartMetric, setChartMetric] = useState<ChartMetric>("cost");
  const [insights, setInsights] = useState<UsageInsightsSnapshot | null>(null);
  const [loadingInsights, setLoadingInsights] = useState(false);
  const [insightsError, setInsightsError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const autoFetchedHostRef = useRef<string | null>(null);
  const reqSeqRef = useRef(0);
  const activeHostRef = useRef(hostId);
  activeHostRef.current = hostId;

  const quotas = hostDetail?.quotas ?? [];
  const showHostBadge =
    Boolean(hostCard) &&
    (fleet.hosts.length > 1 || (hostCard !== null && !isLocalHostCard(hostCard)));

  const fetchInsights = useCallback(
    async (targetHostId: string, days: RangeDays) => {
      const seq = ++reqSeqRef.current;
      setLoadingInsights(true);
      setInsightsError(null);
      try {
        const snap = await orbitActions.getUsageInsights(targetHostId, days);
        if (reqSeqRef.current !== seq || activeHostRef.current !== targetHostId) {
          return;
        }
        setInsights(snap);
      } catch (err) {
        if (reqSeqRef.current !== seq || activeHostRef.current !== targetHostId) {
          return;
        }
        setInsightsError(String(err));
      } finally {
        if (reqSeqRef.current === seq) {
          setLoadingInsights(false);
        }
      }
    },
    [],
  );

  // Immediately clear stale host telemetry when switching hosts.
  useEffect(() => {
    reqSeqRef.current += 1;
    setInsights(null);
    setInsightsError(null);
    setRefreshing(false);
  }, [hostId]);

  useEffect(() => {
    void fetchInsights(hostId, rangeDays);
  }, [fetchInsights, hostId, rangeDays]);

  useEffect(() => {
    if (quotas.length > 0 || autoFetchedHostRef.current === hostId) return;
    autoFetchedHostRef.current = hostId;
    let cancelled = false;
    let timerId: number | null = null;
    setRefreshing(true);
    void (async () => {
      try {
        await orbitIpc.refreshQuotas(hostId);
      } catch {
        // Ignore transient quota probe failures
      }
      if (cancelled || activeHostRef.current !== hostId) return;
      await fetchInsights(hostId, rangeDays);
      if (cancelled || activeHostRef.current !== hostId) return;
      timerId = window.setTimeout(() => {
        if (!cancelled && activeHostRef.current === hostId) {
          setRefreshing(false);
        }
      }, 350);
    })();
    return () => {
      cancelled = true;
      if (timerId !== null) {
        window.clearTimeout(timerId);
      }
    };
  }, [hostId, quotas.length, fetchInsights, rangeDays]);

  const handleRefresh = async () => {
    const targetHostId = hostId;
    setRefreshing(true);
    try {
      try {
        await orbitIpc.refreshQuotas(targetHostId);
      } catch {
        // Still reload ledger insights even if live quota probe fails
      }
      if (activeHostRef.current === targetHostId) {
        await fetchInsights(targetHostId, rangeDays);
      }
    } finally {
      window.setTimeout(() => {
        if (activeHostRef.current === targetHostId) {
          setRefreshing(false);
        }
      }, 350);
    }
  };

  const forecastByKey = useMemo(() => {
    const map = new Map<string, QuotaDepletionForecast>();
    for (const f of insights?.forecasts ?? []) {
      map.set(`${f.provider}::${f.window_label}`, f);
    }
    return map;
  }, [insights]);

  const inputTokens = insights?.total_input_tokens ?? 0;
  const outputTokens = insights?.total_output_tokens ?? 0;
  const cacheReadTokens = insights?.total_cache_read_tokens ?? 0;
  const totalTokens = inputTokens + outputTokens + cacheReadTokens;
  const totalInputWithCache = inputTokens + cacheReadTokens;
  const cacheHitRatio =
    totalInputWithCache > 0 ? cacheReadTokens / totalInputWithCache : 0;
  const inputPct = totalTokens > 0 ? (inputTokens / totalTokens) * 100 : 0;
  const outputPct = totalTokens > 0 ? (outputTokens / totalTokens) * 100 : 0;
  const cachePct = totalTokens > 0 ? (cacheReadTokens / totalTokens) * 100 : 0;

  const activeDaysCount = useMemo(() => {
    const days = insights?.daily ?? [];
    const active = days.filter((d) => d.turns > 0 || d.cost_usd > 0).length;
    return Math.max(1, active || rangeDays);
  }, [insights?.daily, rangeDays]);

  const modelTotalCost = useMemo(
    () => (insights?.by_model ?? []).reduce((sum, r) => sum + r.cost_usd, 0),
    [insights?.by_model],
  );
  const modelTotalTokens = useMemo(
    () =>
      (insights?.by_model ?? []).reduce(
        (sum, r) => sum + r.input_tokens + r.output_tokens + r.cache_read_tokens,
        0,
      ),
    [insights?.by_model],
  );
  const modelMaxMetric = useMemo(() => {
    const rows = insights?.by_model ?? [];
    if (modelTotalCost > 0) {
      return Math.max(0.0001, ...rows.map((r) => r.cost_usd));
    }
    return Math.max(
      1,
      ...rows.map((r) => r.input_tokens + r.output_tokens + r.cache_read_tokens),
    );
  }, [insights?.by_model, modelTotalCost]);

  const projectTotalCost = useMemo(
    () => (insights?.by_project ?? []).reduce((sum, r) => sum + r.cost_usd, 0),
    [insights?.by_project],
  );
  const projectTotalTokens = useMemo(
    () => (insights?.by_project ?? []).reduce((sum, r) => sum + r.total_tokens, 0),
    [insights?.by_project],
  );
  const projectMaxMetric = useMemo(() => {
    const rows = insights?.by_project ?? [];
    if (projectTotalCost > 0) {
      return Math.max(0.0001, ...rows.map((r) => r.cost_usd));
    }
    return Math.max(1, ...rows.map((r) => r.total_tokens));
  }, [insights?.by_project, projectTotalCost]);

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Usage & Quotas"
        description="Token ledger, estimated spend, cache savings, and live subscription burn pace."
        badge={
          showHostBadge && hostCard ? (
            <Badge variant="secondary">{hostCard.display_name}</Badge>
          ) : null
        }
        action={
          <div className="flex items-center gap-1.5">
            <div className="border-border-subtle bg-surface-recessed inline-flex h-[30px] items-center rounded-[8px] border p-0.5 text-[12px]">
              {([7, 30] as const).map((d) => (
                <button
                  key={d}
                  type="button"
                  onClick={() => setRangeDays(d)}
                  className={cn(
                    "cursor-pointer rounded-[6px] px-2.5 py-0.5 font-mono text-[11.5px] font-medium transition-colors",
                    rangeDays === d
                      ? "bg-surface-raised text-ink-primary shadow-2xs"
                      : "text-ink-muted hover:text-ink-primary",
                  )}
                >
                  {d}D
                </button>
              ))}
            </div>
            <IconButton
              size="md"
              tooltip="Refresh usage and quotas"
              disabled={refreshing}
              onClick={() => void handleRefresh()}
              className="border-border-subtle border"
            >
              <RefreshCw className={cn("h-[13.5px] w-[13.5px]", refreshing && "animate-spin")} />
            </IconButton>
          </div>
        }
      />

      {/* Unified Hero Telemetry Card */}
      <SettingCard>
        {/* Top Focal Metric Header */}
        <div className="flex flex-wrap items-start justify-between gap-4 px-5 py-4">
          <div className="space-y-1">
            <div className="text-ink-muted text-[11px] font-semibold tracking-[0.04em] uppercase">
              {chartMetric === "cost"
                ? `Estimated Spend · Last ${rangeDays} Days`
                : `Total Token Volume · Last ${rangeDays} Days`}
            </div>
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
              <span className="text-ink-primary font-mono text-[28px] leading-none font-semibold tracking-[-0.03em] tabular-nums">
                {insights
                  ? chartMetric === "cost"
                    ? formatUsd(insights.total_cost_usd)
                    : formatCompactTokens(totalTokens)
                  : "—"}
              </span>
              {insights && (
                <span className="text-ink-secondary font-mono text-[12px] tabular-nums">
                  {chartMetric === "cost"
                    ? `${formatUsd(insights.total_cost_usd / activeDaysCount)}/active day`
                    : `${formatCompactTokens(totalTokens / activeDaysCount)}/active day`}
                  <span className="text-ink-faint mx-1.5">·</span>
                  {insights.total_turns.toLocaleString()}{" "}
                  {insights.total_turns === 1 ? "turn" : "turns"}
                </span>
              )}
            </div>
          </div>

          <div className="border-border-subtle bg-surface-recessed inline-flex h-[28px] items-center rounded-[7px] border p-0.5 text-[11.5px]">
            {(
              [
                { id: "cost", label: "Spend" },
                { id: "tokens", label: "Tokens" },
              ] as const
            ).map((opt) => (
              <button
                key={opt.id}
                type="button"
                onClick={() => setChartMetric(opt.id)}
                className={cn(
                  "cursor-pointer rounded-[5px] px-2.5 py-0.5 font-medium transition-colors",
                  chartMetric === opt.id
                    ? "bg-surface-raised text-ink-primary shadow-2xs"
                    : "text-ink-muted hover:text-ink-primary",
                )}
              >
                {opt.label}
              </button>
            ))}
          </div>
        </div>

        {/* Hairline-Divided 3-Column Secondary Telemetry Strip */}
        <div className="bg-surface-recessed/45 divide-border-subtle grid grid-cols-1 divide-y sm:grid-cols-3 sm:divide-x sm:divide-y-0">
          {/* Cache Efficiency */}
          <div className="px-5 py-3">
            <div className="flex items-center justify-between gap-2">
              <span className="text-ink-muted text-[11px] font-medium">Cache Saved</span>
              {insights && cacheHitRatio > 0 && (
                <span className="text-signal-success font-mono text-[11px] font-medium tabular-nums">
                  {formatPercent(cacheHitRatio)} hit
                </span>
              )}
            </div>
            <div className="text-ink-primary mt-1 font-mono text-[15px] font-semibold tracking-tight tabular-nums">
              {insights ? formatUsd(insights.total_cache_savings_usd) : "—"}
            </div>
            <div className="text-ink-faint mt-0.5 font-mono text-[11px] tabular-nums">
              {insights ? `${formatCompactTokens(cacheReadTokens)} cached read` : "—"}
            </div>
          </div>

          {/* Token Composition */}
          <div className="px-5 py-3">
            <div className="flex items-center justify-between gap-2">
              <span className="text-ink-muted text-[11px] font-medium">Token Composition</span>
              <span className="text-ink-secondary font-mono text-[11px] tabular-nums">
                {insights ? formatCompactTokens(totalTokens) : "—"}
              </span>
            </div>
            <div className="bg-surface-recessed mt-2 flex h-[5px] w-full overflow-hidden rounded-full">
              {totalTokens > 0 ? (
                <>
                  <div
                    className="bg-ink-primary/75 h-full"
                    style={{ width: `${inputPct}%` }}
                    title={`Input: ${formatCompactTokens(inputTokens)}`}
                  />
                  <div
                    className="bg-signal-info h-full"
                    style={{ width: `${outputPct}%` }}
                    title={`Output: ${formatCompactTokens(outputTokens)}`}
                  />
                  <div
                    className="bg-signal-success h-full"
                    style={{ width: `${cachePct}%` }}
                    title={`Cache Read: ${formatCompactTokens(cacheReadTokens)}`}
                  />
                </>
              ) : (
                <div className="bg-border-default/60 h-full w-full" />
              )}
            </div>
            <div className="text-ink-faint mt-1.5 flex flex-wrap items-center gap-x-2.5 gap-y-0.5 font-mono text-[10.5px] tabular-nums">
              <span className="inline-flex items-center gap-1">
                <span className="bg-ink-primary/75 size-1.5 rounded-full" />
                {formatCompactTokens(inputTokens)} in
              </span>
              <span className="inline-flex items-center gap-1">
                <span className="bg-signal-info size-1.5 rounded-full" />
                {formatCompactTokens(outputTokens)} out
              </span>
              <span className="inline-flex items-center gap-1">
                <span className="bg-signal-success size-1.5 rounded-full" />
                {formatCompactTokens(cacheReadTokens)} cache
              </span>
            </div>
          </div>

          {/* Turn Efficiency */}
          <div className="px-5 py-3">
            <div className="flex items-center justify-between gap-2">
              <span className="text-ink-muted text-[11px] font-medium">Cost / Turn</span>
              <span className="text-ink-faint font-mono text-[11px] tabular-nums">
                {insights ? `${insights.total_turns.toLocaleString()} turns` : "—"}
              </span>
            </div>
            <div className="text-ink-primary mt-1 font-mono text-[15px] font-semibold tracking-tight tabular-nums">
              {insights && insights.total_turns > 0
                ? formatUsd(insights.total_cost_usd / insights.total_turns)
                : "—"}
            </div>
            <div className="text-ink-faint mt-0.5 font-mono text-[11px] tabular-nums">
              {insights && insights.total_turns > 0
                ? `${formatCompactTokens(totalTokens / insights.total_turns)} avg tok/turn`
                : "No recorded turns"}
            </div>
          </div>
        </div>

        {/* Integrated Daily Trend Chart */}
        <div className="px-5 pt-4 pb-4">
          {insightsError ? (
            <div className="text-ink-muted py-6 text-center text-[12.5px]">
              {insightsError}
            </div>
          ) : loadingInsights && !insights ? (
            <div className="text-ink-muted flex items-center justify-center gap-2 py-10 text-[12.5px]">
              <OrbitSpinner size={14} />
              <span>Loading telemetry…</span>
            </div>
          ) : (
            <DailyUsageBarChart daily={insights?.daily ?? []} metric={chartMetric} />
          )}
        </div>
      </SettingCard>

      {/* Subscription Quotas & Depletion Pace */}
      <div className="space-y-2">
        <SettingGroupLabel>Subscription Quotas &amp; Burn Pace</SettingGroupLabel>
        {quotas.length === 0 ? (
          <SettingCard>
            <div className="text-ink-muted flex flex-col items-center gap-2.5 py-9 text-center text-[12.5px]">
              {refreshing ? (
                <span className="inline-flex items-center gap-2">
                  <OrbitSpinner size={14} />
                  <span>Probing live provider quotas…</span>
                </span>
              ) : (
                <>
                  <span>No live subscription quota snapshots available on this host.</span>
                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    onClick={() => void handleRefresh()}
                    className="gap-1.5"
                  >
                    <RefreshCw className="h-[12.5px] w-[12.5px]" />
                    <span>Check Quotas</span>
                  </Button>
                </>
              )}
            </div>
          </SettingCard>
        ) : (
          <div
            className={cn(
              "grid grid-cols-1 gap-3",
              quotas.length > 1 && "md:grid-cols-2",
            )}
          >
            {quotas.map((q) => (
              <SettingCard key={q.provider} className="divide-y-0">
                <div className="space-y-3.5 px-4 py-4">
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex min-w-0 items-center gap-2">
                      <ProviderIcon provider={q.provider} className="h-[15px] w-[15px] shrink-0" />
                      <span className="text-ink-primary text-[13px] font-semibold">
                        {formatProviderName(q.provider)}
                      </span>
                      {q.account_label && (
                        <span className="text-ink-faint truncate text-[11.5px]">
                          {q.account_label}
                        </span>
                      )}
                    </div>
                    {q.credits_remaining !== null && (
                      <span className="text-signal-success shrink-0 font-mono text-[11.5px] font-medium tabular-nums">
                        {q.credits_remaining.toFixed(2)} credits
                      </span>
                    )}
                  </div>

                  {q.error ? (
                    <div className="selectable-text text-ink-muted text-[12px]">{q.error}</div>
                  ) : (
                    <div className="space-y-3">
                      {q.windows.map((w) => {
                        const resetLabel = formatResetTime(w.resets_at);
                        const forecast = forecastByKey.get(`${q.provider}::${w.label}`) ?? null;
                        const depletionLabel =
                          forecast &&
                          forecast.estimated_depletion_secs !== null &&
                          forecast.estimated_depletion_secs !== undefined &&
                          forecast.burn_rate_pct_per_hour > 0.01
                            ? `~${formatDurationShort(forecast.estimated_depletion_secs)} at current pace`
                            : null;
                        const clampedPct = Math.min(100, Math.max(0, w.used_percent));

                        return (
                          <div key={w.label} className="space-y-1.5">
                            <div className="tabular-nums flex items-center justify-between gap-2 text-[12px]">
                              <span className="text-ink-secondary truncate">{w.label}</span>
                              <span className="flex shrink-0 items-center gap-2">
                                {resetLabel && (
                                  <span className="text-ink-faint text-[11px]">{resetLabel}</span>
                                )}
                                <span className="text-ink-primary font-mono font-semibold">
                                  {Math.round(w.used_percent)}%
                                </span>
                              </span>
                            </div>

                            {/* Official shadcn Progress bar with 70% and 85% threshold ticks */}
                            <Progress
                              value={clampedPct}
                              max={100}
                              aria-label={`${formatProviderName(q.provider)} ${w.label}`}
                              indicatorClassName={cn(
                                w.used_percent >= 85
                                  ? "bg-signal-danger"
                                  : w.used_percent >= 70 ||
                                      forecast?.likely_to_exhaust_before_reset
                                    ? "bg-signal-warning"
                                    : "bg-ink-primary/80",
                              )}
                              trackOverlay={
                                <>
                                  <span
                                    aria-hidden="true"
                                    className="bg-surface-raised/85 pointer-events-none absolute top-0 bottom-0 left-[70%] w-px"
                                  />
                                  <span
                                    aria-hidden="true"
                                    className="bg-surface-raised/85 pointer-events-none absolute top-0 bottom-0 left-[85%] w-px"
                                  />
                                </>
                              }
                            />

                            {depletionLabel && (
                              <div className="tabular-nums flex items-center justify-between gap-2 text-[11px]">
                                <span
                                  className={cn(
                                    "inline-flex items-center gap-1.5",
                                    forecast?.likely_to_exhaust_before_reset
                                      ? "text-signal-warning font-medium"
                                      : "text-ink-muted",
                                  )}
                                >
                                  {forecast?.likely_to_exhaust_before_reset && (
                                    <span className="bg-signal-warning size-1.5 rounded-full" />
                                  )}
                                  <span>{depletionLabel}</span>
                                </span>
                                {forecast && forecast.burn_rate_pct_per_hour > 0.01 && (
                                  <span className="text-ink-faint font-mono">
                                    {forecast.burn_rate_pct_per_hour.toFixed(1)}%/hr
                                  </span>
                                )}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              </SettingCard>
            ))}
          </div>
        )}
      </div>

      {/* Model & Project Breakdown Tables */}
      {insights && (insights.by_model.length > 0 || insights.by_project.length > 0) && (
        <div className="space-y-2">
          <SettingGroupLabel>Breakdown</SettingGroupLabel>
          <div className="grid grid-cols-1 gap-3.5 lg:grid-cols-2">
            {/* By Model */}
            <SettingCard>
              <div className="flex items-center justify-between px-4 py-2.5">
                <span className="text-ink-primary text-[12.5px] font-semibold">By Model</span>
                <span className="text-ink-faint font-mono text-[10.5px] uppercase">
                  Spend · Share
                </span>
              </div>
              {insights.by_model.length === 0 ? (
                <div className="text-ink-muted px-4 py-6 text-[12.5px]">
                  No model usage recorded.
                </div>
              ) : (
                <div className="divide-border-subtle/70 divide-y">
                  {insights.by_model.slice(0, 8).map((row) => {
                    const rowTokens =
                      row.input_tokens + row.output_tokens + row.cache_read_tokens;
                    const shareRatio =
                      modelTotalCost > 0
                        ? row.cost_usd / modelTotalCost
                        : modelTotalTokens > 0
                          ? rowTokens / modelTotalTokens
                          : 0;
                    const barRatio =
                      modelTotalCost > 0
                        ? row.cost_usd / modelMaxMetric
                        : rowTokens / modelMaxMetric;

                    return (
                      <div
                        key={`${row.provider}::${row.model}`}
                        className="space-y-1.5 px-4 py-2.5"
                      >
                        <div className="flex items-center justify-between gap-3">
                          <div className="flex min-w-0 items-center gap-2">
                            <ProviderIcon
                              provider={toProviderKind(row.provider)}
                              className="h-[14px] w-[14px] shrink-0"
                            />
                            <div className="min-w-0">
                              <div className="text-ink-primary truncate font-mono text-[12px] font-medium">
                                {row.model}
                              </div>
                              <div className="text-ink-faint tabular-nums font-mono text-[10.5px]">
                                {row.turns} {row.turns === 1 ? "turn" : "turns"} ·{" "}
                                {formatCompactTokens(rowTokens)} tok
                              </div>
                            </div>
                          </div>
                          <div className="shrink-0 text-right font-mono tabular-nums">
                            <div className="text-ink-primary text-[12px] font-medium">
                              {formatUsd(row.cost_usd)}
                            </div>
                            <div className="text-ink-faint text-[10.5px]">
                              {formatPercent(shareRatio)}
                            </div>
                          </div>
                        </div>
                        <div className="bg-surface-recessed h-[2px] w-full overflow-hidden rounded-full">
                          <div
                            className="bg-ink-primary/45 h-full rounded-full"
                            style={{ width: `${Math.max(2, Math.min(100, barRatio * 100))}%` }}
                          />
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </SettingCard>

            {/* By Project */}
            <SettingCard>
              <div className="flex items-center justify-between px-4 py-2.5">
                <span className="text-ink-primary text-[12.5px] font-semibold">By Project</span>
                <span className="text-ink-faint font-mono text-[10.5px] uppercase">
                  Spend · Share
                </span>
              </div>
              {insights.by_project.length === 0 ? (
                <div className="text-ink-muted px-4 py-6 text-[12.5px]">
                  No project usage recorded.
                </div>
              ) : (
                <div className="divide-border-subtle/70 divide-y">
                  {insights.by_project.slice(0, 8).map((row) => {
                    const shareRatio =
                      projectTotalCost > 0
                        ? row.cost_usd / projectTotalCost
                        : projectTotalTokens > 0
                          ? row.total_tokens / projectTotalTokens
                          : 0;
                    const barRatio =
                      projectTotalCost > 0
                        ? row.cost_usd / projectMaxMetric
                        : row.total_tokens / projectMaxMetric;

                    return (
                      <div
                        key={row.workspace_cwd}
                        title={row.workspace_cwd}
                        className="space-y-1.5 px-4 py-2.5"
                      >
                        <div className="flex items-center justify-between gap-3">
                          <div className="flex min-w-0 items-center gap-2">
                            <Folder className="text-ink-muted h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                            <div className="min-w-0">
                              <div className="text-ink-primary truncate text-[12.5px] font-medium">
                                {basenameOfPath(row.workspace_cwd)}
                              </div>
                              <div className="text-ink-faint tabular-nums truncate font-mono text-[10.5px]">
                                {row.turns} {row.turns === 1 ? "turn" : "turns"} ·{" "}
                                {formatCompactTokens(row.total_tokens)} tok
                              </div>
                            </div>
                          </div>
                          <div className="shrink-0 text-right font-mono tabular-nums">
                            <div className="text-ink-primary text-[12px] font-medium">
                              {formatUsd(row.cost_usd)}
                            </div>
                            <div className="text-ink-faint text-[10.5px]">
                              {formatPercent(shareRatio)}
                            </div>
                          </div>
                        </div>
                        <div className="bg-surface-recessed h-[2px] w-full overflow-hidden rounded-full">
                          <div
                            className="bg-ink-primary/45 h-full rounded-full"
                            style={{ width: `${Math.max(2, Math.min(100, barRatio * 100))}%` }}
                          />
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </SettingCard>
          </div>
        </div>
      )}
    </div>
  );
}
