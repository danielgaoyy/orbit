use std::fs::{self, File};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceRole {
    Daemon,
    Relay,
}

impl ServiceRole {
    pub fn unit_name(self) -> &'static str {
        match self {
            Self::Daemon => "orbit-daemon",
            Self::Relay => "orbit-relay",
        }
    }

    pub fn launchd_label(self) -> &'static str {
        match self {
            Self::Daemon => "dev.orbit.daemon",
            Self::Relay => "dev.orbit.relay",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Daemon => "Orbit Local AI Coding Agent Daemon",
            Self::Relay => "Orbit Zero-Knowledge E2EE Relay",
        }
    }

    pub fn subcommand_arg(self) -> &'static str {
        match self {
            Self::Daemon => "daemon",
            Self::Relay => "relay",
        }
    }

    pub fn pid_file(self, home_dir: &Path) -> PathBuf {
        home_dir.join(format!("{}.pid", self.subcommand_arg()))
    }

    pub fn log_file(self, home_dir: &Path) -> PathBuf {
        home_dir
            .join("logs")
            .join(format!("{}.log", self.subcommand_arg()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayProbeOutcome {
    Authorized,
    Forbidden403,
    Unreachable(String),
}

/// Normalize user-entered relay address (`host:port`, `ip`, `http://...`, `ws://...`, `wss://...`, or `ssh+relay://...`)
/// into a canonical Relay URL.
pub fn normalize_relay_url(raw: &str) -> anyhow::Result<String> {
    orbit_relay_transport::normalize_relay_url(raw).map_err(|e| anyhow::anyhow!(e))
}

/// Perform a fast 6-second read-only HTTP preflight check against `/v1/daemon/probe`
/// (supports both direct `ws(s)://` and tunneled `ssh+relay://`).
///
/// Never opens `/v1/daemon/control` and never mutates host registrations, ensuring a probe
/// can never displace an active daemon session.
pub async fn preflight_probe_relay(relay_base: &str, pubkey: &str) -> RelayProbeOutcome {
    let endpoint = match orbit_relay_transport::parse_relay_endpoint(relay_base) {
        Ok(ep) => ep,
        Err(e) => return RelayProbeOutcome::Unreachable(e.to_string()),
    };
    let path_and_query = format!("/v1/daemon/probe?pubkey={pubkey}");

    let probe_res: Result<(u16, String), String> = match endpoint {
        orbit_relay_transport::RelayEndpoint::DirectWs(base_url) => {
            let http_base = if let Some(rest) = base_url.strip_prefix("wss://") {
                format!("https://{rest}")
            } else if let Some(rest) = base_url.strip_prefix("ws://") {
                format!("http://{rest}")
            } else {
                base_url
            };
            let url = format!("{}{}", http_base.trim_end_matches('/'), path_and_query);
            match reqwest::Client::builder()
                .timeout(Duration::from_secs(6))
                .build()
            {
                Ok(client) => match client.get(&url).send().await {
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        let body = resp.text().await.unwrap_or_default();
                        Ok((status, body))
                    }
                    Err(e) => Err(e.to_string()),
                },
                Err(e) => Err(e.to_string()),
            }
        }
        orbit_relay_transport::RelayEndpoint::SshTunnel(spec) => {
            match tokio::time::timeout(
                Duration::from_secs(6),
                orbit_relay_transport::http_get_over_ssh_relay(&spec, &path_and_query),
            )
            .await
            {
                Ok(Ok(res)) => Ok(res),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_) => Err(
                    "connection timed out after 6s (check SSH connectivity / relay port)"
                        .to_string(),
                ),
            }
        }
    };

    match probe_res {
        Ok((200, body)) if body.trim() == orbit_relay::RELAY_PROBE_OK_BODY => {
            RelayProbeOutcome::Authorized
        }
        Ok((403, _)) => RelayProbeOutcome::Forbidden403,
        Ok((400, _)) => RelayProbeOutcome::Unreachable(
            "relay rejected malformed host public key (HTTP 400)".to_string(),
        ),
        Ok((status, _)) => RelayProbeOutcome::Unreachable(format!(
            "unexpected response from Orbit Relay probe endpoint (HTTP {status})"
        )),
        Err(msg) => RelayProbeOutcome::Unreachable(msg),
    }
}

/// Best-effort public IPv4 auto-detection for `orbit relay up` onboarding hints.
pub async fn detect_public_ipv4() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(1800))
        .build()
        .ok()?;
    for url in ["https://checkip.amazonaws.com", "https://api.ipify.org"] {
        if let Ok(resp) = client.get(url).send().await
            && resp.status().is_success()
            && let Ok(txt) = resp.text().await
        {
            let trimmed = txt.trim();
            if trimmed.parse::<Ipv4Addr>().is_ok() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortOwnership {
    Mine,
    Free,
    OccupiedByOther,
}

/// Check whether local HTTP `/healthz` responds `200 OK` on the given port (used for Relay).
pub async fn check_local_health(port: u16) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(400))
        .build()
    else {
        return false;
    };
    let url = format!("http://127.0.0.1:{port}/healthz");
    matches!(client.get(url).send().await, Ok(r) if r.status().is_success())
}

/// Verify whether `127.0.0.1:{port}` is free, owned by `expected_host_id` (`Mine`), or held by another user/process (`OccupiedByOther`).
pub async fn probe_daemon_port(port: u16, expected_host_id: &str) -> PortOwnership {
    if let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(350))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        let url = format!("http://127.0.0.1:{port}/healthz");
        if let Ok(resp) = client.get(url).send().await {
            if resp.status().is_success()
                && let Ok(body) = resp.text().await
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(&body)
                && let Some(hid) = json.get("host_id").and_then(|v| v.as_str())
                && hid == expected_host_id
            {
                return PortOwnership::Mine;
            }
            // Any other HTTP response on this port belongs to another service or daemon;
            // never fall back to sending a bearer token over WebSocket to an unverified service.
            return PortOwnership::OccupiedByOther;
        }
    }

    if TcpListener::bind(("0.0.0.0", port)).is_ok()
        && TcpListener::bind(("127.0.0.1", port)).is_ok()
    {
        PortOwnership::Free
    } else {
        PortOwnership::OccupiedByOther
    }
}

/// Automatically select an isolated, conflict-free port for `expected_host_id` on multi-user shared servers.
/// - Reuses an already-running daemon owned by `expected_host_id` (`Mine`) if present!
/// - Keeps `preferred` (e.g. `6868`) if free or already `Mine`.
/// - Automatically allocates `6869..=6999` if `preferred` is occupied by another Linux user's daemon!
pub async fn resolve_isolated_daemon_listen(
    preferred: SocketAddr,
    expected_host_id: &str,
) -> anyhow::Result<SocketAddr> {
    match probe_daemon_port(preferred.port(), expected_host_id).await {
        PortOwnership::Mine | PortOwnership::Free => return Ok(preferred),
        PortOwnership::OccupiedByOther => {}
    }

    // Pass 1: Check if this user already has a live daemon on one of the isolated ports (`6868..=6999`)
    for candidate in 6868u16..=6999 {
        if candidate == preferred.port() {
            continue;
        }
        // Fast TCP check before HTTP probe so scanning 130 ports takes < 3ms!
        if std::net::TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], candidate)),
            Duration::from_millis(15),
        )
        .is_ok()
            && probe_daemon_port(candidate, expected_host_id).await == PortOwnership::Mine
        {
            return Ok(SocketAddr::new(preferred.ip(), candidate));
        }
    }

    // Pass 2: Pick the first completely free port in `6868..=6999`
    for candidate in 6868u16..=6999 {
        if candidate == preferred.port() {
            continue;
        }
        if TcpListener::bind((preferred.ip(), candidate)).is_ok()
            && TcpListener::bind(("127.0.0.1", candidate)).is_ok()
        {
            return Ok(SocketAddr::new(preferred.ip(), candidate));
        }
    }

    anyhow::bail!("no free isolated daemon port available in 6868..=6999")
}

/// Find the live port currently serving `expected_host_id`, falling back to `fallback_port`.
pub async fn find_live_daemon_port_for_host(expected_host_id: &str, fallback_port: u16) -> u16 {
    if probe_daemon_port(fallback_port, expected_host_id).await == PortOwnership::Mine {
        return fallback_port;
    }
    for candidate in 6868u16..=6999 {
        if candidate == fallback_port {
            continue;
        }
        if std::net::TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], candidate)),
            Duration::from_millis(15),
        )
        .is_ok()
            && probe_daemon_port(candidate, expected_host_id).await == PortOwnership::Mine
        {
            return candidate;
        }
    }
    fallback_port
}

pub async fn wait_until_healthy_for_host(
    port: u16,
    expected_host_id: Option<&str>,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let ok = match expected_host_id {
            Some(hid) => probe_daemon_port(port, hid).await == PortOwnership::Mine,
            None => check_local_health(port).await,
        };
        if ok {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    match expected_host_id {
        Some(hid) => probe_daemon_port(port, hid).await == PortOwnership::Mine,
        None => check_local_health(port).await,
    }
}

fn resolve_user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

fn is_default_orbit_home(home_dir: &Path) -> bool {
    let Some(h) = resolve_user_home() else {
        return false;
    };
    let default_dir = h.join(".orbit");
    fs::canonicalize(home_dir).unwrap_or_else(|_| home_dir.to_path_buf())
        == fs::canonicalize(&default_dir).unwrap_or(default_dir)
}

/// Install & start (or restart) a persistent background service for `daemon` or `relay`.
pub async fn install_and_start_service(
    role: ServiceRole,
    home_dir: &Path,
    expected_port: u16,
    expected_host_id: Option<&str>,
) -> anyhow::Result<String> {
    fs::create_dir_all(home_dir.join("logs"))?;
    let exe = resolve_self_exe()?;
    let service_path_env = build_service_path_env();
    let use_os_supervisor = is_default_orbit_home(home_dir);

    let manager_label = if use_os_supervisor && cfg!(target_os = "linux") && has_user_systemd() {
        ensure_linux_user_linger_best_effort();
        install_systemd_user_service(role, &exe, home_dir, &service_path_env)?;
        format!("systemd --user ({}.service)", role.unit_name())
    } else if use_os_supervisor && cfg!(target_os = "macos") && has_launchctl() {
        install_macos_launch_agent(role, &exe, home_dir, &service_path_env)?;
        format!("launchd ({})", role.launchd_label())
    } else {
        start_detached_pid_service(role, &exe, home_dir, &service_path_env)?;
        format!("background daemon ({})", role.pid_file(home_dir).display())
    };

    if !wait_until_healthy_for_host(expected_port, expected_host_id, Duration::from_secs(4)).await {
        anyhow::bail!(
            "{} did not pass identity health check on 127.0.0.1:{expected_port} (check logs in {})",
            role.unit_name(),
            role.log_file(home_dir).display()
        );
    }

    Ok(manager_label)
}

/// Stop a background `daemon` or `relay` service if running.
pub fn stop_service(role: ServiceRole, home_dir: &Path) -> anyhow::Result<()> {
    let use_os_supervisor = is_default_orbit_home(home_dir);
    if use_os_supervisor && cfg!(target_os = "linux") && has_user_systemd() {
        let unit = format!("{}.service", role.unit_name());
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", &unit])
            .status();
    }
    if use_os_supervisor
        && cfg!(target_os = "macos")
        && has_launchctl()
        && let Some(plist) = macos_plist_path(role)
        && plist.exists()
    {
        let _ = Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&plist)
            .status();
    }
    stop_pid_file_process(&role.pid_file(home_dir));
    Ok(())
}

fn resolve_self_exe() -> anyhow::Result<PathBuf> {
    let cur = std::env::current_exe()?;
    if let Some(user_home) = resolve_user_home() {
        let exe_name = if cfg!(windows) { "orbit.exe" } else { "orbit" };
        let stable_symlink = user_home.join(".local").join("bin").join(exe_name);
        if stable_symlink.exists()
            && fs::canonicalize(&stable_symlink).ok() == fs::canonicalize(&cur).ok()
        {
            return Ok(stable_symlink);
        }
    }
    Ok(cur)
}

fn build_service_path_env() -> String {
    let mut entries: Vec<PathBuf> = Vec::new();
    let mut push_unique = |p: PathBuf| {
        if !p.as_os_str().is_empty() && !entries.contains(&p) {
            entries.push(p);
        }
    };
    if let Some(home) = resolve_user_home() {
        for rel in [".local/bin", ".cargo/bin", ".npm-global/bin"] {
            push_unique(home.join(rel));
        }
        #[cfg(windows)]
        push_unique(home.join("AppData").join("Roaming").join("npm"));
    }
    #[cfg(unix)]
    for fixed in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        push_unique(PathBuf::from(fixed));
    }
    if let Some(current) = std::env::var_os("PATH") {
        for part in std::env::split_paths(&current) {
            push_unique(part);
        }
    }
    std::env::join_paths(&entries)
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|_| std::env::var("PATH").unwrap_or_default())
}

fn has_user_systemd() -> bool {
    Command::new("systemctl")
        .args(["--user", "show-environment"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Ensure Linux `systemd --user` services keep running after closing SSH session (e.g. AWS EC2).
fn ensure_linux_user_linger_best_effort() {
    let Ok(user) = std::env::var("USER") else {
        return;
    };
    let is_lingering = Command::new("loginctl")
        .args(["show-user", &user, "-p", "Linger", "--value"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "yes")
        .unwrap_or(false);
    if is_lingering {
        return;
    }

    let direct_ok = Command::new("loginctl")
        .args(["enable-linger", &user])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !direct_ok {
        let _ = Command::new("sudo")
            .args(["-n", "loginctl", "enable-linger", &user])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn render_systemd_user_unit(
    role: ServiceRole,
    exe: &Path,
    home_dir: &Path,
    path_env: &str,
) -> String {
    let log_file = role.log_file(home_dir);
    // Keep detached `orbit shim` agent sessions alive across `orbit-daemon` restarts/upgrades.
    let kill_mode_line = if role == ServiceRole::Daemon {
        "KillMode=process\n"
    } else {
        ""
    };
    format!(
        "[Unit]\n\
         Description={desc}\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         {kill_mode_line}\
         ExecStart=\"{exe}\" --home \"{home}\" {sub}\n\
         Restart=always\n\
         RestartSec=2\n\
         StandardOutput=append:{log}\n\
         StandardError=append:{log}\n\
         Environment=RUST_LOG=info\n\
         Environment=PATH={path_env}\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        desc = role.description(),
        exe = exe.display(),
        home = home_dir.display(),
        sub = role.subcommand_arg(),
        log = log_file.display(),
    )
}

fn install_systemd_user_service(
    role: ServiceRole,
    exe: &Path,
    home_dir: &Path,
    path_env: &str,
) -> anyhow::Result<()> {
    let user_home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let systemd_dir = user_home.join(".config/systemd/user");
    fs::create_dir_all(&systemd_dir)?;

    let unit_file = systemd_dir.join(format!("{}.service", role.unit_name()));
    let content = render_systemd_user_unit(role, exe, home_dir, path_env);
    fs::write(&unit_file, content)?;

    let unit_name = format!("{}.service", role.unit_name());
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    let _ = Command::new("systemctl")
        .args(["--user", "enable", &unit_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let status = Command::new("systemctl")
        .args(["--user", "restart", &unit_name])
        .status()?;
    if !status.success() {
        anyhow::bail!("systemctl --user restart {unit_name} failed");
    }
    Ok(())
}

fn has_launchctl() -> bool {
    Command::new("launchctl")
        .arg("help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn macos_plist_path(role: ServiceRole) -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", role.launchd_label()))
    })
}

fn render_macos_launch_agent_plist(
    role: ServiceRole,
    exe: &Path,
    home_dir: &Path,
    path_env: &str,
) -> String {
    let log_file = role.log_file(home_dir);
    // Detached `orbit shim` processes already run in their own process group (`process_group(0)`),
    // so launchd must NOT set `AbandonProcessGroup` (which would disable cleanup of unrelated
    // daemon children sharing the job's process group).
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
           <key>Label</key>\n\
           <string>{label}</string>\n\
           <key>ProgramArguments</key>\n\
           <array>\n\
             <string>{exe}</string>\n\
             <string>--home</string>\n\
             <string>{home}</string>\n\
             <string>{sub}</string>\n\
           </array>\n\
           <key>EnvironmentVariables</key>\n\
           <dict>\n\
             <key>RUST_LOG</key>\n\
             <string>info</string>\n\
             <key>PATH</key>\n\
             <string>{path_env}</string>\n\
           </dict>\n\
           <key>RunAtLoad</key>\n\
           <true/>\n\
           <key>KeepAlive</key>\n\
           <true/>\n\
           <key>StandardOutPath</key>\n\
           <string>{log}</string>\n\
           <key>StandardErrorPath</key>\n\
           <string>{log}</string>\n\
         </dict>\n\
         </plist>\n",
        label = role.launchd_label(),
        exe = exe.display(),
        home = home_dir.display(),
        sub = role.subcommand_arg(),
        log = log_file.display(),
    )
}

fn install_macos_launch_agent(
    role: ServiceRole,
    exe: &Path,
    home_dir: &Path,
    path_env: &str,
) -> anyhow::Result<()> {
    let plist_path =
        macos_plist_path(role).ok_or_else(|| anyhow::anyhow!("HOME is not set for LaunchAgent"))?;
    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let plist = render_macos_launch_agent_plist(role, exe, home_dir, path_env);
    fs::write(&plist_path, plist)?;

    let _ = Command::new("launchctl")
        .args(["unload", "-w"])
        .arg(&plist_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let status = Command::new("launchctl")
        .args(["load", "-w"])
        .arg(&plist_path)
        .status()?;
    if !status.success() {
        anyhow::bail!("launchctl load failed for {}", plist_path.display());
    }
    Ok(())
}

fn start_detached_pid_service(
    role: ServiceRole,
    exe: &Path,
    home_dir: &Path,
    path_env: &str,
) -> anyhow::Result<()> {
    let pid_file = role.pid_file(home_dir);
    stop_pid_file_process(&pid_file);

    let log_path = role.log_file(home_dir);
    let out_file = File::options().create(true).append(true).open(&log_path)?;
    let err_file = out_file.try_clone()?;

    let mut cmd = Command::new(exe);
    cmd.arg("--home")
        .arg(home_dir)
        .arg(role.subcommand_arg())
        .env("RUST_LOG", "info")
        .env("PATH", path_env)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0200 | 0x0800_0000);
    }

    let child = cmd.spawn()?;
    fs::write(&pid_file, child.id().to_string())?;
    Ok(())
}

fn stop_pid_file_process(pid_file: &Path) {
    if let Ok(raw) = fs::read_to_string(pid_file) {
        let pid = raw.trim();
        if let Ok(pid_num) = pid.parse::<u32>()
            && pid_num > 1
        {
            #[cfg(unix)]
            {
                let _ = Command::new("kill")
                    .args(["-s", "TERM", "--", &pid_num.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                // Terminate ONLY the daemon/relay PID (NEVER `/T`) so detached `orbit shim`
                // agent and PTY child sessions survive daemon restarts and hot-upgrades!
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid_num.to_string(), "/F"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .creation_flags(0x0800_0000)
                    .status();
            }
        }
        let _ = fs::remove_file(pid_file);
    }
}

fn replace_executable_safely(tmp: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if let Some(parent) = dst.parent()
            && let Some(stem) = dst.file_stem().and_then(|s| s.to_str())
        {
            let stale_prefix = format!("{stem}.exe.old.");
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if name.to_string_lossy().starts_with(&stale_prefix) {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
        let backup = if dst.exists() {
            let b = dst.with_extension(format!("exe.old.{}", std::process::id()));
            let _ = fs::remove_file(&b);
            fs::rename(dst, &b)?;
            Some(b)
        } else {
            None
        };
        if let Err(err) = fs::rename(tmp, dst) {
            if let Some(ref b) = backup {
                let _ = fs::rename(b, dst);
            }
            return Err(err);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(tmp, dst)
    }
}

pub fn default_daemon_listen() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 6868))
}

fn resolve_github_token() -> Option<String> {
    for key in ["ORBIT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(val) = std::env::var(key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn extract_tag_from_release_url(url: &str) -> Option<String> {
    let clean = url.split(['?', '#']).next()?.trim().trim_end_matches('/');
    let (_, after_tag) = clean.rsplit_once("/releases/tag/")?;
    let tag = after_tag.split('/').next()?.trim();
    if tag.is_empty() {
        return None;
    }
    Some(tag.to_string())
}

async fn resolve_latest_release_tag(current_ver: &str) -> anyhow::Result<String> {
    let repo = orbit_protocol::configured_github_repo();
    let latest_web_url = format!("https://github.com/{repo}/releases/latest");
    let latest_api_url = format!("https://api.github.com/repos/{repo}/releases/latest");

    // 1. Primary: lightweight no-redirect web HEAD/GET -> 302 Location (`/releases/tag/vX.Y.Z`).
    //    Never consumes `api.github.com` unauthenticated 60 req/hr quota.
    let no_redirect = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("orbit-cli/{current_ver}"))
        .build()?;

    let mut web_err: Option<String> = None;
    for use_head in [true, false] {
        let req = if use_head {
            no_redirect.head(&latest_web_url)
        } else {
            no_redirect.get(&latest_web_url)
        };
        match req.send().await {
            Ok(resp) => {
                if resp.status().is_redirection()
                    && let Some(loc) = resp
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|v| v.to_str().ok())
                    && let Some(tag) = extract_tag_from_release_url(loc)
                {
                    return Ok(tag);
                }
                if let Some(tag) = extract_tag_from_release_url(resp.url().as_str()) {
                    return Ok(tag);
                }
                if !use_head {
                    web_err = Some(format!(
                        "HTTP {} without release tag redirect",
                        resp.status()
                    ));
                }
            }
            Err(e) if !use_head => {
                web_err = Some(e.to_string());
            }
            Err(_) => {}
        }
    }

    // 2. Fallback: GitHub REST API (automatically authenticated if token env var is present).
    let api_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!("orbit-cli/{current_ver}"))
        .build()?;
    let mut req = api_client
        .get(&latest_api_url)
        .header("Accept", "application/vnd.github+json");
    if let Some(token) = resolve_github_token() {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await.map_err(|e| {
        anyhow::anyhow!(
            "failed to resolve latest Orbit release (web redirect: {}; API: {e})",
            web_err.as_deref().unwrap_or("unknown")
        )
    })?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "failed to resolve latest Orbit release (web redirect: {}; API HTTP {})",
            web_err.as_deref().unwrap_or("unknown"),
            resp.status()
        );
    }
    let meta: serde_json::Value = resp.json().await?;
    meta.get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing tag_name in GitHub release metadata"))
}

async fn download_and_verify_release_asset(
    client: &reqwest::Client,
    release_base_url: &str,
    asset_name: &str,
) -> anyhow::Result<(impl AsRef<[u8]> + std::fmt::Debug + use<>, String)> {
    let base = release_base_url.trim_end_matches('/');
    let sums_url = format!("{base}/{}", orbit_crypto::SHA256SUMS_FILENAME);
    let download_url = format!("{base}/{asset_name}");

    let sums_resp = client
        .get(&sums_url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch release checksums {sums_url}: {e}"))?;
    if !sums_resp.status().is_success() {
        anyhow::bail!(
            "failed to fetch release checksums {sums_url} (HTTP {}); refusing unverified upgrade",
            sums_resp.status()
        );
    }
    let sums_text = sums_resp.text().await?;
    let expected_sha256 =
        orbit_crypto::find_expected_sha256(&sums_text, asset_name).map_err(|e| {
            anyhow::anyhow!("release checksum manifest {sums_url} rejected `{asset_name}`: {e}")
        })?;

    let bytes = client
        .get(&download_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let actual_sha256 = orbit_crypto::sha256_hex_bytes(bytes.as_ref());
    orbit_crypto::verify_sha256_hex(asset_name, &expected_sha256, &actual_sha256)
        .map_err(|e| anyhow::anyhow!("SHA-256 verification failed for {download_url}: {e}"))?;

    Ok((bytes, actual_sha256))
}

pub async fn self_upgrade_cli(
    home_dir: &Path,
    force: bool,
    upgrade_all_hosts: bool,
) -> anyhow::Result<()> {
    let current_ver = env!("CARGO_PKG_VERSION");
    let tag = resolve_latest_release_tag(current_ver).await?;
    let latest_ver = tag.trim_start_matches(['v', 'V']);

    if upgrade_all_hosts && let Ok(rt) = orbit_daemon::DaemonRuntime::bootstrap(home_dir).await {
        let self_host_id = rt.host_info.host_id.clone();
        let store = rt.store.clone();
        drop(rt);
        println!("Checking connected remote hosts for Orbit {tag}...");
        match orbit_daemon::connected_hosts::upgrade_all_saved_connected_hosts(
            &store,
            &self_host_id,
            latest_ver,
            force,
        )
        .await
        {
            Ok(reports) if reports.is_empty() => {
                println!("  (no connected remote hosts registered in this daemon's mesh)");
            }
            Ok(reports) => {
                for rep in reports {
                    match rep.outcome {
                        orbit_daemon::connected_hosts::RemotePeerUpgradeOutcome::Upgraded {
                            from_version,
                            target_version,
                        } => {
                            println!(
                                "  [✓] {}: upgraded v{from_version} -> v{target_version} (hot-restarting)",
                                rep.label
                            );
                        }
                        orbit_daemon::connected_hosts::RemotePeerUpgradeOutcome::AlreadyUpToDate {
                            version,
                        } => {
                            println!("  [=] {}: already on v{version}", rep.label);
                        }
                        orbit_daemon::connected_hosts::RemotePeerUpgradeOutcome::SkippedDisabled => {
                            println!("  [-] {}: disabled, skipped", rep.label);
                        }
                        orbit_daemon::connected_hosts::RemotePeerUpgradeOutcome::Failed(err) => {
                            println!("  [!] {}: upgrade failed: {err}", rep.label);
                        }
                    }
                }
            }
            Err(err) => {
                println!("  [!] Failed to inspect connected remote hosts: {err}");
            }
        }
    }

    if !force && latest_ver == current_ver {
        println!("[✓] Local Orbit is already at the latest version (v{current_ver}).");
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(format!("orbit-cli/{current_ver}"))
        .build()?;

    let asset_name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "orbit-cli-linux-x64",
        ("linux", "aarch64") => "orbit-cli-linux-arm64",
        ("macos", "aarch64") => "orbit-cli-macos-arm64",
        ("windows", "x86_64") => "orbit-windows-x86_64.exe",
        (os, arch) => anyhow::bail!("unsupported auto-upgrade platform: {os}/{arch}"),
    };

    let repo = orbit_protocol::configured_github_repo();
    let release_base_url = format!("https://github.com/{repo}/releases/download/{tag}");
    println!("Downloading & verifying Orbit {tag} ({asset_name}) from GitHub Releases...");
    let (bytes, verified_sha256) =
        download_and_verify_release_asset(&client, &release_base_url, asset_name).await?;
    println!("[✓] Verified SHA-256 checksum for {asset_name} ({verified_sha256})");

    let target_exe = fs::canonicalize(std::env::current_exe()?)?;
    let parent_dir = target_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve executable parent dir"))?;
    let tmp_path = parent_dir.join(format!(".orbit.upgrade.{}.tmp", std::process::id()));
    fs::write(&tmp_path, &bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755))?;
    }

    if let Err(err) = replace_executable_safely(&tmp_path, &target_exe) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err.into());
    }
    println!(
        "[✓] Upgraded Orbit v{current_ver} -> {tag} at {}",
        target_exe.display()
    );

    // Automatically restart active background daemon / relay if running on this host!
    if let Ok(rt) = orbit_daemon::DaemonRuntime::bootstrap(home_dir).await {
        let hid = rt.host_info.host_id.clone();
        drop(rt);
        let d_cfg = orbit_daemon::DaemonPersistedConfig::load(home_dir);
        let d_port = d_cfg
            .listen_addr
            .unwrap_or_else(default_daemon_listen)
            .port();
        let live_port = find_live_daemon_port_for_host(&hid, d_port).await;
        if probe_daemon_port(live_port, &hid).await == PortOwnership::Mine {
            let mgr =
                install_and_start_service(ServiceRole::Daemon, home_dir, live_port, Some(&hid))
                    .await?;
            println!(
                "[✓] Hot-restarted local Orbit Daemon ({mgr}) — active agent shims preserved."
            );
        }
    }

    let r_cfg = orbit_relay::RelayPersistedConfig::load(home_dir);
    if check_local_health(r_cfg.listen_addr.port()).await {
        let mgr =
            install_and_start_service(ServiceRole::Relay, home_dir, r_cfg.listen_addr.port(), None)
                .await?;
        println!("[✓] Restarted local Orbit Relay ({mgr}).");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_relay_url_variants() {
        assert_eq!(
            normalize_relay_url("203.0.113.10:7878").ok().as_deref(),
            Some("ws://203.0.113.10:7878")
        );
        assert_eq!(
            normalize_relay_url("203.0.113.10").ok().as_deref(),
            Some("ws://203.0.113.10:7878")
        );
        assert_eq!(
            normalize_relay_url("http://203.0.113.10:8080/")
                .ok()
                .as_deref(),
            Some("ws://203.0.113.10:8080")
        );
        assert_eq!(
            normalize_relay_url("https://relay.example.com/v1/daemon/control")
                .ok()
                .as_deref(),
            Some("wss://relay.example.com")
        );
        assert_eq!(
            normalize_relay_url("wss://relay.example.com:8443/")
                .ok()
                .as_deref(),
            Some("wss://relay.example.com:8443")
        );
        let key = orbit_relay_transport::generate_ssh_tunnel_key().expect("keygen");
        let ssh_uri = format!("ssh+relay://ec2-user@13.228.27.154/?key={}", key.seed_b64);
        assert_eq!(
            normalize_relay_url(&ssh_uri).ok().as_deref(),
            Some(
                format!(
                    "ssh+relay://ec2-user@13.228.27.154:22/7878?key={}",
                    key.seed_b64
                )
                .as_str()
            )
        );
    }

    #[tokio::test]
    async fn test_preflight_probe_authorized_vs_forbidden() {
        use orbit_crypto::HostIdentityKeypair;
        use orbit_relay::{RelayConfig, run_relay};
        use std::collections::HashSet;
        use std::sync::Arc;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);

        let allowed_kp = HostIdentityKeypair::generate().expect("allowed keygen");
        let stranger_kp = HostIdentityKeypair::generate().expect("stranger keygen");
        let mut allowed = HashSet::new();
        allowed.insert(allowed_kp.public_key_b64.clone());
        let cfg = RelayConfig {
            listen_addr: addr,
            allowed_pubkeys: Arc::new(allowed),
        };
        tokio::spawn(async move {
            let _ = run_relay(cfg).await;
        });
        tokio::time::sleep(Duration::from_millis(80)).await;

        let url = format!("ws://{addr}");
        assert_eq!(
            preflight_probe_relay(&url, &allowed_kp.public_key_b64).await,
            RelayProbeOutcome::Authorized
        );
        assert_eq!(
            preflight_probe_relay(&url, &stranger_kp.public_key_b64).await,
            RelayProbeOutcome::Forbidden403
        );

        // A non-Orbit HTTP server returning 200 without Orbit's probe marker must NOT report Authorized
        let fake_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake");
        let fake_addr = fake_listener.local_addr().expect("fake addr");
        let fake_task = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            if let Ok((mut stream, _)) = fake_listener.accept().await {
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot-orbit")
                    .await;
            }
        });
        assert!(matches!(
            preflight_probe_relay(&format!("ws://{fake_addr}"), &allowed_kp.public_key_b64).await,
            RelayProbeOutcome::Unreachable(_)
        ));
        fake_task.abort();
    }

    #[test]
    fn test_extract_tag_from_release_url() {
        assert_eq!(
            extract_tag_from_release_url(
                "https://github.com/danielgaoyy/orbit/releases/tag/v0.5.4"
            )
            .as_deref(),
            Some("v0.5.4")
        );
        assert_eq!(
            extract_tag_from_release_url("/danielgaoyy/orbit/releases/tag/v0.5.4/?ref=abc#top")
                .as_deref(),
            Some("v0.5.4")
        );
        assert_eq!(
            extract_tag_from_release_url("https://github.com/danielgaoyy/orbit/releases/latest"),
            None
        );
        assert_eq!(
            extract_tag_from_release_url("https://github.com/danielgaoyy/orbit/releases/tag/"),
            None
        );
    }

    #[test]
    fn default_daemon_listen_binds_loopback_only() {
        assert_eq!(
            default_daemon_listen(),
            SocketAddr::from(([127, 0, 0, 1], 6868))
        );
        assert!(default_daemon_listen().ip().is_loopback());
    }

    #[tokio::test]
    async fn probe_daemon_port_never_falls_back_to_ws_for_unidentified_http_service() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let ws_seen = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ws_seen_bg = ws_seen.clone();

        let srv = tokio::spawn(async move {
            for _ in 0..2 {
                let Ok((mut stream, _)) =
                    tokio::time::timeout(Duration::from_millis(500), listener.accept())
                        .await
                        .unwrap_or(Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "timeout",
                        )))
                else {
                    break;
                };
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                if req.contains("GET /ws") || req.to_ascii_lowercase().contains("authorization:") {
                    ws_seen_bg.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let ownership = probe_daemon_port(port, "orbit-expected-host").await;
        assert_eq!(ownership, PortOwnership::OccupiedByOther);
        let _ = srv.await;
        assert!(
            !ws_seen.load(std::sync::atomic::Ordering::SeqCst),
            "probe_daemon_port must never send a credentialed WS probe to an unidentified HTTP service"
        );
    }

    #[test]
    fn test_service_templates_systemd_kill_mode_and_clean_launchd_plist() {
        let exe = Path::new("/usr/local/bin/orbit");
        let home = Path::new("/Users/tester/.orbit");
        let path_env = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin";

        let daemon_plist =
            render_macos_launch_agent_plist(ServiceRole::Daemon, exe, home, path_env);
        assert!(
            !daemon_plist.contains("AbandonProcessGroup"),
            "daemon launchd plist must NOT set AbandonProcessGroup (shims already detach via process_group(0)):\n{daemon_plist}"
        );

        let daemon_unit = render_systemd_user_unit(ServiceRole::Daemon, exe, home, path_env);
        assert!(
            daemon_unit.contains("KillMode=process\n"),
            "daemon systemd unit must set KillMode=process:\n{daemon_unit}"
        );
    }

    #[tokio::test]
    async fn test_download_and_verify_release_asset_enforces_sha256sums() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let genuine: &'static [u8] = b"genuine-orbit-cli-binary-payload";
        let tampered: &'static [u8] = b"tampered-orbit-cli-binary-payload";
        let genuine_sha = orbit_crypto::sha256_hex_bytes(genuine);
        let valid_sums = format!("{genuine_sha}  orbit-cli-macos-arm64\n");
        let missing_entry_sums = format!("{genuine_sha}  other-binary\n");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let sums_ok = valid_sums.clone();
                let sums_missing = missing_entry_sums.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let Ok(n) = stream.read(&mut buf).await else {
                        return;
                    };
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (status, body): (&str, Vec<u8>) = match path {
                        "/ok/SHA256SUMS.txt" | "/tampered/SHA256SUMS.txt" => {
                            ("200 OK", sums_ok.into_bytes())
                        }
                        "/ok/orbit-cli-macos-arm64"
                        | "/no-sums/orbit-cli-macos-arm64"
                        | "/missing/orbit-cli-macos-arm64" => ("200 OK", genuine.to_vec()),
                        "/tampered/orbit-cli-macos-arm64" => ("200 OK", tampered.to_vec()),
                        "/missing/SHA256SUMS.txt" => ("200 OK", sums_missing.into_bytes()),
                        _ => ("404 Not Found", b"not found".to_vec()),
                    };
                    let header = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&body).await;
                });
            }
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");

        // 1. Valid SHA256SUMS.txt + matching binary -> succeeds
        let (bytes, sha) = download_and_verify_release_asset(
            &client,
            &format!("http://{addr}/ok"),
            "orbit-cli-macos-arm64",
        )
        .await
        .expect("valid sha256 must pass");
        assert_eq!(bytes.as_ref(), genuine);
        assert_eq!(sha, genuine_sha);

        // 2. Tampered binary -> fails with SHA-256 mismatch
        let err = download_and_verify_release_asset(
            &client,
            &format!("http://{addr}/tampered"),
            "orbit-cli-macos-arm64",
        )
        .await
        .expect_err("tampered binary must be rejected");
        assert!(
            err.to_string().contains("SHA-256"),
            "unexpected error: {err}"
        );

        // 3. Missing SHA256SUMS.txt -> fails cleanly
        let err = download_and_verify_release_asset(
            &client,
            &format!("http://{addr}/no-sums"),
            "orbit-cli-macos-arm64",
        )
        .await
        .expect_err("missing SHA256SUMS.txt must be rejected");
        assert!(
            err.to_string().contains("refusing unverified upgrade"),
            "unexpected error: {err}"
        );

        // 4. Missing asset entry in SHA256SUMS.txt -> fails cleanly
        let err = download_and_verify_release_asset(
            &client,
            &format!("http://{addr}/missing"),
            "orbit-cli-macos-arm64",
        )
        .await
        .expect_err("missing asset entry must be rejected");
        assert!(
            err.to_string().contains("not found in SHA256SUMS.txt"),
            "unexpected error: {err}"
        );

        server.abort();
    }
}
