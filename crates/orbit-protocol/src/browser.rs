//! Wire types for Pure-Rust CDP Browser Automation & Guarded Computer Use.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Hard upper bound on concurrent live CDP browser sessions per Orbit host daemon.
pub const MAX_BROWSER_SESSIONS: u32 = 4;

fn default_true() -> bool {
    true
}

fn default_max_sessions() -> u32 {
    MAX_BROWSER_SESSIONS
}

/// Host-local security and runtime policy persisted in `$ORBIT_HOME/browser-policy.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPolicyConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub headless: bool,
    #[serde(default = "default_true")]
    pub allow_localhost: bool,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub blocked_domains: Vec<String>,
    #[serde(default = "default_true")]
    pub block_password_inputs: bool,
    #[serde(default = "default_max_sessions")]
    pub max_sessions: u32,
}

impl Default for BrowserPolicyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            headless: true,
            allow_localhost: true,
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            block_password_inputs: true,
            max_sessions: MAX_BROWSER_SESSIONS,
        }
    }
}

impl BrowserPolicyConfig {
    /// Normalize domain lists and clamp `max_sessions` within `1..=MAX_BROWSER_SESSIONS`.
    pub fn sanitized(mut self) -> Self {
        self.allowed_domains = normalize_domain_list(self.allowed_domains);
        self.blocked_domains = normalize_domain_list(self.blocked_domains);
        self.max_sessions = self.max_sessions.clamp(1, MAX_BROWSER_SESSIONS);
        self
    }
}

fn normalize_domain_list(domains: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for raw in domains {
        let cleaned = raw
            .trim()
            .trim_start_matches("*.")
            .trim_end_matches('.')
            .to_ascii_lowercase();
        if !cleaned.is_empty() && !out.contains(&cleaned) {
            out.push(cleaned);
        }
    }
    out
}

/// Compact numbered interactive DOM element reference returned by `Snapshot`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserElementRef {
    pub ref_id: String,
    pub role: String,
    pub name: String,
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_type: Option<String>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Bounded in-memory audit record for every browser automation action attempted on a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserActionAudit {
    pub id: String,
    pub action: String,
    pub target: String,
    pub ok: bool,
    pub detail: String,
    pub timestamp_ms: u64,
}

/// Live summary of an active managed CDP browser session on an Orbit host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserSessionSummary {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub url: String,
    pub title: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub headless: bool,
    pub element_count: u32,
    #[serde(default)]
    pub console_errors: Vec<String>,
    #[serde(default)]
    pub recent_actions: Vec<BrowserActionAudit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_screenshot_base64: Option<String>,
    pub updated_at_ms: u64,
}

/// Bounding box of a picked DOM element in viewport CSS pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickedElementBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Structured DOM element metadata captured by `BrowserActionRequest::PickElement`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickedElementInfo {
    pub selector: String,
    pub tag: String,
    #[serde(default)]
    pub text_preview: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computed_box: Option<PickedElementBox>,
    #[serde(default)]
    pub outer_html_excerpt: String,
    #[serde(default)]
    pub page_url: String,
}

/// Typed browser automation action executed against a live CDP session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserActionRequest {
    Navigate {
        url: String,
    },
    Snapshot,
    Screenshot {
        #[serde(default)]
        full_page: bool,
    },
    Click {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y: Option<f64>,
    },
    Type {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_id: Option<String>,
        text: String,
        #[serde(default)]
        submit: bool,
    },
    Scroll {
        #[serde(default)]
        delta_x: f64,
        #[serde(default)]
        delta_y: f64,
    },
    Back,
    Reload,
    /// Dispatch CDP `Input.dispatchKeyEvent` (`rawKeyDown` + optional `char` + `keyUp`) supporting
    /// special keys (`Enter`, `Escape`, `Tab`, `Backspace`, `ArrowUp/Down/Left/Right`, `Space`)
    /// and modifier chords (`Meta+A`, `Control+C`, `Ctrl+Shift+K`, etc.).
    PressKey {
        key: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        modifiers: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_id: Option<String>,
    },
    /// Select an `<option>` inside a `<select>` element by `value`, `label`, or `index` and
    /// dispatch `input` + `change` events.
    SelectOption {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
    },
    /// Attach one or more strictly workspace-confined files to an `<input type="file">` via
    /// CDP `DOM.setFileInputFiles`.
    UploadFile {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        files: Vec<String>,
    },
    /// Inspect a DOM element at viewport `(x, y)`, CSS `selector`, snapshot `ref_id`, or via an
    /// interactive hover-highlight overlay inside the CDP page.
    PickElement {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_id: Option<String>,
        #[serde(default)]
        interactive: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WireMessage;
    use uuid::Uuid;

    #[test]
    fn browser_policy_defaults_and_sanitization() {
        let def = BrowserPolicyConfig::default();
        assert!(def.enabled);
        assert!(def.headless);
        assert!(def.allow_localhost);
        assert!(def.block_password_inputs);
        assert_eq!(def.max_sessions, 4);

        let sanitized = BrowserPolicyConfig {
            enabled: true,
            headless: false,
            allow_localhost: true,
            allowed_domains: vec!["  *.Example.COM. ".into(), "example.com".into(), "".into()],
            blocked_domains: vec!["Evil.Org".into()],
            block_password_inputs: true,
            max_sessions: 99,
        }
        .sanitized();
        assert_eq!(sanitized.allowed_domains, vec!["example.com"]);
        assert_eq!(sanitized.blocked_domains, vec!["evil.org"]);
        assert_eq!(sanitized.max_sessions, MAX_BROWSER_SESSIONS);

        let zero_cap = BrowserPolicyConfig {
            max_sessions: 0,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(zero_cap.max_sessions, 1);
    }

    #[test]
    fn browser_wire_messages_zstd_roundtrip() {
        let req_id = Uuid::new_v4();
        let policy = BrowserPolicyConfig {
            enabled: true,
            headless: true,
            allow_localhost: true,
            allowed_domains: vec!["example.com".into()],
            blocked_domains: vec!["blocked.test".into()],
            block_password_inputs: true,
            max_sessions: 4,
        };
        let element = BrowserElementRef {
            ref_id: "e1".into(),
            role: "button".into(),
            name: "Submit".into(),
            tag: "button".into(),
            input_type: None,
            x: 100.0,
            y: 200.0,
            width: 80.0,
            height: 32.0,
        };
        let audit = BrowserActionAudit {
            id: "ba_1".into(),
            action: "navigate".into(),
            target: "http://localhost:3000/".into(),
            ok: true,
            detail: "Navigated to Local App".into(),
            timestamp_ms: 1_760_000_000_000,
        };
        let session = BrowserSessionSummary {
            session_id: "br_abc123".into(),
            agent_id: Some("ag_1".into()),
            url: "http://localhost:3000/".into(),
            title: "Local App".into(),
            viewport_width: 1280,
            viewport_height: 800,
            headless: true,
            element_count: 1,
            console_errors: vec!["ReferenceError: x is not defined".into()],
            recent_actions: vec![audit],
            last_screenshot_base64: Some("iVBORw0KGgo=".into()),
            updated_at_ms: 1_760_000_000_000,
        };

        let messages = vec![
            WireMessage::ListBrowserSessions {
                request_id: req_id,
                include_screenshots: true,
            },
            WireMessage::ListBrowserSessions {
                request_id: req_id,
                include_screenshots: false,
            },
            WireMessage::BrowserSessionsSnapshot {
                request_id: req_id,
                sessions: vec![session.clone()],
                policy: policy.clone(),
                chrome_binary: Some(
                    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into(),
                ),
            },
            WireMessage::CreateBrowserSession {
                request_id: req_id,
                agent_id: Some("ag_1".into()),
                initial_url: Some("http://localhost:3000".into()),
                headless: Some(true),
                viewport_width: Some(1280),
                viewport_height: Some(800),
                workspace_root: Some("/tmp/repo".into()),
            },
            WireMessage::BrowserSessionUpdated {
                request_id: req_id,
                session: session.clone(),
            },
            WireMessage::ExecuteBrowserAction {
                request_id: req_id,
                session_id: "br_abc123".into(),
                action: BrowserActionRequest::Click {
                    ref_id: Some("e1".into()),
                    x: None,
                    y: None,
                },
            },
            WireMessage::ExecuteBrowserAction {
                request_id: req_id,
                session_id: "br_abc123".into(),
                action: BrowserActionRequest::Type {
                    ref_id: Some("e2".into()),
                    text: "hello".into(),
                    submit: true,
                },
            },
            WireMessage::ExecuteBrowserAction {
                request_id: req_id,
                session_id: "br_abc123".into(),
                action: BrowserActionRequest::Scroll {
                    delta_x: 0.0,
                    delta_y: 400.0,
                },
            },
            WireMessage::ExecuteBrowserAction {
                request_id: req_id,
                session_id: "br_abc123".into(),
                action: BrowserActionRequest::PressKey {
                    key: "Meta+A".into(),
                    modifiers: vec!["Shift".into()],
                    ref_id: Some("e2".into()),
                },
            },
            WireMessage::ExecuteBrowserAction {
                request_id: req_id,
                session_id: "br_abc123".into(),
                action: BrowserActionRequest::SelectOption {
                    ref_id: Some("e3".into()),
                    selector: None,
                    value: Some("us".into()),
                    label: None,
                    index: None,
                },
            },
            WireMessage::ExecuteBrowserAction {
                request_id: req_id,
                session_id: "br_abc123".into(),
                action: BrowserActionRequest::UploadFile {
                    ref_id: Some("e4".into()),
                    selector: None,
                    files: vec!["fixtures/logo.png".into()],
                },
            },
            WireMessage::ExecuteBrowserAction {
                request_id: req_id,
                session_id: "br_abc123".into(),
                action: BrowserActionRequest::PickElement {
                    x: Some(120.5),
                    y: Some(240.0),
                    selector: None,
                    ref_id: None,
                    interactive: false,
                },
            },
            WireMessage::BrowserActionResponse {
                request_id: req_id,
                session,
                elements: vec![element],
                screenshot_base64: Some("iVBORw0KGgo=".into()),
                text_excerpt: Some("Welcome to Local App".into()),
                captcha_detected: true,
                picked_element: Some(PickedElementInfo {
                    selector: "button#submit".into(),
                    tag: "button".into(),
                    text_preview: "Submit".into(),
                    attributes: BTreeMap::from([("id".into(), "submit".into())]),
                    computed_box: Some(PickedElementBox {
                        x: 100.0,
                        y: 200.0,
                        width: 80.0,
                        height: 32.0,
                    }),
                    outer_html_excerpt: "<button id=\"submit\">Submit</button>".into(),
                    page_url: "http://localhost:3000/".into(),
                }),
            },
            WireMessage::PreviewTunnelOpen {
                request_id: req_id,
                tunnel_id: Uuid::new_v4(),
                target_port: 5173,
                proxy_host: Some("web--orbit.localhost".into()),
            },
            WireMessage::PreviewTunnelOpened {
                request_id: req_id,
                tunnel_id: Uuid::new_v4(),
                target_port: 5173,
            },
            WireMessage::PreviewTunnelData {
                tunnel_id: Uuid::new_v4(),
                payload_b64: "R0VUIC8gSFRUUC8xLjENCg0K".into(),
            },
            WireMessage::PreviewTunnelClose {
                tunnel_id: Uuid::new_v4(),
                reason: Some("eof".into()),
            },
            WireMessage::CloseBrowserSession {
                request_id: req_id,
                session_id: "br_abc123".into(),
            },
            WireMessage::UpdateBrowserPolicy {
                request_id: req_id,
                policy,
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode browser wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode browser wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }
}
