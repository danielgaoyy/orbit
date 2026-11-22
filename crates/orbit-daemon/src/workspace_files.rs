//! Pure-Rust atomic workspace file writer, optimistic concurrency conflict detector,
//! and strict security-bounded file tree mutation engine (`create` / `rename` / `delete`).

use std::fmt::Write as _;
use std::fs::{self, Metadata, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use orbit_protocol::WorkspaceFileConflict;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Hard cap on editable text file writes (4 MiB).
pub const MAX_WORKSPACE_FILE_WRITE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct WriteWorkspaceFileOutcome {
    pub etag: String,
    pub size_bytes: u64,
    pub mtime_unix_ms: Option<i64>,
    pub conflict: Option<WorkspaceFileConflict>,
}

#[derive(Debug, Clone)]
pub struct MutateWorkspaceEntryOutcome {
    pub path: String,
    pub new_path: Option<String>,
    pub action: String,
}

/// Compute lowercase hex SHA-256 digest of exact raw bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Extract file modification timestamp as Unix milliseconds.
pub fn metadata_mtime_unix_ms(meta: &Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|t| match t.duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_millis()).ok(),
            Err(e) => i64::try_from(e.duration().as_millis()).ok().map(|v| -v),
        })
}

/// Detect dominant line ending (`"lf"` | `"crlf"` | `"mixed"`).
pub fn detect_line_ending(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut crlf = 0usize;
    let mut lf = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            crlf += 1;
            i += 2;
            continue;
        }
        if bytes[i] == b'\n' {
            lf += 1;
        }
        i += 1;
    }
    if crlf > 0 && lf > 0 {
        "mixed".to_string()
    } else if crlf > 0 {
        "crlf".to_string()
    } else {
        "lf".to_string()
    }
}

/// Returns `true` if ANY component of `path` (or `canonical_cwd`) is `.git` (case-insensitive).
/// Checking the full path ensures `cwd = "/repo/.git"` is ALWAYS blocked.
pub fn is_path_inside_git(canonical_cwd: &Path, path: &Path) -> bool {
    let has_git_segment = |p: &Path| {
        p.components().any(|c| {
            matches!(
                c,
                Component::Normal(seg) if seg.to_string_lossy().eq_ignore_ascii_case(".git")
            )
        })
    };
    has_git_segment(path) || has_git_segment(canonical_cwd)
}

/// Check whether metadata represents a writable file.
pub fn is_metadata_writable(meta: &Metadata) -> bool {
    !meta.permissions().readonly()
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

/// Validate a relative workspace mutation path and return normalized path segments.
///
/// Rejects:
/// - empty or whitespace-only paths
/// - NUL or any control characters
/// - absolute paths, leading `/` or `\`, home prefixes (`~`, `~/`)
/// - `..` traversal segments or Windows prefixes
/// - backslashes (`\`)
/// - workspace root itself (`.` or `./`)
/// - `.git` or any path nested inside `.git/` (case-insensitive for APFS/NTFS safety)
fn validate_rel_segments(raw_path: &str) -> Result<Vec<String>, String> {
    if raw_path.is_empty() || raw_path.trim().is_empty() {
        return Err("Path cannot be empty".to_string());
    }
    if raw_path.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("Path contains invalid control characters".to_string());
    }
    if raw_path.contains('\\') {
        return Err("Path must use forward slashes (`/`)".to_string());
    }
    let trimmed = raw_path.trim();
    if trimmed.starts_with('/') || trimmed == "~" || trimmed.starts_with("~/") {
        return Err(format!("Absolute path `{trimmed}` is not allowed"));
    }
    let p = Path::new(trimmed);
    if p.is_absolute() {
        return Err(format!("Absolute path `{trimmed}` is not allowed"));
    }

    let mut segments = Vec::new();
    for comp in p.components() {
        match comp {
            Component::Normal(os) => {
                let Some(seg) = os.to_str() else {
                    return Err("Path contains non-UTF-8 characters".to_string());
                };
                if seg.is_empty() || seg == "." || seg == ".." {
                    return Err(format!("Invalid path segment `{seg}`"));
                }
                if seg.eq_ignore_ascii_case(".git") {
                    return Err("Modifying `.git` internals is forbidden".to_string());
                }
                segments.push(seg.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!("Path `{trimmed}` escapes the workspace directory"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Absolute path `{trimmed}` is not allowed"));
            }
        }
    }

    if segments.is_empty() {
        return Err("Refusing to mutate workspace root directory".to_string());
    }

    Ok(segments)
}

/// Canonicalize `cwd` and reject unsafe workspace roots (filesystem root, `$HOME` root,
/// non-directories, or any path containing `.git`).
pub fn canonicalize_workspace_root(cwd: &Path) -> Result<PathBuf, String> {
    let canonical_cwd =
        fs::canonicalize(cwd).map_err(|e| format!("Workspace directory unavailable: {e}"))?;
    let meta = fs::metadata(&canonical_cwd)
        .map_err(|e| format!("Workspace directory unavailable: {e}"))?;
    if !meta.is_dir() {
        return Err("Workspace root is not a directory".to_string());
    }
    if canonical_cwd.parent().is_none() {
        return Err("Refusing to mutate filesystem root directory".to_string());
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let canon_home = fs::canonicalize(&home).unwrap_or(home);
        if canonical_cwd == canon_home {
            return Err("Refusing to mutate home directory root".to_string());
        }
    }
    if is_path_inside_git(&canonical_cwd, &canonical_cwd) {
        return Err("Modifying `.git` internals is forbidden".to_string());
    }
    Ok(canonical_cwd)
}

/// Walk every parent segment of `segments` from `canonical_cwd` downward, verifying that no
/// existing parent symlink escapes `canonical_cwd` or redirects into `.git/`.
/// When `create_missing` is `true`, missing intermediate parent directories are safely created
/// one segment at a time within `canonical_cwd`.
fn resolve_parent_dir_strict(
    canonical_cwd: &Path,
    parent_segments: &[String],
    create_missing: bool,
) -> Result<PathBuf, String> {
    let mut current = canonical_cwd.to_path_buf();

    for seg in parent_segments {
        let next = current.join(seg);
        match fs::symlink_metadata(&next) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    let canon = fs::canonicalize(&next).map_err(|e| {
                        format!("Refusing to traverse invalid symlink `{seg}`: {e}")
                    })?;
                    if !canon.starts_with(canonical_cwd) {
                        return Err(format!(
                            "Refusing to follow symlink `{seg}` outside workspace directory"
                        ));
                    }
                    if is_path_inside_git(canonical_cwd, &canon) {
                        return Err("Modifying `.git` internals is forbidden".to_string());
                    }
                    if !canon.is_dir() {
                        return Err(format!("Parent `{seg}` is not a directory"));
                    }
                    current = canon;
                } else {
                    if !meta.is_dir() {
                        return Err(format!("Parent `{seg}` is not a directory"));
                    }
                    let canon = fs::canonicalize(&next)
                        .map_err(|e| format!("Cannot resolve parent `{seg}`: {e}"))?;
                    if !canon.starts_with(canonical_cwd) {
                        return Err("Parent directory escapes workspace directory".to_string());
                    }
                    if is_path_inside_git(canonical_cwd, &canon) {
                        return Err("Modifying `.git` internals is forbidden".to_string());
                    }
                    current = canon;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !create_missing {
                    return Err(format!("Parent directory `{seg}` does not exist"));
                }
                fs::create_dir(&next)
                    .map_err(|ce| format!("Failed creating parent directory `{seg}`: {ce}"))?;
                let canon = fs::canonicalize(&next)
                    .map_err(|ce| format!("Cannot resolve created directory `{seg}`: {ce}"))?;
                if !canon.starts_with(canonical_cwd) || is_path_inside_git(canonical_cwd, &canon) {
                    return Err("Created directory escapes workspace boundary".to_string());
                }
                current = canon;
            }
            Err(e) => {
                return Err(format!("Cannot inspect parent `{seg}`: {e}"));
            }
        }
    }

    Ok(current)
}

/// Inspect existing parent directories WITHOUT creating anything, verifying none escape
/// `canonical_cwd`, touch `.git`, or resolve inside `forbidden_ancestor` (if provided).
fn verify_existing_parents_not_inside(
    canonical_cwd: &Path,
    parent_segments: &[String],
    forbidden_ancestor: Option<&Path>,
) -> Result<(), String> {
    let mut current = canonical_cwd.to_path_buf();
    for seg in parent_segments {
        let next = current.join(seg);
        match fs::symlink_metadata(&next) {
            Ok(meta) => {
                let canon = if meta.file_type().is_symlink() {
                    fs::canonicalize(&next)
                        .map_err(|e| format!("Refusing to traverse invalid symlink `{seg}`: {e}"))?
                } else {
                    if !meta.is_dir() {
                        return Err(format!("Parent `{seg}` is not a directory"));
                    }
                    fs::canonicalize(&next)
                        .map_err(|e| format!("Cannot resolve parent `{seg}`: {e}"))?
                };
                if !canon.starts_with(canonical_cwd) {
                    return Err(format!("Parent `{seg}` escapes the workspace directory"));
                }
                if is_path_inside_git(canonical_cwd, &canon) {
                    return Err("Modifying `.git` internals is forbidden".to_string());
                }
                if !canon.is_dir() {
                    return Err(format!("Parent `{seg}` is not a directory"));
                }
                if let Some(anc) = forbidden_ancestor
                    && (canon == anc || canon.starts_with(anc))
                {
                    return Err("Cannot move directory inside itself".to_string());
                }
                current = canon;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(format!("Cannot inspect parent `{seg}`: {e}")),
        }
    }
    Ok(())
}

/// Resolve a symlink target relative to `anchor_parent` and check whether it stays strictly
/// inside `canonical_cwd` and outside `.git/`.
fn link_target_escapes_or_targets_git(
    canonical_cwd: &Path,
    anchor_parent: &Path,
    link_dest: &Path,
) -> bool {
    let candidate = if link_dest.is_absolute() {
        link_dest.to_path_buf()
    } else {
        let mut normalized = anchor_parent.to_path_buf();
        for comp in link_dest.components() {
            match comp {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !normalized.pop() {
                        return true;
                    }
                }
                Component::Normal(seg) => normalized.push(seg),
                Component::RootDir | Component::Prefix(_) => return true,
            }
        }
        normalized
    };

    if !candidate.starts_with(canonical_cwd) || is_path_inside_git(canonical_cwd, &candidate) {
        return true;
    }

    if let Ok(canon) = fs::canonicalize(&candidate)
        && (!canon.starts_with(canonical_cwd) || is_path_inside_git(canonical_cwd, &canon))
    {
        return true;
    }

    false
}

/// Check if a symlink at `target` escapes `canonical_cwd` or points into `.git/`.
/// Dangling symlinks whose lexical/read_link target points outside `canonical_cwd` are treated as escaping.
fn symlink_escapes_or_targets_git(canonical_cwd: &Path, target: &Path) -> bool {
    if let Ok(canon) = fs::canonicalize(target) {
        return !canon.starts_with(canonical_cwd) || is_path_inside_git(canonical_cwd, &canon);
    }
    let Ok(link_dest) = fs::read_link(target) else {
        return true;
    };
    let Some(parent) = target.parent() else {
        return true;
    };
    link_target_escapes_or_targets_git(canonical_cwd, parent, &link_dest)
}

/// Atomically write UTF-8 text to a workspace file with SHA-256 optimistic concurrency detection.
pub async fn write_workspace_file(
    cwd: &Path,
    raw_path: &str,
    content: &str,
    expected_etag: Option<&str>,
    force: bool,
) -> Result<WriteWorkspaceFileOutcome, String> {
    if content.len() > MAX_WORKSPACE_FILE_WRITE_BYTES {
        return Err(format!(
            "File content exceeds {} MiB write limit",
            MAX_WORKSPACE_FILE_WRITE_BYTES / (1024 * 1024)
        ));
    }

    let cwd_owned = cwd.to_path_buf();
    let path_owned = raw_path.to_string();
    let content_owned = content.to_string();
    let expected_owned = expected_etag
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    tokio::task::spawn_blocking(move || {
        write_workspace_file_sync(
            &cwd_owned,
            &path_owned,
            &content_owned,
            expected_owned.as_deref(),
            force,
        )
    })
    .await
    .map_err(|_| "Workspace file write task cancelled".to_string())?
}

fn write_workspace_file_sync(
    cwd: &Path,
    raw_path: &str,
    content: &str,
    expected_etag: Option<&str>,
    force: bool,
) -> Result<WriteWorkspaceFileOutcome, String> {
    let segments = validate_rel_segments(raw_path)?;
    let canonical_cwd = canonicalize_workspace_root(cwd)?;
    let (file_name, parent_segments) = segments
        .split_last()
        .ok_or_else(|| "Path cannot be empty".to_string())?;
    let parent_dir = resolve_parent_dir_strict(&canonical_cwd, parent_segments, true)?;
    let target = parent_dir.join(file_name);

    #[cfg(unix)]
    let mut preserved_mode: Option<u32> = None;

    match fs::symlink_metadata(&target) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "Refusing to write through symlink `{}`",
                    segments.join("/")
                ));
            }
            if !meta.is_file() {
                return Err(format!(
                    "Target `{}` is not a regular file",
                    segments.join("/")
                ));
            }
            if !is_metadata_writable(&meta) {
                return Err(format!(
                    "File `{}` is read-only and cannot be overwritten",
                    segments.join("/")
                ));
            }
            let canon_target = fs::canonicalize(&target)
                .map_err(|e| format!("Cannot resolve target file: {e}"))?;
            if !canon_target.starts_with(&canonical_cwd)
                || is_path_inside_git(&canonical_cwd, &canon_target)
            {
                return Err("Target file escapes workspace boundary".to_string());
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                // Mask with 0o0777 to strip SUID/SGID/sticky bits cleanly
                preserved_mode = Some(meta.permissions().mode() & 0o0777);
            }

            let existing_bytes = fs::read(&target)
                .map_err(|e| format!("Failed reading existing file before save: {e}"))?;
            let current_etag = sha256_hex(&existing_bytes);
            let current_size_bytes = existing_bytes.len() as u64;
            let current_mtime_unix_ms = metadata_mtime_unix_ms(&meta);

            // Require an exact expected_etag match whenever !force and the file already exists
            if !force && expected_etag != Some(current_etag.as_str()) {
                let probe_len = existing_bytes.len().min(8192);
                let current_content = if existing_bytes.len() <= MAX_WORKSPACE_FILE_WRITE_BYTES
                    && !existing_bytes[..probe_len].contains(&0)
                {
                    String::from_utf8(existing_bytes).ok()
                } else {
                    None
                };
                return Ok(WriteWorkspaceFileOutcome {
                    etag: current_etag.clone(),
                    size_bytes: current_size_bytes,
                    mtime_unix_ms: current_mtime_unix_ms,
                    conflict: Some(WorkspaceFileConflict {
                        current_etag,
                        current_content,
                        current_size_bytes,
                        current_mtime_unix_ms,
                    }),
                });
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Cannot inspect target file: {e}")),
    }

    let tmp_name = format!(
        ".orbit-edit-tmp-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    );
    let tmp_path = parent_dir.join(tmp_name);
    let mut guard = AtomicTempGuard::new(tmp_path.clone());

    let mut open_opts = OpenOptions::new();
    open_opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Create with restricted mode atomically so 0o600 files are never briefly world-readable;
        // brand-new files use 0o644 masked naturally by the process umask.
        open_opts.mode(preserved_mode.unwrap_or(0o644));
    }

    let mut tmp_file = open_opts
        .open(&tmp_path)
        .map_err(|e| format!("Failed creating temporary file for atomic save: {e}"))?;

    #[cfg(unix)]
    if let Some(mode) = preserved_mode {
        use std::os::unix::fs::PermissionsExt;
        tmp_file
            .set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|e| format!("Failed preserving file permissions: {e}"))?;
    }

    let bytes = content.as_bytes();
    tmp_file
        .write_all(bytes)
        .map_err(|e| format!("Failed writing file content: {e}"))?;
    tmp_file
        .sync_all()
        .map_err(|e| format!("Failed flushing file to disk: {e}"))?;
    drop(tmp_file);

    fs::rename(&tmp_path, &target)
        .map_err(|e| format!("Failed replacing target file atomically: {e}"))?;
    guard.disarm();

    let new_etag = sha256_hex(bytes);
    let new_size_bytes = bytes.len() as u64;
    let new_mtime_unix_ms = fs::metadata(&target)
        .ok()
        .and_then(|m| metadata_mtime_unix_ms(&m));

    Ok(WriteWorkspaceFileOutcome {
        etag: new_etag,
        size_bytes: new_size_bytes,
        mtime_unix_ms: new_mtime_unix_ms,
        conflict: None,
    })
}

/// Create a new file or directory strictly inside `cwd`, refusing to overwrite existing entries.
pub async fn create_workspace_entry(
    cwd: &Path,
    raw_path: &str,
    is_dir: bool,
    initial_content: Option<&str>,
) -> Result<MutateWorkspaceEntryOutcome, String> {
    if let Some(body) = initial_content
        && body.len() > MAX_WORKSPACE_FILE_WRITE_BYTES
    {
        return Err(format!(
            "Initial content exceeds {} MiB limit",
            MAX_WORKSPACE_FILE_WRITE_BYTES / (1024 * 1024)
        ));
    }

    let cwd_owned = cwd.to_path_buf();
    let path_owned = raw_path.to_string();
    let content_owned = initial_content.map(ToOwned::to_owned);

    tokio::task::spawn_blocking(move || {
        create_workspace_entry_sync(&cwd_owned, &path_owned, is_dir, content_owned.as_deref())
    })
    .await
    .map_err(|_| "Workspace create entry task cancelled".to_string())?
}

fn create_workspace_entry_sync(
    cwd: &Path,
    raw_path: &str,
    is_dir: bool,
    initial_content: Option<&str>,
) -> Result<MutateWorkspaceEntryOutcome, String> {
    let segments = validate_rel_segments(raw_path)?;
    let normalized_rel = segments.join("/");
    let canonical_cwd = canonicalize_workspace_root(cwd)?;
    let (entry_name, parent_segments) = segments
        .split_last()
        .ok_or_else(|| "Path cannot be empty".to_string())?;
    let parent_dir = resolve_parent_dir_strict(&canonical_cwd, parent_segments, true)?;
    let target = parent_dir.join(entry_name);

    if fs::symlink_metadata(&target).is_ok() {
        return Err(format!("`{normalized_rel}` already exists"));
    }

    if is_dir {
        fs::create_dir(&target)
            .map_err(|e| format!("Failed creating directory `{normalized_rel}`: {e}"))?;
        Ok(MutateWorkspaceEntryOutcome {
            path: normalized_rel,
            new_path: None,
            action: "created_dir".to_string(),
        })
    } else {
        let mut open_opts = OpenOptions::new();
        open_opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Inherit process umask naturally (no hardcoded chmod override)
            open_opts.mode(0o644);
        }
        let mut file = open_opts
            .open(&target)
            .map_err(|e| format!("Failed creating file `{normalized_rel}`: {e}"))?;
        let mut cleanup_guard = AtomicTempGuard::new(target.clone());
        if let Some(body) = initial_content
            && !body.is_empty()
        {
            file.write_all(body.as_bytes()).map_err(|e| {
                format!("Failed writing initial content to `{normalized_rel}`: {e}")
            })?;
        }
        file.sync_all()
            .map_err(|e| format!("Failed flushing `{normalized_rel}`: {e}"))?;
        cleanup_guard.disarm();
        Ok(MutateWorkspaceEntryOutcome {
            path: normalized_rel,
            new_path: None,
            action: "created_file".to_string(),
        })
    }
}

/// Rename/move a workspace file or directory strictly inside `cwd`, refusing to overwrite an existing destination.
pub async fn rename_workspace_entry(
    cwd: &Path,
    old_path: &str,
    new_path: &str,
) -> Result<MutateWorkspaceEntryOutcome, String> {
    let cwd_owned = cwd.to_path_buf();
    let old_owned = old_path.to_string();
    let new_owned = new_path.to_string();

    tokio::task::spawn_blocking(move || {
        rename_workspace_entry_sync(&cwd_owned, &old_owned, &new_owned)
    })
    .await
    .map_err(|_| "Workspace rename entry task cancelled".to_string())?
}

fn rename_workspace_entry_sync(
    cwd: &Path,
    old_path: &str,
    new_path: &str,
) -> Result<MutateWorkspaceEntryOutcome, String> {
    let old_segments = validate_rel_segments(old_path)?;
    let new_segments = validate_rel_segments(new_path)?;
    let old_rel = old_segments.join("/");
    let new_rel = new_segments.join("/");

    if old_rel == new_rel {
        return Err("Source and destination paths are identical".to_string());
    }
    // Prevent moving a directory inside itself lexically (e.g. `a/b` -> `a/b/c`)
    if new_segments.len() > old_segments.len()
        && new_segments[..old_segments.len()] == old_segments[..]
    {
        return Err(format!(
            "Cannot move `{old_rel}` inside itself (`{new_rel}`)"
        ));
    }

    let canonical_cwd = canonicalize_workspace_root(cwd)?;
    let (old_name, old_parent_segs) = old_segments
        .split_last()
        .ok_or_else(|| "Source path cannot be empty".to_string())?;
    let old_parent = resolve_parent_dir_strict(&canonical_cwd, old_parent_segs, false)?;
    let old_target = old_parent.join(old_name);

    let old_meta = fs::symlink_metadata(&old_target)
        .map_err(|e| format!("Source `{old_rel}` not found: {e}"))?;
    let old_is_symlink = old_meta.file_type().is_symlink();
    let mut canon_old_dir: Option<PathBuf> = None;

    if old_is_symlink {
        if symlink_escapes_or_targets_git(&canonical_cwd, &old_target) {
            return Err(format!(
                "Refusing to rename symlink `{old_rel}` pointing outside workspace"
            ));
        }
    } else {
        let canon_old = fs::canonicalize(&old_target)
            .map_err(|e| format!("Cannot resolve `{old_rel}`: {e}"))?;
        if !canon_old.starts_with(&canonical_cwd)
            || canon_old == canonical_cwd
            || is_path_inside_git(&canonical_cwd, &canon_old)
        {
            return Err(format!("Source `{old_rel}` escapes workspace boundary"));
        }
        if old_meta.is_dir() {
            canon_old_dir = Some(canon_old);
        }
    }

    let (new_name, new_parent_segs) = new_segments
        .split_last()
        .ok_or_else(|| "Destination path cannot be empty".to_string())?;

    // Check canonical self-containment BEFORE creating any missing destination parent directories
    // (prevents leaking dirs on APFS case-insensitive or symlink-aliased self-moves).
    verify_existing_parents_not_inside(&canonical_cwd, new_parent_segs, canon_old_dir.as_deref())?;

    let new_parent = resolve_parent_dir_strict(&canonical_cwd, new_parent_segs, true)?;
    if let Some(ref old_dir) = canon_old_dir
        && (new_parent == *old_dir || new_parent.starts_with(old_dir))
    {
        return Err(format!(
            "Cannot move `{old_rel}` inside itself (`{new_rel}`)"
        ));
    }

    // Validate relative symlink targets against `new_parent` so moving a relative symlink
    // across directory depths cannot cause it to escape `canonical_cwd` or point into `.git`.
    if old_is_symlink {
        let link_dest = fs::read_link(&old_target)
            .map_err(|e| format!("Cannot read symlink `{old_rel}`: {e}"))?;
        if link_target_escapes_or_targets_git(&canonical_cwd, &new_parent, &link_dest) {
            return Err(format!(
                "Refusing to move symlink `{old_rel}` where its target would escape workspace"
            ));
        }
    }

    let new_target = new_parent.join(new_name);
    let mut is_same_inode_case_rename = false;

    match fs::symlink_metadata(&new_target) {
        Ok(new_meta) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if new_meta.dev() == old_meta.dev()
                    && new_meta.ino() == old_meta.ino()
                    && old_rel.eq_ignore_ascii_case(&new_rel)
                    && old_rel != new_rel
                {
                    is_same_inode_case_rename = true;
                } else {
                    return Err(format!("Destination `{new_rel}` already exists"));
                }
            }
            #[cfg(not(unix))]
            {
                let _ = new_meta;
                return Err(format!("Destination `{new_rel}` already exists"));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Cannot inspect destination `{new_rel}`: {e}")),
    }

    if !is_same_inode_case_rename && !old_is_symlink && old_meta.is_file() {
        // Atomic no-clobber file rename via hard_link + remove_file (zero TOCTOU overwrite)
        match fs::hard_link(&old_target, &new_target) {
            Ok(()) => {
                if let Err(e) = fs::remove_file(&old_target) {
                    let _ = fs::remove_file(&new_target);
                    return Err(format!("Failed removing original file `{old_rel}`: {e}"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(format!("Destination `{new_rel}` already exists"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
                fs::rename(&old_target, &new_target)
                    .map_err(|re| format!("Failed renaming `{old_rel}` to `{new_rel}`: {re}"))?;
            }
            Err(e) => {
                return Err(format!("Failed renaming `{old_rel}` to `{new_rel}`: {e}"));
            }
        }
    } else {
        fs::rename(&old_target, &new_target)
            .map_err(|e| format!("Failed renaming `{old_rel}` to `{new_rel}`: {e}"))?;
    }

    Ok(MutateWorkspaceEntryOutcome {
        path: old_rel,
        new_path: Some(new_rel),
        action: "renamed".to_string(),
    })
}

/// Safely delete a workspace file or directory strictly inside `cwd`.
/// Refuses workspace root, `.git`, and symlinks pointing outside workspace;
/// unlinks in-workspace symlinks directly without recursing into targets.
pub async fn delete_workspace_entry(
    cwd: &Path,
    raw_path: &str,
) -> Result<MutateWorkspaceEntryOutcome, String> {
    let cwd_owned = cwd.to_path_buf();
    let path_owned = raw_path.to_string();

    tokio::task::spawn_blocking(move || delete_workspace_entry_sync(&cwd_owned, &path_owned))
        .await
        .map_err(|_| "Workspace delete entry task cancelled".to_string())?
}

fn delete_workspace_entry_sync(
    cwd: &Path,
    raw_path: &str,
) -> Result<MutateWorkspaceEntryOutcome, String> {
    let segments = validate_rel_segments(raw_path)?;
    let normalized_rel = segments.join("/");
    let canonical_cwd = canonicalize_workspace_root(cwd)?;
    let (entry_name, parent_segments) = segments
        .split_last()
        .ok_or_else(|| "Path cannot be empty".to_string())?;
    let parent_dir = resolve_parent_dir_strict(&canonical_cwd, parent_segments, false)?;
    let target = parent_dir.join(entry_name);

    let meta = fs::symlink_metadata(&target)
        .map_err(|e| format!("Entry `{normalized_rel}` not found: {e}"))?;

    if meta.file_type().is_symlink() {
        if symlink_escapes_or_targets_git(&canonical_cwd, &target) {
            return Err(format!(
                "Refusing to delete symlink `{normalized_rel}` pointing outside workspace"
            ));
        }
        // Only unlink the symlink itself; never follow or recurse into its target.
        fs::remove_file(&target)
            .map_err(|e| format!("Failed removing symlink `{normalized_rel}`: {e}"))?;
    } else if meta.is_dir() {
        let canon_target = fs::canonicalize(&target)
            .map_err(|e| format!("Cannot resolve `{normalized_rel}`: {e}"))?;
        if !canon_target.starts_with(&canonical_cwd)
            || canon_target == canonical_cwd
            || is_path_inside_git(&canonical_cwd, &canon_target)
        {
            return Err(format!(
                "Refusing to delete `{normalized_rel}` outside workspace boundary"
            ));
        }
        remove_dir_all_no_follow(&canonical_cwd, &canon_target)?;
    } else {
        let canon_target = fs::canonicalize(&target)
            .map_err(|e| format!("Cannot resolve `{normalized_rel}`: {e}"))?;
        if !canon_target.starts_with(&canonical_cwd)
            || is_path_inside_git(&canonical_cwd, &canon_target)
        {
            return Err(format!(
                "Refusing to delete `{normalized_rel}` outside workspace boundary"
            ));
        }
        fs::remove_file(&target).map_err(|e| format!("Failed deleting `{normalized_rel}`: {e}"))?;
    }

    Ok(MutateWorkspaceEntryOutcome {
        path: normalized_rel,
        new_path: None,
        action: "deleted".to_string(),
    })
}

/// Recursively remove a directory strictly inside `canonical_cwd`, checking `symlink_metadata`
/// on every child so symlinks are ONLY unlinked and never traversed.
fn remove_dir_all_no_follow(canonical_cwd: &Path, dir: &Path) -> Result<(), String> {
    if !dir.starts_with(canonical_cwd) || dir == canonical_cwd {
        return Err("Refusing to remove directory outside workspace root".to_string());
    }
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed reading directory: {e}"))?;
    for entry_res in entries {
        let entry = entry_res.map_err(|e| format!("Failed reading directory entry: {e}"))?;
        let child = entry.path();
        let child_meta = fs::symlink_metadata(&child)
            .map_err(|e| format!("Failed inspecting child entry: {e}"))?;
        if child_meta.file_type().is_symlink() {
            fs::remove_file(&child).map_err(|e| format!("Failed removing symlink: {e}"))?;
        } else if child_meta.is_dir() {
            remove_dir_all_no_follow(canonical_cwd, &child)?;
        } else {
            fs::remove_file(&child).map_err(|e| format!("Failed removing file: {e}"))?;
        }
    }
    fs::remove_dir(dir).map_err(|e| format!("Failed removing directory: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::read_workspace_file;

    fn make_temp_root(prefix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("{prefix}_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("mkdir temp root");
        root
    }

    #[tokio::test]
    async fn read_workspace_file_returns_etag_mtime_writable_and_line_ending() {
        let ws = make_temp_root("orbit_wf_meta");

        let lf_bytes = b"fn main() {\n    println!(\"hi\");\n}\n";
        fs::write(ws.join("lf.rs"), lf_bytes).expect("write lf");
        let lf_res = read_workspace_file(&ws, "lf.rs", None)
            .await
            .expect("read lf");
        assert_eq!(lf_res.etag.as_deref(), Some(sha256_hex(lf_bytes).as_str()));
        assert!(lf_res.mtime_unix_ms.is_some());
        assert!(lf_res.writable);
        assert_eq!(lf_res.line_ending.as_deref(), Some("lf"));

        // Absolute path reads must report writable = false (mutations require relative workspace paths)
        let abs_lf = ws.join("lf.rs");
        let abs_res = read_workspace_file(&ws, abs_lf.to_str().unwrap_or(""), None)
            .await
            .expect("read abs");
        assert!(!abs_res.writable);

        let crlf_bytes = b"a\r\nb\r\n";
        fs::write(ws.join("crlf.txt"), crlf_bytes).expect("write crlf");
        let crlf_res = read_workspace_file(&ws, "crlf.txt", None)
            .await
            .expect("read crlf");
        assert_eq!(
            crlf_res.etag.as_deref(),
            Some(sha256_hex(crlf_bytes).as_str())
        );
        assert_eq!(crlf_res.line_ending.as_deref(), Some("crlf"));

        let mixed_bytes = b"a\r\nb\nc\n";
        fs::write(ws.join("mixed.txt"), mixed_bytes).expect("write mixed");
        let mixed_res = read_workspace_file(&ws, "mixed.txt", None)
            .await
            .expect("read mixed");
        assert_eq!(mixed_res.line_ending.as_deref(), Some("mixed"));

        let _ = fs::remove_dir_all(&ws);
    }

    #[tokio::test]
    async fn write_workspace_file_atomic_etag_conflict_force_and_mode_preservation() {
        let ws = make_temp_root("orbit_wf_write");
        let script = ws.join("run.sh");
        let initial = "#!/bin/sh\necho v1\n";
        fs::write(&script, initial).expect("write initial");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod 755");
        }

        let initial_read = read_workspace_file(&ws, "run.sh", None)
            .await
            .expect("read initial");
        let etag1 = initial_read.etag.expect("initial etag");

        // 1. Matching etag writes atomically, updates etag, preserves 0o755
        let v2 = "#!/bin/sh\necho v2\n";
        let res2 = write_workspace_file(&ws, "run.sh", v2, Some(&etag1), false)
            .await
            .expect("write v2");
        assert!(res2.conflict.is_none());
        assert_eq!(res2.etag, sha256_hex(v2.as_bytes()));
        assert_eq!(res2.size_bytes, v2.len() as u64);
        assert!(res2.mtime_unix_ms.is_some());
        assert_eq!(fs::read_to_string(&script).expect("read disk v2"), v2);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&script).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o755, "file mode 0o755 must be preserved");
        }

        // Verify no stray temp files left behind
        let stray_tmps: Vec<_> = fs::read_dir(&ws)
            .expect("readdir")
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".orbit-edit-tmp-")
            })
            .collect();
        assert!(stray_tmps.is_empty());

        // 2. Stale etag returns conflict WITHOUT mutating disk
        let stale_attempt = "#!/bin/sh\necho stale-overwrite\n";
        let conflict_res = write_workspace_file(&ws, "run.sh", stale_attempt, Some(&etag1), false)
            .await
            .expect("conflict response");
        let conflict = conflict_res.conflict.expect("must return conflict");
        assert_eq!(conflict.current_etag, res2.etag);
        assert_eq!(conflict.current_content.as_deref(), Some(v2));
        assert_eq!(conflict.current_size_bytes, v2.len() as u64);
        assert_eq!(
            fs::read_to_string(&script).expect("disk unchanged"),
            v2,
            "stale etag write must not mutate disk"
        );

        // 3. Missing expected_etag (None) on an existing file with force=false MUST return conflict
        let none_etag_res = write_workspace_file(&ws, "run.sh", stale_attempt, None, false)
            .await
            .expect("none etag conflict response");
        assert!(
            none_etag_res.conflict.is_some(),
            "expected_etag=None on existing file with force=false must return conflict"
        );
        assert_eq!(
            fs::read_to_string(&script).expect("disk unchanged after None etag"),
            v2
        );

        // 4. force: true overwrites stale etag cleanly
        let v3 = "#!/bin/sh\necho forced-v3\n";
        let forced_res = write_workspace_file(&ws, "run.sh", v3, Some(&etag1), true)
            .await
            .expect("forced write");
        assert!(forced_res.conflict.is_none());
        assert_eq!(forced_res.etag, sha256_hex(v3.as_bytes()));
        assert_eq!(fs::read_to_string(&script).expect("read disk v3"), v3);

        let _ = fs::remove_dir_all(&ws);
    }

    #[tokio::test]
    async fn write_refuses_readonly_strips_suid_and_preserves_0600() {
        let ws = make_temp_root("orbit_wf_perms");

        // 1. 0o444 read-only file must be rejected even with force=true
        let ro_file = ws.join("locked.txt");
        fs::write(&ro_file, "immutable\n").expect("write locked");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&ro_file, fs::Permissions::from_mode(0o444)).expect("chmod 444");
        }
        assert!(
            write_workspace_file(&ws, "locked.txt", "hacked", None, true)
                .await
                .is_err(),
            "read-only 0o444 file must not be overwritten"
        );
        assert_eq!(
            fs::read_to_string(&ro_file).expect("read ro"),
            "immutable\n"
        );

        // Restore write permission for cleanup
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&ro_file, fs::Permissions::from_mode(0o644)).expect("chmod 644");
        }

        // 2. SUID/SGID bits must be stripped on atomic replace; 0o600 must be preserved
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let suid_file = ws.join("suid.sh");
            fs::write(&suid_file, "echo 1\n").expect("write suid");
            let _ = fs::set_permissions(&suid_file, fs::Permissions::from_mode(0o4755));
            let res = write_workspace_file(&ws, "suid.sh", "echo 2\n", None, true)
                .await
                .expect("write suid");
            assert!(res.conflict.is_none());
            let new_mode = fs::metadata(&suid_file)
                .expect("meta suid")
                .permissions()
                .mode()
                & 0o7777;
            assert_eq!(
                new_mode & 0o7000,
                0,
                "SUID/SGID/sticky bits must be stripped on save"
            );
            assert_eq!(new_mode & 0o0777, 0o0755);

            let secret_env = ws.join(".env");
            fs::write(&secret_env, "KEY=1\n").expect("write .env");
            fs::set_permissions(&secret_env, fs::Permissions::from_mode(0o600)).expect("chmod 600");
            let etag = sha256_hex(b"KEY=1\n");
            let res_env = write_workspace_file(&ws, ".env", "KEY=2\n", Some(&etag), false)
                .await
                .expect("write .env");
            assert!(res_env.conflict.is_none());
            let env_mode = fs::metadata(&secret_env)
                .expect("meta env")
                .permissions()
                .mode()
                & 0o7777;
            assert_eq!(env_mode, 0o600);
        }

        let _ = fs::remove_dir_all(&ws);
    }

    #[tokio::test]
    async fn mutations_enforce_strict_security_boundaries_and_size_limit() {
        let root = make_temp_root("orbit_wf_sec");
        let ws = root.join("workspace");
        let git_hooks = ws.join(".git").join("hooks");
        fs::create_dir_all(&git_hooks).expect("mkdir .git/hooks");
        fs::write(ws.join(".git").join("config"), "[core]\n").expect("write git config");
        fs::write(git_hooks.join("pre-commit"), "#!/bin/sh\n").expect("write hook");

        let outside = root.join("outside.txt");
        fs::write(&outside, "secret\n").expect("write outside");
        let outside_dir = root.join("outside_dir");
        fs::create_dir_all(&outside_dir).expect("mkdir outside_dir");
        fs::write(outside_dir.join("keep.txt"), "keep\n").expect("write keep");

        // `.git` as `cwd`, filesystem root `/`, and `$HOME` as `cwd` must ALL be rejected
        assert!(
            write_workspace_file(&ws.join(".git"), "config", "bad", None, true)
                .await
                .is_err(),
            "cwd inside .git must be rejected"
        );
        assert!(
            create_workspace_entry(&git_hooks, "pwn.sh", false, Some("bad"))
                .await
                .is_err(),
            "cwd inside .git/hooks must be rejected"
        );
        assert!(
            write_workspace_file(Path::new("/"), "tmp/bad.txt", "x", None, true)
                .await
                .is_err(),
            "filesystem root cwd must be rejected"
        );
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            assert!(
                write_workspace_file(&home, "orbit_should_not_write.txt", "x", None, true)
                    .await
                    .is_err(),
                "$HOME root cwd must be rejected"
            );
        }

        // Traversal & absolute paths rejected across all operations
        for bad in [
            "",
            "   ",
            ".",
            "./",
            "../outside.txt",
            "a/../../outside.txt",
            "/etc/passwd",
            "~/secret.txt",
            "foo\0bar.txt",
            ".git",
            ".git/config",
            ".git/hooks/pre-commit",
            ".GIT/config",
        ] {
            assert!(
                write_workspace_file(&ws, bad, "x", None, true)
                    .await
                    .is_err(),
                "write must reject `{bad}`"
            );
            assert!(
                create_workspace_entry(&ws, bad, false, Some("x"))
                    .await
                    .is_err(),
                "create must reject `{bad}`"
            );
            assert!(
                delete_workspace_entry(&ws, bad).await.is_err(),
                "delete must reject `{bad}`"
            );
            assert!(
                rename_workspace_entry(&ws, bad, "safe.txt").await.is_err(),
                "rename old must reject `{bad}`"
            );
            assert!(
                rename_workspace_entry(&ws, "safe.txt", bad).await.is_err(),
                "rename new must reject `{bad}`"
            );
        }

        // > 4 MiB write rejected
        let oversized = "a".repeat(MAX_WORKSPACE_FILE_WRITE_BYTES + 1);
        assert!(
            write_workspace_file(&ws, "huge.txt", &oversized, None, true)
                .await
                .is_err()
        );
        assert!(
            create_workspace_entry(&ws, "huge.txt", false, Some(&oversized))
                .await
                .is_err()
        );

        // Symlink pointing outside workspace rejected on write / create / rename / delete
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let file_link = ws.join("escape_link.txt");
            symlink(&outside, &file_link).expect("symlink file");
            let dir_link = ws.join("escape_dir");
            symlink(&outside_dir, &dir_link).expect("symlink dir");

            assert!(
                write_workspace_file(&ws, "escape_link.txt", "pwned", None, true)
                    .await
                    .is_err(),
                "write through escaping file symlink must fail"
            );
            assert!(
                write_workspace_file(&ws, "escape_dir/new.txt", "pwned", None, true)
                    .await
                    .is_err(),
                "write inside escaping dir symlink must fail"
            );
            assert!(
                create_workspace_entry(&ws, "escape_dir/created.txt", false, Some("pwned"))
                    .await
                    .is_err(),
                "create inside escaping dir symlink must fail"
            );
            assert!(
                rename_workspace_entry(&ws, "escape_link.txt", "renamed_link.txt")
                    .await
                    .is_err(),
                "rename of escaping file symlink must fail"
            );
            assert!(
                rename_workspace_entry(&ws, "escape_dir/keep.txt", "stolen.txt")
                    .await
                    .is_err(),
                "rename from escaping dir symlink must fail"
            );
            assert!(
                delete_workspace_entry(&ws, "escape_link.txt")
                    .await
                    .is_err(),
                "delete of escaping file symlink must fail"
            );
            assert!(
                delete_workspace_entry(&ws, "escape_dir").await.is_err(),
                "delete of escaping dir symlink must fail"
            );
            assert!(
                delete_workspace_entry(&ws, "escape_dir/keep.txt")
                    .await
                    .is_err(),
                "delete inside escaping dir symlink must fail"
            );

            // Moving an in-workspace relative symlink upward so its relative target would escape cwd must fail
            fs::create_dir_all(ws.join("nested")).expect("mkdir nested");
            fs::write(ws.join("inside.txt"), "ok\n").expect("write inside");
            symlink("../inside.txt", ws.join("nested/rel_link.txt")).expect("symlink rel");
            assert!(
                rename_workspace_entry(&ws, "nested/rel_link.txt", "rel_link.txt")
                    .await
                    .is_err(),
                "moving relative symlink upward to escape workspace must fail"
            );

            assert_eq!(
                fs::read_to_string(&outside).expect("outside intact"),
                "secret\n"
            );
            assert!(outside_dir.join("keep.txt").exists());
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn create_rename_delete_happy_paths_and_case_only_rename() {
        let ws = make_temp_root("orbit_wf_crud");

        // Create nested dir & file
        let d = create_workspace_entry(&ws, "src/utils", true, None)
            .await
            .expect("create dir");
        assert_eq!(d.action, "created_dir");
        assert!(ws.join("src/utils").is_dir());

        let f = create_workspace_entry(&ws, "src/utils/mod.rs", false, Some("pub fn ok() {}\n"))
            .await
            .expect("create file");
        assert_eq!(f.action, "created_file");
        assert_eq!(
            fs::read_to_string(ws.join("src/utils/mod.rs")).expect("read created"),
            "pub fn ok() {}\n"
        );

        // Refuse creating existing entry
        assert!(
            create_workspace_entry(&ws, "src/utils/mod.rs", false, Some("dup"))
                .await
                .is_err()
        );
        assert!(
            create_workspace_entry(&ws, "src/utils", true, None)
                .await
                .is_err()
        );

        // Create second file and verify rename refuses overwriting it
        create_workspace_entry(&ws, "src/utils/existing.rs", false, Some("// keep\n"))
            .await
            .expect("create existing");
        assert!(
            rename_workspace_entry(&ws, "src/utils/mod.rs", "src/utils/existing.rs")
                .await
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(ws.join("src/utils/existing.rs")).expect("existing untouched"),
            "// keep\n"
        );

        // Rename happy path
        let renamed = rename_workspace_entry(&ws, "src/utils/mod.rs", "src/utils/helpers.rs")
            .await
            .expect("rename file");
        assert_eq!(renamed.action, "renamed");
        assert_eq!(renamed.path, "src/utils/mod.rs");
        assert_eq!(renamed.new_path.as_deref(), Some("src/utils/helpers.rs"));
        assert!(!ws.join("src/utils/mod.rs").exists());
        assert!(ws.join("src/utils/helpers.rs").is_file());

        // Case-only rename works on both case-insensitive (APFS) and case-sensitive filesystems
        let case_renamed =
            rename_workspace_entry(&ws, "src/utils/helpers.rs", "src/utils/Helpers.rs")
                .await
                .expect("case-only rename");
        assert_eq!(
            case_renamed.new_path.as_deref(),
            Some("src/utils/Helpers.rs")
        );
        assert_eq!(
            fs::read_to_string(ws.join("src/utils/Helpers.rs")).expect("read case-renamed"),
            "pub fn ok() {}\n"
        );

        // Rename directory into itself (including case-aliased on APFS) refused without leaking dirs
        assert!(
            rename_workspace_entry(&ws, "src/utils", "src/utils/nested")
                .await
                .is_err()
        );
        assert!(
            rename_workspace_entry(&ws, "src/utils", "src/UTILS/nested/deep")
                .await
                .is_err()
        );
        assert!(!ws.join("src/utils/nested").exists());

        // Delete file & directory
        let del_file = delete_workspace_entry(&ws, "src/utils/Helpers.rs")
            .await
            .expect("delete file");
        assert_eq!(del_file.action, "deleted");
        assert!(!ws.join("src/utils/Helpers.rs").exists());

        let del_dir = delete_workspace_entry(&ws, "src")
            .await
            .expect("delete dir");
        assert_eq!(del_dir.action, "deleted");
        assert!(!ws.join("src").exists());

        // Deleting workspace root refused
        assert!(delete_workspace_entry(&ws, ".").await.is_err());

        let _ = fs::remove_dir_all(&ws);
    }
}
