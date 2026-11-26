//! Safe, Rust-first Git write operations for Orbit Daemon:
//! - Stage / unstage individual files or all files
//! - Apply / reverse / discard unified diff hunks via `git apply` over piped `stdin`
//! - Discard tracked files (`git checkout -- <paths>`) and optionally untracked files
//!   (`git clean -f -- <exact_files>` + empty parent dir pruning, never `remove_dir_all`)
//! - Commit via `git commit -F -` (message piped via `stdin`, never exposed in argv)
//! - Push to validated configured remotes (`git push`)
//! - Create GitHub Pull Requests via `gh pr create`
//!
//! Security guarantees:
//! - Zero shell invocation (`Command::new("git")` / `Command::new("gh")` with explicit args only).
//! - `GIT_LITERAL_PATHSPECS=1` + `--` separator on all path-taking git commands so globs (`*`, `?`,
//!   `[...]`, `:(...)`) and leading `-` flags can never expand or inject options.
//! - Exact caller-supplied paths preserved without `.trim()` mutation.
//! - Strict repository boundary & relative path validation (blocks `..`, NUL, control chars,
//!   backslashes, absolute paths, leading `-`, `.git` internals, and symlink escapes).
//! - Per-section unified diff validation prior to `git apply` (validates every file section's
//!   paths, hunk structure, and rejects mode-only / symlink / rename / copy / binary / unsupported
//!   headers).
//! - Concurrent stdin write + stdout/stderr drain under a single child deadline with explicit
//!   kill & reap on timeout.

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use orbit_protocol::{GitCommitResult, GitCreatePrResult, GitMutationOutcome, GitPushResult};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

use crate::workspace_files::is_path_inside_git;

pub(crate) const LOCAL_GIT_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const NETWORK_GIT_TIMEOUT: Duration = Duration::from_secs(60);
const CHILD_REAP_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PATHS_PER_CALL: usize = 1000;
const MAX_REL_PATH_BYTES: usize = 4096;
const MAX_PATCH_BYTES: usize = 2 * 1024 * 1024;
const MAX_COMMIT_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_PR_TITLE_BYTES: usize = 512;
const MAX_PR_BODY_BYTES: usize = 128 * 1024;
pub(crate) const MAX_REF_NAME_BYTES: usize = 256;
pub(crate) const MAX_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum GitWriteError {
    #[error("not a git repository: {0}")]
    NotAGitRepo(String),
    #[error("invalid path `{path}`: {reason}")]
    InvalidPath { path: String, reason: String },
    #[error("invalid patch: {0}")]
    InvalidPatch(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("git {command} failed: {stderr}")]
    GitFailed { command: String, stderr: String },
    #[error("GitHub CLI (`gh`) unavailable: {0}")]
    GhUnavailable(String),
    #[error("GitHub CLI (`gh`) failed: {0}")]
    GhFailed(String),
    #[error("command timed out: {0}")]
    Timeout(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl GitWriteError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotAGitRepo(_) => "not_a_git_repo",
            Self::InvalidPath { .. } => "invalid_path",
            Self::InvalidPatch(_) => "invalid_patch",
            Self::InvalidInput(_) => "invalid_input",
            Self::GitFailed { .. } => "git_failed",
            Self::GhUnavailable(_) => "gh_unavailable",
            Self::GhFailed(_) => "gh_failed",
            Self::Timeout(_) => "git_timeout",
            Self::Io(_) => "git_io_error",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GitRepoContext {
    pub(crate) repo_root: PathBuf,
    pub(crate) cwd: PathBuf,
}

/// Drive a spawned child process by writing optional `stdin_bytes` and draining `stdout`/`stderr`
/// concurrently under ONE unified deadline, explicitly killing and reaping the child on timeout.
pub(crate) async fn drive_child_with_stdin_timeout(
    mut child: Child,
    stdin_bytes: Option<&[u8]>,
    timeout: Duration,
    label: &str,
) -> Result<std::process::Output, GitWriteError> {
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    let run_fut = async {
        let write_fut = async {
            if let (Some(bytes), Some(mut s)) = (stdin_bytes, stdin.take()) {
                // Ignore BrokenPipe if the child exits early after reporting an error to stderr.
                if let Err(e) = s.write_all(bytes).await
                    && e.kind() != ErrorKind::BrokenPipe
                {
                    return Err(e);
                }
                let _ = s.shutdown().await;
            }
            Ok::<(), std::io::Error>(())
        };

        let read_stdout_fut = async {
            let mut buf = Vec::new();
            if let Some(mut out) = stdout.take() {
                out.read_to_end(&mut buf).await?;
            }
            Ok::<Vec<u8>, std::io::Error>(buf)
        };

        let read_stderr_fut = async {
            let mut buf = Vec::new();
            if let Some(mut err) = stderr.take() {
                err.read_to_end(&mut buf).await?;
            }
            Ok::<Vec<u8>, std::io::Error>(buf)
        };

        let (write_res, stdout_res, stderr_res) =
            tokio::join!(write_fut, read_stdout_fut, read_stderr_fut);
        let status = child.wait().await?;
        write_res?;
        let out_bytes = stdout_res?;
        let err_bytes = stderr_res?;
        Ok::<std::process::Output, std::io::Error>(std::process::Output {
            status,
            stdout: out_bytes,
            stderr: err_bytes,
        })
    };

    match tokio::time::timeout(timeout, run_fut).await {
        Ok(res) => res.map_err(GitWriteError::Io),
        Err(_) => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(CHILD_REAP_TIMEOUT, child.wait()).await;
            Err(GitWriteError::Timeout(label.to_string()))
        }
    }
}

pub(crate) async fn run_git_output(
    dir: &Path,
    args: &[&str],
    timeout: Duration,
    label: &str,
) -> Result<std::process::Output, GitWriteError> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn()?;
    drive_child_with_stdin_timeout(child, None, timeout, &format!("git {label}")).await
}

async fn run_git_with_stdin(
    dir: &Path,
    args: &[&str],
    stdin_bytes: &[u8],
    timeout: Duration,
    label: &str,
) -> Result<std::process::Output, GitWriteError> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn()?;
    drive_child_with_stdin_timeout(child, Some(stdin_bytes), timeout, &format!("git {label}")).await
}

pub(crate) fn format_process_failure(out: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !stderr.is_empty() {
        return truncate_utf8(&stderr, MAX_OUTPUT_BYTES);
    }
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !stdout.is_empty() {
        return truncate_utf8(&stdout, MAX_OUTPUT_BYTES);
    }
    format!(
        "exited with status {}",
        out.status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string())
    )
}

pub(crate) fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = s[..cut].to_string();
    out.push_str("\n...[truncated]");
    out
}

/// Resolve and validate the canonical non-bare Git repository top-level root containing `cwd`.
/// Used by daemon RPC handlers before repository-wide operations (`commit`, `push`, `create_pr`)
/// to ensure the entire repository root is authorized within registered workspaces.
pub async fn resolve_repo_root(cwd: &Path) -> Result<PathBuf, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    Ok(ctx.repo_root)
}

/// Verify that `cwd` is an existing non-bare Git repository working directory and resolve its
/// canonical repository root and canonical working directory.
pub(crate) async fn resolve_git_repo_context(cwd: &Path) -> Result<GitRepoContext, GitWriteError> {
    let canon_cwd = crate::platform::canonicalize_clean_async(cwd)
        .await
        .map_err(|e| GitWriteError::NotAGitRepo(format!("{}: {e}", cwd.display())))?;
    let meta = tokio::fs::metadata(&canon_cwd)
        .await
        .map_err(|e| GitWriteError::NotAGitRepo(format!("{}: {e}", canon_cwd.display())))?;
    if !meta.is_dir() {
        return Err(GitWriteError::NotAGitRepo(format!(
            "{} is not a directory",
            canon_cwd.display()
        )));
    }
    if is_path_inside_git(&canon_cwd, &canon_cwd) {
        return Err(GitWriteError::NotAGitRepo(
            "refusing to run git mutations inside `.git` directory".to_string(),
        ));
    }

    let bare_out = run_git_output(
        &canon_cwd,
        &["rev-parse", "--is-bare-repository"],
        LOCAL_GIT_TIMEOUT,
        "rev-parse --is-bare-repository",
    )
    .await?;
    if !bare_out.status.success() {
        return Err(GitWriteError::NotAGitRepo(format!(
            "{} is not inside a git repository",
            canon_cwd.display()
        )));
    }
    if String::from_utf8_lossy(&bare_out.stdout).trim() == "true" {
        return Err(GitWriteError::NotAGitRepo(
            "bare repository has no working tree".to_string(),
        ));
    }

    let top_out = run_git_output(
        &canon_cwd,
        &["rev-parse", "--show-toplevel"],
        LOCAL_GIT_TIMEOUT,
        "rev-parse --show-toplevel",
    )
    .await?;
    if !top_out.status.success() {
        return Err(GitWriteError::NotAGitRepo(format!(
            "{} is not inside a git working tree",
            canon_cwd.display()
        )));
    }
    let raw_root = String::from_utf8_lossy(&top_out.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if raw_root.is_empty() {
        return Err(GitWriteError::NotAGitRepo(
            "unable to determine git repository root".to_string(),
        ));
    }
    let canon_root = crate::platform::canonicalize_clean_async(Path::new(&raw_root))
        .await
        .map_err(|e| GitWriteError::NotAGitRepo(format!("cannot canonicalize repo root: {e}")))?;
    if !canon_cwd.starts_with(&canon_root) {
        return Err(GitWriteError::NotAGitRepo(
            "working directory is outside resolved git repository root".to_string(),
        ));
    }

    Ok(GitRepoContext {
        repo_root: canon_root,
        cwd: canon_cwd,
    })
}

/// Validate a repository-relative file path against traversal, absolute escapes, flag injection,
/// control characters, `.git` internals, and monorepo subdirectory boundaries.
///
/// Preserves the exact caller-supplied path string (never calls `.trim()` so filenames with
/// leading or trailing spaces are never silently rewritten into another file).
pub(crate) fn validate_git_rel_path(
    ctx: &GitRepoContext,
    raw_path: &str,
) -> Result<String, GitWriteError> {
    let err = |reason: &str| GitWriteError::InvalidPath {
        path: raw_path.to_string(),
        reason: reason.to_string(),
    };

    if raw_path.is_empty() {
        return Err(err("path cannot be empty"));
    }
    if raw_path.len() > MAX_REL_PATH_BYTES {
        return Err(err("path exceeds maximum length"));
    }
    if raw_path.chars().any(|c| c == '\0' || c.is_control()) {
        return Err(err("path contains NUL or control characters"));
    }
    if raw_path.contains('\\') {
        return Err(err("path must use forward slashes (`/`)"));
    }
    if raw_path.starts_with('-') {
        return Err(err("path must not start with `-`"));
    }
    if raw_path.starts_with('/') || raw_path == "~" || raw_path.starts_with("~/") {
        return Err(err("absolute or home-prefixed paths are not allowed"));
    }

    let p = Path::new(raw_path);
    if p.is_absolute() {
        return Err(err("absolute paths are not allowed"));
    }

    let mut segments = Vec::new();
    for comp in p.components() {
        match comp {
            Component::Normal(os) => {
                let Some(seg) = os.to_str() else {
                    return Err(err("path contains non-UTF-8 characters"));
                };
                if seg.is_empty() || seg == "." || seg == ".." {
                    return Err(err("invalid path segment"));
                }
                if seg.eq_ignore_ascii_case(".git") {
                    return Err(err("modifying `.git` internals is forbidden"));
                }
                segments.push(seg.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(err("path traversal (`..`) escapes repository root"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(err("absolute paths are not allowed"));
            }
        }
    }

    if segments.is_empty() {
        return Err(err("path must target a file inside the repository"));
    }

    let normalized = segments.join("/");
    if normalized.starts_with('-') {
        return Err(err("normalized path must not start with `-`"));
    }

    // If the authorized workspace `cwd` is a strict subdirectory of `repo_root` (e.g., monorepo
    // package), ensure callers cannot target sibling subdirectories outside `ctx.cwd`.
    let candidate = ctx.repo_root.join(&normalized);
    if !candidate.starts_with(&ctx.cwd) {
        return Err(err("path is outside the authorized workspace directory"));
    }

    Ok(normalized)
}

fn validate_path_list(
    ctx: &GitRepoContext,
    paths: &[String],
) -> Result<Vec<String>, GitWriteError> {
    if paths.len() > MAX_PATHS_PER_CALL {
        return Err(GitWriteError::InvalidInput(format!(
            "too many paths ({}, max {MAX_PATHS_PER_CALL})",
            paths.len()
        )));
    }
    let mut validated = Vec::with_capacity(paths.len());
    let mut seen = HashSet::with_capacity(paths.len());
    for raw in paths {
        let norm = validate_git_rel_path(ctx, raw)?;
        if seen.insert(norm.clone()) {
            validated.push(norm);
        }
    }
    Ok(validated)
}

/// Verify no intermediate ancestor between `ctx.repo_root` and `rel_path` is a symlink, and
/// verify that the canonical parent directory stays inside both `ctx.repo_root` and `ctx.cwd`.
pub(crate) async fn verify_no_symlink_escape(
    ctx: &GitRepoContext,
    rel_path: &str,
) -> Result<PathBuf, GitWriteError> {
    let mut cursor = ctx.repo_root.clone();
    let segments: Vec<&str> = rel_path.split('/').collect();
    let parent_count = segments.len().saturating_sub(1);

    for seg in &segments[..parent_count] {
        cursor.push(seg);
        if let Ok(meta) = tokio::fs::symlink_metadata(&cursor).await
            && meta.file_type().is_symlink()
        {
            return Err(GitWriteError::InvalidPath {
                path: rel_path.to_string(),
                reason: "path traverses a symbolic link".to_string(),
            });
        }
    }

    if parent_count > 0 && tokio::fs::symlink_metadata(&cursor).await.is_ok() {
        let canon_parent = crate::platform::canonicalize_clean_async(&cursor)
            .await
            .map_err(|e| GitWriteError::InvalidPath {
                path: rel_path.to_string(),
                reason: format!("cannot verify parent directory: {e}"),
            })?;
        if !canon_parent.starts_with(&ctx.repo_root) || !canon_parent.starts_with(&ctx.cwd) {
            return Err(GitWriteError::InvalidPath {
                path: rel_path.to_string(),
                reason: "canonical parent directory escapes repository boundary".to_string(),
            });
        }
    }

    if let Some(last) = segments.last() {
        cursor.push(last);
    }
    Ok(cursor)
}

/// Strip required `a/` or `b/` diff prefix and optional tab-delimited timestamp from a unified diff
/// header line (`--- ` or `+++ `), preserving exact spaces in filenames. Returns `Ok(None)` for
/// `/dev/null`.
fn extract_patch_header_path(raw: &str, expected_prefix: &str) -> Result<Option<String>, String> {
    let no_cr = raw.strip_suffix('\r').unwrap_or(raw);
    let without_ts = no_cr.split('\t').next().unwrap_or(no_cr);
    if without_ts.is_empty() {
        return Err("empty file path in diff header".to_string());
    }
    if without_ts == "/dev/null" {
        return Ok(None);
    }
    // Reject C-quoted diff paths with escape sequences to keep boundary validation unambiguous.
    if without_ts.starts_with('"') || without_ts.ends_with('"') {
        return Err("quoted diff paths are not supported for hunk application".to_string());
    }
    let Some(stripped) = without_ts.strip_prefix(expected_prefix) else {
        return Err(format!(
            "diff header path `{without_ts}` must start with `{expected_prefix}`"
        ));
    };
    if stripped.is_empty() {
        return Err("empty file path after diff prefix".to_string());
    }
    Ok(Some(stripped.to_string()))
}

fn validate_regular_file_mode(mode: &str) -> Result<(), String> {
    match mode {
        "100644" | "100755" => Ok(()),
        _ => Err(format!(
            "unsupported file mode `{mode}` (only regular file modes 100644/100755 are allowed)"
        )),
    }
}

fn validate_index_header(rest: &str) -> Result<(), String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.is_empty() || parts.len() > 2 {
        return Err(format!("malformed index header: `{rest}`"));
    }
    let Some((lhs, rhs)) = parts[0].split_once("..") else {
        return Err(format!("malformed index object range: `{rest}`"));
    };
    if lhs.is_empty()
        || rhs.is_empty()
        || !lhs.bytes().all(|b| b.is_ascii_hexdigit())
        || !rhs.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(format!("invalid object hash in index header: `{rest}`"));
    }
    if let Some(mode) = parts.get(1) {
        validate_regular_file_mode(mode)?;
    }
    Ok(())
}

fn parse_hunk_range_count(spec: &str) -> Result<usize, String> {
    if let Some((start_s, count_s)) = spec.split_once(',') {
        let _start: usize = start_s
            .parse()
            .map_err(|_| format!("invalid hunk start `{start_s}`"))?;
        count_s
            .parse::<usize>()
            .map_err(|_| format!("invalid hunk line count `{count_s}`"))
    } else {
        let _start: usize = spec
            .parse()
            .map_err(|_| format!("invalid hunk line number `{spec}`"))?;
        Ok(1)
    }
}

fn parse_hunk_header_counts(line: &str) -> Result<(usize, usize), String> {
    let Some(after_open) = line.strip_prefix("@@ -") else {
        return Err(format!("malformed hunk header: {line}"));
    };
    let Some(close_idx) = after_open.find(" @@") else {
        return Err(format!("malformed hunk header: {line}"));
    };
    let range_spec = &after_open[..close_idx];
    let Some((old_spec, new_spec)) = range_spec.split_once(" +") else {
        return Err(format!("malformed hunk range: {line}"));
    };
    let old_count = parse_hunk_range_count(old_spec)?;
    let new_count = parse_hunk_range_count(new_spec)?;
    if old_count == 0 && new_count == 0 {
        return Err(format!("empty hunk header: {line}"));
    }
    Ok((old_count, new_count))
}

#[derive(Debug)]
struct PatchSection {
    diff_git_line: Option<String>,
    minus_path: Option<Option<String>>,
    plus_path: Option<Option<String>>,
    hunk_count: usize,
}

impl PatchSection {
    fn with_diff_git(line: String) -> Self {
        Self {
            diff_git_line: Some(line),
            minus_path: None,
            plus_path: None,
            hunk_count: 0,
        }
    }

    fn headerless() -> Self {
        Self {
            diff_git_line: None,
            minus_path: None,
            plus_path: None,
            hunk_count: 0,
        }
    }

    fn finalize(self) -> Result<String, GitWriteError> {
        let minus = self.minus_path.ok_or_else(|| {
            GitWriteError::InvalidPatch(
                "patch file section is missing `---` file header".to_string(),
            )
        })?;
        let plus = self.plus_path.ok_or_else(|| {
            GitWriteError::InvalidPatch(
                "patch file section is missing `+++` file header".to_string(),
            )
        })?;
        if self.hunk_count == 0 {
            return Err(GitWriteError::InvalidPatch(
                "every patch file section must contain at least one unified diff hunk".to_string(),
            ));
        }

        let target = match (minus, plus) {
            (Some(a), Some(b)) => {
                if a != b {
                    return Err(GitWriteError::InvalidPatch(format!(
                        "cross-file patch headers (`{a}` -> `{b}`) are not allowed"
                    )));
                }
                a
            }
            (None, Some(b)) => b,
            (Some(a), None) => a,
            (None, None) => {
                return Err(GitWriteError::InvalidPatch(
                    "patch file section cannot have `/dev/null` for both `---` and `+++`"
                        .to_string(),
                ));
            }
        };

        if let Some(diff_git_line) = self.diff_git_line {
            let expected = format!("diff --git a/{target} b/{target}");
            if diff_git_line != expected {
                return Err(GitWriteError::InvalidPatch(format!(
                    "`diff --git` header `{diff_git_line}` does not match validated target `{target}`"
                )));
            }
        }

        Ok(target)
    }
}

/// Validate a unified diff hunk patch prior to feeding it to `git apply` over stdin.
///
/// Parses and validates EVERY file section independently:
/// - Bounded size and zero NUL bytes
/// - Strict hunk body line counting so literal content lines cannot spoof headers
/// - Every file section's `diff --git`, `---`, and `+++` paths validated inside `repo_root` and
///   authorized `cwd`
/// - Every section MUST contain valid hunks
/// - Rejects mode-only changes, symlink/submodule modes, renames, copies, binary patches, and any
///   unsupported headers.
fn validate_hunk_patch(
    ctx: &GitRepoContext,
    patch: &str,
) -> Result<(String, Vec<String>), GitWriteError> {
    if patch.trim().is_empty() {
        return Err(GitWriteError::InvalidPatch(
            "patch cannot be empty".to_string(),
        ));
    }
    if patch.len() > MAX_PATCH_BYTES {
        return Err(GitWriteError::InvalidPatch(format!(
            "patch exceeds maximum allowed size ({MAX_PATCH_BYTES} bytes)"
        )));
    }
    if patch.contains('\0') {
        return Err(GitWriteError::InvalidPatch(
            "patch contains NUL byte".to_string(),
        ));
    }

    let mut current: Option<PatchSection> = None;
    let mut old_rem = 0usize;
    let mut new_rem = 0usize;
    let mut just_finished_hunk = false;
    let mut section_count = 0usize;
    let mut validated_targets = Vec::new();
    let mut seen_targets = HashSet::new();

    let mut record_section = |sec: PatchSection| -> Result<(), GitWriteError> {
        let target = sec.finalize()?;
        section_count += 1;
        if seen_targets.insert(target.clone()) {
            validated_targets.push(target);
        }
        Ok(())
    };

    for raw_line in patch.split('\n') {
        // Ignore the single trailing empty slice produced when `patch` ends with `\n`.
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if old_rem > 0 || new_rem > 0 {
            if line == "\\ No newline at end of file" {
                continue;
            }
            if line.starts_with(' ') || (line.is_empty() && old_rem > 0 && new_rem > 0) {
                if old_rem == 0 || new_rem == 0 {
                    return Err(GitWriteError::InvalidPatch(
                        "unexpected context line exceeds hunk range".to_string(),
                    ));
                }
                old_rem -= 1;
                new_rem -= 1;
            } else if line.starts_with('-') {
                if old_rem == 0 {
                    return Err(GitWriteError::InvalidPatch(
                        "unexpected deletion line exceeds hunk range".to_string(),
                    ));
                }
                old_rem -= 1;
            } else if line.starts_with('+') {
                if new_rem == 0 {
                    return Err(GitWriteError::InvalidPatch(
                        "unexpected addition line exceeds hunk range".to_string(),
                    ));
                }
                new_rem -= 1;
            } else {
                return Err(GitWriteError::InvalidPatch(format!(
                    "unexpected line inside hunk body: `{line}`"
                )));
            }
            if old_rem == 0 && new_rem == 0 {
                just_finished_hunk = true;
            }
            continue;
        }

        if line == "\\ No newline at end of file" && just_finished_hunk {
            continue;
        }
        just_finished_hunk = false;

        if line.is_empty() {
            // Trailing newline yields one empty slice at EOF; handled after loop if mid-patch.
            continue;
        }

        if line.starts_with("diff --git ") {
            if let Some(prev) = current.take() {
                record_section(prev)?;
            }
            current = Some(PatchSection::with_diff_git(line.to_string()));
        } else if line.starts_with("diff ") {
            return Err(GitWriteError::InvalidPatch(format!(
                "unsupported diff header: `{line}`"
            )));
        } else if line.starts_with("old mode ")
            || line.starts_with("new mode ")
            || line.starts_with("rename from ")
            || line.starts_with("rename to ")
            || line.starts_with("copy from ")
            || line.starts_with("copy to ")
            || line.starts_with("similarity index ")
            || line.starts_with("dissimilarity index ")
            || line == "GIT binary patch"
            || line.starts_with("Binary files ")
        {
            return Err(GitWriteError::InvalidPatch(format!(
                "unsupported patch operation header: `{line}`"
            )));
        } else if let Some(rest) = line.strip_prefix("index ") {
            let Some(sec) = current.as_ref() else {
                return Err(GitWriteError::InvalidPatch(
                    "`index` header appeared outside a `diff --git` section".to_string(),
                ));
            };
            if sec.hunk_count > 0 || sec.minus_path.is_some() {
                return Err(GitWriteError::InvalidPatch(
                    "unexpected `index` header after file headers/hunks".to_string(),
                ));
            }
            validate_index_header(rest).map_err(GitWriteError::InvalidPatch)?;
        } else if let Some(mode) = line
            .strip_prefix("new file mode ")
            .or_else(|| line.strip_prefix("deleted file mode "))
        {
            let Some(sec) = current.as_ref() else {
                return Err(GitWriteError::InvalidPatch(
                    "file mode header appeared outside a `diff --git` section".to_string(),
                ));
            };
            if sec.hunk_count > 0 || sec.minus_path.is_some() {
                return Err(GitWriteError::InvalidPatch(
                    "unexpected file mode header after file headers/hunks".to_string(),
                ));
            }
            validate_regular_file_mode(mode).map_err(GitWriteError::InvalidPatch)?;
        } else if let Some(rest) = line.strip_prefix("--- ") {
            if let Some(prev) = current.as_ref()
                && prev.hunk_count > 0
                && let Some(finished) = current.take()
            {
                record_section(finished)?;
            }
            let sec = current.get_or_insert_with(PatchSection::headerless);
            if sec.minus_path.is_some() || sec.plus_path.is_some() || sec.hunk_count > 0 {
                return Err(GitWriteError::InvalidPatch(
                    "duplicate or out-of-order `---` header in patch section".to_string(),
                ));
            }
            let extracted =
                extract_patch_header_path(rest, "a/").map_err(GitWriteError::InvalidPatch)?;
            let validated = match extracted {
                Some(p) => Some(validate_git_rel_path(ctx, &p)?),
                None => None,
            };
            sec.minus_path = Some(validated);
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            let Some(sec) = current.as_mut() else {
                return Err(GitWriteError::InvalidPatch(
                    "`+++` header appeared without preceding `---` header".to_string(),
                ));
            };
            if sec.minus_path.is_none() || sec.plus_path.is_some() || sec.hunk_count > 0 {
                return Err(GitWriteError::InvalidPatch(
                    "duplicate or out-of-order `+++` header in patch section".to_string(),
                ));
            }
            let extracted =
                extract_patch_header_path(rest, "b/").map_err(GitWriteError::InvalidPatch)?;
            let validated = match extracted {
                Some(p) => Some(validate_git_rel_path(ctx, &p)?),
                None => None,
            };
            sec.plus_path = Some(validated);
        } else if line.starts_with("@@ ") {
            let Some(sec) = current.as_mut() else {
                return Err(GitWriteError::InvalidPatch(
                    "hunk header appeared without file headers".to_string(),
                ));
            };
            if sec.minus_path.is_none() || sec.plus_path.is_none() {
                return Err(GitWriteError::InvalidPatch(
                    "hunk header appeared before `---` and `+++` file headers".to_string(),
                ));
            }
            let (o, n) = parse_hunk_header_counts(line).map_err(GitWriteError::InvalidPatch)?;
            old_rem = o;
            new_rem = n;
            sec.hunk_count += 1;
        } else {
            return Err(GitWriteError::InvalidPatch(format!(
                "unsupported or malformed patch line outside hunk: `{line}`"
            )));
        }
    }

    if old_rem > 0 || new_rem > 0 {
        return Err(GitWriteError::InvalidPatch(
            "truncated unified diff hunk".to_string(),
        ));
    }

    if let Some(last) = current.take() {
        record_section(last)?;
    }

    if section_count == 0 || validated_targets.is_empty() {
        return Err(GitWriteError::InvalidPatch(
            "patch does not contain any valid file sections".to_string(),
        ));
    }

    let mut normalized = patch.to_string();
    if !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    Ok((normalized, validated_targets))
}

async fn repo_has_head_commit(repo_root: &Path) -> Result<bool, GitWriteError> {
    let out = run_git_output(
        repo_root,
        &["rev-parse", "--verify", "-q", "HEAD"],
        LOCAL_GIT_TIMEOUT,
        "rev-parse --verify HEAD",
    )
    .await?;
    Ok(out.status.success())
}

async fn current_branch_name(repo_root: &Path) -> Result<Option<String>, GitWriteError> {
    let out = run_git_output(
        repo_root,
        &["symbolic-ref", "--short", "-q", "HEAD"],
        LOCAL_GIT_TIMEOUT,
        "symbolic-ref --short HEAD",
    )
    .await?;
    if !out.status.success() {
        return Ok(None);
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

pub(crate) fn parse_nul_delimited_paths(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).to_string())
        .collect()
}

/// Remove strictly empty parent directories left behind after deleting untracked files, stopping at
/// `ctx.cwd` / `ctx.repo_root`, non-empty directories (e.g. containing `.gitignore`d files), or any
/// symlink. Never calls `remove_dir_all`.
async fn prune_empty_parent_dirs(ctx: &GitRepoContext, deleted_rel_paths: &[String]) {
    for rel in deleted_rel_paths {
        let segments: Vec<&str> = rel.split('/').collect();
        for depth in (1..segments.len()).rev() {
            let parent_rel = segments[..depth].join("/");
            let candidate = ctx.repo_root.join(&parent_rel);
            if candidate == ctx.repo_root
                || candidate == ctx.cwd
                || !candidate.starts_with(&ctx.cwd)
                || !candidate.starts_with(&ctx.repo_root)
                || is_path_inside_git(&ctx.repo_root, &candidate)
            {
                break;
            }
            if verify_no_symlink_escape(ctx, &parent_rel).await.is_err() {
                break;
            }
            match tokio::fs::symlink_metadata(&candidate).await {
                Ok(meta) if meta.file_type().is_dir() && !meta.file_type().is_symlink() => {}
                _ => break,
            }
            if tokio::fs::remove_dir(&candidate).await.is_err() {
                break;
            }
        }
    }
}

/// Stage specific files (`git add -A -- <paths>`) or all changes in the authorized workspace
/// (`all = true`), always with `GIT_LITERAL_PATHSPECS=1`.
pub async fn stage_files(
    cwd: &Path,
    paths: &[String],
    all: bool,
) -> Result<GitMutationOutcome, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    if all {
        let out = run_git_output(
            &ctx.cwd,
            &["add", "-A", "--", "."],
            LOCAL_GIT_TIMEOUT,
            "add -A",
        )
        .await?;
        if !out.status.success() {
            return Err(GitWriteError::GitFailed {
                command: "add -A".to_string(),
                stderr: format_process_failure(&out),
            });
        }
        return Ok(GitMutationOutcome::FilesStaged {
            paths: Vec::new(),
            all: true,
        });
    }

    if paths.is_empty() {
        return Err(GitWriteError::InvalidInput(
            "no file paths provided to stage (set `all: true` to stage all changes)".to_string(),
        ));
    }

    let validated = validate_path_list(&ctx, paths)?;
    let mut args: Vec<&str> = Vec::with_capacity(validated.len() + 3);
    args.push("add");
    args.push("-A");
    args.push("--");
    for p in &validated {
        args.push(p.as_str());
    }

    let out = run_git_output(&ctx.repo_root, &args, LOCAL_GIT_TIMEOUT, "add").await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "add".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    Ok(GitMutationOutcome::FilesStaged {
        paths: validated,
        all: false,
    })
}

/// Unstage specific files or all staged files (`all = true`), supporting both normal repos and
/// freshly initialized repositories with an unborn `HEAD`, always with `GIT_LITERAL_PATHSPECS=1`.
pub async fn unstage_files(
    cwd: &Path,
    paths: &[String],
    all: bool,
) -> Result<GitMutationOutcome, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let has_head = repo_has_head_commit(&ctx.repo_root).await?;

    if all {
        let out = if has_head {
            run_git_output(
                &ctx.cwd,
                &["reset", "-q", "HEAD", "--", "."],
                LOCAL_GIT_TIMEOUT,
                "reset HEAD",
            )
            .await?
        } else {
            run_git_output(
                &ctx.cwd,
                &["rm", "--cached", "-q", "-r", "--ignore-unmatch", "--", "."],
                LOCAL_GIT_TIMEOUT,
                "rm --cached",
            )
            .await?
        };
        if !out.status.success() {
            return Err(GitWriteError::GitFailed {
                command: if has_head {
                    "reset HEAD".to_string()
                } else {
                    "rm --cached".to_string()
                },
                stderr: format_process_failure(&out),
            });
        }
        return Ok(GitMutationOutcome::FilesUnstaged {
            paths: Vec::new(),
            all: true,
        });
    }

    if paths.is_empty() {
        return Err(GitWriteError::InvalidInput(
            "no file paths provided to unstage (set `all: true` to unstage all changes)"
                .to_string(),
        ));
    }

    let validated = validate_path_list(&ctx, paths)?;
    let (label, args): (&str, Vec<&str>) = if has_head {
        let mut a = Vec::with_capacity(validated.len() + 4);
        a.extend(["reset", "-q", "HEAD", "--"]);
        for p in &validated {
            a.push(p.as_str());
        }
        ("reset HEAD", a)
    } else {
        let mut a = Vec::with_capacity(validated.len() + 6);
        a.extend(["rm", "--cached", "-q", "-r", "--ignore-unmatch", "--"]);
        for p in &validated {
            a.push(p.as_str());
        }
        ("rm --cached", a)
    };

    let out = run_git_output(&ctx.repo_root, &args, LOCAL_GIT_TIMEOUT, label).await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: label.to_string(),
            stderr: format_process_failure(&out),
        });
    }

    Ok(GitMutationOutcome::FilesUnstaged {
        paths: validated,
        all: false,
    })
}

/// Apply, unstage, or discard a unified diff hunk cleanly via `git apply` over piped `stdin`.
/// - Stage hunk: `cached = true, reverse = false`
/// - Unstage hunk: `cached = true, reverse = true`
/// - Discard unstaged hunk: `cached = false, reverse = true`
pub async fn apply_hunk(
    cwd: &Path,
    patch: &str,
    reverse: bool,
    cached: bool,
) -> Result<GitMutationOutcome, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let (validated_patch, targets) = validate_hunk_patch(&ctx, patch)?;
    for rel in &targets {
        verify_no_symlink_escape(&ctx, rel).await?;
    }

    let mut args = vec!["apply", "--unidiff-zero", "--whitespace=nowarn"];
    if cached {
        args.push("--cached");
    }
    if reverse {
        args.push("-R");
    }

    let out = run_git_with_stdin(
        &ctx.repo_root,
        &args,
        validated_patch.as_bytes(),
        LOCAL_GIT_TIMEOUT,
        "apply",
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "apply".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    Ok(GitMutationOutcome::HunkApplied { reverse, cached })
}

/// Discard working-tree edits for tracked files via `git checkout -- <paths>` and optionally
/// delete exact enumerated untracked files via `git clean -f -- <files>` (with
/// `GIT_LITERAL_PATHSPECS=1`, preserving `.gitignore`d files and nested git repos; never
/// calling `remove_dir_all`), then prune strictly empty parent directories inside the repo.
pub async fn discard_files(
    cwd: &Path,
    paths: &[String],
    include_untracked: bool,
) -> Result<GitMutationOutcome, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    if paths.is_empty() {
        return Err(GitWriteError::InvalidInput(
            "no file paths provided to discard".to_string(),
        ));
    }

    let validated = validate_path_list(&ctx, paths)?;

    // Preflight: classify every requested path using literal pathspecs before mutating anything.
    let mut tracked_args = Vec::with_capacity(validated.len() + 3);
    tracked_args.extend(["ls-files", "-z", "--"]);
    for p in &validated {
        tracked_args.push(p.as_str());
    }
    let tracked_out =
        run_git_output(&ctx.repo_root, &tracked_args, LOCAL_GIT_TIMEOUT, "ls-files").await?;
    if !tracked_out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "ls-files".to_string(),
            stderr: format_process_failure(&tracked_out),
        });
    }
    let tracked_set: HashSet<String> = parse_nul_delimited_paths(&tracked_out.stdout)
        .into_iter()
        .collect();

    let mut untracked_args = Vec::with_capacity(validated.len() + 5);
    untracked_args.extend(["ls-files", "--others", "--exclude-standard", "-z", "--"]);
    for p in &validated {
        untracked_args.push(p.as_str());
    }
    let untracked_out = run_git_output(
        &ctx.repo_root,
        &untracked_args,
        LOCAL_GIT_TIMEOUT,
        "ls-files --others",
    )
    .await?;
    if !untracked_out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "ls-files --others".to_string(),
            stderr: format_process_failure(&untracked_out),
        });
    }
    let raw_untracked = parse_nul_delimited_paths(&untracked_out.stdout);
    let mut untracked_exact_files = Vec::new();
    let mut nested_repo_markers = Vec::new();
    for entry in raw_untracked {
        if entry.ends_with('/') {
            nested_repo_markers.push(entry);
        } else {
            untracked_exact_files.push(entry);
        }
    }

    let mut tracked_targets = Vec::new();
    let mut untracked_file_targets = Vec::new();
    let mut seen_untracked = HashSet::new();

    for rel in &validated {
        let dir_prefix = format!("{rel}/");
        let is_tracked = tracked_set.contains(rel)
            || tracked_set
                .iter()
                .any(|entry| entry.starts_with(&dir_prefix));
        let matching_untracked: Vec<&String> = untracked_exact_files
            .iter()
            .filter(|entry| *entry == rel || entry.starts_with(&dir_prefix))
            .collect();
        let has_nested_repo = nested_repo_markers
            .iter()
            .any(|m| m.trim_end_matches('/') == rel.as_str() || m.starts_with(&dir_prefix));

        if !is_tracked && matching_untracked.is_empty() {
            if has_nested_repo {
                return Err(GitWriteError::InvalidPath {
                    path: rel.clone(),
                    reason: "refusing to discard nested git repository".to_string(),
                });
            }
            return Err(GitWriteError::InvalidPath {
                path: rel.clone(),
                reason: "path is neither a tracked file nor an untracked file in this repository"
                    .to_string(),
            });
        }

        if is_tracked {
            tracked_targets.push(rel.clone());
        }

        if !matching_untracked.is_empty() {
            if !include_untracked {
                if !is_tracked {
                    return Err(GitWriteError::InvalidInput(format!(
                        "cannot discard untracked path `{rel}` unless `include_untracked` is true"
                    )));
                }
            } else {
                for file_rel in matching_untracked {
                    let norm_file = validate_git_rel_path(&ctx, file_rel)?;
                    let safe_full = verify_no_symlink_escape(&ctx, &norm_file).await?;
                    if let Ok(meta) = tokio::fs::symlink_metadata(&safe_full).await
                        && meta.file_type().is_dir()
                    {
                        return Err(GitWriteError::InvalidPath {
                            path: norm_file,
                            reason: "refusing to delete directory target via untracked file clean"
                                .to_string(),
                        });
                    }
                    if seen_untracked.insert(norm_file.clone()) {
                        untracked_file_targets.push(norm_file);
                    }
                }
            }
        }
    }

    if !tracked_targets.is_empty() {
        let mut checkout_args = Vec::with_capacity(tracked_targets.len() + 2);
        checkout_args.extend(["checkout", "--"]);
        for p in &tracked_targets {
            checkout_args.push(p.as_str());
        }
        let out = run_git_output(
            &ctx.repo_root,
            &checkout_args,
            LOCAL_GIT_TIMEOUT,
            "checkout",
        )
        .await?;
        if !out.status.success() {
            return Err(GitWriteError::GitFailed {
                command: "checkout".to_string(),
                stderr: format_process_failure(&out),
            });
        }
    }

    if !untracked_file_targets.is_empty() {
        // Delegate exact enumerated untracked file removal to `git clean -f -- <files>` (no `-x`,
        // no `-d`, with `GIT_LITERAL_PATHSPECS=1`), preserving `.gitignore`d files and nested repos.
        let mut clean_args = Vec::with_capacity(untracked_file_targets.len() + 3);
        clean_args.extend(["clean", "-f", "--"]);
        for p in &untracked_file_targets {
            clean_args.push(p.as_str());
        }
        let out = run_git_output(&ctx.repo_root, &clean_args, LOCAL_GIT_TIMEOUT, "clean").await?;
        if !out.status.success() {
            return Err(GitWriteError::GitFailed {
                command: "clean".to_string(),
                stderr: format_process_failure(&out),
            });
        }
        prune_empty_parent_dirs(&ctx, &untracked_file_targets).await;
    }

    Ok(GitMutationOutcome::FilesDiscarded {
        paths: validated,
        include_untracked,
    })
}

/// Commit staged changes using `git commit -F -` with the commit message piped via `stdin`.
pub async fn commit_changes(
    cwd: &Path,
    message: &str,
    amend: bool,
) -> Result<GitCommitResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    if message.contains('\0') {
        return Err(GitWriteError::InvalidInput(
            "commit message must not contain NUL bytes".to_string(),
        ));
    }
    if message.trim().is_empty() {
        return Err(GitWriteError::InvalidInput(
            "commit message cannot be empty".to_string(),
        ));
    }
    if message.len() > MAX_COMMIT_MESSAGE_BYTES {
        return Err(GitWriteError::InvalidInput(format!(
            "commit message exceeds maximum size ({MAX_COMMIT_MESSAGE_BYTES} bytes)"
        )));
    }

    let mut args = vec!["commit", "-F", "-"];
    if amend {
        args.push("--amend");
    }

    let out = run_git_with_stdin(
        &ctx.repo_root,
        &args,
        message.as_bytes(),
        LOCAL_GIT_TIMEOUT,
        "commit",
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "commit".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    let sha_out = run_git_output(
        &ctx.repo_root,
        &["rev-parse", "HEAD"],
        LOCAL_GIT_TIMEOUT,
        "rev-parse HEAD",
    )
    .await?;
    if !sha_out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "rev-parse HEAD".to_string(),
            stderr: format_process_failure(&sha_out),
        });
    }
    let commit_sha = String::from_utf8_lossy(&sha_out.stdout).trim().to_string();

    let short_out = run_git_output(
        &ctx.repo_root,
        &["rev-parse", "--short", "HEAD"],
        LOCAL_GIT_TIMEOUT,
        "rev-parse --short HEAD",
    )
    .await?;
    let short_sha = if short_out.status.success() {
        String::from_utf8_lossy(&short_out.stdout)
            .trim()
            .to_string()
    } else {
        commit_sha.chars().take(7).collect()
    };

    let summary_out = run_git_output(
        &ctx.repo_root,
        &["log", "-1", "--pretty=%s", "HEAD"],
        LOCAL_GIT_TIMEOUT,
        "log -1",
    )
    .await?;
    let summary = if summary_out.status.success() {
        String::from_utf8_lossy(&summary_out.stdout)
            .trim()
            .to_string()
    } else {
        message
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string()
    };

    let branch = current_branch_name(&ctx.repo_root).await?;

    Ok(GitCommitResult {
        commit_sha,
        short_sha,
        summary,
        branch,
    })
}

async fn validate_branch_name(
    repo_root: &Path,
    raw: &str,
    field: &str,
) -> Result<String, GitWriteError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(GitWriteError::InvalidInput(format!(
            "{field} cannot be empty"
        )));
    }
    if trimmed.len() > MAX_REF_NAME_BYTES {
        return Err(GitWriteError::InvalidInput(format!(
            "{field} exceeds maximum length"
        )));
    }
    if trimmed
        .chars()
        .any(|c| c == '\0' || c.is_control() || c.is_whitespace())
    {
        return Err(GitWriteError::InvalidInput(format!(
            "{field} contains invalid whitespace or control characters"
        )));
    }
    if trimmed.starts_with('-')
        || trimmed.starts_with('+')
        || trimmed.starts_with(':')
        || trimmed.contains(':')
        || trimmed.contains("..")
    {
        return Err(GitWriteError::InvalidInput(format!(
            "{field} `{trimmed}` is not a valid branch name"
        )));
    }

    let check = run_git_output(
        repo_root,
        &["check-ref-format", "--branch", trimmed],
        LOCAL_GIT_TIMEOUT,
        "check-ref-format",
    )
    .await?;
    if !check.status.success() {
        return Err(GitWriteError::InvalidInput(format!(
            "{field} `{trimmed}` failed git branch name validation"
        )));
    }

    Ok(trimmed.to_string())
}

async fn list_configured_remotes(repo_root: &Path) -> Result<Vec<String>, GitWriteError> {
    let out = run_git_output(repo_root, &["remote"], LOCAL_GIT_TIMEOUT, "remote").await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "remote".to_string(),
            stderr: format_process_failure(&out),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

/// Push a branch to a validated configured Git remote (`git push [--set-upstream] -- <remote> <branch>`).
pub async fn push_branch(
    cwd: &Path,
    remote: Option<&str>,
    branch: Option<&str>,
    set_upstream: bool,
) -> Result<GitPushResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let configured_remotes = list_configured_remotes(&ctx.repo_root).await?;

    let resolved_branch = match branch.map(str::trim).filter(|b| !b.is_empty()) {
        Some(explicit) => validate_branch_name(&ctx.repo_root, explicit, "branch").await?,
        None => current_branch_name(&ctx.repo_root).await?.ok_or_else(|| {
            GitWriteError::InvalidInput(
                "cannot push from detached HEAD without specifying a branch".to_string(),
            )
        })?,
    };

    let resolved_remote = match remote.map(str::trim).filter(|r| !r.is_empty()) {
        Some(explicit) => {
            if explicit.len() > MAX_REF_NAME_BYTES
                || explicit
                    .chars()
                    .any(|c| c == '\0' || c.is_control() || c.is_whitespace())
                || explicit.starts_with('-')
            {
                return Err(GitWriteError::InvalidInput(format!(
                    "invalid remote name `{explicit}`"
                )));
            }
            if !configured_remotes.iter().any(|r| r == explicit) {
                return Err(GitWriteError::InvalidInput(format!(
                    "remote `{explicit}` is not configured in this repository"
                )));
            }
            explicit.to_string()
        }
        None => {
            let cfg_key = format!("branch.{resolved_branch}.remote");
            let cfg_out = run_git_output(
                &ctx.repo_root,
                &["config", "--get", &cfg_key],
                LOCAL_GIT_TIMEOUT,
                "config --get",
            )
            .await?;
            let tracking_remote = if cfg_out.status.success() {
                let r = String::from_utf8_lossy(&cfg_out.stdout).trim().to_string();
                (!r.is_empty() && configured_remotes.iter().any(|c| c == &r)).then_some(r)
            } else {
                None
            };

            if let Some(r) = tracking_remote {
                r
            } else if configured_remotes.iter().any(|r| r == "origin") {
                "origin".to_string()
            } else if configured_remotes.len() == 1 {
                configured_remotes[0].clone()
            } else if configured_remotes.is_empty() {
                return Err(GitWriteError::InvalidInput(
                    "no git remote is configured for this repository".to_string(),
                ));
            } else {
                return Err(GitWriteError::InvalidInput(format!(
                    "multiple remotes configured ({}); specify `remote` explicitly",
                    configured_remotes.join(", ")
                )));
            }
        }
    };

    let mut args = vec!["push"];
    if set_upstream {
        args.push("--set-upstream");
    }
    args.push("--");
    args.push(resolved_remote.as_str());
    args.push(resolved_branch.as_str());

    let out = run_git_output(&ctx.repo_root, &args, NETWORK_GIT_TIMEOUT, "push").await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "push".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let combined = match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => format!("Pushed {resolved_branch} to {resolved_remote}"),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stdout}\n{stderr}"),
    };

    Ok(GitPushResult {
        remote: resolved_remote,
        branch: resolved_branch,
        output: truncate_utf8(&combined, MAX_OUTPUT_BYTES),
    })
}

/// Extract the PR URL and optional numeric PR ID from `gh pr create` stdout.
fn parse_gh_pr_url(stdout: &str) -> Option<(String, Option<u64>)> {
    let url = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with("https://") || line.starts_with("http://"))
        .or_else(|| {
            stdout
                .split_whitespace()
                .rev()
                .find(|tok| tok.starts_with("https://") || tok.starts_with("http://"))
        })?
        .to_string();

    let clean = url
        .split(['?', '#'])
        .next()
        .unwrap_or(&url)
        .trim_end_matches('/');
    let number = clean
        .rsplit_once("/pull/")
        .and_then(|(_, num_part)| num_part.parse::<u64>().ok());

    Some((url, number))
}

/// Create a Pull Request via `gh pr create` without shell interpolation, piping `body` via stdin
/// concurrently with stdout/stderr drain under a single deadline.
pub async fn create_pull_request(
    cwd: &Path,
    title: &str,
    body: &str,
    base: Option<&str>,
    draft: bool,
) -> Result<GitCreatePrResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;

    let trimmed_title = title.trim();
    if trimmed_title.is_empty() {
        return Err(GitWriteError::InvalidInput(
            "pull request title cannot be empty".to_string(),
        ));
    }
    if trimmed_title.len() > MAX_PR_TITLE_BYTES {
        return Err(GitWriteError::InvalidInput(format!(
            "pull request title exceeds maximum length ({MAX_PR_TITLE_BYTES} bytes)"
        )));
    }
    if trimmed_title
        .chars()
        .any(|c| c == '\0' || c == '\r' || c == '\n' || c.is_control())
    {
        return Err(GitWriteError::InvalidInput(
            "pull request title must be a single line without control characters".to_string(),
        ));
    }
    if body.contains('\0') {
        return Err(GitWriteError::InvalidInput(
            "pull request body must not contain NUL bytes".to_string(),
        ));
    }
    if body.len() > MAX_PR_BODY_BYTES {
        return Err(GitWriteError::InvalidInput(format!(
            "pull request body exceeds maximum size ({MAX_PR_BODY_BYTES} bytes)"
        )));
    }

    let validated_base = match base.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => Some(validate_branch_name(&ctx.repo_root, b, "base branch").await?),
        None => None,
    };

    let mut cmd = Command::new("gh");
    cmd.arg("pr")
        .arg("create")
        .arg(format!("--title={trimmed_title}"))
        .arg("--body-file")
        .arg("-");
    if let Some(ref base_branch) = validated_base {
        cmd.arg(format!("--base={base_branch}"));
    }
    if draft {
        cmd.arg("--draft");
    }
    cmd.current_dir(&ctx.repo_root)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(GitWriteError::GhUnavailable(
                "GitHub CLI (`gh`) is not installed or not on PATH. Install it from https://cli.github.com/ and run `gh auth login`.".to_string(),
            ));
        }
        Err(e) => return Err(GitWriteError::Io(e)),
    };

    let out = drive_child_with_stdin_timeout(
        child,
        Some(body.as_bytes()),
        NETWORK_GIT_TIMEOUT,
        "gh pr create",
    )
    .await?;

    if !out.status.success() {
        let detail = format_process_failure(&out);
        let lower = detail.to_ascii_lowercase();
        if lower.contains("not logged into")
            || lower.contains("gh auth login")
            || lower.contains("authentication")
        {
            return Err(GitWriteError::GhFailed(format!(
                "GitHub CLI is not authenticated. Run `gh auth login` on the host. Details: {detail}"
            )));
        }
        return Err(GitWriteError::GhFailed(detail));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let (url, number) = parse_gh_pr_url(&stdout).ok_or_else(|| {
        GitWriteError::GhFailed(format!(
            "gh pr create succeeded but did not return a PR URL: {}",
            truncate_utf8(stdout.trim(), 512)
        ))
    })?;

    Ok(GitCreatePrResult {
        url,
        number,
        title: trimmed_title.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{compute_git_diff, inspect_git_status};
    use uuid::Uuid;

    struct TempDirGuard(PathBuf);

    impl TempDirGuard {
        fn new(prefix: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("orbit-{prefix}-{}", Uuid::new_v4().simple()));
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn run_git_ok(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .await
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {:?} failed: stdout={} stderr={}",
            args,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    async fn init_test_repo(dir: &Path) {
        run_git_ok(dir, &["init", "-b", "main"]).await;
        run_git_ok(dir, &["config", "user.email", "test@orbit.local"]).await;
        run_git_ok(dir, &["config", "user.name", "Orbit Tester"]).await;
        run_git_ok(dir, &["config", "commit.gpgsign", "false"]).await;
        run_git_ok(dir, &["config", "core.autocrlf", "false"]).await;
    }

    #[tokio::test]
    async fn rejects_non_git_directory() {
        let tmp = TempDirGuard::new("non-git");
        let err = stage_files(tmp.path(), &["a.txt".into()], false)
            .await
            .expect_err("non-git dir must fail");
        assert!(matches!(err, GitWriteError::NotAGitRepo(_)));
    }

    #[tokio::test]
    async fn stage_and_unstage_individual_and_all_files_including_unborn_head() {
        let tmp = TempDirGuard::new("git-stage");
        init_test_repo(tmp.path()).await;

        std::fs::write(tmp.path().join("alpha.txt"), "hello\n").expect("write alpha");
        std::fs::write(tmp.path().join("beta.txt"), "world\n").expect("write beta");

        // Stage single file on unborn HEAD
        let res = stage_files(tmp.path(), &["alpha.txt".into()], false)
            .await
            .expect("stage alpha");
        assert_eq!(
            res,
            GitMutationOutcome::FilesStaged {
                paths: vec!["alpha.txt".into()],
                all: false
            }
        );

        let status = inspect_git_status(tmp.path()).await;
        assert!(
            status
                .files
                .iter()
                .any(|f| f.path == "alpha.txt" && f.staged)
        );
        assert!(
            status
                .files
                .iter()
                .any(|f| f.path == "beta.txt" && f.untracked)
        );

        // Unstage single file on unborn HEAD
        unstage_files(tmp.path(), &["alpha.txt".into()], false)
            .await
            .expect("unstage alpha on unborn head");
        let status = inspect_git_status(tmp.path()).await;
        assert!(status.files.iter().all(|f| !f.staged));

        // Stage all, commit, then test normal HEAD stage/unstage
        stage_files(tmp.path(), &[], true).await.expect("stage all");
        let status = inspect_git_status(tmp.path()).await;
        assert_eq!(status.files.iter().filter(|f| f.staged).count(), 2);

        commit_changes(tmp.path(), "initial commit", false)
            .await
            .expect("initial commit");

        std::fs::write(tmp.path().join("alpha.txt"), "hello v2\n").expect("modify alpha");
        std::fs::write(tmp.path().join("beta.txt"), "world v2\n").expect("modify beta");

        stage_files(tmp.path(), &[], true)
            .await
            .expect("stage all v2");
        let status = inspect_git_status(tmp.path()).await;
        assert_eq!(status.files.iter().filter(|f| f.staged).count(), 2);

        unstage_files(tmp.path(), &["alpha.txt".into()], false)
            .await
            .expect("unstage alpha");
        let status = inspect_git_status(tmp.path()).await;
        assert!(
            status
                .files
                .iter()
                .any(|f| f.path == "alpha.txt" && !f.staged)
        );
        assert!(
            status
                .files
                .iter()
                .any(|f| f.path == "beta.txt" && f.staged)
        );

        unstage_files(tmp.path(), &[], true)
            .await
            .expect("unstage all");
        let status = inspect_git_status(tmp.path()).await;
        assert!(status.files.iter().all(|f| !f.staged));
    }

    #[tokio::test]
    async fn literal_pathspecs_do_not_glob_expand_wildcards() {
        let tmp = TempDirGuard::new("git-literal-glob");
        init_test_repo(tmp.path()).await;

        // Character-class globs `[1]` and `[0-9]` are valid filenames on all platforms (including
        // Windows NTFS) and match `file1.txt` whenever `GIT_LITERAL_PATHSPECS=1` is missing.
        std::fs::write(tmp.path().join("file1.txt"), "v1\n").expect("write file1");
        std::fs::write(tmp.path().join("file[1].txt"), "b1\n").expect("write literal [1]");
        std::fs::write(tmp.path().join("file[0-9].txt"), "r1\n").expect("write literal [0-9]");
        #[cfg(unix)]
        {
            std::fs::write(tmp.path().join("file?.txt"), "q1\n").expect("write literal ?");
            std::fs::write(tmp.path().join("file*.txt"), "s1\n").expect("write literal *");
        }
        stage_files(tmp.path(), &[], true).await.expect("stage all");
        commit_changes(tmp.path(), "initial", false)
            .await
            .expect("commit");

        std::fs::write(tmp.path().join("file1.txt"), "dirty1\n").expect("dirty file1");
        std::fs::write(tmp.path().join("file[1].txt"), "dirtyb\n").expect("dirty [1]");
        std::fs::write(tmp.path().join("file[0-9].txt"), "dirtyr\n").expect("dirty [0-9]");
        #[cfg(unix)]
        {
            std::fs::write(tmp.path().join("file?.txt"), "dirtyq\n").expect("dirty ?");
            std::fs::write(tmp.path().join("file*.txt"), "dirtys\n").expect("dirty *");
        }

        // Stage ONLY literal `file[1].txt` — `file1.txt` and `file[0-9].txt` must remain unstaged!
        stage_files(tmp.path(), &["file[1].txt".into()], false)
            .await
            .expect("stage literal [1]");
        let status = inspect_git_status(tmp.path()).await;
        assert!(
            status
                .files
                .iter()
                .any(|f| f.path == "file[1].txt" && f.staged)
        );
        assert!(
            status
                .files
                .iter()
                .any(|f| f.path == "file1.txt" && !f.staged)
        );
        assert!(
            status
                .files
                .iter()
                .any(|f| f.path == "file[0-9].txt" && !f.staged)
        );

        // Discard ONLY literal `file[0-9].txt` — `file1.txt` must keep its dirty edit!
        discard_files(tmp.path(), &["file[0-9].txt".into()], false)
            .await
            .expect("discard literal [0-9]");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("file[0-9].txt")).expect("read [0-9]"),
            "r1\n"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("file1.txt")).expect("read file1"),
            "dirty1\n"
        );

        #[cfg(unix)]
        {
            // Stage ONLY literal `file?.txt` — `file1.txt` and `file*.txt` must remain unstaged!
            stage_files(tmp.path(), &["file?.txt".into()], false)
                .await
                .expect("stage literal ?");
            let status = inspect_git_status(tmp.path()).await;
            assert!(
                status
                    .files
                    .iter()
                    .any(|f| f.path == "file?.txt" && f.staged)
            );
            assert!(
                status
                    .files
                    .iter()
                    .any(|f| f.path == "file1.txt" && !f.staged)
            );
            assert!(
                status
                    .files
                    .iter()
                    .any(|f| f.path == "file*.txt" && !f.staged)
            );

            // Discard ONLY literal `file*.txt` — `file1.txt` must keep its dirty edit!
            discard_files(tmp.path(), &["file*.txt".into()], false)
                .await
                .expect("discard literal *");
            assert_eq!(
                std::fs::read_to_string(tmp.path().join("file*.txt")).expect("read *"),
                "s1\n"
            );
            assert_eq!(
                std::fs::read_to_string(tmp.path().join("file1.txt")).expect("read file1"),
                "dirty1\n"
            );
        }
    }

    #[tokio::test]
    async fn does_not_trim_trailing_whitespace_in_file_paths() {
        let tmp = TempDirGuard::new("git-no-trim");
        init_test_repo(tmp.path()).await;

        std::fs::write(tmp.path().join("report"), "v1\n").expect("write report");
        stage_files(tmp.path(), &["report".into()], false)
            .await
            .expect("stage report");
        commit_changes(tmp.path(), "add report", false)
            .await
            .expect("commit");

        std::fs::write(tmp.path().join("report"), "dirty\n").expect("dirty report");

        // Requesting `"report "` (trailing space) or `" report"` (leading space) must NOT
        // silently trim and discard `"report"`!
        for padded in ["report ", " report"] {
            let err = discard_files(tmp.path(), &[padded.into()], true)
                .await
                .expect_err("whitespace-padded path must not resolve to trimmed path");
            assert!(matches!(err, GitWriteError::InvalidPath { .. }));
            assert_eq!(
                std::fs::read_to_string(tmp.path().join("report")).expect("read report"),
                "dirty\n"
            );
        }

        // Leading-space filenames are valid across Unix and Windows; discarding `" report"`
        // must leave `"report"` untouched.
        std::fs::write(tmp.path().join(" report"), "scratch-leading\n")
            .expect("write leading space report");
        discard_files(tmp.path(), &[" report".into()], true)
            .await
            .expect("discard literal leading-space file");
        assert!(!tmp.path().join(" report").exists());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("report")).expect("read report untouched"),
            "dirty\n"
        );

        // Win32 silently strips trailing spaces when creating files, so only test a literal
        // trailing-space file on non-Windows filesystems.
        #[cfg(not(windows))]
        {
            std::fs::write(tmp.path().join("report "), "scratch\n").expect("write report space");
            discard_files(tmp.path(), &["report ".into()], true)
                .await
                .expect("discard literal trailing-space file");
            assert!(!tmp.path().join("report ").exists());
            assert_eq!(
                std::fs::read_to_string(tmp.path().join("report")).expect("read report untouched"),
                "dirty\n"
            );
        }
    }

    #[tokio::test]
    async fn discard_untracked_preserves_gitignored_files_and_nested_repos() {
        let tmp = TempDirGuard::new("git-discard-safe-untracked");
        init_test_repo(tmp.path()).await;

        std::fs::write(tmp.path().join(".gitignore"), "*.env\n").expect("write gitignore");
        stage_files(tmp.path(), &[".gitignore".into()], false)
            .await
            .expect("stage gitignore");
        commit_changes(tmp.path(), "add gitignore", false)
            .await
            .expect("commit");

        // 1. Untracked dir containing both a normal untracked file and a .gitignored secret
        let mixed_dir = tmp.path().join("scratch_mixed");
        std::fs::create_dir_all(&mixed_dir).expect("mkdir scratch_mixed");
        std::fs::write(mixed_dir.join("temp.txt"), "remove me\n").expect("write temp");
        std::fs::write(mixed_dir.join("secret.env"), "KEEP_ME=1\n").expect("write secret");

        // 2. Pure untracked dir with nested empty-after-clean subdir
        let cleanable_dir = tmp.path().join("scratch_cleanable/nested");
        std::fs::create_dir_all(&cleanable_dir).expect("mkdir scratch_cleanable");
        std::fs::write(cleanable_dir.join("only.txt"), "remove me\n").expect("write only");

        // 3. Nested git repository inside workspace
        let nested_repo = tmp.path().join("nested_repo");
        std::fs::create_dir_all(&nested_repo).expect("mkdir nested_repo");
        init_test_repo(&nested_repo).await;
        std::fs::write(nested_repo.join("important.txt"), "nested\n").expect("write nested");

        discard_files(
            tmp.path(),
            &["scratch_mixed".into(), "scratch_cleanable".into()],
            true,
        )
        .await
        .expect("discard untracked dirs safely");

        // Normal untracked files are deleted, ignored secret.env is preserved!
        assert!(!mixed_dir.join("temp.txt").exists());
        assert!(mixed_dir.join("secret.env").exists());
        assert_eq!(
            std::fs::read_to_string(mixed_dir.join("secret.env")).expect("read secret"),
            "KEEP_ME=1\n"
        );
        // Empty parent dirs from pure untracked tree are pruned cleanly!
        assert!(!tmp.path().join("scratch_cleanable").exists());

        // Attempting to discard a nested git repo is explicitly rejected and preserved!
        let nested_err = discard_files(tmp.path(), &["nested_repo".into()], true)
            .await
            .expect_err("nested git repo must not be discarded");
        assert!(matches!(nested_err, GitWriteError::InvalidPath { .. }));
        assert!(nested_repo.join("important.txt").exists());
    }

    #[tokio::test]
    async fn stage_unstage_and_discard_individual_hunks() {
        let tmp = TempDirGuard::new("git-hunks");
        init_test_repo(tmp.path()).await;

        let original = (1..=20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(tmp.path().join("multi.txt"), &original).expect("write multi");
        stage_files(tmp.path(), &["multi.txt".into()], false)
            .await
            .expect("stage initial");
        commit_changes(tmp.path(), "add multi.txt", false)
            .await
            .expect("commit initial");

        // Edit two well-separated regions so `git diff` produces two distinct hunks.
        let modified = (1..=20)
            .map(|i| match i {
                2 => "line 2 EDITED_HUNK_1".to_string(),
                18 => "line 18 EDITED_HUNK_2".to_string(),
                _ => format!("line {i}"),
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(tmp.path().join("multi.txt"), &modified).expect("modify multi");

        let full_diff = compute_git_diff(tmp.path(), Some("multi.txt"), false)
            .await
            .expect("diff multi");
        let hunk_starts: Vec<usize> = full_diff
            .match_indices("\n@@ ")
            .map(|(idx, _)| idx + 1)
            .collect();
        assert_eq!(
            hunk_starts.len(),
            2,
            "expected exactly two hunks in diff:\n{full_diff}"
        );

        let header = &full_diff[..hunk_starts[0]];
        let hunk1_patch = format!("{header}{}", &full_diff[hunk_starts[0]..hunk_starts[1]]);
        let hunk2_patch = format!("{header}{}", &full_diff[hunk_starts[1]..]);

        // 1. Stage ONLY hunk 1; hunk 2 must remain unstaged!
        apply_hunk(tmp.path(), &hunk1_patch, false, true)
            .await
            .expect("stage hunk 1");

        let staged_diff = compute_git_diff(tmp.path(), Some("multi.txt"), true)
            .await
            .expect("staged diff");
        let unstaged_diff = compute_git_diff(tmp.path(), Some("multi.txt"), false)
            .await
            .expect("unstaged diff");
        assert!(staged_diff.contains("EDITED_HUNK_1"));
        assert!(!staged_diff.contains("EDITED_HUNK_2"));
        assert!(!unstaged_diff.contains("EDITED_HUNK_1"));
        assert!(unstaged_diff.contains("EDITED_HUNK_2"));

        // 2. Unstage hunk 1 (cached=true, reverse=true); staged diff should become empty!
        apply_hunk(tmp.path(), &hunk1_patch, true, true)
            .await
            .expect("unstage hunk 1");
        let staged_after = compute_git_diff(tmp.path(), Some("multi.txt"), true)
            .await
            .expect("staged diff after unstage");
        assert!(staged_after.trim().is_empty());

        // 3. Discard ONLY unstaged hunk 2 (cached=false, reverse=true); hunk 1 stays in working tree!
        apply_hunk(tmp.path(), &hunk2_patch, true, false)
            .await
            .expect("discard hunk 2");
        let working_content =
            std::fs::read_to_string(tmp.path().join("multi.txt")).expect("read working multi");
        assert!(working_content.contains("line 2 EDITED_HUNK_1"));
        assert!(working_content.contains("line 18\n"));
        assert!(!working_content.contains("EDITED_HUNK_2"));
    }

    #[tokio::test]
    async fn discard_tracked_and_untracked_and_reject_unsafe_paths() {
        let tmp = TempDirGuard::new("git-discard");
        init_test_repo(tmp.path()).await;

        std::fs::write(tmp.path().join("tracked.txt"), "v1\n").expect("write tracked");
        stage_files(tmp.path(), &["tracked.txt".into()], false)
            .await
            .expect("stage tracked");
        commit_changes(tmp.path(), "initial", false)
            .await
            .expect("commit");

        std::fs::write(tmp.path().join("tracked.txt"), "dirty\n").expect("dirty tracked");
        std::fs::write(tmp.path().join("untracked.txt"), "temp\n").expect("write untracked");

        for bad in [
            "../escape.txt",
            "sub/../../escape.txt",
            "-flag",
            "--work-tree=/tmp",
            ".git/config",
            "/etc/passwd",
        ] {
            let err = discard_files(tmp.path(), &[bad.into()], true)
                .await
                .expect_err(&format!("expected rejection for `{bad}`"));
            assert!(
                matches!(err, GitWriteError::InvalidPath { .. }),
                "expected InvalidPath for `{bad}`, got {err:?}"
            );
        }

        let err = discard_files(
            tmp.path(),
            &["tracked.txt".into(), "untracked.txt".into()],
            false,
        )
        .await
        .expect_err("untracked discard without flag must fail");
        assert!(matches!(err, GitWriteError::InvalidInput(_)));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("tracked.txt")).expect("read tracked"),
            "dirty\n"
        );
        assert!(tmp.path().join("untracked.txt").exists());

        discard_files(
            tmp.path(),
            &["tracked.txt".into(), "untracked.txt".into()],
            true,
        )
        .await
        .expect("discard tracked and untracked");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("tracked.txt")).expect("read restored"),
            "v1\n"
        );
        assert!(!tmp.path().join("untracked.txt").exists());
    }

    #[tokio::test]
    async fn commit_via_stdin_preserves_special_characters_and_supports_amend() {
        let tmp = TempDirGuard::new("git-commit");
        init_test_repo(tmp.path()).await;

        std::fs::write(tmp.path().join("note.txt"), "content\n").expect("write note");
        stage_files(tmp.path(), &["note.txt".into()], false)
            .await
            .expect("stage note");

        let empty_err = commit_changes(tmp.path(), "   \n\t ", false)
            .await
            .expect_err("empty commit message must fail");
        assert!(matches!(empty_err, GitWriteError::InvalidInput(_)));

        let tricky_msg = "feat: handle $HOME $(whoami) 'single' \"double\" `backtick`\n\nBody line with $PATH and --flag";
        let res = commit_changes(tmp.path(), tricky_msg, false)
            .await
            .expect("commit tricky message");
        assert_eq!(res.commit_sha.len(), 40);
        assert!(!res.short_sha.is_empty());
        assert_eq!(res.branch.as_deref(), Some("main"));
        assert_eq!(
            res.summary,
            "feat: handle $HOME $(whoami) 'single' \"double\" `backtick`"
        );

        let full_msg = run_git_ok(tmp.path(), &["log", "-1", "--pretty=%B", "HEAD"]).await;
        assert_eq!(full_msg.trim(), tricky_msg);

        let amended = commit_changes(tmp.path(), "feat: amended summary", true)
            .await
            .expect("amend commit");
        assert_ne!(amended.commit_sha, res.commit_sha);
        assert_eq!(amended.summary, "feat: amended summary");
    }

    #[tokio::test]
    async fn push_to_local_bare_remote_and_reject_unsafe_refs() {
        let bare = TempDirGuard::new("git-bare-remote");
        run_git_ok(bare.path(), &["init", "--bare"]).await;

        let local = TempDirGuard::new("git-push-local");
        init_test_repo(local.path()).await;
        let bare_str = bare.path().to_string_lossy().to_string();
        run_git_ok(local.path(), &["remote", "add", "origin", &bare_str]).await;

        std::fs::write(local.path().join("README.md"), "# Orbit\n").expect("write readme");
        stage_files(local.path(), &["README.md".into()], false)
            .await
            .expect("stage readme");
        let committed = commit_changes(local.path(), "docs: add README", false)
            .await
            .expect("commit readme");

        for bad_remote in ["-u", "--exec=evil", "ext::sh -c id", "nonexistent"] {
            let err = push_branch(local.path(), Some(bad_remote), Some("main"), false)
                .await
                .expect_err(&format!("bad remote `{bad_remote}` must fail"));
            assert!(matches!(err, GitWriteError::InvalidInput(_)));
        }
        for bad_branch in ["-f", ":main", "+main", "main:other", "a..b"] {
            let err = push_branch(local.path(), Some("origin"), Some(bad_branch), false)
                .await
                .expect_err(&format!("bad branch `{bad_branch}` must fail"));
            assert!(matches!(err, GitWriteError::InvalidInput(_)));
        }

        let pushed = push_branch(local.path(), None, None, true)
            .await
            .expect("push to bare remote");
        assert_eq!(pushed.remote, "origin");
        assert_eq!(pushed.branch, "main");

        let remote_sha = run_git_ok(bare.path(), &["rev-parse", "refs/heads/main"]).await;
        assert_eq!(remote_sha.trim(), committed.commit_sha);
    }

    #[tokio::test]
    async fn rejects_malicious_and_multi_section_invalid_hunk_patches() {
        let tmp = TempDirGuard::new("git-bad-patch");
        init_test_repo(tmp.path()).await;

        let escaping_patch =
            "--- a/../../etc/passwd\n+++ b/../../etc/passwd\n@@ -1,1 +1,2 @@\n root\n+evil\n";
        assert!(matches!(
            apply_hunk(tmp.path(), escaping_patch, false, false).await,
            Err(GitWriteError::InvalidPath { .. })
        ));

        let dot_git_patch =
            "--- a/.git/config\n+++ b/.git/config\n@@ -1,1 +1,2 @@\n [core]\n+evil = true\n";
        assert!(matches!(
            apply_hunk(tmp.path(), dot_git_patch, false, false).await,
            Err(GitWriteError::InvalidPath { .. })
        ));

        let binary_patch = "--- a/foo.bin\n+++ b/foo.bin\nGIT binary patch\nliteral 0\n";
        assert!(matches!(
            apply_hunk(tmp.path(), binary_patch, false, false).await,
            Err(GitWriteError::InvalidPatch(_))
        ));

        // Valid hunk section paired with a sibling mode-only section MUST be rejected!
        let mixed_mode_only = concat!(
            "diff --git a/good.txt b/good.txt\n",
            "index e69de29..d00491f 100644\n",
            "--- a/good.txt\n",
            "+++ b/good.txt\n",
            "@@ -1,1 +1,2 @@\n",
            " line1\n",
            "+line2\n",
            "diff --git a/script.sh b/script.sh\n",
            "old mode 100644\n",
            "new mode 100755\n",
        );
        assert!(matches!(
            apply_hunk(tmp.path(), mixed_mode_only, false, true).await,
            Err(GitWriteError::InvalidPatch(_))
        ));

        // Symlink file mode (120000) MUST be rejected!
        let symlink_mode = concat!(
            "diff --git a/link b/link\n",
            "new file mode 120000\n",
            "--- /dev/null\n",
            "+++ b/link\n",
            "@@ -0,0 +1 @@\n",
            "+/etc/passwd\n",
        );
        assert!(matches!(
            apply_hunk(tmp.path(), symlink_mode, false, true).await,
            Err(GitWriteError::InvalidPatch(_))
        ));

        // Mismatched `diff --git` path MUST be rejected!
        let mismatched_diff_git = concat!(
            "diff --git a/other.txt b/other.txt\n",
            "--- a/good.txt\n",
            "+++ b/good.txt\n",
            "@@ -1,1 +1,2 @@\n",
            " line1\n",
            "+line2\n",
        );
        assert!(matches!(
            apply_hunk(tmp.path(), mismatched_diff_git, false, true).await,
            Err(GitWriteError::InvalidPatch(_))
        ));

        // Rename patch MUST be rejected!
        let rename_patch = concat!(
            "diff --git a/old.txt b/new.txt\n",
            "similarity index 100%\n",
            "rename from old.txt\n",
            "rename to new.txt\n",
        );
        assert!(matches!(
            apply_hunk(tmp.path(), rename_patch, false, true).await,
            Err(GitWriteError::InvalidPatch(_))
        ));
    }

    #[tokio::test]
    async fn child_stdin_timeout_kills_and_reaps_stalled_process() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = cmd.spawn().expect("spawn sleep");
        // Write >128 KiB (> OS pipe buffer) so a sequential writer would deadlock past the deadline.
        let large_stdin = vec![b'x'; 256 * 1024];
        let started = std::time::Instant::now();
        let err = drive_child_with_stdin_timeout(
            child,
            Some(&large_stdin),
            Duration::from_millis(150),
            "sleep test",
        )
        .await
        .expect_err("stalled child must time out");
        assert!(matches!(err, GitWriteError::Timeout(_)));
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "timeout + kill/reap took too long: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn parses_gh_pr_urls() {
        assert_eq!(
            parse_gh_pr_url("Creating pull request...\nhttps://github.com/orbit/orbit/pull/128\n"),
            Some((
                "https://github.com/orbit/orbit/pull/128".to_string(),
                Some(128)
            ))
        );
        assert_eq!(
            parse_gh_pr_url("https://ghe.internal.example/team/repo/pull/7?foo=1"),
            Some((
                "https://ghe.internal.example/team/repo/pull/7?foo=1".to_string(),
                Some(7)
            ))
        );
        assert_eq!(parse_gh_pr_url("no url here"), None);
    }

    #[tokio::test]
    async fn create_pr_validates_title_and_base_before_invoking_gh() {
        let tmp = TempDirGuard::new("git-pr-validation");
        init_test_repo(tmp.path()).await;

        assert!(matches!(
            create_pull_request(tmp.path(), "   ", "body", None, false).await,
            Err(GitWriteError::InvalidInput(_))
        ));
        assert!(matches!(
            create_pull_request(tmp.path(), "line1\nline2", "body", None, false).await,
            Err(GitWriteError::InvalidInput(_))
        ));
        assert!(matches!(
            create_pull_request(tmp.path(), "Valid title", "body", Some("--evil"), false).await,
            Err(GitWriteError::InvalidInput(_))
        ));
    }
}
