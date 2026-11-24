//! Native OS interactive notifications for Orbit Desktop (macOS, Linux, Windows).
//!
//! Safety invariants:
//! - `unsafe_code = "deny"` across all platforms.
//! - `PlanApproval`, `human_only`, and high/critical risk gates NEVER expose a blind Approve
//!   button — they expose only `"Open to Review"` and route to focusing the window in-app.
//! - Standard approval/deny actions invoke `OrbitAppCore::resolve_permission` directly without
//!   stealing window focus from the user's current application.
//! - Untrusted title/body text is strictly sanitized and passed via safe argument/env channels
//!   (never interpolated into shell strings).

use std::sync::Arc;

use orbit_client_core::app_core::OrbitAppCore;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

pub const NATIVE_NOTIFICATION_OPEN_EVENT: &str = "orbit://notification-open";
pub const NATIVE_NOTIFICATION_RESOLVED_EVENT: &str = "orbit://notification-action-resolved";

#[derive(Debug, Clone, Deserialize)]
pub struct NativeApprovalNotificationRequest {
    pub host_id: String,
    pub agent_id: String,
    #[serde(default)]
    pub permission_request_id: Option<String>,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub is_plan_approval: bool,
    #[serde(default)]
    pub human_only: bool,
    #[serde(default)]
    pub risk_level: Option<String>,
    #[serde(default)]
    pub allow_decision: Option<String>,
    #[serde(default)]
    pub deny_decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeNotificationDispatchResult {
    pub delivered: bool,
    pub interactive: bool,
    pub actions_mode: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NativeNotificationOpenPayload {
    pub host_id: String,
    pub agent_id: String,
    pub permission_request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NativeNotificationResolvedPayload {
    pub host_id: String,
    pub agent_id: String,
    pub permission_request_id: String,
    pub decision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeNotificationActions {
    /// Standard permission gate: exposes inline Approve / Deny + Open without stealing focus.
    ApproveDenyOpen,
    /// Plan review, `human_only`, or high-risk gates: strictly forbids blind Approve.
    OpenToReviewOnly,
}

impl NativeNotificationActions {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApproveDenyOpen => "approve_deny_open",
            Self::OpenToReviewOnly => "open_to_review_only",
        }
    }
}

/// Authoritative server-side policy deciding which OS notification actions are permitted.
pub fn resolve_notification_actions(
    req: &NativeApprovalNotificationRequest,
) -> NativeNotificationActions {
    let has_perm_id = req
        .permission_request_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    let is_high_risk = req.risk_level.as_deref().is_some_and(|r| {
        let lower = r.trim().to_ascii_lowercase();
        lower == "high" || lower == "critical"
    });
    if !has_perm_id || req.is_plan_approval || req.human_only || is_high_risk {
        NativeNotificationActions::OpenToReviewOnly
    } else {
        NativeNotificationActions::ApproveDenyOpen
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParsedNotificationAction {
    Approve,
    Deny,
    Open,
    Dismissed,
}

pub fn parse_action_token(raw: &str) -> ParsedNotificationAction {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "approve" | "allow" => ParsedNotificationAction::Approve,
        "deny" | "reject" => ParsedNotificationAction::Deny,
        "open" | "review" | "open to review" | "@contentclicked" | "default" => {
            ParsedNotificationAction::Open
        }
        _ => ParsedNotificationAction::Dismissed,
    }
}

/// Sanitize arbitrary notification text: strip control characters and enforce a safe byte bound.
pub fn sanitize_notification_text(raw: &str, max_chars: usize) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(any(target_os = "macos", test))]
pub fn escape_applescript_string(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}

#[cfg(any(target_os = "windows", test))]
pub fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Execute the user's clicked OS notification action safely against `OrbitAppCore`.
/// Returns `true` if a permission was resolved in-place without focusing the window.
pub fn handle_native_notification_action(
    app: Option<&AppHandle>,
    core: &Arc<OrbitAppCore>,
    req: &NativeApprovalNotificationRequest,
    action: ParsedNotificationAction,
) -> bool {
    let policy = resolve_notification_actions(req);

    match action {
        ParsedNotificationAction::Approve => {
            // Hard safety invariant: NEVER blind-approve PlanApproval, human_only, or high-risk gates.
            if policy != NativeNotificationActions::ApproveDenyOpen {
                focus_and_emit_open(app, req);
                return false;
            }
            if let Some(perm_id) = req
                .permission_request_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let decision = req
                    .allow_decision
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("allow");
                core.resolve_permission(&req.host_id, &req.agent_id, perm_id, decision);
                if let Some(handle) = app {
                    let _ = handle.emit(
                        NATIVE_NOTIFICATION_RESOLVED_EVENT,
                        NativeNotificationResolvedPayload {
                            host_id: req.host_id.clone(),
                            agent_id: req.agent_id.clone(),
                            permission_request_id: perm_id.to_string(),
                            decision: decision.to_string(),
                        },
                    );
                }
                return true;
            }
            false
        }
        ParsedNotificationAction::Deny => {
            if policy != NativeNotificationActions::ApproveDenyOpen {
                focus_and_emit_open(app, req);
                return false;
            }
            if let Some(perm_id) = req
                .permission_request_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let decision = req
                    .deny_decision
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("deny");
                core.resolve_permission(&req.host_id, &req.agent_id, perm_id, decision);
                if let Some(handle) = app {
                    let _ = handle.emit(
                        NATIVE_NOTIFICATION_RESOLVED_EVENT,
                        NativeNotificationResolvedPayload {
                            host_id: req.host_id.clone(),
                            agent_id: req.agent_id.clone(),
                            permission_request_id: perm_id.to_string(),
                            decision: decision.to_string(),
                        },
                    );
                }
                return true;
            }
            false
        }
        ParsedNotificationAction::Open => {
            focus_and_emit_open(app, req);
            false
        }
        ParsedNotificationAction::Dismissed => false,
    }
}

fn focus_and_emit_open(app: Option<&AppHandle>, req: &NativeApprovalNotificationRequest) {
    if let Some(handle) = app {
        let _ = crate::tray::focus_main_window_impl(handle);
        let _ = handle.emit(
            NATIVE_NOTIFICATION_OPEN_EVENT,
            NativeNotificationOpenPayload {
                host_id: req.host_id.clone(),
                agent_id: req.agent_id.clone(),
                permission_request_id: req.permission_request_id.clone(),
            },
        );
    }
}

/// Dispatch an OS-native notification and spawn an asynchronous action listener (when interactive)
/// so clicking Approve/Deny resolves directly in Rust without stealing focus.
pub async fn dispatch_native_approval_notification(
    app: AppHandle,
    core: Arc<OrbitAppCore>,
    req: NativeApprovalNotificationRequest,
) -> NativeNotificationDispatchResult {
    let actions = resolve_notification_actions(&req);
    let title = sanitize_notification_text(&req.title, 120);
    let body = sanitize_notification_text(&req.body, 280);

    #[cfg(target_os = "macos")]
    {
        dispatch_macos(app, core, req, actions, title, body).await
    }

    #[cfg(target_os = "linux")]
    {
        dispatch_linux(app, core, req, actions, title, body).await
    }

    #[cfg(target_os = "windows")]
    {
        dispatch_windows(app, core, req, actions, title, body).await
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (app, core, req, title, body);
        NativeNotificationDispatchResult {
            delivered: false,
            interactive: false,
            actions_mode: actions.as_str().to_string(),
            backend: "unsupported".to_string(),
        }
    }
}

#[cfg(target_os = "macos")]
async fn dispatch_macos(
    app: AppHandle,
    core: Arc<OrbitAppCore>,
    req: NativeApprovalNotificationRequest,
    actions: NativeNotificationActions,
    title: String,
    body: String,
) -> NativeNotificationDispatchResult {
    // 1. Prefer `alerter` when present on PATH — supports real interactive macOS Notification
    //    Center action buttons without stealing focus.
    let alerter_candidates = [
        "alerter",
        "/opt/homebrew/bin/alerter",
        "/usr/local/bin/alerter",
    ];
    for bin in alerter_candidates {
        let mut cmd = tokio::process::Command::new(bin);
        cmd.args([
            "-title",
            &title,
            "-message",
            &body,
            "-sender",
            "com.orbit.desktop",
            "-timeout",
            "120",
        ]);
        match actions {
            NativeNotificationActions::ApproveDenyOpen => {
                cmd.args(["-actions", "Approve,Deny", "-dropdownLabel", "Action"]);
            }
            NativeNotificationActions::OpenToReviewOnly => {
                cmd.args(["-actions", "Open to Review"]);
            }
        }
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());

        if let Ok(child) = cmd.spawn() {
            let app_c = app.clone();
            let core_c = Arc::clone(&core);
            let req_c = req.clone();
            tokio::spawn(async move {
                if let Ok(output) = child.wait_with_output().await {
                    let clicked = String::from_utf8_lossy(&output.stdout);
                    let parsed = parse_action_token(&clicked);
                    handle_native_notification_action(Some(&app_c), &core_c, &req_c, parsed);
                }
            });
            return NativeNotificationDispatchResult {
                delivered: true,
                interactive: true,
                actions_mode: actions.as_str().to_string(),
                backend: "macos_alerter".to_string(),
            };
        }
    }

    // 2. Fallback to built-in macOS `osascript` `display notification` (zero external deps,
    //    never steals window focus, safely escaped AppleScript literal).
    let escaped_body = escape_applescript_string(&body);
    let escaped_title = escape_applescript_string(&title);
    let script = format!("display notification \"{escaped_body}\" with title \"{escaped_title}\"");
    let status = tokio::process::Command::new("osascript")
        .args(["-e", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    NativeNotificationDispatchResult {
        delivered: status.is_ok_and(|s| s.success()),
        interactive: false,
        actions_mode: actions.as_str().to_string(),
        backend: "macos_osascript".to_string(),
    }
}

#[cfg(target_os = "linux")]
async fn dispatch_linux(
    app: AppHandle,
    core: Arc<OrbitAppCore>,
    req: NativeApprovalNotificationRequest,
    actions: NativeNotificationActions,
    title: String,
    body: String,
) -> NativeNotificationDispatchResult {
    // libnotify >= 0.8 `notify-send --action=... --wait` prints the chosen action token to stdout.
    let mut cmd = tokio::process::Command::new("notify-send");
    cmd.args(["--app-name=Orbit", "--wait"]);
    match actions {
        NativeNotificationActions::ApproveDenyOpen => {
            cmd.args([
                "--action=approve=Approve",
                "--action=deny=Deny",
                "--action=open=Open",
            ]);
        }
        NativeNotificationActions::OpenToReviewOnly => {
            cmd.args(["--action=open=Open to Review"]);
        }
    }
    cmd.arg("--").arg(&title).arg(&body);
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    if let Ok(child) = cmd.spawn() {
        let app_c = app.clone();
        let core_c = Arc::clone(&core);
        let req_c = req.clone();
        tokio::spawn(async move {
            if let Ok(output) = child.wait_with_output().await
                && output.status.success()
            {
                let clicked = String::from_utf8_lossy(&output.stdout);
                let parsed = parse_action_token(&clicked);
                handle_native_notification_action(Some(&app_c), &core_c, &req_c, parsed);
            }
        });
        return NativeNotificationDispatchResult {
            delivered: true,
            interactive: true,
            actions_mode: actions.as_str().to_string(),
            backend: "linux_notify_send".to_string(),
        };
    }

    // Fallback to plain non-interactive `notify-send`
    let status = tokio::process::Command::new("notify-send")
        .args(["--app-name=Orbit", "--"])
        .arg(&title)
        .arg(&body)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    NativeNotificationDispatchResult {
        delivered: status.is_ok_and(|s| s.success()),
        interactive: false,
        actions_mode: actions.as_str().to_string(),
        backend: "linux_notify_send_basic".to_string(),
    }
}

#[cfg(target_os = "windows")]
async fn dispatch_windows(
    _app: AppHandle,
    _core: Arc<OrbitAppCore>,
    _req: NativeApprovalNotificationRequest,
    actions: NativeNotificationActions,
    title: String,
    body: String,
) -> NativeNotificationDispatchResult {
    let xml_title = escape_xml_text(&title);
    let xml_body = escape_xml_text(&body);
    let actions_xml = match actions {
        NativeNotificationActions::ApproveDenyOpen => {
            r#"<actions><action content="Approve" arguments="approve" /><action content="Deny" arguments="deny" /></actions>"#
        }
        NativeNotificationActions::OpenToReviewOnly => {
            r#"<actions><action content="Open to Review" arguments="open" /></actions>"#
        }
    };
    let toast_xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{xml_title}</text><text>{xml_body}</text></binding></visual>{actions_xml}</toast>"
    );

    let ps_script = r#"
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
[Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
$xml = New-Object Windows.Data.Xml.Dom.XmlDocument
$xml.LoadXml($env:ORBIT_TOAST_XML)
$toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
$notifier = [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Orbit')
$notifier.Show($toast)
"#;

    let status = tokio::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            ps_script,
        ])
        .env("ORBIT_TOAST_XML", toast_xml)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    NativeNotificationDispatchResult {
        delivered: status.is_ok_and(|s| s.success()),
        interactive: false,
        actions_mode: actions.as_str().to_string(),
        backend: "windows_winrt_toast".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_req() -> NativeApprovalNotificationRequest {
        NativeApprovalNotificationRequest {
            host_id: "h1".into(),
            agent_id: "ag1".into(),
            permission_request_id: Some("perm_1".into()),
            title: "Orbit · MacBook".into(),
            body: "Run cargo test".into(),
            is_plan_approval: false,
            human_only: false,
            risk_level: Some("low".into()),
            allow_decision: Some("allow".into()),
            deny_decision: Some("deny".into()),
        }
    }

    #[test]
    fn enforces_open_to_review_only_for_plan_approval_human_only_and_high_risk() {
        let std_req = sample_req();
        assert_eq!(
            resolve_notification_actions(&std_req),
            NativeNotificationActions::ApproveDenyOpen
        );

        let mut plan_req = sample_req();
        plan_req.is_plan_approval = true;
        assert_eq!(
            resolve_notification_actions(&plan_req),
            NativeNotificationActions::OpenToReviewOnly
        );

        let mut human_req = sample_req();
        human_req.human_only = true;
        assert_eq!(
            resolve_notification_actions(&human_req),
            NativeNotificationActions::OpenToReviewOnly
        );

        let mut high_risk_req = sample_req();
        high_risk_req.risk_level = Some("HIGH".into());
        assert_eq!(
            resolve_notification_actions(&high_risk_req),
            NativeNotificationActions::OpenToReviewOnly
        );

        let mut no_perm_req = sample_req();
        no_perm_req.permission_request_id = None;
        assert_eq!(
            resolve_notification_actions(&no_perm_req),
            NativeNotificationActions::OpenToReviewOnly
        );
    }

    #[test]
    fn handler_refuses_blind_approve_on_plan_and_human_only_requests() {
        let core = OrbitAppCore::open_in_memory().expect("open core");

        let mut plan_req = sample_req();
        plan_req.is_plan_approval = true;
        assert!(
            !handle_native_notification_action(
                None,
                &core,
                &plan_req,
                ParsedNotificationAction::Approve
            ),
            "PlanApproval must NEVER resolve silently on Approve"
        );

        let mut human_req = sample_req();
        human_req.human_only = true;
        assert!(
            !handle_native_notification_action(
                None,
                &core,
                &human_req,
                ParsedNotificationAction::Approve
            ),
            "human_only request must NEVER resolve silently on Approve"
        );

        let std_req = sample_req();
        assert!(
            handle_native_notification_action(
                None,
                &core,
                &std_req,
                ParsedNotificationAction::Approve
            ),
            "standard permission request resolves directly without stealing focus"
        );
    }

    #[test]
    fn sanitizes_and_escapes_notification_strings() {
        assert_eq!(
            sanitize_notification_text("hello\n\tworld\0!", 50),
            "hello  world !"
        );
        assert_eq!(
            escape_applescript_string(r#"say "hi" \ok"#),
            r#"say \"hi\" \\ok"#
        );
        assert_eq!(
            escape_xml_text("<tag attr=\"x\">&'y'"),
            "&lt;tag attr=&quot;x&quot;&gt;&amp;&apos;y&apos;"
        );
        assert_eq!(
            parse_action_token("Approve"),
            ParsedNotificationAction::Approve
        );
        assert_eq!(parse_action_token("Deny"), ParsedNotificationAction::Deny);
        assert_eq!(
            parse_action_token("Open to Review"),
            ParsedNotificationAction::Open
        );
    }
}
