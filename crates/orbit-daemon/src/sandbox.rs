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
pub(crate) fn register(home: &Path, store: &Arc<OrbitStore>) {
    let mut hosts = HOSTS.lock().unwrap_or_else(|e| e.into_inner());
    hosts.retain(|_, v| v.strong_count() > 0);
    hosts.insert(home.into(), Arc::downgrade(store));
}

#[derive(Default)]
pub(crate) struct Boundary {
    pub policy: Option<SandboxPolicy>,
    proxy: Option<egress_proxy::SessionProxy>,
    store: Option<Arc<OrbitStore>>,
}
impl Boundary {
    pub fn url(&self) -> Option<String> {
        self.proxy.as_ref().map(|p| p.url.clone())
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
        let store = HOSTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(home)
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
        })
    }
    pub fn command(
        &self,
        original: Command,
        cwd: &Path,
    ) -> Result<(Command, Option<SandboxCommand>), String> {
        if self.policy.is_none() {
            return Ok((original, None));
        }
        let c = original.as_std();
        let plan = SandboxCommand::prepare(
            c.get_program(),
            c.get_args().map(Into::into).collect(),
            cwd,
            self.policy.as_ref(),
            self.url().as_deref(),
        )
        .map_err(|e| {
            if let Some(st) = &self.store {
                let _ = st.append_audit_event(
                    "sandbox",
                    "launch",
                    "daemon",
                    None,
                    "block",
                    &serde_json::Value::Null,
                );
            }
            e.to_string()
        })?;
        let mut cmd = plan.command();
        cmd.current_dir(cwd);
        Ok((cmd, Some(plan)))
    }
}
