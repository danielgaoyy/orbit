//! Built-in 1-click Desktop Self-Updater for macOS (`.dmg`), Windows (`.exe`/`.msi`), and Linux (`.deb`).
//!
//! - Queries GitHub Releases (`<ORBIT_GITHUB_REPO>/releases/latest`, defaulting to [`orbit_protocol::DEFAULT_GITHUB_REPO`])
//! - Streams release artifact with live MB/MB progress to the UI
//! - On macOS: mounts `.dmg` silently, replaces `/Applications/Orbit.app` (or current `.app` bundle)
//!   via `ditto`, runs `xattr -cr` automatically so Gatekeeper never blocks launch, unmounts, and relaunches!
//! - On Windows: downloads the NSIS `.exe` installer and launches it cleanly!
//! - On Linux: extracts/installs `.deb` binary in place (or via `sudo -n dpkg -i`) and relaunches!

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::io::AsyncWriteExt;

pub const CURRENT_APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseUpdateInfo {
    pub current_version: String,
    pub tag_name: String,
    pub latest_version: String,
    pub html_url: String,
    pub notes: String,
    pub has_newer_version: bool,
    pub macos_dmg_url: Option<String>,
    pub linux_arm64_deb_url: Option<String>,
    pub linux_x64_deb_url: Option<String>,
    #[serde(default)]
    pub windows_nsis_url: Option<String>,
    #[serde(default)]
    pub windows_msi_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUpdateProgress {
    pub progress: f64,
    pub status: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDaemonSyncResult {
    /// "updated" | "restarted" | "skipped_dev"
    pub status: String,
    pub daemon_version: Option<String>,
    /// "bundled" | "github" | "existing" | "dev"
    pub source: String,
}

pub fn compare_semver(a: &str, b: &str) -> Ordering {
    fn split_semver(v: &str) -> ([u32; 3], Option<&str>) {
        let without_build = v
            .trim()
            .trim_start_matches(['v', 'V'])
            .split('+')
            .next()
            .unwrap_or("0.0.0");
        let (core, pre) = match without_build.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p)),
            Some((c, _)) => (c, None),
            None => (without_build, None),
        };
        let mut it = core.split('.');
        let parts = [
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
        ];
        (parts, pre)
    }

    fn cmp_prerelease(a: &str, b: &str) -> Ordering {
        let mut a_ids = a.split('.');
        let mut b_ids = b.split('.');
        loop {
            match (a_ids.next(), b_ids.next()) {
                (Some(ai), Some(bi)) => {
                    let a_num = ai.parse::<u64>().ok();
                    let b_num = bi.parse::<u64>().ok();
                    let ord = match (a_num, b_num) {
                        (Some(an), Some(bn)) => an.cmp(&bn),
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => ai.cmp(bi),
                    };
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
                (Some(_), None) => return Ordering::Greater,
                (None, Some(_)) => return Ordering::Less,
                (None, None) => return Ordering::Equal,
            }
        }
    }

    let (a_parts, a_pre) = split_semver(a);
    let (b_parts, b_pre) = split_semver(b);
    match a_parts.cmp(&b_parts) {
        Ordering::Equal => match (a_pre, b_pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(ap), Some(bp)) => cmp_prerelease(ap, bp),
        },
        ord => ord,
    }
}

pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    compare_semver(candidate, current) == Ordering::Greater
}

fn resolve_github_token() -> Option<String> {
    for key in ["ORBIT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(val) = std::env::var(key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn extract_tag_from_release_url(url: &str) -> Option<String> {
    let clean = url.split(['?', '#']).next()?.trim().trim_end_matches('/');
    let (_, after_tag) = clean.rsplit_once("/releases/tag/")?;
    let tag = after_tag.split('/').next()?.trim();
    if tag.is_empty() {
        return None;
    }
    Some(tag.to_string())
}

fn build_release_info_from_tag(tag_name: &str, current_version: &str) -> ReleaseUpdateInfo {
    let repo = orbit_protocol::configured_github_repo();
    let clean_tag = tag_name.trim().to_string();
    let latest_version = clean_tag.trim_start_matches(['v', 'V']).to_string();
    ReleaseUpdateInfo {
        current_version: current_version.to_string(),
        latest_version: latest_version.clone(),
        has_newer_version: compare_semver(&latest_version, current_version) == Ordering::Greater,
        macos_dmg_url: Some(format!(
            "https://github.com/{repo}/releases/download/{clean_tag}/Orbit-macOS-AppleSilicon.dmg"
        )),
        linux_arm64_deb_url: Some(format!(
            "https://github.com/{repo}/releases/download/{clean_tag}/Orbit-Linux-arm64.deb"
        )),
        linux_x64_deb_url: Some(format!(
            "https://github.com/{repo}/releases/download/{clean_tag}/Orbit-Linux-x64.deb"
        )),
        windows_nsis_url: Some(format!(
            "https://github.com/{repo}/releases/download/{clean_tag}/Orbit-Windows-x64-setup.exe"
        )),
        windows_msi_url: Some(format!(
            "https://github.com/{repo}/releases/download/{clean_tag}/Orbit-Windows-x64.msi"
        )),
        html_url: format!("https://github.com/{repo}/releases/tag/{clean_tag}"),
        tag_name: clean_tag,
        notes: String::new(),
    }
}

async fn resolve_latest_tag_via_web_redirect() -> Result<String, String> {
    let repo = orbit_protocol::configured_github_repo();
    let no_redirect_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("Orbit-Desktop/{CURRENT_APP_VERSION}"))
        .build()
        .map_err(|e| format!("Failed to initialize redirect client: {e}"))?;

    let latest_web_url = format!("https://github.com/{repo}/releases/latest");

    // Prefer a lightweight HEAD request (0 body bytes, never consumes api.github.com quota);
    // fall back to GET if a proxy rejects HEAD.
    for use_head in [true, false] {
        let req = if use_head {
            no_redirect_client.head(&latest_web_url)
        } else {
            no_redirect_client.get(&latest_web_url)
        };
        match req.send().await {
            Ok(resp) => {
                if resp.status().is_redirection()
                    && let Some(loc) = resp
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|v| v.to_str().ok())
                    && let Some(tag) = extract_tag_from_release_url(loc)
                {
                    return Ok(tag);
                }
                if let Some(tag) = extract_tag_from_release_url(resp.url().as_str()) {
                    return Ok(tag);
                }
                if !use_head {
                    return Err(format!(
                        "GitHub Releases redirect did not include a version tag (HTTP {})",
                        resp.status()
                    ));
                }
            }
            Err(e) if !use_head => {
                return Err(format!("Failed to connect to GitHub Releases: {e}"));
            }
            Err(_) => {}
        }
    }

    Err("Unable to resolve latest release tag from GitHub Releases redirect".to_string())
}

async fn fetch_release_json(
    client: &reqwest::Client,
    api_url: &str,
) -> Result<serde_json::Value, String> {
    let mut req = client
        .get(api_url)
        .header("Accept", "application/vnd.github+json");
    if let Some(token) = resolve_github_token() {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Failed to connect to GitHub API: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        if status.as_u16() == 403 {
            return Err("GitHub API rate limit exceeded (HTTP 403)".to_string());
        }
        return Err(format!(
            "GitHub API returned unexpected status: HTTP {status}"
        ));
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse release metadata: {e}"))
}

pub async fn check_latest_release() -> Result<ReleaseUpdateInfo, String> {
    let repo = orbit_protocol::configured_github_repo();
    // 1. Primary path: resolve latest release tag via `github.com/.../releases/latest` 302 redirect.
    //    This bypasses `api.github.com` completely and never hits the 60 req/hr unauthenticated limit.
    match resolve_latest_tag_via_web_redirect().await {
        Ok(tag) => {
            let mut info = build_release_info_from_tag(&tag, CURRENT_APP_VERSION);
            // Best-effort enrichment (release notes & exact asset URLs); silently ignore API rate limits.
            if let Ok(api_client) = reqwest::Client::builder()
                .timeout(Duration::from_secs(4))
                .user_agent(format!("Orbit-Desktop/{CURRENT_APP_VERSION}"))
                .build()
            {
                let api_url = format!("https://api.github.com/repos/{repo}/releases/tags/{tag}");
                if let Ok(json) = fetch_release_json(&api_client, &api_url).await {
                    let enriched = parse_github_release_json(&json, CURRENT_APP_VERSION);
                    if !enriched.notes.is_empty() {
                        info.notes = enriched.notes;
                    }
                    if enriched.macos_dmg_url.is_some() {
                        info.macos_dmg_url = enriched.macos_dmg_url;
                    }
                    if enriched.linux_arm64_deb_url.is_some() {
                        info.linux_arm64_deb_url = enriched.linux_arm64_deb_url;
                    }
                    if enriched.linux_x64_deb_url.is_some() {
                        info.linux_x64_deb_url = enriched.linux_x64_deb_url;
                    }
                    if enriched.windows_nsis_url.is_some() {
                        info.windows_nsis_url = enriched.windows_nsis_url;
                    }
                    if enriched.windows_msi_url.is_some() {
                        info.windows_msi_url = enriched.windows_msi_url;
                    }
                }
            }
            Ok(info)
        }
        Err(web_err) => {
            // 2. Fallback path: query GitHub REST API directly in case github.com web is proxied differently.
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .user_agent(format!("Orbit-Desktop/{CURRENT_APP_VERSION}"))
                .build()
                .map_err(|e| format!("Failed to initialize update client: {e}"))?;
            let api_url = format!("https://api.github.com/repos/{repo}/releases/latest");
            match fetch_release_json(&client, &api_url).await {
                Ok(json) => Ok(parse_github_release_json(&json, CURRENT_APP_VERSION)),
                Err(api_err) => Err(format!("{web_err}; API fallback also failed: {api_err}")),
            }
        }
    }
}

fn parse_github_release_json(json: &serde_json::Value, current_version: &str) -> ReleaseUpdateInfo {
    let repo = orbit_protocol::configured_github_repo();
    let tag_name = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("v0.0.0")
        .trim()
        .to_string();
    let latest_version = tag_name.trim_start_matches(['v', 'V']).to_string();
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("https://github.com/{repo}/releases/latest"));
    let notes = json
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let mut dmg_url = None;
    let mut linux_arm_deb = None;
    let mut linux_x64_deb = None;
    let mut windows_nsis = None;
    let mut windows_msi = None;

    if let Some(assets) = json.get("assets").and_then(|v| v.as_array()) {
        for item in assets {
            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let Some(dl) = item
                .get("browser_download_url")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            if name.ends_with(".dmg") {
                dmg_url = Some(dl.to_string());
            } else if name == "Orbit-Linux-arm64.deb" {
                linux_arm_deb = Some(dl.to_string());
            } else if name == "Orbit-Linux-x64.deb" {
                linux_x64_deb = Some(dl.to_string());
            } else if name == "Orbit-Windows-x64-setup.exe"
                || (name.ends_with("-setup.exe") && name.contains("Orbit"))
            {
                windows_nsis = Some(dl.to_string());
            } else if name == "Orbit-Windows-x64.msi"
                || (name.ends_with(".msi") && name.contains("Orbit"))
            {
                windows_msi = Some(dl.to_string());
            }
        }
    }

    ReleaseUpdateInfo {
        current_version: current_version.to_string(),
        latest_version: latest_version.clone(),
        has_newer_version: compare_semver(&latest_version, current_version) == Ordering::Greater,
        macos_dmg_url: dmg_url.or_else(|| {
            Some(format!(
                "https://github.com/{repo}/releases/download/{tag_name}/Orbit-macOS-AppleSilicon.dmg"
            ))
        }),
        linux_arm64_deb_url: linux_arm_deb.or_else(|| {
            Some(format!(
                "https://github.com/{repo}/releases/download/{tag_name}/Orbit-Linux-arm64.deb"
            ))
        }),
        linux_x64_deb_url: linux_x64_deb.or_else(|| {
            Some(format!(
                "https://github.com/{repo}/releases/download/{tag_name}/Orbit-Linux-x64.deb"
            ))
        }),
        windows_nsis_url: windows_nsis.or_else(|| {
            Some(format!(
                "https://github.com/{repo}/releases/download/{tag_name}/Orbit-Windows-x64-setup.exe"
            ))
        }),
        windows_msi_url: windows_msi.or_else(|| {
            Some(format!(
                "https://github.com/{repo}/releases/download/{tag_name}/Orbit-Windows-x64.msi"
            ))
        }),
        tag_name,
        html_url,
        notes,
    }
}

/// Strictly validate a GitHub release tag (`vX.Y.Z` with optional `-prerelease` / `.prerelease`,
/// disallowing path traversal or control characters).
fn normalize_and_validate_release_tag(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(format!("Invalid release tag: {raw}"));
    }
    let normalized = if let Some(rest) = trimmed.strip_prefix(['v', 'V']) {
        format!("v{rest}")
    } else {
        format!("v{trimmed}")
    };
    let ver = &normalized[1..];
    let (core, suffix) = match ver.split_once('-') {
        Some((c, s)) => (c, Some(s)),
        None => (ver, None),
    };
    let mut parts = core.split('.');
    let valid_core = matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(a), Some(b), Some(c), None)
            if !a.is_empty()
                && !b.is_empty()
                && !c.is_empty()
                && a.bytes().all(|ch| ch.is_ascii_digit())
                && b.bytes().all(|ch| ch.is_ascii_digit())
                && c.bytes().all(|ch| ch.is_ascii_digit())
    );
    if !valid_core {
        return Err(format!("Invalid release tag: {raw}"));
    }
    if let Some(pre) = suffix
        && (pre.is_empty()
            || pre.contains("..")
            || !pre
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == b'.' || ch == b'-'))
    {
        return Err(format!("Invalid release tag: {raw}"));
    }
    Ok(normalized)
}

/// Strict allowlist for HTTPS GitHub release-download hosts (blocks SSRF / arbitrary redirects).
fn is_allowed_github_release_download_url(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com"
    )
}

fn build_github_release_download_client(
    connect_timeout: Duration,
    total_timeout: Option<Duration>,
) -> Result<reqwest::Client, String> {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many redirects while downloading GitHub release");
        }
        if !is_allowed_github_release_download_url(attempt.url()) {
            let blocked = attempt.url().to_string();
            return attempt.error(format!("blocked redirect to untrusted host: {blocked}"));
        }
        attempt.follow()
    });
    let mut builder = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .redirect(policy)
        .user_agent(format!("Orbit-Desktop/{CURRENT_APP_VERSION}"));
    if let Some(t) = total_timeout {
        builder = builder.timeout(t);
    }
    builder
        .build()
        .map_err(|e| format!("Failed to create download client: {e}"))
}

/// Create a private `0700` unique staging directory inside `std::env::temp_dir()` using atomic
/// `mkdir` (never follows pre-existing symlinks or reuses predictable PID-named directories).
fn create_private_update_staging_dir() -> Result<PathBuf, String> {
    let base = std::env::temp_dir();
    for _ in 0..16 {
        let rand_hex =
            orbit_crypto::sha256_hex_bytes(orbit_crypto::generate_vault_salt_b64().as_bytes());
        let dir_name = format!("orbit-update-{}", &rand_hex[..24]);
        let candidate = base.join(dir_name);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(format!(
                    "Failed to create private update staging directory: {e}"
                ));
            }
        }
    }
    Err("Failed to allocate unique private update staging directory".to_string())
}

fn create_private_subdir(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let path = parent.join(name);
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&path).map_err(|e| {
        format!(
            "Failed to create temporary subdirectory {}: {e}",
            path.display()
        )
    })?;
    Ok(path)
}

fn fixed_release_download_urls(tag_name: &str, asset_name: &str) -> (String, String) {
    let repo = orbit_protocol::configured_github_repo();
    let base = format!("https://github.com/{repo}/releases/download/{tag_name}");
    (
        format!("{base}/{asset_name}"),
        format!("{base}/{}", orbit_crypto::SHA256SUMS_FILENAME),
    )
}

fn validate_upgrade_target_tag(raw_tag: &str, current_version: &str) -> Result<String, String> {
    let normalized = normalize_and_validate_release_tag(raw_tag)?;
    let target_version = normalized.trim_start_matches(['v', 'V']);
    if !is_newer_version(target_version, current_version) {
        let cur_clean = current_version.trim().trim_start_matches(['v', 'V']);
        return Err(format!(
            "Refusing downgrade or reinstall: target {normalized} is not newer than current v{cur_clean}"
        ));
    }
    Ok(normalized)
}

pub async fn perform_one_click_update(
    target_tag: Option<String>,
    on_progress: Channel<AppUpdateProgress>,
) -> Result<(), String> {
    let raw_tag = match target_tag
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(raw) => raw.to_string(),
        None => resolve_latest_tag_via_web_redirect().await?,
    };
    let tag_name = validate_upgrade_target_tag(&raw_tag, CURRENT_APP_VERSION)?;

    if cfg!(target_os = "macos") {
        return update_macos_dmg_and_relaunch(&tag_name, &on_progress).await;
    }
    if cfg!(target_os = "linux") {
        return update_linux_deb_and_relaunch(&tag_name, &on_progress).await;
    }
    if cfg!(target_os = "windows") {
        return update_windows_installer_and_relaunch(&tag_name, &on_progress).await;
    }
    let repo = orbit_protocol::configured_github_repo();
    Err(format!(
        "Automatic updates are not supported on this platform. Download from https://github.com/{repo}/releases/tag/{tag_name}"
    ))
}

fn emit_progress(
    ch: &Channel<AppUpdateProgress>,
    progress: f64,
    status: impl Into<String>,
    done: bool,
) {
    let _ = ch.send(AppUpdateProgress {
        progress: progress.clamp(0.0, 1.0),
        status: status.into(),
        done,
    });
}

async fn fetch_expected_release_sha256(
    client: &reqwest::Client,
    sums_url: &str,
    asset_name: &str,
) -> Result<String, String> {
    let resp = client
        .get(sums_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch SHA-256 checksum manifest ({sums_url}): {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Failed to fetch SHA-256 checksum manifest ({sums_url}, HTTP {}); refusing unverified update",
            resp.status()
        ));
    }
    let sums_text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read SHA-256 checksum manifest: {e}"))?;
    orbit_crypto::find_expected_sha256(&sums_text, asset_name).map_err(|e| {
        format!("SHA-256 checksum manifest does not contain a valid entry for {asset_name}: {e}")
    })
}

async fn download_and_verify_release_file<F>(
    client: &reqwest::Client,
    asset_url: &str,
    sums_url: &str,
    asset_name: &str,
    dest_path: &Path,
    mut on_chunk_progress: F,
) -> Result<String, String>
where
    F: FnMut(u64, u64),
{
    let expected_sha256 = fetch_expected_release_sha256(client, sums_url, asset_name).await?;

    let resp = client
        .get(asset_url)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Failed to download installer (HTTP {})",
            resp.status()
        ));
    }

    let total_bytes = resp.content_length().unwrap_or(0);
    let mut received_bytes: u64 = 0;
    let mut file = tokio::fs::File::create(dest_path).await.map_err(|e| {
        format!(
            "Failed to create temporary installer {}: {e}",
            dest_path.display()
        )
    })?;

    let mut hasher = orbit_crypto::Sha256Hasher::new();
    let mut stream = resp.bytes_stream();
    let mut stream_err: Option<String> = None;

    while let Some(chunk_res) = stream.next().await {
        let chunk = match chunk_res {
            Ok(c) => c,
            Err(e) => {
                stream_err = Some(format!("Download stream interrupted: {e}"));
                break;
            }
        };
        hasher.update(&chunk);
        if let Err(e) = file.write_all(&chunk).await {
            stream_err = Some(format!("Failed to write installer: {e}"));
            break;
        }
        received_bytes += chunk.len() as u64;
        on_chunk_progress(received_bytes, total_bytes);
    }

    if let Some(err) = stream_err {
        let _ = tokio::fs::remove_file(dest_path).await;
        return Err(err);
    }

    if let Err(e) = file.flush().await {
        let _ = tokio::fs::remove_file(dest_path).await;
        return Err(format!("Failed to flush temporary installer: {e}"));
    }
    drop(file);

    let actual_sha256 = hasher.finalize_hex();
    if let Err(e) = orbit_crypto::verify_sha256_hex(asset_name, &expected_sha256, &actual_sha256) {
        let _ = tokio::fs::remove_file(dest_path).await;
        return Err(format!(
            "Installer SHA-256 integrity verification failed; update aborted: {e}"
        ));
    }

    Ok(actual_sha256)
}

async fn download_with_progress(
    tag_name: &str,
    asset_name: &str,
    dest_path: &Path,
    on_progress: &Channel<AppUpdateProgress>,
) -> Result<(), String> {
    let (asset_url, sums_url) = fixed_release_download_urls(tag_name, asset_name);

    emit_progress(
        on_progress,
        0.03,
        format!("Fetching SHA-256 checksum manifest for {tag_name}…"),
        false,
    );

    let client = build_github_release_download_client(Duration::from_secs(15), None)?;

    let mut last_emit_bytes: u64 = 0;
    download_and_verify_release_file(
        &client,
        &asset_url,
        &sums_url,
        asset_name,
        dest_path,
        |received_bytes, total_bytes| {
            if received_bytes.saturating_sub(last_emit_bytes) >= 128 * 1024
                || (total_bytes > 0 && received_bytes == total_bytes)
            {
                last_emit_bytes = received_bytes;
                let mb_done = received_bytes as f64 / (1024.0 * 1024.0);
                if total_bytes > 0 {
                    let ratio = (received_bytes as f64 / total_bytes as f64).clamp(0.0, 1.0);
                    let mb_total = total_bytes as f64 / (1024.0 * 1024.0);
                    emit_progress(
                        on_progress,
                        0.05 + ratio * 0.80,
                        format!("Downloading {tag_name} ({mb_done:.1} / {mb_total:.1} MB)…"),
                        false,
                    );
                } else {
                    emit_progress(
                        on_progress,
                        0.50,
                        format!("Downloading {tag_name} ({mb_done:.1} MB)…"),
                        false,
                    );
                }
            }
        },
    )
    .await?;

    emit_progress(
        on_progress,
        0.87,
        format!("Verified SHA-256 checksum for {tag_name}…"),
        false,
    );
    Ok(())
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn resolve_target_macos_app_bundle() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let exe_str = exe.to_string_lossy();
        if let Some(idx) = exe_str.find(".app/") {
            let candidate = &exe_str[..idx + 4];
            if !candidate.starts_with("/Volumes/") {
                return PathBuf::from(candidate);
            }
        }
    }
    PathBuf::from("/Applications/Orbit.app")
}

fn spawn_detached_shell(script: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);

        cmd.spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to launch background updater: {e}"))
    }
    #[cfg(not(unix))]
    {
        let _ = script;
        Err("POSIX shell updater is not used on this platform".to_string())
    }
}

async fn update_windows_installer_and_relaunch(
    tag_name: &str,
    on_progress: &Channel<AppUpdateProgress>,
) -> Result<(), String> {
    let staging_dir = create_private_update_staging_dir()?;
    let nsis_asset = "Orbit-Windows-x64-setup.exe";
    let msi_asset = "Orbit-Windows-x64.msi";

    let nsis_path = staging_dir.join("installer.exe");
    let (installer_path, is_msi) =
        match download_with_progress(tag_name, nsis_asset, &nsis_path, on_progress).await {
            Ok(()) => (nsis_path, false),
            Err(nsis_err) if nsis_err.contains("HTTP 404") || nsis_err.contains("not found") => {
                let msi_path = staging_dir.join("installer.msi");
                if let Err(msi_err) =
                    download_with_progress(tag_name, msi_asset, &msi_path, on_progress).await
                {
                    let _ = std::fs::remove_dir_all(&staging_dir);
                    return Err(format!("{nsis_err}; MSI fallback failed: {msi_err}"));
                }
                (msi_path, true)
            }
            Err(err) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                return Err(err);
            }
        };

    emit_progress(on_progress, 0.96, "Launching Windows installer…", true);

    let mut cmd = if is_msi {
        let mut c = Command::new("msiexec");
        c.args(["/i", &installer_path.to_string_lossy()]);
        c
    } else {
        Command::new(&installer_path)
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0200);
    }
    if let Err(e) = cmd.spawn() {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(format!("Failed to launch Windows installer: {e}"));
    }

    tokio::time::sleep(Duration::from_millis(280)).await;
    std::process::exit(0);
}

async fn update_macos_dmg_and_relaunch(
    tag_name: &str,
    on_progress: &Channel<AppUpdateProgress>,
) -> Result<(), String> {
    let staging_dir = create_private_update_staging_dir()?;
    let my_pid = std::process::id();
    let dmg_path = staging_dir.join("installer.dmg");

    if let Err(e) = download_with_progress(
        tag_name,
        "Orbit-macOS-AppleSilicon.dmg",
        &dmg_path,
        on_progress,
    )
    .await
    {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(e);
    }

    emit_progress(
        on_progress,
        0.90,
        "Mounting and verifying macOS package…",
        false,
    );
    let mount_dir = match create_private_subdir(&staging_dir, "mount") {
        Ok(dir) => dir,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return Err(e);
        }
    };

    let attach_out = tokio::process::Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-mountpoint",
            &mount_dir.to_string_lossy(),
            &dmg_path.to_string_lossy(),
        ])
        .output()
        .await
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging_dir);
            format!("Failed to invoke hdiutil: {e}")
        })?;

    if !attach_out.status.success() {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(format!(
            "Failed to mount DMG: {}",
            String::from_utf8_lossy(&attach_out.stderr).trim()
        ));
    }

    let mut mounted_app: Option<PathBuf> = None;
    let direct_orbit = mount_dir.join("Orbit.app");
    if direct_orbit.is_dir() {
        mounted_app = Some(direct_orbit);
    } else if let Ok(rd) = std::fs::read_dir(&mount_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir()
                && p.extension()
                    .and_then(|x| x.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("app"))
                    .unwrap_or(false)
            {
                mounted_app = Some(p);
                break;
            }
        }
    }

    let Some(src_app) = mounted_app else {
        let _ = Command::new("hdiutil")
            .args(["detach", &mount_dir.to_string_lossy(), "-force", "-quiet"])
            .status();
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err("Orbit.app was not found inside the downloaded package".into());
    };

    let target_app = resolve_target_macos_app_bundle();
    emit_progress(
        on_progress,
        0.98,
        format!(
            "Installing to {} and restarting Orbit…",
            target_app.display()
        ),
        true,
    );

    let sq_target = shell_single_quote(&target_app.to_string_lossy());
    let sq_src_app = shell_single_quote(&src_app.to_string_lossy());
    let sq_mount = shell_single_quote(&mount_dir.to_string_lossy());
    let sq_staging = shell_single_quote(&staging_dir.to_string_lossy());

    let updater_script = format!(
        "(
          for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14; do
            kill -0 {my_pid} 2>/dev/null || break
            sleep 0.25
          done
          DEST={sq_target}
          if ! (rm -rf \"$DEST\" && ditto {sq_src_app} \"$DEST\") 2>/dev/null; then
            mkdir -p \"$HOME/Applications\"
            DEST=\"$HOME/Applications/Orbit.app\"
            rm -rf \"$DEST\"
            ditto {sq_src_app} \"$DEST\"
          fi
          xattr -cr \"$DEST\" 2>/dev/null || true
          if [ -x \"$DEST/Contents/MacOS/orbit\" ] && [ -d \"$HOME/.orbit\" ]; then
            PLIST=\"$HOME/Library/LaunchAgents/dev.orbit.daemon.plist\"
            if [ ! -f \"$PLIST\" ] || ! grep -q '/target/debug/' \"$PLIST\" 2>/dev/null; then
              mkdir -p \"$HOME/.local/bin\"
              TMP_ORBIT=\"$HOME/.local/bin/orbit.new.$$\"
              if cp -f \"$DEST/Contents/MacOS/orbit\" \"$TMP_ORBIT\" && chmod 755 \"$TMP_ORBIT\" && mv -f \"$TMP_ORBIT\" \"$HOME/.local/bin/orbit\"; then
                \"$HOME/.local/bin/orbit\" daemon up >/dev/null 2>&1 || true
              else
                rm -f \"$TMP_ORBIT\"
              fi
            fi
          fi
          hdiutil detach {sq_mount} -quiet 2>/dev/null || hdiutil detach {sq_mount} -force -quiet 2>/dev/null || true
          rm -rf {sq_staging}
          open \"$DEST\"
        ) >/dev/null 2>&1 &"
    );

    if let Err(e) = spawn_detached_shell(&updater_script) {
        let _ = Command::new("hdiutil")
            .args(["detach", &mount_dir.to_string_lossy(), "-force", "-quiet"])
            .status();
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(e);
    }
    tokio::time::sleep(Duration::from_millis(280)).await;
    std::process::exit(0);
}

async fn update_linux_deb_and_relaunch(
    tag_name: &str,
    on_progress: &Channel<AppUpdateProgress>,
) -> Result<(), String> {
    let deb_asset = if std::env::consts::ARCH == "aarch64" {
        "Orbit-Linux-arm64.deb"
    } else {
        "Orbit-Linux-x64.deb"
    };

    let staging_dir = create_private_update_staging_dir()?;
    let my_pid = std::process::id();
    let deb_path = staging_dir.join("installer.deb");

    if let Err(e) = download_with_progress(tag_name, deb_asset, &deb_path, on_progress).await {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(e);
    }

    emit_progress(
        on_progress,
        0.90,
        "Extracting and verifying Linux package…",
        false,
    );
    let stage_dir = match create_private_subdir(&staging_dir, "stage") {
        Ok(dir) => dir,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return Err(e);
        }
    };

    let extract_out = tokio::process::Command::new("dpkg-deb")
        .args([
            "-x",
            &deb_path.to_string_lossy(),
            &stage_dir.to_string_lossy(),
        ])
        .output()
        .await
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging_dir);
            format!("Failed to invoke dpkg-deb: {e}")
        })?;
    if !extract_out.status.success() {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(format!(
            "Failed to extract .deb package: {}",
            String::from_utf8_lossy(&extract_out.stderr).trim()
        ));
    }

    let extracted_bin = stage_dir.join("usr/bin/orbit-desktop");
    if !extracted_bin.is_file() {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err("usr/bin/orbit-desktop was not found inside the downloaded package".into());
    }

    let cur_exe =
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/bin/orbit-desktop"));
    emit_progress(
        on_progress,
        0.98,
        "Installing update and restarting Orbit…",
        true,
    );

    let sq_deb = shell_single_quote(&deb_path.to_string_lossy());
    let sq_stage = shell_single_quote(&stage_dir.to_string_lossy());
    let sq_staging = shell_single_quote(&staging_dir.to_string_lossy());
    let sq_extracted = shell_single_quote(&extracted_bin.to_string_lossy());
    let sq_cur_exe = shell_single_quote(&cur_exe.to_string_lossy());

    let updater_script = format!(
        "(
          for i in 1 2 3 4 5 6 7 8 9 10 11 12; do
            kill -0 {my_pid} 2>/dev/null || break
            sleep 0.25
          done
          DEST={sq_cur_exe}
          LAUNCH_TARGET=\"$DEST\"
          IS_USER_LOCAL=0
          case \"$DEST\" in
            \"$HOME\"/*) IS_USER_LOCAL=1 ;;
          esac
          if [ \"$IS_USER_LOCAL\" = 1 ] && ( [ -w \"$DEST\" ] || [ -w \"$(dirname \"$DEST\")\" ] ); then
            TMP_DEST=\"$DEST.new.$$\"
            cp -f {sq_extracted} \"$TMP_DEST\" && chmod 755 \"$TMP_DEST\" && mv -f \"$TMP_DEST\" \"$DEST\"
            WRAPPER=\"$(dirname \"$(dirname \"$DEST\")\")/orbit-desktop\"
            if [ -x \"$WRAPPER\" ] && [ \"$WRAPPER\" != \"$DEST\" ]; then
              LAUNCH_TARGET=\"$WRAPPER\"
            fi
          elif sudo -n dpkg -i {sq_deb} >/dev/null 2>&1; then
            DEST=/usr/bin/orbit-desktop
            LAUNCH_TARGET=\"$DEST\"
          elif [ -w \"$DEST\" ] || [ -w \"$(dirname \"$DEST\")\" ]; then
            TMP_DEST=\"$DEST.new.$$\"
            cp -f {sq_extracted} \"$TMP_DEST\" && chmod 755 \"$TMP_DEST\" && mv -f \"$TMP_DEST\" \"$DEST\"
          else
            mkdir -p \"$HOME/.local/bin\"
            DEST=\"$HOME/.local/bin/orbit-desktop\"
            TMP_DEST=\"$DEST.new.$$\"
            cp -f {sq_extracted} \"$TMP_DEST\" && chmod 755 \"$TMP_DEST\" && mv -f \"$TMP_DEST\" \"$DEST\"
            LAUNCH_TARGET=\"$DEST\"
          fi
          if [ -x {sq_stage}/usr/bin/orbit ] && [ -d \"$HOME/.orbit\" ]; then
            UNIT=\"$HOME/.config/systemd/user/orbit-daemon.service\"
            if [ ! -f \"$UNIT\" ] || ! grep -q '/target/debug/' \"$UNIT\" 2>/dev/null; then
              mkdir -p \"$HOME/.local/bin\"
              TMP_ORBIT=\"$HOME/.local/bin/orbit.new.$$\"
              if cp -f {sq_stage}/usr/bin/orbit \"$TMP_ORBIT\" && chmod 755 \"$TMP_ORBIT\" && mv -f \"$TMP_ORBIT\" \"$HOME/.local/bin/orbit\"; then
                \"$HOME/.local/bin/orbit\" daemon up >/dev/null 2>&1 || true
              else
                rm -f \"$TMP_ORBIT\"
              fi
            fi
          fi
          rm -rf {sq_staging}
          MALLOC_ARENA_MAX=\"${{MALLOC_ARENA_MAX:-2}}\" nohup \"$LAUNCH_TARGET\" >/dev/null 2>&1 &
        ) >/dev/null 2>&1 &"
    );

    if let Err(e) = spawn_detached_shell(&updater_script) {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(e);
    }
    tokio::time::sleep(Duration::from_millis(280)).await;
    std::process::exit(0);
}

fn parse_orbit_cli_version(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("orbit ") {
            let token = rest.split_whitespace().next().unwrap_or("").trim();
            if !token.is_empty() {
                return Some(token.trim_start_matches(['v', 'V']).to_string());
            }
        }
    }
    None
}

async fn probe_orbit_binary_version(bin: &Path) -> Option<String> {
    if !bin.is_file() {
        return None;
    }
    let out = tokio::process::Command::new(bin)
        .arg("--version")
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_orbit_cli_version(&String::from_utf8_lossy(&out.stdout))
}

fn resolve_user_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

fn is_dev_daemon_configured() -> bool {
    let Some(home) = resolve_user_home_dir() else {
        return false;
    };
    let supervisor_files = [
        home.join("Library/LaunchAgents/dev.orbit.daemon.plist"),
        home.join(".config/systemd/user/orbit-daemon.service"),
    ];
    for file in supervisor_files {
        if let Ok(content) = std::fs::read_to_string(&file)
            && content.contains("/target/debug/")
        {
            return true;
        }
    }
    false
}

fn bundled_orbit_sidecar_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let exe_name = if cfg!(windows) { "orbit.exe" } else { "orbit" };
    let candidate = dir.join(exe_name);
    if candidate.is_file() {
        return Some(candidate);
    }
    None
}

fn orbit_cli_release_asset_name() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("orbit-cli-macos-arm64"),
        ("linux", "x86_64") => Ok("orbit-cli-linux-x64"),
        ("linux", "aarch64") => Ok("orbit-cli-linux-arm64"),
        ("windows", "x86_64") => Ok("orbit-windows-x86_64.exe"),
        (os, arch) => Err(format!(
            "Unsupported platform for local daemon sync: {os}/{arch}"
        )),
    }
}

static LOCAL_DAEMON_SYNC_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
    std::sync::OnceLock::new();
static LOCAL_DAEMON_TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_unique_temp_suffix() -> String {
    let seq = LOCAL_DAEMON_TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}.{seq}", std::process::id())
}

fn replace_executable_safely(tmp: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if let Some(parent) = dst.parent()
            && let Some(stem) = dst.file_stem().and_then(|s| s.to_str())
        {
            let stale_prefix = format!("{stem}.exe.old.");
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if name.to_string_lossy().starts_with(&stale_prefix) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
        let backup = if dst.exists() {
            let b = dst.with_extension(format!("exe.old.{}", std::process::id()));
            let _ = std::fs::remove_file(&b);
            std::fs::rename(dst, &b)?;
            Some(b)
        } else {
            None
        };
        if let Err(err) = std::fs::rename(tmp, dst) {
            if let Some(ref b) = backup {
                let _ = std::fs::rename(b, dst);
            }
            return Err(err);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(tmp, dst)
    }
}

fn atomic_install_executable(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    let tmp = dst.with_extension(format!("new.{}", next_unique_temp_suffix()));
    if let Err(e) = std::fs::copy(src, &tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Failed to stage {}: {e}", tmp.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("Failed to chmod {}: {e}", tmp.display()));
        }
    }
    if let Err(e) = replace_executable_safely(&tmp, dst) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Failed to replace {}: {e}", dst.display()));
    }
    Ok(())
}

/// Install the bundled `orbit` CLI sidecar into `~/.local/bin/orbit` ONLY if no local `orbit`
/// binary exists yet. Called strictly on explicit user opt-in ("Connect This Machine").
pub fn ensure_bundled_sidecar_installed() -> Result<(), String> {
    let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return Ok(());
    };
    let exe_name = if cfg!(windows) { "orbit.exe" } else { "orbit" };
    let install_path = home.join(".local/bin").join(exe_name);
    if install_path.is_file() {
        return Ok(());
    }
    if let Some(sidecar) = bundled_orbit_sidecar_path() {
        atomic_install_executable(&sidecar, &install_path)?;
    }
    Ok(())
}

async fn download_orbit_cli_binary(dst_tmp: &Path) -> Result<(), String> {
    let asset = orbit_cli_release_asset_name()?;
    let client = build_github_release_download_client(
        Duration::from_secs(15),
        Some(Duration::from_secs(90)),
    )?;

    let mut release_tags = vec![normalize_and_validate_release_tag(CURRENT_APP_VERSION)?];
    if let Ok(latest_tag) = resolve_latest_tag_via_web_redirect().await
        && let Ok(valid_latest) = normalize_and_validate_release_tag(&latest_tag)
        && !release_tags.iter().any(|t| t == &valid_latest)
    {
        release_tags.push(valid_latest);
    }

    let repo = orbit_protocol::configured_github_repo();
    let mut last_err = String::from("Failed to download Orbit daemon binary");
    for tag in release_tags {
        let base = format!("https://github.com/{repo}/releases/download/{tag}");
        let sums_url = format!("{base}/{}", orbit_crypto::SHA256SUMS_FILENAME);
        let asset_url = format!("{base}/{asset}");

        // Probe whether this release tag exists before failing; never ignore SHA-256 errors
        // once a release tag is present.
        let sums_resp = match client.get(&sums_url).send().await {
            Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => {
                last_err = format!("Release checksums not found at {sums_url} (HTTP 404)");
                continue;
            }
            Ok(resp) if !resp.status().is_success() => {
                return Err(format!(
                    "Failed to fetch release checksums {sums_url} (HTTP {}); refusing unverified daemon sync",
                    resp.status()
                ));
            }
            Ok(resp) => resp,
            Err(e) => {
                last_err = format!("Network error fetching {sums_url}: {e}");
                continue;
            }
        };

        let sums_text = sums_resp
            .text()
            .await
            .map_err(|e| format!("Failed to read {sums_url}: {e}"))?;
        let expected_sha256 = orbit_crypto::find_expected_sha256(&sums_text, asset)
            .map_err(|e| format!("Release checksum manifest {sums_url} rejected `{asset}`: {e}"))?;

        let resp = client
            .get(&asset_url)
            .send()
            .await
            .map_err(|e| format!("Network error fetching {asset_url}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {} fetching {asset_url}", resp.status()));
        }

        let mut file = tokio::fs::File::create(dst_tmp)
            .await
            .map_err(|e| format!("Failed to create temp binary: {e}"))?;
        let mut hasher = orbit_crypto::Sha256Hasher::new();
        let mut stream = resp.bytes_stream();
        let mut write_failed: Option<String> = None;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    hasher.update(&bytes);
                    if let Err(e) = file.write_all(&bytes).await {
                        write_failed = Some(format!("Failed to write binary: {e}"));
                        break;
                    }
                }
                Err(e) => {
                    write_failed = Some(format!("Download interrupted: {e}"));
                    break;
                }
            }
        }
        if let Some(err) = write_failed {
            let _ = tokio::fs::remove_file(dst_tmp).await;
            return Err(err);
        }
        if let Err(e) = file.flush().await {
            let _ = tokio::fs::remove_file(dst_tmp).await;
            return Err(format!("Failed to flush binary: {e}"));
        }
        drop(file);

        let actual_sha256 = hasher.finalize_hex();
        if let Err(e) = orbit_crypto::verify_sha256_hex(asset, &expected_sha256, &actual_sha256) {
            let _ = tokio::fs::remove_file(dst_tmp).await;
            return Err(format!("SHA-256 verification failed for {asset_url}: {e}"));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) =
                std::fs::set_permissions(dst_tmp, std::fs::Permissions::from_mode(0o755))
            {
                let _ = std::fs::remove_file(dst_tmp);
                return Err(format!("Failed to chmod downloaded binary: {e}"));
            }
        }
        return Ok(());
    }
    Err(last_err)
}

/// Silently synchronize & hot-restart the local `orbit daemon` to match Desktop's version.
///
/// - Preserves all running `orbit shim` agent & PTY sessions uninterrupted.
/// - Prefers the bundled `orbit` sidecar inside `Orbit.app/Contents/MacOS/orbit` (0ms network, offline-capable).
/// - Falls back to downloading the matching release binary if no sidecar is bundled.
/// - Skips automatic overwrite in dev builds (`debug_assertions` or `/target/debug/` LaunchAgent) unless `force` is true.
pub async fn sync_local_daemon(force: bool) -> Result<LocalDaemonSyncResult, String> {
    if !force && (cfg!(debug_assertions) || is_dev_daemon_configured()) {
        return Ok(LocalDaemonSyncResult {
            status: "skipped_dev".to_string(),
            daemon_version: None,
            source: "dev".to_string(),
        });
    }

    let _sync_guard = LOCAL_DAEMON_SYNC_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;

    let home = resolve_user_home_dir()
        .ok_or_else(|| "Unable to resolve user home directory".to_string())?;
    let install_dir = home.join(".local").join("bin");
    let exe_name = if cfg!(windows) { "orbit.exe" } else { "orbit" };
    let install_path = install_dir.join(exe_name);

    let mut source = "existing";
    let mut installed_version = probe_orbit_binary_version(&install_path).await;

    // 1. Prefer bundled sidecar if it matches or is newer than what is currently installed
    // (never silently downgrade a newer CLI binary already present on disk).
    if let Some(sidecar) = bundled_orbit_sidecar_path()
        && let Some(sidecar_ver) = probe_orbit_binary_version(&sidecar).await
    {
        let should_copy = installed_version
            .as_deref()
            .map(|cur| {
                let ord = compare_semver(&sidecar_ver, cur);
                ord == Ordering::Greater || (force && ord == Ordering::Equal)
            })
            .unwrap_or(true);
        if should_copy {
            atomic_install_executable(&sidecar, &install_path)?;
            installed_version = Some(sidecar_ver);
            source = "bundled";
        }
    }

    // 2. Fallback to GitHub Release download if installed CLI is missing or older than Desktop
    let still_outdated = installed_version
        .as_deref()
        .map(|cur| compare_semver(cur, CURRENT_APP_VERSION) == Ordering::Less)
        .unwrap_or(true);
    if still_outdated {
        std::fs::create_dir_all(&install_dir)
            .map_err(|e| format!("Failed to create {}: {e}", install_dir.display()))?;
        let tmp_name = if cfg!(windows) {
            format!("orbit.dl.{}.exe", next_unique_temp_suffix())
        } else {
            format!("orbit.dl.{}", next_unique_temp_suffix())
        };
        let tmp_dl = install_dir.join(tmp_name);
        let dl_res = download_orbit_cli_binary(&tmp_dl).await;
        if let Err(err) = dl_res {
            let _ = std::fs::remove_file(&tmp_dl);
            // If force-restarting an existing binary and offline, still allow restarting existing binary
            if force && installed_version.is_some() {
                tracing::warn!(error = %err, "Offline fallback: restarting existing local orbit binary");
            } else {
                return Err(err);
            }
        } else {
            let verified_ver = probe_orbit_binary_version(&tmp_dl).await;
            if verified_ver.is_none() {
                let _ = std::fs::remove_file(&tmp_dl);
                return Err("Downloaded orbit binary failed self-check (--version)".to_string());
            }
            if let Err(e) = replace_executable_safely(&tmp_dl, &install_path) {
                let _ = std::fs::remove_file(&tmp_dl);
                return Err(format!("Failed to install {}: {e}", install_path.display()));
            }
            installed_version = verified_ver;
            source = "github";
        }
    }

    if !install_path.is_file() {
        return Err("Local orbit binary is not installed".to_string());
    }

    // 3. Hot-restart local daemon cleanly (all `orbit shim` sessions survive uninterrupted)
    let out = tokio::process::Command::new(&install_path)
        .args(["daemon", "up"])
        .output()
        .await
        .map_err(|e| format!("Failed to run `orbit daemon up`: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(if detail.is_empty() {
            format!("`orbit daemon up` exited with {}", out.status)
        } else {
            detail
        });
    }

    Ok(LocalDaemonSyncResult {
        status: if source == "existing" {
            "restarted".to_string()
        } else {
            "updated".to_string()
        },
        daemon_version: installed_version,
        source: source.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_orbit_cli_version_extracts_semver() {
        assert_eq!(
            parse_orbit_cli_version("orbit 0.5.5\n").as_deref(),
            Some("0.5.5")
        );
        assert_eq!(
            parse_orbit_cli_version("orbit v0.6.0 (abc123)\n").as_deref(),
            Some("0.6.0")
        );
        assert_eq!(parse_orbit_cli_version("unknown output"), None);
    }

    #[test]
    fn semver_comparison_handles_v_prefix_and_prerelease() {
        assert_eq!(compare_semver("v0.2.4", "0.2.3"), Ordering::Greater);
        assert_eq!(compare_semver("0.2.3", "v0.2.3"), Ordering::Equal);
        assert_eq!(compare_semver("0.2.2", "0.2.3"), Ordering::Less);
        assert_eq!(compare_semver("1.0.0", "0.9.9"), Ordering::Greater);
        assert_eq!(compare_semver("v0.9.1-rc.1", "0.9.1"), Ordering::Less);
        assert_eq!(compare_semver("0.9.1", "0.9.1-rc.1"), Ordering::Greater);
        assert_eq!(
            compare_semver("0.9.1-rc.2", "0.9.1-rc.1"),
            Ordering::Greater
        );
        assert_eq!(
            compare_semver("0.9.1-rc.10", "0.9.1-rc.2"),
            Ordering::Greater
        );
    }

    #[test]
    fn validate_upgrade_target_tag_rejects_older_and_equal_versions() {
        assert_eq!(
            validate_upgrade_target_tag("v0.9.2", "0.9.1").as_deref(),
            Ok("v0.9.2")
        );
        assert_eq!(
            validate_upgrade_target_tag("0.9.2-rc-1", "0.9.1").as_deref(),
            Ok("v0.9.2-rc-1")
        );
        assert_eq!(
            validate_upgrade_target_tag("v0.9.1", "0.9.1-rc.1").as_deref(),
            Ok("v0.9.1")
        );
        for blocked in [
            "v0.9.1",
            "0.9.1",
            "v0.9.0",
            "0.1.0",
            "v0.9.1-rc.1",
            "v0.9.1-alpha-1",
            "../v9.9.9",
        ] {
            assert!(
                validate_upgrade_target_tag(blocked, "0.9.1").is_err(),
                "expected downgrade/equal/invalid tag `{blocked}` to be rejected"
            );
        }
    }

    #[test]
    fn extract_tag_from_release_redirect_handles_absolute_relative_and_invalid_urls() {
        assert_eq!(
            extract_tag_from_release_url(
                "https://github.com/danielgaoyy/orbit/releases/tag/v0.5.4"
            )
            .as_deref(),
            Some("v0.5.4")
        );
        assert_eq!(
            extract_tag_from_release_url("/danielgaoyy/orbit/releases/tag/v0.5.4/?foo=1#top")
                .as_deref(),
            Some("v0.5.4")
        );
        assert_eq!(
            extract_tag_from_release_url("https://github.com/danielgaoyy/orbit/releases/latest"),
            None
        );
        assert_eq!(
            extract_tag_from_release_url("https://github.com/danielgaoyy/orbit/releases/tag/"),
            None
        );
    }

    #[test]
    fn build_release_info_from_tag_populates_platform_urls_without_api() {
        let repo = orbit_protocol::configured_github_repo();
        let info = build_release_info_from_tag("v0.5.4", "0.5.3");
        assert!(info.has_newer_version);
        assert_eq!(info.tag_name, "v0.5.4");
        assert_eq!(info.latest_version, "0.5.4");
        assert_eq!(
            info.macos_dmg_url.as_deref(),
            Some(
                format!(
                    "https://github.com/{repo}/releases/download/v0.5.4/Orbit-macOS-AppleSilicon.dmg"
                )
                .as_str()
            )
        );
        assert_eq!(
            info.linux_arm64_deb_url.as_deref(),
            Some(
                format!("https://github.com/{repo}/releases/download/v0.5.4/Orbit-Linux-arm64.deb")
                    .as_str()
            )
        );
        assert_eq!(
            info.linux_x64_deb_url.as_deref(),
            Some(
                format!("https://github.com/{repo}/releases/download/v0.5.4/Orbit-Linux-x64.deb")
                    .as_str()
            )
        );
        let (asset_url, sums_url) = fixed_release_download_urls("v0.5.4", "Orbit-Linux-x64.deb");
        assert_eq!(
            asset_url,
            format!("https://github.com/{repo}/releases/download/v0.5.4/Orbit-Linux-x64.deb")
        );
        assert_eq!(
            sums_url,
            format!("https://github.com/{repo}/releases/download/v0.5.4/SHA256SUMS.txt")
        );
    }

    #[test]
    fn release_tag_validation_and_github_redirect_allowlist_block_ssrf() {
        assert_eq!(
            normalize_and_validate_release_tag("v0.9.1").as_deref(),
            Ok("v0.9.1")
        );
        assert_eq!(
            normalize_and_validate_release_tag("0.9.2-rc.1").as_deref(),
            Ok("v0.9.2-rc.1")
        );
        for bad in [
            "",
            "../evil",
            "v0.9.1/../../etc/passwd",
            "v0.9.1?foo=bar",
            "https://evil.example/v0.9.1",
            "v0.9.1-a..b",
        ] {
            assert!(
                normalize_and_validate_release_tag(bad).is_err(),
                "expected invalid tag `{bad}` to be rejected"
            );
        }

        for ok_url in [
            "https://github.com/danielgaoyy/orbit/releases/download/v0.9.1/Orbit-macOS-AppleSilicon.dmg",
            "https://objects.githubusercontent.com/github-production-release-asset-123/file",
            "https://release-assets.githubusercontent.com/github-production-release-asset-123/file",
        ] {
            let parsed = reqwest::Url::parse(ok_url).expect("parse ok url");
            assert!(
                is_allowed_github_release_download_url(&parsed),
                "expected `{ok_url}` to be allowed"
            );
        }
        for bad_url in [
            "http://github.com/danielgaoyy/orbit/releases/download/v0.9.1/a.dmg",
            "https://github.com.evil.example/file",
            "https://127.0.0.1:8080/SHA256SUMS.txt",
            "https://169.254.169.254/latest/meta-data/",
            "https://evil.example/Orbit-macOS-AppleSilicon.dmg",
        ] {
            let parsed = reqwest::Url::parse(bad_url).expect("parse bad url");
            assert!(
                !is_allowed_github_release_download_url(&parsed),
                "expected `{bad_url}` to be blocked"
            );
        }
    }

    #[test]
    fn private_update_staging_dir_is_unique_and_mode_0700() {
        let dir1 = create_private_update_staging_dir().expect("create staging dir 1");
        let dir2 = create_private_update_staging_dir().expect("create staging dir 2");
        assert_ne!(dir1, dir2);
        assert!(dir1.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dir1)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700, "staging dir must be strictly 0700");
        }
        let _ = std::fs::remove_dir_all(&dir1);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[tokio::test]
    async fn desktop_installer_download_enforces_sha256sums() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let genuine: &'static [u8] = b"genuine-orbit-desktop-dmg-payload-bytes";
        let tampered: &'static [u8] = b"tampered-orbit-desktop-dmg-payload-bytes";
        let genuine_sha = orbit_crypto::sha256_hex_bytes(genuine);
        let valid_sums = format!("{genuine_sha}  Orbit-macOS-AppleSilicon.dmg\n");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let sums = valid_sums.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let Ok(n) = stream.read(&mut buf).await else {
                        return;
                    };
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (status, body): (&str, Vec<u8>) = match path {
                        "/ok/SHA256SUMS.txt" | "/tampered/SHA256SUMS.txt" => {
                            ("200 OK", sums.into_bytes())
                        }
                        "/ok/Orbit-macOS-AppleSilicon.dmg"
                        | "/no-sums/Orbit-macOS-AppleSilicon.dmg" => ("200 OK", genuine.to_vec()),
                        "/tampered/Orbit-macOS-AppleSilicon.dmg" => ("200 OK", tampered.to_vec()),
                        _ => ("404 Not Found", b"not found".to_vec()),
                    };
                    let header = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&body).await;
                });
            }
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let tmp_dir = std::env::temp_dir().join(format!(
            "orbit_desktop_updater_sha256_{}",
            next_unique_temp_suffix()
        ));
        std::fs::create_dir_all(&tmp_dir).expect("mkdir");

        // 1. Valid SHA256 passes
        let ok_dest = tmp_dir.join("ok.dmg");
        let verified = download_and_verify_release_file(
            &client,
            &format!("http://{addr}/ok/Orbit-macOS-AppleSilicon.dmg"),
            &format!("http://{addr}/ok/SHA256SUMS.txt"),
            "Orbit-macOS-AppleSilicon.dmg",
            &ok_dest,
            |_, _| {},
        )
        .await
        .expect("matching sha256 must pass");
        assert_eq!(verified, genuine_sha);
        assert_eq!(std::fs::read(&ok_dest).expect("read ok"), genuine);

        // 2. Tampered DMG fails and deletes staged file before mount/launch
        let bad_dest = tmp_dir.join("tampered.dmg");
        let err = download_and_verify_release_file(
            &client,
            &format!("http://{addr}/tampered/Orbit-macOS-AppleSilicon.dmg"),
            &format!("http://{addr}/tampered/SHA256SUMS.txt"),
            "Orbit-macOS-AppleSilicon.dmg",
            &bad_dest,
            |_, _| {},
        )
        .await
        .expect_err("tampered dmg must fail");
        assert!(err.contains("SHA-256"), "unexpected error: {err}");
        assert!(!bad_dest.exists(), "tampered installer must be removed");

        // 3. Missing SHA256SUMS.txt fails cleanly
        let nosums_dest = tmp_dir.join("nosums.dmg");
        let err = download_and_verify_release_file(
            &client,
            &format!("http://{addr}/no-sums/Orbit-macOS-AppleSilicon.dmg"),
            &format!("http://{addr}/no-sums/SHA256SUMS.txt"),
            "Orbit-macOS-AppleSilicon.dmg",
            &nosums_dest,
            |_, _| {},
        )
        .await
        .expect_err("missing SHA256SUMS.txt must fail");
        assert!(err.contains("404"), "unexpected error: {err}");
        assert!(!nosums_dest.exists());

        server.abort();
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
