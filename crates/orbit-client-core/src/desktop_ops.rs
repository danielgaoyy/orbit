//! Desktop-specific client utilities shared by macOS and Linux (Tauri v2):
//! - Zero-config local Orbit Daemon discovery (`~/.orbit/host-keypair.json` -> `127.0.0.1:6868`)
//! - `~/.ssh/config` server auto-discovery (with `Include` expansion)
//! - One-click SSH E2EE Relay provisioning (`orbit relay up --allow-pubkey ...`)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshHostItem {
    pub alias: String,
    pub host_name: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub display_target: String,
}

/// Normalize user-entered Relay host/URL (`1.2.3.4`, `https://relay.example.com`, `ws://host:7878/`) into a clean `ws://` or `wss://` base URL.
pub fn normalize_relay_input(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let had_scheme = trimmed.contains("://");
    let with_scheme = if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        trimmed.to_string()
    } else {
        format!("ws://{trimmed}")
    };

    let (scheme, rest) = with_scheme.split_once("://")?;
    let mut authority = rest.split('/').next()?.trim().to_string();
    let is_ipv4 = authority.split('.').count().eq(&4)
        && authority.split('.').all(|part| part.parse::<u8>().is_ok());

    if !had_scheme && !authority.contains(':') && (authority == "localhost" || is_ipv4) {
        authority = format!("{authority}:7878");
    }
    Some(format!("{scheme}://{authority}"))
}

/// Resolve the real user home directory across macOS, Linux, and Windows.
pub fn resolve_user_home() -> Option<PathBuf> {
    let raw = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    if raw.is_empty() {
        return None;
    }
    if let Some((prefix, _)) = raw.split_once("/Library/Containers/")
        && !prefix.is_empty()
    {
        return Some(PathBuf::from(prefix));
    }
    Some(PathBuf::from(raw))
}

/// Automatically discover a local Orbit Daemon on Desktop (macOS / Linux / Windows) if `~/.orbit/host-keypair.json` exists.
pub fn discover_local_daemon_pairing_uri() -> Option<String> {
    if !cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )) {
        return None;
    }
    let home = resolve_user_home()?;
    let keypair_path = home.join(".orbit").join("host-keypair.json");
    if !keypair_path.is_file() {
        return None;
    }

    let candidates = [
        home.join(".local/bin/orbit"),
        home.join(".cargo/bin/orbit"),
        home.join(".cargo/bin/orbit.exe"),
        PathBuf::from("/usr/local/bin/orbit"),
        PathBuf::from("/opt/homebrew/bin/orbit"),
    ];
    if let Some(orbit_bin) = candidates.into_iter().find(|p| p.is_file())
        && let Ok(out) = Command::new(orbit_bin).arg("pair").output()
        && out.status.success()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if let Some(uri) = stdout
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("orbit://pair/"))
        {
            return Some(uri.to_owned());
        }
    }

    // Pure-Rust direct fallback when running from source without a globally installed `orbit` binary
    let raw = fs::read_to_string(&keypair_path).ok()?;
    let kp: orbit_crypto::HostIdentityKeypair = serde_json::from_str(&raw).ok()?;
    let host_id = kp.host_id().ok()?;
    let hostname = std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string());
    let offer = orbit_crypto::PairingOffer {
        host_id,
        hostname,
        daemon_pubkey_b64: kp.public_key_b64,
        direct_endpoints: vec!["ws://127.0.0.1:6868/ws".to_string()],
        relay_url: None,
        pairing_secret: "local".to_string(),
    };
    offer.to_qr_uri().ok()
}

/// Parse `~/.ssh/config` (and `Include` files up to depth 3) for concrete remote servers.
pub fn discover_user_ssh_hosts() -> Vec<SshHostItem> {
    let Some(home) = resolve_user_home() else {
        return Vec::new();
    };
    let ssh_dir = home.join(".ssh");
    let config_path = ssh_dir.join("config");
    if !config_path.is_file() {
        return Vec::new();
    }
    parse_ssh_file(&config_path, &ssh_dir, &home, 0)
}

pub fn parse_ssh_config_str(raw: &str) -> Vec<SshHostItem> {
    let dummy = PathBuf::from("/nonexistent");
    parse_ssh_lines(raw, &dummy, &dummy, 3)
}

fn parse_ssh_file(path: &Path, ssh_dir: &Path, home: &Path, depth: usize) -> Vec<SshHostItem> {
    if depth > 3 {
        return Vec::new();
    }
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_ssh_lines(&raw, ssh_dir, home, depth)
}

fn parse_ssh_lines(raw: &str, ssh_dir: &Path, home: &Path, depth: usize) -> Vec<SshHostItem> {
    let mut results: Vec<SshHostItem> = Vec::new();
    let mut current_aliases: Vec<String> = Vec::new();
    let mut cur_hostname: Option<String> = None;
    let mut cur_user: Option<String> = None;
    let mut cur_port: Option<u16> = None;

    let flush = |aliases: &mut Vec<String>,
                 hostname: &mut Option<String>,
                 user: &mut Option<String>,
                 port: &mut Option<u16>,
                 out: &mut Vec<SshHostItem>| {
        for alias in aliases.drain(..) {
            let effective_host = hostname.clone().unwrap_or_else(|| alias.clone());
            if is_ignored_ssh_host(&alias, &effective_host) {
                continue;
            }
            if !out.iter().any(|x| x.alias == alias) {
                let user_part = user
                    .as_deref()
                    .filter(|u| !u.is_empty())
                    .map(|u| format!("{u}@"))
                    .unwrap_or_default();
                let port_part = port
                    .filter(|p| *p != 22)
                    .map(|p| format!(":{p}"))
                    .unwrap_or_default();
                let display_target = format!("{user_part}{effective_host}{port_part}");
                out.push(SshHostItem {
                    alias,
                    host_name: effective_host,
                    user: user.clone(),
                    port: *port,
                    display_target,
                });
            }
        }
        *hostname = None;
        *user = None;
        *port = None;
    };

    for line in raw.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((k_raw, v_raw)) = split_directive(trimmed) else {
            continue;
        };
        let key = k_raw.to_ascii_lowercase();
        let val = v_raw.trim().trim_matches('"');

        match key.as_str() {
            "include" if depth < 3 => {
                flush(
                    &mut current_aliases,
                    &mut cur_hostname,
                    &mut cur_user,
                    &mut cur_port,
                    &mut results,
                );
                for pat in val.split_whitespace() {
                    for inc_path in expand_include(pat, ssh_dir, home) {
                        for sub in parse_ssh_file(&inc_path, ssh_dir, home, depth + 1) {
                            if !results.iter().any(|x| x.alias == sub.alias) {
                                results.push(sub);
                            }
                        }
                    }
                }
            }
            "host" => {
                flush(
                    &mut current_aliases,
                    &mut cur_hostname,
                    &mut cur_user,
                    &mut cur_port,
                    &mut results,
                );
                for tok in val.split_whitespace() {
                    let clean = tok.trim_matches('"');
                    if !clean.is_empty()
                        && !clean.contains('*')
                        && !clean.contains('?')
                        && !clean.starts_with('!')
                    {
                        current_aliases.push(clean.to_string());
                    }
                }
            }
            "match" => {
                flush(
                    &mut current_aliases,
                    &mut cur_hostname,
                    &mut cur_user,
                    &mut cur_port,
                    &mut results,
                );
            }
            "hostname" if !current_aliases.is_empty() && cur_hostname.is_none() => {
                cur_hostname = Some(val.to_string());
            }
            "user" if !current_aliases.is_empty() && cur_user.is_none() => {
                cur_user = Some(val.to_string());
            }
            "port" if !current_aliases.is_empty() && cur_port.is_none() => {
                cur_port = val.parse::<u16>().ok();
            }
            _ => {}
        }
    }

    flush(
        &mut current_aliases,
        &mut cur_hostname,
        &mut cur_user,
        &mut cur_port,
        &mut results,
    );
    results
}

fn strip_comment(line: &str) -> &str {
    let mut in_quotes = false;
    for (i, ch) in line.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == '#' && !in_quotes {
            return &line[..i];
        }
    }
    line
}

fn split_directive(line: &str) -> Option<(&str, &str)> {
    let ws = line.find(char::is_whitespace);
    let eq = line.find('=');
    match (ws, eq) {
        (Some(w), Some(e)) if e < w => Some((line[..e].trim(), line[e + 1..].trim())),
        (None, Some(e)) => Some((line[..e].trim(), line[e + 1..].trim())),
        (Some(w), _) => {
            let key = line[..w].trim();
            let rest = line[w..].trim_start();
            let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
            Some((key, rest))
        }
        (None, None) => None,
    }
}

fn is_ignored_ssh_host(alias: &str, hostname: &str) -> bool {
    let la = alias.to_ascii_lowercase();
    let lh = hostname.to_ascii_lowercase();
    if lh == "localhost" || lh == "127.0.0.1" || lh == "::1" || la.contains("orbstack") {
        return true;
    }
    for forge in [
        "github.com",
        "gist.github.com",
        "gitlab.com",
        "bitbucket.org",
        "ssh.dev.azure.com",
        "vs-ssh.visualstudio.com",
    ] {
        if lh == forge || lh.ends_with(&format!(".{forge}")) {
            return true;
        }
    }
    false
}

fn expand_include(pattern: &str, ssh_dir: &Path, home: &Path) -> Vec<PathBuf> {
    let resolved = if let Some(rest) = pattern.strip_prefix("~/") {
        home.join(rest)
    } else if Path::new(pattern).is_absolute() {
        PathBuf::from(pattern)
    } else {
        ssh_dir.join(pattern)
    };
    let s = resolved.to_string_lossy();
    if !s.contains('*') && !s.contains('?') {
        return if resolved.is_file() {
            vec![resolved]
        } else {
            Vec::new()
        };
    }
    let Some(parent) = resolved.parent() else {
        return Vec::new();
    };
    let Some(file_pat) = resolved.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|name| simple_glob(file_pat, name))
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

fn simple_glob(pat: &str, text: &str) -> bool {
    if pat == "*" {
        return !text.starts_with('.');
    }
    if let Some(sfx) = pat.strip_prefix('*') {
        return text.ends_with(sfx);
    }
    if let Some(pfx) = pat.strip_suffix('*') {
        return text.starts_with(pfx);
    }
    pat == text
}

/// Deploy/update Orbit on a remote SSH host and start `orbit relay up` locked to `allowed_pubkeys`.
pub fn provision_relay_over_ssh(
    alias: &str,
    fallback_hostname: &str,
    relay_port: u16,
    allowed_pubkeys: &[String],
) -> Result<String, String> {
    let mut unique_keys: Vec<String> = allowed_pubkeys
        .iter()
        .map(|k| k.replace('\'', "").trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    unique_keys.sort();
    unique_keys.dedup();

    let pubkey_flags = unique_keys
        .iter()
        .map(|k| format!("--allow-pubkey '{k}'"))
        .collect::<Vec<_>>()
        .join(" ");

    let remote_script = format!(
        "set -eu; \
         export PATH=\"$HOME/.local/bin:/usr/local/bin:$PATH\"; \
         if ! command -v orbit >/dev/null 2>&1 || ! orbit relay --help 2>&1 | grep -q 'allow'; then \
           curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh; \
         fi; \
         orbit relay up --port {relay_port} {pubkey_flags}"
    );

    let ssh_bin = if Path::new("/usr/bin/ssh").is_file() {
        "/usr/bin/ssh"
    } else {
        "ssh"
    };

    let out = Command::new(ssh_bin)
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=12",
            alias,
            &remote_script,
        ])
        .output()
        .map_err(|e| format!("failed to launch ssh: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let detail = stderr.trim();
        return Err(if detail.is_empty() {
            format!("SSH exited with status {}", out.status)
        } else {
            detail.to_string()
        });
    }

    for line in stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix("Relay URL")
            && let Some((_, url_part)) = rest.split_once(':')
        {
            let url = url_part.trim();
            if !url.is_empty() && !url.contains("<YOUR_SERVER_IP>") {
                return Ok(url.to_string());
            }
        }
    }

    Ok(format!("ws://{fallback_hostname}:{relay_port}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalEditorKind {
    Cursor,
    Zed,
    VsCode,
    Terminal,
    FileManager,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalEditorInfo {
    pub kind: ExternalEditorKind,
    pub label: String,
    pub installed: bool,
}

fn binary_exists_on_path(bin: &str) -> bool {
    let Some(path_os) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path_os) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return true;
        }
        #[cfg(target_os = "windows")]
        for ext in ["exe", "cmd", "bat"] {
            if dir.join(format!("{bin}.{ext}")).is_file() {
                return true;
            }
        }
    }
    false
}

fn macos_app_exists(app_bundle: &str) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    if Path::new("/Applications").join(app_bundle).exists() {
        return true;
    }
    resolve_user_home()
        .map(|h| h.join("Applications").join(app_bundle).exists())
        .unwrap_or(false)
}

/// Discover installed local editors and system shell/file-manager targets for one-click ADE handoff (`⌘O`).
pub fn detect_installed_editors() -> Vec<ExternalEditorInfo> {
    let cursor_installed = binary_exists_on_path("cursor") || macos_app_exists("Cursor.app");
    let zed_installed = binary_exists_on_path("zed") || macos_app_exists("Zed.app");
    let vscode_installed =
        binary_exists_on_path("code") || macos_app_exists("Visual Studio Code.app");

    let file_manager_label = if cfg!(target_os = "macos") {
        "Finder"
    } else if cfg!(target_os = "windows") {
        "Explorer"
    } else {
        "Files"
    };

    vec![
        ExternalEditorInfo {
            kind: ExternalEditorKind::Cursor,
            label: "Cursor".into(),
            installed: cursor_installed,
        },
        ExternalEditorInfo {
            kind: ExternalEditorKind::Zed,
            label: "Zed".into(),
            installed: zed_installed,
        },
        ExternalEditorInfo {
            kind: ExternalEditorKind::VsCode,
            label: "VS Code".into(),
            installed: vscode_installed,
        },
        ExternalEditorInfo {
            kind: ExternalEditorKind::Terminal,
            label: "System Terminal".into(),
            installed: true,
        },
        ExternalEditorInfo {
            kind: ExternalEditorKind::FileManager,
            label: file_manager_label.into(),
            installed: true,
        },
    ]
}

/// Build a remote SSH workspace URI (`vscode-remote://ssh-remote+...` or `ssh://...`) for remote Devbox handoff.
pub fn build_remote_editor_uri(
    editor: ExternalEditorKind,
    ssh_target: &str,
    cwd: &str,
) -> Option<String> {
    let clean_target = ssh_target.trim();
    if clean_target.is_empty() {
        return None;
    }
    let normalized_path = if cwd.starts_with('/') {
        cwd.to_string()
    } else if let Some(rest) = cwd.strip_prefix("~/") {
        format!("/{rest}")
    } else {
        format!("/{cwd}")
    };

    match editor {
        ExternalEditorKind::Cursor | ExternalEditorKind::VsCode => Some(format!(
            "vscode-remote://ssh-remote+{clean_target}{normalized_path}"
        )),
        ExternalEditorKind::Zed => Some(format!("ssh://{clean_target}{normalized_path}")),
        ExternalEditorKind::Terminal | ExternalEditorKind::FileManager => None,
    }
}

fn expand_local_tilde(cwd: &str) -> PathBuf {
    if cwd == "~"
        && let Some(home) = resolve_user_home()
    {
        return home;
    }
    if let Some(rest) = cwd.strip_prefix("~/")
        && let Some(home) = resolve_user_home()
    {
        return home.join(rest);
    }
    PathBuf::from(cwd)
}

/// Launch the requested external editor or OS tool at `cwd` (either locally or via SSH remote URI).
pub fn open_in_external_editor(
    editor: ExternalEditorKind,
    cwd: &str,
    remote_ssh_target: Option<&str>,
) -> Result<(), String> {
    let ssh_target = remote_ssh_target.map(str::trim).filter(|s| !s.is_empty());

    if let Some(remote) = ssh_target {
        match editor {
            ExternalEditorKind::Cursor => {
                let uri = build_remote_editor_uri(editor, remote, cwd)
                    .ok_or_else(|| "invalid remote SSH target".to_string())?;
                if binary_exists_on_path("cursor") {
                    Command::new("cursor")
                        .args(["--folder-uri", &uri])
                        .spawn()
                        .map_err(|e| format!("failed to launch cursor: {e}"))?;
                    return Ok(());
                }
                #[cfg(target_os = "macos")]
                {
                    Command::new("open")
                        .args(["-a", "Cursor", "--args", "--folder-uri", &uri])
                        .spawn()
                        .map_err(|e| format!("failed to open Cursor.app: {e}"))?;
                    return Ok(());
                }
                #[cfg(not(target_os = "macos"))]
                return Err("`cursor` CLI not found on PATH".into());
            }
            ExternalEditorKind::VsCode => {
                let uri = build_remote_editor_uri(editor, remote, cwd)
                    .ok_or_else(|| "invalid remote SSH target".to_string())?;
                if binary_exists_on_path("code") {
                    Command::new("code")
                        .args(["--folder-uri", &uri])
                        .spawn()
                        .map_err(|e| format!("failed to launch code: {e}"))?;
                    return Ok(());
                }
                #[cfg(target_os = "macos")]
                {
                    Command::new("open")
                        .args(["-a", "Visual Studio Code", "--args", "--folder-uri", &uri])
                        .spawn()
                        .map_err(|e| format!("failed to open Visual Studio Code.app: {e}"))?;
                    return Ok(());
                }
                #[cfg(not(target_os = "macos"))]
                return Err("`code` CLI not found on PATH".into());
            }
            ExternalEditorKind::Zed => {
                let uri = build_remote_editor_uri(editor, remote, cwd)
                    .ok_or_else(|| "invalid remote SSH target".to_string())?;
                if binary_exists_on_path("zed") {
                    Command::new("zed")
                        .arg(&uri)
                        .spawn()
                        .map_err(|e| format!("failed to launch zed: {e}"))?;
                    return Ok(());
                }
                #[cfg(target_os = "macos")]
                {
                    Command::new("open")
                        .args(["-a", "Zed", &uri])
                        .spawn()
                        .map_err(|e| format!("failed to open Zed.app: {e}"))?;
                    return Ok(());
                }
                #[cfg(not(target_os = "macos"))]
                return Err("`zed` CLI not found on PATH".into());
            }
            ExternalEditorKind::Terminal => {
                #[cfg(target_os = "macos")]
                {
                    let escaped_target = remote.replace('\'', "");
                    let script = format!(
                        "tell application \"Terminal\" to do script \"ssh {escaped_target}\""
                    );
                    Command::new("osascript")
                        .args([
                            "-e",
                            "tell application \"Terminal\" to activate",
                            "-e",
                            &script,
                        ])
                        .spawn()
                        .map_err(|e| format!("failed to open Terminal: {e}"))?;
                    return Ok(());
                }
                #[cfg(not(target_os = "macos"))]
                return Err(
                    "Use the built-in Orbit PTY drawer (`Ctrl+J`) for remote shell access".into(),
                );
            }
            ExternalEditorKind::FileManager => {
                return Err(
                    "Finder / File Manager handoff is only available for local hosts".into(),
                );
            }
        }
    }

    let local_path = expand_local_tilde(cwd);
    match editor {
        ExternalEditorKind::Cursor => {
            if binary_exists_on_path("cursor") {
                Command::new("cursor")
                    .arg(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to launch cursor: {e}"))?;
                return Ok(());
            }
            #[cfg(target_os = "macos")]
            {
                Command::new("open")
                    .args(["-a", "Cursor"])
                    .arg(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to open Cursor.app: {e}"))?;
                Ok(())
            }
            #[cfg(not(target_os = "macos"))]
            Err("Cursor is not installed or not on PATH".into())
        }
        ExternalEditorKind::Zed => {
            if binary_exists_on_path("zed") {
                Command::new("zed")
                    .arg(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to launch zed: {e}"))?;
                return Ok(());
            }
            #[cfg(target_os = "macos")]
            {
                Command::new("open")
                    .args(["-a", "Zed"])
                    .arg(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to open Zed.app: {e}"))?;
                Ok(())
            }
            #[cfg(not(target_os = "macos"))]
            Err("Zed is not installed or not on PATH".into())
        }
        ExternalEditorKind::VsCode => {
            if binary_exists_on_path("code") {
                Command::new("code")
                    .arg(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to launch code: {e}"))?;
                return Ok(());
            }
            #[cfg(target_os = "macos")]
            {
                Command::new("open")
                    .args(["-a", "Visual Studio Code"])
                    .arg(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to open Visual Studio Code.app: {e}"))?;
                Ok(())
            }
            #[cfg(not(target_os = "macos"))]
            Err("VS Code (`code`) is not installed or not on PATH".into())
        }
        ExternalEditorKind::Terminal => {
            #[cfg(target_os = "macos")]
            {
                for app in ["Ghostty", "iTerm", "Warp", "Terminal"] {
                    if macos_app_exists(&format!("{app}.app")) || app == "Terminal" {
                        Command::new("open")
                            .args(["-a", app])
                            .arg(&local_path)
                            .spawn()
                            .map_err(|e| format!("failed to open {app}: {e}"))?;
                        return Ok(());
                    }
                }
                Ok(())
            }
            #[cfg(target_os = "linux")]
            {
                Command::new("x-terminal-emulator")
                    .current_dir(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to launch terminal: {e}"))?;
                Ok(())
            }
            #[cfg(target_os = "windows")]
            {
                Command::new("cmd")
                    .args(["/C", "start", "cmd"])
                    .current_dir(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to launch terminal: {e}"))?;
                Ok(())
            }
        }
        ExternalEditorKind::FileManager => {
            #[cfg(target_os = "macos")]
            {
                Command::new("open")
                    .arg(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to open Finder: {e}"))?;
                Ok(())
            }
            #[cfg(target_os = "linux")]
            {
                Command::new("xdg-open")
                    .arg(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to open file manager: {e}"))?;
                Ok(())
            }
            #[cfg(target_os = "windows")]
            {
                Command::new("explorer")
                    .arg(&local_path)
                    .spawn()
                    .map_err(|e| format!("failed to open Explorer: {e}"))?;
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedAttachmentInfo {
    pub path: String,
    pub filename: String,
    pub size_bytes: usize,
}

/// Fast synchronous local repository file scan (`git ls-files` + shallow fallback) for instant `@` file mentions.
pub fn list_local_workspace_files(cwd: &str, limit: usize) -> Option<Vec<String>> {
    let dir = expand_local_tilde(cwd);
    if !dir.is_dir() {
        return None;
    }
    if let Ok(out) = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(&dir)
        .output()
        && out.status.success()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let list: Vec<String> = stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(limit)
            .map(ToOwned::to_owned)
            .collect();
        return Some(list);
    }

    let mut fallback = Vec::new();
    walk_local_shallow(&dir, &dir, 0, limit, &mut fallback);
    Some(fallback)
}

fn walk_local_shallow(root: &Path, dir: &Path, depth: usize, limit: usize, out: &mut Vec<String>) {
    if depth > 3 || out.len() >= limit {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
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
            walk_local_shallow(root, &path, depth + 1, limit, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Stage clipboard-pasted screenshots (`⌘V`) or dropped files into `~/.orbit/attachments/` for CLI agent consumption.
pub fn stage_local_attachment(
    raw_filename: &str,
    bytes: &[u8],
) -> Result<StagedAttachmentInfo, String> {
    if bytes.len() > 25 * 1024 * 1024 {
        return Err("Attachment exceeds 25 MiB limit".into());
    }
    let base_dir = resolve_user_home()
        .map(|h| h.join(".orbit").join("attachments"))
        .unwrap_or_else(|| std::env::temp_dir().join("orbit-attachments"));
    fs::create_dir_all(&base_dir)
        .map_err(|e| format!("failed to create attachments directory: {e}"))?;

    let safe_name: String = raw_filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean_name = if safe_name.trim_matches('_').is_empty() {
        "pasted-image.png".to_string()
    } else {
        safe_name
    };

    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dest = base_dir.join(format!("{ts_ms}_{clean_name}"));
    fs::write(&dest, bytes).map_err(|e| format!("failed to write staged attachment: {e}"))?;

    Ok(StagedAttachmentInfo {
        path: dest.to_string_lossy().to_string(),
        filename: clean_name,
        size_bytes: bytes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_input_normalization_covers_ips_and_schemes() {
        assert_eq!(
            normalize_relay_input(Some("203.0.113.10")),
            Some("ws://203.0.113.10:7878".into())
        );
        assert_eq!(
            normalize_relay_input(Some("https://relay.orbit.dev/")),
            Some("wss://relay.orbit.dev".into())
        );
    }

    #[test]
    fn ssh_config_parser_filters_forges() {
        let raw = "Host github.com\n  HostName github.com\nHost devbox\n  HostName 10.20.30.40\n  User ubuntu\n";
        let list = parse_ssh_config_str(raw);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].alias, "devbox");
        assert_eq!(list[0].display_target, "ubuntu@10.20.30.40");
    }

    #[test]
    fn remote_editor_uri_formats_vscode_and_zed() {
        assert_eq!(
            build_remote_editor_uri(ExternalEditorKind::Cursor, "devbox", "/home/ubuntu/orbit"),
            Some("vscode-remote://ssh-remote+devbox/home/ubuntu/orbit".into())
        );
        assert_eq!(
            build_remote_editor_uri(ExternalEditorKind::Zed, "devbox", "/home/ubuntu/orbit"),
            Some("ssh://devbox/home/ubuntu/orbit".into())
        );
        assert_eq!(
            build_remote_editor_uri(ExternalEditorKind::FileManager, "devbox", "/tmp"),
            None
        );
    }
}
