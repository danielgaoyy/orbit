//! Desktop-specific client utilities shared by macOS and Linux (Tauri v2):
//! - Zero-config local Orbit Daemon discovery (`~/.orbit/host-keypair.json` -> `127.0.0.1:6868`)
//! - `~/.ssh/config` server auto-discovery (with `Include` expansion)
//! - One-click SSH Host Bootstrap (`orbit daemon up` + `orbit pair`) & SSH Local Port-Forward Tunnel (`SshTunnelGuard`)
//! - One-click SSH E2EE Relay provisioning (`orbit relay up --allow-pubkey ...`)

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use orbit_relay_transport::{
    GeneratedTunnelKey, SshRelaySpec, compute_openssh_pubkey_sha256_b64,
    derive_openssh_pubkey_from_seed, format_restricted_authorized_key_line,
    generate_ssh_tunnel_key, normalize_relay_url as transport_normalize_relay_url,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshHostItem {
    pub alias: String,
    pub host_name: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub display_target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalMachineInfo {
    pub hostname: String,
    pub host_id: Option<String>,
    pub os_arch: String,
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

    pub fn take_stderr_summary(&self) -> Option<String> {
        use std::io::Read;
        let mut guard = self.child.lock().ok()?;
        let mut stderr = guard.stderr.take()?;
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        let trimmed = buf.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
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

/// Normalize user-entered Relay host/URL (`1.2.3.4`, `https://relay.example.com`, `ws://host:7878/`, or `ssh+relay://...`) into a clean canonical Relay URL.
pub fn normalize_relay_input(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    transport_normalize_relay_url(trimmed).ok()
}

/// Resolve the real user home directory across macOS, Linux, Windows, and Xcode iOS Simulator (`SIMULATOR_HOST_HOME`).
pub fn resolve_user_home() -> Option<PathBuf> {
    if let Ok(sim_home) = std::env::var("SIMULATOR_HOST_HOME")
        && !sim_home.trim().is_empty()
    {
        return Some(PathBuf::from(sim_home));
    }
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

fn build_ssh_command() -> Command {
    let mut cmd = Command::new(resolve_ssh_binary());
    let mut path_parts = vec![
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/usr/bin".to_string(),
        "/bin".to_string(),
        "/usr/sbin".to_string(),
        "/sbin".to_string(),
    ];
    if let Some(home) = resolve_user_home() {
        path_parts.insert(0, home.join(".local/bin").to_string_lossy().to_string());
    }
    if let Ok(existing) = std::env::var("PATH") {
        for p in existing.split(':') {
            if !p.is_empty() && !path_parts.iter().any(|x| x == p) {
                path_parts.push(p.to_string());
            }
        }
    }
    cmd.env("PATH", path_parts.join(":"));
    cmd
}

/// Extract the WebSocket TCP port from direct endpoints (e.g. `ws://172.31.33.74:6869/ws` -> `Some(6869)`).
pub fn extract_ws_port(endpoints: &[String]) -> Option<u16> {
    for ep in endpoints {
        let trimmed = ep.trim();
        let Some(rest) = trimmed
            .strip_prefix("ws://")
            .or_else(|| trimmed.strip_prefix("wss://"))
        else {
            continue;
        };
        let Some(authority) = rest.split('/').next() else {
            continue;
        };
        if let Some((_, port_str)) = authority.rsplit_once(':')
            && let Ok(port) = port_str.parse::<u16>()
            && port > 0
        {
            return Some(port);
        }
    }
    None
}

/// Resolve the local Orbit state directory (`$ORBIT_HOME`, falling back to `<user_home>/.orbit`).
pub fn resolve_orbit_home_dir() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("ORBIT_HOME") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    Some(resolve_user_home()?.join(".orbit"))
}

fn non_empty_trimmed(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(unix)]
fn run_hostname_cmd(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout.lines().find_map(non_empty_trimmed)
}

/// Detect the real OS hostname without ever falling back to `"orbit-host"` on macOS.
pub fn detect_system_hostname() -> String {
    if let Ok(env_host) = std::env::var("HOSTNAME")
        && let Some(clean) = non_empty_trimmed(&env_host)
    {
        return clean;
    }
    if let Ok(file_host) = fs::read_to_string("/etc/hostname")
        && let Some(clean) = non_empty_trimmed(&file_host)
    {
        return clean;
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(clean) = run_hostname_cmd("/usr/sbin/scutil", &["--get", "LocalHostName"])
            .or_else(|| run_hostname_cmd("/usr/sbin/scutil", &["--get", "ComputerName"]))
        {
            return clean;
        }
    }
    #[cfg(unix)]
    {
        if let Some(clean) = run_hostname_cmd("/bin/hostname", &["-s"])
            .or_else(|| run_hostname_cmd("hostname", &["-s"]))
            .or_else(|| run_hostname_cmd("hostname", &[]))
        {
            return clean;
        }
    }
    #[cfg(windows)]
    {
        if let Ok(win_host) = std::env::var("COMPUTERNAME")
            && let Some(clean) = non_empty_trimmed(&win_host)
        {
            return clean;
        }
    }
    "localhost".to_string()
}

/// Pure read-only lookup of the local Orbit daemon identity (`(host_id, public_key_b64)`), if a
/// keypair already exists on disk. Never spawns `orbit daemon up` or creates files.
pub fn read_local_daemon_identity() -> Option<(String, String)> {
    let is_desktop = cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    ));
    let is_ios_simulator =
        cfg!(target_os = "ios") && std::env::var_os("SIMULATOR_HOST_HOME").is_some();
    if !is_desktop && !is_ios_simulator {
        return None;
    }
    let orbit_dir = resolve_orbit_home_dir()?;
    let keypair_path = orbit_dir.join("host-keypair.json");
    if !keypair_path.is_file() {
        return None;
    }
    let raw = fs::read_to_string(&keypair_path).ok()?;
    let kp: orbit_crypto::HostIdentityKeypair = serde_json::from_str(&raw).ok()?;
    let host_id = kp.host_id().ok()?;
    Some((host_id, kp.public_key_b64))
}

/// Pure read-only metadata about this local machine for explicit "Connect This Machine" UIs.
pub fn inspect_local_machine_info() -> LocalMachineInfo {
    let host_id = read_local_daemon_identity().map(|(id, _)| id);
    LocalMachineInfo {
        hostname: detect_system_hostname(),
        host_id,
        os_arch: format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
    }
}

/// Read the local daemon's persisted pairing secret from `<orbit_dir>/pairing-secret`.
pub fn read_local_daemon_pairing_secret_in(orbit_dir: &Path) -> Option<String> {
    let secret_path = orbit_dir.join("pairing-secret");
    if !secret_path.is_file() {
        return None;
    }
    let raw = fs::read_to_string(&secret_path).ok()?;
    let trimmed = raw.trim();
    if trimmed.len() >= 32
        && !trimmed
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Read the local daemon's persisted pairing secret from `$ORBIT_HOME/pairing-secret`.
pub fn read_local_daemon_pairing_secret() -> Option<String> {
    let orbit_dir = resolve_orbit_home_dir()?;
    read_local_daemon_pairing_secret_in(&orbit_dir)
}

/// Read and normalize the local daemon's persisted `relay_url` from `<orbit_dir>/config.json`.
pub fn read_local_daemon_relay_url_in(orbit_dir: &Path) -> Option<String> {
    let raw = fs::read_to_string(orbit_dir.join("config.json")).ok()?;
    let val: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let relay = val.get("relay_url")?.as_str()?;
    normalize_relay_input(Some(relay))
}

/// Read and normalize the local daemon's persisted `relay_url` on Desktop (or Xcode iOS Simulator).
pub fn read_local_daemon_relay_url() -> Option<String> {
    let is_desktop = cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    ));
    let is_ios_simulator =
        cfg!(target_os = "ios") && std::env::var_os("SIMULATOR_HOST_HOME").is_some();
    if !is_desktop && !is_ios_simulator {
        return None;
    }
    let orbit_dir = resolve_orbit_home_dir()?;
    read_local_daemon_relay_url_in(&orbit_dir)
}

fn read_local_daemon_port(orbit_dir: &Path) -> u16 {
    if let Ok(raw) = fs::read_to_string(orbit_dir.join("daemon.addr"))
        && let Ok(addr) = raw.trim().parse::<std::net::SocketAddr>()
        && addr.port() > 0
    {
        return addr.port();
    }
    if let Ok(raw) = fs::read_to_string(orbit_dir.join("daemon-port"))
        && let Ok(port) = raw.trim().parse::<u16>()
        && port > 0
    {
        return port;
    }
    fs::read_to_string(orbit_dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("listen_addr")?.as_str().map(ToOwned::to_owned))
        .and_then(|addr| addr.parse::<std::net::SocketAddr>().ok())
        .map(|a| a.port())
        .filter(|&p| p > 0)
        .unwrap_or(6868)
}

/// Explicitly ensure and obtain the local Orbit Daemon pairing URI on Desktop (or Xcode iOS Simulator).
pub fn discover_local_daemon_pairing_uri() -> Option<String> {
    let is_desktop = cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    ));
    let is_ios_simulator =
        cfg!(target_os = "ios") && std::env::var_os("SIMULATOR_HOST_HOME").is_some();
    if !is_desktop && !is_ios_simulator {
        return None;
    }
    let home = resolve_user_home()?;
    let mut candidates = vec![
        home.join(".local/bin/orbit"),
        home.join(".cargo/bin/orbit"),
        home.join(".cargo/bin/orbit.exe"),
        PathBuf::from("/usr/local/bin/orbit"),
        PathBuf::from("/opt/homebrew/bin/orbit"),
    ];
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sidecar_name = if cfg!(windows) { "orbit.exe" } else { "orbit" };
        let sidecar = dir.join(sidecar_name);
        if sidecar != exe {
            candidates.push(sidecar);
        }
    }
    if is_desktop && let Some(orbit_bin) = candidates.into_iter().find(|p| p.is_file()) {
        let status_out = Command::new(&orbit_bin)
            .args(["daemon", "status"])
            .output()
            .ok();
        let already_running = status_out
            .as_ref()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("RUNNING"))
            .unwrap_or(false);
        if !already_running {
            let _ = Command::new(&orbit_bin).args(["daemon", "up"]).output();
        }
        if let Ok(out) = Command::new(orbit_bin).arg("pair").output()
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
    }

    // Pure-Rust direct fallback when running from source without a globally installed `orbit` binary
    let orbit_dir = resolve_orbit_home_dir()?;
    let keypair_path = orbit_dir.join("host-keypair.json");
    if !keypair_path.is_file() {
        return None;
    }
    let pairing_secret = read_local_daemon_pairing_secret()?;
    let raw = fs::read_to_string(&keypair_path).ok()?;
    let kp: orbit_crypto::HostIdentityKeypair = serde_json::from_str(&raw).ok()?;
    let host_id = kp.host_id().ok()?;
    let hostname = detect_system_hostname();
    let port = read_local_daemon_port(&orbit_dir);
    let relay_url = read_local_daemon_relay_url_in(&orbit_dir);
    let offer = orbit_crypto::PairingOffer {
        host_id,
        hostname,
        daemon_pubkey_b64: kp.public_key_b64,
        direct_endpoints: vec![format!("ws://127.0.0.1:{port}/ws")],
        relay_url,
        pairing_secret,
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

/// Zero-form 1-click SSH Host Connection with Native Multi-User Isolation:
/// Connects to `alias`, installs/upgrades `orbit` if missing or `< 0.2.3`,
/// ensures this Unix user's own isolated `orbit daemon` is running on a conflict-free port,
/// and returns `orbit://pair/...` encoding that user's exact `host_id` & port.
pub fn bootstrap_ssh_host_and_pair(
    alias: &str,
    explicit_port: Option<u16>,
) -> Result<String, String> {
    let repo = orbit_protocol::configured_github_repo();
    let port_flag = explicit_port
        .map(|p| format!(" --port {p}"))
        .unwrap_or_default();
    let remote_script = format!(
        "set -eu; \
         export PATH=\"$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH\"; \
         NEED_INSTALL=0; \
         if ! command -v orbit >/dev/null 2>&1; then NEED_INSTALL=1; \
         elif orbit --version 2>&1 | grep -Eq 'GLIBC_|0\\.1\\.|0\\.2\\.[012]'; then NEED_INSTALL=1; fi; \
         if [ \"$NEED_INSTALL\" = \"1\" ]; then \
           curl -fsSL https://raw.githubusercontent.com/{repo}/main/deploy/install.sh | ORBIT_GITHUB_REPO='{repo}' sh >/dev/null 2>&1; \
           orbit daemon up{port_flag} >/dev/null 2>&1 || true; \
         elif ! orbit daemon status 2>/dev/null | grep -q 'RUNNING'; then \
           orbit daemon up{port_flag} >/dev/null 2>&1 || true; \
         fi; \
         orbit pair"
    );

    let out = build_ssh_command()
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
        .map_err(|e| format!("Failed to start SSH process: {e}"))?;

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

    stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("orbit://pair/"))
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("No valid pairing URI returned from {alias}"))
}

/// One-click remote Orbit Daemon upgrade & restart over SSH (preserving per-user port isolation).
pub fn upgrade_remote_daemon_over_ssh(
    alias: &str,
    explicit_port: Option<u16>,
) -> Result<(String, Option<String>), String> {
    let repo = orbit_protocol::configured_github_repo();
    let port_flag = explicit_port
        .map(|p| format!(" --port {p}"))
        .unwrap_or_default();
    let remote_script = format!(
        "set -eu; \
         export PATH=\"$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH\"; \
         curl -fsSL https://raw.githubusercontent.com/{repo}/main/deploy/install.sh | ORBIT_GITHUB_REPO='{repo}' sh >/dev/null 2>&1; \
         orbit daemon up{port_flag} >/dev/null 2>&1; \
         if [ -f \"$HOME/.orbit/relay.json\" ] && orbit relay status 2>/dev/null | grep -q 'RUNNING'; then orbit relay up >/dev/null 2>&1 || true; fi; \
         orbit --version; \
         orbit pair"
    );

    let out = build_ssh_command()
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
        .map_err(|e| format!("Failed to launch SSH upgrade: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(if stderr.is_empty() {
            format!("Remote upgrade failed ({})", out.status)
        } else {
            stderr
        });
    }
    let version = stdout
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("orbit "))
        .unwrap_or("orbit upgraded")
        .to_string();
    let pair_uri = stdout
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("orbit://pair/"))
        .map(ToOwned::to_owned);
    Ok((version, pair_uri))
}

/// Open a dedicated SSH local port-forward (`127.0.0.1:<free_port> -> 127.0.0.1:<remote_port>`)
/// without raw TCP probe resets or `ControlMaster` multiplexing drops.
pub fn spawn_ssh_local_tunnel(alias: &str, remote_port: u16) -> Result<SshTunnelGuard, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Failed to bind local port: {e}"))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("Failed to inspect local port: {e}"))?
        .port();
    drop(listener);

    let forward_spec = format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}");
    let mut child = build_ssh_command()
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
        .map_err(|e| format!("Failed to spawn SSH tunnel: {e}"))?;

    std::thread::sleep(Duration::from_millis(220));
    if let Ok(Some(status)) = child.try_wait() {
        return Err(format!("SSH tunnel exited immediately ({alias}: {status})"));
    }

    Ok(SshTunnelGuard {
        ws_url: format!("ws://127.0.0.1:{local_port}/ws"),
        child: Mutex::new(child),
    })
}

#[derive(Serialize, Deserialize)]
struct StoredTunnelKey {
    seed_b64: String,
}

/// Load or create a persistent Ed25519 tunnel identity keypair in `<home_dir>/ssh-relay-tunnel-key.json` (`0600`).
pub fn load_or_create_ssh_tunnel_key(home_dir: &Path) -> Result<GeneratedTunnelKey, String> {
    let path = home_dir.join("ssh-relay-tunnel-key.json");
    if let Ok(raw) = fs::read_to_string(&path)
        && let Ok(stored) = serde_json::from_str::<StoredTunnelKey>(&raw)
        && let Ok(openssh_pubkey) = derive_openssh_pubkey_from_seed(&stored.seed_b64)
    {
        return Ok(GeneratedTunnelKey {
            seed_b64: stored.seed_b64,
            openssh_pubkey,
        });
    }

    let generated = generate_ssh_tunnel_key().map_err(|e| e.to_string())?;
    fs::create_dir_all(home_dir)
        .map_err(|e| format!("failed to create {}: {e}", home_dir.display()))?;
    let payload = serde_json::to_string_pretty(&StoredTunnelKey {
        seed_b64: generated.seed_b64.clone(),
    })
    .map_err(|e| format!("failed to serialize tunnel key: {e}"))?;
    fs::write(&path, payload).map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(generated)
}

/// Resolve effective `(hostname, user, port)` for an SSH alias via `ssh -G`, falling back to explicit metadata.
pub fn resolve_ssh_effective_target(
    alias: &str,
    fallback_host: &str,
    fallback_user: Option<&str>,
    fallback_port: Option<u16>,
) -> (String, String, u16) {
    let mut hostname: Option<String> = None;
    let mut user: Option<String> = None;
    let mut port: Option<u16> = None;

    if let Ok(out) = build_ssh_command().args(["-G", alias]).output()
        && out.status.success()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let trimmed = line.trim();
            if let Some((k, v)) = trimmed.split_once(char::is_whitespace) {
                let val = v.trim();
                if val.is_empty() {
                    continue;
                }
                match k {
                    "hostname" if hostname.is_none() => hostname = Some(val.to_string()),
                    "user" if user.is_none() => user = Some(val.to_string()),
                    "port" if port.is_none() => {
                        port = val.parse::<u16>().ok().filter(|p| *p > 0);
                    }
                    _ => {}
                }
            }
        }
    }

    let resolved_host = hostname.unwrap_or_else(|| fallback_host.to_string());
    let resolved_user = user
        .or_else(|| fallback_user.map(ToOwned::to_owned))
        .or_else(|| std::env::var("USER").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "root".to_string());
    let resolved_port = port.or(fallback_port).filter(|p| *p > 0).unwrap_or(22);

    (resolved_host, resolved_user, resolved_port)
}

/// Load or create a persistent 256-bit shared fleet admission token at `<home_dir>/fleet-relay-token` (`0600`).
pub fn load_or_create_fleet_relay_token(home_dir: &Path) -> Result<String, String> {
    let path = home_dir.join("fleet-relay-token");
    if let Ok(raw) = fs::read_to_string(&path) {
        let tok = raw.trim().to_string();
        if tok.len() >= 16 && !tok.chars().any(char::is_whitespace) {
            return Ok(tok);
        }
    }
    let token = orbit_crypto::generate_pairing_secret();
    fs::create_dir_all(home_dir)
        .map_err(|e| format!("failed to create {}: {e}", home_dir.display()))?;
    fs::write(&path, format!("{token}\n"))
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(token)
}

/// Deploy/update Orbit on a remote SSH host bound strictly to `127.0.0.1:<relay_port>`,
/// install a restricted `authorized_keys` tunnel key, and return the canonical `ssh+relay://...` URL.
/// Credentials (`fleet_token`) are piped exclusively over SSH stdin, never passed in process `argv`.
#[allow(clippy::too_many_arguments)]
pub fn provision_ssh_tunnel_relay_with_home(
    home_dir: &Path,
    alias: &str,
    fallback_hostname: &str,
    fallback_user: Option<&str>,
    fallback_port: Option<u16>,
    relay_port: u16,
    allowed_pubkeys: &[String],
    tag_owner: &str,
) -> Result<String, String> {
    use std::io::Write as _;

    let tunnel_key = load_or_create_ssh_tunnel_key(home_dir)?;
    let fleet_token = load_or_create_fleet_relay_token(home_dir)?;

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

    let clean_owner = tag_owner.replace(['\'', ' '], "");
    let tag = format!("orbit-relay-tunnel:{clean_owner}:{relay_port}");
    let restricted_line =
        format_restricted_authorized_key_line(&tunnel_key.openssh_pubkey, relay_port, &tag)
            .replace('\'', "");

    let repo = orbit_protocol::configured_github_repo();
    let remote_script = format!(
        "set -eu\n\
         export PATH=\"$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH\"\n\
         if ! command -v orbit >/dev/null 2>&1 || orbit --version 2>&1 | grep -q 'GLIBC_' || ! orbit relay --help 2>&1 | grep -q 'allow'; then\n\
           curl -fsSL https://raw.githubusercontent.com/{repo}/main/deploy/install.sh | ORBIT_GITHUB_REPO='{repo}' sh\n\
         fi\n\
         if orbit relay up --help 2>&1 | grep -q 'fleet-token-stdin'; then\n\
           orbit relay up --listen 127.0.0.1:{relay_port} {pubkey_flags} --fleet-token-stdin <<'ORBIT_FT_EOF'\n\
{fleet_token}\n\
ORBIT_FT_EOF\n\
           printf 'ORBIT_FLEET_TOKEN_SUPPORTED=1\\n'\n\
         else\n\
           orbit relay up --listen 127.0.0.1:{relay_port} {pubkey_flags}\n\
           printf 'ORBIT_FLEET_TOKEN_SUPPORTED=0\\n'\n\
         fi\n\
         mkdir -p \"$HOME/.ssh\" && chmod 700 \"$HOME/.ssh\"\n\
         touch \"$HOME/.ssh/authorized_keys\" && chmod 600 \"$HOME/.ssh/authorized_keys\"\n\
         (grep -Fv ' {tag}' \"$HOME/.ssh/authorized_keys\" || true) > \"$HOME/.ssh/authorized_keys.orbit.tmp\"\n\
         printf '%s\\n' '{restricted_line}' >> \"$HOME/.ssh/authorized_keys.orbit.tmp\"\n\
         mv \"$HOME/.ssh/authorized_keys.orbit.tmp\" \"$HOME/.ssh/authorized_keys\" && chmod 600 \"$HOME/.ssh/authorized_keys\"\n\
         printf 'ORBIT_SSH_USER=%s\\n' \"$(whoami)\"\n\
         find /etc/ssh -maxdepth 1 -name 'ssh_host_*_key.pub' -type f 2>/dev/null | while IFS= read -r pubf; do\n\
           if [ -r \"$pubf\" ]; then\n\
             line=\"$(cat \"$pubf\" 2>/dev/null || true)\"\n\
             if [ -n \"$line\" ]; then printf 'ORBIT_HOST_PUBKEY=%s\\n' \"$line\"; fi\n\
           fi\n\
         done || true\n"
    );

    let mut child = build_ssh_command()
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=12",
            "--",
            alias,
            "sh -s",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to launch ssh: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(remote_script.as_bytes());
        let _ = stdin.flush();
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("failed waiting for ssh: {e}"))?;

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

    let mut remote_user: Option<String> = None;
    let mut host_key_fps_b64: Vec<String> = Vec::new();
    let mut fleet_token_supported = false;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(u) = trimmed.strip_prefix("ORBIT_SSH_USER=") {
            let clean = u.trim();
            if !clean.is_empty() {
                remote_user = Some(clean.to_string());
            }
        } else if trimmed == "ORBIT_FLEET_TOKEN_SUPPORTED=1" {
            fleet_token_supported = true;
        } else if let Some(pub_line) = trimmed.strip_prefix("ORBIT_HOST_PUBKEY=")
            && let Ok(fp) = compute_openssh_pubkey_sha256_b64(pub_line.trim())
            && !host_key_fps_b64.contains(&fp)
        {
            host_key_fps_b64.push(fp);
        }
    }

    let (ssh_host, resolved_user, ssh_port) =
        resolve_ssh_effective_target(alias, fallback_hostname, fallback_user, fallback_port);
    let spec = SshRelaySpec {
        user: remote_user.unwrap_or(resolved_user),
        ssh_host,
        ssh_port,
        relay_port,
        key_seed_b64: tunnel_key.seed_b64,
        host_key_fps_b64,
        fleet_token: if fleet_token_supported {
            Some(fleet_token)
        } else {
            None
        },
    };
    Ok(spec.to_uri())
}

/// Deploy/update Orbit on a remote SSH host and start a zero-port SSH-tunneled E2EE Relay locked to `allowed_pubkeys`.
pub fn provision_relay_over_ssh(
    alias: &str,
    fallback_hostname: &str,
    relay_port: u16,
    allowed_pubkeys: &[String],
) -> Result<String, String> {
    let home_dir = resolve_user_home()
        .ok_or_else(|| "cannot resolve user home directory for tunnel key storage".to_string())?
        .join(".orbit");
    let tag_owner = allowed_pubkeys
        .first()
        .map(|k| k.chars().take(12).collect::<String>())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "desktop".to_string());
    provision_ssh_tunnel_relay_with_home(
        &home_dir,
        alias,
        fallback_hostname,
        None,
        None,
        relay_port,
        allowed_pubkeys,
        &tag_owner,
    )
}

/// Extract the provisioned Cloudflare Tunnel `wss://...?ft=...` URL from remote setup stdout.
pub fn parse_cloudflare_tunnel_stdout(stdout: &str, fleet_token: &str) -> Result<String, String> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(raw_url) = trimmed.strip_prefix("ORBIT_CF_URL=") {
            let clean = raw_url.trim().trim_end_matches('/');
            if clean.is_empty() {
                continue;
            }
            let base = if let Some(host) = clean.strip_prefix("https://") {
                format!("wss://{host}")
            } else if let Some(host) = clean.strip_prefix("http://") {
                format!("wss://{host}")
            } else if clean.starts_with("wss://") || clean.starts_with("ws://") {
                clean.to_string()
            } else {
                format!("wss://{clean}")
            };
            let clean_tok = fleet_token.trim();
            if clean_tok.is_empty() {
                return Ok(base);
            }
            let sep = if base.contains('?') { '&' } else { '?' };
            return Ok(format!("{base}{sep}ft={clean_tok}"));
        }
    }
    Err("Cloudflare Tunnel provisioning did not emit a public URL (ORBIT_CF_URL)".to_string())
}

/// Deploy/update Orbit Relay bound strictly to `127.0.0.1:<relay_port>` on an intranet /
/// firewall-restricted SSH host and expose it with ZERO inbound open ports via Cloudflare Tunnel.
/// Secrets (`fleet_token` and optional Named Tunnel `tunnel_token`) are delivered strictly over SSH stdin / `0600` files, never process `argv`.
#[allow(clippy::too_many_arguments)]
pub fn provision_cloudflare_tunnel_relay_with_home(
    home_dir: &Path,
    alias: &str,
    relay_port: u16,
    allowed_pubkeys: &[String],
    public_hostname: Option<&str>,
    tunnel_token: Option<&str>,
) -> Result<String, String> {
    use std::io::Write as _;

    let fleet_token = load_or_create_fleet_relay_token(home_dir)?;

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

    let clean_hostname = public_hostname
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_start_matches("wss://")
                .trim_start_matches("ws://")
                .trim_end_matches('/')
                .replace('\'', "")
        });
    let clean_tunnel_token = tunnel_token
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.replace(['\r', '\n', '\''], ""));

    if clean_hostname.is_some() ^ clean_tunnel_token.is_some() {
        return Err(
            "Named Cloudflare Tunnel requires both Public Hostname and Cloudflare Tunnel Token (or leave both blank for an instant Quick Tunnel)."
                .to_string(),
        );
    }

    let tunnel_env_setup = if let (Some(host), Some(tok)) = (&clean_hostname, &clean_tunnel_token) {
        format!(
            "mkdir -p \"$HOME/.orbit\" && chmod 700 \"$HOME/.orbit\"\n\
             umask 077\n\
             cat > \"$HOME/.orbit/cloudflared-tunnel.env\" <<'ORBIT_CF_TOK_EOF'\n\
             TUNNEL_TOKEN={tok}\n\
             ORBIT_CF_TOK_EOF\n\
             chmod 600 \"$HOME/.orbit/cloudflared-tunnel.env\"\n\
             ORBIT_TUNNEL_EXEC=\"$CF_BIN tunnel --no-autoupdate --logfile $HOME/.orbit/cloudflared-tunnel.log run\"\n\
             ORBIT_ENV_LINE=\"EnvironmentFile=$HOME/.orbit/cloudflared-tunnel.env\"\n\
             ORBIT_NAMED_HOST='{host}'\n"
        )
    } else {
        format!(
            "mkdir -p \"$HOME/.orbit\" && chmod 700 \"$HOME/.orbit\"\n\
             rm -f \"$HOME/.orbit/cloudflared-tunnel.env\"\n\
             ORBIT_TUNNEL_EXEC=\"$CF_BIN tunnel --no-autoupdate --url http://127.0.0.1:{relay_port} --logfile $HOME/.orbit/cloudflared-tunnel.log\"\n\
             ORBIT_ENV_LINE=\"\"\n\
             ORBIT_NAMED_HOST=\"\"\n"
        )
    };

    let repo = orbit_protocol::configured_github_repo();
    let remote_script = format!(
        "set -eu\n\
         export PATH=\"$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH\"\n\
         if ! command -v orbit >/dev/null 2>&1 || orbit --version 2>&1 | grep -q 'GLIBC_' || ! orbit relay up --help 2>&1 | grep -q 'fleet-token-stdin'; then\n\
           curl -fsSL https://raw.githubusercontent.com/{repo}/main/deploy/install.sh | ORBIT_GITHUB_REPO='{repo}' sh\n\
         fi\n\
         orbit relay up --listen 127.0.0.1:{relay_port} {pubkey_flags} --fleet-token-stdin <<'ORBIT_FT_EOF'\n\
{fleet_token}\n\
ORBIT_FT_EOF\n\
         mkdir -p \"$HOME/.local/bin\"\n\
         CF_BIN=\"$(command -v cloudflared || true)\"\n\
         if [ -z \"$CF_BIN\" ]; then\n\
           OS=\"$(uname -s | tr '[:upper:]' '[:lower:]')\"\n\
           ARCH=\"$(uname -m)\"\n\
           case \"$ARCH\" in\n\
             x86_64|amd64) CF_ARCH=amd64 ;;\n\
             aarch64|arm64) CF_ARCH=arm64 ;;\n\
             *) echo \"Unsupported CPU arch for cloudflared: $ARCH\" >&2; exit 1 ;;\n\
           esac\n\
           if [ \"$OS\" = \"linux\" ]; then\n\
             curl -fsSL \"https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-$CF_ARCH\" -o \"$HOME/.local/bin/cloudflared\"\n\
             chmod 755 \"$HOME/.local/bin/cloudflared\"\n\
           elif [ \"$OS\" = \"darwin\" ]; then\n\
             curl -fsSL \"https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-$CF_ARCH.tgz\" -o \"/tmp/cloudflared.$$.tgz\"\n\
             tar -xzf \"/tmp/cloudflared.$$.tgz\" -C \"$HOME/.local/bin\"\n\
             rm -f \"/tmp/cloudflared.$$.tgz\"\n\
             chmod 755 \"$HOME/.local/bin/cloudflared\"\n\
           else\n\
             echo \"Unsupported OS for auto-installing cloudflared: $OS\" >&2; exit 1\n\
           fi\n\
           CF_BIN=\"$HOME/.local/bin/cloudflared\"\n\
         fi\n\
         : > \"$HOME/.orbit/cloudflared-tunnel.log\"\n\
         {tunnel_env_setup}\n\
         STARTED_SYSTEMD=0\n\
         if command -v systemctl >/dev/null 2>&1 && systemctl --user status >/dev/null 2>&1; then\n\
           mkdir -p \"$HOME/.config/systemd/user\"\n\
           cat > \"$HOME/.config/systemd/user/orbit-relay-tunnel.service\" <<ORBIT_SVC_EOF\n\
[Unit]\n\
Description=Orbit Zero-Port Cloudflare Tunnel Relay\n\
After=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
$ORBIT_ENV_LINE\n\
ExecStart=$ORBIT_TUNNEL_EXEC\n\
Restart=always\n\
RestartSec=3\n\
StandardOutput=append:$HOME/.orbit/cloudflared-tunnel.log\n\
StandardError=append:$HOME/.orbit/cloudflared-tunnel.log\n\
\n\
[Install]\n\
WantedBy=default.target\n\
ORBIT_SVC_EOF\n\
           systemctl --user daemon-reload\n\
           systemctl --user enable --now orbit-relay-tunnel.service >/dev/null 2>&1 || true\n\
           systemctl --user restart orbit-relay-tunnel.service\n\
           STARTED_SYSTEMD=1\n\
         fi\n\
         if [ \"$STARTED_SYSTEMD\" -eq 0 ]; then\n\
           if [ -f \"$HOME/.orbit/cloudflared-tunnel.pid\" ]; then\n\
             OLD_PID=\"$(cat \"$HOME/.orbit/cloudflared-tunnel.pid\" 2>/dev/null || true)\"\n\
             if [ -n \"$OLD_PID\" ]; then kill \"$OLD_PID\" 2>/dev/null || true; fi\n\
           fi\n\
           if [ -f \"$HOME/.orbit/cloudflared-tunnel.env\" ]; then\n\
             set -a; . \"$HOME/.orbit/cloudflared-tunnel.env\"; set +a\n\
           fi\n\
           nohup $ORBIT_TUNNEL_EXEC >> \"$HOME/.orbit/cloudflared-tunnel.log\" 2>&1 </dev/null &\n\
           echo $! > \"$HOME/.orbit/cloudflared-tunnel.pid\"\n\
         fi\n\
         if [ -n \"$ORBIT_NAMED_HOST\" ]; then\n\
           printf 'ORBIT_CF_URL=https://%s\\n' \"$ORBIT_NAMED_HOST\"\n\
           exit 0\n\
         fi\n\
         I=0\n\
         CF_URL=\"\"\n\
         while [ \"$I\" -lt 50 ]; do\n\
           CF_URL=\"$(grep -oE 'https://[a-zA-Z0-9-]+\\.trycloudflare\\.com' \"$HOME/.orbit/cloudflared-tunnel.log\" | tail -n 1 || true)\"\n\
           if [ -n \"$CF_URL\" ]; then break; fi\n\
           I=$((I + 1))\n\
           sleep 0.5\n\
         done\n\
         if [ -z \"$CF_URL\" ]; then\n\
           echo \"Timed out waiting for trycloudflare.com URL in cloudflared-tunnel.log\" >&2\n\
           tail -n 20 \"$HOME/.orbit/cloudflared-tunnel.log\" >&2 || true\n\
           exit 1\n\
         fi\n\
         printf 'ORBIT_CF_URL=%s\\n' \"$CF_URL\"\n"
    );

    let mut child = build_ssh_command()
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=15",
            "--",
            alias,
            "sh -s",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to launch ssh: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(remote_script.as_bytes());
        let _ = stdin.flush();
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("failed waiting for ssh: {e}"))?;

    if !out.status.success() {
        let detail = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            format!("SSH exited with status {}", out.status)
        } else {
            detail
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_cloudflare_tunnel_stdout(&stdout, &fleet_token)
}

/// Deploy/update Orbit on a remote SSH host and expose its Relay via Zero-Port Cloudflare Tunnel.
pub fn provision_cloudflare_tunnel_relay_over_ssh(
    alias: &str,
    relay_port: u16,
    allowed_pubkeys: &[String],
    public_hostname: Option<&str>,
    tunnel_token: Option<&str>,
) -> Result<String, String> {
    let home_dir = resolve_user_home()
        .ok_or_else(|| "cannot resolve user home directory for fleet token storage".to_string())?
        .join(".orbit");
    provision_cloudflare_tunnel_relay_with_home(
        &home_dir,
        alias,
        relay_port,
        allowed_pubkeys,
        public_hostname,
        tunnel_token,
    )
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

fn resolve_folder_picker_start_dir(initial_dir: Option<&str>) -> Option<PathBuf> {
    let home = resolve_user_home();
    if let Some(raw) = initial_dir.map(str::trim).filter(|s| !s.is_empty()) {
        let expanded = expand_local_tilde(raw);
        if expanded.is_dir() {
            let is_home = home.as_ref().is_some_and(|h| h == &expanded);
            if !is_home
                && let Some(parent) = expanded.parent()
                && parent.is_dir()
                && parent != Path::new("/")
            {
                return Some(parent.to_path_buf());
            }
            if !is_home {
                return Some(expanded);
            }
        }
    }

    if let Some(h) = home {
        for candidate in ["Code", "Projects", "Developer", "workspace", "src"] {
            let sub = h.join(candidate);
            if sub.is_dir() {
                return Some(sub);
            }
        }
        if h.is_dir() {
            return Some(h);
        }
    }
    None
}

#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux", test)),
    allow(dead_code)
)]
fn normalize_picked_posix_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let clean = trimmed.trim_end_matches('/');
    if clean.is_empty() {
        Some("/".to_string())
    } else {
        Some(clean.to_string())
    }
}

/// Open the native OS directory chooser sheet (`NSOpenPanel` on macOS, `zenity`/`kdialog` on Linux,
/// `FolderBrowserDialog` on Windows). Returns `Ok(None)` cleanly when the user cancels.
pub fn pick_local_directory(initial_dir: Option<&str>) -> Result<Option<String>, String> {
    let start_dir = resolve_folder_picker_start_dir(initial_dir);

    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("/usr/bin/osascript");
        if let Some(ref dir) = start_dir {
            cmd.args([
                "-e",
                "on run argv",
                "-e",
                "try",
                "-e",
                "set startAlias to POSIX file (item 1 of argv) as alias",
                "-e",
                "return POSIX path of (choose folder with prompt \"Select Project Folder\" default location startAlias)",
                "-e",
                "on error errMsg number errNum",
                "-e",
                "if errNum is -128 then error number -128",
                "-e",
                "return POSIX path of (choose folder with prompt \"Select Project Folder\")",
                "-e",
                "end try",
                "-e",
                "end run",
                "--",
            ])
            .arg(dir);
        } else {
            cmd.args([
                "-e",
                "POSIX path of (choose folder with prompt \"Select Project Folder\")",
            ]);
        }

        let out = cmd
            .output()
            .map_err(|e| format!("failed to open macOS folder picker: {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let lower = stderr.to_ascii_lowercase();
            if stderr.contains("(-128)")
                || lower.contains("canceled")
                || lower.contains("cancelled")
            {
                return Ok(None);
            }
            return Err(format!("folder picker failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        Ok(normalize_picked_posix_path(&stdout))
    }

    #[cfg(target_os = "linux")]
    {
        if binary_exists_on_path("zenity") {
            let mut cmd = Command::new("zenity");
            cmd.args([
                "--file-selection",
                "--directory",
                "--title=Select Project Folder",
            ]);
            if let Some(ref dir) = start_dir {
                cmd.arg(format!("--filename={}/", dir.to_string_lossy()));
            }
            let out = cmd
                .output()
                .map_err(|e| format!("failed to launch zenity: {e}"))?;
            if !out.status.success() {
                return Ok(None);
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            return Ok(normalize_picked_posix_path(&stdout));
        }

        if binary_exists_on_path("kdialog") {
            let mut cmd = Command::new("kdialog");
            cmd.arg("--getexistingdirectory");
            if let Some(ref dir) = start_dir {
                cmd.arg(dir);
            } else {
                cmd.arg(".");
            }
            let out = cmd
                .output()
                .map_err(|e| format!("failed to launch kdialog: {e}"))?;
            if !out.status.success() {
                return Ok(None);
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            return Ok(normalize_picked_posix_path(&stdout));
        }

        Err("Install `zenity` or `kdialog` to open the native Linux folder chooser".into())
    }

    #[cfg(target_os = "windows")]
    {
        let init_line = start_dir
            .as_ref()
            .map(|d| {
                let escaped = d.to_string_lossy().replace('\'', "''");
                format!("$d.SelectedPath = '{escaped}'; ")
            })
            .unwrap_or_default();
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $d.Description = 'Select Project Folder'; \
             {init_line}\
             if ($d.ShowDialog() -eq 'OK') {{ [Console]::Out.WriteLine($d.SelectedPath) }}"
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-Sta", "-Command", &script])
            .output()
            .map_err(|e| format!("failed to open Windows folder picker: {e}"))?;
        if !out.status.success() {
            return Ok(None);
        }
        let trimmed = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed))
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = start_dir;
        Err("Native folder picker is not supported on this platform".into())
    }
}

/// Extract a safe basename for native OS save dialogs, blocking path traversal (`..`, `/`, `\`),
/// control characters, and leading `-` characters that CLI dialogs (`kdialog`/`zenity`) could parse as flags.
fn sanitize_save_dialog_filename(raw: &str) -> String {
    let base = raw.trim().rsplit(['/', '\\']).next().unwrap_or("").trim();
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | '\0'))
        .collect();
    let trimmed = cleaned.trim_start_matches(['-', '.']).trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "orbit-conversation.md".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Open the native OS save-file sheet (`NSSavePanel` via `osascript` on macOS, `zenity`/`kdialog`
/// on Linux, `SaveFileDialog` on Windows). Returns `Ok(None)` cleanly when the user cancels.
pub fn pick_local_save_file(
    default_filename: &str,
    extension: &str,
    initial_dir: Option<&str>,
) -> Result<Option<String>, String> {
    let start_dir = resolve_folder_picker_start_dir(initial_dir);
    let effective_name = sanitize_save_dialog_filename(default_filename);
    let clean_ext: String = extension
        .trim()
        .trim_start_matches('.')
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();

    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("/usr/bin/osascript");
        if let Some(ref dir) = start_dir {
            cmd.args([
                "-e",
                "on run argv",
                "-e",
                "set defName to item 1 of argv",
                "-e",
                "try",
                "-e",
                "set startAlias to POSIX file (item 2 of argv) as alias",
                "-e",
                "return POSIX path of (choose file name with prompt \"Export Conversation\" default name defName default location startAlias)",
                "-e",
                "on error errMsg number errNum",
                "-e",
                "if errNum is -128 then error number -128",
                "-e",
                "return POSIX path of (choose file name with prompt \"Export Conversation\" default name defName)",
                "-e",
                "end try",
                "-e",
                "end run",
                "--",
            ])
            .arg(&effective_name)
            .arg(dir);
        } else {
            cmd.args([
                "-e",
                "on run argv",
                "-e",
                "set defName to item 1 of argv",
                "-e",
                "return POSIX path of (choose file name with prompt \"Export Conversation\" default name defName)",
                "-e",
                "end run",
                "--",
            ])
            .arg(&effective_name);
        }

        let _ = clean_ext;
        let out = cmd
            .output()
            .map_err(|e| format!("failed to open macOS save dialog: {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let lower = stderr.to_ascii_lowercase();
            if stderr.contains("(-128)")
                || lower.contains("canceled")
                || lower.contains("cancelled")
            {
                return Ok(None);
            }
            return Err(format!("save dialog failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let trimmed = stdout.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed.to_string()))
        }
    }

    #[cfg(target_os = "linux")]
    {
        let initial_target = match start_dir {
            Some(ref dir) => dir.join(&effective_name),
            None => PathBuf::from(&effective_name),
        };
        if binary_exists_on_path("zenity") {
            let mut cmd = Command::new("zenity");
            cmd.args([
                "--file-selection",
                "--save",
                "--confirm-overwrite",
                "--title=Export Conversation",
                &format!("--filename={}", initial_target.to_string_lossy()),
            ]);
            if !clean_ext.is_empty() {
                cmd.arg(format!("--file-filter=*.{clean_ext}"));
            }
            let out = cmd
                .output()
                .map_err(|e| format!("failed to launch zenity: {e}"))?;
            if !out.status.success() {
                return Ok(None);
            }
            let trimmed = String::from_utf8_lossy(&out.stdout)
                .trim_end_matches(['\r', '\n'])
                .to_string();
            return Ok((!trimmed.is_empty()).then_some(trimmed));
        }

        if binary_exists_on_path("kdialog") {
            let mut cmd = Command::new("kdialog");
            cmd.arg("--getsavefilename").arg("--").arg(&initial_target);
            if !clean_ext.is_empty() {
                cmd.arg(format!("*.{clean_ext}"));
            }
            let out = cmd
                .output()
                .map_err(|e| format!("failed to launch kdialog: {e}"))?;
            if !out.status.success() {
                return Ok(None);
            }
            let trimmed = String::from_utf8_lossy(&out.stdout)
                .trim_end_matches(['\r', '\n'])
                .to_string();
            return Ok((!trimmed.is_empty()).then_some(trimmed));
        }

        Err("Install `zenity` or `kdialog` to open the native Linux save dialog".into())
    }

    #[cfg(target_os = "windows")]
    {
        let filter = if clean_ext.is_empty() {
            "All files (*.*)|*.*".to_string()
        } else {
            format!(
                "{} (*.{clean_ext})|*.{clean_ext}|All files (*.*)|*.*",
                clean_ext.to_ascii_uppercase()
            )
        };
        const SCRIPT: &str = r#"
Add-Type -AssemblyName System.Windows.Forms
$d = New-Object System.Windows.Forms.SaveFileDialog
$d.Title = 'Export Conversation'
$d.FileName = $env:ORBIT_SAVE_NAME
$d.Filter = $env:ORBIT_SAVE_FILTER
if ($env:ORBIT_SAVE_DIR) { $d.InitialDirectory = $env:ORBIT_SAVE_DIR }
if ($d.ShowDialog() -eq 'OK') { [Console]::Out.WriteLine($d.FileName) }
"#;
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-Sta", "-Command", SCRIPT])
            .env("ORBIT_SAVE_NAME", &effective_name)
            .env("ORBIT_SAVE_FILTER", &filter);
        if let Some(ref dir) = start_dir {
            cmd.env("ORBIT_SAVE_DIR", dir);
        } else {
            cmd.env_remove("ORBIT_SAVE_DIR");
        }
        let out = cmd
            .output()
            .map_err(|e| format!("failed to open Windows save dialog: {e}"))?;
        if !out.status.success() {
            return Ok(None);
        }
        let trimmed = String::from_utf8_lossy(&out.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_string();
        Ok((!trimmed.is_empty()).then_some(trimmed))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (start_dir, effective_name, clean_ext);
        Err("Native save dialog is not supported on this platform".into())
    }
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
            #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
            {
                let _ = &local_path;
                Err("External terminal launch is not supported on mobile OS targets".into())
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
            #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
            {
                let _ = &local_path;
                Err("External file manager launch is not supported on mobile OS targets".into())
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

#[derive(Debug, Clone)]
pub struct LocalFileReadPayload {
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
    pub offset_bytes: u64,
    pub next_offset_bytes: u64,
    pub etag_mismatch: bool,
}

fn local_sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn local_metadata_mtime_unix_ms(meta: &fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|t| match t.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_millis()).ok(),
            Err(e) => i64::try_from(e.duration().as_millis()).ok().map(|v| -v),
        })
}

fn local_detect_line_ending(text: &str) -> String {
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

fn managed_attachment_root() -> Option<PathBuf> {
    let orbit_home_dir = std::env::var_os("ORBIT_HOME")
        .map(PathBuf::from)
        .or_else(|| resolve_user_home().map(|h| h.join(".orbit")));
    orbit_home_dir.map(|oh| oh.join("attachments"))
}

#[derive(Debug, Clone, Default)]
struct ManagedLocalWorktreeRoots {
    agy_worktree_roots: Vec<PathBuf>,
    orbit_worktree_roots: Vec<PathBuf>,
}

impl ManagedLocalWorktreeRoots {
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

fn canonicalize_local_existing_prefix(path: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut cur = path;
    loop {
        if let Ok(canon) = fs::canonicalize(cur) {
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

fn strip_local_worktree_rel_with_skip(
    candidate: &Path,
    canon_candidate: Option<&Path>,
    root: &Path,
    skip_leading_segments: usize,
) -> Option<PathBuf> {
    use std::path::Component;

    let canon_root = fs::canonicalize(root).ok();
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

fn extract_deleted_local_worktree_relative_path(
    candidate: &Path,
    worktree_roots: &ManagedLocalWorktreeRoots,
) -> Option<PathBuf> {
    use std::path::Component;

    if !candidate.is_absolute() {
        return None;
    }
    if candidate
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return None;
    }
    let canon_candidate = canonicalize_local_existing_prefix(candidate);
    for root in &worktree_roots.agy_worktree_roots {
        if let Some(rel) =
            strip_local_worktree_rel_with_skip(candidate, canon_candidate.as_deref(), root, 2)
        {
            return Some(rel);
        }
    }
    for root in &worktree_roots.orbit_worktree_roots {
        if let Some(rel) =
            strip_local_worktree_rel_with_skip(candidate, canon_candidate.as_deref(), root, 1)
        {
            return Some(rel);
        }
    }
    None
}

fn resolve_local_git_common_dir(start: &Path, stop_root: Option<&Path>) -> Option<PathBuf> {
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
        if let Ok(meta) = fs::symlink_metadata(&dot_git) {
            if meta.is_dir() {
                return fs::canonicalize(&dot_git).ok();
            }
            if meta.is_file() {
                let raw = fs::read_to_string(&dot_git).ok()?;
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
                let canon_gitdir = fs::canonicalize(&gitdir_path).ok()?;
                let commondir_file = canon_gitdir.join("commondir");
                if let Ok(common_raw) = fs::read_to_string(&commondir_file) {
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
                        return fs::canonicalize(&common_path).ok();
                    }
                }
                return Some(canon_gitdir);
            }
        }
        cur = cur.parent()?;
    }
}

fn is_same_repo_local_worktree_file(
    canonical_cwd: &Path,
    canonical_target: &Path,
    worktree_roots: &ManagedLocalWorktreeRoots,
) -> bool {
    let Some(cwd_common) = resolve_local_git_common_dir(canonical_cwd, None) else {
        return false;
    };
    for wt_root in worktree_roots.all_roots() {
        let Ok(canon_wt_root) = fs::canonicalize(wt_root) else {
            continue;
        };
        if !canonical_target.starts_with(&canon_wt_root) {
            continue;
        }
        if let Some(target_common) =
            resolve_local_git_common_dir(canonical_target, Some(&canon_wt_root))
            && target_common == cwd_common
        {
            return true;
        }
    }
    false
}

#[cfg(unix)]
fn local_unix_o_nofollow_flag() -> Option<i32> {
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
fn open_validated_local_regular_file(
    canonical: &Path,
    trimmed: &str,
) -> Result<(fs::File, fs::Metadata), String> {
    let lmeta_before =
        fs::symlink_metadata(canonical).map_err(|e| format!("Cannot read `{trimmed}`: {e}"))?;
    if lmeta_before.file_type().is_symlink() {
        return Err(format!(
            "Access denied: `{trimmed}` resolved to a symlink during open"
        ));
    }
    if !lmeta_before.is_file() {
        return Err(format!("`{trimmed}` is not a regular file"));
    }

    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    if let Some(flag) = local_unix_o_nofollow_flag() {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(flag);
    }
    let file = opts
        .open(canonical)
        .map_err(|e| format!("Failed opening `{trimmed}`: {e}"))?;
    let fmeta = file
        .metadata()
        .map_err(|e| format!("Cannot inspect `{trimmed}`: {e}"))?;
    if !fmeta.is_file() {
        return Err(format!("`{trimmed}` is not a regular file"));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let lmeta_after = fs::symlink_metadata(canonical)
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

fn resolve_allowed_local_file_path(
    cwd: Option<&str>,
    raw_path: &str,
    allow_agent_log_roots: bool,
) -> Result<PathBuf, String> {
    let home = resolve_user_home();
    let worktree_roots = ManagedLocalWorktreeRoots::from_env(home.as_deref());
    resolve_allowed_local_file_path_with_roots(
        cwd,
        raw_path,
        allow_agent_log_roots,
        &worktree_roots,
    )
}

fn resolve_allowed_local_file_path_with_roots(
    cwd: Option<&str>,
    raw_path: &str,
    allow_agent_log_roots: bool,
    worktree_roots: &ManagedLocalWorktreeRoots,
) -> Result<PathBuf, String> {
    use std::path::Component;

    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("File path cannot be empty".into());
    }

    let mut managed_artifact_roots: Vec<PathBuf> = Vec::new();
    if let Some(att_root) = managed_attachment_root() {
        managed_artifact_roots.push(att_root);
    }
    if allow_agent_log_roots {
        if let Some(h) = resolve_user_home() {
            managed_artifact_roots.push(h.join(".gemini").join("antigravity-cli").join("brain"));
            managed_artifact_roots.push(h.join(".claude").join("projects"));
        }
        managed_artifact_roots.push(std::env::temp_dir().join("claude"));
        let tmp_claude = PathBuf::from("/tmp/claude");
        if !managed_artifact_roots.contains(&tmp_claude) {
            managed_artifact_roots.push(tmp_claude);
        }
    }

    let is_in_managed_root = |canon: &Path| -> bool {
        managed_artifact_roots.iter().any(|root| {
            fs::canonicalize(root)
                .ok()
                .is_some_and(|canon_root| canon.starts_with(&canon_root))
        })
    };

    let artifact_direct = (|| -> Option<PathBuf> {
        let cand = if let Some(rest) = trimmed.strip_prefix("~/") {
            resolve_user_home()?.join(rest)
        } else {
            let p = Path::new(trimmed);
            if !p.is_absolute() {
                return None;
            }
            p.to_path_buf()
        };
        let canon_cand = fs::canonicalize(&cand).ok()?;
        is_in_managed_root(&canon_cand).then_some(canon_cand)
    })();

    if let Some(att_path) = artifact_direct {
        return Ok(att_path);
    }

    let Some(raw_cwd) = cwd else {
        return Err(format!(
            "Access denied: `{trimmed}` is outside the allowed attachments directory"
        ));
    };

    let resolved_cwd = if raw_cwd.trim().is_empty() || raw_cwd.trim() == "~" {
        if !allow_agent_log_roots {
            return Err("Workspace directory is not registered for external opening".into());
        }
        resolve_user_home().unwrap_or_else(|| PathBuf::from("."))
    } else if let Some(rest) = raw_cwd.trim().strip_prefix("~/") {
        resolve_user_home()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(raw_cwd.trim()))
    } else {
        PathBuf::from(raw_cwd.trim())
    };
    let canonical_cwd = fs::canonicalize(&resolved_cwd)
        .map_err(|e| format!("Workspace directory unavailable: {e}"))?;
    if !allow_agent_log_roots && canonical_cwd.parent().is_none() {
        return Err("Refusing to open external files from filesystem root".into());
    }

    let candidate = if let Some(rest) = trimmed.strip_prefix("~/") {
        resolve_user_home()
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

    let canon = match fs::canonicalize(&candidate) {
        Ok(c) => c,
        Err(orig_err) => {
            if allow_agent_log_roots
                && let Some(rel) =
                    extract_deleted_local_worktree_relative_path(&candidate, worktree_roots)
                && let Ok(fallback) = fs::canonicalize(canonical_cwd.join(&rel))
                && fallback.starts_with(&canonical_cwd)
            {
                return Ok(fallback);
            }
            return Err(format!("Cannot read `{trimmed}`: {orig_err}"));
        }
    };
    let allowed = canon.starts_with(&canonical_cwd)
        || is_in_managed_root(&canon)
        || (allow_agent_log_roots
            && is_same_repo_local_worktree_file(&canonical_cwd, &canon, worktree_roots));
    if !allowed {
        return Err(format!(
            "Access denied: `{trimmed}` is outside the workspace directory"
        ));
    }
    Ok(canon)
}

fn is_local_path_inside_git(canonical_cwd: &Path, canonical_target: &Path) -> bool {
    let Ok(rel) = canonical_target.strip_prefix(canonical_cwd) else {
        return false;
    };
    rel.components().any(|c| match c {
        std::path::Component::Normal(part) => part == ".git",
        _ => false,
    })
}

fn compute_local_file_writable(
    cwd: &str,
    trimmed: &str,
    canonical: &Path,
    meta: &fs::Metadata,
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
    if meta.permissions().readonly() {
        return false;
    }
    let resolved_cwd = if cwd.trim().is_empty() || cwd.trim() == "~" {
        return false;
    } else if let Some(rest) = cwd.trim().strip_prefix("~/") {
        resolve_user_home()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(cwd.trim()))
    } else {
        PathBuf::from(cwd.trim())
    };
    let Ok(canonical_cwd) = fs::canonicalize(&resolved_cwd) else {
        return false;
    };
    if !canonical.starts_with(&canonical_cwd) || is_local_path_inside_git(&canonical_cwd, canonical)
    {
        return false;
    }
    let direct_candidate = canonical_cwd.join(Path::new(trimmed));
    if let Ok(sym_meta) = fs::symlink_metadata(&direct_candidate)
        && sym_meta.file_type().is_symlink()
    {
        return false;
    }
    true
}

/// Local filesystem fallback when reading a workspace file on the current machine against an older daemon.
pub fn read_local_workspace_file(
    cwd: &str,
    raw_path: &str,
    max_bytes: Option<u64>,
) -> Result<LocalFileReadPayload, String> {
    read_local_workspace_file_with_writable(cwd, raw_path, max_bytes, false)
}

fn local_stream_file_sha256_hex(
    file: &mut fs::File,
    max_hash_bytes: u64,
    trimmed: &str,
) -> Result<Option<String>, String> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    use std::io::{Read, Seek, SeekFrom};

    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("Failed seeking `{trimmed}`: {e}"))?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = file
            .read(&mut chunk)
            .map_err(|e| format!("Failed hashing `{trimmed}`: {e}"))?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n as u64);
        if total > max_hash_bytes {
            return Ok(None);
        }
        hasher.update(&chunk[..n]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(&mut out, "{b:02x}");
    }
    Ok(Some(out))
}

/// Local filesystem reader for workspace files; computes `writable` using the same rules as the
/// daemon when `allow_writable` is true (i.e., an active local daemon session can persist edits).
pub fn read_local_workspace_file_with_writable(
    cwd: &str,
    raw_path: &str,
    max_bytes: Option<u64>,
    allow_writable: bool,
) -> Result<LocalFileReadPayload, String> {
    read_local_workspace_file_chunk_with_writable(
        cwd,
        raw_path,
        None,
        max_bytes,
        None,
        allow_writable,
    )
}

/// Local filesystem reader supporting bounded chunked text continuation (`offset_bytes`) and
/// full-file SHA-256 `expected_etag` verification matching the daemon implementation.
pub fn read_local_workspace_file_chunk_with_writable(
    cwd: &str,
    raw_path: &str,
    offset_bytes: Option<u64>,
    max_bytes: Option<u64>,
    expected_etag: Option<&str>,
    allow_writable: bool,
) -> Result<LocalFileReadPayload, String> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    use std::io::{Read, Seek, SeekFrom};

    const LOCAL_DEFAULT_MAX_READ_BYTES: u64 = 2 * 1024 * 1024;
    const LOCAL_HARD_MAX_READ_BYTES: u64 = 4 * 1024 * 1024;
    const LOCAL_CHUNKED_MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
    const LOCAL_IMAGE_MAX_READ_BYTES: u64 = 8 * 1024 * 1024;
    const LOCAL_MEDIA_MAX_READ_BYTES: u64 = 25 * 1024 * 1024;

    let trimmed = raw_path.trim();
    let canonical = resolve_allowed_local_file_path(Some(cwd), trimmed, true)?;
    let (mut file, meta) = open_validated_local_regular_file(&canonical, trimmed)?;

    let size_bytes = meta.len();
    let mtime_unix_ms = local_metadata_mtime_unix_ms(&meta);
    let writable = allow_writable && compute_local_file_writable(cwd, trimmed, &canonical, &meta);
    let resolved_path = canonical.to_string_lossy().to_string();
    let start_offset = offset_bytes.unwrap_or(0);
    let clean_expected_etag = expected_etag.map(str::trim).filter(|s| !s.is_empty());

    if start_offset > 0 {
        if start_offset > LOCAL_CHUNKED_MAX_FILE_BYTES {
            return Err(format!(
                "Chunk offset `{start_offset}` exceeds maximum chunked read limit ({LOCAL_CHUNKED_MAX_FILE_BYTES} bytes)"
            ));
        }
        let live_etag = if size_bytes <= LOCAL_CHUNKED_MAX_FILE_BYTES {
            local_stream_file_sha256_hex(&mut file, LOCAL_CHUNKED_MAX_FILE_BYTES, trimmed)?
        } else {
            None
        };
        if let Some(exp) = clean_expected_etag
            && live_etag.as_deref() != Some(exp)
        {
            return Ok(LocalFileReadPayload {
                resolved_path,
                content_kind: "text".into(),
                mime_type: Some("text/plain".into()),
                content: String::new(),
                size_bytes,
                truncated: true,
                etag: live_etag,
                mtime_unix_ms,
                writable,
                line_ending: None,
                offset_bytes: start_offset,
                next_offset_bytes: start_offset,
                etag_mismatch: true,
            });
        }
        if start_offset >= size_bytes {
            return Ok(LocalFileReadPayload {
                resolved_path,
                content_kind: "text".into(),
                mime_type: Some("text/plain".into()),
                content: String::new(),
                size_bytes,
                truncated: false,
                etag: live_etag,
                mtime_unix_ms,
                writable,
                line_ending: None,
                offset_bytes: start_offset,
                next_offset_bytes: size_bytes,
                etag_mismatch: false,
            });
        }
        let remaining_allowed = LOCAL_CHUNKED_MAX_FILE_BYTES.saturating_sub(start_offset);
        let cap = max_bytes
            .unwrap_or(LOCAL_DEFAULT_MAX_READ_BYTES)
            .clamp(4096, LOCAL_HARD_MAX_READ_BYTES)
            .min(remaining_allowed);
        file.seek(SeekFrom::Start(start_offset))
            .map_err(|e| format!("Failed seeking `{trimmed}`: {e}"))?;
        let mut buf = Vec::new();
        (&mut file)
            .take(cap.saturating_add(1))
            .read_to_end(&mut buf)
            .map_err(|e| format!("Failed reading chunk of `{trimmed}`: {e}"))?;
        let has_more_raw =
            (buf.len() as u64) > cap || start_offset.saturating_add(buf.len() as u64) < size_bytes;
        if (buf.len() as u64) > cap {
            buf.truncate(cap as usize);
        }
        if buf.contains(&0) {
            return Ok(LocalFileReadPayload {
                resolved_path,
                content_kind: "binary".into(),
                mime_type: None,
                content: String::new(),
                size_bytes,
                truncated: has_more_raw || start_offset > 0,
                etag: live_etag,
                mtime_unix_ms,
                writable: if start_offset > 0 { false } else { writable },
                line_ending: None,
                offset_bytes: start_offset,
                next_offset_bytes: start_offset,
                etag_mismatch: false,
            });
        }
        let text = match String::from_utf8(buf) {
            Ok(s) => s,
            Err(err) => {
                let is_incomplete_tail = err.utf8_error().error_len().is_none();
                let valid_up_to = err.utf8_error().valid_up_to();
                let raw = err.into_bytes();
                if has_more_raw && is_incomplete_tail {
                    String::from_utf8_lossy(&raw[..valid_up_to]).into_owned()
                } else {
                    return Ok(LocalFileReadPayload {
                        resolved_path,
                        content_kind: "binary".into(),
                        mime_type: None,
                        content: String::new(),
                        size_bytes,
                        truncated: has_more_raw || start_offset > 0,
                        etag: live_etag,
                        mtime_unix_ms,
                        writable: if start_offset > 0 { false } else { writable },
                        line_ending: None,
                        offset_bytes: start_offset,
                        next_offset_bytes: start_offset,
                        etag_mismatch: false,
                    });
                }
            }
        };
        let next_offset_bytes = start_offset.saturating_add(text.len() as u64);
        let truncated = next_offset_bytes < size_bytes;
        let line_ending = Some(local_detect_line_ending(&text));
        return Ok(LocalFileReadPayload {
            resolved_path,
            content_kind: "text".into(),
            mime_type: Some("text/plain".into()),
            content: text,
            size_bytes,
            truncated,
            etag: live_etag,
            mtime_unix_ms,
            writable,
            line_ending,
            offset_bytes: start_offset,
            next_offset_bytes,
            etag_mismatch: false,
        });
    }

    let mut header = Vec::with_capacity((size_bytes as usize).min(4096));
    (&mut file)
        .take(4096)
        .read_to_end(&mut header)
        .map_err(|e| format!("Failed reading `{trimmed}`: {e}"))?;

    let filename_hint = canonical.file_name().and_then(|n| n.to_str());
    if let Some(sniffed) =
        orbit_protocol::media::sniff_media_or_viewer_image_mime(&header, filename_hint)
    {
        let cap = match sniffed.modality {
            orbit_protocol::media::AttachmentModality::Image => max_bytes
                .unwrap_or(LOCAL_IMAGE_MAX_READ_BYTES)
                .clamp(4096, LOCAL_IMAGE_MAX_READ_BYTES),
            orbit_protocol::media::AttachmentModality::Pdf
            | orbit_protocol::media::AttachmentModality::Audio
            | orbit_protocol::media::AttachmentModality::Video => max_bytes
                .unwrap_or(LOCAL_MEDIA_MAX_READ_BYTES)
                .clamp(4096, LOCAL_MEDIA_MAX_READ_BYTES),
        };
        let kind_str = sniffed.modality.as_str();
        if size_bytes > cap {
            return Ok(LocalFileReadPayload {
                resolved_path,
                content_kind: kind_str.into(),
                mime_type: Some(sniffed.mime_type.into()),
                content: String::new(),
                size_bytes,
                truncated: true,
                etag: None,
                mtime_unix_ms,
                writable,
                line_ending: None,
                offset_bytes: 0,
                next_offset_bytes: 0,
                etag_mismatch: false,
            });
        }
        let mut bytes = header;
        let remaining = cap.saturating_sub(bytes.len() as u64).saturating_add(1);
        (&mut file)
            .take(remaining)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Failed reading media `{trimmed}`: {e}"))?;
        if (bytes.len() as u64) > cap {
            return Ok(LocalFileReadPayload {
                resolved_path,
                content_kind: kind_str.into(),
                mime_type: Some(sniffed.mime_type.into()),
                content: String::new(),
                size_bytes: bytes.len() as u64,
                truncated: true,
                etag: None,
                mtime_unix_ms,
                writable,
                line_ending: None,
                offset_bytes: 0,
                next_offset_bytes: 0,
                etag_mismatch: false,
            });
        }
        let etag = Some(local_sha256_hex(&bytes));
        let final_len = bytes.len() as u64;
        return Ok(LocalFileReadPayload {
            resolved_path,
            content_kind: kind_str.into(),
            mime_type: Some(sniffed.mime_type.into()),
            content: B64.encode(&bytes),
            size_bytes: final_len,
            truncated: false,
            etag,
            mtime_unix_ms,
            writable,
            line_ending: None,
            offset_bytes: 0,
            next_offset_bytes: final_len,
            etag_mismatch: false,
        });
    }

    let cap = max_bytes
        .unwrap_or(LOCAL_DEFAULT_MAX_READ_BYTES)
        .clamp(4096, LOCAL_HARD_MAX_READ_BYTES);
    let mut buf = header;
    if (buf.len() as u64) <= cap {
        let remaining = cap.saturating_sub(buf.len() as u64).saturating_add(1);
        (&mut file)
            .take(remaining)
            .read_to_end(&mut buf)
            .map_err(|e| format!("Failed reading `{trimmed}`: {e}"))?;
    }

    let raw_truncated = (buf.len() as u64) > cap || size_bytes > cap;
    if (buf.len() as u64) > cap {
        buf.truncate(cap as usize);
    }
    let etag = if !raw_truncated {
        Some(local_sha256_hex(&buf))
    } else if size_bytes <= LOCAL_CHUNKED_MAX_FILE_BYTES {
        local_stream_file_sha256_hex(&mut file, LOCAL_CHUNKED_MAX_FILE_BYTES, trimmed)?
    } else {
        None
    };
    if let Some(exp) = clean_expected_etag
        && etag.as_deref() != Some(exp)
    {
        return Ok(LocalFileReadPayload {
            resolved_path,
            content_kind: "text".into(),
            mime_type: Some("text/plain".into()),
            content: String::new(),
            size_bytes,
            truncated: true,
            etag,
            mtime_unix_ms,
            writable,
            line_ending: None,
            offset_bytes: 0,
            next_offset_bytes: 0,
            etag_mismatch: true,
        });
    }
    let probe_len = buf.len().min(8192);
    if buf[..probe_len].contains(&0) {
        return Ok(LocalFileReadPayload {
            resolved_path,
            content_kind: "binary".into(),
            mime_type: None,
            content: String::new(),
            size_bytes,
            truncated: raw_truncated,
            etag,
            mtime_unix_ms,
            writable,
            line_ending: None,
            offset_bytes: 0,
            next_offset_bytes: 0,
            etag_mismatch: false,
        });
    }

    let text = match String::from_utf8(buf) {
        Ok(s) => s,
        Err(err) => {
            let is_incomplete_tail = err.utf8_error().error_len().is_none();
            let valid_up_to = err.utf8_error().valid_up_to();
            let raw = err.into_bytes();
            if raw_truncated && is_incomplete_tail {
                String::from_utf8_lossy(&raw[..valid_up_to]).into_owned()
            } else {
                return Ok(LocalFileReadPayload {
                    resolved_path,
                    content_kind: "binary".into(),
                    mime_type: None,
                    content: String::new(),
                    size_bytes,
                    truncated: raw_truncated,
                    etag,
                    mtime_unix_ms,
                    writable,
                    line_ending: None,
                    offset_bytes: 0,
                    next_offset_bytes: 0,
                    etag_mismatch: false,
                });
            }
        }
    };
    let next_offset_bytes = text.len() as u64;
    let truncated = raw_truncated || next_offset_bytes < size_bytes;
    let line_ending = Some(local_detect_line_ending(&text));

    Ok(LocalFileReadPayload {
        resolved_path,
        content_kind: "text".into(),
        mime_type: Some("text/plain".into()),
        content: text,
        size_bytes,
        truncated,
        etag,
        mtime_unix_ms,
        writable,
        line_ending,
        offset_bytes: 0,
        next_offset_bytes,
        etag_mismatch: false,
    })
}

fn is_safe_external_preview_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "pdf"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "bmp"
            | "ico"
            | "mp3"
            | "wav"
            | "m4a"
            | "aac"
            | "ogg"
            | "flac"
            | "opus"
            | "weba"
            | "mp4"
            | "mov"
            | "m4v"
            | "webm"
            | "mkv"
    )
}

/// Validate that `raw_path` resides inside a registered `cwd` (or strictly inside
/// `$ORBIT_HOME/attachments` when `cwd` is `None`), has an allowed preview extension, AND
/// matches supported image/PDF/audio/video magic header bytes.
pub fn validate_external_local_file(cwd: Option<&str>, raw_path: &str) -> Result<PathBuf, String> {
    use std::io::Read;

    let canonical = resolve_allowed_local_file_path(cwd, raw_path, false)?;
    let (file, _meta) = open_validated_local_regular_file(&canonical, raw_path.trim())?;
    if !is_safe_external_preview_extension(&canonical) {
        return Err("Only PDF, image, audio, and video files can be opened externally".into());
    }
    let mut header = Vec::with_capacity(4096);
    file.take(4096)
        .read_to_end(&mut header)
        .map_err(|e| format!("Cannot inspect file header: {e}"))?;
    let filename_hint = canonical.file_name().and_then(|n| n.to_str());
    if orbit_protocol::media::sniff_media_or_viewer_image_mime(&header, filename_hint).is_none() {
        return Err(
            "File content does not match an allowed PDF, image, audio, or video format".into(),
        );
    }
    Ok(canonical)
}

/// Open an allowlisted local workspace/attachment media or PDF file in the OS default viewer
/// without shell expansion. Strictly rejects unregistered roots, non-previewable/executable
/// extensions, spoofed magic bytes, and escapes.
pub fn open_local_file_externally(cwd: Option<&str>, raw_path: &str) -> Result<(), String> {
    let canonical = validate_external_local_file(cwd, raw_path)?;
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("--")
            .arg(&canonical)
            .spawn()
            .map_err(|e| format!("Failed to open file externally: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(&canonical)
            .spawn()
            .map_err(|e| format!("Failed to open file externally: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("rundll32")
            .arg("url.dll,FileProtocolHandler")
            .arg(&canonical)
            .spawn()
            .map_err(|e| format!("Failed to open file externally: {e}"))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = canonical;
        Err("External file launch is unsupported on this platform".into())
    }
}

fn validate_external_http_url(raw_url: &str) -> Result<&str, String> {
    let trimmed = raw_url.trim();
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .ok_or_else(|| "Only http:// and https:// URLs can be opened externally".to_string())?;
    if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("URL contains invalid control or whitespace characters".into());
    }
    let host_part = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host_part.is_empty() {
        return Err("URL must include a valid host".into());
    }
    Ok(trimmed)
}

/// Open an `http://` or `https://` URL in the user's default system browser without shell expansion.
pub fn open_external_url(raw_url: &str) -> Result<(), String> {
    let validated = validate_external_http_url(raw_url)?;
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(validated)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(validated)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", validated])
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = validated;
        Err("External URL launch is unsupported on this platform".into())
    }
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
        let key = generate_ssh_tunnel_key().expect("keygen");
        let ssh_uri = format!(
            "ssh+relay://ec2-user@13.228.27.154:22/7878?key={}",
            key.seed_b64
        );
        assert_eq!(
            normalize_relay_input(Some(&ssh_uri)).as_deref(),
            Some(ssh_uri.as_str())
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

    #[test]
    fn ws_port_extraction_handles_direct_endpoints() {
        assert_eq!(
            extract_ws_port(&["ws://172.31.33.74:6869/ws".to_string()]),
            Some(6869)
        );
        assert_eq!(
            extract_ws_port(&["wss://devbox.internal:7001/ws".to_string()]),
            Some(7001)
        );
    }

    #[test]
    fn picked_posix_path_strips_trailing_slash() {
        assert_eq!(
            normalize_picked_posix_path("/Users/jess/Code/orbit/\n"),
            Some("/Users/jess/Code/orbit".into())
        );
        assert_eq!(normalize_picked_posix_path("/\n"), Some("/".into()));
        assert_eq!(normalize_picked_posix_path("   "), None);
    }

    #[test]
    fn sanitize_save_dialog_filename_strips_paths_flags_and_control_chars() {
        assert_eq!(
            sanitize_save_dialog_filename("../../etc/passwd.md"),
            "passwd.md"
        );
        assert_eq!(
            sanitize_save_dialog_filename("--option-injection.html"),
            "option-injection.html"
        );
        assert_eq!(
            sanitize_save_dialog_filename("C:\\Users\\evil\\notes\n.json"),
            "notes.json"
        );
        assert_eq!(
            sanitize_save_dialog_filename("...---"),
            "orbit-conversation.md"
        );
    }

    #[test]
    fn validate_external_local_file_enforces_root_extension_and_magic_bytes() {
        let dir =
            std::env::temp_dir().join(format!("orbit_ext_open_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("mkdir");
        let cwd = dir.to_string_lossy().to_string();

        let png_path = dir.join("ok.png");
        fs::write(
            &png_path,
            [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0],
        )
        .expect("png");

        let pdf_path = dir.join("spec.pdf");
        fs::write(
            &pdf_path,
            b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n",
        )
        .expect("pdf");

        let weba_path = dir.join("clip.weba");
        fs::write(&weba_path, [0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81]).expect("weba");

        let spoofed_png = dir.join("fake.png");
        fs::write(&spoofed_png, b"not actually a png image").expect("fake png");

        let spoofed_pdf = dir.join("fake.pdf");
        fs::write(&spoofed_pdf, b"#!/bin/sh\necho pwned\n").expect("fake pdf");

        let script_path = dir.join("run.sh");
        fs::write(&script_path, b"#!/bin/sh\necho hi\n").expect("sh");

        let utf16_text = dir.join("notes.txt");
        fs::write(&utf16_text, [0xFF, 0xFE, b'A', 0x00, b'B', 0x00]).expect("utf16");

        // Valid magic-byte media inside registered cwd passes.
        assert!(validate_external_local_file(Some(&cwd), "ok.png").is_ok());
        assert!(validate_external_local_file(Some(&cwd), "spec.pdf").is_ok());
        assert!(validate_external_local_file(Some(&cwd), "clip.weba").is_ok());

        // Filesystem root `/` and unregistered `None` cwd are rejected for workspace files.
        assert!(
            validate_external_local_file(Some("/"), &pdf_path.to_string_lossy()).is_err(),
            "filesystem root cwd must be rejected"
        );
        assert!(
            validate_external_local_file(None, &pdf_path.to_string_lossy()).is_err(),
            "unregistered cwd must restrict to attachments dir"
        );
        assert!(
            validate_external_local_file(Some("~"), &pdf_path.to_string_lossy()).is_err(),
            "tilde cwd must be rejected for external opening"
        );

        // Spoofed magic bytes and executable extensions are rejected.
        assert!(validate_external_local_file(Some(&cwd), "fake.png").is_err());
        assert!(validate_external_local_file(Some(&cwd), "fake.pdf").is_err());
        assert!(validate_external_local_file(Some(&cwd), "run.sh").is_err());

        // Local workspace reader must NOT classify spoofed `fake.png` as image or UTF-16LE as MP3.
        let fake_payload =
            read_local_workspace_file(&cwd, "fake.png", None).expect("read fake.png");
        assert_eq!(fake_payload.content_kind, "text");

        let utf16_payload =
            read_local_workspace_file(&cwd, "notes.txt", None).expect("read utf16.txt");
        assert_ne!(utf16_payload.content_kind, "audio");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_subagent_worktree_reads_allow_live_and_deleted_fallback_and_block_escapes() {
        let root =
            std::env::temp_dir().join(format!("orbit_local_wt_read_{}", uuid::Uuid::new_v4()));
        let ws = root.join("repo_a");
        let repo_b = root.join("repo_b");
        let agy_wt_root = root.join("agy_data").join("worktrees");
        let agy_live_wt = agy_wt_root.join("conv-123").join("subagent-track-1-repo-a");
        let agy_other_repo_wt = agy_wt_root.join("conv-456").join("subagent-other-repo-b");
        let orbit_wt_root = root.join("orbit_home").join("worktrees");
        let orbit_live_wt = orbit_wt_root.join("repo-a-feat-foo");
        let orbit_other_repo_wt = orbit_wt_root.join("repo-b-feat-bar");
        let outside = root.join("outside_secrets");

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
            fs::create_dir_all(&dir).expect("mkdir");
        }
        for (wt_checkout, gitdir) in [
            (&agy_live_wt, &ws_wt_agy_gitdir),
            (&orbit_live_wt, &ws_wt_orbit_gitdir),
            (&agy_other_repo_wt, &repo_b_wt_agy_gitdir),
            (&orbit_other_repo_wt, &repo_b_wt_orbit_gitdir),
        ] {
            fs::write(gitdir.join("commondir"), "../..\n").expect("write commondir");
            fs::write(
                wt_checkout.join(".git"),
                format!("gitdir: {}\n", gitdir.display()),
            )
            .expect("write wt .git");
        }

        let primary_file = ws.join("src").join("lib.rs");
        let agy_live_file = agy_live_wt.join("src").join("lib.rs");
        let agy_other_repo_file = agy_other_repo_wt.join("src").join("secret_b.rs");
        let orbit_live_file = orbit_live_wt.join("src").join("lib.rs");
        let orbit_other_repo_file = orbit_other_repo_wt.join("src").join("secret_b.rs");
        let outside_file = outside.join("secret.txt");

        fs::write(&primary_file, "pub fn primary() {}\n").expect("write primary");
        fs::write(&agy_live_file, "pub fn agy_live() {}\n").expect("write agy live");
        fs::write(&agy_other_repo_file, "pub fn repo_b_secret() {}\n").expect("write agy other");
        fs::write(&orbit_live_file, "pub fn orbit_live() {}\n").expect("write orbit live");
        fs::write(&orbit_other_repo_file, "pub fn repo_b_secret() {}\n")
            .expect("write orbit other");
        fs::write(&outside_file, "top-secret\n").expect("write outside");

        let roots = ManagedLocalWorktreeRoots {
            agy_worktree_roots: vec![agy_wt_root.clone()],
            orbit_worktree_roots: vec![orbit_wt_root.clone()],
        };
        let cwd_str = ws.to_string_lossy().to_string();
        let canon_primary = fs::canonicalize(&primary_file).expect("canon primary");
        let canon_agy_live = fs::canonicalize(&agy_live_file).expect("canon agy live");
        let canon_orbit_live = fs::canonicalize(&orbit_live_file).expect("canon orbit live");

        // (a) Same-repo live Antigravity and Orbit worktree files allowed read-only
        let agy_raw = agy_live_file.to_string_lossy().to_string();
        let resolved_agy =
            resolve_allowed_local_file_path_with_roots(Some(&cwd_str), &agy_raw, true, &roots)
                .expect("same-repo live agy worktree file must resolve");
        assert_eq!(resolved_agy, canon_agy_live);
        let agy_meta = fs::metadata(&resolved_agy).expect("meta");
        assert!(
            !compute_local_file_writable(&cwd_str, &agy_raw, &resolved_agy, &agy_meta),
            "live agy worktree read must remain strictly read-only"
        );

        let orbit_raw = orbit_live_file.to_string_lossy().to_string();
        let resolved_orbit =
            resolve_allowed_local_file_path_with_roots(Some(&cwd_str), &orbit_raw, true, &roots)
                .expect("same-repo live orbit worktree file must resolve");
        assert_eq!(resolved_orbit, canon_orbit_live);

        // (a2) Cross-repository worktree files MUST BE STRICTLY DENIED
        assert!(
            resolve_allowed_local_file_path_with_roots(
                Some(&cwd_str),
                &agy_other_repo_file.to_string_lossy(),
                true,
                &roots,
            )
            .is_err(),
            "repo A must NEVER read a file from repo B's Antigravity worktree"
        );
        assert!(
            resolve_allowed_local_file_path_with_roots(
                Some(&cwd_str),
                &orbit_other_repo_file.to_string_lossy(),
                true,
                &roots,
            )
            .is_err(),
            "repo A must NEVER read a file from repo B's Orbit worktree"
        );

        // (b) Deleted worktree files gracefully fall back to primary workspace `cwd`
        let deleted_agy_raw = agy_wt_root
            .join("conv-999")
            .join("deleted-subagent-wt")
            .join("src")
            .join("lib.rs")
            .to_string_lossy()
            .to_string();
        let fallback_agy = resolve_allowed_local_file_path_with_roots(
            Some(&cwd_str),
            &deleted_agy_raw,
            true,
            &roots,
        )
        .expect("deleted agy worktree must fall back to primary cwd");
        assert_eq!(fallback_agy, canon_primary);
        let fb_meta = fs::metadata(&fallback_agy).expect("meta");
        assert!(!compute_local_file_writable(
            &cwd_str,
            &deleted_agy_raw,
            &fallback_agy,
            &fb_meta
        ));

        let deleted_orbit_raw = orbit_wt_root
            .join("deleted-orbit-wt")
            .join("src")
            .join("lib.rs")
            .to_string_lossy()
            .to_string();
        let fallback_orbit = resolve_allowed_local_file_path_with_roots(
            Some(&cwd_str),
            &deleted_orbit_raw,
            true,
            &roots,
        )
        .expect("deleted orbit worktree must fall back to primary cwd");
        assert_eq!(fallback_orbit, canon_primary);

        // (c) Traversal, outside files, external-open, and direct symlink opens stay denied
        let traversal = format!(
            "{}/conv-999/deleted-wt/../../outside_secrets/secret.txt",
            agy_wt_root.display()
        );
        assert!(
            resolve_allowed_local_file_path_with_roots(Some(&cwd_str), &traversal, true, &roots)
                .is_err()
        );
        assert!(
            resolve_allowed_local_file_path_with_roots(
                Some(&cwd_str),
                &outside_file.to_string_lossy(),
                true,
                &roots,
            )
            .is_err()
        );
        assert!(
            resolve_allowed_local_file_path_with_roots(Some(&cwd_str), &agy_raw, false, &roots)
                .is_err(),
            "external open must not open arbitrary worktree files"
        );

        #[cfg(unix)]
        {
            let sym = ws.join("src").join("swap_link.rs");
            std::os::unix::fs::symlink(&outside_file, &sym).expect("create symlink");
            assert!(
                open_validated_local_regular_file(&sym, "src/swap_link.rs").is_err(),
                "TOCTOU-hardened local open must reject symlinks directly"
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_system_hostname_resolves_non_empty_name() {
        let resolved = detect_system_hostname();
        assert!(!resolved.trim().is_empty());
        assert_ne!(resolved, "orbit-host");
    }

    #[test]
    #[ignore]
    fn live_devbox_ssh_one_click_e2e() {
        use crate::HostConnectionState;
        use crate::app_core::OrbitAppCore;

        let core = OrbitAppCore::open_in_memory().expect("open core");
        let card = core
            .pair_ssh_host_blocking("devbox", None)
            .expect("1-click SSH pair against devbox must succeed");
        assert_eq!(card.display_name, "devbox");

        for _ in 0..40 {
            let overview = core.fleet_overview();
            let detail = core.host_detail(&card.host_id).expect("host detail");
            if detail.host.connection_state == HostConnectionState::OnlineDirect
                && !detail.discovered_projects.is_empty()
            {
                println!(
                    "CONNECTED via_ssh_tunnel={} ping={:?}ms projects={} discovered={} workspaces={} agents={} models={}",
                    detail.host.via_ssh_tunnel,
                    detail.host.last_ping_ms,
                    overview.projects.len(),
                    detail.discovered_projects.len(),
                    detail.workspaces.len(),
                    detail.agents.len(),
                    detail.models_by_provider.len()
                );
                assert!(detail.host.via_ssh_tunnel);
                assert!(
                    detail
                        .discovered_projects
                        .iter()
                        .any(|p| p.path.contains("mono_repo"))
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        panic!(
            "devbox did not reach OnlineDirect with workspaces: {:?}",
            core.host_detail(&card.host_id)
        );
    }

    #[test]
    fn parse_cloudflare_tunnel_stdout_normalizes_url_and_appends_fleet_token() {
        let out = "some log line\nORBIT_CF_URL=https://orbit-mesh-test.trycloudflare.com/\n";
        let url = parse_cloudflare_tunnel_stdout(out, "ft_secret_123").expect("parse cf url");
        assert_eq!(
            url,
            "wss://orbit-mesh-test.trycloudflare.com?ft=ft_secret_123"
        );

        let plain = parse_cloudflare_tunnel_stdout("ORBIT_CF_URL=https://relay.my.org", "")
            .expect("empty token ok");
        assert_eq!(plain, "wss://relay.my.org");

        assert!(parse_cloudflare_tunnel_stdout("no url emitted", "ft").is_err());

        let err = provision_cloudflare_tunnel_relay_with_home(
            std::path::Path::new("/tmp"),
            "devbox",
            7878,
            &[],
            Some("relay.example.com"),
            None,
        )
        .expect_err("half-specified named tunnel must be rejected");
        assert!(err.contains("Named Cloudflare Tunnel requires both"));
    }
}
