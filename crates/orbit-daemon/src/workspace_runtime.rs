//! Worktree full lifecycle, `.worktreeinclude` safe copier, `orbit.json` / `paseo.json`
//! setup/teardown/scripts runtime, dynamic port allocation, and `*.localhost` HTTP reverse proxy.

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::net::TcpListener;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::header::{self, HeaderMap, HeaderName, HeaderValue};
use axum::http::{Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use orbit_protocol::{
    WorkspaceScriptInfo, WorkspaceScriptKind, WorkspaceScriptStatus, WorkspaceSetupProgress,
    WorkspaceSetupStatus, WorktreeCreateMode,
};
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::{RwLock, watch};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::fast_copy::{
    CopyMethod, copy_dir_cow_within, copy_file_cow_within, resolve_symlink_target_safe,
};
use crate::worktree_pool::WorktreePool;

/// Parsed project worktree/script manifest from `orbit.json` (with automatic `paseo.json` fallback).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkspaceManifest {
    #[serde(default)]
    pub worktree: WorktreeManifestSection,
    #[serde(default)]
    pub scripts: HashMap<String, ManifestScriptEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorktreeManifestSection {
    #[serde(default)]
    pub setup: Option<CommandOrList>,
    #[serde(default)]
    pub teardown: Option<CommandOrList>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum CommandOrList {
    Single(String),
    List(Vec<String>),
}

impl CommandOrList {
    pub fn into_commands(self) -> Vec<String> {
        match self {
            Self::Single(s) => s
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(ToOwned::to_owned)
                .collect(),
            Self::List(list) => list
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestScriptEntry {
    pub command: String,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

impl ManifestScriptEntry {
    pub fn script_kind(&self) -> WorkspaceScriptKind {
        if self
            .kind
            .as_deref()
            .is_some_and(|k| k.eq_ignore_ascii_case("service"))
        {
            WorkspaceScriptKind::Service
        } else {
            WorkspaceScriptKind::Command
        }
    }
}

/// Load `orbit.json` if present, falling back cleanly to `paseo.json` for zero-friction compatibility.
pub fn load_workspace_manifest(cwd: &Path) -> WorkspaceManifest {
    load_workspace_manifest_with_fallback(cwd, None)
}

/// Load `orbit.json` / `paseo.json` from `cwd`, falling back to `source_cwd` when working in a
/// git worktree whose source repo contains an uncommitted local manifest.
pub fn load_workspace_manifest_with_fallback(
    cwd: &Path,
    source_cwd: Option<&Path>,
) -> WorkspaceManifest {
    let mut dirs = vec![cwd];
    if let Some(src) = source_cwd
        && src != cwd
    {
        dirs.push(src);
    }
    for dir in dirs {
        for filename in ["orbit.json", "paseo.json"] {
            let candidate = dir.join(filename);
            if candidate.is_file()
                && let Ok(raw) = std::fs::read_to_string(&candidate)
                && let Ok(parsed) = serde_json::from_str::<WorkspaceManifest>(&raw)
            {
                return parsed;
            }
        }
    }
    WorkspaceManifest::default()
}

/// Summary of a `.worktreeinclude` copy pass, including CoW vs standard copy counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeIncludeCopyReport {
    pub copied: Vec<String>,
    pub cow_cloned: usize,
    pub standard_copied: usize,
}

/// Copy gitignored files and directories declared in `<source_cwd>/.worktreeinclude` into the
/// newly created worktree using Copy-on-Write (`copy_file_cow` / `copy_dir_cow`) when supported.
/// Guards against path traversal (`..`) and symlinks escaping `source_cwd`.
pub fn copy_worktree_includes(source_cwd: &Path, worktree_cwd: &Path) -> Vec<String> {
    copy_worktree_includes_with_report(source_cwd, worktree_cwd).copied
}

/// Like [`copy_worktree_includes`], returning detailed CoW vs standard copy metrics.
pub fn copy_worktree_includes_with_report(
    source_cwd: &Path,
    worktree_cwd: &Path,
) -> WorktreeIncludeCopyReport {
    let mut report = WorktreeIncludeCopyReport::default();
    let include_file = source_cwd.join(".worktreeinclude");
    let Ok(raw) = std::fs::read_to_string(&include_file) else {
        return report;
    };
    let Ok(canon_src) = std::fs::canonicalize(source_cwd) else {
        return report;
    };
    let patterns: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|l| !pattern_has_parent_traversal(l))
        .map(ToOwned::to_owned)
        .collect();
    if patterns.is_empty() {
        return report;
    }

    walk_and_copy_matching(
        &canon_src,
        &canon_src,
        worktree_cwd,
        &patterns,
        0,
        &mut report,
    );
    report
}

fn pattern_has_parent_traversal(pattern: &str) -> bool {
    Path::new(pattern)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
}

fn is_heavy_ignored_dir(name: &str) -> bool {
    matches!(name, "node_modules" | "target" | ".venv" | "venv")
}

fn patterns_may_target_inside(rel_dir: &str, dir_name: &str, patterns: &[String]) -> bool {
    let rel_prefix = format!("{rel_dir}/");
    let name_prefix = format!("{dir_name}/");
    patterns.iter().any(|pat| {
        let norm = pat.trim_start_matches('/').trim_end_matches('/');
        norm.starts_with(&rel_prefix)
            || norm.starts_with(&name_prefix)
            || norm.contains(&format!("/{dir_name}/"))
    })
}

fn record_copy_method(report: &mut WorktreeIncludeCopyReport, method: CopyMethod) {
    match method {
        CopyMethod::ApfsClone | CopyMethod::LinuxReflink => report.cow_cloned += 1,
        CopyMethod::Standard => report.standard_copied += 1,
    }
}

const MAX_WORKTREE_INCLUDE_WALK_DEPTH: usize = 6;
const MAX_FILTERED_COPY_DEPTH: usize = 32;

fn walk_and_copy_matching(
    root: &Path,
    dir: &Path,
    dest_root: &Path,
    patterns: &[String],
    depth: usize,
    report: &mut WorktreeIncludeCopyReport,
) {
    if depth > MAX_WORKTREE_INCLUDE_WALK_DEPTH || report.copied.len() >= 256 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if report.copied.len() >= 256 {
            return;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == ".git" {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        // Guard against traversal components
        if rel.components().any(|c| matches!(c, Component::ParentDir)) {
            continue;
        }
        let Ok(sym_meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let ft = sym_meta.file_type();
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        if ft.is_symlink() {
            // Never recurse into symlinked directories (prevents `..`/`.` symlink cycles).
            let Ok(resolved) = resolve_symlink_target_safe(&path, root) else {
                continue;
            };
            if !matches_any_pattern(&rel_str, name, patterns) {
                continue;
            }
            let target = dest_root.join(rel);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if recreate_validated_symlink(&path, &resolved, root, &target, report) {
                report.copied.push(rel_str);
            }
        } else if ft.is_dir() {
            if matches_any_pattern(&rel_str, name, patterns) {
                let target = dest_root.join(rel);
                match copy_dir_cow_within(root, &path, &target) {
                    Ok(method) => {
                        record_copy_method(report, method);
                        report.copied.push(rel_str);
                    }
                    Err(err) => {
                        warn!(
                            path = %rel_str,
                            error = %err,
                            "CoW directory copy rejected; falling back to filtered safe file copy"
                        );
                        copy_dir_filtered_cow(root, &path, &target, 0, report);
                    }
                }
            } else if !is_heavy_ignored_dir(name)
                || patterns_may_target_inside(&rel_str, name, patterns)
            {
                walk_and_copy_matching(root, &path, dest_root, patterns, depth + 1, report);
            }
        } else if ft.is_file() && matches_any_pattern(&rel_str, name, patterns) {
            let target = dest_root.join(rel);
            if let Ok(method) = copy_file_cow_within(root, &path, &target) {
                record_copy_method(report, method);
                report.copied.push(rel_str);
            }
        }
    }
}

/// Recreate an already-validated symlink safely without recursing into its target.
fn recreate_validated_symlink(
    src_link: &Path,
    resolved_target: &Path,
    root: &Path,
    dst: &Path,
    report: &mut WorktreeIncludeCopyReport,
) -> bool {
    #[cfg(unix)]
    {
        let _ = (resolved_target, root);
        let Ok(link_target) = std::fs::read_link(src_link) else {
            return false;
        };
        if let Ok(dst_meta) = std::fs::symlink_metadata(dst) {
            if dst_meta.file_type().is_dir() {
                return false;
            }
            let _ = std::fs::remove_file(dst);
        }
        if std::os::unix::fs::symlink(&link_target, dst).is_ok() {
            report.standard_copied += 1;
            return true;
        }
        false
    }
    #[cfg(not(unix))]
    {
        if resolved_target.is_file()
            && let Ok(method) = copy_file_cow_within(root, src_link, dst)
        {
            record_copy_method(report, method);
            return true;
        }
        false
    }
}

/// Safe per-entry fallback used only when a matched directory contains an escaping symlink that
/// caused whole-directory CoW validation to reject the tree.
fn copy_dir_filtered_cow(
    root: &Path,
    src_dir: &Path,
    dst_dir: &Path,
    depth: usize,
    report: &mut WorktreeIncludeCopyReport,
) {
    if depth > MAX_FILTERED_COPY_DEPTH || report.copied.len() >= 256 {
        return;
    }
    let _ = std::fs::create_dir_all(dst_dir);
    let Ok(entries) = std::fs::read_dir(src_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if report.copied.len() >= 256 {
            return;
        }
        let p = entry.path();
        let Ok(sym_meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        let ft = sym_meta.file_type();
        let target = dst_dir.join(entry.file_name());
        if ft.is_symlink() {
            let Ok(resolved) = resolve_symlink_target_safe(&p, root) else {
                continue;
            };
            if recreate_validated_symlink(&p, &resolved, root, &target, report)
                && let Ok(rel) = p.strip_prefix(root)
            {
                report.copied.push(rel.to_string_lossy().replace('\\', "/"));
            }
        } else if ft.is_dir() {
            copy_dir_filtered_cow(root, &p, &target, depth + 1, report);
        } else if ft.is_file()
            && let Ok(method) = copy_file_cow_within(root, &p, &target)
            && let Ok(rel) = p.strip_prefix(root)
        {
            record_copy_method(report, method);
            report.copied.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn matches_any_pattern(rel_path: &str, file_name: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        let norm = pat.trim_start_matches('/').trim_end_matches('/');
        if norm.is_empty() {
            continue;
        }
        if norm.contains('/') {
            if glob_match(norm, rel_path) {
                return true;
            }
        } else if glob_match(norm, file_name) || glob_match(norm, rel_path) {
            return true;
        }
    }
    false
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_p = Some(pi);
            star_t = ti;
            pi += 1;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn git_cmd_clean(cwd: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        .kill_on_drop(true)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR");
    cmd
}

async fn copy_worktree_includes_async(source_cwd: &Path, worktree_cwd: &Path) {
    let src = source_cwd.to_path_buf();
    let dst = worktree_cwd.to_path_buf();
    if let Ok(report) =
        tokio::task::spawn_blocking(move || copy_worktree_includes_with_report(&src, &dst)).await
        && !report.copied.is_empty()
    {
        debug!(
            entries = report.copied.len(),
            cow_cloned = report.cow_cloned,
            standard_copied = report.standard_copied,
            "Copied .worktreeinclude entries into worktree"
        );
    }
}

fn check_git_ref_lexically_safe(raw: &str, label: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("invalid_git_ref: {label} cannot be empty");
    }
    if trimmed.starts_with('-') {
        anyhow::bail!("invalid_git_ref: {label} `{trimmed}` cannot start with `-`");
    }
    if trimmed.ends_with('/') || trimmed.ends_with('.') || trimmed.ends_with(".lock") {
        anyhow::bail!("invalid_git_ref: {label} `{trimmed}` has an invalid suffix");
    }
    if trimmed.contains("..")
        || trimmed.contains("@{")
        || trimmed.contains("//")
        || trimmed.contains('\\')
        || trimmed.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        anyhow::bail!("invalid_git_ref: {label} `{trimmed}` contains forbidden characters");
    }
    Ok(trimmed.to_string())
}

/// Validate a user/agent-supplied git branch name against option injection and `git check-ref-format`.
pub async fn validate_git_branch_name(repo_cwd: &Path, raw: &str) -> anyhow::Result<String> {
    let trimmed = check_git_ref_lexically_safe(raw, "branch")?;
    if trimmed == "@"
        || trimmed
            .chars()
            .any(|c| matches!(c, '~' | '^' | ':' | '?' | '*' | '['))
    {
        anyhow::bail!("invalid_git_ref: branch `{trimmed}` contains forbidden characters");
    }
    let out = git_cmd_clean(repo_cwd)
        .args(["check-ref-format", "--branch", &trimmed])
        .output()
        .await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!(
            "invalid_git_ref: `{trimmed}` is not a valid git branch name ({})",
            stderr.trim()
        );
    }
    Ok(trimmed)
}

/// Validate a user/agent-supplied git base revision against option injection and verify it resolves
/// to an existing commit via `git rev-parse --verify --end-of-options <base_ref>^{commit}`.
pub async fn validate_git_base_ref(repo_cwd: &Path, raw: &str) -> anyhow::Result<String> {
    let trimmed = check_git_ref_lexically_safe(raw, "base_ref")?;
    let rev_spec = format!("{trimmed}^{{commit}}");
    let out = git_cmd_clean(repo_cwd)
        .args(["rev-parse", "--verify", "--end-of-options", &rev_spec])
        .output()
        .await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!(
            "invalid_git_ref: base_ref `{trimmed}` does not resolve to a valid commit ({})",
            stderr.trim()
        );
    }
    Ok(trimmed)
}

/// Create a git worktree supporting all 3 modes: `branch_off`, `checkout_branch`, `checkout_pr`.
pub async fn create_worktree_with_mode(
    orbit_home: &Path,
    source_cwd: &Path,
    mode: WorktreeCreateMode,
    branch_name: Option<&str>,
    base_ref: Option<&str>,
    pr_number: Option<u32>,
) -> anyhow::Result<(PathBuf, Option<String>)> {
    create_worktree_with_mode_pooled(
        orbit_home,
        source_cwd,
        mode,
        branch_name,
        base_ref,
        pr_number,
        None,
    )
    .await
}

/// Create a git worktree supporting all 3 modes, accelerating provisioning with an optional warm
/// detached-HEAD [`WorktreePool`] and CoW `.worktreeinclude` copying.
pub async fn create_worktree_with_mode_pooled(
    orbit_home: &Path,
    source_cwd: &Path,
    mode: WorktreeCreateMode,
    branch_name: Option<&str>,
    base_ref: Option<&str>,
    pr_number: Option<u32>,
    pool: Option<&Arc<WorktreePool>>,
) -> anyhow::Result<(PathBuf, Option<String>)> {
    let repo_name = source_cwd
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo");

    match mode {
        WorktreeCreateMode::BranchOff => {
            // Best-effort remote fetch (bounded timeout) so `origin/main` is fresh
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(8),
                git_cmd_clean(source_cwd)
                    .args(["fetch", "--quiet", "--prune"])
                    .output(),
            )
            .await;

            let branch = match branch_name.map(str::trim).filter(|b| !b.is_empty()) {
                Some(b) => validate_git_branch_name(source_cwd, b).await?,
                None => format!(
                    "orbit/task-{}",
                    &uuid::Uuid::new_v4().simple().to_string()[..6]
                ),
            };
            let validated_base = match base_ref.map(str::trim).filter(|b| !b.is_empty()) {
                Some(b) => Some(validate_git_base_ref(source_cwd, b).await?),
                None => None,
            };
            let target_dir = allocate_worktree_dir(orbit_home, repo_name, &branch);
            let claimed = if let Some(p) = pool {
                p.try_claim_branch_off(source_cwd, &target_dir, &branch, validated_base.as_deref())
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };

            if claimed.is_none() {
                if let Some(parent) = target_dir.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let mut cmd = git_cmd_clean(source_cwd);
                cmd.arg("worktree")
                    .arg("add")
                    .arg("-b")
                    .arg(&branch)
                    .arg(&target_dir);
                if let Some(ref base) = validated_base {
                    cmd.arg(base);
                }
                let out = cmd.output().await?;
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    anyhow::bail!("git worktree add failed: {stderr}");
                }
            }

            copy_worktree_includes_async(source_cwd, &target_dir).await;
            if let Some(p) = pool {
                p.schedule_replenish(source_cwd.to_path_buf());
            }
            Ok((target_dir, Some(branch)))
        }
        WorktreeCreateMode::CheckoutBranch => {
            let raw_branch = branch_name
                .map(str::trim)
                .filter(|b| !b.is_empty())
                .ok_or_else(|| anyhow::anyhow!("`branch` is required for checkout_branch mode"))?;
            let branch = validate_git_branch_name(source_cwd, raw_branch).await?;
            let target_dir = allocate_worktree_dir(orbit_home, repo_name, &branch);
            let claimed = if let Some(p) = pool {
                p.try_claim_checkout_branch(source_cwd, &target_dir, &branch)
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };

            if claimed.is_none() {
                if let Some(parent) = target_dir.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let out = git_cmd_clean(source_cwd)
                    .arg("worktree")
                    .arg("add")
                    .arg(&target_dir)
                    .arg(&branch)
                    .output()
                    .await?;
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    anyhow::bail!("git worktree checkout branch failed: {stderr}");
                }
            }

            copy_worktree_includes_async(source_cwd, &target_dir).await;
            if let Some(p) = pool {
                p.schedule_replenish(source_cwd.to_path_buf());
            }
            Ok((target_dir, Some(branch)))
        }
        WorktreeCreateMode::CheckoutPr => {
            let pr = pr_number
                .ok_or_else(|| anyhow::anyhow!("`pr_number` is required for checkout_pr mode"))?;
            let local_branch = format!("orbit/pr-{pr}");
            let fetch = git_cmd_clean(source_cwd)
                .args(["fetch", "origin", &format!("pull/{pr}/head:{local_branch}")])
                .output()
                .await?;
            if !fetch.status.success() {
                let stderr = String::from_utf8_lossy(&fetch.stderr);
                anyhow::bail!("git fetch PR #{pr} failed: {stderr}");
            }
            let target_dir = allocate_worktree_dir(orbit_home, repo_name, &local_branch);
            let claimed = if let Some(p) = pool {
                p.try_claim_checkout_branch(source_cwd, &target_dir, &local_branch)
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };

            if claimed.is_none() {
                if let Some(parent) = target_dir.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let out = git_cmd_clean(source_cwd)
                    .arg("worktree")
                    .arg("add")
                    .arg(&target_dir)
                    .arg(&local_branch)
                    .output()
                    .await?;
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    anyhow::bail!("git worktree add PR #{pr} failed: {stderr}");
                }
            }

            copy_worktree_includes_async(source_cwd, &target_dir).await;
            if let Some(p) = pool {
                p.schedule_replenish(source_cwd.to_path_buf());
            }
            Ok((target_dir, Some(local_branch)))
        }
    }
}

fn allocate_worktree_dir(orbit_home: &Path, repo_name: &str, branch: &str) -> PathBuf {
    let sanitized: String = branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let base = orbit_home
        .join("worktrees")
        .join(format!("{repo_name}-{sanitized}"));
    if !base.exists() {
        return base;
    }
    let suffix = &uuid::Uuid::new_v4().simple().to_string()[..4];
    orbit_home
        .join("worktrees")
        .join(format!("{repo_name}-{sanitized}-{suffix}"))
}

/// Run `worktree.setup` commands sequentially inside `worktree_cwd`, emitting progress callbacks.
pub async fn run_worktree_setup_hooks<F>(
    workspace_id: &str,
    source_cwd: &Path,
    worktree_cwd: &Path,
    branch_name: Option<&str>,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(WorkspaceSetupProgress),
{
    let manifest = load_workspace_manifest_with_fallback(worktree_cwd, Some(source_cwd));
    let cmds = manifest
        .worktree
        .setup
        .map(CommandOrList::into_commands)
        .unwrap_or_default();
    if cmds.is_empty() {
        return Ok(());
    }
    let total = cmds.len() as u32;
    let env_map = build_worktree_hook_env(source_cwd, worktree_cwd, branch_name);
    for (idx, cmd_str) in cmds.iter().enumerate() {
        on_progress(WorkspaceSetupProgress {
            workspace_id: workspace_id.to_string(),
            status: WorkspaceSetupStatus::Running,
            command: Some(cmd_str.clone()),
            step_index: (idx + 1) as u32,
            total_steps: total,
            output_tail: None,
            error: None,
        });
        #[cfg(unix)]
        let mut cmd = {
            let mut c = Command::new("/bin/sh");
            let shell_flag = if orbit_providers::apply_cached_shell_env(&mut c) {
                "-c"
            } else {
                "-lc"
            };
            c.arg(shell_flag).arg(cmd_str);
            c
        };
        #[cfg(not(unix))]
        let mut cmd = {
            let mut c = crate::platform::build_shell_command(cmd_str, false);
            orbit_providers::apply_cached_shell_env(&mut c);
            c
        };
        cmd.current_dir(worktree_cwd)
            .envs(&env_map)
            .kill_on_drop(true);
        let output = cmd.output().await;
        match output {
            Ok(out) if out.status.success() => {
                let tail = tail_output(&out.stdout, &out.stderr, 600);
                on_progress(WorkspaceSetupProgress {
                    workspace_id: workspace_id.to_string(),
                    status: WorkspaceSetupStatus::Running,
                    command: Some(cmd_str.clone()),
                    step_index: (idx + 1) as u32,
                    total_steps: total,
                    output_tail: tail,
                    error: None,
                });
            }
            Ok(out) => {
                let err = tail_output(&out.stdout, &out.stderr, 800)
                    .unwrap_or_else(|| format!("setup command exited with {}", out.status));
                on_progress(WorkspaceSetupProgress {
                    workspace_id: workspace_id.to_string(),
                    status: WorkspaceSetupStatus::Failed,
                    command: Some(cmd_str.clone()),
                    step_index: (idx + 1) as u32,
                    total_steps: total,
                    output_tail: Some(err.clone()),
                    error: Some(err.clone()),
                });
                return Err(err);
            }
            Err(e) => {
                let err = e.to_string();
                on_progress(WorkspaceSetupProgress {
                    workspace_id: workspace_id.to_string(),
                    status: WorkspaceSetupStatus::Failed,
                    command: Some(cmd_str.clone()),
                    step_index: (idx + 1) as u32,
                    total_steps: total,
                    output_tail: None,
                    error: Some(err.clone()),
                });
                return Err(err);
            }
        }
    }
    on_progress(WorkspaceSetupProgress {
        workspace_id: workspace_id.to_string(),
        status: WorkspaceSetupStatus::Succeeded,
        command: None,
        step_index: total,
        total_steps: total,
        output_tail: None,
        error: None,
    });
    Ok(())
}

/// Run `worktree.teardown` commands (best-effort) and remove the managed git worktree directory.
pub async fn teardown_and_remove_worktree(
    source_cwd: Option<&Path>,
    worktree_cwd: &Path,
    branch_name: Option<&str>,
) {
    if !worktree_cwd.exists() {
        return;
    }
    let manifest = load_workspace_manifest_with_fallback(worktree_cwd, source_cwd);
    let cmds = manifest
        .worktree
        .teardown
        .map(CommandOrList::into_commands)
        .unwrap_or_default();
    let src = source_cwd.unwrap_or(worktree_cwd);
    let env_map = build_worktree_hook_env(src, worktree_cwd, branch_name);
    for cmd_str in cmds {
        #[cfg(unix)]
        let mut cmd = {
            let mut c = Command::new("/bin/sh");
            let shell_flag = if orbit_providers::apply_cached_shell_env(&mut c) {
                "-c"
            } else {
                "-lc"
            };
            c.arg(shell_flag).arg(&cmd_str);
            c
        };
        #[cfg(not(unix))]
        let mut cmd = {
            let mut c = crate::platform::build_shell_command(&cmd_str, false);
            orbit_providers::apply_cached_shell_env(&mut c);
            c
        };
        cmd.current_dir(worktree_cwd)
            .envs(&env_map)
            .kill_on_drop(true);
        let res = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output()).await;
        if let Err(_) | Ok(Err(_)) = res {
            warn!(cmd = %cmd_str, "worktree teardown hook failed or timed out");
        }
    }

    if let Some(src_repo) = source_cwd {
        let out = git_cmd_clean(src_repo)
            .args([
                "worktree",
                "remove",
                "--force",
                &worktree_cwd.to_string_lossy(),
            ])
            .output()
            .await;
        if out.is_ok_and(|o| o.status.success()) {
            return;
        }
    }
    let _ = tokio::fs::remove_dir_all(worktree_cwd).await;
}

fn build_worktree_hook_env(
    source_cwd: &Path,
    worktree_cwd: &Path,
    branch_name: Option<&str>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let src_s = source_cwd.to_string_lossy().to_string();
    let wt_s = worktree_cwd.to_string_lossy().to_string();
    let br_s = branch_name.unwrap_or("").to_string();
    env.insert("ORBIT_SOURCE_CHECKOUT_PATH".into(), src_s.clone());
    env.insert("ORBIT_WORKTREE_PATH".into(), wt_s.clone());
    env.insert("ORBIT_BRANCH_NAME".into(), br_s.clone());
    // Paseo manifest compatibility aliases
    env.insert("PASEO_SOURCE_CHECKOUT_PATH".into(), src_s);
    env.insert("PASEO_WORKTREE_PATH".into(), wt_s);
    env.insert("PASEO_BRANCH_NAME".into(), br_s);
    env
}

fn tail_output(stdout: &[u8], stderr: &[u8], max_chars: usize) -> Option<String> {
    let mut combined = String::new();
    let so = String::from_utf8_lossy(stdout);
    let se = String::from_utf8_lossy(stderr);
    if !so.trim().is_empty() {
        combined.push_str(so.trim());
    }
    if !se.trim().is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(se.trim());
    }
    if combined.is_empty() {
        return None;
    }
    let total = combined.chars().count();
    if total <= max_chars {
        Some(combined)
    } else {
        Some(combined.chars().skip(total - max_chars).collect())
    }
}

/// Allocate an available ephemeral loopback TCP port.
pub fn allocate_ephemeral_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

pub fn sanitize_dns_label(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 28 {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "svc".to_string()
    } else {
        trimmed
    }
}

/// Build deterministic `*.localhost` hostname for a workspace service script.
pub fn build_service_proxy_host(
    script_name: &str,
    branch: Option<&str>,
    repo_name: &str,
) -> String {
    let s = sanitize_dns_label(script_name);
    let r = sanitize_dns_label(repo_name);
    match branch.filter(|b| !b.is_empty() && *b != "main" && *b != "master") {
        Some(b) => format!("{s}--{}--{r}.localhost", sanitize_dns_label(b)),
        None => format!("{s}--{r}.localhost"),
    }
}

#[derive(Debug, Clone)]
pub struct RunningScriptState {
    pub instance_id: Uuid,
    pub info: WorkspaceScriptInfo,
    pub proxy_host: Option<String>,
    cancel_tx: watch::Sender<bool>,
}

/// Live authorization handle bound to a specific running workspace service instance.
///
/// Automatically invalidated when the service stops, restarts, or its terminal exits so active
/// keep-alive tunnels and upgraded WebSockets cannot reconnect to an unrelated loopback process.
#[derive(Debug, Clone)]
pub struct AuthorizedPreviewService {
    pub instance_id: Uuid,
    pub workspace_id: String,
    pub script_name: String,
    pub target_port: u16,
    pub proxy_host: Option<String>,
    pub advertised_port: Option<u16>,
    cancel_rx: watch::Receiver<bool>,
}

impl AuthorizedPreviewService {
    pub fn cancel_receiver(&self) -> watch::Receiver<bool> {
        self.cancel_rx.clone()
    }
}

/// Expected client-facing preview authority (`Host` and `Origin` origin-tuple boundary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewAuthority {
    /// Direct daemon `*.localhost` reverse proxy (`<slug>.localhost[:expected_port]`).
    DotLocalhost {
        host: String,
        expected_port: Option<u16>,
    },
    /// Remote E2EE preview tunnel forwarded via a local loopback listener
    /// (`127.0.0.1:<port>`, `localhost:<port>`, `[::1]:<port>`, or `<slug>.localhost[:port]`).
    Tunnel { proxy_host: Option<String> },
}

/// Upstream TCP connect timeout for local workspace service proxying.
const SERVICE_PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Daemon-owned registry of running workspace scripts & `*.localhost` service reverse-proxy routes.
#[derive(Default)]
pub struct WorkspaceScriptRegistry {
    /// Keyed by `(workspace_id, script_name)`
    running: RwLock<HashMap<(String, String), RunningScriptState>>,
    /// Keyed by lowercase `Host` without port (e.g. `web--feat-auth--orbit.localhost`)
    /// -> `(workspace_id, script_name)` owning that route.
    proxy_routes: RwLock<HashMap<String, (String, String)>>,
}

impl WorkspaceScriptRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            running: RwLock::new(HashMap::new()),
            proxy_routes: RwLock::new(HashMap::new()),
        })
    }

    pub async fn list_for_workspace(
        &self,
        workspace_id: &str,
        cwd: &Path,
        source_cwd: Option<&Path>,
    ) -> Vec<WorkspaceScriptInfo> {
        let manifest = load_workspace_manifest_with_fallback(cwd, source_cwd);
        let running = self.running.read().await;
        let mut out = Vec::new();
        for (name, entry) in manifest.scripts {
            if let Some(active) = running.get(&(workspace_id.to_string(), name.clone())) {
                out.push(active.info.clone());
            } else {
                out.push(WorkspaceScriptInfo {
                    workspace_id: workspace_id.to_string(),
                    name,
                    command: entry.command.clone(),
                    kind: entry.script_kind(),
                    status: WorkspaceScriptStatus::Stopped,
                    terminal_id: None,
                    port: entry.port,
                    proxy_url: None,
                    last_error: None,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub async fn register_running(
        &self,
        workspace_id: &str,
        script_name: &str,
        info: WorkspaceScriptInfo,
        proxy_host: Option<String>,
    ) {
        let key = (workspace_id.to_string(), script_name.to_string());
        let (cancel_tx, _) = watch::channel(false);
        let new_state = RunningScriptState {
            instance_id: Uuid::new_v4(),
            info,
            proxy_host: proxy_host.map(|h| h.trim().to_ascii_lowercase()),
            cancel_tx,
        };

        let mut running = self.running.write().await;
        let mut routes = self.proxy_routes.write().await;
        if let Some(prev) = running.remove(&key) {
            let _ = prev.cancel_tx.send(true);
            if let Some(ref prev_host) = prev.proxy_host
                && routes.get(prev_host).is_some_and(|owner| owner == &key)
            {
                routes.remove(prev_host);
            }
        }
        if let (Some(host), Some(_port)) = (&new_state.proxy_host, new_state.info.port) {
            routes.insert(host.clone(), key.clone());
        }
        running.insert(key, new_state);
    }

    pub async fn unregister(
        &self,
        workspace_id: &str,
        script_name: &str,
    ) -> Option<RunningScriptState> {
        let key = (workspace_id.to_string(), script_name.to_string());
        let mut running = self.running.write().await;
        let mut routes = self.proxy_routes.write().await;
        let removed = running.remove(&key);
        if let Some(ref st) = removed {
            let _ = st.cancel_tx.send(true);
            if let Some(ref host) = st.proxy_host
                && routes.get(host).is_some_and(|owner| owner == &key)
            {
                routes.remove(host);
            }
        }
        removed
    }

    pub async fn unregister_by_terminal_id(&self, terminal_id: &str) -> Vec<RunningScriptState> {
        let mut running = self.running.write().await;
        let mut routes = self.proxy_routes.write().await;
        let matching_keys: Vec<(String, String)> = running
            .iter()
            .filter(|(_, st)| st.info.terminal_id.as_deref() == Some(terminal_id))
            .map(|(k, _)| k.clone())
            .collect();
        let mut removed = Vec::with_capacity(matching_keys.len());
        for key in matching_keys {
            if let Some(st) = running.remove(&key) {
                let _ = st.cancel_tx.send(true);
                if let Some(ref host) = st.proxy_host
                    && routes.get(host).is_some_and(|owner| owner == &key)
                {
                    routes.remove(host);
                }
                removed.push(st);
            }
        }
        removed
    }

    pub async fn unregister_all_for_workspace(
        &self,
        workspace_id: &str,
    ) -> Vec<RunningScriptState> {
        let keys: Vec<(String, String)> = self
            .running
            .read()
            .await
            .keys()
            .filter(|(ws, _)| ws == workspace_id)
            .cloned()
            .collect();
        let mut removed = Vec::new();
        for (ws, name) in keys {
            if let Some(st) = self.unregister(&ws, &name).await {
                removed.push(st);
            }
        }
        removed
    }

    /// Resolve a `Host` header to a live authorized `*.localhost` workspace service instance.
    pub async fn resolve_proxy_service(
        &self,
        host_header: &str,
    ) -> Option<AuthorizedPreviewService> {
        let (host_lower, _) = parse_http_host_header(host_header)?;
        let running = self.running.read().await;
        let routes = self.proxy_routes.read().await;
        let key = routes.get(&host_lower)?;
        let st = running.get(key)?;
        if st.info.kind != WorkspaceScriptKind::Service
            || st.info.status != WorkspaceScriptStatus::Running
        {
            return None;
        }
        let target_port = st.info.port?;
        Some(AuthorizedPreviewService {
            instance_id: st.instance_id,
            workspace_id: key.0.clone(),
            script_name: key.1.clone(),
            target_port,
            proxy_host: st.proxy_host.clone(),
            advertised_port: st
                .info
                .proxy_url
                .as_deref()
                .and_then(parse_nonzero_url_port),
            cancel_rx: st.cancel_tx.subscribe(),
        })
    }

    pub async fn resolve_proxy_port(&self, host_header: &str) -> Option<u16> {
        self.resolve_proxy_service(host_header)
            .await
            .map(|auth| auth.target_port)
    }

    /// Strictly validate whether a remote preview tunnel may target `(target_port, proxy_host)`
    /// and bind authorization to the exact running service instance.
    ///
    /// Security boundary: ONLY loopback ports owned by currently running registered workspace
    /// services on this daemon are permitted — zero arbitrary localhost SSRF.
    pub async fn resolve_allowed_preview_service(
        &self,
        target_port: u16,
        proxy_host: Option<&str>,
    ) -> Option<AuthorizedPreviewService> {
        if let Some(raw_host) = proxy_host.map(str::trim).filter(|s| !s.is_empty()) {
            let auth = self.resolve_proxy_service(raw_host).await?;
            if target_port != 0 && auth.target_port != target_port {
                return None;
            }
            return Some(auth);
        }
        if target_port == 0 {
            return None;
        }
        let running = self.running.read().await;
        running.iter().find_map(|((ws, name), st)| {
            (st.info.kind == WorkspaceScriptKind::Service
                && st.info.status == WorkspaceScriptStatus::Running
                && st.info.port == Some(target_port))
            .then(|| AuthorizedPreviewService {
                instance_id: st.instance_id,
                workspace_id: ws.clone(),
                script_name: name.clone(),
                target_port,
                proxy_host: st.proxy_host.clone(),
                advertised_port: st
                    .info
                    .proxy_url
                    .as_deref()
                    .and_then(parse_nonzero_url_port),
                cancel_rx: st.cancel_tx.subscribe(),
            })
        })
    }

    pub async fn resolve_allowed_preview_port(
        &self,
        target_port: u16,
        proxy_host: Option<&str>,
    ) -> Option<u16> {
        self.resolve_allowed_preview_service(target_port, proxy_host)
            .await
            .map(|auth| auth.target_port)
    }

    /// Re-verify that the exact authorized service instance is still running and owns its port.
    pub async fn is_service_instance_live(&self, auth: &AuthorizedPreviewService) -> bool {
        if *auth.cancel_rx.borrow() {
            return false;
        }
        let running = self.running.read().await;
        running
            .get(&(auth.workspace_id.clone(), auth.script_name.clone()))
            .is_some_and(|st| {
                st.instance_id == auth.instance_id
                    && st.info.kind == WorkspaceScriptKind::Service
                    && st.info.status == WorkspaceScriptStatus::Running
                    && st.info.port == Some(auth.target_port)
            })
    }

    /// Forward an HTTP/SSE or RFC 6455 WebSocket upgrade request to a registered local service
    /// with strict authority validation, live service re-verification, and zero-buffering streaming.
    pub async fn proxy_http_request_authorized(
        &self,
        auth: &AuthorizedPreviewService,
        authority: &PreviewAuthority,
        req: Request<Body>,
    ) -> Response<Body> {
        if !self.is_service_instance_live(auth).await {
            return proxy_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Preview service is no longer running",
            );
        }
        if is_websocket_upgrade_request(req.headers()) {
            self.proxy_websocket_upgrade(auth, authority, req).await
        } else {
            self.proxy_streaming_http(auth, authority, req).await
        }
    }

    /// Serve an arbitrary bidirectional byte stream (e.g. a remote E2EE preview tunnel) as an
    /// HTTP/1.1 + WebSocket-upgrade reverse proxy strictly pinned to a live workspace service
    /// instance. Re-verifies service ownership before every upstream connect and immediately
    /// aborts the tunnel when the service stops or exits.
    pub async fn serve_preview_tunnel_io<IO>(
        self: Arc<Self>,
        auth: AuthorizedPreviewService,
        io: IO,
    ) where
        IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let authority = PreviewAuthority::Tunnel {
            proxy_host: auth.proxy_host.clone(),
        };
        let mut cancel_rx = auth.cancel_receiver();
        let scripts = Arc::clone(&self);
        let svc_auth = auth.clone();
        let svc = hyper::service::service_fn(move |req: Request<hyper::body::Incoming>| {
            let scripts = Arc::clone(&scripts);
            let auth = svc_auth.clone();
            let authority = authority.clone();
            async move {
                let mapped = req.map(Body::new);
                Ok::<_, Infallible>(
                    scripts
                        .proxy_http_request_authorized(&auth, &authority, mapped)
                        .await,
                )
            }
        });
        let conn = hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(io), svc)
            .with_upgrades();
        tokio::select! {
            res = conn => {
                if let Err(err) = res {
                    debug!(
                        target_port = auth.target_port,
                        error = %err,
                        "Remote preview tunnel HTTP/WS connection ended"
                    );
                }
            }
            _ = wait_service_cancelled(&mut cancel_rx) => {
                debug!(
                    target_port = auth.target_port,
                    workspace_id = %auth.workspace_id,
                    script_name = %auth.script_name,
                    "Remote preview tunnel cancelled because backing service exited"
                );
            }
        }
    }

    async fn proxy_websocket_upgrade(
        &self,
        auth: &AuthorizedPreviewService,
        authority: &PreviewAuthority,
        mut req: Request<Body>,
    ) -> Response<Body> {
        let port = auth.target_port;
        let client_upgrade = hyper::upgrade::on(&mut req);

        // Validate Host/Origin and strip Forwarded/X-Forwarded-* BEFORE touching loopback.
        let upstream_req = match build_upstream_request(req, port, authority, true) {
            Ok(r) => r,
            Err((status, msg)) => return proxy_error_response(status, &msg),
        };

        // Re-verify live service ownership immediately before upstream TCP connect.
        if !self.is_service_instance_live(auth).await {
            return proxy_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Preview service is no longer running",
            );
        }

        let tcp = match connect_local_upstream(port).await {
            Ok(s) => s,
            Err(resp) => return resp,
        };

        let (mut sender, conn) =
            match hyper::client::conn::http1::handshake(TokioIo::new(tcp)).await {
                Ok(pair) => pair,
                Err(err) => {
                    return bad_gateway_response(
                        port,
                        &format!("WebSocket upstream handshake failed: {err}"),
                    );
                }
            };
        tokio::spawn(async move {
            if let Err(err) = conn.with_upgrades().await {
                debug!(port, error = %err, "WebSocket proxy upstream connection closed");
            }
        });

        let mut upstream_resp = match sender.send_request(upstream_req).await {
            Ok(r) => r,
            Err(err) => {
                return bad_gateway_response(
                    port,
                    &format!("WebSocket upgrade request failed: {err}"),
                );
            }
        };

        if upstream_resp.status() != StatusCode::SWITCHING_PROTOCOLS {
            return build_streaming_downstream_response(upstream_resp);
        }

        let upstream_upgrade = hyper::upgrade::on(&mut upstream_resp);
        let mut resp_builder = Response::builder().status(StatusCode::SWITCHING_PROTOCOLS);
        for (name, value) in upstream_resp.headers() {
            resp_builder = resp_builder.header(name, value);
        }

        let mut cancel_rx = auth.cancel_receiver();
        tokio::spawn(async move {
            match tokio::try_join!(client_upgrade, upstream_upgrade) {
                Ok((client_upgraded, upstream_upgraded)) => {
                    let mut client_io = TokioIo::new(client_upgraded);
                    let mut upstream_io = TokioIo::new(upstream_upgraded);
                    tokio::select! {
                        res = tokio::io::copy_bidirectional(&mut client_io, &mut upstream_io) => {
                            if let Err(err) = res {
                                debug!(port, error = %err, "WebSocket proxy tunnel terminated");
                            }
                        }
                        _ = wait_service_cancelled(&mut cancel_rx) => {
                            debug!(port, "WebSocket proxy tunnel aborted because backing service exited");
                        }
                    }
                    let _ = client_io.shutdown().await;
                    let _ = upstream_io.shutdown().await;
                }
                Err(err) => {
                    debug!(port, error = %err, "WebSocket upgrade completion failed");
                }
            }
        });

        resp_builder
            .body(Body::empty())
            .unwrap_or_else(|_| Response::new(Body::empty()))
    }

    async fn proxy_streaming_http(
        &self,
        auth: &AuthorizedPreviewService,
        authority: &PreviewAuthority,
        req: Request<Body>,
    ) -> Response<Body> {
        let port = auth.target_port;
        // Validate Host/Origin and strip Forwarded/X-Forwarded-* BEFORE touching loopback.
        let upstream_req = match build_upstream_request(req, port, authority, false) {
            Ok(r) => r,
            Err((status, msg)) => return proxy_error_response(status, &msg),
        };

        // Re-verify live service ownership immediately before upstream TCP connect.
        if !self.is_service_instance_live(auth).await {
            return proxy_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Preview service is no longer running",
            );
        }

        let tcp = match connect_local_upstream(port).await {
            Ok(s) => s,
            Err(resp) => return resp,
        };

        let (mut sender, conn) = match hyper::client::conn::http1::handshake(TokioIo::new(tcp))
            .await
        {
            Ok(pair) => pair,
            Err(err) => {
                return bad_gateway_response(port, &format!("HTTP upstream init failed: {err}"));
            }
        };
        tokio::spawn(async move {
            if let Err(err) = conn.await {
                debug!(port, error = %err, "HTTP proxy upstream connection closed");
            }
        });

        match sender.send_request(upstream_req).await {
            Ok(upstream_resp) => build_streaming_downstream_response(upstream_resp),
            Err(err) => bad_gateway_response(port, &err.to_string()),
        }
    }
}

async fn wait_service_cancelled(cancel_rx: &mut watch::Receiver<bool>) {
    if *cancel_rx.borrow() {
        return;
    }
    while cancel_rx.changed().await.is_ok() {
        if *cancel_rx.borrow() {
            return;
        }
    }
}

fn parse_nonzero_url_port(url: &str) -> Option<u16> {
    let parsed = reqwest::Url::parse(url).ok()?;
    parsed.port().filter(|&p| p > 0)
}

/// Strictly parse an HTTP `Host` header into `(lowercase_host, optional_nonzero_port)`.
pub(crate) fn parse_http_host_header(raw: &str) -> Option<(String, Option<u16>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed != raw
        || trimmed.chars().any(|c| {
            c.is_ascii_whitespace() || c.is_control() || matches!(c, '/' | '?' | '#' | '@' | '\\')
        })
    {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix('[') {
        let close = rest.find(']')?;
        let host_part = &rest[..close];
        if host_part.is_empty() {
            return None;
        }
        let after = &rest[close + 1..];
        let port = if after.is_empty() {
            None
        } else {
            let p_str = after.strip_prefix(':')?;
            if p_str.is_empty() || !p_str.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let p: u16 = p_str.parse().ok()?;
            if p == 0 {
                return None;
            }
            Some(p)
        };
        return Some((host_part.to_ascii_lowercase(), port));
    }

    let (host_part, port) = if let Some((h, p_str)) = trimmed.split_once(':') {
        if p_str.is_empty() || p_str.contains(':') || !p_str.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let p: u16 = p_str.parse().ok()?;
        if p == 0 {
            return None;
        }
        (h, Some(p))
    } else {
        (trimmed, None)
    };

    if host_part.is_empty()
        || host_part.ends_with('.')
        || !host_part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
    {
        return None;
    }
    Some((host_part.to_ascii_lowercase(), port))
}

fn validate_single_host_authority(
    headers: &HeaderMap,
    authority: &PreviewAuthority,
) -> Result<(String, u16), (StatusCode, String)> {
    let mut host_iter = headers.get_all(header::HOST).iter();
    let Some(first_host) = host_iter.next() else {
        return Err((StatusCode::BAD_REQUEST, "Missing Host header".to_string()));
    };
    if host_iter.next().is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Multiple Host headers are not allowed".to_string(),
        ));
    }
    let Ok(raw_host) = first_host.to_str() else {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid non-UTF8 Host header".to_string(),
        ));
    };
    let Some((host_lower, opt_port)) = parse_http_host_header(raw_host) else {
        return Err((StatusCode::BAD_REQUEST, "Malformed Host header".to_string()));
    };
    let effective_port = opt_port.unwrap_or(80);

    match authority {
        PreviewAuthority::DotLocalhost {
            host: expected_host,
            expected_port,
        } => {
            if host_lower != expected_host.to_ascii_lowercase() {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Host header does not match preview authority".to_string(),
                ));
            }
            if let Some(exp_p) = *expected_port
                && exp_p > 0
                && effective_port != exp_p
            {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Host port does not match advertised preview authority".to_string(),
                ));
            }
            Ok((host_lower, effective_port))
        }
        PreviewAuthority::Tunnel { proxy_host } => {
            let matches_loopback = matches!(host_lower.as_str(), "127.0.0.1" | "localhost" | "::1");
            let matches_proxy_host = proxy_host
                .as_deref()
                .is_some_and(|expected| host_lower == expected.to_ascii_lowercase());
            if !matches_loopback && !matches_proxy_host {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Host header is not permitted for this preview tunnel".to_string(),
                ));
            }
            Ok((host_lower, effective_port))
        }
    }
}

async fn connect_local_upstream(port: u16) -> Result<TcpStream, Response<Body>> {
    match tokio::time::timeout(
        SERVICE_PROXY_CONNECT_TIMEOUT,
        TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    {
        Ok(Ok(stream)) => {
            let _ = stream.set_nodelay(true);
            Ok(stream)
        }
        Ok(Err(err)) => Err(bad_gateway_response(port, &err.to_string())),
        Err(_) => Err(bad_gateway_response(
            port,
            "timed out connecting to upstream service",
        )),
    }
}

fn proxy_error_response(status: StatusCode, detail: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::from(detail.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn bad_gateway_response(port: u16, detail: &str) -> Response<Body> {
    proxy_error_response(
        StatusCode::BAD_GATEWAY,
        &format!("Orbit service proxy upstream error on 127.0.0.1:{port}: {detail}"),
    )
}

fn header_contains_token(headers: &HeaderMap, name: &HeaderName, token: &str) -> bool {
    headers.get_all(name).iter().any(|val| {
        val.to_str().ok().is_some_and(|s| {
            s.split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(token))
        })
    })
}

pub(crate) fn is_websocket_upgrade_request(headers: &HeaderMap) -> bool {
    header_contains_token(headers, &header::UPGRADE, "websocket")
        && header_contains_token(headers, &header::CONNECTION, "upgrade")
}

fn collect_connection_header_tokens(headers: &HeaderMap) -> HashSet<String> {
    let mut tokens = HashSet::new();
    for val in headers.get_all(header::CONNECTION) {
        if let Ok(s) = val.to_str() {
            for part in s.split(',') {
                let norm = part.trim().to_ascii_lowercase();
                if !norm.is_empty() {
                    tokens.insert(norm);
                }
            }
        }
    }
    tokens
}

fn is_hop_by_hop_header(name_lower: &str, connection_tokens: &HashSet<String>) -> bool {
    matches!(
        name_lower,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
    ) || connection_tokens.contains(name_lower)
}

/// Safely rewrite `Origin` to `http://localhost:<target_port>` ONLY when it matches the exact
/// validated preview authority origin tuple `(http, authority_host, authority_port)`.
///
/// Sibling loopback ports, other `*.localhost` subdomains, HTTPS mismatches, and external origins
/// are NEVER rewritten.
pub(crate) fn rewrite_preview_origin_for_upstream(
    raw_origin: &str,
    authority_host: &str,
    authority_port: u16,
    target_port: u16,
) -> Option<String> {
    let trimmed = raw_origin.trim();
    if trimmed.is_empty() || trimmed != raw_origin {
        return None;
    }
    let parsed = reqwest::Url::parse(trimmed).ok()?;
    if parsed.scheme() != "http" {
        return None;
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !matches!(parsed.path(), "" | "/")
    {
        return None;
    }
    let origin_host = parsed
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    let origin_port = parsed.port_or_known_default()?;
    if origin_host == authority_host.to_ascii_lowercase() && origin_port == authority_port {
        Some(format!("http://localhost:{target_port}"))
    } else {
        None
    }
}

fn build_upstream_request(
    req: Request<Body>,
    port: u16,
    authority: &PreviewAuthority,
    preserve_upgrade_headers: bool,
) -> Result<Request<Body>, (StatusCode, String)> {
    let (parts, body) = req.into_parts();
    let (auth_host, auth_port) = validate_single_host_authority(&parts.headers, authority)?;

    if parts.headers.get_all(header::ORIGIN).iter().count() > 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Multiple Origin headers are not allowed".to_string(),
        ));
    }

    let path_q = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let upstream_uri: Uri = path_q.parse().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid upstream request URI `{path_q}`: {e}"),
        )
    })?;

    let mut builder = Request::builder()
        .method(parts.method)
        .uri(upstream_uri)
        .version(axum::http::Version::HTTP_11);

    let connection_tokens = if preserve_upgrade_headers {
        HashSet::new()
    } else {
        collect_connection_header_tokens(&parts.headers)
    };

    for (name, value) in &parts.headers {
        let lower = name.as_str().to_ascii_lowercase();
        if lower == "host" || lower == "forwarded" || lower.starts_with("x-forwarded-") {
            continue;
        }
        if !preserve_upgrade_headers && is_hop_by_hop_header(&lower, &connection_tokens) {
            continue;
        }
        if lower == "origin" {
            let Ok(raw_origin) = value.to_str() else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Invalid non-UTF8 Origin header".to_string(),
                ));
            };
            if let Some(rewritten) =
                rewrite_preview_origin_for_upstream(raw_origin, &auth_host, auth_port, port)
            {
                if let Ok(hv) = HeaderValue::from_str(&rewritten) {
                    builder = builder.header(header::ORIGIN, hv);
                }
                continue;
            }
            if preserve_upgrade_headers {
                return Err((
                    StatusCode::FORBIDDEN,
                    "WebSocket Origin does not match preview authority".to_string(),
                ));
            }
            // For non-upgrade HTTP requests, never spoof a mismatched Origin as localhost.
            builder = builder.header(name, value);
            continue;
        }
        builder = builder.header(name, value);
    }

    if let Ok(host_hv) = HeaderValue::from_str(&format!("localhost:{port}")) {
        builder = builder.header(header::HOST, host_hv);
    }

    let final_body = if preserve_upgrade_headers {
        Body::empty()
    } else {
        body
    };
    builder.body(final_body).map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!(
                "Orbit service proxy upstream error on 127.0.0.1:{port}: failed to build upstream proxy request: {e}"
            ),
        )
    })
}

fn build_streaming_downstream_response(
    upstream_resp: Response<hyper::body::Incoming>,
) -> Response<Body> {
    let (parts, body) = upstream_resp.into_parts();
    let connection_tokens = collect_connection_header_tokens(&parts.headers);
    let is_sse = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.to_ascii_lowercase().contains("text/event-stream"));

    let mut resp = Response::builder().status(parts.status);
    let mut has_cache_control = false;
    for (name, value) in &parts.headers {
        let lower = name.as_str().to_ascii_lowercase();
        if is_hop_by_hop_header(&lower, &connection_tokens) {
            continue;
        }
        if lower == "cache-control" {
            has_cache_control = true;
        }
        resp = resp.header(name, value);
    }
    if is_sse && !has_cache_control {
        resp = resp.header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    resp.body(Body::new(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_worktreeinclude_and_blocks_symlink_escape() {
        let root =
            std::env::temp_dir().join(format!("orbit_wt_inc_{}", uuid::Uuid::new_v4().simple()));
        let src = root.join("src");
        let dst = root.join("dst");
        let outside = root.join("secret.txt");
        std::fs::create_dir_all(src.join("config")).expect("mkdir src");
        std::fs::create_dir_all(&dst).expect("mkdir dst");
        std::fs::write(&outside, "TOP_SECRET").expect("write outside");
        std::fs::write(src.join(".worktreeinclude"), ".env*\nconfig/*.local.json\n").expect("inc");
        std::fs::write(src.join(".env.local"), "API=123").expect("env");
        std::fs::write(src.join("config/app.local.json"), "{\"k\":1}").expect("cfg");
        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(&outside, src.join(".env.escape"));
        }

        let copied = copy_worktree_includes(&src, &dst);
        assert!(copied.iter().any(|p| p == ".env.local"));
        assert!(copied.iter().any(|p| p == "config/app.local.json"));
        assert!(!dst.join(".env.escape").exists());
        assert_eq!(
            std::fs::read_to_string(dst.join(".env.local")).expect("read dst env"),
            "API=123"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn loads_orbit_json_and_falls_back_to_paseo_json() {
        let dir =
            std::env::temp_dir().join(format!("orbit_manifest_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(
            dir.join("paseo.json"),
            r#"{"worktree":{"setup":"echo hi"},"scripts":{"web":{"type":"service","command":"npm run dev","port":3000}}}"#,
        )
        .expect("write paseo.json");
        let m1 = load_workspace_manifest(&dir);
        assert_eq!(m1.scripts.len(), 1);
        assert_eq!(
            m1.scripts["web"].script_kind(),
            WorkspaceScriptKind::Service
        );
        assert_eq!(m1.scripts["web"].port, Some(3000));

        // orbit.json takes precedence when present
        std::fs::write(
            dir.join("orbit.json"),
            r#"{"scripts":{"api":{"type":"service","command":"cargo run"}}}"#,
        )
        .expect("write orbit.json");
        let m2 = load_workspace_manifest(&dir);
        assert!(m2.scripts.contains_key("api"));
        assert!(!m2.scripts.contains_key("web"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn formats_deterministic_proxy_host() {
        assert_eq!(
            build_service_proxy_host("web", Some("main"), "orbit"),
            "web--orbit.localhost"
        );
        assert_eq!(
            build_service_proxy_host("web", Some("feat/auth_v2"), "orbit"),
            "web--feat-auth-v2--orbit.localhost"
        );
    }

    #[test]
    fn copies_worktreeinclude_cow_dirs_like_node_modules_and_venv_safely() {
        let root = std::env::temp_dir().join(format!(
            "orbit_wt_cow_inc_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let src = root.join("src");
        let dst = root.join("dst");
        let outside = root.join("outside.key");
        std::fs::create_dir_all(src.join("node_modules/pkg/bin")).expect("mkdir node_modules");
        std::fs::create_dir_all(src.join("node_modules/.bin")).expect("mkdir .bin");
        std::fs::create_dir_all(src.join(".venv/lib")).expect("mkdir .venv");
        std::fs::create_dir_all(src.join("mixed")).expect("mkdir mixed");
        std::fs::create_dir_all(&dst).expect("mkdir dst");
        std::fs::write(&outside, "SECRET_KEY").expect("write outside");
        std::fs::write(
            src.join(".worktreeinclude"),
            ".env.local\nnode_modules\n.venv/\nmixed\n../outside.key\n",
        )
        .expect("write include");
        std::fs::write(src.join(".env.local"), "PORT=8080").expect("write env");
        std::fs::write(src.join("node_modules/pkg/index.js"), "exports.ok = true;")
            .expect("write pkg");
        std::fs::write(src.join("node_modules/pkg/bin/tool.js"), "console.log(1);")
            .expect("write tool");
        std::fs::write(src.join(".venv/lib/site.py"), "VERSION = 1\n").expect("write site.py");
        std::fs::write(src.join("mixed/safe.txt"), "safe-data").expect("write safe");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("../pkg/bin/tool.js", src.join("node_modules/.bin/tool"))
                .expect("internal bin symlink");
            std::os::unix::fs::symlink(&outside, src.join("mixed/escape.key"))
                .expect("escape symlink");
        }

        let report = copy_worktree_includes_with_report(&src, &dst);
        assert!(report.copied.iter().any(|p| p == ".env.local"));
        assert!(report.copied.iter().any(|p| p == "node_modules"));
        assert!(report.copied.iter().any(|p| p == ".venv"));
        assert_eq!(
            std::fs::read_to_string(dst.join(".env.local")).expect("read env"),
            "PORT=8080"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("node_modules/pkg/index.js")).expect("read pkg"),
            "exports.ok = true;"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join(".venv/lib/site.py")).expect("read venv"),
            "VERSION = 1\n"
        );
        #[cfg(unix)]
        {
            assert!(
                std::fs::symlink_metadata(dst.join("node_modules/.bin/tool"))
                    .expect("bin symlink")
                    .file_type()
                    .is_symlink()
            );
            // Filtered fallback preserved safe file in `mixed` while blocking the escaping symlink
            assert_eq!(
                std::fs::read_to_string(dst.join("mixed/safe.txt")).expect("read mixed safe"),
                "safe-data"
            );
            assert!(!dst.join("mixed/escape.key").exists());
        }
        assert!(!dst.join("outside.key").exists());

        #[cfg(target_os = "macos")]
        assert!(
            report.cow_cloned >= 3,
            "expected APFS CoW clones for .env.local, node_modules, .venv; got {:?}",
            report
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn create_worktree_with_mode_pooled_uses_warm_pool_replenishes_and_copies_cow_includes() {
        use crate::worktree_pool::WorktreePoolConfig;

        let root = std::env::temp_dir().join(format!(
            "orbit_wt_pooled_e2e_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let home = root.join("orbit_home");
        let repo = root.join("sample-app");
        std::fs::create_dir_all(repo.join("node_modules/dep")).expect("mkdir repo");

        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "orbit@example.com"],
            vec!["config", "user.name", "Orbit"],
        ] {
            let out = git_cmd_clean(&repo)
                .args(&args)
                .output()
                .await
                .expect("git init");
            assert!(out.status.success());
        }
        std::fs::write(repo.join(".gitignore"), "node_modules/\n.env.local\n").expect("gitignore");
        std::fs::write(repo.join(".worktreeinclude"), ".env.local\nnode_modules\n")
            .expect("worktreeinclude");
        std::fs::write(repo.join("README.md"), "hello\n").expect("readme");
        std::fs::write(repo.join(".env.local"), "SECRET=dev").expect("env");
        std::fs::write(repo.join("node_modules/dep/index.js"), "module.exports=1;").expect("dep");

        for args in [
            vec!["add", ".gitignore", ".worktreeinclude", "README.md"],
            vec!["commit", "-m", "init"],
            vec!["branch", "existing-feat"],
        ] {
            let out = git_cmd_clean(&repo)
                .args(&args)
                .output()
                .await
                .expect("git setup");
            assert!(out.status.success());
        }

        let pool = WorktreePool::new(home.clone(), WorktreePoolConfig::with_max_standby(2));
        assert_eq!(pool.prewarm_repo(&repo).await.expect("prewarm"), 2);

        // 1. Warm-claimed BranchOff worktree
        let (wt1, br1) = create_worktree_with_mode_pooled(
            &home,
            &repo,
            WorktreeCreateMode::BranchOff,
            Some("feat/warm-one"),
            Some("main"),
            None,
            Some(&pool),
        )
        .await
        .expect("create pooled branch_off");
        assert_eq!(br1.as_deref(), Some("feat/warm-one"));
        assert_eq!(
            std::fs::read_to_string(wt1.join(".env.local")).expect("env copied"),
            "SECRET=dev"
        );
        assert_eq!(
            std::fs::read_to_string(wt1.join("node_modules/dep/index.js")).expect("nm copied"),
            "module.exports=1;"
        );

        // 2. Warm-claimed CheckoutBranch worktree
        let (wt2, br2) = create_worktree_with_mode_pooled(
            &home,
            &repo,
            WorktreeCreateMode::CheckoutBranch,
            Some("existing-feat"),
            None,
            None,
            Some(&pool),
        )
        .await
        .expect("create pooled checkout_branch");
        assert_eq!(br2.as_deref(), Some("existing-feat"));
        assert!(wt2.join("node_modules/dep/index.js").is_file());

        // 3. Shutdown pool cleanup + verify fallback still creates a worktree cleanly
        let _ = pool.shutdown_cleanup().await;
        let (wt3, br3) = create_worktree_with_mode_pooled(
            &home,
            &repo,
            WorktreeCreateMode::BranchOff,
            Some("feat/cold-fallback"),
            Some("main"),
            None,
            Some(&pool),
        )
        .await
        .expect("cold fallback branch_off");
        assert_eq!(br3.as_deref(), Some("feat/cold-fallback"));
        assert_eq!(
            std::fs::read_to_string(wt3.join(".env.local")).expect("env copied cold"),
            "SECRET=dev"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn worktreeinclude_internal_parent_and_self_symlinks_do_not_recurse_infinitely() {
        let root = std::env::temp_dir().join(format!(
            "orbit_wt_symlink_loop_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let src = root.join("src");
        let dst = root.join("dst");
        let outside = root.join("outside.secret");
        std::fs::create_dir_all(src.join("mixed/sub")).expect("mkdir mixed/sub");
        std::fs::create_dir_all(&dst).expect("mkdir dst");
        std::fs::write(&outside, "SECRET").expect("write outside");
        std::fs::write(src.join(".worktreeinclude"), "mixed\n").expect("write include");
        std::fs::write(src.join("mixed/sub/ok.txt"), "safe-payload").expect("write ok");

        // Top-level internal loop symlink in root (exercising walk_and_copy_matching)
        std::os::unix::fs::symlink(".", src.join("self_root")).expect("top self symlink");
        // Inside `mixed`: an escaping symlink forces fallback into `copy_dir_filtered_cow`,
        // alongside internal symlinks pointing to `.` and `..`.
        std::os::unix::fs::symlink(&outside, src.join("mixed/escape")).expect("escape symlink");
        std::os::unix::fs::symlink(".", src.join("mixed/self_loop")).expect("self loop symlink");
        std::os::unix::fs::symlink("..", src.join("mixed/sub/parent_loop"))
            .expect("parent loop symlink");

        let report = copy_worktree_includes_with_report(&src, &dst);
        assert_eq!(
            std::fs::read_to_string(dst.join("mixed/sub/ok.txt")).expect("read safe payload"),
            "safe-payload"
        );
        assert!(!dst.join("mixed/escape").exists());
        assert!(
            std::fs::symlink_metadata(dst.join("mixed/self_loop"))
                .expect("self_loop recreated as symlink")
                .file_type()
                .is_symlink()
        );
        assert!(
            std::fs::symlink_metadata(dst.join("mixed/sub/parent_loop"))
                .expect("parent_loop recreated as symlink")
                .file_type()
                .is_symlink()
        );
        assert!(
            report.copied.len() < 256,
            "symlink loops must not inflate copy count: {:?}",
            report.copied
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn origin_rewrite_matches_exact_preview_authority_and_rejects_siblings() {
        // Exact authority match (`*.localhost:<port>` and local tunnel `127.0.0.1:<port>`).
        assert_eq!(
            rewrite_preview_origin_for_upstream(
                "http://web--feat--orbit.localhost:6868",
                "web--feat--orbit.localhost",
                6868,
                5173,
            )
            .as_deref(),
            Some("http://localhost:5173")
        );
        assert_eq!(
            rewrite_preview_origin_for_upstream(
                "http://127.0.0.1:54321",
                "127.0.0.1",
                54321,
                3000,
            )
            .as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(
            rewrite_preview_origin_for_upstream("http://[::1]:6868", "::1", 6868, 8080).as_deref(),
            Some("http://localhost:8080")
        );

        // Sibling loopback port MUST be rejected (no cross-port localhost origin spoofing!).
        assert_eq!(
            rewrite_preview_origin_for_upstream("http://127.0.0.1:9999", "127.0.0.1", 54321, 3000,),
            None
        );
        assert_eq!(
            rewrite_preview_origin_for_upstream(
                "http://127.0.0.1:6868",
                "web--feat--orbit.localhost",
                6868,
                5173,
            ),
            None
        );
        // Sibling `*.localhost` subdomain or mismatched port MUST be rejected!
        assert_eq!(
            rewrite_preview_origin_for_upstream(
                "http://other--feat--orbit.localhost:6868",
                "web--feat--orbit.localhost",
                6868,
                5173,
            ),
            None
        );
        assert_eq!(
            rewrite_preview_origin_for_upstream(
                "http://web--feat--orbit.localhost:9999",
                "web--feat--orbit.localhost",
                6868,
                5173,
            ),
            None
        );
        assert_eq!(
            rewrite_preview_origin_for_upstream("http://localhost:54321", "127.0.0.1", 54321, 3000,),
            None
        );
        // Scheme mismatch & cross-site origins MUST be rejected!
        assert_eq!(
            rewrite_preview_origin_for_upstream(
                "https://web--feat--orbit.localhost:6868",
                "web--feat--orbit.localhost",
                6868,
                5173,
            ),
            None
        );
        assert_eq!(
            rewrite_preview_origin_for_upstream(
                "https://evil.example",
                "web--feat--orbit.localhost",
                6868,
                5173,
            ),
            None
        );
        assert_eq!(
            rewrite_preview_origin_for_upstream(
                "http://localhost.evil.example:6868",
                "web--feat--orbit.localhost",
                6868,
                5173,
            ),
            None
        );
    }

    #[tokio::test]
    async fn proxy_websocket_upgrade_full_duplex_hmr_and_origin_rewrite() {
        use axum::Router;
        use axum::extract::ws::{Message as AxumWsMsg, WebSocketUpgrade};
        use axum::routing::get;
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::{
            Error as TungErr, Message as TungMsg, client::IntoClientRequest,
            http::HeaderValue as TungHeaderValue,
        };

        // 1. Upstream HMR-like WebSocket echo server checking Origin rewrite, subprotocol &
        //    absence of client-supplied Forwarded / X-Forwarded-* headers.
        let observed_origin = Arc::new(tokio::sync::Mutex::new(None::<String>));
        let observed_forwarded = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
        let obs_bg = Arc::clone(&observed_origin);
        let fwd_bg = Arc::clone(&observed_forwarded);
        let upstream_app = Router::new().route(
            "/ws",
            get(move |headers: HeaderMap, ws: WebSocketUpgrade| {
                let obs = Arc::clone(&obs_bg);
                let fwd = Arc::clone(&fwd_bg);
                async move {
                    if let Some(orig) = headers
                        .get(header::ORIGIN)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string)
                    {
                        *obs.lock().await = Some(orig);
                    }
                    let mut leaked_fwd = Vec::new();
                    for (name, _) in &headers {
                        let lower = name.as_str().to_ascii_lowercase();
                        if lower == "forwarded" || lower.starts_with("x-forwarded-") {
                            leaked_fwd.push(lower);
                        }
                    }
                    *fwd.lock().await = leaked_fwd;
                    ws.protocols(["vite-hmr"])
                        .on_upgrade(|mut socket| async move {
                            while let Some(Ok(msg)) = socket.recv().await {
                                match msg {
                                    AxumWsMsg::Text(txt) => {
                                        let reply = format!("echo:{txt}");
                                        if socket.send(AxumWsMsg::Text(reply.into())).await.is_err()
                                        {
                                            break;
                                        }
                                    }
                                    AxumWsMsg::Binary(bin) => {
                                        if socket.send(AxumWsMsg::Binary(bin)).await.is_err() {
                                            break;
                                        }
                                    }
                                    AxumWsMsg::Close(_) => break,
                                    _ => {}
                                }
                            }
                        })
                }
            }),
        );
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream ws");
        let upstream_port = upstream_listener
            .local_addr()
            .expect("upstream addr")
            .port();
        let upstream_handle = tokio::spawn(async move {
            let _ = axum::serve(upstream_listener, upstream_app).await;
        });

        // 2. Bind proxy listener first so we register the exact advertised port.
        let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind proxy");
        let proxy_port = proxy_listener.local_addr().expect("proxy addr").port();

        let scripts = WorkspaceScriptRegistry::new();
        let proxy_host = build_service_proxy_host("web", None, "orbit");
        scripts
            .register_running(
                "ws_1",
                "web",
                WorkspaceScriptInfo {
                    workspace_id: "ws_1".into(),
                    name: "web".into(),
                    command: "npm run dev".into(),
                    kind: WorkspaceScriptKind::Service,
                    status: WorkspaceScriptStatus::Running,
                    terminal_id: Some("term_1".into()),
                    port: Some(upstream_port),
                    proxy_url: Some(format!("http://{proxy_host}:{proxy_port}")),
                    last_error: None,
                },
                Some(proxy_host.clone()),
            )
            .await;

        let proxy_scripts = Arc::clone(&scripts);
        let proxy_app = Router::new().fallback(move |req: Request<Body>| {
            let reg = Arc::clone(&proxy_scripts);
            async move {
                let mut host_iter = req.headers().get_all(header::HOST).iter();
                let first_host = host_iter.next();
                if host_iter.next().is_some() {
                    return Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::empty())
                        .unwrap_or_else(|_| Response::new(Body::empty()));
                }
                if let Some(h) = first_host.and_then(|v| v.to_str().ok())
                    && let Some(auth) = reg.resolve_proxy_service(h).await
                {
                    let authority = PreviewAuthority::DotLocalhost {
                        host: auth.proxy_host.clone().unwrap_or_default(),
                        expected_port: auth.advertised_port,
                    };
                    return reg
                        .proxy_http_request_authorized(&auth, &authority, req)
                        .await;
                }
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }
        });
        let proxy_handle = tokio::spawn(async move {
            let _ = axum::serve(proxy_listener, proxy_app).await;
        });

        // 3. Reject WebSocket upgrades with sibling loopback port Origin (403 Forbidden)!
        let mut sibling_origin_req = format!("ws://127.0.0.1:{proxy_port}/ws")
            .into_client_request()
            .expect("sibling req");
        sibling_origin_req.headers_mut().insert(
            "host",
            TungHeaderValue::from_str(&format!("{proxy_host}:{proxy_port}")).expect("host"),
        );
        sibling_origin_req.headers_mut().insert(
            "origin",
            TungHeaderValue::from_str("http://127.0.0.1:9999").expect("sibling origin"),
        );
        let sibling_err = tokio_tungstenite::connect_async(sibling_origin_req)
            .await
            .expect_err("sibling loopback origin must be rejected");
        assert!(
            matches!(sibling_err, TungErr::Http(ref resp) if resp.status().as_u16() == 403),
            "expected HTTP 403 for sibling loopback origin, got {sibling_err:?}"
        );

        // Reject WebSocket upgrade with another `*.localhost` subdomain Origin (403 Forbidden)!
        let mut sibling_sub_req = format!("ws://127.0.0.1:{proxy_port}/ws")
            .into_client_request()
            .expect("sibling sub req");
        sibling_sub_req.headers_mut().insert(
            "host",
            TungHeaderValue::from_str(&format!("{proxy_host}:{proxy_port}")).expect("host"),
        );
        sibling_sub_req.headers_mut().insert(
            "origin",
            TungHeaderValue::from_str(&format!("http://other--orbit.localhost:{proxy_port}"))
                .expect("sibling sub origin"),
        );
        let sub_err = tokio_tungstenite::connect_async(sibling_sub_req)
            .await
            .expect_err("sibling *.localhost origin must be rejected");
        assert!(
            matches!(sub_err, TungErr::Http(ref resp) if resp.status().as_u16() == 403),
            "expected HTTP 403 for sibling *.localhost origin, got {sub_err:?}"
        );

        // 4. Connect over WebSocket targeting exact `*.localhost:<proxy_port>` with spoofed
        //    Forwarded / X-Forwarded-* headers — verify Origin rewrite AND header stripping!
        let mut ws_req = format!("ws://127.0.0.1:{proxy_port}/ws")
            .into_client_request()
            .expect("ws req");
        ws_req.headers_mut().insert(
            "host",
            TungHeaderValue::from_str(&format!("{proxy_host}:{proxy_port}")).expect("host"),
        );
        ws_req.headers_mut().insert(
            "origin",
            TungHeaderValue::from_str(&format!("http://{proxy_host}:{proxy_port}"))
                .expect("origin"),
        );
        ws_req.headers_mut().insert(
            "sec-websocket-protocol",
            TungHeaderValue::from_static("vite-hmr"),
        );
        ws_req.headers_mut().insert(
            "forwarded",
            TungHeaderValue::from_static("for=1.2.3.4;proto=https"),
        );
        ws_req
            .headers_mut()
            .insert("x-forwarded-for", TungHeaderValue::from_static("1.2.3.4"));
        ws_req.headers_mut().insert(
            "x-forwarded-host",
            TungHeaderValue::from_static("evil.example"),
        );
        ws_req
            .headers_mut()
            .insert("x-forwarded-proto", TungHeaderValue::from_static("https"));

        let (mut ws_stream, ws_resp) = tokio_tungstenite::connect_async(ws_req)
            .await
            .expect("connect ws through *.localhost proxy");
        assert_eq!(ws_resp.status().as_u16(), 101);
        assert_eq!(
            ws_resp
                .headers()
                .get("sec-websocket-protocol")
                .and_then(|v| v.to_str().ok()),
            Some("vite-hmr")
        );
        assert_eq!(
            observed_origin.lock().await.as_deref(),
            Some(format!("http://localhost:{upstream_port}").as_str())
        );
        assert!(
            observed_forwarded.lock().await.is_empty(),
            "client-supplied Forwarded / X-Forwarded-* headers must be stripped, saw: {:?}",
            observed_forwarded.lock().await
        );

        // Bidirectional text frame
        ws_stream
            .send(TungMsg::Text("hmr-ping".into()))
            .await
            .expect("send text");
        let reply1 = ws_stream.next().await.expect("recv1").expect("ok1");
        assert_eq!(reply1, TungMsg::Text("echo:hmr-ping".into()));

        // Bidirectional binary frame
        let payload = vec![0xDE, 0xAD, 0xBE, 0xEF];
        ws_stream
            .send(TungMsg::Binary(payload.clone().into()))
            .await
            .expect("send bin");
        let reply2 = ws_stream.next().await.expect("recv2").expect("ok2");
        assert_eq!(reply2, TungMsg::Binary(payload.into()));

        let _ = ws_stream.close(None).await;
        upstream_handle.abort();
        proxy_handle.abort();
    }

    #[tokio::test]
    async fn proxy_sse_streams_incremental_chunks_without_buffering_until_eof() {
        use axum::Router;
        use axum::routing::get;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (sse_tx, sse_rx) = tokio::sync::mpsc::channel::<Result<String, Infallible>>(4);
        let sse_rx_slot = Arc::new(tokio::sync::Mutex::new(Some(sse_rx)));
        let upstream_app = Router::new().route(
            "/events",
            get(move || {
                let slot = Arc::clone(&sse_rx_slot);
                async move {
                    let rx = slot.lock().await.take();
                    let body = match rx {
                        Some(mut rx) => Body::from_stream(async_stream_from_mpsc(&mut rx)),
                        None => Body::empty(),
                    };
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .body(body)
                        .unwrap_or_else(|_| Response::new(Body::empty()))
                }
            }),
        );
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sse upstream");
        let upstream_port = upstream_listener.local_addr().expect("sse addr").port();
        let upstream_handle = tokio::spawn(async move {
            let _ = axum::serve(upstream_listener, upstream_app).await;
        });

        let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sse proxy");
        let proxy_port = proxy_listener.local_addr().expect("sse proxy addr").port();

        let scripts = WorkspaceScriptRegistry::new();
        let proxy_host = "sse--orbit.localhost".to_string();
        scripts
            .register_running(
                "ws_sse",
                "sse",
                WorkspaceScriptInfo {
                    workspace_id: "ws_sse".into(),
                    name: "sse".into(),
                    command: "dev".into(),
                    kind: WorkspaceScriptKind::Service,
                    status: WorkspaceScriptStatus::Running,
                    terminal_id: Some("term_sse".into()),
                    port: Some(upstream_port),
                    proxy_url: Some(format!("http://{proxy_host}:{proxy_port}")),
                    last_error: None,
                },
                Some(proxy_host.clone()),
            )
            .await;

        let proxy_scripts = Arc::clone(&scripts);
        let proxy_app = Router::new().fallback(move |req: Request<Body>| {
            let reg = Arc::clone(&proxy_scripts);
            async move {
                if let Some(h) = req
                    .headers()
                    .get(header::HOST)
                    .and_then(|v| v.to_str().ok())
                    && let Some(auth) = reg.resolve_proxy_service(h).await
                {
                    let authority = PreviewAuthority::DotLocalhost {
                        host: auth.proxy_host.clone().unwrap_or_default(),
                        expected_port: auth.advertised_port,
                    };
                    return reg
                        .proxy_http_request_authorized(&auth, &authority, req)
                        .await;
                }
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }
        });
        let proxy_handle = tokio::spawn(async move {
            let _ = axum::serve(proxy_listener, proxy_app).await;
        });

        // Push event 1 BEFORE closing upstream stream (keep `sse_tx` alive!).
        sse_tx
            .send(Ok("data: hmr-ready\n\n".to_string()))
            .await
            .expect("send chunk 1");

        let mut client = TcpStream::connect(("127.0.0.1", proxy_port))
            .await
            .expect("connect proxy");
        client
            .write_all(
                format!(
                    "GET /events HTTP/1.1\r\nHost: {proxy_host}:{proxy_port}\r\nAccept: text/event-stream\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .expect("write GET");

        // Chunk 1 MUST arrive immediately WITHOUT waiting for upstream EOF!
        let mut received = String::new();
        let mut buf = [0u8; 2048];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !received.contains("data: hmr-ready") {
            let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !rem.is_zero(),
                "timed out waiting for first SSE chunk: {received:?}"
            );
            let n = tokio::time::timeout(rem, client.read(&mut buf))
                .await
                .expect("read chunk 1 before EOF")
                .expect("read ok");
            assert!(n > 0, "stream closed prematurely before chunk 1");
            received.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        assert!(
            received
                .to_ascii_lowercase()
                .contains("cache-control: no-cache"),
            "SSE response must enforce Cache-Control: no-cache; got: {received}"
        );

        // Now push chunk 2 and close stream cleanly.
        sse_tx
            .send(Ok("data: hmr-update\n\n".to_string()))
            .await
            .expect("send chunk 2");
        drop(sse_tx);

        let deadline2 = tokio::time::Instant::now() + Duration::from_secs(3);
        while !received.contains("data: hmr-update") {
            let rem = deadline2.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !rem.is_zero(),
                "timed out waiting for second SSE chunk: {received:?}"
            );
            let n = tokio::time::timeout(rem, client.read(&mut buf))
                .await
                .expect("read chunk 2")
                .expect("read ok");
            if n == 0 {
                break;
            }
            received.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        assert!(received.contains("data: hmr-update"));

        upstream_handle.abort();
        proxy_handle.abort();
    }

    fn async_stream_from_mpsc(
        rx: &mut tokio::sync::mpsc::Receiver<Result<String, Infallible>>,
    ) -> impl futures_util::Stream<Item = Result<String, Infallible>> + Send + 'static {
        let (tx_placeholder, mut owned_rx) = tokio::sync::mpsc::channel(1);
        std::mem::swap(rx, &mut owned_rx);
        drop(tx_placeholder);
        futures_util::stream::unfold(owned_rx, |mut rx| async move {
            let item = rx.recv().await?;
            Some((item, rx))
        })
    }

    #[tokio::test]
    async fn remote_preview_tunnel_enforces_service_allowlist_and_streams_over_duplex() {
        use axum::Router;
        use axum::routing::get;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let leaked_fwd = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
        let leaked_bg = Arc::clone(&leaked_fwd);
        let upstream_app = Router::new().route(
            "/ping",
            get(move |headers: HeaderMap| {
                let leaked = Arc::clone(&leaked_bg);
                async move {
                    let mut bad = Vec::new();
                    for (name, _) in &headers {
                        let lower = name.as_str().to_ascii_lowercase();
                        if lower == "forwarded" || lower.starts_with("x-forwarded-") {
                            bad.push(lower);
                        }
                    }
                    *leaked.lock().await = bad;
                    "pong-from-remote-service"
                }
            }),
        );
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind remote service");
        let upstream_port = upstream_listener.local_addr().expect("addr").port();
        let upstream_handle = tokio::spawn(async move {
            let _ = axum::serve(upstream_listener, upstream_app).await;
        });

        let scripts = WorkspaceScriptRegistry::new();
        // Arbitrary unregistered port MUST be forbidden (zero SSRF).
        assert_eq!(
            scripts
                .resolve_allowed_preview_port(upstream_port, None)
                .await,
            None
        );
        assert_eq!(scripts.resolve_allowed_preview_port(6379, None).await, None);
        assert_eq!(
            scripts
                .resolve_allowed_preview_port(0, Some("unregistered.localhost"))
                .await,
            None
        );

        let proxy_host = "app--remote.localhost".to_string();
        scripts
            .register_running(
                "ws_r",
                "app",
                WorkspaceScriptInfo {
                    workspace_id: "ws_r".into(),
                    name: "app".into(),
                    command: "npm run dev".into(),
                    kind: WorkspaceScriptKind::Service,
                    status: WorkspaceScriptStatus::Running,
                    terminal_id: Some("term_r".into()),
                    port: Some(upstream_port),
                    proxy_url: Some(format!("http://{proxy_host}:6868")),
                    last_error: None,
                },
                Some(proxy_host.clone()),
            )
            .await;

        let auth = scripts
            .resolve_allowed_preview_service(upstream_port, None)
            .await
            .expect("running service must resolve");
        assert_eq!(auth.target_port, upstream_port);
        assert_eq!(
            scripts
                .resolve_allowed_preview_port(0, Some(&proxy_host))
                .await,
            Some(upstream_port)
        );
        // Mismatched port + host MUST be rejected!
        assert_eq!(
            scripts
                .resolve_allowed_preview_port(upstream_port.wrapping_add(1), Some(&proxy_host))
                .await,
            None
        );

        // Verify `serve_preview_tunnel_io` forwards HTTP over a keep-alive duplex stream,
        // strips Forwarded/X-Forwarded-*, rejects foreign Host, and aborts immediately when the
        // backing workspace service exits!
        let (mut client_io, server_io) = tokio::io::duplex(64 * 1024);
        let srv_scripts = Arc::clone(&scripts);
        let srv_task = tokio::spawn(srv_scripts.serve_preview_tunnel_io(auth.clone(), server_io));

        // 1. Valid keep-alive request with spoofed Forwarded / X-Forwarded-* headers.
        client_io
            .write_all(
                b"GET /ping HTTP/1.1\r\nHost: 127.0.0.1:54321\r\nForwarded: for=9.9.9.9\r\nX-Forwarded-Host: evil.example\r\n\r\n",
            )
            .await
            .expect("write tunnel GET 1");
        let mut resp1 = String::new();
        let mut buf = [0u8; 2048];
        while !resp1.contains("pong-from-remote-service") {
            let n = tokio::time::timeout(Duration::from_secs(2), client_io.read(&mut buf))
                .await
                .expect("read resp1 timeout")
                .expect("read resp1 ok");
            assert!(n > 0, "unexpected EOF on resp1: {resp1}");
            resp1.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        assert!(resp1.contains("200 OK"), "unexpected tunnel resp: {resp1}");
        assert!(
            leaked_fwd.lock().await.is_empty(),
            "Forwarded / X-Forwarded-* must be stripped in tunnel HTTP: {:?}",
            leaked_fwd.lock().await
        );

        // 2. Foreign Host header on the same keep-alive connection MUST be rejected (403).
        client_io
            .write_all(b"GET /ping HTTP/1.1\r\nHost: evil.example\r\n\r\n")
            .await
            .expect("write foreign host GET");
        let n_bad = tokio::time::timeout(Duration::from_secs(2), client_io.read(&mut buf))
            .await
            .expect("read 403 timeout")
            .expect("read 403 ok");
        let resp_bad = String::from_utf8_lossy(&buf[..n_bad]);
        assert!(
            resp_bad.contains("403 Forbidden"),
            "foreign Host in tunnel must return 403, got: {resp_bad}"
        );

        // 3. Backing service exits (`unregister_by_terminal_id`): keep-alive tunnel MUST
        //    terminate immediately and further requests cannot reach upstream!
        let stopped = scripts.unregister_by_terminal_id("term_r").await;
        assert_eq!(stopped.len(), 1);
        assert!(!scripts.is_service_instance_live(&auth).await);
        assert_eq!(
            scripts
                .resolve_allowed_preview_port(upstream_port, None)
                .await,
            None
        );

        tokio::time::timeout(Duration::from_secs(2), srv_task)
            .await
            .expect("serve_preview_tunnel_io must cancel immediately when service exits")
            .expect("srv_task clean exit");
        let mut eof_buf = [0u8; 64];
        let eof_n = tokio::time::timeout(Duration::from_secs(2), client_io.read(&mut eof_buf))
            .await
            .expect("client EOF after cancel")
            .expect("read ok");
        assert_eq!(
            eof_n, 0,
            "open keep-alive tunnel must reach EOF on service exit"
        );

        upstream_handle.abort();
    }
}
