//! Per-launch policy snapshot and proxy lifetime.
use crate::{DaemonPersistedConfig, egress_proxy};
use orbit_protocol::{SandboxMode, SandboxPolicy};
use orbit_shim::sandbox::SandboxCommand;
use orbit_store::OrbitStore;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, Weak},
};
use tokio::process::Command;

static HOSTS: LazyLock<Mutex<HashMap<PathBuf, Weak<OrbitStore>>>> = LazyLock::new(Default::default);
fn canonical_home_key(home: &Path) -> PathBuf {
    crate::platform::canonicalize_clean(home).unwrap_or_else(|_| home.to_path_buf())
}
pub(crate) fn register(home: &Path, store: &Arc<OrbitStore>) {
    let mut hosts = HOSTS.lock().unwrap_or_else(|e| e.into_inner());
    hosts.retain(|_, v| v.strong_count() > 0);
    hosts.insert(canonical_home_key(home), Arc::downgrade(store));
}

#[derive(Default)]
pub(crate) struct Boundary {
    pub policy: Option<SandboxPolicy>,
    proxy: Option<egress_proxy::SessionProxy>,
    store: Option<Arc<OrbitStore>>,
    orbit_home: Option<PathBuf>,
}
impl Boundary {
    pub fn url(&self) -> Option<String> {
        self.proxy.as_ref().map(|p| p.url.clone())
    }
    pub fn bridge_socket(&self) -> Option<&Path> {
        self.proxy.as_ref().and_then(|p| p.bridge_socket())
    }
    pub async fn load(home: &Path, agent: Option<&str>) -> anyhow::Result<Self> {
        Self::snapshot(home, agent, true).await
    }
    pub async fn for_shim(
        home: &Path,
        agent: Option<&str>,
        acp_callbacks: bool,
    ) -> anyhow::Result<Self> {
        Self::snapshot(home, agent, acp_callbacks).await
    }
    async fn snapshot(
        home: &Path,
        agent: Option<&str>,
        needs_listener: bool,
    ) -> anyhow::Result<Self> {
        let file = match tokio::fs::read(home.join("config.json")).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let cfg: DaemonPersistedConfig = serde_json::from_slice(&file)?;
        let Some(mut policy) = cfg.sandbox_policy.filter(|p| p.mode != SandboxMode::Off) else {
            return Ok(Self::default());
        };
        let home_key = canonical_home_key(home);
        let store = HOSTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&home_key)
            .and_then(Weak::upgrade)
            .ok_or_else(|| anyhow::anyhow!("sandbox audit store unavailable"))?;
        for file in [
            "pairing-secret",
            "daemon.json",
            "config.json",
            "host-keypair.json",
            "orbit.db",
            "orbit.db-wal",
            "orbit.db-shm",
            "shims",
            "relay.json",
            "mcp-token",
            "mcp-servers.json",
            "guardian.json",
            "hooks.json",
            "trusted-project-hooks.json",
            "browser-policy.json",
            "browser-profiles",
            "acp-agents.json",
            "logs",
            "runtime",
        ] {
            policy
                .denied_read_paths
                .push(home.join(file).to_string_lossy().into());
        }
        let proxy = if needs_listener && policy.allow_network {
            Some(
                egress_proxy::start(policy.clone(), store.clone(), agent.map(str::to_owned))
                    .await?,
            )
        } else {
            None
        };
        Ok(Self {
            policy: Some(policy),
            proxy,
            store: Some(store),
            orbit_home: Some(home_key),
        })
    }
    pub fn command_optional_cwd(
        &self,
        original: Command,
        cwd: Option<&Path>,
        fallback_cwd: &Path,
    ) -> Result<(Command, Option<SandboxCommand>), String> {
        let Some(base_policy) = self.policy.as_ref() else {
            return Ok((original, None));
        };
        let effective_cwd = cwd.unwrap_or(fallback_cwd);
        let targets_orbit_home = cwd.is_none()
            || self.orbit_home.as_ref().is_some_and(|home| {
                let norm = canonical_home_key(effective_cwd);
                norm == *home || norm.starts_with(home)
            });
        let mut effective_policy = base_policy.clone();
        if targets_orbit_home && effective_policy.mode == SandboxMode::WorkspaceWrite {
            effective_policy.mode = SandboxMode::StrictReadOnly;
        }
        self.command_with_policy(original, effective_cwd, &effective_policy)
    }
    pub fn command(
        &self,
        original: Command,
        cwd: &Path,
    ) -> Result<(Command, Option<SandboxCommand>), String> {
        let Some(policy) = self.policy.as_ref() else {
            return Ok((original, None));
        };
        self.command_with_policy(original, cwd, policy)
    }
    fn command_with_policy(
        &self,
        original: Command,
        cwd: &Path,
        policy: &SandboxPolicy,
    ) -> Result<(Command, Option<SandboxCommand>), String> {
        let c = original.as_std();
        let orbit_exe = crate::manager::resolve_orbit_exe();
        let plan = SandboxCommand::prepare_with_bridge(
            c.get_program(),
            c.get_args().map(Into::into).collect(),
            cwd,
            Some(policy),
            self.url().as_deref(),
            self.bridge_socket(),
            Some(orbit_exe.as_path()),
        )
        .map_err(|e| {
            if let Some(st) = self.store.clone() {
                tokio::spawn(async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = st.append_audit_event(
                            "sandbox",
                            "launch",
                            "daemon",
                            None,
                            "block",
                            &serde_json::Value::Null,
                        );
                    })
                    .await;
                });
            }
            e.to_string()
        })?;
        let mut cmd = plan.command();
        cmd.current_dir(cwd);
        Ok((cmd, Some(plan)))
    }
}
