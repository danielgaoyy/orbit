//! Pure-Rust Project Instruction Hierarchy Discovery, `@import` Resolution,
//! and Strict Security-Bounded Atomic Editor (`AGENTS.md`, `CLAUDE.md`, `AGENTS.override.md`,
//! `GEMINI.md`, `.cursorrules`, `.github/copilot-instructions.md`).

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use orbit_protocol::{InstructionFileEntry, InstructionScope};
use uuid::Uuid;

use crate::workspace_files::{
    canonicalize_workspace_root, is_metadata_writable, is_path_inside_git, sha256_hex,
};

/// Maximum readable/writable instruction file size (512 KiB).
pub const MAX_INSTRUCTION_FILE_BYTES: usize = 512 * 1024;
/// Maximum `@import` recursion depth (root file is depth 0; follows depth 1..=3).
pub const MAX_IMPORT_DEPTH: usize = 3;
/// Maximum resolved imports per instruction file.
const MAX_IMPORTS_PER_FILE: usize = 32;
/// Maximum preview characters per entry.
const PREVIEW_MAX_CHARS: usize = 220;
/// Maximum total bytes in the merged hierarchy preview.
const MERGED_PREVIEW_MAX_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone)]
pub struct InstructionHierarchyResult {
    pub files: Vec<InstructionFileEntry>,
    pub total_tokens: u32,
    pub merged_preview: String,
}

#[derive(Debug, Clone)]
pub struct InstructionReadResult {
    pub path: String,
    pub content: String,
    pub etag: Option<String>,
    pub exists: bool,
}

#[derive(Debug, Clone)]
pub struct InstructionSaveResult {
    pub path: String,
    pub etag: String,
    pub size_bytes: u64,
    pub approx_tokens: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum InstructionError {
    #[error("instruction file path rejected: {0}")]
    PathRejected(String),
    #[error("instruction file exceeds 512 KiB limit ({actual} bytes)")]
    FileTooLarge { actual: usize },
    #[error("instruction file changed on disk (current etag: {current_etag})")]
    EtagConflict { current_etag: String },
    #[error("{0}")]
    Io(String),
}

impl InstructionError {
    pub fn rpc_code(&self) -> &'static str {
        match self {
            Self::PathRejected(_) => "instruction_path_denied",
            Self::FileTooLarge { .. } => "instruction_too_large",
            Self::EtagConflict { .. } => "etag_conflict",
            Self::Io(_) => "instruction_io_failed",
        }
    }
}

struct AtomicTempGuard {
    path: PathBuf,
    armed: bool,
}

impl AtomicTempGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for AtomicTempGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Global user instruction specs in deterministic precedence order (lowest priority).
const GLOBAL_SPECS: &[(&str, &str, &str)] = &[
    (".orbit", "AGENTS.md", "orbit"),
    (".claude", "CLAUDE.md", "claude_code"),
    (".codex", "AGENTS.md", "codex"),
    (".gemini", "GEMINI.md", "gemini_cli"),
];

/// Standard project-level instruction filenames per directory in deterministic order.
/// Tuple: `(relative_subpath, filename_display, provider_hint)`
const PROJECT_SPECS: &[(&str, &str, &str)] = &[
    ("AGENTS.md", "AGENTS.md", "universal"),
    ("CLAUDE.md", "CLAUDE.md", "claude_code"),
    ("GEMINI.md", "GEMINI.md", "gemini_cli"),
    (".cursorrules", ".cursorrules", "cursor_agent"),
    (
        ".github/copilot-instructions.md",
        ".github/copilot-instructions.md",
        "copilot",
    ),
];

/// Local override instruction filenames (highest priority).
const OVERRIDE_SPECS: &[(&str, &str, &str)] = &[
    ("AGENTS.local.md", "AGENTS.local.md", "universal"),
    ("CLAUDE.local.md", "CLAUDE.local.md", "claude_code"),
    ("AGENTS.override.md", "AGENTS.override.md", "codex"),
];

/// Allowed document extensions for `@import` targets (strictly excludes `.json`/`.yaml`/`.toml`/dotfiles).
const ALLOWED_IMPORT_DOC_EXTENSIONS: &[&str] = &[".md", ".mdx", ".txt", ".rules", ".mdc"];

/// Sensitive directories inside `$HOME` that `@import` must NEVER read even when home-contained.
const FORBIDDEN_HOME_DIRS: &[&str] = &[".ssh", ".gnupg", ".aws", ".kube", ".docker", ".git"];

#[derive(Debug, Clone)]
struct CandidateSlot {
    id: String,
    full_path: PathBuf,
    allowed_root: PathBuf,
    relative_path: String,
    filename: String,
    scope: InstructionScope,
    provider_hint: Option<String>,
    include_if_missing: bool,
    is_global: bool,
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
    )
}

/// Estimate approximate LLM token footprint (UTF-8 & CJK aware).
pub fn estimate_approx_tokens(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    let mut cjk_chars: u64 = 0;
    let mut other_chars: u64 = 0;
    for ch in text.chars() {
        if is_cjk_char(ch) {
            cjk_chars += 1;
        } else {
            other_chars += 1;
        }
    }
    // CJK characters average ~1.25 tokens each (5/4); Latin/code averages ~0.25 tokens/char (1/4).
    let weighted_quarters = cjk_chars.saturating_mul(5).saturating_add(other_chars);
    let tokens = weighted_quarters.div_ceil(4);
    u32::try_from(tokens.max(1)).unwrap_or(u32::MAX)
}

/// Provide a clean, restrained starter template for newly created instruction files.
pub fn instruction_template_for_filename(filename: &str) -> &'static str {
    match filename {
        "AGENTS.md" => {
            "# AGENTS.md\n\n\
             ## Architecture & Conventions\n\
             - Keep changes focused, minimal, and consistent with existing patterns.\n\
             - Add or update unit tests for every behavioral change.\n\n\
             ## Verification\n\
             - Run the project's lint, typecheck, and test suite before finishing.\n"
        }
        "CLAUDE.md" => {
            "# CLAUDE.md\n\n\
             ## Project Overview\n\
             - Describe the core stack, directory layout, and entrypoints.\n\n\
             ## Commands\n\
             - Build: `...`\n\
             - Test: `...`\n\n\
             ## Working Agreements\n\
             - Follow existing code style and avoid unrelated edits.\n"
        }
        "AGENTS.override.md" | "AGENTS.local.md" | "CLAUDE.local.md" => {
            "# Local Overrides\n\n\
             <!-- Personal or machine-specific instructions (highest priority; typically gitignored). -->\n\
             - Prefer concise diffs and verify changes locally.\n"
        }
        "GEMINI.md" => {
            "# GEMINI.md\n\n\
             ## Workspace Guidance\n\
             - Summarize project conventions and preferred verification commands.\n"
        }
        ".cursorrules" => {
            "# Project Rules\n\
             - Follow repository conventions and keep edits tightly scoped.\n"
        }
        ".github/copilot-instructions.md" => {
            "# Copilot Instructions\n\n\
             - Follow the repository coding guidelines and test conventions.\n"
        }
        _ => "# Instructions\n\n- Add project-specific agent guidance here.\n",
    }
}

fn resolve_default_home() -> Option<PathBuf> {
    crate::platform::resolve_user_home_dir()
}

fn canonicalize_optional_home(home: Option<&Path>) -> Option<PathBuf> {
    let raw = home?;
    crate::platform::canonicalize_clean(raw).ok().or_else(|| {
        if raw.is_absolute() {
            Some(raw.to_path_buf())
        } else {
            None
        }
    })
}

/// Find the enclosing Git repository root (supports normal `.git/` dirs and worktree `.git` files).
/// Stops BEFORE `$HOME` and filesystem root so a `~/.git` dotfiles repo never turns `$HOME` into
/// a workspace containment root.
fn find_git_repo_root(canonical_cwd: &Path, canon_home: Option<&Path>) -> Option<PathBuf> {
    let mut cur = canonical_cwd.to_path_buf();
    loop {
        if cur.parent().is_none() || canon_home.is_some_and(|h| cur == h) {
            return None;
        }
        let dot_git = cur.join(".git");
        if dot_git.exists() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Build the canonical directory chain from `repo_root` (if found) down to `canonical_cwd`.
/// Returns `(repo_root_opt, chain_root_to_cwd)`.
fn build_workspace_dir_chain(
    canonical_cwd: &Path,
    canon_home: Option<&Path>,
) -> (Option<PathBuf>, Vec<PathBuf>) {
    let Some(repo_root) = find_git_repo_root(canonical_cwd, canon_home) else {
        return (None, vec![canonical_cwd.to_path_buf()]);
    };
    let mut chain = Vec::new();
    let mut cur = canonical_cwd.to_path_buf();
    loop {
        chain.push(cur.clone());
        if cur == repo_root {
            break;
        }
        if !cur.pop() {
            break;
        }
    }
    chain.reverse();
    (Some(repo_root), chain)
}

fn format_relative_display(
    path: &Path,
    repo_root: Option<&Path>,
    canonical_cwd: &Path,
    home_dir: Option<&Path>,
) -> String {
    if let Some(root) = repo_root
        && let Ok(rel) = path.strip_prefix(root)
    {
        return rel.to_string_lossy().replace('\\', "/");
    }
    if let Ok(rel) = path.strip_prefix(canonical_cwd) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    if let Some(home) = home_dir
        && let Ok(rel) = path.strip_prefix(home)
    {
        return format!("~/{}", rel.to_string_lossy().replace('\\', "/"));
    }
    path.to_string_lossy().replace('\\', "/")
}

fn build_candidate_slots(
    canonical_cwd: &Path,
    home_dir: Option<&Path>,
) -> (Option<PathBuf>, Vec<CandidateSlot>) {
    let (repo_root, chain) = build_workspace_dir_chain(canonical_cwd, home_dir);
    let mut slots = Vec::new();

    // 1. Global user instructions (lowest precedence)
    if let Some(home) = home_dir {
        for &(dir_name, file_name, provider) in GLOBAL_SPECS {
            let allowed_root = home.join(dir_name);
            let full_path = allowed_root.join(file_name);
            let rel = format!("~/{dir_name}/{file_name}");
            slots.push(CandidateSlot {
                id: format!("global:{dir_name}/{file_name}"),
                full_path,
                allowed_root,
                relative_path: rel,
                filename: file_name.to_string(),
                scope: InstructionScope::GlobalUser,
                provider_hint: Some(provider.to_string()),
                include_if_missing: dir_name == ".orbit" && file_name == "AGENTS.md",
                is_global: true,
            });
        }
    }

    // 2. Repo root -> Intermediate Ancestors -> Workspace Dir (standard instruction files)
    let chain_len = chain.len();
    for (idx, dir) in chain.iter().enumerate() {
        let is_repo_root = repo_root.as_deref() == Some(dir.as_path());
        let is_workspace_dir = idx + 1 == chain_len;
        let scope = if is_repo_root {
            InstructionScope::RepoRoot
        } else if is_workspace_dir {
            InstructionScope::WorkspaceDir
        } else {
            InstructionScope::AncestorDir
        };

        for &(subpath, filename, provider) in PROJECT_SPECS {
            let full_path = dir.join(subpath);
            let relative_path =
                format_relative_display(&full_path, repo_root.as_deref(), canonical_cwd, home_dir);
            let include_if_missing =
                is_workspace_dir && matches!(subpath, "AGENTS.md" | "CLAUDE.md");
            slots.push(CandidateSlot {
                id: format!("{:?}:{relative_path}", scope).to_ascii_lowercase(),
                full_path,
                allowed_root: dir.clone(),
                relative_path,
                filename: filename.to_string(),
                scope,
                provider_hint: Some(provider.to_string()),
                include_if_missing,
                is_global: false,
            });
        }
    }

    // 3. Local overrides (highest priority: RepoRoot -> Ancestors -> WorkspaceDir)
    for (idx, dir) in chain.iter().enumerate() {
        let is_workspace_dir = idx + 1 == chain_len;
        for &(subpath, filename, provider) in OVERRIDE_SPECS {
            let full_path = dir.join(subpath);
            let relative_path =
                format_relative_display(&full_path, repo_root.as_deref(), canonical_cwd, home_dir);
            let include_if_missing = is_workspace_dir && subpath == "AGENTS.override.md";
            slots.push(CandidateSlot {
                id: format!("local_override:{relative_path}"),
                full_path,
                allowed_root: dir.clone(),
                relative_path,
                filename: filename.to_string(),
                scope: InstructionScope::LocalOverride,
                provider_hint: Some(provider.to_string()),
                include_if_missing,
                is_global: false,
            });
        }
    }

    (repo_root, slots)
}

fn make_preview(content: &str) -> String {
    let mut collected = String::new();
    for raw_line in content.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }
        if !collected.is_empty() {
            collected.push(' ');
        }
        collected.push_str(trimmed);
        if collected.chars().count() >= PREVIEW_MAX_CHARS {
            break;
        }
    }
    if collected.chars().count() > PREVIEW_MAX_CHARS {
        let truncated: String = collected.chars().take(PREVIEW_MAX_CHARS).collect();
        format!("{}…", truncated.trim_end())
    } else {
        collected
    }
}

/// Extract candidate `@path/to/file.md` import directives from Markdown content,
/// ignoring fenced code blocks (` ``` ` / `~~~`) and inline backtick code spans.
pub fn parse_import_directives(content: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    let mut in_fence = false;
    let mut fence_marker: Option<&'static str> = None;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();
        if let Some(marker) = fence_marker {
            if trimmed.starts_with(marker) {
                in_fence = false;
                fence_marker = None;
            }
            continue;
        } else if trimmed.starts_with("```") {
            in_fence = true;
            fence_marker = Some("```");
            continue;
        } else if trimmed.starts_with("~~~") {
            in_fence = true;
            fence_marker = Some("~~~");
            continue;
        }
        if in_fence {
            continue;
        }

        // Scan line while skipping inline backtick code spans.
        let bytes = raw_line.as_bytes();
        let mut i = 0usize;
        let mut in_inline_code = false;

        while i < bytes.len() {
            if bytes[i] == b'`' {
                in_inline_code = !in_inline_code;
                i += 1;
                continue;
            }
            if !in_inline_code && bytes[i] == b'@' {
                let prev_ok = if i == 0 {
                    true
                } else {
                    let prev = bytes[i - 1];
                    prev.is_ascii_whitespace() || matches!(prev, b'(' | b'[' | b'-' | b'*' | b'>')
                };
                if prev_ok {
                    let start = i + 1;
                    let mut end = start;
                    while end < bytes.len() {
                        let b = bytes[end];
                        if b.is_ascii_whitespace() || matches!(b, b'`' | b'"' | b'\'' | b'<' | b'>')
                        {
                            break;
                        }
                        end += 1;
                    }
                    if end > start {
                        let raw_tok = &raw_line[start..end];
                        let cleaned = raw_tok
                            .trim_end_matches([',', ';', ':', ')', ']', '.', '!'])
                            .trim();
                        if is_plausible_import_path(cleaned) && seen.insert(cleaned.to_string()) {
                            results.push(cleaned.to_string());
                            if results.len() >= MAX_IMPORTS_PER_FILE {
                                return results;
                            }
                        }
                    }
                    i = end;
                    continue;
                }
            }
            i += 1;
        }
    }

    results
}

fn has_allowed_import_doc_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ALLOWED_IMPORT_DOC_EXTENSIONS
        .iter()
        .any(|ext| lower.ends_with(ext))
}

fn is_plausible_import_path(raw: &str) -> bool {
    if raw.is_empty() || raw.len() > 512 {
        return false;
    }
    if raw.chars().any(|c| c == '\0' || c.is_control()) {
        return false;
    }
    if raw.contains("://") {
        return false;
    }
    // Strictly restrict `@import` directives to document extensions (`.md`, `.mdx`, `.txt`, `.rules`, `.mdc`).
    has_allowed_import_doc_extension(raw)
}

fn path_has_forbidden_home_segment(path: &Path, home_dir: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(home_dir) else {
        return false;
    };
    rel.components().any(|comp| {
        matches!(
            comp,
            Component::Normal(seg)
                if FORBIDDEN_HOME_DIRS
                    .iter()
                    .any(|f| seg.to_string_lossy().eq_ignore_ascii_case(f))
        )
    })
}

/// Returns `true` ONLY when `canon_target` is strictly contained within one of the verified,
/// non-symlinked global config directories (`~/.orbit`, `~/.claude`, `~/.codex`, `~/.gemini`).
fn is_allowed_global_config_path(canon_target: &Path, home_dir: &Path) -> bool {
    GLOBAL_SPECS.iter().any(|&(dir_name, _, _)| {
        let dir = home_dir.join(dir_name);
        let Ok(meta) = fs::symlink_metadata(&dir) else {
            return false;
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return false;
        }
        let Ok(canon_dir) = crate::platform::canonicalize_clean(&dir) else {
            return false;
        };
        canon_dir.starts_with(home_dir) && canon_target.starts_with(&canon_dir)
    })
}

fn is_import_target_contained(
    canon_target: &Path,
    importing_is_global: bool,
    containment_root: &Path,
    home_dir: Option<&Path>,
) -> bool {
    let Some(file_name) = canon_target.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !has_allowed_import_doc_extension(file_name) {
        return false;
    }
    if is_path_inside_git(containment_root, canon_target) {
        return false;
    }
    if home_dir.is_some_and(|home| path_has_forbidden_home_segment(canon_target, home)) {
        return false;
    }

    if importing_is_global {
        // Global user instruction files may ONLY import doc files inside the verified,
        // non-symlinked global config dirs (`~/.orbit`, `~/.claude`, `~/.codex`, `~/.gemini`),
        // NEVER arbitrary `$HOME` files like `~/.netrc`, `~/.npmrc`, or `~/.git-credentials`.
        home_dir.is_some_and(|home| is_allowed_global_config_path(canon_target, home))
    } else {
        // Repo/workspace instruction files may ONLY import doc files strictly inside `containment_root`
        // (never `$HOME` or global config directories).
        canon_target.starts_with(containment_root)
    }
}

struct ResolvedImport {
    display_path: String,
    content: String,
}

struct ImportResolutionCtx<'a> {
    containment_root: &'a Path,
    repo_root: Option<&'a Path>,
    canonical_cwd: &'a Path,
    home_dir: Option<&'a Path>,
}

/// Open a regular file safely with bounded read (at most `MAX_INSTRUCTION_FILE_BYTES`) and
/// `u64` length checks to prevent memory exhaustion and 32-bit truncation.
fn read_open_file_bounded(
    path: &Path,
    reject_symlink_leaf: bool,
) -> Result<Option<Vec<u8>>, InstructionError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(InstructionError::Io(format!("Failed inspecting file: {e}"))),
    };
    if reject_symlink_leaf && meta.file_type().is_symlink() {
        return Err(InstructionError::PathRejected(
            "Refusing to access instruction file through symlink".to_string(),
        ));
    }
    if !meta.is_file() {
        return Err(InstructionError::PathRejected(
            "Instruction target is not a regular file".to_string(),
        ));
    }
    if meta.len() > MAX_INSTRUCTION_FILE_BYTES as u64 {
        return Err(InstructionError::FileTooLarge {
            actual: usize::try_from(meta.len()).unwrap_or(usize::MAX),
        });
    }

    let file = match OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(InstructionError::Io(format!("Failed opening file: {e}"))),
    };
    let fd_meta = file
        .metadata()
        .map_err(|e| InstructionError::Io(format!("Failed inspecting open file: {e}")))?;
    if !fd_meta.is_file() {
        return Err(InstructionError::PathRejected(
            "Instruction target is not a regular file".to_string(),
        ));
    }
    if fd_meta.len() > MAX_INSTRUCTION_FILE_BYTES as u64 {
        return Err(InstructionError::FileTooLarge {
            actual: usize::try_from(fd_meta.len()).unwrap_or(usize::MAX),
        });
    }

    let mut bytes = Vec::with_capacity((fd_meta.len() as usize).min(MAX_INSTRUCTION_FILE_BYTES));
    file.take((MAX_INSTRUCTION_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| InstructionError::Io(format!("Failed reading file: {e}")))?;
    if bytes.len() > MAX_INSTRUCTION_FILE_BYTES {
        return Err(InstructionError::FileTooLarge {
            actual: bytes.len(),
        });
    }
    Ok(Some(bytes))
}

fn resolve_imports_recursive(
    ctx: &ImportResolutionCtx<'_>,
    base_file: &Path,
    content: &str,
    importing_is_global: bool,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    out: &mut Vec<ResolvedImport>,
) {
    if depth >= MAX_IMPORT_DEPTH || out.len() >= MAX_IMPORTS_PER_FILE {
        return;
    }
    let Some(base_dir) = base_file.parent() else {
        return;
    };

    for raw_ref in parse_import_directives(content) {
        if out.len() >= MAX_IMPORTS_PER_FILE {
            break;
        }
        let candidate = if let Some(rest) = raw_ref.strip_prefix("~/") {
            let Some(home) = ctx.home_dir else {
                continue;
            };
            home.join(rest)
        } else {
            let p = Path::new(&raw_ref);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                base_dir.join(p)
            }
        };

        let Ok(canon_target) = crate::platform::canonicalize_clean(&candidate) else {
            continue;
        };
        if !is_import_target_contained(
            &canon_target,
            importing_is_global,
            ctx.containment_root,
            ctx.home_dir,
        ) {
            continue;
        }
        if !visited.insert(canon_target.clone()) {
            // Cycle detected or already expanded for this instruction root
            continue;
        }
        let Ok(Some(bytes)) = read_open_file_bounded(&canon_target, true) else {
            continue;
        };
        if bytes.contains(&0) {
            continue;
        }
        let Ok(import_text) = String::from_utf8(bytes) else {
            continue;
        };

        let display_path = format_relative_display(
            &canon_target,
            ctx.repo_root,
            ctx.canonical_cwd,
            ctx.home_dir,
        );
        out.push(ResolvedImport {
            display_path,
            content: import_text.clone(),
        });

        resolve_imports_recursive(
            ctx,
            &canon_target,
            &import_text,
            importing_is_global,
            depth + 1,
            visited,
            out,
        );
    }
}

/// Read an existing instruction file safely without following symlinks and enforcing size/UTF-8 bounds.
fn read_regular_utf8_bounded(path: &Path) -> Result<Option<(String, Vec<u8>)>, InstructionError> {
    let Some(bytes) = read_open_file_bounded(path, true)? else {
        return Ok(None);
    };
    if bytes.contains(&0) {
        return Err(InstructionError::PathRejected(
            "Instruction file must be valid UTF-8 text".to_string(),
        ));
    }
    let text = String::from_utf8(bytes.clone())
        .map_err(|_| InstructionError::PathRejected("Instruction file is not UTF-8".to_string()))?;
    Ok(Some((text, bytes)))
}

fn push_bounded_preview(out: &mut String, piece: &str, truncated: &mut bool) {
    if *truncated || out.len() >= MERGED_PREVIEW_MAX_BYTES {
        *truncated = true;
        return;
    }
    let remaining = MERGED_PREVIEW_MAX_BYTES.saturating_sub(out.len());
    if piece.len() <= remaining {
        out.push_str(piece);
    } else {
        let mut cut = remaining;
        while cut > 0 && !piece.is_char_boundary(cut) {
            cut -= 1;
        }
        out.push_str(&piece[..cut]);
        out.push_str("\n… (truncated)");
        *truncated = true;
    }
}

/// Discover the full effective instruction hierarchy for `cwd` using the process `HOME`.
pub async fn discover_instruction_hierarchy(
    cwd: &Path,
) -> Result<InstructionHierarchyResult, InstructionError> {
    let cwd_owned = cwd.to_path_buf();
    let home = resolve_default_home();
    tokio::task::spawn_blocking(move || {
        discover_instruction_hierarchy_with_home(&cwd_owned, home.as_deref())
    })
    .await
    .map_err(|_| InstructionError::Io("Instruction discovery task cancelled".to_string()))?
}

/// Pure synchronous discovery with explicit `home_dir` (deterministic and test-friendly).
pub fn discover_instruction_hierarchy_with_home(
    cwd: &Path,
    home_dir: Option<&Path>,
) -> Result<InstructionHierarchyResult, InstructionError> {
    let canonical_cwd = canonicalize_workspace_root(cwd).map_err(InstructionError::PathRejected)?;
    let canon_home = canonicalize_optional_home(home_dir);
    let (repo_root, candidates) = build_candidate_slots(&canonical_cwd, canon_home.as_deref());
    let containment_root = repo_root.as_deref().unwrap_or(&canonical_cwd);

    let mut files = Vec::new();
    let mut total_tokens: u32 = 0;
    let mut merged_preview = String::new();
    let mut merged_truncated = false;
    let mut seen_canon_paths = HashSet::new();
    let mut precedence_order: u32 = 0;

    for slot in candidates {
        // Validate parent directory before reading (blocks symlinked `.github` or `~/.claude` etc.)
        let Ok(safe_parent) = ensure_safe_parent_dir(
            &slot.full_path,
            &slot.allowed_root,
            slot.is_global,
            canon_home.as_deref(),
            false,
        ) else {
            continue;
        };
        let Some(file_name) = slot.full_path.file_name() else {
            continue;
        };
        let safe_target = safe_parent.join(file_name);

        // Check whether file exists (and avoid duplicate entries if e.g. repo == home subdir)
        let read_opt = match read_regular_utf8_bounded(&safe_target) {
            Ok(opt) => opt,
            Err(_) => continue,
        };

        if let Some((content, raw_bytes)) = read_opt {
            let canon_file =
                crate::platform::canonicalize_clean(&safe_target).unwrap_or(safe_target.clone());
            if !seen_canon_paths.insert(canon_file.clone()) {
                continue;
            }
            precedence_order += 1;

            let mut visited = HashSet::new();
            visited.insert(canon_file.clone());
            let mut resolved_imports = Vec::new();
            let import_ctx = ImportResolutionCtx {
                containment_root,
                repo_root: repo_root.as_deref(),
                canonical_cwd: &canonical_cwd,
                home_dir: canon_home.as_deref(),
            };
            resolve_imports_recursive(
                &import_ctx,
                &canon_file,
                &content,
                slot.is_global,
                0,
                &mut visited,
                &mut resolved_imports,
            );

            let mut entry_tokens = estimate_approx_tokens(&content);
            for imp in &resolved_imports {
                entry_tokens = entry_tokens.saturating_add(estimate_approx_tokens(&imp.content));
            }
            total_tokens = total_tokens.saturating_add(entry_tokens);

            let etag = sha256_hex(&raw_bytes);
            let preview = make_preview(&content);
            let import_paths: Vec<String> = resolved_imports
                .iter()
                .map(|r| r.display_path.clone())
                .collect();

            if !merged_truncated {
                if !merged_preview.is_empty() {
                    push_bounded_preview(&mut merged_preview, "\n\n---\n\n", &mut merged_truncated);
                }
                let header = format!(
                    "<!-- [{precedence_order}] {} · {} (~{} tokens) -->\n",
                    slot.scope.badge_label(),
                    slot.relative_path,
                    entry_tokens,
                );
                push_bounded_preview(&mut merged_preview, &header, &mut merged_truncated);
                push_bounded_preview(
                    &mut merged_preview,
                    content.trim_end(),
                    &mut merged_truncated,
                );
                for imp in &resolved_imports {
                    if merged_truncated {
                        break;
                    }
                    let imp_header = format!("\n\n<!-- @import {} -->\n", imp.display_path);
                    push_bounded_preview(&mut merged_preview, &imp_header, &mut merged_truncated);
                    push_bounded_preview(
                        &mut merged_preview,
                        imp.content.trim_end(),
                        &mut merged_truncated,
                    );
                }
            }

            files.push(InstructionFileEntry {
                id: slot.id,
                path: canon_file.to_string_lossy().to_string(),
                relative_path: slot.relative_path,
                filename: slot.filename,
                scope: slot.scope,
                provider_hint: slot.provider_hint,
                precedence_order,
                exists: true,
                size_bytes: raw_bytes.len() as u64,
                approx_tokens: entry_tokens,
                etag: Some(etag),
                preview,
                imports: import_paths,
            });
        } else if slot.include_if_missing {
            precedence_order += 1;
            files.push(InstructionFileEntry {
                id: slot.id,
                path: safe_target.to_string_lossy().to_string(),
                relative_path: slot.relative_path,
                filename: slot.filename,
                scope: slot.scope,
                provider_hint: slot.provider_hint,
                precedence_order,
                exists: false,
                size_bytes: 0,
                approx_tokens: 0,
                etag: None,
                preview: String::new(),
                imports: Vec::new(),
            });
        }
    }

    Ok(InstructionHierarchyResult {
        files,
        total_tokens,
        merged_preview,
    })
}

/// Build the exact set of allowlisted instruction file targets for `(canonical_cwd, canon_home)`.
/// Maps normalized path -> `(full_path, allowed_root_dir, is_global)`.
fn build_allowlist_map(
    canonical_cwd: &Path,
    canon_home: Option<&Path>,
) -> HashMap<PathBuf, (PathBuf, PathBuf, bool)> {
    let (_repo_root, chain) = build_workspace_dir_chain(canonical_cwd, canon_home);
    let mut map = HashMap::new();

    if let Some(home) = canon_home {
        for &(dir_name, file_name, _) in GLOBAL_SPECS {
            let root_dir = home.join(dir_name);
            let full = root_dir.join(file_name);
            map.insert(full.clone(), (full, root_dir, true));
        }
    }

    for dir in &chain {
        for &(subpath, _, _) in PROJECT_SPECS {
            let full = dir.join(subpath);
            map.insert(full.clone(), (full, dir.clone(), false));
        }
        for &(subpath, _, _) in OVERRIDE_SPECS {
            let full = dir.join(subpath);
            map.insert(full.clone(), (full, dir.clone(), false));
        }
    }

    map
}

/// Validate and resolve a requested instruction file `raw_path` strictly against the allowlist.
/// Rejects traversal (`..`), control chars, backslashes, arbitrary paths, and symlink escapes.
fn resolve_allowlisted_instruction_path(
    canonical_cwd: &Path,
    canon_home: Option<&Path>,
    raw_path: &str,
) -> Result<(PathBuf, PathBuf, bool), InstructionError> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(InstructionError::PathRejected(
            "Instruction file path cannot be empty".to_string(),
        ));
    }
    if trimmed.chars().any(|c| c == '\0' || c.is_control()) {
        return Err(InstructionError::PathRejected(
            "Instruction file path contains invalid control characters".to_string(),
        ));
    }
    #[cfg(unix)]
    if trimmed.contains('\\') {
        return Err(InstructionError::PathRejected(
            "Instruction file path must use forward slashes".to_string(),
        ));
    }

    let expanded = if let Some(rest) = trimmed.strip_prefix("~/") {
        let Some(home) = canon_home else {
            return Err(InstructionError::PathRejected(
                "Home directory unavailable for global instruction path".to_string(),
            ));
        };
        home.join(rest)
    } else {
        let p = Path::new(trimmed);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            canonical_cwd.join(p)
        }
    };

    // Reject any `..` traversal component lexically before allowlist matching
    for comp in expanded.components() {
        if matches!(comp, Component::ParentDir) {
            return Err(InstructionError::PathRejected(format!(
                "Path traversal is forbidden in `{trimmed}`"
            )));
        }
    }

    // Normalize CurDir `.` away cleanly without touching filesystem
    let mut normalized = PathBuf::new();
    for comp in expanded.components() {
        match comp {
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }

    let allowlist = build_allowlist_map(canonical_cwd, canon_home);

    // Direct match on normalized path
    if let Some(found) = allowlist.get(&normalized) {
        return Ok(found.clone());
    }

    // Also try matching after canonicalizing the parent directory (handles macOS `/var` -> `/private/var` symlinks)
    if let (Some(parent), Some(file_name)) = (normalized.parent(), normalized.file_name())
        && let Ok(canon_parent) = crate::platform::canonicalize_clean(parent)
    {
        let resolved = canon_parent.join(file_name);
        if let Some(found) = allowlist.get(&resolved) {
            return Ok(found.clone());
        }
    }

    Err(InstructionError::PathRejected(format!(
        "`{trimmed}` is not an allowlisted instruction file in this workspace or user config"
    )))
}

/// Ensure parent directory exists (creating `~/.orbit`, `~/.claude`, `~/.codex`, `~/.gemini`, or `.github`
/// safely if missing) and verify no symlink escapes the allowed root directory.
fn ensure_safe_parent_dir(
    target: &Path,
    allowed_root: &Path,
    is_global: bool,
    canon_home: Option<&Path>,
    create_missing: bool,
) -> Result<PathBuf, InstructionError> {
    let parent = target.parent().ok_or_else(|| {
        InstructionError::PathRejected("Instruction target has no parent directory".to_string())
    })?;

    if is_global {
        let home = canon_home.ok_or_else(|| {
            InstructionError::PathRejected("Home directory unavailable".to_string())
        })?;
        // For global config files, `allowed_root` is e.g. `<home>/.orbit` and `parent == allowed_root`.
        match fs::symlink_metadata(allowed_root) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(InstructionError::PathRejected(
                        "Refusing to traverse symlinked config directory".to_string(),
                    ));
                }
                if !meta.is_dir() {
                    return Err(InstructionError::PathRejected(
                        "Config path is not a directory".to_string(),
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !create_missing {
                    return Ok(parent.to_path_buf());
                }
                fs::create_dir(allowed_root).map_err(|ce| {
                    InstructionError::Io(format!("Failed creating config directory: {ce}"))
                })?;
            }
            Err(e) => {
                return Err(InstructionError::Io(format!(
                    "Failed inspecting config directory: {e}"
                )));
            }
        }
        let canon_parent = crate::platform::canonicalize_clean(allowed_root).map_err(|e| {
            InstructionError::Io(format!("Cannot canonicalize config directory: {e}"))
        })?;
        if !canon_parent.starts_with(home) {
            return Err(InstructionError::PathRejected(
                "Config directory escapes home directory".to_string(),
            ));
        }
        return Ok(canon_parent);
    }

    // Workspace/repo/ancestor file: `parent` is either `allowed_root` or `allowed_root/.github`
    let canon_root = crate::platform::canonicalize_clean(allowed_root)
        .map_err(|e| InstructionError::Io(format!("Cannot canonicalize workspace root: {e}")))?;
    if parent == allowed_root {
        return Ok(canon_root);
    }

    // Must be `.github` subdirectory directly under `allowed_root`
    let rel = parent.strip_prefix(allowed_root).map_err(|_| {
        InstructionError::PathRejected("Parent directory escapes allowed root".to_string())
    })?;
    if rel != Path::new(".github") {
        return Err(InstructionError::PathRejected(
            "Unexpected instruction subdirectory".to_string(),
        ));
    }

    let github_dir = canon_root.join(".github");
    match fs::symlink_metadata(&github_dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(InstructionError::PathRejected(
                    "Refusing to traverse symlinked `.github` directory".to_string(),
                ));
            }
            if !meta.is_dir() {
                return Err(InstructionError::PathRejected(
                    "`.github` is not a directory".to_string(),
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !create_missing {
                return Ok(github_dir);
            }
            fs::create_dir(&github_dir).map_err(|ce| {
                InstructionError::Io(format!("Failed creating `.github` directory: {ce}"))
            })?;
        }
        Err(e) => {
            return Err(InstructionError::Io(format!(
                "Failed inspecting `.github` directory: {e}"
            )));
        }
    }

    let canon_github = crate::platform::canonicalize_clean(&github_dir)
        .map_err(|e| InstructionError::Io(format!("Cannot canonicalize `.github`: {e}")))?;
    if !canon_github.starts_with(&canon_root) || is_path_inside_git(&canon_root, &canon_github) {
        return Err(InstructionError::PathRejected(
            "`.github` escapes workspace boundary".to_string(),
        ));
    }
    Ok(canon_github)
}

/// Read an allowlisted instruction file using the process `HOME`.
pub async fn read_instruction_file(
    cwd: &Path,
    raw_path: &str,
) -> Result<InstructionReadResult, InstructionError> {
    let cwd_owned = cwd.to_path_buf();
    let path_owned = raw_path.to_string();
    let home = resolve_default_home();
    tokio::task::spawn_blocking(move || {
        read_instruction_file_with_home(&cwd_owned, &path_owned, home.as_deref())
    })
    .await
    .map_err(|_| InstructionError::Io("Instruction read task cancelled".to_string()))?
}

/// Synchronous read with explicit `home_dir`.
pub fn read_instruction_file_with_home(
    cwd: &Path,
    raw_path: &str,
    home_dir: Option<&Path>,
) -> Result<InstructionReadResult, InstructionError> {
    let canonical_cwd = canonicalize_workspace_root(cwd).map_err(InstructionError::PathRejected)?;
    let canon_home = canonicalize_optional_home(home_dir);
    let (target, allowed_root, is_global) =
        resolve_allowlisted_instruction_path(&canonical_cwd, canon_home.as_deref(), raw_path)?;

    let safe_parent = ensure_safe_parent_dir(
        &target,
        &allowed_root,
        is_global,
        canon_home.as_deref(),
        false,
    )?;
    let file_name = target.file_name().ok_or_else(|| {
        InstructionError::PathRejected("Missing instruction filename".to_string())
    })?;
    let safe_target = safe_parent.join(file_name);

    match read_regular_utf8_bounded(&safe_target)? {
        Some((content, bytes)) => Ok(InstructionReadResult {
            path: safe_target.to_string_lossy().to_string(),
            content,
            etag: Some(sha256_hex(&bytes)),
            exists: true,
        }),
        None => Ok(InstructionReadResult {
            path: safe_target.to_string_lossy().to_string(),
            content: String::new(),
            etag: None,
            exists: false,
        }),
    }
}

/// Atomically save an allowlisted instruction file with optimistic `expected_etag` concurrency check.
pub async fn save_instruction_file(
    cwd: &Path,
    raw_path: &str,
    content: &str,
    expected_etag: Option<&str>,
) -> Result<InstructionSaveResult, InstructionError> {
    if content.len() > MAX_INSTRUCTION_FILE_BYTES {
        return Err(InstructionError::FileTooLarge {
            actual: content.len(),
        });
    }
    let cwd_owned = cwd.to_path_buf();
    let path_owned = raw_path.to_string();
    let content_owned = content.to_string();
    let expected_owned = expected_etag
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let home = resolve_default_home();

    tokio::task::spawn_blocking(move || {
        save_instruction_file_with_home(
            &cwd_owned,
            &path_owned,
            &content_owned,
            expected_owned.as_deref(),
            home.as_deref(),
        )
    })
    .await
    .map_err(|_| InstructionError::Io("Instruction save task cancelled".to_string()))?
}

/// Synchronous atomic save with explicit `home_dir`.
pub fn save_instruction_file_with_home(
    cwd: &Path,
    raw_path: &str,
    content: &str,
    expected_etag: Option<&str>,
    home_dir: Option<&Path>,
) -> Result<InstructionSaveResult, InstructionError> {
    if content.len() > MAX_INSTRUCTION_FILE_BYTES {
        return Err(InstructionError::FileTooLarge {
            actual: content.len(),
        });
    }
    let canonical_cwd = canonicalize_workspace_root(cwd).map_err(InstructionError::PathRejected)?;
    let canon_home = canonicalize_optional_home(home_dir);
    let (target, allowed_root, is_global) =
        resolve_allowlisted_instruction_path(&canonical_cwd, canon_home.as_deref(), raw_path)?;

    let safe_parent = ensure_safe_parent_dir(
        &target,
        &allowed_root,
        is_global,
        canon_home.as_deref(),
        true,
    )?;
    let file_name = target.file_name().ok_or_else(|| {
        InstructionError::PathRejected("Missing instruction filename".to_string())
    })?;
    let safe_target = safe_parent.join(file_name);

    #[cfg(unix)]
    let mut preserved_mode: Option<u32> = None;

    match fs::symlink_metadata(&safe_target) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(InstructionError::PathRejected(
                    "Refusing to write instruction file through symlink".to_string(),
                ));
            }
            if !meta.is_file() {
                return Err(InstructionError::PathRejected(
                    "Instruction target is not a regular file".to_string(),
                ));
            }
            if !is_metadata_writable(&meta) {
                return Err(InstructionError::PathRejected(
                    "Instruction file is read-only".to_string(),
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                preserved_mode = Some(meta.permissions().mode() & 0o0777);
            }
            let existing = read_open_file_bounded(&safe_target, true)?.ok_or_else(|| {
                InstructionError::EtagConflict {
                    current_etag: String::new(),
                }
            })?;
            let current_etag = sha256_hex(&existing);
            if expected_etag != Some(current_etag.as_str()) {
                return Err(InstructionError::EtagConflict { current_etag });
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if expected_etag.is_some() {
                return Err(InstructionError::EtagConflict {
                    current_etag: String::new(),
                });
            }
        }
        Err(e) => {
            return Err(InstructionError::Io(format!(
                "Failed inspecting target instruction file: {e}"
            )));
        }
    }

    let tmp_name = format!(
        ".orbit-instr-tmp-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    );
    let tmp_path = safe_parent.join(tmp_name);
    let mut guard = AtomicTempGuard::new(tmp_path.clone());

    let mut open_opts = OpenOptions::new();
    open_opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open_opts.mode(preserved_mode.unwrap_or(0o644));
    }

    let mut tmp_file = open_opts.open(&tmp_path).map_err(|e| {
        InstructionError::Io(format!("Failed creating temp file for atomic save: {e}"))
    })?;

    #[cfg(unix)]
    if let Some(mode) = preserved_mode {
        use std::os::unix::fs::PermissionsExt;
        tmp_file
            .set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|e| InstructionError::Io(format!("Failed preserving permissions: {e}")))?;
    }

    let bytes = content.as_bytes();
    tmp_file
        .write_all(bytes)
        .map_err(|e| InstructionError::Io(format!("Failed writing instruction content: {e}")))?;
    tmp_file
        .sync_all()
        .map_err(|e| InstructionError::Io(format!("Failed syncing instruction file: {e}")))?;
    drop(tmp_file);

    // Re-check symlink_metadata immediately before atomic rename to guard against symlink swaps
    if let Ok(target_meta) = fs::symlink_metadata(&safe_target)
        && target_meta.file_type().is_symlink()
    {
        return Err(InstructionError::PathRejected(
            "Refusing to overwrite symlink instruction target".to_string(),
        ));
    }

    fs::rename(&tmp_path, &safe_target).map_err(|e| {
        InstructionError::Io(format!("Failed replacing instruction file atomically: {e}"))
    })?;
    guard.disarm();

    let new_etag = sha256_hex(bytes);
    let approx_tokens = estimate_approx_tokens(content);

    Ok(InstructionSaveResult {
        path: safe_target.to_string_lossy().to_string(),
        etag: new_etag,
        size_bytes: bytes.len() as u64,
        approx_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit_instr_test_{tag}_{}_{}",
            std::process::id(),
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn precedence_order_global_repo_ancestor_subdir_and_override() {
        let root = mk_temp_root("precedence");
        let home = root.join("home");
        let repo = root.join("repo");
        let ancestor = repo.join("packages");
        let subdir = ancestor.join("api");

        fs::create_dir_all(home.join(".orbit")).expect("mk .orbit");
        fs::create_dir_all(home.join(".claude")).expect("mk .claude");
        fs::create_dir_all(repo.join(".git")).expect("mk .git");
        fs::create_dir_all(repo.join(".github")).expect("mk .github");
        fs::create_dir_all(&subdir).expect("mk subdir");

        fs::write(home.join(".orbit/AGENTS.md"), "# Global Orbit\n").expect("write global orbit");
        fs::write(home.join(".claude/CLAUDE.md"), "# Global Claude\n")
            .expect("write global claude");
        fs::write(repo.join("AGENTS.md"), "# Repo Root Agents\n").expect("write repo agents");
        fs::write(
            repo.join(".github/copilot-instructions.md"),
            "# Repo Copilot\n",
        )
        .expect("write copilot");
        fs::write(ancestor.join("CLAUDE.md"), "# Ancestor Packages\n").expect("write ancestor");
        fs::write(subdir.join("AGENTS.md"), "# Subdir API\n").expect("write subdir");
        fs::write(subdir.join("AGENTS.override.md"), "# Local Override\n").expect("write override");

        let res = discover_instruction_hierarchy_with_home(&subdir, Some(&home))
            .expect("discover hierarchy");

        let existing: Vec<&InstructionFileEntry> = res.files.iter().filter(|f| f.exists).collect();
        let scopes_and_names: Vec<(InstructionScope, &str)> = existing
            .iter()
            .map(|f| (f.scope, f.relative_path.as_str()))
            .collect();

        assert_eq!(
            scopes_and_names,
            vec![
                (InstructionScope::GlobalUser, "~/.orbit/AGENTS.md"),
                (InstructionScope::GlobalUser, "~/.claude/CLAUDE.md"),
                (InstructionScope::RepoRoot, "AGENTS.md"),
                (
                    InstructionScope::RepoRoot,
                    ".github/copilot-instructions.md"
                ),
                (InstructionScope::AncestorDir, "packages/CLAUDE.md"),
                (InstructionScope::WorkspaceDir, "packages/api/AGENTS.md"),
                (
                    InstructionScope::LocalOverride,
                    "packages/api/AGENTS.override.md"
                ),
            ]
        );

        // Strictly monotonic precedence order
        for pair in res.files.windows(2) {
            assert!(pair[0].precedence_order < pair[1].precedence_order);
        }
        assert!(res.total_tokens > 0);
        assert!(res.merged_preview.contains("# Global Orbit"));
        assert!(res.merged_preview.contains("# Local Override"));
        let pos_global = res
            .merged_preview
            .find("# Global Orbit")
            .expect("global in merged");
        let pos_repo = res
            .merged_preview
            .find("# Repo Root Agents")
            .expect("repo in merged");
        let pos_override = res
            .merged_preview
            .find("# Local Override")
            .expect("override in merged");
        assert!(pos_global < pos_repo && pos_repo < pos_override);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn import_resolution_cycle_guard_depth_limit_and_traversal_rejection() {
        let root = mk_temp_root("imports");
        let home = root.join("home");
        let repo = root.join("repo");
        let docs = repo.join("docs");
        let outside = root.join("outside_secret.md");

        fs::create_dir_all(&home).expect("mk home");
        fs::create_dir_all(repo.join(".git")).expect("mk .git");
        fs::create_dir_all(&docs).expect("mk docs");
        fs::write(&outside, "TOP SECRET DO NOT READ\n").expect("write outside");

        // Depth chain: AGENTS.md (0) -> d1.md (1) -> d2.md (2) -> d3.md (3) -> d4.md (4, beyond cap)
        // Plus cycle: d2.md -> d1.md
        // Plus traversal: @../outside_secret.md and @/etc/passwd
        // Plus fenced code block ignore: ```\n@docs/ignored.md\n```
        fs::write(docs.join("ignored.md"), "SHOULD NOT BE IMPORTED").expect("write ignored");
        fs::write(docs.join("d4.md"), "DEPTH FOUR BEYOND CAP").expect("write d4");
        fs::write(docs.join("d3.md"), "Depth three body\n@./d4.md\n").expect("write d3");
        fs::write(docs.join("d2.md"), "Depth two body\n@./d3.md\n@./d1.md\n").expect("write d2");
        fs::write(docs.join("d1.md"), "Depth one body\n@./d2.md\n").expect("write d1");
        fs::write(
            repo.join("AGENTS.md"),
            "# Root\n\
             @docs/d1.md\n\
             @../outside_secret.md\n\
             @/etc/passwd\n\
             ```md\n\
             @docs/ignored.md\n\
             ```\n\
             Inline `@docs/ignored.md` and email user@example.md should be ignored.\n",
        )
        .expect("write agents");

        let res =
            discover_instruction_hierarchy_with_home(&repo, Some(&home)).expect("discover imports");
        let agents = res
            .files
            .iter()
            .find(|f| f.exists && f.filename == "AGENTS.md")
            .expect("agents entry");

        assert_eq!(
            agents.imports,
            vec!["docs/d1.md", "docs/d2.md", "docs/d3.md"]
        );
        assert!(res.merged_preview.contains("Depth one body"));
        assert!(res.merged_preview.contains("Depth three body"));
        assert!(!res.merged_preview.contains("DEPTH FOUR BEYOND CAP"));
        assert!(!res.merged_preview.contains("TOP SECRET DO NOT READ"));
        assert!(!res.merged_preview.contains("SHOULD NOT BE IMPORTED"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn atomic_save_etag_conflict_and_size_cap() {
        let root = mk_temp_root("save");
        let home = root.join("home");
        let repo = root.join("repo");
        fs::create_dir_all(&home).expect("mk home");
        fs::create_dir_all(repo.join(".git")).expect("mk .git");

        let target_path = repo.join("AGENTS.md");
        let target_str = target_path.to_string_lossy().to_string();

        // 1. Read non-existent allowlisted file returns exists=false
        let initial_read =
            read_instruction_file_with_home(&repo, &target_str, Some(&home)).expect("read missing");
        assert!(!initial_read.exists);
        assert!(initial_read.etag.is_none());

        // 2. Create brand-new file with expected_etag=None
        let saved1 =
            save_instruction_file_with_home(&repo, &target_str, "# Rules v1\n", None, Some(&home))
                .expect("create v1");
        assert_eq!(saved1.size_bytes, 11);
        assert!(saved1.approx_tokens > 0);

        // 3. Creating again with expected_etag=None MUST fail with EtagConflict
        let dup_err =
            save_instruction_file_with_home(&repo, &target_str, "# Clobber\n", None, Some(&home))
                .expect_err("duplicate create must conflict");
        assert!(matches!(dup_err, InstructionError::EtagConflict { .. }));

        // 4. Valid update with matching etag succeeds
        let saved2 = save_instruction_file_with_home(
            &repo,
            &target_str,
            "# Rules v2\n中文规范测试\n",
            Some(&saved1.etag),
            Some(&home),
        )
        .expect("update v2");
        assert_ne!(saved2.etag, saved1.etag);
        assert!(saved2.approx_tokens >= 8);

        // 5. Stale etag MUST fail with EtagConflict and preserve disk content
        let stale_err = save_instruction_file_with_home(
            &repo,
            &target_str,
            "# Stale\n",
            Some(&saved1.etag),
            Some(&home),
        )
        .expect_err("stale etag must conflict");
        match stale_err {
            InstructionError::EtagConflict { current_etag } => {
                assert_eq!(current_etag, saved2.etag);
            }
            other => panic!("expected EtagConflict, got {other:?}"),
        }
        assert_eq!(
            fs::read_to_string(&target_path).expect("read disk"),
            "# Rules v2\n中文规范测试\n"
        );

        // 6. Oversized payload (> 512 KiB) MUST be rejected
        let oversized = "x".repeat(MAX_INSTRUCTION_FILE_BYTES + 1);
        let size_err = save_instruction_file_with_home(
            &repo,
            &target_str,
            &oversized,
            Some(&saved2.etag),
            Some(&home),
        )
        .expect_err("oversized must fail");
        assert!(matches!(size_err, InstructionError::FileTooLarge { .. }));

        // 7. Global ~/.orbit/AGENTS.md creation auto-creates ~/.orbit dir safely
        let global_target = home.join(".orbit/AGENTS.md");
        let global_saved = save_instruction_file_with_home(
            &repo,
            "~/.orbit/AGENTS.md",
            "# Global\n",
            None,
            Some(&home),
        )
        .expect("create global agents");
        assert_eq!(
            fs::read_to_string(&global_target).expect("read global"),
            "# Global\n"
        );
        assert_eq!(global_saved.size_bytes, 9);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn non_instruction_paths_and_traversal_are_strictly_rejected() {
        let root = mk_temp_root("reject");
        let home = root.join("home");
        let repo = root.join("repo");
        fs::create_dir_all(home.join(".ssh")).expect("mk .ssh");
        fs::create_dir_all(home.join(".orbit")).expect("mk .orbit");
        fs::create_dir_all(repo.join(".git")).expect("mk .git");
        fs::write(repo.join("Cargo.toml"), "[package]\n").expect("write cargo");
        fs::write(home.join(".ssh/id_rsa"), "SECRET").expect("write ssh");
        fs::write(home.join(".orbit/host-keypair.json"), "{}").expect("write keypair");

        let bad_paths = [
            "",
            "Cargo.toml",
            "src/main.rs",
            ".env",
            ".git/config",
            "../AGENTS.md",
            "../../etc/passwd",
            "/etc/passwd",
            "~/.ssh/id_rsa",
            "~/.orbit/host-keypair.json",
            "~/.claude/settings.json",
            "subdir\\AGENTS.md",
        ];

        for bad in bad_paths {
            let r_err = read_instruction_file_with_home(&repo, bad, Some(&home))
                .expect_err(&format!("read `{bad}` must be rejected"));
            assert!(
                matches!(r_err, InstructionError::PathRejected(_)),
                "unexpected read err for `{bad}`: {r_err:?}"
            );

            let w_err = save_instruction_file_with_home(&repo, bad, "hacked", None, Some(&home))
                .expect_err(&format!("save `{bad}` must be rejected"));
            assert!(
                matches!(w_err, InstructionError::PathRejected(_)),
                "unexpected save err for `{bad}`: {w_err:?}"
            );
        }

        // Symlink pointing outside repo must also be rejected
        #[cfg(unix)]
        {
            let outside = root.join("secret.md");
            fs::write(&outside, "SECRET").expect("write outside");
            std::os::unix::fs::symlink(&outside, repo.join("CLAUDE.md")).expect("mk symlink");
            let sym_read = read_instruction_file_with_home(&repo, "CLAUDE.md", Some(&home))
                .expect_err("symlink read must fail");
            assert!(matches!(sym_read, InstructionError::PathRejected(_)));
            let sym_save =
                save_instruction_file_with_home(&repo, "CLAUDE.md", "x", None, Some(&home))
                    .expect_err("symlink save must fail");
            assert!(matches!(sym_save, InstructionError::PathRejected(_)));
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn estimate_approx_tokens_handles_ascii_and_cjk() {
        assert_eq!(estimate_approx_tokens(""), 0);
        assert_eq!(estimate_approx_tokens("abcd"), 1);
        assert_eq!(estimate_approx_tokens("abcdefgh"), 2);
        // 4 CJK chars -> 4 * 5 / 4 = 5 tokens
        assert_eq!(estimate_approx_tokens("代码规范"), 5);
    }

    #[test]
    fn import_blocks_credential_exfiltration_and_home_git_dotfiles_repo() {
        let root = mk_temp_root("exfil");
        let home = root.join("home");
        // Simulate a user who tracks dotfiles with `~/.git`
        fs::create_dir_all(home.join(".git")).expect("mk ~/.git");
        fs::create_dir_all(home.join(".orbit")).expect("mk ~/.orbit");
        fs::create_dir_all(home.join(".codex")).expect("mk ~/.codex");
        let workspace = home.join("work/app");
        fs::create_dir_all(&workspace).expect("mk ~/work/app");

        fs::write(
            home.join(".orbit/host-keypair.json"),
            r#"{"private_key":"ORBIT_PRIVATE_KEY_SECRET"}"#,
        )
        .expect("write keypair");
        fs::write(
            home.join(".codex/auth.json"),
            r#"{"token":"CODEX_AUTH_SECRET"}"#,
        )
        .expect("write codex auth");
        fs::write(
            home.join(".netrc"),
            "machine api login u password NETRC_SECRET\n",
        )
        .expect("write netrc");
        fs::write(home.join(".orbit/shared.md"), "# Global Shared Doc\n")
            .expect("write global shared md");
        fs::write(
            home.join(".orbit/AGENTS.md"),
            "# Global Orbit\n@./shared.md\n@./host-keypair.json\n@../.netrc\n",
        )
        .expect("write global agents");
        fs::write(
            workspace.join("config.json"),
            r#"{"secret":"WORKSPACE_JSON_SECRET"}"#,
        )
        .expect("write workspace json");
        fs::write(
            workspace.join("AGENTS.md"),
            "# App\n\
             @~/.orbit/host-keypair.json\n\
             @~/.codex/auth.json\n\
             @~/.netrc\n\
             @~/.orbit/shared.md\n\
             @../../.netrc\n\
             @./config.json\n",
        )
        .expect("write app agents");

        // Workspace inside `$HOME` with `~/.git` MUST NOT adopt `$HOME` as repo_root!
        let canon_ws = crate::platform::canonicalize_clean(&workspace).expect("canon ws");
        let canon_home = crate::platform::canonicalize_clean(&home).expect("canon home");
        assert!(find_git_repo_root(&canon_ws, Some(&canon_home)).is_none());

        let res = discover_instruction_hierarchy_with_home(&workspace, Some(&home))
            .expect("discover exfil");

        let app_entry = res
            .files
            .iter()
            .find(|f| f.exists && f.scope == InstructionScope::WorkspaceDir)
            .expect("workspace entry");
        // Repo/workspace file must NOT import any JSON, dotfile, or global config file!
        assert!(
            app_entry.imports.is_empty(),
            "unexpected repo imports: {:?}",
            app_entry.imports
        );

        let global_entry = res
            .files
            .iter()
            .find(|f| f.exists && f.scope == InstructionScope::GlobalUser)
            .expect("global entry");
        // Global file may ONLY import `.md` inside verified global config dirs
        assert_eq!(global_entry.imports, vec!["~/.orbit/shared.md"]);

        assert!(!res.merged_preview.contains("ORBIT_PRIVATE_KEY_SECRET"));
        assert!(!res.merged_preview.contains("CODEX_AUTH_SECRET"));
        assert!(!res.merged_preview.contains("NETRC_SECRET"));
        assert!(!res.merged_preview.contains("WORKSPACE_JSON_SECRET"));
        assert!(res.merged_preview.contains("# Global Shared Doc"));

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn discovery_rejects_symlinked_github_and_config_parent_dirs() {
        let root = mk_temp_root("symlink_parents");
        let home = root.join("home");
        let repo = root.join("repo");
        let outside_gh = root.join("outside_github");
        let outside_claude = root.join("outside_claude");

        fs::create_dir_all(&home).expect("mk home");
        fs::create_dir_all(repo.join(".git")).expect("mk .git");
        fs::create_dir_all(&outside_gh).expect("mk outside_gh");
        fs::create_dir_all(&outside_claude).expect("mk outside_claude");

        fs::write(
            outside_gh.join("copilot-instructions.md"),
            "SYMLINK_GITHUB_LEAK\n",
        )
        .expect("write outside gh");
        fs::write(outside_claude.join("CLAUDE.md"), "SYMLINK_CLAUDE_LEAK\n")
            .expect("write outside claude");
        fs::write(repo.join("AGENTS.md"), "# Safe Repo\n").expect("write safe repo");

        std::os::unix::fs::symlink(&outside_gh, repo.join(".github")).expect("symlink .github");
        std::os::unix::fs::symlink(&outside_claude, home.join(".claude")).expect("symlink .claude");

        let res = discover_instruction_hierarchy_with_home(&repo, Some(&home))
            .expect("discover symlink parents");

        assert!(
            !res.files
                .iter()
                .any(|f| f.exists && f.filename.contains("copilot-instructions.md"))
        );
        assert!(!res.files.iter().any(|f| f.exists
            && f.scope == InstructionScope::GlobalUser
            && f.filename == "CLAUDE.md"));
        assert!(!res.merged_preview.contains("SYMLINK_GITHUB_LEAK"));
        assert!(!res.merged_preview.contains("SYMLINK_CLAUDE_LEAK"));
        assert!(res.merged_preview.contains("# Safe Repo"));

        let _ = fs::remove_dir_all(&root);
    }
}
