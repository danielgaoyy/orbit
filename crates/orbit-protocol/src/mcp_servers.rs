//! External MCP server definitions, scope taxonomy, masked wire projections,
//! and live JSON-RPC 2.0 (`initialize` + `tools/list`) probe results.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Transport configuration for an external MCP server (secret values are never serialized on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpTransportKind {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    StreamableHttp {
        url: String,
        #[serde(default)]
        header_keys: Vec<String>,
    },
}

/// Discovery and ownership scope of an external MCP server on a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerScope {
    OrbitUser,
    ProjectDotMcp,
    ClaudeUser,
    CodexUser,
    AntigravityUser,
}

impl McpServerScope {
    pub const fn id_prefix(self) -> &'static str {
        match self {
            Self::OrbitUser => "orbit_user",
            Self::ProjectDotMcp => "project_dot_mcp",
            Self::ClaudeUser => "claude_user",
            Self::CodexUser => "codex_user",
            Self::AntigravityUser => "antigravity_user",
        }
    }

    pub const fn is_editable(self) -> bool {
        matches!(self, Self::OrbitUser | Self::ProjectDotMcp)
    }
}

/// Target write scope when creating or updating an external MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpWriteScope {
    User,
    Project,
}

/// Concise summary of a tool advertised by an external MCP server during `tools/list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolSummary {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Live probe outcome (`initialize` -> `notifications/initialized` -> `tools/list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpProbeResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    #[serde(default)]
    pub tools: Vec<McpToolSummary>,
    pub latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub probed_at_ms: u64,
}

/// Wire-safe projection of an external MCP server configured or discovered on a host.
/// Secret env var and header values are always masked before crossing the daemon boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalMcpServerRecord {
    pub id: String,
    pub name: String,
    pub scope: McpServerScope,
    pub transport: McpTransportKind,
    #[serde(default)]
    pub env_keys: Vec<String>,
    #[serde(default)]
    pub masked_env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub masked_headers: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub editable: bool,
    #[serde(default)]
    pub source_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_probe: Option<McpProbeResult>,
}
