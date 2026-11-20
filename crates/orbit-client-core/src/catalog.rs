//! Provider model & thinking effort catalog resolution shared across Desktop, iOS, Android, and HarmonyOS.
//!
//! Mirrors `apps/desktop/src/lib/model-catalog.ts` so every native client gets instant, populated
//! model pickers and effort selectors before or without live CLI discovery.

use orbit_protocol::{
    ProviderKind, ProviderModelCatalog, ProviderModelSpec, ReasoningEffort, ThinkingOptionSpec,
};
use serde::{Deserialize, Serialize};

const CODEX_FULL_EFFORTS: &[ReasoningEffort] = &[
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Xhigh,
    ReasoningEffort::Max,
    ReasoningEffort::Ultra,
];

const CODEX_LUNA_EFFORTS: &[ReasoningEffort] = &[
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Xhigh,
    ReasoningEffort::Max,
];

const CLAUDE_XHIGH_EFFORTS: &[ReasoningEffort] = &[
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Xhigh,
    ReasoningEffort::Max,
];

const CLAUDE_XHIGH_OFF_EFFORTS: &[ReasoningEffort] = &[
    ReasoningEffort::Off,
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Xhigh,
    ReasoningEffort::Max,
];

const CLAUDE_STD_OFF_EFFORTS: &[ReasoningEffort] = &[
    ReasoningEffort::Off,
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Max,
];

const AGY_STD_EFFORTS: &[ReasoningEffort] = &[
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
];

const AGY_XHIGH_EFFORTS: &[ReasoningEffort] = &[
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Xhigh,
];

pub fn format_effort_label(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off => "Off",
        ReasoningEffort::Low => "Low",
        ReasoningEffort::Medium => "Medium",
        ReasoningEffort::High => "High",
        ReasoningEffort::Xhigh => "Extra High",
        ReasoningEffort::Max => "Max",
        ReasoningEffort::Ultra => "Ultra",
    }
}

pub fn default_effort_description(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off => "Thinking disabled",
        ReasoningEffort::Low => "Fast & concise",
        ReasoningEffort::Medium => "Balanced speed & depth",
        ReasoningEffort::High => "Greater reasoning depth",
        ReasoningEffort::Xhigh => "Extra high reasoning depth",
        ReasoningEffort::Max => "Maximum reasoning budget",
        ReasoningEffort::Ultra => "Max reasoning + delegation",
    }
}

fn make_thinking_options(
    efforts: &[ReasoningEffort],
    default_effort: Option<ReasoningEffort>,
) -> Vec<ThinkingOptionSpec> {
    efforts
        .iter()
        .copied()
        .map(|effort| ThinkingOptionSpec {
            effort,
            label: format_effort_label(effort).to_string(),
            description: Some(default_effort_description(effort).to_string()),
            is_default: Some(effort) == default_effort,
        })
        .collect()
}

fn sanitize_model_description(label: &str, description: &str) -> String {
    let clean_desc = description.trim();
    let clean_label = label.trim();
    if !clean_label.is_empty()
        && clean_desc.len() > clean_label.len()
        && clean_desc
            .get(..clean_label.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(clean_label))
    {
        let rest = clean_desc[clean_label.len()..]
            .trim_start_matches([' ', '·', '•', '-', '—', ':'])
            .trim();
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    clean_desc.to_string()
}

fn make_spec(
    id: &str,
    label: &str,
    description: &str,
    is_default: bool,
    default_effort: Option<ReasoningEffort>,
    efforts: &[ReasoningEffort],
) -> ProviderModelSpec {
    ProviderModelSpec {
        id: id.to_string(),
        label: label.to_string(),
        description: sanitize_model_description(label, description),
        is_default,
        default_effort,
        thinking_options: make_thinking_options(efforts, default_effort),
    }
}

pub fn fallback_provider_catalog(provider: ProviderKind) -> ProviderModelCatalog {
    match provider {
        ProviderKind::Codex => ProviderModelCatalog {
            provider: ProviderKind::Codex,
            configured_default_model: Some("gpt-6.2-sol".to_string()),
            configured_default_effort: Some(ReasoningEffort::Low),
            models: vec![
                make_spec(
                    "gpt-6.2-sol",
                    "GPT 6.2 Sol",
                    "Fast, go-to model for coding and everyday work.",
                    true,
                    Some(ReasoningEffort::Low),
                    CODEX_FULL_EFFORTS,
                ),
                make_spec(
                    "gpt-6.2-astra",
                    "GPT 6.2 Astra",
                    "Most capable model for complex coding and professional work.",
                    false,
                    Some(ReasoningEffort::Low),
                    CODEX_FULL_EFFORTS,
                ),
                make_spec(
                    "gpt-6.1-terra",
                    "GPT 6.1 Terra",
                    "Efficient model that excels at visual understanding and computer use.",
                    false,
                    Some(ReasoningEffort::Low),
                    CODEX_FULL_EFFORTS,
                ),
                make_spec(
                    "gpt-6.1-sol",
                    "GPT 6.1 Sol",
                    "Previous generation model for coding and everyday work.",
                    false,
                    Some(ReasoningEffort::Low),
                    CODEX_FULL_EFFORTS,
                ),
                make_spec(
                    "gpt-6-astra",
                    "GPT 6 Astra",
                    "Previous generation model for complex coding and professional work.",
                    false,
                    Some(ReasoningEffort::Medium),
                    CODEX_FULL_EFFORTS,
                ),
                make_spec(
                    "gpt-6-sol",
                    "GPT 6 Sol",
                    "Legacy model for coding and everyday work.",
                    false,
                    Some(ReasoningEffort::Medium),
                    CODEX_FULL_EFFORTS,
                ),
                make_spec(
                    "gpt-6-luna",
                    "GPT 6 Luna",
                    "Fast and affordable model for easier tasks.",
                    false,
                    Some(ReasoningEffort::Medium),
                    CODEX_LUNA_EFFORTS,
                ),
            ],
        },
        ProviderKind::ClaudeCode => ProviderModelCatalog {
            provider: ProviderKind::ClaudeCode,
            configured_default_model: Some("claude-opus-5-5".to_string()),
            configured_default_effort: Some(ReasoningEffort::Medium),
            models: vec![
                make_spec(
                    "claude-opus-5-5",
                    "Opus 5.5",
                    "Opus 5.5 · Latest release",
                    true,
                    Some(ReasoningEffort::Medium),
                    CLAUDE_XHIGH_EFFORTS,
                ),
                make_spec(
                    "claude-fable-5-2",
                    "Fable 5.2",
                    "Fable 5.2 · Most powerful model",
                    false,
                    Some(ReasoningEffort::Medium),
                    CLAUDE_XHIGH_EFFORTS,
                ),
                make_spec(
                    "claude-sonnet-5-5",
                    "Sonnet 5.5",
                    "Sonnet 5.5 · Best for everyday tasks",
                    false,
                    Some(ReasoningEffort::Medium),
                    CLAUDE_XHIGH_EFFORTS,
                ),
                make_spec(
                    "claude-haiku-5-5",
                    "Haiku 5.5",
                    "Haiku 5.5 · Fastest for quick answers",
                    false,
                    Some(ReasoningEffort::Medium),
                    CLAUDE_XHIGH_OFF_EFFORTS,
                ),
                make_spec(
                    "claude-opus-5",
                    "Opus 5",
                    "Opus 5 · Previous release",
                    false,
                    Some(ReasoningEffort::High),
                    CLAUDE_XHIGH_OFF_EFFORTS,
                ),
                make_spec(
                    "claude-sonnet-5",
                    "Sonnet 5",
                    "Sonnet 5 · Previous release",
                    false,
                    Some(ReasoningEffort::High),
                    CLAUDE_XHIGH_OFF_EFFORTS,
                ),
                make_spec(
                    "claude-opus-4-8",
                    "Opus 4.8",
                    "Opus 4.8 · Previous release",
                    false,
                    Some(ReasoningEffort::High),
                    CLAUDE_XHIGH_OFF_EFFORTS,
                ),
                make_spec(
                    "claude-opus-4-6",
                    "Opus 4.6",
                    "Opus 4.6 · Most capable for complex work",
                    false,
                    Some(ReasoningEffort::High),
                    CLAUDE_STD_OFF_EFFORTS,
                ),
                make_spec(
                    "claude-sonnet-4-6",
                    "Sonnet 4.6",
                    "Sonnet 4.6 · Best for everyday tasks",
                    false,
                    Some(ReasoningEffort::High),
                    CLAUDE_STD_OFF_EFFORTS,
                ),
                make_spec(
                    "claude-haiku-4-5",
                    "Haiku 4.5",
                    "Haiku 4.5 · Previous release",
                    false,
                    None,
                    &[],
                ),
            ],
        },
        ProviderKind::Antigravity => ProviderModelCatalog {
            provider: ProviderKind::Antigravity,
            configured_default_model: Some("gemini-4-carbon".to_string()),
            configured_default_effort: Some(ReasoningEffort::Medium),
            models: vec![
                make_spec(
                    "gemini-4-carbon",
                    "Gemini 4 Carbon",
                    "Flagship reasoning & multi-file software engineering",
                    true,
                    Some(ReasoningEffort::Medium),
                    AGY_STD_EFFORTS,
                ),
                make_spec(
                    "gemini-4-argon",
                    "Gemini 4 Argon",
                    "Balanced coding, architecture & tool orchestration",
                    false,
                    Some(ReasoningEffort::Medium),
                    AGY_STD_EFFORTS,
                ),
                make_spec(
                    "gemini-4-hydrogen",
                    "Gemini 4 Hydrogen",
                    "Extended deep reasoning with Extra High budget",
                    false,
                    Some(ReasoningEffort::High),
                    AGY_XHIGH_EFFORTS,
                ),
                make_spec(
                    "gemini-pro-orchestrator",
                    "Orchestrator (Carbon)",
                    "Multi-agent coordinator powered by Carbon",
                    false,
                    None,
                    &[],
                ),
                make_spec(
                    "gemini-pro-orchestrator-argon",
                    "Orchestrator (Argon)",
                    "Multi-agent coordinator powered by Argon",
                    false,
                    None,
                    &[],
                ),
                make_spec(
                    "gemini-3.8-flash",
                    "Gemini 3.8 Flash",
                    "High-speed interactive coding & rapid iteration",
                    false,
                    Some(ReasoningEffort::Medium),
                    AGY_STD_EFFORTS,
                ),
                make_spec(
                    "claude-opus-5-5",
                    "Claude Opus 5.5",
                    "Anthropic flagship model on Antigravity",
                    false,
                    Some(ReasoningEffort::Medium),
                    AGY_STD_EFFORTS,
                ),
                make_spec(
                    "claude-sonnet-5-5",
                    "Claude Sonnet 5.5",
                    "Fast everyday coding & refactoring",
                    false,
                    Some(ReasoningEffort::Medium),
                    AGY_STD_EFFORTS,
                ),
            ],
        },
    }
}

pub fn effective_provider_catalogs(live: &[ProviderModelCatalog]) -> Vec<ProviderModelCatalog> {
    const ORDER: [ProviderKind; 3] = [
        ProviderKind::ClaudeCode,
        ProviderKind::Codex,
        ProviderKind::Antigravity,
    ];
    ORDER
        .into_iter()
        .map(|provider| {
            let mut cat = live
                .iter()
                .find(|c| c.provider == provider && !c.models.is_empty())
                .cloned()
                .unwrap_or_else(|| fallback_provider_catalog(provider));
            for m in &mut cat.models {
                m.description = sanitize_model_description(&m.label, &m.description);
            }
            cat
        })
        .collect()
}

pub fn strip_agy_effort_suffix(model_id: &str) -> (String, Option<ReasoningEffort>) {
    let trimmed = model_id.trim();
    if trimmed.eq_ignore_ascii_case("gpt-oss-120b-medium") {
        return (trimmed.to_string(), None);
    }
    if let Some((base, suffix)) = trimmed.rsplit_once('-')
        && !base.is_empty()
    {
        let inferred = match suffix.to_ascii_lowercase().as_str() {
            "low" => Some(ReasoningEffort::Low),
            "medium" => Some(ReasoningEffort::Medium),
            "high" => Some(ReasoningEffort::High),
            "xhigh" => Some(ReasoningEffort::Xhigh),
            "max" => Some(ReasoningEffort::Max),
            "ultra" => Some(ReasoningEffort::Ultra),
            _ => None,
        };
        if let Some(effort) = inferred {
            return (base.to_string(), Some(effort));
        }
    }
    (trimmed.to_string(), None)
}

pub fn prettify_model_slug(raw_id: &str, provider: Option<ProviderKind>) -> String {
    let mut cleaned = raw_id.trim().to_string();
    cleaned = strip_trailing_yyyymmdd(&cleaned);
    cleaned = strip_trailing_yyyy_mm_dd(&cleaned);
    cleaned = strip_trailing_release_tag(&cleaned);

    match provider {
        Some(ProviderKind::Antigravity) => {
            cleaned = strip_agy_effort_suffix(&cleaned).0;
        }
        Some(ProviderKind::ClaudeCode)
            if cleaned.len() >= 7
                && cleaned.is_char_boundary(7)
                && cleaned[..7].eq_ignore_ascii_case("claude-") =>
        {
            cleaned = cleaned[7..].to_string();
        }
        _ => {}
    }

    if let Some(gpt_label) = try_format_gpt_slug(&cleaned) {
        return gpt_label;
    }

    if let Some(o_label) = try_format_o_series_slug(&cleaned) {
        return o_label;
    }

    cleaned = collapse_numeric_version_dashes(&cleaned);

    cleaned
        .split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            if w.as_bytes().first().is_some_and(u8::is_ascii_digit) {
                w.to_string()
            } else {
                capitalize_first(w)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn resolve_active_model_spec<'a>(
    catalog: &'a ProviderModelCatalog,
    raw_model_id: Option<&str>,
) -> Option<&'a ProviderModelSpec> {
    if catalog.models.is_empty() {
        return None;
    }
    let Some(raw) = raw_model_id.filter(|s| !s.is_empty()) else {
        return catalog
            .models
            .iter()
            .find(|m| m.is_default)
            .or_else(|| catalog.models.first());
    };

    if let Some(exact) = catalog
        .models
        .iter()
        .find(|m| m.id.eq_ignore_ascii_case(raw))
    {
        return Some(exact);
    }

    if catalog.provider == ProviderKind::Antigravity {
        let (stripped, _) = strip_agy_effort_suffix(raw);
        if let Some(by_base) = catalog
            .models
            .iter()
            .find(|m| m.id.eq_ignore_ascii_case(&stripped))
        {
            return Some(by_base);
        }
    }

    if catalog.provider == ProviderKind::ClaudeCode {
        let needle = format!("-{}-", raw.to_ascii_lowercase());
        if let Some(by_family) = catalog
            .models
            .iter()
            .find(|m| m.id.to_ascii_lowercase().contains(&needle))
        {
            return Some(by_family);
        }
    }

    None
}

pub fn format_catalog_model_label(
    catalog: &ProviderModelCatalog,
    raw_model_id: Option<&str>,
) -> String {
    let raw_non_empty = raw_model_id.filter(|s| !s.is_empty());
    if let Some(matched) = resolve_active_model_spec(catalog, raw_non_empty) {
        return matched.label.clone();
    }
    let Some(raw) = raw_non_empty else {
        return "Default".to_string();
    };
    prettify_model_slug(raw, Some(catalog.provider))
}

pub fn resolve_supported_effort_for_model(
    spec: Option<&ProviderModelSpec>,
    desired: Option<ReasoningEffort>,
    catalog_default: Option<ReasoningEffort>,
) -> Option<ReasoningEffort> {
    let Some(spec) = spec else {
        return desired
            .or(catalog_default)
            .or(Some(ReasoningEffort::Medium));
    };
    if spec.thinking_options.is_empty() {
        return None;
    }
    if let Some(want) = desired
        && spec.thinking_options.iter().any(|o| o.effort == want)
    {
        return Some(want);
    }
    if let Some(def) = spec.default_effort
        && spec.thinking_options.iter().any(|o| o.effort == def)
    {
        return Some(def);
    }
    if let Some(cat_def) = catalog_default
        && spec.thinking_options.iter().any(|o| o.effort == cat_def)
    {
        return Some(cat_def);
    }
    spec.thinking_options.first().map(|o| o.effort)
}

const RETIRED_MODEL_SLUGS: &[&str] = &[
    "gpt-5.4",
    "gpt-5.3-codex",
    "gpt-5.2-codex",
    "gpt-5.4-mini",
    "gemini-3.1-pro",
    "gemini-3-flash",
    "gemini-2.5-pro",
    "claude-sonnet-4-5",
    "opusplan",
    "opus",
    "sonnet",
    "haiku",
];

pub fn is_retired_model_slug(slug: &str) -> bool {
    let trimmed = slug.trim();
    if trimmed.is_empty() {
        return false;
    }
    RETIRED_MODEL_SLUGS
        .iter()
        .any(|retired| retired.eq_ignore_ascii_case(trimmed))
}

pub fn sanitize_model_slug(raw: Option<&str>) -> Option<String> {
    let clean = raw.map(str::trim).filter(|s| !s.is_empty())?;
    if is_retired_model_slug(clean) {
        None
    } else {
        Some(clean.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedModelSelectionView {
    pub provider: ProviderKind,
    pub catalog: ProviderModelCatalog,
    pub selected_model_id: Option<String>,
    pub selected_model_label: String,
    pub selected_effort: Option<ReasoningEffort>,
    pub selected_effort_label: Option<String>,
    pub thinking_options: Vec<ThinkingOptionSpec>,
}

pub fn resolve_model_selection(
    effective_catalogs: &[ProviderModelCatalog],
    provider: ProviderKind,
    desired_model: Option<&str>,
    desired_effort: Option<ReasoningEffort>,
) -> ResolvedModelSelectionView {
    let catalog = effective_catalogs
        .iter()
        .find(|c| c.provider == provider)
        .cloned()
        .unwrap_or_else(|| fallback_provider_catalog(provider));

    let clean_desired = sanitize_model_slug(desired_model);
    let active_spec = resolve_active_model_spec(&catalog, clean_desired.as_deref());
    let selected_model_id = active_spec
        .map(|s| s.id.clone())
        .or_else(|| clean_desired.clone())
        .or_else(|| catalog.configured_default_model.clone());

    let agy_suffix_effort = if desired_effort.is_none() && provider == ProviderKind::Antigravity {
        clean_desired
            .as_deref()
            .and_then(|raw| strip_agy_effort_suffix(raw).1)
    } else {
        None
    };

    let selected_effort = resolve_supported_effort_for_model(
        active_spec,
        desired_effort.or(agy_suffix_effort),
        catalog.configured_default_effort,
    );
    let selected_model_label = format_catalog_model_label(&catalog, selected_model_id.as_deref());
    let selected_effort_label = selected_effort.map(|e| format_effort_label(e).to_string());
    let thinking_options = active_spec
        .map(|s| s.thinking_options.clone())
        .unwrap_or_default();

    ResolvedModelSelectionView {
        provider,
        catalog,
        selected_model_id,
        selected_model_label,
        selected_effort,
        selected_effort_label,
        thinking_options,
    }
}

fn capitalize_first(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = first.to_uppercase().collect::<String>();
    out.push_str(chars.as_str());
    out
}

fn capitalize_hyphen_words(raw: &str) -> String {
    raw.split('-')
        .filter(|w| !w.is_empty())
        .map(capitalize_first)
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_trailing_yyyymmdd(s: &str) -> String {
    if let Some((prefix, tail)) = s.rsplit_once('-')
        && !prefix.is_empty()
        && tail.len() == 8
        && tail.bytes().all(|b| b.is_ascii_digit())
    {
        return prefix.to_string();
    }
    s.to_string()
}

fn strip_trailing_yyyy_mm_dd(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() > 11 {
        let start = bytes.len() - 11;
        let tail = &bytes[start..];
        if tail[0] == b'-'
            && tail[1..5].iter().all(u8::is_ascii_digit)
            && tail[5] == b'-'
            && tail[6..8].iter().all(u8::is_ascii_digit)
            && tail[8] == b'-'
            && tail[9..11].iter().all(u8::is_ascii_digit)
        {
            return s[..start].to_string();
        }
    }
    s.to_string()
}

fn strip_trailing_release_tag(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    if let Some(prefix) = lower.strip_suffix("-latest")
        && !prefix.is_empty()
    {
        return s[..prefix.len()].to_string();
    }

    // Match earliest `-(preview|exp)(?:-[a-z0-9-]+)?$`
    for (idx, _) in lower.match_indices('-') {
        if idx == 0 {
            continue;
        }
        let rest = &lower[idx + 1..];
        let after_tag = rest
            .strip_prefix("preview")
            .or_else(|| rest.strip_prefix("exp"));
        if let Some(rem) = after_tag {
            let valid = if rem.is_empty() {
                true
            } else if let Some(extra) = rem.strip_prefix('-') {
                !extra.is_empty()
                    && extra
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            } else {
                false
            };
            if valid {
                return s[..idx].to_string();
            }
        }
    }

    s.to_string()
}

fn try_format_gpt_slug(cleaned: &str) -> Option<String> {
    if cleaned.len() < 5
        || !cleaned.is_char_boundary(4)
        || !cleaned[..4].eq_ignore_ascii_case("gpt-")
    {
        return None;
    }
    let after = &cleaned[4..];
    let d1_len = after.bytes().take_while(u8::is_ascii_digit).count();
    if d1_len == 0 {
        return None;
    }
    let d1 = &after[..d1_len];
    let rem1 = &after[d1_len..];
    if rem1.is_empty() {
        return Some(format!("GPT {d1}"));
    }

    if let Some(after_sep) = rem1.strip_prefix('.').or_else(|| rem1.strip_prefix('-')) {
        let d2_len = after_sep.bytes().take_while(u8::is_ascii_digit).count();
        if d2_len > 0 {
            let d2 = &after_sep[..d2_len];
            let rem2 = &after_sep[d2_len..];
            if rem2.is_empty() {
                return Some(format!("GPT {d1}.{d2}"));
            }
            if let Some(rest) = rem2.strip_prefix('-')
                && !rest.is_empty()
            {
                return Some(format!("GPT {d1}.{d2} {}", capitalize_hyphen_words(rest)));
            }
        }
    }

    if let Some(rest) = rem1.strip_prefix('-')
        && !rest.is_empty()
    {
        return Some(format!("GPT {d1} {}", capitalize_hyphen_words(rest)));
    }

    None
}

fn try_format_o_series_slug(cleaned: &str) -> Option<String> {
    let bytes = cleaned.as_bytes();
    if bytes.first().copied().map(|b| b.to_ascii_lowercase()) != Some(b'o') {
        return None;
    }
    let after_o = &cleaned[1..];
    let digit_len = after_o.bytes().take_while(u8::is_ascii_digit).count();
    if digit_len == 0 {
        return None;
    }
    let base = format!("o{}", &after_o[..digit_len]);
    let rem = &after_o[digit_len..];
    if rem.is_empty() {
        return Some(base);
    }
    if let Some(rest) = rem.strip_prefix('-')
        && !rest.is_empty()
    {
        return Some(format!("{base} {}", capitalize_hyphen_words(rest)));
    }
    None
}

fn collapse_numeric_version_dashes(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start1 = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let end1 = i;
            if end1 < bytes.len() && bytes[end1] == b'-' {
                let start2 = end1 + 1;
                let mut end2 = start2;
                while end2 < bytes.len() && bytes[end2].is_ascii_digit() {
                    end2 += 1;
                }
                if end2 > start2 && (end2 == bytes.len() || bytes[end2] == b'-') {
                    out.push_str(&s[start1..end1]);
                    out.push('.');
                    out.push_str(&s[start2..end2]);
                    i = end2;
                    continue;
                }
            }
            out.push_str(&s[start1..end1]);
            continue;
        }
        if let Some(ch) = s[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_catalogs_fallback_and_live_override() {
        let defaults = effective_provider_catalogs(&[]);
        assert_eq!(defaults.len(), 3);
        assert_eq!(defaults[0].provider, ProviderKind::ClaudeCode);
        assert_eq!(defaults[1].provider, ProviderKind::Codex);
        assert_eq!(defaults[2].provider, ProviderKind::Antigravity);
        assert_eq!(
            defaults[1].configured_default_model.as_deref(),
            Some("gpt-6.2-sol")
        );

        let custom_codex = ProviderModelCatalog {
            provider: ProviderKind::Codex,
            configured_default_model: Some("gpt-custom".into()),
            configured_default_effort: Some(ReasoningEffort::High),
            models: vec![ProviderModelSpec {
                id: "gpt-custom".into(),
                label: "GPT Custom".into(),
                description: "Live".into(),
                is_default: true,
                default_effort: Some(ReasoningEffort::High),
                thinking_options: Vec::new(),
            }],
        };
        let empty_claude = ProviderModelCatalog {
            provider: ProviderKind::ClaudeCode,
            configured_default_model: None,
            configured_default_effort: None,
            models: Vec::new(),
        };
        let merged = effective_provider_catalogs(&[custom_codex, empty_claude]);
        assert_eq!(merged[0].models.len(), 10);
        assert_eq!(merged[1].models.len(), 1);
        assert_eq!(merged[1].models[0].id, "gpt-custom");
        assert_eq!(merged[2].models.len(), 8);
    }

    #[test]
    fn strip_agy_effort_suffix_handles_suffixes_and_gpt_oss_exception() {
        assert_eq!(
            strip_agy_effort_suffix("gemini-4-carbon-high"),
            ("gemini-4-carbon".to_string(), Some(ReasoningEffort::High))
        );
        assert_eq!(
            strip_agy_effort_suffix("gemini-4-hydrogen-XHIGH"),
            (
                "gemini-4-hydrogen".to_string(),
                Some(ReasoningEffort::Xhigh)
            )
        );
        assert_eq!(
            strip_agy_effort_suffix("gpt-oss-120b-medium"),
            ("gpt-oss-120b-medium".to_string(), None)
        );
        assert_eq!(
            strip_agy_effort_suffix("gemini-pro-orchestrator"),
            ("gemini-pro-orchestrator".to_string(), None)
        );
    }

    #[test]
    fn prettify_model_slug_matches_desktop_conventions() {
        assert_eq!(
            prettify_model_slug("gpt-6.2-sol", Some(ProviderKind::Codex)),
            "GPT 6.2 Sol"
        );
        assert_eq!(
            prettify_model_slug("gpt-6-astra", Some(ProviderKind::Codex)),
            "GPT 6 Astra"
        );
        assert_eq!(
            prettify_model_slug("o4-mini-high", Some(ProviderKind::Codex)),
            "o4 Mini High"
        );
        assert_eq!(
            prettify_model_slug("claude-opus-5-5-20260301", Some(ProviderKind::ClaudeCode)),
            "Opus 5.5"
        );
        assert_eq!(
            prettify_model_slug("gemini-4-carbon-high", Some(ProviderKind::Antigravity)),
            "Gemini 4 Carbon"
        );
        assert_eq!(
            prettify_model_slug("gemini-2.5-pro-preview-05-06", None),
            "Gemini 2.5 Pro"
        );
        assert_eq!(
            prettify_model_slug(
                "claude-sonnet-4-6-2026-02-15",
                Some(ProviderKind::ClaudeCode)
            ),
            "Sonnet 4.6"
        );
    }

    #[test]
    fn resolve_active_model_and_effort_fallbacks() {
        let agy = fallback_provider_catalog(ProviderKind::Antigravity);
        let claude = fallback_provider_catalog(ProviderKind::ClaudeCode);

        // Antigravity suffix resolution
        let carbon =
            resolve_active_model_spec(&agy, Some("gemini-4-carbon-high")).expect("carbon spec");
        assert_eq!(carbon.id, "gemini-4-carbon");
        assert_eq!(
            format_catalog_model_label(&agy, Some("gemini-4-carbon-high")),
            "Gemini 4 Carbon"
        );

        // Orchestrator disables thinking options -> effort resolves to None
        let orch =
            resolve_active_model_spec(&agy, Some("gemini-pro-orchestrator")).expect("orchestrator");
        assert_eq!(
            resolve_supported_effort_for_model(
                Some(orch),
                Some(ReasoningEffort::High),
                agy.configured_default_effort
            ),
            None
        );

        // Unsupported Ultra on AGY Carbon falls back to model default (Medium)
        assert_eq!(
            resolve_supported_effort_for_model(
                Some(carbon),
                Some(ReasoningEffort::Ultra),
                agy.configured_default_effort
            ),
            Some(ReasoningEffort::Medium)
        );

        // Claude short alias `sonnet` resolves to `claude-sonnet-5-5`
        let sonnet = resolve_active_model_spec(&claude, Some("sonnet")).expect("sonnet");
        assert_eq!(sonnet.id, "claude-sonnet-5-5");

        // Unknown model spec falls back cleanly
        assert_eq!(
            format_catalog_model_label(&claude, Some("claude-custom-7-1")),
            "Custom 7.1"
        );
        assert_eq!(
            resolve_supported_effort_for_model(None, None, None),
            Some(ReasoningEffort::Medium)
        );
    }

    #[test]
    fn sanitize_and_resolve_model_selection_across_providers() {
        assert!(is_retired_model_slug("gemini-2.5-pro"));
        assert!(is_retired_model_slug("gpt-5.4"));
        assert!(is_retired_model_slug("claude-sonnet-4-5"));
        assert_eq!(sanitize_model_slug(Some("  sonnet  ")), None);
        assert_eq!(
            sanitize_model_slug(Some("claude-sonnet-5-5")),
            Some("claude-sonnet-5-5".into())
        );

        let catalogs = effective_provider_catalogs(&[]);
        // Retired slug resets cleanly to catalog default
        let claude_res =
            resolve_model_selection(&catalogs, ProviderKind::ClaudeCode, Some("sonnet"), None);
        assert_eq!(
            claude_res.selected_model_id.as_deref(),
            Some("claude-opus-5-5")
        );
        assert!(!claude_res.thinking_options.is_empty());

        // Antigravity suffix effort extraction
        let agy_res = resolve_model_selection(
            &catalogs,
            ProviderKind::Antigravity,
            Some("gemini-4-carbon-high"),
            None,
        );
        assert_eq!(
            agy_res.selected_model_id.as_deref(),
            Some("gemini-4-carbon")
        );
        assert_eq!(agy_res.selected_effort, Some(ReasoningEffort::High));
        assert_eq!(agy_res.selected_effort_label.as_deref(), Some("High"));

        // Orchestrator exposes empty thinking options and None effort
        let orch_res = resolve_model_selection(
            &catalogs,
            ProviderKind::Antigravity,
            Some("gemini-pro-orchestrator"),
            Some(ReasoningEffort::High),
        );
        assert_eq!(orch_res.selected_effort, None);
        assert!(orch_res.thinking_options.is_empty());
    }
}
