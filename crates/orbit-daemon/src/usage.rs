//! Pure-Rust Usage Insights, Pricing & Cache-Savings Estimator, Context Occupancy Breakdown,
//! and Quota Burn-Rate Depletion Forecasting.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use chrono::{DateTime, Duration, TimeZone, Utc};
use orbit_protocol::{
    ContextOccupancyBreakdown, ContextSegment, ProviderQuotaSnapshot, QuotaDepletionForecast,
    TimelineEvent, TimelinePayload, ToolCallDetail, UsageDailyPoint, UsageInsightsSnapshot,
    UsageModelBreakdown, UsageProjectBreakdown,
};
use orbit_store::{AgentContextTelemetry, OrbitStore, QuotaSampleRecord, StoreError};

use crate::rules::estimate_tokens;

/// Per-model token pricing (USD per 1M tokens) and context window size.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_write_per_mtok: f64,
    pub context_window_tokens: u64,
}

/// Resolve pricing & context window parameters for a `(provider, model)` pair.
pub fn pricing_for_model(provider: &str, model: &str) -> ModelPricing {
    let m = model.trim().to_ascii_lowercase();
    let p = provider.trim().to_ascii_lowercase();
    let is_1m = m.contains("1m") || m.contains("[1m]");

    let rate = |input, output, read, write, window| ModelPricing {
        input_per_mtok: input,
        output_per_mtok: output,
        cache_read_per_mtok: read,
        cache_write_per_mtok: write,
        context_window_tokens: window,
    };
    if ["ollama", "lmstudio", "lm-studio", "lm_studio"]
        .iter()
        .any(|local| p == *local || m.starts_with(&format!("{local}/")))
    {
        return rate(0., 0., 0., 0., 128_000);
    }
    let version = m.replace('.', "-");
    if version.contains("opus-4-5") || version.contains("opus-4-6") || version.contains("opus-4-7")
    {
        return rate(
            5.,
            25.,
            0.5,
            6.25,
            if is_1m || version.contains("4-6") || version.contains("4-7") {
                1_000_000
            } else {
                200_000
            },
        );
    }
    if version.contains("sonnet-4-6") {
        return rate(3., 15., 0.3, 3.75, 1_000_000);
    }
    if m.contains("gpt-5.4") && !m.contains("mini") && !m.contains("nano") {
        return rate(2.5, 15., 0.25, 2.5, 1_050_000);
    }
    if m.contains("gpt-5.4-mini") {
        return rate(0.75, 4.5, 0.075, 0.75, 400_000);
    }
    if m.contains("gpt-5.4-nano") {
        return rate(0.2, 1.25, 0.02, 0.2, 400_000);
    }
    if m.contains("qwen3-coder-plus") {
        return rate(1., 5., 0.1, 1., 1_000_000);
    }
    if m.contains("qwen3-coder-flash") {
        return rate(0.3, 1.5, 0.03, 0.3, 1_000_000);
    }
    if m.contains("gpt-5.2") {
        return rate(1.75, 14., 0.175, 1.75, 400_000);
    }
    if m.contains("gemini-3") && m.contains("flash") {
        return rate(0.5, 3., 0.05, 0.5, 1_000_000);
    }
    if m.contains("gemini-3") && m.contains("pro") {
        return rate(2., 12., 0.2, 2., 1_000_000);
    }
    if m.contains("deepseek-v4-pro") {
        return rate(1.32, 3.96, 0.044, 1.32, 1_000_000);
    }
    if m.contains("deepseek-flash") {
        return rate(0.30, 1.20, 0.006, 0.30, 1_000_000);
    }
    // Anthropic Claude family
    if m.contains("opus") {
        return ModelPricing {
            input_per_mtok: 15.0,
            output_per_mtok: 75.0,
            cache_read_per_mtok: 1.5,
            cache_write_per_mtok: 18.75,
            context_window_tokens: if is_1m { 1_000_000 } else { 200_000 },
        };
    }
    if m.contains("sonnet") {
        return ModelPricing {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.30,
            cache_write_per_mtok: 3.75,
            context_window_tokens: if is_1m { 1_000_000 } else { 200_000 },
        };
    }
    if m.contains("haiku") {
        return ModelPricing {
            input_per_mtok: 1.0,
            output_per_mtok: 5.0,
            cache_read_per_mtok: 0.10,
            cache_write_per_mtok: 1.25,
            context_window_tokens: 200_000,
        };
    }

    // OpenAI / Codex family
    if m.contains("gpt-5") && (m.contains("mini") || m.contains("nano")) {
        return ModelPricing {
            input_per_mtok: 0.25,
            output_per_mtok: 2.0,
            cache_read_per_mtok: 0.025,
            cache_write_per_mtok: 0.25,
            context_window_tokens: 400_000,
        };
    }
    if m.contains("gpt-5") || m.contains("codex") {
        return ModelPricing {
            input_per_mtok: 1.25,
            output_per_mtok: 10.0,
            cache_read_per_mtok: 0.125,
            cache_write_per_mtok: 1.25,
            context_window_tokens: 400_000,
        };
    }
    if m.contains("o4-mini") || m.contains("o3-mini") {
        return ModelPricing {
            input_per_mtok: 1.10,
            output_per_mtok: 4.40,
            cache_read_per_mtok: 0.275,
            cache_write_per_mtok: 1.10,
            context_window_tokens: 200_000,
        };
    }
    if m == "o3" || m.starts_with("o3-") || m.contains("/o3") {
        return ModelPricing {
            input_per_mtok: 2.0,
            output_per_mtok: 8.0,
            cache_read_per_mtok: 0.50,
            cache_write_per_mtok: 2.0,
            context_window_tokens: 200_000,
        };
    }
    if m.contains("gpt-4.1") {
        return ModelPricing {
            input_per_mtok: 2.0,
            output_per_mtok: 8.0,
            cache_read_per_mtok: 0.50,
            cache_write_per_mtok: 2.0,
            context_window_tokens: 1_000_000,
        };
    }
    if m.contains("gpt-4o") {
        return ModelPricing {
            input_per_mtok: 2.50,
            output_per_mtok: 10.0,
            cache_read_per_mtok: 1.25,
            cache_write_per_mtok: 2.50,
            context_window_tokens: 128_000,
        };
    }

    // Google Gemini / Antigravity family
    if m.contains("gemini") && m.contains("flash") {
        return ModelPricing {
            input_per_mtok: 0.30,
            output_per_mtok: 2.50,
            cache_read_per_mtok: 0.075,
            cache_write_per_mtok: 0.30,
            context_window_tokens: 1_000_000,
        };
    }
    if m.contains("gemini") {
        return ModelPricing {
            input_per_mtok: 1.25,
            output_per_mtok: 10.0,
            cache_read_per_mtok: 0.3125,
            cache_write_per_mtok: 1.25,
            context_window_tokens: 1_000_000,
        };
    }

    // GPT-OSS family (128K context window, distinct from Antigravity's 1M Gemini fallback)
    if m.contains("gpt-oss") {
        return ModelPricing {
            input_per_mtok: 0.40,
            output_per_mtok: 1.60,
            cache_read_per_mtok: 0.10,
            cache_write_per_mtok: 0.40,
            context_window_tokens: 128_000,
        };
    }

    // DeepSeek family
    if m.contains("deepseek") {
        return ModelPricing {
            input_per_mtok: 0.27,
            output_per_mtok: 1.10,
            cache_read_per_mtok: 0.07,
            cache_write_per_mtok: 0.27,
            context_window_tokens: 128_000,
        };
    }

    // Kimi / Moonshot family
    if m.contains("kimi") || m.contains("moonshot") {
        return ModelPricing {
            input_per_mtok: 0.60,
            output_per_mtok: 2.50,
            cache_read_per_mtok: 0.15,
            cache_write_per_mtok: 0.60,
            context_window_tokens: 256_000,
        };
    }

    // Qwen family
    if m.contains("qwen") {
        return ModelPricing {
            input_per_mtok: 0.40,
            output_per_mtok: 1.20,
            cache_read_per_mtok: 0.10,
            cache_write_per_mtok: 0.40,
            context_window_tokens: 256_000,
        };
    }

    // Provider-based fallback when model is unset or generic
    match p.as_str() {
        "claude_code" | "claude" => ModelPricing {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.30,
            cache_write_per_mtok: 3.75,
            context_window_tokens: 200_000,
        },
        "codex" | "openai" => ModelPricing {
            input_per_mtok: 2.50,
            output_per_mtok: 10.0,
            cache_read_per_mtok: 0.25,
            cache_write_per_mtok: 2.50,
            context_window_tokens: 272_000,
        },
        "antigravity" | "agy" | "gemini_cli" | "gemini" => ModelPricing {
            input_per_mtok: 1.25,
            output_per_mtok: 10.0,
            cache_read_per_mtok: 0.3125,
            cache_write_per_mtok: 1.25,
            context_window_tokens: 1_000_000,
        },
        "kimi" => ModelPricing {
            input_per_mtok: 0.60,
            output_per_mtok: 2.50,
            cache_read_per_mtok: 0.15,
            cache_write_per_mtok: 0.60,
            context_window_tokens: 256_000,
        },
        _ => ModelPricing {
            input_per_mtok: 2.0,
            output_per_mtok: 8.0,
            cache_read_per_mtok: 0.40,
            cache_write_per_mtok: 2.0,
            context_window_tokens: 200_000,
        },
    }
}

pub(crate) fn may_compact(
    status: &orbit_protocol::AgentStatus,
    pending: bool,
    recent: bool,
    pct: f64,
    threshold: Option<u8>,
) -> bool {
    matches!(status, orbit_protocol::AgentStatus::Idle)
        && !pending
        && !recent
        && threshold.is_none_or(|t| (50..=95).contains(&t) && pct >= f64::from(t))
}

/// Return the nominal context window size in tokens for `(provider, model)`.
pub fn context_window_for_model(provider: &str, model: &str) -> u64 {
    pricing_for_model(provider, model).context_window_tokens
}

/// Estimate `(estimated_cost_usd, cache_savings_usd)` for a turn's token counts.
pub fn estimate_turn_cost_usd(
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
) -> (f64, f64) {
    estimate_at(
        None,
        provider,
        model,
        [
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
        ],
        input_tokens,
    )
}

fn model_override(home: &Path, provider: &str, model: &str) -> Option<ModelPricing> {
    let bytes = std::fs::read(home.join("model-pricing.json")).ok()?;
    let map = serde_json::from_slice::<HashMap<String, ModelPricing>>(&bytes).ok()?;
    let rate = map
        .get(&format!("{provider}/{model}"))
        .or_else(|| map.get(model))?;
    (rate.context_window_tokens > 0
        && [
            rate.input_per_mtok,
            rate.output_per_mtok,
            rate.cache_read_per_mtok,
            rate.cache_write_per_mtok,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.))
    .then_some(*rate)
}

/// Load a host-local exact model override for each settled usage record.
pub fn estimate_at(
    home: Option<&Path>,
    provider: &str,
    model: &str,
    tokens: [u64; 4],
    prompt_tokens: u64,
) -> (f64, f64) {
    let [
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
    ] = tokens;
    let mut pricing = pricing_for_model(provider, model);
    let id = model.to_ascii_lowercase();
    if pricing.input_per_mtok > 0. && id.contains("qwen3-coder") {
        let rates = if id.contains("plus") {
            [(1., 5.), (1.8, 9.), (3., 15.), (6., 60.)]
        } else {
            [(0.3, 1.5), (0.5, 2.5), (0.8, 4.), (1.6, 9.6)]
        };
        let tier = [32_768, 131_072, 262_144]
            .iter()
            .filter(|&&cap| prompt_tokens > cap)
            .count();
        (pricing.input_per_mtok, pricing.output_per_mtok) = rates[tier];
        pricing.cache_read_per_mtok = pricing.input_per_mtok * 0.1;
    }
    if pricing.input_per_mtok > 0.
        && prompt_tokens > 200_000
        && id.contains("sonnet")
        && id.contains("1m")
        && !id.contains("4-6")
        && !id.contains("4.6")
    {
        pricing.input_per_mtok *= 2.;
        pricing.output_per_mtok *= 1.5;
        pricing.cache_read_per_mtok *= 2.;
        pricing.cache_write_per_mtok *= 2.;
    }
    if pricing.input_per_mtok > 0.
        && prompt_tokens > 200_000
        && id.contains("gemini")
        && id.contains("pro")
    {
        pricing.input_per_mtok *= 2.;
        pricing.output_per_mtok = if id.contains("gemini-3") { 18. } else { 15. };
        pricing.cache_read_per_mtok = pricing.input_per_mtok * 0.1;
    }
    if pricing.input_per_mtok > 0.
        && prompt_tokens > 272_000
        && id.contains("gpt-5.4")
        && !id.contains("mini")
        && !id.contains("nano")
    {
        pricing.input_per_mtok *= 2.;
        pricing.output_per_mtok *= 1.5;
        pricing.cache_read_per_mtok *= 2.;
    }
    if let Some(custom) = home.and_then(|h| model_override(h, provider, model)) {
        pricing = custom;
    }
    let cost = (input_tokens as f64 * pricing.input_per_mtok
        + output_tokens as f64 * pricing.output_per_mtok
        + cache_read_tokens as f64 * pricing.cache_read_per_mtok
        + cache_write_tokens as f64 * pricing.cache_write_per_mtok)
        / 1_000_000.0;
    let savings = (cache_read_tokens as f64
        * (pricing.input_per_mtok - pricing.cache_read_per_mtok).max(0.0))
        / 1_000_000.0;
    (cost.max(0.0), savings.max(0.0))
}

pub(crate) fn record_cost(
    store: &OrbitStore,
    home: &Path,
    entry: &orbit_protocol::UsageLedgerEntry,
    tx: &tokio::sync::broadcast::Sender<crate::manager::DaemonBroadcast>,
) -> Result<(), StoreError> {
    use chrono::Datelike;
    store.record_usage_entry(entry)?;
    let cfg = crate::DaemonPersistedConfig::load(home);
    let Some(now) = DateTime::from_timestamp_millis(entry.recorded_at_ms) else {
        return Ok(());
    };
    for (label, limit, start) in [
        ("daily", cfg.daily_budget_usd, Some(now.date_naive())),
        (
            "monthly",
            cfg.monthly_budget_usd,
            now.date_naive().with_day(1),
        ),
    ] {
        if let (Some(limit), Some(start)) = (limit, start)
            && let Some(midnight) = start.and_hms_opt(0, 0, 0)
            && store.claim_budget_alert(
                &format!("{label}/{start}"),
                midnight.and_utc().timestamp_millis(),
                entry.recorded_at_ms.saturating_add(1),
                limit,
            )?
        {
            let event = store.append_timeline_event(
                &entry.agent_id,
                1,
                None,
                TimelinePayload::Notice {
                    level: "warning".into(),
                    message: format!(
                        "Host {label} spend reached the ${limit:.2} soft limit (UTC)."
                    ),
                },
            )?;
            let _ = tx.send(crate::manager::DaemonBroadcast::TimelineLive(event));
        }
    }
    Ok(())
}

/// Convert live `ProviderQuotaSnapshot`s into persistable `QuotaSampleRecord`s.
pub fn quota_snapshots_to_samples(snapshots: &[ProviderQuotaSnapshot]) -> Vec<QuotaSampleRecord> {
    let mut out = Vec::new();
    for snap in snapshots {
        if snap.error.is_some() {
            continue;
        }
        let sampled_at_ms = snap.updated_at.timestamp_millis();
        for w in &snap.windows {
            if w.label.trim().is_empty() || !w.used_percent.is_finite() {
                continue;
            }
            out.push(QuotaSampleRecord {
                provider: snap.provider.slug().to_string(),
                window_label: w.label.clone(),
                used_percent: f64::from(w.used_percent).clamp(0.0, 100.0),
                resets_at_ms: w.resets_at.map(|t| t.timestamp_millis()),
                sampled_at_ms,
            });
        }
    }
    out
}

fn sanitize_non_negative_f64(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        0.0
    }
}

/// Compute quota burn-rate and depletion ETA forecasts from observed `QuotaSampleRecord`s.
///
/// Automatically detects quota window resets (where `used_percent` drops significantly or
/// `resets_at_ms` advances to a new cycle) so burn rates are never skewed or negative.
pub fn compute_quota_depletion_forecasts(
    samples: &[QuotaSampleRecord],
    now: DateTime<Utc>,
) -> Vec<QuotaDepletionForecast> {
    let mut grouped: BTreeMap<(String, String), Vec<&QuotaSampleRecord>> = BTreeMap::new();
    for s in samples {
        if !s.used_percent.is_finite() {
            continue;
        }
        grouped
            .entry((s.provider.clone(), s.window_label.clone()))
            .or_default()
            .push(s);
    }

    let now_ms = now.timestamp_millis();
    let mut forecasts = Vec::with_capacity(grouped.len());

    for ((provider, window_label), mut series) in grouped {
        series.sort_by_key(|s| s.sampled_at_ms);
        let Some(latest) = series.last().copied() else {
            continue;
        };

        // Slice strictly to the current quota window by finding the last reset boundary.
        let mut window_start_idx = 0usize;
        for i in 1..series.len() {
            let prev = series[i - 1];
            let curr = series[i];
            let used_dropped = curr.used_percent + 2.0 < prev.used_percent;
            let reset_shifted = match (prev.resets_at_ms, curr.resets_at_ms) {
                (Some(prev_r), Some(curr_r)) => curr_r > prev_r + 60_000,
                _ => false,
            };
            if used_dropped || reset_shifted {
                window_start_idx = i;
            }
        }
        let active_window = &series[window_start_idx..];

        // Compute non-negative burn rate (% per hour) over the active window.
        // Prefer spanning from the earliest sample in the active window (up to 6h back) to latest.
        let burn_rate_pct_per_hour = if active_window.len() >= 2 {
            let horizon_ms = latest.sampled_at_ms.saturating_sub(6 * 3_600 * 1_000);
            let anchor = active_window
                .iter()
                .copied()
                .find(|s| s.sampled_at_ms >= horizon_ms && s.sampled_at_ms < latest.sampled_at_ms)
                .unwrap_or(active_window[0]);
            let dt_ms = latest.sampled_at_ms.saturating_sub(anchor.sampled_at_ms);
            if dt_ms >= 30_000 {
                let delta_pct = (latest.used_percent - anchor.used_percent).max(0.0);
                let dt_hours = dt_ms as f64 / 3_600_000.0;
                if dt_hours > 0.0 {
                    delta_pct / dt_hours
                } else {
                    0.0
                }
            } else {
                0.0
            }
        } else {
            0.0
        };

        let remaining_percent = (100.0 - latest.used_percent).clamp(0.0, 100.0);
        let estimated_depletion_secs = if remaining_percent <= 0.0 {
            Some(0)
        } else if burn_rate_pct_per_hour > 0.01 {
            let secs = ((remaining_percent / burn_rate_pct_per_hour) * 3600.0).round();
            if secs.is_finite() && secs >= 0.0 {
                Some(secs.min(u64::MAX as f64) as u64)
            } else {
                None
            }
        } else {
            None
        };

        let resets_in_secs = latest.resets_at_ms.map(|r| {
            let diff_ms = r.saturating_sub(now_ms).max(0);
            (diff_ms / 1_000) as u64
        });

        let likely_to_exhaust_before_reset = match (estimated_depletion_secs, resets_in_secs) {
            (Some(dep), Some(res)) => dep < res,
            (Some(0), None) => true,
            _ => false,
        };

        forecasts.push(QuotaDepletionForecast {
            provider,
            window_label,
            remaining_percent,
            burn_rate_pct_per_hour,
            estimated_depletion_secs,
            resets_in_secs,
            likely_to_exhaust_before_reset,
        });
    }

    forecasts
}

/// Build a host-wide `UsageInsightsSnapshot` from SQLite `usage_ledger` and `quota_samples`.
pub fn build_usage_insights_snapshot(
    store: &OrbitStore,
    days: u32,
    now: DateTime<Utc>,
) -> Result<UsageInsightsSnapshot, StoreError> {
    let clamped_days = days.clamp(1, 90);
    let today_utc = now.date_naive();
    let start_date = today_utc - Duration::days(i64::from(clamped_days.saturating_sub(1)));
    let since_dt = Utc.from_utc_datetime(
        &start_date
            .and_hms_opt(0, 0, 0)
            .unwrap_or_else(|| now.naive_utc()),
    );
    let since_ms = since_dt.timestamp_millis();

    let entries = store.list_usage_entries_since(since_ms)?;

    // Pre-populate contiguous UTC day buckets so charts always have clean X-axis continuity.
    let mut daily_map: BTreeMap<String, UsageDailyPoint> = BTreeMap::new();
    for offset in 0..clamped_days {
        let day = start_date + Duration::days(i64::from(offset));
        let key = day.format("%Y-%m-%d").to_string();
        daily_map.insert(
            key.clone(),
            UsageDailyPoint {
                date: key,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cost_usd: 0.0,
                turns: 0,
            },
        );
    }

    let mut by_model_map: HashMap<(String, String), UsageModelBreakdown> = HashMap::new();
    let mut by_project_map: HashMap<String, UsageProjectBreakdown> = HashMap::new();

    let mut total_cost_usd = 0.0f64;
    let mut total_cache_savings_usd = 0.0f64;
    let mut total_input_tokens = 0u64;
    let mut total_output_tokens = 0u64;
    let mut total_cache_read_tokens = 0u64;
    let mut total_turns = 0u32;

    for e in &entries {
        let entry_cost = sanitize_non_negative_f64(e.estimated_cost_usd);
        let entry_savings = sanitize_non_negative_f64(e.cache_savings_usd);
        total_cost_usd += entry_cost;
        total_cache_savings_usd += entry_savings;
        total_input_tokens = total_input_tokens.saturating_add(e.input_tokens);
        total_output_tokens = total_output_tokens.saturating_add(e.output_tokens);
        total_cache_read_tokens = total_cache_read_tokens.saturating_add(e.cache_read_tokens);
        total_turns = total_turns.saturating_add(1);

        let date_key = DateTime::from_timestamp_millis(e.recorded_at_ms)
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| today_utc.format("%Y-%m-%d").to_string());
        let day_bucket = daily_map
            .entry(date_key.clone())
            .or_insert_with(|| UsageDailyPoint {
                date: date_key,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cost_usd: 0.0,
                turns: 0,
            });
        day_bucket.input_tokens = day_bucket.input_tokens.saturating_add(e.input_tokens);
        day_bucket.output_tokens = day_bucket.output_tokens.saturating_add(e.output_tokens);
        day_bucket.cache_read_tokens = day_bucket
            .cache_read_tokens
            .saturating_add(e.cache_read_tokens);
        day_bucket.cost_usd += entry_cost;
        day_bucket.turns = day_bucket.turns.saturating_add(1);

        let model_label = if e.model.trim().is_empty() {
            "default".to_string()
        } else {
            e.model.clone()
        };
        let model_bucket = by_model_map
            .entry((e.provider.clone(), model_label.clone()))
            .or_insert_with(|| UsageModelBreakdown {
                provider: e.provider.clone(),
                model: model_label,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cost_usd: 0.0,
                turns: 0,
            });
        model_bucket.input_tokens = model_bucket.input_tokens.saturating_add(e.input_tokens);
        model_bucket.output_tokens = model_bucket.output_tokens.saturating_add(e.output_tokens);
        model_bucket.cache_read_tokens = model_bucket
            .cache_read_tokens
            .saturating_add(e.cache_read_tokens);
        model_bucket.cost_usd += entry_cost;
        model_bucket.turns = model_bucket.turns.saturating_add(1);

        let project_cwd = if e.workspace_cwd.trim().is_empty() {
            "~".to_string()
        } else {
            e.workspace_cwd.clone()
        };
        let proj_bucket = by_project_map
            .entry(project_cwd.clone())
            .or_insert_with(|| UsageProjectBreakdown {
                workspace_cwd: project_cwd,
                cost_usd: 0.0,
                total_tokens: 0,
                turns: 0,
            });
        let entry_tokens = e
            .input_tokens
            .saturating_add(e.output_tokens)
            .saturating_add(e.cache_read_tokens);
        proj_bucket.cost_usd += entry_cost;
        proj_bucket.total_tokens = proj_bucket.total_tokens.saturating_add(entry_tokens);
        proj_bucket.turns = proj_bucket.turns.saturating_add(1);
    }

    let mut by_model: Vec<UsageModelBreakdown> = by_model_map.into_values().collect();
    by_model.sort_by(|a, b| {
        b.cost_usd
            .total_cmp(&a.cost_usd)
            .then_with(|| {
                b.input_tokens
                    .saturating_add(b.output_tokens)
                    .cmp(&a.input_tokens.saturating_add(a.output_tokens))
            })
            .then_with(|| a.model.cmp(&b.model))
    });

    let mut by_project: Vec<UsageProjectBreakdown> = by_project_map.into_values().collect();
    by_project.sort_by(|a, b| {
        b.cost_usd
            .total_cmp(&a.cost_usd)
            .then_with(|| b.total_tokens.cmp(&a.total_tokens))
            .then_with(|| a.workspace_cwd.cmp(&b.workspace_cwd))
    });

    // Look back 48h of quota samples for burn-rate forecasting.
    let quota_since_ms = now.timestamp_millis().saturating_sub(48 * 3_600 * 1_000);
    let quota_samples = store.list_recent_quota_samples(quota_since_ms)?;
    let forecasts = compute_quota_depletion_forecasts(&quota_samples, now);

    Ok(UsageInsightsSnapshot {
        days: clamped_days,
        total_cost_usd,
        total_cache_savings_usd,
        total_input_tokens,
        total_output_tokens,
        total_cache_read_tokens,
        total_turns,
        daily: daily_map.into_values().collect(),
        by_model,
        by_project,
        forecasts,
    })
}

/// Estimate tokens in a `ToolCallDetail` payload.
fn estimate_tool_detail_tokens(detail: &ToolCallDetail) -> u64 {
    match detail {
        ToolCallDetail::Shell {
            command,
            output_preview,
            ..
        } => {
            u64::from(estimate_tokens(command))
                + output_preview
                    .as_deref()
                    .map(|s| u64::from(estimate_tokens(s)))
                    .unwrap_or(0)
        }
        ToolCallDetail::FileRead { path, .. } => {
            u64::from(estimate_tokens(path)).saturating_add(64)
        }
        ToolCallDetail::FileEdit { path, unified_diff } => {
            u64::from(estimate_tokens(path))
                + unified_diff
                    .as_deref()
                    .map(|s| u64::from(estimate_tokens(s)))
                    .unwrap_or(0)
        }
        ToolCallDetail::Search { query, scope } => {
            u64::from(estimate_tokens(query))
                + scope
                    .as_deref()
                    .map(|s| u64::from(estimate_tokens(s)))
                    .unwrap_or(0)
        }
        ToolCallDetail::Subagent {
            role,
            prompt_preview,
            ..
        } => u64::from(estimate_tokens(role)) + u64::from(estimate_tokens(prompt_preview)),
        ToolCallDetail::Generic {
            tool_name,
            arguments_json,
        } => u64::from(estimate_tokens(tool_name)) + u64::from(estimate_tokens(arguments_json)),
    }
}

/// Estimate the just-completed turn's `(input_tokens, output_tokens)` from recent timeline events
/// as a fallback when a provider CLI does not report native token usage telemetry.
pub fn estimate_fallback_turn_tokens(events: &[TimelineEvent]) -> (u64, u64) {
    if events.is_empty() {
        return (0, 0);
    }
    let last_prompt_idx = events
        .iter()
        .rposition(|ev| matches!(ev.payload, TimelinePayload::UserPrompt { .. }))
        .unwrap_or(0);

    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;

    for (idx, ev) in events.iter().enumerate() {
        match &ev.payload {
            TimelinePayload::UserPrompt { text, .. } => {
                input_tokens = input_tokens.saturating_add(u64::from(estimate_tokens(text)));
            }
            TimelinePayload::AgentMessage { text, .. } => {
                input_tokens = input_tokens.saturating_add(u64::from(estimate_tokens(text)));
            }
            TimelinePayload::AssistantMessage { text, .. } => {
                let tok = u64::from(estimate_tokens(text));
                if idx >= last_prompt_idx {
                    output_tokens = output_tokens.saturating_add(tok);
                } else {
                    input_tokens = input_tokens.saturating_add(tok);
                }
            }
            TimelinePayload::AssistantThinking { text, .. } => {
                let tok = u64::from(estimate_tokens(text));
                if idx >= last_prompt_idx {
                    output_tokens = output_tokens.saturating_add(tok);
                }
            }
            TimelinePayload::ToolCall { detail, .. } => {
                let tok = estimate_tool_detail_tokens(detail);
                input_tokens = input_tokens.saturating_add(tok);
            }
            _ => {}
        }
    }
    (input_tokens, output_tokens)
}

/// Returns whether a discovered instruction file applies to `provider_slug`.
fn instruction_applies_to_provider(provider_hint: Option<&str>, provider_slug: &str) -> bool {
    let Some(hint) = provider_hint.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    if hint.eq_ignore_ascii_case("orbit") || hint.eq_ignore_ascii_case(provider_slug) {
        return true;
    }
    // Antigravity & Gemini CLI both consume `GEMINI.md`
    (hint.eq_ignore_ascii_case("gemini_cli") && provider_slug.eq_ignore_ascii_case("antigravity"))
        || (hint.eq_ignore_ascii_case("antigravity")
            && provider_slug.eq_ignore_ascii_case("gemini_cli"))
}

/// Pure computation of non-overlapping `ContextOccupancyBreakdown` segments.
///
/// `reported_telemetry` accepts optional single-call provider telemetry `(prompt_tokens,
/// context_window_tokens, recorded_at_ms)` from the most recent LLM API call (never a
/// multi-step turn sum). Prompt tokens are ignored if a context-compaction notice occurred
/// at or after `recorded_at_ms`, while authoritative `context_window_tokens` is always honored.
#[allow(clippy::too_many_arguments)]
pub fn build_context_occupancy_breakdown(
    agent_id: &str,
    provider: &str,
    model: &str,
    instructions_tokens: u64,
    rules_tokens: u64,
    skills_tokens: u64,
    events: &[TimelineEvent],
    reported_telemetry: Option<AgentContextTelemetry>,
) -> ContextOccupancyBreakdown {
    let context_window_tokens = reported_telemetry
        .and_then(|t| t.context_window_tokens)
        .filter(|&w| w > 0)
        .unwrap_or_else(|| context_window_for_model(provider, model))
        .max(1);

    // Only count events after the latest explicit context compaction notice.
    let latest_compaction =
        events
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, ev)| match &ev.payload {
                TimelinePayload::Notice { message, .. } => {
                    let lower = message.to_ascii_lowercase();
                    (lower.contains("context compacted")
                        || lower.contains("conversation compacted"))
                    .then_some((idx + 1, ev.timestamp.timestamp_millis()))
                }
                _ => None,
            });
    let compaction_idx = latest_compaction.map(|(i, _)| i).unwrap_or(0);
    let compaction_ts_ms = latest_compaction.map(|(_, ts)| ts);

    let mut history_tokens = 0u64;
    let mut tool_output_tokens = 0u64;
    let mut reasoning_tokens = 0u64;

    for ev in &events[compaction_idx..] {
        match &ev.payload {
            TimelinePayload::UserPrompt { text, .. }
            | TimelinePayload::AssistantMessage { text, .. }
            | TimelinePayload::AgentMessage { text, .. } => {
                history_tokens = history_tokens.saturating_add(u64::from(estimate_tokens(text)));
            }
            TimelinePayload::AssistantThinking { text, .. } => {
                reasoning_tokens =
                    reasoning_tokens.saturating_add(u64::from(estimate_tokens(text)));
            }
            TimelinePayload::ToolCall { detail, .. } => {
                tool_output_tokens =
                    tool_output_tokens.saturating_add(estimate_tool_detail_tokens(detail));
            }
            _ => {}
        }
    }

    let mut system_tools_tokens = instructions_tokens
        .saturating_add(rules_tokens)
        .saturating_add(skills_tokens);
    let estimated_total = system_tools_tokens
        .saturating_add(history_tokens)
        .saturating_add(tool_output_tokens)
        .saturating_add(reasoning_tokens);

    // Apply provider-reported single-call prompt occupancy unconditionally when recorded strictly
    // after the latest explicit compaction notice (if any):
    // - If `reported > estimated_total`: attribute unmeasured system prompt / MCP tool schema /
    //   full file-read overhead cleanly to `system_tools` instead of distorting conversation history.
    // - If `reported < estimated_total` (e.g. silent provider-side compaction or truncated turns):
    //   scale segments down proportionally so total matches the authoritative provider count.
    if let Some(telemetry) = reported_telemetry
        && telemetry.prompt_tokens > 0
        && compaction_ts_ms.is_none_or(|cts| telemetry.recorded_at_ms > cts)
    {
        let reported = telemetry.prompt_tokens;
        if reported >= estimated_total {
            let extra = reported.saturating_sub(estimated_total);
            system_tools_tokens = system_tools_tokens.saturating_add(extra);
        } else if estimated_total > 0 {
            let scale = reported as f64 / estimated_total as f64;
            let scaled_sys = ((system_tools_tokens as f64) * scale).round() as u64;
            let scaled_hist = ((history_tokens as f64) * scale).round() as u64;
            let scaled_tools = ((tool_output_tokens as f64) * scale).round() as u64;
            let sum_three = scaled_sys
                .saturating_add(scaled_hist)
                .saturating_add(scaled_tools);
            let scaled_reason = reported.saturating_sub(sum_three);
            system_tools_tokens = scaled_sys.min(reported);
            history_tokens = scaled_hist.min(reported.saturating_sub(system_tools_tokens));
            tool_output_tokens = scaled_tools.min(
                reported
                    .saturating_sub(system_tools_tokens)
                    .saturating_sub(history_tokens),
            );
            reasoning_tokens = scaled_reason.min(
                reported
                    .saturating_sub(system_tools_tokens)
                    .saturating_sub(history_tokens)
                    .saturating_sub(tool_output_tokens),
            );
            let residual = reported.saturating_sub(
                system_tools_tokens
                    .saturating_add(history_tokens)
                    .saturating_add(tool_output_tokens)
                    .saturating_add(reasoning_tokens),
            );
            history_tokens = history_tokens.saturating_add(residual);
        }
    }

    let total_used_tokens = system_tools_tokens
        .saturating_add(history_tokens)
        .saturating_add(tool_output_tokens)
        .saturating_add(reasoning_tokens);
    let free_tokens = context_window_tokens.saturating_sub(total_used_tokens);
    let occupancy_percent =
        ((total_used_tokens as f64 / context_window_tokens as f64) * 100.0).clamp(0.0, 100.0);

    // Normalize segment percentages against `max(context_window_tokens, total_used_tokens)`
    // so stacked bar segments are strictly non-overlapping and sum to 100.0%.
    let denom = context_window_tokens.max(total_used_tokens) as f64;
    let pct = |toks: u64| -> f64 { (toks as f64 / denom) * 100.0 };

    let segments = vec![
        ContextSegment {
            category: "system_tools".into(),
            label: "System & tools".into(),
            tokens: system_tools_tokens,
            percent: pct(system_tools_tokens),
        },
        ContextSegment {
            category: "history".into(),
            label: "History".into(),
            tokens: history_tokens,
            percent: pct(history_tokens),
        },
        ContextSegment {
            category: "tool_outputs".into(),
            label: "Tool Outputs".into(),
            tokens: tool_output_tokens,
            percent: pct(tool_output_tokens),
        },
        ContextSegment {
            category: "reasoning".into(),
            label: "Reasoning".into(),
            tokens: reasoning_tokens,
            percent: pct(reasoning_tokens),
        },
        ContextSegment {
            category: "free".into(),
            label: "Free".into(),
            tokens: free_tokens,
            percent: pct(free_tokens),
        },
    ];

    ContextOccupancyBreakdown {
        agent_id: agent_id.to_string(),
        model: if model.trim().is_empty() {
            provider.to_string()
        } else {
            model.to_string()
        },
        context_window_tokens,
        total_used_tokens,
        occupancy_percent,
        segments,
    }
}

/// Compute live `ContextOccupancyBreakdown` for an agent from SQLite + workspace instruction/rule/skill footprints.
///
/// Uses `last_call_telemetry` (in-memory instantaneous single-call telemetry from the active driver,
/// falling back to SQLite `agent_context_telemetry` after daemon restart) when available and strictly
/// newer than the last compaction notice; never uses cumulative multi-call turn sums from `usage_ledger`.
pub fn compute_context_occupancy(
    store: &OrbitStore,
    orbit_home: &Path,
    user_home: &Path,
    agent_id: &str,
    last_call_telemetry: Option<AgentContextTelemetry>,
) -> anyhow::Result<ContextOccupancyBreakdown> {
    let agent = store
        .get_agent(agent_id)?
        .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;

    let effective_telemetry = match last_call_telemetry {
        Some(t) => Some(t),
        None => store.get_agent_context_telemetry(agent_id).ok().flatten(),
    };

    let provider_slug = agent.provider.slug();
    let model_name = agent
        .model
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(provider_slug);

    let effective_telemetry =
        if let Some(rate) = model_override(orbit_home, provider_slug, model_name) {
            let mut t = effective_telemetry.unwrap_or(AgentContextTelemetry {
                prompt_tokens: 0,
                context_window_tokens: None,
                recorded_at_ms: 0,
            });
            t.context_window_tokens
                .get_or_insert(rate.context_window_tokens);
            Some(t)
        } else {
            effective_telemetry
        };
    let expanded_cwd = crate::expand_host_path(&agent.cwd);
    let cwd_opt = if expanded_cwd.is_dir() {
        Some(expanded_cwd.as_path())
    } else {
        None
    };

    let instructions_tokens: u64 = cwd_opt
        .and_then(|p| {
            crate::instructions::discover_instruction_hierarchy_with_home(p, Some(user_home)).ok()
        })
        .map(|res| {
            res.files
                .iter()
                .filter(|f| {
                    instruction_applies_to_provider(f.provider_hint.as_deref(), provider_slug)
                })
                .map(|f| u64::from(f.approx_tokens))
                .sum()
        })
        .unwrap_or(0);

    let rules_tokens =
        u64::from(crate::rules::discover_path_rules(orbit_home, cwd_opt).total_always_tokens);

    let cwd_str = cwd_opt.map(|p| p.to_string_lossy().to_string());
    let skills_tokens: u64 =
        crate::skills_manager::list_managed_skills(orbit_home, user_home, cwd_str.as_deref())
            .into_iter()
            .filter(|s| s.enabled)
            .map(|s| {
                let header = format!("{}: {}", s.name, s.description);
                u64::from(estimate_tokens(&header))
            })
            .sum();

    let events = store.fetch_recent_timeline(agent_id, 2000)?;

    Ok(build_context_occupancy_breakdown(
        agent_id,
        provider_slug,
        model_name,
        instructions_tokens,
        rules_tokens,
        skills_tokens,
        &events,
        effective_telemetry,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_protocol::{AgentStatus, ToolCallStatus, UsageLedgerEntry};

    #[test]
    fn budget_alert_claim_is_durable_and_period_scoped() -> anyhow::Result<()> {
        let store = OrbitStore::open_in_memory()?;
        let entry = UsageLedgerEntry {
            id: "cost".into(),
            agent_id: "agent".into(),
            workspace_cwd: ".".into(),
            provider: "test".into(),
            model: "test".into(),
            input_tokens: 1,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            estimated_cost_usd: 4.0,
            cache_savings_usd: 0.,
            recorded_at_ms: 100,
        };
        store.record_usage_entry(&entry)?;
        assert!(!store.claim_budget_alert("daily/1", 0, 200, 5.)?);
        assert!(store.claim_budget_alert("daily/1", 0, 200, 4.)?);
        assert!(!store.claim_budget_alert("daily/1", 0, 200, 4.)?);
        assert!(store.claim_budget_alert("monthly/1", 0, 200, 4.)?);
        assert!(!store.claim_budget_alert("daily/2", 200, 300, 4.)?);
        Ok(())
    }
    #[test]
    fn compaction_requires_idle_clear_and_outside_cooldown() {
        let ok = |p, r| may_compact(&AgentStatus::Idle, p, r, 80., Some(80));
        assert!(ok(false, false));
        assert!(!ok(true, false));
        assert!(!ok(false, true));
        assert!(!may_compact(
            &AgentStatus::AwaitingPermission {
                turn_id: "t".into(),
                request_id: "p".into()
            },
            false,
            false,
            95.,
            Some(50)
        ));
    }
    #[test]
    fn pricing_overrides_reload_and_tiers_apply() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let price = |home| {
            estimate_at(
                home,
                "test",
                "gemini-2.5-pro",
                [1_000_000, 0, 0, 0],
                200_001,
            )
            .0
        };
        assert_eq!(price(Some(dir.path())), 2.5);
        for input in [8, 3] {
            std::fs::write(dir.path().join("model-pricing.json"),serde_json::json!({"gemini-2.5-pro":{"input_per_mtok":input,"output_per_mtok":1,"cache_read_per_mtok":1,"cache_write_per_mtok":1,"context_window_tokens":200000}}).to_string())?;
            assert_eq!(price(Some(dir.path())), f64::from(input));
        }
        Ok(())
    }

    #[test]
    fn pricing_and_cache_savings_across_providers() {
        // Claude Sonnet 4.5: $3/M in, $15/M out, $0.30/M cache read, $3.75/M cache write
        let (cost, saved) = estimate_turn_cost_usd(
            "claude_code",
            "claude-sonnet-4-5",
            100_000,
            20_000,
            500_000,
            10_000,
        );
        // Expected cost = 0.30 + 0.30 + 0.15 + 0.0375 = 0.7875
        assert!((cost - 0.7875).abs() < 1e-6, "sonnet cost={cost}");
        // Expected cache savings = 0.5M * (3.0 - 0.30) = 1.35
        assert!((saved - 1.35).abs() < 1e-6, "sonnet saved={saved}");
        assert_eq!(
            context_window_for_model("claude_code", "claude-sonnet-4-5"),
            200_000
        );
        assert_eq!(
            context_window_for_model("claude_code", "claude-opus-4-6[1m]"),
            1_000_000
        );

        // Codex GPT-5.4: $2.50/M in, $15/M out, $0.25/M cache read
        let (codex_cost, codex_saved) =
            estimate_turn_cost_usd("codex", "gpt-5.4", 200_000, 50_000, 400_000, 0);
        // Expected cost = 0.50 + 0.75 + 0.10 = 1.35
        assert!((codex_cost - 1.35).abs() < 1e-6, "codex cost={codex_cost}");
        // Expected savings = 0.4M * (2.50 - 0.25) = 0.90
        assert!(
            (codex_saved - 0.90).abs() < 1e-6,
            "codex saved={codex_saved}"
        );
        assert_eq!(context_window_for_model("codex", "gpt-5.4"), 1_050_000);

        // Gemini 2.5 Pro long context: $2.50/M in, $15/M out, $0.25/M cache read
        let (gem_cost, gem_saved) = estimate_turn_cost_usd(
            "antigravity",
            "gemini-2.5-pro",
            1_000_000,
            100_000,
            1_000_000,
            0,
        );
        assert!((gem_cost - 4.25).abs() < 1e-6, "gemini cost={gem_cost}");
        assert!((gem_saved - 2.25).abs() < 1e-6, "gemini saved={gem_saved}");
        assert_eq!(
            context_window_for_model("antigravity", "gemini-2.5-pro"),
            1_000_000
        );
    }

    #[test]
    fn context_occupancy_segments_sum_to_100_percent() {
        let now = Utc::now();
        let events = vec![
            TimelineEvent {
                seq: 1,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: Some("t1".into()),
                timestamp: now,
                payload: TimelinePayload::UserPrompt {
                    message_id: "m1".into(),
                    text: "Inspect the authentication middleware and summarize findings."
                        .repeat(20),
                    badges: Vec::new(),
                },
            },
            TimelineEvent {
                seq: 2,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: Some("t1".into()),
                timestamp: now,
                payload: TimelinePayload::AssistantThinking {
                    item_id: "th1".into(),
                    text: "Checking JWT validation and token expiry paths.".repeat(15),
                },
            },
            TimelineEvent {
                seq: 3,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: Some("t1".into()),
                timestamp: now,
                payload: TimelinePayload::ToolCall {
                    call_id: "c1".into(),
                    status: ToolCallStatus::Completed,
                    detail: ToolCallDetail::Shell {
                        command: "rg auth src/".into(),
                        cwd: None,
                        exit_code: Some(0),
                        output_preview: Some("src/auth.rs:42: validate_jwt()\n".repeat(40)),
                    },
                    background_task: None,
                },
            },
            TimelineEvent {
                seq: 4,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: Some("t1".into()),
                timestamp: now,
                payload: TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "Here is the summary of authentication checks.".repeat(10),
                    streaming: false,
                },
            },
            TimelineEvent {
                seq: 5,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: None,
                timestamp: now,
                payload: TimelinePayload::StatusChanged {
                    status: AgentStatus::Idle,
                },
            },
        ];

        let breakdown = build_context_occupancy_breakdown(
            "ag_1",
            "claude_code",
            "claude-sonnet-4-5",
            2_400,
            1_200,
            400,
            &events,
            None,
        );

        assert_eq!(breakdown.context_window_tokens, 200_000);
        assert_eq!(breakdown.segments.len(), 5);
        assert_eq!(
            breakdown
                .segments
                .iter()
                .find(|s| s.category == "system_tools")
                .map(|s| s.tokens),
            Some(4_000)
        );
        let pct_sum: f64 = breakdown.segments.iter().map(|s| s.percent).sum();
        assert!((pct_sum - 100.0).abs() < 1e-6, "pct_sum={pct_sum}");
        let tok_sum: u64 = breakdown.segments.iter().map(|s| s.tokens).sum();
        assert_eq!(tok_sum, breakdown.context_window_tokens);

        let non_free_sum: u64 = breakdown
            .segments
            .iter()
            .filter(|s| s.category != "free")
            .map(|s| s.tokens)
            .sum();
        assert_eq!(non_free_sum, breakdown.total_used_tokens);
        assert!(
            breakdown
                .segments
                .iter()
                .find(|s| s.category == "tool_outputs")
                .is_some_and(|s| s.tokens > 0)
        );
        assert!(
            breakdown
                .segments
                .iter()
                .find(|s| s.category == "reasoning")
                .is_some_and(|s| s.tokens > 0)
        );
    }

    #[test]
    fn quota_depletion_forecast_handles_window_resets_without_negative_burn_rate() {
        let now = Utc
            .with_ymd_and_hms(2026, 11, 22, 12, 0, 0)
            .single()
            .expect("valid timestamp");
        let now_ms = now.timestamp_millis();
        let hour_ms = 3_600_000i64;

        let samples = vec![
            // Previous 5h window reached 92% used before resetting
            QuotaSampleRecord {
                provider: "claude_code".into(),
                window_label: "5h window".into(),
                used_percent: 70.0,
                resets_at_ms: Some(now_ms - 2 * hour_ms),
                sampled_at_ms: now_ms - 3 * hour_ms,
            },
            QuotaSampleRecord {
                provider: "claude_code".into(),
                window_label: "5h window".into(),
                used_percent: 92.0,
                resets_at_ms: Some(now_ms - 2 * hour_ms),
                sampled_at_ms: now_ms - 2 * hour_ms - 60_000,
            },
            // New window started after reset! Burn rate = (50 - 20) / 1h = 30%/hr
            // Remaining = 50%, ETA = 50/30 hr = 6000s (~1h40m), resets in 3h (10800s) -> likely to exhaust!
            QuotaSampleRecord {
                provider: "claude_code".into(),
                window_label: "5h window".into(),
                used_percent: 20.0,
                resets_at_ms: Some(now_ms + 3 * hour_ms),
                sampled_at_ms: now_ms - hour_ms,
            },
            QuotaSampleRecord {
                provider: "claude_code".into(),
                window_label: "5h window".into(),
                used_percent: 50.0,
                resets_at_ms: Some(now_ms + 3 * hour_ms),
                sampled_at_ms: now_ms,
            },
            // Second series: slow burn, will NOT exhaust before reset
            QuotaSampleRecord {
                provider: "codex".into(),
                window_label: "Weekly".into(),
                used_percent: 10.0,
                resets_at_ms: Some(now_ms + 4 * hour_ms),
                sampled_at_ms: now_ms - 2 * hour_ms,
            },
            QuotaSampleRecord {
                provider: "codex".into(),
                window_label: "Weekly".into(),
                used_percent: 14.0,
                resets_at_ms: Some(now_ms + 4 * hour_ms),
                sampled_at_ms: now_ms,
            },
        ];

        let forecasts = compute_quota_depletion_forecasts(&samples, now);
        assert_eq!(forecasts.len(), 2);

        let claude = forecasts
            .iter()
            .find(|f| f.provider == "claude_code")
            .expect("claude forecast");
        assert!(claude.burn_rate_pct_per_hour >= 0.0);
        assert!((claude.burn_rate_pct_per_hour - 30.0).abs() < 1e-3);
        assert!((claude.remaining_percent - 50.0).abs() < 1e-6);
        assert_eq!(claude.estimated_depletion_secs, Some(6_000));
        assert_eq!(claude.resets_in_secs, Some(10_800));
        assert!(claude.likely_to_exhaust_before_reset);

        let codex = forecasts
            .iter()
            .find(|f| f.provider == "codex")
            .expect("codex forecast");
        assert!((codex.burn_rate_pct_per_hour - 2.0).abs() < 1e-3);
        assert!(!codex.likely_to_exhaust_before_reset);
    }

    #[test]
    fn build_usage_insights_aggregates_daily_model_and_project() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc
            .with_ymd_and_hms(2026, 11, 22, 18, 0, 0)
            .single()
            .expect("valid timestamp");
        let now_ms = now.timestamp_millis();

        let (c1, s1) =
            estimate_turn_cost_usd("claude_code", "claude-sonnet-4-5", 10_000, 2_000, 50_000, 0);
        store
            .record_usage_entry(&UsageLedgerEntry {
                id: "e1".into(),
                agent_id: "ag_1".into(),
                workspace_cwd: "/work/orbit".into(),
                provider: "claude_code".into(),
                model: "claude-sonnet-4-5".into(),
                input_tokens: 10_000,
                output_tokens: 2_000,
                cache_read_tokens: 50_000,
                cache_write_tokens: 0,
                estimated_cost_usd: c1,
                cache_savings_usd: s1,
                recorded_at_ms: now_ms - 3_600_000,
            })
            .expect("record e1");

        let (c2, s2) = estimate_turn_cost_usd("codex", "gpt-5.4", 20_000, 4_000, 0, 0);
        store
            .record_usage_entry(&UsageLedgerEntry {
                id: "e2".into(),
                agent_id: "ag_2".into(),
                workspace_cwd: "/work/api".into(),
                provider: "codex".into(),
                model: "gpt-5.4".into(),
                input_tokens: 20_000,
                output_tokens: 4_000,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                estimated_cost_usd: c2,
                cache_savings_usd: s2,
                recorded_at_ms: now_ms - 26 * 3_600_000,
            })
            .expect("record e2");

        let snap = build_usage_insights_snapshot(&store, 7, now).expect("build snapshot");
        assert_eq!(snap.days, 7);
        assert_eq!(snap.daily.len(), 7);
        assert_eq!(snap.total_turns, 2);
        assert_eq!(snap.total_input_tokens, 30_000);
        assert_eq!(snap.total_output_tokens, 6_000);
        assert_eq!(snap.total_cache_read_tokens, 50_000);
        assert!((snap.total_cost_usd - (c1 + c2)).abs() < 1e-6);
        assert!((snap.total_cache_savings_usd - (s1 + s2)).abs() < 1e-6);
        assert_eq!(snap.by_model.len(), 2);
        assert_eq!(snap.by_project.len(), 2);
    }

    #[test]
    fn context_occupancy_ignores_stale_pre_compaction_prompt_tokens() {
        let t_pre = Utc
            .with_ymd_and_hms(2026, 11, 22, 10, 0, 0)
            .single()
            .expect("valid timestamp");
        let t_compact = t_pre + Duration::minutes(5);
        let t_post = t_compact + Duration::minutes(1);

        let events = vec![
            TimelineEvent {
                seq: 1,
                epoch: 1,
                agent_id: "ag_compact".into(),
                turn_id: Some("t1".into()),
                timestamp: t_pre,
                payload: TimelinePayload::UserPrompt {
                    message_id: "m1".into(),
                    text: "Large pre-compaction context ".repeat(500),
                    badges: Vec::new(),
                },
            },
            TimelineEvent {
                seq: 2,
                epoch: 1,
                agent_id: "ag_compact".into(),
                turn_id: None,
                timestamp: t_compact,
                payload: TimelinePayload::Notice {
                    level: "info".into(),
                    message: "Context compacted".into(),
                },
            },
            TimelineEvent {
                seq: 3,
                epoch: 1,
                agent_id: "ag_compact".into(),
                turn_id: Some("t2".into()),
                timestamp: t_post,
                payload: TimelinePayload::UserPrompt {
                    message_id: "m2".into(),
                    text: "Short post-compaction follow-up.".into(),
                    badges: Vec::new(),
                },
            },
        ];

        // Stale pre-compaction provider prompt occupancy (165k recorded before compaction) MUST be ignored.
        let stale = build_context_occupancy_breakdown(
            "ag_compact",
            "claude_code",
            "claude-sonnet-4-5",
            1_000,
            0,
            0,
            &events,
            Some(AgentContextTelemetry {
                prompt_tokens: 165_000,
                context_window_tokens: None,
                recorded_at_ms: t_pre.timestamp_millis(),
            }),
        );
        assert!(
            stale.total_used_tokens < 5_000,
            "stale pre-compaction occupancy must be ignored, got {}",
            stale.total_used_tokens
        );
        assert!(stale.occupancy_percent < 5.0);

        // Fresh post-compaction single-call prompt occupancy (22k recorded strictly after compaction) IS applied.
        let fresh = build_context_occupancy_breakdown(
            "ag_compact",
            "claude_code",
            "claude-sonnet-4-5",
            1_000,
            0,
            0,
            &events,
            Some(AgentContextTelemetry {
                prompt_tokens: 22_000,
                context_window_tokens: None,
                recorded_at_ms: t_post.timestamp_millis(),
            }),
        );
        assert_eq!(fresh.total_used_tokens, 22_000);
        assert!((fresh.occupancy_percent - 11.0).abs() < 1e-6);

        // Silent compaction: reported prompt tokens LOWER than local estimate MUST scale total down
        // to exact provider-reported tokens, and provider-reported 1M window MUST override 200K fallback.
        let silent_compact = build_context_occupancy_breakdown(
            "ag_compact",
            "claude_code",
            "claude-opus-4-7",
            10_000,
            2_000,
            1_000,
            &events[..1],
            Some(AgentContextTelemetry {
                prompt_tokens: 5_000,
                context_window_tokens: Some(1_000_000),
                recorded_at_ms: t_post.timestamp_millis(),
            }),
        );
        assert_eq!(silent_compact.context_window_tokens, 1_000_000);
        assert_eq!(silent_compact.total_used_tokens, 5_000);
        assert!((silent_compact.occupancy_percent - 0.5).abs() < 1e-6);

        // GPT-OSS 128K window (must not fall back to Antigravity's 1M Gemini window)
        assert_eq!(
            context_window_for_model("antigravity", "gpt-oss-120b-medium"),
            128_000
        );
    }

    #[test]
    fn context_occupancy_does_not_double_count_permission_requests() {
        let now = Utc::now();
        let detail = ToolCallDetail::Shell {
            command: "cargo test --workspace ".repeat(40),
            cwd: None,
            exit_code: None,
            output_preview: None,
        };
        let events = vec![
            TimelineEvent {
                seq: 1,
                epoch: 1,
                agent_id: "ag_perm".into(),
                turn_id: Some("t1".into()),
                timestamp: now,
                payload: TimelinePayload::PermissionRequest {
                    request_id: "req_1".into(),
                    title: "Run command".into(),
                    detail: detail.clone(),
                    allowed_decisions: vec!["allow".into()],
                    guardian_assessment: None,
                    plan_approval: None,
                },
            },
            TimelineEvent {
                seq: 2,
                epoch: 1,
                agent_id: "ag_perm".into(),
                turn_id: Some("t1".into()),
                timestamp: now,
                payload: TimelinePayload::ToolCall {
                    call_id: "c1".into(),
                    detail,
                    status: ToolCallStatus::Completed,
                    background_task: None,
                },
            },
        ];
        let with_perm = build_context_occupancy_breakdown(
            "ag_perm", "codex", "gpt-5.4", 0, 0, 0, &events, None,
        );
        let without_perm = build_context_occupancy_breakdown(
            "ag_perm",
            "codex",
            "gpt-5.4",
            0,
            0,
            0,
            &events[1..],
            None,
        );
        assert_eq!(with_perm.total_used_tokens, without_perm.total_used_tokens);
    }

    #[test]
    fn context_occupancy_does_not_overestimate_from_multi_step_ledger_sum() {
        use orbit_protocol::{
            AgentSummary, AttentionState, ProviderKind, SessionMode, WorkspaceSetupStatus,
            WorkspaceSummary,
        };

        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_1".into(),
                title: "Repo".into(),
                cwd: "/nonexistent/repo".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("upsert workspace");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_multi".into(),
                    workspace_id: "ws_1".into(),
                    cwd: "/nonexistent/repo".into(),
                    title: "Multi-step".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: Some("claude-sonnet-4-5".into()),
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 1,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: Some(now),
                    queued_prompts: Vec::new(),
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        // Record a 6-step turn in `usage_ledger` whose cumulative sum is 240,000 tokens
        // (6 calls * 40k context each) — exceeding the 200k window!
        store
            .record_usage_entry(&UsageLedgerEntry {
                id: "ul_6step".into(),
                agent_id: "ag_multi".into(),
                workspace_cwd: "/nonexistent/repo".into(),
                provider: "claude_code".into(),
                model: "claude-sonnet-4-5".into(),
                input_tokens: 40_000,
                output_tokens: 5_000,
                cache_read_tokens: 180_000,
                cache_write_tokens: 20_000,
                estimated_cost_usd: 0.35,
                cache_savings_usd: 0.48,
                recorded_at_ms: now.timestamp_millis(),
            })
            .expect("record ledger");

        let tmp = std::env::temp_dir();
        // With single-call prompt occupancy (40k), gauge reflects 20% (NOT 100% from 240k sum!)
        let with_single_call = compute_context_occupancy(
            &store,
            &tmp,
            &tmp,
            "ag_multi",
            Some(AgentContextTelemetry {
                prompt_tokens: 40_000,
                context_window_tokens: None,
                recorded_at_ms: now.timestamp_millis(),
            }),
        )
        .expect("compute occupancy with single call");
        assert_eq!(with_single_call.total_used_tokens, 40_000);
        assert!((with_single_call.occupancy_percent - 20.0).abs() < 1e-6);

        // Even without in-memory single-call cache (`None`), MUST NOT read 240k sum from `usage_ledger`!
        let without_single_call = compute_context_occupancy(&store, &tmp, &tmp, "ag_multi", None)
            .expect("compute occupancy fallback");
        assert!(
            without_single_call.occupancy_percent < 50.0,
            "must not peg gauge from cumulative usage_ledger sum, got {}%",
            without_single_call.occupancy_percent
        );

        // Persisted SQLite telemetry survives daemon restart (in-memory `None`) with exact window & count.
        store
            .upsert_agent_context_telemetry(
                "ag_multi",
                AgentContextTelemetry {
                    prompt_tokens: 60_000,
                    context_window_tokens: Some(1_000_000),
                    recorded_at_ms: now.timestamp_millis(),
                },
            )
            .expect("persist telemetry");
        let after_restart = compute_context_occupancy(&store, &tmp, &tmp, "ag_multi", None)
            .expect("compute occupancy from persisted telemetry");
        assert_eq!(after_restart.context_window_tokens, 1_000_000);
        assert_eq!(after_restart.total_used_tokens, 60_000);
        assert!((after_restart.occupancy_percent - 6.0).abs() < 1e-6);
    }

    #[test]
    fn non_finite_floats_are_sanitized_in_usage_and_forecasts() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc
            .with_ymd_and_hms(2026, 11, 22, 18, 0, 0)
            .single()
            .expect("valid timestamp");
        let now_ms = now.timestamp_millis();

        store
            .record_usage_entry(&UsageLedgerEntry {
                id: "e_nan".into(),
                agent_id: "ag_nan".into(),
                workspace_cwd: "/work/orbit".into(),
                provider: "claude_code".into(),
                model: "claude-sonnet-4-5".into(),
                input_tokens: 1_000,
                output_tokens: 500,
                cache_read_tokens: 0,
                cache_write_tokens: 1_000,
                estimated_cost_usd: f64::NAN,
                cache_savings_usd: f64::INFINITY,
                recorded_at_ms: now_ms,
            })
            .expect("record nan entry");

        let forecasts = compute_quota_depletion_forecasts(
            &[
                QuotaSampleRecord {
                    provider: "claude_code".into(),
                    window_label: "5h".into(),
                    used_percent: f64::NAN,
                    resets_at_ms: None,
                    sampled_at_ms: now_ms - 1_000,
                },
                QuotaSampleRecord {
                    provider: "claude_code".into(),
                    window_label: "5h".into(),
                    used_percent: f64::INFINITY,
                    resets_at_ms: None,
                    sampled_at_ms: now_ms,
                },
            ],
            now,
        );
        assert!(forecasts.is_empty());

        let snap = build_usage_insights_snapshot(&store, 7, now).expect("build snapshot");
        assert!(snap.total_cost_usd.is_finite());
        assert!(snap.total_cache_savings_usd.is_finite());
        // Project total_tokens matches top-level KPI (excludes cache_write_tokens consistently)
        assert_eq!(snap.by_project[0].total_tokens, 1_500);
        // Serializes cleanly via serde_json (NaN/Infinity would fail or emit null)
        let encoded = serde_json::to_string(&snap).expect("serialize snapshot");
        assert!(!encoded.contains("NaN"));
    }
}
