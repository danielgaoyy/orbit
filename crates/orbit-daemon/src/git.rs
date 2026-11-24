//! Native Git status, unified diff inspection, workspace file reads, and isolated worktree management.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use orbit_protocol::{GitFileEntry, GitStatusSnapshot, RewindPreviewFile};
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
        } else if line.starts_with("u ") {
            // Unmerged (conflicted) entry: `u XY sub m1 m2 m3 mW h1 h2 h3 path`
            let parts: Vec<&str> = line.splitn(11, ' ').collect();
            if parts.len() >= 11 {
                files.push(GitFileEntry {
                    path: parts[10].to_string(),
                    status: parts[1].to_string(),
                    staged: false,
                    untracked: false,
                });
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

use crate::workspace_files::{
    detect_line_ending, is_metadata_writable, is_path_inside_git, metadata_mtime_unix_ms,
    sha256_hex,
};

const DEFAULT_MAX_READ_BYTES: u64 = 2 * 1024 * 1024;
const HARD_MAX_READ_BYTES: u64 = 4 * 1024 * 1024;
const IMAGE_MAX_READ_BYTES: u64 = 8 * 1024 * 1024;
const MEDIA_MAX_READ_BYTES: u64 = 25 * 1024 * 1024;
const MEDIA_SNIFF_PROBE_BYTES: u64 = 4096;

#[derive(Debug, Clone)]
pub struct WorkspaceFilePayload {
    pub resolved_path: String,
    pub content_kind: String,
    pub mime_type: Option<String>,
    pub content: String,
    pub size_bytes: u64,
    pub truncated: bool,
    pub etag: Option<String>,
    pub mtime_unix_ms: Option<i64>,
    pub writable: bool,
    pub line_ending: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ManagedWorktreeRoots {
    agy_worktree_roots: Vec<PathBuf>,
    orbit_worktree_roots: Vec<PathBuf>,
}

impl ManagedWorktreeRoots {
    fn from_env(home: Option<&Path>) -> Self {
        let mut agy_worktree_roots = Vec::new();
        let mut orbit_worktree_roots = Vec::new();

        if let Some(dir) = std::env::var_os("ANTIGRAVITY_EXECUTABLE_DATA_DIR")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
        {
            agy_worktree_roots.push(dir.join("worktrees"));
        }
        if let Some(h) = home {
            if let Ok(rel) = std::env::var("ANTIGRAVITY_CONFIG_REL_DIR") {
                let rel_trimmed = rel.trim();
                if !rel_trimmed.is_empty() {
                    let custom = h.join(".gemini").join(rel_trimmed).join("worktrees");
                    if !agy_worktree_roots.contains(&custom) {
                        agy_worktree_roots.push(custom);
                    }
                }
            }
            let default_agy = h.join(".gemini").join("antigravity-cli").join("worktrees");
            if !agy_worktree_roots.contains(&default_agy) {
                agy_worktree_roots.push(default_agy);
            }
        }

        if let Some(oh) = std::env::var_os("ORBIT_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
        {
            orbit_worktree_roots.push(oh.join("worktrees"));
        }
        if let Some(h) = home {
            let default_orbit = h.join(".orbit").join("worktrees");
            if !orbit_worktree_roots.contains(&default_orbit) {
                orbit_worktree_roots.push(default_orbit);
            }
        }

        Self {
            agy_worktree_roots,
            orbit_worktree_roots,
        }
    }

    fn all_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.agy_worktree_roots
            .iter()
            .chain(self.orbit_worktree_roots.iter())
    }
}

fn canonicalize_existing_prefix_sync(path: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut cur = path;
    loop {
        if let Ok(canon) = std::fs::canonicalize(cur) {
            let mut out = canon;
            for seg in missing.into_iter().rev() {
                out.push(seg);
            }
            return Some(out);
        }
        let name = cur.file_name()?.to_os_string();
        missing.push(name);
        cur = cur.parent()?;
    }
}

fn strip_worktree_rel_with_skip(
    candidate: &Path,
    canon_candidate: Option<&Path>,
    root: &Path,
    skip_leading_segments: usize,
) -> Option<PathBuf> {
    let canon_root = std::fs::canonicalize(root).ok();
    let tail = candidate
        .strip_prefix(root)
        .ok()
        .or_else(|| {
            canon_root
                .as_deref()
                .and_then(|cr| candidate.strip_prefix(cr).ok())
        })
        .or_else(|| canon_candidate.and_then(|cc| cc.strip_prefix(root).ok()))
        .or_else(|| {
            canon_candidate
                .zip(canon_root.as_deref())
                .and_then(|(cc, cr)| cc.strip_prefix(cr).ok())
        })?;
    let mut normals = Vec::new();
    for comp in tail.components() {
        match comp {
            Component::Normal(seg) => normals.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    if normals.len() <= skip_leading_segments {
        return None;
    }
    let mut rel = PathBuf::new();
    for seg in &normals[skip_leading_segments..] {
        rel.push(seg);
    }
    (!rel.as_os_str().is_empty()).then_some(rel)
}

/// Extract a safe workspace-relative path when `candidate` points inside a temporary Antigravity
/// (`<agy_data>/worktrees/<conv>/<wt>/<rel>`) or Orbit (`<orbit_home>/worktrees/<wt>/<rel>`)
/// subagent worktree that has already been cleaned up after merge.
fn extract_deleted_worktree_relative_path(
    candidate: &Path,
    worktree_roots: &ManagedWorktreeRoots,
) -> Option<PathBuf> {
    if !candidate.is_absolute() {
        return None;
    }
    if candidate
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return None;
    }
    let canon_candidate = canonicalize_existing_prefix_sync(candidate);
    for root in &worktree_roots.agy_worktree_roots {
        if let Some(rel) =
            strip_worktree_rel_with_skip(candidate, canon_candidate.as_deref(), root, 2)
        {
            return Some(rel);
        }
    }
    for root in &worktree_roots.orbit_worktree_roots {
        if let Some(rel) =
            strip_worktree_rel_with_skip(candidate, canon_candidate.as_deref(), root, 1)
        {
            return Some(rel);
        }
    }
    None
}

fn resolve_git_common_dir(start: &Path, stop_root: Option<&Path>) -> Option<PathBuf> {
    let mut cur = if start.is_dir() {
        start
    } else {
        start.parent()?
    };
    loop {
        if let Some(bound) = stop_root
            && (cur == bound || !cur.starts_with(bound))
        {
            return None;
        }
        let dot_git = cur.join(".git");
        if let Ok(meta) = std::fs::symlink_metadata(&dot_git) {
            if meta.is_dir() {
                return std::fs::canonicalize(&dot_git).ok();
            }
            if meta.is_file() {
                let raw = std::fs::read_to_string(&dot_git).ok()?;
                let first_line = raw.lines().next()?.trim();
                let gitdir_raw = first_line
                    .strip_prefix("gitdir:")
                    .or_else(|| first_line.strip_prefix("GITDIR:"))?
                    .trim();
                if gitdir_raw.is_empty() {
                    return None;
                }
                let gitdir_path = {
                    let p = Path::new(gitdir_raw);
                    if p.is_absolute() {
                        p.to_path_buf()
                    } else {
                        cur.join(p)
                    }
                };
                let canon_gitdir = std::fs::canonicalize(&gitdir_path).ok()?;
                let commondir_file = canon_gitdir.join("commondir");
                if let Ok(common_raw) = std::fs::read_to_string(&commondir_file) {
                    let common_trim = common_raw.trim();
                    if !common_trim.is_empty() {
                        let common_path = {
                            let p = Path::new(common_trim);
                            if p.is_absolute() {
                                p.to_path_buf()
                            } else {
                                canon_gitdir.join(p)
                            }
                        };
                        return std::fs::canonicalize(&common_path).ok();
                    }
                }
                return Some(canon_gitdir);
            }
        }
        cur = cur.parent()?;
    }
}

/// Verify that `canonical_target` lives inside a managed worktree root AND belongs to the exact
/// same Git repository (identical canonicalized Git common dir) as `canonical_cwd`.
fn is_same_repo_worktree_file(
    canonical_cwd: &Path,
    canonical_target: &Path,
    worktree_roots: &ManagedWorktreeRoots,
) -> bool {
    let Ok(raw_canon_cwd) = std::fs::canonicalize(canonical_cwd) else {
        return false;
    };
    let Ok(raw_canon_target) = std::fs::canonicalize(canonical_target) else {
        return false;
    };
    let Some(cwd_common) = resolve_git_common_dir(&raw_canon_cwd, None) else {
        return false;
    };
    for wt_root in worktree_roots.all_roots() {
        let Ok(canon_wt_root) = std::fs::canonicalize(wt_root) else {
            continue;
        };
        if !raw_canon_target.starts_with(&canon_wt_root) {
            continue;
        }
        if let Some(target_common) = resolve_git_common_dir(&raw_canon_target, Some(&canon_wt_root))
            && target_common == cwd_common
        {
            return true;
        }
    }
    false
}

#[cfg(unix)]
pub(crate) fn unix_o_nofollow_flag() -> Option<i32> {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    {
        Some(0x0100)
    }
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "x86", target_arch = "riscv64")
    ))]
    {
        Some(0x20000)
    }
    #[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
    {
        // Linux arm/aarch64 `O_NOFOLLOW` is octal 0100000 (0x8000); 0x10000 is `O_DIRECT`.
        Some(0o100000)
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        all(
            target_os = "linux",
            any(
                target_arch = "x86_64",
                target_arch = "x86",
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "arm"
            )
        )
    )))]
    {
        None
    }
}

/// Open a canonicalized regular file with `O_NOFOLLOW` (where supported) and verify via `fstat`
/// on the opened handle + pre/post `symlink_metadata` `(dev, ino)` identity checks that no
/// symlink swap occurred between containment validation and open.
pub(crate) async fn open_validated_regular_file_async(
    canonical: &Path,
    trimmed: &str,
) -> Result<(tokio::fs::File, std::fs::Metadata), String> {
    let lmeta_before = tokio::fs::symlink_metadata(canonical)
        .await
        .map_err(|e| format!("Cannot read `{trimmed}`: {e}"))?;
    if lmeta_before.file_type().is_symlink() {
        return Err(format!(
            "Access denied: `{trimmed}` resolved to a symlink during open"
        ));
    }
    if !lmeta_before.is_file() {
        return Err(format!("`{trimmed}` is not a regular file"));
    }

    let mut opts = tokio::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    if let Some(flag) = unix_o_nofollow_flag() {
        opts.custom_flags(flag);
    }
    let file = opts
        .open(canonical)
        .await
        .map_err(|e| format!("Failed opening `{trimmed}`: {e}"))?;
    let fmeta = file
        .metadata()
        .await
        .map_err(|e| format!("Cannot inspect `{trimmed}`: {e}"))?;
    if !fmeta.is_file() {
        return Err(format!("`{trimmed}` is not a regular file"));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let lmeta_after = tokio::fs::symlink_metadata(canonical)
            .await
            .map_err(|e| format!("Cannot verify `{trimmed}`: {e}"))?;
        if lmeta_after.file_type().is_symlink()
            || fmeta.dev() != lmeta_before.dev()
            || fmeta.ino() != lmeta_before.ino()
            || fmeta.dev() != lmeta_after.dev()
            || fmeta.ino() != lmeta_after.ino()
        {
            return Err(format!(
                "Access denied: `{trimmed}` changed during validation"
            ));
        }
    }

    Ok((file, fmeta))
}

fn managed_artifact_roots(extra_attachments_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(extra) = extra_attachments_dir {
        roots.push(extra.to_path_buf());
    }
    let home = crate::platform::resolve_user_home_dir();
    let orbit_home = std::env::var_os("ORBIT_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".orbit")));
    if let Some(oh) = orbit_home {
        let att = oh.join("attachments");
        if !roots.contains(&att) {
            roots.push(att);
        }
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

/// Resolve a path that points directly into an explicitly allowed read-only agent artifact/log
/// root (such as `$ORBIT_HOME/attachments`, Antigravity brain task logs, or Claude Code task
/// outputs) even when `cwd` is `"~"` or outside that root.
/// Managed worktrees are NOT included here; they are strictly scoped to the same Git repository as
/// `cwd` in [`resolve_contained_file_path_with_roots`].
async fn resolve_managed_artifact_file_if_allowed(
    trimmed: &str,
    allow_managed_artifacts: bool,
    extra_attachments_dir: Option<&Path>,
) -> Option<PathBuf> {
    if !allow_managed_artifacts {
        return None;
    }
    let candidate = if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix(r"~\"))
    {
        crate::platform::resolve_user_home_dir().map(|h| h.join(rest))?
    } else {
        let p = Path::new(trimmed);
        if !p.is_absolute() {
            return None;
        }
        p.to_path_buf()
    };

    let canonical = crate::platform::canonicalize_clean_async(&candidate)
        .await
        .ok()?;
    for root in managed_artifact_roots(extra_attachments_dir) {
        if let Ok(canon_root) = crate::platform::canonicalize_clean_async(&root).await
            && canonical.starts_with(&canon_root)
        {
            return Some(canonical);
        }
    }
    None
}

/// Resolve a canonical file path strictly contained inside `cwd` (or an explicitly allowed
/// read-only agent artifact/log root or same-repository managed worktree when
/// `allow_managed_artifacts` is true), blocking path-traversal and symlink escapes.
async fn resolve_contained_file_path(
    cwd: &Path,
    raw_path: &str,
    allow_managed_artifacts: bool,
    extra_attachments_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    let home = crate::platform::resolve_user_home_dir();
    let worktree_roots = ManagedWorktreeRoots::from_env(home.as_deref());
    resolve_contained_file_path_with_roots(
        cwd,
        raw_path,
        allow_managed_artifacts,
        extra_attachments_dir,
        &worktree_roots,
    )
    .await
}

async fn resolve_contained_file_path_with_roots(
    cwd: &Path,
    raw_path: &str,
    allow_managed_artifacts: bool,
    extra_attachments_dir: Option<&Path>,
    worktree_roots: &ManagedWorktreeRoots,
) -> Result<PathBuf, String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("File path cannot be empty".to_string());
    }

    if let Some(artifact_path) = resolve_managed_artifact_file_if_allowed(
        trimmed,
        allow_managed_artifacts,
        extra_attachments_dir,
    )
    .await
    {
        return Ok(artifact_path);
    }

    let canonical_cwd = crate::platform::canonicalize_clean_async(cwd)
        .await
        .map_err(|e| format!("Workspace directory unavailable: {e}"))?;

    let candidate = if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix(r"~\"))
    {
        crate::platform::resolve_user_home_dir()
            .map(|h| h.join(rest))
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

    let canonical = match crate::platform::canonicalize_clean_async(&candidate).await {
        Ok(canon) => canon,
        Err(orig_err) => {
            if allow_managed_artifacts
                && let Some(rel) =
                    extract_deleted_worktree_relative_path(&candidate, worktree_roots)
                && let Ok(fallback) =
                    crate::platform::canonicalize_clean_async(&canonical_cwd.join(&rel)).await
                && fallback.starts_with(&canonical_cwd)
            {
                return Ok(fallback);
            }
            return Err(format!("Cannot read `{trimmed}`: {orig_err}"));
        }
    };

    if canonical.starts_with(&canonical_cwd) {
        return Ok(canonical);
    }

    if allow_managed_artifacts {
        for root in managed_artifact_roots(extra_attachments_dir) {
            if let Ok(canon_root) = crate::platform::canonicalize_clean_async(&root).await
                && canonical.starts_with(&canon_root)
            {
                return Ok(canonical);
            }
        }
        if is_same_repo_worktree_file(&canonical_cwd, &canonical, worktree_roots) {
            return Ok(canonical);
        }
    }

    Err(format!(
        "Access denied: `{trimmed}` is outside the workspace directory"
    ))
}

async fn compute_workspace_file_writable(
    cwd: &Path,
    trimmed: &str,
    canonical: &Path,
    meta: &std::fs::Metadata,
) -> bool {
    // Mutations strictly require relative workspace paths; never mark absolute or `~/` reads writable.
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed == "~"
        || trimmed.starts_with("~/")
        || Path::new(trimmed).is_absolute()
    {
        return false;
    }
    if !is_metadata_writable(meta) {
        return false;
    }
    let Ok(canonical_cwd) = crate::platform::canonicalize_clean_async(cwd).await else {
        return false;
    };
    if !canonical.starts_with(&canonical_cwd) || is_path_inside_git(&canonical_cwd, canonical) {
        return false;
    }
    let direct_candidate = canonical_cwd.join(Path::new(trimmed));
    if let Ok(sym_meta) = tokio::fs::symlink_metadata(&direct_candidate).await
        && sym_meta.file_type().is_symlink()
    {
        return false;
    }
    true
}

/// Read a single workspace file safely on the host (UTF-8 source/text, raster image, PDF,
/// audio, video base64, or binary marker).
pub async fn read_workspace_file(
    cwd: &Path,
    raw_path: &str,
    max_bytes: Option<u64>,
) -> Result<WorkspaceFilePayload, String> {
    read_workspace_file_with_attachments_dir(cwd, raw_path, max_bytes, None).await
}

/// Read a single workspace or staged attachment file safely on the host, strictly enforcing
/// canonicalized containment inside `cwd` or managed artifact/attachment roots.
pub async fn read_workspace_file_with_attachments_dir(
    cwd: &Path,
    raw_path: &str,
    max_bytes: Option<u64>,
    extra_attachments_dir: Option<&Path>,
) -> Result<WorkspaceFilePayload, String> {
    let trimmed = raw_path.trim();
    let canonical = resolve_contained_file_path(cwd, trimmed, true, extra_attachments_dir).await?;
    let (mut file, meta) = open_validated_regular_file_async(&canonical, trimmed).await?;

    let size_bytes = meta.len();
    let mtime_unix_ms = metadata_mtime_unix_ms(&meta);
    let writable = compute_workspace_file_writable(cwd, trimmed, &canonical, &meta).await;
    let resolved_path = canonical.to_string_lossy().to_string();

    // Read a bounded 4 KiB header first so images, PDF, audio, and video files are classified
    // by magic bytes (never blindly by extension) without allocating tens of megabytes in memory.
    let mut header_buf =
        Vec::with_capacity((size_bytes as usize).min(MEDIA_SNIFF_PROBE_BYTES as usize));
    (&mut file)
        .take(MEDIA_SNIFF_PROBE_BYTES)
        .read_to_end(&mut header_buf)
        .await
        .map_err(|e| format!("Failed reading `{trimmed}`: {e}"))?;

    let filename_hint = canonical.file_name().and_then(|n| n.to_str());
    if let Some(sniffed) =
        orbit_protocol::media::sniff_media_or_viewer_image_mime(&header_buf, filename_hint)
    {
        let cap = match sniffed.modality {
            orbit_protocol::media::AttachmentModality::Image => max_bytes
                .unwrap_or(IMAGE_MAX_READ_BYTES)
                .clamp(4096, IMAGE_MAX_READ_BYTES),
            orbit_protocol::media::AttachmentModality::Pdf
            | orbit_protocol::media::AttachmentModality::Audio
            | orbit_protocol::media::AttachmentModality::Video => max_bytes
                .unwrap_or(MEDIA_MAX_READ_BYTES)
                .clamp(4096, MEDIA_MAX_READ_BYTES),
        };
        let kind_str = sniffed.modality.as_str().to_string();
        if size_bytes > cap {
            return Ok(WorkspaceFilePayload {
                resolved_path,
                content_kind: kind_str,
                mime_type: Some(sniffed.mime_type.to_string()),
                content: String::new(),
                size_bytes,
                truncated: true,
                etag: None,
                mtime_unix_ms,
                writable,
                line_ending: None,
            });
        }
        let mut bytes = header_buf;
        let remaining_cap = cap.saturating_sub(bytes.len() as u64).saturating_add(1);
        (&mut file)
            .take(remaining_cap)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| format!("Failed reading media `{trimmed}`: {e}"))?;
        if (bytes.len() as u64) > cap {
            return Ok(WorkspaceFilePayload {
                resolved_path,
                content_kind: kind_str,
                mime_type: Some(sniffed.mime_type.to_string()),
                content: String::new(),
                size_bytes: bytes.len() as u64,
                truncated: true,
                etag: None,
                mtime_unix_ms,
                writable,
                line_ending: None,
            });
        }
        let etag = Some(sha256_hex(&bytes));
        return Ok(WorkspaceFilePayload {
            resolved_path,
            content_kind: kind_str,
            mime_type: Some(sniffed.mime_type.to_string()),
            content: B64.encode(&bytes),
            size_bytes: bytes.len() as u64,
            truncated: false,
            etag,
            mtime_unix_ms,
            writable,
            line_ending: None,
        });
    }

    let cap = max_bytes
        .unwrap_or(DEFAULT_MAX_READ_BYTES)
        .clamp(4096, HARD_MAX_READ_BYTES);
    let mut buf = header_buf;
    if (buf.len() as u64) <= cap {
        let remaining = cap.saturating_sub(buf.len() as u64).saturating_add(1);
        (&mut file)
            .take(remaining)
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("Failed reading `{trimmed}`: {e}"))?;
    }

    let truncated = (buf.len() as u64) > cap || size_bytes > cap;
    if (buf.len() as u64) > cap {
        buf.truncate(cap as usize);
    }
    let etag = (!truncated).then(|| sha256_hex(&buf));
    let probe_len = buf.len().min(8192);
    if buf[..probe_len].contains(&0) {
        return Ok(WorkspaceFilePayload {
            resolved_path,
            content_kind: "binary".to_string(),
            mime_type: None,
            content: String::new(),
            size_bytes,
            truncated,
            etag,
            mtime_unix_ms,
            writable,
            line_ending: None,
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
                    etag,
                    mtime_unix_ms,
                    writable,
                    line_ending: None,
                });
            }
        }
    };
    let line_ending = Some(detect_line_ending(&text));

    Ok(WorkspaceFilePayload {
        resolved_path,
        content_kind: "text".to_string(),
        mime_type: Some("text/plain".to_string()),
        content: text,
        size_bytes,
        truncated,
        etag,
        mtime_unix_ms,
        writable,
        line_ending,
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
    let canon_root = crate::platform::canonicalize_clean_async(&repo_root)
        .await
        .unwrap_or_else(|_| repo_root.clone());
    let canon_cwd = crate::platform::canonicalize_clean_async(cwd)
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

const MAX_REWIND_PREVIEW_DIFF_BYTES_PER_FILE: usize = 64 * 1024;

fn normalize_safe_relative_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().replace('\\', "/");
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }
    let stripped = trimmed.trim_start_matches("./");
    if stripped.is_empty() || !is_safe_relative_repo_path(stripped) {
        return None;
    }
    let first = stripped.split('/').next().unwrap_or("");
    if first == ".git" {
        return None;
    }
    // Reject embedded `..` segments or leading/trailing double slashes cleanly
    let mut parts: Vec<&str> = Vec::new();
    for seg in stripped.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return None;
        }
        parts.push(seg);
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

fn validate_restore_path_filter(
    restore_paths: Option<&[String]>,
    subdir_scope: Option<&str>,
) -> anyhow::Result<Option<std::collections::HashSet<String>>> {
    let Some(paths) = restore_paths else {
        return Ok(None);
    };
    let mut allowed = std::collections::HashSet::with_capacity(paths.len());
    for raw in paths {
        let Some(norm) = normalize_safe_relative_path(raw) else {
            anyhow::bail!("invalid restore path rejected by path traversal guard: {raw}");
        };
        if let Some(sub) = subdir_scope {
            let sub_clean = sub.trim_matches('/');
            let prefixed = if norm == sub_clean || norm.starts_with(&format!("{sub_clean}/")) {
                norm
            } else {
                format!("{sub_clean}/{norm}")
            };
            allowed.insert(prefixed);
        } else {
            allowed.insert(norm);
        }
    }
    Ok(Some(allowed))
}

fn repo_rel_to_workspace_rel(rel_path: &str, subdir_scope: Option<&str>) -> String {
    if let Some(sub) = subdir_scope {
        let prefix = format!("{}/", sub.trim_matches('/'));
        if let Some(rest) = rel_path.strip_prefix(&prefix) {
            return rest.to_string();
        }
    }
    rel_path.to_string()
}

fn count_diff_add_del(diff: &str) -> (u32, u32) {
    let mut additions = 0u32;
    let mut deletions = 0u32;
    for line in diff.lines() {
        if line.starts_with("+++ ") || line.starts_with("--- ") {
            continue;
        }
        if line.starts_with('+') {
            additions = additions.saturating_add(1);
        } else if line.starts_with('-') {
            deletions = deletions.saturating_add(1);
        }
    }
    (additions, deletions)
}

fn split_git_diff_by_file(raw_diff: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut cur_path: Option<String> = None;
    let mut cur_buf = String::new();

    let flush = |path_opt: &mut Option<String>,
                 buf: &mut String,
                 out: &mut std::collections::HashMap<String, String>| {
        if let Some(path) = path_opt.take()
            && !buf.is_empty()
        {
            let truncated = if buf.len() > MAX_REWIND_PREVIEW_DIFF_BYTES_PER_FILE {
                let mut end = MAX_REWIND_PREVIEW_DIFF_BYTES_PER_FILE;
                while end > 0 && !buf.is_char_boundary(end) {
                    end -= 1;
                }
                let mut s = buf[..end].to_string();
                s.push_str("\n... (diff preview truncated)\n");
                s
            } else {
                std::mem::take(buf)
            };
            out.insert(path, truncated);
        }
        buf.clear();
    };

    for line in raw_diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush(&mut cur_path, &mut cur_buf, &mut map);
            // Parse `a/<path> b/<path>` (also handles quoted git paths cleanly)
            let parsed = rest
                .split(" b/")
                .nth(1)
                .map(|s| s.trim().trim_matches('"').to_string())
                .or_else(|| {
                    rest.split_whitespace()
                        .next()
                        .and_then(|s| s.trim_matches('"').strip_prefix("a/"))
                        .map(ToOwned::to_owned)
                });
            cur_path = parsed;
        }
        if cur_path.is_some() {
            if !cur_buf.is_empty() {
                cur_buf.push('\n');
            }
            cur_buf.push_str(line);
        }
    }
    flush(&mut cur_path, &mut cur_buf, &mut map);
    map
}

fn invert_unified_diff(diff: &str) -> String {
    let mut out = Vec::new();
    let mut old_hdr: Option<String> = None;
    for raw_line in diff.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.starts_with("--- ") {
            old_hdr = Some(line.to_string());
            continue;
        }
        if line.starts_with("+++ ") {
            let new_as_old = line.replacen("+++ ", "--- ", 1);
            out.push(new_as_old);
            if let Some(oh) = old_hdr.take() {
                out.push(oh.replacen("--- ", "+++ ", 1));
            }
            continue;
        }
        if let Some(oh) = old_hdr.take() {
            out.push(oh);
        }
        if line.starts_with("@@ ") {
            // Swap `-a,b +c,d` -> `-c,d +a,b`
            let parts: Vec<&str> = line.splitn(4, ' ').collect();
            if parts.len() >= 3
                && let (Some(minus_part), Some(plus_part)) =
                    (parts[1].strip_prefix('-'), parts[2].strip_prefix('+'))
            {
                let tail = parts.get(3).copied().unwrap_or("@@");
                out.push(format!("@@ -{plus_part} +{minus_part} {tail}"));
            } else {
                out.push(line.to_string());
            }
        } else if let Some(rest) = line.strip_prefix('+') {
            out.push(format!("-{rest}"));
        } else if let Some(rest) = line.strip_prefix('-') {
            out.push(format!("+{rest}"));
        } else {
            out.push(line.to_string());
        }
    }
    if let Some(oh) = old_hdr.take() {
        out.push(oh);
    }
    out.join("\n")
}

/// Preview the exact file-level restore actions, reverse unified diffs, and external conflict
/// warnings for rewinding a Git tree checkpoint without modifying the working tree.
pub async fn preview_git_tree_snapshot_restore(
    cwd: &Path,
    target_tree_oid: &str,
    recorded_edits: &[(String, String)],
) -> anyhow::Result<(Vec<RewindPreviewFile>, Vec<String>)> {
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
        anyhow::bail!("failed to inspect current workspace state before rewind preview");
    };
    if cur_tree_oid == target_tree_oid {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut status_cmd = Command::new("git");
    status_cmd.args([
        "diff",
        "--name-status",
        "-z",
        "--no-renames",
        target_tree_oid,
        &cur_tree_oid,
    ]);
    if let Some(ref rel) = subdir_scope {
        status_cmd.arg("--").arg(rel);
    }
    let status_out = status_cmd
        .current_dir(&repo_root)
        .kill_on_drop(true)
        .output()
        .await?;
    if !status_out.status.success() {
        let stderr = String::from_utf8_lossy(&status_out.stderr);
        anyhow::bail!("git diff checkpoint failed: {}", stderr.trim());
    }

    // Reverse direction (`cur -> target`) shows the exact changes rewinding will apply.
    let mut diff_cmd = Command::new("git");
    diff_cmd.args([
        "diff",
        "-U3",
        "--no-renames",
        "--no-color",
        &cur_tree_oid,
        target_tree_oid,
    ]);
    if let Some(ref rel) = subdir_scope {
        diff_cmd.arg("--").arg(rel);
    }
    let diff_out = diff_cmd
        .current_dir(&repo_root)
        .kill_on_drop(true)
        .output()
        .await?;
    let diffs_by_repo_rel = if diff_out.status.success() {
        split_git_diff_by_file(&String::from_utf8_lossy(&diff_out.stdout))
    } else {
        std::collections::HashMap::new()
    };

    // Map agent-recorded edits to repo-relative paths for external conflict detection.
    let canon_cwd = crate::platform::canonicalize_clean_async(cwd)
        .await
        .unwrap_or_else(|_| cwd.to_path_buf());
    let mut recorded_by_repo_rel: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for (raw_path, diff) in recorded_edits {
        let p = Path::new(raw_path.trim());
        let resolved_repo_rel = if p.is_absolute() {
            p.strip_prefix(&repo_root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .or_else(|| {
                    p.strip_prefix(&canon_cwd).ok().map(|r| {
                        let rel = r.to_string_lossy().replace('\\', "/");
                        match &subdir_scope {
                            Some(sub) => format!("{}/{}", sub.trim_matches('/'), rel),
                            None => rel,
                        }
                    })
                })
        } else {
            normalize_safe_relative_path(raw_path).map(|norm| match &subdir_scope {
                Some(sub) => {
                    let sub_clean = sub.trim_matches('/');
                    if norm == sub_clean || norm.starts_with(&format!("{sub_clean}/")) {
                        norm
                    } else {
                        format!("{sub_clean}/{norm}")
                    }
                }
                None => norm,
            })
        };
        if let Some(repo_rel) = resolved_repo_rel {
            recorded_by_repo_rel
                .entry(repo_rel)
                .or_default()
                .push(diff.clone());
        }
    }

    let mut preview_files = Vec::new();
    let mut external_conflicts = Vec::new();

    let mut fields = status_out
        .stdout
        .split(|&b| b == 0)
        .filter(|slice| !slice.is_empty());
    while let (Some(status_bytes), Some(path_bytes)) = (fields.next(), fields.next()) {
        let status = String::from_utf8_lossy(status_bytes);
        let repo_rel = String::from_utf8_lossy(path_bytes).into_owned();
        if !is_safe_relative_repo_path(&repo_rel) {
            continue;
        }
        let ws_rel = repo_rel_to_workspace_rel(&repo_rel, subdir_scope.as_deref());
        let action = if status.starts_with('A') {
            "delete".to_string()
        } else if status.starts_with('D') {
            "restore".to_string()
        } else {
            "modify".to_string()
        };

        let reverse_unified_diff = diffs_by_repo_rel
            .get(&repo_rel)
            .cloned()
            .unwrap_or_default();
        let (additions, deletions) = count_diff_add_del(&reverse_unified_diff);

        let mut has_external_conflict = false;
        if !recorded_by_repo_rel.is_empty() {
            if let Some(agent_diffs) = recorded_by_repo_rel.get(&repo_rel) {
                // Verify recorded edits still match disk (unless created/deleted as a whole file).
                if action == "modify"
                    && !agent_diffs
                        .iter()
                        .any(|d| diff_header_indicates_new_file(d))
                {
                    let abs = repo_root.join(&repo_rel);
                    if let Ok(mut content) = tokio::fs::read_to_string(&abs).await {
                        for d in agent_diffs.iter().rev() {
                            if let Some(reverted) = reverse_apply_unified_diff_to_text(&content, d)
                            {
                                content = reverted;
                            } else {
                                has_external_conflict = true;
                                break;
                            }
                        }
                    }
                }
            } else {
                // Changed on disk since pre-turn checkpoint, but not in agent's recorded FileEdits.
                has_external_conflict = true;
            }
        }

        if has_external_conflict {
            external_conflicts.push(ws_rel.clone());
        }

        preview_files.push(RewindPreviewFile {
            path: ws_rel,
            action,
            additions,
            deletions,
            reverse_unified_diff,
            has_external_conflict,
        });
    }

    preview_files.sort_by(|a, b| a.path.cmp(&b.path));
    external_conflicts.sort();
    external_conflicts.dedup();
    Ok((preview_files, external_conflicts))
}

/// Preview recorded-diff fallback restoration without touching disk.
pub async fn preview_file_edit_diffs_restore(
    cwd: &Path,
    edits: &[(String, String)],
) -> (Vec<RewindPreviewFile>, Vec<String>) {
    let Ok(canonical_cwd) = crate::platform::canonicalize_clean_async(cwd).await else {
        return (Vec::new(), Vec::new());
    };

    let mut grouped: Vec<(String, PathBuf, Vec<String>)> = Vec::new();
    let mut idx_by_rel: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for (raw_path, diff) in edits {
        let p = Path::new(raw_path.trim());
        if p.as_os_str().is_empty() {
            continue;
        }
        let candidate = if p.is_absolute() {
            p.to_path_buf()
        } else {
            let Some(norm) = normalize_safe_relative_path(raw_path) else {
                continue;
            };
            canonical_cwd.join(norm)
        };
        let Ok(sym_meta) = tokio::fs::symlink_metadata(&candidate).await else {
            continue;
        };
        if sym_meta.file_type().is_symlink() || !sym_meta.is_file() {
            continue;
        }
        let Ok(canonical_target) = crate::platform::canonicalize_clean_async(&candidate).await
        else {
            continue;
        };
        let Ok(rel) = canonical_target.strip_prefix(&canonical_cwd) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if !is_safe_relative_repo_path(&rel_str) {
            continue;
        }
        if let Some(&idx) = idx_by_rel.get(&rel_str) {
            grouped[idx].2.push(diff.clone());
        } else {
            let idx = grouped.len();
            idx_by_rel.insert(rel_str.clone(), idx);
            grouped.push((rel_str, canonical_target, vec![diff.clone()]));
        }
    }

    let mut files = Vec::new();
    let mut external_conflicts = Vec::new();

    for (rel_path, canonical_target, file_diffs) in grouped {
        let creates_file = file_diffs.iter().any(|d| diff_header_indicates_new_file(d));
        let mut reverse_parts: Vec<String> = Vec::new();
        let mut has_external_conflict = false;

        if creates_file {
            if let Ok(cur) = tokio::fs::read_to_string(&canonical_target).await {
                let line_count = cur.lines().count();
                let mut diff_out = format!(
                    "diff --git a/{rel_path} b/{rel_path}\n--- a/{rel_path}\n+++ /dev/null\n@@ -1,{line_count} +0,0 @@\n"
                );
                for line in cur.lines() {
                    diff_out.push('-');
                    diff_out.push_str(line);
                    diff_out.push('\n');
                }
                reverse_parts.push(diff_out);
            }
        } else if let Ok(mut simulated) = tokio::fs::read_to_string(&canonical_target).await {
            for d in file_diffs.iter().rev() {
                if let Some(reverted) = reverse_apply_unified_diff_to_text(&simulated, d) {
                    simulated = reverted;
                    reverse_parts.push(invert_unified_diff(d));
                } else {
                    has_external_conflict = true;
                    reverse_parts.push(invert_unified_diff(d));
                }
            }
        } else {
            has_external_conflict = true;
        }

        let reverse_unified_diff = reverse_parts.join("\n");
        let (additions, deletions) = count_diff_add_del(&reverse_unified_diff);
        if has_external_conflict {
            external_conflicts.push(rel_path.clone());
        }
        files.push(RewindPreviewFile {
            path: rel_path,
            action: if creates_file {
                "delete".into()
            } else {
                "modify".into()
            },
            additions,
            deletions,
            reverse_unified_diff,
            has_external_conflict,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    external_conflicts.sort();
    external_conflicts.dedup();
    (files, external_conflicts)
}

/// Restore only the files within `cwd` that changed since `target_tree_oid`, leaving untouched
/// files, monorepo sibling subdirectories, and the user's real `.git/index` intact.
pub async fn restore_git_tree_snapshot(cwd: &Path, target_tree_oid: &str) -> anyhow::Result<u32> {
    restore_git_tree_snapshot_filtered(cwd, target_tree_oid, None).await
}

/// Restore changed files within `cwd` back to `target_tree_oid`, optionally restricted to an
/// explicit user-selected subset `restore_paths` (validated against path traversal).
pub async fn restore_git_tree_snapshot_filtered(
    cwd: &Path,
    target_tree_oid: &str,
    restore_paths: Option<&[String]>,
) -> anyhow::Result<u32> {
    if !is_valid_git_oid(target_tree_oid) {
        anyhow::bail!("invalid checkpoint tree id");
    }
    let (repo_root, subdir_scope) = resolve_git_scope(cwd)
        .await
        .ok_or_else(|| anyhow::anyhow!("workspace is not a git repository"))?;
    let allowed_repo_paths = validate_restore_path_filter(restore_paths, subdir_scope.as_deref())?;
    if let Some(ref allowed) = allowed_repo_paths
        && allowed.is_empty()
    {
        return Ok(0);
    }

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
        if let Some(ref allowed) = allowed_repo_paths
            && !allowed.contains(&rel_path)
        {
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
    reverse_apply_file_edit_diffs_filtered(cwd, edits, None)
        .await
        .unwrap_or(0)
}

/// Filtered fallback restoration with strict path traversal guards.
pub async fn reverse_apply_file_edit_diffs_filtered(
    cwd: &Path,
    edits: &[(String, String)],
    restore_paths: Option<&[String]>,
) -> anyhow::Result<u32> {
    let allowed = validate_restore_path_filter(restore_paths, None)?;
    if let Some(ref set) = allowed
        && set.is_empty()
    {
        return Ok(0);
    }
    let Ok(canonical_cwd) = crate::platform::canonicalize_clean_async(cwd).await else {
        return Ok(0);
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
        let Ok(canonical_target) = crate::platform::canonicalize_clean_async(&candidate).await
        else {
            continue;
        };
        let Ok(rel_target) = canonical_target.strip_prefix(&canonical_cwd) else {
            continue;
        };
        if let Some(ref allowed_set) = allowed {
            let rel_str = rel_target.to_string_lossy().replace('\\', "/");
            if !allowed_set.contains(&rel_str) {
                continue;
            }
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

    Ok(restored_paths.len() as u32)
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

        // 3. Raster image base64 (verified by PNG magic bytes, not extension alone)
        let img_path = dir.join("pixel.png");
        let png_sig = b"\x89PNG\r\n\x1a\n";
        tokio::fs::write(&img_path, png_sig)
            .await
            .expect("write png");
        let img_res = read_workspace_file(&dir, "pixel.png", None)
            .await
            .expect("read png");
        assert_eq!(img_res.content_kind, "image");
        assert_eq!(img_res.mime_type.as_deref(), Some("image/png"));
        assert_eq!(img_res.content, B64.encode(png_sig));

        // Spoofed `.png` containing plain text MUST NOT be classified as image
        let fake_png = dir.join("fake.png");
        tokio::fs::write(&fake_png, "not actually a png")
            .await
            .expect("write fake png");
        let fake_png_res = read_workspace_file(&dir, "fake.png", None)
            .await
            .expect("read fake png");
        assert_eq!(fake_png_res.content_kind, "text");

        // UTF-16LE BOM file MUST NOT be misclassified as audio/mpeg
        let utf16_path = dir.join("utf16.txt");
        tokio::fs::write(&utf16_path, &[0xFF, 0xFE, 0x41, 0x00, 0x42, 0x00])
            .await
            .expect("write utf16");
        let utf16_res = read_workspace_file(&dir, "utf16.txt", None)
            .await
            .expect("read utf16");
        assert_eq!(utf16_res.content_kind, "binary");

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
            vec!["config", "core.autocrlf", "false"],
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

    #[tokio::test]
    async fn read_workspace_file_classifies_pdf_audio_video_and_enforces_caps_and_allowlist() {
        let root = std::env::temp_dir().join(format!("orbit_media_read_{}", Uuid::new_v4()));
        let ws = root.join("workspace");
        let att_dir = root.join("staged_attachments");
        let outside = root.join("outside");
        tokio::fs::create_dir_all(&ws).await.expect("mkdir ws");
        tokio::fs::create_dir_all(&att_dir)
            .await
            .expect("mkdir att");
        tokio::fs::create_dir_all(&outside)
            .await
            .expect("mkdir outside");

        // 1. PDF inside workspace
        let pdf_bytes = b"%PDF-1.7\n1 0 obj << /Type /Page >> endobj\n%%EOF\n";
        tokio::fs::write(ws.join("spec.pdf"), pdf_bytes)
            .await
            .expect("write pdf");
        let pdf_res = read_workspace_file(&ws, "spec.pdf", None)
            .await
            .expect("read pdf");
        assert_eq!(pdf_res.content_kind, "pdf");
        assert_eq!(pdf_res.mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(pdf_res.content, B64.encode(pdf_bytes));
        assert!(!pdf_res.truncated);

        // 2. Audio (MP3 & WAV)
        let mp3_bytes = b"ID3\x04\x00\x00\x00\x00\x00\x00voice-note";
        tokio::fs::write(ws.join("memo.mp3"), mp3_bytes)
            .await
            .expect("write mp3");
        let mp3_res = read_workspace_file(&ws, "memo.mp3", None)
            .await
            .expect("read mp3");
        assert_eq!(mp3_res.content_kind, "audio");
        assert_eq!(mp3_res.mime_type.as_deref(), Some("audio/mpeg"));
        assert_eq!(mp3_res.content, B64.encode(mp3_bytes));

        // 3. Video (MP4) staged in attachments_dir (outside workspace cwd, explicitly allowlisted)
        let mp4_bytes = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00isomvideo";
        let staged_video = att_dir.join("demo.mp4");
        tokio::fs::write(&staged_video, mp4_bytes)
            .await
            .expect("write staged mp4");
        let vid_res = read_workspace_file_with_attachments_dir(
            &ws,
            staged_video.to_str().unwrap_or(""),
            None,
            Some(&att_dir),
        )
        .await
        .expect("read staged video");
        assert_eq!(vid_res.content_kind, "video");
        assert_eq!(vid_res.mime_type.as_deref(), Some("video/mp4"));
        assert_eq!(vid_res.content, B64.encode(mp4_bytes));

        // 4. Size cap enforcement returns media content_kind with truncated = true and empty body
        let mut large_wav = b"RIFF\x00\x00\x00\x00WAVEfmt ".to_vec();
        large_wav.resize(8192, 0x11);
        tokio::fs::write(ws.join("big.wav"), &large_wav)
            .await
            .expect("write big wav");
        let capped_res = read_workspace_file(&ws, "big.wav", Some(4096))
            .await
            .expect("read capped wav");
        assert_eq!(capped_res.content_kind, "audio");
        assert_eq!(capped_res.mime_type.as_deref(), Some("audio/wav"));
        assert!(capped_res.truncated);
        assert!(capped_res.content.is_empty());

        // 5. Spoofed `.pdf` containing plain text is NOT classified as pdf
        tokio::fs::write(ws.join("fake.pdf"), "plain text masquerading as pdf")
            .await
            .expect("write fake pdf");
        let fake_res = read_workspace_file(&ws, "fake.pdf", None)
            .await
            .expect("read fake pdf");
        assert_eq!(fake_res.content_kind, "text");

        // 6. Outside PDF blocked even when extra_attachments_dir is configured
        let secret_pdf = outside.join("secret.pdf");
        tokio::fs::write(&secret_pdf, pdf_bytes)
            .await
            .expect("write outside pdf");
        assert!(
            read_workspace_file_with_attachments_dir(
                &ws,
                secret_pdf.to_str().unwrap_or(""),
                None,
                Some(&att_dir),
            )
            .await
            .is_err()
        );

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn subagent_worktree_reads_allow_live_and_deleted_fallback_readonly_and_block_escapes() {
        let root = std::env::temp_dir().join(format!("orbit_wt_read_{}", Uuid::new_v4()));
        let ws = root.join("repo_a");
        let repo_b = root.join("repo_b");
        let agy_wt_root = root.join("agy_data").join("worktrees");
        let agy_live_wt = agy_wt_root.join("conv-123").join("subagent-track-1-repo-a");
        let agy_other_repo_wt = agy_wt_root.join("conv-456").join("subagent-other-repo-b");
        let orbit_wt_root = root.join("orbit_home").join("worktrees");
        let orbit_live_wt = orbit_wt_root.join("repo-a-feat-foo");
        let orbit_other_repo_wt = orbit_wt_root.join("repo-b-feat-bar");
        let outside = root.join("outside_secrets");

        // Set up real Git common dir + worktree gitdir/commondir links for repo_a and repo_b
        let ws_git = ws.join(".git");
        let ws_wt_agy_gitdir = ws_git.join("worktrees").join("subagent-track-1-repo-a");
        let ws_wt_orbit_gitdir = ws_git.join("worktrees").join("repo-a-feat-foo");
        let repo_b_git = repo_b.join(".git");
        let repo_b_wt_agy_gitdir = repo_b_git.join("worktrees").join("subagent-other-repo-b");
        let repo_b_wt_orbit_gitdir = repo_b_git.join("worktrees").join("repo-b-feat-bar");

        for dir in [
            ws.join("src"),
            ws_wt_agy_gitdir.clone(),
            ws_wt_orbit_gitdir.clone(),
            repo_b.join("src"),
            repo_b_wt_agy_gitdir.clone(),
            repo_b_wt_orbit_gitdir.clone(),
            agy_live_wt.join("src"),
            agy_other_repo_wt.join("src"),
            orbit_live_wt.join("src"),
            orbit_other_repo_wt.join("src"),
            outside.clone(),
        ] {
            tokio::fs::create_dir_all(&dir).await.expect("mkdir");
        }
        for (wt_checkout, gitdir) in [
            (&agy_live_wt, &ws_wt_agy_gitdir),
            (&orbit_live_wt, &ws_wt_orbit_gitdir),
            (&agy_other_repo_wt, &repo_b_wt_agy_gitdir),
            (&orbit_other_repo_wt, &repo_b_wt_orbit_gitdir),
        ] {
            tokio::fs::write(gitdir.join("commondir"), "../..\n")
                .await
                .expect("write commondir");
            tokio::fs::write(
                wt_checkout.join(".git"),
                format!("gitdir: {}\n", gitdir.display()),
            )
            .await
            .expect("write wt .git");
        }

        let primary_file = ws.join("src").join("lib.rs");
        let agy_live_file = agy_live_wt.join("src").join("lib.rs");
        let agy_other_repo_file = agy_other_repo_wt.join("src").join("secret_b.rs");
        let orbit_live_file = orbit_live_wt.join("src").join("lib.rs");
        let orbit_other_repo_file = orbit_other_repo_wt.join("src").join("secret_b.rs");
        let outside_file = outside.join("secret.txt");

        tokio::fs::write(&primary_file, "pub fn primary() {}\n")
            .await
            .expect("write primary");
        tokio::fs::write(&agy_live_file, "pub fn agy_live() {}\n")
            .await
            .expect("write agy live");
        tokio::fs::write(&agy_other_repo_file, "pub fn repo_b_secret() {}\n")
            .await
            .expect("write agy other");
        tokio::fs::write(&orbit_live_file, "pub fn orbit_live() {}\n")
            .await
            .expect("write orbit live");
        tokio::fs::write(&orbit_other_repo_file, "pub fn repo_b_secret() {}\n")
            .await
            .expect("write orbit other");
        tokio::fs::write(&outside_file, "top-secret\n")
            .await
            .expect("write outside");

        let roots = ManagedWorktreeRoots {
            agy_worktree_roots: vec![agy_wt_root.clone()],
            orbit_worktree_roots: vec![orbit_wt_root.clone()],
        };
        let canon_primary = crate::platform::canonicalize_clean_async(&primary_file)
            .await
            .expect("canon primary");
        let canon_agy_live = crate::platform::canonicalize_clean_async(&agy_live_file)
            .await
            .expect("canon agy live");
        let canon_orbit_live = crate::platform::canonicalize_clean_async(&orbit_live_file)
            .await
            .expect("canon orbit live");

        // (a) Same-repo live Antigravity and Orbit worktree files are allowed and strictly read-only
        let agy_raw = agy_live_file.to_string_lossy().to_string();
        let resolved_agy =
            resolve_contained_file_path_with_roots(&ws, &agy_raw, true, None, &roots)
                .await
                .expect("same-repo live agy worktree file must resolve");
        assert_eq!(resolved_agy, canon_agy_live);
        let agy_meta = tokio::fs::metadata(&resolved_agy).await.expect("meta");
        assert!(
            !compute_workspace_file_writable(&ws, &agy_raw, &resolved_agy, &agy_meta).await,
            "live agy worktree read must remain strictly read-only"
        );

        let orbit_raw = orbit_live_file.to_string_lossy().to_string();
        let resolved_orbit =
            resolve_contained_file_path_with_roots(&ws, &orbit_raw, true, None, &roots)
                .await
                .expect("same-repo live orbit worktree file must resolve");
        assert_eq!(resolved_orbit, canon_orbit_live);
        let orbit_meta = tokio::fs::metadata(&resolved_orbit).await.expect("meta");
        assert!(
            !compute_workspace_file_writable(&ws, &orbit_raw, &resolved_orbit, &orbit_meta).await,
            "live orbit worktree read must remain strictly read-only"
        );

        // (a2) Cross-repository worktree files MUST BE STRICTLY DENIED (repo_a cannot read repo_b's worktree!)
        assert!(
            resolve_contained_file_path_with_roots(
                &ws,
                &agy_other_repo_file.to_string_lossy(),
                true,
                None,
                &roots,
            )
            .await
            .is_err(),
            "repo A must NEVER read a file from repo B's Antigravity worktree"
        );
        assert!(
            resolve_contained_file_path_with_roots(
                &ws,
                &orbit_other_repo_file.to_string_lossy(),
                true,
                None,
                &roots,
            )
            .await
            .is_err(),
            "repo A must NEVER read a file from repo B's Orbit worktree"
        );

        // (b) Deleted worktree files gracefully fall back to primary workspace `cwd` (read-only)
        let deleted_agy_path = agy_wt_root
            .join("conv-999")
            .join("deleted-subagent-wt")
            .join("src")
            .join("lib.rs");
        let deleted_agy_raw = deleted_agy_path.to_string_lossy().to_string();
        let fallback_agy =
            resolve_contained_file_path_with_roots(&ws, &deleted_agy_raw, true, None, &roots)
                .await
                .expect("deleted agy worktree must fall back to primary cwd");
        assert_eq!(fallback_agy, canon_primary);
        let fb_meta = tokio::fs::metadata(&fallback_agy).await.expect("meta");
        assert!(
            !compute_workspace_file_writable(&ws, &deleted_agy_raw, &fallback_agy, &fb_meta).await,
            "fallback absolute worktree read must remain strictly read-only"
        );

        let deleted_orbit_path = orbit_wt_root
            .join("deleted-orbit-wt")
            .join("src")
            .join("lib.rs");
        let deleted_orbit_raw = deleted_orbit_path.to_string_lossy().to_string();
        let fallback_orbit =
            resolve_contained_file_path_with_roots(&ws, &deleted_orbit_raw, true, None, &roots)
                .await
                .expect("deleted orbit worktree must fall back to primary cwd");
        assert_eq!(fallback_orbit, canon_primary);

        // (c) Traversal, outside-root escapes, and symlink opens remain strictly denied
        let traversal_agy = format!(
            "{}/conv-999/deleted-wt/../../outside_secrets/secret.txt",
            agy_wt_root.display()
        );
        assert!(
            resolve_contained_file_path_with_roots(&ws, &traversal_agy, true, None, &roots)
                .await
                .is_err(),
            "parent-dir traversal inside worktree path must be denied"
        );
        assert!(
            resolve_contained_file_path_with_roots(
                &ws,
                outside_file.to_str().unwrap_or(""),
                true,
                None,
                &roots,
            )
            .await
            .is_err(),
            "outside secret file must remain denied"
        );
        assert!(
            resolve_contained_file_path_with_roots(&ws, &agy_raw, false, None, &roots)
                .await
                .is_err(),
            "worktree read must be denied when allow_managed_artifacts is false"
        );

        #[cfg(unix)]
        {
            let sym = ws.join("src").join("swap_link.rs");
            std::os::unix::fs::symlink(&outside_file, &sym).expect("create symlink");
            assert!(
                open_validated_regular_file_async(&sym, "src/swap_link.rs")
                    .await
                    .is_err(),
                "TOCTOU-hardened open must reject symlinks directly"
            );
        }

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn preview_and_filtered_restore_git_snapshot_detects_conflicts_and_blocks_traversal() {
        let dir =
            std::env::temp_dir().join(format!("orbit_rewind_preview_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.expect("mkdir");
        let run_git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .status()
                .expect("run git");
            assert!(status.success(), "git {:?} failed", args);
        };
        run_git(&["init"]);
        run_git(&["config", "user.email", "test@orbit.local"]);
        run_git(&["config", "user.name", "Orbit Test"]);

        tokio::fs::write(dir.join("a.txt"), "alpha-v1\n")
            .await
            .expect("write a");
        tokio::fs::write(dir.join("b.txt"), "beta-v1\n")
            .await
            .expect("write b");
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "init"]);

        let snap = capture_git_tree_snapshot(&dir)
            .await
            .expect("capture pre-turn checkpoint");

        // Agent edits a.txt; external user edits b.txt; agent creates c.txt
        tokio::fs::write(dir.join("a.txt"), "alpha-v2\n")
            .await
            .expect("edit a");
        tokio::fs::write(dir.join("b.txt"), "beta-external\n")
            .await
            .expect("edit b");
        tokio::fs::write(dir.join("c.txt"), "created\n")
            .await
            .expect("create c");

        let recorded = vec![(
            "a.txt".to_string(),
            "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-alpha-v1\n+alpha-v2\n".to_string(),
        )];

        let (preview_files, conflicts) = preview_git_tree_snapshot_restore(&dir, &snap, &recorded)
            .await
            .expect("preview rewind");

        assert_eq!(preview_files.len(), 3);
        let a_prev = preview_files
            .iter()
            .find(|f| f.path == "a.txt")
            .expect("a.txt");
        assert_eq!(a_prev.action, "modify");
        assert!(!a_prev.has_external_conflict);
        assert!(a_prev.reverse_unified_diff.contains("-alpha-v2"));
        assert!(a_prev.reverse_unified_diff.contains("+alpha-v1"));

        let b_prev = preview_files
            .iter()
            .find(|f| f.path == "b.txt")
            .expect("b.txt");
        assert!(b_prev.has_external_conflict);
        assert!(conflicts.contains(&"b.txt".to_string()));

        let c_prev = preview_files
            .iter()
            .find(|f| f.path == "c.txt")
            .expect("c.txt");
        assert_eq!(c_prev.action, "delete");

        // Path traversal guard MUST reject `..`, absolute paths, and `.git/...`
        for bad in ["../escape.txt", "/etc/passwd", ".git/config", "sub/../../x"] {
            let err =
                restore_git_tree_snapshot_filtered(&dir, &snap, Some(&[bad.to_string()])).await;
            assert!(err.is_err(), "expected traversal rejection for {bad}");
        }

        // Filtered restore: only restore a.txt & c.txt, leaving external edit in b.txt untouched
        let selected = vec!["a.txt".to_string(), "c.txt".to_string()];
        let restored = restore_git_tree_snapshot_filtered(&dir, &snap, Some(&selected))
            .await
            .expect("filtered restore");
        assert_eq!(restored, 2);
        assert_eq!(
            tokio::fs::read_to_string(dir.join("a.txt"))
                .await
                .expect("read a"),
            "alpha-v1\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(dir.join("b.txt"))
                .await
                .expect("read b"),
            "beta-external\n"
        );
        assert!(!dir.join("c.txt").exists());

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
