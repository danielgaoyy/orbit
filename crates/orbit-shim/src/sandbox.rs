//! Build the same filesystem and egress boundary for shim and daemon-owned children.
use orbit_protocol::{SandboxMode, SandboxPolicy};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};
use tokio::process::Command;

#[derive(Debug, Serialize)]
pub struct SandboxProbeStatus {
    pub backend: &'static str,
    pub kernel_enforced: bool,
    pub available: bool,
}

pub fn probe_sandbox_support() -> SandboxProbeStatus {
    #[cfg(target_os = "macos")]
    let (backend, kernel_enforced, available) = (
        "sandbox-exec",
        true,
        std::process::Command::new("/usr/bin/sandbox-exec")
            .args(["-p", "(version 1)(allow default)", "--", "/usr/bin/true"])
            .output()
            .is_ok_and(|o| o.status.success()),
    );
    #[cfg(target_os = "linux")]
    let (backend, kernel_enforced, available) = (
        "bubblewrap",
        true,
        bwrap().is_some_and(|p| {
            std::process::Command::new(p)
                .args(["--ro-bind", "/", "/", "--unshare-net", "--", "/bin/true"])
                .output()
                .is_ok_and(|s| s.status.success())
        }),
    );
    #[cfg(windows)]
    let (backend, kernel_enforced, available) = ("environment and workspace scratch", false, true);
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let (backend, kernel_enforced, available) = ("unsupported", false, false);
    SandboxProbeStatus {
        backend,
        kernel_enforced: kernel_enforced && available,
        available,
    }
}

#[cfg(target_os = "linux")]
fn bwrap() -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    env::split_paths(&env::var_os("PATH")?)
        .filter(|d| d.is_absolute())
        .map(|d| d.join("bwrap"))
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}
fn invalid(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, text)
}

fn canonicalize_with_ancestor(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = path.canonicalize() {
        return Some(real);
    }
    let mut missing = Vec::new();
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if let Some(name) = cur.file_name() {
            missing.push(name.to_os_string());
        }
        if let Ok(mut real_parent) = parent.canonicalize() {
            for comp in missing.into_iter().rev() {
                real_parent.push(comp);
            }
            return Some(real_parent);
        }
        cur = parent;
    }
    None
}

pub fn denied_paths(policy: &SandboxPolicy) -> io::Result<BTreeSet<PathBuf>> {
    let home = PathBuf::from(
        env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .ok_or_else(|| invalid("user home unavailable"))?,
    );
    let mut paths = BTreeSet::new();
    for path in orbit_protocol::SANDBOX_CREDENTIAL_PATHS
        .iter()
        .copied()
        .chain(policy.denied_read_paths.iter().map(String::as_str))
        .map(|p| {
            p.strip_prefix("~/")
                .map_or_else(|| PathBuf::from(p), |suffix| home.join(suffix))
        })
    {
        if !path.is_absolute() {
            return Err(invalid("denied paths must be absolute"));
        }
        paths.insert(path.clone());
        if let Some(real) = canonicalize_with_ancestor(&path) {
            paths.insert(real);
        }
    }
    Ok(paths)
}
#[cfg(unix)]
fn writable(policy: &SandboxPolicy, cwd: &Path) -> io::Result<BTreeSet<PathBuf>> {
    let mut paths = BTreeSet::new();
    if policy.mode == SandboxMode::WorkspaceWrite {
        paths.insert(cwd.canonicalize()?);
        for p in &policy.writable_roots {
            if !Path::new(p).is_absolute() {
                return Err(invalid("writable roots must be absolute"));
            }
            paths.insert(fs::canonicalize(p)?);
        }
    }
    Ok(paths)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn sbpl(policy: &SandboxPolicy, cwd: &Path, port: Option<u16>) -> io::Result<String> {
    use std::fmt::Write;
    fn literal(path: &Path) -> io::Result<String> {
        let s = path
            .to_str()
            .filter(|s| !s.chars().any(char::is_control))
            .ok_or_else(|| invalid("invalid sandbox path"))?;
        Ok(format!(
            "\"{}\"",
            s.replace('\\', "\\\\").replace('"', "\\\"")
        ))
    }
    let mut p = String::from(
        "(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write-data (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/tty\") (literal \"/dev/dtracehelper\"))\n",
    );
    let mut roots = writable(policy, cwd)?;
    if policy.mode == SandboxMode::WorkspaceWrite {
        roots.insert(fs::canonicalize("/tmp")?);
        roots.insert(env::temp_dir().canonicalize()?);
    }
    for root in roots {
        let _ = writeln!(p, "(allow file-write* (subpath {}))", literal(&root)?);
    }
    for path in denied_paths(policy)? {
        let _ = writeln!(
            p,
            "(deny file-read* file-write* (subpath {}))",
            literal(&path)?
        );
    }
    if !policy.allow_network || port.is_some() {
        p.push_str("(deny network*)\n");
        if let Some(port) = port {
            let _ = writeln!(
                p,
                "(allow network-outbound (remote tcp \"localhost:{port}\"))"
            );
        }
    }
    Ok(p)
}

#[cfg(any(target_os = "linux", all(test, unix)))]
fn bubblewrap_args(
    policy: &SandboxPolicy,
    cwd: &Path,
    port: Option<u16>,
    bridge_socket: Option<&Path>,
    bridge_exe: Option<&Path>,
) -> io::Result<Vec<OsString>> {
    let mut args: Vec<OsString> = [
        "--die-with-parent",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--cap-drop",
        "ALL",
        "--ro-bind",
        "/",
        "/",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    if !policy.allow_network || port.is_some() {
        args.push("--unshare-net".into());
    }
    if cwd.starts_with("/tmp") && policy.mode == SandboxMode::StrictReadOnly {
        args.extend(["--ro-bind".into(), cwd.into(), cwd.into()]);
    }
    for path in writable(policy, cwd)? {
        args.extend(["--bind".into(), path.as_os_str().into(), path.into()]);
    }
    for path in denied_paths(policy)? {
        if path.is_dir() {
            args.extend([
                "--tmpfs".into(),
                path.clone().into(),
                "--remount-ro".into(),
                path.into(),
            ]);
        } else if path.exists() {
            args.extend(["--ro-bind".into(), "/dev/null".into(), path.into()]);
        }
    }
    if port.is_some() {
        let socket =
            bridge_socket.ok_or_else(|| invalid("missing sandbox egress bridge socket"))?;
        let exe = match bridge_exe {
            Some(p) => p.to_path_buf(),
            None => env::current_exe()?,
        };
        args.extend([
            "--ro-bind".into(),
            socket.as_os_str().into(),
            "/tmp/.orbit-egress.sock".into(),
            "--ro-bind".into(),
            exe.into(),
            "/tmp/.orbit-net-bridge".into(),
        ]);
    }
    if policy.mode == SandboxMode::StrictReadOnly {
        args.extend(["--remount-ro".into(), "/tmp".into()]);
    }
    args.extend(["--chdir".into(), cwd.into(), "--".into()]);
    if let Some(port) = port {
        args.extend([
            "/tmp/.orbit-net-bridge".into(),
            "sandbox-bridge".into(),
            port.to_string().into(),
            "--".into(),
        ]);
    }
    Ok(args)
}

pub struct SandboxCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
    pub remove: Vec<OsString>,
    pub profile: Option<tempfile::TempPath>,
}

impl SandboxCommand {
    pub fn prepare(
        program: &std::ffi::OsStr,
        args: Vec<OsString>,
        cwd: &Path,
        policy: Option<&SandboxPolicy>,
        proxy: Option<&str>,
    ) -> io::Result<Self> {
        Self::prepare_with_bridge(program, args, cwd, policy, proxy, None, None)
    }

    pub fn prepare_with_bridge(
        program: &std::ffi::OsStr,
        args: Vec<OsString>,
        cwd: &Path,
        policy: Option<&SandboxPolicy>,
        proxy: Option<&str>,
        bridge_socket: Option<&Path>,
        bridge_exe: Option<&Path>,
    ) -> io::Result<Self> {
        let _ = (bridge_socket, bridge_exe);
        let mut cmd = Self {
            program: program.into(),
            args,
            env: Vec::new(),
            remove: Vec::new(),
            profile: None,
        };
        let Some(policy) = policy.filter(|p| p.mode != SandboxMode::Off) else {
            return Ok(cmd);
        };
        if policy.allow_network
            && (!policy.allowed_domains.is_empty() || !policy.denied_domains.is_empty())
            && proxy.is_none()
        {
            return Err(invalid("domain policy requires an authenticated proxy"));
        }
        let port = proxy
            .map(|url| {
                url.rsplit_once("@127.0.0.1:")
                    .and_then(|(_, p)| p.parse::<u16>().ok())
                    .filter(|p| *p > 0)
                    .ok_or_else(|| invalid("invalid loopback proxy"))
            })
            .transpose()?;
        let env_proxy = proxy.or_else(|| (!policy.allow_network).then_some("http://127.0.0.1:1"));
        if let Some(url) = env_proxy {
            for key in ["http_proxy", "https_proxy", "all_proxy", "no_proxy"] {
                let value = if key == "no_proxy" { "" } else { url };
                for name in [key.into(), key.to_ascii_uppercase()] {
                    cmd.env.push((name, value.into()));
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            use std::io::Write;
            let mut file = tempfile::Builder::new()
                .prefix("orbit-sandbox-")
                .tempfile()?;
            file.write_all(sbpl(policy, cwd, port)?.as_bytes())?;
            let path = file.into_temp_path();
            cmd.args.splice(
                ..0,
                [
                    OsString::from("-f"),
                    path.as_os_str().into(),
                    "--".into(),
                    cmd.program.clone(),
                ],
            );
            cmd.program = "/usr/bin/sandbox-exec".into();
            cmd.profile = Some(path);
        }
        #[cfg(target_os = "linux")]
        {
            let bin = bwrap().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "sandbox requires bubblewrap (bwrap)",
                )
            })?;
            let mut prefix = bubblewrap_args(policy, cwd, port, bridge_socket, bridge_exe)?;
            prefix.push(cmd.program);
            prefix.append(&mut cmd.args);
            cmd.program = bin.into();
            cmd.args = prefix;
        }
        #[cfg(windows)]
        {
            let _ = port;
            let scratch = cwd.canonicalize()?.join(".orbit-scratch");
            fs::create_dir_all(&scratch)?;
            let scratch = scratch.canonicalize()?;
            if !scratch.starts_with(cwd.canonicalize()?) {
                return Err(invalid("scratch escapes workspace"));
            }
            for key in ["TEMP", "TMP"] {
                cmd.env.push((key.into(), scratch.to_string_lossy().into()));
            }
            cmd.remove.extend(
                env::vars_os()
                    .map(|(k, _)| k)
                    .filter(|k| sensitive_env(&k.to_string_lossy())),
            );
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
        return Err(invalid("unsupported sandbox backend"));
        Ok(cmd)
    }
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args);
        self.apply_env(&mut cmd);
        cmd
    }
    pub fn apply_env(&self, cmd: &mut Command) {
        #[cfg(windows)]
        if self.env.iter().any(|(k, _)| k == "TEMP") {
            let keys: Vec<_> = cmd
                .as_std()
                .get_envs()
                .filter_map(|(k, _)| sensitive_env(&k.to_string_lossy()).then_some(k.to_owned()))
                .collect();
            for k in keys {
                cmd.env_remove(k);
            }
        }
        for k in &self.remove {
            cmd.env_remove(k);
        }
        cmd.envs(self.env.iter().map(|(k, v)| (k, v)));
    }
}

#[cfg(windows)]
pub fn sensitive_env(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "API_KEY",
        "ACCESS_KEY",
        "CREDENTIAL",
        "SSH_AUTH_SOCK",
    ]
    .iter()
    .any(|s| k.contains(s))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn native_backend_enforces_read_and_write_rules() -> io::Result<()> {
        if !probe_sandbox_support().kernel_enforced {
            return Ok(());
        }
        let dir = tempfile::tempdir()?;
        let path = dir.path().canonicalize()?;
        let secret = path.join("private");
        fs::write(&secret, b"private fixture")?;
        let policy = SandboxPolicy {
            denied_read_paths: vec![secret.to_string_lossy().into()],
            ..Default::default()
        };
        let read = SandboxCommand::prepare(
            "/bin/cat".as_ref(),
            vec![secret.into()],
            &path,
            Some(&policy),
            None,
        )?;
        assert!(read.command().output().await?.stdout.is_empty());
        let touch = |name: &str, mode| {
            SandboxCommand::prepare(
                "/usr/bin/touch".as_ref(),
                vec![path.join(name).into()],
                &path,
                Some(&SandboxPolicy {
                    mode,
                    ..policy.clone()
                }),
                None,
            )
        };
        assert!(
            touch("ok", SandboxMode::WorkspaceWrite)?
                .command()
                .output()
                .await?
                .status
                .success()
        );
        assert!(
            !touch("blocked", SandboxMode::StrictReadOnly)?
                .command()
                .output()
                .await?
                .status
                .success()
        );
        let dev_null = SandboxCommand::prepare(
            "/bin/sh".as_ref(),
            vec!["-c".into(), "echo ok > /dev/null".into()],
            &path,
            Some(&SandboxPolicy {
                mode: SandboxMode::StrictReadOnly,
                ..policy.clone()
            }),
            None,
        )?;
        assert!(
            dev_null.command().output().await?.status.success(),
            "StrictReadOnly must allow writing data to /dev/null"
        );
        Ok(())
    }
    #[test]
    fn profiles_apply_mandatory_exclusions() -> io::Result<()> {
        let cwd = env::current_dir()?;
        let policy = SandboxPolicy::default();
        let mac = sbpl(&policy, &cwd, Some(32111))?;
        assert!(
            mac.contains("localhost:32111")
                && mac.contains("/.ssh")
                && mac.contains("(deny network*)")
                && mac.contains("(literal \"/dev/null\")")
        );
        let b = bubblewrap_args(&policy, &cwd, None, None, None)?;
        assert!(b.contains(&"--unshare-net".into()) && b.contains(&"--bind".into()));
        let b_net = bubblewrap_args(
            &policy,
            &cwd,
            Some(32111),
            Some(Path::new("/run/orbit/egress.sock")),
            Some(Path::new("/usr/local/bin/orbit")),
        )?;
        assert!(b_net.contains(&"/run/orbit/egress.sock".into()));
        assert!(b_net.contains(&"/usr/local/bin/orbit".into()));
        let strict = SandboxPolicy {
            mode: SandboxMode::StrictReadOnly,
            ..policy
        };
        assert!(!sbpl(&strict, &cwd, None)?.contains("(allow file-write*"));
        assert!(!bubblewrap_args(&strict, &cwd, None, None, None)?.contains(&"--bind".into()));

        // P0-5: Non-existent denied path under symlinked ancestor resolves canonical path too.
        let dir = tempfile::tempdir()?;
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(dir.path(), &link)?;
        let missing = link.join("nonexistent-secret.json");
        let p2 = SandboxPolicy {
            denied_read_paths: vec![missing.to_string_lossy().into()],
            ..Default::default()
        };
        let denied = denied_paths(&p2)?;
        assert!(denied.contains(&dir.path().canonicalize()?.join("nonexistent-secret.json")));
        Ok(())
    }
}
