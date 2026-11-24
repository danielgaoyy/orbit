//! Diagnostics are projected onto an allowlist. No arbitrary configuration or log text is exported.
use crate::{DaemonPersistedConfig, DaemonRuntime};
use serde_json::{Value, json};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{Arc, atomic::Ordering},
};

fn log_summary(raw: &str) -> Vec<Value> {
    raw.lines()
        .rev()
        .take(40)
        .map(|line| {
            let structured = serde_json::from_str::<Value>(line).ok();
            let level = ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"]
                .into_iter()
                .find(|level| {
                    structured
                        .as_ref()
                        .and_then(|v| v.get("level"))
                        .and_then(Value::as_str)
                        == Some(level)
                        || line.split_whitespace().take(2).any(|s| s == *level)
                })
                .unwrap_or("unknown");
            let ts = structured
                .as_ref()
                .and_then(|v| v["timestamp"].as_str())
                .or_else(|| line.split_whitespace().next())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.to_rfc3339());
            let message = [
                "Orbit Daemon ready",
                "Authenticated (PoP) and connected outbound to Orbit Relay",
                "Relay control heartbeat timed out, reconnecting...",
                "Disconnected from Relay, reconnecting...",
            ]
            .into_iter()
            .find(|msg| line.contains(msg))
            .unwrap_or("[redacted]");
            json!({"timestamp":ts,"level":level,"message":message})
        })
        .collect()
}
fn config_summary(c: &DaemonPersistedConfig) -> Value {
    json!({
        "listen_port":c.listen_addr.map(|a|a.port()),"relay_configured":c.relay_url.is_some(),
        "sandbox_mode":c.sandbox_policy.as_ref().map(|p|p.mode),"telemetry_configured":c.telemetry_endpoint.is_some(),
        "auto_compact_threshold_pct":c.auto_compact_threshold_pct
    })
}
pub async fn bundle(rt: &Arc<DaemonRuntime>) -> anyhow::Result<Value> {
    let store = rt.store.clone();
    let home = rt.home_dir.clone();
    let info = rt.host_info.clone();
    let mut result=tokio::task::spawn_blocking(move ||->anyhow::Result<Value>{
        let cfg=DaemonPersistedConfig::load(&home);
        let mut raw=Vec::new();
        if let Ok(mut file)=std::fs::File::open(home.join("logs/daemon.log")){
            let start=file.metadata()?.len().saturating_sub(32768);file.seek(SeekFrom::Start(start))?;file.take(32768).read_to_end(&mut raw)?;
        }
        let lsp=crate::lsp::builtin_lsp_catalog().into_iter().map(|s|json!({"id":s.id,"available":crate::lsp::resolve_binary(&s.command).is_some()})).collect::<Vec<_>>();
        Ok(json!({"version":env!("CARGO_PKG_VERSION"),"os":info.os,"arch":info.arch,"config":config_summary(&cfg),
            "providers":info.available_providers,"lsp":lsp,"sandbox":orbit_shim::probe_sandbox_support(),"sqlite_integrity_ok":store.integrity_ok()?,
            "audit":store.verify_audit_chain(store.audit_verifying_key().as_ref())?,"logs_newest_first":log_summary(&String::from_utf8_lossy(&raw))}))
    }).await??;
    let mut sockets = tokio::fs::read_dir(rt.home_dir.join("shims")).await?;
    let mut total = 0;
    let mut healthy = 0;
    while let Some(e) = sockets.next_entry().await? {
        if e.path().extension().is_some_and(|s| s == "sock") {
            total += 1;
            if orbit_shim::shim_socket_reachable(&e.path()).await {
                healthy += 1;
            }
        }
    }
    result["shim_sockets"] = json!({"total":total,"healthy":healthy});
    result["relay_connected"] = json!(rt.relay_connected.load(Ordering::Relaxed));
    Ok(result)
}

pub fn write_private(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    write(file.as_file_mut())?;
    file.flush()?;
    file.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exports_only_safe_fields() {
        let cfg:DaemonPersistedConfig=serde_json::from_value(json!({"relay_url":"wss://example/?ft=sk-private","pairing_secret":"secret","prompt":"raw prompt","orchestration":{}})).expect("cfg");
        let summary = json!([
            config_summary(&cfg),
            log_summary(
                "WARN Bearer sensitive\n{\"level\":\"INFO\",\"message\":\"sk-private raw prompt\"}"
            )
        ])
        .to_string();
        for s in [
            "Bearer",
            "sensitive",
            "pairing_secret",
            "sk-private",
            "raw prompt",
        ] {
            assert!(!summary.contains(s))
        }
    }
}
