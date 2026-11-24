//! Pure-Rust Path-Scoped Glob Rules engine.
//!
//! Discovers, parses, validates, persists, and matches rule documents from:
//! - `~/.orbit/rules/*.md` (`UserGlobal`)
//! - `<cwd>/.orbit/rules/*.md` (`ProjectOrbit`)
//! - `<cwd>/.claude/rules/*.md` (`ProjectClaude`)
//! - `<cwd>/.cursor/rules/*.mdc` (`ProjectCursor`)
//!
//! Zero external runtime dependencies; ReDoS-free iterative DP glob matching;
//! strict canonical path confinement & symlink rejection on all read/write/delete operations.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use orbit_protocol::{
    MatchedRuleSummary, PATH_RULES_HARD_CAP_TOKENS, PATH_RULES_WARN_TOKENS, PathScopedRule,
    PromptBadge, RuleScope, RuleTriggerMode,
};
use uuid::Uuid;

use crate::workspace_files::sha256_hex;

/// Maximum size of a single rule file (256 KiB).
pub const MAX_RULE_FILE_BYTES: usize = 256 * 1024;
/// Maximum number of rule files discovered per scope directory.
pub const MAX_RULES_PER_SCOPE: usize = 256;
/// Maximum subdirectory traversal depth inside a rules directory.
const MAX_SCAN_DEPTH: usize = 3;
/// Maximum length of a single glob pattern.
const MAX_GLOB_PATTERN_LEN: usize = 512;
/// Maximum number of expanded patterns produced by `{a,b}` brace expansion.
const MAX_BRACE_EXPANSIONS: usize = 64;
/// Maximum brace nesting depth.
const MAX_BRACE_DEPTH: usize = 4;

/// Discovered rules snapshot returned by discovery / upsert / delete operations.
#[derive(Debug, Clone)]
pub struct DiscoveredPathRules {
    pub rules: Vec<PathScopedRule>,
    pub total_always_tokens: u32,
    pub token_budget_warning: bool,
}

/// Result of matching path-scoped rules against active paths and manual `@rule:` mentions.
#[derive(Debug, Clone, Default)]
pub struct MatchedPathRulesResult {
    pub matched: Vec<MatchedRuleSummary>,
    pub injected_markdown: String,
    pub total_tokens: u32,
}

/// Parsed YAML frontmatter + body of a rule markdown/mdc document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRuleDocument {
    pub description: String,
    pub trigger_mode: RuleTriggerMode,
    pub globs: Vec<String>,
    pub enabled: bool,
    pub body: String,
}

/// Upsert payload for creating or updating a path-scoped rule file.
#[derive(Debug, Clone)]
pub struct UpsertRuleSpec {
    pub scope: RuleScope,
    pub name: String,
    pub description: String,
    pub trigger_mode: RuleTriggerMode,
    pub globs: Vec<String>,
    pub enabled: bool,
    pub body: String,
    pub expected_etag: Option<String>,
}

/// Validated rule relative path inside a scope's rules directory (supports optional nested subdirs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRuleRelPath {
    pub subdirs: Vec<String>,
    pub filename: String,
}

/// Deterministic token approximation (~4 ASCII chars/token; CJK/non-ASCII weighted higher).
pub fn estimate_tokens(text: &str) -> u32 {
    if text.trim().is_empty() {
        return 0;
    }
    let mut ascii_count = 0usize;
    let mut non_ascii_count = 0usize;
    for ch in text.chars() {
        if ch.is_ascii() {
            ascii_count += 1;
        } else {
            non_ascii_count += 1;
        }
    }
    let weighted = ascii_count.saturating_add(non_ascii_count.saturating_mul(3));
    weighted.div_ceil(4).min(u32::MAX as usize) as u32
}

// ============================================================================
// Pure-Rust YAML Frontmatter Parser & Serializer
// ============================================================================

/// Parse YAML frontmatter (optional) and body from a `.md` or `.mdc` rule file.
pub fn parse_rule_document(raw: &str) -> ParsedRuleDocument {
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let (frontmatter, body) = split_frontmatter(text);

    let mut description = String::new();
    let mut globs: Vec<String> = Vec::new();
    let mut explicit_trigger: Option<RuleTriggerMode> = None;
    let mut always_apply: Option<bool> = None;
    let mut enabled: bool = true;

    if let Some(yaml) = frontmatter {
        let lines: Vec<&str> = yaml.lines().collect();
        let mut idx = 0usize;
        while idx < lines.len() {
            let line = lines[idx];
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                idx += 1;
                continue;
            }
            let Some((raw_key, raw_val)) = trimmed.split_once(':') else {
                idx += 1;
                continue;
            };
            let key = raw_key.trim().to_ascii_lowercase();
            let val_trimmed = raw_val.trim();

            match key.as_str() {
                "description" => {
                    if val_trimmed == "|" || val_trimmed == ">" {
                        let (block, next_idx) = read_indented_block(&lines, idx + 1);
                        description = block.trim().to_string();
                        idx = next_idx;
                        continue;
                    }
                    description = unquote_yaml_scalar(val_trimmed);
                }
                "globs" | "paths" => {
                    if val_trimmed.is_empty() {
                        let (items, next_idx) = read_yaml_list_items(&lines, idx + 1);
                        globs.extend(items);
                        idx = next_idx;
                        continue;
                    }
                    globs.extend(parse_globs_inline_value(val_trimmed));
                }
                "trigger" | "trigger_mode" | "triggermode" => {
                    let scalar = unquote_yaml_scalar(val_trimmed);
                    if let Some(mode) = RuleTriggerMode::from_frontmatter_str(&scalar) {
                        explicit_trigger = Some(mode);
                    }
                }
                "alwaysapply" | "always_apply" => {
                    if let Some(b) = parse_yaml_bool(val_trimmed) {
                        always_apply = Some(b);
                    }
                }
                "enabled" => {
                    if let Some(b) = parse_yaml_bool(val_trimmed) {
                        enabled = b;
                    }
                }
                _ => {}
            }
            idx += 1;
        }
    }

    globs = normalize_glob_list(globs);

    let trigger_mode = if let Some(mode) = explicit_trigger {
        mode
    } else if always_apply == Some(true) {
        RuleTriggerMode::Always
    } else if !globs.is_empty() {
        RuleTriggerMode::GlobAutoAttach
    } else if always_apply == Some(false) {
        if !description.is_empty() {
            RuleTriggerMode::AgentDecided
        } else {
            RuleTriggerMode::Manual
        }
    } else {
        RuleTriggerMode::Always
    };

    ParsedRuleDocument {
        description,
        trigger_mode,
        globs,
        enabled,
        body,
    }
}

/// Serialize rule metadata and markdown body into canonical YAML frontmatter + markdown.
pub fn serialize_rule_document(doc: &ParsedRuleDocument) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    if !doc.description.trim().is_empty() {
        out.push_str("description: ");
        out.push_str(&quote_yaml_string(doc.description.trim()));
        out.push('\n');
    }
    out.push_str("trigger: ");
    out.push_str(doc.trigger_mode.as_frontmatter_str());
    out.push('\n');
    out.push_str("alwaysApply: ");
    out.push_str(if doc.trigger_mode == RuleTriggerMode::Always {
        "true"
    } else {
        "false"
    });
    out.push('\n');

    let cleaned_globs = normalize_glob_list(doc.globs.clone());
    if !cleaned_globs.is_empty() {
        out.push_str("globs:\n");
        for g in &cleaned_globs {
            out.push_str("  - ");
            out.push_str(&quote_yaml_string(g));
            out.push('\n');
        }
    }
    if !doc.enabled {
        out.push_str("enabled: false\n");
    }
    out.push_str("---\n");

    let body = doc.body.trim_start_matches(['\r', '\n']);
    if !body.is_empty() {
        out.push('\n');
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

pub(crate) fn split_frontmatter(text: &str) -> (Option<&str>, String) {
    let rest = if let Some(r) = text.strip_prefix("---\r\n") {
        r
    } else if let Some(r) = text.strip_prefix("---\n") {
        r
    } else {
        return (None, text.to_string());
    };

    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        // Require closing `---` / `...` to start at column 0 (no leading indentation) so
        // indented `  ---` inside YAML block scalars does not prematurely terminate frontmatter.
        let without_eol = line.trim_end_matches(['\r', '\n']);
        let trimmed_right = without_eol.trim_end_matches([' ', '\t']);
        if trimmed_right == "---" || trimmed_right == "..." {
            let yaml = &rest[..offset];
            let after = &rest[offset + line.len()..];
            // Strip at most one blank separator line after closing `---`
            let body = after
                .strip_prefix("\r\n")
                .or_else(|| after.strip_prefix('\n'))
                .unwrap_or(after);
            return (Some(yaml), body.to_string());
        }
        offset += line.len();
    }

    // Check final line without trailing newline
    let tail = &rest[offset..];
    let tail_right = tail.trim_end_matches([' ', '\t']);
    if tail_right == "---" || tail_right == "..." {
        return (Some(&rest[..offset]), String::new());
    }

    // Unterminated frontmatter: treat entire file as body
    (None, text.to_string())
}

fn read_indented_block(lines: &[&str], start: usize) -> (String, usize) {
    let mut out = Vec::new();
    let mut idx = start;
    while idx < lines.len() {
        let line = lines[idx];
        if line.starts_with(' ') || line.starts_with('\t') {
            out.push(line.trim());
            idx += 1;
        } else if line.trim().is_empty() {
            out.push("");
            idx += 1;
        } else {
            break;
        }
    }
    (out.join("\n"), idx)
}

fn read_yaml_list_items(lines: &[&str], start: usize) -> (Vec<String>, usize) {
    let mut items = Vec::new();
    let mut idx = start;
    while idx < lines.len() {
        let line = lines[idx];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            idx += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- ") {
            let val = unquote_yaml_scalar(rest.trim());
            if !val.is_empty() {
                items.push(val);
            }
            idx += 1;
        } else if trimmed == "-" {
            idx += 1;
        } else {
            break;
        }
    }
    (items, idx)
}

/// Strip an unquoted trailing YAML comment (` # ...` or `\t# ...`), preserving `#` inside quotes.
fn strip_yaml_inline_comment(raw: &str) -> &str {
    let s = raw.trim();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut prev_ws = false;

    for (byte_idx, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            prev_ws = false;
            continue;
        }
        if in_double && ch == '\\' {
            escaped = true;
            prev_ws = false;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            prev_ws = false;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            prev_ws = false;
            continue;
        }
        if !in_single && !in_double && ch == '#' && prev_ws {
            return s[..byte_idx].trim_end();
        }
        prev_ws = ch == ' ' || ch == '\t';
    }
    s
}

fn parse_globs_inline_value(val: &str) -> Vec<String> {
    let trimmed = strip_yaml_inline_comment(val);
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        let inner = &trimmed[1..trimmed.len() - 1];
        return split_commas_respecting_braces_and_quotes(inner)
            .into_iter()
            .map(|item| unquote_yaml_scalar(item.trim()))
            .filter(|s| !s.is_empty())
            .collect();
    }
    let unquoted = unquote_yaml_scalar(trimmed);
    split_commas_respecting_braces_and_quotes(&unquoted)
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn split_commas_respecting_braces_and_quotes(raw: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut brace_depth = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in raw.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
            continue;
        }
        if in_double && ch == '\\' {
            cur.push(ch);
            escaped = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            cur.push(ch);
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            cur.push(ch);
            continue;
        }
        if !in_single && !in_double {
            match ch {
                '{' => {
                    brace_depth += 1;
                    cur.push(ch);
                    continue;
                }
                '}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                    cur.push(ch);
                    continue;
                }
                ',' if brace_depth == 0 => {
                    parts.push(std::mem::take(&mut cur));
                    continue;
                }
                _ => {}
            }
        }
        cur.push(ch);
    }
    parts.push(cur);
    parts
}

fn unquote_yaml_scalar(raw: &str) -> String {
    let s = strip_yaml_inline_comment(raw);
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('u') => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if hex.len() == 4
                            && let Ok(code) = u32::from_str_radix(&hex, 16)
                            && let Some(decoded) = char::from_u32(code)
                        {
                            out.push(decoded);
                        } else {
                            out.push_str("\\u");
                            out.push_str(&hex);
                        }
                    }
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return s[1..s.len() - 1].replace("''", "'");
    }
    s.to_string()
}

fn quote_yaml_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0085}' => out.push_str("\\u0085"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn parse_yaml_bool(raw: &str) -> Option<bool> {
    match unquote_yaml_scalar(raw).to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn normalize_glob_list(raw: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for item in raw {
        let trimmed = item.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_GLOB_PATTERN_LEN {
            continue;
        }
        let normalized = trimmed.replace('\\', "/");
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }
    out
}

// ============================================================================
// Pure-Rust ReDoS-Free Glob Matcher (`*`, `**`, `?`, `{a,b}`, `[abc]`, `!`)
// ============================================================================

#[derive(Debug, Clone)]
enum CharClassItem {
    Single(char),
    Range(char, char),
}

#[derive(Debug, Clone)]
enum GlobToken {
    /// Matches zero or more complete directory segments (`**/` at start or after `/`).
    DoubleStarSlash,
    /// Matches zero or more arbitrary characters including `/`.
    DoubleStar,
    /// Matches zero or more non-`/` characters.
    Star,
    /// Matches any single non-`/` character.
    Question,
    /// Matches a single non-`/` character belonging to (or excluded from) the class.
    CharClass {
        negated: bool,
        items: Vec<CharClassItem>,
    },
    /// Matches an exact literal character.
    Literal(char),
}

/// Normalize a candidate workspace path for glob matching (`\` -> `/`, strip leading `./` and `/`).
pub fn normalize_match_path(path: &str) -> String {
    let mut s = path.trim().replace('\\', "/");
    while let Some(stripped) = s.strip_prefix("./") {
        s = stripped.to_string();
    }
    while let Some(stripped) = s.strip_prefix('/') {
        s = stripped.to_string();
    }
    s
}

/// Expand `{a,b}` brace groups with strict expansion & depth caps (ReDoS/OOM-free).
fn expand_braces(pattern: &str) -> Vec<String> {
    let mut results = Vec::new();
    expand_braces_inner(pattern, 0, &mut results);
    if results.is_empty() {
        results.push(pattern.to_string());
    }
    results
}

fn expand_braces_inner(pattern: &str, depth: usize, out: &mut Vec<String>) {
    if out.len() >= MAX_BRACE_EXPANSIONS {
        return;
    }
    if depth >= MAX_BRACE_DEPTH {
        out.push(pattern.to_string());
        return;
    }

    let chars: Vec<char> = pattern.chars().collect();
    let mut open_idx: Option<usize> = None;
    let mut close_idx: Option<usize> = None;
    let mut nesting = 0usize;
    let mut comma_positions = Vec::new();

    for (i, &ch) in chars.iter().enumerate() {
        match ch {
            '{' => {
                if nesting == 0 {
                    open_idx = Some(i);
                    comma_positions.clear();
                }
                nesting += 1;
            }
            ',' if nesting == 1 => {
                comma_positions.push(i);
            }
            '}' if nesting > 0 => {
                nesting -= 1;
                if nesting == 0 && open_idx.is_some() {
                    close_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let (Some(start), Some(end)) = (open_idx, close_idx) else {
        out.push(pattern.to_string());
        return;
    };

    let prefix: String = chars[..start].iter().collect();
    let suffix: String = chars[end + 1..].iter().collect();

    let mut alternatives = Vec::new();
    let mut prev = start + 1;
    for comma in comma_positions {
        alternatives.push(chars[prev..comma].iter().collect::<String>());
        prev = comma + 1;
    }
    alternatives.push(chars[prev..end].iter().collect::<String>());

    for alt in alternatives {
        if out.len() >= MAX_BRACE_EXPANSIONS {
            break;
        }
        let combined = format!("{prefix}{alt}{suffix}");
        if combined.len() <= MAX_GLOB_PATTERN_LEN * 2 {
            expand_braces_inner(&combined, depth + 1, out);
        }
    }
}

fn tokenize_glob(pattern: &str) -> Vec<GlobToken> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::with_capacity(chars.len());
    let mut i = 0usize;

    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    let start = i;
                    while i < chars.len() && chars[i] == '*' {
                        i += 1;
                    }
                    let at_segment_start = start == 0 || chars[start - 1] == '/';
                    if at_segment_start && i < chars.len() && chars[i] == '/' {
                        i += 1;
                        tokens.push(GlobToken::DoubleStarSlash);
                    } else {
                        tokens.push(GlobToken::DoubleStar);
                    }
                } else {
                    i += 1;
                    tokens.push(GlobToken::Star);
                }
            }
            '?' => {
                i += 1;
                tokens.push(GlobToken::Question);
            }
            '[' => {
                if let Some((token, next_i)) = parse_char_class(&chars, i) {
                    tokens.push(token);
                    i = next_i;
                } else {
                    i += 1;
                    tokens.push(GlobToken::Literal('['));
                }
            }
            '\\' if i + 1 < chars.len() => {
                tokens.push(GlobToken::Literal(chars[i + 1]));
                i += 2;
            }
            other => {
                i += 1;
                tokens.push(GlobToken::Literal(other));
            }
        }
    }

    tokens
}

fn parse_char_class(chars: &[char], start: usize) -> Option<(GlobToken, usize)> {
    let mut i = start + 1;
    if i >= chars.len() {
        return None;
    }
    let mut negated = false;
    if chars[i] == '!' || chars[i] == '^' {
        negated = true;
        i += 1;
    }
    let mut items = Vec::new();
    let mut first = true;
    while i < chars.len() {
        let ch = chars[i];
        if ch == ']' && !first {
            return Some((GlobToken::CharClass { negated, items }, i + 1));
        }
        first = false;
        if i + 2 < chars.len() && chars[i + 1] == '-' && chars[i + 2] != ']' {
            let lo = ch;
            let hi = chars[i + 2];
            if lo <= hi {
                items.push(CharClassItem::Range(lo, hi));
            } else {
                items.push(CharClassItem::Range(hi, lo));
            }
            i += 3;
        } else {
            items.push(CharClassItem::Single(ch));
            i += 1;
        }
    }
    None
}

fn char_class_matches(ch: char, negated: bool, items: &[CharClassItem]) -> bool {
    let hit = items.iter().any(|item| match *item {
        CharClassItem::Single(c) => c == ch,
        CharClassItem::Range(lo, hi) => ch >= lo && ch <= hi,
    });
    if negated { !hit } else { hit }
}

/// Iterative O(tokens * path_len) DP matcher for a single brace-free positive glob pattern.
fn match_single_glob(pattern: &str, path: &str) -> bool {
    if pattern.is_empty() || pattern.len() > MAX_GLOB_PATTERN_LEN * 2 {
        return false;
    }
    let norm_pat = normalize_match_path(pattern);
    let norm_path = normalize_match_path(path);
    let tokens = tokenize_glob(&norm_pat);
    let path_chars: Vec<char> = norm_path.chars().collect();
    let n = path_chars.len();

    let mut cur = vec![false; n + 1];
    let mut next = vec![false; n + 1];
    cur[0] = true;

    for token in &tokens {
        next.fill(false);
        match token {
            GlobToken::Literal(expected) => {
                for j in 0..n {
                    if cur[j] && path_chars[j] == *expected {
                        next[j + 1] = true;
                    }
                }
            }
            GlobToken::Question => {
                for j in 0..n {
                    if cur[j] && path_chars[j] != '/' {
                        next[j + 1] = true;
                    }
                }
            }
            GlobToken::CharClass { negated, items } => {
                for j in 0..n {
                    if cur[j]
                        && path_chars[j] != '/'
                        && char_class_matches(path_chars[j], *negated, items)
                    {
                        next[j + 1] = true;
                    }
                }
            }
            GlobToken::Star => {
                for j in 0..=n {
                    if cur[j] {
                        next[j] = true;
                    }
                    if j > 0 && next[j - 1] && path_chars[j - 1] != '/' {
                        next[j] = true;
                    }
                }
            }
            GlobToken::DoubleStar => {
                let mut seen = false;
                for j in 0..=n {
                    if cur[j] {
                        seen = true;
                    }
                    if seen {
                        next[j] = true;
                    }
                }
            }
            GlobToken::DoubleStarSlash => {
                let mut active = false;
                for j in 0..=n {
                    if cur[j] {
                        next[j] = true;
                        if j == 0 || path_chars[j - 1] == '/' {
                            active = true;
                        }
                    }
                    if active && j > 0 && path_chars[j - 1] == '/' {
                        next[j] = true;
                    }
                }
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }

    cur[n]
}

/// Match a single positive glob pattern (supporting `{a,b}` braces) against a path.
pub fn glob_matches_path(pattern: &str, path: &str) -> bool {
    let trimmed = pattern.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_GLOB_PATTERN_LEN {
        return false;
    }
    let norm_path = normalize_match_path(path);
    if norm_path.is_empty() {
        return false;
    }
    let expanded = expand_braces(trimmed);
    expanded
        .iter()
        .any(|pat| match_single_glob(pat, &norm_path))
}

/// Evaluate a rule's glob list (positive patterns + `!negation` patterns) against a single path.
/// Returns `Some(matched_positive_globs)` if the path matches and is not excluded by any `!` pattern.
pub fn match_glob_set(globs: &[String], path: &str) -> Option<Vec<String>> {
    let norm_path = normalize_match_path(path);
    if norm_path.is_empty() || globs.is_empty() {
        return None;
    }

    let mut positive_patterns = Vec::new();
    let mut negative_patterns = Vec::new();
    for raw in globs {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_GLOB_PATTERN_LEN {
            continue;
        }
        if let Some(neg) = trimmed.strip_prefix('!') {
            let neg_trimmed = neg.trim();
            if !neg_trimmed.is_empty() {
                negative_patterns.push(neg_trimmed);
            }
        } else {
            positive_patterns.push(trimmed);
        }
    }

    if positive_patterns.is_empty() && negative_patterns.is_empty() {
        return None;
    }

    for neg in &negative_patterns {
        if glob_matches_path(neg, &norm_path) {
            return None;
        }
    }

    if positive_patterns.is_empty() {
        return Some(
            negative_patterns
                .into_iter()
                .map(|n| format!("!{n}"))
                .collect(),
        );
    }

    let mut matched = Vec::new();
    for pos in positive_patterns {
        if glob_matches_path(pos, &norm_path) {
            matched.push(pos.to_string());
        }
    }
    if matched.is_empty() {
        None
    } else {
        Some(matched)
    }
}

// ============================================================================
// Bounded Canonical Discovery, Strict Path-Confined Upsert & Delete
// ============================================================================

fn scope_segments(scope: RuleScope) -> &'static [&'static str] {
    match scope {
        RuleScope::UserGlobal => &["rules"],
        RuleScope::ProjectOrbit => &[".orbit", "rules"],
        RuleScope::ProjectClaude => &[".claude", "rules"],
        RuleScope::ProjectCursor => &[".cursor", "rules"],
    }
}

fn scope_relative_prefix(scope: RuleScope) -> &'static str {
    match scope {
        RuleScope::UserGlobal => "~/.orbit/rules",
        RuleScope::ProjectOrbit => ".orbit/rules",
        RuleScope::ProjectClaude => ".claude/rules",
        RuleScope::ProjectCursor => ".cursor/rules",
    }
}

/// Resolve an existing confined rules directory by walking EVERY segment from `root_boundary`
/// with `fs::symlink_metadata`, rejecting any symlink or non-directory and verifying canonical
/// containment within `root_boundary`.
fn resolve_existing_confined_rules_dir(root_boundary: &Path, segments: &[&str]) -> Option<PathBuf> {
    let mut current = root_boundary.to_path_buf();
    for seg in segments {
        let next = current.join(seg);
        let meta = fs::symlink_metadata(&next).ok()?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return None;
        }
        let canon = fs::canonicalize(&next).ok()?;
        if !canon.starts_with(root_boundary) {
            return None;
        }
        current = canon;
    }
    Some(current)
}

/// Bounded read of a regular file (refuses files > `max_bytes`).
fn read_file_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|e| format!("Failed opening file: {e}"))?;
    let mut buf = Vec::new();
    file.take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed reading file: {e}"))?;
    if buf.len() > max_bytes {
        return Err(format!(
            "Rule file exceeds maximum size of {} KiB",
            max_bytes / 1024
        ));
    }
    Ok(buf)
}

/// Discover all path-scoped rules across `UserGlobal` and (when `cwd` is `Some`) project scopes.
/// Never follows symlinks and never scans the same canonical rules directory twice.
pub fn discover_path_rules(orbit_home: &Path, cwd: Option<&Path>) -> DiscoveredPathRules {
    let canon_orbit_home = fs::canonicalize(orbit_home).ok().filter(|p| p.is_dir());
    let canon_cwd = cwd.and_then(|c| fs::canonicalize(c).ok().filter(|p| p.is_dir()));

    let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
    let mut rules: Vec<PathScopedRule> = Vec::new();

    for scope in [
        RuleScope::ProjectOrbit,
        RuleScope::ProjectClaude,
        RuleScope::ProjectCursor,
        RuleScope::UserGlobal,
    ] {
        let Some(root_boundary) = (match scope {
            RuleScope::UserGlobal => canon_orbit_home.as_deref(),
            RuleScope::ProjectOrbit | RuleScope::ProjectClaude | RuleScope::ProjectCursor => {
                canon_cwd.as_deref()
            }
        }) else {
            continue;
        };
        let Some(canon_rules_dir) =
            resolve_existing_confined_rules_dir(root_boundary, scope_segments(scope))
        else {
            continue;
        };
        if !seen_dirs.insert(canon_rules_dir.clone()) {
            continue;
        }

        let mut found_files: Vec<(PathBuf, String)> = Vec::new();
        scan_rules_dir_bounded(
            &canon_rules_dir,
            &canon_rules_dir,
            scope.file_extension(),
            0,
            &mut found_files,
        );

        for (file_path, rel_inside) in found_files {
            let Ok(sym_meta) = fs::symlink_metadata(&file_path) else {
                continue;
            };
            if sym_meta.file_type().is_symlink()
                || !sym_meta.is_file()
                || sym_meta.len() > MAX_RULE_FILE_BYTES as u64
            {
                continue;
            }
            let Ok(bytes) = read_file_bounded(&file_path, MAX_RULE_FILE_BYTES) else {
                continue;
            };
            let Ok(raw_text) = String::from_utf8(bytes.clone()) else {
                continue;
            };

            let parsed = parse_rule_document(&raw_text);
            let ext_suffix = format!(".{}", scope.file_extension());
            let stem = rel_inside
                .strip_suffix(&ext_suffix)
                .unwrap_or(&rel_inside)
                .to_string();
            let id = format!("{}:{stem}", scope.slug());
            let relative_path = format!("{}/{rel_inside}", scope_relative_prefix(scope));
            let approx_tokens = estimate_tokens(&parsed.body);
            let etag = sha256_hex(&bytes);

            rules.push(PathScopedRule {
                id,
                name: stem,
                description: parsed.description,
                scope,
                path: file_path.to_string_lossy().to_string(),
                relative_path,
                trigger_mode: parsed.trigger_mode,
                globs: parsed.globs,
                enabled: parsed.enabled,
                body: parsed.body,
                approx_tokens,
                etag,
            });
        }
    }

    rules.sort_by(|a, b| {
        a.scope
            .sort_rank()
            .cmp(&b.scope.sort_rank())
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });

    let total_always_tokens: u32 = rules
        .iter()
        .filter(|r| r.enabled && r.trigger_mode == RuleTriggerMode::Always)
        .fold(0u32, |acc, r| acc.saturating_add(r.approx_tokens));
    let token_budget_warning = total_always_tokens > PATH_RULES_WARN_TOKENS;

    DiscoveredPathRules {
        rules,
        total_always_tokens,
        token_budget_warning,
    }
}

fn scan_rules_dir_bounded(
    canon_root: &Path,
    dir: &Path,
    expected_ext: &str,
    depth: usize,
    out: &mut Vec<(PathBuf, String)>,
) {
    if depth > MAX_SCAN_DEPTH || out.len() >= MAX_RULES_PER_SCOPE {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|x| x.path())).collect();
    children.sort();

    for entry_path in children {
        if out.len() >= MAX_RULES_PER_SCOPE {
            break;
        }
        let Ok(sym_meta) = fs::symlink_metadata(&entry_path) else {
            continue;
        };
        // Reject ALL symlinks (both directories and files) before canonicalization
        if sym_meta.file_type().is_symlink() {
            continue;
        }
        let Ok(canon) = fs::canonicalize(&entry_path) else {
            continue;
        };
        if !canon.starts_with(canon_root) {
            continue;
        }
        if sym_meta.is_dir() {
            scan_rules_dir_bounded(canon_root, &canon, expected_ext, depth + 1, out);
            continue;
        }
        if !sym_meta.is_file() {
            continue;
        }
        let ext_matches = canon
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(expected_ext));
        if !ext_matches {
            continue;
        }
        let Ok(rel) = canon.strip_prefix(canon_root) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        out.push((canon, rel_str));
    }
}

fn validate_ascii_segment(seg: &str, label: &str) -> Result<(), String> {
    if seg.is_empty() || seg.len() > 64 {
        return Err(format!("{label} must be between 1 and 64 characters"));
    }
    let mut chars = seg.chars();
    let Some(first) = chars.next() else {
        return Err(format!("{label} cannot be empty"));
    };
    if !first.is_ascii_alphanumeric() {
        return Err(format!("{label} must start with an ASCII letter or digit"));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!(
            "{label} may only contain ASCII letters, digits, hyphens (`-`), and underscores (`_`)"
        ));
    }
    Ok(())
}

/// Validate a rule name (supporting optional nested subdirectory segments up to `MAX_SCAN_DEPTH`,
/// e.g. `rust-style` or `backend/sql`, with zero `..`, backslashes, or control chars).
pub fn validate_rule_filename(
    raw_name: &str,
    scope: RuleScope,
) -> Result<ValidatedRuleRelPath, String> {
    let trimmed = raw_name.trim();
    if trimmed.is_empty() {
        return Err("Rule name cannot be empty".to_string());
    }
    if trimmed.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("Rule name contains invalid control characters".to_string());
    }
    if trimmed.contains('\\') || trimmed.starts_with('/') || trimmed.ends_with('/') {
        return Err("Rule name must not use backslashes or leading/trailing `/`".to_string());
    }

    let raw_segments: Vec<&str> = trimmed.split('/').collect();
    if raw_segments.len() > MAX_SCAN_DEPTH + 1 {
        return Err(format!(
            "Rule path exceeds maximum nesting depth of {MAX_SCAN_DEPTH}"
        ));
    }

    let (leaf_raw, parent_raws) = raw_segments
        .split_last()
        .ok_or_else(|| "Rule name cannot be empty".to_string())?;

    let mut subdirs = Vec::with_capacity(parent_raws.len());
    for &dir_seg in parent_raws {
        if dir_seg.is_empty() || dir_seg == "." || dir_seg == ".." || dir_seg.contains('.') {
            return Err(format!("Invalid rule subdirectory `{dir_seg}`"));
        }
        validate_ascii_segment(dir_seg, "Rule subdirectory")?;
        subdirs.push(dir_seg.to_string());
    }

    if leaf_raw.contains("..") {
        return Err("Rule name must not contain `..`".to_string());
    }
    let expected_ext = scope.file_extension();
    let expected_suffix = format!(".{expected_ext}");
    let stem = if let Some(stripped) = leaf_raw.strip_suffix(&expected_suffix) {
        stripped
    } else if leaf_raw.contains('.') {
        return Err(format!(
            "Rule name must use alphanumeric, `-`, or `_` characters (optional `{expected_suffix}` suffix)"
        ));
    } else {
        leaf_raw
    };

    validate_ascii_segment(stem, "Rule name")?;

    Ok(ValidatedRuleRelPath {
        subdirs,
        filename: format!("{stem}.{expected_ext}"),
    })
}

/// Ensure the confined target directory (scope rules dir + optional validated subdirectories)
/// exists strictly inside its canonical root without traversing any symlinks.
fn ensure_confined_rules_dir(
    orbit_home: &Path,
    cwd: Option<&Path>,
    scope: RuleScope,
    subdirs: &[String],
) -> Result<PathBuf, String> {
    let root_dir: PathBuf = match scope {
        RuleScope::UserGlobal => {
            fs::create_dir_all(orbit_home)
                .map_err(|e| format!("Failed creating Orbit home directory: {e}"))?;
            fs::canonicalize(orbit_home)
                .map_err(|e| format!("Cannot resolve Orbit home directory: {e}"))?
        }
        RuleScope::ProjectOrbit | RuleScope::ProjectClaude | RuleScope::ProjectCursor => {
            let cwd_path = cwd.ok_or_else(|| {
                "Workspace directory is required for project-scoped rules".to_string()
            })?;
            fs::canonicalize(cwd_path)
                .map_err(|e| format!("Workspace directory unavailable: {e}"))?
        }
    };

    let mut current = root_dir.clone();
    let base_segs = scope_segments(scope);
    for seg in base_segs
        .iter()
        .copied()
        .chain(subdirs.iter().map(String::as_str))
    {
        let next = current.join(seg);
        match fs::symlink_metadata(&next) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(format!(
                        "Refusing to traverse symlink `{seg}` in rules path"
                    ));
                }
                if !meta.is_dir() {
                    return Err(format!("Rules path segment `{seg}` is not a directory"));
                }
                let canon = fs::canonicalize(&next)
                    .map_err(|e| format!("Cannot resolve rules directory `{seg}`: {e}"))?;
                if !canon.starts_with(&root_dir) {
                    return Err("Rules directory escapes allowed root".to_string());
                }
                current = canon;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&next)
                    .map_err(|ce| format!("Failed creating rules directory `{seg}`: {ce}"))?;
                let canon = fs::canonicalize(&next).map_err(|ce| {
                    format!("Cannot resolve created rules directory `{seg}`: {ce}")
                })?;
                if !canon.starts_with(&root_dir) {
                    return Err("Created rules directory escapes allowed root".to_string());
                }
                current = canon;
            }
            Err(e) => return Err(format!("Cannot inspect rules directory `{seg}`: {e}")),
        }
    }

    Ok(current)
}

/// Atomically create or update a path-scoped rule file strictly inside its allowed rules directory.
/// Enforces optimistic concurrency:
/// - `expected_etag == None` refuses to overwrite an existing rule.
/// - `expected_etag == Some(etag)` refuses if the file was deleted or modified on disk.
pub fn upsert_path_rule(
    orbit_home: &Path,
    cwd: Option<&Path>,
    spec: &UpsertRuleSpec,
) -> Result<DiscoveredPathRules, String> {
    let validated = validate_rule_filename(&spec.name, spec.scope)?;
    let target_dir = ensure_confined_rules_dir(orbit_home, cwd, spec.scope, &validated.subdirs)?;
    let target = target_dir.join(&validated.filename);
    let expected = spec
        .expected_etag
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match fs::symlink_metadata(&target) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "Refusing to write rule through symlink `{}`",
                    validated.filename
                ));
            }
            if !meta.is_file() {
                return Err(format!(
                    "Target `{}` is not a regular file",
                    validated.filename
                ));
            }
            let Some(expected_hash) = expected else {
                return Err("Rule already exists; edit it or pick another name".to_string());
            };
            if meta.len() > MAX_RULE_FILE_BYTES as u64 {
                return Err(format!(
                    "Existing rule `{}` exceeds maximum size limit",
                    validated.filename
                ));
            }
            let existing = read_file_bounded(&target, MAX_RULE_FILE_BYTES)?;
            let actual = sha256_hex(&existing);
            if actual != expected_hash {
                return Err(
                    "Rule was modified on disk since it was loaded; refresh and retry".to_string(),
                );
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if expected.is_some() {
                return Err(
                    "Rule was deleted on disk since it was loaded; refresh and retry".to_string(),
                );
            }
        }
        Err(e) => {
            return Err(format!(
                "Cannot inspect target rule `{}`: {e}",
                validated.filename
            ));
        }
    }

    let doc = ParsedRuleDocument {
        description: spec.description.clone(),
        trigger_mode: spec.trigger_mode,
        globs: normalize_glob_list(spec.globs.clone()),
        enabled: spec.enabled,
        body: spec.body.clone(),
    };
    let serialized = serialize_rule_document(&doc);
    if serialized.len() > MAX_RULE_FILE_BYTES {
        return Err(format!(
            "Rule exceeds maximum size of {} KiB",
            MAX_RULE_FILE_BYTES / 1024
        ));
    }

    let tmp_name = format!(
        ".orbit-rule-tmp-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    );
    let tmp_path = target_dir.join(tmp_name);
    let mut tmp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .map_err(|e| format!("Failed creating temp rule file: {e}"))?;
    if let Err(e) = tmp_file
        .write_all(serialized.as_bytes())
        .and_then(|_| tmp_file.sync_all())
    {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed writing rule `{}`: {e}", validated.filename));
    }
    drop(tmp_file);
    if let Err(e) = fs::rename(&tmp_path, &target) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed saving rule `{}`: {e}", validated.filename));
    }

    Ok(discover_path_rules(orbit_home, cwd))
}

/// Walk relative components from `canon_rules_dir` downward using `symlink_metadata`,
/// rejecting any symlink on intermediate subdirectories OR the target file itself.
fn walk_confined_leaf_no_symlinks(
    canon_rules_dir: &Path,
    rel_path: &Path,
    expected_ext: &str,
) -> Result<PathBuf, String> {
    let mut segs: Vec<&std::ffi::OsStr> = Vec::new();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(s) => segs.push(s),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("Rule path contains forbidden traversal component".to_string());
            }
        }
    }
    let (leaf, parents) = segs
        .split_last()
        .ok_or_else(|| "Rule path cannot point to rules directory itself".to_string())?;

    let mut current = canon_rules_dir.to_path_buf();
    for &dir_seg in parents {
        let next = current.join(dir_seg);
        let meta =
            fs::symlink_metadata(&next).map_err(|e| format!("Rule subdirectory not found: {e}"))?;
        if meta.file_type().is_symlink() {
            return Err("Refusing to traverse symlink inside rules directory".to_string());
        }
        if !meta.is_dir() {
            return Err("Rule parent segment is not a directory".to_string());
        }
        let canon = fs::canonicalize(&next)
            .map_err(|e| format!("Cannot resolve rule subdirectory: {e}"))?;
        if !canon.starts_with(canon_rules_dir) {
            return Err("Rule subdirectory escapes rules boundary".to_string());
        }
        current = canon;
    }

    let leaf_path = current.join(leaf);
    let leaf_meta =
        fs::symlink_metadata(&leaf_path).map_err(|e| format!("Rule file not found: {e}"))?;
    if leaf_meta.file_type().is_symlink() {
        return Err("Refusing to delete symlink rule target".to_string());
    }
    if !leaf_meta.is_file() {
        return Err("Rule target is not a regular file".to_string());
    }
    let ext_ok = leaf_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(expected_ext));
    if !ext_ok {
        return Err(format!(
            "Refusing to delete non-`.{expected_ext}` file inside rules directory"
        ));
    }
    let canon_leaf =
        fs::canonicalize(&leaf_path).map_err(|e| format!("Cannot resolve rule file: {e}"))?;
    if !canon_leaf.starts_with(canon_rules_dir) {
        return Err("Rule target escapes allowed rules directory".to_string());
    }
    Ok(canon_leaf)
}

/// Resolve and strictly validate a deletion target inside an allowed, non-symlinked rules directory.
/// When `cwd` is `None`, ONLY `UserGlobal` (`<orbit_home>/rules`) is eligible.
pub fn resolve_confined_delete_target(
    orbit_home: &Path,
    cwd: Option<&Path>,
    raw_path: &str,
) -> Result<(PathBuf, RuleScope), String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("Rule path cannot be empty".to_string());
    }
    if trimmed.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("Rule path contains invalid control characters".to_string());
    }
    let p = Path::new(trimmed);
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Rule path must not contain `..` traversal segments".to_string());
    }

    let canon_orbit_home = fs::canonicalize(orbit_home).ok().filter(|d| d.is_dir());
    let canon_cwd = cwd.and_then(|c| fs::canonicalize(c).ok().filter(|d| d.is_dir()));

    for scope in [
        RuleScope::UserGlobal,
        RuleScope::ProjectOrbit,
        RuleScope::ProjectClaude,
        RuleScope::ProjectCursor,
    ] {
        let (root_boundary, raw_root) = match scope {
            RuleScope::UserGlobal => (canon_orbit_home.as_deref(), Some(orbit_home)),
            RuleScope::ProjectOrbit | RuleScope::ProjectClaude | RuleScope::ProjectCursor => {
                (canon_cwd.as_deref(), cwd)
            }
        };
        let Some(root_boundary) = root_boundary else {
            continue;
        };
        let Some(canon_rules_dir) =
            resolve_existing_confined_rules_dir(root_boundary, scope_segments(scope))
        else {
            continue;
        };

        // Determine relative path inside `canon_rules_dir` without calling canonicalize on symlinks
        let rel_inside: Option<PathBuf> = if p.is_absolute() {
            if let Ok(r) = p.strip_prefix(&canon_rules_dir) {
                Some(r.to_path_buf())
            } else if let Some(rr) = raw_root {
                let mut raw_dir = rr.to_path_buf();
                for seg in scope_segments(scope) {
                    raw_dir.push(seg);
                }
                p.strip_prefix(&raw_dir).ok().map(Path::to_path_buf)
            } else {
                None
            }
        } else {
            let prefix = format!("{}/", scope_relative_prefix(scope));
            trimmed.strip_prefix(&prefix).map(PathBuf::from)
        };

        if let Some(rel) = rel_inside {
            let leaf =
                walk_confined_leaf_no_symlinks(&canon_rules_dir, &rel, scope.file_extension())?;
            return Ok((leaf, scope));
        }
    }

    Err("Target file is outside allowed rules directories".to_string())
}

/// Delete a path-scoped rule file after strict canonical confinement & symlink verification.
pub fn delete_path_rule(
    orbit_home: &Path,
    cwd: Option<&Path>,
    raw_path: &str,
) -> Result<DiscoveredPathRules, String> {
    let (canon_target, _scope) = resolve_confined_delete_target(orbit_home, cwd, raw_path)?;
    fs::remove_file(&canon_target).map_err(|e| format!("Failed deleting rule file: {e}"))?;
    Ok(discover_path_rules(orbit_home, cwd))
}

// ============================================================================
// Deterministic Rule Matching & Prompt Injection Builder
// ============================================================================

/// Match discovered rules against active workspace paths and explicit `@rule:<name>` names,
/// building a deterministic `<WORKSPACE_RULES>` markdown block bounded by `token_cap`.
pub fn match_and_build_rules_injection(
    rules: &[PathScopedRule],
    active_paths: &[String],
    manual_rule_names: &[String],
    token_cap: u32,
) -> MatchedPathRulesResult {
    let normalized_paths: Vec<String> = active_paths
        .iter()
        .map(|p| normalize_match_path(p))
        .filter(|p| !p.is_empty())
        .collect();
    let manual_set: HashSet<String> = manual_rule_names
        .iter()
        .map(|n| n.trim().trim_start_matches("@rule:").to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .collect();

    let mut matched_full: Vec<(&PathScopedRule, Vec<String>)> = Vec::new();
    let mut agent_decided: Vec<&PathScopedRule> = Vec::new();
    let mut seen_ids: HashSet<&str> = HashSet::new();

    // Priority 1: Explicitly requested via `@rule:<name>` (Manual or any enabled rule referenced by name)
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        let name_lower = rule.name.to_ascii_lowercase();
        let id_lower = rule.id.to_ascii_lowercase();
        if (manual_set.contains(&name_lower) || manual_set.contains(&id_lower))
            && seen_ids.insert(rule.id.as_str())
        {
            matched_full.push((rule, Vec::new()));
        }
    }

    // Priority 2: Always-applied rules
    for rule in rules {
        if !rule.enabled || rule.trigger_mode != RuleTriggerMode::Always {
            continue;
        }
        if seen_ids.insert(rule.id.as_str()) {
            matched_full.push((rule, Vec::new()));
        }
    }

    // Priority 3: GlobAutoAttach rules matching at least one active path
    for rule in rules {
        if !rule.enabled || rule.trigger_mode != RuleTriggerMode::GlobAutoAttach {
            continue;
        }
        if seen_ids.contains(rule.id.as_str()) {
            continue;
        }
        let mut matched_globs: Vec<String> = Vec::new();
        let mut seen_globs: HashSet<String> = HashSet::new();
        for path in &normalized_paths {
            if let Some(hits) = match_glob_set(&rule.globs, path) {
                for g in hits {
                    if seen_globs.insert(g.clone()) {
                        matched_globs.push(g);
                    }
                }
            }
        }
        if !matched_globs.is_empty() && seen_ids.insert(rule.id.as_str()) {
            matched_full.push((rule, matched_globs));
        }
    }

    // Priority 4: AgentDecided rules not already full-attached -> concise 1-line index
    for rule in rules {
        if !rule.enabled || rule.trigger_mode != RuleTriggerMode::AgentDecided {
            continue;
        }
        if !seen_ids.contains(rule.id.as_str()) {
            agent_decided.push(rule);
        }
    }

    if matched_full.is_empty() && agent_decided.is_empty() {
        return MatchedPathRulesResult::default();
    }

    let mut matched_summaries: Vec<MatchedRuleSummary> = Vec::new();
    let mut sections: Vec<String> = Vec::new();
    let mut running_tokens: u32 = 0;
    let mut truncated = false;

    for (rule, matched_globs) in matched_full {
        let body = rule.body.trim();
        if body.is_empty() {
            continue;
        }
        let cost = rule.approx_tokens.max(1);
        if running_tokens.saturating_add(cost) > token_cap {
            truncated = true;
            continue;
        }
        running_tokens = running_tokens.saturating_add(cost);
        let glob_note = if matched_globs.is_empty() {
            String::new()
        } else {
            format!(" | matched: {}", matched_globs.join(", "))
        };
        sections.push(format!(
            "### Rule: {} ({}{})\n{}",
            rule.name, rule.relative_path, glob_note, body
        ));
        matched_summaries.push(MatchedRuleSummary {
            id: rule.id.clone(),
            name: rule.name.clone(),
            trigger_mode: rule.trigger_mode,
            matched_globs,
            approx_tokens: cost,
        });
    }

    let mut index_lines: Vec<String> = Vec::new();
    for rule in agent_decided {
        let desc = if rule.description.trim().is_empty() {
            "Read when relevant to the task."
        } else {
            rule.description.trim()
        };
        let line = format!("- `{}` ({}): {}", rule.name, rule.relative_path, desc);
        let cost = estimate_tokens(&line).max(1);
        if running_tokens.saturating_add(cost) > token_cap {
            truncated = true;
            break;
        }
        running_tokens = running_tokens.saturating_add(cost);
        index_lines.push(line);
        matched_summaries.push(MatchedRuleSummary {
            id: rule.id.clone(),
            name: rule.name.clone(),
            trigger_mode: rule.trigger_mode,
            matched_globs: Vec::new(),
            approx_tokens: cost,
        });
    }

    if !index_lines.is_empty() {
        sections.push(format!(
            "### Available Optional Rules (read file if relevant)\n{}",
            index_lines.join("\n")
        ));
    }

    if sections.is_empty() {
        return MatchedPathRulesResult::default();
    }

    if truncated {
        sections.push(
            "<!-- Additional workspace rules omitted due to token budget cap -->".to_string(),
        );
    }

    let injected_markdown = format!(
        "<WORKSPACE_RULES>\n{}\n</WORKSPACE_RULES>",
        sections.join("\n\n")
    );

    MatchedPathRulesResult {
        matched: matched_summaries,
        injected_markdown,
        total_tokens: running_tokens,
    }
}

/// Convenience helper combining discovery + matching with the default hard token cap.
pub fn match_rules_for_workspace(
    orbit_home: &Path,
    cwd: Option<&Path>,
    active_paths: &[String],
    manual_rule_names: &[String],
) -> MatchedPathRulesResult {
    let discovered = discover_path_rules(orbit_home, cwd);
    if discovered.rules.is_empty() {
        return MatchedPathRulesResult::default();
    }
    match_and_build_rules_injection(
        &discovered.rules,
        active_paths,
        manual_rule_names,
        PATH_RULES_HARD_CAP_TOKENS,
    )
}

/// Session-deduplicated variant of [`match_rules_for_workspace`].
///
/// Tracks `(rule.id -> sha256(trigger_mode + body))` in `already_injected` so unmodified
/// `Always` / `GlobAutoAttach` / `AgentDecided` rules are injected only once per session (keeping
/// later turns clean for LLM prompt caching). Explicit `@rule:<name>` manual mentions still re-inject
/// on demand.
pub fn match_rules_for_workspace_dedup(
    orbit_home: &Path,
    cwd: Option<&Path>,
    active_paths: &[String],
    manual_rule_names: &[String],
    already_injected: &mut std::collections::HashMap<String, String>,
) -> MatchedPathRulesResult {
    let discovered = discover_path_rules(orbit_home, cwd);
    if discovered.rules.is_empty() {
        return MatchedPathRulesResult::default();
    }
    let manual_set: HashSet<String> = manual_rule_names
        .iter()
        .map(|n| n.trim().trim_start_matches("@rule:").to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .collect();

    let mut rule_hashes: std::collections::HashMap<String, String> =
        std::collections::HashMap::with_capacity(discovered.rules.len());
    let mut filtered_rules: Vec<PathScopedRule> = Vec::with_capacity(discovered.rules.len());
    for rule in &discovered.rules {
        let content_hash =
            sha256_hex(format!("{:?}\n{}", rule.trigger_mode, rule.body.trim()).as_bytes());
        let is_manual = manual_set.contains(&rule.name.to_ascii_lowercase())
            || manual_set.contains(&rule.id.to_ascii_lowercase());
        let unchanged = already_injected
            .get(&rule.id)
            .is_some_and(|prev| prev == &content_hash);
        if is_manual || !unchanged {
            rule_hashes.insert(rule.id.clone(), content_hash);
            filtered_rules.push(rule.clone());
        }
    }

    if filtered_rules.is_empty() {
        return MatchedPathRulesResult::default();
    }

    let result = match_and_build_rules_injection(
        &filtered_rules,
        active_paths,
        manual_rule_names,
        PATH_RULES_HARD_CAP_TOKENS,
    );
    for m in &result.matched {
        if let Some(h) = rule_hashes.get(&m.id) {
            already_injected.insert(m.id.clone(), h.clone());
        }
    }
    result
}

/// Extract active workspace file paths and `@rule:<name>` tokens from a user prompt and its badges.
pub fn extract_prompt_rule_signals(
    prompt: &str,
    badges: &[PromptBadge],
    cwd: &str,
    extra_edited_paths: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut active_paths: Vec<String> = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut manual_rules: Vec<String> = Vec::new();
    let mut seen_rules: HashSet<String> = HashSet::new();

    let mut push_path = |raw: &str| {
        let trimmed = raw
            .trim()
            .trim_matches(|c| matches!(c, '`' | '"' | '\'' | ',' | ';' | ')' | '('));
        if trimmed.is_empty() {
            return;
        }
        let without_line = match trimmed.rsplit_once(':') {
            Some((prefix, line_part))
                if !prefix.is_empty()
                    && !line_part.is_empty()
                    && line_part.chars().all(|c| c.is_ascii_digit()) =>
            {
                prefix
            }
            _ => trimmed,
        };
        let rel = if !cwd.is_empty() {
            Path::new(without_line)
                .strip_prefix(cwd)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| without_line.to_string())
        } else {
            without_line.to_string()
        };
        let norm = normalize_match_path(&rel);
        if !norm.is_empty() && seen_paths.insert(norm.clone()) {
            active_paths.push(norm);
        }
    };

    for badge in badges {
        if let Some(ref p) = badge.path {
            push_path(p);
        }
    }
    for edited in extra_edited_paths {
        push_path(edited);
    }

    let bytes = prompt.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let valid_boundary = i == 0
                || matches!(
                    bytes[i - 1],
                    b' ' | b'\t' | b'\n' | b'\r' | b'`' | b'(' | b'[' | b'"' | b'\''
                );
            if valid_boundary {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() {
                    let b = bytes[end];
                    if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_' | b':') {
                        end += 1;
                    } else {
                        break;
                    }
                }
                if end > start {
                    let token = &prompt[start..end];
                    let clean_token = token.trim_end_matches(['.', ':', ',', ';']);
                    if let Some(rule_name) = clean_token
                        .strip_prefix("rule:")
                        .or_else(|| clean_token.strip_prefix("RULE:"))
                    {
                        let clean_name = rule_name.trim();
                        if !clean_name.is_empty()
                            && seen_rules.insert(clean_name.to_ascii_lowercase())
                        {
                            manual_rules.push(clean_name.to_string());
                        }
                    } else if (clean_token.contains('/') || clean_token.contains('.'))
                        && !clean_token.starts_with('.')
                    {
                        push_path(clean_token);
                    }
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }

    (active_paths, manual_rules)
}

// ============================================================================
// Comprehensive Unit & Security Regression Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct TestTempDir(PathBuf);

    impl TestTempDir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("orbit-rules-test-{}", Uuid::new_v4()));
            fs::create_dir_all(&p).expect("create test tempdir");
            Self(p)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tempdir() -> std::io::Result<TestTempDir> {
        Ok(TestTempDir::new())
    }

    #[test]
    fn frontmatter_parse_and_serialize_roundtrip_md_and_mdc() {
        // 1. GlobAutoAttach (.mdc style with comma-separated globs + braces + trailing comment)
        let mdc_raw = "\
---
description: TypeScript React guidelines
alwaysApply: false
globs: \"apps/desktop/src/**/*.{ts,tsx}, !**/tests/**\" # desktop code only
---

Always use strict TypeScript types.
Keep components concise.
";
        let parsed = parse_rule_document(mdc_raw);
        assert_eq!(parsed.description, "TypeScript React guidelines");
        assert_eq!(parsed.trigger_mode, RuleTriggerMode::GlobAutoAttach);
        assert_eq!(
            parsed.globs,
            vec!["apps/desktop/src/**/*.{ts,tsx}", "!**/tests/**"]
        );
        assert!(parsed.enabled);
        assert_eq!(
            parsed.body,
            "Always use strict TypeScript types.\nKeep components concise.\n"
        );

        let serialized = serialize_rule_document(&parsed);
        let reparsed = parse_rule_document(&serialized);
        assert_eq!(reparsed, parsed);

        // 2. Always (.md style with disabled flag + Unicode line separator in description)
        let md_disabled = "\
---
description: \"Rust safety\u{2028}rules\"
trigger: always
enabled: false
---

Never use `unsafe` or `.unwrap()` in production code.
";
        let parsed2 = parse_rule_document(md_disabled);
        assert_eq!(parsed2.trigger_mode, RuleTriggerMode::Always);
        assert!(!parsed2.enabled);
        let serialized2 = serialize_rule_document(&parsed2);
        assert!(serialized2.contains("\\u2028"));
        let reparsed2 = parse_rule_document(&serialized2);
        assert_eq!(reparsed2, parsed2);

        // 3. Indented `---` inside YAML block scalar does NOT prematurely close frontmatter
        let block_scalar = "\
---
description: |
  Line one
  ---
  Line two
trigger: agent
---

Body after real closing delimiter.
";
        let parsed_block = parse_rule_document(block_scalar);
        assert_eq!(parsed_block.trigger_mode, RuleTriggerMode::AgentDecided);
        assert!(parsed_block.description.contains("Line two"));
        assert_eq!(parsed_block.body, "Body after real closing delimiter.\n");

        // 4. Claude `.claude/rules/*.md` using `paths:` YAML list
        let claude_raw = "\
---
paths:
  - \"crates/**/*.rs\"
  - \"!**/target/**\"
---

Follow workspace Clippy rules.
";
        let parsed3 = parse_rule_document(claude_raw);
        assert_eq!(parsed3.trigger_mode, RuleTriggerMode::GlobAutoAttach);
        assert_eq!(parsed3.globs, vec!["crates/**/*.rs", "!**/target/**"]);
        assert_eq!(parsed3.body, "Follow workspace Clippy rules.\n");

        // 5. AgentDecided (Cursor style: alwaysApply: false + description, no globs)
        let agent_raw = "\
---
description: Database migration checklist
alwaysApply: false
---

Always preserve PRAGMA user_version = 6.
";
        let parsed4 = parse_rule_document(agent_raw);
        assert_eq!(parsed4.trigger_mode, RuleTriggerMode::AgentDecided);
        let reparsed4 = parse_rule_document(&serialize_rule_document(&parsed4));
        assert_eq!(reparsed4, parsed4);

        // 6. Manual (explicit trigger: manual)
        let manual_raw = "\
---
trigger: manual
---

Run release verification steps.
";
        let parsed5 = parse_rule_document(manual_raw);
        assert_eq!(parsed5.trigger_mode, RuleTriggerMode::Manual);
        let reparsed5 = parse_rule_document(&serialize_rule_document(&parsed5));
        assert_eq!(reparsed5, parsed5);

        // 7. Plain markdown without frontmatter defaults to Always + enabled
        let plain = "# Always Active\nKeep commits focused.\n";
        let parsed6 = parse_rule_document(plain);
        assert_eq!(parsed6.trigger_mode, RuleTriggerMode::Always);
        assert!(parsed6.enabled);
        assert_eq!(parsed6.body, plain);
    }

    #[test]
    fn glob_matching_supports_double_star_braces_char_class_and_negation() {
        assert!(glob_matches_path("**/*.rs", "lib.rs"));
        assert!(glob_matches_path(
            "**/*.rs",
            "crates/orbit-daemon/src/rules.rs"
        ));
        assert!(!glob_matches_path(
            "**/*.rs",
            "crates/orbit-daemon/Cargo.toml"
        ));

        assert!(glob_matches_path(
            "apps/desktop/src/**/*.{ts,tsx}",
            "apps/desktop/src/App.tsx"
        ));
        assert!(glob_matches_path(
            "apps/desktop/src/**/*.{ts,tsx}",
            "apps/desktop/src/lib/orbit-ipc.ts"
        ));
        assert!(!glob_matches_path(
            "apps/desktop/src/**/*.{ts,tsx}",
            "apps/desktop/src/index.css"
        ));
        assert!(!glob_matches_path(
            "apps/desktop/src/**/*.{ts,tsx}",
            "apps/mobile/src/App.tsx"
        ));

        assert!(glob_matches_path("src/v?/mod_[0-9].rs", "src/v2/mod_7.rs"));
        assert!(!glob_matches_path(
            "src/v?/mod_[0-9].rs",
            "src/v10/mod_7.rs"
        ));
        assert!(!glob_matches_path("src/v?/mod_[0-9].rs", "src/v2/mod_a.rs"));
        assert!(glob_matches_path("src/[!0-9]*.rs", "src/rules.rs"));
        assert!(!glob_matches_path("src/[!0-9]*.rs", "src/9rules.rs"));

        let globs = vec!["**/*.rs".to_string(), "!**/tests/**".to_string()];
        assert!(match_glob_set(&globs, "crates/orbit-daemon/src/rules.rs").is_some());
        assert!(match_glob_set(&globs, "crates/orbit-daemon/tests/integration.rs").is_none());
        assert!(match_glob_set(&globs, "tests/e2e.rs").is_none());

        let evil_pat = "a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*b";
        let evil_input = "a".repeat(200);
        assert!(!glob_matches_path(evil_pat, &evil_input));
    }

    #[test]
    fn all_four_trigger_modes_and_token_budget_cap() {
        let dir = tempdir().expect("tempdir");
        let orbit_home = dir.path().join("orbit_home");
        let repo = dir.path().join("repo");
        fs::create_dir_all(&orbit_home).expect("mkdir orbit_home");
        fs::create_dir_all(&repo).expect("mkdir repo");

        upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: RuleScope::UserGlobal,
                name: "global-style".into(),
                description: "Global tone".into(),
                trigger_mode: RuleTriggerMode::Always,
                globs: vec![],
                enabled: true,
                body: "Be concise and clear.".into(),
                expected_etag: None,
            },
        )
        .expect("upsert global");

        upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: RuleScope::ProjectOrbit,
                name: "rust-strict".into(),
                description: "No unwrap in Rust".into(),
                trigger_mode: RuleTriggerMode::GlobAutoAttach,
                globs: vec!["**/*.rs".into(), "!**/tests/**".into()],
                enabled: true,
                body: "Never use `.unwrap()` in production Rust.".into(),
                expected_etag: None,
            },
        )
        .expect("upsert project orbit");

        upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: RuleScope::ProjectClaude,
                name: "db-migrations".into(),
                description: "Use when modifying SQLite schema".into(),
                trigger_mode: RuleTriggerMode::AgentDecided,
                globs: vec![],
                enabled: true,
                body: "Keep PRAGMA user_version = 6.".into(),
                expected_etag: None,
            },
        )
        .expect("upsert project claude");

        upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: RuleScope::ProjectCursor,
                name: "release-gate".into(),
                description: "Release checklist".into(),
                trigger_mode: RuleTriggerMode::Manual,
                globs: vec![],
                enabled: true,
                body: "Run full workspace verification before release.".into(),
                expected_etag: None,
            },
        )
        .expect("upsert project cursor");

        let discovered = discover_path_rules(&orbit_home, Some(&repo));
        assert_eq!(discovered.rules.len(), 4);
        assert!(!discovered.token_budget_warning);

        // Case 1: Rust source file -> Always + Glob + AgentDecided index (NOT Manual)
        let res1 = match_and_build_rules_injection(
            &discovered.rules,
            &["crates/orbit-daemon/src/rules.rs".into()],
            &[],
            PATH_RULES_HARD_CAP_TOKENS,
        );
        let names1: Vec<&str> = res1.matched.iter().map(|m| m.name.as_str()).collect();
        assert!(names1.contains(&"global-style"));
        assert!(names1.contains(&"rust-strict"));
        assert!(names1.contains(&"db-migrations"));
        assert!(!names1.contains(&"release-gate"));

        // Case 2: Negated test file + @rule:release-gate -> Manual + Always + AgentDecided
        let res2 = match_and_build_rules_injection(
            &discovered.rules,
            &["crates/orbit-daemon/tests/foo.rs".into()],
            &["release-gate".into()],
            PATH_RULES_HARD_CAP_TOKENS,
        );
        let names2: Vec<&str> = res2.matched.iter().map(|m| m.name.as_str()).collect();
        assert!(names2.contains(&"release-gate"));
        assert!(names2.contains(&"global-style"));
        assert!(names2.contains(&"db-migrations"));
        assert!(!names2.contains(&"rust-strict"));

        // Case 3: Token budget hard cap enforcement
        let big_body = "word ".repeat(20_000);
        upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: RuleScope::ProjectOrbit,
                name: "huge-always".into(),
                description: "Large rule".into(),
                trigger_mode: RuleTriggerMode::Always,
                globs: vec![],
                enabled: true,
                body: big_body,
                expected_etag: None,
            },
        )
        .expect("upsert huge rule");

        let discovered_big = discover_path_rules(&orbit_home, Some(&repo));
        assert!(discovered_big.token_budget_warning);
        let capped = match_and_build_rules_injection(
            &discovered_big.rules,
            &[],
            &[],
            PATH_RULES_HARD_CAP_TOKENS,
        );
        assert!(capped.total_tokens <= PATH_RULES_HARD_CAP_TOKENS);
        assert!(
            capped
                .injected_markdown
                .contains("omitted due to token budget cap")
        );
    }

    #[test]
    fn path_traversal_and_unsafe_mutations_are_rejected() {
        let dir = tempdir().expect("tempdir");
        let orbit_home = dir.path().join("orbit_home");
        let repo = dir.path().join("repo");
        fs::create_dir_all(&orbit_home).expect("mkdir orbit_home");
        fs::create_dir_all(&repo).expect("mkdir repo");

        let secret = repo.join("README.md");
        fs::write(&secret, "do not delete").expect("write secret");

        for bad_name in [
            "../evil",
            "../../etc/passwd",
            "a/../b",
            "/leading",
            "trailing/",
            "back\\slash",
            ".hidden",
            "a/.hidden",
            "bad.txt",
            "bad.exe",
            "   ",
            "foo\0bar",
            "a/b/c/d/e", // exceeds MAX_SCAN_DEPTH
        ] {
            let err = upsert_path_rule(
                &orbit_home,
                Some(&repo),
                &UpsertRuleSpec {
                    scope: RuleScope::ProjectOrbit,
                    name: bad_name.into(),
                    description: String::new(),
                    trigger_mode: RuleTriggerMode::Always,
                    globs: vec![],
                    enabled: true,
                    body: "bad".into(),
                    expected_etag: None,
                },
            );
            assert!(err.is_err(), "expected upsert rejection for `{bad_name}`");
        }

        assert!(
            validate_rule_filename("style.md", RuleScope::ProjectCursor).is_err(),
            "cursor rule should reject .md"
        );
        assert_eq!(
            validate_rule_filename("style.mdc", RuleScope::ProjectCursor)
                .expect("valid mdc")
                .filename,
            "style.mdc"
        );

        assert!(delete_path_rule(&orbit_home, Some(&repo), &secret.to_string_lossy()).is_err());
        assert!(delete_path_rule(&orbit_home, Some(&repo), "README.md").is_err());
        assert!(
            delete_path_rule(&orbit_home, Some(&repo), ".orbit/rules/../../README.md").is_err()
        );
        assert!(secret.exists());

        // When cwd is None (UserGlobal-only), project rule creation & deletion MUST fail
        assert!(
            upsert_path_rule(
                &orbit_home,
                None,
                &UpsertRuleSpec {
                    scope: RuleScope::ProjectOrbit,
                    name: "proj-rule".into(),
                    description: String::new(),
                    trigger_mode: RuleTriggerMode::Always,
                    globs: vec![],
                    enabled: true,
                    body: "Body".into(),
                    expected_etag: None,
                },
            )
            .is_err()
        );

        let created = upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: RuleScope::ProjectOrbit,
                name: "temp-rule".into(),
                description: "Temporary".into(),
                trigger_mode: RuleTriggerMode::Manual,
                globs: vec![],
                enabled: true,
                body: "Body".into(),
                expected_etag: None,
            },
        )
        .expect("create temp-rule");
        assert_eq!(created.rules.len(), 1);
        // Deleting a project rule when cwd is None MUST fail
        assert!(delete_path_rule(&orbit_home, None, &created.rules[0].path).is_err());
        // Deleting with matching cwd succeeds
        let after_delete = delete_path_rule(&orbit_home, Some(&repo), &created.rules[0].path)
            .expect("delete temp-rule");
        assert!(after_delete.rules.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_rules_dir_and_files_cannot_ingest_or_delete_outside_files() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().expect("tempdir");
        let orbit_home = dir.path().join("orbit_home");
        let repo = dir.path().join("repo");
        fs::create_dir_all(&orbit_home).expect("mkdir orbit_home");
        fs::create_dir_all(repo.join(".orbit")).expect("mkdir .orbit");

        let readme = repo.join("README.md");
        fs::write(&readme, "# Repo README\nDo not ingest or delete.\n").expect("write README");

        // 1. Symlink `.orbit/rules -> ..` (pointing back to repo root containing README.md)
        symlink(&repo, repo.join(".orbit/rules")).expect("symlink rules dir");

        let discovered = discover_path_rules(&orbit_home, Some(&repo));
        assert!(
            discovered.rules.is_empty(),
            "symlinked .orbit/rules must not ingest README.md"
        );
        assert!(
            delete_path_rule(&orbit_home, Some(&repo), &readme.to_string_lossy()).is_err(),
            "symlinked .orbit/rules must not allow deleting README.md"
        );
        assert!(
            delete_path_rule(&orbit_home, Some(&repo), ".orbit/rules/README.md").is_err(),
            "relative path through symlinked .orbit/rules must be rejected"
        );
        assert!(readme.exists());

        // 2. Real `.claude/rules` directory containing a symlinked `.md` file & symlinked subdir
        let claude_rules = repo.join(".claude/rules");
        fs::create_dir_all(&claude_rules).expect("mkdir claude rules");
        symlink(&readme, claude_rules.join("linked.md")).expect("symlink md file");
        symlink(&repo, claude_rules.join("escape")).expect("symlink subdir");

        let discovered2 = discover_path_rules(&orbit_home, Some(&repo));
        assert!(
            discovered2.rules.is_empty(),
            "symlinked rule files & subdirs must be skipped during discovery"
        );
        assert!(
            delete_path_rule(
                &orbit_home,
                Some(&repo),
                &claude_rules.join("linked.md").to_string_lossy()
            )
            .is_err()
        );
        assert!(
            delete_path_rule(
                &orbit_home,
                Some(&repo),
                &claude_rules.join("escape/README.md").to_string_lossy()
            )
            .is_err()
        );
        assert!(readme.exists());
    }

    #[test]
    fn etag_prevents_silent_overwrite_and_deleted_file_recreation() {
        let dir = tempdir().expect("tempdir");
        let orbit_home = dir.path().join("orbit_home");
        let repo = dir.path().join("repo");
        fs::create_dir_all(&orbit_home).expect("mkdir orbit_home");
        fs::create_dir_all(&repo).expect("mkdir repo");

        let created = upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: RuleScope::ProjectOrbit,
                name: "api-rules".into(),
                description: "v1".into(),
                trigger_mode: RuleTriggerMode::Always,
                globs: vec![],
                enabled: true,
                body: "Version 1".into(),
                expected_etag: None,
            },
        )
        .expect("create v1");
        let etag_v1 = created.rules[0].etag.clone();
        let rule_path = created.rules[0].path.clone();

        // 1. Creating again with expected_etag: None MUST fail (no silent overwrite)
        let dup_err = upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: RuleScope::ProjectOrbit,
                name: "api-rules".into(),
                description: "overwrite attempt".into(),
                trigger_mode: RuleTriggerMode::Always,
                globs: vec![],
                enabled: true,
                body: "Clobbered".into(),
                expected_etag: None,
            },
        );
        assert!(
            dup_err.is_err(),
            "expected_etag: None must refuse overwrite"
        );

        // 2. Updating with valid etag succeeds
        let updated = upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: RuleScope::ProjectOrbit,
                name: "api-rules".into(),
                description: "v2".into(),
                trigger_mode: RuleTriggerMode::Always,
                globs: vec![],
                enabled: true,
                body: "Version 2".into(),
                expected_etag: Some(etag_v1.clone()),
            },
        )
        .expect("update v2");
        let etag_v2 = updated.rules[0].etag.clone();
        assert_ne!(etag_v1, etag_v2);

        // 3. Updating with stale etag fails
        assert!(
            upsert_path_rule(
                &orbit_home,
                Some(&repo),
                &UpsertRuleSpec {
                    scope: RuleScope::ProjectOrbit,
                    name: "api-rules".into(),
                    description: "stale".into(),
                    trigger_mode: RuleTriggerMode::Always,
                    globs: vec![],
                    enabled: true,
                    body: "Stale".into(),
                    expected_etag: Some(etag_v1),
                },
            )
            .is_err()
        );

        // 4. Delete on disk, then attempt update with previous etag MUST fail (no silent recreate)
        fs::remove_file(&rule_path).expect("remove on disk");
        let deleted_err = upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: RuleScope::ProjectOrbit,
                name: "api-rules".into(),
                description: "recreate".into(),
                trigger_mode: RuleTriggerMode::Always,
                globs: vec![],
                enabled: true,
                body: "Recreated".into(),
                expected_etag: Some(etag_v2),
            },
        );
        assert!(
            deleted_err.is_err(),
            "expected_etag: Some(...) must fail if file was deleted on disk"
        );
        assert!(!Path::new(&rule_path).exists());
    }

    #[test]
    fn user_global_rules_are_not_duplicated_when_cwd_is_home_or_none() {
        let dir = tempdir().expect("tempdir");
        let fake_home = dir.path().join("home");
        let orbit_home = fake_home.join(".orbit");
        fs::create_dir_all(&orbit_home).expect("mkdir orbit_home");

        upsert_path_rule(
            &orbit_home,
            None,
            &UpsertRuleSpec {
                scope: RuleScope::UserGlobal,
                name: "global-only".into(),
                description: "Global rule".into(),
                trigger_mode: RuleTriggerMode::Always,
                globs: vec![],
                enabled: true,
                body: "Global body".into(),
                expected_etag: None,
            },
        )
        .expect("create global rule");

        // cwd = None -> 1 rule
        let snap_none = discover_path_rules(&orbit_home, None);
        assert_eq!(snap_none.rules.len(), 1);
        assert_eq!(snap_none.rules[0].scope, RuleScope::UserGlobal);

        // Even if cwd happens to equal `$HOME` (where `<cwd>/.orbit/rules == <orbit_home>/rules`),
        // canonical dir deduplication prevents returning the same rule twice!
        let snap_home = discover_path_rules(&orbit_home, Some(&fake_home));
        assert_eq!(snap_home.rules.len(), 1);
    }

    #[test]
    fn nested_subdirectory_rules_support_discover_toggle_edit_and_delete() {
        let dir = tempdir().expect("tempdir");
        let orbit_home = dir.path().join("orbit_home");
        let repo = dir.path().join("repo");
        fs::create_dir_all(&orbit_home).expect("mkdir orbit_home");
        fs::create_dir_all(repo.join(".claude/rules/backend")).expect("mkdir nested claude dir");

        let sql_path = repo.join(".claude/rules/backend/sql.md");
        fs::write(
            &sql_path,
            "---\npaths:\n  - \"**/*.sql\"\n---\n\nAlways parameterize queries.\n",
        )
        .expect("write sql.md");

        let snap1 = discover_path_rules(&orbit_home, Some(&repo));
        assert_eq!(snap1.rules.len(), 1);
        let rule = &snap1.rules[0];
        assert_eq!(rule.name, "backend/sql");
        assert_eq!(rule.relative_path, ".claude/rules/backend/sql.md");
        assert!(rule.enabled);

        // Toggle disabled via upsert with nested name + etag
        let snap2 = upsert_path_rule(
            &orbit_home,
            Some(&repo),
            &UpsertRuleSpec {
                scope: rule.scope,
                name: rule.name.clone(),
                description: "SQL safety".into(),
                trigger_mode: rule.trigger_mode,
                globs: rule.globs.clone(),
                enabled: false,
                body: rule.body.clone(),
                expected_etag: Some(rule.etag.clone()),
            },
        )
        .expect("toggle nested rule");
        assert_eq!(snap2.rules.len(), 1);
        assert!(!snap2.rules[0].enabled);
        assert_eq!(snap2.rules[0].description, "SQL safety");

        // Delete nested rule cleanly
        let snap3 = delete_path_rule(&orbit_home, Some(&repo), &snap2.rules[0].path)
            .expect("delete nested");
        assert!(snap3.rules.is_empty());
        assert!(!sql_path.exists());
    }

    #[test]
    fn extract_prompt_rule_signals_parses_badges_files_and_manual_rules() {
        let prompt = "Please check @crates/orbit-daemon/src/rules.rs and apply @rule:rust-strict and @rule:release-gate.\nReferenced workspace files: `@apps/desktop/src/App.tsx`";
        let badges = vec![PromptBadge {
            kind: "quote".into(),
            label: "lib.rs:42".into(),
            path: Some("/repo/crates/orbit-protocol/src/lib.rs:42".into()),
        }];
        let (paths, manual) = extract_prompt_rule_signals(
            prompt,
            &badges,
            "/repo",
            &["crates/orbit-daemon/src/manager.rs".into()],
        );
        assert!(paths.contains(&"crates/orbit-protocol/src/lib.rs".to_string()));
        assert!(paths.contains(&"crates/orbit-daemon/src/manager.rs".to_string()));
        assert!(paths.contains(&"crates/orbit-daemon/src/rules.rs".to_string()));
        assert!(paths.contains(&"apps/desktop/src/App.tsx".to_string()));
        assert_eq!(
            manual,
            vec!["rust-strict".to_string(), "release-gate".to_string()]
        );
    }

    #[test]
    fn match_rules_for_workspace_dedup_suppresses_unchanged_rules_on_subsequent_turns() {
        let dir = tempdir().expect("tempdir");
        let orbit_home = dir.path().join("orbit_home");
        let repo = dir.path().join("repo");
        fs::create_dir_all(&orbit_home).expect("mkdir orbit_home");
        fs::create_dir_all(repo.join(".orbit/rules")).expect("mkdir rules");

        let rule_path = repo.join(".orbit/rules/rust-style.md");
        fs::write(
            &rule_path,
            "---\ndescription: Rust rules\npaths:\n  - \"src/**/*.rs\"\n---\n\nNever use unwrap.\n",
        )
        .expect("write rule");

        let mut seen = std::collections::HashMap::new();
        let active = vec!["src/lib.rs".to_string()];

        // Turn 1: injects rule
        let r1 = match_rules_for_workspace_dedup(&orbit_home, Some(&repo), &active, &[], &mut seen);
        assert!(r1.injected_markdown.contains("Never use unwrap."));
        assert_eq!(seen.len(), 1);

        // Turn 2 with same edited file: 0 bytes re-injected
        let r2 = match_rules_for_workspace_dedup(&orbit_home, Some(&repo), &active, &[], &mut seen);
        assert!(r2.injected_markdown.is_empty());

        // Turn 3 if user explicitly requests @rule:rust-style -> re-injected on demand
        let r3 = match_rules_for_workspace_dedup(
            &orbit_home,
            Some(&repo),
            &active,
            &["rust-style".to_string()],
            &mut seen,
        );
        assert!(r3.injected_markdown.contains("Never use unwrap."));

        // Turn 4 if rule content changes on disk -> automatically re-injected
        fs::write(
            &rule_path,
            "---\ndescription: Rust rules\npaths:\n  - \"src/**/*.rs\"\n---\n\nNever use unwrap or expect in prod.\n",
        )
        .expect("update rule");
        let r4 = match_rules_for_workspace_dedup(&orbit_home, Some(&repo), &active, &[], &mut seen);
        assert!(
            r4.injected_markdown
                .contains("Never use unwrap or expect in prod.")
        );
    }
}
