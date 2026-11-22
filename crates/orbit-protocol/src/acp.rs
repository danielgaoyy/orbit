//! Universal Agent Client Protocol (ACP) registry catalog, custom agent specifications,
//! host availability projections, and live JSON-RPC 2.0 `initialize` handshake probe results.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Origin of an ACP agent specification on a host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpAgentSource {
    #[default]
    BuiltinCatalog,
    RemoteRegistry,
    Custom,
}

/// Declarative specification for launching any stdio ACP-compatible coding agent CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpAgentSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub source: AcpAgentSource,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Host-local availability status for a catalog or custom ACP agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpAgentAvailability {
    pub spec: AcpAgentSpec,
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
}

/// Result of spawning an ACP agent briefly to perform a live JSON-RPC 2.0 `initialize` handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpProbeResult {
    pub agent_id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    #[serde(default)]
    pub capabilities_summary: Vec<String>,
    #[serde(default)]
    pub auth_methods: Vec<String>,
    pub latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
