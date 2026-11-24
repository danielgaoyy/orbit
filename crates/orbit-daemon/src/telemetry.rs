//! Bounded OTLP/HTTP log export. Only static event metadata leaves the daemon.
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc};
use tracing_subscriber::Layer;

enum Item {
    Log(Value),
    Shutdown,
}
pub struct LogLayer(mpsc::Sender<Item>);
pub struct Exporter {
    tx: mpsc::Sender<Item>,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
}
impl<S: tracing::Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        let meta = event.metadata();
        if !meta.target().starts_with("orbit_") {
            return;
        }
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let _=self.0.try_send(Item::Log(json!({"timeUnixNano":ns.to_string(),"severityText":meta.level().as_str(),
            "body":{"stringValue":meta.name()},"attributes":[{"key":"target","value":{"stringValue":meta.target()}}]})));
    }
}

pub fn start(endpoint: &str) -> anyhow::Result<(LogLayer, Arc<Exporter>)> {
    let mut url = reqwest::Url::parse(endpoint)?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https"),
        "invalid OTLP endpoint scheme"
    );
    let path = format!("{}/v1/logs", url.path().trim_end_matches('/'));
    url.set_path(&path);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let (tx, mut rx) = mpsc::channel(512);
    let worker = tokio::spawn(async move {
        let mut batch = Vec::with_capacity(64);
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            let mut stop = false;
            tokio::select! {
                item=rx.recv()=>match item{
                    Some(Item::Log(log))=>{batch.push(log);if batch.len()<64{continue}},
                    None|Some(Item::Shutdown)=>stop=true,
                },
                _=tick.tick()=>{}
            }
            if !batch.is_empty() {
                let data = json!({"resourceLogs":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"orbit-daemon"}}]},"scopeLogs":[{"scope":{"name":"orbit"},"logRecords":batch}]}]});
                let _ = client.post(url.clone()).json(&data).send().await;
                batch.clear();
            }
            if stop {
                break;
            }
        }
    });
    Ok((
        LogLayer(tx.clone()),
        Arc::new(Exporter {
            tx,
            worker: Mutex::new(Some(worker)),
        }),
    ))
}
impl Exporter {
    pub async fn flush_and_shutdown(&self) {
        let mut worker = self.worker.lock().await;
        if let Some(task) = worker.take() {
            let _ = self.tx.send(Item::Shutdown).await;
            let _ = task.await;
        }
    }
}

pub fn openmetrics(values: [u64; 8]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for ((name, kind), n) in [
        ("agents_active", "gauge"),
        ("shims", "gauge"),
        ("permission_approvals", "counter"),
        ("permission_denials", "counter"),
        ("egress_blocks", "counter"),
        ("audit_entries", "gauge"),
        ("rpc", "counter"),
        ("compactions", "counter"),
    ]
    .into_iter()
    .zip(values)
    {
        let _ = writeln!(out, "# TYPE orbit_{name} {kind}");
        let suffix = if kind == "counter" { "_total" } else { "" };
        let _ = writeln!(out, "orbit_{name}{suffix} {n}");
    }
    out.push_str("# EOF\n");
    out
}
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn shutdown_flushes_redacted_otlp_http_records() -> anyhow::Result<()> {
        use tracing_subscriber::prelude::*;
        let (tx, mut rx) = tokio::sync::mpsc::channel(2);
        let app = axum::Router::new().route(
            "/v1/logs",
            axum::routing::post(
                move |axum::Json(body): axum::Json<serde_json::Value>| async move {
                    let _ = tx.send(body).await;
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let (layer, handle) = super::start(&endpoint)?;
        tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
            tracing::info!(target:"orbit_test",token="sk-private","Bearer secret");
        });
        handle.flush_and_shutdown().await;
        let body = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await?
            .expect("batch");
        let log = &body["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log["severityText"], "INFO");
        assert!(!body.to_string().contains("Bearer"));
        assert!(!body.to_string().contains("sk-private"));
        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn live_metrics_requires_host_credential() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let (addr, rt) = crate::spawn_embedded_daemon(tmp.path().into(), None).await?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let url = format!("http://{addr}/metrics");
        assert_eq!(
            client.get(&url).send().await?.status(),
            reqwest::StatusCode::FORBIDDEN
        );
        let res = client
            .get(url)
            .bearer_auth(&rt.pairing_secret)
            .send()
            .await?;
        assert!(res.status().is_success());
        assert!(res.text().await?.ends_with("# EOF\n"));
        Ok(())
    }
    #[test]
    fn formats_counters_and_eof() {
        let s = super::openmetrics([1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(s.contains("orbit_rpc_total 7\n"));
        assert!(s.ends_with("# EOF\n"));
    }
}
