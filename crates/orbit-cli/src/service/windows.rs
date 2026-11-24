//! Windows SCM and interactive-token Task Scheduler supervisors.
use super::ServiceRole;

fn quote(arg: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        if c == '"' {
            out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n('\\', slashes));
        }
        out.push(c);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn task_xml(exe: &str, home: &str, sid: &str) -> String {
    let args = xml(&format!("--home {} daemon", quote(home)));
    let exe = xml(exe);
    let sid = xml(sid);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
<Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{sid}</UserId></LogonTrigger></Triggers>
<Principals><Principal id="user"><UserId>{sid}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
<Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><RestartOnFailure><Interval>PT1M</Interval><Count>5</Count></RestartOnFailure></Settings>
<Actions Context="user"><Exec><Command>{exe}</Command><Arguments>{args}</Arguments></Exec></Actions></Task>"#
    )
}
fn service_args(role: ServiceRole, exe: &str, home: &str, exists: bool) -> Vec<String> {
    [
        if exists { "config" } else { "create" },
        role.unit_name(),
        "binPath=",
        &format!(
            "{} --home {} service-run {}",
            quote(exe),
            quote(home),
            role.subcommand_arg()
        ),
        "start=",
        "auto",
        "obj=",
        r"NT AUTHORITY\LocalService",
    ]
    .map(str::to_owned)
    .to_vec()
}
fn failure_args(role: ServiceRole) -> [&'static str; 6] {
    [
        "failure",
        role.unit_name(),
        "reset=",
        "86400",
        "actions=",
        "restart/5000/restart/15000/restart/60000",
    ]
}

#[cfg(windows)]
pub(super) mod native {
    use super::*;
    use std::{
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::OnceLock,
        time::Duration,
    };
    use windows_service::{
        define_windows_service,
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
        service_dispatcher,
    };
    static START: OnceLock<(ServiceRole, PathBuf)> = OnceLock::new();
    define_windows_service!(service_entry, service_main);

    fn tool(name: &str) -> PathBuf {
        PathBuf::from(
            std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows")),
        )
        .join("System32")
        .join(name)
    }
    fn run(cmd: &mut Command) -> anyhow::Result<()> {
        let res = cmd.output()?;
        anyhow::ensure!(
            res.status.success(),
            "Windows supervisor returned {}: {}",
            res.status,
            String::from_utf8_lossy(&res.stderr)
        );
        Ok(())
    }
    fn sid() -> anyhow::Result<String> {
        let result = Command::new(tool("whoami.exe"))
            .args(["/user", "/fo", "csv", "/nh"])
            .output()?;
        let text = String::from_utf8(result.stdout)?;
        let sid = text
            .trim()
            .rsplit(',')
            .next()
            .unwrap_or("")
            .trim_matches('"');
        anyhow::ensure!(
            sid.starts_with("S-1-") && sid[2..].chars().all(|c| c.is_ascii_digit() || c == '-'),
            "unable to determine current user SID"
        );
        Ok(sid.to_owned())
    }
    fn canonical_windows_path(path: &Path) -> std::io::Result<PathBuf> {
        let canon = path.canonicalize()?;
        let s = canon.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            Ok(PathBuf::from(format!(r"\\{rest}")))
        } else if let Some(rest) = s.strip_prefix(r"\\?\") {
            Ok(PathBuf::from(rest))
        } else {
            Ok(canon)
        }
    }
    fn task_name(sid: &str, home: &Path) -> String {
        use sha2::{Digest, Sha256};
        use std::fmt::Write as _;
        let norm = home.to_string_lossy().to_ascii_lowercase();
        let digest = Sha256::digest(norm.as_bytes());
        let mut short = String::with_capacity(16);
        for b in &digest[..8] {
            let _ = write!(short, "{b:02x}");
        }
        format!("Orbit-Daemon-{sid}-{short}")
    }
    fn marker(home: &Path, role: ServiceRole) -> PathBuf {
        home.join(format!("{}.supervisor", role.unit_name()))
    }
    pub fn install(
        role: ServiceRole,
        exe: &Path,
        home: &Path,
        service: bool,
    ) -> anyhow::Result<String> {
        let home = canonical_windows_path(home)?;
        let marker = marker(&home, role);
        let is_scm = role == ServiceRole::Relay
            || service
            || fs::read_to_string(&marker).is_ok_and(|s| s == "scm");
        if is_scm {
            let sc = tool("sc.exe");
            let name = role.unit_name();
            let exists = Command::new(&sc)
                .args(["query", name])
                .output()?
                .status
                .success();
            if exists {
                stop_scm(role)?;
            }
            run(Command::new(&sc).args(service_args(
                role,
                &exe.to_string_lossy(),
                &home.to_string_lossy(),
                exists,
            )))?;
            run(Command::new(&sc).args(["sidtype", name, "unrestricted"]))?;
            run(Command::new(tool("icacls.exe"))
                .arg(&home)
                .arg("/grant")
                .arg(format!(r"NT SERVICE\{name}:(OI)(CI)M")))?;
            run(Command::new(&sc).args(failure_args(role)))?;
            run(Command::new(&sc).args(["failureflag", name, "1"]))?;
            if fs::read_to_string(&marker).is_ok_and(|v| v == "task") {
                let name = task_name(&sid()?, &home);
                let sch = tool("schtasks.exe");
                let _ = Command::new(&sch).args(["/End", "/TN", &name]).output();
                run(Command::new(sch).args(["/Delete", "/F", "/TN", &name]))?;
            }
            fs::write(marker, "scm")?;
            run(Command::new(&sc).args(["start", name]))?;
            Ok(format!("Windows SCM ({name})"))
        } else {
            let sid = sid()?;
            let name = task_name(&sid, &home);
            let sch = tool("schtasks.exe");
            let task = task_xml(&exe.to_string_lossy(), &home.to_string_lossy(), &sid);
            let bytes: Vec<u8> = std::iter::once(0xfeff)
                .chain(task.encode_utf16())
                .flat_map(u16::to_le_bytes)
                .collect();
            let template = home.join(format!("task-{}.xml", uuid::Uuid::new_v4()));
            fs::write(&template, bytes)?;
            let res = run(Command::new(&sch)
                .args(["/Create", "/F", "/TN", &name, "/XML"])
                .arg(&template));
            let _ = fs::remove_file(template);
            res?;
            let _ = Command::new(&sch).args(["/End", "/TN", &name]).output();
            fs::write(marker, "task")?;
            run(Command::new(sch).args(["/Run", "/TN", &name]))?;
            Ok(format!("Task Scheduler ({name})"))
        }
    }
    fn stop_scm(role: ServiceRole) -> anyhow::Result<()> {
        use windows_service::{
            service::ServiceAccess,
            service_manager::{ServiceManager, ServiceManagerAccess},
        };
        let scm = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = scm.open_service(
            role.unit_name(),
            ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
        )?;
        let state = service.query_status()?.current_state;
        if state == ServiceState::Stopped {
            return Ok(());
        }
        if state != ServiceState::StopPending {
            service.stop()?;
        }
        for _ in 0..100 {
            if service.query_status()?.current_state == ServiceState::Stopped {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        anyhow::bail!("Windows service stop timed out")
    }
    pub fn stop(role: ServiceRole, home: &Path) -> anyhow::Result<()> {
        let home = canonical_windows_path(home)?;
        let file = marker(&home, role);
        match fs::read_to_string(&file) {
            Ok(v) if v == "scm" => {
                stop_scm(role)?;
                run(Command::new(tool("sc.exe")).args(["delete", role.unit_name()]))?;
            }
            Ok(v) if v == "task" => {
                let task = task_name(&sid()?, &home);
                let sch = tool("schtasks.exe");
                let _ = Command::new(&sch).args(["/End", "/TN", &task]).output();
                run(Command::new(sch).args(["/Delete", "/F", "/TN", &task]))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            _ => anyhow::bail!("unrecognized Windows supervisor"),
        }
        fs::remove_file(file)?;
        Ok(())
    }
    pub fn dispatch(role: ServiceRole, home: PathBuf) -> anyhow::Result<()> {
        START
            .set((role, home))
            .map_err(|_| anyhow::anyhow!("SCM already initialized"))?;
        service_dispatcher::start(role.unit_name(), service_entry)?;
        Ok(())
    }
    fn service_main(_: Vec<OsString>) {
        let _ = worker();
    }
    fn report_status(
        handler: ServiceStatusHandle,
        state: ServiceState,
        code: u32,
        wait_hint: Duration,
    ) -> windows_service::Result<()> {
        handler.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running {
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
            } else {
                ServiceControlAccept::empty()
            },
            exit_code: ServiceExitCode::Win32(code),
            checkpoint: 0,
            wait_hint,
            process_id: None,
        })
    }
    fn worker() -> anyhow::Result<()> {
        let (role, home) = START
            .get()
            .ok_or_else(|| anyhow::anyhow!("missing SCM context"))?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let mut tx = Some(tx);
        let handler = service_control_handler::register(role.unit_name(), move |c| match c {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                if let Some(tx) = tx.take() {
                    let _ = tx.send(());
                }
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        })?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .thread_stack_size(8 * 1024 * 1024)
            .enable_all()
            .build()?;
        report_status(handler, ServiceState::Running, 0, Duration::ZERO)?;
        let result = rt.block_on(async {
            let exporter = crate::setup_logging(home, *role == ServiceRole::Daemon)?;
            match role {
                ServiceRole::Daemon => {
                    let persisted = orbit_daemon::DaemonPersistedConfig::load(home);
                    let config = orbit_daemon::DaemonConfig {
                        home_dir: home.clone(),
                        listen_addr: persisted
                            .listen_addr
                            .unwrap_or_else(super::super::default_daemon_listen),
                        relay_url: persisted.relay_url,
                    };
                    orbit_daemon::run_daemon_until(config, exporter, async move {
                        let _ = rx.await;
                        let _ = report_status(
                            handler,
                            ServiceState::StopPending,
                            0,
                            Duration::from_secs(15),
                        );
                    })
                    .await
                }
                ServiceRole::Relay => {
                    let persisted = orbit_relay::RelayPersistedConfig::load(home);
                    let config = orbit_relay::RelayConfig {
                        listen_addr: persisted.listen_addr,
                        allowed_pubkeys: std::sync::Arc::new(
                            persisted.allowed_pubkeys.into_iter().collect(),
                        ),
                        fleet_token: persisted.fleet_token,
                    };
                    tokio::select! {
                        result = orbit_relay::run_relay(config) => result,
                        _ = rx => {
                            let _ = report_status(
                                handler,
                                ServiceState::StopPending,
                                0,
                                Duration::from_secs(15),
                            );
                            Ok(())
                        }
                    }
                }
            }
        });
        report_status(
            handler,
            ServiceState::Stopped,
            u32::from(result.is_err()),
            Duration::ZERO,
        )?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interactive_task_quotes_paths_and_escapes_xml() {
        let t = task_xml(
            r"C:\Orbit & Co\orbit.exe",
            r"C:\O'Brien\home folder",
            "S-1-5-42",
        );
        assert!(t.contains(r"C:\Orbit &amp; Co\orbit.exe"));
        assert!(t.contains(r"--home &quot;C:\O&apos;Brien\home folder&quot; daemon"));
        assert!(t.contains("InteractiveToken"));
        assert!(t.contains("LeastPrivilege"));
    }
    #[test]
    fn scm_arguments_and_recovery() {
        let args = service_args(
            ServiceRole::Relay,
            r"C:\Program Files\orbit.exe",
            r"C:\orbit",
            false,
        );
        assert_eq!(
            args[3],
            r#""C:\Program Files\orbit.exe" --home "C:\orbit" service-run relay"#
        );
        assert_eq!(
            failure_args(ServiceRole::Relay)[5],
            "restart/5000/restart/15000/restart/60000"
        );
    }
}
