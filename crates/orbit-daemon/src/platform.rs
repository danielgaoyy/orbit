//! Cross-platform OS portability helpers for `orbit-daemon`.
//!
//! Centralizes Windows/Unix path normalization (`\\?\` verbatim prefix stripping, drive-root
//! detection), home-directory resolution (`HOME` / `USERPROFILE` / `HOMEDRIVE`+`HOMEPATH`),
//! executable extension lookup (`.exe`/`.cmd`/`.bat`), default shell selection, and zero-`unsafe`
//! process-tree termination.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

/// Windows `CREATE_NEW_PROCESS_GROUP` flag (safe `CommandExt::creation_flags`).
#[cfg(windows)]
pub const WIN_CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Windows `CREATE_NO_WINDOW` flag (prevents console window flashing on Windows desktop).
#[cfg(windows)]
pub const WIN_CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Resolve the current user's home directory portably across Unix (`HOME`) and Windows
/// (`USERPROFILE`, `HOMEDRIVE` + `HOMEPATH`).
pub fn resolve_user_home_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(home));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(profile));
    }
    if let (Some(drive), Some(path)) = (
        std::env::var_os("HOMEDRIVE").filter(|v| !v.is_empty()),
        std::env::var_os("HOMEPATH").filter(|v| !v.is_empty()),
    ) {
        let mut combined = drive;
        combined.push(path);
        return Some(PathBuf::from(combined));
    }
    None
}

/// Strip Windows extended-length verbatim path prefixes (`\\?\C:\...` -> `C:\...`,
/// `\\?\UNC\server\share` -> `\\server\share`) from any path string.
/// Safe and idempotent on all platforms (Unix paths pass through unchanged).
pub fn strip_windows_verbatim_prefix_str(raw: &str) -> String {
    if let Some(unc_rest) = raw
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| raw.strip_prefix(r"\\?\unc\"))
    {
        return format!(r"\\{unc_rest}");
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        // Only strip when followed by a drive prefix (`C:\...` or `C:/...` or `C:`)
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return rest.to_string();
        }
    }
    raw.to_string()
}

/// Strip Windows extended-length verbatim path prefixes (`\\?\C:\...`, `\\?\UNC\...`) from a `Path`.
pub fn strip_windows_verbatim_prefix(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let stripped = strip_windows_verbatim_prefix_str(&raw);
    if stripped == raw {
        path.to_path_buf()
    } else {
        PathBuf::from(stripped)
    }
}

/// Canonicalize `path` and strip Windows `\\?\` verbatim prefixes so downstream tools
/// (`git`, `cmd.exe`, LSP URIs, UI path prefix checks) receive clean standard paths.
pub fn canonicalize_clean(path: &Path) -> std::io::Result<PathBuf> {
    let canon = std::fs::canonicalize(path)?;
    Ok(strip_windows_verbatim_prefix(&canon))
}

/// Async counterpart of [`canonicalize_clean`].
pub async fn canonicalize_clean_async(path: &Path) -> std::io::Result<PathBuf> {
    let canon = tokio::fs::canonicalize(path).await?;
    Ok(strip_windows_verbatim_prefix(&canon))
}

/// Returns `true` if `path` represents a filesystem root on Unix (`/`) or Windows
/// (`\`, `C:\`, `C:/`, `C:`, `\\?\C:\`, `\\server\share\`).
pub fn is_filesystem_root_path(path: &Path) -> bool {
    if path.parent().is_none() {
        return true;
    }
    let raw = strip_windows_verbatim_prefix_str(&path.to_string_lossy());
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed == "/" || trimmed == "\\" {
        return true;
    }
    // Windows drive root: `C:`, `C:\`, `C:/`, `C:\\`
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let rest = &trimmed[2..];
        if rest.is_empty() || rest.chars().all(|c| c == '\\' || c == '/') {
            return true;
        }
    }
    // Windows UNC share root: `\\server\share` or `\\server\share\`
    if let Some(unc) = trimmed
        .strip_prefix(r"\\")
        .or_else(|| trimmed.strip_prefix("//"))
    {
        let parts: Vec<&str> = unc
            .split(['\\', '/'])
            .filter(|seg| !seg.is_empty())
            .collect();
        if parts.len() <= 2 {
            return true;
        }
    }
    // Fallback component check
    let has_normal = path.components().any(|c| matches!(c, Component::Normal(_)));
    !has_normal
}

/// Build ordered Windows executable candidate paths for `binary_name` inside `dir`, consulting
/// `pathext` (e.g. `".COM;.EXE;.BAT;.CMD"`) or default Windows executable extensions.
///
/// Works identically across all OSes so Windows lookup semantics can be unit-tested on Unix too.
pub fn windows_executable_candidates(
    dir: &Path,
    binary_name: &str,
    pathext: Option<&str>,
) -> Vec<PathBuf> {
    let trimmed = binary_name.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let push_candidate = |out: &mut Vec<PathBuf>, seen: &mut HashSet<String>, p: PathBuf| {
        let key = p.to_string_lossy().to_ascii_lowercase();
        if seen.insert(key) {
            out.push(p);
        }
    };

    let raw_exts: Vec<String> = pathext
        .unwrap_or(".EXE;.CMD;.BAT;.COM")
        .split(';')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| {
            let lower = e.to_ascii_lowercase();
            if lower.starts_with('.') {
                lower
            } else {
                format!(".{lower}")
            }
        })
        .collect();

    let lower_name = trimmed.to_ascii_lowercase();
    let already_has_exec_ext = raw_exts.iter().any(|ext| lower_name.ends_with(ext));

    if already_has_exec_ext {
        push_candidate(&mut out, &mut seen, dir.join(trimmed));
        return out;
    }

    // Prefer explicit Windows executable extensions first (avoid matching extensionless POSIX shims
    // like npm's `foo` shell script before `foo.cmd` / `foo.exe`!).
    for ext in &raw_exts {
        push_candidate(&mut out, &mut seen, dir.join(format!("{trimmed}{ext}")));
    }
    // Keep exact match as a last resort (e.g. explicitly provided custom binary)
    push_candidate(&mut out, &mut seen, dir.join(trimmed));
    out
}

/// Resolve an executable file inside `dir` taking Windows `PATHEXT` extensions into account.
pub fn find_executable_in_dir(
    dir: &Path,
    binary_name: &str,
    is_exec: fn(&Path) -> bool,
) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let pathext = std::env::var("PATHEXT").ok();
        for candidate in windows_executable_candidates(dir, binary_name, pathext.as_deref()) {
            if is_exec(&candidate) {
                return Some(candidate);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let candidate = dir.join(binary_name);
        is_exec(&candidate).then_some(candidate)
    }
}

/// Return the default interactive shell binary for the host OS.
pub fn default_shell_program() -> String {
    #[cfg(unix)]
    {
        std::env::var("SHELL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "/bin/bash".to_string())
    }
    #[cfg(windows)]
    {
        if let Some(pwsh) = find_windows_shell_in_path(&["pwsh.exe", "powershell.exe"]) {
            return pwsh;
        }
        std::env::var("COMSPEC")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string())
    }
    #[cfg(not(any(unix, windows)))]
    {
        "/bin/sh".to_string()
    }
}

#[cfg(windows)]
fn find_windows_shell_in_path(names: &[&str]) -> Option<String> {
    let path_env = std::env::var_os("PATH")?;
    for name in names {
        for dir in std::env::split_paths(&path_env) {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Return `(program, args)` to execute a shell command string cleanly on the current platform.
pub fn shell_command_for_platform(
    command_str: &str,
    login_shell_on_unix: bool,
) -> (String, Vec<String>) {
    #[cfg(unix)]
    {
        let flag = if login_shell_on_unix { "-lc" } else { "-c" };
        (
            "/bin/sh".to_string(),
            vec![flag.to_string(), command_str.to_string()],
        )
    }
    #[cfg(windows)]
    {
        let _ = login_shell_on_unix;
        let comspec = std::env::var("COMSPEC")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string());
        (comspec, vec!["/C".to_string(), command_str.to_string()])
    }
    #[cfg(not(any(unix, windows)))]
    {
        let flag = if login_shell_on_unix { "-lc" } else { "-c" };
        (
            "/bin/sh".to_string(),
            vec![flag.to_string(), command_str.to_string()],
        )
    }
}

/// Build a `tokio::process::Command` executing `command_str` via the platform shell
/// (`/bin/sh -c`/`-lc` on Unix, `cmd.exe /C "<command_str>"` via `raw_arg` on Windows).
pub fn build_shell_command(
    command_str: &str,
    login_shell_on_unix: bool,
) -> tokio::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = login_shell_on_unix;
        let comspec = std::env::var("COMSPEC")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string());
        let mut std_cmd = std::process::Command::new(comspec);
        // `cmd.exe /C` strips outer quotes when wrapped in `"` and preserves inner quoted paths
        // and `&&`/`||` pipelines verbatim; `raw_arg` bypasses MSVC argv backslash-escaping.
        std_cmd
            .arg("/C")
            .raw_arg(format!("\"{command_str}\""))
            .creation_flags(WIN_CREATE_NO_WINDOW);
        tokio::process::Command::from(std_cmd)
    }
    #[cfg(not(windows))]
    {
        let (program, args) = shell_command_for_platform(command_str, login_shell_on_unix);
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        cmd
    }
}

/// Replace an executable at `dst` with a staged file `tmp` safely across platforms.
///
/// On Windows, a running `.exe` cannot be overwritten in-place via `fs::rename`
/// (`ERROR_ACCESS_DENIED`), so this renames the active `dst` aside to `<stem>.exe.old.<pid>`
/// before moving `tmp -> dst` (rolling back on failure) and best-effort cleans stale backups.
pub fn replace_executable_safely(tmp: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if let Some(parent) = dst.parent()
            && let Some(stem) = dst.file_stem().and_then(|s| s.to_str())
        {
            let stale_prefix = format!("{stem}.exe.old.");
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if name.to_string_lossy().starts_with(&stale_prefix) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
        let backup = if dst.exists() {
            let b = dst.with_extension(format!("exe.old.{}", std::process::id()));
            let _ = std::fs::remove_file(&b);
            std::fs::rename(dst, &b)?;
            Some(b)
        } else {
            None
        };
        if let Err(err) = std::fs::rename(tmp, dst) {
            if let Some(ref b) = backup {
                let _ = std::fs::rename(b, dst);
            }
            return Err(err);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(tmp, dst)
    }
}

/// Isolate a child process in its own process group (Unix) or headless process group (Windows)
/// without using `unsafe`.
pub fn configure_isolated_process_group(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        cmd.creation_flags(WIN_CREATE_NEW_PROCESS_GROUP | WIN_CREATE_NO_WINDOW);
    }
}

/// Async, zero-`unsafe` process-tree termination (`kill -9 -<pid>` on Unix, `taskkill /PID <pid> /T /F` on Windows).
pub async fn terminate_process_tree_async(pid: u32) {
    if pid <= 1 {
        return;
    }
    #[cfg(unix)]
    {
        let target = format!("-{pid}");
        for bin in ["/bin/kill", "/usr/bin/kill", "kill"] {
            let status = tokio::process::Command::new(bin)
                .args(["-9", &target])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            if status.is_ok() {
                break;
            }
        }
    }
    #[cfg(windows)]
    {
        let pid_str = pid.to_string();
        let mut cmd = tokio::process::Command::new("taskkill");
        cmd.args(["/PID", &pid_str, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(WIN_CREATE_NO_WINDOW);
        let _ = cmd.status().await;
    }
}

/// Synchronous zero-`unsafe` process-tree termination for `Drop` guards.
pub fn terminate_process_tree_sync(pid: u32) {
    if pid <= 1 {
        return;
    }
    #[cfg(unix)]
    {
        let target = format!("-{pid}");
        for bin in ["/bin/kill", "/usr/bin/kill", "kill"] {
            let status = std::process::Command::new(bin)
                .args(["-9", &target])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if status.is_ok() {
                break;
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let pid_str = pid.to_string();
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid_str, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(WIN_CREATE_NO_WINDOW)
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_windows_verbatim_prefix_handles_drive_unc_and_unix_paths() {
        assert_eq!(
            strip_windows_verbatim_prefix_str(r"\\?\C:\Users\jess\orbit"),
            r"C:\Users\jess\orbit"
        );
        assert_eq!(strip_windows_verbatim_prefix_str(r"\\?\D:\"), r"D:\");
        assert_eq!(
            strip_windows_verbatim_prefix_str(r"\\?\UNC\fileserver\share\repo"),
            r"\\fileserver\share\repo"
        );
        assert_eq!(
            strip_windows_verbatim_prefix_str(r"\\?\unc\box\proj"),
            r"\\box\proj"
        );
        // Normal Windows and Unix paths remain untouched
        assert_eq!(
            strip_windows_verbatim_prefix_str(r"C:\Users\jess\orbit"),
            r"C:\Users\jess\orbit"
        );
        assert_eq!(
            strip_windows_verbatim_prefix_str("/Users/jess/Code/orbit"),
            "/Users/jess/Code/orbit"
        );
    }

    #[test]
    fn is_filesystem_root_path_rejects_unix_and_windows_drive_roots() {
        assert!(is_filesystem_root_path(Path::new("/")));
        assert!(is_filesystem_root_path(Path::new(r"\")));
        assert!(is_filesystem_root_path(Path::new(r"C:\")));
        assert!(is_filesystem_root_path(Path::new("C:/")));
        assert!(is_filesystem_root_path(Path::new("C:")));
        assert!(is_filesystem_root_path(Path::new(r"\\?\C:\")));
        assert!(is_filesystem_root_path(Path::new(r"\\server\share")));
        assert!(is_filesystem_root_path(Path::new(r"\\server\share\")));

        assert!(!is_filesystem_root_path(Path::new(r"C:\Users\jess\orbit")));
        assert!(!is_filesystem_root_path(Path::new(
            r"\\?\C:\Users\jess\orbit"
        )));
        assert!(!is_filesystem_root_path(Path::new(
            "/Users/jess/Code/orbit"
        )));
        assert!(!is_filesystem_root_path(Path::new(
            r"\\server\share\subdir"
        )));
    }

    #[test]
    fn windows_executable_candidates_respects_pathext_and_existing_extension() {
        let dir = Path::new(r"C:\Tools\bin");
        let cands = windows_executable_candidates(dir, "claude", Some(".EXE;.CMD;.BAT"));
        let names: Vec<String> = cands
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        assert_eq!(
            names,
            vec!["claude.exe", "claude.cmd", "claude.bat", "claude"]
        );

        let exact = windows_executable_candidates(dir, "node.exe", Some(".EXE;.CMD"));
        assert_eq!(exact.len(), 1);
        assert_eq!(
            exact[0]
                .file_name()
                .map(|n| n.to_string_lossy().to_string()),
            Some("node.exe".to_string())
        );

        let cmd_exact = windows_executable_candidates(dir, "codex.CMD", Some(".EXE;.CMD"));
        assert_eq!(cmd_exact.len(), 1);
    }

    #[test]
    fn shell_command_for_platform_returns_valid_shell_invocation() {
        let (prog, args) = shell_command_for_platform("echo hello", false);
        assert!(!prog.is_empty());
        assert_eq!(args.len(), 2);
        assert_eq!(args[1], "echo hello");
        #[cfg(unix)]
        {
            assert_eq!(prog, "/bin/sh");
            assert_eq!(args[0], "-c");
        }
        #[cfg(windows)]
        {
            assert_eq!(args[0], "/C");
        }
    }

    #[test]
    fn replace_executable_safely_replaces_existing_file() {
        let dir = std::env::temp_dir().join(format!(
            "orbit-platform-replace-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let dst = dir.join("orbit.bin");
        let tmp = dir.join("orbit.bin.tmp");
        std::fs::write(&dst, b"old-binary").expect("write old");
        std::fs::write(&tmp, b"new-binary").expect("write new");

        replace_executable_safely(&tmp, &dst).expect("replace executable");
        assert_eq!(std::fs::read(&dst).expect("read replaced"), b"new-binary");
        assert!(!tmp.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
