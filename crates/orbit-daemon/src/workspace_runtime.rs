//! Worktree full lifecycle, `.worktreeinclude` safe copier, `orbit.json` / `paseo.json`
//! setup/teardown/scripts runtime, dynamic port allocation, and `*.localhost` HTTP reverse proxy.

use std::collections::HashMap;
use std::net::TcpListener;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use orbit_protocol::{
    WorkspaceScriptInfo, WorkspaceScriptKind, WorkspaceScriptStatus, WorkspaceSetupProgress,
    WorkspaceSetupStatus, WorktreeCreateMode,
};
use serde::Deserialize;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, warn};

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

fn walk_and_copy_matching(
    root: &Path,
    dir: &Path,
    dest_root: &Path,
    patterns: &[String],
    depth: usize,
    report: &mut WorktreeIncludeCopyReport,
) {
    if depth > 6 || report.copied.len() >= 256 {
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
        // Guard against any top-level or nested symlink escaping `root`
        if sym_meta.file_type().is_symlink() && resolve_symlink_target_safe(&path, root).is_err() {
            continue;
        }

        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if path.is_dir() {
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
                        copy_dir_filtered_cow(root, &path, &target, report);
                    }
                }
            } else if !is_heavy_ignored_dir(name)
                || patterns_may_target_inside(&rel_str, name, patterns)
            {
                walk_and_copy_matching(root, &path, dest_root, patterns, depth + 1, report);
            }
        } else if path.is_file() && matches_any_pattern(&rel_str, name, patterns) {
            let target = dest_root.join(rel);
            if let Ok(method) = copy_file_cow_within(root, &path, &target) {
                record_copy_method(report, method);
                report.copied.push(rel_str);
            }
        }
    }
}

/// Safe per-entry fallback used only when a matched directory contains an escaping symlink that
/// caused whole-directory CoW validation to reject the tree.
fn copy_dir_filtered_cow(
    root: &Path,
    src_dir: &Path,
    dst_dir: &Path,
    report: &mut WorktreeIncludeCopyReport,
) {
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
        if sym_meta.file_type().is_symlink() && resolve_symlink_target_safe(&p, root).is_err() {
            continue;
        }
        let target = dst_dir.join(entry.file_name());
        if p.is_dir() {
            copy_dir_filtered_cow(root, &p, &target, report);
        } else if p.is_file()
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

            let branch = branch_name
                .filter(|b| !b.trim().is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    format!(
                        "orbit/task-{}",
                        &uuid::Uuid::new_v4().simple().to_string()[..6]
                    )
                });
            let target_dir = allocate_worktree_dir(orbit_home, repo_name, &branch);
            let claimed = if let Some(p) = pool {
                p.try_claim_branch_off(source_cwd, &target_dir, &branch, base_ref)
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
                if let Some(base) = base_ref.filter(|b| !b.trim().is_empty()) {
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
            let branch = branch_name
                .filter(|b| !b.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`branch` is required for checkout_branch mode"))?;
            let target_dir = allocate_worktree_dir(orbit_home, repo_name, branch);
            let claimed = if let Some(p) = pool {
                p.try_claim_checkout_branch(source_cwd, &target_dir, branch)
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
                    .arg(branch)
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
            Ok((target_dir, Some(branch.to_string())))
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
        let output = Command::new("/bin/sh")
            .arg("-lc")
            .arg(cmd_str)
            .current_dir(worktree_cwd)
            .envs(&env_map)
            .output()
            .await;
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
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("/bin/sh")
                .arg("-lc")
                .arg(&cmd_str)
                .current_dir(worktree_cwd)
                .envs(&env_map)
                .output(),
        )
        .await;
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
    pub info: WorkspaceScriptInfo,
    pub proxy_host: Option<String>,
}

/// Daemon-owned registry of running workspace scripts & `*.localhost` service reverse-proxy routes.
#[derive(Default)]
pub struct WorkspaceScriptRegistry {
    /// Keyed by `(workspace_id, script_name)`
    running: RwLock<HashMap<(String, String), RunningScriptState>>,
    /// Keyed by lowercase `Host` without port (e.g. `web--feat-auth--orbit.localhost`) -> target loopback port
    proxy_routes: RwLock<HashMap<String, u16>>,
    http_client: reqwest::Client,
}

impl WorkspaceScriptRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            running: RwLock::new(HashMap::new()),
            proxy_routes: RwLock::new(HashMap::new()),
            http_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()
                .unwrap_or_default(),
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
        if let (Some(host), Some(port)) = (&proxy_host, info.port) {
            self.proxy_routes
                .write()
                .await
                .insert(host.to_ascii_lowercase(), port);
        }
        self.running.write().await.insert(
            (workspace_id.to_string(), script_name.to_string()),
            RunningScriptState { info, proxy_host },
        );
    }

    pub async fn unregister(
        &self,
        workspace_id: &str,
        script_name: &str,
    ) -> Option<RunningScriptState> {
        let removed = self
            .running
            .write()
            .await
            .remove(&(workspace_id.to_string(), script_name.to_string()));
        if let Some(ref st) = removed
            && let Some(ref host) = st.proxy_host
        {
            self.proxy_routes
                .write()
                .await
                .remove(&host.to_ascii_lowercase());
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

    pub async fn resolve_proxy_port(&self, host_header: &str) -> Option<u16> {
        let host_no_port = host_header
            .split(':')
            .next()
            .unwrap_or(host_header)
            .trim()
            .to_ascii_lowercase();
        self.proxy_routes.read().await.get(&host_no_port).copied()
    }

    /// Forward an HTTP request arriving for a registered `*.localhost` service host.
    pub async fn proxy_http_request(&self, port: u16, req: Request<Body>) -> Response<Body> {
        let method = req.method().clone();
        let path_q = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");
        let target_url = format!("http://127.0.0.1:{port}{path_q}");

        let mut builder = self.http_client.request(method, &target_url);
        for (k, v) in req.headers() {
            if k.as_str().eq_ignore_ascii_case("host") {
                continue;
            }
            builder = builder.header(k, v);
        }
        let body_bytes = match axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024).await {
            Ok(b) => b,
            Err(_) => {
                return Response::builder()
                    .status(StatusCode::PAYLOAD_TOO_LARGE)
                    .body(Body::from("request body too large"))
                    .unwrap_or_else(|_| Response::new(Body::empty()));
            }
        };
        if !body_bytes.is_empty() {
            builder = builder.body(body_bytes);
        }

        match builder.send().await {
            Ok(upstream) => {
                let status = upstream.status();
                let headers = upstream.headers().clone();
                let bytes = upstream.bytes().await.unwrap_or_default();
                let mut resp = Response::builder().status(status);
                for (k, v) in &headers {
                    if k.as_str().eq_ignore_ascii_case("transfer-encoding") {
                        continue;
                    }
                    resp = resp.header(k, v);
                }
                resp.body(Body::from(bytes))
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }
            Err(e) => Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from(format!(
                    "Orbit service proxy upstream error on 127.0.0.1:{port}: {e}"
                )))
                .unwrap_or_else(|_| Response::new(Body::empty())),
        }
    }
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
}
