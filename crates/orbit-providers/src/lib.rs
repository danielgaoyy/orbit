//! First-class native Rust drivers for Antigravity (`agy`), Codex (`codex`), and Claude Code (`claude`).

pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod driver;

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use orbit_protocol::{ModelOption, ProviderKind, ProviderQuotaSnapshot};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub use driver::{DriverCommand, DriverEvent, ProviderSessionHandle, SessionLaunchConfig};

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

/// Discover available LLM models across all three providers (combining live CLI probes with curated defaults).
pub async fn probe_all_provider_models() -> HashMap<ProviderKind, Vec<ModelOption>> {
    let agy_bin = find_provider_executable(ProviderKind::Antigravity);
    let codex_bin = find_provider_executable(ProviderKind::Codex);

    let agy_fut = async move {
        if let Some(bin) = agy_bin
            && let Some(live) = probe_agy_models_live(&bin).await
            && !live.is_empty()
        {
            return live;
        }
        default_antigravity_models()
    };

    let codex_fut = async move {
        if let Some(bin) = codex_bin
            && let Some(live) = probe_codex_models_live(&bin).await
            && !live.is_empty()
        {
            return live;
        }
        default_codex_models()
    };

    let (agy_models, codex_models) = tokio::join!(agy_fut, codex_fut);
    let mut map = HashMap::new();
    map.insert(ProviderKind::Antigravity, agy_models);
    map.insert(ProviderKind::Codex, codex_models);
    map.insert(ProviderKind::ClaudeCode, default_claude_models());
    map
}

async fn probe_agy_models_live(bin: &Path) -> Option<Vec<ModelOption>> {
    let mut child = tokio::process::Command::new(bin)
        .args(["acp", "--stdio"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;

    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let mut reader = BufReader::new(stdout).lines();

    let cwd = env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let init_line = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": { "name": "orbit-model-probe", "version": "0.2.1" }
        }
    })
    .to_string();
    stdin
        .write_all(format!("{init_line}\n").as_bytes())
        .await
        .ok()?;
    let _ = tokio::time::timeout(Duration::from_secs(4), reader.next_line())
        .await
        .ok()?
        .ok()??;

    let new_line = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/new",
        "params": { "cwd": cwd, "mcpServers": [] }
    })
    .to_string();
    stdin
        .write_all(format!("{new_line}\n").as_bytes())
        .await
        .ok()?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(Ok(Some(line))) = tokio::time::timeout(rem, reader.next_line()).await else {
            break;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if val.get("id").and_then(|v| v.as_i64()) == Some(2) {
            let current = val
                .pointer("/result/models/currentModelId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arr = val
                .pointer("/result/models/availableModels")
                .and_then(|v| v.as_array())?;
            let mut out = Vec::new();
            for item in arr {
                let Some(id) = item.get("modelId").and_then(|v| v.as_str()) else {
                    continue;
                };
                let label = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(id)
                    .to_string();
                let desc = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned);
                out.push(ModelOption {
                    id: id.to_string(),
                    label,
                    description: desc,
                    is_default: id == current,
                });
            }
            return Some(out);
        }
    }
    None
}

async fn probe_codex_models_live(bin: &Path) -> Option<Vec<ModelOption>> {
    let mut child = tokio::process::Command::new(bin)
        .arg("app-server")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;

    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let mut reader = BufReader::new(stdout).lines();

    let init = serde_json::json!({
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": { "name": "orbit_probe", "title": "Orbit", "version": "0.2.1" },
            "capabilities": { "experimentalApi": true }
        }
    })
    .to_string();
    stdin.write_all(format!("{init}\n").as_bytes()).await.ok()?;
    let _ = tokio::time::timeout(Duration::from_secs(4), reader.next_line())
        .await
        .ok()?
        .ok()??;

    let initialized = serde_json::json!({ "method": "initialized", "params": {} }).to_string();
    let list_req = serde_json::json!({ "id": 2, "method": "model/list", "params": {} }).to_string();
    stdin
        .write_all(format!("{initialized}\n{list_req}\n").as_bytes())
        .await
        .ok()?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    while tokio::time::Instant::now() < deadline {
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(Ok(Some(line))) = tokio::time::timeout(rem, reader.next_line()).await else {
            break;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if val.get("id").and_then(|v| v.as_i64()) == Some(2) {
            let arr = val.pointer("/result/data").and_then(|v| v.as_array())?;
            let mut out = Vec::new();
            for item in arr {
                let Some(id) = item
                    .get("id")
                    .or_else(|| item.get("model"))
                    .and_then(|v| v.as_str())
                else {
                    continue;
                };
                let label = item
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .unwrap_or(id)
                    .to_string();
                let desc = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned);
                let is_def = item
                    .get("isDefault")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                out.push(ModelOption {
                    id: id.to_string(),
                    label,
                    description: desc,
                    is_default: is_def,
                });
            }
            return Some(out);
        }
    }
    None
}

fn default_antigravity_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            id: "gemini-4-carbon-high".into(),
            label: "Gemini 4 Carbon (High)".into(),
            description: Some("深度推理与架构设计旗舰".into()),
            is_default: true,
        },
        ModelOption {
            id: "gemini-4-carbon-medium".into(),
            label: "Gemini 4 Carbon (Medium)".into(),
            description: Some("速度与深度均衡主力".into()),
            is_default: false,
        },
        ModelOption {
            id: "gemini-4-argon-high".into(),
            label: "Gemini 4 Argon (High)".into(),
            description: Some("高强度复杂代码推理".into()),
            is_default: false,
        },
        ModelOption {
            id: "claude-opus-5-5-high".into(),
            label: "Claude Opus 5.5 (High)".into(),
            description: Some("Antigravity 内置 Opus 深度模式".into()),
            is_default: false,
        },
        ModelOption {
            id: "claude-sonnet-5-5-high".into(),
            label: "Claude Sonnet 5.5 (High)".into(),
            description: Some("高响应速度编码模式".into()),
            is_default: false,
        },
        ModelOption {
            id: "gemini-3.8-flash-high".into(),
            label: "Gemini 3.8 Flash (High)".into(),
            description: Some("极速轻量任务响应".into()),
            is_default: false,
        },
    ]
}

fn default_codex_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            id: "gpt-6.1-sol".into(),
            label: "GPT-6.1-Sol".into(),
            description: Some("最新日常编码与重构主力模型".into()),
            is_default: true,
        },
        ModelOption {
            id: "gpt-6-astra".into(),
            label: "GPT-6-Astra".into(),
            description: Some("高复杂度架构与攻坚模型".into()),
            is_default: false,
        },
        ModelOption {
            id: "gpt-6.1-terra".into(),
            label: "GPT-6.1-Terra".into(),
            description: Some("均衡稳定编码模型".into()),
            is_default: false,
        },
        ModelOption {
            id: "gpt-5.4".into(),
            label: "GPT-5.4".into(),
            description: Some("通用高兼容 Codex 模型".into()),
            is_default: false,
        },
    ]
}

fn default_claude_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            id: "claude-opus-5-5".into(),
            label: "Opus 5.5".into(),
            description: Some("最新旗舰深度架构与推理模型".into()),
            is_default: true,
        },
        ModelOption {
            id: "claude-sonnet-5-5".into(),
            label: "Sonnet 5.5".into(),
            description: Some("高速高质主力编码模型".into()),
            is_default: false,
        },
        ModelOption {
            id: "claude-opus-4-6".into(),
            label: "Opus 4.6".into(),
            description: Some("经典稳定复杂工程模型".into()),
            is_default: false,
        },
        ModelOption {
            id: "claude-sonnet-4-6".into(),
            label: "Sonnet 4.6".into(),
            description: Some("轻量快速日常开发".into()),
            is_default: false,
        },
        ModelOption {
            id: "claude-haiku-5-5".into(),
            label: "Haiku 5.5".into(),
            description: Some("瞬时响应问答与检索".into()),
            is_default: false,
        },
    ]
}
