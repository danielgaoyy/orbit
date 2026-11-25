import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Folder, RefreshCw } from "lucide-react";
import {
  SettingCard,
  SettingGroup,
  SettingPageHeader,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { Progress } from "@/components/ui/progress";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { Segmented, type SegmentedOption } from "@/components/ui/segmented";
import { t, useI18n } from "@/lib/i18n";
import {
  PROVIDER_LIST,
  type ProviderKind,
  type ProviderQuotaSnapshot,
  type QuotaDepletionForecast,
  type UsageDailyPoint,
  type UsageInsightsSnapshot,
  type UsageModelBreakdown,
  formatProviderLabel,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

type RangeDays = 7 | 30;
type RangeDaysKey = "7" | "30";
type BreakdownView = "provider" | "model" | "project";

const EMPTY_QUOTAS: readonly ProviderQuotaSnapshot[] = [];
const DEFAULT_VISIBLE_ROWS = 5;

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

function formatTurns(count: number): string {
  return t(count === 1 ? "usage.turn" : "usage.turns", { count: count.toLocaleString() });
}

function formatDurationShort(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return t("usage.now");
  const mins = Math.round(secs / 60);
  if (mins < 60) return t("usage.durationMins", { mins: Math.max(1, mins) });
  const hours = Math.floor(mins / 60);
  const remMins = mins % 60;
  if (hours < 24) {
    return remMins > 0
      ? t("usage.durationHoursMins", { hours, mins: remMins })
      : t("usage.durationHours", { hours });
  }
  const days = Math.floor(hours / 24);
  const remHours = hours % 24;
  return remHours > 0
    ? t("usage.durationDaysHours", { days, hours: remHours })
    : t("usage.durationDays", { days });
}

function formatResetTime(resetsAt: string | null): string | null {
  if (!resetsAt) return null;
  const ts = Date.parse(resetsAt);
  if (Number.isNaN(ts)) return resetsAt;
  const diffSecs = Math.round((ts - Date.now()) / 1000);
  if (diffSecs <= 0) return t("usage.resetSoon");
  return t("usage.resetsIn", { duration: formatDurationShort(diffSecs) });
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

function SpendSparkline({ daily }: { daily: UsageDailyPoint[] }) {
  if (daily.length === 0) {
    return (
      <div className="text-ink-muted flex h-[40px] items-center text-[11px] @lg:justify-end">
        {t("usage.noDaily")}
      </div>
    );
  }

  const width = 240;
  const height = 40;
  const padY = 4;
  const plotH = height - padY * 2;
  const values = daily.map((d) => Math.max(0, d.cost_usd));
  const maxVal = Math.max(0, ...values);

  if (maxVal <= 0) {
    return (
      <svg
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        className="block h-[40px] w-full select-none"
        role="img"
        aria-label={t("usage.dailySpend")}
      >
        <path
          d={`M 0 ${height - padY} L ${width} ${height - padY}`}
          fill="none"
          strokeWidth={1.5}
          vectorEffect="non-scaling-stroke"
          className="stroke-border-default"
        />
      </svg>
    );
  }

  const points = values.map((val, idx) => {
    const x = values.length === 1 ? width / 2 : (idx / (values.length - 1)) * width;
    const y = padY + plotH - (val / maxVal) * plotH;
    return [x, y] as const;
  });

  const linePath =
    points.length === 1
      ? `M 0 ${points[0][1].toFixed(2)} L ${width} ${points[0][1].toFixed(2)}`
      : points
          .map(([x, y], idx) => `${idx === 0 ? "M" : "L"} ${x.toFixed(2)} ${y.toFixed(2)}`)
          .join(" ");

  const areaPath =
    points.length === 1
      ? `M 0 ${height} L 0 ${points[0][1].toFixed(2)} L ${width} ${points[0][1].toFixed(2)} L ${width} ${height} Z`
      : `${linePath} L ${width} ${height} L 0 ${height} Z`;

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      className="block h-[40px] w-full select-none"
      role="img"
      aria-label={t("usage.dailySpend")}
    >
      <path d={areaPath} className="fill-ink-primary/[0.04]" />
      <path
        d={linePath}
        fill="none"
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
        vectorEffect="non-scaling-stroke"
        className="stroke-ink-secondary"
      />
    </svg>
  );
}

interface BreakdownRow {
  id: string;
  label: string;
  provider?: string;
  path?: string;
  cost: number;
  tokens: number;
  turns: number;
}

function usageRow(model: UsageModelBreakdown, label: string): BreakdownRow {
  return {
    id: JSON.stringify([model.provider, label]),
    label,
    provider: model.provider,
    cost: model.cost_usd,
    tokens: model.input_tokens + model.output_tokens + model.cache_read_tokens,
    turns: model.turns,
  };
}

export function HostUsageSection({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const quotas = hostDetail?.quotas ?? EMPTY_QUOTAS;
  const [rangeDays, setRangeDays] = useState<RangeDays>(7);
  const [breakdownView, setBreakdownView] = useState<BreakdownView>("provider");
  const [breakdownExpanded, setBreakdownExpanded] = useState(false);
  const [insights, setInsights] = useState<UsageInsightsSnapshot | null>(null);
  const [loadingInsights, setLoadingInsights] = useState(true);
  const [insightsError, setInsightsError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(() => quotas.length === 0);
  const autoFetchedHostRef = useRef<string | null>(null);
  const reqSeqRef = useRef(0);
  const activeHostRef = useRef(hostId);
  const prevHostIdRef = useRef(hostId);
  activeHostRef.current = hostId;

  const rangeRef = useRef(rangeDays);
  rangeRef.current = rangeDays;

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
    if (prevHostIdRef.current === hostId) return;
    prevHostIdRef.current = hostId;
    reqSeqRef.current += 1;
    setInsights(null);
    setInsightsError(null);
    setLoadingInsights(true);
    setRefreshing(false);
  }, [hostId]);

  useEffect(() => {
    setInsights(null);
    void fetchInsights(hostId, rangeDays);
  }, [fetchInsights, hostId, rangeDays]);

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    try {
      await orbitIpc.refreshQuotas(hostId);
    } catch {
      // The historical ledger is available even when a live quota probe fails.
    }
    if (activeHostRef.current === hostId) {
      await fetchInsights(hostId, rangeRef.current);
      if (activeHostRef.current === hostId) setRefreshing(false);
    }
  }, [fetchInsights, hostId]);

  useEffect(() => {
    if (quotas.length > 0 || autoFetchedHostRef.current === hostId) return;
    autoFetchedHostRef.current = hostId;
    void handleRefresh();
  }, [handleRefresh, hostId, quotas.length]);

  const forecastByKey = useMemo(() => {
    const map = new Map<string, QuotaDepletionForecast>();
    for (const f of insights?.forecasts ?? []) {
      map.set(`${f.provider}::${f.window_label}`, f);
    }
    return map;
  }, [insights]);

  const activeDaysCount = useMemo(() => {
    const days = insights?.daily ?? [];
    const active = days.filter((d) => d.turns > 0 || d.cost_usd > 0).length;
    return Math.max(1, active || rangeDays);
  }, [insights?.daily, rangeDays]);

  const breakdownRows = useMemo<BreakdownRow[]>(() => {
    if (breakdownView === "project") {
      return (insights?.by_project ?? []).map((row) => ({
        id: row.workspace_cwd,
        label: basenameOfPath(row.workspace_cwd),
        path: row.workspace_cwd,
        cost: row.cost_usd,
        tokens: row.total_tokens,
        turns: row.turns,
      }));
    }
    const models = insights?.by_model ?? [];
    if (breakdownView === "model") {
      return models.map((model) => usageRow(model, model.model));
    }
    const providers = new Map<string, BreakdownRow>();
    for (const model of models) {
      const row = usageRow(model, formatProviderName(model.provider));
      const prior = providers.get(model.provider);
      if (prior) {
        prior.cost += row.cost;
        prior.tokens += row.tokens;
        prior.turns += row.turns;
      } else {
        providers.set(model.provider, row);
      }
    }
    return [...providers.values()];
  }, [breakdownView, insights?.by_model, insights?.by_project]);

  const { sortedRows, totalCost, totalMetric, peakMetric } = useMemo(() => {
    const costSum = breakdownRows.reduce((sum, row) => sum + row.cost, 0);
    const metricOf = (row: BreakdownRow) => (costSum > 0 ? row.cost : row.tokens);
    const sorted = [...breakdownRows].sort((a, b) => metricOf(b) - metricOf(a));
    const total = costSum || breakdownRows.reduce((sum, row) => sum + row.tokens, 0);
    const peak = sorted.length > 0 ? metricOf(sorted[0]) : 0;
    return { sortedRows: sorted, totalCost: costSum, totalMetric: total, peakMetric: peak };
  }, [breakdownRows]);

  const visibleRows = breakdownExpanded ? sortedRows : sortedRows.slice(0, DEFAULT_VISIBLE_ROWS);

  const rangeOptions = useMemo<readonly SegmentedOption<RangeDaysKey>[]>(
    () => [
      { value: "7", label: t("usage.days", { days: 7 }) },
      { value: "30", label: t("usage.days", { days: 30 }) },
    ],
    [t],
  );

  const breakdownOptions = useMemo<readonly SegmentedOption<BreakdownView>[]>(
    () => [
      { value: "provider", label: t("usage.byProvider") },
      { value: "model", label: t("usage.byModel") },
      { value: "project", label: t("usage.byProject") },
    ],
    [t],
  );

  const handleBreakdownViewChange = useCallback((next: BreakdownView) => {
    setBreakdownView(next);
    setBreakdownExpanded(false);
  }, []);

  return (
    <div className="@container flex min-w-0 flex-col gap-6">
      <SettingPageHeader
        title={t("settings.navUsage")}
        description={t("palette.settingsUsageSub")}
        action={
          <div className="flex items-center gap-2">
            <Segmented<RangeDaysKey>
              size="sm"
              ariaLabel={t("usage.dailyUsage")}
              value={String(rangeDays) as RangeDaysKey}
              options={rangeOptions}
              onValueChange={(next) => setRangeDays(next === "30" ? 30 : 7)}
            />
            <IconButton
              size="md"
              tooltip={t("usage.refresh")}
              disabled={refreshing}
              onClick={() => void handleRefresh()}
              className="border-border-subtle border"
            >
              <RefreshCw className={cn("size-[14px]", refreshing && "animate-spin")} />
            </IconButton>
          </div>
        }
      />

      {/* Section 1: Quota Headroom (Focal, Top) */}
      <SettingGroup settingId="usage.quotas" title={t("usage.quotas")}>
        {quotas.length === 0 ? (
          <SettingCard>
            <div className="text-ink-muted flex flex-col items-center gap-2 px-4 py-8 text-center text-[13px]">
              {refreshing ? (
                <span className="inline-flex items-center gap-2">
                  <OrbitSpinner size={14} />
                  <span>{t("usage.probing")}</span>
                </span>
              ) : (
                <>
                  <span>{t("usage.noQuotas")}</span>
                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    onClick={() => void handleRefresh()}
                    className="gap-2"
                  >
                    <RefreshCw className="size-[12px]" />
                    <span>{t("usage.check")}</span>
                  </Button>
                </>
              )}
            </div>
          </SettingCard>
        ) : (
          <div
            className={cn(
              "grid grid-cols-1 gap-3",
              quotas.length > 1 && "@2xl:grid-cols-2",
            )}
          >
            {quotas.map((q) => (
              <SettingCard key={q.provider} className="divide-y-0">
                <div className="flex flex-col gap-3 p-4">
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex min-w-0 items-center gap-2">
                      <ProviderIcon provider={q.provider} className="size-[15px] shrink-0" />
                      <span className="text-ink-primary shrink-0 text-[13px] font-semibold">
                        {formatProviderName(q.provider)}
                      </span>
                      {q.account_label && (
                        <span
                          title={q.account_label}
                          className="text-ink-faint min-w-0 truncate text-[11px]"
                        >
                          {q.account_label}
                        </span>
                      )}
                    </div>
                    {q.credits_remaining !== null && (
                      <span className="text-ink-secondary shrink-0 font-mono text-[11px] font-medium tabular-nums">
                        {t("usage.credits", { count: q.credits_remaining.toFixed(2) })}
                      </span>
                    )}
                  </div>

                  {q.error ? (
                    <div className="selectable-text text-ink-muted text-[13px]">{q.error}</div>
                  ) : (
                    <div className="flex flex-col gap-3">
                      {q.windows.map((w) => {
                        const resetLabel = formatResetTime(w.resets_at);
                        const forecast = forecastByKey.get(`${q.provider}::${w.label}`) ?? null;
                        const depletionLabel =
                          forecast &&
                          forecast.estimated_depletion_secs !== null &&
                          forecast.estimated_depletion_secs !== undefined &&
                          forecast.burn_rate_pct_per_hour > 0.01
                            ? t("usage.pace", {
                                duration: formatDurationShort(forecast.estimated_depletion_secs),
                              })
                            : null;
                        const clampedPct = Math.min(100, Math.max(0, w.used_percent));
                        const isDanger = w.used_percent >= 90;
                        const isWarning = !isDanger && w.used_percent >= 75;

                        return (
                          <div key={w.label} className="flex flex-col gap-1">
                            <div className="flex items-center gap-3">
                              <span
                                title={w.label}
                                className="text-ink-secondary w-[100px] shrink-0 truncate text-[13px]"
                              >
                                {w.label}
                              </span>
                              <div className="min-w-0 flex-1">
                                <Progress
                                  value={clampedPct}
                                  max={100}
                                  aria-label={`${formatProviderName(q.provider)} ${w.label}`}
                                  className="h-[4px]"
                                  indicatorClassName={
                                    isDanger
                                      ? "bg-signal-danger"
                                      : isWarning
                                        ? "bg-signal-warning"
                                        : "bg-ink-secondary"
                                  }
                                />
                              </div>
                              <span
                                className={cn(
                                  "w-[36px] shrink-0 text-right font-mono text-[13px] font-medium tabular-nums",
                                  isDanger
                                    ? "text-signal-danger"
                                    : isWarning
                                      ? "text-signal-warning"
                                      : "text-ink-primary",
                                )}
                              >
                                {Math.round(w.used_percent)}%
                              </span>
                            </div>

                            {(resetLabel || depletionLabel) && (
                              <div className="flex flex-wrap items-center gap-2 text-[11px] tabular-nums">
                                {resetLabel && (
                                  <span className="text-ink-muted">{resetLabel}</span>
                                )}
                                {resetLabel && depletionLabel && (
                                  <span aria-hidden className="text-ink-faint">
                                    ·
                                  </span>
                                )}
                                {depletionLabel && (
                                  <span
                                    className={cn(
                                      "inline-flex items-center gap-1",
                                      forecast?.likely_to_exhaust_before_reset
                                        ? "text-signal-warning font-medium"
                                        : "text-ink-muted",
                                    )}
                                  >
                                    {forecast?.likely_to_exhaust_before_reset && (
                                      <span className="bg-signal-warning size-[6px] shrink-0 rounded-full" />
                                    )}
                                    <span>{depletionLabel}</span>
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
      </SettingGroup>

      {/* Section 2: Cost Attribution (Single Card, 3 Views) */}
      <SettingGroup title={t("usage.spend")}>
        <SettingCard>
          <div className="flex flex-wrap items-center justify-between gap-2 px-4 py-3">
            <Segmented<BreakdownView>
              size="sm"
              ariaLabel={t("usage.spend")}
              value={breakdownView}
              options={breakdownOptions}
              onValueChange={handleBreakdownViewChange}
            />
            <span className="text-ink-faint text-[11px]">
              {t(totalCost > 0 ? "usage.share" : "usage.tokenShare")}
            </span>
          </div>

          {insightsError ? (
            <p className="text-ink-muted px-4 py-6 text-center text-[13px]">{insightsError}</p>
          ) : loadingInsights && !insights ? (
            <div className="text-ink-muted flex items-center justify-center gap-2 px-4 py-6 text-[13px]">
              <OrbitSpinner size={14} />
              <span>{t("common.loading")}</span>
            </div>
          ) : sortedRows.length === 0 ? (
            <p className="text-ink-muted px-4 py-6 text-center text-[13px]">
              {t("usage.noActivity")}
            </p>
          ) : (
            visibleRows.map((row) => {
              const rowMetric = totalCost > 0 ? row.cost : row.tokens;
              const barPct = peakMetric > 0 ? (rowMetric / peakMetric) * 100 : 0;
              return (
                <div
                  key={row.id}
                  title={row.path}
                  className="flex items-start gap-3 px-4 py-3"
                >
                  <span className="flex h-[20px] shrink-0 items-center">
                    {row.provider ? (
                      <ProviderIcon
                        provider={toProviderKind(row.provider)}
                        className="size-[14px]"
                      />
                    ) : (
                      <Folder className="text-ink-muted size-[14px]" />
                    )}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-3">
                      <span className="text-ink-primary min-w-0 flex-1 truncate text-[13px] font-medium">
                        {row.label}
                      </span>
                      <div className="bg-surface-recessed h-[4px] w-[72px] shrink-0 overflow-hidden rounded-full @md:w-[104px]">
                        <div
                          className="bg-ink-secondary h-full rounded-full"
                          style={{ width: `${barPct}%` }}
                        />
                      </div>
                      <span className="text-ink-primary w-[68px] shrink-0 text-right font-mono text-[13px] font-medium tabular-nums">
                        {totalCost > 0 ? formatUsd(row.cost) : formatCompactTokens(row.tokens)}
                      </span>
                    </div>
                    <div className="text-ink-faint mt-1 flex items-center justify-between gap-2 font-mono text-[11px] tabular-nums">
                      <span className="truncate">
                        {t("usage.turnsAndTokens", {
                          turns: formatTurns(row.turns),
                          tokens: formatCompactTokens(row.tokens),
                        })}
                      </span>
                      <span className="shrink-0">
                        {formatPercent(totalMetric > 0 ? rowMetric / totalMetric : 0)}
                      </span>
                    </div>
                  </div>
                </div>
              );
            })
          )}

          {sortedRows.length > DEFAULT_VISIBLE_ROWS && (
            <Button
              variant="ghost"
              className="w-full rounded-none text-[13px]"
              aria-expanded={breakdownExpanded}
              onClick={() => setBreakdownExpanded((v) => !v)}
            >
              {breakdownExpanded
                ? t("usage.showLess")
                : t("usage.showMore", { count: sortedRows.length - DEFAULT_VISIBLE_ROWS })}
            </Button>
          )}
        </SettingCard>
      </SettingGroup>

      {/* Section 3: Daily Spend Trend (Demoted 40px Sparkline) */}
      <SettingGroup settingId="usage.dailyUsage" title={t("usage.dailyUsage")}>
        <SettingCard className="divide-y-0">
          {insightsError ? (
            <div className="text-ink-muted px-4 py-6 text-center text-[13px]">{insightsError}</div>
          ) : loadingInsights && !insights ? (
            <div className="text-ink-muted flex items-center justify-center gap-2 px-4 py-6 text-[13px]">
              <OrbitSpinner size={14} />
              <span>{t("common.loading")}</span>
            </div>
          ) : (
            <div className="flex flex-col gap-3 p-4 @lg:flex-row @lg:items-center @lg:gap-6">
              <div className="flex shrink-0 flex-col gap-1">
                <span className="text-ink-muted text-[11px] font-medium">
                  {t("usage.spendPeriod", { days: rangeDays })}
                </span>
                <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 font-mono tabular-nums">
                  <span className="text-ink-primary text-[17px] leading-none font-semibold tracking-[-0.02em]">
                    {insights ? formatUsd(insights.total_cost_usd) : "—"}
                  </span>
                  {insights && (
                    <span className="text-ink-secondary text-[11px]">
                      {t("usage.perActiveDay", {
                        value: formatUsd(insights.total_cost_usd / activeDaysCount),
                      })}
                    </span>
                  )}
                </div>
              </div>
              <div className="min-w-0 flex-1">
                <SpendSparkline daily={insights?.daily ?? []} />
              </div>
            </div>
          )}
        </SettingCard>
      </SettingGroup>
    </div>
  );
}
