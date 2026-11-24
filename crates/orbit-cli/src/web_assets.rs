//! Foreground-only static asset hosting; all opens are relative to a confined directory handle.
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use cap_std::{ambient_authority, fs::Dir};

const CSP: &str = "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; connect-src 'self' wss: ws:; manifest-src 'self'; worker-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

pub fn router(directory: PathBuf) -> anyhow::Result<Router> {
    let root = Dir::open_ambient_dir(directory, ambient_authority())?;
    root.open("index.html")?;
    Ok(Router::new()
        .fallback(get(asset))
        .with_state(Arc::new(root)))
}

async fn asset(State(root): State<Arc<Dir>>, uri: Uri) -> Response {
    let raw = uri.path().strip_prefix('/').unwrap_or("");
    let name = if raw.is_empty() { "index.html" } else { raw };
    let path = Path::new(name);
    if name.contains(['%', '\\', ':'])
        || !path.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mime = match path.extension().and_then(|s| s.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("wasm") => "application/wasm",
        Some("webmanifest") => "application/manifest+json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    const MAX_STATIC_ASSET_BYTES: u64 = 32 * 1024 * 1024;

    let path = path.to_owned();
    match tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let meta = root.metadata(&path)?;
        if !meta.is_file() || meta.len() > MAX_STATIC_ASSET_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unsupported static asset",
            ));
        }
        root.read(&path)
    })
    .await
    {
        Ok(Ok(bytes)) => (
            [
                (header::CONTENT_TYPE, mime),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::REFERRER_POLICY, "no-referrer"),
                (header::CONTENT_SECURITY_POLICY, CSP),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            Body::from(bytes),
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}
