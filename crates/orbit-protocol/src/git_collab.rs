//! Wire types for Orbit Deep Git Collaboration Loop (B3-04):
//! - Git Stash management (`list`, `push`, `apply`, `pop`, `drop`)
//! - Git Rebase orchestration (`status`, `start`, `continue`, `abort`, `skip`)
//! - 3-Way Merge/Rebase Conflict inspection & resolution (`list`, `resolve`)
//! - GitHub PR CI Checks & Review Comments inspection (`pr_status`)
//! - Fast deterministic Conventional Commit message synthesis (`generate_commit_message`)

use serde::{Deserialize, Serialize};

fn default_rebase_kind() -> GitRebaseKind {
    GitRebaseKind::None
}

fn default_pr_availability() -> GitPrAvailability {
    GitPrAvailability::Unavailable
}

fn default_ci_check_state() -> GitCiCheckState {
    GitCiCheckState::Neutral
}

/// Single entry returned by `git stash list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStashEntry {
    /// Canonical git stash reference (`stash@{N}`).
    pub stash_ref: String,
    /// Zero-based stash index.
    pub index: u32,
    /// Optional branch name extracted from the stash reflog message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Human-readable stash summary/message.
    pub message: String,
    /// Full commit OID of the stash entry (used to pin apply/pop/drop against index shifts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oid: Option<String>,
    /// Short commit SHA of the stash entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_sha: Option<String>,
    /// ISO-8601 committer timestamp string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Result payload returned by `WireMessage::GitStashList`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GitStashListResult {
    #[serde(default)]
    pub entries: Vec<GitStashEntry>,
}

/// Result payload returned after mutating the Git stash stack (`push` / `apply` / `pop` / `drop`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStashMutationResult {
    /// Action executed: `"push"`, `"apply"`, `"pop"`, or `"drop"`.
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stash_ref: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub had_conflicts: bool,
}

/// Active Git rebase backend directory type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitRebaseKind {
    #[default]
    None,
    Merge,
    Apply,
}

/// Snapshot of active Git rebase / merge / cherry-pick state and unmerged paths.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GitRebaseStatus {
    #[serde(default)]
    pub in_progress: bool,
    #[serde(default = "default_rebase_kind")]
    pub kind: GitRebaseKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onto: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_message: Option<String>,
    #[serde(default)]
    pub conflicted_files: Vec<String>,
    #[serde(default)]
    pub merge_in_progress: bool,
    #[serde(default)]
    pub cherry_pick_in_progress: bool,
}

/// Result payload returned after a Git rebase lifecycle action (`start` / `continue` / `abort` / `skip`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRebaseActionResult {
    /// Action executed: `"start"`, `"continue"`, `"abort"`, or `"skip"`.
    pub action: String,
    pub status: GitRebaseStatus,
    pub summary: String,
}

/// Parsed 2-way or 3-way (`diff3` / `zdiff3`) conflict block inside a conflicted file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitConflictBlock {
    /// Zero-based index of this conflict block within the file.
    pub index: u32,
    /// 1-based line number of the opening `<<<<<<<` marker.
    pub start_line: u32,
    /// 1-based line number of the closing `>>>>>>>` marker (inclusive).
    pub end_line: u32,
    /// Label following `<<<<<<<`.
    pub ours_label: String,
    /// Optional label following `|||||||` (common ancestor in `diff3` / `zdiff3`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_label: Option<String>,
    /// Label following `>>>>>>>`.
    pub theirs_label: String,
    /// Content of the "ours" section (lines joined with `\n`).
    pub ours: String,
    /// Optional common-ancestor section when `|||||||` markers are present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Content of the "theirs" section (lines joined with `\n`).
    pub theirs: String,
    /// Up to 2 surrounding lines immediately before `start_line`.
    #[serde(default)]
    pub context_before: Vec<String>,
    /// Up to 2 surrounding lines immediately after `end_line`.
    #[serde(default)]
    pub context_after: Vec<String>,
}

/// Conflicted file entry returned by `WireMessage::GitListConflicts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitConflictFile {
    pub path: String,
    #[serde(default)]
    pub blocks: Vec<GitConflictBlock>,
    #[serde(default)]
    pub is_binary: bool,
    #[serde(default)]
    pub unmerged_in_index: bool,
    #[serde(default)]
    pub total_blocks: u32,
    /// SHA-256 hex digest of the inspected file bytes (used for optimistic concurrency checks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
}

/// Result payload returned by `WireMessage::GitListConflicts`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GitConflictListResult {
    #[serde(default)]
    pub files: Vec<GitConflictFile>,
}

/// Conflict resolution strategy for a single conflict block or an entire file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "strategy", rename_all = "snake_case")]
pub enum GitConflictResolution {
    Ours,
    Theirs,
    Both,
    ManualContent { content: String },
}

/// Result payload returned by `WireMessage::GitResolveConflict`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitResolveConflictResult {
    pub path: String,
    #[serde(default)]
    pub remaining_blocks: u32,
    #[serde(default)]
    pub staged: bool,
    pub summary: String,
}

/// Availability status of GitHub Pull Request metadata via `gh`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitPrAvailability {
    Available,
    NoPullRequest,
    #[default]
    Unavailable,
}

/// Normalized CI status check state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitCiCheckState {
    Passing,
    Failing,
    Pending,
    #[default]
    Neutral,
}

/// Single CI status check / GitHub Actions check run on a Pull Request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitPrCheckItem {
    pub name: String,
    #[serde(default = "default_ci_check_state")]
    pub state: GitCiCheckState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_conclusion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details_url: Option<String>,
}

/// Single Pull Request review thread comment, review summary, or issue comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitPrReviewComment {
    pub id: String,
    pub author: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default)]
    pub is_resolved: bool,
    #[serde(default)]
    pub is_outdated: bool,
    /// `"review_thread"`, `"review_summary"`, or `"issue_comment"`.
    #[serde(default)]
    pub kind: String,
}

/// Pull Request status, CI check rollup, and review comments for the active Git branch.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GitPrStatus {
    #[serde(default = "default_pr_availability")]
    pub availability: GitPrAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default)]
    pub is_draft: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mergeable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_decision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub checks: Vec<GitPrCheckItem>,
    #[serde(default)]
    pub passing_checks: u32,
    #[serde(default)]
    pub failing_checks: u32,
    #[serde(default)]
    pub pending_checks: u32,
    #[serde(default)]
    pub review_comments: Vec<GitPrReviewComment>,
    #[serde(default)]
    pub unresolved_comment_count: u32,
}

/// Synthesized conventional commit message returned by `WireMessage::GitGenerateCommitMessage`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitGeneratedCommitMessage {
    pub message: String,
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub commit_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// `"staged"` or `"working_tree"`.
    pub diff_source: String,
    #[serde(default)]
    pub files_analyzed: u32,
    /// `"deterministic"` or `"provider_cli"`.
    #[serde(default)]
    pub generator: String,
}

/// Typed read-only query payload returned by `WireMessage::GitCollabResponse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum GitCollabQueryResult {
    StashList(GitStashListResult),
    RebaseStatus(GitRebaseStatus),
    ConflictList(GitConflictListResult),
    PrStatus(GitPrStatus),
    CommitMessage(GitGeneratedCommitMessage),
}
