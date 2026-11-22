//! Wire types for bidirectional Git Worktree <-> Local checkout Handoff (`Apply` / `Move`).

use serde::{Deserialize, Serialize};

use crate::WorkspaceSummary;

/// Direction of a Git worktree <-> local checkout handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeHandoffDirection {
    /// Transfer changes from an isolated git worktree back into the primary local checkout.
    WorktreeToLocal,
    /// Transfer uncommitted changes from a local checkout into a new or existing git worktree.
    LocalToWorktree,
}

impl WorktreeHandoffDirection {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::WorktreeToLocal => "worktree_to_local",
            Self::LocalToWorktree => "local_to_worktree",
        }
    }
}

/// Transfer semantics for a worktree <-> local handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeHandoffMode {
    /// Apply changes to target while keeping the source working tree intact.
    Apply,
    /// Apply changes to target and cleanly reset uncommitted source changes after verified success.
    Move,
}

impl WorktreeHandoffMode {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::Move => "move",
        }
    }
}

/// Per-file summary entry included in a dry-run handoff preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffFileChange {
    pub path: String,
    /// Git status code: `"M"`, `"A"`, `"D"`, `"R"`, or `"??"`.
    pub status: String,
    #[serde(default)]
    pub insertions: u32,
    #[serde(default)]
    pub deletions: u32,
    #[serde(default)]
    pub is_untracked: bool,
    #[serde(default)]
    pub has_conflict: bool,
}

/// Non-mutating dry-run preview of a worktree <-> local handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeHandoffPreview {
    pub direction: WorktreeHandoffDirection,
    pub source_cwd: String,
    pub target_cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_branch: Option<String>,
    /// Number of commits ahead of target/base in the source worktree.
    #[serde(default)]
    pub commit_count: u32,
    #[serde(default)]
    pub files: Vec<HandoffFileChange>,
    #[serde(default)]
    pub total_insertions: u32,
    #[serde(default)]
    pub total_deletions: u32,
    #[serde(default)]
    pub target_dirty: bool,
    /// Exact conflicting file paths detected via non-mutating dry-run checks.
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub can_apply_cleanly: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Authoritative result of an executed worktree <-> local handoff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeHandoffOutcome {
    pub direction: WorktreeHandoffDirection,
    pub mode: WorktreeHandoffMode,
    #[serde(default)]
    pub applied_files: u32,
    /// Hidden Git safety ref (`refs/orbit/handoff-backup/<ts>-<id>`) capturing pre-handoff state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_ref: Option<String>,
    /// Populated when `LocalToWorktree` provisions a new worktree workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_workspace: Option<WorkspaceSummary>,
    pub target_cwd: String,
    #[serde(default)]
    pub had_conflicts: bool,
    #[serde(default)]
    pub conflict_files: Vec<String>,
    pub summary: String,
}
