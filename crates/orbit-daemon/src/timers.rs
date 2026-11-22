//! Early-Wakeup One-Shot Conditional Timer Engine (`ConditionalTimerEngine`).
//!
//! Persists armed one-shot timers in SQLite (`conditional_timers`), schedules Tokio wakeups,
//! enforces strict workspace containment & symlink rejection for `TimerCondition::FileChanged`,
//! offloads blocking filesystem/SQLite calls off async Tokio workers, supports startup recovery
//! (firing overdue timers within a grace window), and atomically cancels timers early when
//! matching messages, subagent settlements, or watched file changes occur so agents are never
//! double-woken.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageKind, ConditionalTimerRecord, ConditionalTimerState,
    MAX_CONDITIONAL_TIMER_PROMPT_BYTES, MAX_CONDITIONAL_TIMER_SECONDS,
    MIN_CONDITIONAL_TIMER_SECONDS, TimerCondition,
};
use orbit_store::OrbitStore;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tracing::{info, warn};
use uuid::Uuid;

use crate::manager::{AgentOrchestrator, DaemonBroadcast};
use crate::messaging::{AgentMessageDelivery, AgentPromptSource, QueuedAgentMessage};
use crate::workspace_files::{canonicalize_workspace_root, is_path_inside_git};

/// Maximum overdue age on daemon startup that still fires immediately instead of expiring.
pub const OVERDUE_GRACE_WINDOW_MS: u64 = 15 * 60 * 1000;

/// Polling cadence for `TimerCondition::FileChanged` while a timer is armed.
const DEFAULT_FILE_POLL_INTERVAL: Duration = Duration::from_secs(2);

type WakeupDispatchFn = Arc<
    dyn Fn(
            ConditionalTimerRecord,
            String,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    exists: bool,
    len: u64,
    modified_nanos: Option<u128>,
}

/// Inspect a watched file strictly without following symlinks at any segment under `canonical_root`.
///
/// Returns `Err` if any segment along `rel_path` has been swapped to a symlink (blocking TOCTOU
/// symlink swaps on every poll) or references `.git`.
fn capture_file_fingerprint_strict(
    canonical_root: &Path,
    rel_path: &str,
) -> Result<FileFingerprint, String> {
    let mut cursor = canonical_root.to_path_buf();
    let segments: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Err("watched path is empty".to_string());
    }

    for (idx, seg) in segments.iter().enumerate() {
        if *seg == "." || *seg == ".." || seg.eq_ignore_ascii_case(".git") {
            return Err(format!("invalid watched path segment `{seg}`"));
        }
        cursor.push(seg);
        let is_last = idx + 1 == segments.len();
        match std::fs::symlink_metadata(&cursor) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(format!(
                        "refusing to watch symlink at `{}`",
                        cursor.display()
                    ));
                }
                if is_last {
                    let modified_nanos = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos());
                    return Ok(FileFingerprint {
                        exists: true,
                        len: meta.len(),
                        modified_nanos,
                    });
                }
                if !meta.is_dir() {
                    return Ok(FileFingerprint {
                        exists: false,
                        len: 0,
                        modified_nanos: None,
                    });
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(FileFingerprint {
                    exists: false,
                    len: 0,
                    modified_nanos: None,
                });
            }
            Err(err) => {
                return Err(format!("failed to stat `{}`: {err}", cursor.display()));
            }
        }
    }

    Ok(FileFingerprint {
        exists: false,
        len: 0,
        modified_nanos: None,
    })
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// Validate and resolve a `TimerCondition::FileChanged { path }` against an agent's workspace `cwd`.
///
/// Enforces:
/// - `canonicalize_workspace_root` (rejects `/`, `$HOME`, non-directories, and `.git` roots)
/// - Rejects NUL/control characters, backslashes, `..` parent traversal, and `.git` components
/// - Walks every segment with `std::fs::symlink_metadata`, refusing symlinks (including broken
///   and escaping symlinks) anywhere along the path
/// - Returns `(normalized_relative_path, canonical_workspace_root)`.
pub fn validate_workspace_file_path(
    workspace_cwd: &str,
    raw_path: &str,
) -> anyhow::Result<(String, PathBuf)> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("file watch path must not be empty");
    }
    if trimmed.chars().any(|c| c == '\0' || c.is_control()) {
        anyhow::bail!("file watch path contains invalid control characters");
    }
    if trimmed.contains('\\') {
        anyhow::bail!("file watch path must use forward slashes (`/`)");
    }
    if trimmed == "~" || trimmed.starts_with("~/") {
        anyhow::bail!("file watch path `{trimmed}` must be relative to the agent workspace");
    }

    let root = canonicalize_workspace_root(Path::new(workspace_cwd))
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let raw = Path::new(trimmed);
    if is_path_inside_git(&root, raw) {
        anyhow::bail!("watching `.git` internals is forbidden");
    }

    // Extract strictly forward-only relative segments (supporting in-workspace absolute paths).
    let rel_path: PathBuf = if raw.is_absolute() {
        if raw.components().any(|c| matches!(c, Component::ParentDir)) {
            anyhow::bail!("file watch path `{trimmed}` must not contain parent traversal (`..`)");
        }
        if let Ok(stripped) = raw.strip_prefix(&root) {
            stripped.to_path_buf()
        } else if let Ok(stripped) = raw.strip_prefix(Path::new(workspace_cwd)) {
            stripped.to_path_buf()
        } else {
            anyhow::bail!(
                "file watch path `{trimmed}` escapes agent workspace `{}`",
                root.display()
            );
        }
    } else {
        raw.to_path_buf()
    };

    let mut segments: Vec<String> = Vec::new();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(os) => {
                let Some(seg) = os.to_str() else {
                    anyhow::bail!("file watch path contains non-UTF-8 characters");
                };
                if seg.is_empty() || seg == "." || seg == ".." {
                    anyhow::bail!("invalid path segment `{seg}`");
                }
                if seg.eq_ignore_ascii_case(".git") {
                    anyhow::bail!("watching `.git` internals is forbidden");
                }
                segments.push(seg.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                anyhow::bail!(
                    "file watch path `{trimmed}` must not contain parent traversal (`..`)"
                );
            }
            Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("file watch path `{trimmed}` escapes agent workspace");
            }
        }
    }

    if segments.is_empty() {
        anyhow::bail!(
            "file watch path must point to a file inside the workspace, not the workspace root"
        );
    }

    // Walk every segment with `symlink_metadata` so broken, escaping, or in-tree symlinks are
    // caught even when the target does not exist.
    let mut cursor = root.clone();
    for seg in &segments {
        cursor.push(seg);
        if is_path_inside_git(&root, &cursor) {
            anyhow::bail!("watching `.git` internals is forbidden");
        }
        match std::fs::symlink_metadata(&cursor) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    anyhow::bail!(
                        "refusing to watch symlink `{}` inside workspace",
                        segments.join("/")
                    );
                }
                if let Ok(canon) = std::fs::canonicalize(&cursor) {
                    if !canon.starts_with(&root) {
                        anyhow::bail!(
                            "file watch path `{trimmed}` escapes agent workspace `{}`",
                            root.display()
                        );
                    }
                    if is_path_inside_git(&root, &canon) {
                        anyhow::bail!("watching `.git` internals is forbidden");
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Non-existent suffix; no symlink can exist beneath a non-existent segment.
                break;
            }
            Err(err) => {
                anyhow::bail!("unable to inspect file watch path `{trimmed}`: {err}");
            }
        }
    }

    let normalized_rel = segments.join("/");
    Ok((normalized_rel, root))
}

pub struct ConditionalTimerEngine {
    store: Arc<OrbitStore>,
    orchestrator: Arc<AgentOrchestrator>,
    dispatcher: WakeupDispatchFn,
    armed_tasks: Mutex<HashMap<String, AbortHandle>>,
    file_poll_interval: Duration,
}

impl ConditionalTimerEngine {
    pub fn new(
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        delivery: Arc<AgentMessageDelivery>,
    ) -> Arc<Self> {
        let dispatcher: WakeupDispatchFn = Arc::new(move |timer, body| {
            let delivery = Arc::clone(&delivery);
            Box::pin(async move {
                let (_, ack_rx) = delivery
                    .enqueue(QueuedAgentMessage {
                        recipient_id: timer.agent_id.clone(),
                        message_id: format!("tmr_{}", timer.timer_id),
                        source: AgentPromptSource {
                            kind: AgentMessageKind::Heartbeat,
                            source_agent_id: None,
                            source_host_id: None,
                            source_title: Some("Conditional Timer".to_string()),
                            event: Some("conditional_timer".to_string()),
                            permission_context: None,
                        },
                        body,
                        active_turn_behavior: ActiveTurnBehavior::Queue,
                        session_mode: None,
                        unarchive: false,
                    })
                    .await?;
                match ack_rx.await {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(err)) => Err(anyhow::anyhow!("timer wakeup delivery failed: {err}")),
                    Err(_) => Err(anyhow::anyhow!("timer wakeup delivery canceled")),
                }
            })
        });

        Arc::new(Self {
            store,
            orchestrator,
            dispatcher,
            armed_tasks: Mutex::new(HashMap::new()),
            file_poll_interval: DEFAULT_FILE_POLL_INTERVAL,
        })
    }

    #[cfg(test)]
    fn with_custom_dispatcher(
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        dispatcher: WakeupDispatchFn,
        file_poll_interval: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            orchestrator,
            dispatcher,
            armed_tasks: Mutex::new(HashMap::new()),
            file_poll_interval,
        })
    }

    /// Reload all persisted `Armed` timers on daemon startup, immediately firing overdue timers
    /// within [`OVERDUE_GRACE_WINDOW_MS`] and rescheduling future timers.
    pub async fn start_recovery(self: &Arc<Self>) {
        let store_for_load = Arc::clone(&self.store);
        let Ok(Ok(armed)) =
            tokio::task::spawn_blocking(move || store_for_load.list_armed_conditional_timers(None))
                .await
        else {
            return;
        };

        let now_ms = now_unix_ms();
        for timer in armed {
            let store_ag = Arc::clone(&self.store);
            let aid = timer.agent_id.clone();
            let agent_res = tokio::task::spawn_blocking(move || store_ag.get_agent(&aid)).await;

            match agent_res {
                Ok(Ok(Some(ag))) if ag.archived_at.is_none() => {
                    if now_ms >= timer.fire_at_ms {
                        let overdue_ms = now_ms.saturating_sub(timer.fire_at_ms);
                        if overdue_ms <= OVERDUE_GRACE_WINDOW_MS {
                            let this = Arc::clone(self);
                            let tid = timer.timer_id.clone();
                            tokio::spawn(async move {
                                let _ = this
                                    .fire_timer_internal(
                                        &tid,
                                        "Fired on daemon restart (within overdue grace window)",
                                        false,
                                    )
                                    .await;
                            });
                        } else {
                            let _ = self
                                .cancel_timer_internal(
                                    &timer.timer_id,
                                    ConditionalTimerState::CanceledEarly,
                                    "Expired while daemon was offline",
                                )
                                .await;
                        }
                    } else {
                        let watch_root = match &timer.condition {
                            TimerCondition::FileChanged { path } => {
                                let cwd_owned = ag.cwd.clone();
                                let path_owned = path.clone();
                                match tokio::task::spawn_blocking(move || {
                                    validate_workspace_file_path(&cwd_owned, &path_owned)
                                })
                                .await
                                {
                                    Ok(Ok((_, root))) => Some(root),
                                    Ok(Err(err)) => {
                                        warn!(
                                            timer_id = %timer.timer_id,
                                            path = %path,
                                            error = %err,
                                            "Canceling recovered FileChanged timer because watched path is no longer valid"
                                        );
                                        let _ = self
                                            .cancel_timer_internal(
                                                &timer.timer_id,
                                                ConditionalTimerState::CanceledEarly,
                                                "Watched workspace path is no longer valid",
                                            )
                                            .await;
                                        continue;
                                    }
                                    Err(err) => {
                                        warn!(
                                            timer_id = %timer.timer_id,
                                            error = %err,
                                            "FileChanged recovery validation task failed; canceling timer"
                                        );
                                        let _ = self
                                            .cancel_timer_internal(
                                                &timer.timer_id,
                                                ConditionalTimerState::CanceledEarly,
                                                "Watched workspace path is no longer valid",
                                            )
                                            .await;
                                        continue;
                                    }
                                }
                            }
                            _ => None,
                        };
                        self.spawn_timer_task(timer, watch_root).await;
                    }
                }
                _ => {
                    let _ = self
                        .cancel_timer_internal(
                            &timer.timer_id,
                            ConditionalTimerState::CanceledEarly,
                            "Target agent archived or deleted",
                        )
                        .await;
                }
            }
        }
    }

    /// Validate, persist, and arm a new one-shot conditional timer.
    pub async fn create_timer(
        self: &Arc<Self>,
        agent_id: &str,
        duration_seconds: u64,
        prompt: String,
        condition: TimerCondition,
    ) -> anyhow::Result<ConditionalTimerRecord> {
        if !(MIN_CONDITIONAL_TIMER_SECONDS..=MAX_CONDITIONAL_TIMER_SECONDS)
            .contains(&duration_seconds)
        {
            anyhow::bail!(
                "duration_seconds must be between {MIN_CONDITIONAL_TIMER_SECONDS} and {MAX_CONDITIONAL_TIMER_SECONDS}"
            );
        }
        let trimmed_prompt = prompt.trim();
        if trimmed_prompt.is_empty() {
            anyhow::bail!("timer wakeup prompt must not be empty");
        }
        if prompt.len() > MAX_CONDITIONAL_TIMER_PROMPT_BYTES {
            anyhow::bail!(
                "timer wakeup prompt exceeds maximum limit of {} bytes (8 KiB)",
                MAX_CONDITIONAL_TIMER_PROMPT_BYTES
            );
        }

        let store_ag = Arc::clone(&self.store);
        let aid_owned = agent_id.to_string();
        let agent = tokio::task::spawn_blocking(move || store_ag.get_agent(&aid_owned))
            .await
            .map_err(|e| anyhow::anyhow!("agent lookup task failed: {e}"))??
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
        if agent.archived_at.is_some() {
            anyhow::bail!("cannot arm conditional timer on archived agent `{agent_id}`");
        }

        let (normalized_condition, watch_root) = match condition {
            TimerCondition::Sender { sender_id } => {
                let clean = sender_id.trim().to_string();
                if clean.is_empty() {
                    anyhow::bail!("TimerCondition::Sender requires a non-empty sender_id");
                }
                (TimerCondition::Sender { sender_id: clean }, None)
            }
            TimerCondition::FileChanged { path } => {
                let cwd_owned = agent.cwd.clone();
                let (rel, root) = tokio::task::spawn_blocking(move || {
                    validate_workspace_file_path(&cwd_owned, &path)
                })
                .await
                .map_err(|e| anyhow::anyhow!("path validation task failed: {e}"))??;
                (TimerCondition::FileChanged { path: rel }, Some(root))
            }
            other => (other, None),
        };

        let created_at_ms = now_unix_ms();
        let fire_at_ms = created_at_ms.saturating_add(duration_seconds.saturating_mul(1000));
        let timer = ConditionalTimerRecord {
            timer_id: format!("tmr_{}", &Uuid::new_v4().simple().to_string()[..10]),
            agent_id: agent.agent_id,
            workspace_id: agent.workspace_id,
            prompt: trimmed_prompt.to_string(),
            duration_seconds,
            condition: normalized_condition,
            state: ConditionalTimerState::Armed,
            created_at_ms,
            fire_at_ms,
            resolved_at_ms: None,
            resolved_reason: None,
        };

        let store_ins = Arc::clone(&self.store);
        let timer_ins = timer.clone();
        tokio::task::spawn_blocking(move || store_ins.insert_conditional_timer(&timer_ins))
            .await
            .map_err(|e| anyhow::anyhow!("timer insert task failed: {e}"))??;

        self.spawn_timer_task(timer.clone(), watch_root).await;
        let _ = self
            .orchestrator
            .broadcast_tx
            .send(DaemonBroadcast::ConditionalTimerEvent(timer.clone()));

        info!(
            timer_id = %timer.timer_id,
            agent_id = %timer.agent_id,
            duration_seconds = timer.duration_seconds,
            condition = %timer.condition.to_shorthand(),
            "Armed one-shot conditional timer"
        );
        Ok(timer)
    }

    /// List conditional timers (optionally filtered by `agent_id`).
    pub fn list_timers(
        &self,
        agent_id: Option<&str>,
        include_resolved: bool,
    ) -> anyhow::Result<Vec<ConditionalTimerRecord>> {
        Ok(self
            .store
            .list_conditional_timers(agent_id, include_resolved)?)
    }

    /// Manually cancel an armed conditional timer.
    pub async fn cancel_timer(&self, timer_id: &str) -> anyhow::Result<ConditionalTimerRecord> {
        if let Some(updated) = self
            .cancel_timer_internal(
                timer_id,
                ConditionalTimerState::CanceledManual,
                "Canceled manually",
            )
            .await?
        {
            return Ok(updated);
        }
        let store_get = Arc::clone(&self.store);
        let tid_owned = timer_id.to_string();
        let existing =
            tokio::task::spawn_blocking(move || store_get.get_conditional_timer(&tid_owned))
                .await
                .map_err(|e| anyhow::anyhow!("timer lookup task failed: {e}"))??
                .ok_or_else(|| anyhow::anyhow!("conditional timer `{timer_id}` not found"))?;
        Ok(existing)
    }

    /// Immediately fire an armed conditional timer and abort its pending sleep task.
    pub async fn fire_timer_now(&self, timer_id: &str) -> anyhow::Result<ConditionalTimerRecord> {
        if let Some(updated) = self
            .fire_timer_internal(timer_id, "Triggered manually", true)
            .await?
        {
            return Ok(updated);
        }
        let store_get = Arc::clone(&self.store);
        let tid_owned = timer_id.to_string();
        let existing =
            tokio::task::spawn_blocking(move || store_get.get_conditional_timer(&tid_owned))
                .await
                .map_err(|e| anyhow::anyhow!("timer lookup task failed: {e}"))??
                .ok_or_else(|| anyhow::anyhow!("conditional timer `{timer_id}` not found"))?;
        anyhow::bail!(
            "conditional timer `{timer_id}` is already resolved ({})",
            existing.state.slug()
        )
    }

    /// Cancel all matching `Armed` timers when a message arrives for `agent_id`
    /// (`TimerCondition::AnyMessage` or matching `TimerCondition::Sender { sender_id }`).
    pub async fn notify_agent_message(
        &self,
        agent_id: &str,
        sender_id: Option<&str>,
    ) -> Vec<ConditionalTimerRecord> {
        let store_list = Arc::clone(&self.store);
        let aid_owned = agent_id.to_string();
        let Ok(Ok(armed)) = tokio::task::spawn_blocking(move || {
            store_list.list_armed_conditional_timers(Some(&aid_owned))
        })
        .await
        else {
            return Vec::new();
        };

        let mut canceled = Vec::new();
        for timer in armed {
            let matched = match &timer.condition {
                TimerCondition::AnyMessage => true,
                TimerCondition::Sender {
                    sender_id: expected,
                } => sender_id.is_some_and(|s| s == expected),
                _ => false,
            };
            if !matched {
                continue;
            }
            let reason = match sender_id {
                Some(s) if !s.is_empty() => format!("Canceled early by message from {s}"),
                _ => "Canceled early by incoming message".to_string(),
            };
            if let Ok(Some(updated)) = self
                .cancel_timer_internal(
                    &timer.timer_id,
                    ConditionalTimerState::CanceledEarly,
                    &reason,
                )
                .await
            {
                canceled.push(updated);
            }
        }
        canceled
    }

    /// Cancel all matching `TimerCondition::AllSubagentsSettled` armed timers for `parent_agent_id`
    /// once `all_settled` is `true`.
    pub async fn notify_subagent_settled(
        &self,
        parent_agent_id: &str,
        all_settled: bool,
    ) -> Vec<ConditionalTimerRecord> {
        if !all_settled {
            return Vec::new();
        }
        let store_list = Arc::clone(&self.store);
        let pid_owned = parent_agent_id.to_string();
        let Ok(Ok(armed)) = tokio::task::spawn_blocking(move || {
            store_list.list_armed_conditional_timers(Some(&pid_owned))
        })
        .await
        else {
            return Vec::new();
        };

        let mut canceled = Vec::new();
        for timer in armed {
            if timer.condition != TimerCondition::AllSubagentsSettled {
                continue;
            }
            if let Ok(Some(updated)) = self
                .cancel_timer_internal(
                    &timer.timer_id,
                    ConditionalTimerState::CanceledEarly,
                    "Canceled early: all subagents settled",
                )
                .await
            {
                canceled.push(updated);
            }
        }
        canceled
    }

    /// Cancel all armed timers belonging to an agent (e.g. when the agent is archived or deleted).
    pub async fn cancel_all_for_agent(&self, agent_id: &str, reason: &str) {
        let store_list = Arc::clone(&self.store);
        let aid_owned = agent_id.to_string();
        let Ok(Ok(armed)) = tokio::task::spawn_blocking(move || {
            store_list.list_armed_conditional_timers(Some(&aid_owned))
        })
        .await
        else {
            return;
        };
        for timer in armed {
            let _ = self
                .cancel_timer_internal(
                    &timer.timer_id,
                    ConditionalTimerState::CanceledEarly,
                    reason,
                )
                .await;
        }
    }

    /// Abort all active timer tasks during graceful daemon shutdown (timers remain `Armed` in SQLite
    /// for recovery on next restart).
    pub async fn shutdown(&self) {
        let mut guard = self.armed_tasks.lock().await;
        for (_, handle) in guard.drain() {
            handle.abort();
        }
    }

    async fn spawn_timer_task(
        self: &Arc<Self>,
        timer: ConditionalTimerRecord,
        watch_root: Option<PathBuf>,
    ) {
        let this = Arc::clone(self);
        let tid = timer.timer_id.clone();
        let fire_at_ms = timer.fire_at_ms;
        let condition = timer.condition.clone();
        let poll_interval = self.file_poll_interval;

        let handle = tokio::spawn(async move {
            if let (TimerCondition::FileChanged { path: rel_path }, Some(root)) =
                (&condition, watch_root)
            {
                let initial_root = root.clone();
                let initial_rel = rel_path.clone();
                let initial_fp = match tokio::task::spawn_blocking(move || {
                    capture_file_fingerprint_strict(&initial_root, &initial_rel)
                })
                .await
                {
                    Ok(Ok(fp)) => fp,
                    Ok(Err(err)) => {
                        warn!(timer_id = %tid, error = %err, "Canceling FileChanged timer on invalid initial watch target");
                        let _ = this
                            .cancel_timer_internal_self(
                                &tid,
                                ConditionalTimerState::CanceledEarly,
                                "Canceled early: watched path became invalid",
                            )
                            .await;
                        return;
                    }
                    Err(_) => return,
                };

                loop {
                    let now_ms = now_unix_ms();
                    if now_ms >= fire_at_ms {
                        break;
                    }
                    let remaining = Duration::from_millis(fire_at_ms.saturating_sub(now_ms));
                    tokio::time::sleep(remaining.min(poll_interval)).await;
                    let poll_root = root.clone();
                    let poll_rel = rel_path.clone();
                    match tokio::task::spawn_blocking(move || {
                        capture_file_fingerprint_strict(&poll_root, &poll_rel)
                    })
                    .await
                    {
                        Ok(Ok(current_fp)) => {
                            if current_fp != initial_fp {
                                let _ = this.trigger_file_changed_early(&tid, rel_path).await;
                                return;
                            }
                        }
                        Ok(Err(err)) => {
                            warn!(timer_id = %tid, error = %err, "Refusing symlink/invalid watch target during poll; canceling timer early");
                            let _ = this
                                .cancel_timer_internal_self(
                                    &tid,
                                    ConditionalTimerState::CanceledEarly,
                                    "Canceled early: watched path became invalid symlink",
                                )
                                .await;
                            return;
                        }
                        Err(_) => return,
                    }
                }
            } else {
                let now_ms = now_unix_ms();
                if fire_at_ms > now_ms {
                    tokio::time::sleep(Duration::from_millis(fire_at_ms - now_ms)).await;
                }
            }
            let _ = this.fire_timer_internal(&tid, "Timer elapsed", false).await;
        });

        let timer_id = timer.timer_id;
        {
            let mut guard = self.armed_tasks.lock().await;
            if let Some(prev) = guard.insert(timer_id.clone(), handle.abort_handle()) {
                prev.abort();
            }
        }

        // Close create_timer / concurrent cancellation race: if the timer was already canceled or
        // fired between DB insert and `armed_tasks` insertion, abort the newly spawned task now.
        let store_check = Arc::clone(&self.store);
        let tid_check = timer_id.clone();
        let still_armed = tokio::task::spawn_blocking(move || {
            store_check
                .get_conditional_timer(&tid_check)
                .ok()
                .flatten()
                .is_some_and(|rec| rec.state.is_armed())
        })
        .await
        .unwrap_or(false);
        if !still_armed && let Some(abort) = self.armed_tasks.lock().await.remove(&timer_id) {
            abort.abort();
        }
    }

    async fn cancel_timer_internal(
        &self,
        timer_id: &str,
        next_state: ConditionalTimerState,
        reason: &str,
    ) -> anyhow::Result<Option<ConditionalTimerRecord>> {
        let store_tx = Arc::clone(&self.store);
        let tid_owned = timer_id.to_string();
        let reason_owned = reason.to_string();
        let now_ms = now_unix_ms();
        let updated = tokio::task::spawn_blocking(move || {
            store_tx.transition_conditional_timer(
                &tid_owned,
                ConditionalTimerState::Armed,
                next_state,
                now_ms,
                Some(&reason_owned),
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("timer cancel task failed: {e}"))??;

        if let Some(record) = updated {
            if let Some(abort) = self.armed_tasks.lock().await.remove(timer_id) {
                abort.abort();
            }
            let _ = self
                .orchestrator
                .broadcast_tx
                .send(DaemonBroadcast::ConditionalTimerEvent(record.clone()));
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    /// Self-cancellation helper invoked from inside the timer's own task (never aborts self).
    async fn cancel_timer_internal_self(
        &self,
        timer_id: &str,
        next_state: ConditionalTimerState,
        reason: &str,
    ) -> anyhow::Result<Option<ConditionalTimerRecord>> {
        let store_tx = Arc::clone(&self.store);
        let tid_owned = timer_id.to_string();
        let reason_owned = reason.to_string();
        let now_ms = now_unix_ms();
        let updated = tokio::task::spawn_blocking(move || {
            store_tx.transition_conditional_timer(
                &tid_owned,
                ConditionalTimerState::Armed,
                next_state,
                now_ms,
                Some(&reason_owned),
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("timer cancel task failed: {e}"))??;

        if let Some(record) = updated {
            self.armed_tasks.lock().await.remove(timer_id);
            let _ = self
                .orchestrator
                .broadcast_tx
                .send(DaemonBroadcast::ConditionalTimerEvent(record.clone()));
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    async fn trigger_file_changed_early(
        &self,
        timer_id: &str,
        rel_path: &str,
    ) -> anyhow::Result<Option<ConditionalTimerRecord>> {
        let reason = format!("Canceled early: file changed ({rel_path})");
        let store_tx = Arc::clone(&self.store);
        let tid_owned = timer_id.to_string();
        let reason_owned = reason.clone();
        let now_ms = now_unix_ms();
        let updated = tokio::task::spawn_blocking(move || {
            store_tx.transition_conditional_timer(
                &tid_owned,
                ConditionalTimerState::Armed,
                ConditionalTimerState::CanceledEarly,
                now_ms,
                Some(&reason_owned),
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("timer file-change transition task failed: {e}"))??;

        let Some(record) = updated else {
            return Ok(None);
        };
        // Remove our task handle from the registry (we are running inside it, so do not abort self).
        self.armed_tasks.lock().await.remove(timer_id);
        let _ = self
            .orchestrator
            .broadcast_tx
            .send(DaemonBroadcast::ConditionalTimerEvent(record.clone()));

        let body = format!(
            "[Conditional Timer {} · Early Wakeup: file changed `{}`]\n\n{}",
            record.timer_id, rel_path, record.prompt
        );
        if let Err(err) = (self.dispatcher)(record.clone(), body).await {
            warn!(
                timer_id = %record.timer_id,
                agent_id = %record.agent_id,
                error = %err,
                "Failed to dispatch file-changed early wakeup"
            );
        }
        Ok(Some(record))
    }

    async fn fire_timer_internal(
        &self,
        timer_id: &str,
        reason: &str,
        abort_existing: bool,
    ) -> anyhow::Result<Option<ConditionalTimerRecord>> {
        let store_tx = Arc::clone(&self.store);
        let tid_owned = timer_id.to_string();
        let reason_owned = reason.to_string();
        let now_ms = now_unix_ms();
        let updated = tokio::task::spawn_blocking(move || {
            store_tx.transition_conditional_timer(
                &tid_owned,
                ConditionalTimerState::Armed,
                ConditionalTimerState::Fired,
                now_ms,
                Some(&reason_owned),
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("timer fire transition task failed: {e}"))??;

        let Some(record) = updated else {
            return Ok(None);
        };
        if let Some(abort) = self.armed_tasks.lock().await.remove(timer_id)
            && abort_existing
        {
            abort.abort();
        }
        let _ = self
            .orchestrator
            .broadcast_tx
            .send(DaemonBroadcast::ConditionalTimerEvent(record.clone()));

        let body = format!(
            "[Conditional Timer {} · {}]\n\n{}",
            record.timer_id, reason, record.prompt
        );
        if let Err(err) = (self.dispatcher)(record.clone(), body).await {
            warn!(
                timer_id = %record.timer_id,
                agent_id = %record.agent_id,
                error = %err,
                "Failed to dispatch conditional timer wakeup"
            );
        }
        Ok(Some(record))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Utc;
    use orbit_protocol::{
        AgentStatus, AgentSummary, AttentionState, OrchestrationConfig, ProviderKind, SessionMode,
        WorkspaceSetupStatus, WorkspaceSummary,
    };
    use tokio::sync::RwLock;

    use crate::keep_awake::KeepAwakeManager;
    use crate::workspace_runtime::WorkspaceScriptRegistry;

    struct TestTempDir(PathBuf);

    impl TestTempDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("orbit-timer-test-{}", Uuid::new_v4().simple()));
            std::fs::create_dir_all(&path).expect("create temp test dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn make_agent(
        agent_id: &str,
        workspace_id: &str,
        cwd: &Path,
        parent: Option<&str>,
        status: AgentStatus,
    ) -> AgentSummary {
        let now = Utc::now();
        AgentSummary {
            agent_id: agent_id.into(),
            workspace_id: workspace_id.into(),
            cwd: cwd.to_string_lossy().to_string(),
            title: agent_id.into(),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::Default,
            status,
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
            attention: AttentionState::None,
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
        }
    }

    async fn setup_test_engine_with_delay(
        temp_dir: &Path,
        workspace_dir: &Path,
        dispatch_delay: Duration,
    ) -> (
        Arc<OrbitStore>,
        Arc<AgentOrchestrator>,
        Arc<ConditionalTimerEngine>,
        Arc<Mutex<Vec<(String, String)>>>,
        Arc<AtomicUsize>,
    ) {
        let store = Arc::new(OrbitStore::open(temp_dir.join("orbit.db")).expect("open store"));
        let now = Utc::now();
        let ws = WorkspaceSummary {
            workspace_id: "ws_timer_test".into(),
            title: "Timer Test".into(),
            cwd: workspace_dir.to_string_lossy().to_string(),
            git_branch: None,
            is_worktree: false,
            updated_at: now,
            source_cwd: None,
            base_ref: None,
            parent_workspace_id: None,
            archived_at: None,
            background: false,
            setup_status: WorkspaceSetupStatus::Idle,
            mission_goal: None,
            project_root: None,
            board_revision: 0,
            title_locked: false,
            auto_clustered: false,
            is_curated: false,
        };
        store.upsert_workspace(&ws).expect("upsert ws");

        let agent = make_agent(
            "ag_timer_main",
            "ws_timer_test",
            workspace_dir,
            None,
            AgentStatus::Idle,
        );
        store.upsert_agent(&agent, None).expect("upsert agent");

        let orchestrator = AgentOrchestrator::new(
            temp_dir.to_path_buf(),
            "host_test".into(),
            "tok_test".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            crate::hooks::HooksEngine::new(temp_dir.to_path_buf()),
        )
        .await;

        let dispatches: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let dispatch_count = Arc::new(AtomicUsize::new(0));
        let dispatches_cb = Arc::clone(&dispatches);
        let count_cb = Arc::clone(&dispatch_count);
        let dispatcher: WakeupDispatchFn = Arc::new(move |timer, body| {
            let dispatches_cb = Arc::clone(&dispatches_cb);
            let count_cb = Arc::clone(&count_cb);
            Box::pin(async move {
                if !dispatch_delay.is_zero() {
                    tokio::time::sleep(dispatch_delay).await;
                }
                count_cb.fetch_add(1, Ordering::SeqCst);
                dispatches_cb.lock().await.push((timer.timer_id, body));
                Ok(())
            })
        });

        let engine = ConditionalTimerEngine::with_custom_dispatcher(
            Arc::clone(&store),
            Arc::clone(&orchestrator),
            dispatcher,
            Duration::from_millis(50),
        );
        orchestrator
            .set_conditional_timers(Arc::clone(&engine))
            .await;
        (store, orchestrator, engine, dispatches, dispatch_count)
    }

    async fn setup_test_engine(
        temp_dir: &Path,
        workspace_dir: &Path,
    ) -> (
        Arc<OrbitStore>,
        Arc<AgentOrchestrator>,
        Arc<ConditionalTimerEngine>,
        Arc<Mutex<Vec<(String, String)>>>,
        Arc<AtomicUsize>,
    ) {
        setup_test_engine_with_delay(temp_dir, workspace_dir, Duration::ZERO).await
    }

    #[tokio::test]
    async fn timer_firing_completes_async_yielding_dispatcher_without_self_abort() {
        let tmp = TestTempDir::new();
        let ws_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir");
        // Crucial P0 regression test: dispatcher yields via `tokio::time::sleep(40ms)` before completing.
        // Previously, `fire_timer_internal` self-aborted the task and killed the future on its first Poll::Pending!
        let (store, _orch, engine, dispatches, count) =
            setup_test_engine_with_delay(tmp.path(), &ws_dir, Duration::from_millis(40)).await;

        let timer = engine
            .create_timer(
                "ag_timer_main",
                1,
                "Async yielding wakeup".into(),
                TimerCondition::Never,
            )
            .await
            .expect("create timer");
        assert_eq!(timer.state, ConditionalTimerState::Armed);

        tokio::time::sleep(Duration::from_millis(1350)).await;
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "natural expiration must NOT self-abort before async dispatcher finishes"
        );
        let captured = dispatches.lock().await;
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, timer.timer_id);
        assert!(captured[0].1.contains("Async yielding wakeup"));

        let persisted = store
            .get_conditional_timer(&timer.timer_id)
            .expect("get")
            .expect("exists");
        assert_eq!(persisted.state, ConditionalTimerState::Fired);
        assert!(persisted.resolved_at_ms.is_some());
    }

    #[tokio::test]
    async fn early_cancellation_for_any_message_sender_and_subagents_prevents_double_wakeup() {
        let tmp = TestTempDir::new();
        let ws_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir");
        let (store, _orch, engine, _dispatches, count) =
            setup_test_engine(tmp.path(), &ws_dir).await;

        let t_any = engine
            .create_timer(
                "ag_timer_main",
                1,
                "Any msg wakeup".into(),
                TimerCondition::AnyMessage,
            )
            .await
            .expect("create any timer");

        let t_sender = engine
            .create_timer(
                "ag_timer_main",
                1,
                "Specific sender wakeup".into(),
                TimerCondition::Sender {
                    sender_id: "ag_worker_2".into(),
                },
            )
            .await
            .expect("create sender timer");

        let t_sub = engine
            .create_timer(
                "ag_timer_main",
                1,
                "Subagents wakeup".into(),
                TimerCondition::AllSubagentsSettled,
            )
            .await
            .expect("create subagents timer");

        let canceled_1 = engine
            .notify_agent_message("ag_timer_main", Some("ag_worker_1"))
            .await;
        assert_eq!(canceled_1.len(), 1);
        assert_eq!(canceled_1[0].timer_id, t_any.timer_id);
        assert_eq!(canceled_1[0].state, ConditionalTimerState::CanceledEarly);
        assert!(
            canceled_1[0]
                .resolved_reason
                .as_deref()
                .unwrap_or_default()
                .contains("ag_worker_1")
        );

        let canceled_partial = engine.notify_subagent_settled("ag_timer_main", false).await;
        assert!(canceled_partial.is_empty());

        let canceled_2 = engine
            .notify_agent_message("ag_timer_main", Some("ag_worker_2"))
            .await;
        assert_eq!(canceled_2.len(), 1);
        assert_eq!(canceled_2[0].timer_id, t_sender.timer_id);

        let canceled_sub = engine.notify_subagent_settled("ag_timer_main", true).await;
        assert_eq!(canceled_sub.len(), 1);
        assert_eq!(canceled_sub[0].timer_id, t_sub.timer_id);

        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert_eq!(count.load(Ordering::SeqCst), 0);

        for tid in [&t_any.timer_id, &t_sender.timer_id, &t_sub.timer_id] {
            let rec = store
                .get_conditional_timer(tid)
                .expect("get")
                .expect("exists");
            assert_eq!(rec.state, ConditionalTimerState::CanceledEarly);
        }
    }

    #[tokio::test]
    async fn admission_hook_cancels_timer_before_coalescing_window_flushes() {
        let tmp = TestTempDir::new();
        let ws_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir");
        let (store, _orch, engine, _dispatches, timer_dispatch_count) =
            setup_test_engine(tmp.path(), &ws_dir).await;

        let delivery = AgentMessageDelivery::new(
            Duration::from_secs(2),
            Arc::new(|_batch| Box::pin(async { Ok(()) })),
        );
        let engine_hook = Arc::clone(&engine);
        delivery
            .set_on_admitted(Arc::new(move |recipient_id, kind, sender_id| {
                let engine_hook = Arc::clone(&engine_hook);
                Box::pin(async move {
                    if matches!(
                        kind,
                        AgentMessageKind::DirectMessage | AgentMessageKind::FinishNotification
                    ) {
                        engine_hook
                            .notify_agent_message(&recipient_id, sender_id.as_deref())
                            .await;
                    }
                })
            }))
            .await;

        // Arm a 1-second timer, then immediately enqueue a DirectMessage into a 2-second coalescing window.
        let timer = engine
            .create_timer(
                "ag_timer_main",
                1,
                "Should be canceled on admission".into(),
                TimerCondition::AnyMessage,
            )
            .await
            .expect("arm timer");

        let _ = delivery
            .enqueue(QueuedAgentMessage {
                recipient_id: "ag_timer_main".into(),
                message_id: "m_1".into(),
                source: AgentPromptSource {
                    kind: AgentMessageKind::DirectMessage,
                    source_agent_id: Some("ag_peer".into()),
                    source_host_id: None,
                    source_title: Some("Peer".into()),
                    event: None,
                    permission_context: None,
                },
                body: "Done!".into(),
                active_turn_behavior: ActiveTurnBehavior::Queue,
                session_mode: None,
                unarchive: false,
            })
            .await
            .expect("enqueue");

        // Immediately canceled before coalescing window even ticks!
        let rec = store
            .get_conditional_timer(&timer.timer_id)
            .expect("get")
            .expect("exists");
        assert_eq!(rec.state, ConditionalTimerState::CanceledEarly);

        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert_eq!(
            timer_dispatch_count.load(Ordering::SeqCst),
            0,
            "timer must NOT fire during the 2s coalescing window"
        );
    }

    #[tokio::test]
    async fn subagent_kill_detach_and_delete_notify_all_subagents_settled() {
        let tmp = TestTempDir::new();
        let ws_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir");
        let (store, orch, engine, _dispatches, _count) =
            setup_test_engine(tmp.path(), &ws_dir).await;

        // 1. kill_agent settles last running child
        let c1 = make_agent(
            "ag_child_kill",
            "ws_timer_test",
            &ws_dir,
            Some("ag_timer_main"),
            AgentStatus::Running {
                turn_id: "t1".into(),
            },
        );
        store.upsert_agent(&c1, None).expect("upsert c1");
        let t1 = engine
            .create_timer(
                "ag_timer_main",
                30,
                "Wait for kill".into(),
                TimerCondition::AllSubagentsSettled,
            )
            .await
            .expect("arm t1");
        orch.kill_agent("ag_child_kill").await.expect("kill c1");
        assert_eq!(
            store
                .get_conditional_timer(&t1.timer_id)
                .expect("get")
                .expect("exists")
                .state,
            ConditionalTimerState::CanceledEarly
        );

        // 2. detach_agent settles parent when running child is detached
        let c2 = make_agent(
            "ag_child_detach",
            "ws_timer_test",
            &ws_dir,
            Some("ag_timer_main"),
            AgentStatus::Running {
                turn_id: "t2".into(),
            },
        );
        store.upsert_agent(&c2, None).expect("upsert c2");
        let t2 = engine
            .create_timer(
                "ag_timer_main",
                30,
                "Wait for detach".into(),
                TimerCondition::AllSubagentsSettled,
            )
            .await
            .expect("arm t2");
        orch.detach_agent("ag_child_detach")
            .await
            .expect("detach c2");
        assert_eq!(
            store
                .get_conditional_timer(&t2.timer_id)
                .expect("get")
                .expect("exists")
                .state,
            ConditionalTimerState::CanceledEarly
        );

        // 3. delete_agent settles parent when running child is deleted
        let c3 = make_agent(
            "ag_child_del",
            "ws_timer_test",
            &ws_dir,
            Some("ag_timer_main"),
            AgentStatus::Running {
                turn_id: "t3".into(),
            },
        );
        store.upsert_agent(&c3, None).expect("upsert c3");
        let t3 = engine
            .create_timer(
                "ag_timer_main",
                30,
                "Wait for delete".into(),
                TimerCondition::AllSubagentsSettled,
            )
            .await
            .expect("arm t3");
        orch.delete_agent("ag_child_del").await.expect("delete c3");
        assert_eq!(
            store
                .get_conditional_timer(&t3.timer_id)
                .expect("get")
                .expect("exists")
                .state,
            ConditionalTimerState::CanceledEarly
        );
    }

    #[tokio::test]
    async fn file_changed_rejects_traversal_home_git_broken_symlink_and_post_arm_symlink_swap() {
        let tmp = TestTempDir::new();
        let ws_dir = tmp.path().join("repo");
        let outside_file = tmp.path().join("secret.txt");
        std::fs::create_dir_all(ws_dir.join(".git")).expect("mkdir .git");
        std::fs::write(ws_dir.join(".git/HEAD"), "ref: refs/heads/main\n").expect("write HEAD");
        std::fs::write(&outside_file, "top secret").expect("write outside");

        let (store, _orch, engine, dispatches, count) =
            setup_test_engine(tmp.path(), &ws_dir).await;

        // 1. Reject `$HOME` as workspace root
        if let Some(home) = std::env::var_os("HOME") {
            let home_str = PathBuf::from(home).to_string_lossy().to_string();
            assert!(
                validate_workspace_file_path(&home_str, "foo.txt").is_err(),
                "$HOME workspace root must be rejected"
            );
        }

        // 2. Reject `.git` internals
        assert!(
            engine
                .create_timer(
                    "ag_timer_main",
                    10,
                    "Watch git".into(),
                    TimerCondition::FileChanged {
                        path: ".git/HEAD".into(),
                    },
                )
                .await
                .is_err(),
            ".git path must be rejected"
        );

        // 3. Reject relative traversal & out-of-workspace absolute path
        assert!(
            engine
                .create_timer(
                    "ag_timer_main",
                    10,
                    "Watch outside".into(),
                    TimerCondition::FileChanged {
                        path: "../secret.txt".into(),
                    },
                )
                .await
                .is_err()
        );
        assert!(
            engine
                .create_timer(
                    "ag_timer_main",
                    10,
                    "Watch abs outside".into(),
                    TimerCondition::FileChanged {
                        path: outside_file.to_string_lossy().to_string(),
                    },
                )
                .await
                .is_err()
        );

        // 4. Reject broken symlink & post-arming symlink swap (Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let broken_link = ws_dir.join("broken_link.txt");
            symlink(tmp.path().join("does_not_exist.txt"), &broken_link).expect("broken symlink");
            assert!(
                engine
                    .create_timer(
                        "ag_timer_main",
                        10,
                        "Watch broken symlink".into(),
                        TimerCondition::FileChanged {
                            path: "broken_link.txt".into(),
                        },
                    )
                    .await
                    .is_err(),
                "broken symlink must be rejected"
            );

            // Arm on a valid normal file, then swap it to a symlink pointing outside the workspace
            let swap_file = ws_dir.join("swap_target.txt");
            std::fs::write(&swap_file, "initial").expect("write swap_target");
            let swap_timer = engine
                .create_timer(
                    "ag_timer_main",
                    30,
                    "Watch swap target".into(),
                    TimerCondition::FileChanged {
                        path: "swap_target.txt".into(),
                    },
                )
                .await
                .expect("arm on normal file");

            tokio::time::sleep(Duration::from_millis(80)).await;
            std::fs::remove_file(&swap_file).expect("remove normal file");
            symlink(&outside_file, &swap_file).expect("replace with symlink");
            tokio::time::sleep(Duration::from_millis(200)).await;

            // Timer must cancel early safely and MUST NOT dispatch a wakeup
            let persisted = store
                .get_conditional_timer(&swap_timer.timer_id)
                .expect("get")
                .expect("exists");
            assert_eq!(persisted.state, ConditionalTimerState::CanceledEarly);
            assert!(
                persisted
                    .resolved_reason
                    .as_deref()
                    .unwrap_or_default()
                    .contains("symlink")
            );
            assert_eq!(count.load(Ordering::SeqCst), 0);
        }

        // 5. Valid workspace file change triggers early wakeup cleanly
        let watched = ws_dir.join("status.txt");
        std::fs::write(&watched, "building").expect("write initial");
        let timer = engine
            .create_timer(
                "ag_timer_main",
                30,
                "Inspect build output".into(),
                TimerCondition::FileChanged {
                    path: "status.txt".into(),
                },
            )
            .await
            .expect("arm valid file watch");

        tokio::time::sleep(Duration::from_millis(80)).await;
        std::fs::write(&watched, "build succeeded!").expect("mutate file");
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert_eq!(count.load(Ordering::SeqCst), 1);
        let captured = dispatches.lock().await;
        assert!(captured[0].1.contains("status.txt"));
        assert!(captured[0].1.contains("Inspect build output"));

        let persisted = store
            .get_conditional_timer(&timer.timer_id)
            .expect("get")
            .expect("exists");
        assert_eq!(persisted.state, ConditionalTimerState::CanceledEarly);
    }

    #[tokio::test]
    async fn restart_recovery_fires_overdue_reschedules_future_and_cancels_invalid_file_watch() {
        let tmp = TestTempDir::new();
        let ws_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir");
        let (store, _orch, engine, dispatches, count) =
            setup_test_engine(tmp.path(), &ws_dir).await;

        let now = now_unix_ms();
        let overdue = ConditionalTimerRecord {
            timer_id: "tmr_overdue".into(),
            agent_id: "ag_timer_main".into(),
            workspace_id: "ws_timer_test".into(),
            prompt: "Overdue wakeup".into(),
            duration_seconds: 10,
            condition: TimerCondition::Never,
            state: ConditionalTimerState::Armed,
            created_at_ms: now.saturating_sub(15_000),
            fire_at_ms: now.saturating_sub(5_000),
            resolved_at_ms: None,
            resolved_reason: None,
        };
        store
            .insert_conditional_timer(&overdue)
            .expect("insert overdue");

        let future = ConditionalTimerRecord {
            timer_id: "tmr_future".into(),
            agent_id: "ag_timer_main".into(),
            workspace_id: "ws_timer_test".into(),
            prompt: "Future wakeup".into(),
            duration_seconds: 1,
            condition: TimerCondition::Never,
            state: ConditionalTimerState::Armed,
            created_at_ms: now,
            fire_at_ms: now + 250,
            resolved_at_ms: None,
            resolved_reason: None,
        };
        store
            .insert_conditional_timer(&future)
            .expect("insert future");

        // Invalid persisted FileChanged path (e.g., `.git/HEAD`) must cancel early instead of degrading to Never!
        let bad_watch = ConditionalTimerRecord {
            timer_id: "tmr_bad_watch".into(),
            agent_id: "ag_timer_main".into(),
            workspace_id: "ws_timer_test".into(),
            prompt: "Invalid watch".into(),
            duration_seconds: 1,
            condition: TimerCondition::FileChanged {
                path: ".git/HEAD".into(),
            },
            state: ConditionalTimerState::Armed,
            created_at_ms: now,
            fire_at_ms: now + 200,
            resolved_at_ms: None,
            resolved_reason: None,
        };
        store
            .insert_conditional_timer(&bad_watch)
            .expect("insert bad watch");

        engine.start_recovery().await;
        tokio::time::sleep(Duration::from_millis(450)).await;

        assert_eq!(count.load(Ordering::SeqCst), 2);
        let captured = dispatches.lock().await;
        let ids: Vec<&str> = captured.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"tmr_overdue"));
        assert!(ids.contains(&"tmr_future"));
        assert!(!ids.contains(&"tmr_bad_watch"));

        let bad_rec = store
            .get_conditional_timer("tmr_bad_watch")
            .expect("get")
            .expect("exists");
        assert_eq!(bad_rec.state, ConditionalTimerState::CanceledEarly);
    }
}
