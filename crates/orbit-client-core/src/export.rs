//! Pure-Rust conversation transcript exporter (Markdown, JSON, self-contained HTML5)
//! and fenced code-block extractor shared across Desktop, CLI, iOS, Android, and HarmonyOS.
//!
//! Security & Reliability Invariants:
//! - Zero `unsafe` code, zero `.unwrap()` / `.expect()` in production paths.
//! - HTML export emits zero external CDN links and zero `<script>` tags; every dynamic
//!   text, attribute, URL, and code block is strictly HTML-escaped (XSS-proof).
//! - Markdown export dynamically sizes code fences (`max(3, longest_backtick_run + 1)`)
//!   so nested triple-backtick snippets never break formatting.
//! - Filenames are strictly sanitized against path traversal (`..`, `/`, `\`), control
//!   characters, Windows reserved device names, and filesystem byte limits while
//!   preserving Unicode / CJK characters.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    ProviderKind, TimelineEvent, TimelinePayload, ToolCallDetail, ToolCallStatus,
};
use pulldown_cmark::{
    CodeBlockKind, Event as MdEvent, HeadingLevel, Options as MdOptions, Parser, Tag, TagEnd,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app_core::coalesce_timeline_slice;

pub const TRANSCRIPT_JSON_SCHEMA: &str = "orbit.transcript.v1";

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    #[default]
    Markdown,
    Json,
    Html,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
            Self::Html => "html",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Markdown => "text/markdown; charset=utf-8",
            Self::Json => "application/json; charset=utf-8",
            Self::Html => "text/html; charset=utf-8",
        }
    }

    pub fn from_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "md" | "markdown" => Some(Self::Markdown),
            "json" => Some(Self::Json),
            "html" | "htm" => Some(Self::Html),
            _ => None,
        }
    }
}

impl std::str::FromStr for ExportFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_slug(s).ok_or_else(|| {
            format!("unsupported export format `{s}` (expected `markdown`, `json`, or `html`)")
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportOptions {
    #[serde(default)]
    pub format: ExportFormat,
    #[serde(default = "default_true")]
    pub include_thinking: bool,
    #[serde(default = "default_true")]
    pub include_tool_calls: bool,
    #[serde(default = "default_true")]
    pub include_timestamps: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Markdown,
            include_thinking: true,
            include_tool_calls: true,
            include_timestamps: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportAgentMeta {
    pub host_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_name: Option<String>,
    pub agent_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedTranscript {
    pub suggested_filename: String,
    pub mime_type: String,
    pub content: String,
    pub turn_count: usize,
    pub byte_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedCodeBlock {
    pub index: usize,
    pub turn_index: usize,
    pub language: String,
    pub preview: String,
    pub line_count: usize,
    pub code: String,
}

/// Export a conversation timeline to Markdown, JSON, or self-contained HTML5.
///
/// Automatically coalesces streaming chunks and repeated tool-call updates so
/// raw SQLite / wire timelines and hydrated in-memory timelines produce identical output.
pub fn export_timeline_transcript(
    meta: &ExportAgentMeta,
    events: &[TimelineEvent],
    opts: &ExportOptions,
) -> ExportedTranscript {
    let coalesced = coalesce_timeline_slice(events);
    let exported_at = meta.exported_at.unwrap_or_else(Utc::now);
    let turn_count = count_turns(&coalesced);
    let suggested_filename = build_suggested_filename(&meta.title, &meta.agent_id, opts.format);

    let content = match opts.format {
        ExportFormat::Markdown => render_markdown(meta, &coalesced, opts, exported_at),
        ExportFormat::Json => render_json(meta, &coalesced, opts, exported_at, turn_count),
        ExportFormat::Html => render_html(meta, &coalesced, opts, exported_at),
    };

    let byte_len = content.len();
    ExportedTranscript {
        suggested_filename,
        mime_type: opts.format.mime_type().to_string(),
        content,
        turn_count,
        byte_len,
    }
}

/// Extract all fenced code blocks from assistant messages in reverse-chronological order
/// (index `0` is the most recently emitted code block). Supports 3+ backticks and 3+ tildes.
pub fn extract_code_blocks_from_timeline(events: &[TimelineEvent]) -> Vec<ExtractedCodeBlock> {
    let coalesced = coalesce_timeline_slice(events);
    let mut blocks = Vec::new();
    let mut turn_index: usize = 0;

    for ev in &coalesced {
        match &ev.payload {
            TimelinePayload::UserPrompt { .. } => {
                turn_index += 1;
            }
            TimelinePayload::AssistantMessage { text, .. } => {
                let effective_turn = turn_index.max(1);
                for (lang, code) in extract_fenced_blocks_from_markdown(text) {
                    let line_count = if code.is_empty() {
                        0
                    } else {
                        code.lines().count()
                    };
                    let preview = code_block_preview(&code);
                    blocks.push(ExtractedCodeBlock {
                        index: 0,
                        turn_index: effective_turn,
                        language: lang,
                        preview,
                        line_count,
                        code,
                    });
                }
            }
            _ => {}
        }
    }

    blocks.reverse();
    for (idx, block) in blocks.iter_mut().enumerate() {
        block.index = idx;
    }
    blocks
}

/// Atomically write bytes to `path` via an exclusively created (`O_EXCL`, `0o600` on Unix)
/// fixed-length sibling temporary file + `rename` so partial exports are never left behind
/// and symlinks / world-readable temp files are refused.
pub fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("export path cannot be empty".into());
    }
    if path.file_name().is_none() {
        return Err(format!(
            "export path `{}` does not specify a file name",
            path.display()
        ));
    }
    if path.is_dir() {
        return Err(format!(
            "export path `{}` is an existing directory",
            path.display()
        ));
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directory `{}`: {e}", parent.display()))?;
    }

    let tmp_name = format!(
        ".orbit-export-tmp-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    );
    let tmp_path = path.with_file_name(tmp_name);

    let write_res = (|| -> Result<(), String> {
        let mut open_opts = std::fs::OpenOptions::new();
        open_opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open_opts.mode(0o600);
        }
        let mut file = open_opts.open(&tmp_path).map_err(|e| {
            format!(
                "failed to create temporary file `{}`: {e}",
                tmp_path.display()
            )
        })?;
        file.write_all(bytes)
            .map_err(|e| format!("failed to write exported transcript: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("failed to flush exported transcript: {e}"))?;
        drop(file);
        std::fs::rename(&tmp_path, path)
            .map_err(|e| format!("failed to finalize export `{}`: {e}", path.display()))?;
        Ok(())
    })();

    if write_res.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    write_res
}

/// Sanitize a conversation title into a safe, cross-platform filename slug with extension.
pub fn build_suggested_filename(title: &str, agent_id: &str, format: ExportFormat) -> String {
    let mut slug = sanitize_filename_slug(title);
    if slug.is_empty() {
        slug = sanitize_filename_slug(agent_id);
    }
    if slug.is_empty() {
        slug = "orbit-conversation".to_string();
    }
    format!("{slug}.{}", format.extension())
}

fn sanitize_filename_slug(raw: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    let mut char_count = 0usize;

    for ch in raw.trim().chars() {
        if ch.is_control()
            || matches!(
                ch,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'
            )
        {
            if !slug.is_empty() && !prev_dash {
                slug.push('-');
                prev_dash = true;
            }
            continue;
        }

        if ch.is_alphanumeric() {
            let lower_str: String = ch.to_lowercase().collect();
            if slug.len() + lower_str.len() > 110 || char_count >= 64 {
                break;
            }
            slug.push_str(&lower_str);
            char_count += lower_str.chars().count();
            prev_dash = false;
        } else if !slug.is_empty() && !prev_dash {
            if slug.len() + 1 > 110 || char_count >= 64 {
                break;
            }
            slug.push('-');
            char_count += 1;
            prev_dash = true;
        }
    }

    let trimmed = slug.trim_matches(['-', '.', '_']);
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return String::new();
    }
    if is_windows_reserved_device_name(trimmed) {
        return format!("orbit-{trimmed}");
    }
    trimmed.to_string()
}

fn is_windows_reserved_device_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

fn effective_title(meta: &ExportAgentMeta) -> &str {
    let t = meta.title.trim();
    if t.is_empty() { "Conversation" } else { t }
}

fn count_turns(events: &[TimelineEvent]) -> usize {
    let user_turns = events
        .iter()
        .filter(|e| matches!(e.payload, TimelinePayload::UserPrompt { .. }))
        .count();
    if user_turns > 0 {
        return user_turns;
    }
    let has_content = events.iter().any(|e| {
        matches!(
            e.payload,
            TimelinePayload::AssistantMessage { .. }
                | TimelinePayload::AgentMessage { .. }
                | TimelinePayload::ToolCall { .. }
        )
    });
    usize::from(has_content)
}

fn format_timestamp_human(ts: DateTime<Utc>) -> String {
    ts.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn longest_backtick_run(text: &str) -> usize {
    let mut best = 0usize;
    let mut current = 0usize;
    for b in text.bytes() {
        if b == b'`' {
            current += 1;
            if current > best {
                best = current;
            }
        } else {
            current = 0;
        }
    }
    best
}

/// Wrap `code` in a Markdown fenced code block whose delimiter is strictly longer than
/// any backtick run inside `code`, guaranteeing nested fences never break rendering.
fn wrap_fenced_code(code: &str, lang: &str) -> String {
    let fence_len = longest_backtick_run(code).saturating_add(1).max(3);
    let fence: String = "`".repeat(fence_len);
    let clean_lang: String = lang
        .trim()
        .chars()
        .filter(|c| !c.is_control() && *c != '`')
        .collect();
    let trimmed = code.trim_end_matches('\n');
    format!("{fence}{clean_lang}\n{trimmed}\n{fence}")
}

fn tool_status_label(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Running => "running",
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Failed => "failed",
        ToolCallStatus::Denied => "denied",
    }
}

fn format_tool_summary(status: ToolCallStatus, detail: &ToolCallDetail) -> String {
    let st = tool_status_label(status);
    match detail {
        ToolCallDetail::Shell {
            command, exit_code, ..
        } => {
            let cmd = single_line_preview(command, 72);
            if let Some(code) = exit_code {
                format!("Tool: Shell ({st}, exit {code}) — {cmd}")
            } else {
                format!("Tool: Shell ({st}) — {cmd}")
            }
        }
        ToolCallDetail::FileRead {
            path,
            start_line,
            end_line,
        } => {
            let range = match (start_line, end_line) {
                (Some(s), Some(e)) => format!(":{s}-{e}"),
                (Some(s), None) => format!(":{s}"),
                _ => String::new(),
            };
            format!("Tool: Read {path}{range} ({st})")
        }
        ToolCallDetail::FileEdit { path, .. } => format!("Tool: Edit {path} ({st})"),
        ToolCallDetail::Search { query, scope } => {
            let q = single_line_preview(query, 64);
            if let Some(sc) = scope.as_deref().filter(|s| !s.trim().is_empty()) {
                format!("Tool: Search \"{q}\" in {sc} ({st})")
            } else {
                format!("Tool: Search \"{q}\" ({st})")
            }
        }
        ToolCallDetail::Subagent { role, .. } => format!("Tool: Subagent {role} ({st})"),
        ToolCallDetail::Generic { tool_name, .. } => format!("Tool: {tool_name} ({st})"),
    }
}

fn single_line_preview(raw: &str, max_chars: usize) -> String {
    let collapsed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, max_chars)
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        raw.to_string()
    } else {
        let mut out: String = raw.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn code_block_preview(code: &str) -> String {
    let first_non_empty = code
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("(empty)");
    truncate_chars(first_non_empty, 88)
}

/// Wrap `raw` in a CommonMark-compliant inline code span whose backtick delimiter is strictly
/// longer than any backtick run inside `raw`, collapsing line breaks to spaces so Markdown
/// metadata and tool detail lines never break.
fn wrap_inline_code(raw: &str) -> String {
    let single_line = sanitize_md_single_line(raw);
    let ticks = longest_backtick_run(&single_line).saturating_add(1).max(1);
    let delim = "`".repeat(ticks);
    let needs_pad = single_line.starts_with('`')
        || single_line.ends_with('`')
        || (single_line.starts_with(' ')
            && single_line.ends_with(' ')
            && single_line.bytes().any(|b| b != b' '));
    if needs_pad {
        format!("{delim} {single_line} {delim}")
    } else {
        format!("{delim}{single_line}{delim}")
    }
}

fn sanitize_md_single_line(raw: &str) -> String {
    raw.replace(['\r', '\n'], " ").trim().to_string()
}

fn extract_fenced_blocks_from_markdown(markdown: &str) -> Vec<(String, String)> {
    let mut md_opts = MdOptions::empty();
    md_opts.insert(MdOptions::ENABLE_TABLES);
    md_opts.insert(MdOptions::ENABLE_STRIKETHROUGH);
    md_opts.insert(MdOptions::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, md_opts);
    let mut blocks = Vec::new();
    let mut active_fenced: Option<(String, String)> = None;

    for event in parser {
        match event {
            MdEvent::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                let lang_token = info
                    .trim()
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .next()
                    .unwrap_or("")
                    .trim();
                let lang = if lang_token.is_empty() {
                    "text".to_string()
                } else {
                    lang_token.to_string()
                };
                active_fenced = Some((lang, String::new()));
            }
            MdEvent::Text(chunk) => {
                if let Some((_, ref mut buf)) = active_fenced {
                    buf.push_str(&chunk);
                }
            }
            MdEvent::End(TagEnd::CodeBlock) => {
                if let Some((lang, mut code)) = active_fenced.take() {
                    if code.ends_with('\n') {
                        code.pop();
                        if code.ends_with('\r') {
                            code.pop();
                        }
                    }
                    blocks.push((lang, code));
                }
            }
            _ => {}
        }
    }

    if let Some((lang, mut code)) = active_fenced {
        if code.ends_with('\n') {
            code.pop();
            if code.ends_with('\r') {
                code.pop();
            }
        }
        blocks.push((lang, code));
    }

    blocks
}

// ---------------------------------------------------------------------------
// Markdown renderer
// ---------------------------------------------------------------------------

fn render_markdown(
    meta: &ExportAgentMeta,
    events: &[TimelineEvent],
    opts: &ExportOptions,
    exported_at: DateTime<Utc>,
) -> String {
    let mut out = String::with_capacity(4096);
    let _ = writeln!(
        out,
        "# {}\n",
        sanitize_md_single_line(effective_title(meta))
    );

    if let Some(provider) = meta.provider {
        let _ = writeln!(out, "- **Provider**: {}", provider.display_name());
    }
    if let Some(model) = meta.model.as_deref().filter(|s| !s.trim().is_empty()) {
        let _ = writeln!(out, "- **Model**: {}", wrap_inline_code(model));
    }
    if let Some(host) = meta.host_name.as_deref().filter(|s| !s.trim().is_empty()) {
        let _ = writeln!(out, "- **Host**: {}", sanitize_md_single_line(host));
    }
    if let Some(ws) = meta
        .workspace_title
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let _ = writeln!(out, "- **Workspace**: {}", sanitize_md_single_line(ws));
    }
    if let Some(cwd) = meta.cwd.as_deref().filter(|s| !s.trim().is_empty()) {
        let _ = writeln!(out, "- **Directory**: {}", wrap_inline_code(cwd));
    }
    if let Some(summary) = meta
        .task_summary
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let _ = writeln!(out, "- **Summary**: {}", sanitize_md_single_line(summary));
    }
    if opts.include_timestamps {
        let _ = writeln!(
            out,
            "- **Exported**: {}",
            format_timestamp_human(exported_at)
        );
    }
    out.push_str("\n---\n");

    let mut wrote_section = false;
    for ev in events {
        match &ev.payload {
            TimelinePayload::UserPrompt { text, badges, .. } => {
                let body = text.trim();
                if body.is_empty() && badges.is_empty() {
                    continue;
                }
                if wrote_section {
                    out.push_str("\n---\n");
                }
                wrote_section = true;
                if opts.include_timestamps {
                    let _ = writeln!(
                        out,
                        "\n## User · `{}`\n",
                        format_timestamp_human(ev.timestamp)
                    );
                } else {
                    out.push_str("\n## User\n\n");
                }
                if !badges.is_empty() {
                    let badge_str: Vec<String> =
                        badges.iter().map(|b| wrap_inline_code(&b.label)).collect();
                    let _ = writeln!(out, "> Context: {}\n", badge_str.join(" · "));
                }
                if !body.is_empty() {
                    out.push_str(body);
                    out.push('\n');
                }
            }
            TimelinePayload::AssistantThinking { text, .. } => {
                if !opts.include_thinking {
                    continue;
                }
                let body = text.trim();
                if body.is_empty() {
                    continue;
                }
                wrote_section = true;
                let _ = write!(
                    out,
                    "\n<details>\n<summary>Thinking</summary>\n\n{}\n\n</details>\n",
                    wrap_fenced_code(body, "text")
                );
            }
            TimelinePayload::ToolCall { status, detail, .. } => {
                if !opts.include_tool_calls {
                    continue;
                }
                wrote_section = true;
                let summary = escape_html(&format_tool_summary(*status, detail));
                let body = render_tool_detail_markdown(detail);
                if body.is_empty() {
                    let _ = write!(
                        out,
                        "\n<details>\n<summary>{summary}</summary>\n</details>\n"
                    );
                } else {
                    let _ = write!(
                        out,
                        "\n<details>\n<summary>{summary}</summary>\n\n{body}\n\n</details>\n"
                    );
                }
            }
            TimelinePayload::AssistantMessage { text, .. } => {
                let body = text.trim();
                if body.is_empty() {
                    continue;
                }
                wrote_section = true;
                if opts.include_timestamps {
                    let _ = writeln!(
                        out,
                        "\n## Assistant · `{}`\n",
                        format_timestamp_human(ev.timestamp)
                    );
                } else {
                    out.push_str("\n## Assistant\n\n");
                }
                out.push_str(body);
                out.push('\n');
            }
            TimelinePayload::AgentMessage {
                source_title, text, ..
            } => {
                let body = text.trim();
                if body.is_empty() {
                    continue;
                }
                wrote_section = true;
                let heading = source_title
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| format!("Agent ({})", sanitize_md_single_line(s)))
                    .unwrap_or_else(|| "Agent Message".to_string());
                if opts.include_timestamps {
                    let _ = writeln!(
                        out,
                        "\n## {heading} · `{}`\n",
                        format_timestamp_human(ev.timestamp)
                    );
                } else {
                    let _ = writeln!(out, "\n## {heading}\n");
                }
                out.push_str(body);
                out.push('\n');
            }
            TimelinePayload::Notice { level, message } => {
                let body = message.trim();
                if body.is_empty() {
                    continue;
                }
                wrote_section = true;
                let clean_level = sanitize_md_single_line(level);
                let mut lines = body.lines();
                if let Some(first) = lines.next() {
                    let _ = writeln!(out, "\n> **Notice ({clean_level})**: {first}");
                    for extra in lines {
                        if extra.trim().is_empty() {
                            out.push_str(">\n");
                        } else {
                            let _ = writeln!(out, "> {extra}");
                        }
                    }
                    out.push('\n');
                }
            }
            TimelinePayload::PermissionRequest { .. }
            | TimelinePayload::PermissionResolved { .. }
            | TimelinePayload::QuestionRequest { .. }
            | TimelinePayload::QuestionResolved { .. }
            | TimelinePayload::StatusChanged { .. } => {}
        }
    }

    out
}

fn render_tool_detail_markdown(detail: &ToolCallDetail) -> String {
    match detail {
        ToolCallDetail::Shell {
            command,
            cwd,
            exit_code,
            output_preview,
        } => {
            let mut buf = String::new();
            if let Some(dir) = cwd.as_deref().filter(|s| !s.trim().is_empty()) {
                let _ = writeln!(buf, "Directory: {}\n", wrap_inline_code(dir));
            }
            buf.push_str(&wrap_fenced_code(command, "sh"));
            if let Some(code) = exit_code {
                let _ = write!(buf, "\n\nExit code: `{code}`");
            }
            if let Some(out) = output_preview.as_deref().filter(|s| !s.trim().is_empty()) {
                buf.push_str("\n\n");
                buf.push_str(&wrap_fenced_code(out, "text"));
            }
            buf
        }
        ToolCallDetail::FileRead {
            path,
            start_line,
            end_line,
        } => {
            let wrapped = wrap_inline_code(path);
            match (start_line, end_line) {
                (Some(s), Some(e)) => format!("Path: {wrapped} (lines {s}–{e})"),
                (Some(s), None) => format!("Path: {wrapped} (from line {s})"),
                _ => format!("Path: {wrapped}"),
            }
        }
        ToolCallDetail::FileEdit { path, unified_diff } => {
            let mut buf = format!("Path: {}", wrap_inline_code(path));
            if let Some(diff) = unified_diff.as_deref().filter(|s| !s.trim().is_empty()) {
                buf.push_str("\n\n");
                buf.push_str(&wrap_fenced_code(diff, "diff"));
            }
            buf
        }
        ToolCallDetail::Search { query, scope } => {
            let q = wrap_inline_code(query);
            if let Some(sc) = scope.as_deref().filter(|s| !s.trim().is_empty()) {
                format!("Query: {q} in {}", wrap_inline_code(sc))
            } else {
                format!("Query: {q}")
            }
        }
        ToolCallDetail::Subagent {
            child_agent_id,
            role,
            prompt_preview,
        } => {
            let mut buf = format!("Role: {}", wrap_inline_code(role));
            if let Some(cid) = child_agent_id.as_deref().filter(|s| !s.trim().is_empty()) {
                let _ = write!(buf, " · Agent: {}", wrap_inline_code(cid));
            }
            if !prompt_preview.trim().is_empty() {
                buf.push_str("\n\n");
                buf.push_str(&wrap_fenced_code(prompt_preview, "text"));
            }
            buf
        }
        ToolCallDetail::Generic {
            tool_name,
            arguments_json,
        } => {
            let mut buf = format!("Tool: {}", wrap_inline_code(tool_name));
            if !arguments_json.trim().is_empty() {
                buf.push_str("\n\n");
                buf.push_str(&wrap_fenced_code(arguments_json, "json"));
            }
            buf
        }
    }
}

// ---------------------------------------------------------------------------
// JSON renderer
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ExportJsonDocument<'a> {
    schema: &'static str,
    exported_at: DateTime<Utc>,
    options: &'a ExportOptions,
    agent: ExportJsonAgent<'a>,
    turn_count: usize,
    events: Vec<ExportJsonEvent>,
}

#[derive(Debug, Serialize)]
struct ExportJsonAgent<'a> {
    host_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    host_name: Option<&'a str>,
    agent_id: &'a str,
    title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<ProviderKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_summary: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct ExportJsonEvent {
    seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<DateTime<Utc>>,
    payload: TimelinePayload,
}

fn render_json(
    meta: &ExportAgentMeta,
    events: &[TimelineEvent],
    opts: &ExportOptions,
    exported_at: DateTime<Utc>,
    turn_count: usize,
) -> String {
    let filtered_events: Vec<ExportJsonEvent> = events
        .iter()
        .filter(|ev| match &ev.payload {
            TimelinePayload::AssistantThinking { .. } => opts.include_thinking,
            TimelinePayload::ToolCall { .. } => opts.include_tool_calls,
            TimelinePayload::StatusChanged { .. } => false,
            _ => true,
        })
        .map(|ev| ExportJsonEvent {
            seq: ev.seq,
            turn_id: ev.turn_id.clone(),
            timestamp: opts.include_timestamps.then_some(ev.timestamp),
            payload: ev.payload.clone(),
        })
        .collect();

    let doc = ExportJsonDocument {
        schema: TRANSCRIPT_JSON_SCHEMA,
        exported_at,
        options: opts,
        agent: ExportJsonAgent {
            host_id: &meta.host_id,
            host_name: meta.host_name.as_deref(),
            agent_id: &meta.agent_id,
            title: effective_title(meta),
            provider: meta.provider,
            model: meta.model.as_deref(),
            cwd: meta.cwd.as_deref(),
            workspace_title: meta.workspace_title.as_deref(),
            task_summary: meta.task_summary.as_deref(),
            created_at: meta.created_at,
        },
        turn_count,
        events: filtered_events,
    };

    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
}

// ---------------------------------------------------------------------------
// Self-contained HTML5 renderer (ZERO scripts, ZERO CDN, 100% HTML-escaped)
// ---------------------------------------------------------------------------

pub(crate) fn escape_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn is_safe_href(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return false;
    }
    if trimmed.starts_with("//") || trimmed.starts_with("/\\") || trimmed.starts_with("\\\\") {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || trimmed.starts_with('#')
        || trimmed.starts_with('/')
}

fn render_markdown_to_safe_html(markdown: &str) -> String {
    let mut md_opts = MdOptions::empty();
    md_opts.insert(MdOptions::ENABLE_TABLES);
    md_opts.insert(MdOptions::ENABLE_STRIKETHROUGH);
    md_opts.insert(MdOptions::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, md_opts);
    let mut html = String::with_capacity(markdown.len() + 64);
    let mut in_code_block = false;
    let mut in_table_head = false;
    let mut active_link_opened: Vec<bool> = Vec::new();
    let mut image_depth: usize = 0;
    let mut image_alt = String::new();

    for event in parser {
        if image_depth > 0 {
            match event {
                MdEvent::Start(Tag::Image { .. }) => {
                    image_depth += 1;
                }
                MdEvent::Text(t) | MdEvent::Code(t) => image_alt.push_str(&t),
                MdEvent::SoftBreak | MdEvent::HardBreak => {
                    if !image_alt.ends_with(' ') && !image_alt.is_empty() {
                        image_alt.push(' ');
                    }
                }
                MdEvent::End(TagEnd::Image) => {
                    image_depth -= 1;
                    if image_depth == 0 {
                        let trimmed = image_alt.trim();
                        let label = if trimmed.is_empty() { "image" } else { trimmed };
                        let _ = write!(
                            html,
                            "<span class=\"img-ref\">[{}]</span>",
                            escape_html(label)
                        );
                        image_alt.clear();
                    }
                }
                _ => {}
            }
            continue;
        }

        match event {
            MdEvent::Start(tag) => match tag {
                Tag::Paragraph => html.push_str("<p>"),
                Tag::Heading { level, .. } => {
                    let n = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    let _ = write!(html, "<h{n}>");
                }
                Tag::BlockQuote(_) => html.push_str("<blockquote>"),
                Tag::CodeBlock(kind) => {
                    in_code_block = true;
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => info
                            .split(|c: char| c.is_whitespace() || c == ',')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    html.push_str("<div class=\"code-wrap\">");
                    if !lang.is_empty() {
                        let _ = write!(
                            html,
                            "<div class=\"code-lang\">{}</div>",
                            escape_html(&lang)
                        );
                    }
                    html.push_str("<pre><code>");
                }
                Tag::List(Some(start)) => {
                    if start == 1 {
                        html.push_str("<ol>");
                    } else {
                        let _ = write!(html, "<ol start=\"{start}\">");
                    }
                }
                Tag::List(None) => html.push_str("<ul>"),
                Tag::Item => html.push_str("<li>"),
                Tag::Table(_) => html.push_str("<div class=\"table-wrap\"><table>"),
                Tag::TableHead => {
                    in_table_head = true;
                    html.push_str("<thead><tr>");
                }
                Tag::TableRow => html.push_str("<tr>"),
                Tag::TableCell => {
                    if in_table_head {
                        html.push_str("<th>");
                    } else {
                        html.push_str("<td>");
                    }
                }
                Tag::Emphasis => html.push_str("<em>"),
                Tag::Strong => html.push_str("<strong>"),
                Tag::Strikethrough => html.push_str("<del>"),
                Tag::Link {
                    dest_url, title, ..
                } => {
                    let clean_url = dest_url.trim();
                    if is_safe_href(clean_url) {
                        let _ = write!(
                            html,
                            "<a href=\"{}\" rel=\"noopener noreferrer\"",
                            escape_html(clean_url)
                        );
                        if !title.is_empty() {
                            let _ = write!(html, " title=\"{}\"", escape_html(&title));
                        }
                        html.push('>');
                        active_link_opened.push(true);
                    } else {
                        active_link_opened.push(false);
                    }
                }
                Tag::Image { .. } => {
                    image_depth = 1;
                    image_alt.clear();
                }
                _ => {}
            },
            MdEvent::End(tag_end) => match tag_end {
                TagEnd::Paragraph => html.push_str("</p>\n"),
                TagEnd::Heading(level) => {
                    let n = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    let _ = writeln!(html, "</h{n}>");
                }
                TagEnd::BlockQuote(_) => html.push_str("</blockquote>\n"),
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    html.push_str("</code></pre></div>\n");
                }
                TagEnd::List(ordered) => {
                    if ordered {
                        html.push_str("</ol>\n");
                    } else {
                        html.push_str("</ul>\n");
                    }
                }
                TagEnd::Item => html.push_str("</li>\n"),
                TagEnd::Table => html.push_str("</tbody></table></div>\n"),
                TagEnd::TableHead => {
                    in_table_head = false;
                    html.push_str("</tr></thead><tbody>\n");
                }
                TagEnd::TableRow => html.push_str("</tr>\n"),
                TagEnd::TableCell => {
                    if in_table_head {
                        html.push_str("</th>");
                    } else {
                        html.push_str("</td>");
                    }
                }
                TagEnd::Emphasis => html.push_str("</em>"),
                TagEnd::Strong => html.push_str("</strong>"),
                TagEnd::Strikethrough => html.push_str("</del>"),
                TagEnd::Link if active_link_opened.pop().unwrap_or(false) => {
                    html.push_str("</a>");
                }
                _ => {}
            },
            MdEvent::Text(text) => {
                html.push_str(&escape_html(&text));
            }
            MdEvent::Code(code) => {
                let _ = write!(html, "<code>{}</code>", escape_html(&code));
            }
            // Strict XSS defense: raw HTML embedded in user/assistant markdown is ALWAYS escaped.
            MdEvent::Html(raw) | MdEvent::InlineHtml(raw) => {
                if in_code_block {
                    html.push_str(&escape_html(&raw));
                } else {
                    let _ = write!(html, "<code>{}</code>", escape_html(raw.trim()));
                }
            }
            MdEvent::SoftBreak => html.push('\n'),
            MdEvent::HardBreak => html.push_str("<br>\n"),
            MdEvent::Rule => html.push_str("<hr>\n"),
            MdEvent::TaskListMarker(checked) => {
                if checked {
                    html.push_str("<span class=\"task-check\" aria-label=\"done\">&#9745;</span> ");
                } else {
                    html.push_str("<span class=\"task-check\" aria-label=\"todo\">&#9744;</span> ");
                }
            }
            MdEvent::FootnoteReference(label) => {
                let _ = write!(html, "<sup>[{}]</sup>", escape_html(&label));
            }
            MdEvent::InlineMath(math) | MdEvent::DisplayMath(math) => {
                let _ = write!(html, "<code>{}</code>", escape_html(&math));
            }
        }
    }

    html
}

const EXPORT_HTML_CSS: &str = r#"
:root {
  --bg: #fafaf9;
  --surface: #ffffff;
  --surface-subtle: #f4f4f5;
  --surface-user: #f3f4f6;
  --border: rgba(24, 24, 27, 0.10);
  --border-subtle: rgba(24, 24, 27, 0.06);
  --text: #18181b;
  --text-secondary: #52525b;
  --text-muted: #71717a;
  --code-bg: #f4f4f5;
  --code-header: #ececee;
  --accent: #2563eb;
  --radius: 12px;
  --font-sans: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Inter", "Segoe UI", system-ui, sans-serif;
  --font-mono: "SF Mono", ui-monospace, "JetBrains Mono", Menlo, Consolas, monospace;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0c0c0e;
    --surface: #141417;
    --surface-subtle: #1c1c21;
    --surface-user: #1b1c22;
    --border: rgba(255, 255, 255, 0.10);
    --border-subtle: rgba(255, 255, 255, 0.06);
    --text: #f4f4f5;
    --text-secondary: #a1a1aa;
    --text-muted: #71717a;
    --code-bg: #18181c;
    --code-header: #202026;
    --accent: #60a5fa;
  }
}
* { box-sizing: border-box; }
html, body {
  margin: 0;
  padding: 0;
  background: var(--bg);
  color: var(--text);
  font-family: var(--font-sans);
  font-size: 15px;
  line-height: 1.65;
  -webkit-font-smoothing: antialiased;
}
main.transcript {
  max-width: 820px;
  margin: 0 auto;
  padding: 40px 24px 72px;
}
header.doc-header {
  padding-bottom: 24px;
  margin-bottom: 28px;
  border-bottom: 1px solid var(--border);
}
header.doc-header h1 {
  margin: 0 0 12px;
  font-size: 24px;
  font-weight: 650;
  letter-spacing: -0.02em;
  line-height: 1.3;
}
p.doc-summary {
  margin: 0 0 14px;
  color: var(--text-secondary);
  font-size: 14px;
}
.meta-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  font-size: 12.5px;
  color: var(--text-secondary);
}
.meta-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 3px 10px;
  border-radius: 999px;
  background: var(--surface-subtle);
  border: 1px solid var(--border-subtle);
}
.meta-pill strong {
  font-weight: 550;
  color: var(--text-muted);
}
.meta-pill code {
  background: transparent;
  padding: 0;
  font-size: 12px;
}
section.entry {
  margin: 18px 0;
}
section.entry-user {
  background: var(--surface-user);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  padding: 16px 20px;
}
section.entry-assistant {
  padding: 6px 2px;
}
section.entry-agent {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 14px 18px;
}
.entry-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}
.role-label {
  font-size: 12px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
}
.entry-time {
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-muted);
}
.badge-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 10px;
}
.ctx-badge {
  font-family: var(--font-mono);
  font-size: 11.5px;
  padding: 2px 8px;
  border-radius: 6px;
  background: var(--surface);
  border: 1px solid var(--border);
  color: var(--text-secondary);
}
.entry-body > :first-child { margin-top: 0; }
.entry-body > :last-child { margin-bottom: 0; }
.entry-body p { margin: 0 0 12px; }
.entry-body h1, .entry-body h2, .entry-body h3, .entry-body h4 {
  margin: 20px 0 8px;
  line-height: 1.35;
  letter-spacing: -0.015em;
}
.entry-body h1 { font-size: 20px; }
.entry-body h2 { font-size: 17px; }
.entry-body h3 { font-size: 15.5px; }
.entry-body ul, .entry-body ol {
  margin: 0 0 12px;
  padding-left: 22px;
}
.entry-body li { margin: 4px 0; }
.entry-body blockquote {
  margin: 12px 0;
  padding: 4px 14px;
  border-left: 3px solid var(--border);
  color: var(--text-secondary);
}
.entry-body a {
  color: var(--accent);
  text-decoration: none;
}
.entry-body a:hover { text-decoration: underline; }
.entry-body hr {
  border: 0;
  border-top: 1px solid var(--border);
  margin: 20px 0;
}
code {
  font-family: var(--font-mono);
  font-size: 0.88em;
  background: var(--code-bg);
  padding: 2px 5px;
  border-radius: 5px;
}
.code-wrap {
  margin: 12px 0;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--code-bg);
  overflow: hidden;
}
.code-lang {
  padding: 5px 12px;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-muted);
  background: var(--code-header);
  border-bottom: 1px solid var(--border-subtle);
}
.code-wrap pre {
  margin: 0;
  padding: 12px 14px;
  overflow-x: auto;
  font-size: 13px;
  line-height: 1.55;
}
.code-wrap pre code {
  background: transparent;
  padding: 0;
  border-radius: 0;
  font-size: inherit;
}
.table-wrap {
  overflow-x: auto;
  margin: 12px 0;
}
table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13.5px;
}
th, td {
  text-align: left;
  padding: 7px 10px;
  border: 1px solid var(--border);
}
th {
  background: var(--surface-subtle);
  font-weight: 600;
}
details.disclosure {
  margin: 10px 0;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface);
  overflow: hidden;
}
details.disclosure > summary {
  cursor: pointer;
  padding: 8px 13px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  list-style: none;
  user-select: none;
}
details.disclosure > summary::-webkit-details-marker { display: none; }
details.disclosure > summary::before {
  content: "▸ ";
  color: var(--text-muted);
}
details.disclosure[open] > summary::before {
  content: "▾ ";
}
.disclosure-body {
  padding: 10px 13px 12px;
  border-top: 1px solid var(--border-subtle);
  font-size: 13.5px;
}
.disclosure-body pre {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: var(--font-mono);
  font-size: 12.5px;
  line-height: 1.5;
}
.notice-banner {
  margin: 12px 0;
  padding: 10px 14px;
  border-radius: 9px;
  border: 1px solid var(--border);
  background: var(--surface-subtle);
  font-size: 13.5px;
  color: var(--text-secondary);
}
footer.doc-footer {
  margin-top: 40px;
  padding-top: 16px;
  border-top: 1px solid var(--border-subtle);
  font-size: 12px;
  color: var(--text-muted);
}
@media print {
  body { background: #ffffff; color: #000000; }
  main.transcript { max-width: 100%; padding: 12px; }
  details.disclosure { break-inside: avoid; }
}
"#;

fn render_html(
    meta: &ExportAgentMeta,
    events: &[TimelineEvent],
    opts: &ExportOptions,
    exported_at: DateTime<Utc>,
) -> String {
    let title_esc = escape_html(effective_title(meta));
    let mut out = String::with_capacity(8192);
    let _ = write!(
        out,
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title_esc} — Orbit Transcript</title>\n\
         <style>{EXPORT_HTML_CSS}</style>\n\
         </head>\n<body>\n<main class=\"transcript\">\n"
    );

    // Header
    out.push_str("<header class=\"doc-header\">\n");
    let _ = writeln!(out, "<h1>{title_esc}</h1>");
    if let Some(summary) = meta
        .task_summary
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let _ = writeln!(out, "<p class=\"doc-summary\">{}</p>", escape_html(summary));
    }
    out.push_str("<div class=\"meta-grid\">");
    if let Some(provider) = meta.provider {
        let _ = write!(
            out,
            "<span class=\"meta-pill\"><strong>Provider</strong> {}</span>",
            escape_html(provider.display_name())
        );
    }
    if let Some(model) = meta.model.as_deref().filter(|s| !s.trim().is_empty()) {
        let _ = write!(
            out,
            "<span class=\"meta-pill\"><strong>Model</strong> <code>{}</code></span>",
            escape_html(model)
        );
    }
    if let Some(host) = meta.host_name.as_deref().filter(|s| !s.trim().is_empty()) {
        let _ = write!(
            out,
            "<span class=\"meta-pill\"><strong>Host</strong> {}</span>",
            escape_html(host)
        );
    }
    if let Some(ws) = meta
        .workspace_title
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let _ = write!(
            out,
            "<span class=\"meta-pill\"><strong>Workspace</strong> {}</span>",
            escape_html(ws)
        );
    }
    if let Some(cwd) = meta.cwd.as_deref().filter(|s| !s.trim().is_empty()) {
        let _ = write!(
            out,
            "<span class=\"meta-pill\"><strong>Dir</strong> <code>{}</code></span>",
            escape_html(cwd)
        );
    }
    if opts.include_timestamps {
        let _ = write!(
            out,
            "<span class=\"meta-pill\"><strong>Exported</strong> {}</span>",
            escape_html(&format_timestamp_human(exported_at))
        );
    }
    out.push_str("</div>\n</header>\n");

    // Conversation timeline
    for ev in events {
        match &ev.payload {
            TimelinePayload::UserPrompt { text, badges, .. } => {
                let body = text.trim();
                if body.is_empty() && badges.is_empty() {
                    continue;
                }
                out.push_str("<section class=\"entry entry-user\">\n<div class=\"entry-head\"><span class=\"role-label\">User</span>");
                if opts.include_timestamps {
                    let _ = write!(
                        out,
                        "<time class=\"entry-time\">{}</time>",
                        escape_html(&format_timestamp_human(ev.timestamp))
                    );
                }
                out.push_str("</div>\n");
                if !badges.is_empty() {
                    out.push_str("<div class=\"badge-row\">");
                    for b in badges {
                        let _ = write!(
                            out,
                            "<span class=\"ctx-badge\">{}</span>",
                            escape_html(&b.label)
                        );
                    }
                    out.push_str("</div>\n");
                }
                if !body.is_empty() {
                    let _ = writeln!(
                        out,
                        "<div class=\"entry-body\">{}</div>",
                        render_markdown_to_safe_html(body)
                    );
                }
                out.push_str("</section>\n");
            }
            TimelinePayload::AssistantThinking { text, .. } => {
                if !opts.include_thinking {
                    continue;
                }
                let body = text.trim();
                if body.is_empty() {
                    continue;
                }
                let _ = writeln!(
                    out,
                    "<details class=\"disclosure\"><summary>Thinking</summary><div class=\"disclosure-body\"><pre>{}</pre></div></details>",
                    escape_html(body)
                );
            }
            TimelinePayload::ToolCall { status, detail, .. } => {
                if !opts.include_tool_calls {
                    continue;
                }
                let summary = escape_html(&format_tool_summary(*status, detail));
                let body_html = render_tool_detail_html(detail);
                let _ = writeln!(
                    out,
                    "<details class=\"disclosure tool-disclosure\"><summary>{summary}</summary><div class=\"disclosure-body\">{body_html}</div></details>"
                );
            }
            TimelinePayload::AssistantMessage { text, .. } => {
                let body = text.trim();
                if body.is_empty() {
                    continue;
                }
                out.push_str("<section class=\"entry entry-assistant\">\n<div class=\"entry-head\"><span class=\"role-label\">Assistant</span>");
                if opts.include_timestamps {
                    let _ = write!(
                        out,
                        "<time class=\"entry-time\">{}</time>",
                        escape_html(&format_timestamp_human(ev.timestamp))
                    );
                }
                out.push_str("</div>\n");
                let _ = write!(
                    out,
                    "<div class=\"entry-body\">{}</div>\n</section>\n",
                    render_markdown_to_safe_html(body)
                );
            }
            TimelinePayload::AgentMessage {
                source_title, text, ..
            } => {
                let body = text.trim();
                if body.is_empty() {
                    continue;
                }
                let label = source_title
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| format!("Agent · {s}"))
                    .unwrap_or_else(|| "Agent Message".to_string());
                let _ = write!(
                    out,
                    "<section class=\"entry entry-agent\">\n<div class=\"entry-head\"><span class=\"role-label\">{}</span>",
                    escape_html(&label)
                );
                if opts.include_timestamps {
                    let _ = write!(
                        out,
                        "<time class=\"entry-time\">{}</time>",
                        escape_html(&format_timestamp_human(ev.timestamp))
                    );
                }
                let _ = write!(
                    out,
                    "</div>\n<div class=\"entry-body\">{}</div>\n</section>\n",
                    render_markdown_to_safe_html(body)
                );
            }
            TimelinePayload::Notice { level, message } => {
                let body = message.trim();
                if body.is_empty() {
                    continue;
                }
                let _ = writeln!(
                    out,
                    "<div class=\"notice-banner\"><strong>Notice ({}):</strong> {}</div>",
                    escape_html(level),
                    escape_html(body)
                );
            }
            TimelinePayload::PermissionRequest { .. }
            | TimelinePayload::PermissionResolved { .. }
            | TimelinePayload::QuestionRequest { .. }
            | TimelinePayload::QuestionResolved { .. }
            | TimelinePayload::StatusChanged { .. } => {}
        }
    }

    let _ = write!(
        out,
        "<footer class=\"doc-footer\">Exported from Orbit · {}</footer>\n</main>\n</body>\n</html>\n",
        escape_html(&meta.agent_id)
    );
    out
}

fn render_tool_detail_html(detail: &ToolCallDetail) -> String {
    match detail {
        ToolCallDetail::Shell {
            command,
            cwd,
            exit_code,
            output_preview,
        } => {
            let mut buf = String::new();
            if let Some(dir) = cwd.as_deref().filter(|s| !s.trim().is_empty()) {
                let _ = write!(buf, "<p>Directory: <code>{}</code></p>", escape_html(dir));
            }
            let _ = write!(
                buf,
                "<div class=\"code-wrap\"><div class=\"code-lang\">sh</div><pre><code>{}</code></pre></div>",
                escape_html(command)
            );
            if let Some(code) = exit_code {
                let _ = write!(buf, "<p>Exit code: <code>{code}</code></p>");
            }
            if let Some(out) = output_preview.as_deref().filter(|s| !s.trim().is_empty()) {
                let _ = write!(
                    buf,
                    "<div class=\"code-wrap\"><div class=\"code-lang\">output</div><pre><code>{}</code></pre></div>",
                    escape_html(out)
                );
            }
            buf
        }
        ToolCallDetail::FileRead {
            path,
            start_line,
            end_line,
        } => match (start_line, end_line) {
            (Some(s), Some(e)) => format!(
                "<p>Path: <code>{}</code> (lines {s}–{e})</p>",
                escape_html(path)
            ),
            (Some(s), None) => format!(
                "<p>Path: <code>{}</code> (from line {s})</p>",
                escape_html(path)
            ),
            _ => format!("<p>Path: <code>{}</code></p>", escape_html(path)),
        },
        ToolCallDetail::FileEdit { path, unified_diff } => {
            let mut buf = format!("<p>Path: <code>{}</code></p>", escape_html(path));
            if let Some(diff) = unified_diff.as_deref().filter(|s| !s.trim().is_empty()) {
                let _ = write!(
                    buf,
                    "<div class=\"code-wrap\"><div class=\"code-lang\">diff</div><pre><code>{}</code></pre></div>",
                    escape_html(diff)
                );
            }
            buf
        }
        ToolCallDetail::Search { query, scope } => {
            if let Some(sc) = scope.as_deref().filter(|s| !s.trim().is_empty()) {
                format!(
                    "<p>Query: <code>{}</code> in <code>{}</code></p>",
                    escape_html(query),
                    escape_html(sc)
                )
            } else {
                format!("<p>Query: <code>{}</code></p>", escape_html(query))
            }
        }
        ToolCallDetail::Subagent {
            child_agent_id,
            role,
            prompt_preview,
        } => {
            let mut buf = format!("<p>Role: <code>{}</code>", escape_html(role));
            if let Some(cid) = child_agent_id.as_deref().filter(|s| !s.trim().is_empty()) {
                let _ = write!(buf, " · Agent: <code>{}</code>", escape_html(cid));
            }
            buf.push_str("</p>");
            if !prompt_preview.trim().is_empty() {
                let _ = write!(buf, "<pre>{}</pre>", escape_html(prompt_preview));
            }
            buf
        }
        ToolCallDetail::Generic {
            tool_name,
            arguments_json,
        } => {
            let mut buf = format!("<p>Tool: <code>{}</code></p>", escape_html(tool_name));
            if !arguments_json.trim().is_empty() {
                let _ = write!(
                    buf,
                    "<div class=\"code-wrap\"><div class=\"code-lang\">json</div><pre><code>{}</code></pre></div>",
                    escape_html(arguments_json)
                );
            }
            buf
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use orbit_protocol::{AgentStatus, PromptBadge};

    fn fixed_ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_760_000_000 + secs, 0)
            .single()
            .expect("valid ts")
    }

    fn sample_meta() -> ExportAgentMeta {
        ExportAgentMeta {
            host_id: "host_devbox".into(),
            host_name: Some("devbox".into()),
            agent_id: "ag_sample123".into(),
            title: "修复导出 & 中文标题".into(),
            provider: Some(ProviderKind::ClaudeCode),
            model: Some("claude-sonnet-4".into()),
            cwd: Some("/workspace/orbit".into()),
            workspace_title: Some("Wave 2 Export".into()),
            task_summary: Some("Add MD/JSON/HTML transcript export".into()),
            created_at: Some(fixed_ts(0)),
            exported_at: Some(fixed_ts(600)),
        }
    }

    fn sample_events() -> Vec<TimelineEvent> {
        vec![
            TimelineEvent {
                seq: 1,
                epoch: 1,
                agent_id: "ag_sample123".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(10),
                payload: TimelinePayload::UserPrompt {
                    message_id: "m1".into(),
                    text: "请帮我写一个 Rust 示例并运行测试".into(),
                    badges: vec![PromptBadge {
                        kind: "directive".into(),
                        label: "/plan".into(),
                        path: None,
                    }],
                },
            },
            TimelineEvent {
                seq: 2,
                epoch: 1,
                agent_id: "ag_sample123".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(12),
                payload: TimelinePayload::AssistantThinking {
                    item_id: "th1".into(),
                    text: "Checking nested ```rust\nfn x() {}\n``` fence handling.".into(),
                },
            },
            TimelineEvent {
                seq: 3,
                epoch: 1,
                agent_id: "ag_sample123".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(15),
                payload: TimelinePayload::ToolCall {
                    call_id: "tc1".into(),
                    status: ToolCallStatus::Running,
                    detail: ToolCallDetail::Shell {
                        command: "cargo test".into(),
                        cwd: Some("/workspace/orbit".into()),
                        exit_code: None,
                        output_preview: None,
                    },
                    background_task: None,
                },
            },
            TimelineEvent {
                seq: 4,
                epoch: 1,
                agent_id: "ag_sample123".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(20),
                payload: TimelinePayload::ToolCall {
                    call_id: "tc1".into(),
                    status: ToolCallStatus::Completed,
                    detail: ToolCallDetail::Shell {
                        command: "cargo test".into(),
                        cwd: Some("/workspace/orbit".into()),
                        exit_code: Some(0),
                        output_preview: Some("test result: ok.\n```raw```".into()),
                    },
                    background_task: None,
                },
            },
            TimelineEvent {
                seq: 5,
                epoch: 1,
                agent_id: "ag_sample123".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(25),
                payload: TimelinePayload::AssistantMessage {
                    item_id: "am1".into(),
                    text: "Partial...".into(),
                    streaming: true,
                },
            },
            TimelineEvent {
                seq: 6,
                epoch: 1,
                agent_id: "ag_sample123".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(30),
                payload: TimelinePayload::AssistantMessage {
                    item_id: "am1".into(),
                    text: "这是最终回答：\n\n```rust\nfn main() {\n    println!(\"你好 Orbit\");\n}\n```".into(),
                    streaming: false,
                },
            },
            TimelineEvent {
                seq: 7,
                epoch: 1,
                agent_id: "ag_sample123".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(31),
                payload: TimelinePayload::StatusChanged {
                    status: AgentStatus::Idle,
                },
            },
        ]
    }

    #[test]
    fn markdown_export_handles_nested_fences_toggles_and_cjk() {
        let meta = sample_meta();
        let events = sample_events();

        let full = export_timeline_transcript(
            &meta,
            &events,
            &ExportOptions {
                format: ExportFormat::Markdown,
                include_thinking: true,
                include_tool_calls: true,
                include_timestamps: true,
            },
        );
        assert_eq!(full.turn_count, 1);
        assert_eq!(full.byte_len, full.content.len());
        assert_eq!(full.mime_type, "text/markdown; charset=utf-8");
        assert!(full.suggested_filename.ends_with(".md"));
        assert!(full.suggested_filename.contains("修复导出"));
        assert!(full.content.contains("# 修复导出 & 中文标题"));
        assert!(full.content.contains("- **Provider**: Claude Code"));
        assert!(full.content.contains("## User · `"));
        assert!(full.content.contains("<summary>Thinking</summary>"));
        // Thinking contains ``` so wrapper must escalate to at least 4 backticks
        assert!(full.content.contains("````text"));
        // Tool output contains ```raw``` so wrapper must escalate to 4 backticks
        assert!(
            full.content
                .contains("<summary>Tool: Shell (completed, exit 0)")
        );
        // Streaming chunk was coalesced — only final message remains
        assert!(!full.content.contains("Partial..."));
        assert!(full.content.contains("println!(\"你好 Orbit\")"));

        let minimal = export_timeline_transcript(
            &meta,
            &events,
            &ExportOptions {
                format: ExportFormat::Markdown,
                include_thinking: false,
                include_tool_calls: false,
                include_timestamps: false,
            },
        );
        assert!(!minimal.content.contains("<summary>Thinking</summary>"));
        assert!(!minimal.content.contains("<summary>Tool:"));
        assert!(!minimal.content.contains("- **Exported**:"));
        assert!(minimal.content.contains("## User\n\n"));
        assert!(minimal.content.contains("## Assistant\n\n"));
    }

    #[test]
    fn json_export_has_versioned_schema_and_respects_filters() {
        let meta = sample_meta();
        let events = sample_events();

        let exported = export_timeline_transcript(
            &meta,
            &events,
            &ExportOptions {
                format: ExportFormat::Json,
                include_thinking: false,
                include_tool_calls: true,
                include_timestamps: false,
            },
        );
        assert_eq!(exported.mime_type, "application/json; charset=utf-8");
        assert!(exported.suggested_filename.ends_with(".json"));

        let parsed: serde_json::Value =
            serde_json::from_str(&exported.content).expect("valid json");
        assert_eq!(parsed["schema"], TRANSCRIPT_JSON_SCHEMA);
        assert_eq!(parsed["agent"]["agent_id"], "ag_sample123");
        assert_eq!(parsed["agent"]["host_id"], "host_devbox");
        assert_eq!(parsed["turn_count"], 1);

        let arr = parsed["events"].as_array().expect("events array");
        // UserPrompt + coalesced ToolCall + coalesced AssistantMessage (Thinking excluded, StatusChanged excluded)
        assert_eq!(arr.len(), 3);
        assert!(
            arr.iter()
                .all(|e| e["payload"]["type"] != "assistant_thinking")
        );
        assert!(arr.iter().any(|e| e["payload"]["type"] == "tool_call"));
        assert!(arr.iter().all(|e| e.get("timestamp").is_none()));
    }

    #[test]
    fn html_export_is_self_contained_dark_mode_and_xss_safe() {
        let mut meta = sample_meta();
        meta.title = "Test <script>alert(\"title\")</script> & 'quotes'".into();
        meta.cwd = Some("/tmp/\"bad\"<tag>".into());

        let events = vec![
            TimelineEvent {
                seq: 1,
                epoch: 1,
                agent_id: meta.agent_id.clone(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(10),
                payload: TimelinePayload::UserPrompt {
                    message_id: "m1".into(),
                    text: "Check <script>alert('user')</script> and [evil](javascript:alert(1))"
                        .into(),
                    badges: vec![],
                },
            },
            TimelineEvent {
                seq: 2,
                epoch: 1,
                agent_id: meta.agent_id.clone(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(15),
                payload: TimelinePayload::ToolCall {
                    call_id: "c1".into(),
                    status: ToolCallStatus::Completed,
                    detail: ToolCallDetail::Shell {
                        command: "echo '<img src=x onerror=alert(1)>'".into(),
                        cwd: None,
                        exit_code: Some(0),
                        output_preview: Some("<script>evil()</script>".into()),
                    },
                    background_task: None,
                },
            },
            TimelineEvent {
                seq: 3,
                epoch: 1,
                agent_id: meta.agent_id.clone(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(20),
                payload: TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "Raw inline <b><script>xss()</script></b> and:\n\n```html\n<script>console.log(\"safe\")</script>\n```\n\nSafe [Orbit](https://orbit.dev)".into(),
                    streaming: false,
                },
            },
        ];

        let exported = export_timeline_transcript(
            &meta,
            &events,
            &ExportOptions {
                format: ExportFormat::Html,
                include_thinking: true,
                include_tool_calls: true,
                include_timestamps: true,
            },
        );
        assert_eq!(exported.mime_type, "text/html; charset=utf-8");
        assert!(exported.suggested_filename.ends_with(".html"));
        assert!(exported.content.starts_with("<!DOCTYPE html>"));
        assert!(
            exported
                .content
                .contains("@media (prefers-color-scheme: dark)")
        );
        // Zero unescaped script/img tags, zero javascript: hrefs, zero external CDN links in head
        assert!(!exported.content.contains("<script"));
        assert!(!exported.content.contains("<img"));
        assert!(!exported.content.contains("javascript:alert"));
        assert!(!exported.content.contains("cdn."));
        assert!(
            exported
                .content
                .contains("&lt;img src=x onerror=alert(1)&gt;")
        );
        assert!(
            exported
                .content
                .contains("&lt;script&gt;alert(&quot;title&quot;)&lt;/script&gt;")
        );
        assert!(exported.content.contains("&amp; &#39;quotes&#39;"));
        assert!(exported.content.contains("href=\"https://orbit.dev\""));
    }

    #[test]
    fn filename_sanitization_blocks_traversal_reserved_names_and_preserves_unicode() {
        assert_eq!(
            build_suggested_filename("../../etc/passwd", "ag_1", ExportFormat::Markdown),
            "etc-passwd.md"
        );
        assert_eq!(
            build_suggested_filename("CON", "ag_1", ExportFormat::Json),
            "orbit-con.json"
        );
        assert_eq!(
            build_suggested_filename("///...***???", "ag_fallback_9", ExportFormat::Html),
            "ag-fallback-9.html"
        );
        assert_eq!(
            build_suggested_filename("", "", ExportFormat::Markdown),
            "orbit-conversation.md"
        );
        let cjk =
            build_suggested_filename("重构 Rust 导出模块: v2/final", "ag_2", ExportFormat::Html);
        assert_eq!(cjk, "重构-rust-导出模块-v2-final.html");
        assert!(!cjk.contains('/'));
        assert!(!cjk.contains(':'));
    }

    #[test]
    fn code_block_extractor_handles_backticks_tildes_nested_and_unclosed() {
        let events = vec![
            TimelineEvent {
                seq: 1,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(1),
                payload: TimelinePayload::UserPrompt {
                    message_id: "u1".into(),
                    text: "Turn 1".into(),
                    badges: vec![],
                },
            },
            TimelineEvent {
                seq: 2,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(2),
                payload: TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "First:\n````markdown\n```rust\nfn nested() {}\n```\n````\n\nSecond:\n~~~python\nprint(42)\n~~~".into(),
                    streaming: false,
                },
            },
            TimelineEvent {
                seq: 3,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: Some("t2".into()),
                timestamp: fixed_ts(3),
                payload: TimelinePayload::UserPrompt {
                    message_id: "u2".into(),
                    text: "Turn 2".into(),
                    badges: vec![],
                },
            },
            TimelineEvent {
                seq: 4,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: Some("t2".into()),
                timestamp: fixed_ts(4),
                payload: TimelinePayload::AssistantMessage {
                    item_id: "a2".into(),
                    text: "Unclosed:\n```ts\nconst x = 1;\nconst y = 2;".into(),
                    streaming: false,
                },
            },
        ];

        let blocks = extract_code_blocks_from_timeline(&events);
        assert_eq!(blocks.len(), 3);

        // Reverse-chronological: latest block first (index 0)
        assert_eq!(blocks[0].index, 0);
        assert_eq!(blocks[0].turn_index, 2);
        assert_eq!(blocks[0].language, "ts");
        assert_eq!(blocks[0].line_count, 2);
        assert_eq!(blocks[0].preview, "const x = 1;");
        assert_eq!(blocks[0].code, "const x = 1;\nconst y = 2;");

        assert_eq!(blocks[1].index, 1);
        assert_eq!(blocks[1].turn_index, 1);
        assert_eq!(blocks[1].language, "python");
        assert_eq!(blocks[1].line_count, 1);
        assert_eq!(blocks[1].code, "print(42)");

        assert_eq!(blocks[2].index, 2);
        assert_eq!(blocks[2].turn_index, 1);
        assert_eq!(blocks[2].language, "markdown");
        assert!(blocks[2].code.contains("```rust"));
    }

    #[test]
    fn app_core_exports_full_persisted_replica_and_code_blocks_multi_host() {
        let dir =
            std::env::temp_dir().join(format!("orbit-export-core-{}", Uuid::new_v4().simple()));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("client.sqlite");
        let store = orbit_store::OrbitStore::open(&db_path).expect("open store");
        let now = fixed_ts(0);

        let agent = orbit_protocol::AgentSummary {
            agent_id: "ag_remote_42".into(),
            workspace_id: "ws_1".into(),
            cwd: "/srv/project".into(),
            title: "Remote Fleet Migration".into(),
            provider: ProviderKind::Codex,
            model: Some("gpt-5-codex".into()),
            effort: None,
            mode: orbit_protocol::SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 2,
            timeline_epoch: 1,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: std::collections::HashMap::new(),
            attention: orbit_protocol::AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: vec![],
            read_seq: 2,
            is_pinned: false,
            pinned_order: 0,
            handle: Some("migration".into()),
            cluster_suggestion: None,
            task_summary: Some("Migrated multi-host schema".into()),
            forked_from_agent_id: None,
            forked_at_seq: None,
        };
        store
            .upsert_single_replica_agent("host_workstation", &agent)
            .expect("upsert replica agent");
        store
            .upsert_replica_timeline_events_batch_for_host(
                "host_workstation",
                &[
                    TimelineEvent {
                        seq: 1,
                        epoch: 1,
                        agent_id: "ag_remote_42".into(),
                        turn_id: Some("t1".into()),
                        timestamp: fixed_ts(10),
                        payload: TimelinePayload::UserPrompt {
                            message_id: "u1".into(),
                            text: "Show the migration SQL".into(),
                            badges: vec![],
                        },
                    },
                    TimelineEvent {
                        seq: 2,
                        epoch: 1,
                        agent_id: "ag_remote_42".into(),
                        turn_id: Some("t1".into()),
                        timestamp: fixed_ts(20),
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "a1".into(),
                            text: "Here is the migration:\n\n```sql\nALTER TABLE hosts ADD COLUMN region TEXT;\n```".into(),
                            streaming: false,
                        },
                    },
                ],
            )
            .expect("upsert events");
        drop(store);

        let core = crate::app_core::OrbitAppCore::open(&db_path, false).expect("open core");
        let exported = core
            .export_agent_transcript(
                "host_workstation",
                "ag_remote_42",
                &ExportOptions {
                    format: ExportFormat::Markdown,
                    include_thinking: true,
                    include_tool_calls: true,
                    include_timestamps: true,
                },
            )
            .expect("export from replica");
        assert_eq!(exported.turn_count, 1);
        assert_eq!(exported.suggested_filename, "remote-fleet-migration.md");
        assert!(exported.content.contains("# Remote Fleet Migration"));
        assert!(
            exported
                .content
                .contains("ALTER TABLE hosts ADD COLUMN region TEXT;")
        );

        let blocks = core
            .list_agent_code_blocks("host_workstation", "ag_remote_42")
            .expect("list code blocks");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "sql");
        assert_eq!(blocks[0].code, "ALTER TABLE hosts ADD COLUMN region TEXT;");

        // Unknown host/agent returns a clear error instead of panicking
        assert!(
            core.export_agent_transcript("host_other", "ag_missing", &ExportOptions::default())
                .is_err()
        );
        // P0 #2 regression: strict multi-host isolation — `host_other` must NEVER export
        // `ag_remote_42` owned by `host_workstation` even though `client_replica_events` is keyed by `agent_id`.
        assert!(
            core.export_agent_transcript("host_other", "ag_remote_42", &ExportOptions::default())
                .is_err()
        );
        assert!(
            core.list_agent_code_blocks("host_other", "ag_remote_42")
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn html_href_sanitization_and_nested_images_are_safe() {
        assert!(!is_safe_href("//evil.example.com/phish"));
        assert!(!is_safe_href("/\\evil.example.com/phish"));
        assert!(!is_safe_href("\\\\evil\\share"));
        assert!(!is_safe_href("javascript:alert(1)"));
        assert!(!is_safe_href("https://example.com/\nfoo"));
        assert!(!is_safe_href("https://example.com/\x00bar"));
        assert!(is_safe_href("https://orbit.dev/docs"));
        assert!(is_safe_href("http://localhost:3000/status"));
        assert!(is_safe_href("mailto:team@orbit.dev"));
        assert!(is_safe_href("#section-1"));
        assert!(is_safe_href("/relative/path"));

        let md = "Links: [proto](//evil.com) [slash](/\\evil.com) [good](  https://orbit.dev  )\n\nNested img: ![outer ![inner](https://a.test/i.png)\nline2](https://a.test/o.png) after";
        let rendered = render_markdown_to_safe_html(md);
        assert!(!rendered.contains("href=\"//evil.com\""));
        assert!(!rendered.contains("href=\"/\\evil.com\""));
        assert!(rendered.contains("href=\"https://orbit.dev\""));
        assert!(rendered.contains("<span class=\"img-ref\">[outer inner line2]</span>"));
        assert!(rendered.contains("after</p>"));
    }

    #[test]
    fn write_file_atomically_rejects_invalid_paths_and_sets_private_mode() {
        assert!(write_file_atomically(Path::new(""), b"hello").is_err());
        let dir = std::env::temp_dir().join(format!("orbit-atomic-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        assert!(write_file_atomically(&dir, b"hello").is_err());

        let target = dir.join("export.md");
        write_file_atomically(&target, b"# Private Transcript\n").expect("atomic write");
        assert_eq!(
            std::fs::read_to_string(&target).expect("read"),
            "# Private Transcript\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn code_block_extractor_handles_list_and_blockquote_indented_fences() {
        let events = vec![
            TimelineEvent {
                seq: 1,
                epoch: 1,
                agent_id: "ag_indented".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(1),
                payload: TimelinePayload::UserPrompt {
                    message_id: "u1".into(),
                    text: "Give steps".into(),
                    badges: vec![],
                },
            },
            TimelineEvent {
                seq: 2,
                epoch: 1,
                agent_id: "ag_indented".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(2),
                payload: TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "- Step 1:\n\n  ```rust\n  fn inside_list() {\n      println!(\"hi\");\n  }\n  ```\n\n> Quote:\n> ```bash\n> echo \"quoted\"\n> ```".into(),
                    streaming: false,
                },
            },
        ];

        let blocks = extract_code_blocks_from_timeline(&events);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].language, "bash");
        assert_eq!(blocks[0].code, "echo \"quoted\"");
        assert_eq!(blocks[1].language, "rust");
        assert_eq!(
            blocks[1].code,
            "fn inside_list() {\n    println!(\"hi\");\n}"
        );
    }

    #[test]
    fn inline_code_multiline_notice_and_format_from_str() {
        assert_eq!(wrap_inline_code("plain"), "`plain`");
        assert_eq!(wrap_inline_code("a`b"), "``a`b``");
        assert_eq!(wrap_inline_code("`leading`"), "`` `leading` ``");
        assert_eq!(wrap_inline_code("line1\nline2"), "`line1 line2`");

        assert_eq!(
            "md".parse::<ExportFormat>().expect("md"),
            ExportFormat::Markdown
        );
        assert_eq!(
            "json".parse::<ExportFormat>().expect("json"),
            ExportFormat::Json
        );
        assert_eq!(
            "HTML".parse::<ExportFormat>().expect("html"),
            ExportFormat::Html
        );
        assert!("pdf".parse::<ExportFormat>().is_err());

        let mut meta = sample_meta();
        meta.title = "Line1\nLine2".into();
        let events = vec![TimelineEvent {
            seq: 1,
            epoch: 1,
            agent_id: meta.agent_id.clone(),
            turn_id: None,
            timestamp: fixed_ts(1),
            payload: TimelinePayload::Notice {
                level: "warn".into(),
                message: "First notice line\nSecond notice line".into(),
            },
        }];
        let md = export_timeline_transcript(&meta, &events, &ExportOptions::default()).content;
        assert!(md.contains("# Line1 Line2\n"));
        assert!(md.contains("> **Notice (warn)**: First notice line\n> Second notice line\n"));
    }

    #[test]
    fn merge_persisted_and_memory_preserves_middle_gaps_and_dedups_optimistic_prompts() {
        let persisted = vec![
            TimelineEvent {
                seq: 1,
                epoch: 2,
                agent_id: "ag_1".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(1),
                payload: TimelinePayload::UserPrompt {
                    message_id: "u1".into(),
                    text: "Persisted question".into(),
                    badges: vec![],
                },
            },
            TimelineEvent {
                seq: 3,
                epoch: 2,
                agent_id: "ag_1".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(3),
                payload: TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "Final answer".into(),
                    streaming: false,
                },
            },
        ];
        let mem = vec![
            // Stale pre-rewind epoch 1 event should be ignored
            TimelineEvent {
                seq: 99,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: None,
                timestamp: fixed_ts(0),
                payload: TimelinePayload::Notice {
                    level: "info".into(),
                    message: "stale".into(),
                },
            },
            // Unpersisted middle seq 2 must be preserved and sorted between 1 and 3
            TimelineEvent {
                seq: 2,
                epoch: 2,
                agent_id: "ag_1".into(),
                turn_id: Some("t1".into()),
                timestamp: fixed_ts(2),
                payload: TimelinePayload::AssistantThinking {
                    item_id: "th1".into(),
                    text: "Middle thought".into(),
                },
            },
            // Optimistic duplicate of seq 1 prompt must be skipped
            TimelineEvent {
                seq: 0,
                epoch: 2,
                agent_id: "ag_1".into(),
                turn_id: None,
                timestamp: fixed_ts(4),
                payload: TimelinePayload::UserPrompt {
                    message_id: "opt_dup".into(),
                    text: "Persisted question".into(),
                    badges: vec![],
                },
            },
            // Fresh optimistic prompt must be preserved at tail
            TimelineEvent {
                seq: 0,
                epoch: 2,
                agent_id: "ag_1".into(),
                turn_id: None,
                timestamp: fixed_ts(5),
                payload: TimelinePayload::UserPrompt {
                    message_id: "opt_new".into(),
                    text: "Brand new follow-up".into(),
                    badges: vec![],
                },
            },
        ];

        let merged = crate::app_core::merge_persisted_and_memory_timeline(persisted, mem);
        let seqs: Vec<u64> = merged.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![1, 2, 3, 0]);
        assert!(matches!(
            &merged[3].payload,
            TimelinePayload::UserPrompt { text, .. } if text == "Brand new follow-up"
        ));
    }
}
