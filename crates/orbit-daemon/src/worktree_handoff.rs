//! Pure-Rust bidirectional Git Worktree <-> Local checkout Handoff engine (`Apply` / `Move`).
//!
//! Security & safety guarantees:
//! - Zero shell interpolation: every `git` invocation uses `tokio::process::Command::new("git")`
//!   with explicit `--` / `--end-of-options` separators, clean git env vars, `.kill_on_drop(true)`,
//!   and bounded timeouts.
//! - Config isolation: overrides `core.quotePath=false`, `diff.noprefix=false`, `diff.relative=false`,
//!   `--no-color`, `--no-ext-diff`, `--no-textconv`, `--src-prefix=a/`, `--dst-prefix=b/` so user
//!   `~/.gitconfig` or environment settings never corrupt patches.
//! - Same-repository enforcement: validates `git rev-parse --git-common-dir` matches between source
//!   and target checkouts before any preview or execution.
//! - Path traversal & symlink containment: rejects `..`, case-insensitive `.git/` paths, absolute
//!   or escaping symlinks (both tracked `120000` and untracked), and symlink target parents.
//! - Hidden Git backup refs: seeds an isolated temporary `GIT_INDEX_FILE` from the real index/HEAD
//!   (never mutating `.git/index`) and stores a commit object under
//!   `refs/orbit/handoff-backup/<unix_ts>-<short_id>-<target|source>` before mutating or resetting.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use orbit_protocol::{
    HandoffFileChange, WorkspaceSummary, WorktreeHandoffDirection, WorktreeHandoffMode,
    WorktreeHandoffOutcome, WorktreeHandoffPreview,
};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use uuid::Uuid;

use crate::fast_copy::{copy_file_cow_within, resolve_symlink_target_safe};
use crate::workspace_runtime::validate_git_branch_name;

const GIT_FAST_TIMEOUT: Duration = Duration::from_secs(10);
const GIT_DIFF_TIMEOUT: Duration = Duration::from_secs(30);
const EMPTY_TREE_OID: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const MAX_UNTRACKED_LINE_COUNT_BYTES: u64 = 512 * 1024;
const FILE_COMPARE_CHUNK_BYTES: usize = 64 * 1024;

/// Config-isolated prefix for every `git diff` invocation so `~/.gitconfig` (`diff.noprefix`,
/// `diff.external`, `color.ui`, `core.quotePath`, `diff.relative`, textconv) cannot break patches.
const GIT_DIFF_ISOLATED_PREFIX: &[&str] = &[
    "-c",
    "core.quotePath=false",
    "-c",
    "diff.noprefix=false",
    "-c",
    "diff.relative=false",
    "diff",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// Guard ensuring temporary `GIT_INDEX_FILE` (and any `.lock` sibling) is always removed.
struct TempIndexGuard(PathBuf);

impl Drop for TempIndexGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let mut lock_os = self.0.as_os_str().to_os_string();
        lock_os.push(".lock");
        let _ = std::fs::remove_file(PathBuf::from(lock_os));
    }
}

/// Resolved canonical Git repository checkout metadata.
#[derive(Debug, Clone)]
pub struct GitCheckoutInfo {
    pub toplevel: PathBuf,
    pub common_dir: PathBuf,
    pub branch: Option<String>,
    pub head_oid: Option<String>,
}

/// Request specification for previewing a handoff between resolved filesystem paths.
#[derive(Debug, Clone)]
pub struct HandoffPreviewPathsSpec {
    pub source_cwd: PathBuf,
    pub target_cwd: Option<PathBuf>,
    pub direction: WorktreeHandoffDirection,
    pub include_committed: bool,
    pub base_ref: Option<String>,
}

/// Request specification for executing a handoff between already-resolved source and target paths.
#[derive(Debug, Clone)]
pub struct HandoffExecutePathsSpec {
    pub source_cwd: PathBuf,
    pub target_cwd: PathBuf,
    pub direction: WorktreeHandoffDirection,
    pub mode: WorktreeHandoffMode,
    pub include_committed: bool,
    pub base_ref: Option<String>,
    pub allow_three_way_conflicts: bool,
    pub target_workspace: Option<WorkspaceSummary>,
    /// Snapshot source instead of target for primary backup ref (used when target is a brand-new worktree).
    pub backup_source_as_primary: bool,
}

#[derive(Debug, Clone)]
struct TrackedEntry {
    change: HandoffFileChange,
    old_path: Option<String>,
}

#[derive(Debug, Clone)]
struct CollectedChanges {
    commit_count: u32,
    patch_bytes: Vec<u8>,
    tracked_entries: Vec<TrackedEntry>,
    untracked_valid: Vec<ValidatedUntracked>,
    escaping_untracked: Vec<String>,
    unsafe_tracked_symlinks: Vec<String>,
}

#[derive(Debug, Clone)]
struct ValidatedUntracked {
    rel_path: String,
    is_symlink: bool,
    insertions: u32,
}

fn git_cmd_clean(cwd: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_PAGER", "cat")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_EXTERNAL_DIFF")
        .env_remove("GIT_DIFF_OPTS");
    cmd
}

async fn run_git_output(
    cwd: &Path,
    args: &[&str],
    timeout: Duration,
) -> anyhow::Result<std::process::Output> {
    let out = tokio::time::timeout(timeout, git_cmd_clean(cwd).args(args).output())
        .await
        .with_context(|| format!("git `{}` timed out", args.first().copied().unwrap_or("")))?
        .with_context(|| {
            format!(
                "failed to spawn git `{}`",
                args.first().copied().unwrap_or("")
            )
        })?;
    Ok(out)
}

async fn run_git_output_with_index(
    cwd: &Path,
    args: &[&str],
    index_file: &Path,
    timeout: Duration,
) -> anyhow::Result<std::process::Output> {
    let out = tokio::time::timeout(
        timeout,
        git_cmd_clean(cwd)
            .args(args)
            .env("GIT_INDEX_FILE", index_file)
            .output(),
    )
    .await
    .with_context(|| format!("git `{}` timed out", args.first().copied().unwrap_or("")))?
    .with_context(|| {
        format!(
            "failed to spawn git `{}`",
            args.first().copied().unwrap_or("")
        )
    })?;
    Ok(out)
}

/// Pipe `stdin_bytes` into a `git` subprocess concurrently with reading stdout/stderr under a
/// single bounded timeout so patches larger than the OS pipe buffer (>64 KiB) never deadlock.
async fn run_git_with_stdin(
    cwd: &Path,
    args: &[&str],
    stdin_bytes: &[u8],
    index_file: Option<&Path>,
    timeout: Duration,
) -> anyhow::Result<std::process::Output> {
    let mut cmd = git_cmd_clean(cwd);
    cmd.args(args).stdin(Stdio::piped());
    if let Some(idx) = index_file {
        cmd.env("GIT_INDEX_FILE", idx);
    }
    let mut child = cmd.spawn().with_context(|| {
        format!(
            "failed to spawn git `{}`",
            args.first().copied().unwrap_or("")
        )
    })?;

    let child_stdin = child.stdin.take();
    let io_future = async {
        let write_fut = async move {
            if let Some(mut stdin) = child_stdin {
                let _ = stdin.write_all(stdin_bytes).await;
                let _ = stdin.shutdown().await;
            }
        };
        let wait_fut = child.wait_with_output();
        let (_, wait_res) = tokio::join!(write_fut, wait_fut);
        wait_res
    };

    let out = tokio::time::timeout(timeout, io_future)
        .await
        .with_context(|| format!("git `{}` timed out", args.first().copied().unwrap_or("")))?
        .with_context(|| format!("git `{}` failed", args.first().copied().unwrap_or("")))?;
    Ok(out)
}

fn is_valid_git_oid(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_git_dir_component(seg: &OsStr) -> bool {
    seg.to_str().is_some_and(|s| s.eq_ignore_ascii_case(".git"))
}

fn path_contains_git_dir_component(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, Component::Normal(seg) if is_git_dir_component(seg)))
}

/// Strictly validate a repository-relative path (no empty, no absolute, no `..`, no case-insensitive `.git`).
pub fn is_safe_handoff_rel_path(rel: &str) -> bool {
    if rel.is_empty() || rel.contains('\0') {
        return false;
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return false;
    }
    let mut has_normal = false;
    for comp in p.components() {
        match comp {
            Component::Normal(seg) => {
                if is_git_dir_component(seg) {
                    return false;
                }
                has_normal = true;
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    has_normal
}

/// Escape glob metacharacters before passing a literal repo-relative path to `git apply --include=...`.
fn escape_git_apply_glob(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 8);
    for ch in path.chars() {
        if matches!(ch, '\\' | '*' | '?' | '[' | ']') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Inspect canonical Git checkout metadata for `dir`.
pub async fn resolve_git_checkout_info(dir: &Path) -> anyhow::Result<GitCheckoutInfo> {
    if !tokio::fs::try_exists(dir).await.unwrap_or(false) {
        anyhow::bail!("directory_not_found: `{}` does not exist", dir.display());
    }

    let top_out = run_git_output(dir, &["rev-parse", "--show-toplevel"], GIT_FAST_TIMEOUT).await?;
    if !top_out.status.success() {
        anyhow::bail!(
            "not_a_git_repo: `{}` is not a Git repository",
            dir.display()
        );
    }
    let top_stdout = String::from_utf8_lossy(&top_out.stdout);
    let top_raw = crate::git::strip_single_line_ending(&top_stdout).to_string();
    if top_raw.is_empty() {
        anyhow::bail!(
            "not_a_git_repo: `{}` is not a Git repository",
            dir.display()
        );
    }
    let toplevel = crate::platform::canonicalize_clean_async(Path::new(&top_raw))
        .await
        .unwrap_or_else(|_| PathBuf::from(&top_raw));

    let common_out = run_git_output(
        &toplevel,
        &["rev-parse", "--git-common-dir"],
        GIT_FAST_TIMEOUT,
    )
    .await?;
    if !common_out.status.success() {
        anyhow::bail!(
            "not_a_git_repo: cannot resolve git common dir for `{}`",
            dir.display()
        );
    }
    let common_raw = String::from_utf8_lossy(&common_out.stdout)
        .trim()
        .to_string();
    let common_candidate = {
        let p = PathBuf::from(&common_raw);
        if p.is_absolute() { p } else { toplevel.join(p) }
    };
    let common_dir = crate::platform::canonicalize_clean_async(&common_candidate)
        .await
        .unwrap_or(common_candidate);

    let branch_out = run_git_output(
        &toplevel,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        GIT_FAST_TIMEOUT,
    )
    .await?;
    let branch = if branch_out.status.success() {
        let b = String::from_utf8_lossy(&branch_out.stdout)
            .trim()
            .to_string();
        if b.is_empty() { None } else { Some(b) }
    } else {
        None
    };

    let head_out = run_git_output(
        &toplevel,
        &["rev-parse", "--verify", "--quiet", "HEAD"],
        GIT_FAST_TIMEOUT,
    )
    .await?;
    let head_oid = if head_out.status.success() {
        let oid = String::from_utf8_lossy(&head_out.stdout).trim().to_string();
        if is_valid_git_oid(&oid) {
            Some(oid)
        } else {
            None
        }
    } else {
        None
    };

    Ok(GitCheckoutInfo {
        toplevel,
        common_dir,
        branch,
        head_oid,
    })
}

/// Find the primary (main) local checkout directory for a linked Git worktree.
pub async fn resolve_primary_local_checkout(worktree_cwd: &Path) -> anyhow::Result<PathBuf> {
    let source_info = resolve_git_checkout_info(worktree_cwd).await?;

    // If `.git` is a directory, this checkout is ALREADY the primary local checkout — never pick
    // an arbitrary linked worktree as "local".
    if tokio::fs::metadata(source_info.toplevel.join(".git"))
        .await
        .is_ok_and(|m| m.is_dir())
    {
        anyhow::bail!(
            "not_a_worktree: `{}` is already the primary local checkout; specify a target workspace or use local_to_worktree",
            worktree_cwd.display()
        );
    }

    let out = run_git_output(
        &source_info.toplevel,
        &["worktree", "list", "--porcelain"],
        GIT_FAST_TIMEOUT,
    )
    .await?;
    if !out.status.success() {
        anyhow::bail!(
            "failed to list git worktrees for `{}`",
            worktree_cwd.display()
        );
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut candidates = Vec::new();
    for line in stdout.lines() {
        if let Some(path_str) = line.strip_prefix("worktree ") {
            let trimmed = path_str.trim();
            if !trimmed.is_empty() {
                let p = PathBuf::from(trimmed);
                let canon = crate::platform::canonicalize_clean_async(&p)
                    .await
                    .unwrap_or(p);
                candidates.push(canon);
            }
        }
    }

    // Prefer the main checkout whose `.git` entry is a directory (not a linked `.git` file)
    for cand in &candidates {
        if cand != &source_info.toplevel
            && tokio::fs::metadata(cand.join(".git"))
                .await
                .is_ok_and(|m| m.is_dir())
        {
            return Ok(cand.clone());
        }
    }
    // Fallback to the first worktree entry if it differs from source (e.g. bare repo worktrees)
    if let Some(first) = candidates.into_iter().find(|c| c != &source_info.toplevel) {
        return Ok(first);
    }

    anyhow::bail!(
        "not_a_worktree: `{}` has no primary local checkout; specify a target workspace or use local_to_worktree",
        worktree_cwd.display()
    )
}

fn ensure_same_repository(
    source: &GitCheckoutInfo,
    target: &GitCheckoutInfo,
) -> anyhow::Result<()> {
    if source.common_dir != target.common_dir {
        anyhow::bail!(
            "unrelated_git_repo: source `{}` and target `{}` do not belong to the same Git repository",
            source.toplevel.display(),
            target.toplevel.display()
        );
    }
    if source.toplevel == target.toplevel {
        anyhow::bail!(
            "same_directory: source and target resolve to the same checkout `{}`",
            source.toplevel.display()
        );
    }
    Ok(())
}

/// Validate an optional `base_ref` and resolve to commit OID if valid.
async fn resolve_commit_oid(cwd: &Path, rev: &str) -> Option<String> {
    let trimmed = rev.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return None;
    }
    let spec = format!("{trimmed}^{{commit}}");
    let out = run_git_output(
        cwd,
        &["rev-parse", "--verify", "--end-of-options", &spec],
        GIT_FAST_TIMEOUT,
    )
    .await
    .ok()?;
    if !out.status.success() {
        return None;
    }
    let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if is_valid_git_oid(&oid) {
        Some(oid)
    } else {
        None
    }
}

async fn resolve_merge_base(cwd: &Path, a: &str, b: &str) -> Option<String> {
    let out = run_git_output(cwd, &["merge-base", "--", a, b], GIT_FAST_TIMEOUT)
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if is_valid_git_oid(&oid) {
        Some(oid)
    } else {
        None
    }
}

async fn compute_merge_base_and_commits(
    source: &GitCheckoutInfo,
    target: Option<&GitCheckoutInfo>,
    base_ref: Option<&str>,
) -> (Option<String>, u32) {
    let Some(ref src_head) = source.head_oid else {
        return (None, 0);
    };

    let merge_base = if let Some(tgt) = target
        && let Some(ref tgt_head) = tgt.head_oid
    {
        resolve_merge_base(&source.toplevel, tgt_head, src_head).await
    } else {
        None
    };

    let effective_base = match merge_base {
        Some(mb) => Some(mb),
        None => match base_ref {
            Some(br) => {
                if let Some(resolved_base) = resolve_commit_oid(&source.toplevel, br).await {
                    resolve_merge_base(&source.toplevel, &resolved_base, src_head)
                        .await
                        .or(Some(resolved_base))
                } else {
                    None
                }
            }
            None => None,
        },
    };

    let Some(base_oid) = effective_base else {
        return (None, 0);
    };

    if base_oid == *src_head {
        return (Some(base_oid), 0);
    }

    let range = format!("{base_oid}..{src_head}");
    let count = run_git_output(
        &source.toplevel,
        &["rev-list", "--count", "--end-of-options", &range],
        GIT_FAST_TIMEOUT,
    )
    .await
    .ok()
    .filter(|o| o.status.success())
    .and_then(|o| {
        String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse::<u32>()
            .ok()
    })
    .unwrap_or(0);

    (Some(base_oid), count)
}

/// Parse `git diff --name-status -z -M` output into `(new_path, normalized_status, old_path)`.
fn parse_name_status_z(stdout: &[u8]) -> Vec<(String, String, Option<String>)> {
    let mut entries = Vec::new();
    let mut fields = stdout.split(|&b| b == 0).peekable();

    while let Some(status_bytes) = fields.next() {
        if status_bytes.is_empty() {
            continue;
        }
        let raw_status = String::from_utf8_lossy(status_bytes);
        let first_char = raw_status.chars().next().unwrap_or('M');
        if first_char == 'R' || first_char == 'C' {
            let Some(old_path_bytes) = fields.next() else {
                break;
            };
            let Some(new_path_bytes) = fields.next() else {
                break;
            };
            let old_path = String::from_utf8_lossy(old_path_bytes).into_owned();
            let path = String::from_utf8_lossy(new_path_bytes).into_owned();
            if !path.is_empty() {
                let norm = if first_char == 'R' { "R" } else { "A" };
                let old_opt = if first_char == 'R' && !old_path.is_empty() {
                    Some(old_path)
                } else {
                    None
                };
                entries.push((path, norm.to_string(), old_opt));
            }
        } else {
            let Some(path_bytes) = fields.next() else {
                break;
            };
            let path = String::from_utf8_lossy(path_bytes).into_owned();
            if !path.is_empty() {
                let norm = match first_char {
                    'A' => "A",
                    'D' => "D",
                    'R' => "R",
                    _ => "M",
                };
                entries.push((path, norm.to_string(), None));
            }
        }
    }
    entries
}

/// Parse `git diff --numstat -z -M` output into `path -> (insertions, deletions)`.
fn parse_numstat_z(stdout: &[u8]) -> BTreeMap<String, (u32, u32)> {
    let mut stats = BTreeMap::new();
    let mut fields = stdout.split(|&b| b == 0);

    while let Some(rec) = fields.next() {
        if rec.is_empty() {
            continue;
        }
        let rec_str = String::from_utf8_lossy(rec);
        let mut parts = rec_str.splitn(3, '\t');
        let Some(ins_str) = parts.next() else {
            continue;
        };
        let Some(del_str) = parts.next() else {
            continue;
        };
        let Some(path_part) = parts.next() else {
            continue;
        };
        let ins = ins_str.parse::<u32>().unwrap_or(0);
        let del = del_str.parse::<u32>().unwrap_or(0);

        if path_part.is_empty() {
            // Rename/copy entry in `--numstat -z`: `<ins>\t<del>\t\0<old_path>\0<new_path>\0`
            let Some(_old_path) = fields.next() else {
                break;
            };
            let Some(new_path_bytes) = fields.next() else {
                break;
            };
            let new_path = String::from_utf8_lossy(new_path_bytes).into_owned();
            if !new_path.is_empty() {
                stats.insert(new_path, (ins, del));
            }
        } else {
            stats.insert(path_part.to_string(), (ins, del));
        }
    }
    stats
}

/// Parse `git diff --raw -z --no-renames` and return all paths whose new mode is `120000` (symlink).
fn parse_raw_tracked_symlink_paths(stdout: &[u8]) -> Vec<String> {
    let mut symlinks = Vec::new();
    let mut fields = stdout.split(|&b| b == 0);

    while let Some(meta_bytes) = fields.next() {
        if meta_bytes.is_empty() {
            continue;
        }
        let Some(path_bytes) = fields.next() else {
            break;
        };
        // Format: `:<old_mode> <new_mode> <old_oid> <new_oid> <status>`
        let meta = String::from_utf8_lossy(meta_bytes);
        let new_mode = meta
            .trim_start_matches(':')
            .split_whitespace()
            .nth(1)
            .unwrap_or("");
        if new_mode == "120000" {
            let path = String::from_utf8_lossy(path_bytes).into_owned();
            if !path.is_empty() {
                symlinks.push(path);
            }
        }
    }
    symlinks
}

fn validate_symlink_strict_sync(canon_root: &Path, abs_link: &Path) -> bool {
    let Ok(link_target) = std::fs::read_link(abs_link) else {
        return false;
    };
    if link_target.is_absolute() {
        return false;
    }
    let Ok(resolved) = resolve_symlink_target_safe(abs_link, canon_root) else {
        return false;
    };
    match resolved.strip_prefix(canon_root) {
        Ok(rel) => !path_contains_git_dir_component(rel),
        Err(_) => false,
    }
}

fn validate_tracked_symlinks_sync(canon_root: &Path, symlink_paths: Vec<String>) -> Vec<String> {
    let mut unsafe_paths = Vec::new();
    for rel in symlink_paths {
        if !is_safe_handoff_rel_path(&rel) {
            unsafe_paths.push(rel);
            continue;
        }
        let abs = canon_root.join(&rel);
        let Ok(meta) = std::fs::symlink_metadata(&abs) else {
            unsafe_paths.push(rel);
            continue;
        };
        if !meta.file_type().is_symlink() || !validate_symlink_strict_sync(canon_root, &abs) {
            unsafe_paths.push(rel);
        }
    }
    unsafe_paths
}

fn count_text_lines_capped(path: &Path) -> u32 {
    let Ok(meta) = std::fs::metadata(path) else {
        return 0;
    };
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_UNTRACKED_LINE_COUNT_BYTES {
        return 0;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return 0;
    };
    let probe = bytes.len().min(8192);
    if bytes[..probe].contains(&0) {
        return 0;
    }
    let text = String::from_utf8_lossy(&bytes);
    text.lines().count() as u32
}

/// Inspect a repository's untracked files (excluding gitignored), strictly classifying safe
/// entries vs escaping/absolute symlinks and unsafe paths.
fn inspect_untracked_files_sync(
    canon_root: &Path,
    raw_rel_paths: Vec<String>,
) -> anyhow::Result<(Vec<ValidatedUntracked>, Vec<String>)> {
    let mut valid = Vec::new();
    let mut escaping = Vec::new();

    for rel in raw_rel_paths {
        if !is_safe_handoff_rel_path(&rel) {
            escaping.push(rel);
            continue;
        }
        let abs = canon_root.join(&rel);
        // Walk ancestors inside repo root to ensure no intermediate symlink escapes root
        let mut cur = canon_root.to_path_buf();
        let mut ancestor_escaped = false;
        let rel_path_obj = Path::new(&rel);
        if let Some(parent) = rel_path_obj.parent() {
            for comp in parent.components() {
                if let Component::Normal(seg) = comp {
                    cur.push(seg);
                    if let Ok(meta) = std::fs::symlink_metadata(&cur)
                        && meta.file_type().is_symlink()
                        && !validate_symlink_strict_sync(canon_root, &cur)
                    {
                        ancestor_escaped = true;
                        break;
                    }
                }
            }
        }
        if ancestor_escaped {
            escaping.push(rel);
            continue;
        }

        let Ok(sym_meta) = std::fs::symlink_metadata(&abs) else {
            continue;
        };
        let ft = sym_meta.file_type();
        if ft.is_symlink() {
            if validate_symlink_strict_sync(canon_root, &abs) {
                valid.push(ValidatedUntracked {
                    rel_path: rel,
                    is_symlink: true,
                    insertions: 0,
                });
            } else {
                escaping.push(rel);
            }
        } else if ft.is_file() {
            match crate::platform::canonicalize_clean(&abs) {
                Ok(canon) if canon.starts_with(canon_root) => {
                    let insertions = count_text_lines_capped(&canon);
                    valid.push(ValidatedUntracked {
                        rel_path: rel,
                        is_symlink: false,
                        insertions,
                    });
                }
                _ => {
                    escaping.push(rel);
                }
            }
        }
    }

    Ok((valid, escaping))
}

async fn run_isolated_git_diff(
    cwd: &Path,
    extra_args: &[&str],
    timeout: Duration,
) -> anyhow::Result<std::process::Output> {
    let mut args = Vec::with_capacity(GIT_DIFF_ISOLATED_PREFIX.len() + extra_args.len());
    args.extend_from_slice(GIT_DIFF_ISOLATED_PREFIX);
    args.extend_from_slice(extra_args);
    run_git_output(cwd, &args, timeout).await
}

async fn collect_source_changes(
    source: &GitCheckoutInfo,
    target: Option<&GitCheckoutInfo>,
    direction: WorktreeHandoffDirection,
    include_committed: bool,
    base_ref: Option<&str>,
) -> anyhow::Result<CollectedChanges> {
    let (merge_base, commit_count) = match direction {
        WorktreeHandoffDirection::WorktreeToLocal => {
            compute_merge_base_and_commits(source, target, base_ref).await
        }
        WorktreeHandoffDirection::LocalToWorktree => (None, 0),
    };

    let diff_base = match direction {
        WorktreeHandoffDirection::WorktreeToLocal => {
            if include_committed && commit_count > 0 {
                merge_base.clone().or_else(|| source.head_oid.clone())
            } else {
                source.head_oid.clone()
            }
        }
        WorktreeHandoffDirection::LocalToWorktree => source.head_oid.clone(),
    };

    let base_tree_arg = diff_base.as_deref().unwrap_or(EMPTY_TREE_OID);

    let ns_out = run_isolated_git_diff(
        &source.toplevel,
        &["--name-status", "-z", "-M", base_tree_arg, "--"],
        GIT_DIFF_TIMEOUT,
    )
    .await?;
    if !ns_out.status.success() {
        let stderr = String::from_utf8_lossy(&ns_out.stderr);
        anyhow::bail!("git diff --name-status failed: {}", stderr.trim());
    }
    let name_statuses = parse_name_status_z(&ns_out.stdout);

    let numstat_out = run_isolated_git_diff(
        &source.toplevel,
        &["--numstat", "-z", "-M", base_tree_arg, "--"],
        GIT_DIFF_TIMEOUT,
    )
    .await?;
    let numstats = if numstat_out.status.success() {
        parse_numstat_z(&numstat_out.stdout)
    } else {
        BTreeMap::new()
    };

    // Inspect raw mode changes to catch tracked symlinks (`120000`) targeting outside repo or `.git`
    let raw_out = run_isolated_git_diff(
        &source.toplevel,
        &["--raw", "-z", "--no-renames", base_tree_arg, "--"],
        GIT_DIFF_TIMEOUT,
    )
    .await?;
    if !raw_out.status.success() {
        let stderr = String::from_utf8_lossy(&raw_out.stderr);
        anyhow::bail!("git diff --raw failed: {}", stderr.trim());
    }
    let tracked_symlink_paths = parse_raw_tracked_symlink_paths(&raw_out.stdout);
    let unsafe_tracked_symlinks = if tracked_symlink_paths.is_empty() {
        Vec::new()
    } else {
        let canon_src = source.toplevel.clone();
        tokio::task::spawn_blocking(move || {
            validate_tracked_symlinks_sync(&canon_src, tracked_symlink_paths)
        })
        .await
        .context("tracked symlink validation task panicked")?
    };

    let patch_out = run_isolated_git_diff(
        &source.toplevel,
        &["--binary", "--full-index", "-M", base_tree_arg, "--"],
        GIT_DIFF_TIMEOUT,
    )
    .await?;
    if !patch_out.status.success() {
        let stderr = String::from_utf8_lossy(&patch_out.stderr);
        anyhow::bail!("git diff --binary failed: {}", stderr.trim());
    }

    let tracked_entries: Vec<TrackedEntry> = name_statuses
        .into_iter()
        .map(|(path, status, old_path)| {
            let (insertions, deletions) = numstats.get(&path).copied().unwrap_or((0, 0));
            TrackedEntry {
                change: HandoffFileChange {
                    path,
                    status,
                    insertions,
                    deletions,
                    is_untracked: false,
                    has_conflict: false,
                },
                old_path,
            }
        })
        .collect();

    let untracked_out = run_git_output(
        &source.toplevel,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        GIT_FAST_TIMEOUT,
    )
    .await?;
    let raw_untracked: Vec<String> = if untracked_out.status.success() {
        untracked_out
            .stdout
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect()
    } else {
        Vec::new()
    };

    let canon_source = source.toplevel.clone();
    let (untracked_valid, escaping_untracked) = tokio::task::spawn_blocking(move || {
        inspect_untracked_files_sync(&canon_source, raw_untracked)
    })
    .await
    .context("untracked inspection task panicked")??;

    Ok(CollectedChanges {
        commit_count,
        patch_bytes: patch_out.stdout,
        tracked_entries,
        untracked_valid,
        escaping_untracked,
        unsafe_tracked_symlinks,
    })
}

async fn is_checkout_dirty(toplevel: &Path) -> bool {
    let Ok(out) = run_git_output(
        toplevel,
        &["status", "--porcelain=v1", "-z"],
        GIT_FAST_TIMEOUT,
    )
    .await
    else {
        return false;
    };
    out.status.success() && !out.stdout.is_empty()
}

/// Parse conflicting file paths from `git apply --check` stderr output.
fn parse_git_apply_conflict_paths(stderr: &str) -> BTreeSet<String> {
    let mut conflicts = BTreeSet::new();
    for raw_line in stderr.lines() {
        let line = raw_line.trim();
        let Some(rest) = line.strip_prefix("error: ") else {
            continue;
        };
        if let Some(after) = rest.strip_prefix("patch failed: ") {
            // Format: `path/to/file.rs:12`
            if let Some((path, _line)) = after.rsplit_once(':')
                && !path.trim().is_empty()
            {
                conflicts.insert(path.trim().to_string());
                continue;
            }
        }
        for prefix in [
            "binary patch does not apply to '",
            "cannot apply binary patch to '",
        ] {
            if let Some(after) = rest.strip_prefix(prefix)
                && let Some((path, _)) = after.split_once('\'')
                && !path.trim().is_empty()
            {
                conflicts.insert(path.trim().to_string());
                continue;
            }
        }
        for suffix in [
            ": patch does not apply",
            ": already exists in working directory",
            ": No such file or directory",
            ": does not exist in working directory",
            ": does not exist in index",
            ": file type changed",
        ] {
            if let Some(path) = rest.strip_suffix(suffix)
                && !path.trim().is_empty()
            {
                conflicts.insert(path.trim().to_string());
            }
        }
    }
    conflicts
}

/// Stream-compare two files in 64 KiB chunks (without reading unbounded files into RAM).
fn files_have_identical_content_sync(src: &Path, dst: &Path) -> bool {
    let (Ok(src_meta), Ok(dst_meta)) = (
        std::fs::symlink_metadata(src),
        std::fs::symlink_metadata(dst),
    ) else {
        return false;
    };
    let (src_ft, dst_ft) = (src_meta.file_type(), dst_meta.file_type());
    if src_ft.is_symlink() && dst_ft.is_symlink() {
        return match (std::fs::read_link(src), std::fs::read_link(dst)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
    }
    if !src_ft.is_file() || !dst_ft.is_file() {
        return false;
    }
    if src_meta.len() != dst_meta.len() {
        return false;
    }
    let (Ok(mut f_src), Ok(mut f_dst)) = (std::fs::File::open(src), std::fs::File::open(dst))
    else {
        return false;
    };
    let mut buf_src = vec![0u8; FILE_COMPARE_CHUNK_BYTES];
    let mut buf_dst = vec![0u8; FILE_COMPARE_CHUNK_BYTES];
    loop {
        let n_src = match f_src.read(&mut buf_src) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n_src == 0 {
            let extra = match f_dst.read(&mut buf_dst[..1]) {
                Ok(n) => n,
                Err(_) => return false,
            };
            return extra == 0;
        }
        if f_dst.read_exact(&mut buf_dst[..n_src]).is_err() {
            return false;
        }
        if buf_src[..n_src] != buf_dst[..n_src] {
            return false;
        }
    }
}

/// Non-mutating conflict check against `target_toplevel`.
async fn detect_conflicts_non_mutating(
    source_toplevel: &Path,
    target_toplevel: &Path,
    changes: &CollectedChanges,
) -> anyhow::Result<Vec<String>> {
    let mut conflicts = BTreeSet::new();

    // Map rename old_path -> new_path so conflicts on a renamed source file attribute properly
    let old_to_new: BTreeMap<&str, &str> = changes
        .tracked_entries
        .iter()
        .filter_map(|e| {
            e.old_path
                .as_deref()
                .map(|old| (old, e.change.path.as_str()))
        })
        .collect();

    if !changes.patch_bytes.is_empty() {
        let check_out = run_git_with_stdin(
            target_toplevel,
            &[
                "-c",
                "core.quotePath=false",
                "apply",
                "--check",
                "--whitespace=nowarn",
                "-",
            ],
            &changes.patch_bytes,
            None,
            GIT_DIFF_TIMEOUT,
        )
        .await?;

        if !check_out.status.success() {
            let stderr = String::from_utf8_lossy(&check_out.stderr);
            let parsed = parse_git_apply_conflict_paths(&stderr);
            if !parsed.is_empty() {
                for p in parsed {
                    if let Some(&mapped_new) = old_to_new.get(p.as_str()) {
                        conflicts.insert(mapped_new.to_string());
                    } else {
                        conflicts.insert(p);
                    }
                }
            } else {
                // Fallback: probe each tracked entry individually using glob-escaped `--include`
                for entry in &changes.tracked_entries {
                    let inc_new =
                        format!("--include={}", escape_git_apply_glob(&entry.change.path));
                    let inc_old = entry
                        .old_path
                        .as_deref()
                        .map(|old| format!("--include={}", escape_git_apply_glob(old)));
                    let mut args = vec![
                        "-c",
                        "core.quotePath=false",
                        "apply",
                        "--check",
                        "--whitespace=nowarn",
                        &inc_new,
                    ];
                    if let Some(ref old_arg) = inc_old {
                        args.push(old_arg);
                    }
                    args.push("-");
                    if let Ok(single) = run_git_with_stdin(
                        target_toplevel,
                        &args,
                        &changes.patch_bytes,
                        None,
                        GIT_FAST_TIMEOUT,
                    )
                    .await
                        && !single.status.success()
                    {
                        conflicts.insert(entry.change.path.clone());
                    }
                }
                if conflicts.is_empty() {
                    // Patch still failed globally; report all tracked files conservatively
                    for entry in &changes.tracked_entries {
                        conflicts.insert(entry.change.path.clone());
                    }
                }
            }
        }
    }

    // Check untracked file collisions on target (existing differing file, symlink, directory, or symlinked parent)
    let src_root = source_toplevel.to_path_buf();
    let tgt_root = target_toplevel.to_path_buf();
    let untracked_paths: Vec<String> = changes
        .untracked_valid
        .iter()
        .map(|u| u.rel_path.clone())
        .collect();

    let untracked_conflicts =
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
            let mut found = Vec::new();
            for rel in untracked_paths {
                // Check parent chain for symlink or non-dir collision
                let rel_p = Path::new(&rel);
                let mut cur = tgt_root.clone();
                let mut parent_conflict = false;
                if let Some(parent) = rel_p.parent() {
                    for comp in parent.components() {
                        if let Component::Normal(seg) = comp {
                            cur.push(seg);
                            if let Ok(meta) = std::fs::symlink_metadata(&cur)
                                && (meta.file_type().is_symlink() || !meta.file_type().is_dir())
                            {
                                parent_conflict = true;
                                break;
                            }
                        }
                    }
                }
                if parent_conflict {
                    found.push(rel);
                    continue;
                }

                let dst = tgt_root.join(&rel);
                let Ok(dst_meta) = std::fs::symlink_metadata(&dst) else {
                    continue;
                };
                if dst_meta.file_type().is_dir() {
                    found.push(rel);
                    continue;
                }
                let src = src_root.join(&rel);
                if !files_have_identical_content_sync(&src, &dst) {
                    found.push(rel);
                }
            }
            Ok(found)
        })
        .await
        .context("untracked conflict check task panicked")??;

    conflicts.extend(untracked_conflicts);
    Ok(conflicts.into_iter().collect())
}

/// Populate an isolated temporary `GIT_INDEX_FILE` by seeding from the real checkout index (or
/// `HEAD` tree when index is absent), running `git add -u` for tracked files (preserving
/// tracked-but-gitignored files and submodules), and staging explicit validated untracked files
/// while skipping nested `.git` directories and escaping symlinks.
async fn populate_temp_index_snapshot(
    repo_toplevel: &Path,
    head_oid: Option<&str>,
    tmp_index: &Path,
) -> anyhow::Result<()> {
    // 1. Seed tmp_index from the real checkout index file (or HEAD tree if unborn/missing index)
    let mut seeded = false;
    let idx_path_out = run_git_output(
        repo_toplevel,
        &["rev-parse", "--git-path", "index"],
        GIT_FAST_TIMEOUT,
    )
    .await?;
    if idx_path_out.status.success() {
        let raw_idx = String::from_utf8_lossy(&idx_path_out.stdout)
            .trim()
            .to_string();
        if !raw_idx.is_empty() {
            let idx_p = PathBuf::from(&raw_idx);
            let resolved_idx = if idx_p.is_absolute() {
                idx_p
            } else {
                repo_toplevel.join(idx_p)
            };
            if tokio::fs::try_exists(&resolved_idx).await.unwrap_or(false) {
                tokio::fs::copy(&resolved_idx, tmp_index)
                    .await
                    .context("failed seeding temporary index from checkout index")?;
                seeded = true;
            }
        }
    }
    if !seeded && let Some(head) = head_oid.filter(|o| is_valid_git_oid(o)) {
        let read_out = run_git_output_with_index(
            repo_toplevel,
            &["read-tree", head],
            tmp_index,
            GIT_FAST_TIMEOUT,
        )
        .await?;
        if !read_out.status.success() {
            let stderr = String::from_utf8_lossy(&read_out.stderr);
            anyhow::bail!(
                "git read-tree HEAD for temp index failed: {}",
                stderr.trim()
            );
        }
    }

    // 2. Refresh all tracked entries (including tracked-but-gitignored files)
    let add_u = run_git_output_with_index(
        repo_toplevel,
        &["add", "-u", "--"],
        tmp_index,
        GIT_DIFF_TIMEOUT,
    )
    .await?;
    if !add_u.status.success() {
        let stderr = String::from_utf8_lossy(&add_u.stderr);
        anyhow::bail!("git add -u for temp index failed: {}", stderr.trim());
    }

    // 3. Stage explicit validated untracked files (skipping nested `.git` repos & escaping symlinks)
    let untracked_out = run_git_output(
        repo_toplevel,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        GIT_FAST_TIMEOUT,
    )
    .await?;
    if untracked_out.status.success() && !untracked_out.stdout.is_empty() {
        let raw_untracked: Vec<String> = untracked_out
            .stdout
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        let root_owned = repo_toplevel.to_path_buf();
        let stageable: Vec<String> = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            for rel in raw_untracked {
                if !is_safe_handoff_rel_path(&rel) {
                    continue;
                }
                let abs = root_owned.join(&rel);
                let Ok(meta) = std::fs::symlink_metadata(&abs) else {
                    continue;
                };
                let ft = meta.file_type();
                if ft.is_dir() {
                    // Never stage nested git repos or raw directory entries directly
                    continue;
                }
                if ft.is_symlink() && !validate_symlink_strict_sync(&root_owned, &abs) {
                    continue;
                }
                out.push(rel);
            }
            out
        })
        .await
        .context("temp index untracked filter panicked")?;

        for chunk in stageable.chunks(200) {
            let mut args: Vec<&str> = Vec::with_capacity(2 + chunk.len());
            args.push("add");
            args.push("--");
            for p in chunk {
                args.push(p.as_str());
            }
            let add_out =
                run_git_output_with_index(repo_toplevel, &args, tmp_index, GIT_DIFF_TIMEOUT)
                    .await?;
            if !add_out.status.success() {
                let stderr = String::from_utf8_lossy(&add_out.stderr);
                anyhow::bail!("git add untracked for temp index failed: {}", stderr.trim());
            }
        }
    }

    Ok(())
}

/// Capture a full working-tree + untracked snapshot using a seeded temporary `GIT_INDEX_FILE`
/// (never modifying `.git/index`), commit the tree, and store it under
/// `refs/orbit/handoff-backup/<unix_ts>-<short_id>-<tag>`.
pub async fn capture_handoff_backup_ref(
    repo_toplevel: &Path,
    head_oid: Option<&str>,
    direction: WorktreeHandoffDirection,
    mode: WorktreeHandoffMode,
    tag: &str,
) -> anyhow::Result<String> {
    let tmp_index = std::env::temp_dir().join(format!(
        "orbit-handoff-snap-{}.idx",
        Uuid::new_v4().simple()
    ));
    let _guard = TempIndexGuard(tmp_index.clone());

    populate_temp_index_snapshot(repo_toplevel, head_oid, &tmp_index).await?;

    let write_out =
        run_git_output_with_index(repo_toplevel, &["write-tree"], &tmp_index, GIT_FAST_TIMEOUT)
            .await?;
    if !write_out.status.success() {
        let stderr = String::from_utf8_lossy(&write_out.stderr);
        anyhow::bail!("failed to write backup tree: {}", stderr.trim());
    }
    let tree_oid = String::from_utf8_lossy(&write_out.stdout)
        .trim()
        .to_string();
    if !is_valid_git_oid(&tree_oid) {
        anyhow::bail!("invalid backup tree OID `{tree_oid}`");
    }

    let msg = format!(
        "orbit: handoff backup ({}, {}, {})",
        direction.slug(),
        mode.slug(),
        tag
    );
    let mut commit_cmd = git_cmd_clean(repo_toplevel);
    commit_cmd
        .args(["commit-tree", &tree_oid])
        .env("GIT_AUTHOR_NAME", "Orbit Handoff")
        .env("GIT_AUTHOR_EMAIL", "orbit@local")
        .env("GIT_COMMITTER_NAME", "Orbit Handoff")
        .env("GIT_COMMITTER_EMAIL", "orbit@local");
    if let Some(parent) = head_oid.filter(|o| is_valid_git_oid(o)) {
        commit_cmd.args(["-p", parent]);
    }
    commit_cmd.args(["-m", &msg]);

    let commit_out = tokio::time::timeout(GIT_FAST_TIMEOUT, commit_cmd.output())
        .await
        .context("git commit-tree for backup snapshot timed out")??;
    if !commit_out.status.success() {
        let stderr = String::from_utf8_lossy(&commit_out.stderr);
        anyhow::bail!("failed to create backup commit: {}", stderr.trim());
    }
    let commit_oid = String::from_utf8_lossy(&commit_out.stdout)
        .trim()
        .to_string();
    if !is_valid_git_oid(&commit_oid) {
        anyhow::bail!("invalid backup commit OID `{commit_oid}`");
    }

    let clean_tag: String = tag
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let suffix = if clean_tag.is_empty() {
        "backup"
    } else {
        clean_tag.as_str()
    };
    let ref_name = format!(
        "refs/orbit/handoff-backup/{}-{}-{}",
        Utc::now().timestamp(),
        &Uuid::new_v4().simple().to_string()[..8],
        suffix
    );
    let update_out = run_git_output(
        repo_toplevel,
        &["update-ref", &ref_name, &commit_oid],
        GIT_FAST_TIMEOUT,
    )
    .await?;
    if !update_out.status.success() {
        let stderr = String::from_utf8_lossy(&update_out.stderr);
        anyhow::bail!("failed to save backup ref `{ref_name}`: {}", stderr.trim());
    }

    Ok(ref_name)
}

/// Pre-validate every untracked target destination BEFORE `git apply` mutates disk.
fn validate_target_untracked_destinations_sync(
    canon_source_root: &Path,
    canon_target_root: &Path,
    entries: &[ValidatedUntracked],
) -> anyhow::Result<()> {
    for entry in entries {
        if !is_safe_handoff_rel_path(&entry.rel_path) {
            anyhow::bail!("unsafe_path: `{}` rejected", entry.rel_path);
        }
        let rel_p = Path::new(&entry.rel_path);
        let mut cur = canon_target_root.to_path_buf();
        if let Some(parent) = rel_p.parent() {
            for comp in parent.components() {
                if let Component::Normal(seg) = comp {
                    cur.push(seg);
                    if let Ok(meta) = std::fs::symlink_metadata(&cur) {
                        if meta.file_type().is_symlink() {
                            anyhow::bail!(
                                "handoff_conflict: target parent `{}` is a symlink",
                                cur.display()
                            );
                        }
                        if !meta.file_type().is_dir() {
                            anyhow::bail!(
                                "handoff_conflict: target parent `{}` is not a directory",
                                cur.display()
                            );
                        }
                    }
                }
            }
        }
        let dst_abs = canon_target_root.join(&entry.rel_path);
        if let Ok(dst_meta) = std::fs::symlink_metadata(&dst_abs) {
            if dst_meta.file_type().is_dir() {
                anyhow::bail!(
                    "handoff_conflict: destination `{}` is a directory",
                    entry.rel_path
                );
            }
            let src_abs = canon_source_root.join(&entry.rel_path);
            if !files_have_identical_content_sync(&src_abs, &dst_abs) {
                anyhow::bail!(
                    "handoff_conflict: target already contains differing untracked file `{}`",
                    entry.rel_path
                );
            }
        }
    }
    Ok(())
}

/// Atomically copy validated untracked regular files first, then validated relative symlinks.
/// On mid-way failure, cleans up any newly created partial files and empty dirs inside `canon_target_root`.
fn copy_untracked_files_atomically_sync(
    canon_source_root: &Path,
    canon_target_root: &Path,
    entries: &[ValidatedUntracked],
) -> anyhow::Result<usize> {
    validate_target_untracked_destinations_sync(canon_source_root, canon_target_root, entries)?;

    // Copy regular files first so internal relative symlinks pointing to sibling untracked files
    // resolve cleanly against `canon_target_root`.
    let mut ordered: Vec<&ValidatedUntracked> = Vec::with_capacity(entries.len());
    ordered.extend(entries.iter().filter(|e| !e.is_symlink));
    ordered.extend(entries.iter().filter(|e| e.is_symlink));

    let mut newly_written: Vec<PathBuf> = Vec::new();
    let mut copied = 0usize;

    let copy_result: anyhow::Result<usize> = (|| {
        for entry in ordered {
            let src_abs = canon_source_root.join(&entry.rel_path);
            let dst_abs = canon_target_root.join(&entry.rel_path);
            let existed_before = std::fs::symlink_metadata(&dst_abs).is_ok();

            if let Some(parent) = dst_abs.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed creating parent directory for `{}`", entry.rel_path)
                })?;
            }

            if entry.is_symlink {
                if !validate_symlink_strict_sync(canon_source_root, &src_abs) {
                    anyhow::bail!(
                        "escaping_symlink: untracked symlink `{}` rejected",
                        entry.rel_path
                    );
                }
                #[cfg(unix)]
                {
                    let link_target = std::fs::read_link(&src_abs)?;
                    if link_target.is_absolute() {
                        anyhow::bail!(
                            "escaping_symlink: absolute untracked symlink `{}` rejected",
                            entry.rel_path
                        );
                    }
                    let parent = dst_abs.parent().ok_or_else(|| {
                        anyhow::anyhow!("destination has no parent: {}", dst_abs.display())
                    })?;
                    let tmp_link =
                        parent.join(format!(".orbit-handoff-ln-{}.tmp", Uuid::new_v4().simple()));
                    std::os::unix::fs::symlink(&link_target, &tmp_link).with_context(|| {
                        format!("failed creating temp symlink for `{}`", entry.rel_path)
                    })?;
                    // Validate resolved symlink target inside canon_target_root before renaming into place
                    let target_valid = validate_symlink_strict_sync(canon_target_root, &tmp_link);
                    if !target_valid {
                        let _ = std::fs::remove_file(&tmp_link);
                        anyhow::bail!(
                            "escaping_symlink: untracked symlink `{}` escapes target repository root",
                            entry.rel_path
                        );
                    }
                    if let Err(err) = std::fs::rename(&tmp_link, &dst_abs) {
                        let _ = std::fs::remove_file(&tmp_link);
                        return Err(err).with_context(|| {
                            format!("failed atomically installing symlink `{}`", entry.rel_path)
                        });
                    }
                    if !existed_before {
                        newly_written.push(dst_abs);
                    }
                    copied += 1;
                }
                #[cfg(not(unix))]
                {
                    if src_abs.is_file() {
                        copy_file_atomic_via_temp(canon_source_root, &src_abs, &dst_abs)?;
                        if !existed_before {
                            newly_written.push(dst_abs);
                        }
                        copied += 1;
                    }
                }
            } else {
                copy_file_atomic_via_temp(canon_source_root, &src_abs, &dst_abs)?;
                if !existed_before {
                    newly_written.push(dst_abs.clone());
                }
                let src_len = std::fs::metadata(&src_abs)?.len();
                let dst_len = std::fs::metadata(&dst_abs)?.len();
                if src_len != dst_len {
                    anyhow::bail!(
                        "verification_failed: copied file `{}` size mismatch ({dst_len} != {src_len})",
                        entry.rel_path
                    );
                }
                copied += 1;
            }
        }
        Ok(copied)
    })();

    if let Err(err) = copy_result {
        for written in newly_written.into_iter().rev() {
            let _ = std::fs::remove_file(&written);
            let mut cur_parent = written.parent();
            while let Some(parent) = cur_parent {
                if parent == canon_target_root || !parent.starts_with(canon_target_root) {
                    break;
                }
                if std::fs::remove_dir(parent).is_err() {
                    break;
                }
                cur_parent = parent.parent();
            }
        }
        return Err(err);
    }

    Ok(copied)
}

fn copy_file_atomic_via_temp(
    canon_source_root: &Path,
    src_abs: &Path,
    dst_abs: &Path,
) -> anyhow::Result<()> {
    let parent = dst_abs
        .parent()
        .ok_or_else(|| anyhow::anyhow!("destination has no parent: {}", dst_abs.display()))?;
    let tmp_name = format!(".orbit-handoff-{}.tmp", Uuid::new_v4().simple());
    let tmp_path = parent.join(tmp_name);

    let copy_res = copy_file_cow_within(canon_source_root, src_abs, &tmp_path);
    if let Err(err) = copy_res {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    if let Ok(meta) = std::fs::metadata(src_abs) {
        let _ = std::fs::set_permissions(&tmp_path, meta.permissions());
    }

    if let Ok(dst_meta) = std::fs::symlink_metadata(dst_abs)
        && dst_meta.file_type().is_symlink()
    {
        let _ = std::fs::remove_file(dst_abs);
    }

    if let Err(err) = std::fs::rename(&tmp_path, dst_abs) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err).with_context(|| {
            format!(
                "failed atomically renaming temp file into {}",
                dst_abs.display()
            )
        });
    }
    Ok(())
}

/// Cleanly reset ONLY uncommitted working-tree changes (staged + intent-to-add + unstaged +
/// transferred untracked) on `source_toplevel`, preserving all committed Git history untouched
/// and never deleting files that exist in `HEAD` (e.g. `git rm --cached f`).
async fn reset_source_uncommitted_changes(
    source: &GitCheckoutInfo,
    transferred_untracked: &[ValidatedUntracked],
) -> anyhow::Result<()> {
    let mut paths_to_remove: BTreeSet<String> = transferred_untracked
        .iter()
        .map(|u| u.rel_path.clone())
        .collect();

    if source.head_oid.is_some() {
        // 1. Collect all paths tracked in HEAD so they are NEVER deleted (handles `git rm --cached`)
        let head_tree_out = run_git_output(
            &source.toplevel,
            &["ls-tree", "-r", "--name-only", "-z", "HEAD"],
            GIT_FAST_TIMEOUT,
        )
        .await?;
        if !head_tree_out.status.success() {
            let stderr = String::from_utf8_lossy(&head_tree_out.stderr);
            anyhow::bail!("git ls-tree HEAD failed: {}", stderr.trim());
        }
        let head_paths: BTreeSet<String> = head_tree_out
            .stdout
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();

        // 2. Capture newly staged files (`--no-renames` handles `git mv` destination paths!)
        let added_cached_out = run_isolated_git_diff(
            &source.toplevel,
            &[
                "--cached",
                "--name-only",
                "--diff-filter=A",
                "--no-renames",
                "-z",
                "HEAD",
                "--",
            ],
            GIT_FAST_TIMEOUT,
        )
        .await?;
        if added_cached_out.status.success() {
            for raw in added_cached_out
                .stdout
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
            {
                paths_to_remove.insert(String::from_utf8_lossy(raw).into_owned());
            }
        }

        // 3. Also capture intent-to-add (`git add -N`) paths!
        let added_wt_out = run_isolated_git_diff(
            &source.toplevel,
            &[
                "--name-only",
                "--diff-filter=A",
                "--no-renames",
                "-z",
                "HEAD",
                "--",
            ],
            GIT_FAST_TIMEOUT,
        )
        .await?;
        if added_wt_out.status.success() {
            for raw in added_wt_out
                .stdout
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
            {
                paths_to_remove.insert(String::from_utf8_lossy(raw).into_owned());
            }
        }

        // Never delete any path present in HEAD
        paths_to_remove.retain(|p| !head_paths.contains(p));

        let reset_out = run_git_output(
            &source.toplevel,
            &["reset", "--mixed", "--quiet", "HEAD"],
            GIT_FAST_TIMEOUT,
        )
        .await?;
        if !reset_out.status.success() {
            let stderr = String::from_utf8_lossy(&reset_out.stderr);
            anyhow::bail!("git reset HEAD failed: {}", stderr.trim());
        }

        // Skip `git checkout -- .` when HEAD is an empty commit (`head_paths` is empty)
        if !head_paths.is_empty() {
            let checkout_out =
                run_git_output(&source.toplevel, &["checkout", "--", "."], GIT_DIFF_TIMEOUT)
                    .await?;
            if !checkout_out.status.success() {
                let stderr = String::from_utf8_lossy(&checkout_out.stderr);
                anyhow::bail!("git checkout -- . failed: {}", stderr.trim());
            }
        }
    } else {
        // Unborn HEAD: collect staged files and remove from index
        let ls_out =
            run_git_output(&source.toplevel, &["ls-files", "-z"], GIT_FAST_TIMEOUT).await?;
        if ls_out.status.success() {
            for raw in ls_out.stdout.split(|&b| b == 0).filter(|s| !s.is_empty()) {
                paths_to_remove.insert(String::from_utf8_lossy(raw).into_owned());
            }
        }
        let _ = run_git_output(
            &source.toplevel,
            &["rm", "--cached", "-rf", "--quiet", "--", "."],
            GIT_FAST_TIMEOUT,
        )
        .await;
    }

    let canon_root = source.toplevel.clone();
    tokio::task::spawn_blocking(move || {
        for rel in paths_to_remove {
            if !is_safe_handoff_rel_path(&rel) {
                continue;
            }
            let abs = canon_root.join(&rel);
            let Ok(meta) = std::fs::symlink_metadata(&abs) else {
                continue;
            };
            if meta.file_type().is_dir() && !meta.file_type().is_symlink() {
                continue;
            }
            let _ = std::fs::remove_file(&abs);
            // Prune now-empty parent directories strictly inside repo root
            let mut cur_parent = abs.parent();
            while let Some(parent) = cur_parent {
                if parent == canon_root || !parent.starts_with(&canon_root) {
                    break;
                }
                if std::fs::remove_dir(parent).is_err() {
                    break;
                }
                cur_parent = parent.parent();
            }
        }
    })
    .await
    .context("cleanup task failed")?;

    Ok(())
}

/// Non-mutating dry-run preview of a worktree <-> local handoff between resolved filesystem paths.
pub async fn preview_worktree_handoff_paths(
    spec: HandoffPreviewPathsSpec,
) -> anyhow::Result<WorktreeHandoffPreview> {
    let source = resolve_git_checkout_info(&spec.source_cwd).await?;

    let target = match (&spec.direction, &spec.target_cwd) {
        (WorktreeHandoffDirection::WorktreeToLocal, Some(t)) => {
            let tgt = resolve_git_checkout_info(t).await?;
            ensure_same_repository(&source, &tgt)?;
            Some(tgt)
        }
        (WorktreeHandoffDirection::WorktreeToLocal, None) => {
            let inferred = resolve_primary_local_checkout(&source.toplevel).await?;
            let tgt = resolve_git_checkout_info(&inferred).await?;
            ensure_same_repository(&source, &tgt)?;
            Some(tgt)
        }
        (WorktreeHandoffDirection::LocalToWorktree, Some(t)) => {
            let tgt = resolve_git_checkout_info(t).await?;
            ensure_same_repository(&source, &tgt)?;
            Some(tgt)
        }
        (WorktreeHandoffDirection::LocalToWorktree, None) => None,
    };

    let changes = collect_source_changes(
        &source,
        target.as_ref(),
        spec.direction,
        spec.include_committed,
        spec.base_ref.as_deref(),
    )
    .await?;

    let (target_dirty, conflicts) = if let Some(ref tgt) = target {
        let dirty = is_checkout_dirty(&tgt.toplevel).await;
        let confs =
            detect_conflicts_non_mutating(&source.toplevel, &tgt.toplevel, &changes).await?;
        (dirty, confs)
    } else {
        (false, Vec::new())
    };

    let conflict_set: BTreeSet<&str> = conflicts.iter().map(String::as_str).collect();
    let mut files: Vec<HandoffFileChange> = changes
        .tracked_entries
        .into_iter()
        .map(|e| e.change)
        .collect();
    for u in &changes.untracked_valid {
        files.push(HandoffFileChange {
            path: u.rel_path.clone(),
            status: "??".to_string(),
            insertions: u.insertions,
            deletions: 0,
            is_untracked: true,
            has_conflict: false,
        });
    }

    let mut total_insertions = 0u32;
    let mut total_deletions = 0u32;
    for f in &mut files {
        f.has_conflict = conflict_set.contains(f.path.as_str());
        total_insertions = total_insertions.saturating_add(f.insertions);
        total_deletions = total_deletions.saturating_add(f.deletions);
    }

    let can_apply_cleanly = conflicts.is_empty() && changes.unsafe_tracked_symlinks.is_empty();
    let mut warning_parts: Vec<String> = Vec::new();
    if !changes.unsafe_tracked_symlinks.is_empty() {
        warning_parts.push(format!(
            "Blocked {} unsafe tracked symlink(s) escaping repository root: {}",
            changes.unsafe_tracked_symlinks.len(),
            changes.unsafe_tracked_symlinks.join(", ")
        ));
    }
    if !changes.escaping_untracked.is_empty() {
        warning_parts.push(format!(
            "Skipped {} unsafe untracked symlink(s) escaping repository root",
            changes.escaping_untracked.len()
        ));
    }
    if !conflicts.is_empty() {
        warning_parts.push(format!(
            "{} file(s) conflict with target checkout",
            conflicts.len()
        ));
    } else if files.is_empty() && changes.unsafe_tracked_symlinks.is_empty() {
        warning_parts.push("No changes to transfer".to_string());
    }

    let target_cwd_display = target
        .as_ref()
        .map(|t| t.toplevel.to_string_lossy().to_string())
        .unwrap_or_else(|| "(new worktree)".to_string());

    Ok(WorktreeHandoffPreview {
        direction: spec.direction,
        source_cwd: source.toplevel.to_string_lossy().to_string(),
        target_cwd: target_cwd_display,
        source_branch: source.branch,
        target_branch: target.and_then(|t| t.branch),
        commit_count: changes.commit_count,
        files,
        total_insertions,
        total_deletions,
        target_dirty,
        conflicts,
        can_apply_cleanly,
        warning: if warning_parts.is_empty() {
            None
        } else {
            Some(warning_parts.join(" · "))
        },
    })
}

/// Validate a proposed branch name for `LocalToWorktree` before creating any worktree or mutating state.
pub async fn validate_handoff_branch_name(
    source_cwd: &Path,
    branch: &str,
) -> anyhow::Result<String> {
    let source = resolve_git_checkout_info(source_cwd).await?;
    validate_git_branch_name(&source.toplevel, branch).await
}

/// Validate source checkout has no escaping untracked or tracked (`120000`) symlinks BEFORE
/// provisioning a new worktree or mutating disk.
pub async fn preflight_validate_source_safety(source_cwd: &Path) -> anyhow::Result<()> {
    let source = resolve_git_checkout_info(source_cwd).await?;
    let base_tree_arg = source.head_oid.as_deref().unwrap_or(EMPTY_TREE_OID);

    // 1. Validate tracked symlinks in working-tree delta
    let raw_out = run_isolated_git_diff(
        &source.toplevel,
        &["--raw", "-z", "--no-renames", base_tree_arg, "--"],
        GIT_DIFF_TIMEOUT,
    )
    .await?;
    if raw_out.status.success() {
        let symlink_paths = parse_raw_tracked_symlink_paths(&raw_out.stdout);
        if !symlink_paths.is_empty() {
            let canon_source = source.toplevel.clone();
            let unsafe_tracked = tokio::task::spawn_blocking(move || {
                validate_tracked_symlinks_sync(&canon_source, symlink_paths)
            })
            .await
            .context("tracked symlink preflight validation panicked")?;
            if !unsafe_tracked.is_empty() {
                anyhow::bail!(
                    "escaping_symlink: refused to hand off unsafe tracked symlink(s): {}",
                    unsafe_tracked.join(", ")
                );
            }
        }
    }

    // 2. Validate untracked files
    let untracked_out = run_git_output(
        &source.toplevel,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        GIT_FAST_TIMEOUT,
    )
    .await?;
    if !untracked_out.status.success() {
        return Ok(());
    }
    let raw_untracked: Vec<String> = untracked_out
        .stdout
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    let canon_source = source.toplevel.clone();
    let (_, escaping) = tokio::task::spawn_blocking(move || {
        inspect_untracked_files_sync(&canon_source, raw_untracked)
    })
    .await
    .context("untracked preflight validation failed")??;
    if !escaping.is_empty() {
        anyhow::bail!(
            "escaping_symlink: refused to hand off unsafe untracked path(s) escaping repository root: {}",
            escaping.join(", ")
        );
    }
    Ok(())
}

/// Delete a newly created local Git branch during rollback (never fails caller flow).
pub async fn force_delete_git_branch(repo_cwd: &Path, branch: &str) {
    let trimmed = branch.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return;
    }
    let _ = run_git_output(repo_cwd, &["branch", "-D", "--", trimmed], GIT_FAST_TIMEOUT).await;
}

/// Atomically execute a backup-protected worktree <-> local handoff between resolved source and target paths.
pub async fn execute_worktree_handoff_paths(
    spec: HandoffExecutePathsSpec,
) -> anyhow::Result<WorktreeHandoffOutcome> {
    let source = resolve_git_checkout_info(&spec.source_cwd).await?;
    let target = resolve_git_checkout_info(&spec.target_cwd).await?;
    ensure_same_repository(&source, &target)?;

    let changes = collect_source_changes(
        &source,
        Some(&target),
        spec.direction,
        spec.include_committed,
        spec.base_ref.as_deref(),
    )
    .await?;

    // Strict security check: reject any escaping tracked or untracked symlink before mutation
    if !changes.unsafe_tracked_symlinks.is_empty() {
        anyhow::bail!(
            "escaping_symlink: refused to transfer unsafe tracked symlink(s) escaping repository root: {}",
            changes.unsafe_tracked_symlinks.join(", ")
        );
    }
    if !changes.escaping_untracked.is_empty() {
        anyhow::bail!(
            "escaping_symlink: refused to transfer unsafe untracked path(s) escaping repository root: {}",
            changes.escaping_untracked.join(", ")
        );
    }

    let total_files_to_apply = changes.tracked_entries.len() + changes.untracked_valid.len();
    if total_files_to_apply == 0 && changes.patch_bytes.is_empty() {
        let target_label = target.branch.as_deref().unwrap_or("target");
        let summary = match spec.direction {
            WorktreeHandoffDirection::WorktreeToLocal => {
                "No changes to apply to local checkout.".to_string()
            }
            WorktreeHandoffDirection::LocalToWorktree => {
                if spec.target_workspace.is_some() {
                    format!(
                        "Created worktree `{target_label}` (no uncommitted changes to transfer)."
                    )
                } else {
                    "No uncommitted changes to transfer.".to_string()
                }
            }
        };
        return Ok(WorktreeHandoffOutcome {
            direction: spec.direction,
            mode: spec.mode,
            applied_files: 0,
            backup_ref: None,
            target_workspace: spec.target_workspace,
            target_cwd: target.toplevel.to_string_lossy().to_string(),
            had_conflicts: false,
            conflict_files: Vec::new(),
            summary,
        });
    }

    // Dry-run conflict detection AND untracked destination pre-validation BEFORE mutating anything
    let detected_conflicts =
        detect_conflicts_non_mutating(&source.toplevel, &target.toplevel, &changes).await?;

    // Untracked/directory collisions are ALWAYS rejected (even under `allow_three_way_conflicts`)
    // so differing untracked files in target are never silently clobbered.
    let untracked_set: BTreeSet<&str> = changes
        .untracked_valid
        .iter()
        .map(|u| u.rel_path.as_str())
        .collect();
    let untracked_conflicts: Vec<String> = detected_conflicts
        .iter()
        .filter(|p| untracked_set.contains(p.as_str()))
        .cloned()
        .collect();
    if !untracked_conflicts.is_empty() {
        anyhow::bail!(
            "handoff_conflict: target already contains differing untracked file/directory collision(s): {}",
            untracked_conflicts.join(", ")
        );
    }
    if !changes.untracked_valid.is_empty() {
        let src_root = source.toplevel.clone();
        let tgt_root = target.toplevel.clone();
        let entries = changes.untracked_valid.clone();
        tokio::task::spawn_blocking(move || {
            validate_target_untracked_destinations_sync(&src_root, &tgt_root, &entries)
        })
        .await
        .context("untracked destination validation task failed")??;
    }
    if !detected_conflicts.is_empty() && !spec.allow_three_way_conflicts {
        anyhow::bail!(
            "handoff_conflict: {} file(s) conflict with target checkout: {}",
            detected_conflicts.len(),
            detected_conflicts.join(", ")
        );
    }

    // Capture fail-closed safety backup refs BEFORE mutating target or source
    let backup_ref = if spec.backup_source_as_primary {
        Some(
            capture_handoff_backup_ref(
                &source.toplevel,
                source.head_oid.as_deref(),
                spec.direction,
                spec.mode,
                "source",
            )
            .await?,
        )
    } else {
        let primary = capture_handoff_backup_ref(
            &target.toplevel,
            target.head_oid.as_deref(),
            spec.direction,
            spec.mode,
            "target",
        )
        .await?;
        if spec.mode == WorktreeHandoffMode::Move {
            // Fail-closed: source snapshot MUST succeed before any mutation in Move mode
            capture_handoff_backup_ref(
                &source.toplevel,
                source.head_oid.as_deref(),
                spec.direction,
                spec.mode,
                "source",
            )
            .await
            .context("failed capturing pre-Move source safety backup ref")?;
        }
        Some(primary)
    };

    // Apply tracked patch
    let mut had_conflicts = false;
    let mut conflict_files: Vec<String> = Vec::new();
    if !changes.patch_bytes.is_empty() {
        let clean_out = run_git_with_stdin(
            &target.toplevel,
            &[
                "-c",
                "core.quotePath=false",
                "apply",
                "--whitespace=nowarn",
                "-",
            ],
            &changes.patch_bytes,
            None,
            GIT_DIFF_TIMEOUT,
        )
        .await?;

        if !clean_out.status.success() {
            if !spec.allow_three_way_conflicts {
                let stderr = String::from_utf8_lossy(&clean_out.stderr);
                anyhow::bail!("git apply failed: {}", stderr.trim());
            }

            // Seed isolated temporary index from target's real index/HEAD + working tree so
            // `git apply --3way` works cleanly with unstaged edits, without touching `.git/index`.
            let tmp_index = std::env::temp_dir().join(format!(
                "orbit-handoff-3way-{}.idx",
                Uuid::new_v4().simple()
            ));
            let _guard = TempIndexGuard(tmp_index.clone());

            populate_temp_index_snapshot(&target.toplevel, target.head_oid.as_deref(), &tmp_index)
                .await
                .context("failed preparing temporary 3-way index")?;

            let three_way = run_git_with_stdin(
                &target.toplevel,
                &[
                    "-c",
                    "core.quotePath=false",
                    "apply",
                    "--3way",
                    "--whitespace=nowarn",
                    "-",
                ],
                &changes.patch_bytes,
                Some(&tmp_index),
                GIT_DIFF_TIMEOUT,
            )
            .await?;

            if !three_way.status.success() {
                let unmerged_out = run_git_output_with_index(
                    &target.toplevel,
                    &[
                        "-c",
                        "core.quotePath=false",
                        "diff",
                        "--name-only",
                        "--diff-filter=U",
                        "-z",
                    ],
                    &tmp_index,
                    GIT_FAST_TIMEOUT,
                )
                .await?;
                let unmerged: Vec<String> = if unmerged_out.status.success() {
                    unmerged_out
                        .stdout
                        .split(|&b| b == 0)
                        .filter(|s| !s.is_empty())
                        .map(|s| String::from_utf8_lossy(s).into_owned())
                        .collect()
                } else {
                    Vec::new()
                };
                if unmerged.is_empty() {
                    let stderr = String::from_utf8_lossy(&three_way.stderr);
                    anyhow::bail!("git apply --3way failed: {}", stderr.trim());
                }
                had_conflicts = true;
                conflict_files = unmerged;
            }
        }
    }

    // Copy validated untracked files atomically; if this fails after patch apply, surface backup_ref clearly
    let copied_untracked = if !changes.untracked_valid.is_empty() {
        let src_root = source.toplevel.clone();
        let tgt_root = target.toplevel.clone();
        let entries = changes.untracked_valid.clone();
        let copy_res = tokio::task::spawn_blocking(move || {
            copy_untracked_files_atomically_sync(&src_root, &tgt_root, &entries)
        })
        .await
        .context("untracked copy task failed")?;
        match copy_res {
            Ok(n) => n,
            Err(err) => {
                if let Some(ref bref) = backup_ref {
                    anyhow::bail!(
                        "failed copying untracked files after patch apply (recover target from backup ref `{bref}`): {err}"
                    );
                }
                return Err(err);
            }
        }
    } else {
        0
    };

    let applied_files = u32::try_from(
        changes
            .tracked_entries
            .len()
            .saturating_add(copied_untracked),
    )
    .unwrap_or(u32::MAX);

    // In `Move` mode, cleanly reset uncommitted source changes ONLY after clean verified transfer
    let mut source_reset_done = false;
    if spec.mode == WorktreeHandoffMode::Move && !had_conflicts {
        reset_source_uncommitted_changes(&source, &changes.untracked_valid).await?;
        source_reset_done = true;
    }

    let verb = match (spec.mode, source_reset_done) {
        (WorktreeHandoffMode::Move, true) => "Moved",
        _ => "Applied",
    };
    let dest_label = match spec.direction {
        WorktreeHandoffDirection::WorktreeToLocal => match target.branch.as_deref() {
            Some(b) => format!("Local ({b})"),
            None => "Local checkout".to_string(),
        },
        WorktreeHandoffDirection::LocalToWorktree => match target.branch.as_deref() {
            Some(b) => format!("worktree `{b}`"),
            None => "worktree".to_string(),
        },
    };
    let file_word = if applied_files == 1 { "file" } else { "files" };
    let mut summary = format!("{verb} {applied_files} {file_word} to {dest_label}");
    if had_conflicts {
        summary.push_str(&format!(
            " ({} conflict marker file(s) need resolution)",
            conflict_files.len()
        ));
    } else if spec.direction == WorktreeHandoffDirection::WorktreeToLocal
        && spec.mode == WorktreeHandoffMode::Move
        && changes.commit_count > 0
    {
        summary.push_str(" (worktree commits preserved)");
    }

    Ok(WorktreeHandoffOutcome {
        direction: spec.direction,
        mode: spec.mode,
        applied_files,
        backup_ref,
        target_workspace: spec.target_workspace,
        target_cwd: target.toplevel.to_string_lossy().to_string(),
        had_conflicts,
        conflict_files,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn git_ok(cwd: &Path, args: &[&str]) {
        let out = git_cmd_clean(cwd)
            .args(args)
            .output()
            .await
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {:?} failed in {}:\nstdout: {}\nstderr: {}",
            args,
            cwd.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    async fn git_stdout(cwd: &Path, args: &[&str]) -> String {
        let out = git_cmd_clean(cwd)
            .args(args)
            .output()
            .await
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    async fn init_test_repo(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("orbit_handoff_{name}_{}", Uuid::new_v4()));
        let main_dir = root.join("main-repo");
        tokio::fs::create_dir_all(&main_dir).await.expect("mkdir");
        git_ok(&main_dir, &["init", "-q", "-b", "main"]).await;
        git_ok(&main_dir, &["config", "core.autocrlf", "false"]).await;
        git_ok(&main_dir, &["config", "user.email", "test@orbit.local"]).await;
        git_ok(&main_dir, &["config", "user.name", "Orbit Test"]).await;

        tokio::fs::write(main_dir.join("app.txt"), "line1\nline2\nline3\n")
            .await
            .expect("write app");
        tokio::fs::write(main_dir.join("keep.txt"), "unchanged\n")
            .await
            .expect("write keep");
        git_ok(&main_dir, &["add", "-A"]).await;
        git_ok(&main_dir, &["commit", "-q", "-m", "initial"]).await;
        (root, main_dir)
    }

    async fn add_linked_worktree(main_dir: &Path, wt_dir: &Path, branch: &str) {
        git_ok(
            main_dir,
            &[
                "worktree",
                "add",
                "-b",
                branch,
                wt_dir.to_str().expect("utf8 path"),
            ],
        )
        .await;
    }

    #[tokio::test]
    async fn worktree_to_local_apply_committed_unstaged_untracked_and_binary_with_backup_ref() {
        let (root, main_dir) = init_test_repo("wt_to_local_apply").await;
        let wt_dir = root.join("wt-feature");
        add_linked_worktree(&main_dir, &wt_dir, "feat/handoff").await;

        // 1. Committed change in worktree
        tokio::fs::write(wt_dir.join("app.txt"), "line1\nline2-committed\nline3\n")
            .await
            .expect("edit app");
        git_ok(&wt_dir, &["add", "app.txt"]).await;
        git_ok(&wt_dir, &["commit", "-q", "-m", "worktree commit"]).await;

        // 2. Unstaged tracked modification + binary committed file
        let bin_bytes: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x00, 0xff, 0x10, 0x42];
        tokio::fs::write(wt_dir.join("icon.bin"), &bin_bytes)
            .await
            .expect("write bin");
        git_ok(&wt_dir, &["add", "icon.bin"]).await;
        git_ok(&wt_dir, &["commit", "-q", "-m", "add binary"]).await;

        tokio::fs::write(wt_dir.join("keep.txt"), "unchanged\nunstaged-addition\n")
            .await
            .expect("edit keep");

        // 3. Untracked nested file
        tokio::fs::create_dir_all(wt_dir.join("notes"))
            .await
            .expect("mkdir notes");
        tokio::fs::write(wt_dir.join("notes/todo.md"), "# Todo\n- ship it\n")
            .await
            .expect("write untracked");

        // Preview first (non-mutating)
        let preview = preview_worktree_handoff_paths(HandoffPreviewPathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: None,
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            include_committed: true,
            base_ref: None,
        })
        .await
        .expect("preview");

        assert_eq!(preview.commit_count, 2);
        assert!(preview.can_apply_cleanly);
        assert!(preview.conflicts.is_empty());
        assert_eq!(preview.files.len(), 4);
        assert_eq!(preview.source_branch.as_deref(), Some("feat/handoff"));
        assert_eq!(preview.target_branch.as_deref(), Some("main"));
        assert!(!main_dir.join("icon.bin").exists());
        assert!(!main_dir.join("notes/todo.md").exists());

        // Execute Apply
        let outcome = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: main_dir.clone(),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            mode: WorktreeHandoffMode::Apply,
            include_committed: true,
            base_ref: None,
            allow_three_way_conflicts: false,
            target_workspace: None,
            backup_source_as_primary: false,
        })
        .await
        .expect("execute apply");

        assert_eq!(outcome.applied_files, 4);
        assert!(!outcome.had_conflicts);
        let backup_ref = outcome.backup_ref.expect("backup ref created");
        assert!(backup_ref.starts_with("refs/orbit/handoff-backup/"));
        assert!(backup_ref.ends_with("-target"));

        // Verify backup ref resolves to pre-handoff local state
        let backup_oid = git_stdout(&main_dir, &["rev-parse", "--verify", &backup_ref]).await;
        assert!(is_valid_git_oid(&backup_oid));
        let backup_app = git_stdout(&main_dir, &["show", &format!("{backup_ref}:app.txt")]).await;
        assert_eq!(backup_app, "line1\nline2\nline3");

        // Verify exact content in local checkout
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("app.txt"))
                .await
                .expect("read local app"),
            "line1\nline2-committed\nline3\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("keep.txt"))
                .await
                .expect("read local keep"),
            "unchanged\nunstaged-addition\n"
        );
        assert_eq!(
            tokio::fs::read(main_dir.join("icon.bin"))
                .await
                .expect("read local bin"),
            bin_bytes
        );
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("notes/todo.md"))
                .await
                .expect("read local untracked"),
            "# Todo\n- ship it\n"
        );

        // Apply mode keeps source worktree intact
        assert!(wt_dir.join("notes/todo.md").exists());
        assert_eq!(
            tokio::fs::read_to_string(wt_dir.join("keep.txt"))
                .await
                .expect("read wt keep"),
            "unchanged\nunstaged-addition\n"
        );

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn worktree_to_local_preview_detects_conflicts_with_zero_disk_mutation() {
        let (root, main_dir) = init_test_repo("wt_conflict").await;
        let wt_dir = root.join("wt-conflict");
        add_linked_worktree(&main_dir, &wt_dir, "feat/conflict").await;

        // Both worktree and local edit the exact same line differently
        tokio::fs::write(wt_dir.join("app.txt"), "line1\nWORKTREE-EDIT\nline3\n")
            .await
            .expect("edit wt");
        tokio::fs::write(main_dir.join("app.txt"), "line1\nLOCAL-DIRTY-EDIT\nline3\n")
            .await
            .expect("edit local");

        let preview = preview_worktree_handoff_paths(HandoffPreviewPathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: Some(main_dir.clone()),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            include_committed: true,
            base_ref: None,
        })
        .await
        .expect("preview conflict");

        assert!(preview.target_dirty);
        assert!(!preview.can_apply_cleanly);
        assert_eq!(preview.conflicts, vec!["app.txt".to_string()]);
        assert!(
            preview
                .files
                .iter()
                .any(|f| f.path == "app.txt" && f.has_conflict)
        );

        // Local file MUST remain untouched after dry-run preview
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("app.txt"))
                .await
                .expect("read local"),
            "line1\nLOCAL-DIRTY-EDIT\nline3\n"
        );

        // Clean execute without 3-way flag MUST fail cleanly without mutating disk
        let err = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: main_dir.clone(),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            mode: WorktreeHandoffMode::Apply,
            include_committed: true,
            base_ref: None,
            allow_three_way_conflicts: false,
            target_workspace: None,
            backup_source_as_primary: false,
        })
        .await
        .expect_err("must reject clean apply on conflict");
        assert!(err.to_string().contains("handoff_conflict"));
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("app.txt"))
                .await
                .expect("read local after reject"),
            "line1\nLOCAL-DIRTY-EDIT\nline3\n"
        );

        // With `allow_three_way_conflicts: true`, 3-way conflict markers are written cleanly
        let three_way = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: main_dir.clone(),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            mode: WorktreeHandoffMode::Apply,
            include_committed: true,
            base_ref: None,
            allow_three_way_conflicts: true,
            target_workspace: None,
            backup_source_as_primary: false,
        })
        .await
        .expect("3way apply");
        assert!(three_way.had_conflicts);
        assert_eq!(three_way.conflict_files, vec!["app.txt".to_string()]);
        let merged = tokio::fs::read_to_string(main_dir.join("app.txt"))
            .await
            .expect("read merged");
        assert!(merged.contains("<<<<<<<") && merged.contains(">>>>>>>"));

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn worktree_to_local_move_resets_uncommitted_while_preserving_committed_history() {
        let (root, main_dir) = init_test_repo("wt_move").await;
        let wt_dir = root.join("wt-move");
        add_linked_worktree(&main_dir, &wt_dir, "feat/move-test").await;

        // 1. Commit in worktree (must NEVER be deleted by Move!)
        tokio::fs::write(wt_dir.join("app.txt"), "line1\ncommitted-in-wt\nline3\n")
            .await
            .expect("write committed");
        git_ok(&wt_dir, &["add", "app.txt"]).await;
        git_ok(&wt_dir, &["commit", "-q", "-m", "keep this commit"]).await;
        let wt_head_before = git_stdout(&wt_dir, &["rev-parse", "HEAD"]).await;

        // 2. Uncommitted staged + unstaged + untracked changes
        tokio::fs::write(wt_dir.join("staged_new.txt"), "brand new staged\n")
            .await
            .expect("write staged new");
        git_ok(&wt_dir, &["add", "staged_new.txt"]).await;
        tokio::fs::write(wt_dir.join("keep.txt"), "unchanged\ndirty-in-wt\n")
            .await
            .expect("write dirty");
        tokio::fs::write(wt_dir.join("scratch.txt"), "untracked-in-wt\n")
            .await
            .expect("write untracked");

        let outcome = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: main_dir.clone(),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            mode: WorktreeHandoffMode::Move,
            include_committed: true,
            base_ref: None,
            allow_three_way_conflicts: false,
            target_workspace: None,
            backup_source_as_primary: false,
        })
        .await
        .expect("execute move");

        assert_eq!(outcome.applied_files, 4);
        assert!(!outcome.had_conflicts);

        // Local checkout received all 4 changes
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("app.txt"))
                .await
                .expect("read local app"),
            "line1\ncommitted-in-wt\nline3\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("staged_new.txt"))
                .await
                .expect("read local staged"),
            "brand new staged\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("keep.txt"))
                .await
                .expect("read local keep"),
            "unchanged\ndirty-in-wt\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("scratch.txt"))
                .await
                .expect("read local scratch"),
            "untracked-in-wt\n"
        );

        // Worktree uncommitted edits are cleanly reset, while committed history is untouched!
        let wt_head_after = git_stdout(&wt_dir, &["rev-parse", "HEAD"]).await;
        assert_eq!(wt_head_before, wt_head_after);
        assert!(!is_checkout_dirty(&wt_dir).await);
        assert!(!wt_dir.join("staged_new.txt").exists());
        assert!(!wt_dir.join("scratch.txt").exists());
        assert_eq!(
            tokio::fs::read_to_string(wt_dir.join("keep.txt"))
                .await
                .expect("read wt keep"),
            "unchanged\n"
        );

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn local_to_worktree_move_transfers_staged_unstaged_untracked_and_cleans_local() {
        let (root, main_dir) = init_test_repo("local_to_wt_move").await;

        // Dirty staged + unstaged + untracked changes in local checkout
        tokio::fs::write(main_dir.join("app.txt"), "line1\nstaged-edit\nline3\n")
            .await
            .expect("edit app");
        git_ok(&main_dir, &["add", "app.txt"]).await;
        tokio::fs::write(main_dir.join("newly_staged.rs"), "pub fn staged() {}\n")
            .await
            .expect("write newly staged");
        git_ok(&main_dir, &["add", "newly_staged.rs"]).await;
        tokio::fs::write(main_dir.join("keep.txt"), "unchanged\nunstaged-edit\n")
            .await
            .expect("edit keep");
        tokio::fs::create_dir_all(main_dir.join("sub/dir"))
            .await
            .expect("mkdir sub");
        tokio::fs::write(main_dir.join("sub/dir/new.txt"), "untracked content\n")
            .await
            .expect("write untracked");

        // Preview targeting new worktree (`target_cwd: None`)
        let preview = preview_worktree_handoff_paths(HandoffPreviewPathsSpec {
            source_cwd: main_dir.clone(),
            target_cwd: None,
            direction: WorktreeHandoffDirection::LocalToWorktree,
            include_committed: false,
            base_ref: None,
        })
        .await
        .expect("preview local->new wt");
        assert!(preview.can_apply_cleanly);
        assert!(preview.conflicts.is_empty());
        assert_eq!(preview.files.len(), 4);

        // Create fresh worktree off current local HEAD & execute Move
        let wt_dir = root.join("wt-fresh");
        add_linked_worktree(&main_dir, &wt_dir, "orbit/moved-local").await;

        let outcome = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
            source_cwd: main_dir.clone(),
            target_cwd: wt_dir.clone(),
            direction: WorktreeHandoffDirection::LocalToWorktree,
            mode: WorktreeHandoffMode::Move,
            include_committed: false,
            base_ref: None,
            allow_three_way_conflicts: false,
            target_workspace: None,
            backup_source_as_primary: true,
        })
        .await
        .expect("execute local->wt move");

        assert_eq!(outcome.applied_files, 4);
        assert!(
            outcome
                .backup_ref
                .as_deref()
                .is_some_and(|r| r.ends_with("-source"))
        );

        // Worktree received every change
        assert_eq!(
            tokio::fs::read_to_string(wt_dir.join("app.txt"))
                .await
                .expect("wt app"),
            "line1\nstaged-edit\nline3\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(wt_dir.join("newly_staged.rs"))
                .await
                .expect("wt newly_staged"),
            "pub fn staged() {}\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(wt_dir.join("keep.txt"))
                .await
                .expect("wt keep"),
            "unchanged\nunstaged-edit\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(wt_dir.join("sub/dir/new.txt"))
                .await
                .expect("wt untracked"),
            "untracked content\n"
        );

        // Local checkout is now 100% clean!
        assert!(!is_checkout_dirty(&main_dir).await);
        assert!(!main_dir.join("newly_staged.rs").exists());
        assert!(!main_dir.join("sub/dir/new.txt").exists());
        assert!(!main_dir.join("sub").exists());
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("app.txt"))
                .await
                .expect("local app clean"),
            "line1\nline2\nline3\n"
        );

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn security_rejects_escaping_symlink_flag_branch_and_unrelated_repos() {
        let (root_a, repo_a) = init_test_repo("sec_a").await;
        let (root_b, repo_b) = init_test_repo("sec_b").await;
        let wt_a = root_a.join("wt-a");
        add_linked_worktree(&repo_a, &wt_a, "feat/sec").await;

        // 1. Reject unrelated git repositories
        let unrelated_err = preview_worktree_handoff_paths(HandoffPreviewPathsSpec {
            source_cwd: wt_a.clone(),
            target_cwd: Some(repo_b.clone()),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            include_committed: true,
            base_ref: None,
        })
        .await
        .expect_err("must reject unrelated repos");
        assert!(unrelated_err.to_string().contains("unrelated_git_repo"));

        // 2. Reject primary checkout passed to WorktreeToLocal with no target
        let primary_err = preview_worktree_handoff_paths(HandoffPreviewPathsSpec {
            source_cwd: repo_a.clone(),
            target_cwd: None,
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            include_committed: true,
            base_ref: None,
        })
        .await
        .expect_err("must reject primary checkout as source worktree");
        assert!(primary_err.to_string().contains("not_a_worktree"));

        // 3. Reject branch names starting with `-` (flag injection)
        assert!(
            validate_handoff_branch_name(&repo_a, "--exec=evil")
                .await
                .is_err()
        );
        assert!(validate_handoff_branch_name(&repo_a, "-b").await.is_err());

        // 4. Reject case-insensitive `.GIT` paths
        assert!(!is_safe_handoff_rel_path(".GIT/config"));
        assert!(!is_safe_handoff_rel_path("sub/.Git/hooks/pre-commit"));

        // 5. Reject escaping symlink untracked files
        #[cfg(unix)]
        {
            let secret_outside = root_a.join("outside-secret.txt");
            tokio::fs::write(&secret_outside, "SECRET-PAYLOAD\n")
                .await
                .expect("write outside secret");
            std::os::unix::fs::symlink(&secret_outside, wt_a.join("leak.txt"))
                .expect("create escaping symlink");

            // Preview skips the escaping symlink and surfaces a warning
            let preview = preview_worktree_handoff_paths(HandoffPreviewPathsSpec {
                source_cwd: wt_a.clone(),
                target_cwd: Some(repo_a.clone()),
                direction: WorktreeHandoffDirection::WorktreeToLocal,
                include_committed: true,
                base_ref: None,
            })
            .await
            .expect("preview with escaping symlink");
            assert!(
                preview.files.iter().all(|f| f.path != "leak.txt"),
                "preview must exclude escaping symlink"
            );
            assert!(
                preview
                    .warning
                    .as_deref()
                    .is_some_and(|w| w.contains("escaping")),
                "preview should warn about skipped escaping symlink"
            );

            // Execution MUST strictly reject escaping symlink untracked files
            let exec_err = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
                source_cwd: wt_a.clone(),
                target_cwd: repo_a.clone(),
                direction: WorktreeHandoffDirection::WorktreeToLocal,
                mode: WorktreeHandoffMode::Apply,
                include_committed: true,
                base_ref: None,
                allow_three_way_conflicts: false,
                target_workspace: None,
                backup_source_as_primary: false,
            })
            .await
            .expect_err("execute must reject escaping symlink");
            assert!(exec_err.to_string().contains("escaping_symlink"));
            assert!(!repo_a.join("leak.txt").exists());
        }

        let _ = tokio::fs::remove_dir_all(&root_a).await;
        let _ = tokio::fs::remove_dir_all(&root_b).await;
    }

    #[tokio::test]
    async fn regression_git_diff_isolated_from_user_diff_noprefix_and_color_config() {
        let (root, main_dir) = init_test_repo("cfg_isolation").await;
        // Enable hostile user gitconfig options that break standard `git apply` if not isolated
        git_ok(&main_dir, &["config", "diff.noprefix", "true"]).await;
        git_ok(&main_dir, &["config", "color.ui", "always"]).await;
        git_ok(&main_dir, &["config", "core.quotePath", "true"]).await;

        let wt_dir = root.join("wt-cfg");
        add_linked_worktree(&main_dir, &wt_dir, "feat/cfg-iso").await;
        tokio::fs::write(wt_dir.join("app.txt"), "line1\nisolated-edit\nline3\n")
            .await
            .expect("edit wt");

        let preview = preview_worktree_handoff_paths(HandoffPreviewPathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: Some(main_dir.clone()),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            include_committed: true,
            base_ref: None,
        })
        .await
        .expect("preview with diff.noprefix=true");
        assert!(preview.can_apply_cleanly);
        assert_eq!(preview.files.len(), 1);

        let outcome = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: main_dir.clone(),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            mode: WorktreeHandoffMode::Apply,
            include_committed: true,
            base_ref: None,
            allow_three_way_conflicts: false,
            target_workspace: None,
            backup_source_as_primary: false,
        })
        .await
        .expect("execute with diff.noprefix=true");
        assert_eq!(outcome.applied_files, 1);
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("app.txt"))
                .await
                .expect("read main"),
            "line1\nisolated-edit\nline3\n"
        );

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn regression_rejects_tracked_escaping_symlink_120000() {
        let (root, main_dir) = init_test_repo("tracked_symlink").await;
        let wt_dir = root.join("wt-sym");
        add_linked_worktree(&main_dir, &wt_dir, "feat/sym").await;

        let outside_secret = root.join("host-secret.txt");
        tokio::fs::write(&outside_secret, "TOP-SECRET\n")
            .await
            .expect("write secret");
        std::os::unix::fs::symlink(&outside_secret, wt_dir.join("evil_link"))
            .expect("create symlink");
        git_ok(&wt_dir, &["add", "evil_link"]).await;
        git_ok(&wt_dir, &["commit", "-q", "-m", "track escaping symlink"]).await;

        let preview = preview_worktree_handoff_paths(HandoffPreviewPathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: Some(main_dir.clone()),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            include_committed: true,
            base_ref: None,
        })
        .await
        .expect("preview tracked escaping symlink");
        assert!(
            !preview.can_apply_cleanly,
            "preview must block tracked escaping symlink"
        );
        assert!(
            preview
                .warning
                .as_deref()
                .is_some_and(|w| w.contains("tracked symlink"))
        );

        let err = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: main_dir.clone(),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            mode: WorktreeHandoffMode::Apply,
            include_committed: true,
            base_ref: None,
            allow_three_way_conflicts: false,
            target_workspace: None,
            backup_source_as_primary: false,
        })
        .await
        .expect_err("must reject tracked escaping symlink");
        assert!(err.to_string().contains("escaping_symlink"));
        assert!(!main_dir.join("evil_link").exists());

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn regression_move_handles_git_rm_cached_git_mv_and_intent_to_add() {
        let (root, main_dir) = init_test_repo("move_edge_cases").await;
        let wt_dir = root.join("wt-edge");
        add_linked_worktree(&main_dir, &wt_dir, "feat/edge-move").await;

        // 1. `git rm --cached keep.txt` (untracked in index, still exists in HEAD & working tree)
        git_ok(&main_dir, &["rm", "--cached", "-q", "keep.txt"]).await;
        // 2. `git mv app.txt app_renamed.txt`
        git_ok(&main_dir, &["mv", "app.txt", "app_renamed.txt"]).await;
        // 3. `git add -N intent.txt` (intent-to-add new file)
        tokio::fs::write(main_dir.join("intent.txt"), "intent-to-add-body\n")
            .await
            .expect("write intent");
        git_ok(&main_dir, &["add", "-N", "intent.txt"]).await;

        let outcome = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
            source_cwd: main_dir.clone(),
            target_cwd: wt_dir.clone(),
            direction: WorktreeHandoffDirection::LocalToWorktree,
            mode: WorktreeHandoffMode::Move,
            include_committed: false,
            base_ref: None,
            allow_three_way_conflicts: false,
            target_workspace: None,
            backup_source_as_primary: true,
        })
        .await
        .expect("move with rm --cached, mv, add -N");
        assert!(outcome.applied_files >= 2);

        // Source (`main_dir`) MUST be 100% clean AND still contain `keep.txt` and `app.txt` from HEAD!
        assert!(!is_checkout_dirty(&main_dir).await);
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("keep.txt"))
                .await
                .expect("keep.txt must NOT be deleted after Move"),
            "unchanged\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("app.txt"))
                .await
                .expect("app.txt restored"),
            "line1\nline2\nline3\n"
        );
        assert!(
            !main_dir.join("app_renamed.txt").exists(),
            "renamed destination must be cleaned from source"
        );
        assert!(
            !main_dir.join("intent.txt").exists(),
            "intent-to-add file must be cleaned from source"
        );

        // Target (`wt_dir`) received renamed file & intent-to-add file
        assert!(wt_dir.join("app_renamed.txt").exists());
        assert_eq!(
            tokio::fs::read_to_string(wt_dir.join("intent.txt"))
                .await
                .expect("wt intent"),
            "intent-to-add-body\n"
        );

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn regression_nextjs_bracket_path_conflict_and_untracked_three_way_no_clobber() {
        let (root, main_dir) = init_test_repo("nextjs_brackets").await;
        tokio::fs::create_dir_all(main_dir.join("app/users/[id]"))
            .await
            .expect("mkdir nextjs");
        tokio::fs::write(
            main_dir.join("app/users/[id]/page.tsx"),
            "export default function Page() {\n  return <div>v1</div>;\n}\n",
        )
        .await
        .expect("write page.tsx");
        git_ok(&main_dir, &["add", "-A"]).await;
        git_ok(&main_dir, &["commit", "-q", "-m", "add nextjs route"]).await;

        let wt_dir = root.join("wt-next");
        add_linked_worktree(&main_dir, &wt_dir, "feat/next").await;

        // Conflicting edit on Next.js `[id]` route path
        tokio::fs::write(
            wt_dir.join("app/users/[id]/page.tsx"),
            "export default function Page() {\n  return <div>wt</div>;\n}\n",
        )
        .await
        .expect("edit wt page");
        tokio::fs::write(
            main_dir.join("app/users/[id]/page.tsx"),
            "export default function Page() {\n  return <div>local</div>;\n}\n",
        )
        .await
        .expect("edit local page");

        // Also create colliding differing untracked file
        tokio::fs::write(wt_dir.join("notes.txt"), "from-worktree\n")
            .await
            .expect("wt notes");
        tokio::fs::write(main_dir.join("notes.txt"), "important-local-untracked\n")
            .await
            .expect("local notes");

        let preview = preview_worktree_handoff_paths(HandoffPreviewPathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: Some(main_dir.clone()),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            include_committed: true,
            base_ref: None,
        })
        .await
        .expect("preview nextjs conflict");
        assert!(!preview.can_apply_cleanly);
        assert!(
            preview
                .conflicts
                .contains(&"app/users/[id]/page.tsx".to_string()),
            "must detect bracketed path conflict: {:?}",
            preview.conflicts
        );
        assert!(preview.conflicts.contains(&"notes.txt".to_string()));

        // Even with `allow_three_way_conflicts: true`, colliding untracked file MUST be rejected
        // and NEVER silently clobbered!
        let err = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
            source_cwd: wt_dir.clone(),
            target_cwd: main_dir.clone(),
            direction: WorktreeHandoffDirection::WorktreeToLocal,
            mode: WorktreeHandoffMode::Apply,
            include_committed: true,
            base_ref: None,
            allow_three_way_conflicts: true,
            target_workspace: None,
            backup_source_as_primary: false,
        })
        .await
        .expect_err("must never clobber colliding untracked file under 3-way");
        assert!(err.to_string().contains("handoff_conflict"));
        assert_eq!(
            tokio::fs::read_to_string(main_dir.join("notes.txt"))
                .await
                .expect("read local notes intact"),
            "important-local-untracked\n"
        );
        // Verify glob escaper directly
        assert_eq!(
            escape_git_apply_glob("app/users/[id]/a*b?c\\d.tsx"),
            "app/users/\\[id\\]/a\\*b\\?c\\\\d.tsx"
        );

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[test]
    fn parse_name_status_and_numstat_handle_renames_and_binary() {
        let ns = b"M\0src/lib.rs\0R100\0old name.txt\0new name.txt\0A\0bin.dat\0";
        let parsed_ns = parse_name_status_z(ns);
        assert_eq!(
            parsed_ns,
            vec![
                ("src/lib.rs".to_string(), "M".to_string(), None),
                (
                    "new name.txt".to_string(),
                    "R".to_string(),
                    Some("old name.txt".to_string())
                ),
                ("bin.dat".to_string(), "A".to_string(), None),
            ]
        );

        let numstat = b"12\t3\tsrc/lib.rs\x005\t2\t\0old name.txt\0new name.txt\0-\t-\tbin.dat\0";
        let parsed_stats = parse_numstat_z(numstat);
        assert_eq!(parsed_stats.get("src/lib.rs"), Some(&(12, 3)));
        assert_eq!(parsed_stats.get("new name.txt"), Some(&(5, 2)));
        assert_eq!(parsed_stats.get("bin.dat"), Some(&(0, 0)));
    }
}
