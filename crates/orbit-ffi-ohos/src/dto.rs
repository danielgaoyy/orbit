use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct OhosThreadPrefSeed {
    pub(crate) host_id: String,
    pub(crate) agent_id: String,
    #[serde(default)]
    pub(crate) seen_seq: u64,
    #[serde(default)]
    pub(crate) is_pinned: bool,
    #[serde(default)]
    pub(crate) pinned_order: i64,
    #[serde(default)]
    pub(crate) updated_at_ms: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OhosAskSideQuestionPayload {
    pub(crate) host_id: String,
    #[serde(default)]
    pub(crate) agent_id: Option<String>,
    pub(crate) cwd: String,
    pub(crate) provider: String,
    #[serde(default)]
    pub(crate) model: Option<String>,
    pub(crate) question: String,
    #[serde(default)]
    pub(crate) context_summary: Option<String>,
}

fn default_true_bool() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub(crate) struct OhosCreateSchedulePayload {
    pub(crate) host_id: String,
    pub(crate) name: String,
    pub(crate) cron_expr: String,
    #[serde(default)]
    pub(crate) timezone: Option<String>,
    pub(crate) prompt: String,
    #[serde(default)]
    pub(crate) existing_agent_id: Option<String>,
    #[serde(default)]
    pub(crate) cwd: String,
    #[serde(default)]
    pub(crate) workspace_id: Option<String>,
    #[serde(default)]
    pub(crate) provider: String,
    #[serde(default)]
    pub(crate) acp_agent_id: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) effort: Option<String>,
    #[serde(default)]
    pub(crate) fast_mode: bool,
    #[serde(default)]
    pub(crate) mode: String,
    #[serde(default)]
    pub(crate) worktree: bool,
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
    #[serde(default = "default_true_bool")]
    pub(crate) archive_on_finish: bool,
    #[serde(default)]
    pub(crate) background: bool,
    #[serde(default)]
    pub(crate) max_runs: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OhosUpdateSchedulePayload {
    pub(crate) host_id: String,
    pub(crate) schedule_id: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) cron_expr: Option<String>,
    #[serde(default)]
    pub(crate) timezone: Option<String>,
    #[serde(default)]
    pub(crate) prompt: Option<String>,
    #[serde(default)]
    pub(crate) max_runs: Option<u32>,
    #[serde(default)]
    pub(crate) archive_on_finish: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OhosGitPushPayload {
    pub(crate) host_id: String,
    pub(crate) cwd: String,
    #[serde(default)]
    pub(crate) remote: Option<String>,
    #[serde(default)]
    pub(crate) branch: Option<String>,
    #[serde(default)]
    pub(crate) set_upstream: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OhosGitDiscardFilesPayload {
    pub(crate) host_id: String,
    pub(crate) cwd: String,
    #[serde(default)]
    pub(crate) paths: Vec<String>,
    #[serde(default)]
    pub(crate) include_untracked: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OhosHighlightWorkspaceCodePayload {
    #[serde(default)]
    pub(crate) language: String,
    #[serde(default)]
    pub(crate) code: String,
    #[serde(default)]
    pub(crate) max_lines: Option<u32>,
    #[serde(default)]
    pub(crate) plain_line_numbers: Vec<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OhosCreateWorkspaceV2Payload {
    pub(crate) host_id: String,
    pub(crate) source_cwd: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) isolation: Option<String>,
    #[serde(default)]
    pub(crate) worktree_mode: Option<String>,
    #[serde(default)]
    pub(crate) worktree_branch: Option<String>,
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
    #[serde(default)]
    pub(crate) pr_number: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OhosUpdateWorkspaceBoardCasPayload {
    pub(crate) host_id: String,
    pub(crate) workspace_id: String,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) section: Option<String>,
    #[serde(default)]
    pub(crate) updated_by: Option<String>,
    #[serde(default)]
    pub(crate) expected_revision: Option<u64>,
}
