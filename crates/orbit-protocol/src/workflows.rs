//! Wire types for Batch CSV Fan-Out, Programmable DAG Workflows, and Best-of-N Parallel Arena.

use serde::{Deserialize, Serialize};

use crate::ProviderKind;

fn default_dag_step_timeout_secs() -> u64 {
    1800
}

/// Orchestration mode for a multi-step workflow run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowKind {
    BatchCsv,
    Dag,
    BestOfN,
}

impl WorkflowKind {
    /// Canonical snake_case representation for persistence.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BatchCsv => "batch_csv",
            Self::Dag => "dag",
            Self::BestOfN => "best_of_n",
        }
    }

    /// Parse from persisted snake_case string.
    pub fn from_str_opt(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "batch_csv" | "batch" => Some(Self::BatchCsv),
            "dag" => Some(Self::Dag),
            "best_of_n" | "best-of-n" | "arena" => Some(Self::BestOfN),
            _ => None,
        }
    }
}

/// Lifecycle status of a workflow run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Succeeded,
    PartialFailure,
    Failed,
    Canceled,
}

impl WorkflowRunStatus {
    /// Canonical snake_case representation for persistence.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::PartialFailure => "partial_failure",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    /// Parse from persisted snake_case string.
    pub fn from_str_opt(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "succeeded" => Some(Self::Succeeded),
            "partial_failure" => Some(Self::PartialFailure),
            "failed" => Some(Self::Failed),
            "canceled" | "cancelled" => Some(Self::Canceled),
            _ => None,
        }
    }

    /// Returns `true` when the workflow run is still queued or actively executing.
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }

    /// Returns `true` when the workflow run has reached a terminal state.
    pub const fn is_terminal(self) -> bool {
        !self.is_active()
    }
}

/// Lifecycle status of an individual step / batch item / Best-of-N candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepStatus {
    Pending,
    Running,
    Verifying,
    Succeeded,
    Failed,
    Canceled,
    Skipped,
}

impl WorkflowStepStatus {
    /// Canonical snake_case representation for persistence.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Skipped => "skipped",
        }
    }

    /// Parse from persisted snake_case string.
    pub fn from_str_opt(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "verifying" => Some(Self::Verifying),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "canceled" | "cancelled" => Some(Self::Canceled),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }

    /// Returns `true` when the step has finished (no further transitions expected).
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Canceled | Self::Skipped
        )
    }
}

/// Declarative node specification for a Programmable DAG Workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagStepSpec {
    pub step_id: String,
    pub name: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub prompt_template: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub isolated_worktree: bool,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default = "default_dag_step_timeout_secs")]
    pub timeout_secs: u64,
}

/// Candidate specification for a Best-of-N Parallel Arena run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BestOfNCandidateSpec {
    pub label: String,
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// Persisted & wire-synced state of a single workflow step or arena candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStepRecord {
    pub run_id: String,
    pub step_id: String,
    pub name: String,
    pub status: WorkflowStepStatus,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_preview: Option<String>,
    #[serde(default)]
    pub diff_additions: u32,
    #[serde(default)]
    pub diff_deletions: u32,
    #[serde(default)]
    pub diff_files: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_passed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub is_winner: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
}

/// Persisted & wire-synced state of a Batch CSV, DAG, or Best-of-N workflow run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRunRecord {
    pub run_id: String,
    pub name: String,
    pub kind: WorkflowKind,
    pub workspace_cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_agent_id: Option<String>,
    pub status: WorkflowRunStatus,
    pub max_concurrency: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_step_id: Option<String>,
    #[serde(default)]
    pub steps: Vec<WorkflowStepRecord>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
}
