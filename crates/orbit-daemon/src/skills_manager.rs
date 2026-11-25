//! Pure-Rust Skills Ecosystem Manager:
//! - Multi-source `SKILL.md` discovery across Orbit, Claude Code, Codex, Antigravity, and Built-in skills
//! - Zero-dependency YAML frontmatter parser (`name`, `description`, `triggers`, `allowed-tools`)
//! - Atomic enabled/disabled state persistence in `$ORBIT_HOME/skills-state.json`
//! - Security-hardened CRUD (`OrbitUser` / `OrbitProject`) with strict slug validation,
//!   canonical path containment, symlink traversal prevention, optimistic `etag` conflict
//!   detection, serialized file mutations, and safe Markdown/path import.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::UNIX_EPOCH;

pub use orbit_protocol::expand_command_arguments;
use orbit_protocol::{ManagedSkillRecord, SkillSource, SkillWriteScope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::skills::BUNDLED_SKILLS;
use crate::workspace_files::{canonicalize_workspace_root, sha256_hex};

/// Maximum permitted `SKILL.md` size (512 KiB).
pub const MAX_SKILL_CONTENT_BYTES: usize = 512 * 1024;

const SKILLS_STATE_FILE: &str = "skills-state.json";
const ORBIT_MANAGED_MARKER: &str = ".orbit-managed.json";

/// Process-wide lock serializing read-modify-write mutations to `skills-state.json` and `SKILL.md`.
static SKILLS_IO_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn lock_skills_io() -> Result<MutexGuard<'static, ()>, SkillManagerError> {
    SKILLS_IO_LOCK
        .lock()
        .map_err(|_| SkillManagerError::Io("skills manager I/O lock poisoned".to_string()))
}

/// Typed error for skill manager operations so RPC handlers return actionable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillManagerError {
    InvalidInput(String),
    NotFound(String),
    PermissionDenied(String),
    Conflict {
        expected: String,
        actual: String,
    },
    /// A create-only write (`SkillWritePrecondition::MustNotExist`) targeted a slug that already
    /// exists. Reported separately from `Conflict` so clients can offer rename / open / replace
    /// instead of a stale-etag reload prompt.
    AlreadyExists(String),
    TooLarge(String),
    Io(String),
}

impl SkillManagerError {
    pub const fn rpc_code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_skill_input",
            Self::NotFound(_) => "skill_not_found",
            Self::PermissionDenied(_) => "skill_permission_denied",
            Self::Conflict { .. } => "skill_etag_conflict",
            Self::AlreadyExists(_) => "skill_already_exists",
            Self::TooLarge(_) => "skill_too_large",
            Self::Io(_) => "skill_io_error",
        }
    }
}

impl std::fmt::Display for SkillManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(msg)
            | Self::NotFound(msg)
            | Self::PermissionDenied(msg)
            | Self::AlreadyExists(msg)
            | Self::TooLarge(msg)
            | Self::Io(msg) => f.write_str(msg),
            Self::Conflict { expected, actual } => write!(
                f,
                "skill was modified concurrently (expected etag {expected}, found {actual})"
            ),
        }
    }
}

impl std::error::Error for SkillManagerError {}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SkillsStateFile {
    #[serde(default)]
    enabled_overrides: BTreeMap<String, bool>,
}

#[derive(Debug, Clone)]
pub struct ReadSkillOutcome {
    pub skill_id: String,
    pub content: String,
    pub etag: String,
}

/// Write intent for [`upsert_managed_skill`], making "create a new skill" and "modify an existing
/// one" impossible to confuse at the type level.
///
/// The former `Option<String>` etag field could not express create-only intent: a `None` etag meant
/// "skip the conflict check", so every caller that omitted an etag silently overwrote any existing
/// `SKILL.md` at the same slug.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SkillWritePrecondition {
    /// Create-only: reject with [`SkillManagerError::AlreadyExists`] if the target already exists.
    MustNotExist,
    /// Optimistic update: the on-disk content hash must equal this etag.
    MustMatch(String),
    /// No precondition — blind upsert. Preserved as the default so existing non-UI callers keep
    /// their original behavior.
    #[default]
    Unchecked,
}

impl SkillWritePrecondition {
    /// Resolve the wire representation (`create_only` flag plus optional etag) into a precondition.
    ///
    /// `create_only` wins over an etag so a create request can never degrade into a blind write;
    /// a blank or whitespace-only etag is treated as absent, matching the previous filter.
    pub fn from_wire(create_only: bool, expected_etag: Option<String>) -> Self {
        if create_only {
            return Self::MustNotExist;
        }
        match expected_etag
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(etag) => Self::MustMatch(etag.to_string()),
            None => Self::Unchecked,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpsertSkillParams {
    pub scope: SkillWriteScope,
    pub cwd: Option<String>,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub enabled: bool,
    pub precondition: SkillWritePrecondition,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedSkillFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub triggers: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub has_frontmatter: bool,
}

/// Strictly validate a skill slug against `^[a-z0-9][a-z0-9_-]{0,63}$`.
pub fn validate_skill_slug(raw: &str) -> Result<String, SkillManagerError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SkillManagerError::InvalidInput(
            "skill slug must not be empty".to_string(),
        ));
    }
    if trimmed.len() > 64 {
        return Err(SkillManagerError::InvalidInput(
            "skill slug must be at most 64 characters".to_string(),
        ));
    }
    if trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
    {
        return Err(SkillManagerError::InvalidInput(format!(
            "skill slug `{trimmed}` contains forbidden path characters"
        )));
    }
    let bytes = trimmed.as_bytes();
    let first = bytes[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(SkillManagerError::InvalidInput(format!(
            "skill slug `{trimmed}` must start with a lowercase letter or digit"
        )));
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'_')
    {
        return Err(SkillManagerError::InvalidInput(format!(
            "skill slug `{trimmed}` may only contain lowercase letters, digits, `-`, and `_`"
        )));
    }
    Ok(trimmed.to_string())
}

fn slugify_label(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            out.push(lower);
            prev_dash = false;
        } else if (lower == '-' || lower == '_' || ch.is_whitespace())
            && !out.is_empty()
            && !prev_dash
        {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 64 {
            break;
        }
    }
    while out.ends_with('-') || out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        "imported-skill".to_string()
    } else {
        out
    }
}

fn title_case_slug(slug: &str) -> String {
    slug.split(['-', '_', ':'])
        .filter(|s| !s.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = first.to_uppercase().to_string();
                    word.push_str(chars.as_str());
                    word
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn skill_id_for(source: SkillSource, slug: &str) -> String {
    format!("{}:{slug}", source.as_str())
}

fn command_id_for(source: SkillSource, slug: &str) -> String {
    format!("{}:cmd:{slug}", source.as_str())
}

fn source_priority(source: SkillSource) -> u8 {
    match source {
        SkillSource::OrbitProject => 0,
        SkillSource::OrbitUser => 1,
        SkillSource::ClaudeProject => 2,
        SkillSource::ClaudeUser => 3,
        SkillSource::CodexUser => 4,
        SkillSource::AntigravityUser => 5,
        SkillSource::Builtin => 6,
    }
}

fn approx_tokens_for(content: &str) -> u32 {
    if content.is_empty() {
        return 0;
    }
    let est = content.len().saturating_add(3) / 4;
    u32::try_from(est.max(1)).unwrap_or(u32::MAX)
}

fn mtime_ms(path: &Path) -> u64 {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

fn strip_yaml_quotes(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
    }
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        // YAML single-quoted scalars treat backslashes literally and escape `'` as `''`.
        return trimmed[1..trimmed.len() - 1].replace("''", "'");
    }
    trimmed.to_string()
}

fn parse_inline_or_csv_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let inner = if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    inner
        .split(',')
        .map(strip_yaml_quotes)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Pure-Rust YAML frontmatter parser tailored for `SKILL.md` metadata.
pub fn parse_skill_frontmatter(content: &str) -> (ParsedSkillFrontmatter, &str) {
    let normalized_start = content.strip_prefix('\u{feff}').unwrap_or(content);
    let rest_after_open = if let Some(rest) = normalized_start.strip_prefix("---\n") {
        rest
    } else if let Some(rest) = normalized_start.strip_prefix("---\r\n") {
        rest
    } else {
        return (ParsedSkillFrontmatter::default(), normalized_start);
    };

    // Locate closing `---` line
    let mut offset = 0usize;
    let mut fm_end = None;
    let mut body_start = None;
    for line in rest_after_open.split_inclusive('\n') {
        let stripped = line.trim_end_matches(['\r', '\n']);
        if stripped == "---" || stripped == "..." {
            fm_end = Some(offset);
            body_start = Some(offset + line.len());
            break;
        }
        offset += line.len();
    }

    let (Some(end), Some(b_start)) = (fm_end, body_start) else {
        return (ParsedSkillFrontmatter::default(), normalized_start);
    };

    let fm_block = &rest_after_open[..end];
    let body = &rest_after_open[b_start..];
    let mut parsed = ParsedSkillFrontmatter {
        has_frontmatter: true,
        ..Default::default()
    };

    let lines: Vec<&str> = fm_block.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut idx = 0usize;
    while idx < lines.len() {
        let line = lines[idx];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            idx += 1;
            continue;
        }
        // Only handle top-level key: value pairs (no leading indentation)
        if line.starts_with(' ') || line.starts_with('\t') {
            idx += 1;
            continue;
        }
        let Some((raw_key, raw_val)) = line.split_once(':') else {
            idx += 1;
            continue;
        };
        let key = raw_key.trim().to_ascii_lowercase();
        let val = raw_val.trim();

        match key.as_str() {
            "name" | "description" | "argument-hint" | "argument_hint" => {
                let value = if matches!(val, ">" | ">-" | "|" | "|-") {
                    let is_folded = val.starts_with('>');
                    let mut block_lines = Vec::new();
                    let mut j = idx + 1;
                    while j < lines.len() {
                        let next = lines[j];
                        if next.starts_with(' ') || next.starts_with('\t') {
                            block_lines.push(next.trim());
                            j += 1;
                        } else if next.trim().is_empty() {
                            block_lines.push("");
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    idx = j.saturating_sub(1);
                    if is_folded {
                        block_lines
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(" ")
                    } else {
                        block_lines.join("\n").trim().to_string()
                    }
                } else if !val.is_empty() {
                    strip_yaml_quotes(val)
                } else {
                    String::new()
                };

                if !value.is_empty() {
                    if key == "name" {
                        parsed.name = Some(value);
                    } else if key == "description" {
                        parsed.description = Some(value);
                    } else {
                        parsed.argument_hint = Some(value);
                    }
                }
            }
            "triggers" | "allowed-tools" | "allowed_tools" => {
                let mut items = Vec::new();
                if !val.is_empty() {
                    items.extend(parse_inline_or_csv_list(val));
                } else {
                    let mut j = idx + 1;
                    while j < lines.len() {
                        let next = lines[j];
                        let t = next.trim();
                        if t.is_empty() {
                            j += 1;
                            continue;
                        }
                        if (next.starts_with(' ') || next.starts_with('\t') || t.starts_with("- "))
                            && let Some(item) = t.strip_prefix("- ")
                        {
                            let cleaned = strip_yaml_quotes(item);
                            if !cleaned.is_empty() {
                                items.push(cleaned);
                            }
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    idx = j.saturating_sub(1);
                }
                if key == "triggers" {
                    parsed.triggers = items;
                } else {
                    parsed.allowed_tools = items;
                }
            }
            _ => {}
        }
        idx += 1;
    }

    (parsed, body)
}

fn extract_fallback_heading_and_desc(body: &str) -> (Option<String>, String) {
    let mut heading: Option<String> = None;
    let mut desc = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(h) = trimmed.strip_prefix('#') {
            let title = h.trim_start_matches('#').trim();
            if heading.is_none() && !title.is_empty() {
                heading = Some(title.to_string());
            }
            continue;
        }
        if desc.is_empty() && !trimmed.starts_with("```") {
            let truncated: String = trimmed.chars().take(200).collect();
            desc = truncated;
            break;
        }
    }
    (heading, desc)
}

fn escape_yaml_scalar(raw: &str) -> String {
    let single_line = raw.replace(['\r', '\n'], " ").trim().to_string();
    let escaped = single_line.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Synthesize valid YAML frontmatter if `content` does not already have a complete frontmatter block.
pub fn ensure_skill_frontmatter(name: &str, description: &str, content: &str) -> String {
    let trimmed = content.strip_prefix('\u{feff}').unwrap_or(content);
    if parse_skill_frontmatter(trimmed).0.has_frontmatter {
        return trimmed.to_string();
    }
    let clean_name = name.trim();
    let clean_desc = description.trim();
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", escape_yaml_scalar(clean_name)));
    out.push_str(&format!(
        "description: {}\n",
        escape_yaml_scalar(clean_desc)
    ));
    out.push_str("---\n\n");
    out.push_str(trimmed.trim_start_matches(['\r', '\n']));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn load_skills_state(orbit_home: &Path) -> SkillsStateFile {
    let path = orbit_home.join(SKILLS_STATE_FILE);
    let Ok(raw) = fs::read_to_string(&path) else {
        return SkillsStateFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn atomic_write_bytes(target_path: &Path, bytes: &[u8]) -> Result<(), SkillManagerError> {
    let parent = target_path.parent().ok_or_else(|| {
        SkillManagerError::Io(format!("invalid target path: {}", target_path.display()))
    })?;
    fs::create_dir_all(parent)
        .map_err(|e| SkillManagerError::Io(format!("create directory failed: {e}")))?;

    let tmp_name = format!(
        ".{}.{}.tmp",
        target_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("orbit"),
        Uuid::new_v4().simple()
    );
    let tmp_path = parent.join(tmp_name);

    let write_res = (|| -> std::io::Result<()> {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(SkillManagerError::Io(format!(
            "atomic write failed for {}: {e}",
            target_path.display()
        )));
    }

    if let Err(e) = fs::rename(&tmp_path, target_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(SkillManagerError::Io(format!(
            "atomic rename failed for {}: {e}",
            target_path.display()
        )));
    }
    Ok(())
}

fn save_skills_state(orbit_home: &Path, state: &SkillsStateFile) -> Result<(), SkillManagerError> {
    let serialized = serde_json::to_vec_pretty(state)
        .map_err(|e| SkillManagerError::Io(format!("serialize skills state failed: {e}")))?;
    atomic_write_bytes(&orbit_home.join(SKILLS_STATE_FILE), &serialized)
}

fn validate_project_cwd(cwd: &str, user_home: &Path) -> Result<PathBuf, SkillManagerError> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() || trimmed == "~" {
        return Err(SkillManagerError::InvalidInput(
            "project-scoped skills require an active workspace cwd".to_string(),
        ));
    }
    let cwd_path = PathBuf::from(trimmed);
    let canonical_cwd = canonicalize_workspace_root(&cwd_path).map_err(|msg| {
        if msg.contains("Refusing") || msg.contains("forbidden") {
            SkillManagerError::PermissionDenied(msg)
        } else {
            SkillManagerError::InvalidInput(msg)
        }
    })?;
    if let Ok(canon_user_home) = crate::platform::canonicalize_clean(user_home)
        && canonical_cwd == canon_user_home
    {
        return Err(SkillManagerError::PermissionDenied(
            "refusing to use home directory root as project workspace".to_string(),
        ));
    }
    Ok(canonical_cwd)
}

fn resolve_optional_cwd(cwd: Option<&str>, user_home: &Path) -> Option<PathBuf> {
    let raw = cwd?.trim();
    if raw.is_empty() || raw == "~" {
        return None;
    }
    validate_project_cwd(raw, user_home).ok()
}

fn orbit_user_skills_root(orbit_home: &Path) -> PathBuf {
    orbit_home.join("skills")
}

/// Walk `segments` downward from `canonical_boundary`, verifying via `symlink_metadata` that no
/// segment is a symlink. When `create_missing` is `true`, creates missing segments one at a time.
fn walk_contained_dir_segments(
    canonical_boundary: &Path,
    segments: &[&str],
    create_missing: bool,
) -> Result<PathBuf, SkillManagerError> {
    let mut current = canonical_boundary.to_path_buf();
    for seg in segments {
        let next = current.join(seg);
        match fs::symlink_metadata(&next) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(SkillManagerError::PermissionDenied(format!(
                        "refusing to traverse symlinked path segment `{seg}`"
                    )));
                }
                if !meta.is_dir() {
                    return Err(SkillManagerError::InvalidInput(format!(
                        "expected directory at `{}`",
                        next.display()
                    )));
                }
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                if create_missing {
                    fs::create_dir(&next).map_err(|err| {
                        SkillManagerError::Io(format!(
                            "create directory `{}` failed: {err}",
                            next.display()
                        ))
                    })?;
                } else {
                    return Err(SkillManagerError::NotFound(format!(
                        "directory `{}` does not exist",
                        next.display()
                    )));
                }
            }
            Err(e) => {
                return Err(SkillManagerError::Io(format!(
                    "inspect `{}` failed: {e}",
                    next.display()
                )));
            }
        }
        current = next;
    }
    let canonical_leaf = crate::platform::canonicalize_clean(&current).map_err(|e| {
        SkillManagerError::Io(format!("canonicalize `{}` failed: {e}", current.display()))
    })?;
    if !canonical_leaf.starts_with(canonical_boundary) {
        return Err(SkillManagerError::PermissionDenied(
            "resolved directory escapes allowed boundary".to_string(),
        ));
    }
    Ok(canonical_leaf)
}

struct DiscoveryRootSpec {
    source: SkillSource,
    boundary: PathBuf,
    segments: &'static [&'static str],
}

fn discovery_root_specs(
    orbit_home: &Path,
    user_home: &Path,
    cwd: Option<&Path>,
) -> Vec<DiscoveryRootSpec> {
    let mut specs = Vec::new();
    if let Some(project_root) = cwd {
        specs.push(DiscoveryRootSpec {
            source: SkillSource::OrbitProject,
            boundary: project_root.to_path_buf(),
            segments: &[".orbit", "skills"],
        });
    }
    specs.push(DiscoveryRootSpec {
        source: SkillSource::OrbitUser,
        boundary: orbit_home.to_path_buf(),
        segments: &["skills"],
    });
    let home_orbit_skills = user_home.join(".orbit").join("skills");
    if home_orbit_skills != orbit_user_skills_root(orbit_home) {
        specs.push(DiscoveryRootSpec {
            source: SkillSource::OrbitUser,
            boundary: user_home.to_path_buf(),
            segments: &[".orbit", "skills"],
        });
    }
    if let Some(project_root) = cwd {
        specs.push(DiscoveryRootSpec {
            source: SkillSource::ClaudeProject,
            boundary: project_root.to_path_buf(),
            segments: &[".claude", "skills"],
        });
    }
    specs.push(DiscoveryRootSpec {
        source: SkillSource::ClaudeUser,
        boundary: user_home.to_path_buf(),
        segments: &[".claude", "skills"],
    });
    specs.push(DiscoveryRootSpec {
        source: SkillSource::CodexUser,
        boundary: user_home.to_path_buf(),
        segments: &[".codex", "skills"],
    });
    specs.push(DiscoveryRootSpec {
        source: SkillSource::AntigravityUser,
        boundary: user_home.to_path_buf(),
        segments: &[".gemini", "config", "skills"],
    });
    specs.push(DiscoveryRootSpec {
        source: SkillSource::AntigravityUser,
        boundary: user_home.to_path_buf(),
        segments: &[".gemini", "antigravity", "skills"],
    });
    specs.push(DiscoveryRootSpec {
        source: SkillSource::AntigravityUser,
        boundary: user_home.to_path_buf(),
        segments: &[".gemini", "antigravity-cli", "skills"],
    });
    specs
}

struct SkillRecordInput<'a> {
    source: SkillSource,
    slug: &'a str,
    path: String,
    content: &'a str,
    has_scripts: bool,
    has_references: bool,
    updated_at_ms: u64,
    is_command: bool,
}

fn build_record_from_content(
    input: SkillRecordInput<'_>,
    state: &SkillsStateFile,
) -> ManagedSkillRecord {
    let (fm, body) = parse_skill_frontmatter(input.content);
    let (fallback_heading, fallback_desc) = extract_fallback_heading_and_desc(body);
    let name = fm
        .name
        .or(fallback_heading)
        .unwrap_or_else(|| title_case_slug(input.slug));
    let description = fm.description.unwrap_or(fallback_desc);
    let id = if input.is_command {
        command_id_for(input.source, input.slug)
    } else {
        skill_id_for(input.source, input.slug)
    };
    let enabled = state.enabled_overrides.get(&id).copied().unwrap_or(true);

    ManagedSkillRecord {
        id,
        slug: input.slug.to_string(),
        name,
        description,
        source: input.source,
        path: input.path,
        enabled,
        editable: !input.is_command && input.source.is_editable(),
        triggers: fm.triggers,
        allowed_tools: fm.allowed_tools,
        has_scripts: input.has_scripts,
        has_references: input.has_references,
        approx_tokens: approx_tokens_for(input.content),
        updated_at_ms: input.updated_at_ms,
        argument_hint: fm.argument_hint,
        is_command: input.is_command,
    }
}

const MAX_COMMAND_NAMESPACE_DEPTH: usize = 6;
const MAX_DISCOVERED_COMMANDS: usize = 256;

fn command_root_specs(user_home: &Path, cwd: Option<&Path>) -> Vec<DiscoveryRootSpec> {
    let mut specs = Vec::new();
    if let Some(project_root) = cwd {
        specs.push(DiscoveryRootSpec {
            source: SkillSource::ClaudeProject,
            boundary: project_root.to_path_buf(),
            segments: &[".claude", "commands"],
        });
    }
    specs.push(DiscoveryRootSpec {
        source: SkillSource::ClaudeUser,
        boundary: user_home.to_path_buf(),
        segments: &[".claude", "commands"],
    });
    specs
}

#[allow(clippy::too_many_arguments)]
fn collect_claude_commands_recursive(
    source: SkillSource,
    canonical_root: &Path,
    current_dir: &Path,
    namespace_parts: &mut Vec<String>,
    depth: usize,
    state: &SkillsStateFile,
    seen_canonical_paths: &mut HashSet<PathBuf>,
    seen_command_slugs: &mut HashSet<String>,
    records: &mut Vec<ManagedSkillRecord>,
    discovered_count: &mut usize,
) {
    if depth > MAX_COMMAND_NAMESPACE_DEPTH || *discovered_count >= MAX_DISCOVERED_COMMANDS {
        return;
    }
    let Ok(entries) = fs::read_dir(current_dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|entry| entry.path()))
        .collect();
    paths.sort();

    for entry_path in paths {
        if *discovered_count >= MAX_DISCOVERED_COMMANDS {
            break;
        }
        let Ok(meta) = fs::symlink_metadata(&entry_path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let Some(file_name) = entry_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_name.starts_with('.') {
            continue;
        }

        if meta.is_dir() {
            let Ok(seg) = validate_skill_slug(&file_name.to_ascii_lowercase()) else {
                continue;
            };
            let Ok(canonical_dir) = crate::platform::canonicalize_clean(&entry_path) else {
                continue;
            };
            if !canonical_dir.starts_with(canonical_root) {
                continue;
            }
            namespace_parts.push(seg);
            collect_claude_commands_recursive(
                source,
                canonical_root,
                &canonical_dir,
                namespace_parts,
                depth + 1,
                state,
                seen_canonical_paths,
                seen_command_slugs,
                records,
                discovered_count,
            );
            namespace_parts.pop();
            continue;
        }

        if !meta.is_file() {
            continue;
        }
        let is_md = entry_path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if !is_md {
            continue;
        }
        let Some(stem) = entry_path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(stem_slug) = validate_skill_slug(&stem.to_ascii_lowercase()) else {
            continue;
        };
        let slug = if namespace_parts.is_empty() {
            stem_slug
        } else {
            format!("{}:{stem_slug}", namespace_parts.join(":"))
        };
        if slug.len() > 64 {
            continue;
        }

        let Ok(canonical_md) = crate::platform::canonicalize_clean(&entry_path) else {
            continue;
        };
        if !canonical_md.starts_with(canonical_root) {
            continue;
        }
        if !seen_canonical_paths.insert(canonical_md.clone()) {
            continue;
        }
        if !seen_command_slugs.insert(slug.clone()) {
            continue;
        }
        if meta.len() > MAX_SKILL_CONTENT_BYTES as u64 {
            continue;
        }
        let Ok(content) = fs::read_to_string(&canonical_md) else {
            continue;
        };

        *discovered_count += 1;
        records.push(build_record_from_content(
            SkillRecordInput {
                source,
                slug: &slug,
                path: canonical_md.to_string_lossy().to_string(),
                content: &content,
                has_scripts: false,
                has_references: false,
                updated_at_ms: mtime_ms(&canonical_md),
                is_command: true,
            },
            state,
        ));
    }
}

/// Discover all skills across Orbit, Claude, Codex, Antigravity, and Built-in roots,
/// deduplicating cleanly by canonical path and slug precedence while rejecting symlink escapes.
pub fn list_managed_skills(
    orbit_home: &Path,
    user_home: &Path,
    cwd: Option<&str>,
) -> Vec<ManagedSkillRecord> {
    let resolved_cwd = resolve_optional_cwd(cwd, user_home);
    let state = load_skills_state(orbit_home);
    let mut records = Vec::new();
    let mut seen_canonical_paths: HashSet<PathBuf> = HashSet::new();
    let mut seen_slugs: HashSet<String> = HashSet::new();

    for spec in discovery_root_specs(orbit_home, user_home, resolved_cwd.as_deref()) {
        let Ok(canonical_boundary) = crate::platform::canonicalize_clean(&spec.boundary) else {
            continue;
        };
        let Ok(canonical_root) =
            walk_contained_dir_segments(&canonical_boundary, spec.segments, false)
        else {
            continue;
        };

        let Ok(entries) = fs::read_dir(&canonical_root) else {
            continue;
        };
        let mut dir_entries: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|entry| entry.path()))
            .collect();
        dir_entries.sort();

        for skill_dir in dir_entries {
            let Ok(dir_meta) = fs::symlink_metadata(&skill_dir) else {
                continue;
            };
            if dir_meta.file_type().is_symlink() || !dir_meta.is_dir() {
                continue;
            }
            let Some(dir_name) = skill_dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // Editable Orbit scopes require exact lowercase slug directory names.
            let slug = if spec.source.is_editable() {
                match validate_skill_slug(dir_name) {
                    Ok(s) => s,
                    Err(_) => continue,
                }
            } else {
                match validate_skill_slug(&dir_name.to_ascii_lowercase()) {
                    Ok(s) => s,
                    Err(_) => continue,
                }
            };

            let Ok(canonical_dir) = crate::platform::canonicalize_clean(&skill_dir) else {
                continue;
            };
            if canonical_dir.parent() != Some(canonical_root.as_path()) {
                continue;
            }

            // Skip bundled orchestration skills synced into external roots so Builtin isn't duplicated.
            if !spec.source.is_editable() && canonical_dir.join(ORBIT_MANAGED_MARKER).is_file() {
                continue;
            }

            let md_path = canonical_dir.join("SKILL.md");
            let Ok(md_meta) = fs::symlink_metadata(&md_path) else {
                continue;
            };
            if md_meta.file_type().is_symlink() || !md_meta.is_file() {
                continue;
            }

            let Ok(canonical_md) = crate::platform::canonicalize_clean(&md_path) else {
                continue;
            };
            if !canonical_md.starts_with(&canonical_root)
                || canonical_md.parent() != Some(canonical_dir.as_path())
            {
                continue;
            }
            if !seen_canonical_paths.insert(canonical_md.clone()) {
                continue;
            }
            if !seen_slugs.insert(slug.clone()) {
                continue;
            }

            if md_meta.len() > MAX_SKILL_CONTENT_BYTES as u64 {
                continue;
            }
            let Ok(content) = fs::read_to_string(&canonical_md) else {
                continue;
            };

            let has_scripts = fs::symlink_metadata(canonical_dir.join("scripts"))
                .is_ok_and(|m| !m.file_type().is_symlink() && m.is_dir());
            let has_references = fs::symlink_metadata(canonical_dir.join("references"))
                .is_ok_and(|m| !m.file_type().is_symlink() && m.is_dir());
            let updated_at_ms = mtime_ms(&canonical_md);

            records.push(build_record_from_content(
                SkillRecordInput {
                    source: spec.source,
                    slug: &slug,
                    path: canonical_md.to_string_lossy().to_string(),
                    content: &content,
                    has_scripts,
                    has_references,
                    updated_at_ms,
                    is_command: false,
                },
                &state,
            ));
        }
    }

    // Discover project `.claude/commands/**/*.md` and user `~/.claude/commands/**/*.md` custom slash commands
    let mut seen_command_slugs: HashSet<String> = HashSet::new();
    let mut discovered_commands = 0usize;
    for spec in command_root_specs(user_home, resolved_cwd.as_deref()) {
        let Ok(canonical_boundary) = crate::platform::canonicalize_clean(&spec.boundary) else {
            continue;
        };
        let Ok(canonical_root) =
            walk_contained_dir_segments(&canonical_boundary, spec.segments, false)
        else {
            continue;
        };
        let mut ns = Vec::new();
        collect_claude_commands_recursive(
            spec.source,
            &canonical_root,
            &canonical_root,
            &mut ns,
            0,
            &state,
            &mut seen_canonical_paths,
            &mut seen_command_slugs,
            &mut records,
            &mut discovered_commands,
        );
    }

    // Append bundled built-in Orbit orchestration skills unless shadowed by a higher-priority source
    for bundled in BUNDLED_SKILLS {
        if !seen_slugs.insert(bundled.id.to_string()) {
            continue;
        }
        records.push(build_record_from_content(
            SkillRecordInput {
                source: SkillSource::Builtin,
                slug: bundled.id,
                path: format!("builtin://skills/{}/SKILL.md", bundled.id),
                content: bundled.content,
                has_scripts: false,
                has_references: false,
                updated_at_ms: 0,
                is_command: false,
            },
            &state,
        ));
    }

    records.sort_by(|a, b| {
        source_priority(a.source)
            .cmp(&source_priority(b.source))
            .then_with(|| a.is_command.cmp(&b.is_command))
            .then_with(|| a.slug.cmp(&b.slug))
    });

    records
}

/// Read the raw UTF-8 `SKILL.md` content and SHA-256 `etag` for a discovered skill.
pub fn read_managed_skill(
    orbit_home: &Path,
    user_home: &Path,
    skill_id: &str,
    cwd: Option<&str>,
) -> Result<ReadSkillOutcome, SkillManagerError> {
    let skills = list_managed_skills(orbit_home, user_home, cwd);
    let record = skills
        .into_iter()
        .find(|s| s.id == skill_id)
        .ok_or_else(|| SkillManagerError::NotFound(format!("skill `{skill_id}` not found")))?;

    if record.source == SkillSource::Builtin {
        let bundled = BUNDLED_SKILLS
            .iter()
            .find(|b| b.id == record.slug)
            .ok_or_else(|| SkillManagerError::NotFound(format!("skill `{skill_id}` not found")))?;
        let etag = sha256_hex(bundled.content.as_bytes());
        return Ok(ReadSkillOutcome {
            skill_id: record.id,
            content: bundled.content.to_string(),
            etag,
        });
    }

    let path = PathBuf::from(&record.path);
    let sym_meta = fs::symlink_metadata(&path)
        .map_err(|e| SkillManagerError::Io(format!("stat skill failed: {e}")))?;
    if sym_meta.file_type().is_symlink() || !sym_meta.is_file() {
        return Err(SkillManagerError::PermissionDenied(
            "refusing to read symlinked SKILL.md".to_string(),
        ));
    }
    if sym_meta.len() > MAX_SKILL_CONTENT_BYTES as u64 {
        return Err(SkillManagerError::TooLarge(format!(
            "skill exceeds maximum allowed size of {} bytes",
            MAX_SKILL_CONTENT_BYTES
        )));
    }
    let bytes =
        fs::read(&path).map_err(|e| SkillManagerError::Io(format!("read skill failed: {e}")))?;
    let content = String::from_utf8(bytes)
        .map_err(|_| SkillManagerError::InvalidInput("SKILL.md is not valid UTF-8".to_string()))?;
    let etag = sha256_hex(content.as_bytes());
    Ok(ReadSkillOutcome {
        skill_id: record.id,
        content,
        etag,
    })
}

struct WriteRootTarget {
    source: SkillSource,
    canonical_boundary: PathBuf,
    root_segments: &'static [&'static str],
}

fn resolve_write_root(
    orbit_home: &Path,
    user_home: &Path,
    scope: SkillWriteScope,
    cwd: Option<&str>,
) -> Result<WriteRootTarget, SkillManagerError> {
    match scope {
        SkillWriteScope::User => {
            fs::create_dir_all(orbit_home).map_err(|e| {
                SkillManagerError::Io(format!("create orbit_home directory failed: {e}"))
            })?;
            let canonical_orbit_home =
                crate::platform::canonicalize_clean(orbit_home).map_err(|e| {
                    SkillManagerError::Io(format!("canonicalize orbit_home failed: {e}"))
                })?;
            Ok(WriteRootTarget {
                source: SkillSource::OrbitUser,
                canonical_boundary: canonical_orbit_home,
                root_segments: &["skills"],
            })
        }
        SkillWriteScope::Project => {
            let raw_cwd = cwd.ok_or_else(|| {
                SkillManagerError::InvalidInput(
                    "project-scoped skills require an active workspace cwd".to_string(),
                )
            })?;
            let canonical_cwd = validate_project_cwd(raw_cwd, user_home)?;
            Ok(WriteRootTarget {
                source: SkillSource::OrbitProject,
                canonical_boundary: canonical_cwd,
                root_segments: &[".orbit", "skills"],
            })
        }
    }
}

fn ensure_contained_skill_dir(
    target: &WriteRootTarget,
    slug: &str,
) -> Result<PathBuf, SkillManagerError> {
    let canonical_root =
        walk_contained_dir_segments(&target.canonical_boundary, target.root_segments, true)?;
    if !canonical_root.starts_with(&target.canonical_boundary) {
        return Err(SkillManagerError::PermissionDenied(
            "skills root escapes allowed boundary".to_string(),
        ));
    }

    let canonical_dir = walk_contained_dir_segments(&canonical_root, &[slug], true)?;
    if canonical_dir.parent() != Some(canonical_root.as_path()) {
        return Err(SkillManagerError::PermissionDenied(
            "skill directory escapes allowed skills root".to_string(),
        ));
    }
    Ok(canonical_dir)
}

fn upsert_managed_skill_unlocked(
    orbit_home: &Path,
    user_home: &Path,
    params: UpsertSkillParams,
) -> Result<Vec<ManagedSkillRecord>, SkillManagerError> {
    let slug = validate_skill_slug(&params.slug)?;
    let fallback_name = if params.name.trim().is_empty() {
        title_case_slug(&slug)
    } else {
        params.name.trim().to_string()
    };
    let final_content =
        ensure_skill_frontmatter(&fallback_name, &params.description, &params.content);
    if final_content.len() > MAX_SKILL_CONTENT_BYTES {
        return Err(SkillManagerError::TooLarge(format!(
            "skill content exceeds maximum size of {} bytes",
            MAX_SKILL_CONTENT_BYTES
        )));
    }

    let target = resolve_write_root(orbit_home, user_home, params.scope, params.cwd.as_deref())?;
    let skill_dir = ensure_contained_skill_dir(&target, &slug)?;
    let md_path = skill_dir.join("SKILL.md");

    if let Ok(meta) = fs::symlink_metadata(&md_path)
        && meta.file_type().is_symlink()
    {
        return Err(SkillManagerError::PermissionDenied(
            "refusing to write through symlinked SKILL.md".to_string(),
        ));
    }

    // Enforce the write precondition BEFORE touching either `SKILL.md` or the enabled-state file,
    // so a rejected create leaves the existing skill byte-for-byte and toggle-for-toggle intact.
    match &params.precondition {
        SkillWritePrecondition::MustNotExist => {
            if md_path.exists() {
                return Err(SkillManagerError::AlreadyExists(format!(
                    "a skill named `{slug}` already exists in this scope"
                )));
            }
        }
        SkillWritePrecondition::MustMatch(expected) => {
            if md_path.exists() {
                let existing_bytes = fs::read(&md_path).map_err(|e| {
                    SkillManagerError::Io(format!("read existing SKILL.md failed: {e}"))
                })?;
                let actual = sha256_hex(&existing_bytes);
                if actual != *expected {
                    return Err(SkillManagerError::Conflict {
                        expected: expected.clone(),
                        actual,
                    });
                }
            } else {
                return Err(SkillManagerError::Conflict {
                    expected: expected.clone(),
                    actual: "<missing>".to_string(),
                });
            }
        }
        SkillWritePrecondition::Unchecked => {}
    }

    atomic_write_bytes(&md_path, final_content.as_bytes())?;

    let id = skill_id_for(target.source, &slug);
    let mut state = load_skills_state(orbit_home);
    state.enabled_overrides.insert(id, params.enabled);
    save_skills_state(orbit_home, &state)?;

    Ok(list_managed_skills(
        orbit_home,
        user_home,
        params.cwd.as_deref(),
    ))
}

/// Create or update an `OrbitUser` or `OrbitProject` skill with strict slug validation,
/// canonical containment, symlink protection, size cap, atomic writes, and the
/// [`SkillWritePrecondition`] guard (create-only, etag match, or unchecked).
pub fn upsert_managed_skill(
    orbit_home: &Path,
    user_home: &Path,
    params: UpsertSkillParams,
) -> Result<Vec<ManagedSkillRecord>, SkillManagerError> {
    let _guard = lock_skills_io()?;
    upsert_managed_skill_unlocked(orbit_home, user_home, params)
}

/// Persist the enabled/disabled toggle for any discovered skill in `$ORBIT_HOME/skills-state.json`.
pub fn toggle_managed_skill(
    orbit_home: &Path,
    user_home: &Path,
    skill_id: &str,
    enabled: bool,
    cwd: Option<&str>,
) -> Result<Vec<ManagedSkillRecord>, SkillManagerError> {
    let _guard = lock_skills_io()?;
    let skills = list_managed_skills(orbit_home, user_home, cwd);
    if !skills.iter().any(|s| s.id == skill_id) {
        return Err(SkillManagerError::NotFound(format!(
            "skill `{skill_id}` not found"
        )));
    }

    let mut state = load_skills_state(orbit_home);
    state
        .enabled_overrides
        .insert(skill_id.to_string(), enabled);
    save_skills_state(orbit_home, &state)?;

    Ok(list_managed_skills(orbit_home, user_home, cwd))
}

/// Delete an `OrbitUser` or `OrbitProject` skill directory safely by resolving directly
/// from the validated scope root + slug (never trusting `record.path`), verifying no symlinks.
pub fn delete_managed_skill(
    orbit_home: &Path,
    user_home: &Path,
    skill_id: &str,
    cwd: Option<&str>,
) -> Result<Vec<ManagedSkillRecord>, SkillManagerError> {
    let _guard = lock_skills_io()?;
    let skills = list_managed_skills(orbit_home, user_home, cwd);
    let record = skills
        .into_iter()
        .find(|s| s.id == skill_id)
        .ok_or_else(|| SkillManagerError::NotFound(format!("skill `{skill_id}` not found")))?;

    if !record.editable {
        return Err(SkillManagerError::PermissionDenied(format!(
            "skill `{skill_id}` is read-only ({:?}); only Orbit user/project skills can be deleted",
            record.source
        )));
    }

    let slug = validate_skill_slug(&record.slug)?;
    let scope = match record.source {
        SkillSource::OrbitUser => SkillWriteScope::User,
        SkillSource::OrbitProject => SkillWriteScope::Project,
        _ => {
            return Err(SkillManagerError::PermissionDenied(
                "only Orbit skills can be deleted".to_string(),
            ));
        }
    };

    // Build candidate (boundary, segments) targets for this editable scope
    let mut candidate_targets: Vec<(PathBuf, &'static [&'static str])> = Vec::new();
    match scope {
        SkillWriteScope::User => {
            if let Ok(canon_orbit) = crate::platform::canonicalize_clean(orbit_home) {
                candidate_targets.push((canon_orbit, &["skills"]));
            }
            if let Ok(canon_home) = crate::platform::canonicalize_clean(user_home) {
                let home_orbit = canon_home.join(".orbit").join("skills");
                if !candidate_targets
                    .iter()
                    .any(|(b, _)| b.join("skills") == home_orbit)
                {
                    candidate_targets.push((canon_home, &[".orbit", "skills"]));
                }
            }
        }
        SkillWriteScope::Project => {
            let raw_cwd = cwd.ok_or_else(|| {
                SkillManagerError::InvalidInput(
                    "project-scoped skill deletion requires an active workspace cwd".to_string(),
                )
            })?;
            let canonical_cwd = validate_project_cwd(raw_cwd, user_home)?;
            candidate_targets.push((canonical_cwd, &[".orbit", "skills"]));
        }
    }

    let mut deleted = false;
    let mut last_err: Option<SkillManagerError> = None;

    for (boundary, segments) in candidate_targets {
        let canonical_root = match walk_contained_dir_segments(&boundary, segments, false) {
            Ok(root) => root,
            Err(SkillManagerError::NotFound(_)) => continue,
            Err(e) => {
                last_err = Some(e);
                continue;
            }
        };
        if !canonical_root.starts_with(&boundary) {
            return Err(SkillManagerError::PermissionDenied(
                "skills root escapes allowed boundary".to_string(),
            ));
        }

        let candidate_dir = canonical_root.join(&slug);
        match fs::symlink_metadata(&candidate_dir) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(SkillManagerError::PermissionDenied(
                        "refusing to delete symlinked skill directory".to_string(),
                    ));
                }
                if !meta.is_dir() {
                    return Err(SkillManagerError::InvalidInput(
                        "expected skill path to be a directory".to_string(),
                    ));
                }
            }
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(SkillManagerError::Io(format!(
                    "inspect skill directory failed: {e}"
                )));
            }
        }

        let canonical_dir = crate::platform::canonicalize_clean(&candidate_dir).map_err(|e| {
            SkillManagerError::Io(format!("canonicalize skill directory failed: {e}"))
        })?;
        if canonical_dir.parent() != Some(canonical_root.as_path()) {
            return Err(SkillManagerError::PermissionDenied(
                "skill directory failed parent containment check".to_string(),
            ));
        }

        fs::remove_dir_all(&candidate_dir)
            .map_err(|e| SkillManagerError::Io(format!("delete skill directory failed: {e}")))?;
        deleted = true;
        break;
    }

    if !deleted {
        return Err(last_err.unwrap_or_else(|| {
            SkillManagerError::NotFound(format!("skill directory for `{skill_id}` not found"))
        }));
    }

    let mut state = load_skills_state(orbit_home);
    state.enabled_overrides.remove(skill_id);
    save_skills_state(orbit_home, &state)?;

    Ok(list_managed_skills(orbit_home, user_home, cwd))
}

fn looks_like_remote_url(raw: &str) -> bool {
    let lower = raw.trim().to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || lower.starts_with("file://")
        || lower.starts_with("ssh://")
        || lower.starts_with("data:")
}

fn looks_like_local_path_input(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\n') || trimmed.contains('\r') {
        return false;
    }
    trimmed.starts_with('/')
        || trimmed.starts_with("~/")
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.ends_with(".md")
        || trimmed.ends_with(".MD")
        || trimmed.ends_with(".markdown")
        || trimmed.ends_with(".MARKDOWN")
}

fn has_markdown_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

fn allowed_import_bases(
    orbit_home: &Path,
    user_home: &Path,
    cwd: Option<&str>,
) -> Vec<(PathBuf, PathBuf)> {
    let mut raw_bases: Vec<PathBuf> = Vec::new();
    if let Some(canon_cwd) = resolve_optional_cwd(cwd, user_home) {
        if let Some(raw_cwd) = cwd.map(str::trim).filter(|s| !s.is_empty()) {
            raw_bases.push(PathBuf::from(raw_cwd));
        }
        raw_bases.push(canon_cwd);
    }
    // Explicit skills directories only — NEVER raw `user_home` or `orbit_home`!
    raw_bases.push(orbit_user_skills_root(orbit_home));
    raw_bases.push(user_home.join(".orbit").join("skills"));
    raw_bases.push(user_home.join(".claude").join("skills"));
    raw_bases.push(user_home.join(".codex").join("skills"));
    raw_bases.push(user_home.join(".gemini").join("config").join("skills"));
    raw_bases.push(user_home.join(".gemini").join("antigravity").join("skills"));
    raw_bases.push(
        user_home
            .join(".gemini")
            .join("antigravity-cli")
            .join("skills"),
    );

    let mut pairs = Vec::new();
    for raw in raw_bases {
        // Skip symlinked base directories
        if fs::symlink_metadata(&raw).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        if let Ok(canon) = crate::platform::canonicalize_clean(&raw) {
            pairs.push((raw, canon));
        }
    }
    pairs
}

fn verify_relative_path_no_symlinks(
    canonical_base: &Path,
    rel: &Path,
) -> Result<PathBuf, SkillManagerError> {
    let mut current = canonical_base.to_path_buf();
    for comp in rel.components() {
        match comp {
            Component::CurDir => continue,
            Component::Normal(seg) => {
                current.push(seg);
                let meta = fs::symlink_metadata(&current).map_err(|e| {
                    SkillManagerError::NotFound(format!(
                        "import path segment `{}` unavailable: {e}",
                        current.display()
                    ))
                })?;
                if meta.file_type().is_symlink() {
                    return Err(SkillManagerError::PermissionDenied(format!(
                        "refusing to follow symlink `{}` during skill import",
                        current.display()
                    )));
                }
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SkillManagerError::PermissionDenied(
                    "import path contains forbidden traversal component".to_string(),
                ));
            }
        }
    }
    Ok(current)
}

fn resolve_import_local_file(
    orbit_home: &Path,
    user_home: &Path,
    cwd: Option<&str>,
    raw_path: &str,
) -> Result<(String, String), SkillManagerError> {
    let trimmed = raw_path.trim();
    let candidate = if let Some(rest) = trimmed.strip_prefix("~/") {
        user_home.join(rest)
    } else {
        let p = PathBuf::from(trimmed);
        if p.is_absolute() {
            p
        } else if let Some(cwd_dir) = resolve_optional_cwd(cwd, user_home) {
            cwd_dir.join(p)
        } else {
            return Err(SkillManagerError::InvalidInput(
                "relative import path requires an active workspace cwd".to_string(),
            ));
        }
    };

    // Reject symlinked candidate before descending into directory or reading file
    let cand_meta = fs::symlink_metadata(&candidate).map_err(|e| {
        SkillManagerError::NotFound(format!(
            "import source `{trimmed}` could not be resolved: {e}"
        ))
    })?;
    if cand_meta.file_type().is_symlink() {
        return Err(SkillManagerError::PermissionDenied(
            "refusing to follow symlink during skill import".to_string(),
        ));
    }

    let target_file = if cand_meta.is_dir() {
        candidate.join("SKILL.md")
    } else {
        candidate
    };

    if !has_markdown_extension(&target_file) {
        return Err(SkillManagerError::PermissionDenied(
            "skill import only permits Markdown files (.md or .markdown)".to_string(),
        ));
    }

    let target_meta = fs::symlink_metadata(&target_file).map_err(|e| {
        SkillManagerError::NotFound(format!(
            "import source `{trimmed}` could not be resolved: {e}"
        ))
    })?;
    if target_meta.file_type().is_symlink() {
        return Err(SkillManagerError::PermissionDenied(
            "refusing to import symlinked Markdown file".to_string(),
        ));
    }
    if !target_meta.is_file() {
        return Err(SkillManagerError::InvalidInput(format!(
            "import source `{trimmed}` is not a regular file"
        )));
    }

    let canonical_file = crate::platform::canonicalize_clean(&target_file).map_err(|e| {
        SkillManagerError::NotFound(format!(
            "import source `{trimmed}` could not be resolved: {e}"
        ))
    })?;

    if !has_markdown_extension(&canonical_file) || !canonical_file.is_file() {
        return Err(SkillManagerError::PermissionDenied(
            "resolved import target must be a regular Markdown file (.md or .markdown)".to_string(),
        ));
    }

    // Strict containment: ONLY authorized workspace cwd or explicit skills directories (NEVER raw $HOME/$ORBIT_HOME)
    let bases = allowed_import_bases(orbit_home, user_home, cwd);
    let mut verified_under_base = false;
    for (raw_base, canon_base) in &bases {
        if !canonical_file.starts_with(canon_base) {
            continue;
        }
        let rel = target_file
            .strip_prefix(canon_base)
            .or_else(|_| target_file.strip_prefix(raw_base));
        let Ok(rel_path) = rel else {
            continue;
        };
        let walked = verify_relative_path_no_symlinks(canon_base, rel_path)?;
        let walked_canon = crate::platform::canonicalize_clean(&walked)
            .map_err(|e| SkillManagerError::Io(format!("canonicalize import path failed: {e}")))?;
        if walked_canon == canonical_file {
            verified_under_base = true;
            break;
        }
    }

    if !verified_under_base {
        return Err(SkillManagerError::PermissionDenied(
            "import file must reside inside the active workspace or a skills directory without symlinks"
                .to_string(),
        ));
    }

    if target_meta.len() > MAX_SKILL_CONTENT_BYTES as u64 {
        return Err(SkillManagerError::TooLarge(format!(
            "import file exceeds maximum size of {} bytes",
            MAX_SKILL_CONTENT_BYTES
        )));
    }

    let bytes = fs::read(&canonical_file)
        .map_err(|e| SkillManagerError::Io(format!("read import file failed: {e}")))?;
    let content = String::from_utf8(bytes).map_err(|_| {
        SkillManagerError::InvalidInput("import file must be valid UTF-8 Markdown".to_string())
    })?;

    let inferred_slug = if canonical_file
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("SKILL.md"))
    {
        canonical_file
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(slugify_label)
            .unwrap_or_else(|| "imported-skill".to_string())
    } else {
        canonical_file
            .file_stem()
            .and_then(|n| n.to_str())
            .map(slugify_label)
            .unwrap_or_else(|| "imported-skill".to_string())
    };

    Ok((content, inferred_slug))
}

/// Safely import a skill from pasted Markdown or a validated local Markdown file inside the
/// active workspace or explicit skills directories. Rejects remote URLs, symlinks, non-Markdown
/// files, and arbitrary `$HOME`/`$ORBIT_HOME` files.
///
/// Pass [`SkillWritePrecondition::MustNotExist`] to refuse overwriting an existing skill at the
/// resolved slug (the slug is derived from the override, path, frontmatter, or heading, so the
/// caller cannot always predict it up front).
pub fn import_managed_skill(
    orbit_home: &Path,
    user_home: &Path,
    scope: SkillWriteScope,
    cwd: Option<&str>,
    source_markdown_or_path: &str,
    slug_override: Option<&str>,
    precondition: SkillWritePrecondition,
) -> Result<Vec<ManagedSkillRecord>, SkillManagerError> {
    let _guard = lock_skills_io()?;
    let trimmed = source_markdown_or_path.trim();
    if trimmed.is_empty() {
        return Err(SkillManagerError::InvalidInput(
            "import source must not be empty".to_string(),
        ));
    }
    if looks_like_remote_url(trimmed) {
        return Err(SkillManagerError::PermissionDenied(
            "remote URLs are not permitted for skill import; paste Markdown or specify a local Markdown file inside the active workspace or skills directory".to_string(),
        ));
    }

    let (raw_markdown, inferred_path_slug) = if looks_like_local_path_input(trimmed) {
        let (content, slug) = resolve_import_local_file(orbit_home, user_home, cwd, trimmed)?;
        (content, Some(slug))
    } else {
        if source_markdown_or_path.len() > MAX_SKILL_CONTENT_BYTES {
            return Err(SkillManagerError::TooLarge(format!(
                "imported Markdown exceeds maximum size of {} bytes",
                MAX_SKILL_CONTENT_BYTES
            )));
        }
        (source_markdown_or_path.to_string(), None)
    };

    let (fm, body) = parse_skill_frontmatter(&raw_markdown);
    let (heading, fallback_desc) = extract_fallback_heading_and_desc(body);

    let slug = if let Some(override_raw) = slug_override.map(str::trim).filter(|s| !s.is_empty()) {
        validate_skill_slug(override_raw)?
    } else if let Some(path_slug) = inferred_path_slug {
        validate_skill_slug(&path_slug)?
    } else if let Some(ref fm_name) = fm.name {
        validate_skill_slug(&slugify_label(fm_name))?
    } else if let Some(ref h) = heading {
        validate_skill_slug(&slugify_label(h))?
    } else {
        "imported-skill".to_string()
    };

    let name = fm
        .name
        .or(heading)
        .unwrap_or_else(|| title_case_slug(&slug));
    let description = fm.description.unwrap_or(fallback_desc);
    let final_content = ensure_skill_frontmatter(&name, &description, &raw_markdown);

    upsert_managed_skill_unlocked(
        orbit_home,
        user_home,
        UpsertSkillParams {
            scope,
            cwd: cwd.map(ToString::to_string),
            slug,
            name,
            description,
            content: final_content,
            enabled: true,
            precondition,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempSandbox {
        root: PathBuf,
        orbit_home: PathBuf,
        user_home: PathBuf,
        workspace: PathBuf,
    }

    impl TempSandbox {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "orbit_skills_mgr_{label}_{}",
                Uuid::new_v4().simple()
            ));
            let user_home = root.join("home");
            let orbit_home = user_home.join(".orbit");
            let workspace = root.join("workspace");
            fs::create_dir_all(&orbit_home).expect("mkdir orbit_home");
            fs::create_dir_all(&workspace).expect("mkdir workspace");
            Self {
                root,
                orbit_home,
                user_home,
                workspace,
            }
        }
    }

    impl Drop for TempSandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn write_skill(root: &Path, slug: &str, content: &str) -> PathBuf {
        let dir = root.join(slug);
        fs::create_dir_all(&dir).expect("mkdir skill");
        let md = dir.join("SKILL.md");
        fs::write(&md, content).expect("write SKILL.md");
        dir
    }

    #[test]
    fn parses_yaml_frontmatter_scalars_blocks_and_lists() {
        let md = r#"---
name: "Rust Audit"
description: >-
  Audit Rust code for safety,
  idiom, and maintainability.
triggers: [rust, clippy, 'it''s\raw']
allowed-tools:
  - Read
  - Grep
  - Bash
---

# Rust Audit

Run static checks first.
"#;
        let (fm, body) = parse_skill_frontmatter(md);
        assert!(fm.has_frontmatter);
        assert_eq!(fm.name.as_deref(), Some("Rust Audit"));
        assert_eq!(
            fm.description.as_deref(),
            Some("Audit Rust code for safety, idiom, and maintainability.")
        );
        assert_eq!(fm.triggers, vec!["rust", "clippy", r"it's\raw"]);
        assert_eq!(fm.allowed_tools, vec!["Read", "Grep", "Bash"]);
        assert!(body.contains("# Rust Audit"));

        // Unclosed frontmatter delimiter gets synthesized valid frontmatter
        let unclosed = "---\nThis is a horizontal rule, not frontmatter.\n";
        let ensured = ensure_skill_frontmatter("Rule", "Desc", unclosed);
        assert!(parse_skill_frontmatter(&ensured).0.has_frontmatter);
        assert_eq!(
            parse_skill_frontmatter(&ensured).0.name.as_deref(),
            Some("Rule")
        );

        // Comma-separated allowed-tools format
        let md2 = "---\nname: Quick\nallowed-tools: Read, Glob, Grep\n---\nHello world.\n";
        let (fm2, _) = parse_skill_frontmatter(md2);
        assert_eq!(fm2.allowed_tools, vec!["Read", "Glob", "Grep"]);
    }

    #[test]
    fn discovers_multi_source_skills_with_dedup_and_subdirs() {
        let sb = TempSandbox::new("discover");

        let orbit_dir = write_skill(
            &sb.orbit_home.join("skills"),
            "deploy-check",
            "---\nname: Deploy Check\ndescription: Pre-flight deploy audit\ntriggers: [deploy]\n---\nBody text.\n",
        );
        fs::create_dir_all(orbit_dir.join("scripts")).expect("mkdir scripts");
        fs::create_dir_all(orbit_dir.join("references")).expect("mkdir references");

        // Uppercase dir under OrbitUser must be ignored for editable scopes
        write_skill(
            &sb.orbit_home.join("skills"),
            "UpperCaseOrbit",
            "---\nname: Bad Case\n---\nBody.\n",
        );

        // Project orbit skill overriding same slug in Claude project
        write_skill(
            &sb.workspace.join(".orbit").join("skills"),
            "shared-slug",
            "---\nname: Project Override\ndescription: From OrbitProject\n---\nProject body.\n",
        );
        write_skill(
            &sb.workspace.join(".claude").join("skills"),
            "shared-slug",
            "---\nname: Claude Project Copy\ndescription: Should be deduped\n---\nClaude body.\n",
        );

        write_skill(
            &sb.user_home.join(".claude").join("skills"),
            "claude-helper",
            "---\nname: Claude Helper\ndescription: Helpful Claude skill\n---\nInstructions.\n",
        );
        write_skill(
            &sb.user_home.join(".codex").join("skills"),
            "codex-lint",
            "---\nname: Codex Lint\ndescription: Lint helper\n---\nInstructions.\n",
        );
        write_skill(
            &sb.user_home.join(".gemini").join("config").join("skills"),
            "agy-ui",
            "---\nname: AGY UI\ndescription: UI review\n---\nInstructions.\n",
        );
        write_skill(
            &sb.user_home
                .join(".gemini")
                .join("antigravity")
                .join("skills"),
            "agy-ui",
            "---\nname: Duplicate AGY UI\ndescription: Should be deduped\n---\nDup.\n",
        );

        let cwd_str = sb.workspace.to_string_lossy().to_string();
        let skills = list_managed_skills(&sb.orbit_home, &sb.user_home, Some(&cwd_str));

        assert!(!skills.iter().any(|s| s.slug == "uppercaseorbit"));

        let deploy = skills
            .iter()
            .find(|s| s.slug == "deploy-check")
            .expect("deploy-check");
        assert_eq!(deploy.source, SkillSource::OrbitUser);
        assert!(deploy.editable);
        assert!(deploy.enabled);
        assert!(deploy.has_scripts);
        assert!(deploy.has_references);
        assert!(deploy.approx_tokens > 0);

        let shared_matches: Vec<_> = skills.iter().filter(|s| s.slug == "shared-slug").collect();
        assert_eq!(shared_matches.len(), 1);
        assert_eq!(shared_matches[0].source, SkillSource::OrbitProject);
        assert_eq!(shared_matches[0].name, "Project Override");

        let agy_matches: Vec<_> = skills.iter().filter(|s| s.slug == "agy-ui").collect();
        assert_eq!(agy_matches.len(), 1);
        assert_eq!(agy_matches[0].source, SkillSource::AntigravityUser);
        assert!(!agy_matches[0].editable);

        assert!(
            skills
                .iter()
                .any(|s| s.slug == "claude-helper" && s.source == SkillSource::ClaudeUser)
        );
        assert!(
            skills
                .iter()
                .any(|s| s.slug == "codex-lint" && s.source == SkillSource::CodexUser)
        );
        assert!(
            skills
                .iter()
                .any(|s| s.slug == "orbit" && s.source == SkillSource::Builtin)
        );
    }

    #[test]
    fn crud_toggle_etag_conflict_and_persistence_roundtrip() {
        let sb = TempSandbox::new("crud");
        let cwd_str = sb.workspace.to_string_lossy().to_string();

        let after_create = upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::User,
                cwd: Some(cwd_str.clone()),
                slug: "release-notes".to_string(),
                name: "Release Notes".to_string(),
                description: "Generate concise changelogs".to_string(),
                content: "# Release Notes\n\nSummarize commits by impact.".to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::Unchecked,
            },
        )
        .expect("create skill");

        let created = after_create
            .iter()
            .find(|s| s.slug == "release-notes")
            .expect("created record");
        assert_eq!(created.id, "orbit_user:release-notes");
        assert!(created.enabled);
        assert!(created.editable);

        let read_v1 =
            read_managed_skill(&sb.orbit_home, &sb.user_home, &created.id, Some(&cwd_str))
                .expect("read v1");
        assert!(read_v1.content.starts_with("---\n"));
        assert!(read_v1.content.contains("Release Notes"));

        let conflict = upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::User,
                cwd: Some(cwd_str.clone()),
                slug: "release-notes".to_string(),
                name: "Release Notes".to_string(),
                description: "Updated".to_string(),
                content: "---\nname: Release Notes\ndescription: Updated\n---\nV2\n".to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::MustMatch("0000000000000000".to_string()),
            },
        );
        assert!(matches!(conflict, Err(SkillManagerError::Conflict { .. })));

        let updated = upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::User,
                cwd: Some(cwd_str.clone()),
                slug: "release-notes".to_string(),
                name: "Release Notes v2".to_string(),
                description: "Updated desc".to_string(),
                content: "---\nname: Release Notes v2\ndescription: Updated desc\n---\nV2 body\n"
                    .to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::MustMatch(read_v1.etag),
            },
        )
        .expect("update with valid etag");
        let rec_v2 = updated
            .iter()
            .find(|s| s.id == created.id)
            .expect("updated rec");
        assert_eq!(rec_v2.name, "Release Notes v2");

        let toggled = toggle_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            &created.id,
            false,
            Some(&cwd_str),
        )
        .expect("toggle off");
        assert!(
            !toggled
                .iter()
                .find(|s| s.id == created.id)
                .expect("toggled rec")
                .enabled
        );
        let reloaded = list_managed_skills(&sb.orbit_home, &sb.user_home, Some(&cwd_str));
        assert!(
            !reloaded
                .iter()
                .find(|s| s.id == created.id)
                .expect("reloaded rec")
                .enabled
        );

        let after_delete =
            delete_managed_skill(&sb.orbit_home, &sb.user_home, &created.id, Some(&cwd_str))
                .expect("delete skill");
        assert!(!after_delete.iter().any(|s| s.id == created.id));
    }

    #[test]
    fn rejects_slug_traversal_unsafe_project_cwd_and_readonly_delete() {
        let sb = TempSandbox::new("security");

        for bad in [
            "",
            "../escape",
            "foo/bar",
            "foo\\bar",
            "UpperCase",
            "-leading-dash",
            "has space",
            "a..b",
        ] {
            assert!(
                validate_skill_slug(bad).is_err(),
                "expected `{bad}` to be rejected"
            );
        }

        // Unsafe project cwds: `/`, `user_home`, `.git`
        let git_dir = sb.workspace.join(".git");
        fs::create_dir_all(&git_dir).expect("mkdir .git");
        for unsafe_cwd in [
            "/".to_string(),
            sb.user_home.to_string_lossy().to_string(),
            git_dir.to_string_lossy().to_string(),
        ] {
            let res = upsert_managed_skill(
                &sb.orbit_home,
                &sb.user_home,
                UpsertSkillParams {
                    scope: SkillWriteScope::Project,
                    cwd: Some(unsafe_cwd.clone()),
                    slug: "bad-root".to_string(),
                    name: "Bad".to_string(),
                    description: String::new(),
                    content: "body".to_string(),
                    enabled: true,
                    precondition: SkillWritePrecondition::Unchecked,
                },
            );
            assert!(
                matches!(res, Err(SkillManagerError::PermissionDenied(_))),
                "expected unsafe cwd `{unsafe_cwd}` to be denied, got {res:?}"
            );
        }

        // External Claude skill cannot be deleted
        write_skill(
            &sb.user_home.join(".claude").join("skills"),
            "ext-only",
            "---\nname: Ext Only\ndescription: Read only\n---\nBody.\n",
        );
        let err = delete_managed_skill(&sb.orbit_home, &sb.user_home, "claude_user:ext-only", None);
        assert!(matches!(err, Err(SkillManagerError::PermissionDenied(_))));
    }

    #[test]
    fn imports_pasted_markdown_and_blocks_secrets_and_symlinks() {
        let sb = TempSandbox::new("import");
        let cwd_str = sb.workspace.to_string_lossy().to_string();

        // 1. Pasted plain Markdown auto-synthesizes frontmatter
        let imported_md = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::Project,
            Some(&cwd_str),
            "# SQL Performance Checklist\n\nInspect query plans before adding indexes.",
            None,
            SkillWritePrecondition::Unchecked,
        )
        .expect("import pasted markdown");
        let sql_rec = imported_md
            .iter()
            .find(|s| s.slug == "sql-performance-checklist")
            .expect("sql skill");
        assert_eq!(sql_rec.source, SkillSource::OrbitProject);
        assert_eq!(sql_rec.name, "SQL Performance Checklist");
        let read_sql =
            read_managed_skill(&sb.orbit_home, &sb.user_home, &sql_rec.id, Some(&cwd_str))
                .expect("read imported sql skill");
        assert!(read_sql.content.starts_with("---\n"));

        // 2. Local Markdown file inside authorized workspace imports cleanly
        let local_doc = sb.workspace.join("api-guidelines.md");
        fs::write(&local_doc, "# API Guidelines\n\nKeep payloads small.").expect("write local doc");
        let imported_file = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::User,
            Some(&cwd_str),
            &local_doc.to_string_lossy(),
            Some("api-guide"),
            SkillWritePrecondition::Unchecked,
        )
        .expect("import local file");
        assert!(imported_file.iter().any(|s| s.id == "orbit_user:api-guide"));

        // 3. Remote URL rejected
        let url_err = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::User,
            Some(&cwd_str),
            "https://evil.example.com/SKILL.md",
            None,
            SkillWritePrecondition::Unchecked,
        );
        assert!(matches!(
            url_err,
            Err(SkillManagerError::PermissionDenied(_))
        ));

        // 4. Secret files inside $HOME and $ORBIT_HOME MUST be rejected (non-md AND outside skills dirs)
        let ssh_dir = sb.user_home.join(".ssh");
        fs::create_dir_all(&ssh_dir).expect("mkdir .ssh");
        let ssh_key = ssh_dir.join("id_ed25519");
        fs::write(&ssh_key, "PRIVATE KEY").expect("write ssh key");
        let ssh_err = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::User,
            Some(&cwd_str),
            "~/.ssh/id_ed25519",
            None,
            SkillWritePrecondition::Unchecked,
        );
        assert!(matches!(
            ssh_err,
            Err(SkillManagerError::PermissionDenied(_))
        ));

        let orbit_key = sb.orbit_home.join("identity.key");
        fs::write(&orbit_key, "SECRET IDENTITY KEY").expect("write identity.key");
        let key_err = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::User,
            Some(&cwd_str),
            "~/.orbit/identity.key",
            None,
            SkillWritePrecondition::Unchecked,
        );
        assert!(matches!(
            key_err,
            Err(SkillManagerError::PermissionDenied(_))
        ));

        // Even a .md file directly under $HOME (outside skills dirs and outside cwd) MUST be rejected
        let home_md = sb.user_home.join("private-notes.md");
        fs::write(&home_md, "# Private").expect("write home md");
        let home_md_err = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::User,
            Some(&cwd_str),
            &home_md.to_string_lossy(),
            None,
            SkillWritePrecondition::Unchecked,
        );
        assert!(matches!(
            home_md_err,
            Err(SkillManagerError::PermissionDenied(_))
        ));

        // 5. File outside workspace/skills rejected
        let outside_file = sb.root.join("outside.md");
        fs::write(&outside_file, "# Outside\nSecret").expect("write outside");
        let outside_err = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::User,
            Some(&cwd_str),
            &outside_file.to_string_lossy(),
            None,
            SkillWritePrecondition::Unchecked,
        );
        assert!(matches!(
            outside_err,
            Err(SkillManagerError::PermissionDenied(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn blocks_symlink_escapes_in_discovery_upsert_delete_and_import() {
        use std::os::unix::fs::symlink;

        let sb = TempSandbox::new("symlinks");
        let cwd_str = sb.workspace.to_string_lossy().to_string();

        // External target outside allowed boundaries
        let external_dir = sb.root.join("external_victim");
        fs::create_dir_all(&external_dir).expect("mkdir victim");
        let external_file = external_dir.join("secret.md");
        fs::write(&external_file, "# Secret\nDo not leak").expect("write victim file");

        // 1. Import symlinked .md -> external file (or /etc/passwd) MUST be rejected
        let link_md = sb.workspace.join("evil.md");
        symlink("/etc/passwd", &link_md).expect("symlink passwd");
        let import_sym_err = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::User,
            Some(&cwd_str),
            &link_md.to_string_lossy(),
            None,
            SkillWritePrecondition::Unchecked,
        );
        assert!(matches!(
            import_sym_err,
            Err(SkillManagerError::PermissionDenied(_))
        ));

        // Symlinked parent dir inside workspace -> external dir MUST be rejected
        let link_dir = sb.workspace.join("linked_dir");
        symlink(&external_dir, &link_dir).expect("symlink dir");
        let import_parent_sym_err = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::User,
            Some(&cwd_str),
            &link_dir.join("secret.md").to_string_lossy(),
            None,
            SkillWritePrecondition::Unchecked,
        );
        assert!(matches!(
            import_parent_sym_err,
            Err(SkillManagerError::PermissionDenied(_))
        ));

        // 2. Discovery must skip symlinked skill_dir and symlinked SKILL.md
        let user_skills = sb.orbit_home.join("skills");
        fs::create_dir_all(&user_skills).expect("mkdir user_skills");
        let external_skill_dir = sb.root.join("ext_skill");
        fs::create_dir_all(&external_skill_dir).expect("mkdir ext_skill");
        fs::write(
            external_skill_dir.join("SKILL.md"),
            "---\nname: Escaped\n---\nBody\n",
        )
        .expect("write ext SKILL.md");
        symlink(&external_skill_dir, user_skills.join("sym-dir")).expect("symlink skill dir");

        let real_dir_with_sym_md = user_skills.join("sym-md");
        fs::create_dir_all(&real_dir_with_sym_md).expect("mkdir sym-md");
        symlink(
            external_skill_dir.join("SKILL.md"),
            real_dir_with_sym_md.join("SKILL.md"),
        )
        .expect("symlink SKILL.md");

        let discovered = list_managed_skills(&sb.orbit_home, &sb.user_home, Some(&cwd_str));
        assert!(!discovered.iter().any(|s| s.slug == "sym-dir"));
        assert!(!discovered.iter().any(|s| s.slug == "sym-md"));

        // 3. Upsert into symlinked project `.orbit` dir MUST be rejected
        let sym_project_ws = sb.root.join("ws_sym_orbit");
        fs::create_dir_all(&sym_project_ws).expect("mkdir ws_sym_orbit");
        symlink(&external_dir, sym_project_ws.join(".orbit")).expect("symlink .orbit");
        let upsert_sym_err = upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::Project,
                cwd: Some(sym_project_ws.to_string_lossy().to_string()),
                slug: "escape-write".to_string(),
                name: "Escape".to_string(),
                description: String::new(),
                content: "Body".to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::Unchecked,
            },
        );
        assert!(matches!(
            upsert_sym_err,
            Err(SkillManagerError::PermissionDenied(_))
        ));

        // 4. Upsert into symlinked skill slug dir MUST be rejected
        let upsert_slug_sym_err = upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::User,
                cwd: Some(cwd_str.clone()),
                slug: "sym-dir".to_string(),
                name: "Sym".to_string(),
                description: String::new(),
                content: "Body".to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::Unchecked,
            },
        );
        assert!(matches!(
            upsert_slug_sym_err,
            Err(SkillManagerError::PermissionDenied(_))
        ));

        // 5. Delete must refuse if skill directory is replaced by a symlink (victim dir stays intact)
        let del_res = delete_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            "orbit_user:sym-dir",
            Some(&cwd_str),
        );
        assert!(del_res.is_err());
        assert!(external_skill_dir.join("SKILL.md").exists());
    }

    #[test]
    fn concurrent_toggles_preserve_all_overrides() {
        let sb = TempSandbox::new("concurrent");
        let cwd_str = sb.workspace.to_string_lossy().to_string();

        for idx in 0..4 {
            upsert_managed_skill(
                &sb.orbit_home,
                &sb.user_home,
                UpsertSkillParams {
                    scope: SkillWriteScope::User,
                    cwd: Some(cwd_str.clone()),
                    slug: format!("skill-{idx}"),
                    name: format!("Skill {idx}"),
                    description: "Concurrent test".to_string(),
                    content: "Body".to_string(),
                    enabled: true,
                    precondition: SkillWritePrecondition::Unchecked,
                },
            )
            .expect("create skill");
        }

        std::thread::scope(|s| {
            for idx in 0..4 {
                let orbit_home = sb.orbit_home.clone();
                let user_home = sb.user_home.clone();
                let cwd = cwd_str.clone();
                s.spawn(move || {
                    toggle_managed_skill(
                        &orbit_home,
                        &user_home,
                        &format!("orbit_user:skill-{idx}"),
                        false,
                        Some(&cwd),
                    )
                    .expect("toggle off");
                });
            }
        });

        let final_list = list_managed_skills(&sb.orbit_home, &sb.user_home, Some(&cwd_str));
        for idx in 0..4 {
            let rec = final_list
                .iter()
                .find(|r| r.id == format!("orbit_user:skill-{idx}"))
                .expect("skill present");
            assert!(!rec.enabled, "skill-{idx} override lost under concurrency");
        }
    }

    #[test]
    fn discovers_claude_commands_with_nested_namespaces_frontmatter_and_arguments() {
        let sb = TempSandbox::new("claude-commands");
        let cwd_str = sb.workspace.to_string_lossy().to_string();

        // Project nested command `.claude/commands/ops/deploy.md`
        let proj_ops = sb.workspace.join(".claude").join("commands").join("ops");
        fs::create_dir_all(&proj_ops).expect("mkdir project ops");
        fs::write(
            proj_ops.join("deploy.md"),
            "---\ndescription: \"Deploy service to target environment\"\nargument-hint: \"[env] [tag]\"\n---\nDeploy target `$ARGUMENTS` and verify health checks for `$ARGUMENTS`.\n",
        )
        .expect("write project deploy command");

        // Project root command overriding user command with same slug
        fs::write(
            sb.workspace.join(".claude").join("commands").join("release.md"),
            "---\ndescription: Project release checklist\n---\nCut project release for $ARGUMENTS.\n",
        )
        .expect("write project release");

        // User commands `~/.claude/commands/release.md` (shadowed) and `~/.claude/commands/review-pr.md`
        let user_cmds = sb.user_home.join(".claude").join("commands");
        fs::create_dir_all(&user_cmds).expect("mkdir user cmds");
        fs::write(
            user_cmds.join("release.md"),
            "---\ndescription: User release fallback\n---\nUser fallback.\n",
        )
        .expect("write user release");
        fs::write(
            user_cmds.join("review-pr.md"),
            "# Review Pull Request\nInspect PR diff and summarize risks.\n",
        )
        .expect("write user review-pr");

        let skills = list_managed_skills(&sb.orbit_home, &sb.user_home, Some(&cwd_str));

        let deploy = skills
            .iter()
            .find(|s| s.is_command && s.slug == "ops:deploy")
            .expect("nested ops:deploy command discovered");
        assert_eq!(deploy.source, SkillSource::ClaudeProject);
        assert_eq!(deploy.id, "claude_project:cmd:ops:deploy");
        assert_eq!(deploy.description, "Deploy service to target environment");
        assert_eq!(deploy.argument_hint.as_deref(), Some("[env] [tag]"));
        assert!(!deploy.editable);

        let release_matches: Vec<_> = skills
            .iter()
            .filter(|s| s.is_command && s.slug == "release")
            .collect();
        assert_eq!(release_matches.len(), 1, "project command overrides user");
        assert_eq!(release_matches[0].source, SkillSource::ClaudeProject);
        assert_eq!(release_matches[0].description, "Project release checklist");

        let review_pr = skills
            .iter()
            .find(|s| s.is_command && s.slug == "review-pr")
            .expect("user review-pr command discovered");
        assert_eq!(review_pr.source, SkillSource::ClaudeUser);
        assert_eq!(review_pr.name, "Review Pull Request");
        assert_eq!(
            review_pr.description,
            "Inspect PR diff and summarize risks."
        );

        // `read_managed_skill` reads the command markdown cleanly and `expand_command_arguments` substitutes `$ARGUMENTS`
        let read = read_managed_skill(&sb.orbit_home, &sb.user_home, &deploy.id, Some(&cwd_str))
            .expect("read deploy command");
        assert_eq!(
            expand_command_arguments(&read.content, "staging v1.2.0"),
            "Deploy target `staging v1.2.0` and verify health checks for `staging v1.2.0`."
        );
        assert_eq!(
            expand_command_arguments(&read.content, ""),
            "Deploy target `` and verify health checks for ``."
        );

        let read_review =
            read_managed_skill(&sb.orbit_home, &sb.user_home, &review_pr.id, Some(&cwd_str))
                .expect("read review-pr");
        assert_eq!(
            expand_command_arguments(&read_review.content, "#61 focus on security"),
            "# Review Pull Request\nInspect PR diff and summarize risks.\n\n#61 focus on security"
        );
    }

    #[cfg(unix)]
    #[test]
    fn claude_commands_discovery_rejects_symlinks() {
        use std::os::unix::fs::symlink;
        let sb = TempSandbox::new("claude-commands-sym");
        let cwd_str = sb.workspace.to_string_lossy().to_string();

        let outside = sb.root.join("secret.md");
        fs::write(&outside, "---\ndescription: Secret\n---\nSecret body").expect("write outside");

        let proj_cmds = sb.workspace.join(".claude").join("commands");
        fs::create_dir_all(&proj_cmds).expect("mkdir cmds");
        symlink(&outside, proj_cmds.join("leak.md")).expect("symlink leak");

        let skills = list_managed_skills(&sb.orbit_home, &sb.user_home, Some(&cwd_str));
        assert!(
            !skills.iter().any(|s| s.slug == "leak"),
            "symlinked command file must be rejected"
        );
    }

    /// Regression: the settings "new skill" entry passed no etag, so `upsert_managed_skill`
    /// skipped its conflict check and silently replaced an existing `SKILL.md` (and forced it
    /// back to enabled). Create-only must refuse and leave BOTH content and enabled untouched.
    #[test]
    fn create_only_rejects_existing_slug_without_mutating_content_or_enabled() {
        let sb = TempSandbox::new("create-only-existing");
        let cwd_str = sb.workspace.to_string_lossy().to_string();

        let created = upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::User,
                cwd: Some(cwd_str.clone()),
                slug: "review".to_string(),
                name: "Review".to_string(),
                description: "Original description".to_string(),
                content: "# Review\n\nOriginal body worth protecting.".to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::MustNotExist,
            },
        )
        .expect("first create succeeds");
        let rec = created
            .iter()
            .find(|s| s.slug == "review")
            .expect("created record");

        // Disable it so we can prove a rejected create does not flip `enabled` back to true.
        toggle_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            &rec.id,
            false,
            Some(&cwd_str),
        )
        .expect("disable skill");

        let md_path = sb.orbit_home.join("skills").join("review").join("SKILL.md");
        let bytes_before = fs::read(&md_path).expect("read before");

        let err = upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::User,
                cwd: Some(cwd_str.clone()),
                slug: "review".to_string(),
                name: "Review".to_string(),
                description: "Clobbering description".to_string(),
                content: "# Review\n\nCLOBBERED BODY".to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::MustNotExist,
            },
        );
        assert!(
            matches!(err, Err(SkillManagerError::AlreadyExists(_))),
            "create-only on an existing slug must report AlreadyExists, got {err:?}"
        );

        // The whole point of the fix: nothing on disk moved.
        let bytes_after = fs::read(&md_path).expect("read after");
        assert_eq!(
            bytes_before, bytes_after,
            "rejected create must not rewrite SKILL.md"
        );
        assert!(
            String::from_utf8_lossy(&bytes_after).contains("Original body worth protecting"),
            "original body must survive"
        );
        assert!(
            !String::from_utf8_lossy(&bytes_after).contains("CLOBBERED"),
            "clobbering content must never reach disk"
        );

        let after = list_managed_skills(&sb.orbit_home, &sb.user_home, Some(&cwd_str));
        let rec_after = after
            .iter()
            .find(|s| s.slug == "review")
            .expect("record still present");
        assert!(
            !rec_after.enabled,
            "rejected create must not re-enable the existing skill"
        );
    }

    #[test]
    fn create_only_creates_when_absent_and_etag_update_still_works() {
        let sb = TempSandbox::new("create-only-absent");
        let cwd_str = sb.workspace.to_string_lossy().to_string();

        let created = upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::User,
                cwd: Some(cwd_str.clone()),
                slug: "fresh-skill".to_string(),
                name: "Fresh Skill".to_string(),
                description: "Brand new".to_string(),
                content: "# Fresh Skill\n\nBody.".to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::MustNotExist,
            },
        )
        .expect("create-only succeeds when target is absent");
        let rec = created
            .iter()
            .find(|s| s.slug == "fresh-skill")
            .expect("created record");
        assert_eq!(rec.id, "orbit_user:fresh-skill");
        assert!(rec.enabled);

        // Regression guard: the etag path must keep working after the enum migration.
        let read = read_managed_skill(&sb.orbit_home, &sb.user_home, &rec.id, Some(&cwd_str))
            .expect("read created");
        let updated = upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::User,
                cwd: Some(cwd_str.clone()),
                slug: "fresh-skill".to_string(),
                name: "Fresh Skill v2".to_string(),
                description: "Edited".to_string(),
                content: "---\nname: Fresh Skill v2\ndescription: Edited\n---\nV2 body\n"
                    .to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::MustMatch(read.etag),
            },
        )
        .expect("etag update still works");
        assert_eq!(
            updated
                .iter()
                .find(|s| s.slug == "fresh-skill")
                .expect("updated record")
                .name,
            "Fresh Skill v2"
        );

        // An unchecked write remains a blind upsert for non-UI callers (backward compatibility).
        upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::User,
                cwd: Some(cwd_str.clone()),
                slug: "fresh-skill".to_string(),
                name: "Fresh Skill v3".to_string(),
                description: "Blind".to_string(),
                content: "# Fresh Skill v3\n\nBlind overwrite.".to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::Unchecked,
            },
        )
        .expect("unchecked upsert keeps legacy overwrite behavior");
    }

    /// Regression: skill import did no existence check at all, overwrote the target, and the UI
    /// still reported "imported" because its `beforeIds` diff found no new id.
    #[test]
    fn import_create_only_rejects_existing_slug_without_mutating_content() {
        let sb = TempSandbox::new("import-create-only");
        let cwd_str = sb.workspace.to_string_lossy().to_string();

        upsert_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            UpsertSkillParams {
                scope: SkillWriteScope::User,
                cwd: Some(cwd_str.clone()),
                slug: "api-guide".to_string(),
                name: "Api Guide".to_string(),
                description: "Hand written".to_string(),
                content: "# Api Guide\n\nHand written body.".to_string(),
                enabled: true,
                precondition: SkillWritePrecondition::MustNotExist,
            },
        )
        .expect("seed existing skill");

        let md_path = sb
            .orbit_home
            .join("skills")
            .join("api-guide")
            .join("SKILL.md");
        let before = fs::read(&md_path).expect("read before");

        let err = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::User,
            Some(&cwd_str),
            "# Imported Guide\n\nIMPORTED BODY",
            Some("api-guide"),
            SkillWritePrecondition::MustNotExist,
        );
        assert!(
            matches!(err, Err(SkillManagerError::AlreadyExists(_))),
            "create-only import onto an existing slug must report AlreadyExists, got {err:?}"
        );
        assert_eq!(
            before,
            fs::read(&md_path).expect("read after"),
            "rejected import must not rewrite SKILL.md"
        );

        // A non-colliding slug still imports normally.
        let ok = import_managed_skill(
            &sb.orbit_home,
            &sb.user_home,
            SkillWriteScope::User,
            Some(&cwd_str),
            "# Imported Guide\n\nIMPORTED BODY",
            Some("api-guide-2"),
            SkillWritePrecondition::MustNotExist,
        )
        .expect("create-only import succeeds for a free slug");
        assert!(ok.iter().any(|s| s.id == "orbit_user:api-guide-2"));
    }

    #[test]
    fn precondition_from_wire_maps_create_only_and_etag() {
        assert_eq!(
            SkillWritePrecondition::from_wire(false, None),
            SkillWritePrecondition::Unchecked
        );
        // Blank etags were previously filtered out; keep treating them as "no precondition".
        assert_eq!(
            SkillWritePrecondition::from_wire(false, Some("   ".to_string())),
            SkillWritePrecondition::Unchecked
        );
        assert_eq!(
            SkillWritePrecondition::from_wire(false, Some("abc123".to_string())),
            SkillWritePrecondition::MustMatch("abc123".to_string())
        );
        assert_eq!(
            SkillWritePrecondition::from_wire(true, None),
            SkillWritePrecondition::MustNotExist
        );
        // create_only must win so a create can never degrade into a blind overwrite.
        assert_eq!(
            SkillWritePrecondition::from_wire(true, Some("abc123".to_string())),
            SkillWritePrecondition::MustNotExist
        );
    }
}
