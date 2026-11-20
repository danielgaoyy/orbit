//! First-class native Rust drivers for Antigravity (`agy`), Codex (`codex`), and Claude Code (`claude`).

pub mod antigravity;
pub mod attachments;
pub mod claude;
pub mod codex;
pub mod driver;
pub mod side_question;

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use orbit_protocol::{ModelOption, ProviderKind, ProviderModelCatalog, ProviderQuotaSnapshot};
use serde::{Deserialize, Serialize};

pub use driver::{DriverCommand, DriverEvent, ProviderSessionHandle, SessionLaunchConfig};
pub use side_question::{SideQuestionSpec, run_side_question};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredProvider {
    pub kind: ProviderKind,
    pub executable_path: PathBuf,
    pub protocol_transport: &'static str,
}

/// Probe `PATH` and standard user bin locations (`~/.local/bin`, `~/.cargo/bin`, `/opt/homebrew/bin`)
/// across Linux workstations, cloud Devboxes, and macOS (Mac mini).
pub fn discover_installed_providers() -> Vec<DiscoveredProvider> {
    let candidates = [
        (ProviderKind::Antigravity, "agy", "acp-stdio + transcript"),
        (ProviderKind::Codex, "codex", "app-server json-rpc"),
        (ProviderKind::ClaudeCode, "claude", "stream-json control"),
    ];

    let mut found = Vec::new();
    for (kind, bin_name, transport) in candidates {
        if let Some(path) = find_executable(bin_name) {
            found.push(DiscoveredProvider {
                kind,
                executable_path: path,
                protocol_transport: transport,
            });
        }
    }
    found
}

pub fn find_provider_executable(kind: ProviderKind) -> Option<PathBuf> {
    find_executable(kind.default_binary())
}

pub fn find_executable(binary_name: &str) -> Option<PathBuf> {
    let mut search_dirs: Vec<PathBuf> = env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).collect())
        .unwrap_or_default();

    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        search_dirs.push(home.join(".local/bin"));
        search_dirs.push(home.join(".npm-global/bin"));
        search_dirs.push(home.join(".cargo/bin"));
    }
    search_dirs.push(PathBuf::from("/opt/homebrew/bin"));
    search_dirs.push(PathBuf::from("/usr/local/bin"));

    for dir in search_dirs {
        let candidate = dir.join(binary_name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Unified session launcher dispatching to the appropriate native Rust protocol driver.
pub async fn spawn_provider_session(
    cfg: SessionLaunchConfig,
) -> Result<ProviderSessionHandle, String> {
    match cfg.provider {
        ProviderKind::Antigravity => antigravity::spawn_antigravity_session(cfg).await,
        ProviderKind::Codex => codex::spawn_codex_session(cfg).await,
        ProviderKind::ClaudeCode => claude::spawn_claude_session(cfg).await,
    }
}

/// Concurrently fetch real-time quota/usage windows across all installed providers on this host.
pub async fn probe_all_provider_quotas() -> Vec<ProviderQuotaSnapshot> {
    let installed = discover_installed_providers();
    let mut agy_path = None;
    let mut has_codex = false;
    let mut has_claude = false;

    for p in installed {
        match p.kind {
            ProviderKind::Antigravity => agy_path = Some(p.executable_path),
            ProviderKind::Codex => has_codex = true,
            ProviderKind::ClaudeCode => has_claude = true,
        }
    }

    let agy_fut = async move {
        if let Some(p) = agy_path {
            Some(antigravity::probe_antigravity_quota(&p).await)
        } else {
            None
        }
    };
    let codex_fut = async move {
        if has_codex {
            Some(codex::probe_codex_quota().await)
        } else {
            None
        }
    };
    let claude_fut = async move {
        if has_claude {
            Some(claude::probe_claude_quota().await)
        } else {
            None
        }
    };

    let (a, b, c) = tokio::join!(agy_fut, codex_fut, claude_fut);
    [a, b, c].into_iter().flatten().collect()
}

/// Instant non-subprocess model catalogs (reads `~/.codex/models_cache.json` + Paseo manifests).
pub async fn fallback_all_provider_models() -> Vec<ProviderModelCatalog> {
    let codex = codex::fallback_codex_models().await;
    let claude = claude::fallback_claude_models();
    let agy = antigravity::fallback_antigravity_models();
    vec![claude, codex, agy]
}

/// Concurrently probe live available models & per-model thinking options across all three providers.
pub async fn probe_all_provider_models() -> Vec<ProviderModelCatalog> {
    let installed = discover_installed_providers();
    let mut agy_path = None;
    let mut codex_path = None;
    let mut claude_path = None;

    for p in installed {
        match p.kind {
            ProviderKind::Antigravity => agy_path = Some(p.executable_path),
            ProviderKind::Codex => codex_path = Some(p.executable_path),
            ProviderKind::ClaudeCode => claude_path = Some(p.executable_path),
        }
    }

    let (claude_cat, codex_cat, agy_cat) = tokio::join!(
        claude::probe_claude_models(claude_path.as_deref()),
        codex::probe_codex_models(codex_path.as_deref()),
        antigravity::probe_antigravity_models(agy_path.as_deref()),
    );

    vec![claude_cat, codex_cat, agy_cat]
}

/// Project structured `ProviderModelCatalog` slices into legacy `ModelOption` maps for mobile FFI.
pub fn catalogs_to_model_map(
    catalogs: &[ProviderModelCatalog],
) -> HashMap<ProviderKind, Vec<ModelOption>> {
    let mut map = HashMap::new();
    for cat in catalogs {
        let items = cat
            .models
            .iter()
            .map(|m| ModelOption {
                id: m.id.clone(),
                label: m.label.clone(),
                description: if m.description.is_empty() {
                    None
                } else {
                    Some(m.description.clone())
                },
                is_default: m.is_default,
            })
            .collect();
        map.insert(cat.provider, items);
    }
    map
}
