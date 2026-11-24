//! Shared provider-side helpers for Plan Mode approval detection, safe plan-file I/O,
//! and provider-specific control-response formatting.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use orbit_protocol::{
    PlanApprovalAction, PlanApprovalPayload, SessionMode, build_plan_resolution_message,
    extract_codex_proposed_plan, extract_plan_approval_from_tool_input, extract_plan_title,
    is_plan_edited,
};
use serde_json::{Value, json};

const MAX_PLAN_FILE_BYTES: u64 = 256 * 1024;
const MAX_EDITED_PLAN_BYTES: usize = 512 * 1024;

#[cfg(unix)]
fn unix_o_nofollow_flag() -> Option<i32> {
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

fn resolve_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix(r"~\"))
        && let Some(home) = resolve_home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

fn has_parent_dir_traversal(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

fn has_allowed_plan_extension(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(ext.as_deref(), Some("md" | "markdown" | "txt"))
}

struct AllowedRoot {
    raw: PathBuf,
    canonical: PathBuf,
}

/// Strictly permit plan file reads/writes ONLY inside the agent workspace root and
/// dedicated provider plan directories (`~/.claude/plans`, `~/.gemini/antigravity/brain`,
/// `~/.gemini/antigravity-cli/brain`). Never allow `$ORBIT_HOME` or `~/.orbit`.
fn allowed_plan_roots(workspace_cwd: &Path) -> Vec<AllowedRoot> {
    let mut roots = Vec::new();
    let mut push_root = |raw: PathBuf| {
        if let Ok(canonical) = std::fs::canonicalize(&raw)
            && !roots.iter().any(|r: &AllowedRoot| r.canonical == canonical)
        {
            roots.push(AllowedRoot { raw, canonical });
        }
    };

    if workspace_cwd.is_absolute() {
        push_root(workspace_cwd.to_path_buf());
    } else if let Ok(abs) = std::env::current_dir().map(|cwd| cwd.join(workspace_cwd)) {
        push_root(abs);
    }

    if let Some(home) = resolve_home_dir() {
        for rel in [
            ".claude/plans",
            ".gemini/antigravity/brain",
            ".gemini/antigravity-cli/brain",
        ] {
            push_root(home.join(rel));
        }
    }

    roots
}

/// Verify that every path component under `root_prefix` is a normal entry (no symlinks,
/// no `..`), returning the verified path if all checks pass.
fn verify_no_symlink_components_under_root(
    root_prefix: &Path,
    candidate: &Path,
) -> Option<PathBuf> {
    let rel = candidate.strip_prefix(root_prefix).ok()?;
    let mut current = root_prefix.to_path_buf();
    let mut saw_normal = false;
    for comp in rel.components() {
        match comp {
            Component::CurDir => continue,
            Component::Normal(seg) => {
                saw_normal = true;
                current.push(seg);
                let lmeta = std::fs::symlink_metadata(&current).ok()?;
                if lmeta.file_type().is_symlink() {
                    return None;
                }
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if saw_normal { Some(current) } else { None }
}

/// Resolve and strictly validate a candidate plan file path:
/// - rejects empty paths and `..` traversal
/// - enforces `.md`/`.markdown`/`.txt` extension
/// - verifies path sits inside an allowed root with zero symlink components inside the root
/// - verifies target is an existing non-symlink regular file
fn resolve_checked_plan_path(raw_path: &str, workspace_cwd: &Path) -> Option<PathBuf> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = expand_tilde(trimmed);
    if has_parent_dir_traversal(&expanded) {
        return None;
    }
    let candidate = if expanded.is_absolute() {
        expanded
    } else {
        workspace_cwd.join(expanded)
    };
    if !has_allowed_plan_extension(&candidate) {
        return None;
    }

    let roots = allowed_plan_roots(workspace_cwd);
    for root in &roots {
        let verified = verify_no_symlink_components_under_root(&root.canonical, &candidate)
            .or_else(|| verify_no_symlink_components_under_root(&root.raw, &candidate));
        let Some(verified_path) = verified else {
            continue;
        };
        let canonical = std::fs::canonicalize(&verified_path).ok()?;
        if !canonical.starts_with(&root.canonical) || !has_allowed_plan_extension(&canonical) {
            continue;
        }
        // Also verify canonical components under canonical root have no symlinks.
        if verify_no_symlink_components_under_root(&root.canonical, &canonical).is_none() {
            continue;
        }
        let lmeta = std::fs::symlink_metadata(&canonical).ok()?;
        if lmeta.file_type().is_symlink() || !lmeta.is_file() {
            return None;
        }
        return Some(canonical);
    }

    None
}

fn open_verified_plan_file_for_read(canonical: &Path) -> Option<std::fs::File> {
    let lmeta_before = std::fs::symlink_metadata(canonical).ok()?;
    if lmeta_before.file_type().is_symlink() || !lmeta_before.is_file() {
        return None;
    }

    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    if let Some(flag) = unix_o_nofollow_flag() {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(flag);
    }
    let file = opts.open(canonical).ok()?;
    let fmeta = file.metadata().ok()?;
    if !fmeta.is_file() || fmeta.len() == 0 || fmeta.len() > MAX_PLAN_FILE_BYTES {
        return None;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let lmeta_after = std::fs::symlink_metadata(canonical).ok()?;
        if lmeta_after.file_type().is_symlink()
            || fmeta.dev() != lmeta_before.dev()
            || fmeta.ino() != lmeta_before.ino()
            || fmeta.dev() != lmeta_after.dev()
            || fmeta.ino() != lmeta_after.ino()
        {
            return None;
        }
    }

    Some(file)
}

/// Safely read a `.md`/`.markdown`/`.txt` plan file from the agent workspace,
/// `~/.claude/plans`, `~/.gemini/antigravity/brain`, or `~/.gemini/antigravity-cli/brain`.
///
/// Hardened against symlink traversal/swap (`O_NOFOLLOW` + `(dev, ino)` identity checks)
/// and bounded to `MAX_PLAN_FILE_BYTES`.
pub fn read_plan_file_safe(raw_path: &str, workspace_cwd: &Path) -> Option<String> {
    let canonical = resolve_checked_plan_path(raw_path, workspace_cwd)?;
    let file = open_verified_plan_file_for_read(&canonical)?;

    let mut buf = Vec::new();
    file.take(MAX_PLAN_FILE_BYTES + 1)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.is_empty() || (buf.len() as u64) > MAX_PLAN_FILE_BYTES {
        return None;
    }
    let content = String::from_utf8(buf).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Safely persist user edits back into an existing verified Markdown plan file via atomic
/// `create_new(true)` + `O_NOFOLLOW` temp file + `rename` inside a verified non-symlink parent.
///
/// Returns `Ok(true)` if the file was updated, `Ok(false)` if the path was outside allowed
/// plan roots or not an existing verified `.md`/`.markdown`/`.txt` file.
pub fn write_edited_plan_file_safe(
    raw_path: &str,
    workspace_cwd: &Path,
    edited_markdown: &str,
) -> Result<bool, String> {
    if edited_markdown.len() > MAX_EDITED_PLAN_BYTES {
        return Err("edited plan exceeds maximum supported size".to_string());
    }
    let Some(canonical) = resolve_checked_plan_path(raw_path, workspace_cwd) else {
        return Ok(false);
    };
    let parent = canonical
        .parent()
        .ok_or_else(|| "plan file has no parent directory".to_string())?;
    let parent_meta = std::fs::symlink_metadata(parent)
        .map_err(|e| format!("failed to inspect plan parent directory: {e}"))?;
    if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
        return Err("plan file parent directory is not a verified directory".to_string());
    }
    let target_meta_before = std::fs::symlink_metadata(&canonical)
        .map_err(|e| format!("failed to inspect plan file: {e}"))?;
    if target_meta_before.file_type().is_symlink() || !target_meta_before.is_file() {
        return Err("plan file target is not a verified regular file".to_string());
    }

    let tmp_name = format!(".orbit-plan-edit-{}.tmp", uuid::Uuid::new_v4().simple());
    let tmp_path = parent.join(tmp_name);

    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
        if let Some(flag) = unix_o_nofollow_flag() {
            opts.custom_flags(flag);
        }
    }

    let mut tmp_file = opts
        .open(&tmp_path)
        .map_err(|e| format!("failed to create temporary plan file: {e}"))?;

    let write_res = (|| -> Result<(), String> {
        tmp_file
            .write_all(edited_markdown.as_bytes())
            .map_err(|e| format!("failed to write temporary plan file: {e}"))?;
        tmp_file
            .flush()
            .map_err(|e| format!("failed to flush temporary plan file: {e}"))?;
        tmp_file
            .sync_all()
            .map_err(|e| format!("failed to sync temporary plan file: {e}"))?;
        let _ = std::fs::set_permissions(&tmp_path, target_meta_before.permissions());
        Ok(())
    })();
    drop(tmp_file);

    if let Err(err) = write_res {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    // Re-verify parent & target identity immediately prior to atomic rename.
    let parent_meta_after = match std::fs::symlink_metadata(parent) {
        Ok(m) => m,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(format!("failed to re-verify plan parent directory: {e}"));
        }
    };
    let target_meta_after = match std::fs::symlink_metadata(&canonical) {
        Ok(m) => m,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(format!("failed to re-verify plan file before rename: {e}"));
        }
    };
    let unchanged_identity = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            !parent_meta_after.file_type().is_symlink()
                && parent_meta_after.is_dir()
                && !target_meta_after.file_type().is_symlink()
                && target_meta_after.is_file()
                && target_meta_after.dev() == target_meta_before.dev()
                && target_meta_after.ino() == target_meta_before.ino()
        }
        #[cfg(not(unix))]
        {
            !parent_meta_after.file_type().is_symlink()
                && parent_meta_after.is_dir()
                && !target_meta_after.file_type().is_symlink()
                && target_meta_after.is_file()
        }
    };
    if !unchanged_identity {
        let _ = std::fs::remove_file(&tmp_path);
        return Err("plan file changed or became a symlink during save".to_string());
    }

    if let Err(e) = std::fs::rename(&tmp_path, &canonical) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("failed to atomically replace plan file: {e}"));
    }

    Ok(true)
}

/// Populate missing `plan_markdown` and `plan_title` fields on a [`PlanApprovalPayload`]
/// from verified disk files or recent turn context.
///
/// Strict write-back invariants:
/// - NEVER attaches a historical `fallback_plan_file` to an inline plan whose `plan_markdown`
///   is already populated.
/// - Sets `plan_file_backed = true` ONLY when `plan_markdown` was actually loaded from (or
///   verified identical to) that exact verified `plan_file_path` on disk.
pub fn enrich_plan_payload(
    payload: &mut PlanApprovalPayload,
    workspace_cwd: &Path,
    fallback_plan_file: Option<&str>,
    fallback_assistant_text: Option<&str>,
) {
    if payload.plan_markdown.trim().is_empty() {
        if let Some(ref fp) = payload.plan_file_path {
            if let Some(disk_md) = read_plan_file_safe(fp, workspace_cwd) {
                payload.plan_markdown = disk_md;
                payload.plan_file_backed = true;
            } else {
                payload.plan_file_backed = false;
            }
        } else if let Some(fp) = fallback_plan_file.map(str::trim).filter(|s| !s.is_empty())
            && let Some(disk_md) = read_plan_file_safe(fp, workspace_cwd)
        {
            payload.plan_file_path = Some(fp.to_string());
            payload.plan_markdown = disk_md;
            payload.plan_file_backed = true;
        }

        if payload.plan_markdown.trim().is_empty() {
            if let Some(raw_text) = fallback_assistant_text {
                let candidate = extract_codex_proposed_plan(raw_text)
                    .unwrap_or_else(|| raw_text.trim().to_string());
                if !candidate.is_empty() {
                    payload.plan_markdown = candidate;
                    payload.plan_file_backed = false;
                }
            } else if let Some(ref summary) = payload.summary
                && !summary.trim().is_empty()
            {
                // Last-resort display fallback when the backing artifact could not be read;
                // `plan_file_backed` MUST remain `false` so edits never overwrite disk.
                payload.plan_markdown = summary.trim().to_string();
                payload.plan_file_backed = false;
            }
        }
    } else if !payload.plan_file_backed {
        // Inline plan already present: NEVER attach `fallback_plan_file`.
        // If the tool explicitly declared `plan_file_path`, only enable disk write-back when
        // that exact verified disk file currently holds the exact same plan content.
        if let Some(ref fp) = payload.plan_file_path
            && let Some(disk_md) = read_plan_file_safe(fp, workspace_cwd)
            && !is_plan_edited(&disk_md, Some(&payload.plan_markdown))
        {
            payload.plan_file_backed = true;
        }
    }

    if payload.plan_title.is_none() && !payload.plan_markdown.trim().is_empty() {
        payload.plan_title = extract_plan_title(&payload.plan_markdown);
    }
}

/// Extract a [`PlanApprovalPayload`] from an explicit ACP `session/request_permission`
/// plan-review `toolCall` object (`ExitPlanMode` / `notify_user` / explicit `kind: "plan"`).
///
/// Ordinary ACP file-edit/read/shell tools are NEVER promoted even if their path looks like
/// a plan file.
pub fn extract_plan_approval_from_acp_tool_call(
    tool_call: &Value,
    mode: &SessionMode,
    workspace_cwd: &Path,
    source: &str,
) -> Option<PlanApprovalPayload> {
    let in_plan_mode = matches!(mode, SessionMode::Plan);
    let kind_str = tool_call.get("kind").and_then(Value::as_str).unwrap_or("");
    let title = tool_call.get("title").and_then(Value::as_str);
    let raw_input = tool_call.get("rawInput").unwrap_or(&Value::Null);
    let tool_name = tool_call
        .get("toolName")
        .or_else(|| tool_call.get("name"))
        .or_else(|| tool_call.pointer("/_meta/toolName"))
        .or_else(|| raw_input.get("tool_name"))
        .or_else(|| raw_input.get("name"))
        .and_then(Value::as_str)
        .or(title)
        .unwrap_or("");

    let location_path = tool_call
        .pointer("/locations/0/path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut payload = extract_plan_approval_from_tool_input(
        tool_name,
        raw_input,
        title,
        in_plan_mode,
        Some(source),
    );

    if payload.is_none()
        && matches!(
            kind_str.to_ascii_lowercase().as_str(),
            "plan" | "plan_approval" | "plan_review"
        )
    {
        let mut plan_markdown = raw_input
            .get("plan")
            .or_else(|| raw_input.get("plan_markdown"))
            .or_else(|| raw_input.get("markdown"))
            .or_else(|| raw_input.get("content"))
            .or_else(|| tool_call.get("description"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        if plan_markdown.is_empty()
            && let Some(content_arr) = tool_call.get("content").and_then(Value::as_array)
        {
            plan_markdown = content_arr
                .iter()
                .filter_map(|item| {
                    item.get("text")
                        .or_else(|| item.pointer("/content/text"))
                        .and_then(Value::as_str)
                })
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
        }
        payload = Some(PlanApprovalPayload {
            plan_title: extract_plan_title(&plan_markdown).or_else(|| {
                title
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
            }),
            plan_markdown,
            plan_file_path: location_path.map(ToOwned::to_owned),
            summary: None,
            plan_file_backed: false,
            allowed_prompts: Vec::new(),
            source: Some(source.to_string()),
        });
    }

    if let Some(ref mut p) = payload {
        // Only supply `location_path` as fallback when the payload itself did not specify
        // an explicit inline plan or path.
        let fallback_file = if p.plan_markdown.trim().is_empty() && p.plan_file_path.is_none() {
            location_path
        } else {
            None
        };
        enrich_plan_payload(p, workspace_cwd, fallback_file, None);
    }

    payload
}

/// Build the Claude Code `control_response` frame for a [`PlanApprovalAction`].
///
/// On one-time `ApproveAndBuild`, emits ONLY the authorized session-scoped `setMode`
/// transition — never auto-granting arbitrary upstream `permission_suggestions`.
#[allow(clippy::too_many_arguments)]
pub fn build_claude_plan_control_response(
    control_request_id: &str,
    tool_use_id: &str,
    original_input: &Value,
    original_plan_markdown: &str,
    target_claude_mode: &str,
    action: PlanApprovalAction,
    edited_plan_markdown: Option<&str>,
    feedback: Option<&str>,
) -> Value {
    match action {
        PlanApprovalAction::ApproveAndBuild => {
            let mut updated_input = original_input.clone();
            if !updated_input.is_object() {
                updated_input = json!({});
            }
            if let Some(obj) = updated_input.as_object_mut() {
                if let Some(edited) = edited_plan_markdown
                    && is_plan_edited(original_plan_markdown, Some(edited))
                {
                    obj.insert("plan".to_string(), Value::String(edited.to_string()));
                }
                if let Some(notes) = feedback.map(str::trim).filter(|s| !s.is_empty()) {
                    obj.insert("userFeedback".to_string(), Value::String(notes.to_string()));
                }
            }
            let updated_perms = json!([
                {
                    "type": "setMode",
                    "mode": target_claude_mode,
                    "destination": "session"
                }
            ]);
            json!({
                "type": "control_response",
                "response": {
                    "subtype": "success",
                    "request_id": control_request_id,
                    "response": {
                        "behavior": "allow",
                        "updatedInput": updated_input,
                        "updatedPermissions": updated_perms,
                        "toolUseID": tool_use_id,
                        "decisionClassification": "user_temporary"
                    }
                }
            })
        }
        PlanApprovalAction::RequestChanges | PlanApprovalAction::Dismiss => {
            let msg = build_plan_resolution_message(
                action,
                original_plan_markdown,
                edited_plan_markdown,
                feedback,
            );
            json!({
                "type": "control_response",
                "response": {
                    "subtype": "success",
                    "request_id": control_request_id,
                    "response": {
                        "behavior": "deny",
                        "message": msg,
                        "interrupt": false,
                        "toolUseID": tool_use_id,
                        "decisionClassification": "user_reject"
                    }
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_plan_file_within_workspace_and_blocks_traversal_and_non_markdown() {
        let dir = std::env::temp_dir().join(format!("orbit_plan_io_{}", uuid::Uuid::new_v4()));
        assert!(std::fs::create_dir_all(&dir).is_ok());
        let plan_path = dir.join("implementation_plan.md");
        assert!(std::fs::write(&plan_path, "# Original\n\n- Step 1\n").is_ok());

        // Non-markdown files inside workspace must be rejected on BOTH read and write
        let secret_env = dir.join("credentials.json");
        assert!(std::fs::write(&secret_env, r#"{"token":"secret"}"#).is_ok());
        assert!(read_plan_file_safe("credentials.json", &dir).is_none());
        assert_eq!(
            write_edited_plan_file_safe("credentials.json", &dir, "clobbered"),
            Ok(false)
        );

        let loaded = read_plan_file_safe("implementation_plan.md", &dir);
        assert_eq!(loaded.as_deref(), Some("# Original\n\n- Step 1"));

        // Traversal attempt must be rejected cleanly
        assert!(read_plan_file_safe("../../etc/passwd", &dir).is_none());

        let updated =
            write_edited_plan_file_safe("implementation_plan.md", &dir, "# Edited\n\n- Step 1\n");
        assert_eq!(updated, Ok(true));
        let reread = read_plan_file_safe("implementation_plan.md", &dir);
        assert_eq!(reread.as_deref(), Some("# Edited\n\n- Step 1"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_orbit_home_reads_and_writes_even_for_markdown() {
        let ws = std::env::temp_dir().join(format!("orbit_plan_ws_{}", uuid::Uuid::new_v4()));
        let fake_orbit_home =
            std::env::temp_dir().join(format!("orbit_fake_home_{}", uuid::Uuid::new_v4()));
        assert!(std::fs::create_dir_all(&ws).is_ok());
        assert!(std::fs::create_dir_all(&fake_orbit_home).is_ok());
        let orbit_secret_md = fake_orbit_home.join("pairing_notes.md");
        assert!(std::fs::write(&orbit_secret_md, "# Secret Orbit Home File\n").is_ok());

        // Even if ORBIT_HOME points to fake_orbit_home (or ~/.orbit exists), reads/writes must be rejected!
        let path_str = orbit_secret_md.to_string_lossy().to_string();
        assert!(
            read_plan_file_safe(&path_str, &ws).is_none(),
            "files outside workspace / provider plan dirs (including $ORBIT_HOME) must NEVER be readable"
        );
        assert_eq!(
            write_edited_plan_file_safe(&path_str, &ws, "# Overwritten"),
            Ok(false)
        );

        // Also explicitly verify `~/.orbit/...` is not in allowed_plan_roots
        let roots = allowed_plan_roots(&ws);
        for root in &roots {
            let s = root.canonical.to_string_lossy();
            assert!(
                !s.contains("/.orbit"),
                "~/.orbit must never appear in allowed_plan_roots: {s}"
            );
        }

        let _ = std::fs::remove_dir_all(&ws);
        let _ = std::fs::remove_dir_all(&fake_orbit_home);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_inside_workspace_for_read_and_write() {
        use std::os::unix::fs::symlink;

        let ws = std::env::temp_dir().join(format!("orbit_plan_sym_{}", uuid::Uuid::new_v4()));
        let outside = std::env::temp_dir().join(format!("orbit_plan_out_{}", uuid::Uuid::new_v4()));
        assert!(std::fs::create_dir_all(&ws).is_ok());
        assert!(std::fs::create_dir_all(&outside).is_ok());

        let target = outside.join("secret.md");
        assert!(std::fs::write(&target, "# Outside Secret\n").is_ok());
        let link_file = ws.join("plan_link.md");
        assert!(symlink(&target, &link_file).is_ok());

        // Symlink inside workspace pointing outside
        assert!(read_plan_file_safe("plan_link.md", &ws).is_none());
        assert_eq!(
            write_edited_plan_file_safe("plan_link.md", &ws, "# Hacked"),
            Ok(false)
        );

        // Symlink inside workspace pointing to a real file inside workspace must ALSO be rejected
        let real_inside = ws.join("real_plan.md");
        assert!(std::fs::write(&real_inside, "# Real Plan\n").is_ok());
        let internal_link = ws.join("internal_link.md");
        assert!(symlink(&real_inside, &internal_link).is_ok());
        assert!(read_plan_file_safe("internal_link.md", &ws).is_none());
        assert_eq!(
            write_edited_plan_file_safe("internal_link.md", &ws, "# Clobbered"),
            Ok(false)
        );

        // Symlink directory component inside workspace must be rejected
        let sub_real = ws.join("real_dir");
        assert!(std::fs::create_dir_all(&sub_real).is_ok());
        let nested = sub_real.join("plan.md");
        assert!(std::fs::write(&nested, "# Nested\n").is_ok());
        let sym_dir = ws.join("sym_dir");
        assert!(symlink(&sub_real, &sym_dir).is_ok());
        assert!(read_plan_file_safe("sym_dir/plan.md", &ws).is_none());
        assert_eq!(
            write_edited_plan_file_safe("sym_dir/plan.md", &ws, "# Clobbered"),
            Ok(false)
        );

        let _ = std::fs::remove_dir_all(&ws);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn builds_claude_approve_with_only_session_set_mode_and_request_changes_responses() {
        let orig = "# Plan\n- A";
        let allow = build_claude_plan_control_response(
            "ctrl_1",
            "toolu_1",
            &json!({ "plan": orig }),
            orig,
            "default",
            PlanApprovalAction::ApproveAndBuild,
            Some("# Plan\n- A\n- B"),
            Some("Focus on tests first"),
        );
        assert_eq!(allow["response"]["response"]["behavior"], json!("allow"));
        assert_eq!(
            allow["response"]["response"]["updatedInput"]["plan"],
            json!("# Plan\n- A\n- B")
        );
        assert_eq!(
            allow["response"]["response"]["updatedInput"]["userFeedback"],
            json!("Focus on tests first")
        );
        assert_eq!(
            allow["response"]["response"]["updatedPermissions"],
            json!([
                {
                    "type": "setMode",
                    "mode": "default",
                    "destination": "session"
                }
            ])
        );

        let revise = build_claude_plan_control_response(
            "ctrl_2",
            "toolu_2",
            &json!({ "plan": orig }),
            orig,
            "default",
            PlanApprovalAction::RequestChanges,
            None,
            Some("Split phase 2"),
        );
        assert_eq!(revise["response"]["response"]["behavior"], json!("deny"));
        assert_eq!(revise["response"]["response"]["interrupt"], json!(false));
        let msg = revise["response"]["response"]["message"]
            .as_str()
            .unwrap_or("");
        assert!(msg.contains("Stay in plan mode"));
        assert!(msg.contains("Split phase 2"));
    }

    #[test]
    fn extracts_acp_plan_approval_explicitly_and_never_promotes_ordinary_write() {
        let dir = std::env::temp_dir().join(format!("orbit_acp_plan_{}", uuid::Uuid::new_v4()));
        assert!(std::fs::create_dir_all(&dir).is_ok());
        let plan_file = dir.join("implementation_plan.md");
        assert!(std::fs::write(&plan_file, "# Auth Migration\n\n1. Rotate keys\n").is_ok());

        // Ordinary ACP Write tool call touching a plan-looking file MUST NOT become a plan gate
        let write_call = json!({
            "title": "Write implementation_plan.md",
            "toolName": "Write",
            "kind": "edit",
            "locations": [{ "path": plan_file.to_string_lossy() }],
            "rawInput": {
                "file_path": plan_file.to_string_lossy(),
                "content": "# Auth Migration\n\n1. Rotate keys\n"
            }
        });
        assert!(
            extract_plan_approval_from_acp_tool_call(
                &write_call,
                &SessionMode::Plan,
                &dir,
                "acp_plan"
            )
            .is_none(),
            "ordinary ACP Write touching a plan path must NEVER be promoted to a plan approval gate"
        );

        // Explicit ExitPlanMode IS recognized
        let exit_call = json!({
            "title": "ExitPlanMode",
            "kind": "other",
            "rawInput": {
                "plan": "# Refactor Auth\n\n1. Token rotation"
            }
        });
        let extracted = extract_plan_approval_from_acp_tool_call(
            &exit_call,
            &SessionMode::Plan,
            &dir,
            "acp_plan",
        );
        let extracted = match extracted {
            Some(p) => p,
            None => panic!("expected ACP ExitPlanMode plan payload"),
        };
        assert_eq!(extracted.plan_title.as_deref(), Some("Refactor Auth"));
        assert_eq!(extracted.source.as_deref(), Some("acp_plan"));
        assert!(!extracted.plan_file_backed);

        // Antigravity notify_user loads the actual plan file from disk and keeps Message as summary
        let agy_call = json!({
            "toolName": "notify_user",
            "kind": "other",
            "rawInput": {
                "BlockedOnUser": true,
                "PathsToReview": [plan_file.to_string_lossy()],
                "Message": "Please review the plan before coding."
            }
        });
        let agy_plan = extract_plan_approval_from_acp_tool_call(
            &agy_call,
            &SessionMode::Plan,
            &dir,
            "antigravity_plan",
        );
        let agy_plan = match agy_plan {
            Some(p) => p,
            None => panic!("expected AGY notify_user plan payload"),
        };
        assert_eq!(agy_plan.plan_markdown, "# Auth Migration\n\n1. Rotate keys");
        assert_eq!(agy_plan.plan_title.as_deref(), Some("Auth Migration"));
        assert_eq!(
            agy_plan.summary.as_deref(),
            Some("Please review the plan before coding.")
        );
        assert!(agy_plan.plan_file_backed);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enrich_never_attaches_historical_fallback_path_to_inline_plan() {
        let dir = std::env::temp_dir().join(format!("orbit_plan_enr_{}", uuid::Uuid::new_v4()));
        assert!(std::fs::create_dir_all(&dir).is_ok());
        let old_file = dir.join("old_plan.md");
        assert!(std::fs::write(&old_file, "# Old Plan\n- Old step\n").is_ok());

        let mut inline = PlanApprovalPayload {
            plan_markdown: "# New Inline Plan\n- Fresh step".into(),
            plan_title: Some("New Inline Plan".into()),
            plan_file_path: None,
            summary: None,
            plan_file_backed: false,
            allowed_prompts: Vec::new(),
            source: Some("exit_plan_mode".into()),
        };
        enrich_plan_payload(&mut inline, &dir, Some(&old_file.to_string_lossy()), None);
        assert!(
            inline.plan_file_path.is_none(),
            "inline plan must never inherit an unrelated historical fallback plan_file_path"
        );
        assert!(!inline.plan_file_backed);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
