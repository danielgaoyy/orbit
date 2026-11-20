//! Built-in 1-click Desktop Self-Updater for macOS (`.dmg`) and Linux (`.deb`).
//!
//! - Queries GitHub Releases (`danielgaoyy/orbit/releases/latest`)
//! - Streams release artifact with live MB/MB progress to the UI
//! - On macOS: mounts `.dmg` silently, replaces `/Applications/Orbit.app` (or current `.app` bundle)
//!   via `ditto`, runs `xattr -cr` automatically so Gatekeeper never blocks launch, unmounts, and relaunches!
//! - On Linux: extracts/installs `.deb` binary in place (or via `sudo -n dpkg -i`) and relaunches!

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::io::AsyncWriteExt;

pub const GITHUB_REPO: &str = "danielgaoyy/orbit";
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUpdateProgress {
    pub progress: f64,
    pub status: String,
    pub done: bool,
}

pub fn compare_semver(a: &str, b: &str) -> Ordering {
    fn parse_parts(v: &str) -> [u32; 3] {
        let clean = v
            .trim()
            .trim_start_matches(['v', 'V'])
            .split(['+', '-'])
            .next()
            .unwrap_or("0.0.0");
        let mut it = clean.split('.');
        [
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
        ]
    }
    parse_parts(a).cmp(&parse_parts(b))
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
    let clean_tag = tag_name.trim().to_string();
    let latest_version = clean_tag.trim_start_matches(['v', 'V']).to_string();
    ReleaseUpdateInfo {
        current_version: current_version.to_string(),
        latest_version: latest_version.clone(),
        has_newer_version: compare_semver(&latest_version, current_version) == Ordering::Greater,
        macos_dmg_url: Some(format!(
            "https://github.com/{GITHUB_REPO}/releases/download/{clean_tag}/Orbit-macOS-AppleSilicon.dmg"
        )),
        linux_arm64_deb_url: Some(format!(
            "https://github.com/{GITHUB_REPO}/releases/download/{clean_tag}/Orbit-Linux-arm64.deb"
        )),
        linux_x64_deb_url: Some(format!(
            "https://github.com/{GITHUB_REPO}/releases/download/{clean_tag}/Orbit-Linux-x64.deb"
        )),
        html_url: format!("https://github.com/{GITHUB_REPO}/releases/tag/{clean_tag}"),
        tag_name: clean_tag,
        notes: String::new(),
    }
}

async fn resolve_latest_tag_via_web_redirect() -> Result<String, String> {
    let no_redirect_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("Orbit-Desktop/{CURRENT_APP_VERSION}"))
        .build()
        .map_err(|e| format!("初始化重定向客户端失败: {e}"))?;

    let latest_web_url = format!("https://github.com/{GITHUB_REPO}/releases/latest");

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
                        "GitHub Releases 重定向未包含版本标签 (HTTP {})",
                        resp.status()
                    ));
                }
            }
            Err(e) if !use_head => {
                return Err(format!("连接 GitHub Releases 失败: {e}"));
            }
            Err(_) => {}
        }
    }

    Err("无法从 GitHub Releases 重定向解析最新版本".to_string())
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
        .map_err(|e| format!("连接 GitHub API 失败: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        if status.as_u16() == 403 {
            return Err("GitHub API 触发速率限制 (HTTP 403)".to_string());
        }
        return Err(format!("GitHub API 返回状态异常: HTTP {status}"));
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("解析版本信息失败: {e}"))
}

pub async fn check_latest_release() -> Result<ReleaseUpdateInfo, String> {
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
                let api_url =
                    format!("https://api.github.com/repos/{GITHUB_REPO}/releases/tags/{tag}");
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
                .map_err(|e| format!("初始化更新客户端失败: {e}"))?;
            let api_url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
            match fetch_release_json(&client, &api_url).await {
                Ok(json) => Ok(parse_github_release_json(&json, CURRENT_APP_VERSION)),
                Err(api_err) => Err(format!("{web_err}；API 回退亦失败: {api_err}")),
            }
        }
    }
}

fn parse_github_release_json(json: &serde_json::Value, current_version: &str) -> ReleaseUpdateInfo {
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
        .unwrap_or_else(|| format!("https://github.com/{GITHUB_REPO}/releases/latest"));
    let notes = json
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let mut dmg_url = None;
    let mut linux_arm_deb = None;
    let mut linux_x64_deb = None;

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
            }
        }
    }

    ReleaseUpdateInfo {
        current_version: current_version.to_string(),
        latest_version: latest_version.clone(),
        has_newer_version: compare_semver(&latest_version, current_version) == Ordering::Greater,
        macos_dmg_url: dmg_url.or_else(|| {
            Some(format!(
                "https://github.com/{GITHUB_REPO}/releases/download/{tag_name}/Orbit-macOS-AppleSilicon.dmg"
            ))
        }),
        linux_arm64_deb_url: linux_arm_deb.or_else(|| {
            Some(format!(
                "https://github.com/{GITHUB_REPO}/releases/download/{tag_name}/Orbit-Linux-arm64.deb"
            ))
        }),
        linux_x64_deb_url: linux_x64_deb.or_else(|| {
            Some(format!(
                "https://github.com/{GITHUB_REPO}/releases/download/{tag_name}/Orbit-Linux-x64.deb"
            ))
        }),
        tag_name,
        html_url,
        notes,
    }
}

pub async fn perform_one_click_update(
    release: ReleaseUpdateInfo,
    on_progress: Channel<AppUpdateProgress>,
) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        return update_macos_dmg_and_relaunch(&release, &on_progress).await;
    }
    if cfg!(target_os = "linux") {
        return update_linux_deb_and_relaunch(&release, &on_progress).await;
    }
    Err(format!("当前平台请前往 {} 下载安装", release.html_url))
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

async fn download_with_progress(
    url: &str,
    tag_name: &str,
    dest_path: &Path,
    on_progress: &Channel<AppUpdateProgress>,
) -> Result<(), String> {
    emit_progress(
        on_progress,
        0.03,
        format!("正在连接 GitHub Release 下载 {tag_name}…"),
        false,
    );

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .user_agent(format!("Orbit-Desktop/{CURRENT_APP_VERSION}"))
        .build()
        .map_err(|e| format!("创建下载连接失败: {e}"))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载安装包失败 (HTTP {})", resp.status()));
    }

    let total_bytes = resp.content_length().unwrap_or(0);
    let mut received_bytes: u64 = 0;
    let mut file = tokio::fs::File::create(dest_path)
        .await
        .map_err(|e| format!("无法写入临时安装包 {}: {e}", dest_path.display()))?;

    let mut stream = resp.bytes_stream();
    let mut last_emit_bytes: u64 = 0;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| format!("网络流式下载中断: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入安装包失败: {e}"))?;
        received_bytes += chunk.len() as u64;

        if received_bytes.saturating_sub(last_emit_bytes) >= 128 * 1024
            || received_bytes == total_bytes
        {
            last_emit_bytes = received_bytes;
            let mb_done = received_bytes as f64 / (1024.0 * 1024.0);
            if total_bytes > 0 {
                let ratio = (received_bytes as f64 / total_bytes as f64).clamp(0.0, 1.0);
                let mb_total = total_bytes as f64 / (1024.0 * 1024.0);
                emit_progress(
                    on_progress,
                    0.05 + ratio * 0.80,
                    format!("正在下载 {tag_name} ({mb_done:.1} / {mb_total:.1} MB)…"),
                    false,
                );
            } else {
                emit_progress(
                    on_progress,
                    0.50,
                    format!("正在下载 {tag_name} ({mb_done:.1} MB)…"),
                    false,
                );
            }
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("刷新临时文件失败: {e}"))?;
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
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("无法拉起后台更新进程: {e}"))
}

async fn update_macos_dmg_and_relaunch(
    release: &ReleaseUpdateInfo,
    on_progress: &Channel<AppUpdateProgress>,
) -> Result<(), String> {
    let dmg_url = release
        .macos_dmg_url
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "未找到 macOS 安装包下载链接".to_string())?;

    let tmp_dir = std::env::temp_dir();
    let my_pid = std::process::id();
    let dmg_path = tmp_dir.join(format!("Orbit-{}-{my_pid}.dmg", release.latest_version));
    let mount_dir = tmp_dir.join(format!("orbit_mnt_{my_pid}"));

    download_with_progress(dmg_url, &release.tag_name, &dmg_path, on_progress).await?;

    emit_progress(on_progress, 0.90, "正在挂载并校验 macOS 安装包…", false);
    std::fs::create_dir_all(&mount_dir).map_err(|e| format!("创建挂载目录失败: {e}"))?;

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
        .map_err(|e| format!("调用 hdiutil 失败: {e}"))?;

    if !attach_out.status.success() {
        return Err(format!(
            "挂载 DMG 失败: {}",
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
        return Err("安装包内未找到 Orbit.app".into());
    };

    let target_app = resolve_target_macos_app_bundle();
    emit_progress(
        on_progress,
        0.98,
        format!("正在自动替换 {} 并重启新版 Orbit…", target_app.display()),
        true,
    );

    let sq_target = shell_single_quote(&target_app.to_string_lossy());
    let sq_src_app = shell_single_quote(&src_app.to_string_lossy());
    let sq_mount = shell_single_quote(&mount_dir.to_string_lossy());
    let sq_dmg = shell_single_quote(&dmg_path.to_string_lossy());

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
          hdiutil detach {sq_mount} -quiet 2>/dev/null || hdiutil detach {sq_mount} -force -quiet 2>/dev/null || true
          rm -f {sq_dmg}
          open \"$DEST\"
        ) >/dev/null 2>&1 &"
    );

    spawn_detached_shell(&updater_script)?;
    tokio::time::sleep(Duration::from_millis(280)).await;
    std::process::exit(0);
}

async fn update_linux_deb_and_relaunch(
    release: &ReleaseUpdateInfo,
    on_progress: &Channel<AppUpdateProgress>,
) -> Result<(), String> {
    let deb_url = if std::env::consts::ARCH == "aarch64" {
        release.linux_arm64_deb_url.as_deref()
    } else {
        release.linux_x64_deb_url.as_deref()
    }
    .filter(|s| !s.is_empty())
    .ok_or_else(|| "未找到当前架构的 Linux 桌面安装包链接".to_string())?;

    let tmp_dir = std::env::temp_dir();
    let my_pid = std::process::id();
    let deb_path = tmp_dir.join(format!("Orbit-{}-{my_pid}.deb", release.latest_version));
    let stage_dir = tmp_dir.join(format!("orbit_deb_stage_{my_pid}"));

    download_with_progress(deb_url, &release.tag_name, &deb_path, on_progress).await?;

    emit_progress(on_progress, 0.90, "正在解包校验 Linux 安装包…", false);
    std::fs::create_dir_all(&stage_dir).map_err(|e| format!("创建临时目录失败: {e}"))?;

    let extract_out = tokio::process::Command::new("dpkg-deb")
        .args([
            "-x",
            &deb_path.to_string_lossy(),
            &stage_dir.to_string_lossy(),
        ])
        .output()
        .await
        .map_err(|e| format!("调用 dpkg-deb 解包失败: {e}"))?;
    if !extract_out.status.success() {
        return Err(format!(
            "解包 .deb 失败: {}",
            String::from_utf8_lossy(&extract_out.stderr).trim()
        ));
    }

    let extracted_bin = stage_dir.join("usr/bin/orbit-desktop");
    if !extracted_bin.is_file() {
        return Err("安装包内未找到 usr/bin/orbit-desktop".into());
    }

    let cur_exe =
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/bin/orbit-desktop"));
    emit_progress(on_progress, 0.98, "正在自动替换并重启新版 Orbit…", true);

    let sq_deb = shell_single_quote(&deb_path.to_string_lossy());
    let sq_stage = shell_single_quote(&stage_dir.to_string_lossy());
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
          rm -rf {sq_stage} {sq_deb}
          nohup \"$LAUNCH_TARGET\" >/dev/null 2>&1 &
        ) >/dev/null 2>&1 &"
    );

    spawn_detached_shell(&updater_script)?;
    tokio::time::sleep(Duration::from_millis(280)).await;
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_handles_v_prefix_and_prerelease() {
        assert_eq!(compare_semver("v0.2.4", "0.2.3"), Ordering::Greater);
        assert_eq!(compare_semver("0.2.3", "v0.2.3"), Ordering::Equal);
        assert_eq!(compare_semver("0.2.2", "0.2.3"), Ordering::Less);
        assert_eq!(compare_semver("1.0.0", "0.9.9"), Ordering::Greater);
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
        let info = build_release_info_from_tag("v0.5.4", "0.5.3");
        assert!(info.has_newer_version);
        assert_eq!(info.tag_name, "v0.5.4");
        assert_eq!(info.latest_version, "0.5.4");
        assert_eq!(
            info.macos_dmg_url.as_deref(),
            Some(
                "https://github.com/danielgaoyy/orbit/releases/download/v0.5.4/Orbit-macOS-AppleSilicon.dmg"
            )
        );
        assert_eq!(
            info.linux_arm64_deb_url.as_deref(),
            Some(
                "https://github.com/danielgaoyy/orbit/releases/download/v0.5.4/Orbit-Linux-arm64.deb"
            )
        );
        assert_eq!(
            info.linux_x64_deb_url.as_deref(),
            Some(
                "https://github.com/danielgaoyy/orbit/releases/download/v0.5.4/Orbit-Linux-x64.deb"
            )
        );
    }
}
