//! Wire types for Pure-Rust LSP Language Server Integration.
//!
//! All line and column coordinates exposed over the Orbit wire protocol use **1-based**
//! indexing (`line >= 1`, `col >= 1`) to match editor UI, file citations, and compiler output.
//! The host-local LSP engine in `orbit-daemon` converts between 1-based wire coordinates and
//! 0-based JSON-RPC LSP coordinates transparently.

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Origin of a language server configuration on a host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspConfigSource {
    #[default]
    Builtin,
    ProjectJson,
    UserConfig,
}

/// Declarative configuration for launching a stdio LSP language server on a host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspServerConfig {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub file_extensions: Vec<String>,
    #[serde(default)]
    pub language_ids: Vec<String>,
    #[serde(default)]
    pub root_markers: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub source: LspConfigSource,
}

/// Runtime lifecycle state of an LSP language server for a workspace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspServerState {
    #[default]
    Stopped,
    Starting,
    Ready,
    Error,
    Unavailable,
}

/// Status snapshot for a discovered or running LSP language server in a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspServerStatus {
    pub id: String,
    pub name: String,
    pub state: LspServerState,
    pub workspace_cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<String>,
    #[serde(default)]
    pub diagnostics_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub language_ids: Vec<String>,
    #[serde(default)]
    pub file_extensions: Vec<String>,
    #[serde(default)]
    pub source: LspConfigSource,
}

/// Normalized severity level for an LSP diagnostic.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum LspDiagnosticSeverity {
    #[default]
    Error,
    Warning,
    Info,
    Hint,
}

/// A single diagnostic reported by an LSP server (`textDocument/publishDiagnostics`).
///
/// Coordinates (`line`, `col`, `end_line`, `end_col`) are **1-based**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub path: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub severity: LspDiagnosticSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// A symbol returned by `workspace/symbol` or `textDocument/documentSymbol`.
///
/// Coordinates (`line`, `col`, `end_line`) are **1-based**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspSymbolInfo {
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
    pub path: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
}

/// A resolved jump target returned by `textDocument/definition`.
///
/// Coordinates (`line`, `col`, `end_line`) are **1-based**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspLocation {
    pub path: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WireMessage;
    use uuid::Uuid;

    #[test]
    fn lsp_wire_messages_zstd_roundtrip() {
        let req_id = Uuid::new_v4();
        let status = LspServerStatus {
            id: "rust-analyzer".into(),
            name: "rust-analyzer".into(),
            state: LspServerState::Ready,
            workspace_cwd: "/workspace/orbit".into(),
            binary_path: Some("/usr/local/bin/rust-analyzer".into()),
            diagnostics_count: 2,
            error: None,
            language_ids: vec!["rust".into()],
            file_extensions: vec!["rs".into()],
            source: LspConfigSource::Builtin,
        };
        let diag = LspDiagnostic {
            path: "src/lib.rs".into(),
            line: 12,
            col: 5,
            end_line: 12,
            end_col: 18,
            severity: LspDiagnosticSeverity::Error,
            message: "mismatched types".into(),
            source: Some("rustc".into()),
            code: Some("E0308".into()),
        };
        let symbol = LspSymbolInfo {
            name: "LspManager".into(),
            kind: "struct".into(),
            container_name: Some("lsp".into()),
            path: "crates/orbit-daemon/src/lsp.rs".into(),
            line: 42,
            col: 1,
            end_line: 55,
        };
        let location = LspLocation {
            path: "crates/orbit-daemon/src/lsp.rs".into(),
            line: 42,
            col: 12,
            end_line: 42,
            preview: Some("pub struct LspManager {".into()),
        };

        let messages = vec![
            WireMessage::ListLspServers {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
            },
            WireMessage::LspServersSnapshot {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                servers: vec![status],
            },
            WireMessage::LspQuerySymbols {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                query: "LspManager".into(),
                path: Some("crates/orbit-daemon/src/lsp.rs".into()),
                limit: 50,
            },
            WireMessage::LspSymbolsResponse {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                symbols: vec![symbol],
            },
            WireMessage::LspGetDiagnostics {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                path: Some("src/lib.rs".into()),
            },
            WireMessage::LspDiagnosticsResponse {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                diagnostics: vec![diag],
            },
            WireMessage::LspGotoDefinition {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                path: "src/lib.rs".into(),
                line: 10,
                col: 14,
            },
            WireMessage::LspLocationsResponse {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                locations: vec![location],
            },
            WireMessage::RestartLspServer {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                server_id: "rust-analyzer".into(),
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode lsp wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode lsp wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig json"),
                serde_json::to_value(&decoded).expect("decoded json")
            );
        }

        let cfg = LspServerConfig {
            id: "custom-ls".into(),
            name: "Custom LS".into(),
            command: "custom-ls".into(),
            args: vec!["--stdio".into()],
            file_extensions: vec!["foo".into()],
            language_ids: vec!["foo".into()],
            root_markers: vec!["foo.toml".into()],
            enabled: true,
            source: LspConfigSource::ProjectJson,
        };
        let cfg_json = serde_json::to_string(&cfg).expect("serialize cfg");
        let cfg_parsed: LspServerConfig = serde_json::from_str(&cfg_json).expect("parse cfg");
        assert_eq!(cfg_parsed, cfg);
    }
}
