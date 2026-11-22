//! Pure-Rust Batch CSV Fan-Out, Programmable DAG Workflows & Best-of-N Parallel Arena Engine.
//!
//! Zero external runtime dependencies (no Node.js / Python), `unsafe_code = "deny"`, zero production
//! `.unwrap()`.
//!
//! Security & reliability guarantees:
//! - Authorizes workspace `cwd` via `authorize_workspace_mutation_cwd` before launching runs or
//!   applying a winning Best-of-N worktree.
//! - Enforces global (`MAX_ACTIVE_WORKFLOW_RUNS = 8`) and per-owner (`MAX_ACTIVE_WORKFLOW_RUNS_PER_OWNER = 2`)
//!   active workflow run caps atomically before persistence or spawn.
//! - Tracks `owner_agent_id`, rejects recursive workflow launches from child/workflow step agents,
//!   clamps MCP-requested `SessionMode` to never exceed the caller's mode, disallows `verify_command`
//!   from `Plan`/`Default` MCP callers, and enforces caller ownership on `cancel_workflow`.
//! - Caps batch CSV rows (`<= 200` enforced streaming inside the parser, `<= 2 MiB`), DAG nodes
//!   (`<= 64`), Best-of-N candidates (`2..=5`), concurrency (`1..=8`), and step retries (`<= 3`).
//! - Enforces DAG acyclicity via Kahn's topological sort and single-pass `{{col}}` /
//!   `{{step.<id>.output}}` template interpolation (immune to recursive template injection).
//! - Isolates Best-of-N candidate worktrees using Orbit's accelerated `WorktreePool` and applies
//!   winning changes via `worktree_handoff::execute_worktree_handoff_paths` with per-run RAII lock,
//!   active-run drain coordination, double-apply prevention, and backup-ref safety.
//! - Runs optional verification commands with scrubbed dangerous env vars (`LD_*`, `DYLD_*`,
//!   `ORBIT_*`, `NODE_OPTIONS`, `PYTHONPATH`, etc.), isolated Unix process group (`process_group(0)`),
//!   120s hard timeout, pre-spawn cancel check, and full PGID cleanup on exit/timeout/cancel/shutdown.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orbit_protocol::{
    AgentStatus, BestOfNCandidateSpec, DagStepSpec, ProviderKind, ReasoningEffort, SessionMode,
    WorkflowKind, WorkflowRunRecord, WorkflowRunStatus, WorkflowStepRecord, WorkflowStepStatus,
    WorktreeCreateMode, WorktreeHandoffDirection, WorktreeHandoffMode,
};
use orbit_store::OrbitStore;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::{Mutex, Semaphore, broadcast, mpsc, watch};
use tokio::task::AbortHandle;
use tracing::{info, warn};
use uuid::Uuid;

use crate::goals::is_dangerous_env_var;
use crate::keep_awake::KeepAwakeManager;
use crate::manager::{AgentOrchestrator, CreateAgentSpec, DaemonBroadcast};
use crate::workspace_files::canonicalize_workspace_root;
use crate::worktree_handoff::{
    HandoffExecutePathsSpec, execute_worktree_handoff_paths, resolve_git_checkout_info,
};

/// Maximum simultaneous active workflow runs daemon-wide.
pub const MAX_ACTIVE_WORKFLOW_RUNS: usize = 8;
/// Maximum simultaneous active workflow runs owned by a single calling agent.
pub const MAX_ACTIVE_WORKFLOW_RUNS_PER_OWNER: usize = 2;
/// Maximum data rows allowed in a single Batch CSV Fan-Out run.
pub const MAX_BATCH_CSV_ROWS: usize = 200;
/// Maximum raw byte size of CSV content (2 MiB).
pub const MAX_BATCH_CSV_BYTES: usize = 2 * 1024 * 1024;
/// Maximum nodes allowed in a Programmable DAG Workflow.
pub const MAX_DAG_NODES: usize = 64;
/// Minimum candidates allowed in a Best-of-N Arena run.
pub const MIN_BEST_OF_N_CANDIDATES: usize = 2;
/// Maximum candidates allowed in a Best-of-N Arena run.
pub const MAX_BEST_OF_N_CANDIDATES: usize = 5;
/// Concurrency bounds for semaphore-controlled execution.
pub const MIN_CONCURRENCY: u32 = 1;
pub const MAX_CONCURRENCY: u32 = 8;
/// Maximum per-step retry attempts.
pub const MAX_STEP_RETRIES: u32 = 3;
/// Step timeout bounds in seconds.
pub const MIN_STEP_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_STEP_TIMEOUT_SECS: u64 = 1800;
pub const MAX_STEP_TIMEOUT_SECS: u64 = 14_400;
/// Hard timeout for optional Best-of-N verification command.
pub const VERIFY_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
/// Maximum captured verification command output tail (16 KiB).
const MAX_VERIFY_OUTPUT_BYTES: usize = 16 * 1024;
/// Maximum stored assistant output preview length (chars).
const MAX_OUTPUT_PREVIEW_CHARS: usize = 2000;
/// Maximum step ID length.
const MAX_STEP_ID_LEN: usize = 64;
/// Maximum untracked files inspected when computing git diff stats.
const MAX_UNTRACKED_DIFF_FILES: usize = 200;

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn keep_awake_workflow_key(run_id: &str) -> String {
    format!("workflow_run:{run_id}")
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let prefix: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{prefix}…")
}

fn sanitize_branch_slug(raw: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
        if slug.len() >= 24 {
            break;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "step".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Build a unique worktree branch name for a workflow step, appending `-a{attempt}` on retries.
pub(crate) fn workflow_step_branch_name(run_id: &str, step_id: &str, attempt: u32) -> String {
    let short_run: String = run_id.chars().take(12).collect();
    let base = format!(
        "orbit/{}-{}",
        sanitize_branch_slug(&short_run),
        sanitize_branch_slug(step_id)
    );
    if attempt > 1 {
        format!("{base}-a{attempt}")
    } else {
        base
    }
}

// ==================== MCP OWNERSHIP & PRIVILEGE CLAMPING ====================

fn session_mode_privilege_rank(mode: &SessionMode) -> u8 {
    match mode {
        SessionMode::Plan => 0,
        SessionMode::Default | SessionMode::Custom(_) => 1,
        SessionMode::Auto => 2,
        SessionMode::AcceptEdits => 3,
        SessionMode::FullAccess => 4,
    }
}

/// Clamp an MCP-requested worker `SessionMode` so it can NEVER exceed the calling agent's mode.
pub fn clamp_workflow_session_mode(
    caller_mode: &SessionMode,
    requested: SessionMode,
) -> SessionMode {
    if session_mode_privilege_rank(&requested) > session_mode_privilege_rank(caller_mode) {
        caller_mode.clone()
    } else {
        requested
    }
}

/// Reject `verify_command` execution when invoked by a restricted (`Plan` / `Default` / `Custom`)
/// MCP calling agent. Direct UI/CLI callers (`caller_mode == None`) are allowed.
pub fn validate_mcp_verify_command(
    caller_mode: Option<&SessionMode>,
    verify_command: Option<&str>,
) -> Result<(), String> {
    let has_cmd = verify_command.map(str::trim).is_some_and(|s| !s.is_empty());
    if !has_cmd {
        return Ok(());
    }
    if let Some(mode) = caller_mode
        && matches!(
            mode,
            SessionMode::Plan | SessionMode::Default | SessionMode::Custom(_)
        )
    {
        return Err(
            "workflow_permission_denied: `verify_command` is not allowed from `plan` or `default` mode agents"
                .into(),
        );
    }
    Ok(())
}

/// Reject recursive workflow launches from child subagents or workflow step workers.
pub fn authorize_workflow_launch_from_caller(
    store: &OrbitStore,
    caller_agent_id: Option<&str>,
) -> Result<(), String> {
    let Some(cid) = caller_agent_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let caller = store
        .get_agent(cid)
        .map_err(|e| format!("failed to inspect caller agent `{cid}`: {e}"))?
        .ok_or_else(|| format!("workflow_caller_unknown: caller agent `{cid}` not found"))?;
    if caller.parent_agent_id.is_some() {
        return Err(
            "workflow_recursion_denied: child/subagent sessions cannot recursively launch workflows"
                .into(),
        );
    }
    if store
        .is_workflow_step_agent(cid)
        .map_err(|e| format!("failed to check workflow step membership: {e}"))?
    {
        return Err(
            "workflow_recursion_denied: workflow step agents cannot recursively launch workflows"
                .into(),
        );
    }
    Ok(())
}

/// Enforce caller ownership before cancelling a workflow run (both locally and remotely).
///
/// - Direct UI/CLI callers (`caller_agent_id == None`) may cancel any run.
/// - MCP agent callers may ONLY cancel runs where `run.owner_agent_id == Some(caller_id)`.
///   Cancelling `owner_agent_id: None` (user-initiated) or another agent's runs is forbidden.
pub fn authorize_workflow_cancel(
    caller_agent_id: Option<&str>,
    run: &WorkflowRunRecord,
) -> Result<(), String> {
    let Some(cid) = caller_agent_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    match run.owner_agent_id.as_deref() {
        Some(owner) if owner == cid => Ok(()),
        Some(_) => Err(format!(
            "workflow_cancel_denied: agent `{cid}` does not own workflow run `{}`",
            run.run_id
        )),
        None => Err(format!(
            "workflow_cancel_denied: agent `{cid}` cannot cancel user-owned workflow run `{}`",
            run.run_id
        )),
    }
}

// ==================== PURE-RUST RFC 4180 CSV PARSER ====================

/// Parse RFC 4180 CSV content into header-keyed row maps.
///
/// Supports quoted commas, quoted newlines (LF/CRLF), escaped double quotes (`""`),
/// UTF-8 BOM stripping, and enforces `1..=MAX_BATCH_CSV_ROWS` (200) data rows directly
/// inside the row parser to prevent heap amplification on adversarial 2 MiB inputs.
pub fn parse_csv_records(csv_content: &str) -> Result<Vec<BTreeMap<String, String>>, String> {
    if csv_content.len() > MAX_BATCH_CSV_BYTES {
        return Err(format!(
            "csv_too_large: CSV input exceeds {} byte limit",
            MAX_BATCH_CSV_BYTES
        ));
    }
    let input = csv_content.strip_prefix('\u{feff}').unwrap_or(csv_content);
    let raw_rows = parse_rfc4180_rows(input, MAX_BATCH_CSV_ROWS)?;

    let mut non_empty_rows = raw_rows.into_iter();
    let Some(header_row) = non_empty_rows.next() else {
        return Err(
            "csv_empty: CSV content must contain a header row and at least one data row".into(),
        );
    };

    let mut headers: Vec<String> = Vec::with_capacity(header_row.len());
    let mut seen_headers: HashSet<String> = HashSet::with_capacity(header_row.len());
    for (idx, raw_h) in header_row.into_iter().enumerate() {
        let h = raw_h.trim().to_string();
        if h.is_empty() {
            return Err(format!(
                "csv_invalid_header: header column {} must not be empty",
                idx + 1
            ));
        }
        if !seen_headers.insert(h.clone()) {
            return Err(format!(
                "csv_duplicate_header: duplicate header column `{h}`"
            ));
        }
        headers.push(h);
    }

    let mut records: Vec<BTreeMap<String, String>> = Vec::new();
    for (row_idx, row) in non_empty_rows.enumerate() {
        if row.len() > headers.len() {
            return Err(format!(
                "csv_column_mismatch: data row {} has {} column(s), expected at most {}",
                row_idx + 1,
                row.len(),
                headers.len()
            ));
        }
        let mut map = BTreeMap::new();
        for (col_idx, header) in headers.iter().enumerate() {
            let val = row.get(col_idx).cloned().unwrap_or_default();
            map.insert(header.clone(), val);
        }
        records.push(map);
    }

    if records.is_empty() {
        return Err(
            "csv_no_data_rows: CSV must contain at least one data row after the header".into(),
        );
    }

    Ok(records)
}

fn is_empty_csv_row(row: &[String]) -> bool {
    row.is_empty() || (row.len() == 1 && row[0].trim().is_empty())
}

fn push_bounded_csv_row(
    rows: &mut Vec<Vec<String>>,
    row: Vec<String>,
    max_data_rows: usize,
) -> Result<(), String> {
    if is_empty_csv_row(&row) {
        return Ok(());
    }
    // 1 header row + `max_data_rows` data rows
    if rows.len() > max_data_rows {
        return Err(format!(
            "csv_row_limit_exceeded: CSV contains more than {max_data_rows} data rows"
        ));
    }
    rows.push(row);
    Ok(())
}

fn parse_rfc4180_rows(input: &str, max_data_rows: usize) -> Result<Vec<Vec<String>>, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut just_closed_quote = false;
    let mut field_started = false;

    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    // Escaped double-quote `""` -> `"`
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                    just_closed_quote = true;
                }
            } else {
                field.push(ch);
            }
            continue;
        }

        if just_closed_quote {
            match ch {
                ',' => {
                    row.push(std::mem::take(&mut field));
                    just_closed_quote = false;
                    field_started = false;
                }
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    row.push(std::mem::take(&mut field));
                    push_bounded_csv_row(&mut rows, std::mem::take(&mut row), max_data_rows)?;
                    just_closed_quote = false;
                    field_started = false;
                }
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    push_bounded_csv_row(&mut rows, std::mem::take(&mut row), max_data_rows)?;
                    just_closed_quote = false;
                    field_started = false;
                }
                ' ' | '\t' => {
                    // Tolerate optional trailing horizontal whitespace after closing quote
                }
                other => {
                    return Err(format!(
                        "csv_parse_error: unexpected character `{other}` after closing quote"
                    ));
                }
            }
            continue;
        }

        match ch {
            '"' => {
                if !field_started {
                    in_quotes = true;
                    field_started = true;
                } else {
                    return Err("csv_parse_error: unescaped quote inside unquoted field".into());
                }
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                field_started = false;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                push_bounded_csv_row(&mut rows, std::mem::take(&mut row), max_data_rows)?;
                field_started = false;
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                push_bounded_csv_row(&mut rows, std::mem::take(&mut row), max_data_rows)?;
                field_started = false;
            }
            other => {
                field.push(other);
                field_started = true;
            }
        }
    }

    if in_quotes {
        return Err("csv_parse_error: unterminated quoted field".into());
    }

    if field_started || !field.is_empty() || !row.is_empty() || just_closed_quote {
        row.push(field);
        push_bounded_csv_row(&mut rows, row, max_data_rows)?;
    }

    Ok(rows)
}

/// Single-pass `{{col}}` template interpolation against a CSV row map.
///
/// Substituted values are never re-scanned, preventing nested template injection.
pub fn interpolate_csv_template(template: &str, row: &BTreeMap<String, String>) -> String {
    interpolate_double_braces(template, |key| row.get(key).map(String::as_str))
}

/// Single-pass `{{step.<id>.output}}` template interpolation against completed DAG step outputs.
pub fn interpolate_dag_template(template: &str, outputs: &HashMap<String, String>) -> String {
    interpolate_double_braces(template, |key| {
        let rest = key.strip_prefix("step.")?;
        let step_id = rest.strip_suffix(".output")?;
        let clean_id = step_id.trim();
        if clean_id.is_empty() {
            return None;
        }
        Some(outputs.get(clean_id).map(String::as_str).unwrap_or(""))
    })
}

fn interpolate_double_braces<'a, F>(template: &str, mut lookup: F) -> String
where
    F: FnMut(&str) -> Option<&'a str>,
{
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open_idx) = rest.find("{{") {
        out.push_str(&rest[..open_idx]);
        let after_open = &rest[open_idx + 2..];
        let Some(close_idx) = after_open.find("}}") else {
            out.push_str(&rest[open_idx..]);
            return out;
        };
        let raw_key = &after_open[..close_idx];
        let key = raw_key.trim();
        if let Some(replacement) = lookup(key) {
            out.push_str(replacement);
        } else {
            out.push_str("{{");
            out.push_str(raw_key);
            out.push_str("}}");
        }
        rest = &after_open[close_idx + 2..];
    }
    out.push_str(rest);
    out
}

// ==================== PURE-RUST DAG VALIDATOR & KAHN'S TOPO SORT ====================

/// Validate DAG node specs (count cap, unique valid step IDs, known dependencies, acyclicity)
/// and return the deterministic Kahn topological execution order of step IDs.
pub fn validate_dag_steps(steps: &[DagStepSpec]) -> Result<Vec<String>, String> {
    if steps.is_empty() {
        return Err("dag_empty: DAG workflow must contain at least one step".into());
    }
    if steps.len() > MAX_DAG_NODES {
        return Err(format!(
            "dag_too_large: DAG workflow has {} steps, exceeding limit of {MAX_DAG_NODES}",
            steps.len()
        ));
    }

    let mut id_to_idx: HashMap<&str, usize> = HashMap::with_capacity(steps.len());
    for (idx, step) in steps.iter().enumerate() {
        let sid = step.step_id.trim();
        if sid.is_empty() {
            return Err(format!(
                "dag_invalid_step_id: step {} has an empty `step_id`",
                idx + 1
            ));
        }
        if sid.len() > MAX_STEP_ID_LEN
            || !sid
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        {
            return Err(format!(
                "dag_invalid_step_id: step_id `{sid}` must be 1..={MAX_STEP_ID_LEN} chars of [A-Za-z0-9_.-]"
            ));
        }
        if step.prompt_template.trim().is_empty() {
            return Err(format!(
                "dag_empty_prompt: step `{sid}` must have a non-empty prompt_template"
            ));
        }
        if id_to_idx.insert(sid, idx).is_some() {
            return Err(format!("dag_duplicate_step_id: duplicate step_id `{sid}`"));
        }
    }

    let n = steps.len();
    let mut in_degree: Vec<usize> = vec![0; n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (idx, step) in steps.iter().enumerate() {
        let sid = step.step_id.trim();
        let mut seen_deps: HashSet<&str> = HashSet::new();
        for dep_raw in &step.depends_on {
            let dep = dep_raw.trim();
            if dep.is_empty() {
                continue;
            }
            if dep == sid {
                return Err(format!(
                    "dag_cycle_detected: step `{sid}` cannot depend on itself"
                ));
            }
            let Some(&dep_idx) = id_to_idx.get(dep) else {
                return Err(format!(
                    "dag_missing_dependency: step `{sid}` depends on unknown step `{dep}`"
                ));
            };
            if seen_deps.insert(dep) {
                dependents[dep_idx].push(idx);
                in_degree[idx] += 1;
            }
        }
    }

    // Kahn's topological sort using a BTreeSet for deterministic declaration-order tie-breaking
    let mut ready: BTreeSet<usize> = BTreeSet::new();
    for (idx, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            ready.insert(idx);
        }
    }

    let mut ordered: Vec<String> = Vec::with_capacity(n);
    while let Some(u) = ready.pop_first() {
        ordered.push(steps[u].step_id.trim().to_string());
        for &v in &dependents[u] {
            in_degree[v] = in_degree[v].saturating_sub(1);
            if in_degree[v] == 0 {
                ready.insert(v);
            }
        }
    }

    if ordered.len() != n {
        let cyclic: Vec<&str> = steps
            .iter()
            .enumerate()
            .filter(|(idx, _)| in_degree[*idx] > 0)
            .map(|(_, s)| s.step_id.trim())
            .collect();
        return Err(format!(
            "dag_cycle_detected: cycle detected involving step(s): {}",
            cyclic.join(", ")
        ));
    }

    Ok(ordered)
}

/// Validate Best-of-N Arena candidate list (`2..=5` candidates) and optional effort slugs.
pub fn validate_best_of_n_candidates(
    prompt: &str,
    candidates: &[BestOfNCandidateSpec],
) -> Result<Vec<(String, BestOfNCandidateSpec, Option<ReasoningEffort>)>, String> {
    if prompt.trim().is_empty() {
        return Err("best_of_n_empty_prompt: prompt must not be empty".into());
    }
    if candidates.len() < MIN_BEST_OF_N_CANDIDATES || candidates.len() > MAX_BEST_OF_N_CANDIDATES {
        return Err(format!(
            "best_of_n_invalid_candidates: expected {MIN_BEST_OF_N_CANDIDATES}..={MAX_BEST_OF_N_CANDIDATES} candidates, got {}",
            candidates.len()
        ));
    }

    let mut normalized = Vec::with_capacity(candidates.len());
    for (idx, cand) in candidates.iter().enumerate() {
        let effort = match cand
            .effort
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(slug) => Some(ReasoningEffort::from_slug(slug).ok_or_else(|| {
                format!(
                    "best_of_n_invalid_effort: unknown reasoning effort `{slug}` on candidate {}",
                    idx + 1
                )
            })?),
            None => None,
        };
        let label = {
            let trimmed = cand.label.trim();
            if trimmed.is_empty() {
                format!("{} #{}", cand.provider.display_name(), idx + 1)
            } else {
                trimmed.to_string()
            }
        };
        let mut spec = cand.clone();
        spec.label = label;
        normalized.push((format!("cand_{}", idx + 1), spec, effort));
    }
    Ok(normalized)
}

// ==================== STEP EXECUTION ABSTRACTION ====================

#[derive(Debug, Clone)]
pub struct StartBatchFanoutSpec {
    pub name: String,
    pub cwd: String,
    pub owner_agent_id: Option<String>,
    pub csv_content: String,
    pub prompt_template: String,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub mode: SessionMode,
    pub max_concurrency: u32,
    pub isolated_worktree: bool,
}

#[derive(Debug, Clone)]
pub struct StartDagWorkflowSpec {
    pub name: String,
    pub cwd: String,
    pub owner_agent_id: Option<String>,
    pub steps: Vec<DagStepSpec>,
    pub default_provider: ProviderKind,
    pub default_mode: SessionMode,
    pub max_concurrency: u32,
}

#[derive(Debug, Clone)]
pub struct StartBestOfNSpec {
    pub name: String,
    pub cwd: String,
    pub owner_agent_id: Option<String>,
    pub prompt: String,
    pub candidates: Vec<BestOfNCandidateSpec>,
    pub mode: SessionMode,
    pub verify_command: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StepExecutionRequest {
    pub run_id: String,
    pub run_name: String,
    pub owner_agent_id: Option<String>,
    pub kind: WorkflowKind,
    pub step_id: String,
    pub step_name: String,
    /// 1-based execution attempt number (incremented on DAG retries).
    pub attempt: u32,
    pub workspace_cwd: PathBuf,
    pub prompt: String,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
    pub mode: SessionMode,
    pub isolated_worktree: bool,
    pub timeout: Duration,
    pub verify_command: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StepExecutionOutcome {
    pub succeeded: bool,
    pub agent_id: Option<String>,
    pub worktree_cwd: Option<String>,
    pub output_preview: Option<String>,
    pub diff_additions: u32,
    pub diff_deletions: u32,
    pub diff_files: u32,
    pub verify_passed: Option<bool>,
    pub verify_output: Option<String>,
    pub error: Option<String>,
    pub canceled: bool,
}

/// Callback used by a step executor to report incremental step metadata (agent spawned,
/// entering verification phase) through the ordered coordinator mpsc channel.
pub type StepProgressFn = Arc<dyn Fn(StepProgressUpdate) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum StepProgressUpdate {
    AgentSpawned {
        agent_id: String,
        worktree_cwd: Option<String>,
    },
    Verifying,
}

/// Pluggable step runner (production: `AgentOrchestrator`, tests: deterministic mock).
pub trait WorkflowStepExecutor: Send + Sync + 'static {
    fn execute_step(
        &self,
        req: StepExecutionRequest,
        cancel_rx: watch::Receiver<bool>,
        active_pgids: Arc<std::sync::Mutex<Vec<u32>>>,
        on_progress: StepProgressFn,
    ) -> Pin<Box<dyn Future<Output = StepExecutionOutcome> + Send>>;

    fn archive_agent_and_worktree(
        &self,
        agent_id: String,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// Production step executor backed by Orbit's `AgentOrchestrator` + `OrbitStore`.
struct OrchestratorStepExecutor {
    orchestrator: Weak<AgentOrchestrator>,
    store: Arc<OrbitStore>,
}

impl WorkflowStepExecutor for OrchestratorStepExecutor {
    fn execute_step(
        &self,
        req: StepExecutionRequest,
        mut cancel_rx: watch::Receiver<bool>,
        active_pgids: Arc<std::sync::Mutex<Vec<u32>>>,
        on_progress: StepProgressFn,
    ) -> Pin<Box<dyn Future<Output = StepExecutionOutcome> + Send>> {
        let orch_weak = self.orchestrator.clone();
        let store = Arc::clone(&self.store);
        Box::pin(async move {
            if *cancel_rx.borrow() {
                return StepExecutionOutcome {
                    canceled: true,
                    error: Some("Canceled before start".into()),
                    ..Default::default()
                };
            }
            let Some(orch) = orch_weak.upgrade() else {
                return StepExecutionOutcome {
                    succeeded: false,
                    error: Some("Daemon orchestrator unavailable".into()),
                    ..Default::default()
                };
            };

            // Subscribe BEFORE creating the agent so we cannot miss fast status transitions.
            let mut bcast_rx = orch.broadcast_tx.subscribe();

            let branch_name = req
                .isolated_worktree
                .then(|| workflow_step_branch_name(&req.run_id, &req.step_id, req.attempt));

            let title = format!("[{}] {}", req.run_name, req.step_name);
            let spec = CreateAgentSpec {
                cwd: req.workspace_cwd.to_string_lossy().to_string(),
                extra_dirs: Vec::new(),
                title: Some(title),
                provider: req.provider,
                model: req.model.clone(),
                effort: req.effort,
                fast_mode: None,
                mode: req.mode.clone(),
                initial_prompt: req.prompt.clone(),
                initial_display_prompt: None,
                initial_badges: Vec::new(),
                worktree: req.isolated_worktree,
                workspace_id: None,
                parent_agent_id: req.owner_agent_id.clone(),
                parent_host_id: None,
                profile_id: None,
                background: req.kind == WorkflowKind::BatchCsv,
                auto_archive: false,
                base_ref: None,
                worktree_mode: WorktreeCreateMode::BranchOff,
                branch: branch_name,
                pr_number: None,
                handle: None,
                auto_cluster: Some(false),
                acp_agent_id: None,
            };

            let agent = match orch.create_and_start_agent(spec).await {
                Ok(ag) => ag,
                Err(e) => {
                    return StepExecutionOutcome {
                        succeeded: false,
                        error: Some(format!("Failed to launch step agent: {e}")),
                        ..Default::default()
                    };
                }
            };

            let agent_id = agent.agent_id.clone();
            let worktree_cwd = if req.isolated_worktree {
                Some(agent.cwd.clone())
            } else {
                None
            };
            let base_head = resolve_git_head_oid(Path::new(&agent.cwd)).await;

            on_progress(StepProgressUpdate::AgentSpawned {
                agent_id: agent_id.clone(),
                worktree_cwd: worktree_cwd.clone(),
            });

            // Wait for the spawned agent's initial turn to settle (Idle / Error / Closed)
            // or timeout / cancel.
            let deadline = tokio::time::Instant::now() + req.timeout;
            let mut poll_tick = tokio::time::interval(Duration::from_secs(1));
            poll_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            let mut final_status: Option<AgentStatus> = None;
            let mut timed_out = false;
            let mut was_canceled = false;

            if let Ok(Some(cur)) = store.get_agent(&agent_id)
                && is_agent_step_settled(&cur.status)
            {
                final_status = Some(cur.status);
            }

            while final_status.is_none() {
                tokio::select! {
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            was_canceled = true;
                            let _ = orch.interrupt_agent(&agent_id).await;
                            break;
                        }
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        timed_out = true;
                        let _ = orch.interrupt_agent(&agent_id).await;
                        break;
                    }
                    ev = bcast_rx.recv() => {
                        match ev {
                            Ok(DaemonBroadcast::AgentUpdated(updated)) if updated.agent_id == agent_id => {
                                if is_agent_step_settled(&updated.status) {
                                    final_status = Some(updated.status);
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                if let Ok(Some(cur)) = store.get_agent(&agent_id)
                                    && is_agent_step_settled(&cur.status)
                                {
                                    final_status = Some(cur.status);
                                }
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                            _ => {}
                        }
                    }
                    _ = poll_tick.tick() => {
                        if let Ok(Some(cur)) = store.get_agent(&agent_id)
                            && is_agent_step_settled(&cur.status)
                        {
                            final_status = Some(cur.status);
                        }
                    }
                }
            }

            let output_preview = extract_latest_assistant_preview(&store, &agent_id);
            let step_dir = PathBuf::from(&agent.cwd);
            let (diff_additions, diff_deletions, diff_files) =
                compute_git_diff_stats(&step_dir, base_head.as_deref()).await;

            if was_canceled {
                return StepExecutionOutcome {
                    succeeded: false,
                    canceled: true,
                    agent_id: Some(agent_id),
                    worktree_cwd,
                    output_preview,
                    diff_additions,
                    diff_deletions,
                    diff_files,
                    error: Some("Workflow canceled".into()),
                    ..Default::default()
                };
            }

            if timed_out {
                return StepExecutionOutcome {
                    succeeded: false,
                    agent_id: Some(agent_id),
                    worktree_cwd,
                    output_preview,
                    diff_additions,
                    diff_deletions,
                    diff_files,
                    error: Some(format!("Step timed out after {}s", req.timeout.as_secs())),
                    ..Default::default()
                };
            }

            let (agent_ok, agent_err) = match final_status {
                Some(AgentStatus::Idle) => (true, None),
                Some(AgentStatus::Error { message, .. }) => (false, Some(message)),
                Some(AgentStatus::Closed) => {
                    (false, Some("Agent session closed unexpectedly".into()))
                }
                _ => (false, Some("Agent session ended before settling".into())),
            };

            if !agent_ok {
                return StepExecutionOutcome {
                    succeeded: false,
                    agent_id: Some(agent_id),
                    worktree_cwd,
                    output_preview,
                    diff_additions,
                    diff_deletions,
                    diff_files,
                    error: agent_err,
                    ..Default::default()
                };
            }

            // Optional scrubbed verification command (typically on Best-of-N Arena)
            let (verify_passed, verify_output) = if let Some(vcmd) = req
                .verify_command
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                on_progress(StepProgressUpdate::Verifying);
                let vres = run_workflow_verify_command(
                    &step_dir,
                    vcmd,
                    VERIFY_COMMAND_TIMEOUT,
                    cancel_rx,
                    active_pgids,
                )
                .await;
                if vres.canceled {
                    return StepExecutionOutcome {
                        succeeded: false,
                        canceled: true,
                        agent_id: Some(agent_id),
                        worktree_cwd,
                        output_preview,
                        diff_additions,
                        diff_deletions,
                        diff_files,
                        verify_passed: Some(false),
                        verify_output: Some(vres.output_tail),
                        error: Some("Canceled during verification".into()),
                    };
                }
                (Some(vres.exit_code == 0), Some(vres.output_tail))
            } else {
                (None, None)
            };

            StepExecutionOutcome {
                succeeded: true,
                agent_id: Some(agent_id),
                worktree_cwd,
                output_preview,
                diff_additions,
                diff_deletions,
                diff_files,
                verify_passed,
                verify_output,
                error: None,
                canceled: false,
            }
        })
    }

    fn archive_agent_and_worktree(
        &self,
        agent_id: String,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let orch_weak = self.orchestrator.clone();
        let store = Arc::clone(&self.store);
        Box::pin(async move {
            let Some(orch) = orch_weak.upgrade() else {
                return;
            };
            if let Ok(Some(ag)) = store.get_agent(&agent_id)
                && let Ok(Some(ws)) = store.get_workspace(&ag.workspace_id)
                && ws.is_worktree
            {
                let _ = orch.archive_workspace(&ws.workspace_id).await;
                return;
            }
            let _ = orch.archive_agent(&agent_id).await;
        })
    }
}

fn is_agent_step_settled(status: &AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Idle | AgentStatus::Error { .. } | AgentStatus::Closed
    )
}

fn extract_latest_assistant_preview(store: &OrbitStore, agent_id: &str) -> Option<String> {
    let text = store.latest_assistant_message(agent_id).ok().flatten()?;
    Some(truncate_chars(&text, MAX_OUTPUT_PREVIEW_CHARS))
}

// ==================== WORKFLOW ENGINE ====================

#[derive(Debug, Clone)]
pub struct WorkflowEngineConfig {
    /// Enforce `authorize_workspace_mutation_cwd` against registered projects (true in production).
    pub enforce_registered_cwd: bool,
    /// Require a valid Git repository for Best-of-N runs (true in production; mock tests may override).
    pub require_git_for_best_of_n: bool,
}

impl Default for WorkflowEngineConfig {
    fn default() -> Self {
        Self {
            enforce_registered_cwd: true,
            require_git_for_best_of_n: true,
        }
    }
}

#[derive(Debug, Clone)]
struct ExecutableStepSpec {
    step_id: String,
    name: String,
    depends_on: Vec<String>,
    prompt_template: String,
    is_dag_template: bool,
    provider: ProviderKind,
    model: Option<String>,
    effort: Option<ReasoningEffort>,
    mode: SessionMode,
    isolated_worktree: bool,
    retry_count: u32,
    timeout: Duration,
}

struct ActiveRunHandle {
    owner_agent_id: Option<String>,
    cancel_tx: watch::Sender<bool>,
    finished_rx: watch::Receiver<bool>,
    run_state: Arc<Mutex<WorkflowRunRecord>>,
    active_pgids: Arc<std::sync::Mutex<Vec<u32>>>,
    step_aborts: Arc<Mutex<Vec<AbortHandle>>>,
    driver_abort: Option<AbortHandle>,
}

/// RAII guard guaranteeing `KeepAwakeManager` lease release when a workflow task exits.
struct WorkflowKeepAwakeGuard {
    keep_awake: Arc<KeepAwakeManager>,
    key: String,
}

impl WorkflowKeepAwakeGuard {
    fn acquire(keep_awake: Arc<KeepAwakeManager>, run_id: &str) -> Self {
        let key = keep_awake_workflow_key(run_id);
        keep_awake.mark_busy(&key);
        Self { keep_awake, key }
    }
}

impl Drop for WorkflowKeepAwakeGuard {
    fn drop(&mut self) {
        self.keep_awake.mark_idle(&self.key);
    }
}

/// RAII per-`run_id` mutual exclusion guard preventing concurrent Best-of-N winner applications.
struct WinnerApplyGuard {
    set: Arc<std::sync::Mutex<HashSet<String>>>,
    run_id: String,
}

impl WinnerApplyGuard {
    fn try_acquire(
        set: &Arc<std::sync::Mutex<HashSet<String>>>,
        run_id: &str,
    ) -> Result<Self, String> {
        let mut guard = set.lock().unwrap_or_else(|e| e.into_inner());
        if !guard.insert(run_id.to_string()) {
            return Err(format!(
                "winner_apply_in_progress: winner selection is already in progress for `{run_id}`"
            ));
        }
        Ok(Self {
            set: Arc::clone(set),
            run_id: run_id.to_string(),
        })
    }
}

impl Drop for WinnerApplyGuard {
    fn drop(&mut self) {
        let mut guard = self.set.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(&self.run_id);
    }
}

/// Orchestrates Batch CSV Fan-Out, Programmable DAG Workflows, and Best-of-N Parallel Arena runs.
pub struct WorkflowEngine {
    store: Arc<OrbitStore>,
    broadcast_tx: broadcast::Sender<DaemonBroadcast>,
    keep_awake: Arc<KeepAwakeManager>,
    executor: Arc<dyn WorkflowStepExecutor>,
    config: WorkflowEngineConfig,
    active_runs: Mutex<HashMap<String, ActiveRunHandle>>,
    applying_winners: Arc<std::sync::Mutex<HashSet<String>>>,
}

impl WorkflowEngine {
    pub fn new(
        store: Arc<OrbitStore>,
        orchestrator: &Arc<AgentOrchestrator>,
        keep_awake: Arc<KeepAwakeManager>,
    ) -> Arc<Self> {
        let executor: Arc<dyn WorkflowStepExecutor> = Arc::new(OrchestratorStepExecutor {
            orchestrator: Arc::downgrade(orchestrator),
            store: Arc::clone(&store),
        });
        Self::with_executor(
            store,
            orchestrator.broadcast_tx.clone(),
            keep_awake,
            executor,
            WorkflowEngineConfig::default(),
        )
    }

    pub fn with_executor(
        store: Arc<OrbitStore>,
        broadcast_tx: broadcast::Sender<DaemonBroadcast>,
        keep_awake: Arc<KeepAwakeManager>,
        executor: Arc<dyn WorkflowStepExecutor>,
        config: WorkflowEngineConfig,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            broadcast_tx,
            keep_awake,
            executor,
            config,
            active_runs: Mutex::new(HashMap::new()),
            applying_winners: Arc::new(std::sync::Mutex::new(HashSet::new())),
        })
    }

    /// Reconcile any workflow runs that were left `Pending` or `Running` across a daemon restart.
    pub async fn start_recovery(self: &Arc<Self>) {
        let Ok(active) = self.store.list_active_workflow_runs() else {
            return;
        };
        let now = now_unix_ms();
        for mut run in active {
            for step in &mut run.steps {
                if !step.status.is_terminal() {
                    step.status = WorkflowStepStatus::Canceled;
                    step.error = Some("Interrupted by daemon restart".into());
                    step.finished_at_ms = Some(now);
                }
            }
            run.status = WorkflowRunStatus::Canceled;
            run.updated_at_ms = now;
            run.finished_at_ms = Some(now);
            let _ = self.store.upsert_workflow_run(&run);
            self.emit_run_event(&run);
        }
    }

    /// Cancel all active runs, abort driver & step tasks first, then reap all verification PGIDs.
    pub async fn shutdown_all(&self) {
        let handles: Vec<(String, ActiveRunHandle)> = {
            let mut guard = self.active_runs.lock().await;
            guard.drain().collect()
        };
        for (run_id, handle) in handles {
            let _ = handle.cancel_tx.send(true);
            // 1. Abort driver and all spawned step tasks BEFORE draining active_pgids so no task
            //    can spawn a new verification process after PGID drain.
            if let Some(abort) = handle.driver_abort {
                abort.abort();
            }
            {
                let mut aborts = handle.step_aborts.lock().await;
                for abort in aborts.drain(..) {
                    abort.abort();
                }
            }
            // 2. Now terminate all tracked verification process groups.
            let pgids: Vec<u32> = handle
                .active_pgids
                .lock()
                .map(|mut g| g.drain(..).collect())
                .unwrap_or_default();
            for pid in pgids {
                terminate_process_group(Some(pid)).await;
            }
            self.keep_awake.mark_idle(&keep_awake_workflow_key(&run_id));
        }
    }

    /// List workflow runs from SQLite.
    pub fn list_runs(&self, include_completed: bool) -> Result<Vec<WorkflowRunRecord>, String> {
        self.store
            .list_workflow_runs(include_completed)
            .map_err(|e| format!("failed to list workflow runs: {e}"))
    }

    /// Fetch a single workflow run by `run_id`.
    pub fn get_run(&self, run_id: &str) -> Result<Option<WorkflowRunRecord>, String> {
        self.store
            .get_workflow_run(run_id)
            .map_err(|e| format!("failed to load workflow run `{run_id}`: {e}"))
    }

    /// Start a Batch CSV Fan-Out workflow run.
    pub async fn start_batch_fanout(
        self: &Arc<Self>,
        spec: StartBatchFanoutSpec,
    ) -> Result<WorkflowRunRecord, String> {
        if spec.prompt_template.trim().is_empty() {
            return Err("batch_empty_prompt: prompt_template must not be empty".into());
        }
        let rows = parse_csv_records(&spec.csv_content)?;
        let authorized_cwd = self.authorize_cwd(&spec.cwd).await?;
        let concurrency = spec.max_concurrency.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);
        let run_id = format!("wf_{}", Uuid::new_v4().simple());
        let now = now_unix_ms();
        let name = if spec.name.trim().is_empty() {
            format!("Batch CSV ({} rows)", rows.len())
        } else {
            spec.name.trim().to_string()
        };

        let mut step_records = Vec::with_capacity(rows.len());
        let mut exec_steps = Vec::with_capacity(rows.len());
        for (idx, row) in rows.iter().enumerate() {
            let step_id = format!("row_{}", idx + 1);
            let label = row
                .values()
                .find(|v| !v.trim().is_empty())
                .map(|v| format!("#{} {}", idx + 1, truncate_chars(v, 36)))
                .unwrap_or_else(|| format!("Row {}", idx + 1));
            let prompt = interpolate_csv_template(&spec.prompt_template, row);

            step_records.push(WorkflowStepRecord {
                run_id: run_id.clone(),
                step_id: step_id.clone(),
                name: label.clone(),
                status: WorkflowStepStatus::Pending,
                depends_on: Vec::new(),
                agent_id: None,
                worktree_cwd: None,
                provider: spec.provider,
                model: spec.model.clone(),
                output_preview: None,
                diff_additions: 0,
                diff_deletions: 0,
                diff_files: 0,
                verify_passed: None,
                verify_output: None,
                error: None,
                is_winner: false,
                started_at_ms: None,
                finished_at_ms: None,
            });
            exec_steps.push(ExecutableStepSpec {
                step_id,
                name: label,
                depends_on: Vec::new(),
                prompt_template: prompt,
                is_dag_template: false,
                provider: spec.provider,
                model: spec.model.clone(),
                effort: None,
                mode: spec.mode.clone(),
                isolated_worktree: spec.isolated_worktree,
                retry_count: 0,
                timeout: Duration::from_secs(DEFAULT_STEP_TIMEOUT_SECS),
            });
        }

        let run = WorkflowRunRecord {
            run_id,
            name,
            kind: WorkflowKind::BatchCsv,
            workspace_cwd: authorized_cwd.to_string_lossy().to_string(),
            owner_agent_id: spec.owner_agent_id,
            status: WorkflowRunStatus::Running,
            max_concurrency: concurrency,
            verify_command: None,
            winner_step_id: None,
            steps: step_records,
            created_at_ms: now,
            updated_at_ms: now,
            finished_at_ms: None,
        };

        self.launch_run(run, exec_steps, authorized_cwd).await
    }

    /// Start a Programmable DAG Workflow run.
    pub async fn start_dag_workflow(
        self: &Arc<Self>,
        spec: StartDagWorkflowSpec,
    ) -> Result<WorkflowRunRecord, String> {
        let topo_order = validate_dag_steps(&spec.steps)?;
        let authorized_cwd = self.authorize_cwd(&spec.cwd).await?;
        let concurrency = spec.max_concurrency.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);
        let run_id = format!("wf_{}", Uuid::new_v4().simple());
        let now = now_unix_ms();
        let name = if spec.name.trim().is_empty() {
            format!("DAG Workflow ({} steps)", spec.steps.len())
        } else {
            spec.name.trim().to_string()
        };

        let spec_by_id: HashMap<&str, &DagStepSpec> =
            spec.steps.iter().map(|s| (s.step_id.trim(), s)).collect();

        let mut step_records = Vec::with_capacity(topo_order.len());
        let mut exec_steps = Vec::with_capacity(topo_order.len());
        for sid in topo_order {
            let Some(st) = spec_by_id.get(sid.as_str()) else {
                continue;
            };
            let provider = st.provider.unwrap_or(spec.default_provider);
            let step_name = if st.name.trim().is_empty() {
                sid.clone()
            } else {
                st.name.trim().to_string()
            };
            let deps: Vec<String> = st
                .depends_on
                .iter()
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty())
                .collect();
            let timeout_secs = st
                .timeout_secs
                .clamp(MIN_STEP_TIMEOUT_SECS, MAX_STEP_TIMEOUT_SECS);

            step_records.push(WorkflowStepRecord {
                run_id: run_id.clone(),
                step_id: sid.clone(),
                name: step_name.clone(),
                status: WorkflowStepStatus::Pending,
                depends_on: deps.clone(),
                agent_id: None,
                worktree_cwd: None,
                provider,
                model: st.model.clone(),
                output_preview: None,
                diff_additions: 0,
                diff_deletions: 0,
                diff_files: 0,
                verify_passed: None,
                verify_output: None,
                error: None,
                is_winner: false,
                started_at_ms: None,
                finished_at_ms: None,
            });
            exec_steps.push(ExecutableStepSpec {
                step_id: sid,
                name: step_name,
                depends_on: deps,
                prompt_template: st.prompt_template.clone(),
                is_dag_template: true,
                provider,
                model: st.model.clone(),
                effort: None,
                mode: spec.default_mode.clone(),
                isolated_worktree: st.isolated_worktree,
                retry_count: st.retry_count.min(MAX_STEP_RETRIES),
                timeout: Duration::from_secs(timeout_secs),
            });
        }

        let run = WorkflowRunRecord {
            run_id,
            name,
            kind: WorkflowKind::Dag,
            workspace_cwd: authorized_cwd.to_string_lossy().to_string(),
            owner_agent_id: spec.owner_agent_id,
            status: WorkflowRunStatus::Running,
            max_concurrency: concurrency,
            verify_command: None,
            winner_step_id: None,
            steps: step_records,
            created_at_ms: now,
            updated_at_ms: now,
            finished_at_ms: None,
        };

        self.launch_run(run, exec_steps, authorized_cwd).await
    }

    /// Start a Best-of-N Parallel Arena run (each candidate isolated in its own Git worktree).
    pub async fn start_best_of_n(
        self: &Arc<Self>,
        spec: StartBestOfNSpec,
    ) -> Result<WorkflowRunRecord, String> {
        let candidates = validate_best_of_n_candidates(&spec.prompt, &spec.candidates)?;
        let authorized_cwd = self.authorize_cwd(&spec.cwd).await?;
        if self.config.require_git_for_best_of_n {
            resolve_git_checkout_info(&authorized_cwd)
                .await
                .map_err(|e| {
                    format!(
                        "best_of_n_requires_git: Best-of-N Arena requires a Git repository ({e})"
                    )
                })?;
        }

        let concurrency = (candidates.len() as u32).clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);
        let run_id = format!("wf_{}", Uuid::new_v4().simple());
        let now = now_unix_ms();
        let name = if spec.name.trim().is_empty() {
            format!("Best-of-{} Arena", candidates.len())
        } else {
            spec.name.trim().to_string()
        };
        let verify_command = spec
            .verify_command
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        let mut step_records = Vec::with_capacity(candidates.len());
        let mut exec_steps = Vec::with_capacity(candidates.len());
        for (step_id, cand, effort) in candidates {
            step_records.push(WorkflowStepRecord {
                run_id: run_id.clone(),
                step_id: step_id.clone(),
                name: cand.label.clone(),
                status: WorkflowStepStatus::Pending,
                depends_on: Vec::new(),
                agent_id: None,
                worktree_cwd: None,
                provider: cand.provider,
                model: cand.model.clone(),
                output_preview: None,
                diff_additions: 0,
                diff_deletions: 0,
                diff_files: 0,
                verify_passed: None,
                verify_output: None,
                error: None,
                is_winner: false,
                started_at_ms: None,
                finished_at_ms: None,
            });
            exec_steps.push(ExecutableStepSpec {
                step_id,
                name: cand.label,
                depends_on: Vec::new(),
                prompt_template: spec.prompt.clone(),
                is_dag_template: false,
                provider: cand.provider,
                model: cand.model,
                effort,
                mode: spec.mode.clone(),
                isolated_worktree: true,
                retry_count: 0,
                timeout: Duration::from_secs(DEFAULT_STEP_TIMEOUT_SECS),
            });
        }

        let run = WorkflowRunRecord {
            run_id,
            name,
            kind: WorkflowKind::BestOfN,
            workspace_cwd: authorized_cwd.to_string_lossy().to_string(),
            owner_agent_id: spec.owner_agent_id,
            status: WorkflowRunStatus::Running,
            max_concurrency: concurrency,
            verify_command,
            winner_step_id: None,
            steps: step_records,
            created_at_ms: now,
            updated_at_ms: now,
            finished_at_ms: None,
        };

        self.launch_run(run, exec_steps, authorized_cwd).await
    }

    /// Cancel an active workflow run (interrupting in-flight agents & verification processes)
    /// while updating the shared `run_state` so `drive_workflow_run` can never regress it back.
    pub async fn cancel_run(&self, run_id: &str) -> Result<WorkflowRunRecord, String> {
        let active_parts = {
            let guard = self.active_runs.lock().await;
            guard.get(run_id).map(|h| {
                let _ = h.cancel_tx.send(true);
                (Arc::clone(&h.run_state), Arc::clone(&h.active_pgids))
            })
        };

        if let Some((shared_state, active_pgids)) = active_parts {
            let pgids: Vec<u32> = active_pgids
                .lock()
                .map(|mut g| g.drain(..).collect())
                .unwrap_or_default();
            for pid in pgids {
                terminate_process_group(Some(pid)).await;
            }

            let mut run = shared_state.lock().await;
            if !run.status.is_terminal() {
                let now = now_unix_ms();
                for step in &mut run.steps {
                    if !step.status.is_terminal() {
                        step.status = WorkflowStepStatus::Canceled;
                        if step.error.is_none() {
                            step.error = Some("Canceled by user".into());
                        }
                        step.finished_at_ms = Some(now);
                    }
                }
                run.status = WorkflowRunStatus::Canceled;
                run.updated_at_ms = now;
                run.finished_at_ms = Some(now);
                self.store
                    .upsert_workflow_run(&run)
                    .map_err(|e| format!("failed to persist canceled workflow run: {e}"))?;
                self.emit_run_event(&run);
            }
            return Ok(run.clone());
        }

        let mut run = self
            .store
            .get_workflow_run(run_id)
            .map_err(|e| format!("failed to load workflow run `{run_id}`: {e}"))?
            .ok_or_else(|| format!("workflow_not_found: workflow run `{run_id}` not found"))?;

        if !run.status.is_terminal() {
            let now = now_unix_ms();
            for step in &mut run.steps {
                if !step.status.is_terminal() {
                    step.status = WorkflowStepStatus::Canceled;
                    if step.error.is_none() {
                        step.error = Some("Canceled by user".into());
                    }
                    step.finished_at_ms = Some(now);
                }
            }
            run.status = WorkflowRunStatus::Canceled;
            run.updated_at_ms = now;
            run.finished_at_ms = Some(now);
            self.store
                .upsert_workflow_run(&run)
                .map_err(|e| format!("failed to persist canceled workflow run: {e}"))?;
            self.emit_run_event(&run);
        }

        Ok(run)
    }

    /// Select the winning candidate of a Best-of-N Arena run, optionally applying its worktree
    /// changes onto the main workspace via Orbit's backup-protected handoff engine and archiving
    /// losing candidate agents/worktrees.
    pub async fn select_best_of_n_winner(
        &self,
        run_id: &str,
        winner_step_id: &str,
        apply_to_workspace: bool,
    ) -> Result<WorkflowRunRecord, String> {
        let _winner_guard = WinnerApplyGuard::try_acquire(&self.applying_winners, run_id)?;
        let target_step_id = winner_step_id.trim();

        // Pre-validate against current persisted/live snapshot before cancelling active siblings.
        {
            let current = self
                .store
                .get_workflow_run(run_id)
                .map_err(|e| format!("failed to load workflow run `{run_id}`: {e}"))?
                .ok_or_else(|| format!("workflow_not_found: workflow run `{run_id}` not found"))?;

            if current.kind != WorkflowKind::BestOfN {
                return Err(
                    "invalid_workflow_kind: winner selection is only supported for Best-of-N Arena runs"
                        .into(),
                );
            }
            if current.winner_step_id.is_some() {
                return Err(format!(
                    "winner_already_selected: workflow `{run_id}` already has winner `{}`",
                    current.winner_step_id.as_deref().unwrap_or("")
                ));
            }
            let Some(cand) = current.steps.iter().find(|s| s.step_id == target_step_id) else {
                return Err(format!(
                    "step_not_found: candidate `{target_step_id}` not found in workflow `{run_id}`"
                ));
            };
            if cand.status != WorkflowStepStatus::Succeeded {
                return Err(format!(
                    "candidate_not_succeeded: candidate `{target_step_id}` must be Succeeded before applying (status: {:?})",
                    cand.status
                ));
            }
        }

        // Coordinate with any still-running sibling candidates: cancel AND await driver exit so
        // `drive_workflow_run` can NEVER overwrite `winner_step_id` or spawn late worktrees.
        let maybe_finished_rx = {
            let guard = self.active_runs.lock().await;
            guard.get(run_id).map(|h| {
                let _ = h.cancel_tx.send(true);
                h.finished_rx.clone()
            })
        };
        if let Some(mut finished_rx) = maybe_finished_rx {
            let wait_res = tokio::time::timeout(Duration::from_secs(30), async {
                if *finished_rx.borrow() {
                    return;
                }
                while finished_rx.changed().await.is_ok() {
                    if *finished_rx.borrow() {
                        break;
                    }
                }
            })
            .await;
            if wait_res.is_err() {
                return Err(format!(
                    "workflow_still_stopping: timed out waiting for active sibling steps of `{run_id}` to stop"
                ));
            }
        }

        // Reload fresh post-termination snapshot from SQLite.
        let mut run = self
            .store
            .get_workflow_run(run_id)
            .map_err(|e| format!("failed to reload workflow run `{run_id}`: {e}"))?
            .ok_or_else(|| format!("workflow_not_found: workflow run `{run_id}` not found"))?;

        if run.winner_step_id.is_some() {
            return Err(format!(
                "winner_already_selected: workflow `{run_id}` already has winner `{}`",
                run.winner_step_id.as_deref().unwrap_or("")
            ));
        }

        let Some(winner_step) = run
            .steps
            .iter()
            .find(|s| s.step_id == target_step_id)
            .cloned()
        else {
            return Err(format!(
                "step_not_found: candidate `{target_step_id}` not found in workflow `{run_id}`"
            ));
        };

        if winner_step.status != WorkflowStepStatus::Succeeded {
            return Err(format!(
                "candidate_not_succeeded: candidate `{target_step_id}` is {:?}, expected Succeeded",
                winner_step.status
            ));
        }

        if apply_to_workspace {
            let authorized_target = self.authorize_cwd(&run.workspace_cwd).await?;
            let Some(ref winner_wt) = winner_step.worktree_cwd else {
                return Err(format!(
                    "winner_missing_worktree: candidate `{target_step_id}` does not have an isolated worktree"
                ));
            };
            let source_cwd = PathBuf::from(winner_wt);
            if !tokio::fs::try_exists(&source_cwd).await.unwrap_or(false) {
                return Err(format!(
                    "winner_worktree_missing: candidate worktree `{}` no longer exists",
                    source_cwd.display()
                ));
            }
            let src_for_canon = source_cwd.clone();
            let canon_source =
                tokio::task::spawn_blocking(move || canonicalize_workspace_root(&src_for_canon))
                    .await
                    .map_err(|_| "worktree canonicalization task cancelled".to_string())??;
            if canon_source == authorized_target {
                return Err(
                    "winner_same_as_workspace: candidate worktree must be distinct from the target workspace"
                        .into(),
                );
            }

            // Verify candidate's worktree workspace exists and is unarchived when registered in store.
            if let Some(ref winner_aid) = winner_step.agent_id
                && let Ok(Some(ag)) = self.store.get_agent(winner_aid)
            {
                let ws = self
                    .store
                    .get_workspace(&ag.workspace_id)
                    .map_err(|e| format!("failed to inspect winner workspace: {e}"))?
                    .ok_or_else(|| {
                        "winner_workspace_missing: winner worktree workspace not found".to_string()
                    })?;
                if !ws.is_worktree || ws.archived_at.is_some() {
                    return Err(
                        "winner_workspace_invalid: winner candidate worktree has already been archived"
                            .into(),
                    );
                }
            } else if self.config.enforce_registered_cwd {
                return Err(
                    "winner_workspace_untracked: winner candidate is not backed by an active worktree workspace"
                        .into(),
                );
            }

            let outcome = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
                source_cwd: canon_source,
                target_cwd: authorized_target,
                direction: WorktreeHandoffDirection::WorktreeToLocal,
                mode: WorktreeHandoffMode::Apply,
                include_committed: true,
                base_ref: None,
                allow_three_way_conflicts: false,
                target_workspace: None,
                backup_source_as_primary: false,
            })
            .await
            .map_err(|e| format!("apply_winner_failed: {e}"))?;

            info!(
                run_id = %run.run_id,
                winner = %target_step_id,
                applied_files = outcome.applied_files,
                backup_ref = ?outcome.backup_ref,
                "Applied Best-of-N winning candidate changes to workspace"
            );

            // Soft-archive losing candidate agents & their dedicated worktrees
            for step in &run.steps {
                if step.step_id != target_step_id
                    && let Some(ref loser_agent_id) = step.agent_id
                {
                    self.executor
                        .archive_agent_and_worktree(loser_agent_id.clone())
                        .await;
                }
            }
        }

        let now = now_unix_ms();
        run.winner_step_id = Some(target_step_id.to_string());
        for step in &mut run.steps {
            step.is_winner = step.step_id == target_step_id;
        }
        run.updated_at_ms = now;
        self.store
            .upsert_workflow_run(&run)
            .map_err(|e| format!("failed to persist Best-of-N winner: {e}"))?;
        self.emit_run_event(&run);
        Ok(run)
    }

    async fn launch_run(
        self: &Arc<Self>,
        run: WorkflowRunRecord,
        exec_steps: Vec<ExecutableStepSpec>,
        authorized_cwd: PathBuf,
    ) -> Result<WorkflowRunRecord, String> {
        let run_id = run.run_id.clone();
        let owner_agent_id = run.owner_agent_id.clone();

        // Atomically check global & per-owner active workflow run caps under `active_runs` lock
        // BEFORE persisting to SQLite or spawning the driver task.
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (finished_tx, finished_rx) = watch::channel(false);
        let run_state = Arc::new(Mutex::new(run.clone()));
        let active_pgids = Arc::new(std::sync::Mutex::new(Vec::new()));
        let step_aborts = Arc::new(Mutex::new(Vec::new()));

        {
            let mut active = self.active_runs.lock().await;
            if active.len() >= MAX_ACTIVE_WORKFLOW_RUNS {
                return Err(format!(
                    "workflow_cap_exceeded: maximum of {MAX_ACTIVE_WORKFLOW_RUNS} active workflow runs reached"
                ));
            }
            if let Some(ref owner) = owner_agent_id {
                let owner_count = active
                    .values()
                    .filter(|h| h.owner_agent_id.as_deref() == Some(owner.as_str()))
                    .count();
                if owner_count >= MAX_ACTIVE_WORKFLOW_RUNS_PER_OWNER {
                    return Err(format!(
                        "workflow_owner_cap_exceeded: agent `{owner}` already has {MAX_ACTIVE_WORKFLOW_RUNS_PER_OWNER} active workflow runs"
                    ));
                }
            }

            if let Err(e) = self.store.upsert_workflow_run(&run) {
                return Err(format!("failed to persist workflow run: {e}"));
            }
            self.emit_run_event(&run);

            let engine = Arc::clone(self);
            let rs_task = Arc::clone(&run_state);
            let pgids_task = Arc::clone(&active_pgids);
            let aborts_task = Arc::clone(&step_aborts);
            let join_handle = tokio::spawn(async move {
                engine
                    .drive_workflow_run(
                        rs_task,
                        exec_steps,
                        authorized_cwd,
                        cancel_rx,
                        finished_tx,
                        pgids_task,
                        aborts_task,
                    )
                    .await;
            });

            active.insert(
                run_id,
                ActiveRunHandle {
                    owner_agent_id,
                    cancel_tx,
                    finished_rx,
                    run_state,
                    active_pgids,
                    step_aborts,
                    driver_abort: Some(join_handle.abort_handle()),
                },
            );
        }

        Ok(run)
    }

    #[allow(clippy::too_many_arguments)]
    async fn drive_workflow_run(
        self: Arc<Self>,
        run_state: Arc<Mutex<WorkflowRunRecord>>,
        exec_steps: Vec<ExecutableStepSpec>,
        authorized_cwd: PathBuf,
        mut cancel_rx: watch::Receiver<bool>,
        finished_tx: watch::Sender<bool>,
        active_pgids: Arc<std::sync::Mutex<Vec<u32>>>,
        step_aborts: Arc<Mutex<Vec<AbortHandle>>>,
    ) {
        let (run_id, concurrency) = {
            let g = run_state.lock().await;
            (
                g.run_id.clone(),
                g.max_concurrency.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY) as usize,
            )
        };
        let _keep_awake_guard =
            WorkflowKeepAwakeGuard::acquire(Arc::clone(&self.keep_awake), &run_id);

        let semaphore = Arc::new(Semaphore::new(concurrency));

        let step_specs: HashMap<String, ExecutableStepSpec> = exec_steps
            .iter()
            .map(|s| (s.step_id.clone(), s.clone()))
            .collect();
        let step_order: Vec<String> = exec_steps.iter().map(|s| s.step_id.clone()).collect();
        let mut retries_used: HashMap<String, u32> = HashMap::new();
        let mut current_attempt: HashMap<String, u32> = HashMap::new();
        let mut step_outputs: HashMap<String, String> = HashMap::new();
        let mut dispatched: HashSet<String> = HashSet::new();

        enum StepLoopMsg {
            Progress {
                step_id: String,
                attempt: u32,
                update: StepProgressUpdate,
            },
            Completed {
                step_id: String,
                attempt: u32,
                outcome: StepExecutionOutcome,
            },
        }

        let (done_tx, mut done_rx) = mpsc::channel::<StepLoopMsg>(256);

        loop {
            let is_canceled = *cancel_rx.borrow();

            // 1. Mark downstream steps Skipped when any upstream dependency failed/canceled/skipped.
            // 2. Collect ready Pending steps (all deps Succeeded).
            let mut to_dispatch: Vec<(ExecutableStepSpec, u32)> = Vec::new();
            let mut any_in_flight = false;
            let mut changed = false;

            {
                let mut run = run_state.lock().await;
                let now = now_unix_ms();

                if is_canceled || run.status == WorkflowRunStatus::Canceled {
                    for step in &mut run.steps {
                        if step.status == WorkflowStepStatus::Pending {
                            step.status = WorkflowStepStatus::Canceled;
                            step.error = Some("Canceled by user".into());
                            step.finished_at_ms = Some(now);
                            changed = true;
                        }
                    }
                } else {
                    // Fixed-point skip propagation for DAG dependencies
                    loop {
                        let status_by_id: HashMap<String, WorkflowStepStatus> = run
                            .steps
                            .iter()
                            .map(|s| (s.step_id.clone(), s.status))
                            .collect();
                        let mut skipped_any = false;
                        for step in &mut run.steps {
                            if step.status != WorkflowStepStatus::Pending {
                                continue;
                            }
                            if let Some(bad_dep) = step.depends_on.iter().find(|dep| {
                                matches!(
                                    status_by_id.get(dep.as_str()),
                                    Some(
                                        WorkflowStepStatus::Failed
                                            | WorkflowStepStatus::Canceled
                                            | WorkflowStepStatus::Skipped
                                    )
                                )
                            }) {
                                step.status = WorkflowStepStatus::Skipped;
                                step.error = Some(format!(
                                    "Skipped because upstream dependency `{bad_dep}` did not succeed"
                                ));
                                step.finished_at_ms = Some(now);
                                skipped_any = true;
                                changed = true;
                            }
                        }
                        if !skipped_any {
                            break;
                        }
                    }

                    let status_by_id: HashMap<String, WorkflowStepStatus> = run
                        .steps
                        .iter()
                        .map(|s| (s.step_id.clone(), s.status))
                        .collect();

                    for sid in &step_order {
                        if dispatched.contains(sid) {
                            continue;
                        }
                        if status_by_id.get(sid) != Some(&WorkflowStepStatus::Pending) {
                            continue;
                        }
                        let Some(spec) = step_specs.get(sid) else {
                            continue;
                        };
                        let deps_ok = spec.depends_on.iter().all(|dep| {
                            status_by_id.get(dep) == Some(&WorkflowStepStatus::Succeeded)
                        });
                        if deps_ok {
                            dispatched.insert(sid.clone());
                            let attempt = retries_used.get(sid).copied().unwrap_or(0) + 1;
                            current_attempt.insert(sid.clone(), attempt);
                            to_dispatch.push((spec.clone(), attempt));
                        }
                    }
                }

                for step in &run.steps {
                    if matches!(
                        step.status,
                        WorkflowStepStatus::Running | WorkflowStepStatus::Verifying
                    ) || (step.status == WorkflowStepStatus::Pending
                        && dispatched.contains(&step.step_id))
                    {
                        any_in_flight = true;
                    }
                }

                if changed {
                    run.updated_at_ms = now;
                    let _ = self.store.upsert_workflow_run(&run);
                    self.emit_run_event(&run);
                }
            }

            // 3. Spawn newly ready steps bounded by the run semaphore.
            for (spec, attempt) in to_dispatch {
                any_in_flight = true;
                let sem = Arc::clone(&semaphore);
                let engine_bg = Arc::clone(&self);
                let run_state_bg = Arc::clone(&run_state);
                let cancel_bg = cancel_rx.clone();
                let pgids_bg = Arc::clone(&active_pgids);
                let done_tx_bg = done_tx.clone();
                let cwd_bg = authorized_cwd.clone();
                let prompt = if spec.is_dag_template {
                    interpolate_dag_template(&spec.prompt_template, &step_outputs)
                } else {
                    spec.prompt_template.clone()
                };

                let step_handle = tokio::spawn(async move {
                    let permit = tokio::select! {
                        p = sem.acquire_owned() => match p {
                            Ok(p) => p,
                            Err(_) => return,
                        },
                        _ = wait_until_canceled(cancel_bg.clone()) => {
                            let _ = done_tx_bg.send(StepLoopMsg::Completed {
                                step_id: spec.step_id,
                                attempt,
                                outcome: StepExecutionOutcome {
                                    canceled: true,
                                    error: Some("Canceled while queued".into()),
                                    ..Default::default()
                                },
                            }).await;
                            return;
                        }
                    };

                    if *cancel_bg.borrow() {
                        drop(permit);
                        let _ = done_tx_bg
                            .send(StepLoopMsg::Completed {
                                step_id: spec.step_id,
                                attempt,
                                outcome: StepExecutionOutcome {
                                    canceled: true,
                                    error: Some("Canceled before execution".into()),
                                    ..Default::default()
                                },
                            })
                            .await;
                        return;
                    }

                    // Mark step Running now that we hold a concurrency permit (guarding against
                    // concurrent `cancel_run` marking the shared `run_state` Canceled).
                    let (run_id_val, run_name, owner_agent_id, kind, verify_command) = {
                        let mut run = run_state_bg.lock().await;
                        if run.status == WorkflowRunStatus::Canceled || *cancel_bg.borrow() {
                            drop(run);
                            drop(permit);
                            let _ = done_tx_bg
                                .send(StepLoopMsg::Completed {
                                    step_id: spec.step_id,
                                    attempt,
                                    outcome: StepExecutionOutcome {
                                        canceled: true,
                                        error: Some("Canceled before execution".into()),
                                        ..Default::default()
                                    },
                                })
                                .await;
                            return;
                        }
                        let now = now_unix_ms();
                        if let Some(st) = run.steps.iter_mut().find(|s| s.step_id == spec.step_id) {
                            st.status = WorkflowStepStatus::Running;
                            st.started_at_ms = Some(now);
                            st.finished_at_ms = None;
                            st.error = None;
                        }
                        run.updated_at_ms = now;
                        let _ = engine_bg.store.upsert_workflow_run(&run);
                        engine_bg.emit_run_event(&run);
                        (
                            run.run_id.clone(),
                            run.name.clone(),
                            run.owner_agent_id.clone(),
                            run.kind,
                            run.verify_command.clone(),
                        )
                    };

                    // Route progress updates through the ordered coordinator `done_tx` mpsc channel
                    // (zero detached unordered `tokio::spawn` tasks!).
                    let step_id_prog = spec.step_id.clone();
                    let tx_prog = done_tx_bg.clone();
                    let on_progress: StepProgressFn = Arc::new(move |update| {
                        let _ = tx_prog.try_send(StepLoopMsg::Progress {
                            step_id: step_id_prog.clone(),
                            attempt,
                            update,
                        });
                    });

                    let exec_req = StepExecutionRequest {
                        run_id: run_id_val,
                        run_name,
                        owner_agent_id,
                        kind,
                        step_id: spec.step_id.clone(),
                        step_name: spec.name.clone(),
                        attempt,
                        workspace_cwd: cwd_bg,
                        prompt,
                        provider: spec.provider,
                        model: spec.model.clone(),
                        effort: spec.effort,
                        mode: spec.mode.clone(),
                        isolated_worktree: spec.isolated_worktree,
                        timeout: spec.timeout,
                        verify_command,
                    };

                    let outcome = engine_bg
                        .executor
                        .execute_step(exec_req, cancel_bg, pgids_bg, on_progress)
                        .await;

                    drop(permit);
                    let _ = done_tx_bg
                        .send(StepLoopMsg::Completed {
                            step_id: spec.step_id,
                            attempt,
                            outcome,
                        })
                        .await;
                });
                step_aborts.lock().await.push(step_handle.abort_handle());
            }

            if !any_in_flight {
                break;
            }

            // 4. Wait for ordered progress/completion messages or a cancellation trigger.
            tokio::select! {
                msg = done_rx.recv() => {
                    let Some(loop_msg) = msg else {
                        break;
                    };

                    match loop_msg {
                        StepLoopMsg::Progress { step_id, attempt, update } => {
                            if current_attempt.get(&step_id).copied() != Some(attempt) {
                                continue;
                            }
                            let mut run = run_state.lock().await;
                            let is_run_canceled = run.status == WorkflowRunStatus::Canceled;
                            let now = now_unix_ms();
                            if let Some(st) = run.steps.iter_mut().find(|s| s.step_id == step_id) {
                                match update {
                                    StepProgressUpdate::AgentSpawned {
                                        agent_id,
                                        worktree_cwd,
                                    } => {
                                        st.agent_id = Some(agent_id);
                                        if worktree_cwd.is_some() {
                                            st.worktree_cwd = worktree_cwd;
                                        }
                                    }
                                    StepProgressUpdate::Verifying => {
                                        if st.status == WorkflowStepStatus::Running
                                            && !is_run_canceled
                                        {
                                            st.status = WorkflowStepStatus::Verifying;
                                        }
                                    }
                                }
                                run.updated_at_ms = now;
                                let _ = self.store.upsert_workflow_run(&run);
                                self.emit_run_event(&run);
                            }
                        }
                        StepLoopMsg::Completed { step_id, attempt, outcome } => {
                            if current_attempt.get(&step_id).copied() != Some(attempt) {
                                continue;
                            }

                            let can_retry = !outcome.succeeded
                                && !outcome.canceled
                                && !*cancel_rx.borrow()
                                && step_specs.get(&step_id).is_some_and(|s| {
                                    let used = retries_used.get(&step_id).copied().unwrap_or(0);
                                    used < s.retry_count
                                });

                            if can_retry {
                                // Clean up the failed attempt's agent & isolated worktree BEFORE re-queueing.
                                if let Some(failed_agent_id) = outcome.agent_id.clone() {
                                    self.executor.archive_agent_and_worktree(failed_agent_id).await;
                                }
                                let used = retries_used.entry(step_id.clone()).or_insert(0);
                                *used += 1;
                                dispatched.remove(&step_id);
                                let mut run = run_state.lock().await;
                                if run.status == WorkflowRunStatus::Canceled || *cancel_rx.borrow() {
                                    continue;
                                }
                                let now = now_unix_ms();
                                if let Some(st) = run.steps.iter_mut().find(|s| s.step_id == step_id) {
                                    st.status = WorkflowStepStatus::Pending;
                                    st.agent_id = None;
                                    st.worktree_cwd = None;
                                    st.error = Some(format!(
                                        "Attempt {} failed ({}); retrying…",
                                        *used,
                                        outcome.error.as_deref().unwrap_or("unknown error")
                                    ));
                                }
                                run.updated_at_ms = now;
                                let _ = self.store.upsert_workflow_run(&run);
                                self.emit_run_event(&run);
                                continue;
                            }

                            if outcome.succeeded {
                                step_outputs.insert(
                                    step_id.clone(),
                                    outcome.output_preview.clone().unwrap_or_default(),
                                );
                            }

                            let mut run = run_state.lock().await;
                            let now = now_unix_ms();
                            if let Some(st) = run.steps.iter_mut().find(|s| s.step_id == step_id) {
                                if outcome.agent_id.is_some() {
                                    st.agent_id = outcome.agent_id;
                                }
                                if outcome.worktree_cwd.is_some() {
                                    st.worktree_cwd = outcome.worktree_cwd;
                                }
                                if outcome.output_preview.is_some() {
                                    st.output_preview = outcome.output_preview;
                                }
                                st.diff_additions = outcome.diff_additions;
                                st.diff_deletions = outcome.diff_deletions;
                                st.diff_files = outcome.diff_files;
                                st.verify_passed = outcome.verify_passed;
                                st.verify_output = outcome.verify_output;
                                st.error = outcome.error;
                                st.finished_at_ms = Some(now);
                                st.status = if outcome.canceled {
                                    WorkflowStepStatus::Canceled
                                } else if outcome.succeeded {
                                    WorkflowStepStatus::Succeeded
                                } else {
                                    WorkflowStepStatus::Failed
                                };
                            }
                            run.updated_at_ms = now;
                            let _ = self.store.upsert_workflow_run(&run);
                            self.emit_run_event(&run);
                        }
                    }
                }
                _ = cancel_rx.changed() => {
                    // Loop re-evaluates `is_canceled` and marks pending steps canceled;
                    // running tasks observe `cancel_rx` and report completion promptly.
                }
            }
        }

        // Finalize aggregate run status.
        {
            let mut run = run_state.lock().await;
            let now = now_unix_ms();
            let was_canceled = *cancel_rx.borrow() || run.status == WorkflowRunStatus::Canceled;
            if was_canceled {
                for st in &mut run.steps {
                    if !st.status.is_terminal() {
                        st.status = WorkflowStepStatus::Canceled;
                        st.finished_at_ms = Some(now);
                    }
                }
                run.status = WorkflowRunStatus::Canceled;
            } else {
                let total = run.steps.len();
                let succeeded = run
                    .steps
                    .iter()
                    .filter(|s| s.status == WorkflowStepStatus::Succeeded)
                    .count();
                let any_canceled = run
                    .steps
                    .iter()
                    .any(|s| s.status == WorkflowStepStatus::Canceled);
                run.status = if total > 0 && succeeded == total {
                    WorkflowRunStatus::Succeeded
                } else if succeeded > 0 {
                    WorkflowRunStatus::PartialFailure
                } else if any_canceled {
                    WorkflowRunStatus::Canceled
                } else {
                    WorkflowRunStatus::Failed
                };
            }
            run.updated_at_ms = now;
            if run.finished_at_ms.is_none() {
                run.finished_at_ms = Some(now);
            }
            if let Err(e) = self.store.upsert_workflow_run(&run) {
                warn!(run_id = %run_id, error = %e, "Failed to persist final workflow run state");
            }
            self.emit_run_event(&run);
        }

        self.active_runs.lock().await.remove(&run_id);
        let _ = finished_tx.send(true);
    }

    async fn authorize_cwd(&self, raw_cwd: &str) -> Result<PathBuf, String> {
        let trimmed = raw_cwd.trim();
        if trimmed.is_empty() {
            return Err("invalid_cwd: workspace cwd must not be empty".into());
        }
        let expanded = crate::expand_host_path(trimmed);
        if self.config.enforce_registered_cwd {
            crate::authorize_workspace_mutation_cwd(&self.store, &expanded).await
        } else {
            tokio::task::spawn_blocking(move || canonicalize_workspace_root(&expanded))
                .await
                .map_err(|_| "workspace canonicalization task cancelled".to_string())?
        }
    }

    fn emit_run_event(&self, run: &WorkflowRunRecord) {
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkflowRunEvent(run.clone()));
    }
}

async fn wait_until_canceled(mut rx: watch::Receiver<bool>) {
    if *rx.borrow() {
        return;
    }
    while rx.changed().await.is_ok() {
        if *rx.borrow() {
            return;
        }
    }
    std::future::pending::<()>().await;
}

// ==================== GIT DIFF STATS HELPER ====================

async fn resolve_git_head_oid(cwd: &Path) -> Option<String> {
    let out = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("git")
            .current_dir(cwd)
            .args(["rev-parse", "--verify", "HEAD"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.len() >= 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(sha)
    } else {
        None
    }
}

/// Compute `(additions, deletions, changed_files)` for a step's working directory relative to its
/// initial `base_head` (captures both committed and uncommitted tracked changes + up to 200 untracked files).
async fn compute_git_diff_stats(cwd: &Path, base_head: Option<&str>) -> (u32, u32, u32) {
    let target_ref = base_head.unwrap_or("HEAD");
    let mut additions: u32 = 0;
    let mut deletions: u32 = 0;
    let mut files: u32 = 0;

    if let Ok(Ok(out)) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new("git")
            .current_dir(cwd)
            .args([
                "-c",
                "core.quotePath=false",
                "diff",
                "--numstat",
                "--no-color",
                "--no-ext-diff",
                target_ref,
                "--",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
        && out.status.success()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let mut parts = line.split('\t');
            let Some(add_s) = parts.next() else {
                continue;
            };
            let Some(del_s) = parts.next() else {
                continue;
            };
            if parts.next().is_none() {
                continue;
            }
            files = files.saturating_add(1);
            if let Ok(a) = add_s.parse::<u32>() {
                additions = additions.saturating_add(a);
            }
            if let Ok(d) = del_s.parse::<u32>() {
                deletions = deletions.saturating_add(d);
            }
        }
    }

    // Count untracked non-ignored files (capped at MAX_UNTRACKED_DIFF_FILES to bound I/O).
    if let Ok(Ok(out)) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new("git")
            .current_dir(cwd)
            .args(["ls-files", "--others", "--exclude-standard", "-z"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
        && out.status.success()
    {
        for raw_path in out
            .stdout
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .take(MAX_UNTRACKED_DIFF_FILES)
        {
            let rel = String::from_utf8_lossy(raw_path);
            files = files.saturating_add(1);
            let full = cwd.join(rel.as_ref());
            if let Ok(meta) = tokio::fs::symlink_metadata(&full).await
                && meta.is_file()
                && meta.len() <= 256 * 1024
                && let Ok(bytes) = tokio::fs::read(&full).await
                && !bytes.contains(&0)
            {
                let lines = bytes.iter().filter(|&&b| b == b'\n').count() as u32
                    + u32::from(!bytes.is_empty() && !bytes.ends_with(b"\n"));
                additions = additions.saturating_add(lines);
            }
        }
    }

    (additions, deletions, files)
}

// ==================== SCRUBBED VERIFY COMMAND RUNNER ====================

struct VerifyOutcome {
    exit_code: i32,
    output_tail: String,
    canceled: bool,
}

async fn run_workflow_verify_command(
    authorized_cwd: &Path,
    command_str: &str,
    timeout: Duration,
    cancel_rx: watch::Receiver<bool>,
    active_pgids: Arc<std::sync::Mutex<Vec<u32>>>,
) -> VerifyOutcome {
    if *cancel_rx.borrow() {
        return VerifyOutcome {
            exit_code: -1,
            output_tail: String::new(),
            canceled: true,
        };
    }

    #[cfg(unix)]
    let mut cmd = {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.arg("-c").arg(command_str);
        c
    };
    #[cfg(not(unix))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(command_str);
        c
    };

    cmd.current_dir(authorized_cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_clear();

    for (k, v) in std::env::vars_os() {
        let key_lossy = k.to_string_lossy();
        if !is_dangerous_env_var(&key_lossy) {
            cmd.env(k, v);
        }
    }

    #[cfg(unix)]
    {
        cmd.process_group(0);
    }

    // Re-check cancel immediately before spawn to close the env-setup window.
    if *cancel_rx.borrow() {
        return VerifyOutcome {
            exit_code: -1,
            output_tail: String::new(),
            canceled: true,
        };
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => {
            return VerifyOutcome {
                exit_code: -1,
                output_tail: format!("failed to spawn verification command: {err}"),
                canceled: false,
            };
        }
    };

    let pid = child.id();
    if let Some(p) = pid
        && let Ok(mut g) = active_pgids.lock()
    {
        g.push(p);
    }

    let tail = Arc::new(Mutex::new(TailBuf::new(MAX_VERIFY_OUTPUT_BYTES)));
    let stdout_task = child.stdout.take().map(|out| {
        let b = Arc::clone(&tail);
        tokio::spawn(async move {
            drain_tail(out, b).await;
        })
    });
    let stderr_task = child.stderr.take().map(|err_io| {
        let b = Arc::clone(&tail);
        tokio::spawn(async move {
            drain_tail(err_io, b).await;
        })
    });

    let mut timed_out = false;
    let mut canceled = false;
    let exit_code = tokio::select! {
        res = tokio::time::timeout(timeout, child.wait()) => {
            match res {
                Ok(Ok(status)) => status.code().unwrap_or(-1),
                Ok(Err(_)) => -1,
                Err(_) => {
                    timed_out = true;
                    terminate_process_group(pid).await;
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    -1
                }
            }
        }
        _ = wait_until_canceled(cancel_rx) => {
            canceled = true;
            terminate_process_group(pid).await;
            let _ = child.kill().await;
            let _ = child.wait().await;
            -1
        }
    };

    // Always reap any leftover grandchildren inside this isolated PGID and unregister PID.
    terminate_process_group(pid).await;
    if let Some(p) = pid
        && let Ok(mut g) = active_pgids.lock()
    {
        g.retain(|&x| x != p);
    }

    for task in [stdout_task, stderr_task].into_iter().flatten() {
        let abort = task.abort_handle();
        if tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .is_err()
        {
            abort.abort();
        }
    }

    let mut output_tail = tail.lock().await.to_utf8_string();
    if timed_out {
        let notice = format!(
            "Verification command timed out after {}s.",
            timeout.as_secs()
        );
        if output_tail.is_empty() {
            output_tail = notice;
        } else {
            output_tail.push_str(&format!("\n{notice}"));
        }
    }

    VerifyOutcome {
        exit_code,
        output_tail,
        canceled,
    }
}

struct TailBuf {
    cap: usize,
    bytes: VecDeque<u8>,
}

impl TailBuf {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            bytes: VecDeque::with_capacity(cap.min(4096)),
        }
    }

    fn push_slice(&mut self, data: &[u8]) {
        if self.cap == 0 {
            return;
        }
        if data.len() >= self.cap {
            self.bytes.clear();
            self.bytes.extend(&data[data.len() - self.cap..]);
            return;
        }
        let overflow = (self.bytes.len() + data.len()).saturating_sub(self.cap);
        for _ in 0..overflow {
            self.bytes.pop_front();
        }
        self.bytes.extend(data);
    }

    fn to_utf8_string(&self) -> String {
        let raw: Vec<u8> = self.bytes.iter().copied().collect();
        let mut start = 0;
        while start < raw.len() && (raw[start] & 0b1100_0000) == 0b1000_0000 {
            start += 1;
        }
        String::from_utf8_lossy(&raw[start..]).into_owned()
    }
}

async fn drain_tail<R: AsyncRead + Unpin>(mut reader: R, tail: Arc<Mutex<TailBuf>>) {
    let mut buf = [0u8; 4096];
    while let Ok(n) = reader.read(&mut buf).await {
        if n == 0 {
            break;
        }
        tail.lock().await.push_slice(&buf[..n]);
    }
}

async fn terminate_process_group(pgid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pgid
        && pid > 1
    {
        let target = format!("-{pid}");
        for bin in ["/bin/kill", "/usr/bin/kill", "kill"] {
            let status = tokio::process::Command::new(bin)
                .args(["-9", &target])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            if status.is_ok() {
                break;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = pgid;
}

// ==================== UNIT TESTS ====================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn rfc4180_csv_parser_handles_quotes_commas_newlines_and_escapes() {
        let csv = "\u{feff}id,file,notes\r\n1,\"src/a,b.rs\",\"Line 1\nLine \"\"2\"\"\"\r\n2,src/c.rs,\"simple\"\n\n";
        let rows = parse_csv_records(csv).expect("parse rfc4180 csv");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("id").map(String::as_str), Some("1"));
        assert_eq!(rows[0].get("file").map(String::as_str), Some("src/a,b.rs"));
        assert_eq!(
            rows[0].get("notes").map(String::as_str),
            Some("Line 1\nLine \"2\"")
        );
        assert_eq!(rows[1].get("file").map(String::as_str), Some("src/c.rs"));
        assert_eq!(rows[1].get("notes").map(String::as_str), Some("simple"));

        // Single-pass template interpolation does not re-expand substituted values
        let mut tricky = BTreeMap::new();
        tricky.insert("a".into(), "{{b}}".into());
        tricky.insert("b".into(), "INJECTED".into());
        assert_eq!(
            interpolate_csv_template("val={{ a }} other={{unknown}}", &tricky),
            "val={{b}} other={{unknown}}"
        );
    }

    #[test]
    fn rfc4180_csv_parser_enforces_200_row_cap_and_rejects_malformed() {
        // 200 rows -> OK
        let mut ok_csv = String::from("k,v\n");
        for i in 0..MAX_BATCH_CSV_ROWS {
            ok_csv.push_str(&format!("{i},val{i}\n"));
        }
        assert_eq!(
            parse_csv_records(&ok_csv).expect("200 rows allowed").len(),
            200
        );

        // 201 rows -> Err (enforced streaming inside parse_rfc4180_rows!)
        ok_csv.push_str("200,overflow\n");
        let err = parse_csv_records(&ok_csv).expect_err("201 rows must fail");
        assert!(err.contains("csv_row_limit_exceeded"), "got: {err}");

        // Unterminated quote -> Err
        assert!(parse_csv_records("a,b\n1,\"unclosed").is_err());
        // Duplicate header -> Err
        assert!(parse_csv_records("a,a\n1,2").is_err());
        // Header only -> Err
        assert!(parse_csv_records("a,b\n").is_err());
    }

    #[test]
    fn dag_validator_detects_cycles_missing_deps_and_computes_topological_order() {
        let mk = |id: &str, deps: &[&str]| DagStepSpec {
            step_id: id.into(),
            name: id.into(),
            depends_on: deps.iter().map(|s| (*s).to_string()).collect(),
            prompt_template: format!("Run {id}"),
            provider: None,
            model: None,
            isolated_worktree: false,
            retry_count: 0,
            timeout_secs: 600,
        };

        // Valid diamond DAG: a -> {b, c} -> d
        let order = validate_dag_steps(&[
            mk("d", &["b", "c"]),
            mk("b", &["a"]),
            mk("c", &["a"]),
            mk("a", &[]),
        ])
        .expect("diamond dag valid");
        let pos = |id: &str| order.iter().position(|x| x == id).expect("id present");
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));

        // Cycle: a -> b -> c -> a
        let cycle_err = validate_dag_steps(&[mk("a", &["c"]), mk("b", &["a"]), mk("c", &["b"])])
            .expect_err("cycle must fail");
        assert!(cycle_err.contains("dag_cycle_detected"), "got: {cycle_err}");

        // Missing dependency
        let miss_err =
            validate_dag_steps(&[mk("a", &["ghost"])]).expect_err("missing dep must fail");
        assert!(
            miss_err.contains("dag_missing_dependency"),
            "got: {miss_err}"
        );

        // Template interpolation for `{{step.<id>.output}}`
        let outputs = HashMap::from([("plan".to_string(), "Step 1: edit lib.rs".to_string())]);
        let rendered = interpolate_dag_template(
            "Implement using:\n{{step.plan.output}}\nMissing: {{ step.other.output }} Keep: {{raw}}",
            &outputs,
        );
        assert_eq!(
            rendered,
            "Implement using:\nStep 1: edit lib.rs\nMissing:  Keep: {{raw}}"
        );
    }

    #[test]
    fn best_of_n_validates_candidate_bounds() {
        let mk = |label: &str| BestOfNCandidateSpec {
            label: label.into(),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
        };
        assert!(validate_best_of_n_candidates("Fix bug", &[mk("A")]).is_err());
        assert!(validate_best_of_n_candidates("Fix bug", &[mk("A"), mk("B")]).is_ok());
        assert!(
            validate_best_of_n_candidates(
                "Fix bug",
                &[mk("1"), mk("2"), mk("3"), mk("4"), mk("5")]
            )
            .is_ok()
        );
        assert!(
            validate_best_of_n_candidates(
                "Fix bug",
                &[mk("1"), mk("2"), mk("3"), mk("4"), mk("5"), mk("6")]
            )
            .is_err()
        );
    }

    #[test]
    fn mcp_privilege_clamping_and_verify_command_gating_and_cancel_ownership() {
        // Privilege clamping: never exceed caller mode
        assert_eq!(
            clamp_workflow_session_mode(&SessionMode::Plan, SessionMode::FullAccess),
            SessionMode::Plan
        );
        assert_eq!(
            clamp_workflow_session_mode(&SessionMode::Plan, SessionMode::AcceptEdits),
            SessionMode::Plan
        );
        assert_eq!(
            clamp_workflow_session_mode(&SessionMode::Default, SessionMode::AcceptEdits),
            SessionMode::Default
        );
        assert_eq!(
            clamp_workflow_session_mode(&SessionMode::Default, SessionMode::Plan),
            SessionMode::Plan
        );
        assert_eq!(
            clamp_workflow_session_mode(&SessionMode::AcceptEdits, SessionMode::FullAccess),
            SessionMode::AcceptEdits
        );
        assert_eq!(
            clamp_workflow_session_mode(&SessionMode::FullAccess, SessionMode::AcceptEdits),
            SessionMode::AcceptEdits
        );

        // verify_command blocked for Plan & Default MCP callers; allowed for AcceptEdits & UI (None)
        assert!(validate_mcp_verify_command(Some(&SessionMode::Plan), Some("cargo test")).is_err());
        assert!(
            validate_mcp_verify_command(Some(&SessionMode::Default), Some("cargo test")).is_err()
        );
        assert!(validate_mcp_verify_command(Some(&SessionMode::Plan), None).is_ok());
        assert!(
            validate_mcp_verify_command(Some(&SessionMode::AcceptEdits), Some("cargo test"))
                .is_ok()
        );
        assert!(validate_mcp_verify_command(None, Some("cargo test")).is_ok());

        // Cancel ownership: MCP callers cannot cancel user runs (`owner_agent_id: None`) or other agents' runs
        let user_run = WorkflowRunRecord {
            run_id: "wf_user".into(),
            name: "User Run".into(),
            kind: WorkflowKind::Dag,
            workspace_cwd: "/tmp/repo".into(),
            owner_agent_id: None,
            status: WorkflowRunStatus::Running,
            max_concurrency: 2,
            verify_command: None,
            winner_step_id: None,
            steps: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            finished_at_ms: None,
        };
        assert!(authorize_workflow_cancel(None, &user_run).is_ok());
        assert!(authorize_workflow_cancel(Some("ag_1"), &user_run).is_err());

        let mut owned_run = user_run;
        owned_run.owner_agent_id = Some("ag_owner".into());
        assert!(authorize_workflow_cancel(Some("ag_owner"), &owned_run).is_ok());
        assert!(authorize_workflow_cancel(Some("ag_other"), &owned_run).is_err());
        assert!(authorize_workflow_cancel(None, &owned_run).is_ok());

        // Branch naming appends `-a{attempt}` on retries
        assert_eq!(
            workflow_step_branch_name("wf_1234567890abcd", "build", 1),
            "orbit/wf-123456789-build"
        );
        assert_eq!(
            workflow_step_branch_name("wf_1234567890abcd", "build", 2),
            "orbit/wf-123456789-build-a2"
        );
    }

    #[test]
    fn anti_recursion_rejects_child_and_workflow_step_agents() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = chrono::Utc::now();
        store
            .upsert_workspace(&orbit_protocol::WorkspaceSummary {
                workspace_id: "ws_1".into(),
                title: "Repo".into(),
                cwd: "/tmp/repo".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("upsert ws");

        let mk_agent = |id: &str, parent: Option<&str>| orbit_protocol::AgentSummary {
            agent_id: id.into(),
            workspace_id: "ws_1".into(),
            cwd: "/tmp/repo".into(),
            title: id.into(),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            parent_agent_id: parent.map(ToOwned::to_owned),
            parent_host_id: None,
            latest_seq: 0,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::new(),
            attention: orbit_protocol::AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };

        store
            .upsert_agent(&mk_agent("ag_root", None), None)
            .expect("root");
        store
            .upsert_agent(&mk_agent("ag_child", Some("ag_root")), None)
            .expect("child");
        store
            .upsert_agent(&mk_agent("ag_step_worker", None), None)
            .expect("step worker");

        store
            .upsert_workflow_run(&WorkflowRunRecord {
                run_id: "wf_rec".into(),
                name: "UI Run".into(),
                kind: WorkflowKind::Dag,
                workspace_cwd: "/tmp/repo".into(),
                owner_agent_id: None,
                status: WorkflowRunStatus::Running,
                max_concurrency: 1,
                verify_command: None,
                winner_step_id: None,
                steps: vec![WorkflowStepRecord {
                    run_id: "wf_rec".into(),
                    step_id: "s1".into(),
                    name: "Step 1".into(),
                    status: WorkflowStepStatus::Running,
                    depends_on: Vec::new(),
                    agent_id: Some("ag_step_worker".into()),
                    worktree_cwd: None,
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    output_preview: None,
                    diff_additions: 0,
                    diff_deletions: 0,
                    diff_files: 0,
                    verify_passed: None,
                    verify_output: None,
                    error: None,
                    is_winner: false,
                    started_at_ms: Some(1),
                    finished_at_ms: None,
                }],
                created_at_ms: 1,
                updated_at_ms: 1,
                finished_at_ms: None,
            })
            .expect("upsert wf");

        assert!(authorize_workflow_launch_from_caller(&store, None).is_ok());
        assert!(authorize_workflow_launch_from_caller(&store, Some("ag_root")).is_ok());
        assert!(authorize_workflow_launch_from_caller(&store, Some("ag_child")).is_err());
        assert!(authorize_workflow_launch_from_caller(&store, Some("ag_step_worker")).is_err());
    }

    struct MockStepExecutor {
        in_flight: Arc<AtomicUsize>,
        peak_in_flight: Arc<AtomicUsize>,
        fail_steps: HashSet<String>,
        fail_first_attempt_steps: HashSet<String>,
        captured_prompts: Arc<std::sync::Mutex<HashMap<String, String>>>,
        captured_attempts: Arc<std::sync::Mutex<Vec<(String, u32)>>>,
        archived_agents: Arc<std::sync::Mutex<Vec<String>>>,
        delay: Duration,
    }

    impl WorkflowStepExecutor for MockStepExecutor {
        fn execute_step(
            &self,
            req: StepExecutionRequest,
            cancel_rx: watch::Receiver<bool>,
            _active_pgids: Arc<std::sync::Mutex<Vec<u32>>>,
            on_progress: StepProgressFn,
        ) -> Pin<Box<dyn Future<Output = StepExecutionOutcome> + Send>> {
            let in_flight = Arc::clone(&self.in_flight);
            let peak = Arc::clone(&self.peak_in_flight);
            let should_fail = self.fail_steps.contains(&req.step_id)
                || (req.attempt == 1 && self.fail_first_attempt_steps.contains(&req.step_id));
            let prompts = Arc::clone(&self.captured_prompts);
            let attempts = Arc::clone(&self.captured_attempts);
            let delay = self.delay;
            Box::pin(async move {
                let cur = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(cur, Ordering::SeqCst);
                let aid = format!("ag_{}_a{}", req.step_id, req.attempt);
                on_progress(StepProgressUpdate::AgentSpawned {
                    agent_id: aid.clone(),
                    worktree_cwd: None,
                });
                if let Ok(mut g) = prompts.lock() {
                    g.insert(req.step_id.clone(), req.prompt.clone());
                }
                if let Ok(mut g) = attempts.lock() {
                    g.push((req.step_id.clone(), req.attempt));
                }

                let canceled = tokio::select! {
                    _ = tokio::time::sleep(delay) => false,
                    _ = wait_until_canceled(cancel_rx) => true,
                };
                in_flight.fetch_sub(1, Ordering::SeqCst);

                if canceled {
                    return StepExecutionOutcome {
                        succeeded: false,
                        canceled: true,
                        agent_id: Some(aid),
                        error: Some("canceled".into()),
                        ..Default::default()
                    };
                }

                if should_fail {
                    StepExecutionOutcome {
                        succeeded: false,
                        agent_id: Some(aid),
                        error: Some(format!("boom in {} attempt {}", req.step_id, req.attempt)),
                        ..Default::default()
                    }
                } else {
                    StepExecutionOutcome {
                        succeeded: true,
                        agent_id: Some(aid),
                        output_preview: Some(format!("out:{}", req.step_id)),
                        diff_additions: 5,
                        diff_deletions: 1,
                        diff_files: 1,
                        ..Default::default()
                    }
                }
            })
        }

        fn archive_agent_and_worktree(
            &self,
            agent_id: String,
        ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            let archived = Arc::clone(&self.archived_agents);
            Box::pin(async move {
                if let Ok(mut g) = archived.lock() {
                    g.push(agent_id);
                }
            })
        }
    }

    #[tokio::test]
    async fn bounded_concurrency_dag_retry_cleanup_and_skip_on_failure() {
        let store = Arc::new(OrbitStore::open_in_memory().expect("open store"));
        let (bcast_tx, _) = broadcast::channel(256);
        let keep_awake = Arc::new(KeepAwakeManager::new(false));

        let tmp = std::env::temp_dir().join(format!("orbit_wf_test_{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&tmp).expect("create temp workspace");

        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak_in_flight = Arc::new(AtomicUsize::new(0));
        let captured_prompts = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let captured_attempts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let archived_agents = Arc::new(std::sync::Mutex::new(Vec::new()));

        let executor = Arc::new(MockStepExecutor {
            in_flight: Arc::clone(&in_flight),
            peak_in_flight: Arc::clone(&peak_in_flight),
            fail_steps: HashSet::from(["bad_branch".to_string()]),
            fail_first_attempt_steps: HashSet::from(["flaky".to_string()]),
            captured_prompts: Arc::clone(&captured_prompts),
            captured_attempts: Arc::clone(&captured_attempts),
            archived_agents: Arc::clone(&archived_agents),
            delay: Duration::from_millis(30),
        });

        let engine = WorkflowEngine::with_executor(
            Arc::clone(&store),
            bcast_tx,
            keep_awake,
            executor,
            WorkflowEngineConfig {
                enforce_registered_cwd: false,
                require_git_for_best_of_n: false,
            },
        );

        // 1. Batch CSV with 6 rows and max_concurrency = 2 -> peak concurrency <= 2
        let batch_run = engine
            .start_batch_fanout(StartBatchFanoutSpec {
                name: "Parallel Batch".into(),
                cwd: tmp.to_string_lossy().to_string(),
                owner_agent_id: None,
                csv_content: "item\na\nb\nc\nd\ne\nf\n".into(),
                prompt_template: "Process {{item}}".into(),
                provider: ProviderKind::ClaudeCode,
                model: None,
                mode: SessionMode::AcceptEdits,
                max_concurrency: 2,
                isolated_worktree: false,
            })
            .await
            .expect("start batch");

        let finished_batch = wait_for_run_terminal(&engine, &batch_run.run_id).await;
        assert_eq!(finished_batch.status, WorkflowRunStatus::Succeeded);
        assert_eq!(finished_batch.steps.len(), 6);
        assert!(peak_in_flight.load(Ordering::SeqCst) <= 2);
        assert!(peak_in_flight.load(Ordering::SeqCst) >= 2);

        // 2. DAG workflow: `root` -> `flaky` (fails attempt 1, archives failed agent, succeeds attempt 2)
        //    + `good_child` (interpolates {{step.root.output}})
        //    + `bad_branch` -> `skipped_child` (becomes Skipped)
        let dag_run = engine
            .start_dag_workflow(StartDagWorkflowSpec {
                name: "DAG Test".into(),
                cwd: tmp.to_string_lossy().to_string(),
                owner_agent_id: None,
                steps: vec![
                    DagStepSpec {
                        step_id: "root".into(),
                        name: "Root".into(),
                        depends_on: Vec::new(),
                        prompt_template: "Start".into(),
                        provider: None,
                        model: None,
                        isolated_worktree: false,
                        retry_count: 0,
                        timeout_secs: 60,
                    },
                    DagStepSpec {
                        step_id: "flaky".into(),
                        name: "Flaky".into(),
                        depends_on: vec!["root".into()],
                        prompt_template: "Retry me".into(),
                        provider: None,
                        model: None,
                        isolated_worktree: true,
                        retry_count: 1,
                        timeout_secs: 60,
                    },
                    DagStepSpec {
                        step_id: "good_child".into(),
                        name: "Good Child".into(),
                        depends_on: vec!["root".into()],
                        prompt_template: "Got={{step.root.output}}".into(),
                        provider: None,
                        model: None,
                        isolated_worktree: false,
                        retry_count: 0,
                        timeout_secs: 60,
                    },
                    DagStepSpec {
                        step_id: "bad_branch".into(),
                        name: "Bad Branch".into(),
                        depends_on: vec!["root".into()],
                        prompt_template: "Fail me".into(),
                        provider: None,
                        model: None,
                        isolated_worktree: false,
                        retry_count: 0,
                        timeout_secs: 60,
                    },
                    DagStepSpec {
                        step_id: "skipped_child".into(),
                        name: "Skipped Child".into(),
                        depends_on: vec!["bad_branch".into()],
                        prompt_template: "Should never run".into(),
                        provider: None,
                        model: None,
                        isolated_worktree: false,
                        retry_count: 0,
                        timeout_secs: 60,
                    },
                ],
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::AcceptEdits,
                max_concurrency: 2,
            })
            .await
            .expect("start dag");

        let finished_dag = wait_for_run_terminal(&engine, &dag_run.run_id).await;
        assert_eq!(finished_dag.status, WorkflowRunStatus::PartialFailure);
        let status_of = |id: &str| {
            finished_dag
                .steps
                .iter()
                .find(|s| s.step_id == id)
                .expect("step exists")
                .status
        };
        assert_eq!(status_of("root"), WorkflowStepStatus::Succeeded);
        assert_eq!(status_of("flaky"), WorkflowStepStatus::Succeeded);
        assert_eq!(status_of("good_child"), WorkflowStepStatus::Succeeded);
        assert_eq!(status_of("bad_branch"), WorkflowStepStatus::Failed);
        assert_eq!(status_of("skipped_child"), WorkflowStepStatus::Skipped);

        // Verify flaky step ran attempts 1 and 2, and attempt 1's agent was archived before retry!
        let flaky_attempts: Vec<u32> = captured_attempts
            .lock()
            .expect("lock attempts")
            .iter()
            .filter(|(sid, _)| sid == "flaky")
            .map(|(_, a)| *a)
            .collect();
        assert_eq!(flaky_attempts, vec![1, 2]);
        assert!(
            archived_agents
                .lock()
                .expect("lock archived")
                .contains(&"ag_flaky_a1".to_string())
        );

        let prompts_guard = captured_prompts.lock().expect("lock prompts");
        assert_eq!(
            prompts_guard.get("good_child").map(String::as_str),
            Some("Got=out:root")
        );
        assert!(!prompts_guard.contains_key("skipped_child"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn active_run_caps_enforced_globally_and_per_owner() {
        let store = Arc::new(OrbitStore::open_in_memory().expect("open store"));
        let (bcast_tx, _) = broadcast::channel(128);
        let keep_awake = Arc::new(KeepAwakeManager::new(false));
        let tmp = std::env::temp_dir().join(format!("orbit_wf_cap_{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&tmp).expect("create temp dir");

        let executor = Arc::new(MockStepExecutor {
            in_flight: Arc::new(AtomicUsize::new(0)),
            peak_in_flight: Arc::new(AtomicUsize::new(0)),
            fail_steps: HashSet::new(),
            fail_first_attempt_steps: HashSet::new(),
            captured_prompts: Arc::new(std::sync::Mutex::new(HashMap::new())),
            captured_attempts: Arc::new(std::sync::Mutex::new(Vec::new())),
            archived_agents: Arc::new(std::sync::Mutex::new(Vec::new())),
            delay: Duration::from_secs(5),
        });
        let engine = WorkflowEngine::with_executor(
            Arc::clone(&store),
            bcast_tx,
            keep_awake,
            executor,
            WorkflowEngineConfig {
                enforce_registered_cwd: false,
                require_git_for_best_of_n: false,
            },
        );

        // Per-owner cap (MAX_ACTIVE_WORKFLOW_RUNS_PER_OWNER = 2)
        for i in 0..MAX_ACTIVE_WORKFLOW_RUNS_PER_OWNER {
            engine
                .start_batch_fanout(StartBatchFanoutSpec {
                    name: format!("Owner Run {i}"),
                    cwd: tmp.to_string_lossy().to_string(),
                    owner_agent_id: Some("ag_owner_1".into()),
                    csv_content: "x\n1\n".into(),
                    prompt_template: "{{x}}".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    mode: SessionMode::AcceptEdits,
                    max_concurrency: 1,
                    isolated_worktree: false,
                })
                .await
                .expect("within per-owner cap");
        }
        let owner_err = engine
            .start_batch_fanout(StartBatchFanoutSpec {
                name: "Owner Overflow".into(),
                cwd: tmp.to_string_lossy().to_string(),
                owner_agent_id: Some("ag_owner_1".into()),
                csv_content: "x\n1\n".into(),
                prompt_template: "{{x}}".into(),
                provider: ProviderKind::ClaudeCode,
                model: None,
                mode: SessionMode::AcceptEdits,
                max_concurrency: 1,
                isolated_worktree: false,
            })
            .await
            .expect_err("3rd active run for same owner must fail");
        assert!(owner_err.contains("workflow_owner_cap_exceeded"));

        // Fill remaining global slots up to MAX_ACTIVE_WORKFLOW_RUNS (8)
        for i in MAX_ACTIVE_WORKFLOW_RUNS_PER_OWNER..MAX_ACTIVE_WORKFLOW_RUNS {
            engine
                .start_batch_fanout(StartBatchFanoutSpec {
                    name: format!("Global Run {i}"),
                    cwd: tmp.to_string_lossy().to_string(),
                    owner_agent_id: None,
                    csv_content: "x\n1\n".into(),
                    prompt_template: "{{x}}".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    mode: SessionMode::AcceptEdits,
                    max_concurrency: 1,
                    isolated_worktree: false,
                })
                .await
                .expect("within global cap");
        }
        let global_err = engine
            .start_batch_fanout(StartBatchFanoutSpec {
                name: "Global Overflow".into(),
                cwd: tmp.to_string_lossy().to_string(),
                owner_agent_id: None,
                csv_content: "x\n1\n".into(),
                prompt_template: "{{x}}".into(),
                provider: ProviderKind::ClaudeCode,
                model: None,
                mode: SessionMode::AcceptEdits,
                max_concurrency: 1,
                isolated_worktree: false,
            })
            .await
            .expect_err("9th active run must fail");
        assert!(global_err.contains("workflow_cap_exceeded"));

        engine.shutdown_all().await;
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn select_best_of_n_winner_rejects_double_apply_and_non_succeeded_and_preserves_winner_on_cancel()
     {
        let store = Arc::new(OrbitStore::open_in_memory().expect("open store"));
        let (bcast_tx, _) = broadcast::channel(128);
        let keep_awake = Arc::new(KeepAwakeManager::new(false));
        let tmp = std::env::temp_dir().join(format!("orbit_wf_winner_{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&tmp).expect("create temp dir");

        let executor = Arc::new(MockStepExecutor {
            in_flight: Arc::new(AtomicUsize::new(0)),
            peak_in_flight: Arc::new(AtomicUsize::new(0)),
            fail_steps: HashSet::from(["cand_2".to_string()]),
            fail_first_attempt_steps: HashSet::new(),
            captured_prompts: Arc::new(std::sync::Mutex::new(HashMap::new())),
            captured_attempts: Arc::new(std::sync::Mutex::new(Vec::new())),
            archived_agents: Arc::new(std::sync::Mutex::new(Vec::new())),
            delay: Duration::from_millis(20),
        });
        let engine = WorkflowEngine::with_executor(
            Arc::clone(&store),
            bcast_tx,
            keep_awake,
            executor,
            WorkflowEngineConfig {
                enforce_registered_cwd: false,
                require_git_for_best_of_n: false,
            },
        );

        let run = engine
            .start_best_of_n(StartBestOfNSpec {
                name: "Arena Double-Apply Guard".into(),
                cwd: tmp.to_string_lossy().to_string(),
                owner_agent_id: None,
                prompt: "Implement feature".into(),
                candidates: vec![
                    BestOfNCandidateSpec {
                        label: "A".into(),
                        provider: ProviderKind::ClaudeCode,
                        model: None,
                        effort: None,
                    },
                    BestOfNCandidateSpec {
                        label: "B".into(),
                        provider: ProviderKind::Codex,
                        model: None,
                        effort: None,
                    },
                ],
                mode: SessionMode::AcceptEdits,
                verify_command: None,
            })
            .await
            .expect("start arena");

        let finished = wait_for_run_terminal(&engine, &run.run_id).await;
        assert_eq!(finished.status, WorkflowRunStatus::PartialFailure);

        // Selecting a Failed candidate must be rejected
        let fail_err = engine
            .select_best_of_n_winner(&run.run_id, "cand_2", false)
            .await
            .expect_err("failed candidate must not be selectable");
        assert!(fail_err.contains("candidate_not_succeeded"));

        // Selecting the Succeeded candidate succeeds once
        let selected = engine
            .select_best_of_n_winner(&run.run_id, "cand_1", false)
            .await
            .expect("select winner");
        assert_eq!(selected.winner_step_id.as_deref(), Some("cand_1"));
        assert!(selected.steps[0].is_winner);

        // Second winner selection / double-apply MUST be rejected
        let dup_err = engine
            .select_best_of_n_winner(&run.run_id, "cand_1", false)
            .await
            .expect_err("double winner selection must fail");
        assert!(dup_err.contains("winner_already_selected"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn cancel_run_never_regresses_back_to_running() {
        let store = Arc::new(OrbitStore::open_in_memory().expect("open store"));
        let (bcast_tx, mut bcast_rx) = broadcast::channel(128);
        let keep_awake = Arc::new(KeepAwakeManager::new(false));
        let tmp = std::env::temp_dir().join(format!("orbit_wf_cancel_{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&tmp).expect("create temp dir");

        let executor = Arc::new(MockStepExecutor {
            in_flight: Arc::new(AtomicUsize::new(0)),
            peak_in_flight: Arc::new(AtomicUsize::new(0)),
            fail_steps: HashSet::new(),
            fail_first_attempt_steps: HashSet::new(),
            captured_prompts: Arc::new(std::sync::Mutex::new(HashMap::new())),
            captured_attempts: Arc::new(std::sync::Mutex::new(Vec::new())),
            archived_agents: Arc::new(std::sync::Mutex::new(Vec::new())),
            delay: Duration::from_secs(5),
        });
        let engine = WorkflowEngine::with_executor(
            Arc::clone(&store),
            bcast_tx,
            keep_awake,
            executor,
            WorkflowEngineConfig {
                enforce_registered_cwd: false,
                require_git_for_best_of_n: false,
            },
        );

        let run = engine
            .start_batch_fanout(StartBatchFanoutSpec {
                name: "Cancelable Batch".into(),
                cwd: tmp.to_string_lossy().to_string(),
                owner_agent_id: None,
                csv_content: "x\n1\n2\n3\n4\n".into(),
                prompt_template: "{{x}}".into(),
                provider: ProviderKind::ClaudeCode,
                model: None,
                mode: SessionMode::AcceptEdits,
                max_concurrency: 2,
                isolated_worktree: false,
            })
            .await
            .expect("start batch");

        tokio::time::sleep(Duration::from_millis(30)).await;
        let canceled = engine.cancel_run(&run.run_id).await.expect("cancel run");
        assert_eq!(canceled.status, WorkflowRunStatus::Canceled);

        // Give background driver a moment to settle and verify status NEVER regressed to Running
        tokio::time::sleep(Duration::from_millis(60)).await;
        let mut saw_canceled = false;
        while let Ok(ev) = bcast_rx.try_recv() {
            if let DaemonBroadcast::WorkflowRunEvent(r) = ev
                && r.run_id == run.run_id
            {
                if saw_canceled {
                    assert_eq!(
                        r.status,
                        WorkflowRunStatus::Canceled,
                        "workflow run status regressed after Canceled"
                    );
                }
                if r.status == WorkflowRunStatus::Canceled {
                    saw_canceled = true;
                }
            }
        }
        assert!(saw_canceled);
        let stored = engine.get_run(&run.run_id).expect("load").expect("exists");
        assert_eq!(stored.status, WorkflowRunStatus::Canceled);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    async fn wait_for_run_terminal(engine: &WorkflowEngine, run_id: &str) -> WorkflowRunRecord {
        for _ in 0..100 {
            if let Ok(Some(run)) = engine.get_run(run_id)
                && run.status.is_terminal()
            {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("workflow run `{run_id}` did not reach terminal state in time");
    }
}
