//! Project Instruction Hierarchy wire models (`CLAUDE.md`, `AGENTS.md`, `AGENTS.override.md`,
//! `GEMINI.md`, `.cursorrules`, `.github/copilot-instructions.md`).

use serde::{Deserialize, Serialize};

/// Scope layer of an instruction file within the deterministic precedence stack
/// (lowest to highest priority: `GlobalUser` -> `RepoRoot` -> `AncestorDir` -> `WorkspaceDir` -> `LocalOverride`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionScope {
    GlobalUser,
    RepoRoot,
    AncestorDir,
    WorkspaceDir,
    LocalOverride,
}

impl InstructionScope {
    /// Human-readable short badge label for UI rendering.
    pub const fn badge_label(self) -> &'static str {
        match self {
            Self::GlobalUser => "Global",
            Self::RepoRoot => "Repo",
            Self::AncestorDir => "Ancestor",
            Self::WorkspaceDir => "Local",
            Self::LocalOverride => "Override",
        }
    }
}

/// Discovered instruction file (or canonical creatable layer slot) in the effective hierarchy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionFileEntry {
    /// Stable deterministic identifier for this entry within the snapshot.
    pub id: String,
    /// Absolute host path (or `~`-prefixed display-safe path if unresolvable).
    pub path: String,
    /// Compact display path relative to repo root, workspace dir, or `~/`.
    pub relative_path: String,
    /// Canonical instruction filename (e.g. `AGENTS.md`, `CLAUDE.md`, `AGENTS.override.md`).
    pub filename: String,
    /// Precedence layer scope.
    pub scope: InstructionScope,
    /// Optional provider association hint (e.g. `"claude_code"`, `"codex"`, `"gemini_cli"`, `"cursor_agent"`, `"copilot"`, `"orbit"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_hint: Option<String>,
    /// 1-based deterministic precedence rank (lowest priority = 1, highest priority = N).
    pub precedence_order: u32,
    /// Whether the file currently exists on disk.
    pub exists: bool,
    /// Exact UTF-8 byte length on disk (0 if missing).
    #[serde(default)]
    pub size_bytes: u64,
    /// Approximate LLM token footprint (UTF-8 / CJK aware, including resolved `@import` files).
    #[serde(default)]
    pub approx_tokens: u32,
    /// Lowercase hex SHA-256 digest of exact disk bytes for optimistic concurrency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// Short single-line or trimmed text preview of the file body.
    #[serde(default)]
    pub preview: String,
    /// Resolved `@path/to/file.md` import targets (canonicalized & contained inside repo/home).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<String>,
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::WireMessage;

    #[test]
    fn instruction_hierarchy_wire_roundtrip() {
        let req_id = Uuid::new_v4();
        let entry = InstructionFileEntry {
            id: "repo_root:AGENTS.md".into(),
            path: "/workspace/orbit/AGENTS.md".into(),
            relative_path: "AGENTS.md".into(),
            filename: "AGENTS.md".into(),
            scope: InstructionScope::RepoRoot,
            provider_hint: Some("universal".into()),
            precedence_order: 2,
            exists: true,
            size_bytes: 512,
            approx_tokens: 140,
            etag: Some("a".repeat(64)),
            preview: "# AGENTS.md — Orbit Engineering Guidelines".into(),
            imports: vec!["docs/architecture.md".into()],
        };

        let messages = vec![
            WireMessage::ListInstructionFiles {
                request_id: req_id,
                cwd: "/workspace/orbit/crates/orbit-daemon".into(),
            },
            WireMessage::InstructionFilesSnapshot {
                request_id: req_id,
                cwd: "/workspace/orbit/crates/orbit-daemon".into(),
                files: vec![entry],
                total_tokens: 140,
                merged_preview: "<!-- [2] Repo · AGENTS.md (~140 tokens) -->\n# AGENTS.md".into(),
            },
            WireMessage::ReadInstructionFile {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                path: "/workspace/orbit/AGENTS.md".into(),
            },
            WireMessage::InstructionFileContentResponse {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                path: "/workspace/orbit/AGENTS.md".into(),
                content: "# AGENTS.md\n".into(),
                etag: Some("b".repeat(64)),
                exists: true,
            },
            WireMessage::SaveInstructionFile {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                path: "/workspace/orbit/AGENTS.md".into(),
                content: "# AGENTS.md\nUpdated\n".into(),
                expected_etag: Some("b".repeat(64)),
            },
            WireMessage::InstructionFileSavedResponse {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                path: "/workspace/orbit/AGENTS.md".into(),
                etag: "c".repeat(64),
                size_bytes: 20,
                approx_tokens: 6,
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode instruction wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode instruction wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }
}
