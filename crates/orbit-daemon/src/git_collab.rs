//! Deep Git Collaboration Loop (B3-04) for Orbit Daemon:
//! - Git Stash lifecycle (`list`, `push`, `apply`, `pop`, `drop`) with strict `^stash@\{\d+\}$` validation
//! - Git Rebase orchestration (`status`, `start`, `continue`, `abort`, `skip`) with non-interactive editor guards
//! - 3-Way Merge/Rebase Conflict inspector (`diff3`/`zdiff3` + 2-way markers) & atomic block/file resolver
//! - GitHub PR CI Checks & Review Comments inspection via `gh` (graceful `unavailable` / `no_pull_request` fallback)
//! - Fast, deterministic pure-Rust Conventional Commit message synthesis (< 50ms, zero external failures)
//!
//! Security guarantees (matching `git_write.rs`):
//! - Zero shell invocation (`Command::new("git")` / `Command::new("gh")` with explicit args only).
//! - `GIT_LITERAL_PATHSPECS=1` + `GIT_TERMINAL_PROMPT=0` + non-interactive `GIT_EDITOR=true`.
//! - Strict repository boundary, relative path, symlink-escape, stash-ref, and revision validation.
//! - Atomic conflict resolution writes strictly inside verified parent directories (never `remove_dir_all`).
//! - Bounded timeouts on every spawned process; zero `.unwrap()` in production paths.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use orbit_protocol::{
    GitCiCheckState, GitConflictBlock, GitConflictFile, GitConflictListResult,
    GitConflictResolution, GitGeneratedCommitMessage, GitPrAvailability, GitPrCheckItem,
    GitPrReviewComment, GitPrStatus, GitRebaseActionResult, GitRebaseKind, GitRebaseStatus,
    GitResolveConflictResult, GitStashEntry, GitStashListResult, GitStashMutationResult,
    is_valid_github_repo,
};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use uuid::Uuid;

use crate::git_write::{
    GitRepoContext, GitWriteError, LOCAL_GIT_TIMEOUT, MAX_OUTPUT_BYTES, MAX_REF_NAME_BYTES,
    drive_child_with_stdin_timeout, format_process_failure, parse_nul_delimited_paths,
    resolve_git_repo_context, run_git_output, truncate_utf8, validate_git_rel_path,
    verify_no_symlink_escape,
};
use crate::workspace_files::sha256_hex;

const GH_QUERY_TIMEOUT: Duration = Duration::from_secs(15);
const GH_GRAPHQL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_STASH_MESSAGE_BYTES: usize = 512;
const MAX_CONFLICT_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_MANUAL_CONTENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_COMMIT_DIFF_ANALYSIS_BYTES: usize = 256 * 1024;
const MAX_COMMIT_SUBJECT_CHARS: usize = 72;

static WORKTREE_MUTATION_LOCKS: LazyLock<
    std::sync::Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
> = LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// Acquire a per-worktree async mutex keyed by canonical repository root so concurrent
/// stash/rebase/conflict mutations on the same repository serialize cleanly.
fn worktree_mutation_lock(repo_root: &Path) -> Arc<tokio::sync::Mutex<()>> {
    let mut map = WORKTREE_MUTATION_LOCKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if map.len() > 256 {
        map.retain(|_, lock| Arc::strong_count(lock) > 1);
    }
    Arc::clone(
        map.entry(repo_root.to_path_buf())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

// ============================================================================
// Validation Helpers
// ============================================================================

/// Strictly validate a Git stash reference matching `^stash@\{\d+\}$`.
pub fn validate_stash_ref(raw: &str) -> Result<(String, u32), GitWriteError> {
    if raw.is_empty() || raw.len() > 24 || raw.chars().any(|c| c == '\0' || c.is_control()) {
        return Err(GitWriteError::InvalidInput(format!(
            "invalid stash ref `{raw}`: expected `stash@{{N}}`"
        )));
    }
    let Some(inner) = raw
        .strip_prefix("stash@{")
        .and_then(|s| s.strip_suffix('}'))
    else {
        return Err(GitWriteError::InvalidInput(format!(
            "invalid stash ref `{raw}`: expected `stash@{{N}}`"
        )));
    };
    if inner.is_empty() || inner.len() > 8 || !inner.bytes().all(|b| b.is_ascii_digit()) {
        return Err(GitWriteError::InvalidInput(format!(
            "invalid stash ref `{raw}`: index must be a non-negative integer"
        )));
    }
    let index = inner
        .parse::<u32>()
        .map_err(|_| GitWriteError::InvalidInput(format!("invalid stash index in `{raw}`")))?;
    Ok((raw.to_string(), index))
}

/// Validate and resolve a rebase target revision (`onto_ref`) to its pinned full commit OID via
/// `git rev-parse --verify -q --end-of-options <rev>^{commit}` (zero shell invocation; no brittle
/// character blacklist).
pub async fn validate_rebase_onto_ref(
    repo_root: &Path,
    raw: &str,
) -> Result<String, GitWriteError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(GitWriteError::InvalidInput(
            "rebase target ref cannot be empty".to_string(),
        ));
    }
    if trimmed.len() > MAX_REF_NAME_BYTES {
        return Err(GitWriteError::InvalidInput(
            "rebase target ref exceeds maximum length".to_string(),
        ));
    }
    if trimmed
        .chars()
        .any(|c| c == '\0' || c.is_control() || c.is_whitespace())
    {
        return Err(GitWriteError::InvalidInput(
            "rebase target ref contains invalid whitespace or control characters".to_string(),
        ));
    }

    let commit_spec = format!("{trimmed}^{{commit}}");
    let out = run_git_output(
        repo_root,
        &[
            "rev-parse",
            "--verify",
            "-q",
            "--end-of-options",
            &commit_spec,
        ],
        LOCAL_GIT_TIMEOUT,
        "rev-parse --verify",
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::InvalidInput(format!(
            "rebase target `{trimmed}` does not resolve to a valid commit"
        )));
    }

    let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if oid.len() < 40 || oid.contains('\n') || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(GitWriteError::InvalidInput(format!(
            "rebase target `{trimmed}` did not resolve to a single commit OID"
        )));
    }

    Ok(oid)
}

/// Run a Git command that takes NO caller-supplied pathspecs (e.g. `git stash push -u`) with
/// `GIT_LITERAL_PATHSPECS` unset, because Git's internal `stash --include-untracked` relies on
/// internal pathspec handling that breaks silently when `GIT_LITERAL_PATHSPECS=1` is inherited.
async fn run_git_no_pathspec(
    dir: &Path,
    args: &[&str],
    timeout: Duration,
    label: &str,
) -> Result<std::process::Output, GitWriteError> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_LITERAL_PATHSPECS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn()?;
    drive_child_with_stdin_timeout(child, None, timeout, &format!("git {label}")).await
}

async fn run_git_noninteractive(
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
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .env("EDITOR", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn()?;
    drive_child_with_stdin_timeout(child, None, timeout, &format!("git {label}")).await
}

async fn list_conflicted_paths(repo_root: &Path) -> Result<Vec<String>, GitWriteError> {
    let out = run_git_output(
        repo_root,
        &["diff", "--name-only", "--diff-filter=U", "-z"],
        LOCAL_GIT_TIMEOUT,
        "diff --diff-filter=U",
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "diff --diff-filter=U".to_string(),
            stderr: format_process_failure(&out),
        });
    }
    let mut paths = parse_nul_delimited_paths(&out.stdout);
    paths.sort();
    paths.dedup();
    Ok(paths)
}

async fn resolve_git_dir(repo_root: &Path) -> Result<PathBuf, GitWriteError> {
    let out = run_git_output(
        repo_root,
        &["rev-parse", "--git-dir"],
        LOCAL_GIT_TIMEOUT,
        "rev-parse --git-dir",
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "rev-parse --git-dir".to_string(),
            stderr: format_process_failure(&out),
        });
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if raw.is_empty() {
        return Err(GitWriteError::NotAGitRepo(
            "could not determine `.git` directory".to_string(),
        ));
    }
    let p = Path::new(&raw);
    if p.is_absolute() {
        Ok(p.to_path_buf())
    } else {
        Ok(repo_root.join(p))
    }
}

// ============================================================================
// 1. Git Stash
// ============================================================================

fn parse_stash_branch_and_message(raw_subject: &str) -> (Option<String>, String) {
    let trimmed = raw_subject.trim();
    for prefix in ["WIP on ", "On "] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && let Some((branch, msg)) = rest.split_once(": ")
        {
            let b = branch.trim();
            let m = msg.trim();
            if !b.is_empty() {
                return (
                    Some(b.to_string()),
                    if m.is_empty() {
                        trimmed.to_string()
                    } else {
                        m.to_string()
                    },
                );
            }
        }
    }
    (None, trimmed.to_string())
}

/// Verify that `stash_ref` currently resolves to `expected_oid` before mutating the stash stack.
async fn verify_stash_expected_oid(
    repo_root: &Path,
    validated_ref: &str,
    expected_oid: Option<&str>,
) -> Result<(), GitWriteError> {
    let Some(raw_expected) = expected_oid.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    if raw_expected.len() < 4
        || raw_expected.len() > 64
        || !raw_expected.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(GitWriteError::InvalidInput(format!(
            "invalid expected stash OID `{raw_expected}`"
        )));
    }

    let commit_spec = format!("{validated_ref}^{{commit}}");
    let out = run_git_output(
        repo_root,
        &[
            "rev-parse",
            "--verify",
            "-q",
            "--end-of-options",
            &commit_spec,
        ],
        LOCAL_GIT_TIMEOUT,
        "rev-parse stash oid",
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::InvalidInput(format!(
            "stash entry `{validated_ref}` no longer exists; refresh stashes and retry"
        )));
    }
    let live_oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !live_oid.eq_ignore_ascii_case(raw_expected) {
        return Err(GitWriteError::InvalidInput(format!(
            "stash stack changed (`{validated_ref}` now points to a different commit); refresh stashes and retry"
        )));
    }
    Ok(())
}

/// List saved Git stashes in the repository containing `cwd`.
pub async fn git_stash_list(cwd: &Path) -> Result<GitStashListResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let out = run_git_output(
        &ctx.repo_root,
        &["stash", "list", "--format=%gd%x1f%H%x1f%h%x1f%cI%x1f%gs"],
        LOCAL_GIT_TIMEOUT,
        "stash list",
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "stash list".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut entries = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(5, '\x1f');
        let Some(raw_ref) = parts.next() else {
            continue;
        };
        let Ok((stash_ref, index)) = validate_stash_ref(raw_ref.trim()) else {
            continue;
        };
        let oid = parts
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let short_sha = parts
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let created_at = parts
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let raw_subject = parts.next().unwrap_or("");
        let (branch, message) = parse_stash_branch_and_message(raw_subject);
        entries.push(GitStashEntry {
            stash_ref,
            index,
            branch,
            message,
            oid,
            short_sha,
            created_at,
        });
    }

    Ok(GitStashListResult { entries })
}

/// Push current working-tree changes onto the Git stash stack.
pub async fn git_stash_push(
    cwd: &Path,
    message: Option<&str>,
    include_untracked: bool,
) -> Result<GitStashMutationResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let lock = worktree_mutation_lock(&ctx.repo_root);
    let _guard = lock.lock().await;

    let clean_msg = match message.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => {
            if m.len() > MAX_STASH_MESSAGE_BYTES {
                return Err(GitWriteError::InvalidInput(format!(
                    "stash message exceeds maximum length ({MAX_STASH_MESSAGE_BYTES} bytes)"
                )));
            }
            if m.chars()
                .any(|c| c == '\0' || c == '\r' || c == '\n' || c.is_control())
            {
                return Err(GitWriteError::InvalidInput(
                    "stash message must be a single line without control characters".to_string(),
                ));
            }
            Some(m.to_string())
        }
        None => None,
    };

    let mut owned_args: Vec<String> = vec!["stash".into(), "push".into()];
    if include_untracked {
        owned_args.push("--include-untracked".into());
    }
    if let Some(ref msg) = clean_msg {
        owned_args.push(format!("--message={msg}"));
    }
    let arg_refs: Vec<&str> = owned_args.iter().map(String::as_str).collect();

    let out =
        run_git_no_pathspec(&ctx.repo_root, &arg_refs, LOCAL_GIT_TIMEOUT, "stash push").await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "stash push".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if stdout.contains("No local changes to save") {
        return Err(GitWriteError::InvalidInput(
            "No local changes to stash".to_string(),
        ));
    }

    Ok(GitStashMutationResult {
        action: "push".to_string(),
        stash_ref: Some("stash@{0}".to_string()),
        summary: if stdout.is_empty() {
            "Saved working tree changes to stash@{0}".to_string()
        } else {
            truncate_utf8(&stdout, MAX_OUTPUT_BYTES)
        },
        had_conflicts: false,
    })
}

/// Apply a validated stash entry (`^stash@\{\d+\}$`) without dropping it, optionally verifying
/// that `stash_ref` still points to `expected_oid` under the per-worktree mutation lock.
pub async fn git_stash_apply(
    cwd: &Path,
    stash_ref: &str,
    expected_oid: Option<&str>,
) -> Result<GitStashMutationResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let (validated_ref, _) = validate_stash_ref(stash_ref)?;
    let lock = worktree_mutation_lock(&ctx.repo_root);
    let _guard = lock.lock().await;
    verify_stash_expected_oid(&ctx.repo_root, &validated_ref, expected_oid).await?;

    let out = run_git_no_pathspec(
        &ctx.repo_root,
        &["stash", "apply", &validated_ref],
        LOCAL_GIT_TIMEOUT,
        "stash apply",
    )
    .await?;

    if !out.status.success() {
        let conflicts = list_conflicted_paths(&ctx.repo_root)
            .await
            .unwrap_or_default();
        if !conflicts.is_empty() {
            return Ok(GitStashMutationResult {
                action: "apply".to_string(),
                stash_ref: Some(validated_ref),
                summary: format!(
                    "Applied stash with conflicts in {} file(s)",
                    conflicts.len()
                ),
                had_conflicts: true,
            });
        }
        return Err(GitWriteError::GitFailed {
            command: "stash apply".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    Ok(GitStashMutationResult {
        action: "apply".to_string(),
        stash_ref: Some(validated_ref.clone()),
        summary: format!("Applied {validated_ref}"),
        had_conflicts: false,
    })
}

/// Pop (apply & drop) a validated stash entry (`^stash@\{\d+\}$`), optionally verifying
/// `expected_oid` under the per-worktree mutation lock.
pub async fn git_stash_pop(
    cwd: &Path,
    stash_ref: &str,
    expected_oid: Option<&str>,
) -> Result<GitStashMutationResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let (validated_ref, _) = validate_stash_ref(stash_ref)?;
    let lock = worktree_mutation_lock(&ctx.repo_root);
    let _guard = lock.lock().await;
    verify_stash_expected_oid(&ctx.repo_root, &validated_ref, expected_oid).await?;

    let out = run_git_no_pathspec(
        &ctx.repo_root,
        &["stash", "pop", &validated_ref],
        LOCAL_GIT_TIMEOUT,
        "stash pop",
    )
    .await?;

    if !out.status.success() {
        let conflicts = list_conflicted_paths(&ctx.repo_root)
            .await
            .unwrap_or_default();
        if !conflicts.is_empty() {
            return Ok(GitStashMutationResult {
                action: "pop".to_string(),
                stash_ref: Some(validated_ref),
                summary: format!(
                    "Popped stash with conflicts in {} file(s); stash entry retained by Git",
                    conflicts.len()
                ),
                had_conflicts: true,
            });
        }
        return Err(GitWriteError::GitFailed {
            command: "stash pop".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    Ok(GitStashMutationResult {
        action: "pop".to_string(),
        stash_ref: Some(validated_ref.clone()),
        summary: format!("Popped {validated_ref}"),
        had_conflicts: false,
    })
}

/// Drop a validated stash entry (`^stash@\{\d+\}$`), optionally verifying `expected_oid` under the
/// per-worktree mutation lock so index shifts cannot delete the wrong snapshot.
pub async fn git_stash_drop(
    cwd: &Path,
    stash_ref: &str,
    expected_oid: Option<&str>,
) -> Result<GitStashMutationResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let (validated_ref, _) = validate_stash_ref(stash_ref)?;
    let lock = worktree_mutation_lock(&ctx.repo_root);
    let _guard = lock.lock().await;
    verify_stash_expected_oid(&ctx.repo_root, &validated_ref, expected_oid).await?;

    let out = run_git_no_pathspec(
        &ctx.repo_root,
        &["stash", "drop", &validated_ref],
        LOCAL_GIT_TIMEOUT,
        "stash drop",
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "stash drop".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    Ok(GitStashMutationResult {
        action: "drop".to_string(),
        stash_ref: Some(validated_ref.clone()),
        summary: format!("Dropped {validated_ref}"),
        had_conflicts: false,
    })
}

// ============================================================================
// 2. Git Rebase
// ============================================================================

async fn read_trimmed_file(path: &Path) -> Option<String> {
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

async fn read_first_line(path: &Path) -> Option<String> {
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    raw.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(ToOwned::to_owned)
}

async fn inspect_rebase_status_ctx(ctx: &GitRepoContext) -> Result<GitRebaseStatus, GitWriteError> {
    let git_dir = resolve_git_dir(&ctx.repo_root).await?;
    let rebase_merge = git_dir.join("rebase-merge");
    let rebase_apply = git_dir.join("rebase-apply");

    let conflicted_files = list_conflicted_paths(&ctx.repo_root).await?;
    let merge_in_progress = tokio::fs::try_exists(git_dir.join("MERGE_HEAD"))
        .await
        .unwrap_or(false);
    let cherry_pick_in_progress = tokio::fs::try_exists(git_dir.join("CHERRY_PICK_HEAD"))
        .await
        .unwrap_or(false);

    let (in_progress, kind, state_dir) =
        if tokio::fs::try_exists(&rebase_merge).await.unwrap_or(false) {
            (true, GitRebaseKind::Merge, Some(rebase_merge))
        } else if tokio::fs::try_exists(&rebase_apply).await.unwrap_or(false) {
            (true, GitRebaseKind::Apply, Some(rebase_apply))
        } else {
            (false, GitRebaseKind::None, None)
        };

    let (mut current_step, mut total_steps, mut head_name, mut onto, mut stopped_message) =
        (None, None, None, None, None);

    if let Some(dir) = state_dir {
        match kind {
            GitRebaseKind::Merge => {
                current_step = read_trimmed_file(&dir.join("msgnum"))
                    .await
                    .and_then(|s| s.parse::<u32>().ok());
                total_steps = read_trimmed_file(&dir.join("end"))
                    .await
                    .and_then(|s| s.parse::<u32>().ok());
                stopped_message = read_first_line(&dir.join("message")).await;
            }
            GitRebaseKind::Apply => {
                current_step = read_trimmed_file(&dir.join("next"))
                    .await
                    .and_then(|s| s.parse::<u32>().ok());
                total_steps = read_trimmed_file(&dir.join("last"))
                    .await
                    .and_then(|s| s.parse::<u32>().ok());
                stopped_message = match read_first_line(&dir.join("msg-clean")).await {
                    Some(msg) => Some(msg),
                    None => read_first_line(&dir.join("final-commit")).await,
                };
            }
            GitRebaseKind::None => {}
        }

        head_name = read_trimmed_file(&dir.join("head-name"))
            .await
            .map(|raw| raw.strip_prefix("refs/heads/").unwrap_or(&raw).to_string());
        onto = read_trimmed_file(&dir.join("onto")).await.map(|raw| {
            if raw.len() >= 7 && raw.bytes().all(|b| b.is_ascii_hexdigit()) {
                raw[..7].to_string()
            } else {
                raw
            }
        });
    }

    Ok(GitRebaseStatus {
        in_progress,
        kind,
        current_step,
        total_steps,
        head_name,
        onto,
        stopped_message,
        conflicted_files,
        merge_in_progress,
        cherry_pick_in_progress,
    })
}

/// Inspect active Git rebase / merge / cherry-pick status and unmerged conflicted files.
pub async fn git_rebase_status(cwd: &Path) -> Result<GitRebaseStatus, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    inspect_rebase_status_ctx(&ctx).await
}

/// Start rebasing the current branch onto the pinned commit resolved from `onto_ref`.
pub async fn git_rebase_start(
    cwd: &Path,
    onto_ref: &str,
) -> Result<GitRebaseActionResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let lock = worktree_mutation_lock(&ctx.repo_root);
    let _guard = lock.lock().await;

    let current = inspect_rebase_status_ctx(&ctx).await?;
    if current.in_progress {
        return Err(GitWriteError::InvalidInput(
            "a rebase is already in progress; continue, skip, or abort it first".to_string(),
        ));
    }

    let display_ref = onto_ref.trim();
    let pinned_oid = validate_rebase_onto_ref(&ctx.repo_root, display_ref).await?;
    let out = run_git_noninteractive(
        &ctx.repo_root,
        &["-c", "core.editor=true", "rebase", &pinned_oid],
        LOCAL_GIT_TIMEOUT,
        "rebase",
    )
    .await?;

    let status = inspect_rebase_status_ctx(&ctx).await?;
    if !out.status.success() {
        if status.in_progress {
            return Ok(GitRebaseActionResult {
                action: "start".to_string(),
                summary: format!(
                    "Rebase onto {display_ref} paused with {} conflicted file(s)",
                    status.conflicted_files.len()
                ),
                status,
            });
        }
        return Err(GitWriteError::GitFailed {
            command: "rebase".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    Ok(GitRebaseActionResult {
        action: "start".to_string(),
        summary: format!("Rebased cleanly onto {display_ref}"),
        status,
    })
}

/// Continue an active Git rebase after resolving and staging conflicts.
pub async fn git_rebase_continue(cwd: &Path) -> Result<GitRebaseActionResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let lock = worktree_mutation_lock(&ctx.repo_root);
    let _guard = lock.lock().await;

    let current = inspect_rebase_status_ctx(&ctx).await?;
    if !current.in_progress {
        return Err(GitWriteError::InvalidInput(
            "no rebase is currently in progress".to_string(),
        ));
    }
    if !current.conflicted_files.is_empty() {
        return Err(GitWriteError::InvalidInput(format!(
            "resolve and stage {} remaining conflicted file(s) before continuing rebase",
            current.conflicted_files.len()
        )));
    }

    let out = run_git_noninteractive(
        &ctx.repo_root,
        &["-c", "core.editor=true", "rebase", "--continue"],
        LOCAL_GIT_TIMEOUT,
        "rebase --continue",
    )
    .await?;

    let status = inspect_rebase_status_ctx(&ctx).await?;
    if !out.status.success() {
        if status.in_progress && !status.conflicted_files.is_empty() {
            return Ok(GitRebaseActionResult {
                action: "continue".to_string(),
                summary: format!(
                    "Rebase paused on next commit with {} conflicted file(s)",
                    status.conflicted_files.len()
                ),
                status,
            });
        }
        return Err(GitWriteError::GitFailed {
            command: "rebase --continue".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    let summary = if status.in_progress {
        "Advanced rebase".to_string()
    } else {
        "Rebase completed".to_string()
    };
    Ok(GitRebaseActionResult {
        action: "continue".to_string(),
        status,
        summary,
    })
}

/// Abort an active Git rebase and restore the original branch state.
pub async fn git_rebase_abort(cwd: &Path) -> Result<GitRebaseActionResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let lock = worktree_mutation_lock(&ctx.repo_root);
    let _guard = lock.lock().await;

    let current = inspect_rebase_status_ctx(&ctx).await?;
    if !current.in_progress {
        return Err(GitWriteError::InvalidInput(
            "no rebase is currently in progress".to_string(),
        ));
    }

    let out = run_git_noninteractive(
        &ctx.repo_root,
        &["rebase", "--abort"],
        LOCAL_GIT_TIMEOUT,
        "rebase --abort",
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "rebase --abort".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    let status = inspect_rebase_status_ctx(&ctx).await?;
    Ok(GitRebaseActionResult {
        action: "abort".to_string(),
        status,
        summary: "Aborted rebase".to_string(),
    })
}

/// Skip the current conflicting commit in an active Git rebase, optionally verifying that the
/// active rebase step still matches `expected_step` under the per-worktree mutation lock.
pub async fn git_rebase_skip(
    cwd: &Path,
    expected_step: Option<u32>,
) -> Result<GitRebaseActionResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let lock = worktree_mutation_lock(&ctx.repo_root);
    let _guard = lock.lock().await;

    let current = inspect_rebase_status_ctx(&ctx).await?;
    if !current.in_progress {
        return Err(GitWriteError::InvalidInput(
            "no rebase is currently in progress".to_string(),
        ));
    }
    if let Some(expected) = expected_step
        && current.current_step != Some(expected)
    {
        return Err(GitWriteError::InvalidInput(format!(
            "rebase step changed (expected step {expected}, currently at {}); refresh and retry",
            current
                .current_step
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )));
    }

    let out = run_git_noninteractive(
        &ctx.repo_root,
        &["-c", "core.editor=true", "rebase", "--skip"],
        LOCAL_GIT_TIMEOUT,
        "rebase --skip",
    )
    .await?;

    let status = inspect_rebase_status_ctx(&ctx).await?;
    if !out.status.success() {
        if status.in_progress && !status.conflicted_files.is_empty() {
            return Ok(GitRebaseActionResult {
                action: "skip".to_string(),
                summary: format!(
                    "Skipped commit; next commit has {} conflicted file(s)",
                    status.conflicted_files.len()
                ),
                status,
            });
        }
        return Err(GitWriteError::GitFailed {
            command: "rebase --skip".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    let summary = if status.in_progress {
        "Skipped commit and continued rebase".to_string()
    } else {
        "Skipped commit; rebase completed".to_string()
    };
    Ok(GitRebaseActionResult {
        action: "skip".to_string(),
        status,
        summary,
    })
}

// ============================================================================
// 3. 3-Way Merge/Rebase Conflict Inspector & Resolver
// ============================================================================

#[derive(Debug, Clone)]
struct ParsedConflictSpan {
    /// Zero-based inclusive line index of `<<<<<<<`.
    start_idx: usize,
    /// Zero-based inclusive line index of `>>>>>>>`.
    end_idx: usize,
    ours_label: String,
    base_label: Option<String>,
    theirs_label: String,
    ours_lines: Vec<String>,
    base_lines: Option<Vec<String>>,
    theirs_lines: Vec<String>,
    context_before: Vec<String>,
    context_after: Vec<String>,
}

fn match_conflict_marker(line: &str, ch: u8) -> Option<(usize, String)> {
    let bytes = line.as_bytes();
    let mut len = 0usize;
    while len < bytes.len() && bytes[len] == ch {
        len += 1;
    }
    if len < 7 {
        return None;
    }
    if len == bytes.len() {
        return Some((len, String::new()));
    }
    if bytes[len] == b' ' || bytes[len] == b'\t' {
        return Some((len, line[len..].trim().to_string()));
    }
    None
}

fn match_exact_separator(line: &str, ch: u8, expected_len: usize) -> Option<String> {
    let (len, label) = match_conflict_marker(line, ch)?;
    if len == expected_len {
        Some(label)
    } else {
        None
    }
}

fn split_lines_preserving_empty(content: &str) -> (Vec<String>, &'static str, bool) {
    let line_ending = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let had_trailing_newline = content.ends_with('\n');
    if content.is_empty() {
        return (Vec::new(), line_ending, false);
    }
    let stripped = if had_trailing_newline {
        content
            .strip_suffix("\r\n")
            .or_else(|| content.strip_suffix('\n'))
            .unwrap_or(content)
    } else {
        content
    };
    let lines = stripped
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect();
    (lines, line_ending, had_trailing_newline)
}

fn parse_conflict_spans_from_lines(lines: &[String]) -> Vec<ParsedConflictSpan> {
    let mut spans = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let Some((marker_len, ours_label)) = match_conflict_marker(&lines[i], b'<') else {
            i += 1;
            continue;
        };
        let start_idx = i;
        let mut cursor = i + 1;
        let mut ours_lines = Vec::new();
        let mut base_label = None;
        let mut base_lines: Option<Vec<String>> = None;
        let mut theirs_lines = Vec::new();
        let mut theirs_label = None;

        enum Section {
            Ours,
            Base,
            Theirs,
        }
        let mut section = Section::Ours;
        let mut end_idx = None;

        while cursor < lines.len() {
            let line = &lines[cursor];
            match section {
                Section::Ours => {
                    if let Some(label) = match_exact_separator(line, b'|', marker_len) {
                        base_label = Some(label);
                        base_lines = Some(Vec::new());
                        section = Section::Base;
                    } else if let Some(label) = match_exact_separator(line, b'=', marker_len)
                        && label.is_empty()
                    {
                        section = Section::Theirs;
                    } else {
                        ours_lines.push(line.clone());
                    }
                }
                Section::Base => {
                    if let Some(label) = match_exact_separator(line, b'=', marker_len)
                        && label.is_empty()
                    {
                        section = Section::Theirs;
                    } else if let Some(ref mut bl) = base_lines {
                        bl.push(line.clone());
                    }
                }
                Section::Theirs => {
                    if let Some(label) = match_exact_separator(line, b'>', marker_len) {
                        theirs_label = Some(label);
                        end_idx = Some(cursor);
                        break;
                    } else {
                        theirs_lines.push(line.clone());
                    }
                }
            }
            cursor += 1;
        }

        if let Some(closing) = end_idx {
            let ctx_before_start = start_idx.saturating_sub(2);
            let context_before = lines[ctx_before_start..start_idx].to_vec();
            let ctx_after_end = (closing + 1 + 2).min(lines.len());
            let context_after = if closing + 1 < lines.len() {
                lines[closing + 1..ctx_after_end].to_vec()
            } else {
                Vec::new()
            };

            spans.push(ParsedConflictSpan {
                start_idx,
                end_idx: closing,
                ours_label,
                base_label,
                theirs_label: theirs_label.unwrap_or_default(),
                ours_lines,
                base_lines,
                theirs_lines,
                context_before,
                context_after,
            });
            i = closing + 1;
        } else {
            // Unterminated marker; advance past opening line without looping forever.
            i += 1;
        }
    }

    spans
}

/// Parse 2-way and 3-way (`diff3` / `zdiff3`) Git conflict markers from file text.
pub fn parse_conflict_blocks(content: &str) -> Vec<GitConflictBlock> {
    let (lines, _, _) = split_lines_preserving_empty(content);
    parse_conflict_spans_from_lines(&lines)
        .into_iter()
        .enumerate()
        .map(|(idx, span)| GitConflictBlock {
            index: idx as u32,
            start_line: (span.start_idx + 1) as u32,
            end_line: (span.end_idx + 1) as u32,
            ours_label: span.ours_label,
            base_label: span.base_label,
            theirs_label: span.theirs_label,
            ours: span.ours_lines.join("\n"),
            base: span.base_lines.map(|b| b.join("\n")),
            theirs: span.theirs_lines.join("\n"),
            context_before: span.context_before,
            context_after: span.context_after,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default)]
struct UnmergedStages {
    has_base: bool,
    has_ours: bool,
    has_theirs: bool,
}

impl UnmergedStages {
    fn is_unmerged(self) -> bool {
        self.has_base || self.has_ours || self.has_theirs
    }
}

/// Inspect Git index unmerged stages (`1` = base, `2` = ours, `3` = theirs) for `rel_path`.
async fn inspect_unmerged_stages(
    repo_root: &Path,
    rel_path: &str,
) -> Result<UnmergedStages, GitWriteError> {
    let out = run_git_output(
        repo_root,
        &["ls-files", "-u", "-z", "--", rel_path],
        LOCAL_GIT_TIMEOUT,
        "ls-files -u",
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "ls-files -u".to_string(),
            stderr: format_process_failure(&out),
        });
    }

    let mut stages = UnmergedStages::default();
    for record in out.stdout.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let text = String::from_utf8_lossy(record);
        let Some((meta_part, path_part)) = text.split_once('\t') else {
            continue;
        };
        if path_part != rel_path {
            continue;
        }
        let stage = meta_part.split_whitespace().nth(2);
        match stage {
            Some("1") => stages.has_base = true,
            Some("2") => stages.has_ours = true,
            Some("3") => stages.has_theirs = true,
            _ => {}
        }
    }
    Ok(stages)
}

enum BoundedConflictRead {
    Missing,
    Oversized,
    Binary,
    Text { bytes: Vec<u8>, sha256: String },
}

/// Safely read a conflict file with `O_NOFOLLOW`, regular-file + `(dev, ino)` verification, and
/// a strict `MAX_CONFLICT_FILE_BYTES + 1` read cap so oversized/binary conflicts never exhaust RAM.
async fn read_conflict_file_bounded(
    ctx: &GitRepoContext,
    rel_path: &str,
) -> Result<BoundedConflictRead, GitWriteError> {
    let full_path = verify_no_symlink_escape(ctx, rel_path).await?;
    let lmeta_before = match tokio::fs::symlink_metadata(&full_path).await {
        Ok(m) => m,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(BoundedConflictRead::Missing),
        Err(e) => return Err(GitWriteError::Io(e)),
    };
    if lmeta_before.file_type().is_symlink() {
        return Err(GitWriteError::InvalidPath {
            path: rel_path.to_string(),
            reason: "refusing to read conflict through a symbolic link".to_string(),
        });
    }
    if lmeta_before.file_type().is_dir() {
        return Err(GitWriteError::InvalidPath {
            path: rel_path.to_string(),
            reason: "target path is a directory".to_string(),
        });
    }
    if !lmeta_before.is_file() {
        return Err(GitWriteError::InvalidPath {
            path: rel_path.to_string(),
            reason: "target path is not a regular file".to_string(),
        });
    }
    if lmeta_before.len() > MAX_CONFLICT_FILE_BYTES as u64 {
        return Ok(BoundedConflictRead::Oversized);
    }

    let mut opts = tokio::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    if let Some(flag) = crate::git::unix_o_nofollow_flag() {
        opts.custom_flags(flag);
    }
    let file = match opts.open(&full_path).await {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(BoundedConflictRead::Missing),
        Err(e) => return Err(GitWriteError::Io(e)),
    };

    let fmeta = file.metadata().await?;
    if !fmeta.is_file() {
        return Err(GitWriteError::InvalidPath {
            path: rel_path.to_string(),
            reason: "target path is not a regular file".to_string(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let lmeta_after = tokio::fs::symlink_metadata(&full_path).await?;
        if lmeta_after.file_type().is_symlink()
            || fmeta.dev() != lmeta_before.dev()
            || fmeta.ino() != lmeta_before.ino()
            || fmeta.dev() != lmeta_after.dev()
            || fmeta.ino() != lmeta_after.ino()
        {
            return Err(GitWriteError::InvalidPath {
                path: rel_path.to_string(),
                reason: "target file changed during symlink validation".to_string(),
            });
        }
    }

    if fmeta.len() > MAX_CONFLICT_FILE_BYTES as u64 {
        return Ok(BoundedConflictRead::Oversized);
    }

    let mut buf = Vec::with_capacity((fmeta.len() as usize).min(64 * 1024));
    let mut limited = file.take((MAX_CONFLICT_FILE_BYTES + 1) as u64);
    limited.read_to_end(&mut buf).await?;
    if buf.len() > MAX_CONFLICT_FILE_BYTES {
        return Ok(BoundedConflictRead::Oversized);
    }

    let probe_len = buf.len().min(8192);
    if buf[..probe_len].contains(&0) {
        return Ok(BoundedConflictRead::Binary);
    }

    let sha256 = sha256_hex(&buf);
    Ok(BoundedConflictRead::Text { bytes: buf, sha256 })
}

/// List all unmerged conflicted files inside the authorized workspace directory along with
/// parsed 2-way / 3-way conflict blocks and SHA-256 content digests.
pub async fn git_list_conflicts(cwd: &Path) -> Result<GitConflictListResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let unmerged_paths = list_conflicted_paths(&ctx.repo_root).await?;
    let mut files = Vec::new();

    for raw_path in unmerged_paths {
        // Respect monorepo subdirectory boundaries cleanly.
        let Ok(rel_path) = validate_git_rel_path(&ctx, &raw_path) else {
            continue;
        };

        match read_conflict_file_bounded(&ctx, &rel_path).await {
            Ok(BoundedConflictRead::Missing) => {
                // File deleted on one side (delete/modify conflict).
                files.push(GitConflictFile {
                    path: rel_path,
                    blocks: Vec::new(),
                    is_binary: false,
                    unmerged_in_index: true,
                    total_blocks: 0,
                    content_sha256: None,
                });
            }
            Ok(BoundedConflictRead::Oversized | BoundedConflictRead::Binary) => {
                files.push(GitConflictFile {
                    path: rel_path,
                    blocks: Vec::new(),
                    is_binary: true,
                    unmerged_in_index: true,
                    total_blocks: 0,
                    content_sha256: None,
                });
            }
            Ok(BoundedConflictRead::Text { bytes, sha256 }) => {
                let content = String::from_utf8_lossy(&bytes);
                let blocks = parse_conflict_blocks(&content);
                let total_blocks = blocks.len() as u32;
                files.push(GitConflictFile {
                    path: rel_path,
                    blocks,
                    is_binary: false,
                    unmerged_in_index: true,
                    total_blocks,
                    content_sha256: Some(sha256),
                });
            }
            Err(_) => continue,
        }
    }

    Ok(GitConflictListResult { files })
}

fn replacement_lines_for_span(
    span: &ParsedConflictSpan,
    resolution: &GitConflictResolution,
) -> Vec<String> {
    match resolution {
        GitConflictResolution::Ours => span.ours_lines.clone(),
        GitConflictResolution::Theirs => span.theirs_lines.clone(),
        GitConflictResolution::Both => {
            let mut combined = Vec::with_capacity(span.ours_lines.len() + span.theirs_lines.len());
            combined.extend(span.ours_lines.iter().cloned());
            combined.extend(span.theirs_lines.iter().cloned());
            combined
        }
        GitConflictResolution::ManualContent { content } => {
            if content.is_empty() {
                return Vec::new();
            }
            let normalized = content
                .strip_suffix("\r\n")
                .or_else(|| content.strip_suffix('\n'))
                .unwrap_or(content);
            normalized
                .split('\n')
                .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
                .collect()
        }
    }
}

/// Resolve a whole-file `Ours` / `Theirs` conflict directly via Git index stages (supporting
/// modify/delete via `git rm -f` when the chosen side deleted the file, and always failing closed
/// on non-zero exit). Returns `Ok(true)` if `git rm -f` already staged the resolution.
async fn resolve_via_git_index_stage(
    repo_root: &Path,
    rel_path: &str,
    stages: UnmergedStages,
    resolution: &GitConflictResolution,
) -> Result<bool, GitWriteError> {
    let (flag, stage_exists) = match resolution {
        GitConflictResolution::Ours => ("--ours", stages.has_ours),
        GitConflictResolution::Theirs => ("--theirs", stages.has_theirs),
        _ => {
            return Err(GitWriteError::InvalidInput(format!(
                "cannot resolve `{rel_path}` via index stage with this strategy; choose `ours` or `theirs`"
            )));
        }
    };

    if !stage_exists {
        let rm_out = run_git_output(
            repo_root,
            &["rm", "-f", "--", rel_path],
            LOCAL_GIT_TIMEOUT,
            "rm resolved conflict",
        )
        .await?;
        if !rm_out.status.success() {
            return Err(GitWriteError::GitFailed {
                command: "rm -f".to_string(),
                stderr: format_process_failure(&rm_out),
            });
        }
        return Ok(true);
    }

    let out = run_git_output(
        repo_root,
        &["checkout", flag, "--", rel_path],
        LOCAL_GIT_TIMEOUT,
        &format!("checkout {flag}"),
    )
    .await?;
    if !out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: format!("checkout {flag}"),
            stderr: format_process_failure(&out),
        });
    }
    Ok(false)
}

async fn atomic_write_inside_verified_parent(
    ctx: &GitRepoContext,
    rel_path: &str,
    bytes: &[u8],
) -> Result<(), GitWriteError> {
    let target_path = verify_no_symlink_escape(ctx, rel_path).await?;
    let parent = target_path.parent().ok_or_else(|| {
        GitWriteError::InvalidInput("cannot resolve parent directory for conflict file".to_string())
    })?;

    let existing_perms = match tokio::fs::symlink_metadata(&target_path).await {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(GitWriteError::InvalidPath {
                    path: rel_path.to_string(),
                    reason: "refusing to overwrite a symbolic link".to_string(),
                });
            }
            if meta.file_type().is_dir() {
                return Err(GitWriteError::InvalidPath {
                    path: rel_path.to_string(),
                    reason: "target path is a directory".to_string(),
                });
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                // Strip SUID/SGID/sticky bits while preserving regular mode bits.
                Some(std::fs::Permissions::from_mode(
                    meta.permissions().mode() & 0o777,
                ))
            }
            #[cfg(not(unix))]
            {
                Some(meta.permissions())
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => return Err(GitWriteError::Io(e)),
    };

    let tmp_name = format!(".orbit-conflict-{}.tmp", Uuid::new_v4().simple());
    let tmp_path = parent.join(tmp_name);

    let mut opts = tokio::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(flag) = crate::git::unix_o_nofollow_flag() {
        opts.custom_flags(flag);
    }
    let mut file = opts.open(&tmp_path).await?;

    let write_res = async {
        file.write_all(bytes).await?;
        if let Some(perms) = existing_perms {
            file.set_permissions(perms).await?;
        }
        file.flush().await?;
        file.sync_all().await?;
        Ok::<(), std::io::Error>(())
    }
    .await;

    if let Err(err) = write_res {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(GitWriteError::Io(err));
    }
    drop(file);

    // Re-verify parent & target symlink invariants immediately before atomic rename.
    let verified_target = verify_no_symlink_escape(ctx, rel_path).await?;
    if let Ok(meta) = tokio::fs::symlink_metadata(&verified_target).await
        && (meta.file_type().is_symlink() || meta.file_type().is_dir())
    {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(GitWriteError::InvalidPath {
            path: rel_path.to_string(),
            reason: "target path became a symlink or directory before rename".to_string(),
        });
    }

    if let Err(err) = tokio::fs::rename(&tmp_path, &verified_target).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(GitWriteError::Io(err));
    }

    Ok(())
}

/// Resolve one conflict block (`block_index = Some(i)`) or the entire file (`block_index = None`)
/// using `Ours`, `Theirs`, `Both`, or `ManualContent`, under the per-worktree mutation lock with
/// unmerged-index & optional `expected_sha256` optimistic concurrency verification.
pub async fn git_resolve_conflict(
    cwd: &Path,
    raw_path: &str,
    block_index: Option<u32>,
    resolution: &GitConflictResolution,
    auto_stage: bool,
    expected_sha256: Option<&str>,
) -> Result<GitResolveConflictResult, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let rel_path = validate_git_rel_path(&ctx, raw_path)?;

    if let GitConflictResolution::ManualContent { content } = resolution {
        if content.len() > MAX_MANUAL_CONTENT_BYTES {
            return Err(GitWriteError::InvalidInput(format!(
                "manual resolution content exceeds maximum size ({MAX_MANUAL_CONTENT_BYTES} bytes)"
            )));
        }
        if content.contains('\0') {
            return Err(GitWriteError::InvalidInput(
                "manual resolution content must not contain NUL bytes".to_string(),
            ));
        }
    }

    let lock = worktree_mutation_lock(&ctx.repo_root);
    let _guard = lock.lock().await;

    // Verify target path actually has an unmerged conflict in the Git index before touching disk.
    let stages = inspect_unmerged_stages(&ctx.repo_root, &rel_path).await?;
    if !stages.is_unmerged() {
        return Err(GitWriteError::InvalidInput(format!(
            "file `{rel_path}` is not an unmerged conflict in the Git index; refresh conflicts"
        )));
    }

    // Read bounded current state (never loads > 2 MiB binary files into RAM) and verify expected_sha256.
    let current_read = read_conflict_file_bounded(&ctx, &rel_path).await?;
    if let Some(expected) = expected_sha256.map(str::trim).filter(|s| !s.is_empty()) {
        let matches = matches!(
            &current_read,
            BoundedConflictRead::Text { sha256, .. } if sha256.eq_ignore_ascii_case(expected)
        );
        if !matches {
            return Err(GitWriteError::InvalidInput(format!(
                "conflict file `{rel_path}` changed on disk since it was loaded; refresh and retry"
            )));
        }
    }

    let remaining_blocks: u32;
    let mut already_staged_by_rm = false;

    if block_index.is_none()
        && let GitConflictResolution::ManualContent { content } = resolution
    {
        atomic_write_inside_verified_parent(&ctx, &rel_path, content.as_bytes()).await?;
        remaining_blocks = parse_conflict_blocks(content).len() as u32;
    } else {
        match current_read {
            BoundedConflictRead::Missing => {
                if block_index.is_some() {
                    return Err(GitWriteError::InvalidInput(format!(
                        "file `{rel_path}` does not exist on disk; resolve whole file instead"
                    )));
                }
                already_staged_by_rm =
                    resolve_via_git_index_stage(&ctx.repo_root, &rel_path, stages, resolution)
                        .await?;
                remaining_blocks = 0;
            }
            BoundedConflictRead::Oversized | BoundedConflictRead::Binary => {
                if block_index.is_some() {
                    return Err(GitWriteError::InvalidInput(
                        "block-level conflict resolution is not supported for binary or oversized files"
                            .to_string(),
                    ));
                }
                already_staged_by_rm =
                    resolve_via_git_index_stage(&ctx.repo_root, &rel_path, stages, resolution)
                        .await?;
                remaining_blocks = 0;
            }
            BoundedConflictRead::Text { bytes, .. } => {
                let raw_text = String::from_utf8(bytes).map_err(|_| {
                    GitWriteError::InvalidInput(format!(
                        "file `{rel_path}` is not valid UTF-8 for inline conflict resolution"
                    ))
                })?;
                let (lines, line_ending, had_trailing_newline) =
                    split_lines_preserving_empty(&raw_text);
                let spans = parse_conflict_spans_from_lines(&lines);

                if spans.is_empty() {
                    if block_index.is_some() {
                        return Err(GitWriteError::InvalidInput(format!(
                            "no conflict markers found in `{rel_path}`"
                        )));
                    }
                    match resolution {
                        GitConflictResolution::Ours | GitConflictResolution::Theirs => {
                            already_staged_by_rm = resolve_via_git_index_stage(
                                &ctx.repo_root,
                                &rel_path,
                                stages,
                                resolution,
                            )
                            .await?;
                        }
                        GitConflictResolution::Both
                        | GitConflictResolution::ManualContent { .. } => {
                            // Keep current disk contents cleanly so auto_stage can mark resolved.
                        }
                    }
                    remaining_blocks = 0;
                } else {
                    if let Some(idx) = block_index
                        && (idx as usize) >= spans.len()
                    {
                        return Err(GitWriteError::InvalidInput(format!(
                            "conflict block index {idx} out of range ({} blocks in `{rel_path}`)",
                            spans.len()
                        )));
                    }

                    let mut out_lines = Vec::with_capacity(lines.len());
                    let mut cursor = 0usize;
                    for (span_idx, span) in spans.iter().enumerate() {
                        let should_replace = match block_index {
                            Some(target_idx) => span_idx == target_idx as usize,
                            None => true,
                        };
                        if !should_replace {
                            continue;
                        }
                        out_lines.extend_from_slice(&lines[cursor..span.start_idx]);
                        out_lines.extend(replacement_lines_for_span(span, resolution));
                        cursor = span.end_idx + 1;
                    }
                    if cursor < lines.len() {
                        out_lines.extend_from_slice(&lines[cursor..]);
                    }

                    let mut rendered = out_lines.join(line_ending);
                    if had_trailing_newline && !out_lines.is_empty() {
                        rendered.push_str(line_ending);
                    }

                    atomic_write_inside_verified_parent(&ctx, &rel_path, rendered.as_bytes())
                        .await?;
                    remaining_blocks = parse_conflict_blocks(&rendered).len() as u32;
                }
            }
        }
    }

    let mut staged = already_staged_by_rm;
    if !staged && auto_stage && remaining_blocks == 0 {
        let add_out = run_git_output(
            &ctx.repo_root,
            &["add", "--", &rel_path],
            LOCAL_GIT_TIMEOUT,
            "add resolved conflict",
        )
        .await?;
        if !add_out.status.success() {
            return Err(GitWriteError::GitFailed {
                command: "add".to_string(),
                stderr: format_process_failure(&add_out),
            });
        }
        staged = true;
    }

    let summary = if remaining_blocks == 0 && staged {
        format!("Resolved and staged {rel_path}")
    } else if remaining_blocks == 0 {
        format!("Resolved all conflicts in {rel_path}")
    } else {
        format!("Resolved block in {rel_path} ({remaining_blocks} remaining)")
    };

    Ok(GitResolveConflictResult {
        path: rel_path,
        remaining_blocks,
        staged,
        summary,
    })
}

// ============================================================================
// 4. PR Status, CI Checks & Review Comments (`gh`)
// ============================================================================

fn normalize_ci_check_state(
    status: Option<&str>,
    conclusion: Option<&str>,
    legacy_state: Option<&str>,
) -> GitCiCheckState {
    if let Some(c) = conclusion
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_uppercase)
    {
        return match c.as_str() {
            "SUCCESS" => GitCiCheckState::Passing,
            "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
            | "STALE" => GitCiCheckState::Failing,
            "NEUTRAL" | "SKIPPED" => GitCiCheckState::Neutral,
            _ => GitCiCheckState::Pending,
        };
    }

    if let Some(s) = status
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_uppercase)
    {
        return match s.as_str() {
            "QUEUED" | "IN_PROGRESS" | "PENDING" | "WAITING" | "REQUESTED" => {
                GitCiCheckState::Pending
            }
            "COMPLETED" => GitCiCheckState::Neutral,
            _ => GitCiCheckState::Pending,
        };
    }

    if let Some(ls) = legacy_state
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_uppercase)
    {
        return match ls.as_str() {
            "SUCCESS" => GitCiCheckState::Passing,
            "FAILURE" | "ERROR" => GitCiCheckState::Failing,
            "PENDING" | "EXPECTED" => GitCiCheckState::Pending,
            _ => GitCiCheckState::Neutral,
        };
    }

    GitCiCheckState::Neutral
}

fn parse_pr_checks(rollup: Option<&Value>) -> Vec<GitPrCheckItem> {
    let Some(arr) = rollup.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut checks = Vec::new();
    for item in arr {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| item.get("context").and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(name) = name else {
            continue;
        };
        let raw_status = item
            .get("status")
            .and_then(Value::as_str)
            .or_else(|| item.get("state").and_then(Value::as_str))
            .map(ToOwned::to_owned);
        let raw_conclusion = item
            .get("conclusion")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let legacy_state = item.get("state").and_then(Value::as_str);
        let state = normalize_ci_check_state(
            item.get("status").and_then(Value::as_str),
            raw_conclusion.as_deref(),
            legacy_state,
        );
        let workflow = item
            .get("workflowName")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let details_url = item
            .get("detailsUrl")
            .and_then(Value::as_str)
            .or_else(|| item.get("targetUrl").and_then(Value::as_str))
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        checks.push(GitPrCheckItem {
            name: name.to_string(),
            state,
            workflow,
            raw_status,
            raw_conclusion,
            details_url,
        });
    }
    checks
}

/// Extract validated `(owner, repo)` from a GitHub Pull Request URL.
pub fn parse_github_owner_repo_from_pr_url(url: &str) -> Option<(String, String)> {
    let without_scheme = url
        .trim()
        .strip_prefix("https://")
        .or_else(|| url.trim().strip_prefix("http://"))?;
    let clean = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme);
    let segments: Vec<&str> = clean.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 4 || segments[3] != "pull" {
        return None;
    }
    let owner = segments[1];
    let repo = segments[2];
    let slug = format!("{owner}/{repo}");
    if !is_valid_github_repo(&slug) {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

const PR_REVIEW_THREADS_GRAPHQL: &str = r#"
query($owner: String!, $repo: String!, $number: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      reviewThreads(first: 50) {
        nodes {
          isResolved
          isOutdated
          path
          line
          startLine
          comments(first: 10) {
            nodes {
              id
              body
              url
              createdAt
              author { login }
            }
          }
        }
      }
    }
  }
}
"#;

async fn fetch_pr_review_threads_graphql(
    repo_root: &Path,
    owner: &str,
    repo: &str,
    number: u64,
) -> Option<Vec<GitPrReviewComment>> {
    let mut cmd = Command::new("gh");
    cmd.args([
        "api",
        "graphql",
        "-f",
        &format!("query={PR_REVIEW_THREADS_GRAPHQL}"),
        "-F",
        &format!("owner={owner}"),
        "-F",
        &format!("repo={repo}"),
        "-F",
        &format!("number={number}"),
    ])
    .current_dir(repo_root)
    .env("GH_PROMPT_DISABLED", "1")
    .env("GIT_TERMINAL_PROMPT", "0")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

    let child = cmd.spawn().ok()?;
    let out = drive_child_with_stdin_timeout(child, None, GH_GRAPHQL_TIMEOUT, "gh api graphql")
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }

    let json: Value = serde_json::from_slice(&out.stdout).ok()?;
    let threads = json
        .pointer("/data/repository/pullRequest/reviewThreads/nodes")
        .and_then(Value::as_array)?;

    let mut out_comments = Vec::new();
    for (t_idx, thread) in threads.iter().enumerate() {
        let is_resolved = thread
            .get("isResolved")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let is_outdated = thread
            .get("isOutdated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let path = thread
            .get("path")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let line = thread
            .get("line")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok());
        let start_line = thread
            .get("startLine")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok());

        let Some(nodes) = thread.pointer("/comments/nodes").and_then(Value::as_array) else {
            continue;
        };

        for (c_idx, node) in nodes.iter().enumerate() {
            let body = node
                .get("body")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if body.is_empty() {
                continue;
            }
            let id = node
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("thread_{t_idx}_{c_idx}"));
            let author = node
                .pointer("/author/login")
                .and_then(Value::as_str)
                .unwrap_or("reviewer")
                .to_string();
            let url = node
                .get("url")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let created_at = node
                .get("createdAt")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);

            out_comments.push(GitPrReviewComment {
                id,
                author,
                body: truncate_utf8(body, 2048),
                path: path.clone(),
                line,
                start_line,
                url,
                created_at,
                is_resolved,
                is_outdated,
                kind: "review_thread".to_string(),
            });
        }
    }

    Some(out_comments)
}

fn parse_pr_view_json(
    json: &Value,
    graphql_threads: Option<Vec<GitPrReviewComment>>,
) -> GitPrStatus {
    let number = json.get("number").and_then(Value::as_u64);
    let title = json
        .get("title")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let url = json
        .get("url")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let state = json
        .get("state")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let is_draft = json
        .get("isDraft")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mergeable = json
        .get("mergeable")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let review_decision = json
        .get("reviewDecision")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let head_ref = json
        .get("headRefName")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let base_ref = json
        .get("baseRefName")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let checks = parse_pr_checks(json.get("statusCheckRollup"));
    let mut passing_checks = 0u32;
    let mut failing_checks = 0u32;
    let mut pending_checks = 0u32;
    for check in &checks {
        match check.state {
            GitCiCheckState::Passing => passing_checks += 1,
            GitCiCheckState::Failing => failing_checks += 1,
            GitCiCheckState::Pending => pending_checks += 1,
            GitCiCheckState::Neutral => {}
        }
    }

    let mut review_comments = graphql_threads.unwrap_or_default();
    let mut seen_ids: HashSet<String> = review_comments.iter().map(|c| c.id.clone()).collect();

    if let Some(reviews) = json.get("reviews").and_then(Value::as_array) {
        for (idx, review) in reviews.iter().enumerate() {
            let body = review
                .get("body")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if body.is_empty() {
                continue;
            }
            let r_state = review.get("state").and_then(Value::as_str).unwrap_or("");
            let id = review
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("review_{idx}"));
            if !seen_ids.insert(id.clone()) {
                continue;
            }
            let author = review
                .pointer("/author/login")
                .and_then(Value::as_str)
                .unwrap_or("reviewer")
                .to_string();
            let created_at = review
                .get("submittedAt")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            review_comments.push(GitPrReviewComment {
                id,
                author,
                body: truncate_utf8(body, 2048),
                path: None,
                line: None,
                start_line: None,
                url: url.clone(),
                created_at,
                is_resolved: r_state == "APPROVED" || r_state == "DISMISSED",
                is_outdated: false,
                kind: "review_summary".to_string(),
            });
        }
    }

    if let Some(comments) = json.get("comments").and_then(Value::as_array) {
        for (idx, comment) in comments.iter().enumerate() {
            let body = comment
                .get("body")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if body.is_empty() {
                continue;
            }
            let id = comment
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("comment_{idx}"));
            if !seen_ids.insert(id.clone()) {
                continue;
            }
            let author = comment
                .pointer("/author/login")
                .and_then(Value::as_str)
                .unwrap_or("reviewer")
                .to_string();
            let comment_url = comment
                .get("url")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| url.clone());
            let created_at = comment
                .get("createdAt")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            review_comments.push(GitPrReviewComment {
                id,
                author,
                body: truncate_utf8(body, 2048),
                path: None,
                line: None,
                start_line: None,
                url: comment_url,
                created_at,
                is_resolved: false,
                is_outdated: false,
                kind: "issue_comment".to_string(),
            });
        }
    }

    let unresolved_thread_count = review_comments
        .iter()
        .filter(|c| c.kind == "review_thread" && !c.is_resolved)
        .count() as u32;
    let unresolved_summary_count = review_comments
        .iter()
        .filter(|c| c.kind == "review_summary" && !c.is_resolved)
        .count() as u32;
    let unresolved_comment_count = unresolved_thread_count + unresolved_summary_count;

    GitPrStatus {
        availability: GitPrAvailability::Available,
        reason: None,
        number,
        title,
        url,
        state,
        is_draft,
        mergeable,
        review_decision,
        head_ref,
        base_ref,
        checks,
        passing_checks,
        failing_checks,
        pending_checks,
        review_comments,
        unresolved_comment_count,
    }
}

/// Fetch Pull Request status, CI check rollup, and review comments for the active Git branch via
/// GitHub CLI (`gh`). Gracefully reports `GitPrAvailability::Unavailable` or
/// `GitPrAvailability::NoPullRequest` without hard errors when `gh` is missing or unconfigured.
pub async fn git_pr_status(cwd: &Path) -> Result<GitPrStatus, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;

    let mut cmd = Command::new("gh");
    cmd.args([
        "pr",
        "view",
        "--json",
        "number,title,url,state,isDraft,mergeable,reviewDecision,headRefName,baseRefName,statusCheckRollup,reviews,comments",
    ])
    .current_dir(&ctx.repo_root)
    .env("GH_PROMPT_DISABLED", "1")
    .env("GIT_TERMINAL_PROMPT", "0")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(GitPrStatus {
                availability: GitPrAvailability::Unavailable,
                reason: Some("GitHub CLI (`gh`) is not installed on this host.".to_string()),
                ..GitPrStatus::default()
            });
        }
        Err(e) => {
            return Ok(GitPrStatus {
                availability: GitPrAvailability::Unavailable,
                reason: Some(format!("Unable to launch `gh`: {e}")),
                ..GitPrStatus::default()
            });
        }
    };

    let out =
        match drive_child_with_stdin_timeout(child, None, GH_QUERY_TIMEOUT, "gh pr view").await {
            Ok(out) => out,
            Err(GitWriteError::Timeout(_)) => {
                return Ok(GitPrStatus {
                    availability: GitPrAvailability::Unavailable,
                    reason: Some("Timed out querying GitHub PR status.".to_string()),
                    ..GitPrStatus::default()
                });
            }
            Err(e) => {
                return Ok(GitPrStatus {
                    availability: GitPrAvailability::Unavailable,
                    reason: Some(e.to_string()),
                    ..GitPrStatus::default()
                });
            }
        };

    if !out.status.success() {
        let detail = format_process_failure(&out);
        let lower = detail.to_ascii_lowercase();
        if lower.contains("no pull requests found")
            || lower.contains("no pull request found")
            || lower.contains("could not find pull request")
        {
            return Ok(GitPrStatus {
                availability: GitPrAvailability::NoPullRequest,
                reason: Some("No pull request associated with the current branch.".to_string()),
                ..GitPrStatus::default()
            });
        }
        return Ok(GitPrStatus {
            availability: GitPrAvailability::Unavailable,
            reason: Some(truncate_utf8(&detail, 256)),
            ..GitPrStatus::default()
        });
    }

    let json: Value = match serde_json::from_slice(&out.stdout) {
        Ok(v) => v,
        Err(e) => {
            return Ok(GitPrStatus {
                availability: GitPrAvailability::Unavailable,
                reason: Some(format!("Failed to parse `gh pr view` output: {e}")),
                ..GitPrStatus::default()
            });
        }
    };

    let pr_number = json.get("number").and_then(Value::as_u64);
    let pr_url = json.get("url").and_then(Value::as_str);

    let graphql_threads = match (
        pr_number,
        pr_url.and_then(parse_github_owner_repo_from_pr_url),
    ) {
        (Some(num), Some((owner, repo))) => {
            fetch_pr_review_threads_graphql(&ctx.repo_root, &owner, &repo, num).await
        }
        _ => None,
    };

    Ok(parse_pr_view_json(&json, graphql_threads))
}

// ============================================================================
// 5. Fast Deterministic Conventional Commit Message Synthesis
// ============================================================================

#[derive(Debug, Clone)]
struct ChangedFileSummary {
    path: String,
    status: char, // 'A', 'M', 'D', 'R'
    additions: u32,
    deletions: u32,
}

fn is_doc_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with("docs/")
        || lower.ends_with(".md")
        || lower.ends_with(".mdx")
        || lower.ends_with(".txt")
        || lower.ends_with("license")
        || lower.contains("changelog")
}

fn is_ci_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with(".github/")
        || lower.starts_with(".gitlab-ci")
        || lower.starts_with(".circleci/")
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/tests/")
        || lower.starts_with("tests/")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_test.go")
        || lower.ends_with("_test.py")
}

fn is_style_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".css")
        || lower.ends_with(".scss")
        || lower.ends_with(".less")
        || lower.contains(".prettierrc")
        || lower.contains(".eslintrc")
        || lower.ends_with("rustfmt.toml")
}

fn is_chore_manifest_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "Cargo.toml"
            | "Cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "Makefile"
            | "Dockerfile"
            | ".gitignore"
    )
}

fn infer_path_scope(path: &str) -> Option<String> {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return None;
    }
    if segs[0] == "crates" && segs.len() >= 2 {
        let crate_name = segs[1].strip_prefix("orbit-").unwrap_or(segs[1]);
        return Some(crate_name.to_string());
    }
    if segs[0] == "apps" && segs.len() >= 2 {
        return Some(segs[1].to_string());
    }
    if segs[0] == "packages" && segs.len() >= 2 {
        return Some(segs[1].to_string());
    }
    if segs[0] == ".github" {
        return Some("ci".to_string());
    }
    if segs[0] == "docs" {
        return None;
    }
    if segs[0] == "src" && segs.len() >= 3 {
        return Some(segs[1].to_string());
    }
    if segs.len() >= 2 {
        return Some(segs[0].to_string());
    }
    None
}

fn infer_commit_scope(files: &[ChangedFileSummary]) -> Option<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for f in files {
        if let Some(scope) = infer_path_scope(&f.path) {
            *counts.entry(scope).or_insert(0) += 1;
        }
    }
    if counts.is_empty() {
        return None;
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if ranked.len() == 1 {
        Some(ranked[0].0.clone())
    } else if ranked.len() == 2 {
        let combined = format!("{},{}", ranked[0].0, ranked[1].0);
        if combined.len() <= 24 {
            Some(combined)
        } else {
            Some(ranked[0].0.clone())
        }
    } else {
        // When changes span 3+ modules, omit scope unless one module dominates (>= 70%).
        let total: usize = ranked.iter().map(|(_, c)| *c).sum();
        if total > 0 && ranked[0].1 * 10 >= total * 7 {
            Some(ranked[0].0.clone())
        } else {
            None
        }
    }
}

fn extract_symbols_from_diff(diff: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();

    for line in diff.lines() {
        if symbols.len() >= 8 {
            break;
        }
        // Extract from hunk section headers (`@@ -1,2 +1,3 @@ pub fn foo(...)`)
        if line.starts_with("@@ ")
            && let Some(after_second) = line.splitn(3, "@@").nth(2)
            && let Some(sym) = extract_identifier_from_declaration(after_second.trim())
            && seen.insert(sym.clone())
        {
            symbols.push(sym);
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            if added.starts_with("++") {
                continue;
            }
            if let Some(sym) = extract_identifier_from_declaration(added.trim())
                && seen.insert(sym.clone())
            {
                symbols.push(sym);
            }
        }
    }

    symbols
}

fn extract_identifier_from_declaration(line: &str) -> Option<String> {
    let keywords = [
        "pub async fn ",
        "pub(crate) async fn ",
        "pub fn ",
        "pub(crate) fn ",
        "async fn ",
        "fn ",
        "pub struct ",
        "pub enum ",
        "struct ",
        "enum ",
        "export async function ",
        "export function ",
        "export const ",
        "export interface ",
        "export type ",
        "function ",
        "class ",
        "def ",
    ];
    for kw in keywords {
        if let Some(rest) = line.strip_prefix(kw) {
            let ident: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if ident.len() >= 3 && !matches!(ident.as_str(), "new" | "test" | "main" | "default") {
                return Some(ident);
            }
        }
    }
    None
}

fn infer_commit_type(files: &[ChangedFileSummary], diff: &str) -> &'static str {
    if files.iter().all(|f| is_doc_path(&f.path)) {
        return "docs";
    }
    if files.iter().all(|f| is_ci_path(&f.path)) {
        return "ci";
    }
    if files.iter().all(|f| is_test_path(&f.path)) {
        return "test";
    }
    if files.iter().all(|f| is_style_path(&f.path)) {
        return "style";
    }
    if files.iter().all(|f| is_chore_manifest_path(&f.path)) {
        return "chore";
    }

    let added_files = files.iter().filter(|f| f.status == 'A').count();
    let renamed_files = files.iter().filter(|f| f.status == 'R').count();
    let deleted_files = files.iter().filter(|f| f.status == 'D').count();
    let total_add: u32 = files.iter().map(|f| f.additions).sum();
    let total_del: u32 = files.iter().map(|f| f.deletions).sum();

    let mut fix_hits = 0usize;
    let mut feat_hits = 0usize;
    let mut refactor_hits = 0usize;

    for line in diff.lines() {
        let Some(added) = line.strip_prefix('+') else {
            continue;
        };
        if added.starts_with("++") {
            continue;
        }
        let lower = added.to_ascii_lowercase();
        for token in [
            "fix", "bug", "guard", "clamp", "prevent", "fallback", "crash", "panic", "race",
            "leak", "sanitize", "escape", "reject", "boundary", "err(", "error",
        ] {
            if lower.contains(token) {
                fix_hits += 1;
            }
        }
        for token in [
            "pub async fn ",
            "pub fn ",
            "export function ",
            "export const ",
            "#[tauri::command]",
        ] {
            if lower.contains(token) {
                feat_hits += 2;
            }
        }
        for token in ["refactor", "extract", "rename", "cleanup", "simplify"] {
            if lower.contains(token) {
                refactor_hits += 1;
            }
        }
    }

    if added_files > 0 && (feat_hits > 0 || total_add >= total_del.saturating_mul(2)) {
        return "feat";
    }
    if renamed_files > 0 && total_add <= total_del.saturating_add(20) {
        return "refactor";
    }
    if deleted_files == files.len() {
        return "refactor";
    }
    if fix_hits >= 2 && added_files == 0 && total_add <= 80 {
        return "fix";
    }
    if feat_hits >= 4 || (total_add >= 30 && total_add > total_del.saturating_mul(2)) {
        return "feat";
    }
    if refactor_hits >= 2 || (total_del >= 20 && total_add >= 20 && total_add <= total_del * 2) {
        return "refactor";
    }
    if fix_hits > 0 && added_files == 0 {
        return "fix";
    }
    if added_files > 0 { "feat" } else { "chore" }
}

fn short_stem(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    let stem = base
        .rsplit_once('.')
        .map(|(s, _)| s)
        .filter(|s| !s.is_empty())
        .unwrap_or(base);
    stem.replace(['_', '-'], " ")
}

fn clamp_subject(prefix: &str, description: &str) -> String {
    let clean_desc = description
        .trim()
        .trim_end_matches('.')
        .replace(['\r', '\n'], " ");
    let full = format!("{prefix}{clean_desc}");
    if full.chars().count() <= MAX_COMMIT_SUBJECT_CHARS {
        return full;
    }
    let budget = MAX_COMMIT_SUBJECT_CHARS.saturating_sub(1);
    let mut clipped: String = full.chars().take(budget).collect();
    if let Some(last_space) = clipped.rfind(' ')
        && last_space > prefix.len() + 8
    {
        clipped.truncate(last_space);
    }
    clipped.trim_end_matches([',', ' ', '-']).to_string()
}

fn synthesize_commit_message(
    files: &[ChangedFileSummary],
    diff: &str,
    diff_source: &str,
) -> GitGeneratedCommitMessage {
    let commit_type = infer_commit_type(files, diff);
    let scope = infer_commit_scope(files);
    let symbols = extract_symbols_from_diff(diff);

    let prefix = match &scope {
        Some(sc) => format!("{commit_type}({sc}): "),
        None => format!("{commit_type}: "),
    };

    let verb = match commit_type {
        "feat" => {
            if files.iter().any(|f| f.status == 'A') {
                "add"
            } else {
                "support"
            }
        }
        "fix" => "fix",
        "docs" => "update",
        "test" => "add tests for",
        "refactor" => "refactor",
        "style" => "polish",
        "ci" => "update",
        _ => "update",
    };

    let desc = if files.len() == 1 {
        let f = &files[0];
        let stem = short_stem(&f.path);
        if let Some(sym) = symbols.first()
            && !stem.eq_ignore_ascii_case(sym)
            && prefix.len() + verb.len() + stem.len() + sym.len() + 6 <= MAX_COMMIT_SUBJECT_CHARS
        {
            format!("{verb} {stem} ({sym})")
        } else if f.status == 'D' {
            format!("remove {stem}")
        } else {
            format!("{verb} {stem}")
        }
    } else if files.len() == 2 {
        let a = short_stem(&files[0].path);
        let b = short_stem(&files[1].path);
        if a == b {
            format!("{verb} {a}")
        } else {
            format!("{verb} {a} and {b}")
        }
    } else {
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        for f in files {
            let s = short_stem(&f.path);
            if seen.insert(s.clone()) {
                names.push(s);
            }
            if names.len() >= 2 {
                break;
            }
        }
        let rem = files.len().saturating_sub(names.len());
        if rem > 0 && !names.is_empty() {
            format!("{verb} {} and {rem} more files", names.join(", "))
        } else {
            format!("{verb} {} files", files.len())
        }
    };

    let subject = clamp_subject(&prefix, &desc);

    let mut bullets = Vec::new();
    for f in files.iter().take(6) {
        let action = match f.status {
            'A' => "Add",
            'D' => "Remove",
            'R' => "Rename",
            _ => "Update",
        };
        if f.additions > 0 || f.deletions > 0 {
            bullets.push(format!(
                "- {action} {} (+{}/-{})",
                f.path, f.additions, f.deletions
            ));
        } else {
            bullets.push(format!("- {action} {}", f.path));
        }
    }
    if files.len() > 6 {
        bullets.push(format!("- ...and {} more files", files.len() - 6));
    }
    if !symbols.is_empty() && files.len() > 1 {
        let top_syms: Vec<&str> = symbols.iter().take(4).map(String::as_str).collect();
        bullets.push(format!("- Key symbols: {}", top_syms.join(", ")));
    }

    let body = if bullets.is_empty() {
        None
    } else {
        Some(bullets.join("\n"))
    };

    let message = match &body {
        Some(b) => format!("{subject}\n\n{b}"),
        None => subject.clone(),
    };

    GitGeneratedCommitMessage {
        message,
        subject,
        body,
        commit_type: commit_type.to_string(),
        scope,
        diff_source: diff_source.to_string(),
        files_analyzed: files.len() as u32,
        generator: "deterministic".to_string(),
    }
}

async fn collect_diff_summaries(
    repo_root: &Path,
    staged: bool,
) -> Result<(Vec<ChangedFileSummary>, String), GitWriteError> {
    let status_args: &[&str] = if staged {
        &["diff", "--cached", "--name-status", "-z"]
    } else {
        &["diff", "--name-status", "-z"]
    };
    let status_out = run_git_output(
        repo_root,
        status_args,
        LOCAL_GIT_TIMEOUT,
        "diff --name-status",
    )
    .await?;
    if !status_out.status.success() {
        return Err(GitWriteError::GitFailed {
            command: "diff --name-status".to_string(),
            stderr: format_process_failure(&status_out),
        });
    }

    let tokens: Vec<String> = status_out
        .stdout
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).to_string())
        .collect();

    let mut status_by_path: BTreeMap<String, char> = BTreeMap::new();
    let mut idx = 0usize;
    while idx < tokens.len() {
        let code = &tokens[idx];
        let ch = code.chars().next().unwrap_or('M');
        if matches!(ch, 'R' | 'C') && idx + 2 < tokens.len() {
            let new_path = tokens[idx + 2].clone();
            status_by_path.insert(new_path, ch);
            idx += 3;
        } else if idx + 1 < tokens.len() {
            let path = tokens[idx + 1].clone();
            status_by_path.insert(path, ch);
            idx += 2;
        } else {
            break;
        }
    }

    let numstat_args: &[&str] = if staged {
        &["diff", "--cached", "--numstat"]
    } else {
        &["diff", "--numstat"]
    };
    let numstat_out =
        run_git_output(repo_root, numstat_args, LOCAL_GIT_TIMEOUT, "diff --numstat").await?;
    let mut stats_by_path: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    if numstat_out.status.success() {
        for line in String::from_utf8_lossy(&numstat_out.stdout).lines() {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() == 3 {
                let adds = parts[0].parse::<u32>().unwrap_or(0);
                let dels = parts[1].parse::<u32>().unwrap_or(0);
                let path = parts[2].trim().to_string();
                stats_by_path.insert(path, (adds, dels));
            }
        }
    }

    let mut files: Vec<ChangedFileSummary> = status_by_path
        .into_iter()
        .map(|(path, status)| {
            let (additions, deletions) = stats_by_path.get(&path).copied().unwrap_or((0, 0));
            ChangedFileSummary {
                path,
                status,
                additions,
                deletions,
            }
        })
        .collect();

    if !staged {
        let untracked_out = run_git_output(
            repo_root,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            LOCAL_GIT_TIMEOUT,
            "ls-files --others",
        )
        .await?;
        if untracked_out.status.success() {
            for rel in parse_nul_delimited_paths(&untracked_out.stdout) {
                if !rel.ends_with('/') && !files.iter().any(|f| f.path == rel) {
                    files.push(ChangedFileSummary {
                        path: rel,
                        status: 'A',
                        additions: 0,
                        deletions: 0,
                    });
                }
            }
        }
    }

    if files.is_empty() {
        return Ok((Vec::new(), String::new()));
    }

    let diff_args: &[&str] = if staged {
        &["diff", "--cached", "--no-color", "--unified=0"]
    } else {
        &["diff", "--no-color", "--unified=0"]
    };
    let diff_out =
        run_git_output(repo_root, diff_args, LOCAL_GIT_TIMEOUT, "diff --unified=0").await?;
    let diff_text = if diff_out.status.success() {
        let slice = if diff_out.stdout.len() > MAX_COMMIT_DIFF_ANALYSIS_BYTES {
            &diff_out.stdout[..MAX_COMMIT_DIFF_ANALYSIS_BYTES]
        } else {
            &diff_out.stdout
        };
        String::from_utf8_lossy(slice).to_string()
    } else {
        String::new()
    };

    Ok((files, diff_text))
}

/// Generate a crisp conventional commit message from the repository's staged diff (or working-tree
/// diff if nothing is staged), deterministically in pure Rust in sub-100ms.
pub async fn git_generate_commit_message(
    cwd: &Path,
) -> Result<GitGeneratedCommitMessage, GitWriteError> {
    let ctx = resolve_git_repo_context(cwd).await?;
    let (staged_files, staged_diff) = collect_diff_summaries(&ctx.repo_root, true).await?;
    if !staged_files.is_empty() {
        return Ok(synthesize_commit_message(
            &staged_files,
            &staged_diff,
            "staged",
        ));
    }

    let (working_files, working_diff) = collect_diff_summaries(&ctx.repo_root, false).await?;
    if !working_files.is_empty() {
        return Ok(synthesize_commit_message(
            &working_files,
            &working_diff,
            "working_tree",
        ));
    }

    Err(GitWriteError::InvalidInput(
        "No staged or working-tree changes to summarize".to_string(),
    ))
}

// ============================================================================
// Tests (Real Git Integration Tests + Parser / Validator Unit Tests)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_write::{commit_changes, stage_files};

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

    #[test]
    fn stash_ref_validator_enforces_strict_pattern() {
        assert_eq!(validate_stash_ref("stash@{0}").expect("valid 0").1, 0);
        assert_eq!(validate_stash_ref("stash@{42}").expect("valid 42").1, 42);

        for bad in [
            "",
            "stash@{}",
            "stash@{-1}",
            "stash@{0} --all",
            "--all",
            "stash@{0};rm -rf /",
            "HEAD",
            "stash@{0}\n",
            " stash@{0}",
            "stash@{1a}",
        ] {
            assert!(
                validate_stash_ref(bad).is_err(),
                "expected invalid stash ref `{bad}` to be rejected"
            );
        }
    }

    #[test]
    fn parses_two_way_and_diff3_conflict_markers_with_context() {
        let text = concat!(
            "line 1\n",
            "line 2\n",
            "<<<<<<< HEAD\n",
            "ours alpha\n",
            "ours beta\n",
            "||||||| base-sha\n",
            "base value\n",
            "=======\n",
            "theirs value\n",
            ">>>>>>> feat/branch\n",
            "after 1\n",
            "after 2\n",
            "<<<<<<< HEAD\n",
            "second ours\n",
            "=======\n",
            ">>>>>>> feat/branch\n",
        );

        let blocks = parse_conflict_blocks(text);
        assert_eq!(blocks.len(), 2);

        assert_eq!(blocks[0].index, 0);
        assert_eq!(blocks[0].start_line, 3);
        assert_eq!(blocks[0].end_line, 10);
        assert_eq!(blocks[0].ours_label, "HEAD");
        assert_eq!(blocks[0].base_label.as_deref(), Some("base-sha"));
        assert_eq!(blocks[0].theirs_label, "feat/branch");
        assert_eq!(blocks[0].ours, "ours alpha\nours beta");
        assert_eq!(blocks[0].base.as_deref(), Some("base value"));
        assert_eq!(blocks[0].theirs, "theirs value");
        assert_eq!(blocks[0].context_before, vec!["line 1", "line 2"]);
        assert_eq!(blocks[0].context_after, vec!["after 1", "after 2"]);

        assert_eq!(blocks[1].index, 1);
        assert_eq!(blocks[1].start_line, 13);
        assert_eq!(blocks[1].end_line, 16);
        assert_eq!(blocks[1].ours, "second ours");
        assert_eq!(blocks[1].base, None);
        assert_eq!(blocks[1].theirs, "");
    }

    #[test]
    fn parses_github_owner_repo_and_pr_view_json() {
        assert_eq!(
            parse_github_owner_repo_from_pr_url("https://github.com/danielgaoyy/orbit/pull/128"),
            Some(("danielgaoyy".into(), "orbit".into()))
        );
        assert_eq!(
            parse_github_owner_repo_from_pr_url("https://github.com/../evil/pull/1"),
            None
        );

        let sample = serde_json::json!({
            "number": 77,
            "title": "Deep Git Collab",
            "url": "https://github.com/danielgaoyy/orbit/pull/77",
            "state": "OPEN",
            "isDraft": false,
            "mergeable": "MERGEABLE",
            "reviewDecision": "CHANGES_REQUESTED",
            "headRefName": "feat/git-collab",
            "baseRefName": "main",
            "statusCheckRollup": [
                {"name": "clippy", "status": "COMPLETED", "conclusion": "SUCCESS", "workflowName": "CI", "detailsUrl": "https://ci/1"},
                {"name": "desktop-tests", "status": "COMPLETED", "conclusion": "FAILURE", "workflowName": "CI", "detailsUrl": "https://ci/2"},
                {"name": "build-ios", "status": "IN_PROGRESS", "conclusion": "", "workflowName": "CI"}
            ],
            "reviews": [
                {"id": "rev1", "state": "CHANGES_REQUESTED", "body": "Fix clippy warning", "author": {"login": "alice"}}
            ],
            "comments": []
        });

        let parsed = parse_pr_view_json(
            &sample,
            Some(vec![GitPrReviewComment {
                id: "t1".into(),
                author: "bob".into(),
                body: "Add range check".into(),
                path: Some("src/lib.rs".into()),
                line: Some(12),
                start_line: Some(10),
                url: None,
                created_at: None,
                is_resolved: false,
                is_outdated: false,
                kind: "review_thread".into(),
            }]),
        );

        assert_eq!(parsed.availability, GitPrAvailability::Available);
        assert_eq!(parsed.number, Some(77));
        assert_eq!(parsed.passing_checks, 1);
        assert_eq!(parsed.failing_checks, 1);
        assert_eq!(parsed.pending_checks, 1);
        assert_eq!(parsed.unresolved_comment_count, 2);
    }

    #[tokio::test]
    async fn stash_full_lifecycle_push_list_apply_pop_drop() {
        let tmp = TempDirGuard::new("git-stash-lifecycle");
        init_test_repo(tmp.path()).await;

        std::fs::write(tmp.path().join("app.rs"), "fn main() {}\n").expect("write app");
        stage_files(tmp.path(), &["app.rs".into()], false)
            .await
            .expect("stage");
        commit_changes(tmp.path(), "chore: initial", false)
            .await
            .expect("commit");

        // Clean working tree push should error cleanly
        let clean_err = git_stash_push(tmp.path(), Some("empty"), false)
            .await
            .expect_err("clean stash must fail");
        assert!(matches!(clean_err, GitWriteError::InvalidInput(_)));

        // Modify tracked file + add untracked file, push stash with include_untracked=true
        std::fs::write(
            tmp.path().join("app.rs"),
            "fn main() { println!(\"v2\"); }\n",
        )
        .expect("modify app");
        std::fs::write(tmp.path().join("notes.txt"), "wip notes\n").expect("write notes");

        let pushed = git_stash_push(tmp.path(), Some("checkpoint alpha"), true)
            .await
            .expect("stash push");
        assert_eq!(pushed.action, "push");
        assert!(!tmp.path().join("notes.txt").exists());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("app.rs")).expect("read app"),
            "fn main() {}\n"
        );

        let listed = git_stash_list(tmp.path()).await.expect("stash list");
        assert_eq!(listed.entries.len(), 1);
        assert_eq!(listed.entries[0].stash_ref, "stash@{0}");
        assert_eq!(listed.entries[0].index, 0);
        assert_eq!(listed.entries[0].branch.as_deref(), Some("main"));
        assert!(listed.entries[0].message.contains("checkpoint alpha"));
        let first_oid = listed.entries[0]
            .oid
            .clone()
            .expect("stash entry must include full commit OID");
        assert!(first_oid.len() >= 40);

        // Apply restores files AND keeps stash entry (pinned to expected_oid)
        let applied = git_stash_apply(tmp.path(), "stash@{0}", Some(&first_oid))
            .await
            .expect("stash apply");
        assert!(!applied.had_conflicts);
        assert!(tmp.path().join("notes.txt").exists());
        assert_eq!(
            git_stash_list(tmp.path())
                .await
                .expect("list after apply")
                .entries
                .len(),
            1
        );

        // Pushing another stash shifts stash@{0}; dropping stash@{0} with the stale OID MUST fail!
        git_stash_push(tmp.path(), Some("second checkpoint"), true)
            .await
            .expect("stash push second");
        let stale_drop = git_stash_drop(tmp.path(), "stash@{0}", Some(&first_oid))
            .await
            .expect_err("shifted stash drop with stale expected_oid must fail");
        assert!(matches!(stale_drop, GitWriteError::InvalidInput(_)));
        assert_eq!(
            git_stash_list(tmp.path())
                .await
                .expect("both stashes preserved")
                .entries
                .len(),
            2
        );

        // Drop the shifted first entry at stash@{1} using its exact pinned OID
        git_stash_drop(tmp.path(), "stash@{1}", Some(&first_oid))
            .await
            .expect("drop stash@{1} with matching OID");
        let after_drop = git_stash_list(tmp.path()).await.expect("list after drop");
        assert_eq!(after_drop.entries.len(), 1);
        let second_oid = after_drop.entries[0].oid.clone().expect("second stash OID");

        // Pop removes remaining stash entry while restoring changes
        let popped = git_stash_pop(tmp.path(), "stash@{0}", Some(&second_oid))
            .await
            .expect("stash pop");
        assert!(!popped.had_conflicts);
        assert!(tmp.path().join("notes.txt").exists());
        assert!(
            git_stash_list(tmp.path())
                .await
                .expect("list after pop")
                .entries
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rebase_conflict_3way_parse_block_resolution_continue_and_abort() {
        let tmp = TempDirGuard::new("git-rebase-3way");
        init_test_repo(tmp.path()).await;
        // Enable diff3 markers to verify 3-way base parsing end-to-end with real Git!
        run_git_ok(tmp.path(), &["config", "merge.conflictStyle", "diff3"]).await;

        std::fs::write(
            tmp.path().join("config.txt"),
            "header\nsetting = base\nfooter\nmiddle\nmode = base\nend\n",
        )
        .expect("write base");
        stage_files(tmp.path(), &["config.txt".into()], false)
            .await
            .expect("stage base");
        commit_changes(tmp.path(), "chore: base config", false)
            .await
            .expect("commit base");

        // Create feature branch with conflicting edits
        run_git_ok(tmp.path(), &["checkout", "-b", "feat/work"]).await;
        std::fs::write(
            tmp.path().join("config.txt"),
            "header\nsetting = feature\nfooter\nmiddle\nmode = feature\nend\n",
        )
        .expect("write feat");
        stage_files(tmp.path(), &["config.txt".into()], false)
            .await
            .expect("stage feat");
        commit_changes(tmp.path(), "feat: update config on feature", false)
            .await
            .expect("commit feat");

        // Advance main with conflicting edits
        run_git_ok(tmp.path(), &["checkout", "main"]).await;
        std::fs::write(
            tmp.path().join("config.txt"),
            "header\nsetting = main\nfooter\nmiddle\nmode = main\nend\n",
        )
        .expect("write main");
        stage_files(tmp.path(), &["config.txt".into()], false)
            .await
            .expect("stage main");
        commit_changes(tmp.path(), "fix: update config on main", false)
            .await
            .expect("commit main");

        // Switch back to feat/work and start rebase onto main
        run_git_ok(tmp.path(), &["checkout", "feat/work"]).await;

        // Unsafe onto refs must be rejected
        for bad_ref in ["-i", "--exec=id", "main..HEAD", "nonexistent-branch-xyz"] {
            assert!(
                git_rebase_start(tmp.path(), bad_ref).await.is_err(),
                "expected bad onto ref `{bad_ref}` to be rejected"
            );
        }

        let started = git_rebase_start(tmp.path(), "main")
            .await
            .expect("rebase start pauses on conflict");
        assert!(started.status.in_progress);
        assert_eq!(started.status.current_step, Some(1));
        assert_eq!(started.status.total_steps, Some(1));
        assert_eq!(started.status.conflicted_files, vec!["config.txt"]);

        // List conflicts and verify 3-way diff3 blocks + content_sha256
        let conflicts = git_list_conflicts(tmp.path())
            .await
            .expect("list conflicts");
        assert_eq!(conflicts.files.len(), 1);
        assert_eq!(conflicts.files[0].path, "config.txt");
        assert_eq!(conflicts.files[0].blocks.len(), 2);
        assert_eq!(conflicts.files[0].blocks[0].ours, "setting = main");
        assert_eq!(
            conflicts.files[0].blocks[0].base.as_deref(),
            Some("setting = base")
        );
        assert_eq!(conflicts.files[0].blocks[0].theirs, "setting = feature");
        let initial_sha = conflicts.files[0]
            .content_sha256
            .clone()
            .expect("text conflict must report content_sha256");

        // Reject path traversal during conflict resolution
        let bad_path = git_resolve_conflict(
            tmp.path(),
            "../escape.txt",
            None,
            &GitConflictResolution::Ours,
            true,
            None,
        )
        .await
        .expect_err("path traversal must fail");
        assert!(matches!(bad_path, GitWriteError::InvalidPath { .. }));

        // Stale expected_sha256 MUST be rejected cleanly before mutating disk!
        let stale_sha_err = git_resolve_conflict(
            tmp.path(),
            "config.txt",
            Some(0),
            &GitConflictResolution::Theirs,
            true,
            Some(&"0".repeat(64)),
        )
        .await
        .expect_err("stale expected_sha256 must fail");
        assert!(matches!(stale_sha_err, GitWriteError::InvalidInput(_)));

        // Rebase skip with wrong expected_step MUST be rejected!
        let wrong_step_err = git_rebase_skip(tmp.path(), Some(99))
            .await
            .expect_err("mismatched expected_step must fail");
        assert!(matches!(wrong_step_err, GitWriteError::InvalidInput(_)));

        // Resolve block 0 with Theirs using exact initial_sha -> 1 block remains, NOT yet staged
        let res0 = git_resolve_conflict(
            tmp.path(),
            "config.txt",
            Some(0),
            &GitConflictResolution::Theirs,
            true,
            Some(&initial_sha),
        )
        .await
        .expect("resolve block 0");
        assert_eq!(res0.remaining_blocks, 1);
        assert!(!res0.staged);

        // Reusing the previous initial_sha after block 0 changed the file MUST fail!
        let stale_after_block0 = git_resolve_conflict(
            tmp.path(),
            "config.txt",
            Some(0),
            &GitConflictResolution::Ours,
            true,
            Some(&initial_sha),
        )
        .await
        .expect_err("reused stale sha after edit must fail");
        assert!(matches!(stale_after_block0, GitWriteError::InvalidInput(_)));

        // Continuing while still conflicted must return helpful error
        assert!(git_rebase_continue(tmp.path()).await.is_err());

        // Reload fresh content_sha256 and resolve remaining block with ManualContent -> auto-staged!
        let refreshed = git_list_conflicts(tmp.path())
            .await
            .expect("refresh conflicts");
        let next_sha = refreshed.files[0]
            .content_sha256
            .clone()
            .expect("refreshed sha");
        let res1 = git_resolve_conflict(
            tmp.path(),
            "config.txt",
            Some(0),
            &GitConflictResolution::ManualContent {
                content: "mode = hybrid".into(),
            },
            true,
            Some(&next_sha),
        )
        .await
        .expect("resolve block 1");
        assert_eq!(res1.remaining_blocks, 0);
        assert!(res1.staged);

        // Once resolved and no longer unmerged in Git index, resolving again MUST fail!
        let non_unmerged = git_resolve_conflict(
            tmp.path(),
            "config.txt",
            None,
            &GitConflictResolution::Ours,
            true,
            None,
        )
        .await
        .expect_err("resolving already-staged non-unmerged file must fail");
        assert!(matches!(non_unmerged, GitWriteError::InvalidInput(_)));

        // Continue rebase -> completes cleanly!
        let cont = git_rebase_continue(tmp.path())
            .await
            .expect("rebase continue");
        assert!(!cont.status.in_progress);
        assert!(cont.status.conflicted_files.is_empty());

        let final_content =
            std::fs::read_to_string(tmp.path().join("config.txt")).expect("read final config");
        assert_eq!(
            final_content,
            "header\nsetting = feature\nfooter\nmiddle\nmode = hybrid\nend\n"
        );

        // Also verify rebase abort on a second conflicting branch
        run_git_ok(tmp.path(), &["checkout", "-b", "feat/abort-test", "HEAD~1"]).await;
        std::fs::write(
            tmp.path().join("config.txt"),
            "header\nsetting = abort-branch\nfooter\nmiddle\nmode = abort\nend\n",
        )
        .expect("write abort branch");
        stage_files(tmp.path(), &["config.txt".into()], false)
            .await
            .expect("stage abort");
        commit_changes(tmp.path(), "feat: conflicting abort branch", false)
            .await
            .expect("commit abort");

        let started_abort = git_rebase_start(tmp.path(), "feat/work")
            .await
            .expect("start conflicting rebase for abort");
        assert!(started_abort.status.in_progress);

        let aborted = git_rebase_abort(tmp.path()).await.expect("rebase abort");
        assert!(!aborted.status.in_progress);
        assert!(
            std::fs::read_to_string(tmp.path().join("config.txt"))
                .expect("read restored")
                .contains("setting = abort-branch")
        );
    }

    #[tokio::test]
    async fn deterministic_commit_message_synthesis_handles_staged_and_working_diffs() {
        let tmp = TempDirGuard::new("git-commit-gen");
        init_test_repo(tmp.path()).await;

        std::fs::create_dir_all(tmp.path().join("crates/orbit-daemon/src"))
            .expect("mkdir daemon src");
        std::fs::write(
            tmp.path().join("crates/orbit-daemon/src/git_collab.rs"),
            "pub fn git_stash_list() {}\npub fn git_rebase_start() {}\n",
        )
        .expect("write file");

        // Unstaged working tree fallback works before staging
        let working_msg = git_generate_commit_message(tmp.path())
            .await
            .expect("working tree commit msg");
        assert_eq!(working_msg.diff_source, "working_tree");
        assert_eq!(working_msg.commit_type, "feat");
        assert_eq!(working_msg.scope.as_deref(), Some("daemon"));
        assert!(working_msg.subject.chars().count() <= MAX_COMMIT_SUBJECT_CHARS);

        // Stage the file and verify staged diff analysis + symbol extraction
        stage_files(
            tmp.path(),
            &["crates/orbit-daemon/src/git_collab.rs".into()],
            false,
        )
        .await
        .expect("stage");
        let staged_msg = git_generate_commit_message(tmp.path())
            .await
            .expect("staged commit msg");
        assert_eq!(staged_msg.diff_source, "staged");
        assert_eq!(staged_msg.commit_type, "feat");
        assert_eq!(staged_msg.scope.as_deref(), Some("daemon"));
        assert!(staged_msg.subject.starts_with("feat(daemon): "));
        assert!(staged_msg.subject.chars().count() <= MAX_COMMIT_SUBJECT_CHARS);
        assert!(
            staged_msg
                .message
                .contains("crates/orbit-daemon/src/git_collab.rs")
        );

        commit_changes(tmp.path(), &staged_msg.message, false)
            .await
            .expect("commit synthesized");

        // Clean repo returns clear InvalidInput error
        let clean = git_generate_commit_message(tmp.path())
            .await
            .expect_err("clean repo must fail");
        assert!(matches!(clean, GitWriteError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn modify_delete_conflict_records_deletion_via_git_rm_or_restores_modified_side() {
        let tmp = TempDirGuard::new("git-modify-delete");
        init_test_repo(tmp.path()).await;

        std::fs::write(tmp.path().join("obsolete.txt"), "v1\n").expect("write obsolete");
        std::fs::write(tmp.path().join("keep_me.txt"), "v1\n").expect("write keep_me");
        stage_files(tmp.path(), &[], true).await.expect("stage");
        commit_changes(tmp.path(), "chore: initial files", false)
            .await
            .expect("commit");

        // Branch modifies both files
        run_git_ok(tmp.path(), &["checkout", "-b", "feat/mod"]).await;
        std::fs::write(tmp.path().join("obsolete.txt"), "v2 modified\n").expect("mod obsolete");
        std::fs::write(tmp.path().join("keep_me.txt"), "v2 keep\n").expect("mod keep_me");
        stage_files(tmp.path(), &[], true).await.expect("stage mod");
        commit_changes(tmp.path(), "feat: modify both files", false)
            .await
            .expect("commit mod");

        // Main deletes both files
        run_git_ok(tmp.path(), &["checkout", "main"]).await;
        run_git_ok(tmp.path(), &["rm", "obsolete.txt", "keep_me.txt"]).await;
        commit_changes(tmp.path(), "refactor: delete both files", false)
            .await
            .expect("commit delete");

        // Rebase feat/mod onto main -> during rebase, Ours (stage 2) is main (deleted!), Theirs (stage 3) is feat/mod (modified!)
        run_git_ok(tmp.path(), &["checkout", "feat/mod"]).await;
        let started = git_rebase_start(tmp.path(), "main")
            .await
            .expect("rebase start pauses on modify/delete conflicts");
        assert!(started.status.in_progress);
        assert_eq!(started.status.conflicted_files.len(), 2);

        // Resolve `obsolete.txt` by accepting Ours (the deletion) -> MUST remove file & stage deletion via `git rm -f`!
        let del_res = git_resolve_conflict(
            tmp.path(),
            "obsolete.txt",
            None,
            &GitConflictResolution::Ours,
            true,
            None,
        )
        .await
        .expect("accept deletion via git rm");
        assert!(del_res.staged);
        assert_eq!(del_res.remaining_blocks, 0);
        assert!(!tmp.path().join("obsolete.txt").exists());

        // Resolve `keep_me.txt` by accepting Theirs (the modified version) -> keeps file & stages it!
        let keep_res = git_resolve_conflict(
            tmp.path(),
            "keep_me.txt",
            None,
            &GitConflictResolution::Theirs,
            true,
            None,
        )
        .await
        .expect("accept modified side");
        assert!(keep_res.staged);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("keep_me.txt")).expect("read keep_me"),
            "v2 keep\n"
        );

        let cont = git_rebase_continue(tmp.path())
            .await
            .expect("continue rebase after modify/delete resolution");
        assert!(!cont.status.in_progress);
        assert!(!tmp.path().join("obsolete.txt").exists());
        assert!(tmp.path().join("keep_me.txt").exists());
    }

    #[tokio::test]
    async fn validate_rebase_onto_ref_pins_full_oid_via_end_of_options() {
        let tmp = TempDirGuard::new("git-rev-parse-oid");
        init_test_repo(tmp.path()).await;

        std::fs::write(tmp.path().join("a.txt"), "hello\n").expect("write a");
        stage_files(tmp.path(), &["a.txt".into()], false)
            .await
            .expect("stage");
        let commit = commit_changes(tmp.path(), "chore: init", false)
            .await
            .expect("commit");

        // Also create a valid tag containing characters that a brittle shell blacklist would wrongly reject.
        run_git_ok(tmp.path(), &["tag", "release/v1'0"]).await;

        let pinned_main = validate_rebase_onto_ref(tmp.path(), "main")
            .await
            .expect("resolve main to full OID");
        assert_eq!(pinned_main, commit.commit_sha);

        let pinned_tag = validate_rebase_onto_ref(tmp.path(), "release/v1'0")
            .await
            .expect("resolve tag via --end-of-options without shell blacklist");
        assert_eq!(pinned_tag, commit.commit_sha);

        for invalid in ["-i", "--exec=id", "HEAD~1..HEAD", "nonexistent-ref"] {
            assert!(
                validate_rebase_onto_ref(tmp.path(), invalid).await.is_err(),
                "expected invalid rev `{invalid}` to be rejected"
            );
        }
    }

    #[tokio::test]
    async fn large_binary_conflict_resolves_via_index_stages_without_unbounded_read() {
        let tmp = TempDirGuard::new("git-large-binary-conflict");
        init_test_repo(tmp.path()).await;

        let base_blob = vec![0u8; 64];
        std::fs::write(tmp.path().join("asset.bin"), &base_blob).expect("write base bin");
        stage_files(tmp.path(), &["asset.bin".into()], false)
            .await
            .expect("stage base");
        commit_changes(tmp.path(), "chore: base binary", false)
            .await
            .expect("commit base");

        // Feature branch writes > 2 MiB binary file
        run_git_ok(tmp.path(), &["checkout", "-b", "feat/bin"]).await;
        let feat_blob = vec![0xAAu8; MAX_CONFLICT_FILE_BYTES + 4096];
        std::fs::write(tmp.path().join("asset.bin"), &feat_blob).expect("write feat bin");
        stage_files(tmp.path(), &["asset.bin".into()], false)
            .await
            .expect("stage feat");
        commit_changes(tmp.path(), "feat: update binary", false)
            .await
            .expect("commit feat");

        // Main writes different > 2 MiB binary file
        run_git_ok(tmp.path(), &["checkout", "main"]).await;
        let main_blob = vec![0xBBu8; MAX_CONFLICT_FILE_BYTES + 4096];
        std::fs::write(tmp.path().join("asset.bin"), &main_blob).expect("write main bin");
        stage_files(tmp.path(), &["asset.bin".into()], false)
            .await
            .expect("stage main");
        commit_changes(tmp.path(), "fix: update binary on main", false)
            .await
            .expect("commit main");

        run_git_ok(tmp.path(), &["checkout", "feat/bin"]).await;
        let started = git_rebase_start(tmp.path(), "main")
            .await
            .expect("start binary conflict rebase");
        assert!(started.status.in_progress);

        let listed = git_list_conflicts(tmp.path())
            .await
            .expect("list binary conflict");
        assert_eq!(listed.files.len(), 1);
        assert!(listed.files[0].is_binary);
        assert_eq!(listed.files[0].content_sha256, None);

        // Resolve oversized binary to Theirs (feat/bin) cleanly via Git index stage
        let resolved = git_resolve_conflict(
            tmp.path(),
            "asset.bin",
            None,
            &GitConflictResolution::Theirs,
            true,
            None,
        )
        .await
        .expect("resolve oversized binary");
        assert!(resolved.staged);
        let on_disk = std::fs::read(tmp.path().join("asset.bin")).expect("read resolved bin");
        assert_eq!(on_disk.len(), feat_blob.len());
        assert_eq!(on_disk[0], 0xAA);
    }
}
