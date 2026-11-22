//! Safe, zero-`unsafe` Copy-on-Write (CoW) / reflink cloning and reference-linked directory
//! sharing primitives for accelerating Git worktree provisioning, `.worktreeinclude` copying,
//! and workspace/session forks.
//!
//! - **macOS (APFS)**: Uses `/bin/cp -c` / `/bin/cp -cRP` (native APFS `clonefile(2)` O(1) CoW)
//!   with automatic fallback to standard copy on non-APFS or cross-volume destinations.
//! - **Linux (Btrfs / XFS)**: Uses `cp --reflink=always` / `cp --reflink=auto -RP` (native
//!   `FICLONE` ioctl) with automatic fallback to standard copy.
//! - **Security**: Every entry point rejects `..` path traversal, symlink destination overwrites,
//!   overlapping source/destination trees, and any symlink whose target escapes its allowed root.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::Context;

const MAX_SYMLINK_HOPS: usize = 16;

/// Mechanism actually used to complete a copy operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyMethod {
    /// macOS APFS `clonefile` copy-on-write (`/bin/cp -c`).
    ApfsClone,
    /// Linux Btrfs/XFS `FICLONE` copy-on-write (`cp --reflink=always`).
    LinuxReflink,
    /// Standard byte copy fallback (cross-volume, non-CoW filesystem, or unsupported platform).
    Standard,
}

/// Sharing policy for [`link_or_clone_dir`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirShareMode {
    /// Reference-link via directory symlink (O(1), zero disk duplication, shared view),
    /// transparently falling back to CoW clone if symlink creation is unsupported.
    ReferenceLink,
    /// Isolated writable copy via CoW / reflink (O(1) on APFS/Btrfs/XFS, standard fallback).
    /// Mutations in `dst` do not affect `src`.
    CopyOnWrite,
}

/// Outcome returned by [`link_or_clone_dir`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOrCloneOutcome {
    /// Created (or verified existing) directory symlink to the canonical source directory.
    Symlinked,
    /// Cloned/copied directory contents via [`CopyMethod`].
    Cloned(CopyMethod),
}

/// Copy a single regular file using OS Copy-on-Write when available, falling back to standard copy.
///
/// Rejects `..` path traversal, symlink destinations, non-file sources, and symlinks escaping the
/// source's parent directory.
pub fn copy_file_cow(src: &Path, dst: &Path) -> anyhow::Result<CopyMethod> {
    reject_invalid_path(src, "source")?;
    reject_invalid_path(dst, "destination")?;
    let parent = src
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let allowed_root = std::fs::canonicalize(parent).with_context(|| {
        format!(
            "failed to canonicalize source parent directory: {}",
            parent.display()
        )
    })?;
    copy_file_cow_within(&allowed_root, src, dst)
}

/// Copy a single file while enforcing that `src` (including any symlink target) stays strictly
/// within `allowed_root`.
pub fn copy_file_cow_within(
    allowed_root: &Path,
    src: &Path,
    dst: &Path,
) -> anyhow::Result<CopyMethod> {
    reject_invalid_path(src, "source")?;
    reject_invalid_path(dst, "destination")?;

    let canon_root = std::fs::canonicalize(allowed_root).with_context(|| {
        format!(
            "failed to canonicalize allowed root: {}",
            allowed_root.display()
        )
    })?;

    let src_sym_meta = std::fs::symlink_metadata(src)
        .with_context(|| format!("source file not accessible: {}", src.display()))?;
    let canon_src = if src_sym_meta.file_type().is_symlink() {
        resolve_symlink_target_safe(src, &canon_root)?
    } else {
        std::fs::canonicalize(src)
            .with_context(|| format!("failed to canonicalize source: {}", src.display()))?
    };

    if !canon_src.starts_with(&canon_root) {
        anyhow::bail!(
            "source path escapes allowed root ({}): {}",
            canon_root.display(),
            src.display()
        );
    }

    let src_meta = std::fs::metadata(&canon_src)
        .with_context(|| format!("source target not accessible: {}", canon_src.display()))?;
    if !src_meta.is_file() {
        anyhow::bail!("source is not a regular file: {}", src.display());
    }

    validate_dst_not_symlink(dst)?;
    if dst.exists() && dst.is_dir() {
        anyhow::bail!(
            "destination is a directory, expected file path: {}",
            dst.display()
        );
    }
    if let Some(parent) = dst.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create destination parent directory: {}",
                parent.display()
            )
        })?;
    }

    // If source and destination resolve to the exact same file, treat as no-op standard copy.
    if dst.exists()
        && let Ok(canon_dst) = std::fs::canonicalize(dst)
        && canon_dst == canon_src
    {
        return Ok(CopyMethod::Standard);
    }

    copy_file_cow_validated(&canon_src, dst)
}

/// Recursively clone a directory tree using OS Copy-on-Write when available, falling back to
/// safe standard recursive copy.
///
/// Rejects `..` path traversal, symlink destinations, overlapping destination inside source,
/// and any symlink in `src` (including `src` itself) escaping `src`'s boundary.
pub fn copy_dir_cow(src: &Path, dst: &Path) -> anyhow::Result<CopyMethod> {
    reject_invalid_path(src, "source")?;
    reject_invalid_path(dst, "destination")?;

    let src_sym_meta = std::fs::symlink_metadata(src)
        .with_context(|| format!("source directory not accessible: {}", src.display()))?;
    let canon_src = if src_sym_meta.file_type().is_symlink() {
        let parent = src
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let canon_parent = std::fs::canonicalize(parent).with_context(|| {
            format!(
                "failed to canonicalize source parent directory: {}",
                parent.display()
            )
        })?;
        resolve_symlink_target_safe(src, &canon_parent)?
    } else {
        std::fs::canonicalize(src)
            .with_context(|| format!("failed to canonicalize source dir: {}", src.display()))?
    };

    copy_dir_cow_within(&canon_src, src, dst)
}

/// Recursively clone a directory while enforcing that `src` and every nested symlink stay
/// strictly within `allowed_root`.
pub fn copy_dir_cow_within(
    allowed_root: &Path,
    src: &Path,
    dst: &Path,
) -> anyhow::Result<CopyMethod> {
    reject_invalid_path(src, "source")?;
    reject_invalid_path(dst, "destination")?;

    let canon_root = std::fs::canonicalize(allowed_root).with_context(|| {
        format!(
            "failed to canonicalize allowed root: {}",
            allowed_root.display()
        )
    })?;

    let src_sym_meta = std::fs::symlink_metadata(src)
        .with_context(|| format!("source directory not accessible: {}", src.display()))?;
    let canon_src = if src_sym_meta.file_type().is_symlink() {
        resolve_symlink_target_safe(src, &canon_root)?
    } else {
        std::fs::canonicalize(src)
            .with_context(|| format!("failed to canonicalize source dir: {}", src.display()))?
    };

    if !canon_src.starts_with(&canon_root) {
        anyhow::bail!(
            "source directory escapes allowed root ({}): {}",
            canon_root.display(),
            src.display()
        );
    }

    let src_meta = std::fs::metadata(&canon_src)
        .with_context(|| format!("source directory not readable: {}", canon_src.display()))?;
    if !src_meta.is_dir() {
        anyhow::bail!("source is not a directory: {}", src.display());
    }

    validate_dst_not_symlink(dst)?;
    let dst_existed = dst.exists();
    if dst_existed && !dst.is_dir() {
        anyhow::bail!(
            "destination exists and is not a directory: {}",
            dst.display()
        );
    }

    // Prevent infinite recursion / self-overwrite when destination overlaps source.
    let resolved_dst = canonicalize_best_effort(dst)?;
    if resolved_dst.starts_with(&canon_src) {
        anyhow::bail!(
            "destination directory cannot be inside or equal to source directory: {} -> {}",
            canon_src.display(),
            dst.display()
        );
    }

    // Pre-validate the entire tree before invoking native CoW `cp` so no escaping symlink can
    // ever be cloned or dereferenced.
    validate_tree_no_escape(&canon_root, &canon_src)?;

    if let Some(parent) = dst.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create destination parent directory: {}",
                parent.display()
            )
        })?;
    }

    copy_dir_cow_validated(&canon_src, dst, dst_existed, &canon_root)
}

/// Share or fork a scratch/artifact directory in O(1) without duplicating large folder trees.
///
/// - [`DirShareMode::ReferenceLink`]: Creates a symlink at `dst` pointing to canonical `src` on
///   Unix (idempotent if `dst` already links to `src`), falling back to [`copy_dir_cow`] if
///   symlink creation is unavailable.
/// - [`DirShareMode::CopyOnWrite`]: Performs an isolated CoW clone via [`copy_dir_cow`].
pub fn link_or_clone_dir(
    src: &Path,
    dst: &Path,
    mode: DirShareMode,
) -> anyhow::Result<LinkOrCloneOutcome> {
    reject_invalid_path(src, "source")?;
    reject_invalid_path(dst, "destination")?;

    let src_sym_meta = std::fs::symlink_metadata(src)
        .with_context(|| format!("source directory not accessible: {}", src.display()))?;
    let canon_src = if src_sym_meta.file_type().is_symlink() {
        let parent = src
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let canon_parent = std::fs::canonicalize(parent).with_context(|| {
            format!(
                "failed to canonicalize source parent directory: {}",
                parent.display()
            )
        })?;
        resolve_symlink_target_safe(src, &canon_parent)?
    } else {
        std::fs::canonicalize(src)
            .with_context(|| format!("failed to canonicalize source dir: {}", src.display()))?
    };

    if !canon_src.is_dir() {
        anyhow::bail!("source is not a directory: {}", src.display());
    }

    validate_tree_no_escape(&canon_src, &canon_src)?;

    // Idempotent fast-path when `dst` is already a reference link pointing to `canon_src`.
    if mode == DirShareMode::ReferenceLink
        && let Ok(dst_meta) = std::fs::symlink_metadata(dst)
        && dst_meta.file_type().is_symlink()
        && let Ok(existing_target) = std::fs::canonicalize(dst)
        && existing_target == canon_src
    {
        return Ok(LinkOrCloneOutcome::Symlinked);
    }

    let resolved_dst = canonicalize_best_effort(dst)?;
    if resolved_dst.starts_with(&canon_src) {
        anyhow::bail!(
            "destination cannot be inside or equal to source directory: {} -> {}",
            canon_src.display(),
            dst.display()
        );
    }

    match mode {
        DirShareMode::ReferenceLink => {
            if std::fs::symlink_metadata(dst).is_ok() {
                anyhow::bail!(
                    "destination already exists for reference link: {}",
                    dst.display()
                );
            }
            if let Some(parent) = dst.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "failed to create destination parent directory: {}",
                        parent.display()
                    )
                })?;
            }
            #[cfg(unix)]
            {
                if std::os::unix::fs::symlink(&canon_src, dst).is_ok() {
                    return Ok(LinkOrCloneOutcome::Symlinked);
                }
            }
            let method = copy_dir_cow_within(&canon_src, &canon_src, dst)?;
            Ok(LinkOrCloneOutcome::Cloned(method))
        }
        DirShareMode::CopyOnWrite => {
            let method = copy_dir_cow_within(&canon_src, &canon_src, dst)?;
            Ok(LinkOrCloneOutcome::Cloned(method))
        }
    }
}

// --- Platform CoW & Standard Fallback Engines ---

#[cfg(target_os = "macos")]
fn copy_file_cow_validated(canon_src: &Path, dst: &Path) -> anyhow::Result<CopyMethod> {
    let status = Command::new("/bin/cp")
        .arg("-cf")
        .arg("--")
        .arg(canon_src)
        .arg(dst)
        .status();
    if let Ok(st) = status
        && st.success()
    {
        return Ok(CopyMethod::ApfsClone);
    }
    std::fs::copy(canon_src, dst).with_context(|| {
        format!(
            "standard file copy fallback failed: {} -> {}",
            canon_src.display(),
            dst.display()
        )
    })?;
    Ok(CopyMethod::Standard)
}

#[cfg(target_os = "linux")]
fn copy_file_cow_validated(canon_src: &Path, dst: &Path) -> anyhow::Result<CopyMethod> {
    if let Ok(st) = Command::new("cp")
        .arg("--reflink=always")
        .arg("-f")
        .arg("--")
        .arg(canon_src)
        .arg(dst)
        .status()
        && st.success()
    {
        return Ok(CopyMethod::LinuxReflink);
    }
    if let Ok(st) = Command::new("cp")
        .arg("--reflink=auto")
        .arg("-f")
        .arg("--")
        .arg(canon_src)
        .arg(dst)
        .status()
        && st.success()
    {
        return Ok(CopyMethod::Standard);
    }
    std::fs::copy(canon_src, dst).with_context(|| {
        format!(
            "standard file copy fallback failed: {} -> {}",
            canon_src.display(),
            dst.display()
        )
    })?;
    Ok(CopyMethod::Standard)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn copy_file_cow_validated(canon_src: &Path, dst: &Path) -> anyhow::Result<CopyMethod> {
    std::fs::copy(canon_src, dst).with_context(|| {
        format!(
            "standard file copy failed: {} -> {}",
            canon_src.display(),
            dst.display()
        )
    })?;
    Ok(CopyMethod::Standard)
}

#[cfg(target_os = "macos")]
fn copy_dir_cow_validated(
    canon_src: &Path,
    dst: &Path,
    dst_existed: bool,
    allowed_root: &Path,
) -> anyhow::Result<CopyMethod> {
    let cp_src = if dst_existed {
        canon_src.join(".")
    } else {
        canon_src.to_path_buf()
    };
    let flags = if dst_existed { "-cRPf" } else { "-cRP" };
    let status = Command::new("/bin/cp")
        .arg(flags)
        .arg("--")
        .arg(&cp_src)
        .arg(dst)
        .status();
    if let Ok(st) = status
        && st.success()
    {
        return Ok(CopyMethod::ApfsClone);
    }

    if !dst_existed && dst.exists() {
        let _ = std::fs::remove_dir_all(dst);
    }
    copy_dir_standard(canon_src, dst, allowed_root)?;
    Ok(CopyMethod::Standard)
}

#[cfg(target_os = "linux")]
fn copy_dir_cow_validated(
    canon_src: &Path,
    dst: &Path,
    dst_existed: bool,
    allowed_root: &Path,
) -> anyhow::Result<CopyMethod> {
    let cp_src = if dst_existed {
        canon_src.join(".")
    } else {
        canon_src.to_path_buf()
    };

    if let Ok(st) = Command::new("cp")
        .arg("--reflink=always")
        .arg("-R")
        .arg("-P")
        .arg("-f")
        .arg("--")
        .arg(&cp_src)
        .arg(dst)
        .status()
        && st.success()
    {
        return Ok(CopyMethod::LinuxReflink);
    }
    if !dst_existed && dst.exists() {
        let _ = std::fs::remove_dir_all(dst);
    }

    if let Ok(st) = Command::new("cp")
        .arg("--reflink=auto")
        .arg("-R")
        .arg("-P")
        .arg("-f")
        .arg("--")
        .arg(&cp_src)
        .arg(dst)
        .status()
        && st.success()
    {
        return Ok(CopyMethod::Standard);
    }
    if !dst_existed && dst.exists() {
        let _ = std::fs::remove_dir_all(dst);
    }

    copy_dir_standard(canon_src, dst, allowed_root)?;
    Ok(CopyMethod::Standard)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn copy_dir_cow_validated(
    canon_src: &Path,
    dst: &Path,
    _dst_existed: bool,
    allowed_root: &Path,
) -> anyhow::Result<CopyMethod> {
    copy_dir_standard(canon_src, dst, allowed_root)?;
    Ok(CopyMethod::Standard)
}

/// Safe pure-Rust recursive directory copy fallback.
fn copy_dir_standard(src: &Path, dst: &Path, allowed_root: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("failed to create directory: {}", dst.display()))?;
    if let Ok(meta) = std::fs::metadata(src) {
        let _ = std::fs::set_permissions(dst, meta.permissions());
    }

    let entries = std::fs::read_dir(src)
        .with_context(|| format!("failed to read directory: {}", src.display()))?;
    for entry_res in entries {
        let entry = entry_res
            .with_context(|| format!("failed to read directory entry in {}", src.display()))?;
        let entry_path = entry.path();
        let dst_entry = dst.join(entry.file_name());
        let sym_meta = std::fs::symlink_metadata(&entry_path)
            .with_context(|| format!("failed to stat {}", entry_path.display()))?;
        let ft = sym_meta.file_type();

        if ft.is_symlink() {
            let _ = resolve_symlink_target_safe(&entry_path, allowed_root)?;
            #[cfg(unix)]
            {
                let link_target = std::fs::read_link(&entry_path)
                    .with_context(|| format!("failed to read symlink {}", entry_path.display()))?;
                if std::fs::symlink_metadata(&dst_entry).is_ok() {
                    let _ = std::fs::remove_file(&dst_entry);
                }
                std::os::unix::fs::symlink(&link_target, &dst_entry).with_context(|| {
                    format!(
                        "failed to recreate symlink {} -> {}",
                        dst_entry.display(),
                        link_target.display()
                    )
                })?;
            }
            #[cfg(not(unix))]
            {
                if entry_path.is_file() {
                    std::fs::copy(&entry_path, &dst_entry)?;
                }
            }
        } else if ft.is_dir() {
            copy_dir_standard(&entry_path, &dst_entry, allowed_root)?;
        } else if ft.is_file() {
            validate_dst_not_symlink(&dst_entry)?;
            std::fs::copy(&entry_path, &dst_entry).with_context(|| {
                format!(
                    "failed to copy file {} -> {}",
                    entry_path.display(),
                    dst_entry.display()
                )
            })?;
        }
    }
    Ok(())
}

// --- Path Traversal & Symlink Escape Guards ---

fn reject_invalid_path(path: &Path, label: &str) -> anyhow::Result<()> {
    if path.as_os_str().is_empty() {
        anyhow::bail!("empty {label} path rejected");
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        anyhow::bail!(
            "path traversal (`..`) rejected in {label} path: {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_dst_not_symlink(dst: &Path) -> anyhow::Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(dst)
        && meta.file_type().is_symlink()
    {
        anyhow::bail!(
            "refusing to overwrite symlink destination: {}",
            dst.display()
        );
    }
    Ok(())
}

/// Recursively inspect every entry in `dir` (without following directory symlinks) and ensure
/// no symlink resolves outside `allowed_root` or forms a cycle.
pub(crate) fn validate_tree_no_escape(allowed_root: &Path, dir: &Path) -> anyhow::Result<()> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("failed to scan directory: {}", dir.display()))?;
    for entry_res in entries {
        let entry =
            entry_res.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        let sym_meta = std::fs::symlink_metadata(&path)
            .with_context(|| format!("failed to stat {}", path.display()))?;
        let ft = sym_meta.file_type();
        if ft.is_symlink() {
            let _ = resolve_symlink_target_safe(&path, allowed_root)?;
        } else if ft.is_dir() {
            validate_tree_no_escape(allowed_root, &path)?;
        }
    }
    Ok(())
}

/// Resolve a symlink (including chained or dangling symlinks) and reject any target that escapes
/// `allowed_root` or loops.
pub(crate) fn resolve_symlink_target_safe(
    link_path: &Path,
    allowed_root: &Path,
) -> anyhow::Result<PathBuf> {
    if let Ok(canon) = std::fs::canonicalize(link_path) {
        if !canon.starts_with(allowed_root) {
            anyhow::bail!(
                "symlink escape rejected: {} resolves to {} outside {}",
                link_path.display(),
                canon.display(),
                allowed_root.display()
            );
        }
        return Ok(canon);
    }

    // Dangling symlink or symlink loop: follow hops lexically against canonicalized ancestors.
    let mut current = canonicalize_best_effort(link_path)?;
    for _ in 0..MAX_SYMLINK_HOPS {
        let Ok(meta) = std::fs::symlink_metadata(&current) else {
            if !current.starts_with(allowed_root) {
                anyhow::bail!(
                    "dangling symlink escape rejected: {} resolves to {} outside {}",
                    link_path.display(),
                    current.display(),
                    allowed_root.display()
                );
            }
            return Ok(current);
        };
        if !meta.file_type().is_symlink() {
            let canon = std::fs::canonicalize(&current).unwrap_or(current);
            if !canon.starts_with(allowed_root) {
                anyhow::bail!(
                    "symlink escape rejected: {} resolves to {} outside {}",
                    link_path.display(),
                    canon.display(),
                    allowed_root.display()
                );
            }
            return Ok(canon);
        }

        let raw_target = std::fs::read_link(&current)
            .with_context(|| format!("failed to read symlink {}", current.display()))?;
        let next = if raw_target.is_absolute() {
            canonicalize_best_effort(&raw_target)?
        } else {
            let base_dir = current
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let canon_base = canonicalize_best_effort(base_dir)?;
            lexical_normalize(&canon_base.join(&raw_target))
        };

        if !next.starts_with(allowed_root) {
            anyhow::bail!(
                "symlink escape rejected: {} -> {} escapes {}",
                link_path.display(),
                next.display(),
                allowed_root.display()
            );
        }
        current = next;
    }

    anyhow::bail!(
        "symlink loop detected while validating {}",
        link_path.display()
    )
}

/// Canonicalize `path` if it exists; otherwise canonicalize its longest existing ancestor and
/// lexically append remaining components (handling macOS `/var` -> `/private/var` consistently).
fn canonicalize_best_effort(path: &Path) -> anyhow::Result<PathBuf> {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return Ok(canon);
    }
    let normalized = lexical_normalize(path);
    let mut missing_tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = normalized.as_path();

    while !cursor.exists() {
        let Some(name) = cursor.file_name() else {
            break;
        };
        missing_tail.push(name.to_os_string());
        match cursor.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => cursor = parent,
            _ => {
                cursor = Path::new(".");
                break;
            }
        }
    }

    let mut base = if cursor.exists() {
        std::fs::canonicalize(cursor).unwrap_or_else(|_| cursor.to_path_buf())
    } else {
        cursor.to_path_buf()
    };
    for part in missing_tail.into_iter().rev() {
        base.push(part);
    }
    Ok(lexical_normalize(&base))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(comp.as_os_str());
                }
            }
            Component::Normal(seg) => out.push(seg),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit_fast_copy_{tag}_{}",
            &Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&dir).expect("mkdir temp dir");
        dir
    }

    #[test]
    fn copy_file_cow_preserves_contents_and_isolates_writes() {
        let root = temp_dir("file_ok");
        let src = root.join("src.txt");
        let dst = root.join("nested").join("dst.txt");
        std::fs::write(&src, "hello cow world").expect("write src");

        let method = copy_file_cow(&src, &dst).expect("copy_file_cow");
        #[cfg(target_os = "macos")]
        assert_eq!(method, CopyMethod::ApfsClone);
        #[cfg(not(target_os = "macos"))]
        assert!(matches!(
            method,
            CopyMethod::LinuxReflink | CopyMethod::Standard
        ));

        assert_eq!(
            std::fs::read_to_string(&dst).expect("read dst"),
            "hello cow world"
        );

        // Overwriting destination must NOT mutate source (true CoW isolation).
        std::fs::write(&dst, "modified copy").expect("write dst");
        assert_eq!(
            std::fs::read_to_string(&src).expect("read src"),
            "hello cow world"
        );

        // Overwriting an existing destination file also succeeds.
        let method2 = copy_file_cow(&src, &dst).expect("overwrite copy_file_cow");
        #[cfg(target_os = "macos")]
        assert_eq!(method2, CopyMethod::ApfsClone);
        assert_eq!(
            std::fs::read_to_string(&dst).expect("read dst again"),
            "hello cow world"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_file_cow_rejects_traversal_and_symlink_escapes() {
        let root = temp_dir("file_guard");
        let src_dir = root.join("src");
        let dst_dir = root.join("dst");
        let outside = root.join("outside.secret");
        std::fs::create_dir_all(&src_dir).expect("mkdir src");
        std::fs::create_dir_all(&dst_dir).expect("mkdir dst");
        std::fs::write(&outside, "TOP_SECRET").expect("write outside");
        std::fs::write(src_dir.join("ok.txt"), "safe").expect("write ok");

        // Reject `..` traversal in source or destination
        assert!(
            copy_file_cow(&src_dir.join("../outside.secret"), &dst_dir.join("out.txt")).is_err()
        );
        assert!(copy_file_cow(&src_dir.join("ok.txt"), &dst_dir.join("../escape.txt")).is_err());

        // Reject directory as source
        assert!(copy_file_cow(&src_dir, &dst_dir.join("dir_as_file")).is_err());

        #[cfg(unix)]
        {
            // Reject symlink pointing outside source parent
            let escape_link = src_dir.join("evil.txt");
            std::os::unix::fs::symlink(&outside, &escape_link).expect("symlink escape");
            assert!(copy_file_cow(&escape_link, &dst_dir.join("evil.txt")).is_err());
            assert!(!dst_dir.join("evil.txt").exists());

            // Reject symlink destination
            let dst_link = dst_dir.join("dst_link.txt");
            std::os::unix::fs::symlink(&outside, &dst_link).expect("dst symlink");
            assert!(copy_file_cow(&src_dir.join("ok.txt"), &dst_link).is_err());
            assert_eq!(
                std::fs::read_to_string(&outside).expect("outside untouched"),
                "TOP_SECRET"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_dir_cow_preserves_tree_internal_symlinks_and_existing_dst() {
        let root = temp_dir("dir_ok");
        let src = root.join("node_modules");
        std::fs::create_dir_all(src.join("pkg/bin")).expect("mkdir pkg");
        std::fs::create_dir_all(src.join(".bin")).expect("mkdir .bin");
        std::fs::write(src.join("pkg/index.js"), "module.exports = 42;").expect("write js");
        std::fs::write(src.join("pkg/bin/cli.js"), "#!/usr/bin/env node\n").expect("write cli");

        #[cfg(unix)]
        {
            // Legitimate internal relative symlink (like npm/pnpm `.bin/*`)
            std::os::unix::fs::symlink("../pkg/bin/cli.js", src.join(".bin/cli"))
                .expect("internal symlink");
        }

        // Case 1: Fresh destination directory
        let dst1 = root.join("wt1/node_modules");
        let method1 = copy_dir_cow(&src, &dst1).expect("copy_dir_cow fresh");
        #[cfg(target_os = "macos")]
        assert_eq!(method1, CopyMethod::ApfsClone);
        #[cfg(not(target_os = "macos"))]
        assert!(matches!(
            method1,
            CopyMethod::LinuxReflink | CopyMethod::Standard
        ));

        assert_eq!(
            std::fs::read_to_string(dst1.join("pkg/index.js")).expect("read dst1"),
            "module.exports = 42;"
        );
        #[cfg(unix)]
        {
            let link_meta = std::fs::symlink_metadata(dst1.join(".bin/cli")).expect("dst1 symlink");
            assert!(link_meta.file_type().is_symlink());
            assert_eq!(
                std::fs::read_to_string(dst1.join(".bin/cli")).expect("read via internal symlink"),
                "#!/usr/bin/env node\n"
            );
        }

        // CoW isolation: mutating dst1 must not alter src
        std::fs::write(dst1.join("pkg/index.js"), "module.exports = 99;").expect("mutate dst1");
        assert_eq!(
            std::fs::read_to_string(src.join("pkg/index.js")).expect("src untouched"),
            "module.exports = 42;"
        );

        // Case 2: Existing destination directory must NOT nest `dst2/node_modules/...`
        let dst2 = root.join("wt2/node_modules");
        std::fs::create_dir_all(&dst2).expect("mkdir dst2");
        let _method2 = copy_dir_cow(&src, &dst2).expect("copy_dir_cow existing");
        assert!(dst2.join("pkg/index.js").is_file());
        assert!(!dst2.join("node_modules").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_dir_cow_rejects_traversal_overlap_and_nested_symlink_escapes() {
        let root = temp_dir("dir_guard");
        let src = root.join("src");
        let dst = root.join("dst");
        let outside = root.join("secret.txt");
        std::fs::create_dir_all(src.join("sub")).expect("mkdir src");
        std::fs::write(&outside, "CLASSIFIED").expect("write outside");
        std::fs::write(src.join("sub/ok.txt"), "ok").expect("write ok");

        // Reject `..` traversal
        assert!(copy_dir_cow(&root.join("src/../src"), &dst).is_err());
        assert!(copy_dir_cow(&src, &root.join("dst/../escape")).is_err());

        // Reject destination inside source
        assert!(copy_dir_cow(&src, &src.join("nested_dst")).is_err());

        #[cfg(unix)]
        {
            // Nested symlink escaping source directory
            std::os::unix::fs::symlink(&outside, src.join("sub/leak.txt")).expect("symlink leak");
            assert!(copy_dir_cow(&src, &dst).is_err());
            assert!(!dst.exists());

            // Remove existing-target escape, test dangling outside symlink
            std::fs::remove_file(src.join("sub/leak.txt")).expect("rm leak");
            std::os::unix::fs::symlink("../../nonexistent_outside.txt", src.join("sub/dangling"))
                .expect("dangling symlink");
            assert!(copy_dir_cow(&src, &dst).is_err());
            assert!(!dst.exists());
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn link_or_clone_dir_supports_reference_link_and_isolated_cow() {
        let root = temp_dir("link_clone");
        let src = root.join("artifacts");
        std::fs::create_dir_all(&src).expect("mkdir artifacts");
        std::fs::write(src.join("report.bin"), "artifact-v1").expect("write artifact");

        // ReferenceLink mode
        let linked = root.join("fork_a/artifacts");
        let outcome =
            link_or_clone_dir(&src, &linked, DirShareMode::ReferenceLink).expect("reflink");
        #[cfg(unix)]
        assert_eq!(outcome, LinkOrCloneOutcome::Symlinked);
        assert_eq!(
            std::fs::read_to_string(linked.join("report.bin")).expect("read linked"),
            "artifact-v1"
        );
        // Idempotent when already linked to same canonical source
        #[cfg(unix)]
        assert_eq!(
            link_or_clone_dir(&src, &linked, DirShareMode::ReferenceLink).expect("idempotent"),
            LinkOrCloneOutcome::Symlinked
        );

        // CopyOnWrite mode
        let cloned = root.join("fork_b/artifacts");
        let outcome_cow =
            link_or_clone_dir(&src, &cloned, DirShareMode::CopyOnWrite).expect("cow clone");
        assert!(matches!(outcome_cow, LinkOrCloneOutcome::Cloned(_)));
        std::fs::write(cloned.join("report.bin"), "artifact-fork-b").expect("mutate fork_b");
        assert_eq!(
            std::fs::read_to_string(src.join("report.bin")).expect("src unaffected"),
            "artifact-v1"
        );

        // Reject traversal & overlapping destination in link_or_clone_dir
        assert!(
            link_or_clone_dir(
                &src,
                &root.join("fork_c/../escape"),
                DirShareMode::ReferenceLink
            )
            .is_err()
        );
        assert!(link_or_clone_dir(&src, &src.join("nested"), DirShareMode::ReferenceLink).is_err());

        // Verify pure-Rust standard recursive fallback preserves files & safe internal symlinks
        let std_dst = root.join("std_fallback");
        let canon_src = std::fs::canonicalize(&src).expect("canon src");
        copy_dir_standard(&canon_src, &std_dst, &canon_src).expect("copy_dir_standard");
        assert_eq!(
            std::fs::read_to_string(std_dst.join("report.bin")).expect("read std fallback"),
            "artifact-v1"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
