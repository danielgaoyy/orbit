use std::fs::{self, File};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use orbit_protocol::{PROTOCOL_VERSION, WireMessage};
use tokio_tungstenite::tungstenite::{Error as WsError, Message as TungMsg};

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

/// Normalize user-entered relay address (`host:port`, `ip`, `http://...`, `ws://...`, `wss://...`)
/// into a clean base WebSocket URL (`ws://host:port` or `wss://host`).
pub fn normalize_relay_url(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        anyhow::bail!("relay URL cannot be empty");
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

    let (scheme, after_scheme) = with_scheme
        .split_once("://")
        .ok_or_else(|| anyhow::anyhow!("invalid relay URL: {raw}"))?;

    // Strip accidental path suffixes like `/ws` or `/v1/...`
    let authority = after_scheme
        .split('/')
        .next()
        .unwrap_or(after_scheme)
        .trim();
    if authority.is_empty() {
        anyhow::bail!("missing host in relay URL: {raw}");
    }

    // If user typed a bare IPv4 or `localhost` without `:port`, default to `:7878`
    let final_authority = if !had_scheme
        && !authority.contains(':')
        && (authority == "localhost" || authority.parse::<Ipv4Addr>().is_ok())
    {
        format!("{authority}:7878")
    } else {
        authority.to_string()
    };

    Ok(format!("{scheme}://{final_authority}"))
}

/// Perform a fast 4-second preflight WebSocket check against the Relay control endpoint.
pub async fn preflight_probe_relay(relay_base: &str, pubkey: &str) -> RelayProbeOutcome {
    let base = relay_base.trim_end_matches('/');
    let ctrl_url = format!("{base}/v1/daemon/control?pubkey={pubkey}");

    let res = tokio::time::timeout(
        Duration::from_secs(4),
        tokio_tungstenite::connect_async(&ctrl_url),
    )
    .await;

    match res {
        Ok(Ok((mut ws, _))) => {
            let _ = ws.close(None).await;
            RelayProbeOutcome::Authorized
        }
        Ok(Err(WsError::Http(resp))) if resp.status().as_u16() == 403 => {
            RelayProbeOutcome::Forbidden403
        }
        Ok(Err(e)) => {
            let msg = e.to_string();
            if msg.contains("403") {
                RelayProbeOutcome::Forbidden403
            } else {
                RelayProbeOutcome::Unreachable(msg)
            }
        }
        Err(_) => RelayProbeOutcome::Unreachable(
            "connection timed out after 4s (check port / cloud firewall / AWS Security Group)"
                .to_string(),
        ),
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
/// Works seamlessly across both `0.2.3+` JSON `/healthz` and `<= 0.2.2` legacy WebSocket `Welcome` handshakes.
pub async fn probe_daemon_port(port: u16, expected_host_id: &str) -> PortOwnership {
    if let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(350))
        .build()
    {
        let url = format!("http://127.0.0.1:{port}/healthz");
        if let Ok(resp) = client.get(url).send().await
            && resp.status().is_success()
            && let Ok(body) = resp.text().await
        {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body)
                && let Some(hid) = json.get("host_id").and_then(|v| v.as_str())
            {
                return if hid == expected_host_id {
                    PortOwnership::Mine
                } else {
                    PortOwnership::OccupiedByOther
                };
            }
            // Legacy <= 0.2.2 daemon returning plain `"ok"`: verify `host_id` via local WS `Welcome`
            return match probe_ws_welcome_host_id(port).await {
                Some(hid) if hid == expected_host_id => PortOwnership::Mine,
                _ => PortOwnership::OccupiedByOther,
            };
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

async fn probe_ws_welcome_host_id(port: u16) -> Option<String> {
    let url = format!("ws://127.0.0.1:{port}/ws");
    let (ws, _) = tokio::time::timeout(
        Duration::from_millis(450),
        tokio_tungstenite::connect_async(&url),
    )
    .await
    .ok()?
    .ok()?;
    let (mut sink, mut stream) = ws.split();
    let hello = WireMessage::Hello {
        client_id: "health-probe".into(),
        client_name: "orbit-cli-probe".into(),
        protocol_version: PROTOCOL_VERSION,
        supports_zstd: false,
    };
    let txt = serde_json::to_string(&hello).ok()?;
    sink.send(TungMsg::Text(txt.into())).await.ok()?;

    let msg = tokio::time::timeout(Duration::from_millis(450), stream.next())
        .await
        .ok()??
        .ok()?;
    match msg {
        TungMsg::Text(t) => match serde_json::from_str::<WireMessage>(&t).ok()? {
            WireMessage::Welcome { host } => Some(host.host_id),
            _ => None,
        },
        _ => None,
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

fn is_default_orbit_home(home_dir: &Path) -> bool {
    let Some(h) = std::env::var_os("HOME") else {
        return false;
    };
    let default_dir = PathBuf::from(h).join(".orbit");
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
    if let Some(user_home) = std::env::var_os("HOME") {
        let stable_symlink = PathBuf::from(user_home).join(".local/bin/orbit");
        if stable_symlink.exists()
            && fs::canonicalize(&stable_symlink).ok() == fs::canonicalize(&cur).ok()
        {
            return Ok(stable_symlink);
        }
    }
    Ok(cur)
}

fn build_service_path_env() -> String {
    let mut entries: Vec<String> = Vec::new();
    if let Some(h) = std::env::var_os("HOME") {
        let home = PathBuf::from(h);
        for rel in [".local/bin", ".cargo/bin", ".npm-global/bin"] {
            entries.push(home.join(rel).to_string_lossy().to_string());
        }
    }
    for fixed in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        if !entries.iter().any(|e| e == fixed) {
            entries.push(fixed.to_string());
        }
    }
    if let Ok(current) = std::env::var("PATH") {
        for part in current.split(':') {
            if !part.is_empty() && !entries.iter().any(|e| e == part) {
                entries.push(part.to_string());
            }
        }
    }
    entries.join(":")
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
    let log_file = role.log_file(home_dir);
    let content = format!(
        "[Unit]\n\
         Description={desc}\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
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
    );
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
    let log_file = role.log_file(home_dir);
    let plist = format!(
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
    );
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

    let child = cmd.spawn()?;
    fs::write(&pid_file, child.id().to_string())?;
    Ok(())
}

fn stop_pid_file_process(pid_file: &Path) {
    if let Ok(raw) = fs::read_to_string(pid_file) {
        let pid = raw.trim();
        if !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) {
            let _ = Command::new("kill")
                .arg(pid)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = fs::remove_file(pid_file);
    }
}

pub fn default_daemon_listen() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], 6868))
}

pub async fn self_upgrade_cli(home_dir: &Path, force: bool) -> anyhow::Result<()> {
    let current_ver = env!("CARGO_PKG_VERSION");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(format!("orbit-cli/{current_ver}"))
        .build()?;

    let meta: serde_json::Value = client
        .get("https://api.github.com/repos/danielgaoyy/orbit/releases/latest")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let tag = meta
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing tag_name in GitHub release metadata"))?;
    let latest_ver = tag.trim_start_matches('v');

    if !force && latest_ver == current_ver {
        println!("[✓] Orbit is already at the latest version (v{current_ver}).");
        return Ok(());
    }

    let asset_name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "orbit-cli-linux-x64",
        ("linux", "aarch64") => "orbit-cli-linux-arm64",
        ("macos", "aarch64") => "orbit-cli-macos-arm64",
        (os, arch) => anyhow::bail!("unsupported auto-upgrade platform: {os}/{arch}"),
    };

    let download_url =
        format!("https://github.com/danielgaoyy/orbit/releases/download/{tag}/{asset_name}");
    println!("Downloading Orbit {tag} ({asset_name}) from GitHub Releases...");
    let bytes = client
        .get(&download_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

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

    fs::rename(&tmp_path, &target_exe)?;
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
    }

    #[tokio::test]
    async fn test_preflight_probe_authorized_vs_forbidden() {
        use orbit_relay::{RelayConfig, run_relay};
        use std::collections::HashSet;
        use std::sync::Arc;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);

        let mut allowed = HashSet::new();
        allowed.insert("my_allowed_pubkey".to_string());
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
            preflight_probe_relay(&url, "my_allowed_pubkey").await,
            RelayProbeOutcome::Authorized
        );
        assert_eq!(
            preflight_probe_relay(&url, "stranger_pubkey").await,
            RelayProbeOutcome::Forbidden403
        );
    }
}
