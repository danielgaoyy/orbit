//! `orbit mcp` — Stdio-to-HTTP JSON-RPC 2.0 MCP bridge for providers or external MCP clients
//! that only speak stdio transport.
//!
//! Reads newline-delimited JSON-RPC 2.0 frames (and also accepts LSP-style `Content-Length:`
//! headers for maximum client compatibility) from stdin, forwards each request to the local
//! Orbit Daemon's Streamable HTTP MCP server (`POST /mcp/agents`) with Bearer auth from
//! `$ORBIT_MCP_TOKEN` or `$ORBIT_HOME/mcp-token`, and writes responses strictly to stdout.

use std::path::Path;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub async fn run_mcp_stdio_bridge(
    home_dir: &Path,
    caller_agent_id: Option<String>,
    url_override: Option<String>,
) -> anyhow::Result<()> {
    let base_url = url_override
        .or_else(|| std::env::var("ORBIT_MCP_URL").ok())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            let port = orbit_daemon::read_live_daemon_port(home_dir);
            format!("http://127.0.0.1:{port}/mcp/agents")
        });

    let effective_caller = caller_agent_id
        .or_else(|| std::env::var("ORBIT_AGENT_ID").ok())
        .filter(|s| !s.trim().is_empty());

    let endpoint = if let Some(ref cid) = effective_caller {
        if base_url.contains("caller_agent_id=") || base_url.contains("callerAgentId=") {
            base_url
        } else if base_url.contains('?') {
            format!("{base_url}&caller_agent_id={cid}")
        } else {
            format!("{base_url}?caller_agent_id={cid}")
        }
    } else {
        base_url
    };

    let token = std::env::var("ORBIT_MCP_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::fs::read_to_string(home_dir.join("mcp-token"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Orbit MCP token not found in $ORBIT_MCP_TOKEN or {}",
                home_dir.join("mcp-token").display()
            )
        })?;

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let mut reader = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Support optional LSP Content-Length header framing in addition to newline-delimited JSON
        let payload_str = if let Some(len_str) = trimmed
            .strip_prefix("Content-Length:")
            .or_else(|| trimmed.strip_prefix("content-length:"))
        {
            let content_len: usize = len_str.trim().parse().unwrap_or(0);
            // Consume remaining header lines until blank line
            loop {
                line.clear();
                let hn = reader.read_line(&mut line).await?;
                if hn == 0 || line.trim().is_empty() {
                    break;
                }
            }
            if content_len == 0 {
                continue;
            }
            let mut buf = vec![0u8; content_len];
            reader.read_exact(&mut buf).await?;
            String::from_utf8_lossy(&buf).to_string()
        } else {
            trimmed.to_string()
        };

        let req_val: serde_json::Value = match serde_json::from_str(&payload_str) {
            Ok(v) => v,
            Err(e) => {
                let err_resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": serde_json::Value::Null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                let out = format!("{}\n", serde_json::to_string(&err_resp)?);
                stdout.write_all(out.as_bytes()).await?;
                stdout.flush().await?;
                continue;
            }
        };

        let req_id = req_val.get("id").cloned();
        let http_res = client
            .post(&endpoint)
            .bearer_auth(&token)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&req_val)
            .send()
            .await;

        match http_res {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if status == reqwest::StatusCode::ACCEPTED || body.trim().is_empty() {
                    continue;
                }
                if !status.is_success() {
                    if let Some(id_val) = req_id {
                        let out_val = serde_json::from_str::<serde_json::Value>(&body).unwrap_or_else(|_| {
                            serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id_val,
                                "error": {
                                    "code": -32000,
                                    "message": format!("Orbit MCP HTTP {status}: {body}")
                                }
                            })
                        });
                        let out = format!("{}\n", serde_json::to_string(&out_val)?);
                        stdout.write_all(out.as_bytes()).await?;
                        stdout.flush().await?;
                    }
                    continue;
                }
                let compact = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => serde_json::to_string(&v)?,
                    Err(_) => body.trim().to_string(),
                };
                stdout.write_all(compact.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
            Err(e) => {
                if let Some(id_val) = req_id {
                    let err_resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id_val,
                        "error": {
                            "code": -32002,
                            "message": format!("Failed to reach Orbit Daemon MCP server ({endpoint}): {e}")
                        }
                    });
                    let out = format!("{}\n", serde_json::to_string(&err_resp)?);
                    stdout.write_all(out.as_bytes()).await?;
                    stdout.flush().await?;
                }
            }
        }
    }

    Ok(())
}
