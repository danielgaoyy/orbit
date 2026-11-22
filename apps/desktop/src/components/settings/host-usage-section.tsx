import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Loader2, RefreshCw } from "lucide-react";
import {
  SettingCard,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
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
import { ProviderIcon } from "@/components/ui/provider-icons";
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
  const maxVal = useMemo(() => Math.max(0, ...values), [values]);

  if (daily.length === 0) {
    return (
      <div className="text-muted-foreground py-8 text-center text-[12.5px]">
        No daily usage recorded yet.
      </div>
    );
  }

  const width = 560;
  const height = 112;
  const padX = 4;
  const padTop = 8;
  const padBottom = 20;
  const plotW = width - padX * 2;
  const plotH = height - padTop - padBottom;
  const n = daily.length;
  const step = plotW / n;
  const barW = Math.max(3, Math.min(22, step * 0.62));

  const hovered = hoveredIdx !== null ? daily[hoveredIdx] : null;
  const labelStep = n <= 7 ? 1 : 5;

  return (
    <div className="space-y-2">
      <div className="flex min-h-[18px] items-center justify-between text-[11.5px]">
        {hovered ? (
          <div className="text-foreground/85 tabular-nums flex flex-wrap items-center gap-x-2.5 gap-y-0.5">
            <span className="font-mono font-medium">{hovered.date}</span>
            <span>{formatUsd(hovered.cost_usd)}</span>
            <span className="text-muted-foreground">
              {formatCompactTokens(
                hovered.input_tokens + hovered.output_tokens + hovered.cache_read_tokens,
              )}{" "}
              tok
            </span>
            <span className="text-muted-foreground">
              {hovered.turns} {hovered.turns === 1 ? "turn" : "turns"}
            </span>
          </div>
        ) : (
          <span className="text-muted-foreground">
            {metric === "cost" ? "Daily estimated spend (USD)" : "Daily token volume"}
          </span>
        )}
        <span className="text-muted-foreground tabular-nums font-mono text-[11px]">
          {maxVal > 0
            ? metric === "cost"
              ? `Peak ${formatUsd(maxVal)}`
              : `Peak ${formatCompactTokens(maxVal)}`
            : "No activity"}
        </span>
      </div>

      <svg
        viewBox={`0 0 ${width} ${height}`}
        className="h-[112px] w-full overflow-visible select-none"
        role="img"
        aria-label={metric === "cost" ? "Daily spend bar chart" : "Daily token volume bar chart"}
        onMouseLeave={() => setHoveredIdx(null)}
      >
        {/* Subtle hairline baseline */}
        <line
          x1={padX}
          x2={width - padX}
          y1={padTop + plotH}
          y2={padTop + plotH}
          className="stroke-border"
          strokeWidth={1}
        />
        <line
          x1={padX}
          x2={width - padX}
          y1={padTop + plotH / 2}
          y2={padTop + plotH / 2}
          className="stroke-border/50"
          strokeDasharray="2 3"
          strokeWidth={1}
        />

        {daily.map((d, idx) => {
          const val = values[idx];
          const ratio = maxVal > 0 ? val / maxVal : 0;
          const barH = val > 0 ? Math.max(2, ratio * plotH) : 1;
          const x = padX + idx * step + (step - barW) / 2;
          const y = padTop + plotH - barH;
          const isHovered = hoveredIdx === idx;
          const showLabel = idx === 0 || idx === n - 1 || idx % labelStep === 0;

          return (
            <g
              key={d.date}
              className="cursor-pointer"
              onMouseEnter={() => setHoveredIdx(idx)}
            >
              {/* Full-height invisible hit target */}
              <rect
                x={padX + idx * step}
                y={0}
                width={step}
                height={height}
                fill="transparent"
              />
              <rect
                x={x}
                y={y}
                width={barW}
                height={barH}
                rx={Math.min(2, barW / 2)}
                className={cn(
                  "transition-opacity duration-150",
                  val === 0
                    ? "fill-muted-foreground/20"
                    : isHovered
                      ? "fill-foreground"
                      : "fill-foreground/65",
                )}
              />
              {showLabel && (
                <text
                  x={padX + idx * step + step / 2}
                  y={height - 4}
                  textAnchor="middle"
                  className="fill-muted-foreground/75 font-mono text-[9.5px]"
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

  const totalTokens =
    (insights?.total_input_tokens ?? 0) +
    (insights?.total_output_tokens ?? 0) +
    (insights?.total_cache_read_tokens ?? 0);

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Usage"
        description="Token ledger, estimated spend, cache savings, and quota depletion pace."
        badge={
          showHostBadge && hostCard ? (
            <span className="bg-muted text-muted-foreground rounded-md px-2 py-0.5 text-[11.5px] font-medium">
              {hostCard.display_name}
            </span>
          ) : null
        }
        action={
          <div className="flex items-center gap-1.5">
            <div className="border-border bg-muted/40 inline-flex h-[30px] items-center rounded-lg border p-0.5 text-[12px]">
              {([7, 30] as const).map((d) => (
                <button
                  key={d}
                  type="button"
                  onClick={() => setRangeDays(d)}
                  className={cn(
                    "cursor-pointer rounded-md px-2 py-0.5 font-medium transition-colors",
                    rangeDays === d
                      ? "bg-background text-foreground shadow-xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {d}d
                </button>
              ))}
            </div>
            <button
              type="button"
              disabled={refreshing}
              onClick={() => void handleRefresh()}
              className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
            >
              <RefreshCw className={cn("h-[13px] w-[13px]", refreshing && "animate-spin")} />
              <span>Refresh</span>
            </button>
          </div>
        }
      />

      {/* 4-KPI Monochrome Telemetry Strip */}
      <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-4">
        <div className="border-border/80 bg-card/50 rounded-xl border px-3.5 py-3">
          <div className="text-muted-foreground text-[11.5px]">Estimated Spend</div>
          <div className="text-foreground tabular-nums mt-1 font-mono text-[17px] font-semibold tracking-tight">
            {insights ? formatUsd(insights.total_cost_usd) : "—"}
          </div>
          <div className="text-muted-foreground/75 mt-0.5 text-[11px]">
            Last {rangeDays} days
          </div>
        </div>
        <div className="border-border/80 bg-card/50 rounded-xl border px-3.5 py-3">
          <div className="text-muted-foreground text-[11.5px]">Cache Saved</div>
          <div className="text-foreground tabular-nums mt-1 font-mono text-[17px] font-semibold tracking-tight">
            {insights ? formatUsd(insights.total_cache_savings_usd) : "—"}
          </div>
          <div className="text-muted-foreground/75 tabular-nums mt-0.5 text-[11px]">
            {insights ? `${formatCompactTokens(insights.total_cache_read_tokens)} cached` : "—"}
          </div>
        </div>
        <div className="border-border/80 bg-card/50 rounded-xl border px-3.5 py-3">
          <div className="text-muted-foreground text-[11.5px]">Total Tokens</div>
          <div className="text-foreground tabular-nums mt-1 font-mono text-[17px] font-semibold tracking-tight">
            {insights ? formatCompactTokens(totalTokens) : "—"}
          </div>
          <div className="text-muted-foreground/75 tabular-nums mt-0.5 text-[11px]">
            {insights
              ? `${formatCompactTokens(insights.total_input_tokens)} in · ${formatCompactTokens(insights.total_output_tokens)} out`
              : "—"}
          </div>
        </div>
        <div className="border-border/80 bg-card/50 rounded-xl border px-3.5 py-3">
          <div className="text-muted-foreground text-[11.5px]">Active Turns</div>
          <div className="text-foreground tabular-nums mt-1 font-mono text-[17px] font-semibold tracking-tight">
            {insights ? insights.total_turns.toLocaleString() : "—"}
          </div>
          <div className="text-muted-foreground/75 mt-0.5 text-[11px]">
            {insights && insights.total_turns > 0
              ? `${formatUsd(insights.total_cost_usd / insights.total_turns)} / turn`
              : "No turns"}
          </div>
        </div>
      </div>

      {/* Daily Activity Monochrome SVG Chart */}
      <SettingCard>
        <div className="space-y-3 px-4 py-3.5">
          <div className="flex items-center justify-between gap-2">
            <span className="text-foreground text-[13px] font-medium">Daily Activity</span>
            <div className="border-border bg-muted/40 inline-flex items-center rounded-md border p-0.5 text-[11.5px]">
              {(["cost", "tokens"] as const).map((m) => (
                <button
                  key={m}
                  type="button"
                  onClick={() => setChartMetric(m)}
                  className={cn(
                    "cursor-pointer rounded px-2 py-0.5 font-medium capitalize transition-colors",
                    chartMetric === m
                      ? "bg-background text-foreground shadow-xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {m}
                </button>
              ))}
            </div>
          </div>

          {insightsError ? (
            <div className="text-muted-foreground py-6 text-center text-[12.5px]">
              {insightsError}
            </div>
          ) : loadingInsights && !insights ? (
            <div className="text-muted-foreground flex items-center justify-center gap-2 py-8 text-[12.5px]">
              <Loader2 className="h-[14px] w-[14px] animate-spin" />
              <span>Loading telemetry…</span>
            </div>
          ) : (
            <DailyUsageBarChart daily={insights?.daily ?? []} metric={chartMetric} />
          )}
        </div>
      </SettingCard>

      {/* Model & Project Breakdown Tables */}
      {insights && (insights.by_model.length > 0 || insights.by_project.length > 0) && (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <SettingCard>
            <div className="px-4 py-3">
              <div className="text-foreground text-[13px] font-medium">By Model</div>
            </div>
            {insights.by_model.length === 0 ? (
              <div className="text-muted-foreground px-4 py-5 text-[12.5px]">
                No model usage yet.
              </div>
            ) : (
              <div className="divide-border/60 divide-y">
                {insights.by_model.slice(0, 8).map((row) => (
                  <div
                    key={`${row.provider}::${row.model}`}
                    className="flex items-center justify-between gap-3 px-4 py-2.5 text-[12.5px]"
                  >
                    <div className="flex min-w-0 items-center gap-2">
                      <ProviderIcon
                        provider={toProviderKind(row.provider)}
                        className="h-[14px] w-[14px] shrink-0"
                      />
                      <div className="min-w-0">
                        <div className="text-foreground truncate font-mono text-[12px]">
                          {row.model}
                        </div>
                        <div className="text-muted-foreground tabular-nums text-[11px]">
                          {row.turns} {row.turns === 1 ? "turn" : "turns"} ·{" "}
                          {formatCompactTokens(
                            row.input_tokens + row.output_tokens + row.cache_read_tokens,
                          )}{" "}
                          tok
                        </div>
                      </div>
                    </div>
                    <span className="text-foreground/90 tabular-nums shrink-0 font-mono text-[12px] font-medium">
                      {formatUsd(row.cost_usd)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </SettingCard>

          <SettingCard>
            <div className="px-4 py-3">
              <div className="text-foreground text-[13px] font-medium">By Project</div>
            </div>
            {insights.by_project.length === 0 ? (
              <div className="text-muted-foreground px-4 py-5 text-[12.5px]">
                No project usage yet.
              </div>
            ) : (
              <div className="divide-border/60 divide-y">
                {insights.by_project.slice(0, 8).map((row) => (
                  <div
                    key={row.workspace_cwd}
                    title={row.workspace_cwd}
                    className="flex items-center justify-between gap-3 px-4 py-2.5 text-[12.5px]"
                  >
                    <div className="min-w-0">
                      <div className="text-foreground truncate text-[12.5px] font-medium">
                        {basenameOfPath(row.workspace_cwd)}
                      </div>
                      <div className="text-muted-foreground tabular-nums truncate font-mono text-[11px]">
                        {row.turns} {row.turns === 1 ? "turn" : "turns"} ·{" "}
                        {formatCompactTokens(row.total_tokens)} tok
                      </div>
                    </div>
                    <span className="text-foreground/90 tabular-nums shrink-0 font-mono text-[12px] font-medium">
                      {formatUsd(row.cost_usd)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </SettingCard>
        </div>
      )}

      {/* Live Subscription Quotas & Depletion Forecasts */}
      <SettingCard>
        <div className="px-4 py-3">
          <div className="text-foreground text-[13px] font-medium">
            Subscription Quotas &amp; Depletion Pace
          </div>
        </div>
        {quotas.length === 0 ? (
          <div className="text-muted-foreground flex flex-col items-center gap-2.5 py-10 text-center text-[13px]">
            {refreshing ? (
              <span className="inline-flex items-center gap-2">
                <Loader2 className="h-[14px] w-[14px] animate-spin" />
                <span>Checking usage…</span>
              </span>
            ) : (
              <>
                <span>No usage snapshots available.</span>
                <button
                  type="button"
                  onClick={() => void handleRefresh()}
                  className="border-border text-foreground hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-3 text-[12px] font-medium transition-colors"
                >
                  <RefreshCw className="h-[13px] w-[13px]" />
                  <span>Check Usage</span>
                </button>
              </>
            )}
          </div>
        ) : (
          quotas.map((q) => (
            <div key={q.provider} className="space-y-3 px-4 py-4">
              <div className="flex items-center justify-between gap-2">
                <div className="flex min-w-0 items-center gap-2">
                  <ProviderIcon provider={q.provider} className="h-[15px] w-[15px]" />
                  <span className="text-foreground text-[13.5px] font-medium">
                    {formatProviderName(q.provider)}
                  </span>
                  {q.account_label && (
                    <span className="text-muted-foreground truncate text-[12px]">
                      {q.account_label}
                    </span>
                  )}
                </div>
                {q.credits_remaining !== null && (
                  <span className="tabular-nums shrink-0 font-mono text-[12px] text-emerald-600 dark:text-emerald-400">
                    {q.credits_remaining.toFixed(2)} credits
                  </span>
                )}
              </div>

              {q.error ? (
                <div className="selectable-text text-muted-foreground text-[12.5px]">
                  {q.error}
                </div>
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

                    return (
                      <div key={w.label} className="space-y-1.5">
                        <div className="text-muted-foreground tabular-nums flex items-center justify-between gap-2 text-[12px]">
                          <span className="truncate">{w.label}</span>
                          <span className="flex shrink-0 items-center gap-2">
                            {resetLabel && (
                              <span className="text-muted-foreground/70 text-[11.5px]">
                                {resetLabel}
                              </span>
                            )}
                            <span className="text-foreground/85 font-medium">
                              {Math.round(w.used_percent)}%
                            </span>
                          </span>
                        </div>
                        <div className="bg-muted h-1.5 w-full overflow-hidden rounded-full">
                          <div
                            className={cn(
                              "h-full w-full origin-left rounded-full transition-transform duration-300 ease-[var(--ease-out-expo)]",
                              w.used_percent >= 85
                                ? "bg-rose-500"
                                : w.used_percent >= 70 || forecast?.likely_to_exhaust_before_reset
                                  ? "bg-amber-500"
                                  : "bg-foreground/75",
                            )}
                            style={{
                              transform: `scaleX(${Math.min(1, Math.max(0, w.used_percent / 100))})`,
                            }}
                          />
                        </div>
                        {depletionLabel && (
                          <div className="tabular-nums flex items-center justify-between gap-2 text-[11px]">
                            <span
                              className={cn(
                                "inline-flex items-center gap-1.5",
                                forecast?.likely_to_exhaust_before_reset
                                  ? "text-amber-600 dark:text-amber-400"
                                  : "text-muted-foreground/80",
                              )}
                            >
                              {forecast?.likely_to_exhaust_before_reset && (
                                <span className="size-1.5 rounded-full bg-amber-500" />
                              )}
                              <span>{depletionLabel}</span>
                            </span>
                            {forecast && forecast.burn_rate_pct_per_hour > 0.01 && (
                              <span className="text-muted-foreground/70 font-mono">
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
          ))
        )}
      </SettingCard>
    </div>
  );
}
