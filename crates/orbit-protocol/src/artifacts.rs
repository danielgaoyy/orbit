//! Wire types for First-Class Artifacts & SEP-1865 MCP Apps.
//!
//! Artifacts represent rich, versioned deliverables produced by agents or users inside a workspace
//! (Markdown documents, interactive sandboxed HTML, code snapshots, SVG/Mermaid diagrams, and
//! SEP-1865 `ui://` MCP App interactive views).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Canonical rendering kind for an Orbit Artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    #[serde(alias = "md")]
    Markdown,
    #[serde(alias = "htm")]
    Html,
    Code,
    Svg,
    #[serde(alias = "mmd")]
    Mermaid,
    #[serde(alias = "mcp-app", alias = "mcpapp", alias = "ui")]
    McpApp,
}

impl ArtifactKind {
    const ALL: [Self; 6] = [
        Self::Markdown,
        Self::Html,
        Self::Code,
        Self::Svg,
        Self::Mermaid,
        Self::McpApp,
    ];

    pub const fn all() -> &'static [Self] {
        &Self::ALL
    }

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Code => "code",
            Self::Svg => "svg",
            Self::Mermaid => "mermaid",
            Self::McpApp => "mcp_app",
        }
    }

    pub fn from_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "markdown" | "md" | "text/markdown" => Some(Self::Markdown),
            "html" | "htm" | "text/html" | "interactive" | "web" => Some(Self::Html),
            "code" | "source" | "snippet" => Some(Self::Code),
            "svg" | "image/svg+xml" => Some(Self::Svg),
            "mermaid" | "mmd" | "diagram" => Some(Self::Mermaid),
            "mcp_app"
            | "mcp-app"
            | "mcpapp"
            | "ui"
            | "app"
            | "text/html+mcp"
            | "text/html;profile=mcp-app" => Some(Self::McpApp),
            _ => None,
        }
    }

    pub const fn default_mime_type(self) -> &'static str {
        match self {
            Self::Markdown => "text/markdown",
            Self::Html => "text/html",
            Self::Code => "text/plain",
            Self::Svg => "image/svg+xml",
            Self::Mermaid => "text/vnd.mermaid",
            Self::McpApp => "text/html;profile=mcp-app",
        }
    }

    pub const fn default_extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Html | Self::McpApp => "html",
            Self::Code => "txt",
            Self::Svg => "svg",
            Self::Mermaid => "mmd",
        }
    }
}

impl std::str::FromStr for ArtifactKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_slug(s).ok_or_else(|| {
            format!(
                "unsupported artifact kind `{s}` (expected: markdown, html, code, svg, mermaid, mcp_app)"
            )
        })
    }
}

/// Lightweight metadata summary for listing and live-syncing workspace Artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSummary {
    pub artifact_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub title: String,
    pub kind: ArtifactKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Optional SEP-1865 MCP App resource URI (e.g. `ui://dashboard/metrics`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_uri: Option<String>,
    /// Optional owning external MCP server name/ID for SEP-1865 `resources/read` and `tools/call`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server_name: Option<String>,
    pub version: u32,
    pub byte_len: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Metadata header for a single historical revision of an Artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRevisionMeta {
    pub version: u32,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_note: Option<String>,
    pub byte_len: usize,
}

/// Full Artifact payload including active revision content and version history metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDetail {
    pub summary: ArtifactSummary,
    pub content: String,
    #[serde(default)]
    pub revisions: Vec<ArtifactRevisionMeta>,
}

/// Standalone content record for a single Artifact revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRevisionContent {
    pub artifact_id: String,
    pub version: u32,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WireMessage;
    use uuid::Uuid;

    #[test]
    fn artifact_kind_slugs_and_aliases_roundtrip() {
        for &kind in ArtifactKind::all() {
            assert_eq!(ArtifactKind::from_slug(kind.slug()), Some(kind));
            let json = serde_json::to_string(&kind).expect("serialize kind");
            assert_eq!(json, format!("\"{}\"", kind.slug()));
            let parsed: ArtifactKind = serde_json::from_str(&json).expect("deserialize kind");
            assert_eq!(parsed, kind);
            assert!(!kind.default_mime_type().is_empty());
            assert!(!kind.default_extension().is_empty());
        }

        assert_eq!(ArtifactKind::from_slug("md"), Some(ArtifactKind::Markdown));
        assert_eq!(ArtifactKind::from_slug("htm"), Some(ArtifactKind::Html));
        assert_eq!(ArtifactKind::from_slug("mmd"), Some(ArtifactKind::Mermaid));
        assert_eq!(
            ArtifactKind::from_slug("mcp-app"),
            Some(ArtifactKind::McpApp)
        );
        assert_eq!(
            ArtifactKind::from_slug("text/html;profile=mcp-app"),
            Some(ArtifactKind::McpApp)
        );
        assert_eq!(ArtifactKind::from_slug("unknown"), None);
        assert_eq!(
            "mcp-app".parse::<ArtifactKind>().ok(),
            Some(ArtifactKind::McpApp)
        );
        assert!("unknown".parse::<ArtifactKind>().is_err());
    }

    #[test]
    fn artifact_wire_messages_zstd_roundtrip() {
        let now = Utc::now();
        let req_id = Uuid::new_v4();
        let summary = ArtifactSummary {
            artifact_id: "art_abc123".into(),
            workspace_id: "ws_1".into(),
            agent_id: Some("ag_1".into()),
            title: "Metrics Dashboard".into(),
            kind: ArtifactKind::McpApp,
            language: Some("html".into()),
            mime_type: Some("text/html;profile=mcp-app".into()),
            resource_uri: Some("ui://orbit/metrics".into()),
            mcp_server_name: Some("metrics".into()),
            version: 2,
            byte_len: 42,
            created_at: now,
            updated_at: now,
        };
        let detail = ArtifactDetail {
            summary: summary.clone(),
            content: "<div>Hello MCP App</div>".into(),
            revisions: vec![
                ArtifactRevisionMeta {
                    version: 2,
                    created_at: now,
                    summary_note: Some("Add chart".into()),
                    byte_len: 25,
                },
                ArtifactRevisionMeta {
                    version: 1,
                    created_at: now,
                    summary_note: None,
                    byte_len: 18,
                },
            ],
        };

        let messages = vec![
            WireMessage::ListArtifacts {
                request_id: req_id,
                workspace_id: Some("ws_1".into()),
                agent_id: None,
            },
            WireMessage::ArtifactsSnapshot {
                request_id: req_id,
                artifacts: vec![summary.clone()],
            },
            WireMessage::GetArtifact {
                request_id: req_id,
                artifact_id: "art_abc123".into(),
                version: Some(2),
            },
            WireMessage::ArtifactContent {
                request_id: req_id,
                artifact: detail.clone(),
            },
            WireMessage::UpsertArtifact {
                request_id: req_id,
                artifact_id: Some("art_abc123".into()),
                workspace_id: "ws_1".into(),
                agent_id: Some("ag_1".into()),
                title: "Metrics Dashboard".into(),
                kind: ArtifactKind::McpApp,
                language: Some("html".into()),
                mime_type: Some("text/html;profile=mcp-app".into()),
                resource_uri: Some("ui://orbit/metrics".into()),
                mcp_server_name: Some("metrics".into()),
                content: "<div>Hello MCP App</div>".into(),
                summary_note: Some("Add chart".into()),
            },
            WireMessage::ReadMcpResource {
                request_id: req_id,
                artifact_id: "art_abc123".into(),
                artifact_version: Some(2),
                uri: "ui://orbit/metrics".into(),
            },
            WireMessage::McpResourceResponse {
                request_id: req_id,
                server_name: "metrics".into(),
                uri: "ui://orbit/metrics".into(),
                content: crate::McpResourceContent {
                    uri: "ui://orbit/metrics".into(),
                    mime_type: Some("text/html;profile=mcp-app".into()),
                    text: Some("<div>Metrics</div>".into()),
                    blob_base64: None,
                },
            },
            WireMessage::CallArtifactMcpTool {
                request_id: req_id,
                artifact_id: "art_abc123".into(),
                artifact_version: Some(2),
                tool_name: "refresh_metrics".into(),
                arguments: serde_json::json!({ "window": "1h" }),
            },
            WireMessage::ArtifactMcpToolResponse {
                request_id: req_id,
                artifact_id: "art_abc123".into(),
                tool_name: "refresh_metrics".into(),
                outcome: crate::McpToolCallOutcome {
                    ok: true,
                    is_error: false,
                    content: vec![serde_json::json!({ "type": "text", "text": "refreshed" })],
                    structured_content: Some(serde_json::json!({ "count": 42 })),
                    error: None,
                },
            },
            WireMessage::ArtifactUpdated {
                request_id: Some(req_id),
                artifact: summary.clone(),
            },
            WireMessage::ArtifactUpdated {
                request_id: None,
                artifact: summary,
            },
            WireMessage::DeleteArtifact {
                request_id: req_id,
                artifact_id: "art_abc123".into(),
            },
            WireMessage::ArtifactDeleted {
                request_id: Some(req_id),
                artifact_id: "art_abc123".into(),
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode zstd");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode zstd");
            let orig_json = serde_json::to_value(&msg).expect("orig json");
            let dec_json = serde_json::to_value(&decoded).expect("dec json");
            assert_eq!(orig_json, dec_json);
        }

        let rev_content = ArtifactRevisionContent {
            artifact_id: "art_abc123".into(),
            version: 1,
            content: "<div>v1</div>".into(),
            created_at: now,
        };
        let rev_json = serde_json::to_string(&rev_content).expect("serialize rev");
        let rev_parsed: ArtifactRevisionContent =
            serde_json::from_str(&rev_json).expect("deserialize rev");
        assert_eq!(rev_parsed, rev_content);
    }
}
