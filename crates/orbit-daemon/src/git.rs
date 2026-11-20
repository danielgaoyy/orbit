//! Native Git status, unified diff inspection, workspace file reads, and isolated worktree management.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use orbit_protocol::{GitFileEntry, GitStatusSnapshot};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use uuid::Uuid;

/// Inspect git repository status using `git status --porcelain=v2 --branch`.
pub async fn inspect_git_status(cwd: &Path) -> GitStatusSnapshot {
    let cwd_str = cwd.to_string_lossy().to_string();
    let output = Command::new("git")
        .args(["status", "--porcelain=v2", "--branch"])
        .current_dir(cwd)
        .output()
        .await;

    let Ok(out) = output else {
        return empty_non_git(cwd_str);
    };
    if !out.status.success() {
        return empty_non_git(cwd_str);
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut branch = None;
    let mut upstream = None;
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut files = Vec::new();

    for line in stdout.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            if head != "(detached)" {
                branch = Some(head.trim().to_string());
            }
        } else if let Some(up) = line.strip_prefix("# branch.upstream ") {
            upstream = Some(up.trim().to_string());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for part in ab.split_whitespace() {
                if let Some(a) = part.strip_prefix('+') {
                    ahead = a.parse().unwrap_or(0);
                } else if let Some(b) = part.strip_prefix('-') {
                    behind = b.parse().unwrap_or(0);
                }
            }
        } else if let Some(untracked_path) = line.strip_prefix("? ") {
            files.push(GitFileEntry {
                path: untracked_path.trim().to_string(),
                status: "??".into(),
                staged: false,
                untracked: true,
            });
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            // Format: `1 XY sub mH mI mW hH hI path` or `2 XY sub mH mI mW hH hI Xscore path\torigPath`
            let parts: Vec<&str> = line.splitn(9, ' ').collect();
            if parts.len() >= 9 {
                let xy = parts[1];
                let raw_tail = parts[8];
                let path = if line.starts_with("2 ") {
                    // Rename entry has `Xscore path\torigPath` in tail
                    raw_tail
                        .split_once(' ')
                        .map(|(_, rest)| rest.split('\t').next().unwrap_or(rest))
                        .unwrap_or(raw_tail)
                } else {
                    raw_tail
                };
                let mut chars = xy.chars();
                let x = chars.next().unwrap_or('.');
                let y = chars.next().unwrap_or('.');
                if x != '.' {
                    files.push(GitFileEntry {
                        path: path.to_string(),
                        status: x.to_string(),
                        staged: true,
                        untracked: false,
                    });
                }
                if y != '.' {
                    files.push(GitFileEntry {
                        path: path.to_string(),
                        status: y.to_string(),
                        staged: false,
                        untracked: false,
                    });
                }
            }
        }
    }

    GitStatusSnapshot {
        cwd: cwd_str,
        is_git_repo: true,
        branch,
        upstream,
        ahead,
        behind,
        files,
        nested_git_repos: Vec::new(),
    }
}

const MAX_GIT_DIFF_BYTES: usize = 4 * 1024 * 1024;
const MAX_UNTRACKED_DIFF_BYTES: usize = 256 * 1024;

/// Compute unified diff for an entire workspace or a single file (including untracked file preview).
pub async fn compute_git_diff(
    cwd: &Path,
    rel_path: Option<&str>,
    staged: bool,
) -> anyhow::Result<String> {
    let mut args = vec!["diff", "--no-color"];
    if staged {
        args.push("--cached");
    }
    if let Some(p) = rel_path {
        args.push("--");
        args.push(p);
    }

    let out = Command::new("git")
        .args(&args)
        .current_dir(cwd)
        .output()
        .await?;

    let diff_text = if out.stdout.len() > MAX_GIT_DIFF_BYTES {
        let mut cut = MAX_GIT_DIFF_BYTES;
        while cut > 0 && (out.stdout[cut] & 0b1100_0000) == 0b1000_0000 {
            cut -= 1;
        }
        let mut s = String::from_utf8_lossy(&out.stdout[..cut]).to_string();
        s.push_str("\n... [diff truncated at 4 MiB] ...\n");
        s
    } else {
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    if !diff_text.trim().is_empty() || staged {
        return Ok(diff_text);
    }

    // If diff is empty for a requested untracked file, synthesize an added-file unified diff
    if let Some(p) = rel_path {
        let candidate = cwd.join(p);
        if candidate.is_file()
            && let Ok(file) = tokio::fs::File::open(&candidate).await
        {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::with_capacity(64 * 1024);
            let mut limited = file.take(MAX_UNTRACKED_DIFF_BYTES as u64);
            if limited.read_to_end(&mut buf).await.is_ok() {
                let content = String::from_utf8_lossy(&buf);
                let mut synth = format!("--- /dev/null\n+++ b/{p}\n");
                let lines: Vec<&str> = content.lines().collect();
                synth.push_str(&format!("@@ -0,0 +1,{} @@\n", lines.len()));
                for l in lines.into_iter().take(500) {
                    synth.push('+');
                    synth.push_str(l);
                    synth.push('\n');
                }
                return Ok(synth);
            }
        }
    }

    Ok(diff_text)
}

/// Create an isolated git worktree under `$ORBIT_HOME/worktrees/<repo>-<branch>`.
pub async fn create_isolated_worktree(
    orbit_home: &Path,
    source_cwd: &Path,
    branch_name: &str,
    base_ref: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let sanitized: String = branch_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let repo_name = source_cwd
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo");

    let target_dir = orbit_home
        .join("worktrees")
        .join(format!("{repo_name}-{sanitized}"));
    if let Some(parent) = target_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut cmd = Command::new("git");
    cmd.arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(branch_name)
        .arg(&target_dir);
    if let Some(base) = base_ref {
        cmd.arg(base);
    }
    let out = cmd.current_dir(source_cwd).output().await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git worktree add failed: {stderr}");
    }

    Ok(target_dir)
}

/// Fast repository file listing (`git ls-files`) for `@` context mention autocomplete across local & remote hosts.
pub async fn list_workspace_files(cwd: &Path, limit: usize) -> Vec<String> {
    if let Ok(out) = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(cwd)
        .output()
        .await
        && out.status.success()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        return stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(limit)
            .map(ToOwned::to_owned)
            .collect();
    }

    let cwd_owned = cwd.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        collect_shallow_files_sync(&cwd_owned, &cwd_owned, 0, limit, &mut out);
        out
    })
    .await
    .unwrap_or_default()
}

fn collect_shallow_files_sync(
    root: &Path,
    dir: &Path,
    depth: usize,
    limit: usize,
    out: &mut Vec<String>,
) {
    if depth > 3 || out.len() >= limit {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= limit {
            break;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.')
            || matches!(name, "node_modules" | "target" | "dist" | "build" | ".git")
        {
            continue;
        }
        if path.is_dir() {
            collect_shallow_files_sync(root, &path, depth + 1, limit, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn empty_non_git(cwd: String) -> GitStatusSnapshot {
    let nested_git_repos = crate::discovery::discover_nested_git_repos(Path::new(&cwd), &[], 3, 24);
    GitStatusSnapshot {
        cwd,
        is_git_repo: false,
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        files: Vec::new(),
        nested_git_repos,
    }
}

const DEFAULT_MAX_READ_BYTES: u64 = 2 * 1024 * 1024;
const HARD_MAX_READ_BYTES: u64 = 4 * 1024 * 1024;
const IMAGE_MAX_READ_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct WorkspaceFilePayload {
    pub resolved_path: String,
    pub content_kind: String,
    pub mime_type: Option<String>,
    pub content: String,
    pub size_bytes: u64,
    pub truncated: bool,
}

fn image_mime_for_path(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "ico" => Some("image/x-icon"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

fn managed_artifact_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let orbit_home = std::env::var_os("ORBIT_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".orbit")));
    if let Some(oh) = orbit_home {
        roots.push(oh.join("attachments"));
    }
    if let Some(h) = home {
        roots.push(h.join(".gemini").join("antigravity-cli").join("brain"));
        roots.push(h.join(".claude").join("projects"));
    }
    roots.push(std::env::temp_dir().join("claude"));
    let tmp_claude = PathBuf::from("/tmp/claude");
    if !roots.contains(&tmp_claude) {
        roots.push(tmp_claude);
    }
    roots
}

/// Resolve a path that points directly into an explicitly allowed read-only agent artifact/log root
/// (such as `$ORBIT_HOME/attachments`, Antigravity brain task logs, or Claude Code task outputs)
/// even when `cwd` is `"~"` or outside that root.
async fn resolve_managed_artifact_file_if_allowed(
    trimmed: &str,
    allow_managed_artifacts: bool,
) -> Option<PathBuf> {
    if !allow_managed_artifacts {
        return None;
    }
    let candidate = if let Some(rest) = trimmed.strip_prefix("~/") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(rest))?
    } else {
        let p = Path::new(trimmed);
        if !p.is_absolute() {
            return None;
        }
        p.to_path_buf()
    };

    let canonical = tokio::fs::canonicalize(&candidate).await.ok()?;
    for root in managed_artifact_roots() {
        if let Ok(canon_root) = tokio::fs::canonicalize(&root).await
            && canonical.starts_with(&canon_root)
        {
            return Some(canonical);
        }
    }
    None
}

/// Resolve a canonical file path strictly contained inside `cwd` (or an explicitly allowed
/// read-only agent artifact/log root when `allow_managed_artifacts` is true), blocking
/// path-traversal and symlink escapes.
async fn resolve_contained_file_path(
    cwd: &Path,
    raw_path: &str,
    allow_managed_artifacts: bool,
) -> Result<PathBuf, String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("File path cannot be empty".to_string());
    }

    if let Some(artifact_path) =
        resolve_managed_artifact_file_if_allowed(trimmed, allow_managed_artifacts).await
    {
        return Ok(artifact_path);
    }

    let canonical_cwd = tokio::fs::canonicalize(cwd)
        .await
        .map_err(|e| format!("Workspace directory unavailable: {e}"))?;

    let candidate = if let Some(rest) = trimmed.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(rest))
            .ok_or_else(|| "Cannot resolve home directory".to_string())?
    } else {
        let p = Path::new(trimmed);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            if p.components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
            {
                return Err(format!("Path `{trimmed}` escapes the workspace directory"));
            }
            canonical_cwd.join(p)
        }
    };

    let canonical = tokio::fs::canonicalize(&candidate)
        .await
        .map_err(|e| format!("Cannot read `{trimmed}`: {e}"))?;

    if canonical.starts_with(&canonical_cwd) {
        return Ok(canonical);
    }

    if allow_managed_artifacts {
        for root in managed_artifact_roots() {
            if let Ok(canon_root) = tokio::fs::canonicalize(&root).await
                && canonical.starts_with(&canon_root)
            {
                return Ok(canonical);
            }
        }
    }

    Err(format!(
        "Access denied: `{trimmed}` is outside the workspace directory"
    ))
}

/// Read a single workspace file safely on the host (UTF-8 source/text, raster image base64, or binary marker).
pub async fn read_workspace_file(
    cwd: &Path,
    raw_path: &str,
    max_bytes: Option<u64>,
) -> Result<WorkspaceFilePayload, String> {
    let trimmed = raw_path.trim();
    let canonical = resolve_contained_file_path(cwd, trimmed, true).await?;

    let meta = tokio::fs::metadata(&canonical)
        .await
        .map_err(|e| format!("Cannot read `{trimmed}`: {e}"))?;

    if !meta.is_file() {
        return Err(format!("`{trimmed}` is not a regular file"));
    }

    let size_bytes = meta.len();
    let is_image = image_mime_for_path(&canonical).is_some();
    let cap = if is_image {
        max_bytes
            .unwrap_or(IMAGE_MAX_READ_BYTES)
            .clamp(4096, IMAGE_MAX_READ_BYTES)
    } else {
        max_bytes
            .unwrap_or(DEFAULT_MAX_READ_BYTES)
            .clamp(4096, HARD_MAX_READ_BYTES)
    };
    let resolved_path = canonical.to_string_lossy().to_string();

    if let Some(mime) = image_mime_for_path(&canonical) {
        if size_bytes > cap {
            return Ok(WorkspaceFilePayload {
                resolved_path,
                content_kind: "binary".to_string(),
                mime_type: Some(mime.to_string()),
                content: String::new(),
                size_bytes,
                truncated: true,
            });
        }
        let mut img_file = tokio::fs::File::open(&canonical)
            .await
            .map_err(|e| format!("Failed opening image `{trimmed}`: {e}"))?;
        let mut bytes = Vec::with_capacity((size_bytes as usize).min(cap as usize));
        (&mut img_file)
            .take(cap.saturating_add(1))
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| format!("Failed reading image `{trimmed}`: {e}"))?;
        if (bytes.len() as u64) > cap {
            return Ok(WorkspaceFilePayload {
                resolved_path,
                content_kind: "binary".to_string(),
                mime_type: Some(mime.to_string()),
                content: String::new(),
                size_bytes: bytes.len() as u64,
                truncated: true,
            });
        }
        return Ok(WorkspaceFilePayload {
            resolved_path,
            content_kind: "image".to_string(),
            mime_type: Some(mime.to_string()),
            content: B64.encode(&bytes),
            size_bytes: bytes.len() as u64,
            truncated: false,
        });
    }

    let mut file = tokio::fs::File::open(&canonical)
        .await
        .map_err(|e| format!("Failed opening `{trimmed}`: {e}"))?;

    let read_limit = (cap as usize).min(size_bytes as usize);
    let mut buf = Vec::with_capacity(read_limit);
    let mut limited = (&mut file).take(cap);
    limited
        .read_to_end(&mut buf)
        .await
        .map_err(|e| format!("Failed reading `{trimmed}`: {e}"))?;

    let truncated = size_bytes > cap;
    let probe_len = buf.len().min(8192);
    if buf[..probe_len].contains(&0) {
        return Ok(WorkspaceFilePayload {
            resolved_path,
            content_kind: "binary".to_string(),
            mime_type: None,
            content: String::new(),
            size_bytes,
            truncated,
        });
    }

    // Trim incomplete trailing UTF-8 codepoint only when capped mid-character
    let text = match String::from_utf8(buf) {
        Ok(s) => s,
        Err(err) => {
            let is_incomplete_tail = err.utf8_error().error_len().is_none();
            let valid_up_to = err.utf8_error().valid_up_to();
            let raw = err.into_bytes();
            if truncated && is_incomplete_tail {
                String::from_utf8_lossy(&raw[..valid_up_to]).into_owned()
            } else {
                return Ok(WorkspaceFilePayload {
                    resolved_path,
                    content_kind: "binary".to_string(),
                    mime_type: None,
                    content: String::new(),
                    size_bytes,
                    truncated,
                });
            }
        }
    };

    Ok(WorkspaceFilePayload {
        resolved_path,
        content_kind: "text".to_string(),
        mime_type: Some("text/plain".to_string()),
        content: text,
        size_bytes,
        truncated,
    })
}

// --- Turn Checkpoint Snapshots & Rewind File Restoration ---

struct TempFileGuard(PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let mut lock_os = self.0.as_os_str().to_os_string();
        lock_os.push(".lock");
        let _ = std::fs::remove_file(PathBuf::from(lock_os));
    }
}

fn is_safe_relative_repo_path(rel: &str) -> bool {
    if rel.is_empty() {
        return false;
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return false;
    }
    p.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn is_valid_git_oid(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Resolve Git repo root and optional relative subdir scope when `cwd` is nested inside a monorepo.
pub(crate) async fn resolve_git_scope(cwd: &Path) -> Option<(PathBuf, Option<String>)> {
    let out = tokio::time::timeout(
        Duration::from_secs(2),
        Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(cwd)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if raw.is_empty() {
        return None;
    }
    let repo_root = PathBuf::from(raw);
    let canon_root = tokio::fs::canonicalize(&repo_root)
        .await
        .unwrap_or_else(|_| repo_root.clone());
    let canon_cwd = tokio::fs::canonicalize(cwd)
        .await
        .unwrap_or_else(|_| cwd.to_path_buf());
    let subdir_scope = canon_cwd
        .strip_prefix(&canon_root)
        .ok()
        .filter(|rel| !rel.as_os_str().is_empty())
        .map(|rel| rel.to_string_lossy().replace('\\', "/"));
    Some((canon_root, subdir_scope))
}

/// Capture an isolated Git tree snapshot of the working tree (scoped to `cwd` if inside a monorepo
/// subdirectory) using a temporary `GIT_INDEX_FILE` so the user's real `.git/index`, `HEAD`,
/// stash, and reflog are never modified.
pub async fn capture_git_tree_snapshot(cwd: &Path) -> Option<String> {
    let (repo_root, subdir_scope) = resolve_git_scope(cwd).await?;
    let tmp_index =
        std::env::temp_dir().join(format!("orbit-snap-{}.idx", Uuid::new_v4().simple()));
    let _guard = TempFileGuard(tmp_index.clone());

    let mut add_cmd = Command::new("git");
    add_cmd.args(["add", "-A"]);
    if let Some(ref rel) = subdir_scope {
        add_cmd.arg("--").arg(rel);
    }
    let add_out = tokio::time::timeout(
        Duration::from_secs(3),
        add_cmd
            .env("GIT_INDEX_FILE", &tmp_index)
            .current_dir(&repo_root)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !add_out.status.success() {
        return None;
    }

    let write_out = tokio::time::timeout(
        Duration::from_secs(3),
        Command::new("git")
            .arg("write-tree")
            .env("GIT_INDEX_FILE", &tmp_index)
            .current_dir(&repo_root)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !write_out.status.success() {
        return None;
    }

    let oid = String::from_utf8_lossy(&write_out.stdout)
        .trim()
        .to_string();
    if is_valid_git_oid(&oid) {
        Some(oid)
    } else {
        None
    }
}

/// Restore only the files within `cwd` that changed since `target_tree_oid`, leaving untouched
/// files, monorepo sibling subdirectories, and the user's real `.git/index` intact.
pub async fn restore_git_tree_snapshot(cwd: &Path, target_tree_oid: &str) -> anyhow::Result<u32> {
    if !is_valid_git_oid(target_tree_oid) {
        anyhow::bail!("invalid checkpoint tree id");
    }
    let (repo_root, subdir_scope) = resolve_git_scope(cwd)
        .await
        .ok_or_else(|| anyhow::anyhow!("workspace is not a git repository"))?;

    let type_out = Command::new("git")
        .args(["cat-file", "-t", target_tree_oid])
        .current_dir(&repo_root)
        .kill_on_drop(true)
        .output()
        .await?;
    if !type_out.status.success() || String::from_utf8_lossy(&type_out.stdout).trim() != "tree" {
        anyhow::bail!("checkpoint snapshot is no longer available in git object store");
    }

    let Some(cur_tree_oid) = capture_git_tree_snapshot(cwd).await else {
        anyhow::bail!("failed to inspect current workspace state before restore");
    };
    if cur_tree_oid == target_tree_oid {
        return Ok(0);
    }

    let mut diff_cmd = Command::new("git");
    diff_cmd.args([
        "diff",
        "--name-status",
        "-z",
        "--no-renames",
        target_tree_oid,
        &cur_tree_oid,
    ]);
    if let Some(ref rel) = subdir_scope {
        diff_cmd.arg("--").arg(rel);
    }
    let diff_out = diff_cmd
        .current_dir(&repo_root)
        .kill_on_drop(true)
        .output()
        .await?;
    if !diff_out.status.success() {
        let stderr = String::from_utf8_lossy(&diff_out.stderr);
        anyhow::bail!("git diff checkpoint failed: {}", stderr.trim());
    }

    let mut paths_to_delete: Vec<String> = Vec::new();
    let mut paths_to_restore: Vec<String> = Vec::new();

    let mut fields = diff_out
        .stdout
        .split(|&b| b == 0)
        .filter(|slice| !slice.is_empty());
    while let (Some(status_bytes), Some(path_bytes)) = (fields.next(), fields.next()) {
        let status = String::from_utf8_lossy(status_bytes);
        let rel_path = String::from_utf8_lossy(path_bytes).into_owned();
        if !is_safe_relative_repo_path(&rel_path) {
            continue;
        }
        if status.starts_with('A') {
            paths_to_delete.push(rel_path);
        } else {
            paths_to_restore.push(rel_path);
        }
    }

    let mut touched_count: u32 = 0;
    let prune_stop = match &subdir_scope {
        Some(rel) => repo_root.join(rel),
        None => repo_root.clone(),
    };

    for rel in &paths_to_delete {
        let abs = repo_root.join(rel);
        if abs.exists() || abs.symlink_metadata().is_ok() {
            if abs.is_dir() && !abs.is_symlink() {
                let _ = tokio::fs::remove_dir_all(&abs).await;
            } else {
                let _ = tokio::fs::remove_file(&abs).await;
            }
            touched_count = touched_count.saturating_add(1);
            // Prune now-empty parent directories strictly inside the workspace boundary
            let mut cur_parent = abs.parent();
            while let Some(parent) = cur_parent {
                if parent == prune_stop || !parent.starts_with(&prune_stop) {
                    break;
                }
                if tokio::fs::remove_dir(parent).await.is_err() {
                    break;
                }
                cur_parent = parent.parent();
            }
        }
    }

    if !paths_to_restore.is_empty() {
        let tmp_index =
            std::env::temp_dir().join(format!("orbit-restore-{}.idx", Uuid::new_v4().simple()));
        let _guard = TempFileGuard(tmp_index.clone());

        let read_out = Command::new("git")
            .args(["read-tree", target_tree_oid])
            .env("GIT_INDEX_FILE", &tmp_index)
            .current_dir(&repo_root)
            .kill_on_drop(true)
            .output()
            .await?;
        if !read_out.status.success() {
            let stderr = String::from_utf8_lossy(&read_out.stderr);
            anyhow::bail!("git read-tree failed: {}", stderr.trim());
        }

        for chunk in paths_to_restore.chunks(200) {
            let mut cmd = Command::new("git");
            cmd.args(["checkout-index", "-f", "--"])
                .args(chunk)
                .env("GIT_INDEX_FILE", &tmp_index)
                .current_dir(&repo_root)
                .kill_on_drop(true);
            let out = cmd.output().await?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                anyhow::bail!("git checkout-index failed: {}", stderr.trim());
            }
            touched_count = touched_count.saturating_add(chunk.len() as u32);
        }
    }

    Ok(touched_count)
}

#[derive(Debug)]
struct ParsedHunk {
    new_start: usize,
    lines: Vec<(char, String)>,
}

fn parse_unified_hunks(diff: &str) -> Vec<ParsedHunk> {
    let mut hunks = Vec::new();
    let mut cur: Option<ParsedHunk> = None;

    for raw_line in diff.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.starts_with("diff --git ") || line.starts_with("--- ") || line.starts_with("+++ ") {
            if let Some(prev) = cur.take() {
                hunks.push(prev);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some(prev) = cur.take() {
                hunks.push(prev);
            }
            // Format: `@@ -old_start,old_len +new_start,new_len @@`
            let new_start = rest
                .split_whitespace()
                .find_map(|tok| tok.strip_prefix('+'))
                .and_then(|s| s.split(',').next())
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(1);
            cur = Some(ParsedHunk {
                new_start,
                lines: Vec::new(),
            });
            continue;
        }
        if let Some(hunk) = cur.as_mut() {
            if line.starts_with("\\ No newline at end of file") {
                continue;
            }
            if line.is_empty() {
                hunk.lines.push((' ', String::new()));
                continue;
            }
            let mut chars = line.chars();
            if let Some(sigil) = chars.next()
                && matches!(sigil, ' ' | '+' | '-')
            {
                hunk.lines.push((sigil, chars.as_str().to_string()));
            }
        }
    }
    if let Some(last) = cur {
        hunks.push(last);
    }
    hunks
}

/// Pure-Rust reverse unified diff applier for pre-checkpoint turns or non-Git workspaces.
/// Verifies post-image lines match before reversing each hunk so unrelated manual edits are never clobbered.
pub fn reverse_apply_unified_diff_to_text(current: &str, diff: &str) -> Option<String> {
    let hunks = parse_unified_hunks(diff);
    if hunks.is_empty() {
        return None;
    }

    let line_ending = if current.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let trailing_newline = current.ends_with('\n');
    let mut lines: Vec<String> = current
        .lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect();

    // Apply hunks from bottom to top so line indices remain exact
    for hunk in hunks.iter().rev() {
        let post_lines: Vec<&str> = hunk
            .lines
            .iter()
            .filter_map(|(c, text)| {
                if *c == ' ' || *c == '+' {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        let pre_lines: Vec<String> = hunk
            .lines
            .iter()
            .filter_map(|(c, text)| {
                if *c == ' ' || *c == '-' {
                    Some(text.clone())
                } else {
                    None
                }
            })
            .collect();

        let expected_idx = hunk.new_start.saturating_sub(1);
        let match_idx = if expected_idx + post_lines.len() <= lines.len()
            && lines[expected_idx..expected_idx + post_lines.len()]
                .iter()
                .map(String::as_str)
                .eq(post_lines.iter().copied())
        {
            Some(expected_idx)
        } else {
            // Small fuzzy search within ±20 lines around expected offset
            let lo = expected_idx.saturating_sub(20);
            let hi = (expected_idx + 20).min(lines.len().saturating_sub(post_lines.len()));
            (lo..=hi).find(|&idx| {
                idx + post_lines.len() <= lines.len()
                    && lines[idx..idx + post_lines.len()]
                        .iter()
                        .map(String::as_str)
                        .eq(post_lines.iter().copied())
            })
        };

        let start = match_idx?;
        lines.splice(start..start + post_lines.len(), pre_lines);
    }

    let mut out = lines.join(line_ending);
    if trailing_newline && !out.is_empty() {
        out.push_str(line_ending);
    }
    Some(out)
}

fn diff_header_indicates_new_file(diff: &str) -> bool {
    diff.lines()
        .take_while(|l| !l.starts_with("@@ "))
        .any(|l| l.trim() == "--- /dev/null")
}

/// Fallback restoration when no Git tree checkpoint exists: reverse-applies recorded `FileEdit`
/// unified diffs in reverse chronological order, strictly contained inside canonical `cwd`.
pub async fn reverse_apply_file_edit_diffs(cwd: &Path, edits: &[(String, String)]) -> u32 {
    let Ok(canonical_cwd) = tokio::fs::canonicalize(cwd).await else {
        return 0;
    };
    let mut restored_paths = std::collections::HashSet::new();

    for (raw_path, diff) in edits.iter().rev() {
        let p = Path::new(raw_path.trim());
        if p.as_os_str().is_empty() {
            continue;
        }
        let candidate = if p.is_absolute() {
            p.to_path_buf()
        } else {
            if p.components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
            {
                continue;
            }
            canonical_cwd.join(p)
        };
        let Ok(sym_meta) = tokio::fs::symlink_metadata(&candidate).await else {
            continue;
        };
        if sym_meta.file_type().is_symlink() || !sym_meta.is_file() {
            continue;
        }
        let Ok(canonical_target) = tokio::fs::canonicalize(&candidate).await else {
            continue;
        };
        if !canonical_target.starts_with(&canonical_cwd) {
            continue;
        }

        if diff_header_indicates_new_file(diff) {
            if tokio::fs::remove_file(&canonical_target).await.is_ok() {
                restored_paths.insert(canonical_target);
            }
            continue;
        }
        let Ok(current) = tokio::fs::read_to_string(&canonical_target).await else {
            continue;
        };
        if let Some(reverted) = reverse_apply_unified_diff_to_text(&current, diff)
            && tokio::fs::write(&canonical_target, reverted).await.is_ok()
        {
            restored_paths.insert(canonical_target);
        }
    }

    restored_paths.len() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_workspace_file_handles_text_image_binary_and_truncation() {
        let dir = std::env::temp_dir().join(format!("orbit_read_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.expect("mkdir");

        // 1. UTF-8 text file
        let txt_path = dir.join("hello.rs");
        tokio::fs::write(&txt_path, "fn main() {\n    println!(\"hi\");\n}\n")
            .await
            .expect("write text");
        let text_res = read_workspace_file(&dir, "hello.rs", None)
            .await
            .expect("read text");
        assert_eq!(text_res.content_kind, "text");
        assert!(!text_res.truncated);
        assert!(text_res.content.contains("fn main()"));

        // 2. Binary detection
        let bin_path = dir.join("blob.bin");
        tokio::fs::write(&bin_path, &[0x7f, b'E', b'L', b'F', 0x00, 0x01])
            .await
            .expect("write bin");
        let bin_res = read_workspace_file(&dir, "blob.bin", None)
            .await
            .expect("read bin");
        assert_eq!(bin_res.content_kind, "binary");
        assert!(bin_res.content.is_empty());

        // 3. Raster image base64
        let img_path = dir.join("pixel.png");
        tokio::fs::write(&img_path, &[0x89, b'P', b'N', b'G'])
            .await
            .expect("write png");
        let img_res = read_workspace_file(&dir, "pixel.png", None)
            .await
            .expect("read png");
        assert_eq!(img_res.content_kind, "image");
        assert_eq!(img_res.mime_type.as_deref(), Some("image/png"));
        assert_eq!(img_res.content, B64.encode([0x89, b'P', b'N', b'G']));

        // 4. Truncation guard
        let big_path = dir.join("big.txt");
        let big_body = "x".repeat(6000);
        tokio::fs::write(&big_path, &big_body)
            .await
            .expect("write big");
        let trunc_res = read_workspace_file(&dir, "big.txt", Some(4096))
            .await
            .expect("read trunc");
        assert_eq!(trunc_res.content_kind, "text");
        assert!(trunc_res.truncated);
        assert_eq!(trunc_res.size_bytes, 6000);
        assert_eq!(trunc_res.content.len(), 4096);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn git_tree_snapshot_captures_and_restores_without_touching_index() {
        let dir = std::env::temp_dir().join(format!("orbit_cp_test_{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.expect("mkdir");

        // Initialize minimal repo with one committed file and one dirty uncommitted edit
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@orbit.local"],
            vec!["config", "user.name", "Orbit Test"],
        ] {
            let st = Command::new("git")
                .args(&args)
                .current_dir(&dir)
                .status()
                .await
                .expect("git init/config");
            assert!(st.success());
        }

        tokio::fs::write(dir.join("base.txt"), "line1\nline2\n")
            .await
            .expect("write base");
        tokio::fs::write(dir.join("keep_del.txt"), "original\n")
            .await
            .expect("write keep_del");
        assert!(
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(&dir)
                .status()
                .await
                .expect("git add")
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-q", "-m", "init"])
                .current_dir(&dir)
                .status()
                .await
                .expect("git commit")
                .success()
        );

        // Pre-turn dirty edit + untracked file
        tokio::fs::write(dir.join("base.txt"), "line1\npre-turn-edit\n")
            .await
            .expect("dirty edit");
        tokio::fs::write(dir.join("pre_untracked.txt"), "kept\n")
            .await
            .expect("pre untracked");

        let snap = capture_git_tree_snapshot(&dir)
            .await
            .expect("capture snapshot");

        // Simulate agent turn: modify base.txt, delete keep_del.txt, create nested new file
        tokio::fs::write(dir.join("base.txt"), "line1\nagent-changed\n")
            .await
            .expect("agent edit");
        tokio::fs::remove_file(dir.join("keep_del.txt"))
            .await
            .expect("agent delete");
        tokio::fs::create_dir_all(dir.join("nested/dir"))
            .await
            .expect("mkdir nested");
        tokio::fs::write(dir.join("nested/dir/created.txt"), "brand new\n")
            .await
            .expect("agent create");

        let restored = restore_git_tree_snapshot(&dir, &snap)
            .await
            .expect("restore snapshot");
        assert_eq!(restored, 3);
        assert_eq!(
            tokio::fs::read_to_string(dir.join("base.txt"))
                .await
                .expect("read base"),
            "line1\npre-turn-edit\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(dir.join("keep_del.txt"))
                .await
                .expect("read keep_del"),
            "original\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(dir.join("pre_untracked.txt"))
                .await
                .expect("read pre_untracked"),
            "kept\n"
        );
        assert!(!dir.join("nested/dir/created.txt").exists());
        assert!(!dir.join("nested").exists());

        // Real git index must still have zero staged changes (pre_untracked stays untracked)
        let status = inspect_git_status(&dir).await;
        assert!(status.files.iter().all(|f| !f.staged));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn reverse_unified_diff_reverts_hunk_safely_and_preserves_crlf() {
        let post = "alpha\r\nbeta-new\r\ngamma\r\n";
        let diff =
            "--- a/foo.txt\n+++ b/foo.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta-old\n+beta-new\n gamma\n";
        let reverted = reverse_apply_unified_diff_to_text(post, diff).expect("reverse hunk");
        assert_eq!(reverted, "alpha\r\nbeta-old\r\ngamma\r\n");

        // Ensure `--- /dev/null` appearing inside a hunk body is NOT mistaken for a new-file header
        let hunk_with_dashes = "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,2 +1,2 @@\n--- /dev/null\n+replacement\n keep\n";
        assert!(!diff_header_indicates_new_file(hunk_with_dashes));
        assert!(diff_header_indicates_new_file(
            "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+hello\n"
        ));
    }

    #[tokio::test]
    async fn file_read_and_fallback_rewind_block_path_traversal() {
        let root = std::env::temp_dir().join(format!("orbit_sec_test_{}", Uuid::new_v4()));
        let ws = root.join("workspace");
        tokio::fs::create_dir_all(&ws).await.expect("mkdir ws");
        let outside = root.join("secret.txt");
        tokio::fs::write(&outside, "top-secret\n")
            .await
            .expect("write outside");

        assert!(
            read_workspace_file(&ws, "../secret.txt", None)
                .await
                .is_err()
        );
        assert!(
            read_workspace_file(&ws, outside.to_str().unwrap_or(""), None)
                .await
                .is_err()
        );

        let fake_new_diff = "--- /dev/null\n+++ b/secret.txt\n@@ -0,0 +1 @@\n+top-secret\n";
        let count = reverse_apply_file_edit_diffs(
            &ws,
            &[("../secret.txt".to_string(), fake_new_diff.to_string())],
        )
        .await;
        assert_eq!(count, 0);
        assert!(outside.exists(), "outside file must never be deleted");

        let _ = tokio::fs::remove_dir_all(&root).await;
    }
}
