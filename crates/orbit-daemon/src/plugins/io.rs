//! Descriptor-relative file reads and DNS-pinned HTTPS for the guest imports and installer.
use std::{io::Read, path::Path, time::Duration};

use anyhow::{Context, Result, ensure};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use orbit_protocol::valid_plugin_relative_path;

use crate::{goals::validate_and_pin_http_health_url, mcp_servers::is_blocked_ssrf_ip};

pub(super) fn open_root(canonical: &Path) -> Result<Dir> {
    // The RPC already authorized this canonical path. Do not resolve it a second
    // time: pin each directory without following a subsequently substituted link.
    ensure!(
        canonical.is_absolute(),
        "expected a canonical workspace path"
    );
    #[cfg(windows)]
    {
        return Ok(Dir::open_ambient_dir(canonical, ambient_authority())?);
    }
    #[cfg(not(windows))]
    {
        let anchor = canonical
            .ancestors()
            .last()
            .context("missing filesystem root")?;
        let mut dir = Dir::open_ambient_dir(anchor, ambient_authority())?;
        for part in canonical.components() {
            if let std::path::Component::Normal(name) = part {
                dir = dir.open_dir_nofollow(name)?;
            }
        }
        Ok(dir)
    }
}

pub(super) fn read_relative(root: &Dir, name: &str, cap: usize) -> Result<Vec<u8>> {
    ensure!(
        valid_plugin_relative_path(name),
        "invalid relative file path"
    );
    let mut dir = root.try_clone()?;
    let mut parts = name.split('/');
    let file_name = parts.next_back().context("missing file name")?;
    for part in parts {
        dir = dir.open_dir_nofollow(part)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    cap_std::fs::OpenOptionsExt::custom_flags(
        &mut options,
        rustix::fs::OFlags::NONBLOCK.bits() as i32,
    );
    let file = dir.open_with(file_name, &options)?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.len() <= cap as u64,
        "expected a bounded regular file"
    );
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= cap, "file exceeds byte limit");
    Ok(bytes)
}

pub(super) async fn https_get(raw: &str, domains: &[String], cap: usize) -> Result<Vec<u8>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let url = reqwest::Url::parse(raw)?;
        ensure!(
            url.scheme() == "https"
                && url.port_or_known_default() == Some(443)
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none(),
            "plugin requires HTTPS on port 443 without embedded credentials or fragment"
        );
        ensure!(
            domains.iter().any(|d| Some(d.as_str()) == url.host_str()),
            "plugin HTTP domain denied"
        );
        let (url, host, addrs) = validate_and_pin_http_health_url(url.as_str())
            .await
            .map_err(anyhow::Error::msg)?;
        ensure!(
            !addrs.iter().any(|a| is_blocked_ssrf_ip(a.ip())),
            "blocked plugin HTTP address"
        );
        let mut response = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(&host, &addrs)
            .user_agent("Orbit-Plugins/1")
            .build()?
            .get(url)
            .send()
            .await?;
        ensure!(
            response.status().is_success(),
            "HTTP response {}",
            response.status()
        );
        ensure!(
            response.content_length().is_none_or(|n| n <= cap as u64),
            "HTTP response exceeds byte limit"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                chunk.len() <= cap - bytes.len(),
                "HTTP response exceeds byte limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    })
    .await
    .context("plugin HTTP deadline exceeded")?
}
