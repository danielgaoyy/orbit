//! Authenticated, session-scoped outbound HTTP forwarding.
use axum::{
    body::Body,
    http::{HeaderValue, Method, Request, Response, StatusCode, Uri, header, uri::Authority},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use hyper::{body::Incoming, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use orbit_protocol::SandboxPolicy;
use orbit_store::OrbitStore;
use std::{
    net::{IpAddr, SocketAddr},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinHandle,
};

pub(crate) struct SessionProxy {
    pub url: String,
    listener: JoinHandle<()>,
    shutdown: tokio::sync::watch::Sender<()>,
    #[cfg(target_os = "linux")]
    bridge: LinuxBridge,
}
impl SessionProxy {
    pub fn bridge_socket(&self) -> Option<&Path> {
        #[cfg(target_os = "linux")]
        {
            Some(&self.bridge.socket_path)
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }
}
impl Drop for SessionProxy {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
        self.listener.abort();
    }
}

fn domain_matches(pattern: &str, host: &str) -> bool {
    let p = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if let (Ok(p_ip), Ok(h_ip)) = (
        p.trim_matches(['[', ']']).parse::<IpAddr>(),
        h.trim_matches(['[', ']']).parse::<IpAddr>(),
    ) {
        return p_ip == h_ip;
    }
    if let Some(suffix) = p.strip_prefix("*.") {
        h.strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.') && prefix.len() > 1)
    } else {
        p == h
    }
}
fn permits(p: &SandboxPolicy, h: &str) -> bool {
    p.allow_network
        && !p.denied_domains.iter().any(|d| domain_matches(d, h))
        && (p.allowed_domains.is_empty() || p.allowed_domains.iter().any(|d| domain_matches(d, h)))
}

fn public_ip(ip: IpAddr) -> bool {
    if crate::mcp_servers::is_blocked_ssrf_ip(ip) {
        return false;
    }
    match ip {
        IpAddr::V4(v) => {
            let [a, b, ..] = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_documentation()
                || a == 0
                || a >= 224
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (b == 18 || b == 19))
                || (a == 192 && b == 0))
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            // Global unicast only. Exclude transition prefixes, documentation and Teredo.
            s[0] & 0xe000 == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
        }
    }
}

struct Session {
    auth: String,
    policy: SandboxPolicy,
    store: Arc<OrbitStore>,
    agent: Option<String>,
    shutdown: tokio::sync::watch::Receiver<()>,
    tunnels: Arc<Semaphore>,
}
impl Session {
    async fn record(
        &self,
        outcome: &'static str,
        reason: &'static str,
        host: Option<&str>,
    ) -> bool {
        let store = self.store.clone();
        let agent = self.agent.clone();
        let details = serde_json::json!({"reason":reason,"host":host});
        matches!(
            tokio::task::spawn_blocking(move || store.append_audit_event(
                "egress",
                "connect",
                "sandbox",
                agent.as_deref(),
                outcome,
                &details
            ))
            .await,
            Ok(Ok(_))
        )
    }
}

pub(crate) async fn start(
    policy: SandboxPolicy,
    store: Arc<OrbitStore>,
    agent: Option<String>,
) -> std::io::Result<SessionProxy> {
    listen(policy, store, agent, async |addresses: Vec<SocketAddr>| {
        TcpStream::connect(addresses.as_slice()).await
    })
    .await
}

async fn listen<D, F>(
    policy: SandboxPolicy,
    store: Arc<OrbitStore>,
    agent: Option<String>,
    dial: D,
) -> std::io::Result<SessionProxy>
where
    D: Fn(Vec<SocketAddr>) -> F + Clone + Send + Sync + 'static,
    F: std::future::Future<Output = std::io::Result<TcpStream>> + Send + 'static,
{
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    #[cfg(target_os = "linux")]
    let bridge = LinuxBridge::bind(port)?;
    let token = orbit_crypto::generate_pairing_secret();
    let (shutdown, shutdown_rx) = tokio::sync::watch::channel(());
    let session = Arc::new(Session {
        shutdown: shutdown_rx,
        tunnels: Arc::new(Semaphore::new(64)),
        auth: format!("Basic {}", STANDARD.encode(format!("orbit:{token}"))),
        policy,
        store,
        agent,
    });
    let task = tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(64));
        let mut children = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                Some(_) = children.join_next(), if !children.is_empty()=>{},
                res=listener.accept()=>{
                    let Ok((stream,_))=res else {break};
                    let Ok(permit)=permits.clone().try_acquire_owned() else {continue};
                    let session=session.clone();let dial=dial.clone();
                    children.spawn(async move{
                        let _permit=permit;
                        let service=service_fn(move |req| {let s=session.clone();let dial=dial.clone();async move{Ok::<_,std::convert::Infallible>(
                            match tokio::time::timeout(Duration::from_secs(20),forward(req,s,dial)).await{
                                Ok(r)=>r,Err(_)=>response(StatusCode::GATEWAY_TIMEOUT)
                            })}});
                        let _ = tokio::time::timeout(
                            Duration::from_secs(600),
                            hyper::server::conn::http1::Builder::new()
                                .timer(TokioTimer::new())
                                .header_read_timeout(Duration::from_secs(10))
                                .keep_alive(false)
                                .max_buf_size(32768)
                                .serve_connection(TokioIo::new(stream), service)
                                .with_upgrades(),
                        )
                        .await;
                    });
                }
            }
        }
    });
    Ok(SessionProxy {
        url: format!("http://orbit:{token}@127.0.0.1:{port}"),
        listener: task,
        shutdown,
        #[cfg(target_os = "linux")]
        bridge,
    })
}
#[cfg(target_os = "linux")]
struct LinuxBridge {
    task: JoinHandle<()>,
    socket_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
}
#[cfg(target_os = "linux")]
impl LinuxBridge {
    fn bind(port: u16) -> std::io::Result<Self> {
        let dir = tempfile::Builder::new().prefix("orbit-egress-").tempdir()?;
        let socket_path = dir.path().join("socket");
        let socket = tokio::net::UnixListener::bind(&socket_path)?;
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    Some(_)=tasks.join_next(), if !tasks.is_empty()=>{},
                    item=socket.accept(), if tasks.len()<64=>{
                        let Ok((mut src,_))=item else{break};
                        tasks.spawn(async move{if let Ok(mut dst)=TcpStream::connect(("127.0.0.1",port)).await{let _=tokio::io::copy_bidirectional(&mut src,&mut dst).await;}});
                    }
                }
            }
        });
        Ok(Self {
            task,
            socket_path,
            _dir: dir,
        })
    }
}
#[cfg(target_os = "linux")]
impl Drop for LinuxBridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn response(status: StatusCode) -> Response<Body> {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = status;
    if status == StatusCode::PROXY_AUTHENTICATION_REQUIRED {
        r.headers_mut().insert(
            header::PROXY_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"Orbit\""),
        );
    }
    r
}
async fn forward<F>(
    req: Request<Incoming>,
    s: Arc<Session>,
    dial: impl FnOnce(Vec<SocketAddr>) -> F,
) -> Response<Body>
where
    F: std::future::Future<Output = std::io::Result<TcpStream>>,
{
    if !req
        .headers()
        .get(header::PROXY_AUTHORIZATION)
        .is_some_and(|h| orbit_crypto::constant_time_eq(h.as_bytes(), s.auth.as_bytes()))
    {
        s.record("block", "authentication", None).await;
        return response(StatusCode::PROXY_AUTHENTICATION_REQUIRED);
    }
    let connect = req.method() == Method::CONNECT;
    let Some(auth) = req.uri().authority().cloned().filter(|a| {
        !a.as_str().contains('@')
            && if connect {
                a.port_u16().is_some()
            } else {
                req.uri().scheme_str() == Some("http")
            }
    }) else {
        s.record("block", "authority", None).await;
        return response(StatusCode::BAD_REQUEST);
    };
    let host = auth.host().trim_matches(['[', ']']);
    let port = auth.port_u16().unwrap_or(80);
    if !permits(&s.policy, host) {
        s.record("block", "domain", Some(host)).await;
        return response(StatusCode::FORBIDDEN);
    }
    let resolved = match tokio::net::lookup_host((host, port)).await {
        Ok(a) => a.collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };
    if resolved.is_empty() || resolved.iter().any(|a| !public_ip(a.ip())) {
        s.record("block", "address", Some(host)).await;
        return response(StatusCode::FORBIDDEN);
    }
    let tunnel_permit = if connect {
        match s.tunnels.clone().try_acquire_owned() {
            Ok(p) => Some(p),
            Err(_) => return response(StatusCode::SERVICE_UNAVAILABLE),
        }
    } else {
        None
    };
    if !s.record("allow", "policy", Some(host)).await {
        return response(StatusCode::SERVICE_UNAVAILABLE);
    }
    let Ok(stream) = dial(resolved).await else {
        return response(StatusCode::BAD_GATEWAY);
    };
    relay_request(req, auth, stream, tunnel_permit, s).await
}
async fn relay_request(
    mut req: Request<Incoming>,
    auth: Authority,
    mut stream: TcpStream,
    tunnel_permit: Option<tokio::sync::OwnedSemaphorePermit>,
    session: Arc<Session>,
) -> Response<Body> {
    if let Some(permit) = tunnel_permit {
        let mut shutdown = session.shutdown.clone();
        let upgrade = hyper::upgrade::on(&mut req);
        tokio::spawn(async move {
            let _permit = permit;
            if let Ok(upgraded) = upgrade.await {
                let mut client = TokioIo::new(upgraded);
                tokio::select! {_=shutdown.changed()=>{},_=async{
                let _ = tokio::time::timeout(
                    Duration::from_secs(600),
                    tokio::io::copy_bidirectional(&mut client, &mut stream),
                )
                .await;
                }=>{}}
            }
        });
        return response(StatusCode::OK);
    }
    let Ok(host) = HeaderValue::from_str(auth.as_str()) else {
        return response(StatusCode::BAD_REQUEST);
    };
    let nominated = req
        .headers()
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(','))
        .filter_map(|n| axum::http::HeaderName::try_from(n.trim()).ok())
        .collect::<Vec<_>>();
    for name in nominated {
        req.headers_mut().remove(name);
    }
    req.headers_mut().insert(header::HOST, host);
    for name in [
        header::PROXY_AUTHORIZATION,
        header::CONNECTION,
        header::UPGRADE,
        header::TE,
        header::TRAILER,
    ] {
        req.headers_mut().remove(name);
    }
    req.headers_mut().remove("proxy-connection");
    req.headers_mut().remove("keep-alive");
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let Ok(uri) = path.parse::<Uri>() else {
        return response(StatusCode::BAD_REQUEST);
    };
    *req.uri_mut() = uri;
    let Ok((mut sender, conn)) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await
    else {
        return response(StatusCode::BAD_GATEWAY);
    };
    let mut shutdown = session.shutdown.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = shutdown.changed() => {}
            _ = tokio::time::timeout(Duration::from_secs(30), conn) => {}
        }
    });
    sender
        .send_request(req)
        .await
        .map(|r| r.map(Body::new))
        .unwrap_or_else(|_| response(StatusCode::BAD_GATEWAY))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domains_and_special_addresses() {
        let p = SandboxPolicy {
            allow_network: true,
            allowed_domains: vec!["*.example.com".into()],
            denied_domains: vec!["blocked.example.com".into()],
            ..Default::default()
        };
        assert!(permits(&p, "API.EXAMPLE.COM."));
        assert!(!permits(&p, "blocked.example.com"));
        assert!(!permits(&p, "badexample.com"));
        assert!(!permits(&p, "example.com"));
        for ip in [
            "::1",
            "10.1.2.3",
            "169.254.0.1",
            "::ffff:127.0.0.1",
            "fd00::1",
            "64:ff9b::a00:1",
        ] {
            assert!(!public_ip(ip.parse().expect("ip")))
        }
    }
    #[tokio::test]
    async fn forwards_plain_http_and_connect_to_pinned_destination() -> anyhow::Result<()> {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        let upstream = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = upstream.local_addr()?;
        let server = tokio::spawn(async move {
            let (stream, _) = upstream.accept().await?;
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await?;
            assert_eq!(line, "GET /ok HTTP/1.1\r\n");
            loop {
                line.clear();
                stream.read_line(&mut line).await?;
                assert!(!line.to_ascii_lowercase().starts_with("proxy-authorization"));
                if line == "\r\n" {
                    break;
                }
            }
            stream
                .get_mut()
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await?;
            let (mut echo, _) = upstream.accept().await?;
            let mut b = [0; 4];
            echo.read_exact(&mut b).await?;
            echo.write_all(&b).await?;
            Ok::<_, std::io::Error>(())
        });
        let store = Arc::new(OrbitStore::open_in_memory()?);
        let proxy = listen(
            SandboxPolicy {
                allow_network: true,
                allowed_domains: vec!["93.184.216.34".into()],
                ..Default::default()
            },
            store.clone(),
            None,
            move |validated| async move {
                assert_eq!(
                    validated,
                    ["93.184.216.34:80".parse::<SocketAddr>().expect("fixture")]
                );
                TcpStream::connect(addr).await
            },
        )
        .await?;
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(&proxy.url)?)
            .build()?;
        assert_eq!(
            client
                .get("http://93.184.216.34/ok")
                .send()
                .await?
                .text()
                .await?,
            "ok"
        );
        let url = reqwest::Url::parse(&proxy.url)?;
        let mut peer =
            BufReader::new(TcpStream::connect(("127.0.0.1", url.port().expect("port"))).await?);
        let auth = STANDARD.encode(format!("orbit:{}", url.password().expect("token")));
        peer.get_mut().write_all(format!("CONNECT 93.184.216.34:80 HTTP/1.1\r\nHost: 93.184.216.34:80\r\nProxy-Authorization: Basic {auth}\r\n\r\n").as_bytes()).await?;
        let mut line = String::new();
        peer.read_line(&mut line).await?;
        assert!(line.contains("200"));
        loop {
            line.clear();
            peer.read_line(&mut line).await?;
            if line == "\r\n" {
                break;
            }
        }
        peer.get_mut().write_all(b"ping").await?;
        let mut echo = [0; 4];
        peer.read_exact(&mut echo).await?;
        assert_eq!(&echo, b"ping");
        server.await??;
        assert_eq!(store.audit_count(Some("egress"), Some("allow"))?, 2);
        Ok(())
    }
    #[tokio::test]
    async fn authenticated_proxy_blocks_private_destinations() -> anyhow::Result<()> {
        let store = Arc::new(OrbitStore::open_in_memory()?);
        let proxy = start(
            SandboxPolicy {
                allow_network: true,
                ..Default::default()
            },
            store.clone(),
            None,
        )
        .await?;
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(&proxy.url)?)
            .build()?;
        assert_eq!(
            client.get("http://127.0.0.1:1").send().await?.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(store.audit_count(Some("egress"), Some("block"))?, 1);
        Ok(())
    }
}
