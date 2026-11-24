//! Host-local plugin registry. Only a manifest and its referenced module are installed.
mod guest;
mod io;

use anyhow::{Context, Result, bail, ensure};
use orbit_protocol::{
    InstalledPluginView, OrbitPluginManifest, OrbitPluginUiSlot, OrbitPluginUiSpec, valid_plugin_id,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};
use tokio::sync::Semaphore;

use guest::{BUFFER_LIMIT, DEADLINE, Guest};
use io::{open_root, read_relative};

const MANIFEST: &str = "orbit-plugin.json";
const ENABLED: &str = ".enabled";
const MANIFEST_LIMIT: usize = 32 * 1024;
const WASM_LIMIT: usize = 1024 * 1024;
const MAX_PLUGINS: usize = 32;

struct Installed {
    guest: Arc<Guest>,
    enabled: bool,
}
impl Installed {
    fn view(&self) -> InstalledPluginView {
        InstalledPluginView {
            manifest: self.guest.manifest.clone(),
            enabled: self.enabled,
        }
    }
}

struct Package {
    manifest: OrbitPluginManifest,
    wasm: Vec<u8>,
}

impl Package {
    fn manifest(bytes: &[u8]) -> Result<OrbitPluginManifest> {
        let manifest: OrbitPluginManifest = serde_json::from_slice(bytes)?;
        manifest.validate().map_err(anyhow::Error::msg)?;
        Ok(manifest)
    }
    fn local(path: &Path) -> Result<Self> {
        let dir = open_root(&path.canonicalize()?)?;
        let manifest = Self::manifest(&read_relative(&dir, MANIFEST, MANIFEST_LIMIT)?)?;
        let wasm = read_relative(&dir, &manifest.wasm_entry, WASM_LIMIT)?;
        Ok(Self { manifest, wasm })
    }
    async fn remote(source: &str) -> Result<Self> {
        // Read precisely two blobs at one commit via the Git host's HTTPS API.
        // No checkout, local Git configuration, credential helper or external process.
        let (repo, revision) = github_source(source)?;
        let sha = match revision {
            Some(sha) => sha.to_owned(),
            None => {
                let url = format!("https://api.github.com/repos/{repo}/commits?per_page=1");
                let body = io::https_get(&url, &["api.github.com".into()], MANIFEST_LIMIT).await?;
                let commits: Value = serde_json::from_slice(&body)?;
                let sha = commits[0]["sha"].as_str().context("missing Git commit")?;
                ensure!(valid_commit(sha), "invalid Git commit");
                sha.to_owned()
            }
        };
        let base =
            reqwest::Url::parse(&format!("https://raw.githubusercontent.com/{repo}/{sha}/"))?;
        let hosts = ["raw.githubusercontent.com".into()];
        let manifest = Self::manifest(
            &io::https_get(base.join(MANIFEST)?.as_str(), &hosts, MANIFEST_LIMIT).await?,
        )?;
        let mut entry = base;
        entry
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid Git URL"))?
            .pop_if_empty()
            .extend(manifest.wasm_entry.split('/'));
        let wasm = io::https_get(entry.as_str(), &hosts, WASM_LIMIT).await?;
        Ok(Self { manifest, wasm })
    }
}

fn valid_commit(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

fn github_source(source: &str) -> Result<(&str, Option<&str>)> {
    let raw = source
        .strip_prefix("https://github.com/")
        .context("expected a public HTTPS GitHub repository URL")?;
    let (repo, revision) = raw
        .split_once('#')
        .map_or((raw, None), |(repo, sha)| (repo, Some(sha)));
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    ensure!(
        orbit_protocol::is_valid_github_repo(repo) && revision.is_none_or(valid_commit),
        "invalid Git repository or commit"
    );
    Ok((repo, revision))
}

pub struct PluginRuntime {
    orbit_home: PathBuf,
    worktrees_dir: PathBuf,
    root: PathBuf,
    installed: Mutex<BTreeMap<String, Installed>>,
    workers: Arc<Semaphore>,
}

impl PluginRuntime {
    pub(crate) fn open(home: &Path) -> Result<Arc<Self>> {
        let root = home.join("plugins");
        crate::ensure_private_dir(&root)?;
        let orbit_home =
            crate::platform::canonicalize_clean(home).unwrap_or_else(|_| home.to_path_buf());
        let worktrees_dir = orbit_home.join("worktrees");
        let mut installed = BTreeMap::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !valid_plugin_id(&id) || !entry.file_type()?.is_dir() {
                continue;
            }
            if installed.len() >= MAX_PLUGINS {
                break;
            }
            let load = || -> Result<Installed> {
                let path = entry.path();
                let package = Package::local(&path)?;
                ensure!(
                    package.manifest.id == id,
                    "directory and manifest IDs differ"
                );
                let guest = Guest::load(package.manifest, &package.wasm)?;
                Ok(Installed {
                    guest,
                    enabled: path.join(ENABLED).is_file(),
                })
            };
            match load() {
                Ok(plugin) => {
                    installed.insert(id, plugin);
                }
                Err(error) => {
                    tracing::warn!(plugin_id=%id,%error,"skipping invalid installed plugin")
                }
            }
        }
        Ok(Arc::new(Self {
            orbit_home,
            worktrees_dir,
            root,
            installed: Mutex::new(installed),
            workers: Arc::new(Semaphore::new(4)),
        }))
    }
    fn registry(&self) -> Result<MutexGuard<'_, BTreeMap<String, Installed>>> {
        self.installed
            .lock()
            .map_err(|_| anyhow::anyhow!("plugin registry unavailable"))
    }
    pub fn list(&self) -> Result<Vec<InstalledPluginView>> {
        Ok(self.registry()?.values().map(Installed::view).collect())
    }
    pub(crate) fn has_ops(&self) -> bool {
        self.registry().is_ok_and(|r| {
            r.values()
                .any(|p| p.enabled && !p.guest.manifest.mcp_ops.is_empty())
        })
    }
    pub async fn install(
        self: &Arc<Self>,
        source: String,
        enabled: bool,
    ) -> Result<InstalledPluginView> {
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .context("plugin runtime busy")?;
        let remote = if source.contains("://") {
            Some(Package::remote(&source).await?)
        } else {
            None
        };
        let runtime = self.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let package = match remote {
                Some(p) => p,
                None => Package::local(Path::new(&source))?,
            };
            let guest = Guest::load(package.manifest, &package.wasm)?;
            let manifest = &guest.manifest;
            let stage = tempfile::tempdir_in(&runtime.root)?;
            let entry = stage.path().join(&manifest.wasm_entry);
            fs::create_dir_all(entry.parent().context("missing module directory")?)?;
            fs::write(entry, package.wasm)?;
            fs::write(
                stage.path().join(MANIFEST),
                serde_json::to_vec_pretty(manifest)?,
            )?;
            if enabled {
                fs::write(stage.path().join(ENABLED), [])?;
            }
            let mut registry = runtime.registry()?;
            let dest = runtime.root.join(&manifest.id);
            ensure!(
                registry.len() < MAX_PLUGINS,
                "installed plugin limit reached"
            );
            ensure!(
                !registry.contains_key(&manifest.id) && !dest.exists(),
                "plugin already installed"
            );
            fs::rename(stage.path(), dest)?;
            let _ = stage.keep();
            let id = manifest.id.clone();
            let installed = Installed { guest, enabled };
            let view = installed.view();
            registry.insert(id, installed);
            Ok(view)
        })
        .await?
    }
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let mut reg = self.registry()?;
        let p = reg.get_mut(id).context("unknown plugin")?;
        if p.enabled != enabled {
            let marker = self.root.join(id).join(ENABLED);
            if enabled {
                fs::write(marker, [])?;
            } else {
                match fs::remove_file(marker) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            p.enabled = enabled;
        }
        Ok(())
    }
    pub fn uninstall(&self, id: &str) -> Result<()> {
        let mut reg = self.registry()?;
        ensure!(reg.contains_key(id), "unknown plugin");
        let trash = tempfile::tempdir_in(&self.root)?;
        fs::rename(self.root.join(id), trash.path().join("removed"))?;
        reg.remove(id);
        drop(reg);
        Ok(())
    }
    fn enabled(&self, id: &str) -> Result<Arc<Guest>> {
        let reg = self.registry()?;
        let plugin = reg.get(id).context("unknown plugin")?;
        ensure!(plugin.enabled, "plugin is disabled");
        Ok(plugin.guest.clone())
    }
    async fn execute(
        &self,
        guest: Arc<Guest>,
        cwd: Option<PathBuf>,
        request: Request<'_>,
    ) -> Result<Value> {
        if let Some(ref root) = cwd {
            let norm = crate::platform::canonicalize_clean(root).unwrap_or_else(|_| root.clone());
            let inside_worktrees =
                norm.starts_with(&self.worktrees_dir) && norm != self.worktrees_dir;
            let touches_orbit_home = norm == self.orbit_home
                || self.orbit_home.starts_with(&norm)
                || (norm.starts_with(&self.orbit_home) && !inside_worktrees);
            ensure!(
                !touches_orbit_home,
                "plugin workspace cannot access Orbit home directory"
            );
        }
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .context("plugin runtime busy")?;
        let bytes = serde_json::to_vec(&request)?;
        ensure!(
            bytes.len() <= BUFFER_LIMIT,
            "plugin input exceeds byte limit"
        );
        let deadline = Instant::now() + DEADLINE;
        let job = tokio::task::spawn_blocking(move || {
            // Keep the permit until execution actually stops, including after a caller timeout.
            let _permit = permit;
            guest.run(cwd.as_deref(), &bytes, deadline)
        });
        tokio::time::timeout_at(deadline.into(), job)
            .await
            .context("plugin execution timed out")??
    }
    pub async fn invoke(
        &self,
        id: &str,
        op: &str,
        cwd: Option<PathBuf>,
        input: &Value,
    ) -> Result<Value> {
        let guest = self.enabled(id)?;
        ensure!(
            guest.manifest.mcp_ops.iter().any(|o| o.name == op),
            "undeclared plugin operation"
        );
        self.execute(guest, cwd, Request::Call { op, input }).await
    }
    pub async fn render(
        &self,
        id: &str,
        slot: OrbitPluginUiSlot,
        cwd: Option<PathBuf>,
        action_id: Option<&str>,
        payload: Option<&Value>,
    ) -> Result<OrbitPluginUiSpec> {
        let guest = self.enabled(id)?;
        ensure!(
            guest.manifest.ui_slots.iter().any(|s| s.slot == slot),
            "undeclared plugin UI slot"
        );
        ensure!(
            action_id.is_none_or(valid_plugin_id),
            "invalid plugin action ID"
        );
        let value = self
            .execute(
                guest,
                cwd,
                Request::Render {
                    slot,
                    action_id,
                    payload,
                },
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Request<'a> {
    Call {
        op: &'a str,
        input: &'a Value,
    },
    Render {
        slot: OrbitPluginUiSlot,
        action_id: Option<&'a str>,
        payload: Option<&'a Value>,
    },
}

pub(super) async fn handle_rpc(
    runtime: &crate::DaemonRuntime,
    req: orbit_protocol::WireMessage,
    request_id: uuid::Uuid,
) -> Result<orbit_protocol::WireMessage> {
    use orbit_protocol::WireMessage::*;
    let plugins = &runtime.plugins;
    Ok(match req {
        ListPlugins { .. } => PluginsList {
            request_id,
            plugins: plugins.list()?,
        },
        InstallPlugin { source, enable, .. } => PluginInstalled {
            request_id,
            plugin: plugins.install(source, enable).await?,
        },
        SetPluginEnabled {
            plugin_id, enabled, ..
        } => {
            let plugins = plugins.clone();
            tokio::task::spawn_blocking(move || plugins.set_enabled(&plugin_id, enabled)).await??;
            Ack { request_id }
        }
        UninstallPlugin { plugin_id, .. } => {
            let plugins = plugins.clone();
            tokio::task::spawn_blocking(move || plugins.uninstall(&plugin_id)).await??;
            Ack { request_id }
        }
        InvokePluginOp {
            plugin_id,
            op,
            cwd,
            input,
            ..
        } => {
            let root = crate::authorize_optional_mcp_cwd(&runtime.store, cwd)
                .await
                .map_err(anyhow::Error::msg)?;
            let result = plugins.invoke(&plugin_id, &op, root, &input).await?;
            PluginOpResult {
                request_id,
                plugin_id,
                op,
                result,
            }
        }
        RenderPluginUi {
            plugin_id,
            slot,
            cwd,
            action_id,
            payload,
            ..
        } => {
            let root = crate::authorize_optional_mcp_cwd(&runtime.store, cwd)
                .await
                .map_err(anyhow::Error::msg)?;
            let spec = plugins
                .render(
                    &plugin_id,
                    slot,
                    root,
                    action_id.as_deref(),
                    payload.as_ref(),
                )
                .await?;
            PluginUiRendered {
                request_id,
                plugin_id,
                spec,
            }
        }
        _ => bail!("unexpected plugin request"),
    })
}

#[cfg(test)]
mod tests;
