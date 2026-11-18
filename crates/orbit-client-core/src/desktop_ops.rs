//! Desktop-specific client utilities shared by macOS and Linux (Tauri v2):
//! - Zero-config local Orbit Daemon discovery (`~/.orbit/host-keypair.json` -> `127.0.0.1:6868`)
//! - `~/.ssh/config` server auto-discovery (with `Include` expansion)
//! - One-click SSH Host Bootstrap (`orbit daemon up` + `orbit pair`) & SSH Local Port-Forward Tunnel (`SshTunnelGuard`)
//! - One-click SSH E2EE Relay provisioning (`orbit relay up --allow-pubkey ...`)

use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshHostItem {
    pub alias: String,
    pub host_name: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub display_target: String,
}

/// RAII guard for a dedicated background `ssh -N -L` tunnel process.
///
/// Uses `ControlMaster=no` + `ControlPath=none` + `ServerAliveInterval=10` so user `~/.ssh/config`
/// multiplexing timeouts never drop Orbit's WebSocket stream, and kills/reaps the child cleanly on drop.
pub struct SshTunnelGuard {
    pub ws_url: String,
    child: Mutex<Child>,
}

impl SshTunnelGuard {
    pub fn has_exited(&self) -> bool {
        if let Ok(mut guard) = self.child.lock() {
            matches!(guard.try_wait(), Ok(Some(_)) | Err(_))
        } else {
            true
        }
    }
}

impl Drop for SshTunnelGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.child.lock() {
            let _ = guard.kill();
            let _ = guard.wait();
        }
    }
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

fn resolve_ssh_binary() -> &'static str {
    if Path::new("/usr/bin/ssh").is_file() {
        "/usr/bin/ssh"
    } else {
        "ssh"
    }
}

/// Automatically discover and ensure a local Orbit Daemon on Desktop (macOS / Linux / Windows) if `orbit` CLI is installed.
pub fn discover_local_daemon_pairing_uri() -> Option<String> {
    if !cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )) {
        return None;
    }
    let home = resolve_user_home()?;
    let candidates = [
        home.join(".local/bin/orbit"),
        home.join(".cargo/bin/orbit"),
        home.join(".cargo/bin/orbit.exe"),
        PathBuf::from("/usr/local/bin/orbit"),
        PathBuf::from("/opt/homebrew/bin/orbit"),
    ];
    let orbit_bin = candidates.into_iter().find(|p| p.is_file())?;

    if TcpStream::connect_timeout(&"127.0.0.1:6868".parse().ok()?, Duration::from_millis(250))
        .is_err()
    {
        let _ = Command::new(&orbit_bin).args(["daemon", "up"]).output();
    }

    let out = Command::new(orbit_bin).arg("pair").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("orbit://pair/"))
        .map(ToOwned::to_owned)
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

/// Zero-form 1-click SSH Host Connection:
/// Connects to `alias`, installs/upgrades `orbit` if missing or incompatible (`GLIBC_` / `<= 0.2.0`),
/// ensures `orbit daemon up` is running, and returns `orbit://pair/...`.
pub fn bootstrap_ssh_host_and_pair(alias: &str, remote_port: u16) -> Result<String, String> {
    let remote_script = format!(
        "set -eu; \
         export PATH=\"$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH\"; \
         NEED_INSTALL=0; \
         if ! command -v orbit >/dev/null 2>&1; then NEED_INSTALL=1; \
         elif orbit --version 2>&1 | grep -Eq 'GLIBC_|0\\.1\\.|0\\.2\\.0'; then NEED_INSTALL=1; fi; \
         if [ \"$NEED_INSTALL\" = \"1\" ]; then \
           curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh >/dev/null 2>&1; \
           orbit daemon restart --port {remote_port} >/dev/null 2>&1 || orbit daemon up --port {remote_port} >/dev/null 2>&1 || true; \
         else \
           orbit daemon up --port {remote_port} >/dev/null 2>&1 || true; \
         fi; \
         orbit pair --port {remote_port}"
    );

    let out = Command::new(resolve_ssh_binary())
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
        .map_err(|e| format!("无法启动 SSH 进程: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let detail = stderr.trim();
        return Err(if detail.is_empty() {
            format!("SSH 连接退出状态 {}", out.status)
        } else {
            detail.to_string()
        });
    }

    stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("orbit://pair/"))
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("未能在 {alias} 获取到有效配对码"))
}

/// One-click remote Orbit Daemon upgrade & restart over SSH.
pub fn upgrade_remote_daemon_over_ssh(alias: &str, remote_port: u16) -> Result<String, String> {
    let remote_script = format!(
        "set -eu; \
         export PATH=\"$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH\"; \
         curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh >/dev/null 2>&1; \
         orbit daemon restart --port {remote_port} >/dev/null 2>&1 || orbit daemon up --port {remote_port} >/dev/null 2>&1; \
         orbit --version"
    );

    let out = Command::new(resolve_ssh_binary())
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=15",
            alias,
            &remote_script,
        ])
        .output()
        .map_err(|e| format!("无法启动 SSH 升级: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(if stderr.is_empty() {
            format!("远端升级失败 ({})", out.status)
        } else {
            stderr
        });
    }
    Ok(if stdout.is_empty() {
        "orbit upgraded".into()
    } else {
        stdout
    })
}

/// Open a dedicated SSH local port-forward (`127.0.0.1:<free_port> -> 127.0.0.1:<remote_port>`)
/// without raw TCP probe resets or `ControlMaster` multiplexing drops.
pub fn spawn_ssh_local_tunnel(alias: &str, remote_port: u16) -> Result<SshTunnelGuard, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("本地端口分配失败: {e}"))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("读取本地端口失败: {e}"))?
        .port();
    drop(listener);

    let forward_spec = format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}");
    let mut child = Command::new(resolve_ssh_binary())
        .args([
            "-N",
            "-o",
            "ControlMaster=no",
            "-o",
            "ControlPath=none",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=10",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "TCPKeepAlive=yes",
            "-o",
            "ConnectTimeout=10",
            "-L",
            &forward_spec,
            alias,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法拉起 SSH 隧道进程: {e}"))?;

    std::thread::sleep(Duration::from_millis(220));
    if let Ok(Some(status)) = child.try_wait() {
        return Err(format!("SSH 隧道立即退出 ({alias}: {status})"));
    }

    Ok(SshTunnelGuard {
        ws_url: format!("ws://127.0.0.1:{local_port}/ws"),
        child: Mutex::new(child),
    })
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
         if ! command -v orbit >/dev/null 2>&1 || orbit --version 2>&1 | grep -q 'GLIBC_' || ! orbit relay --help 2>&1 | grep -q 'allow'; then \
           curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh; \
         fi; \
         orbit relay up --port {relay_port} {pubkey_flags}"
    );

    let out = Command::new(resolve_ssh_binary())
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
}
