//! Ephemeral, zero-tool side-question (`/btw`) execution across Antigravity, Codex, and Claude Code.
//!
//! Runs a non-persistent, read-only query against the active host's provider CLI using a bounded
//! snapshot of the active session context so users can ask clarifying questions without blocking
//! an in-flight turn or polluting the primary conversation history.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use orbit_protocol::ProviderKind;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::find_provider_executable;

const MAX_CONTEXT_CHARS: usize = 12_000;
const MAX_QUESTION_CHARS: usize = 8_000;
const MAX_AGY_ARGV_BYTES: usize = 24_000;
const MAX_STDERR_BYTES: usize = 32 * 1024;
const SIDE_QUESTION_TIMEOUT_SECS: u64 = 90;

#[derive(Debug, Clone)]
pub struct SideQuestionSpec {
    pub provider: ProviderKind,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub question: String,
    pub context_summary: String,
}

/// Build a concise, bounded read-only prompt for an ephemeral side question.
pub fn build_side_question_prompt(question: &str, context_summary: &str) -> String {
    let trimmed_q = truncate_utf8_head(question.trim(), MAX_QUESTION_CHARS);
    let bounded_ctx = truncate_utf8_tail(context_summary.trim(), MAX_CONTEXT_CHARS);

    let mut out = String::from(
        "You are answering a quick, ephemeral side question (/btw) alongside an active coding session.\n\
         Rules:\n\
         - Answer directly, accurately, and concisely in Markdown.\n\
         - Rely on the session context below and general engineering knowledge.\n\
         - Do NOT modify files, run shell commands, or invoke external tools.\n",
    );

    if !bounded_ctx.is_empty() {
        out.push_str("\n<SESSION_CONTEXT>\n");
        out.push_str(&bounded_ctx);
        out.push_str("\n</SESSION_CONTEXT>\n");
    }

    out.push_str("\n<SIDE_QUESTION>\n");
    out.push_str(&trimmed_q);
    out.push_str("\n</SIDE_QUESTION>\n");
    out
}

fn truncate_utf8_head(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut kept: String = input.chars().take(max_chars).collect();
    kept.push('…');
    kept
}

fn truncate_utf8_bytes(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }
    let mut cut = max_bytes;
    while cut > 0 && !input.is_char_boundary(cut) {
        cut -= 1;
    }
    input[..cut].to_string()
}

fn truncate_utf8_tail(input: &str, max_chars: usize) -> String {
    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }
    let skip = count.saturating_sub(max_chars);
    let mut kept = String::from("…(earlier context truncated)…\n");
    kept.extend(input.chars().skip(skip));
    kept
}

/// Drain stderr concurrently in the background (retaining up to `MAX_STDERR_BYTES`) so child
/// processes with verbose stderr never deadlock on a full OS pipe buffer.
fn spawn_bounded_stderr_reader(
    stderr: Option<tokio::process::ChildStderr>,
) -> tokio::task::JoinHandle<String> {
    tokio::spawn(async move {
        let Some(mut pipe) = stderr else {
            return String::new();
        };
        let mut captured = Vec::new();
        let mut scratch = [0u8; 4096];
        loop {
            match pipe.read(&mut scratch).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let remaining = MAX_STDERR_BYTES.saturating_sub(captured.len());
                    if remaining > 0 {
                        captured.extend_from_slice(&scratch[..n.min(remaining)]);
                    }
                }
            }
        }
        String::from_utf8_lossy(&captured).trim().to_string()
    })
}

fn resolve_side_question_backend(
    requested: ProviderKind,
    requested_model: Option<String>,
) -> Result<(ProviderKind, PathBuf, Option<String>), String> {
    if matches!(
        requested,
        ProviderKind::ClaudeCode | ProviderKind::Codex | ProviderKind::Antigravity
    ) {
        let executable = find_provider_executable(requested).ok_or_else(|| {
            format!(
                "{} binary (`{}`) not found on host PATH",
                requested.display_name(),
                requested.default_binary()
            )
        })?;
        return Ok((requested, executable, requested_model));
    }

    // For ACP/Pi providers without an ephemeral zero-tool side-question CLI mode,
    // gracefully fall back to the first installed core provider with its default model.
    for fallback in [
        ProviderKind::ClaudeCode,
        ProviderKind::Codex,
        ProviderKind::Antigravity,
    ] {
        if let Some(executable) = find_provider_executable(fallback) {
            return Ok((fallback, executable, None));
        }
    }

    Err(format!(
        "Ephemeral side questions (/btw) are not yet natively supported on {} and no fallback CLI (Claude Code, Codex, or Antigravity) was found on host PATH",
        requested.display_name()
    ))
}

/// Execute a single non-persistent side question and stream incremental text deltas through `delta_tx`.
/// Returns the final accumulated Markdown answer.
pub async fn run_side_question(
    spec: SideQuestionSpec,
    delta_tx: mpsc::UnboundedSender<String>,
) -> Result<String, String> {
    if spec.question.trim().is_empty() {
        return Err("Side question cannot be empty".to_string());
    }

    let (effective_provider, executable, effective_model) =
        resolve_side_question_backend(spec.provider, spec.model)?;

    let prompt = build_side_question_prompt(&spec.question, &spec.context_summary);
    let cwd = if spec.cwd.is_dir() {
        spec.cwd
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };

    let fut = async {
        match effective_provider {
            ProviderKind::ClaudeCode => {
                run_claude_side_question(executable, cwd, effective_model, prompt, delta_tx).await
            }
            ProviderKind::Codex => {
                run_codex_side_question(executable, cwd, effective_model, prompt, delta_tx).await
            }
            ProviderKind::Antigravity => {
                run_agy_side_question(executable, cwd, effective_model, prompt, delta_tx).await
            }
            ProviderKind::Acp => Err(
                "Ephemeral side questions (/btw) are only supported on native Claude Code, Codex, and Antigravity sessions."
                    .to_string(),
            ),
            other => Err(format!(
                "Side questions are not supported on {}",
                other.display_name()
            )),
        }
    };

    tokio::time::timeout(Duration::from_secs(SIDE_QUESTION_TIMEOUT_SECS), fut)
        .await
        .map_err(|_| format!("Side question timed out after {SIDE_QUESTION_TIMEOUT_SECS}s"))?
}

/// Execute a non-persistent, read-only, zero-tool raw text prompt (for example, JSON-structured
/// AI classification or conversation summarization) using an installed provider CLI without
/// creating or polluting any conversation history.
pub async fn run_ephemeral_text_prompt(
    provider: ProviderKind,
    cwd: PathBuf,
    model: Option<String>,
    prompt: String,
    timeout_secs: u64,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("Ephemeral prompt cannot be empty".to_string());
    }
    let (effective_provider, executable, effective_model) =
        resolve_side_question_backend(provider, model)?;
    let safe_cwd = if cwd.is_dir() {
        cwd
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    let (delta_tx, mut delta_rx) = mpsc::unbounded_channel::<String>();
    // Drain deltas concurrently so the provider driver never considers the channel closed.
    let drain_task = tokio::spawn(async move { while delta_rx.recv().await.is_some() {} });

    let bounded_secs = timeout_secs.clamp(5, 180);
    let fut = async {
        match effective_provider {
            ProviderKind::ClaudeCode => {
                run_claude_side_question(executable, safe_cwd, effective_model, prompt, delta_tx)
                    .await
            }
            ProviderKind::Codex => {
                run_codex_side_question(executable, safe_cwd, effective_model, prompt, delta_tx)
                    .await
            }
            ProviderKind::Antigravity => {
                run_agy_side_question(executable, safe_cwd, effective_model, prompt, delta_tx).await
            }
            ProviderKind::Acp => {
                Err("Ephemeral text prompts are not supported on generic ACP agents.".to_string())
            }
            other => Err(format!(
                "Ephemeral text prompts are not supported on {}",
                other.display_name()
            )),
        }
    };

    let result = tokio::time::timeout(Duration::from_secs(bounded_secs), fut)
        .await
        .map_err(|_| format!("Ephemeral prompt timed out after {bounded_secs}s"))?;
    drain_task.abort();
    result
}

/// Hardened subprocess specification for Guardian Tier-2 LLM risk classification.
///
/// Security invariants:
/// - Always runs inside a fresh, isolated **empty** temp directory (NEVER the user's workspace).
/// - Prompt is delivered STRICTLY over stdin (NEVER via process `argv`).
/// - All tools, slash commands, MCP servers, session persistence, and workspace settings are disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardenedGuardianCommandSpec {
    pub provider: ProviderKind,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub isolated_cwd: PathBuf,
}

/// Returns `true` iff the provider supports hardened zero-tool, isolated-cwd, stdin-only
/// Guardian LLM classification (never passing untrusted prompt data in process `argv`).
pub fn is_hardened_guardian_provider(provider: ProviderKind) -> bool {
    matches!(provider, ProviderKind::ClaudeCode | ProviderKind::Codex)
}

/// Build the hardened subprocess command specification for Guardian Tier-2 LLM classification.
///
/// Explicitly rejects providers that only accept prompts via `argv` (such as Antigravity CLI).
pub fn build_hardened_guardian_command_spec(
    provider: ProviderKind,
    executable: &Path,
    isolated_cwd: &Path,
    model: Option<&str>,
) -> Result<HardenedGuardianCommandSpec, String> {
    let trimmed_model = model.map(str::trim).filter(|s| !s.is_empty());
    let args = match provider {
        ProviderKind::ClaudeCode => {
            let mut a = vec![
                "--print".to_string(),
                "--tools".to_string(),
                String::new(),
                "--disable-slash-commands".to_string(),
                "--setting-sources".to_string(),
                String::new(),
                "--no-session-persistence".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--verbose".to_string(),
                "--include-partial-messages".to_string(),
            ];
            if let Some(m) = trimmed_model {
                a.push("--model".to_string());
                a.push(m.to_string());
            }
            a
        }
        ProviderKind::Codex => {
            let mut a = vec![
                "exec".to_string(),
                "--ephemeral".to_string(),
                "--skip-git-repo-check".to_string(),
                "--sandbox".to_string(),
                "read-only".to_string(),
                "-c".to_string(),
                "mcp_servers={}".to_string(),
                "--json".to_string(),
            ];
            if let Some(m) = trimmed_model {
                a.push("--model".to_string());
                a.push(m.to_string());
            }
            a.push("-".to_string());
            a
        }
        other => {
            return Err(format!(
                "Provider `{}` does not support hardened stdin-only isolated Guardian classification (argv prompt delivery is disallowed)",
                other.display_name()
            ));
        }
    };

    Ok(HardenedGuardianCommandSpec {
        provider,
        executable: executable.to_path_buf(),
        args,
        isolated_cwd: isolated_cwd.to_path_buf(),
    })
}

struct IsolatedGuardianTempDir {
    path: PathBuf,
}

impl IsolatedGuardianTempDir {
    fn create() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!(
            "orbit-guardian-iso-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&dir)
            .map_err(|e| format!("Failed to create isolated Guardian temp directory: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        Ok(Self { path: dir })
    }
}

impl Drop for IsolatedGuardianTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Execute a hardened, zero-tool, isolated-temp-dir, stdin-only Guardian LLM classification turn.
///
/// Never runs inside the user's workspace, never passes prompts via `argv`, strips Orbit internal
/// secret environment variables, and enforces a strict bounded timeout with `kill_on_drop(true)`.
pub async fn run_hardened_guardian_prompt(
    provider: ProviderKind,
    model: Option<String>,
    prompt: String,
    timeout_ms: u64,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("Guardian LLM prompt cannot be empty".to_string());
    }
    if !is_hardened_guardian_provider(provider) {
        return Err(format!(
            "Provider `{}` does not support hardened stdin-only isolated Guardian classification",
            provider.display_name()
        ));
    }
    if crate::is_provider_overridden(provider) {
        return Err(format!(
            "Provider `{}` is overridden in test mode and unavailable for live Guardian LLM calls",
            provider.display_name()
        ));
    }
    let executable = find_provider_executable(provider).ok_or_else(|| {
        format!(
            "{} binary (`{}`) not found on host PATH",
            provider.display_name(),
            provider.default_binary()
        )
    })?;

    let iso_dir = IsolatedGuardianTempDir::create()?;
    let spec = build_hardened_guardian_command_spec(
        provider,
        &executable,
        &iso_dir.path,
        model.as_deref(),
    )?;
    let bounded_ms = timeout_ms.clamp(1_000, 20_000);

    let fut = run_hardened_guardian_spec(spec, prompt);
    tokio::time::timeout(Duration::from_millis(bounded_ms), fut)
        .await
        .map_err(|_| format!("Hardened Guardian LLM prompt timed out after {bounded_ms}ms"))?
}

async fn run_hardened_guardian_spec(
    spec: HardenedGuardianCommandSpec,
    prompt: String,
) -> Result<String, String> {
    let mut cmd = Command::new(&spec.executable);
    cmd.args(&spec.args)
        .current_dir(&spec.isolated_cwd)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::shell_snapshot::apply_cached_shell_env(&mut cmd);
    // Strip Orbit internal credentials from the isolated classifier child process.
    for key in [
        "ORBIT_MCP_TOKEN",
        "ORBIT_PAIRING_SECRET",
        "ORBIT_RELAY_TOKEN",
        "ORBIT_HOOK_ORIGIN_TOKEN",
    ] {
        cmd.env_remove(key);
    }

    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "Failed to spawn hardened {} Guardian classifier: {e}",
            spec.provider.display_name()
        )
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Guardian classifier stdout pipe unavailable".to_string())?;
    let stderr_task = spawn_bounded_stderr_reader(child.stderr.take());

    let mut reader = BufReader::new(stdout).lines();
    let mut accumulated = String::new();
    let mut streamed_any = false;
    let mut fallback_final: Option<String> = None;

    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed = match spec.provider {
            ProviderKind::ClaudeCode => parse_claude_side_line(trimmed),
            ProviderKind::Codex => parse_codex_side_line(trimmed),
            _ => ParsedSideEvent::Ignore,
        };
        match parsed {
            ParsedSideEvent::Delta(chunk) => {
                if !chunk.is_empty() {
                    streamed_any = true;
                    accumulated.push_str(&chunk);
                }
            }
            ParsedSideEvent::Snapshot(full) => {
                if !full.is_empty() {
                    if spec.provider == ProviderKind::Codex && !streamed_any {
                        if let Some(suffix) = full.strip_prefix(&accumulated) {
                            accumulated.push_str(suffix);
                        } else if accumulated.is_empty() {
                            accumulated = full;
                        } else {
                            fallback_final = Some(full);
                        }
                    } else {
                        fallback_final = Some(full);
                    }
                }
            }
            ParsedSideEvent::Error(msg) => {
                let _ = child.kill().await;
                return Err(msg);
            }
            ParsedSideEvent::PlainFallback(_) | ParsedSideEvent::Ignore => {}
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed awaiting hardened Guardian classifier: {e}"))?;
    let err_text = stderr_task.await.unwrap_or_default();

    if (accumulated.trim().is_empty()
        || (!streamed_any && spec.provider == ProviderKind::ClaudeCode))
        && let Some(final_text) = fallback_final
    {
        accumulated = final_text;
    }

    if !status.success() {
        return Err(if err_text.is_empty() {
            format!(
                "Hardened {} Guardian classifier exited with status {status}",
                spec.provider.display_name()
            )
        } else {
            err_text
        });
    }

    Ok(accumulated)
}

async fn run_claude_side_question(
    executable: PathBuf,
    cwd: PathBuf,
    model: Option<String>,
    prompt: String,
    delta_tx: mpsc::UnboundedSender<String>,
) -> Result<String, String> {
    let mut cmd = Command::new(executable);
    cmd.args([
        "-p",
        "--no-session-persistence",
        "--tools",
        "",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
    ]);
    if let Some(m) = model.as_deref().filter(|s| !s.trim().is_empty()) {
        cmd.args(["--model", m]);
    }
    cmd.current_dir(cwd)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::shell_snapshot::apply_cached_shell_env(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn Claude side question process: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Claude stdout pipe unavailable".to_string())?;
    let stderr_task = spawn_bounded_stderr_reader(child.stderr.take());

    let mut reader = BufReader::new(stdout).lines();
    let mut accumulated = String::new();
    let mut streamed_any = false;
    let mut fallback_final: Option<String> = None;

    while let Ok(Some(line)) = reader.next_line().await {
        if delta_tx.is_closed() {
            let _ = child.kill().await;
            return Err("Side question cancelled".to_string());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match parse_claude_side_line(trimmed) {
            ParsedSideEvent::Delta(chunk) => {
                if !chunk.is_empty() {
                    streamed_any = true;
                    accumulated.push_str(&chunk);
                    if delta_tx.send(chunk).is_err() {
                        let _ = child.kill().await;
                        return Err("Side question cancelled".to_string());
                    }
                }
            }
            ParsedSideEvent::Snapshot(full) => {
                if !full.is_empty() {
                    fallback_final = Some(full);
                }
            }
            ParsedSideEvent::Error(msg) => {
                let _ = child.kill().await;
                return Err(msg);
            }
            ParsedSideEvent::PlainFallback(_) | ParsedSideEvent::Ignore => {}
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed awaiting Claude side question process: {e}"))?;
    let err_text = stderr_task.await.unwrap_or_default();

    if !streamed_any && let Some(final_text) = fallback_final {
        accumulated = final_text.clone();
        let _ = delta_tx.send(final_text);
    }

    if !status.success() {
        return Err(if err_text.is_empty() {
            format!("Claude side question exited with status {status}")
        } else {
            err_text
        });
    }

    Ok(accumulated)
}

async fn run_codex_side_question(
    executable: PathBuf,
    cwd: PathBuf,
    model: Option<String>,
    prompt: String,
    delta_tx: mpsc::UnboundedSender<String>,
) -> Result<String, String> {
    let mut cmd = Command::new(executable);
    cmd.args([
        "exec",
        "--ephemeral",
        "--skip-git-repo-check",
        "--sandbox",
        "read-only",
        "--json",
    ]);
    if let Some(m) = model.as_deref().filter(|s| !s.trim().is_empty()) {
        cmd.args(["--model", m]);
    }
    cmd.arg("-")
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::shell_snapshot::apply_cached_shell_env(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn Codex side question process: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Codex stdout pipe unavailable".to_string())?;
    let stderr_task = spawn_bounded_stderr_reader(child.stderr.take());

    let mut reader = BufReader::new(stdout).lines();
    let mut accumulated = String::new();
    let mut streamed_any = false;
    let mut fallback_final: Option<String> = None;

    while let Ok(Some(line)) = reader.next_line().await {
        if delta_tx.is_closed() {
            let _ = child.kill().await;
            return Err("Side question cancelled".to_string());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match parse_codex_side_line(trimmed) {
            ParsedSideEvent::Delta(chunk) => {
                if !chunk.is_empty() {
                    streamed_any = true;
                    accumulated.push_str(&chunk);
                    if delta_tx.send(chunk).is_err() {
                        let _ = child.kill().await;
                        return Err("Side question cancelled".to_string());
                    }
                }
            }
            ParsedSideEvent::Snapshot(full) => {
                if !full.is_empty() {
                    if !streamed_any {
                        if let Some(suffix) = full.strip_prefix(&accumulated) {
                            if !suffix.is_empty() {
                                accumulated.push_str(suffix);
                                if delta_tx.send(suffix.to_string()).is_err() {
                                    let _ = child.kill().await;
                                    return Err("Side question cancelled".to_string());
                                }
                            }
                        } else if accumulated.is_empty() {
                            accumulated = full.clone();
                            if delta_tx.send(full).is_err() {
                                let _ = child.kill().await;
                                return Err("Side question cancelled".to_string());
                            }
                        } else {
                            let sep_chunk = format!("\n\n{full}");
                            accumulated.push_str(&sep_chunk);
                            if delta_tx.send(sep_chunk).is_err() {
                                let _ = child.kill().await;
                                return Err("Side question cancelled".to_string());
                            }
                        }
                    } else {
                        fallback_final = Some(full);
                    }
                }
            }
            ParsedSideEvent::Error(msg) => {
                let _ = child.kill().await;
                return Err(msg);
            }
            ParsedSideEvent::PlainFallback(_) | ParsedSideEvent::Ignore => {}
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed awaiting Codex side question process: {e}"))?;
    let err_text = stderr_task.await.unwrap_or_default();

    if accumulated.trim().is_empty()
        && let Some(final_text) = fallback_final
    {
        accumulated = final_text.clone();
        let _ = delta_tx.send(final_text);
    }

    if !status.success() {
        return Err(if err_text.is_empty() {
            format!("Codex side question exited with status {status}")
        } else {
            err_text
        });
    }

    Ok(accumulated)
}

async fn run_agy_side_question(
    executable: PathBuf,
    cwd: PathBuf,
    model: Option<String>,
    prompt: String,
    delta_tx: mpsc::UnboundedSender<String>,
) -> Result<String, String> {
    let bounded_prompt = truncate_utf8_bytes(&prompt, MAX_AGY_ARGV_BYTES);
    let mut cmd = Command::new(executable);
    cmd.args([
        "--print",
        &bounded_prompt,
        "--output-format",
        "stream-json",
        "--disable-slash-commands",
        "--print-timeout",
        "85s",
    ]);
    if let Some(m) = model.as_deref().filter(|s| !s.trim().is_empty()) {
        cmd.args(["--model", m]);
    }
    cmd.current_dir(cwd)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::shell_snapshot::apply_cached_shell_env(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn Antigravity side question process: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Antigravity stdout pipe unavailable".to_string())?;
    let stderr_task = spawn_bounded_stderr_reader(child.stderr.take());

    let mut reader = BufReader::new(stdout).lines();
    let mut accumulated = String::new();
    let mut plain_fallback = String::new();
    let mut streamed_json_any = false;
    let mut fallback_final: Option<String> = None;

    while let Ok(Some(line)) = reader.next_line().await {
        if delta_tx.is_closed() {
            let _ = child.kill().await;
            return Err("Side question cancelled".to_string());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match parse_agy_side_line(trimmed) {
            ParsedSideEvent::Delta(chunk) => {
                if !chunk.is_empty() {
                    streamed_json_any = true;
                    accumulated.push_str(&chunk);
                    if delta_tx.send(chunk).is_err() {
                        let _ = child.kill().await;
                        return Err("Side question cancelled".to_string());
                    }
                }
            }
            ParsedSideEvent::Snapshot(full) => {
                if !full.is_empty() {
                    fallback_final = Some(full);
                }
            }
            ParsedSideEvent::PlainFallback(raw) => {
                plain_fallback.push_str(&raw);
            }
            ParsedSideEvent::Error(msg) => {
                let _ = child.kill().await;
                return Err(msg);
            }
            ParsedSideEvent::Ignore => {}
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed awaiting Antigravity side question process: {e}"))?;
    let err_text = stderr_task.await.unwrap_or_default();

    if !streamed_json_any {
        if let Some(final_text) = fallback_final {
            accumulated = final_text.clone();
            let _ = delta_tx.send(final_text);
        } else if !plain_fallback.trim().is_empty() {
            accumulated = plain_fallback.trim().to_string();
            let _ = delta_tx.send(accumulated.clone());
        }
    }

    if !status.success() {
        return Err(if err_text.is_empty() {
            format!("Antigravity side question exited with status {status}")
        } else {
            err_text
        });
    }

    Ok(accumulated)
}

#[derive(Debug, PartialEq, Eq)]
enum ParsedSideEvent {
    Delta(String),
    Snapshot(String),
    PlainFallback(String),
    Error(String),
    Ignore,
}

fn parse_claude_side_line(line: &str) -> ParsedSideEvent {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return ParsedSideEvent::Ignore;
    };
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "stream_event" => {
            if let Some(ev) = v.get("event")
                && ev.get("type").and_then(Value::as_str) == Some("content_block_delta")
                && let Some(delta) = ev.get("delta")
                && delta.get("type").and_then(Value::as_str) == Some("text_delta")
                && let Some(text) = delta.get("text").and_then(Value::as_str)
            {
                return ParsedSideEvent::Delta(text.to_string());
            }
            ParsedSideEvent::Ignore
        }
        "assistant" => {
            if let Some(blocks) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(Value::as_array)
            {
                let mut combined = String::new();
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) == Some("text")
                        && let Some(t) = block.get("text").and_then(Value::as_str)
                    {
                        combined.push_str(t);
                    }
                }
                if !combined.is_empty() {
                    return ParsedSideEvent::Snapshot(combined);
                }
            }
            ParsedSideEvent::Ignore
        }
        "result" => {
            if v.get("is_error").and_then(Value::as_bool) == Some(true) {
                let msg = v
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or("Claude side question returned an error")
                    .to_string();
                return ParsedSideEvent::Error(msg);
            }
            if let Some(res) = v.get("result").and_then(Value::as_str)
                && !res.is_empty()
            {
                return ParsedSideEvent::Snapshot(res.to_string());
            }
            ParsedSideEvent::Ignore
        }
        _ => ParsedSideEvent::Ignore,
    }
}

fn parse_codex_side_line(line: &str) -> ParsedSideEvent {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return ParsedSideEvent::Ignore;
    };
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "response.output_text.delta" | "item.agentMessage.delta" => {
            if let Some(delta) = v.get("delta").and_then(Value::as_str) {
                return ParsedSideEvent::Delta(delta.to_string());
            }
            ParsedSideEvent::Ignore
        }
        "item.updated" | "item.completed" => {
            if let Some(item) = v.get("item") {
                let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
                if matches!(item_type, "agent_message" | "agentMessage" | "message")
                    && let Some(text) = extract_text_from_value(item)
                {
                    return ParsedSideEvent::Snapshot(text);
                }
            }
            ParsedSideEvent::Ignore
        }
        "error" | "turn.failed" => {
            let msg = v
                .get("message")
                .or_else(|| v.get("error").and_then(|e| e.get("message")))
                .and_then(Value::as_str)
                .unwrap_or("Codex side question failed")
                .to_string();
            ParsedSideEvent::Error(msg)
        }
        _ => ParsedSideEvent::Ignore,
    }
}

fn parse_agy_side_line(line: &str) -> ParsedSideEvent {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return ParsedSideEvent::PlainFallback(format!("{line}\n"));
    };

    // Current `agy --print --output-format stream-json` emits `{"event":"step_update"|"result"|...}`.
    if let Some(event_kind) = v.get("event").and_then(Value::as_str) {
        return match event_kind {
            "step_update" => {
                if let Some(step) = v.get("step_update") {
                    let step_type = step.get("step_type").and_then(Value::as_str).unwrap_or("");
                    if matches!(step_type, "agent_response" | "assistant_message" | "text")
                        && let Some(delta) = step.get("text_delta").and_then(Value::as_str)
                        && !delta.is_empty()
                    {
                        return ParsedSideEvent::Delta(delta.to_string());
                    }
                    if matches!(step_type, "agent_response" | "assistant_message" | "text")
                        && let Some(full) = step
                            .get("text")
                            .or_else(|| step.get("response"))
                            .and_then(Value::as_str)
                        && !full.is_empty()
                    {
                        return ParsedSideEvent::Snapshot(full.to_string());
                    }
                }
                ParsedSideEvent::Ignore
            }
            "result" => {
                if let Some(res_obj) = v.get("result").and_then(Value::as_object) {
                    let status = res_obj
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("SUCCESS");
                    if status.eq_ignore_ascii_case("ERROR") || status.eq_ignore_ascii_case("FAILED")
                    {
                        let msg = res_obj
                            .get("error")
                            .or_else(|| res_obj.get("message"))
                            .or_else(|| res_obj.get("response"))
                            .and_then(Value::as_str)
                            .unwrap_or("Antigravity side question returned an error")
                            .to_string();
                        return ParsedSideEvent::Error(msg);
                    }
                    if let Some(resp) = res_obj
                        .get("response")
                        .or_else(|| res_obj.get("text"))
                        .and_then(Value::as_str)
                        && !resp.trim().is_empty()
                    {
                        return ParsedSideEvent::Snapshot(resp.trim().to_string());
                    }
                }
                ParsedSideEvent::Ignore
            }
            "error" => {
                let msg = v
                    .get("error")
                    .and_then(|e| {
                        e.as_str()
                            .or_else(|| e.get("message").and_then(Value::as_str))
                    })
                    .or_else(|| v.get("message").and_then(Value::as_str))
                    .unwrap_or("Antigravity side question failed")
                    .to_string();
                ParsedSideEvent::Error(msg)
            }
            _ => ParsedSideEvent::Ignore,
        };
    }

    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "stream_event" => parse_claude_side_line(line),
        "delta" | "text_delta" | "content_block_delta" => {
            if let Some(text) = v
                .get("text")
                .or_else(|| v.get("delta").and_then(|d| d.get("text")))
                .or_else(|| v.get("delta"))
                .and_then(Value::as_str)
            {
                return ParsedSideEvent::Delta(text.to_string());
            }
            ParsedSideEvent::Ignore
        }
        "assistant" | "message" => {
            if let Some(text) = extract_text_from_value(&v) {
                return ParsedSideEvent::Snapshot(text);
            }
            ParsedSideEvent::Ignore
        }
        "result" => {
            if v.get("is_error").and_then(Value::as_bool) == Some(true) {
                let msg = v
                    .get("result")
                    .or_else(|| v.get("error"))
                    .and_then(Value::as_str)
                    .unwrap_or("Antigravity side question returned an error")
                    .to_string();
                return ParsedSideEvent::Error(msg);
            }
            if let Some(res) = v
                .get("result")
                .or_else(|| v.get("response"))
                .or_else(|| v.get("text"))
                .and_then(Value::as_str)
                && !res.is_empty()
            {
                return ParsedSideEvent::Snapshot(res.to_string());
            }
            ParsedSideEvent::Ignore
        }
        "error" => {
            let msg = v
                .get("message")
                .or_else(|| v.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("Antigravity side question failed")
                .to_string();
            ParsedSideEvent::Error(msg)
        }
        _ => ParsedSideEvent::Ignore,
    }
}

fn extract_text_from_value(v: &Value) -> Option<String> {
    if let Some(text) = v.get("text").and_then(Value::as_str)
        && !text.is_empty()
    {
        return Some(text.to_string());
    }
    if let Some(content) = v.get("content").and_then(Value::as_str)
        && !content.is_empty()
    {
        return Some(content.to_string());
    }
    let blocks = v
        .get("content")
        .or_else(|| v.get("message").and_then(|m| m.get("content")))
        .and_then(Value::as_array)?;
    let mut combined = String::new();
    for b in blocks {
        let block_type = b.get("type").and_then(Value::as_str).unwrap_or("text");
        if matches!(block_type, "text" | "output_text")
            && let Some(t) = b.get("text").and_then(Value::as_str)
        {
            combined.push_str(t);
        }
    }
    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_streaming_deltas_and_result() {
        let delta_line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello "}}}"#;
        assert_eq!(
            parse_claude_side_line(delta_line),
            ParsedSideEvent::Delta("Hello ".to_string())
        );

        let result_line = r#"{"type":"result","is_error":false,"result":"Hello world"}"#;
        assert_eq!(
            parse_claude_side_line(result_line),
            ParsedSideEvent::Snapshot("Hello world".to_string())
        );
    }

    #[test]
    fn parses_codex_item_completed_message() {
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"Because RwLock allows concurrent readers."}}"#;
        assert_eq!(
            parse_codex_side_line(line),
            ParsedSideEvent::Snapshot("Because RwLock allows concurrent readers.".to_string())
        );
    }

    #[test]
    fn parses_agy_stream_json_step_update_and_result() {
        let delta_line = r#"{"event":"step_update","step_update":{"conversation_id":"c1","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"{\"handle\":\"缓存设计\"}"}}"#;
        assert_eq!(
            parse_agy_side_line(delta_line),
            ParsedSideEvent::Delta(r#"{"handle":"缓存设计"}"#.to_string())
        );

        let result_line = r#"{"event":"result","result":{"conversation_id":"c1","status":"SUCCESS","response":"{\"handle\":\"缓存设计\"}\n"}}"#;
        assert_eq!(
            parse_agy_side_line(result_line),
            ParsedSideEvent::Snapshot(r#"{"handle":"缓存设计"}"#.to_string())
        );

        let error_line =
            r#"{"event":"result","result":{"status":"ERROR","error":"quota exceeded"}}"#;
        assert_eq!(
            parse_agy_side_line(error_line),
            ParsedSideEvent::Error("quota exceeded".to_string())
        );
    }

    #[test]
    fn truncates_oversized_context_preserving_recent_tail() {
        let big = "a".repeat(MAX_CONTEXT_CHARS + 500);
        let prompt = build_side_question_prompt("Why?", &big);
        assert!(prompt.contains("earlier context truncated"));
        assert!(prompt.contains("<SIDE_QUESTION>\nWhy?\n</SIDE_QUESTION>"));
    }

    #[test]
    fn hardened_guardian_command_spec_enforces_isolated_cwd_stdin_only_and_rejects_argv_providers()
    {
        let iso = PathBuf::from("/tmp/orbit-guardian-iso-test");
        let exe = PathBuf::from("/usr/local/bin/mock-cli");

        let claude = build_hardened_guardian_command_spec(
            ProviderKind::ClaudeCode,
            &exe,
            &iso,
            Some("claude-haiku-4-5"),
        )
        .expect("Claude hardened spec");
        assert_eq!(claude.isolated_cwd, iso);
        assert!(claude.args.contains(&"--print".to_string()));
        assert!(
            claude
                .args
                .contains(&"--disable-slash-commands".to_string())
        );
        assert!(
            claude
                .args
                .contains(&"--no-session-persistence".to_string())
        );
        let tools_pos = claude
            .args
            .iter()
            .position(|a| a == "--tools")
            .expect("--tools flag");
        assert_eq!(claude.args.get(tools_pos + 1).map(String::as_str), Some(""));
        let settings_pos = claude
            .args
            .iter()
            .position(|a| a == "--setting-sources")
            .expect("--setting-sources flag");
        assert_eq!(
            claude.args.get(settings_pos + 1).map(String::as_str),
            Some("")
        );
        let model_pos = claude
            .args
            .iter()
            .position(|a| a == "--model")
            .expect("--model flag");
        assert_eq!(
            claude.args.get(model_pos + 1).map(String::as_str),
            Some("claude-haiku-4-5")
        );

        let codex = build_hardened_guardian_command_spec(
            ProviderKind::Codex,
            &exe,
            &iso,
            Some("gpt-5-mini"),
        )
        .expect("Codex hardened spec");
        assert_eq!(codex.isolated_cwd, iso);
        assert_eq!(codex.args.first().map(String::as_str), Some("exec"));
        assert!(codex.args.contains(&"--ephemeral".to_string()));
        assert!(codex.args.contains(&"--skip-git-repo-check".to_string()));
        let sandbox_pos = codex
            .args
            .iter()
            .position(|a| a == "--sandbox")
            .expect("--sandbox flag");
        assert_eq!(
            codex.args.get(sandbox_pos + 1).map(String::as_str),
            Some("read-only")
        );
        let cfg_pos = codex.args.iter().position(|a| a == "-c").expect("-c flag");
        assert_eq!(
            codex.args.get(cfg_pos + 1).map(String::as_str),
            Some("mcp_servers={}")
        );
        assert!(codex.args.contains(&"--json".to_string()));
        assert_eq!(
            codex.args.last().map(String::as_str),
            Some("-"),
            "Codex hardened spec must read prompt strictly from stdin via `-`"
        );

        // Antigravity passes prompts on process argv (`--print <prompt>`) and MUST be rejected.
        let agy_err =
            build_hardened_guardian_command_spec(ProviderKind::Antigravity, &exe, &iso, None)
                .expect_err("Antigravity must be rejected for hardened Guardian classification");
        assert!(agy_err.contains("argv"));
        assert!(!is_hardened_guardian_provider(ProviderKind::Antigravity));
        assert!(is_hardened_guardian_provider(ProviderKind::ClaudeCode));
        assert!(is_hardened_guardian_provider(ProviderKind::Codex));
    }
}
