use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHostEntry {
    pub alias: String,
    pub host_name: String,
    pub user: Option<String>,
    pub port: Option<u16>,
}

impl SshHostEntry {
    pub fn display_target(&self) -> String {
        let user_prefix = self
            .user
            .as_deref()
            .filter(|u| !u.is_empty())
            .map(|u| format!("{u}@"))
            .unwrap_or_default();
        let port_suffix = self
            .port
            .filter(|p| *p != 22)
            .map(|p| format!(":{p}"))
            .unwrap_or_default();
        format!("{user_prefix}{}{port_suffix}", self.host_name)
    }
}

/// Parse `~/.ssh/config` (including `Include` directives) and return concrete remote server entries.
pub fn discover_ssh_hosts() -> Vec<SshHostEntry> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let ssh_dir = home.join(".ssh");
    let root_cfg = ssh_dir.join("config");
    if !root_cfg.exists() {
        return Vec::new();
    }
    parse_ssh_config_file(&root_cfg, &ssh_dir, &home, 0)
}

#[cfg(test)]
pub fn parse_ssh_config_str(content: &str) -> Vec<SshHostEntry> {
    let dummy_home = PathBuf::from("/nonexistent");
    parse_ssh_config_lines(content, &dummy_home, &dummy_home, 3)
}

fn parse_ssh_config_file(
    path: &Path,
    ssh_dir: &Path,
    home: &Path,
    depth: usize,
) -> Vec<SshHostEntry> {
    if depth > 3 {
        return Vec::new();
    }
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_ssh_config_lines(&raw, ssh_dir, home, depth)
}

fn parse_ssh_config_lines(
    raw: &str,
    ssh_dir: &Path,
    home: &Path,
    depth: usize,
) -> Vec<SshHostEntry> {
    let mut results: Vec<SshHostEntry> = Vec::new();
    let mut current_aliases: Vec<String> = Vec::new();
    let mut cur_hostname: Option<String> = None;
    let mut cur_user: Option<String> = None;
    let mut cur_port: Option<u16> = None;

    let flush = |aliases: &mut Vec<String>,
                 hostname: &mut Option<String>,
                 user: &mut Option<String>,
                 port: &mut Option<u16>,
                 out: &mut Vec<SshHostEntry>| {
        for alias in aliases.drain(..) {
            let effective_host = hostname.clone().unwrap_or_else(|| alias.clone());
            if is_ignored_host(&alias, &effective_host) {
                continue;
            }
            if !out.iter().any(|x| x.alias == alias) {
                out.push(SshHostEntry {
                    alias,
                    host_name: effective_host,
                    user: user.clone(),
                    port: *port,
                });
            }
        }
        *hostname = None;
        *user = None;
        *port = None;
    };

    for line in raw.lines() {
        let trimmed = strip_inline_comment(line).trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((key_raw, val_raw)) = split_directive(trimmed) else {
            continue;
        };
        let key = key_raw.to_ascii_lowercase();
        let val = val_raw.trim().trim_matches('"');

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
                    for inc_path in expand_include_pattern(pat, ssh_dir, home) {
                        for entry in parse_ssh_config_file(&inc_path, ssh_dir, home, depth + 1) {
                            if !results.iter().any(|x| x.alias == entry.alias) {
                                results.push(entry);
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
                for token in val.split_whitespace() {
                    let clean = token.trim_matches('"');
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

fn strip_inline_comment(line: &str) -> &str {
    let mut in_quotes = false;
    for (idx, ch) in line.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == '#' && !in_quotes {
            return &line[..idx];
        }
    }
    line
}

fn split_directive(line: &str) -> Option<(&str, &str)> {
    let first_ws = line.find(char::is_whitespace);
    let first_eq = line.find('=');
    match (first_ws, first_eq) {
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

fn is_ignored_host(alias: &str, hostname: &str) -> bool {
    let lower_alias = alias.to_ascii_lowercase();
    let lower_host = hostname.to_ascii_lowercase();
    if lower_host == "localhost"
        || lower_host == "127.0.0.1"
        || lower_host == "::1"
        || lower_alias.contains("orbstack")
    {
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
        if lower_host == forge || lower_host.ends_with(&format!(".{forge}")) {
            return true;
        }
    }
    false
}

fn expand_include_pattern(pattern: &str, ssh_dir: &Path, home: &Path) -> Vec<PathBuf> {
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
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|name| wildcard_match(file_pat, name))
                    .unwrap_or(false)
        })
        .collect();
    paths.sort();
    paths
}

fn wildcard_match(pat: &str, text: &str) -> bool {
    if pat == "*" {
        return !text.starts_with('.');
    }
    if let Some(suffix) = pat.strip_prefix('*') {
        return text.ends_with(suffix);
    }
    if let Some(prefix) = pat.strip_suffix('*') {
        return text.starts_with(prefix);
    }
    pat == text
}

/// Remotely install/update Orbit on an SSH host from `~/.ssh/config` and start `orbit relay up`
/// locked to `allow_pubkey`, returning the resolved `ws://...` Relay URL.
pub fn provision_relay_via_ssh(
    entry: &SshHostEntry,
    relay_port: u16,
    allow_pubkey: &str,
) -> anyhow::Result<String> {
    let escaped_key = allow_pubkey.replace('\'', "");
    let remote_cmd = format!(
        "set -eu; \
         export PATH=\"$HOME/.local/bin:/usr/local/bin:$PATH\"; \
         if ! command -v orbit >/dev/null 2>&1 || ! orbit relay --help 2>&1 | grep -q 'allow'; then \
           curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh; \
         fi; \
         orbit relay up --port {relay_port} --allow-pubkey '{escaped_key}'"
    );

    let out = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=12",
            &entry.alias,
            &remote_cmd,
        ])
        .output()?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        anyhow::bail!(
            "SSH relay deployment on `{}` failed: {}",
            entry.alias,
            stderr.trim()
        );
    }

    for line in stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix("Relay URL")
            && let Some((_, url_part)) = rest.split_once(':')
        {
            let candidate = url_part.trim();
            if !candidate.is_empty() && !candidate.contains("<YOUR_SERVER_IP>") {
                return Ok(candidate.to_string());
            }
        }
    }

    Ok(format!("ws://{}:{relay_port}", entry.host_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ssh_config_hosts_and_filters() {
        let sample = r#"
# Global defaults
Host *
  ServerAliveInterval 30

Host github.com
  HostName github.com
  User git

Host devbox
  HostName 203.0.113.42
  User ec2-user
  IdentityFile ~/.ssh/devbox.pem

Host=mini
  HostName=198.51.100.7
  User=admin
  Port 2222
"#;
        let hosts = parse_ssh_config_str(sample);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].alias, "devbox");
        assert_eq!(hosts[0].host_name, "203.0.113.42");
        assert_eq!(hosts[0].user.as_deref(), Some("ec2-user"));
        assert_eq!(hosts[1].alias, "mini");
        assert_eq!(hosts[1].host_name, "198.51.100.7");
        assert_eq!(hosts[1].port, Some(2222));
    }
}
