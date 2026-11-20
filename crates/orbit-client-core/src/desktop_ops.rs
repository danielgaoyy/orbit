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

/// Automatically discover and ensure the current user's isolated local Orbit Daemon on Desktop (or Xcode iOS Simulator).
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
    let candidates = [
        home.join(".local/bin/orbit"),
        home.join(".cargo/bin/orbit"),
        home.join(".cargo/bin/orbit.exe"),
        PathBuf::from("/usr/local/bin/orbit"),
        PathBuf::from("/opt/homebrew/bin/orbit"),
    ];
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
    let keypair_path = home.join(".orbit").join("host-keypair.json");
    if !keypair_path.is_file() {
        return None;
    }
    let raw = fs::read_to_string(&keypair_path).ok()?;
    let kp: orbit_crypto::HostIdentityKeypair = serde_json::from_str(&raw).ok()?;
    let host_id = kp.host_id().ok()?;
    let hostname = std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string());
    let relay_url = fs::read_to_string(home.join(".orbit").join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("relay_url")
                .and_then(|r| r.as_str())
                .map(ToOwned::to_owned)
        })
        .and_then(|r| normalize_relay_input(Some(&r)));
    let offer = orbit_crypto::PairingOffer {
        host_id,
        hostname,
        daemon_pubkey_b64: kp.public_key_b64,
        direct_endpoints: vec!["ws://127.0.0.1:6868/ws".to_string()],
        relay_url,
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

/// Zero-form 1-click SSH Host Connection with Native Multi-User Isolation:
/// Connects to `alias`, installs/upgrades `orbit` if missing or `< 0.2.3`,
/// ensures this Unix user's own isolated `orbit daemon` is running on a conflict-free port,
/// and returns `orbit://pair/...` encoding that user's exact `host_id` & port.
pub fn bootstrap_ssh_host_and_pair(
    alias: &str,
    explicit_port: Option<u16>,
) -> Result<String, String> {
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
           curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh >/dev/null 2>&1; \
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
    let port_flag = explicit_port
        .map(|p| format!(" --port {p}"))
        .unwrap_or_default();
    let remote_script = format!(
        "set -eu; \
         export PATH=\"$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH\"; \
         curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh >/dev/null 2>&1; \
         orbit daemon up{port_flag} >/dev/null 2>&1; \
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

/// Deploy/update Orbit on a remote SSH host bound strictly to `127.0.0.1:<relay_port>`,
/// install a restricted `authorized_keys` tunnel key, and return the canonical `ssh+relay://...` URL.
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
    let tunnel_key = load_or_create_ssh_tunnel_key(home_dir)?;

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

    let remote_script = format!(
        "set -eu; \
         export PATH=\"$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH\"; \
         if ! command -v orbit >/dev/null 2>&1 || orbit --version 2>&1 | grep -q 'GLIBC_' || ! orbit relay --help 2>&1 | grep -q 'allow'; then \
           curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh; \
         fi; \
         orbit relay up --listen 127.0.0.1:{relay_port} {pubkey_flags}; \
         mkdir -p \"$HOME/.ssh\" && chmod 700 \"$HOME/.ssh\"; \
         touch \"$HOME/.ssh/authorized_keys\" && chmod 600 \"$HOME/.ssh/authorized_keys\"; \
         (grep -Fv ' {tag}' \"$HOME/.ssh/authorized_keys\" || true) > \"$HOME/.ssh/authorized_keys.orbit.tmp\"; \
         printf '%s\\n' '{restricted_line}' >> \"$HOME/.ssh/authorized_keys.orbit.tmp\"; \
         mv \"$HOME/.ssh/authorized_keys.orbit.tmp\" \"$HOME/.ssh/authorized_keys\" && chmod 600 \"$HOME/.ssh/authorized_keys\"; \
         printf 'ORBIT_SSH_USER=%s\\n' \"$(whoami)\"; \
         find /etc/ssh -maxdepth 1 -name 'ssh_host_*_key.pub' -type f 2>/dev/null | while IFS= read -r pubf; do \
           if [ -r \"$pubf\" ]; then \
             line=\"$(cat \"$pubf\" 2>/dev/null || true)\"; \
             if [ -n \"$line\" ]; then printf 'ORBIT_HOST_PUBKEY=%s\\n' \"$line\"; fi; \
           fi; \
         done || true"
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

    let mut remote_user: Option<String> = None;
    let mut host_key_fps_b64: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(u) = trimmed.strip_prefix("ORBIT_SSH_USER=") {
            let clean = u.trim();
            if !clean.is_empty() {
                remote_user = Some(clean.to_string());
            }
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
}

fn local_image_mime_for_path(path: &Path) -> Option<&'static str> {
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

/// Local filesystem fallback when reading a workspace file on the current machine against an older daemon.
pub fn read_local_workspace_file(
    cwd: &str,
    raw_path: &str,
    max_bytes: Option<u64>,
) -> Result<LocalFileReadPayload, String> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    use std::io::Read;
    use std::path::Component;

    const LOCAL_IMAGE_MAX_READ_BYTES: u64 = 8 * 1024 * 1024;

    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("File path cannot be empty".into());
    }

    let orbit_home_dir = std::env::var_os("ORBIT_HOME")
        .map(PathBuf::from)
        .or_else(|| resolve_user_home().map(|h| h.join(".orbit")));

    let attachment_direct = (|| -> Option<PathBuf> {
        let cand = if let Some(rest) = trimmed.strip_prefix("~/") {
            resolve_user_home()?.join(rest)
        } else {
            let p = Path::new(trimmed);
            if !p.is_absolute() {
                return None;
            }
            p.to_path_buf()
        };
        let home = orbit_home_dir.as_ref()?;
        let canon_att = fs::canonicalize(home.join("attachments")).ok()?;
        let canon_cand = fs::canonicalize(&cand).ok()?;
        canon_cand.starts_with(&canon_att).then_some(canon_cand)
    })();

    let canonical = if let Some(att_path) = attachment_direct {
        att_path
    } else {
        let resolved_cwd = if cwd.trim().is_empty() || cwd.trim() == "~" {
            resolve_user_home().unwrap_or_else(|| PathBuf::from("."))
        } else if let Some(rest) = cwd.trim().strip_prefix("~/") {
            resolve_user_home()
                .map(|h| h.join(rest))
                .unwrap_or_else(|| PathBuf::from(cwd.trim()))
        } else {
            PathBuf::from(cwd.trim())
        };
        let canonical_cwd = fs::canonicalize(&resolved_cwd)
            .map_err(|e| format!("Workspace directory unavailable: {e}"))?;

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

        let canon =
            fs::canonicalize(&candidate).map_err(|e| format!("Cannot read `{trimmed}`: {e}"))?;
        let mut allowed = canon.starts_with(&canonical_cwd);
        if !allowed
            && let Some(home) = orbit_home_dir.as_ref()
            && let Ok(canon_att) = fs::canonicalize(home.join("attachments"))
            && canon.starts_with(&canon_att)
        {
            allowed = true;
        }
        if !allowed {
            return Err(format!(
                "Access denied: `{trimmed}` is outside the workspace directory"
            ));
        }
        canon
    };

    let meta = fs::metadata(&canonical).map_err(|e| format!("Cannot read `{trimmed}`: {e}"))?;
    if !meta.is_file() {
        return Err(format!("`{trimmed}` is not a regular file"));
    }

    let size_bytes = meta.len();
    let is_image = local_image_mime_for_path(&canonical).is_some();
    let cap = if is_image {
        max_bytes
            .unwrap_or(LOCAL_IMAGE_MAX_READ_BYTES)
            .clamp(4096, LOCAL_IMAGE_MAX_READ_BYTES)
    } else {
        max_bytes
            .unwrap_or(2 * 1024 * 1024)
            .clamp(4096, 4 * 1024 * 1024)
    };
    let resolved_path = canonical.to_string_lossy().to_string();

    if let Some(mime) = local_image_mime_for_path(&canonical) {
        if size_bytes > cap {
            return Ok(LocalFileReadPayload {
                resolved_path,
                content_kind: "binary".into(),
                mime_type: Some(mime.into()),
                content: String::new(),
                size_bytes,
                truncated: true,
            });
        }
        let img_file = fs::File::open(&canonical)
            .map_err(|e| format!("Failed opening image `{trimmed}`: {e}"))?;
        let mut bytes = Vec::with_capacity((size_bytes as usize).min(cap as usize));
        img_file
            .take(cap.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Failed reading image `{trimmed}`: {e}"))?;
        if (bytes.len() as u64) > cap {
            return Ok(LocalFileReadPayload {
                resolved_path,
                content_kind: "binary".into(),
                mime_type: Some(mime.into()),
                content: String::new(),
                size_bytes: bytes.len() as u64,
                truncated: true,
            });
        }
        return Ok(LocalFileReadPayload {
            resolved_path,
            content_kind: "image".into(),
            mime_type: Some(mime.into()),
            content: B64.encode(&bytes),
            size_bytes: bytes.len() as u64,
            truncated: false,
        });
    }

    let file =
        fs::File::open(&canonical).map_err(|e| format!("Failed opening `{trimmed}`: {e}"))?;
    let mut buf = Vec::with_capacity((cap as usize).min(size_bytes as usize));
    file.take(cap)
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed reading `{trimmed}`: {e}"))?;

    let truncated = size_bytes > cap;
    let probe_len = buf.len().min(8192);
    if buf[..probe_len].contains(&0) {
        return Ok(LocalFileReadPayload {
            resolved_path,
            content_kind: "binary".into(),
            mime_type: None,
            content: String::new(),
            size_bytes,
            truncated,
        });
    }

    let text = match String::from_utf8(buf) {
        Ok(s) => s,
        Err(err) => {
            let is_incomplete_tail = err.utf8_error().error_len().is_none();
            let valid_up_to = err.utf8_error().valid_up_to();
            let raw = err.into_bytes();
            if truncated && is_incomplete_tail {
                String::from_utf8_lossy(&raw[..valid_up_to]).into_owned()
            } else {
                return Ok(LocalFileReadPayload {
                    resolved_path,
                    content_kind: "binary".into(),
                    mime_type: None,
                    content: String::new(),
                    size_bytes,
                    truncated,
                });
            }
        }
    };

    Ok(LocalFileReadPayload {
        resolved_path,
        content_kind: "text".into(),
        mime_type: Some("text/plain".into()),
        content: text,
        size_bytes,
        truncated,
    })
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
}
