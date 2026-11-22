//! Automatic Mission Workspace handle allocation and two-stage Auto-Clustering engine.
//!
//! Stage 1 (Deterministic high-signal rules, 0ms):
//!   - Active feature-branch match inside the same project root
//!   - Explicit `@handle` or workspace title reference in the prompt
//!   - Overlap between mentioned/attached file paths and recently edited files in a workspace
//!
//! Stage 2 (Calibrated lexical + code-identifier similarity with recency decay, 0ms):
//!   - Computes token similarity (English identifiers + CJK bigrams) against active Mission
//!     Workspaces in the same project root updated within the last 7 days.
//!   - High confidence (>= 0.78): auto-applies grouping with reversible `previous_workspace_id`
//!   - Medium confidence (0.42..0.78): leaves standalone and attaches a 1-click suggestion chip

use std::collections::HashSet;

use chrono::Utc;
use orbit_protocol::{
    AgentSummary, PromptBadge, ProviderKind, WorkspaceClusterSuggestion, WorkspaceSummary,
};
use orbit_store::OrbitStore;

pub const AUTO_CLUSTER_HIGH_THRESHOLD: f32 = 0.78;
pub const AUTO_CLUSTER_SUGGEST_THRESHOLD: f32 = 0.42;

const DEFAULT_BRANCHES: &[&str] = &["main", "master", "develop", "dev", "head", "trunk"];

const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "have", "will", "into", "your", "about",
    "help", "please", "make", "need", "want", "check", "look", "code", "file", "files", "change",
    "update", "add", "fix", "work", "can", "you", "how", "what", "why", "when", "where", "are",
    "not", "but", "all", "new", "use", "run", "test", "tests", "using", "should", "would", "could",
];

const GENERIC_HARNESS_HANDLES: &[&str] = &[
    "peer",
    "lead",
    "agy",
    "codex",
    "claude",
    "claude-code",
    "claude_code",
    "antigravity",
    "agent",
    "assistant",
    "chat",
    "session",
    "task",
    "default",
    "worker",
];

/// Returns `true` when `handle` is just a bare harness/placeholder name (e.g. `agy`, `claude`, `codex-2`, `peer`).
pub fn is_generic_harness_handle(handle: &str) -> bool {
    let norm = handle.trim().trim_start_matches('@').to_ascii_lowercase();
    if norm.is_empty() {
        return true;
    }
    if GENERIC_HARNESS_HANDLES.contains(&norm.as_str()) {
        return true;
    }
    if let Some((prefix, suffix)) = norm.rsplit_once('-')
        && suffix.chars().all(|c| c.is_ascii_digit())
        && GENERIC_HARNESS_HANDLES.contains(&prefix)
    {
        return true;
    }
    false
}

/// Sanitize a requested collaborative handle into a concise Unicode/CJK-friendly slug
/// (up to 18 characters; lowercases ASCII, preserves CJK/Unicode letters & digits, collapses whitespace to `-`).
pub fn sanitize_handle(raw: &str) -> String {
    let trimmed = raw.trim().trim_start_matches('@');
    let mut out = String::new();
    let mut char_count: usize = 0;
    let mut prev_sep = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            char_count += 1;
            prev_sep = false;
        } else if ch.is_alphanumeric() {
            out.push(ch);
            char_count += 1;
            prev_sep = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace() || ch == '/')
            && !out.is_empty()
            && !prev_sep
        {
            out.push('-');
            char_count += 1;
            prev_sep = true;
        }
        if char_count >= 18 {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

/// Derive a concise, human-meaningful semantic tab label directly from a conversation's prompt/title
/// so new sessions never start as bare harness names (`@agy`/`@claude`/`@codex`) even before LLM summary completes.
pub fn derive_semantic_handle_from_prompt(prompt: &str) -> Option<String> {
    let first_line = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('<') && !l.starts_with("```"))?;
    let stripped = first_line
        .trim_start_matches(['#', '-', '*', '>', '1', '2', '3', '4', '5', '.', ' '])
        .trim();
    if stripped.is_empty() {
        return None;
    }

    // If the prompt contains CJK, extract a concise clean phrase (up to 8 chars, stopping at punctuation).
    let has_cjk = stripped.chars().any(is_cjk);
    if has_cjk {
        let mut phrase = String::new();
        let mut count = 0usize;
        // Skip common Chinese filler prefixes
        let mut candidate = stripped;
        for prefix in [
            "请帮我",
            "帮我",
            "请你",
            "请",
            "帮忙",
            "我想让你",
            "我想",
            "需要你",
            "需要",
            "能不能",
            "可以",
            "麻烦",
            "说几个需求",
            "问一下",
            "看一下",
            "看看",
        ] {
            if let Some(rest) = candidate.strip_prefix(prefix) {
                let r = rest.trim_start_matches(['：', ':', '，', ',', '、', ' ']);
                if !r.is_empty() {
                    candidate = r;
                    break;
                }
            }
        }
        for ch in candidate.chars() {
            if matches!(
                ch,
                '，' | '。'
                    | '？'
                    | '！'
                    | '；'
                    | '：'
                    | ','
                    | '.'
                    | '?'
                    | '!'
                    | ';'
                    | ':'
                    | '('
                    | ')'
                    | '（'
                    | '）'
                    | '\n'
            ) {
                if count >= 2 {
                    break;
                }
                continue;
            }
            if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                phrase.push(ch);
                count += 1;
                if count >= 9 {
                    break;
                }
            }
        }
        let cleaned = sanitize_handle(&phrase);
        if cleaned.chars().count() >= 2 && !is_generic_harness_handle(&cleaned) {
            return Some(cleaned);
        }
    }

    // English / code-centric prompt: pick up to 2 highest-signal non-stopword tokens
    let mut picked: Vec<String> = Vec::new();
    for raw_tok in stripped.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-') {
        let lower = raw_tok.trim_matches(['_', '-']).to_ascii_lowercase();
        if lower.len() < 3 || STOP_WORDS.contains(&lower.as_str()) {
            continue;
        }
        if GENERIC_HARNESS_HANDLES.contains(&lower.as_str()) {
            continue;
        }
        if !picked.contains(&lower) {
            picked.push(lower);
        }
        if picked.len() >= 2 {
            break;
        }
    }
    if !picked.is_empty() {
        let joined = sanitize_handle(&picked.join("-"));
        if !joined.is_empty() && !is_generic_harness_handle(&joined) {
            return Some(joined);
        }
    }
    None
}

/// Allocate a short, human-readable handle that is unique among active peers in a workspace.
pub fn allocate_unique_handle(
    requested: Option<&str>,
    provider: ProviderKind,
    profile_name: Option<&str>,
    prompt_seed: Option<&str>,
    existing_peers: &[AgentSummary],
) -> String {
    let taken: HashSet<String> = existing_peers
        .iter()
        .filter(|a| a.archived_at.is_none())
        .filter_map(|a| a.handle.as_deref())
        .map(|h| h.to_lowercase())
        .collect();

    let base = requested
        .map(sanitize_handle)
        .filter(|s| !s.is_empty())
        .or_else(|| profile_name.map(sanitize_handle).filter(|s| !s.is_empty()))
        .or_else(|| prompt_seed.and_then(derive_semantic_handle_from_prompt))
        .unwrap_or_else(|| {
            if taken.is_empty() {
                "lead".to_string()
            } else {
                provider.short_label().to_ascii_lowercase()
            }
        });

    let base_lower = base.to_lowercase();
    if !taken.contains(&base_lower) {
        return base;
    }
    for n in 2..=99 {
        let candidate = format!("{base}-{n}");
        if !taken.contains(&candidate.to_lowercase()) {
            return candidate;
        }
    }
    format!("{base}-{}", &uuid::Uuid::new_v4().simple().to_string()[..4])
}

/// Normalize a path string for project-root matching (strip trailing slash).
pub fn normalize_root_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    }
}

/// Extract mentioned file paths from prompt badges and inline path-like tokens.
fn extract_mentioned_paths(prompt: &str, badges: &[PromptBadge]) -> HashSet<String> {
    let mut paths = HashSet::new();
    for b in badges {
        if let Some(ref p) = b.path {
            let norm = p.trim().trim_start_matches("./").to_ascii_lowercase();
            if !norm.is_empty() {
                paths.insert(norm);
            }
        }
    }
    for raw_tok in prompt.split_whitespace() {
        let tok = raw_tok
            .trim_matches(|c: char| {
                matches!(
                    c,
                    '`' | '\'' | '"' | ',' | ';' | ':' | '(' | ')' | '[' | ']' | '<' | '>'
                )
            })
            .trim_start_matches('@')
            .trim_start_matches("./");
        if tok.len() >= 4
            && tok.contains('.')
            && (tok.contains('/')
                || tok.ends_with(".rs")
                || tok.ends_with(".ts")
                || tok.ends_with(".tsx")
                || tok.ends_with(".py")
                || tok.ends_with(".swift")
                || tok.ends_with(".kt")
                || tok.ends_with(".ets")
                || tok.ends_with(".go")
                || tok.ends_with(".md")
                || tok.ends_with(".json"))
        {
            paths.insert(tok.to_ascii_lowercase());
        }
    }
    paths
}

/// Extract normalized English/code tokens + CJK bigrams for fast zero-dependency semantic matching.
pub(crate) fn extract_semantic_tokens(text: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut cur_ascii = String::new();
    let mut cjk_run: Vec<char> = Vec::new();

    let flush_ascii = |buf: &mut String, out: &mut HashSet<String>| {
        if buf.len() >= 3 {
            let lower = buf.to_ascii_lowercase();
            if !STOP_WORDS.contains(&lower.as_str()) {
                out.insert(lower.clone());
                // Also split snake_case / kebab-case sub-tokens for code identifier matching
                for sub in lower.split(['_', '-']) {
                    if sub.len() >= 3 && !STOP_WORDS.contains(&sub) {
                        out.insert(sub.to_string());
                    }
                }
            }
        }
        buf.clear();
    };

    let flush_cjk = |run: &mut Vec<char>, out: &mut HashSet<String>| {
        if run.len() >= 2 {
            for win in run.windows(2) {
                let mut bg = String::with_capacity(6);
                bg.push(win[0]);
                bg.push(win[1]);
                out.insert(bg);
            }
        }
        run.clear();
    };

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            flush_cjk(&mut cjk_run, &mut tokens);
            cur_ascii.push(ch);
        } else if is_cjk(ch) {
            flush_ascii(&mut cur_ascii, &mut tokens);
            cjk_run.push(ch);
        } else {
            flush_ascii(&mut cur_ascii, &mut tokens);
            flush_cjk(&mut cjk_run, &mut tokens);
        }
    }
    flush_ascii(&mut cur_ascii, &mut tokens);
    flush_cjk(&mut cjk_run, &mut tokens);
    tokens
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x3040..=0x30FF | 0xAC00..=0xD7AF
    )
}

fn is_feature_branch(branch: Option<&str>) -> Option<&str> {
    let b = branch.map(str::trim).filter(|s| !s.is_empty())?;
    let lower = b.to_ascii_lowercase();
    if DEFAULT_BRANCHES.contains(&lower.as_str()) {
        None
    } else {
        Some(b)
    }
}

/// Evaluate whether a newly created conversation should auto-cluster into (or suggest joining)
/// an existing active Mission Workspace in the same project.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_auto_cluster(
    store: &OrbitStore,
    project_root: &str,
    cwd: &str,
    git_branch: Option<&str>,
    prompt: &str,
    badges: &[PromptBadge],
    exclude_workspace_id: Option<&str>,
    auto_apply_enabled: bool,
) -> Option<WorkspaceClusterSuggestion> {
    let norm_root = normalize_root_path(project_root);
    let norm_cwd = normalize_root_path(cwd);
    let now = Utc::now();

    let candidates: Vec<WorkspaceSummary> = store
        .list_workspaces_filtered(false, false)
        .ok()?
        .into_iter()
        .filter(|ws| {
            if !ws.is_curated {
                return false;
            }
            if exclude_workspace_id == Some(ws.workspace_id.as_str()) {
                return false;
            }
            let age_days = (now - ws.updated_at).num_hours() as f32 / 24.0;
            if age_days > 7.0 {
                return false;
            }
            let ws_root = ws
                .project_root
                .as_deref()
                .or(ws.source_cwd.as_deref())
                .unwrap_or(ws.cwd.as_str());
            let norm_ws_root = normalize_root_path(ws_root);
            let norm_ws_cwd = normalize_root_path(&ws.cwd);
            norm_ws_root == norm_root
                || norm_ws_cwd == norm_cwd
                || norm_ws_root == norm_cwd
                || norm_ws_cwd == norm_root
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    let prompt_lower = prompt.to_ascii_lowercase();
    let mentioned_paths = extract_mentioned_paths(prompt, badges);
    let prompt_tokens = extract_semantic_tokens(prompt);
    let feature_branch = is_feature_branch(git_branch);

    let mut best: Option<(f32, String, WorkspaceSummary)> = None;

    for ws in candidates {
        let peers = store
            .list_agents_filtered(Some(&ws.workspace_id), false, false, None)
            .unwrap_or_default();
        if peers.is_empty() {
            continue;
        }

        // Stage 1A: Feature branch exact match
        if let Some(fb) = feature_branch
            && ws.git_branch.as_deref().is_some_and(|wb| wb == fb)
        {
            let score = 0.95_f32;
            let reason = format!("Same feature branch `{fb}`");
            if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
                best = Some((score, reason, ws));
            }
            continue;
        }

        // Stage 1B: Explicit workspace title or @peer mention in prompt
        let title_lower = ws.title.trim().to_ascii_lowercase();
        let explicit_title_hit = title_lower.len() >= 5 && prompt_lower.contains(&title_lower);
        let explicit_handle_hit = peers.iter().any(|p| {
            p.handle
                .as_deref()
                .filter(|h| h.len() >= 3 && *h != "lead")
                .is_some_and(|h| prompt_lower.contains(&format!("@{h}")))
        });
        if explicit_title_hit || explicit_handle_hit {
            let score = 0.92_f32;
            let reason = if explicit_handle_hit {
                "Mentions a peer @handle in this workspace".to_string()
            } else {
                format!("References workspace `{}`", ws.title)
            };
            if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
                best = Some((score, reason, ws));
            }
            continue;
        }

        // Stage 1C + Stage 2: File overlap + lexical/identifier similarity + recency decay
        let ws_edited_files: HashSet<String> = store
            .list_workspace_edited_files(&ws.workspace_id, 40)
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.trim_start_matches("./").to_ascii_lowercase())
            .collect();

        let file_overlap_count = mentioned_paths
            .iter()
            .filter(|mp| {
                ws_edited_files.iter().any(|ef| {
                    ef == *mp || ef.ends_with(&format!("/{mp}")) || mp.ends_with(&format!("/{ef}"))
                })
            })
            .count();

        let mut corpus = String::new();
        corpus.push_str(&ws.title);
        corpus.push(' ');
        if let Some(ref goal) = ws.mission_goal {
            corpus.push_str(goal);
            corpus.push(' ');
        }
        if let Ok(Some(board)) = store.get_workspace_board(&ws.workspace_id) {
            corpus.push_str(&board.content);
            corpus.push(' ');
        }
        for p in &peers {
            corpus.push_str(&p.title);
            corpus.push(' ');
        }
        for f in &ws_edited_files {
            corpus.push_str(f);
            corpus.push(' ');
        }

        let ws_tokens = extract_semantic_tokens(&corpus);
        let shared_tokens = prompt_tokens.intersection(&ws_tokens).count();
        let lexical_score = if prompt_tokens.is_empty() || ws_tokens.is_empty() {
            0.0_f32
        } else {
            // Overlap coefficient blended with cosine similarity (rewards focused short prompts
            // whose key technical terms align strongly with a larger workspace corpus).
            let denom_cos = ((prompt_tokens.len() * ws_tokens.len()) as f32).sqrt();
            let cosine = shared_tokens as f32 / denom_cos.max(1.0);
            let overlap =
                shared_tokens as f32 / prompt_tokens.len().min(ws_tokens.len()).max(1) as f32;
            (0.45 * cosine + 0.55 * overlap).clamp(0.0, 1.0)
        };

        let age_hours = (now - ws.updated_at).num_hours().max(0) as f32;
        let recency_weight = if age_hours <= 24.0 {
            1.0
        } else {
            (1.0 - ((age_hours - 24.0) / (6.0 * 24.0)) * 0.25).clamp(0.75, 1.0)
        };

        let mut score = lexical_score * recency_weight;
        let reason = if file_overlap_count >= 2 {
            score = score.max(0.86);
            format!("Touches {file_overlap_count} shared workspace files")
        } else if file_overlap_count == 1 {
            score = (score + 0.35).clamp(0.65, 0.90);
            "References an active file edited in this workspace".to_string()
        } else if shared_tokens >= 3 {
            format!("Shares {shared_tokens} topic & symbol matches")
        } else {
            "Related topic in this project".to_string()
        };

        if score >= AUTO_CLUSTER_SUGGEST_THRESHOLD
            && best.as_ref().is_none_or(|(s, _, _)| score > *s)
        {
            best = Some((score, reason, ws));
        }
    }

    let (confidence, reason, target_ws) = best?;
    let auto_applied = auto_apply_enabled && confidence >= AUTO_CLUSTER_HIGH_THRESHOLD;

    Some(WorkspaceClusterSuggestion {
        target_workspace_id: target_ws.workspace_id,
        target_workspace_title: target_ws.title,
        confidence: (confidence * 100.0).round() / 100.0,
        reason,
        auto_applied,
        previous_workspace_id: None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::Utc;
    use orbit_protocol::{
        AgentStatus, AgentSummary, AttentionState, ProviderKind, SessionMode, WorkspaceEventKind,
        WorkspaceSetupStatus, WorkspaceSummary,
    };

    use super::*;

    fn mk_ws(id: &str, title: &str, cwd: &str, branch: Option<&str>) -> WorkspaceSummary {
        WorkspaceSummary {
            workspace_id: id.into(),
            title: title.into(),
            cwd: cwd.into(),
            git_branch: branch.map(ToOwned::to_owned),
            is_worktree: false,
            updated_at: Utc::now(),
            source_cwd: None,
            base_ref: None,
            parent_workspace_id: None,
            archived_at: None,
            background: false,
            setup_status: WorkspaceSetupStatus::Idle,
            mission_goal: Some("Implement cross-device read_seq sync and sidebar reorder".into()),
            project_root: Some(cwd.into()),
            board_revision: 0,
            title_locked: false,
            auto_clustered: false,
            is_curated: true,
        }
    }

    fn mk_agent(id: &str, ws_id: &str, cwd: &str, handle: &str) -> AgentSummary {
        let now = Utc::now();
        AgentSummary {
            agent_id: id.into(),
            workspace_id: ws_id.into(),
            cwd: cwd.into(),
            title: "Cross-device read_seq sync".into(),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 10,
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
            read_seq: 10,
            is_pinned: false,
            pinned_order: 0,
            handle: Some(handle.into()),
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
        }
    }

    #[test]
    fn allocates_unique_sanitized_handles() {
        let a1 = mk_agent("ag_1", "ws_1", "/repo", "lead");
        assert_eq!(
            allocate_unique_handle(
                None,
                ProviderKind::Codex,
                None,
                None,
                std::slice::from_ref(&a1)
            ),
            "codex"
        );
        let a2 = mk_agent("ag_2", "ws_1", "/repo", "codex");
        assert_eq!(
            allocate_unique_handle(
                None,
                ProviderKind::Codex,
                None,
                None,
                &[a1.clone(), a2.clone()]
            ),
            "codex-2"
        );
        assert_eq!(
            allocate_unique_handle(
                Some("@Code Reviewer!"),
                ProviderKind::Antigravity,
                None,
                None,
                std::slice::from_ref(&a1)
            ),
            "code-reviewer"
        );
        // Semantic prompt derivation avoids bare harness names and supports CJK
        assert_eq!(
            allocate_unique_handle(
                None,
                ProviderKind::ClaudeCode,
                None,
                Some("请帮我修复跨端已读状态同步问题"),
                std::slice::from_ref(&a1)
            ),
            "修复跨端已读状态同"
        );
        assert!(is_generic_harness_handle("agy"));
        assert!(is_generic_harness_handle("codex-2"));
        assert!(!is_generic_harness_handle("sync-core"));
    }

    #[test]
    fn auto_clusters_on_shared_files_and_semantic_overlap() {
        let store = OrbitStore::open_in_memory().expect("open");
        store
            .upsert_workspace(&mk_ws(
                "ws_sync",
                "Multi-device read sync",
                "/repo",
                Some("main"),
            ))
            .expect("ws");
        store
            .upsert_agent(&mk_agent("ag_1", "ws_sync", "/repo", "backend"), None)
            .expect("ag");
        store
            .append_workspace_event(
                "ws_sync",
                Some("ag_1"),
                Some("backend"),
                WorkspaceEventKind::FileEdited,
                "Edited crates/orbit-client-core/src/app_core.rs",
                Some("crates/orbit-client-core/src/app_core.rs"),
            )
            .expect("ev1");
        store
            .append_workspace_event(
                "ws_sync",
                Some("ag_1"),
                Some("backend"),
                WorkspaceEventKind::FileEdited,
                "Edited crates/orbit-daemon/src/manager.rs",
                Some("crates/orbit-daemon/src/manager.rs"),
            )
            .expect("ev2");

        // High confidence: mentions both edited files
        let high = evaluate_auto_cluster(
            &store,
            "/repo",
            "/repo",
            Some("main"),
            "Check read_seq handling in app_core.rs and manager.rs",
            &[],
            None,
            true,
        )
        .expect("high cluster");
        assert_eq!(high.target_workspace_id, "ws_sync");
        assert!(high.auto_applied);
        assert!(high.confidence >= AUTO_CLUSTER_HIGH_THRESHOLD);

        // Unrelated prompt should NOT cluster
        let unrelated = evaluate_auto_cluster(
            &store,
            "/repo",
            "/repo",
            Some("main"),
            "Generate a dockerfile for nginx reverse proxy tls certificate renewal",
            &[],
            None,
            true,
        );
        assert!(unrelated.is_none());
    }
}
