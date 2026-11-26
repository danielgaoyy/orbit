//! AI-powered Mission Workspace auto-triage & semantic conversation summarizer.
//!
//! Uses Orbit's zero-tool, non-persistent ephemeral prompt runner (`run_ephemeral_text_prompt`)
//! against the host's installed provider CLI (`claude`, `agy`, or `codex`) to:
//! 1. Understand each conversation's real intent and generate a concise semantic tab label (`handle`)
//!    + 1-line live status summary (`task_summary`) — eliminating generic harness names like `@agy`.
//! 2. Classify unfiled sessions into existing curated Mission Workspaces or group
//!    cohesive sessions into newly created curated Mission Workspaces (always reversible).
//!
//! Falls back gracefully to a fast deterministic lexical/file-overlap organizer when running under
//! test mock overrides (`is_provider_overridden`), offline environments, or `ORBIT_ORGANIZER_MODE=heuristic`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use orbit_protocol::{
    AgentSummary, ProviderKind, WorkspaceClusterSuggestion, WorkspaceEventKind, WorkspaceIsolation,
    WorkspaceOrganizeEvent, WorkspaceOrganizePhase, WorkspaceSummary, WorktreeCreateMode,
};
use orbit_providers::{
    find_provider_executable, has_any_provider_override, is_provider_overridden,
    run_ephemeral_text_prompt,
};
use orbit_store::{AgentContextSnippet, SemanticIdentityOptions};
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use crate::manager::{AgentOrchestrator, CreateWorkspaceSpec, DaemonBroadcast};
use crate::workspace_cluster::{
    AUTO_CLUSTER_HIGH_THRESHOLD, allocate_unique_handle, derive_semantic_handle_from_prompt,
    evaluate_auto_cluster, extract_semantic_tokens, is_generic_harness_handle, normalize_root_path,
    sanitize_handle,
};

const MAX_AGENTS_PER_ORGANIZE_BATCH: usize = 16;
const LLM_SUMMARY_TIMEOUT_SECS: u64 = 35;
const LLM_ORGANIZE_TIMEOUT_SECS: u64 = 55;
const MAX_CONCISE_TITLE_CHARS: usize = 48;

pub const LABEL_TITLE_LOCKED: &str = "orbit.title_locked";
pub const LABEL_AUTO_NAMED: &str = "orbit.auto_named";

pub fn is_title_locked(labels: &HashMap<String, String>) -> bool {
    labels
        .get(LABEL_TITLE_LOCKED)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn is_auto_named(labels: &HashMap<String, String>) -> bool {
    labels
        .get(LABEL_AUTO_NAMED)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Deserialize)]
struct LlmSummaryPayload {
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    task_summary: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmOrganizePayload {
    #[serde(default)]
    decisions: Vec<LlmOrganizeDecision>,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmOrganizeDecision {
    agent_id: String,
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    task_summary: Option<String>,
    /// `"join"` | `"create"` | `"unclassified"`
    #[serde(default)]
    action: String,
    #[serde(default)]
    target_workspace_id: Option<String>,
    #[serde(default)]
    new_workspace_key: Option<String>,
    #[serde(default)]
    new_workspace_title: Option<String>,
    #[serde(default)]
    new_workspace_goal: Option<String>,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    reason: Option<String>,
}

/// Choose installed providers suitable for fast ephemeral organizer prompts in priority order.
/// Returns empty when running in forced heuristic mode or when mock provider overrides are active.
fn pick_organizer_providers(preferred: Option<ProviderKind>) -> Vec<ProviderKind> {
    let mode = std::env::var("ORBIT_ORGANIZER_MODE")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if mode == "heuristic" || mode == "off" || mode == "disabled" {
        return Vec::new();
    }
    let force_llm = mode == "llm" || mode == "force_llm";
    // When running unit tests or when in-process mock provider overrides are active, never invoke
    // real host CLIs accidentally unless explicitly forced via `ORBIT_ORGANIZER_MODE=llm`.
    if !force_llm && (cfg!(test) || has_any_provider_override()) {
        return Vec::new();
    }

    let mut order: Vec<ProviderKind> = Vec::with_capacity(4);
    if let Some(p) = preferred
        && matches!(
            p,
            ProviderKind::ClaudeCode | ProviderKind::Antigravity | ProviderKind::Codex
        )
    {
        order.push(p);
    }
    for fallback in [
        ProviderKind::ClaudeCode,
        ProviderKind::Antigravity,
        ProviderKind::Codex,
    ] {
        if !order.contains(&fallback) {
            order.push(fallback);
        }
    }

    order
        .into_iter()
        .filter(|&kind| {
            find_provider_executable(kind).is_some() && (force_llm || !is_provider_overridden(kind))
        })
        .collect()
}

/// Extract the outermost balanced JSON object `{ ... }` from raw LLM output (handles prose or code fences).
pub(crate) fn extract_json_object(raw: &str) -> Option<&str> {
    let bytes = raw.as_bytes();
    let start = raw.find('{')?;
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, &b) in bytes[start..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&raw[start..=start + idx]);
                }
            }
            _ => {}
        }
    }
    None
}

fn compact_snippet(raw: Option<&str>, max_chars: usize) -> String {
    let Some(text) = raw else {
        return String::new();
    };
    // Strip injected `<orbit-workspace>...</orbit-workspace>` blocks so the organizer only sees the real conversation!
    let stripped = strip_orbit_workspace_xml(text);
    let collapsed: String = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, max_chars)
}

fn strip_orbit_workspace_xml(raw: &str) -> &str {
    let mut cur = raw;
    loop {
        let trimmed = cur.trim_start();
        if trimmed.starts_with("<orbit-workspace-delta")
            && let Some(end_idx) = trimmed.find("</orbit-workspace-delta>")
        {
            cur = trimmed[end_idx + "</orbit-workspace-delta>".len()..].trim_start();
            continue;
        }
        if (trimmed.starts_with("<orbit-workspace ") || trimmed.starts_with("<orbit-workspace>"))
            && let Some(end_idx) = trimmed.find("</orbit-workspace>")
        {
            cur = trimmed[end_idx + "</orbit-workspace>".len()..].trim_start();
            continue;
        }
        if trimmed.starts_with("<WORKSPACE_RULES>")
            && let Some(end_idx) = trimmed.find("</WORKSPACE_RULES>")
        {
            cur = trimmed[end_idx + "</WORKSPACE_RULES>".len()..].trim_start();
            continue;
        }
        break;
    }
    cur
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        raw.to_string()
    } else {
        let mut s: String = raw.chars().take(max_chars.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

fn clean_semantic_handle(raw: Option<&str>) -> Option<String> {
    let cleaned = raw.map(sanitize_handle).filter(|s| !s.is_empty())?;
    if is_generic_harness_handle(&cleaned) {
        None
    } else {
        Some(cleaned)
    }
}

fn clean_summary_line(raw: Option<&str>) -> Option<String> {
    let collapsed: String = raw?.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(truncate_chars(&collapsed, 96))
    }
}

fn clean_concise_title(raw: Option<&str>) -> Option<String> {
    let text = raw?;
    let trimmed = text
        .trim()
        .trim_matches(['"', '\'', '`', '“', '”', '‘', '’']);
    let collapsed: String = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(truncate_title_smart(&collapsed, MAX_CONCISE_TITLE_CHARS))
    }
}

fn truncate_title_smart(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        return raw.to_string();
    }
    let budget = max_chars.saturating_sub(1);
    let prefix: String = raw.chars().take(budget).collect();
    // Prefer breaking at an ASCII word boundary when there is a late space so English titles don't cut mid-word.
    let candidate = if let Some(last_space) = prefix.rfind(' ')
        && prefix[..last_space].chars().count() >= budget / 2
    {
        prefix[..last_space].trim_end()
    } else {
        prefix.trim_end()
    };
    let clean = candidate.trim_end_matches([',', ';', ':', '-', '—', '·']);
    if clean.is_empty() {
        let mut s: String = raw.chars().take(budget).collect();
        s.push('…');
        s
    } else {
        format!("{clean}…")
    }
}

fn strip_markdown_fenced_blocks(raw: &str) -> String {
    let mut kept = Vec::new();
    let mut active_fence: Option<(char, usize)> = None;

    for line in raw.lines() {
        let trimmed = line.trim_start();
        if let Some((fch, flen)) = active_fence {
            let run = trimmed.chars().take_while(|&c| c == fch).count();
            if run >= flen && trimmed[run..].trim().is_empty() {
                active_fence = None;
            }
            continue;
        }
        if let Some(first) = trimmed.chars().next()
            && (first == '`' || first == '~')
        {
            let run = trimmed.chars().take_while(|&c| c == first).count();
            if run >= 3 {
                active_fence = Some((first, run));
                continue;
            }
        }
        kept.push(line);
    }
    kept.join("\n")
}

fn strip_markdown_links(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find('[') {
        let is_img = open > 0 && rest.as_bytes().get(open - 1) == Some(&b'!');
        let prefix_end = if is_img { open - 1 } else { open };
        out.push_str(&rest[..prefix_end]);
        let after_open = &rest[open + 1..];
        if let Some(close_bracket) = after_open.find("](") {
            let label = &after_open[..close_bracket];
            let after_mid = &after_open[close_bracket + 2..];
            if let Some(close_paren) = after_mid.find(')') {
                out.push_str(label);
                rest = &after_mid[close_paren + 1..];
                continue;
            }
        }
        out.push_str(&rest[prefix_end..=open]);
        rest = after_open;
    }
    out.push_str(rest);
    out
}

fn strip_leading_slash_directive(raw: &str) -> &str {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with('/') {
        return trimmed;
    }
    let lower = trimmed.to_ascii_lowercase();
    for prefix in [
        "/orbit-committee",
        "/orbit-handoff",
        "/orbit-advisor",
        "/orbit",
        "/plan",
        "/review",
        "/compact",
        "/goal",
        "/fork",
        "/btw",
    ] {
        if let Some(after) = trimmed.get(prefix.len()..)
            && lower.starts_with(prefix)
            && (after.is_empty() || after.starts_with(char::is_whitespace))
        {
            return after.trim_start();
        }
    }
    trimmed
}

fn strip_markdown_line_prefix(line: &str) -> &str {
    let mut s = line.trim();
    loop {
        let prev = s;
        s = s.trim_start_matches(['#', '>', ' ', '\t']);
        if let Some(rest) = s
            .strip_prefix("- ")
            .or_else(|| s.strip_prefix("* "))
            .or_else(|| s.strip_prefix("• "))
            .or_else(|| s.strip_prefix("+ "))
        {
            s = rest.trim_start();
        } else {
            let digits = s.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0
                && digits <= 3
                && let Some(after_digits) = s.get(digits..)
                && (after_digits.starts_with(". ") || after_digits.starts_with(") "))
            {
                s = after_digits[2..].trim_start();
            }
        }
        if s == prev {
            break;
        }
    }
    s
}

/// Lightweight deterministic fallback title extractor used when the ephemeral LLM call is
/// unavailable or times out. Strips `<orbit-workspace>` XML, fenced code blocks, markdown syntax,
/// `@mentions`, slash directives, and newlines to produce a crisp title (`<= 48` chars).
pub fn extract_fallback_title(prompt: &str) -> String {
    let no_xml = strip_orbit_workspace_xml(prompt);
    let no_fences = strip_markdown_fenced_blocks(no_xml);
    let no_links = strip_markdown_links(&no_fences);

    // Pick the first substantive non-empty line (or join short consecutive prose lines).
    let mut prose_lines: Vec<String> = Vec::new();
    for raw_line in no_links.lines() {
        let cleaned_line = strip_markdown_line_prefix(strip_leading_slash_directive(raw_line));
        if cleaned_line.is_empty() {
            if !prose_lines.is_empty() {
                break;
            }
            continue;
        }
        prose_lines.push(cleaned_line.to_string());
        let joined_chars: usize = prose_lines.iter().map(|l| l.chars().count()).sum();
        if joined_chars >= 64 || prose_lines.len() >= 2 {
            break;
        }
    }

    let combined = prose_lines.join(" ");
    // Remove markdown emphasis/code delimiters while keeping inner text intact.
    let mut stripped = String::with_capacity(combined.len());
    for ch in combined.chars() {
        if matches!(ch, '`' | '*' | '~') {
            continue;
        }
        stripped.push(ch);
    }
    let stripped = stripped.replace("__", "");

    // Strip `@mention` / `@path/to/file` tokens when enough substantive prose remains;
    // otherwise shorten `@path/to/file.rs` -> `file.rs`.
    let tokens: Vec<&str> = stripped.split_whitespace().collect();
    let without_mentions: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|tok| !tok.starts_with('@') || tok.len() <= 1)
        .collect();
    let non_mention_chars: usize = without_mentions.iter().map(|t| t.chars().count()).sum();
    let normalized = if non_mention_chars >= 4 {
        without_mentions.join(" ")
    } else {
        tokens
            .iter()
            .map(|tok| {
                if let Some(rest) = tok.strip_prefix('@') {
                    let base = rest.rsplit('/').next().unwrap_or(rest);
                    base.trim_matches([',', ';', ':', ')'])
                } else {
                    tok
                }
            })
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };

    let clean = normalized
        .trim()
        .trim_matches(['"', '\'', ':', '-', '—', '·', ',', ';']);
    if clean.is_empty() {
        let raw_collapsed: String = no_xml.split_whitespace().collect::<Vec<_>>().join(" ");
        if raw_collapsed.is_empty() {
            return "New Conversation".to_string();
        }
        return truncate_title_smart(&raw_collapsed, MAX_CONCISE_TITLE_CHARS);
    }

    // If a crisp first sentence ends before our budget, stop cleanly at the sentence boundary.
    let mut sentence_end: Option<(usize, bool)> = None;
    for (idx, ch) in clean.char_indices() {
        let char_pos = clean[..idx].chars().count();
        if char_pos < 8 {
            continue;
        }
        if char_pos >= MAX_CONCISE_TITLE_CHARS {
            break;
        }
        match ch {
            '。' | '！' | '？' => {
                let keep_punct = ch == '？';
                sentence_end = Some((idx + ch.len_utf8(), keep_punct));
                break;
            }
            '.' | '!' | '?' => {
                let next_is_space = clean[idx + 1..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace);
                if next_is_space {
                    let keep_punct = ch == '?';
                    sentence_end = Some((idx + 1, keep_punct));
                    break;
                }
            }
            _ => {}
        }
    }

    let focused = if let Some((end_byte, keep_punct)) = sentence_end {
        let slice = &clean[..end_byte];
        if keep_punct {
            slice.trim()
        } else {
            slice.trim().trim_end_matches(['.', '。', '!', '！']).trim()
        }
    } else {
        clean
    };

    truncate_title_smart(focused, MAX_CONCISE_TITLE_CHARS)
}

fn raw_default_prompt_title(prompt: &str) -> String {
    let first_line = prompt
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("New Session");
    let trimmed = first_line.trim();
    if trimmed.chars().count() <= 48 {
        trimmed.to_string()
    } else {
        let mut s: String = trimmed.chars().take(48).collect();
        s.push('…');
        s
    }
}

/// Returns `true` when `agent.title` is still a placeholder or raw truncated prompt default
/// (and NOT locked via `orbit.title_locked = "1"`), meaning auto-naming may safely refine it
/// without clobbering an explicit user title.
pub fn is_auto_nameable_title(agent: &AgentSummary, first_user_prompt: Option<&str>) -> bool {
    if is_title_locked(&agent.labels) {
        return false;
    }
    let title = agent.title.trim();
    if title.is_empty()
        || title.eq_ignore_ascii_case("new thread")
        || title.eq_ignore_ascii_case("new session")
        || title.eq_ignore_ascii_case("new conversation")
        || title.eq_ignore_ascii_case("conversation")
        || title == "新会话"
    {
        return true;
    }

    let Some(raw_prompt) = first_user_prompt else {
        return false;
    };
    let stripped_prompt = strip_orbit_workspace_xml(raw_prompt);
    for candidate in [raw_prompt, stripped_prompt] {
        let default_title = raw_default_prompt_title(candidate);
        if title == default_title {
            return true;
        }
        let first_line = candidate
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(str::trim)
            .unwrap_or("");
        if first_line.is_empty() {
            continue;
        }
        let title_stem = title.trim_end_matches('…').trim_end_matches("...").trim();
        if !title_stem.is_empty() && first_line.starts_with(title_stem) {
            // Exact match or truncated prefix of the raw first user prompt line
            if title_stem == first_line
                || title.ends_with('…')
                || title.ends_with("...")
                || title_stem.chars().count() >= 24
            {
                return true;
            }
        }
    }

    false
}

fn derive_heuristic_identity(
    agent: &AgentSummary,
    snippet: &AgentContextSnippet,
    allow_title_update: bool,
) -> (Option<String>, Option<String>, String) {
    let first = compact_snippet(snippet.first_user_prompt.as_deref(), 160);
    let latest = compact_snippet(snippet.latest_user_prompt.as_deref(), 160);
    let reply = compact_snippet(snippet.latest_assistant_message.as_deref(), 140);
    let raw_seed = snippet
        .first_user_prompt
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            snippet
                .latest_user_prompt
                .as_deref()
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or(agent.title.as_str());
    let seed = if !first.is_empty() {
        first.as_str()
    } else if !latest.is_empty() {
        latest.as_str()
    } else {
        agent.title.as_str()
    };

    let handle = derive_semantic_handle_from_prompt(seed)
        .or_else(|| derive_semantic_handle_from_prompt(&agent.title));

    let fallback_title = extract_fallback_title(raw_seed);
    let summary_text = if !snippet.edited_files.is_empty() {
        let files_preview: Vec<&str> = snippet
            .edited_files
            .iter()
            .take(2)
            .map(|p| p.rsplit('/').next().unwrap_or(p.as_str()))
            .collect();
        let base = if !fallback_title.is_empty() && fallback_title != "New Conversation" {
            truncate_chars(&fallback_title, 48)
        } else if !first.is_empty() {
            truncate_chars(&first, 56)
        } else {
            truncate_chars(&agent.title, 56)
        };
        format!("{base} ({})", files_preview.join(", "))
    } else if !first.is_empty() {
        truncate_chars(&first, 76)
    } else if !reply.is_empty() {
        truncate_chars(&reply, 76)
    } else {
        truncate_chars(&agent.title, 76)
    };

    let title = if allow_title_update {
        Some(fallback_title)
    } else {
        None
    };

    (handle, title, summary_text)
}

fn build_summarize_prompt(agent: &AgentSummary, snippet: &AgentContextSnippet) -> String {
    let first = compact_snippet(snippet.first_user_prompt.as_deref(), 420);
    let latest = compact_snippet(snippet.latest_user_prompt.as_deref(), 320);
    let reply = compact_snippet(snippet.latest_assistant_message.as_deref(), 360);
    let files = if snippet.edited_files.is_empty() {
        "(none)".to_string()
    } else {
        snippet.edited_files.join(", ")
    };

    format!(
        "You are Orbit's ultra-fast conversation identity summarizer.\n\
         Given the coding conversation context below, return ONLY a valid JSON object (no markdown, no explanation) with keys:\n\
         - \"handle\": ultra-concise semantic tab label (1-2 short words or 2-8 CJK chars matching the user's language, e.g. \"sync-fix\", \"clipboard\", \"oauth-flow\", \"rate-limiter\"). NEVER use generic tool/model names like agy, claude, codex, peer, lead, agent, chat.\n\
         - \"title\": crisp concise thread title (<= 48 chars, matching the user's language, plain text with no surrounding quotes or markdown).\n\
         - \"task_summary\": single crisp sentence summarizing what this conversation is doing or achieved (<= 48 Chinese chars or <= 84 English chars, matching the user's language).\n\
         Do NOT call any tools or read any files.\n\n\
         Current Title: {}\n\
         First User Message: {}\n\
         Latest User Message: {}\n\
         Latest Assistant Reply: {}\n\
         Edited Files: {}\n",
        truncate_chars(&agent.title, 80),
        if first.is_empty() { "(empty)" } else { &first },
        if latest.is_empty() { "(same)" } else { &latest },
        if reply.is_empty() {
            "(pending)"
        } else {
            &reply
        },
        files,
    )
}

/// Bounded automatic naming + summary triggered once after a conversation completes its first
/// foreground turn (`Running -> Idle` with at least 1 user message + 1 assistant reply).
///
/// Strictly respects:
/// - `orbit.title_locked = "1"` (never overwrites locked titles)
/// - `orbit.auto_named = "1"` (runs at most once per conversation across daemon restarts)
/// - explicit non-default titles (only updates empty / `"New thread"` / raw truncated prompt defaults)
/// - bounded concurrency via `AgentOrchestrator::auto_name_semaphore`
pub async fn auto_name_agent_after_first_turn(
    mgr: &Arc<AgentOrchestrator>,
    agent_id: &str,
) -> anyhow::Result<Option<AgentSummary>> {
    let lock_key = format!("sum:{agent_id}");
    let Some(_guard) = mgr.try_acquire_organizer_lock(&lock_key) else {
        return Ok(None);
    };
    auto_name_agent_after_first_turn_inner(mgr, agent_id).await
}

async fn auto_name_agent_after_first_turn_inner(
    mgr: &Arc<AgentOrchestrator>,
    agent_id: &str,
) -> anyhow::Result<Option<AgentSummary>> {
    let Some(agent) = mgr.store().get_agent(agent_id)? else {
        return Ok(None);
    };
    if agent.archived_at.is_some()
        || agent.background
        || agent.parent_agent_id.is_some()
        || is_auto_named(&agent.labels)
    {
        return Ok(None);
    }

    let snippet = mgr.store().agent_context_snippet(agent_id)?;
    let has_user_prompt = snippet
        .first_user_prompt
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let has_assistant_reply = snippet
        .latest_assistant_message
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    if !has_user_prompt || !has_assistant_reply {
        return Ok(None);
    }

    let allow_title = is_auto_nameable_title(&agent, snippet.first_user_prompt.as_deref());
    let updated = run_agent_summary_pipeline(
        mgr,
        &agent,
        &snippet,
        allow_title,
        SemanticIdentityOptions {
            mark_auto_named: true,
            force_title: false,
        },
        true,
    )
    .await?;
    Ok(Some(updated))
}

/// Generate and persist a semantic tab handle, concise title (when unlocked), and 1-line
/// `task_summary` for one conversation. Always respects `orbit.title_locked = "1"` unless
/// `force_regenerate_title` is explicitly requested.
pub async fn summarize_agent_context(
    mgr: &Arc<AgentOrchestrator>,
    agent_id: &str,
) -> anyhow::Result<AgentSummary> {
    summarize_agent_context_with_options(mgr, agent_id, false).await
}

pub async fn summarize_agent_context_with_options(
    mgr: &Arc<AgentOrchestrator>,
    agent_id: &str,
    force_regenerate_title: bool,
) -> anyhow::Result<AgentSummary> {
    let lock_key = format!("sum:{agent_id}");
    let Some(_guard) = mgr.try_acquire_organizer_lock(&lock_key) else {
        return mgr
            .store()
            .get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"));
    };
    summarize_agent_context_inner(mgr, agent_id, force_regenerate_title).await
}

async fn summarize_agent_context_inner(
    mgr: &Arc<AgentOrchestrator>,
    agent_id: &str,
    force_regenerate_title: bool,
) -> anyhow::Result<AgentSummary> {
    let agent = mgr
        .store()
        .get_agent(agent_id)?
        .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
    let snippet = mgr.store().agent_context_snippet(agent_id)?;
    let allow_title = force_regenerate_title || !is_title_locked(&agent.labels);
    run_agent_summary_pipeline(
        mgr,
        &agent,
        &snippet,
        allow_title,
        SemanticIdentityOptions {
            mark_auto_named: true,
            force_title: force_regenerate_title,
        },
        false,
    )
    .await
}

async fn run_agent_summary_pipeline(
    mgr: &Arc<AgentOrchestrator>,
    agent: &AgentSummary,
    snippet: &AgentContextSnippet,
    allow_title: bool,
    identity_opts: SemanticIdentityOptions,
    throttle_llm: bool,
) -> anyhow::Result<AgentSummary> {
    let agent_id = agent.agent_id.as_str();
    let (mut desired_handle, mut desired_title, mut desired_summary) =
        derive_heuristic_identity(agent, snippet, allow_title);

    let providers = pick_organizer_providers(Some(agent.provider));
    if !providers.is_empty() {
        // Acquire bounded concurrency permit lazily right around the ephemeral LLM call
        // so ineligible/fast-path sessions never queue on the semaphore.
        let _llm_permit = if throttle_llm {
            mgr.auto_name_semaphore().clone().acquire_owned().await.ok()
        } else {
            None
        };
        if !throttle_llm || _llm_permit.is_some() {
            let prompt = build_summarize_prompt(agent, snippet);
            for provider in providers.into_iter().take(2) {
                match run_ephemeral_text_prompt(
                    provider,
                    PathBuf::from(&agent.cwd),
                    None,
                    prompt.clone(),
                    LLM_SUMMARY_TIMEOUT_SECS,
                )
                .await
                {
                    Ok(raw) => {
                        if let Some(json_str) = extract_json_object(&raw)
                            && let Ok(parsed) = serde_json::from_str::<LlmSummaryPayload>(json_str)
                        {
                            if let Some(h) = clean_semantic_handle(parsed.handle.as_deref()) {
                                desired_handle = Some(h);
                            }
                            if allow_title
                                && let Some(t) = clean_concise_title(parsed.title.as_deref())
                            {
                                desired_title = Some(t);
                            }
                            if let Some(ts) = clean_summary_line(parsed.task_summary.as_deref()) {
                                desired_summary = ts;
                            }
                            break;
                        }
                        warn!(
                            agent_id = %agent_id,
                            provider = ?provider,
                            "Organizer summary response was not parseable JSON; trying fallback"
                        );
                    }
                    Err(err) => {
                        warn!(
                            agent_id = %agent_id,
                            provider = ?provider,
                            error = %err,
                            "Organizer ephemeral summary call failed; trying fallback"
                        );
                    }
                }
            }
        }
    }

    if !allow_title {
        desired_title = None;
    }

    // Re-check latest persisted agent before committing so a concurrent manual rename
    // while the ephemeral LLM call was in flight is NEVER clobbered.
    let latest_agent = mgr
        .store()
        .get_agent(agent_id)?
        .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
    if !identity_opts.force_title && is_title_locked(&latest_agent.labels) {
        desired_title = None;
    }

    // Ensure uniqueness inside the agent's workspace if we're updating its handle
    let final_handle = if let Some(h) = desired_handle {
        let peers: Vec<AgentSummary> = mgr
            .store()
            .list_agents_filtered(Some(&latest_agent.workspace_id), false, false, None)
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.agent_id != agent_id)
            .collect();
        Some(allocate_unique_handle(
            Some(&h),
            latest_agent.provider,
            None,
            None,
            &peers,
        ))
    } else {
        None
    };

    let updated = mgr
        .store()
        .set_agent_semantic_identity_with_options(
            agent_id,
            final_handle.as_deref(),
            desired_title.as_deref(),
            Some(desired_summary.as_str()),
            identity_opts,
        )?
        .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
    let _ = mgr
        .broadcast_tx
        .send(DaemonBroadcast::AgentUpdated(updated.clone()));
    Ok(updated)
}

fn workspace_belongs_to_project(ws: &WorkspaceSummary, norm_project_root: &str) -> bool {
    let ws_root = ws
        .project_root
        .as_deref()
        .or(ws.source_cwd.as_deref())
        .unwrap_or(ws.cwd.as_str());
    let norm_ws_root = normalize_root_path(ws_root);
    let norm_ws_cwd = normalize_root_path(&ws.cwd);
    norm_ws_root == norm_project_root
        || norm_ws_cwd == norm_project_root
        || norm_ws_root.starts_with(&format!("{norm_project_root}/"))
        || norm_ws_cwd.starts_with(&format!("{norm_project_root}/"))
}

fn build_organize_prompt(
    project_root: &str,
    curated_workspaces: &[(WorkspaceSummary, Vec<AgentSummary>)],
    targets: &[(AgentSummary, AgentContextSnippet)],
    single_agent_manual: bool,
) -> String {
    let mut out = String::from(
        "You are Orbit's AI Mission Workspace Organizer.\n\
         Your job is to inspect unclassified coding conversations in a project, assign each a semantic tab handle and 1-line task summary, and group related conversations into Mission Workspaces.\n\
         Return ONLY a single valid JSON object (no markdown fences, no commentary) with schema:\n\
         {\n\
           \"decisions\": [\n\
             {\n\
               \"agent_id\": \"ag_...\",\n\
               \"handle\": \"concise 2-8 CJK char or 1-2 word English tab label (NEVER agy/claude/codex/peer/lead)\",\n\
               \"title\": \"concise thread title (<= 36 chars)\",\n\
               \"task_summary\": \"1-line summary of what this conversation is doing (<= 48 CJK chars or <= 84 EN chars)\",\n\
               \"action\": \"join\" | \"create\" | \"unclassified\",\n\
               \"target_workspace_id\": \"ws_... (required when action == join)\",\n\
               \"new_workspace_key\": \"short-slug shared by conversations that belong in the SAME newly created workspace (required when action == create)\",\n\
               \"new_workspace_title\": \"human-friendly Mission Workspace name (required when action == create)\",\n\
               \"new_workspace_goal\": \"one-line mission goal (optional)\",\n\
               \"confidence\": 0.0 to 1.0,\n\
               \"reason\": \"short explanation\"\n\
             }\n\
           ]\n\
         }\n\
         Rules:\n\
         - Match the user's language (Chinese prompts -> Chinese workspace titles, handles, summaries).\n\
         - Prefer \"join\" when a conversation clearly belongs to an existing curated Workspace.\n\
         - Use \"create\" with the SAME \"new_workspace_key\" when 2+ unclassified conversations share the same feature, bugfix, release, or topic.\n\
         - Keep trivial greetings/empty tests as \"unclassified\".\n",
    );
    if single_agent_manual {
        out.push_str(
            "- If a single substantial engineering conversation does not match any existing curated Workspace, use \"create\" with a crisp Mission Workspace title & goal.\n",
        );
    } else {
        out.push_str(
            "- For a standalone substantial engineering task with no matching Workspace, you may use \"create\" (confidence >= 0.78) if it represents a clear project mission, or \"unclassified\" if it is a quick one-off question.\n",
        );
    }
    out.push_str("Do NOT call tools or read files.\n\n");
    out.push_str(&format!("Project Root: {project_root}\n\n"));

    out.push_str("Existing Curated Workspaces:\n");
    if curated_workspaces.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for (ws, peers) in curated_workspaces {
            let peer_briefs: Vec<String> = peers
                .iter()
                .take(4)
                .map(|p| {
                    let h = p.handle.as_deref().unwrap_or("peer");
                    let s = p.task_summary.as_deref().unwrap_or(&p.title);
                    format!("@{h}: {}", truncate_chars(s, 48))
                })
                .collect();
            out.push_str(&format!(
                "  - id={} | title=\"{}\" | goal=\"{}\" | peers=[{}]\n",
                ws.workspace_id,
                truncate_chars(&ws.title, 48),
                truncate_chars(ws.mission_goal.as_deref().unwrap_or(""), 72),
                peer_briefs.join("; ")
            ));
        }
    }

    out.push_str("\nUnclassified Conversations To Organize:\n");
    for (ag, snip) in targets {
        let first = compact_snippet(snip.first_user_prompt.as_deref(), 220);
        let latest = compact_snippet(snip.latest_user_prompt.as_deref(), 160);
        let reply = compact_snippet(snip.latest_assistant_message.as_deref(), 160);
        let files = snip.edited_files.join(", ");
        out.push_str(&format!(
            "  - agent_id={} | title=\"{}\" | first=\"{}\" | latest=\"{}\" | reply=\"{}\" | files=[{}]\n",
            ag.agent_id,
            truncate_chars(&ag.title, 56),
            first,
            if latest == first { "" } else { &latest },
            reply,
            files
        ));
    }
    out
}

/// Deterministic zero-network fallback organizer used when test overrides are active or LLM is unavailable.
fn build_heuristic_decisions(
    mgr: &Arc<AgentOrchestrator>,
    project_root: &str,
    targets: &[(AgentSummary, AgentContextSnippet)],
    auto_apply: bool,
) -> Vec<LlmOrganizeDecision> {
    let mut decisions: Vec<LlmOrganizeDecision> = Vec::with_capacity(targets.len());
    let mut unmatched_indices: Vec<usize> = Vec::new();
    let mut token_sets: Vec<HashSet<String>> = Vec::with_capacity(targets.len());

    for (idx, (ag, snip)) in targets.iter().enumerate() {
        let allow_title = is_auto_nameable_title(ag, snip.first_user_prompt.as_deref());
        let (handle, title, summary) = derive_heuristic_identity(ag, snip, allow_title);
        let combined = format!(
            "{} {} {} {}",
            ag.title,
            compact_snippet(snip.first_user_prompt.as_deref(), 240),
            compact_snippet(snip.latest_user_prompt.as_deref(), 160),
            snip.edited_files.join(" ")
        );
        token_sets.push(extract_semantic_tokens(&combined));

        // 1. Check against existing curated workspaces via Stage-1/Stage-2 lexical + file overlap
        let existing_match = evaluate_auto_cluster(
            mgr.store(),
            project_root,
            &ag.cwd,
            None,
            &combined,
            &[],
            Some(&ag.workspace_id),
            true,
        );
        if let Some(sug) = existing_match {
            decisions.push(LlmOrganizeDecision {
                agent_id: ag.agent_id.clone(),
                handle,
                title,
                task_summary: Some(summary),
                action: "join".into(),
                target_workspace_id: Some(sug.target_workspace_id),
                new_workspace_key: None,
                new_workspace_title: None,
                new_workspace_goal: None,
                confidence: sug.confidence,
                reason: Some(sug.reason),
            });
        } else {
            unmatched_indices.push(idx);
            decisions.push(LlmOrganizeDecision {
                agent_id: ag.agent_id.clone(),
                handle,
                title,
                task_summary: Some(summary),
                action: "unclassified".into(),
                target_workspace_id: None,
                new_workspace_key: None,
                new_workspace_title: None,
                new_workspace_goal: None,
                confidence: 0.0,
                reason: None,
            });
        }
    }

    // 2. Pairwise cluster unmatched sessions that share strong semantic tokens or edited files
    let mut visited: HashSet<usize> = HashSet::new();
    let mut group_counter = 0u32;
    for &i in &unmatched_indices {
        if !visited.insert(i) {
            continue;
        }
        let mut cluster = vec![i];
        let files_i: HashSet<&str> = targets[i]
            .1
            .edited_files
            .iter()
            .map(String::as_str)
            .collect();
        for &j in &unmatched_indices {
            if visited.contains(&j) {
                continue;
            }
            let shared_tokens = token_sets[i].intersection(&token_sets[j]).count();
            let shared_files = targets[j]
                .1
                .edited_files
                .iter()
                .filter(|f| files_i.contains(f.as_str()))
                .count();
            let min_len = token_sets[i].len().min(token_sets[j].len()).max(1);
            let overlap = shared_tokens as f32 / min_len as f32;
            if shared_files >= 1 || (shared_tokens >= 2 && overlap >= 0.25) {
                visited.insert(j);
                cluster.push(j);
            }
        }
        if cluster.len() >= 2 {
            group_counter += 1;
            let key = format!("topic-{group_counter}");
            let rep = &targets[cluster[0]].0;
            let ws_title = truncate_chars(&rep.title, 36);
            for &member_idx in &cluster {
                decisions[member_idx].action = "create".into();
                decisions[member_idx].new_workspace_key = Some(key.clone());
                decisions[member_idx].new_workspace_title = Some(ws_title.clone());
                decisions[member_idx].new_workspace_goal =
                    decisions[member_idx].task_summary.clone();
                decisions[member_idx].confidence = 0.85;
                decisions[member_idx].reason =
                    Some("Shared topic & codebase context across unclassified sessions".into());
            }
        } else if auto_apply && targets.len() == 1 && token_sets[i].len() >= 2 {
            // Single-conversation manual classify: promote substantial session into its own curated workspace
            let rep = &targets[i].0;
            decisions[i].action = "create".into();
            decisions[i].new_workspace_key = Some("solo-1".into());
            decisions[i].new_workspace_title = Some(truncate_chars(&rep.title, 36));
            decisions[i].new_workspace_goal = decisions[i].task_summary.clone();
            decisions[i].confidence = 0.82;
            decisions[i].reason =
                Some("Created dedicated Mission Workspace from session context".into());
        }
    }

    decisions
}

/// Organize unclassified conversations in `project_root` into curated Mission Workspaces and
/// refresh every processed conversation's semantic handle + 1-line `task_summary`.
pub async fn organize_project_workspaces(
    mgr: &Arc<AgentOrchestrator>,
    request_id: Option<Uuid>,
    project_root: &str,
    target_agent_ids: Vec<String>,
    auto_apply: bool,
) -> anyhow::Result<WorkspaceOrganizeEvent> {
    let norm_root = normalize_root_path(project_root);
    let lock_key = format!("org:{norm_root}");
    let mut lock_guard = mgr.try_acquire_organizer_lock(&lock_key);
    if lock_guard.is_none() && request_id.is_some() {
        // Explicit user-triggered AI organize waits briefly for any in-flight background Turn-1 triage
        // instead of silently dropping the user's click.
        for _ in 0..300 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if let Some(g) = mgr.try_acquire_organizer_lock(&lock_key) {
                lock_guard = Some(g);
                break;
            }
        }
    }
    let Some(_guard) = lock_guard else {
        return Ok(WorkspaceOrganizeEvent {
            request_id,
            project_root: project_root.to_string(),
            phase: WorkspaceOrganizePhase::Running,
            message: "Workspace organization is already running…".into(),
            created_workspaces: 0,
            grouped_agents: 0,
            summarized_agents: 0,
        });
    };

    let emit = |event: WorkspaceOrganizeEvent| {
        let _ = mgr
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceOrganizeProgress(event.clone()));
        event
    };

    emit(WorkspaceOrganizeEvent {
        request_id,
        project_root: project_root.to_string(),
        phase: WorkspaceOrganizePhase::Running,
        message: "Analyzing conversation context and planning workspaces…".into(),
        created_workspaces: 0,
        grouped_agents: 0,
        summarized_agents: 0,
    });

    let res = organize_project_workspaces_inner(
        mgr,
        request_id,
        project_root,
        &norm_root,
        target_agent_ids,
        auto_apply,
    )
    .await;
    drop(_guard);

    match res {
        Ok(done_ev) => Ok(emit(done_ev)),
        Err(err) => {
            let failed = emit(WorkspaceOrganizeEvent {
                request_id,
                project_root: project_root.to_string(),
                phase: WorkspaceOrganizePhase::Failed,
                message: format!("Workspace organization failed: {err}"),
                created_workspaces: 0,
                grouped_agents: 0,
                summarized_agents: 0,
            });
            Ok(failed)
        }
    }
}

async fn organize_project_workspaces_inner(
    mgr: &Arc<AgentOrchestrator>,
    request_id: Option<Uuid>,
    project_root: &str,
    norm_root: &str,
    target_agent_ids: Vec<String>,
    auto_apply: bool,
) -> anyhow::Result<WorkspaceOrganizeEvent> {
    let all_workspaces = mgr.store().list_workspaces_filtered(false, false)?;
    let ws_by_id: HashMap<String, WorkspaceSummary> = all_workspaces
        .iter()
        .map(|w| (w.workspace_id.clone(), w.clone()))
        .collect();
    let all_agents = mgr.store().list_agents_filtered(None, false, false, None)?;

    let mut curated_workspaces: Vec<(WorkspaceSummary, Vec<AgentSummary>)> = Vec::new();
    for ws in &all_workspaces {
        if ws.is_curated && workspace_belongs_to_project(ws, norm_root) {
            let peers: Vec<AgentSummary> = all_agents
                .iter()
                .filter(|a| a.workspace_id == ws.workspace_id)
                .cloned()
                .collect();
            curated_workspaces.push((ws.clone(), peers));
        }
    }

    let target_filter: HashSet<String> = target_agent_ids.iter().cloned().collect();
    let mut target_agents: Vec<AgentSummary> = all_agents
        .into_iter()
        .filter(|ag| {
            if !target_filter.is_empty() {
                return target_filter.contains(&ag.agent_id);
            }
            let ws_curated = ws_by_id.get(&ag.workspace_id).is_some_and(|w| w.is_curated);
            if ws_curated {
                return false;
            }
            let ag_norm = normalize_root_path(&ag.cwd);
            ws_by_id
                .get(&ag.workspace_id)
                .is_some_and(|w| workspace_belongs_to_project(w, norm_root))
                || ag_norm == norm_root
                || ag_norm.starts_with(&format!("{norm_root}/"))
        })
        .collect();
    target_agents.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
    target_agents.truncate(MAX_AGENTS_PER_ORGANIZE_BATCH);

    if target_agents.is_empty() {
        return Ok(WorkspaceOrganizeEvent {
            request_id,
            project_root: project_root.to_string(),
            phase: WorkspaceOrganizePhase::Completed,
            message: "No unfiled conversations to organize".into(),
            created_workspaces: 0,
            grouped_agents: 0,
            summarized_agents: 0,
        });
    }

    let targets_with_snippets: Vec<(AgentSummary, AgentContextSnippet)> = target_agents
        .iter()
        .map(|ag| {
            let snip = mgr
                .store()
                .agent_context_snippet(&ag.agent_id)
                .unwrap_or_default();
            (ag.clone(), snip)
        })
        .collect();

    let preferred_provider = targets_with_snippets.first().map(|(a, _)| a.provider);
    let mut decisions: Option<Vec<LlmOrganizeDecision>> = None;

    let providers = pick_organizer_providers(preferred_provider);
    if !providers.is_empty() {
        let prompt = build_organize_prompt(
            project_root,
            &curated_workspaces,
            &targets_with_snippets,
            auto_apply && target_agent_ids.len() == 1,
        );
        for provider in providers.into_iter().take(2) {
            match run_ephemeral_text_prompt(
                provider,
                PathBuf::from(project_root),
                None,
                prompt.clone(),
                LLM_ORGANIZE_TIMEOUT_SECS,
            )
            .await
            {
                Ok(raw) => {
                    if let Some(json_str) = extract_json_object(&raw)
                        && let Ok(parsed) = serde_json::from_str::<LlmOrganizePayload>(json_str)
                        && !parsed.decisions.is_empty()
                    {
                        info!(
                            project_root = %project_root,
                            provider = ?provider,
                            decisions = parsed.decisions.len(),
                            "AI Workspace Organizer produced structured plan"
                        );
                        decisions = Some(parsed.decisions);
                        break;
                    }
                    warn!(
                        project_root = %project_root,
                        provider = ?provider,
                        "AI Workspace Organizer output was not valid JSON; trying fallback"
                    );
                }
                Err(err) => {
                    warn!(
                        project_root = %project_root,
                        provider = ?provider,
                        error = %err,
                        "AI Workspace Organizer ephemeral call failed; trying fallback"
                    );
                }
            }
        }
    }

    let final_decisions = decisions.unwrap_or_else(|| {
        build_heuristic_decisions(mgr, project_root, &targets_with_snippets, auto_apply)
    });

    let curated_ids: HashSet<String> = curated_workspaces
        .iter()
        .map(|(w, _)| w.workspace_id.clone())
        .collect();
    let target_map: HashMap<String, (AgentSummary, AgentContextSnippet)> = targets_with_snippets
        .into_iter()
        .map(|(a, s)| (a.agent_id.clone(), (a, s)))
        .collect();

    let mut created_by_key: HashMap<String, WorkspaceSummary> = HashMap::new();
    let mut created_workspaces: u32 = 0;
    let mut grouped_agents: u32 = 0;
    let mut summarized_agents: u32 = 0;
    let mut seen_agents: HashSet<String> = HashSet::new();

    for dec in final_decisions {
        let Some((agent, snippet)) = target_map.get(&dec.agent_id) else {
            continue;
        };
        if !seen_agents.insert(dec.agent_id.clone()) {
            continue;
        }

        let allow_title = is_auto_nameable_title(agent, snippet.first_user_prompt.as_deref());
        let (fallback_handle, fallback_title, fallback_summary) =
            derive_heuristic_identity(agent, snippet, allow_title);
        let sem_handle = clean_semantic_handle(dec.handle.as_deref()).or(fallback_handle);
        let sem_title = if allow_title {
            clean_concise_title(dec.title.as_deref()).or(fallback_title)
        } else {
            None
        };
        let sem_summary =
            clean_summary_line(dec.task_summary.as_deref()).unwrap_or(fallback_summary);

        let action = dec.action.trim().to_ascii_lowercase();
        let conf = dec.confidence.clamp(0.0, 1.0);
        let should_apply_now = auto_apply || conf >= AUTO_CLUSTER_HIGH_THRESHOLD;

        let mut target_ws_opt: Option<WorkspaceSummary> = None;
        if action == "join"
            && let Some(ref ws_id) = dec.target_workspace_id
            && curated_ids.contains(ws_id)
            && let Ok(Some(ws)) = mgr.store().get_workspace(ws_id)
        {
            target_ws_opt = Some(ws);
        } else if action == "create" && should_apply_now {
            let key = dec
                .new_workspace_key
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(agent.agent_id.as_str())
                .to_string();
            if let Some(existing_created) = created_by_key.get(&key) {
                target_ws_opt = Some(existing_created.clone());
            } else {
                let ws_title = dec
                    .new_workspace_title
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| truncate_chars(s, 48))
                    .unwrap_or_else(|| truncate_chars(&agent.title, 36));
                let ws_goal = dec
                    .new_workspace_goal
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| truncate_chars(s, 140));
                if let Ok(new_ws) = mgr
                    .create_workspace_v2(CreateWorkspaceSpec {
                        source_cwd: project_root.to_string(),
                        title: Some(ws_title),
                        worktree_branch: None,
                        base_ref: None,
                        isolation: WorkspaceIsolation::Local,
                        worktree_mode: WorktreeCreateMode::BranchOff,
                        pr_number: None,
                        parent_workspace_id: None,
                        background: false,
                        project_root: Some(project_root.to_string()),
                        mission_goal: ws_goal,
                        allow_reuse_existing: false,
                        is_curated: true,
                    })
                    .await
                {
                    let _ = mgr.store().update_workspace_goal(
                        &new_ws.workspace_id,
                        None,
                        None,
                        None,
                        Some(true),
                    );
                    let fresh = mgr
                        .store()
                        .get_workspace(&new_ws.workspace_id)
                        .ok()
                        .flatten()
                        .unwrap_or(new_ws);
                    let _ = mgr
                        .broadcast_tx
                        .send(DaemonBroadcast::WorkspaceUpdated(fresh.clone()));
                    created_workspaces += 1;
                    created_by_key.insert(key, fresh.clone());
                    target_ws_opt = Some(fresh);
                }
            }
        }

        let prev_ws_id = agent.workspace_id.clone();
        let mut final_ws_id = prev_ws_id.clone();

        if let Some(target_ws) = target_ws_opt {
            if should_apply_now && target_ws.workspace_id != prev_ws_id {
                let _ = mgr.store().move_agent_to_workspace(
                    &agent.agent_id,
                    &target_ws.workspace_id,
                    false,
                );
                let _ = mgr.store().update_workspace_goal(
                    &target_ws.workspace_id,
                    None,
                    None,
                    None,
                    Some(true),
                );
                let sug = WorkspaceClusterSuggestion {
                    target_workspace_id: target_ws.workspace_id.clone(),
                    target_workspace_title: target_ws.title.clone(),
                    confidence: (conf.max(0.75) * 100.0).round() / 100.0,
                    reason: dec
                        .reason
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| "Automatically grouped into this workspace".into()),
                    auto_applied: true,
                    previous_workspace_id: Some(prev_ws_id.clone()),
                };
                let _ = mgr
                    .store()
                    .set_agent_cluster_suggestion(&agent.agent_id, Some(&sug));
                mgr.invalidate_roster_fingerprint(&agent.agent_id).await;
                final_ws_id = target_ws.workspace_id.clone();
                grouped_agents += 1;

                if let Ok(ev) = mgr.store().append_workspace_event(
                    &target_ws.workspace_id,
                    Some(&agent.agent_id),
                    sem_handle.as_deref().or(agent.handle.as_deref()),
                    WorkspaceEventKind::PeerJoined,
                    &format!(
                        "@{} joined workspace `{}` (AI Triage)",
                        sem_handle
                            .as_deref()
                            .or(agent.handle.as_deref())
                            .unwrap_or("peer"),
                        target_ws.title
                    ),
                    None,
                ) {
                    let _ = mgr
                        .broadcast_tx
                        .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
                }
                if let Ok(Some(refreshed_ws)) = mgr.store().get_workspace(&target_ws.workspace_id) {
                    let _ = mgr
                        .broadcast_tx
                        .send(DaemonBroadcast::WorkspaceUpdated(refreshed_ws));
                }
            } else if !should_apply_now && conf >= 0.45 && target_ws.workspace_id != prev_ws_id {
                let sug = WorkspaceClusterSuggestion {
                    target_workspace_id: target_ws.workspace_id.clone(),
                    target_workspace_title: target_ws.title.clone(),
                    confidence: (conf * 100.0).round() / 100.0,
                    reason: dec
                        .reason
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| "Suggested for this workspace".into()),
                    auto_applied: false,
                    previous_workspace_id: None,
                };
                let _ = mgr
                    .store()
                    .set_agent_cluster_suggestion(&agent.agent_id, Some(&sug));
            }
        }

        // Apply semantic handle + task_summary (respecting user locks)
        let dest_peers: Vec<AgentSummary> = mgr
            .store()
            .list_agents_filtered(Some(&final_ws_id), false, false, None)
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.agent_id != agent.agent_id)
            .collect();
        let unique_handle = sem_handle
            .map(|h| allocate_unique_handle(Some(&h), agent.provider, None, None, &dest_peers));
        if let Ok(Some(updated_ag)) = mgr.store().set_agent_semantic_identity(
            &agent.agent_id,
            unique_handle.as_deref(),
            sem_title.as_deref(),
            Some(sem_summary.as_str()),
        ) {
            summarized_agents += 1;
            let _ = mgr
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated_ag));
        }
    }

    let msg = if grouped_agents > 0 || created_workspaces > 0 {
        let ws_word = if created_workspaces == 1 {
            "workspace"
        } else {
            "workspaces"
        };
        let conv_word = if grouped_agents == 1 {
            "conversation"
        } else {
            "conversations"
        };
        let sum_word = if summarized_agents == 1 {
            "summary"
        } else {
            "summaries"
        };
        format!(
            "Done: created {created_workspaces} {ws_word}, grouped {grouped_agents} {conv_word}, updated {summarized_agents} {sum_word}"
        )
    } else if summarized_agents > 0 {
        let sum_word = if summarized_agents == 1 {
            "summary"
        } else {
            "summaries"
        };
        format!("Updated {summarized_agents} conversation {sum_word} — no new groups needed")
    } else {
        "Workspace organization complete".into()
    };

    Ok(WorkspaceOrganizeEvent {
        request_id,
        project_root: project_root.to_string(),
        phase: WorkspaceOrganizePhase::Completed,
        message: msg,
        created_workspaces,
        grouped_agents,
        summarized_agents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_fenced_and_prose_responses() {
        let raw = "Sure! Here is the JSON:\n```json\n{\"handle\": \"跨端同步\", \"task_summary\": \"修复多端已读状态\"}\n```\nHope this helps.";
        let extracted = extract_json_object(raw).expect("json");
        let parsed: LlmSummaryPayload = serde_json::from_str(extracted).expect("parse");
        assert_eq!(parsed.handle.as_deref(), Some("跨端同步"));
        assert_eq!(parsed.task_summary.as_deref(), Some("修复多端已读状态"));
    }

    #[test]
    fn rejects_generic_harness_handles_in_cleaner() {
        assert!(clean_semantic_handle(Some("agy")).is_none());
        assert!(clean_semantic_handle(Some("@claude-2")).is_none());
        assert!(clean_semantic_handle(Some("peer")).is_none());
        assert_eq!(
            clean_semantic_handle(Some("@跨端同步")).as_deref(),
            Some("跨端同步")
        );
    }

    #[test]
    fn fallback_title_extractor_cleans_markdown_mentions_fences_and_newlines() {
        let prompt = "/plan # Fix `auth_middleware.rs` login bug @crates/orbit-daemon/src/lib.rs\n\n```rust\nfn panic_here() {}\n```\nMore details follow.";
        let title = extract_fallback_title(prompt);
        assert!(title.chars().count() <= MAX_CONCISE_TITLE_CHARS);
        assert!(!title.contains("```"));
        assert!(!title.contains('\n'));
        assert!(!title.starts_with("/plan"));
        assert!(!title.starts_with('#'));
        assert!(!title.contains("@crates/"));
        assert!(title.contains("Fix auth_middleware.rs login bug"));

        let cjk = "<orbit-workspace><peers/></orbit-workspace>\n\n- **修复** [导出模块](https://example.com) 的暗色模式样式问题。顺便加上单元测试。";
        let cjk_title = extract_fallback_title(cjk);
        assert!(cjk_title.chars().count() <= MAX_CONCISE_TITLE_CHARS);
        assert_eq!(cjk_title, "修复 导出模块 的暗色模式样式问题");

        let only_mention = "@src/components/export-modal.tsx";
        assert_eq!(extract_fallback_title(only_mention), "export-modal.tsx");

        assert_eq!(
            extract_fallback_title("/goal Finish all unit tests cleanly"),
            "Finish all unit tests cleanly"
        );
        assert_eq!(
            extract_fallback_title("/fork Refactor sidebar spacing"),
            "Refactor sidebar spacing"
        );
        assert_eq!(
            extract_fallback_title("/goalkeeper stats summary"),
            "/goalkeeper stats summary"
        );
    }

    async fn make_test_orchestrator() -> (Arc<AgentOrchestrator>, Arc<orbit_store::OrbitStore>) {
        let store = Arc::new(orbit_store::OrbitStore::open_in_memory().expect("store"));
        let dir = std::env::temp_dir().join(format!("orbit-autoname-{}", Uuid::new_v4().simple()));
        let _ = std::fs::create_dir_all(&dir);
        let orch_cfg = Arc::new(tokio::sync::RwLock::new(
            orbit_protocol::OrchestrationConfig::default(),
        ));
        let listen_port = Arc::new(tokio::sync::RwLock::new(0u16));
        let scripts = crate::workspace_runtime::WorkspaceScriptRegistry::new();
        let keep_awake = Arc::new(crate::keep_awake::KeepAwakeManager::new(false));
        let hooks = crate::hooks::HooksEngine::new(dir.clone());
        let mgr = AgentOrchestrator::new(
            dir,
            "host_test".into(),
            "tok".into(),
            listen_port,
            orch_cfg,
            Arc::clone(&store),
            scripts,
            keep_awake,
            Arc::new(tokio::sync::RwLock::new(None)),
            hooks,
        )
        .await;
        (mgr, store)
    }

    fn seed_agent(
        store: &orbit_store::OrbitStore,
        agent_id: &str,
        title: &str,
        labels: HashMap<String, String>,
    ) {
        let now = chrono::Utc::now();
        let ws = WorkspaceSummary {
            workspace_id: "ws_test".into(),
            title: "Test WS".into(),
            cwd: "/tmp/repo".into(),
            git_branch: None,
            is_worktree: false,
            updated_at: now,
            source_cwd: None,
            base_ref: None,
            parent_workspace_id: None,
            archived_at: None,
            background: false,
            setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
            mission_goal: None,
            project_root: Some("/tmp/repo".into()),
            board_revision: 0,
            title_locked: false,
            auto_clustered: false,
            is_curated: false,
        };
        store.upsert_workspace(&ws).expect("upsert ws");
        let agent = AgentSummary {
            agent_id: agent_id.into(),
            workspace_id: "ws_test".into(),
            cwd: "/tmp/repo".into(),
            title: title.into(),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            fast_mode: false,
            mode: orbit_protocol::SessionMode::Default,
            status: orbit_protocol::AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels,
            attention: orbit_protocol::AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: Some("claude".into()),
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };
        store.upsert_agent(&agent, None).expect("upsert agent");
    }

    #[tokio::test]
    async fn auto_naming_runs_once_after_first_completed_turn_and_never_clobbers_locked_titles() {
        let (mgr, store) = make_test_orchestrator().await;

        // 1. Agent with raw markdown prompt default: should skip before assistant reply, then auto-name once.
        let prompt = "/plan # Refactor `export.rs` HTML escaping\n\n```rs\nfn x(){}\n```";
        seed_agent(&store, "ag_auto_1", "New thread", HashMap::new());
        store
            .append_timeline_event(
                "ag_auto_1",
                1,
                Some("t1"),
                orbit_protocol::TimelinePayload::UserPrompt {
                    message_id: "m1".into(),
                    text: prompt.into(),
                    badges: vec![],
                },
            )
            .expect("append user");

        // No assistant reply yet -> must NOT trigger or mark orbit.auto_named
        let before_reply = auto_name_agent_after_first_turn(&mgr, "ag_auto_1")
            .await
            .expect("auto name before reply");
        assert!(before_reply.is_none());
        let ag_check = store.get_agent("ag_auto_1").expect("get").expect("exists");
        assert!(!is_auto_named(&ag_check.labels));

        // Assistant reply arrives -> first turn complete
        store
            .append_timeline_event(
                "ag_auto_1",
                1,
                Some("t1"),
                orbit_protocol::TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "Done refactoring HTML escaping.".into(),
                    streaming: false,
                },
            )
            .expect("append assistant");

        let first_run = auto_name_agent_after_first_turn(&mgr, "ag_auto_1")
            .await
            .expect("auto name first run")
            .expect("updated agent");
        assert_eq!(first_run.title, "Refactor export.rs HTML escaping");
        assert!(first_run.task_summary.is_some());
        assert_eq!(
            first_run.labels.get(LABEL_AUTO_NAMED).map(String::as_str),
            Some("1")
        );

        // Second invocation must be a no-op because `orbit.auto_named = "1"` is persisted.
        let second_run = auto_name_agent_after_first_turn(&mgr, "ag_auto_1")
            .await
            .expect("auto name second run");
        assert!(second_run.is_none());

        // 2. Agent with `orbit.title_locked = "1"` must NEVER have its title overwritten.
        let mut locked_labels = HashMap::new();
        locked_labels.insert(LABEL_TITLE_LOCKED.into(), "1".into());
        seed_agent(&store, "ag_locked", "My Custom Title", locked_labels);
        store
            .append_timeline_event(
                "ag_locked",
                1,
                Some("t1"),
                orbit_protocol::TimelinePayload::UserPrompt {
                    message_id: "m2".into(),
                    text: "Investigate websocket timeout in relay".into(),
                    badges: vec![],
                },
            )
            .expect("append user");
        store
            .append_timeline_event(
                "ag_locked",
                1,
                Some("t1"),
                orbit_protocol::TimelinePayload::AssistantMessage {
                    item_id: "a2".into(),
                    text: "Found missing keepalive frame.".into(),
                    streaming: false,
                },
            )
            .expect("append assistant");

        let locked_auto = auto_name_agent_after_first_turn(&mgr, "ag_locked")
            .await
            .expect("auto name locked")
            .expect("summary persisted");
        assert_eq!(locked_auto.title, "My Custom Title");
        assert!(locked_auto.task_summary.is_some());

        // Manual summarize must also respect `orbit.title_locked = "1"`.
        let locked_manual = summarize_agent_context(&mgr, "ag_locked")
            .await
            .expect("manual summarize locked");
        assert_eq!(locked_manual.title, "My Custom Title");

        // 3. Explicit user rename via `update_agent` locks title automatically.
        seed_agent(&store, "ag_renamed", "New thread", HashMap::new());
        let renamed = mgr
            .update_agent(
                "ag_renamed",
                Some("Pinned Release Plan".into()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("rename agent");
        assert!(is_title_locked(&renamed.labels));
        assert_eq!(renamed.title, "Pinned Release Plan");

        // 4. Raw truncated prompt default IS eligible for auto-naming, whereas an unrelated custom title is NOT.
        let long_prompt = "### Implement end-to-end encrypted transcript export with strict XSS protection across desktop and CLI";
        let truncated_default = raw_default_prompt_title(long_prompt);
        seed_agent(&store, "ag_trunc", &truncated_default, HashMap::new());
        let ag_trunc = store.get_agent("ag_trunc").expect("get").expect("exists");
        assert!(is_auto_nameable_title(&ag_trunc, Some(long_prompt)));

        seed_agent(
            &store,
            "ag_explicit_unlocked",
            "Q4 Security Audit",
            HashMap::new(),
        );
        let ag_explicit = store
            .get_agent("ag_explicit_unlocked")
            .expect("get")
            .expect("exists");
        assert!(!is_auto_nameable_title(&ag_explicit, Some(long_prompt)));

        // 5. Atomic semantic identity & update_agent_fields never overwrite concurrent lifecycle fields
        //    (`status`, `is_pinned`, `attention`, `archived_at`).
        seed_agent(&store, "ag_atomic", "New thread", HashMap::new());
        store
            .set_agent_pinned("ag_atomic", true, Some(42))
            .expect("pin");
        store
            .set_agent_attention("ag_atomic", orbit_protocol::AttentionState::Finished)
            .expect("attention");
        store
            .set_agent_semantic_identity_with_options(
                "ag_atomic",
                Some("atomic-fix"),
                Some("Atomic Title"),
                Some("Atomic summary"),
                SemanticIdentityOptions {
                    mark_auto_named: true,
                    force_title: false,
                },
            )
            .expect("atomic semantic update");
        let after_sem = store.get_agent("ag_atomic").expect("get").expect("exists");
        assert_eq!(after_sem.title, "Atomic Title");
        assert!(after_sem.is_pinned);
        assert_eq!(after_sem.pinned_order, 42);
        assert_eq!(
            after_sem.attention,
            orbit_protocol::AttentionState::Finished
        );

        mgr.update_agent(
            "ag_atomic",
            Some("User Locked Title".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("update_agent");
        let after_upd = store.get_agent("ag_atomic").expect("get").expect("exists");
        assert_eq!(after_upd.title, "User Locked Title");
        assert!(is_title_locked(&after_upd.labels));
        assert!(after_upd.is_pinned);
        assert_eq!(
            after_upd.attention,
            orbit_protocol::AttentionState::Finished
        );

        // 6. RAII OrganizerLockGuard releases lock and clears keep-awake on drop
        {
            let guard = mgr.try_acquire_organizer_lock("sum:test_raii");
            assert!(guard.is_some());
            assert!(mgr.try_acquire_organizer_lock("sum:test_raii").is_none());
        }
        assert!(mgr.try_acquire_organizer_lock("sum:test_raii").is_some());

        // 7. strip_orbit_workspace_xml strips leading `<orbit-workspace>` and `<WORKSPACE_RULES>` wrappers
        let prose_mention = "Please inspect </orbit-workspace> parser edge case";
        assert_eq!(strip_orbit_workspace_xml(prose_mention), prose_mention);
        let wrapped = "<orbit-workspace id=\"ws_1\">meta</orbit-workspace>\n\n<WORKSPACE_RULES>\n### Rule: rust\nNo unwrap\n</WORKSPACE_RULES>\n\nFix login bug";
        assert_eq!(strip_orbit_workspace_xml(wrapped), "Fix login bug");
        let delta_wrapped = "<orbit-workspace-delta id=\"ws_1\" board-revision=\"2\"><board-updated rev=\"2\" by=\"@proto\">spec</board-updated></orbit-workspace-delta>\n\nImplement rate limiter";
        assert_eq!(
            strip_orbit_workspace_xml(delta_wrapped),
            "Implement rate limiter"
        );
        let rules_first =
            "<WORKSPACE_RULES>\n### Rule: a\nBody\n</WORKSPACE_RULES>\n\nRefactor auth";
        assert_eq!(strip_orbit_workspace_xml(rules_first), "Refactor auth");
    }
}
