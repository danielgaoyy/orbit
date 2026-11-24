//! Usage insights, cost/token ledger, quota depletion forecasting, and context occupancy wire types.

use serde::{Deserialize, Serialize};

/// Single persisted turn-level token & cost ledger entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageLedgerEntry {
    pub id: String,
    pub agent_id: String,
    pub workspace_cwd: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub estimated_cost_usd: f64,
    pub cache_savings_usd: f64,
    pub recorded_at_ms: i64,
}

/// Aggregated daily usage point (`YYYY-MM-DD`, UTC).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageDailyPoint {
    pub date: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
    pub turns: u32,
}

/// Aggregated usage breakdown by `(provider, model)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageModelBreakdown {
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
    pub turns: u32,
}

/// Aggregated usage breakdown by project / workspace directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageProjectBreakdown {
    pub workspace_cwd: String,
    pub cost_usd: f64,
    pub total_tokens: u64,
    pub turns: u32,
}

/// Observed burn-rate & depletion forecast for a provider quota window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaDepletionForecast {
    pub provider: String,
    pub window_label: String,
    pub remaining_percent: f64,
    pub burn_rate_pct_per_hour: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_depletion_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_in_secs: Option<u64>,
    pub likely_to_exhaust_before_reset: bool,
}

/// Single non-overlapping segment within an agent's context window occupancy breakdown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSegment {
    /// Canonical category slug: `instructions`, `rules`, `skills`, `history`, `tool_outputs`, `reasoning`, `free`.
    pub category: String,
    pub label: String,
    pub tokens: u64,
    pub percent: f64,
}

/// Full context-window occupancy snapshot for a specific agent session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextOccupancyBreakdown {
    pub agent_id: String,
    pub model: String,
    pub context_window_tokens: u64,
    pub total_used_tokens: u64,
    pub occupancy_percent: f64,
    pub segments: Vec<ContextSegment>,
}

/// Host-wide usage insights & quota depletion forecast payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageInsightsSnapshot {
    pub days: u32,
    pub total_cost_usd: f64,
    pub total_cache_savings_usd: f64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_turns: u32,
    #[serde(default)]
    pub daily: Vec<UsageDailyPoint>,
    #[serde(default)]
    pub by_model: Vec<UsageModelBreakdown>,
    #[serde(default)]
    pub by_project: Vec<UsageProjectBreakdown>,
    #[serde(default)]
    pub forecasts: Vec<QuotaDepletionForecast>,
}

/// Totals for one session across all retained turn-level ledger entries.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionUsage {
    pub agent_id: String,
    pub turns: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub estimated_cost_usd: f64,
    pub cache_savings_usd: f64,
}
