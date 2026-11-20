//! Unified Composer domain engine shared across Desktop, iOS, Android, and HarmonyOS:
//! - Canonical slash command catalog & fuzzy filtering
//! - `/plan`, `/review`, `/compact`, `/btw` directive parsing & workflow instruction assembly
//! - User-facing prompt badges + structured agent wire prompt composition
//! - `@` workspace file & context mention fuzzy ranking

use std::collections::{HashMap, HashSet};

use orbit_protocol::{PromptBadge, ProviderKind};
use serde::{Deserialize, Serialize};

use crate::stage::basename_of_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlashDirective {
    Plan,
    Review,
    Compact,
}

impl SlashDirective {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Review => "review",
            Self::Compact => "compact",
        }
    }

    pub fn badge_label(self) -> &'static str {
        match self {
            Self::Plan => "/plan",
            Self::Review => "/review",
            Self::Compact => "/compact",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientPlatform {
    #[default]
    Desktop,
    Ios,
    Android,
    Harmony,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashCommandView {
    pub id: String,
    pub command: String,
    pub label: String,
    pub description: String,
    pub shortcut_key: Option<String>,
    pub icon_key: String,
    pub keywords: Vec<String>,
    pub desktop_only: bool,
}

pub fn slash_commands(platform: ClientPlatform) -> Vec<SlashCommandView> {
    let all = vec![
        SlashCommandView {
            id: "btw".into(),
            command: "/btw".into(),
            label: "Ask a Side Question".into(),
            description: "Read-only Q&A without interrupting or altering main chat".into(),
            shortcut_key: Some(";".into()),
            icon_key: "question".into(),
            keywords: vec![
                "btw".into(),
                "side".into(),
                "question".into(),
                "ask".into(),
                "quick".into(),
            ],
            desktop_only: false,
        },
        SlashCommandView {
            id: "plan".into(),
            command: "/plan".into(),
            label: "Draft Implementation Plan".into(),
            description: "Prompt the agent to design a step-by-step plan before editing".into(),
            shortcut_key: None,
            icon_key: "plan".into(),
            keywords: vec![
                "plan".into(),
                "design".into(),
                "architect".into(),
                "checklist".into(),
            ],
            desktop_only: false,
        },
        SlashCommandView {
            id: "review".into(),
            command: "/review".into(),
            label: "Code Review Working Tree".into(),
            description: "Attach uncommitted git diff and prepare an engineering review".into(),
            shortcut_key: None,
            icon_key: "review".into(),
            keywords: vec![
                "review".into(),
                "audit".into(),
                "critique".into(),
                "git".into(),
            ],
            desktop_only: false,
        },
        SlashCommandView {
            id: "diff".into(),
            command: "/diff".into(),
            label: "Toggle Diff Review".into(),
            description: "Open or focus the Git & Turn Diff Review inspector".into(),
            shortcut_key: Some("D".into()),
            icon_key: "diff".into(),
            keywords: vec!["diff".into(), "inspector".into(), "changes".into()],
            desktop_only: false,
        },
        SlashCommandView {
            id: "files".into(),
            command: "/files".into(),
            label: "Browse Workspace Files".into(),
            description: "Open the workspace file explorer in Side Panel".into(),
            shortcut_key: Some("P".into()),
            icon_key: "files".into(),
            keywords: vec![
                "files".into(),
                "open".into(),
                "explorer".into(),
                "tree".into(),
                "code".into(),
            ],
            desktop_only: true,
        },
        SlashCommandView {
            id: "browser".into(),
            command: "/browser".into(),
            label: "Open Preview Browser".into(),
            description: "Open the built-in web & dev server preview tab".into(),
            shortcut_key: None,
            icon_key: "browser".into(),
            keywords: vec![
                "browser".into(),
                "preview".into(),
                "web".into(),
                "localhost".into(),
                "url".into(),
            ],
            desktop_only: true,
        },
        SlashCommandView {
            id: "compact".into(),
            command: "/compact".into(),
            label: "Compact Context".into(),
            description: "Ask active agent to summarize and compact conversation history".into(),
            shortcut_key: None,
            icon_key: "compact".into(),
            keywords: vec![
                "compact".into(),
                "summarize".into(),
                "context".into(),
                "memory".into(),
            ],
            desktop_only: false,
        },
        SlashCommandView {
            id: "supervised".into(),
            command: "/ask".into(),
            label: "Always Ask Permissions".into(),
            description: "Require approval before file edits and shell commands".into(),
            shortcut_key: None,
            icon_key: "shield".into(),
            keywords: vec![
                "ask".into(),
                "supervised".into(),
                "default".into(),
                "safe".into(),
                "permission".into(),
            ],
            desktop_only: false,
        },
        SlashCommandView {
            id: "auto".into(),
            command: "/edits".into(),
            label: "Accept Edits Permissions".into(),
            description: "Auto-approve file edits, ask before shell commands".into(),
            shortcut_key: None,
            icon_key: "shield_check".into(),
            keywords: vec![
                "edits".into(),
                "auto".into(),
                "edit".into(),
                "accept".into(),
                "permission".into(),
            ],
            desktop_only: false,
        },
        SlashCommandView {
            id: "full".into(),
            command: "/full".into(),
            label: "Full Access Permissions".into(),
            description: "Edit files and run commands without approval prompts".into(),
            shortcut_key: None,
            icon_key: "shield_off".into(),
            keywords: vec![
                "full".into(),
                "autonomous".into(),
                "bypass".into(),
                "unrestricted".into(),
                "permission".into(),
            ],
            desktop_only: false,
        },
        SlashCommandView {
            id: "terminal".into(),
            command: "/terminal".into(),
            label: "Toggle PTY Terminal".into(),
            description: "Show or hide the persistent multi-slot terminal".into(),
            shortcut_key: Some("J".into()),
            icon_key: "terminal".into(),
            keywords: vec![
                "terminal".into(),
                "pty".into(),
                "shell".into(),
                "console".into(),
            ],
            desktop_only: false,
        },
        SlashCommandView {
            id: "new".into(),
            command: "/new".into(),
            label: "New Agent Session".into(),
            description: "Start a fresh isolated session in the stage".into(),
            shortcut_key: Some("N".into()),
            icon_key: "new".into(),
            keywords: vec![
                "new".into(),
                "thread".into(),
                "session".into(),
                "clear".into(),
            ],
            desktop_only: false,
        },
        SlashCommandView {
            id: "settings".into(),
            command: "/settings".into(),
            label: "Settings".into(),
            description: "Configure hosts, engines, worktrees, and preferences".into(),
            shortcut_key: Some(",".into()),
            icon_key: "settings".into(),
            keywords: vec!["settings".into(), "preferences".into(), "config".into()],
            desktop_only: false,
        },
    ];

    if platform == ClientPlatform::Desktop {
        all
    } else {
        all.into_iter().filter(|c| !c.desktop_only).collect()
    }
}

pub fn filter_slash_commands(query: &str, platform: ClientPlatform) -> Vec<SlashCommandView> {
    let all = slash_commands(platform);
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return all;
    }
    all.into_iter()
        .filter(|c| {
            let cmd_slug = c.command.trim_start_matches('/').to_ascii_lowercase();
            cmd_slug.contains(&q)
                || c.id.to_ascii_lowercase().contains(&q)
                || c.label.to_ascii_lowercase().contains(&q)
                || c.keywords
                    .iter()
                    .any(|k| k.to_ascii_lowercase().contains(&q))
        })
        .collect()
}

pub fn parse_leading_slash_directive(raw_text: &str) -> (Option<SlashDirective>, String) {
    let trimmed = raw_text.trim();
    if !trimmed.starts_with('/') {
        return (None, trimmed.to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    for (prefix, dir) in [
        ("/plan", SlashDirective::Plan),
        ("/review", SlashDirective::Review),
        ("/compact", SlashDirective::Compact),
    ] {
        if lower == prefix {
            return (Some(dir), String::new());
        }
        if let Some(after) = trimmed.get(prefix.len()..)
            && lower.starts_with(prefix)
            && after.starts_with(char::is_whitespace)
        {
            return (Some(dir), after.trim().to_string());
        }
    }
    (None, trimmed.to_string())
}

pub fn parse_leading_btw_question(raw_text: &str) -> Option<String> {
    let trimmed = raw_text.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower == "/btw" {
        return Some(String::new());
    }
    if lower.starts_with("/btw")
        && let Some(after) = trimmed.get(4..)
        && after.starts_with(char::is_whitespace)
    {
        return Some(after.trim().to_string());
    }
    None
}

pub fn build_workflow_instruction(directive: SlashDirective) -> &'static str {
    match directive {
        SlashDirective::Plan => concat!(
            "<WORKFLOW_INSTRUCTION command=\"/plan\">\n",
            "You are invoked with the /plan workflow.\n",
            "First inspect and verify the relevant codebase files, architecture, and constraints.\n",
            "Clarify any critical ambiguities if needed, then present a concise, structured, step-by-step implementation plan.\n",
            "Do NOT modify files or run mutating commands until the user approves the plan.\n",
            "</WORKFLOW_INSTRUCTION>"
        ),
        SlashDirective::Review => concat!(
            "<WORKFLOW_INSTRUCTION command=\"/review\">\n",
            "You are invoked with the /review workflow.\n",
            "Perform a thorough engineering code review of the attached changes.\n",
            "Focus on correctness bugs, regressions, edge cases, concurrency/state hazards, security, and adherence to project conventions.\n",
            "Cite exact file paths and line numbers, prioritize findings by severity, and do NOT modify files unless asked.\n",
            "</WORKFLOW_INSTRUCTION>"
        ),
        SlashDirective::Compact => concat!(
            "<WORKFLOW_INSTRUCTION command=\"/compact\">\n",
            "Summarize and compact our conversation context into a concise, high-signal checkpoint:\n",
            "1. Core goal & user preferences\n",
            "2. Key architectural decisions & completed changes (with file paths)\n",
            "3. Open issues & immediate next steps\n",
            "</WORKFLOW_INSTRUCTION>"
        ),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComposerContextInput {
    Image {
        path: String,
        filename: String,
    },
    StagedFile {
        path: String,
        filename: String,
    },
    WorkspaceFile {
        path: String,
    },
    GitDiff {
        #[serde(default)]
        staged: bool,
        label: String,
        #[serde(default)]
        diff_text: Option<String>,
    },
    TerminalOutput {
        label: String,
        content: String,
    },
    AssistantQuote {
        label: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposerQuoteInput {
    pub path: String,
    #[serde(default)]
    pub line_number: Option<u32>,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposerPasteInput {
    pub label: String,
    #[serde(default)]
    pub filename: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposePromptRequest {
    pub provider: ProviderKind,
    #[serde(default)]
    pub raw_text: String,
    #[serde(default)]
    pub explicit_directive: Option<SlashDirective>,
    #[serde(default)]
    pub host_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub contexts: Vec<ComposerContextInput>,
    #[serde(default)]
    pub quotes: Vec<ComposerQuoteInput>,
    #[serde(default)]
    pub pastes: Vec<ComposerPasteInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposedPromptView {
    pub wire_prompt: String,
    pub display_prompt: String,
    pub badges: Vec<PromptBadge>,
    pub effective_directive: Option<SlashDirective>,
    pub is_compact: bool,
    pub is_btw: bool,
    pub btw_question: Option<String>,
    pub is_empty: bool,
}

fn clip_utf8_prefix(raw: &str, max_bytes: usize) -> &str {
    if raw.len() <= max_bytes {
        return raw;
    }
    let mut end = max_bytes;
    while end > 0 && !raw.is_char_boundary(end) {
        end -= 1;
    }
    &raw[..end]
}

pub fn compose_prompt(
    req: &ComposePromptRequest,
    fallback_working_diff: Option<&str>,
    fallback_staged_diff: Option<&str>,
) -> ComposedPromptView {
    let raw_trimmed = req.raw_text.trim();
    if req.explicit_directive.is_none()
        && let Some(btw_q) = parse_leading_btw_question(raw_trimmed)
    {
        return ComposedPromptView {
            wire_prompt: String::new(),
            display_prompt: String::new(),
            badges: Vec::new(),
            effective_directive: None,
            is_compact: false,
            is_btw: true,
            btw_question: Some(btw_q),
            is_empty: false,
        };
    }

    let (parsed_dir, clean_user_text) = if req.explicit_directive.is_some() {
        (None, raw_trimmed.to_string())
    } else {
        parse_leading_slash_directive(raw_trimmed)
    };
    let effective_directive = req.explicit_directive.or(parsed_dir);

    if effective_directive == Some(SlashDirective::Compact) {
        let badges = vec![PromptBadge {
            kind: "slash".into(),
            label: "/compact".into(),
            path: None,
        }];
        let wire_prompt = match req.provider {
            ProviderKind::ClaudeCode | ProviderKind::Antigravity => {
                if clean_user_text.is_empty() {
                    "/compact".to_string()
                } else {
                    format!("/compact {clean_user_text}")
                }
            }
            ProviderKind::Codex => {
                let instr = build_workflow_instruction(SlashDirective::Compact);
                if clean_user_text.is_empty() {
                    instr.to_string()
                } else {
                    format!("{instr}\n\n<USER_REQUEST>\n{clean_user_text}\n</USER_REQUEST>")
                }
            }
        };
        return ComposedPromptView {
            wire_prompt,
            display_prompt: clean_user_text,
            badges,
            effective_directive,
            is_compact: true,
            is_btw: false,
            btw_question: None,
            is_empty: false,
        };
    }

    let mut contexts = req.contexts.clone();
    if effective_directive == Some(SlashDirective::Review)
        && !contexts
            .iter()
            .any(|c| matches!(c, ComposerContextInput::GitDiff { .. }))
    {
        contexts.push(ComposerContextInput::GitDiff {
            staged: false,
            label: "Working Tree Diff".into(),
            diff_text: None,
        });
    }

    let has_substantive = !clean_user_text.is_empty()
        || !contexts.is_empty()
        || !req.quotes.is_empty()
        || !req.pastes.is_empty();
    if !has_substantive && effective_directive != Some(SlashDirective::Review) {
        return ComposedPromptView {
            wire_prompt: String::new(),
            display_prompt: String::new(),
            badges: Vec::new(),
            effective_directive,
            is_compact: false,
            is_btw: false,
            btw_question: None,
            is_empty: true,
        };
    }

    let mut badges: Vec<PromptBadge> = Vec::new();
    if let Some(dir) = effective_directive {
        badges.push(PromptBadge {
            kind: "slash".into(),
            label: dir.badge_label().into(),
            path: None,
        });
    }

    let mut image_paths: Vec<String> = Vec::new();
    let mut staged_file_paths: Vec<String> = Vec::new();
    let mut workspace_file_paths: Vec<String> = Vec::new();
    let mut context_blocks: Vec<String> = Vec::new();

    for ctx in &contexts {
        match ctx {
            ComposerContextInput::Image { path, filename } => {
                let clean_path = path.trim();
                badges.push(PromptBadge {
                    kind: "image".into(),
                    label: filename.clone(),
                    path: (!clean_path.is_empty()).then(|| clean_path.to_string()),
                });
                image_paths.push(path.clone());
            }
            ComposerContextInput::StagedFile { path, filename } => {
                let lbl = if filename.trim().is_empty() {
                    basename_of_path(path)
                } else {
                    filename.clone()
                };
                let clean_path = path.trim();
                badges.push(PromptBadge {
                    kind: "file".into(),
                    label: lbl,
                    path: (!clean_path.is_empty()).then(|| clean_path.to_string()),
                });
                staged_file_paths.push(path.clone());
            }
            ComposerContextInput::WorkspaceFile { path } => {
                let base = basename_of_path(path);
                let clean_path = path.trim();
                if !clean_user_text.contains(&format!("@{base}"))
                    && !clean_user_text.contains(&format!("@{path}"))
                {
                    badges.push(PromptBadge {
                        kind: "file".into(),
                        label: base,
                        path: (!clean_path.is_empty()).then(|| clean_path.to_string()),
                    });
                }
                workspace_file_paths.push(path.clone());
            }
            ComposerContextInput::GitDiff {
                staged,
                label,
                diff_text,
            } => {
                let alias = if *staged {
                    "@staged-diff"
                } else {
                    "@working-diff"
                };
                if !clean_user_text.contains(alias)
                    && effective_directive != Some(SlashDirective::Review)
                {
                    badges.push(PromptBadge {
                        kind: "git_diff".into(),
                        label: label.clone(),
                        path: None,
                    });
                }
                let raw_diff = diff_text
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .or(if *staged {
                        fallback_staged_diff
                    } else {
                        fallback_working_diff
                    })
                    .unwrap_or("");
                if !raw_diff.trim().is_empty() {
                    let clipped = clip_utf8_prefix(raw_diff, 14_000);
                    context_blocks.push(format!("{label}:\n```diff\n{clipped}\n```"));
                } else {
                    let scope = if *staged { "staged" } else { "working tree" };
                    context_blocks.push(format!(
                        "Context: Inspect {scope} git changes in workspace."
                    ));
                }
            }
            ComposerContextInput::TerminalOutput { label, content } => {
                if !clean_user_text.contains("@terminal") {
                    badges.push(PromptBadge {
                        kind: "terminal".into(),
                        label: label.clone(),
                        path: None,
                    });
                }
                context_blocks.push(format!("{label}:\n```text\n{content}\n```"));
            }
            ComposerContextInput::AssistantQuote { label, content } => {
                badges.push(PromptBadge {
                    kind: "quote".into(),
                    label: label.clone(),
                    path: None,
                });
                context_blocks.push(format!("{label}:\n```text\n{content}\n```"));
            }
        }
    }

    let mut ordered_blocks: Vec<String> = Vec::new();
    if !image_paths.is_empty() {
        ordered_blocks.push(
            image_paths
                .iter()
                .map(|p| format!("Attached image file: `{p}`"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !staged_file_paths.is_empty() {
        ordered_blocks.push(
            staged_file_paths
                .iter()
                .map(|p| format!("Attached file: `{p}`"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !workspace_file_paths.is_empty() {
        let joined = workspace_file_paths
            .iter()
            .map(|p| format!("`@{p}`"))
            .collect::<Vec<_>>()
            .join(", ");
        ordered_blocks.push(format!("Referenced workspace files: {joined}"));
    }
    ordered_blocks.extend(context_blocks);

    for q in &req.quotes {
        let short = basename_of_path(&q.path);
        let badge_lbl = match q.line_number {
            Some(line) => format!("{short}:{line}"),
            None => short,
        };
        let loc = match q.line_number {
            Some(line) => format!("{}:{line}", q.path),
            None => q.path.clone(),
        };
        badges.push(PromptBadge {
            kind: "quote".into(),
            label: badge_lbl,
            path: Some(loc.clone()),
        });
        ordered_blocks.push(format!("Reference `{loc}`:\n```\n{}\n```", q.code));
    }

    for p in &req.pastes {
        let badge_lbl = p
            .filename
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&p.label)
            .to_string();
        badges.push(PromptBadge {
            kind: "paste".into(),
            label: badge_lbl,
            path: None,
        });
        ordered_blocks.push(format!("{}:\n```text\n{}\n```", p.label, p.content));
    }

    let mut wire_sections: Vec<String> = Vec::new();
    if let Some(dir) = effective_directive {
        wire_sections.push(build_workflow_instruction(dir).to_string());
    }
    if !ordered_blocks.is_empty() {
        wire_sections.push(format!(
            "<ATTACHED_CONTEXT>\n{}\n</ATTACHED_CONTEXT>",
            ordered_blocks.join("\n\n")
        ));
    }

    if !clean_user_text.is_empty() {
        if wire_sections.is_empty() {
            wire_sections.push(clean_user_text.clone());
        } else {
            wire_sections.push(format!(
                "<USER_REQUEST>\n{clean_user_text}\n</USER_REQUEST>"
            ));
        }
    } else if effective_directive == Some(SlashDirective::Review) {
        wire_sections.push(
            "<USER_REQUEST>\nReview the attached workspace changes thoroughly.\n</USER_REQUEST>"
                .to_string(),
        );
    }

    ComposedPromptView {
        wire_prompt: wire_sections.join("\n\n"),
        display_prompt: clean_user_text,
        badges,
        effective_directive,
        is_compact: false,
        is_btw: false,
        btw_question: None,
        is_empty: false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MentionCandidateView {
    pub key: String,
    pub kind: String,
    pub token_label: String,
    pub title: String,
    pub subtitle: String,
    pub file_path: String,
    pub git_status: String,
    pub score: i32,
}

fn score_file_match(full_lower: &str, base_lower: &str, q: &str) -> i32 {
    if q.is_empty() {
        return 10;
    }
    if base_lower == q {
        return 125;
    }
    let stem_lower = base_lower
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(base_lower);
    if stem_lower == q {
        return 115;
    }
    if base_lower.starts_with(q) {
        return 100;
    }
    if base_lower.contains(q) {
        return 80;
    }
    if full_lower.contains(q) {
        return 60;
    }
    let q_bytes = q.as_bytes();
    let mut qi = 0usize;
    for &b in full_lower.as_bytes() {
        if qi < q_bytes.len() && b == q_bytes[qi] {
            qi += 1;
        }
    }
    if qi == q_bytes.len() { 36 } else { -1 }
}

pub fn rank_mention_candidates(
    query: &str,
    workspace_files: &[String],
    git_files: &[(String, String)],
    limit: usize,
) -> Vec<MentionCandidateView> {
    let max_items = if limit == 0 { 40 } else { limit.min(200) };
    let q = query.trim().to_ascii_lowercase();
    let mut out: Vec<MentionCandidateView> = Vec::new();

    for (key, kind, token, subtitle, base_order) in [
        (
            "builtin_working_diff",
            "git_diff",
            "@working-diff",
            "Working Tree Diff",
            980i32,
        ),
        (
            "builtin_staged_diff",
            "staged",
            "@staged-diff",
            "Staged Diff",
            970i32,
        ),
        (
            "builtin_terminal",
            "terminal",
            "@terminal",
            "Active Terminal Output",
            960i32,
        ),
    ] {
        let token_slug = token.trim_start_matches('@');
        let matched_score = if q.is_empty() {
            base_order
        } else if token_slug.starts_with(&q) {
            base_order + 20
        } else if token_slug.contains(&q) || subtitle.to_ascii_lowercase().contains(&q) {
            base_order - 20
        } else {
            -1
        };
        if matched_score >= 0 {
            out.push(MentionCandidateView {
                key: key.into(),
                kind: kind.into(),
                token_label: token.into(),
                title: token.into(),
                subtitle: subtitle.into(),
                file_path: String::new(),
                git_status: String::new(),
                score: matched_score,
            });
        }
    }

    let git_status_map: HashMap<&str, &str> = git_files
        .iter()
        .map(|(p, st)| (p.as_str(), st.as_str()))
        .collect();

    let mut seen_paths: HashSet<&str> = HashSet::new();
    let mut scored_files: Vec<MentionCandidateView> = Vec::new();

    for raw_path in workspace_files
        .iter()
        .chain(git_files.iter().map(|(p, _)| p))
    {
        let clean = raw_path.trim();
        if clean.is_empty() || !seen_paths.insert(clean) {
            continue;
        }
        let base = basename_of_path(clean);
        let full_lower = clean.to_ascii_lowercase();
        let base_lower = base.to_ascii_lowercase();
        let mut score = score_file_match(&full_lower, &base_lower, &q);
        if score < 0 {
            continue;
        }
        let git_status = git_status_map.get(clean).copied().unwrap_or("");
        if !git_status.is_empty() {
            score += 12;
        }
        let subtitle = clean
            .rsplit_once('/')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_default();
        scored_files.push(MentionCandidateView {
            key: format!("file::{clean}"),
            kind: "file".into(),
            token_label: format!("@{base}"),
            title: base,
            subtitle,
            file_path: clean.to_string(),
            git_status: git_status.to_string(),
            score,
        });
    }

    scored_files.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.file_path.cmp(&b.file_path))
    });
    out.extend(scored_files);
    out.truncate(max_items);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_commands_filters_platform_and_query() {
        let desktop = slash_commands(ClientPlatform::Desktop);
        let ios = slash_commands(ClientPlatform::Ios);
        assert!(desktop.iter().any(|c| c.id == "files"));
        assert!(!ios.iter().any(|c| c.id == "files"));
        assert!(ios.iter().any(|c| c.id == "btw"));

        let rev = filter_slash_commands("audit", ClientPlatform::Harmony);
        assert_eq!(rev.len(), 1);
        assert_eq!(rev[0].id, "review");
    }

    #[test]
    fn compose_prompt_handles_btw_plan_review_compact_and_inline_badges() {
        let btw = compose_prompt(
            &ComposePromptRequest {
                provider: ProviderKind::ClaudeCode,
                raw_text: "  /btw how does cache work? ".into(),
                explicit_directive: None,
                host_id: None,
                cwd: None,
                contexts: Vec::new(),
                quotes: Vec::new(),
                pastes: Vec::new(),
            },
            None,
            None,
        );
        assert!(btw.is_btw);
        assert_eq!(btw.btw_question.as_deref(), Some("how does cache work?"));

        let compact_claude = compose_prompt(
            &ComposePromptRequest {
                provider: ProviderKind::ClaudeCode,
                raw_text: "/compact focus on auth".into(),
                explicit_directive: None,
                host_id: None,
                cwd: None,
                contexts: Vec::new(),
                quotes: Vec::new(),
                pastes: Vec::new(),
            },
            None,
            None,
        );
        assert!(compact_claude.is_compact);
        assert_eq!(compact_claude.wire_prompt, "/compact focus on auth");

        let compact_codex = compose_prompt(
            &ComposePromptRequest {
                provider: ProviderKind::Codex,
                raw_text: "/compact focus on auth".into(),
                explicit_directive: None,
                host_id: None,
                cwd: None,
                contexts: Vec::new(),
                quotes: Vec::new(),
                pastes: Vec::new(),
            },
            None,
            None,
        );
        assert!(compact_codex.is_compact);
        assert!(
            compact_codex
                .wire_prompt
                .contains("<WORKFLOW_INSTRUCTION command=\"/compact\">")
        );

        // Review auto-attaches working diff & avoids duplicate inline @badge
        let utf8_diff = format!("+{}", "汉".repeat(5000));
        let review = compose_prompt(
            &ComposePromptRequest {
                provider: ProviderKind::Codex,
                raw_text: "check @stage.rs".into(),
                explicit_directive: Some(SlashDirective::Review),
                host_id: Some("h1".into()),
                cwd: Some("/repo".into()),
                contexts: vec![ComposerContextInput::WorkspaceFile {
                    path: "src/stage.rs".into(),
                }],
                quotes: vec![ComposerQuoteInput {
                    path: "src/stage.rs".into(),
                    line_number: Some(42),
                    code: "let x = 1;".into(),
                }],
                pastes: Vec::new(),
            },
            Some(&utf8_diff),
            None,
        );
        assert_eq!(review.badges.len(), 2); // /review + quote (inline @stage.rs suppressed)
        assert!(review.wire_prompt.contains("Working Tree Diff"));
        assert!(review.wire_prompt.contains("Reference `src/stage.rs:42`"));
    }

    #[test]
    fn rank_mention_candidates_boosts_git_modified_and_exact_basename() {
        let ranked = rank_mention_candidates(
            "stage",
            &[
                "crates/app/src/old_stage.rs".into(),
                "crates/app/src/stage.rs".into(),
                "crates/app/src/other.rs".into(),
            ],
            &[("crates/app/src/old_stage.rs".into(), "M".into())],
            10,
        );
        // `@staged-diff` starts with "stage", followed by exact stem match `stage.rs` (115) > modified `old_stage.rs` (92)
        assert_eq!(ranked[0].token_label, "@staged-diff");
        let files: Vec<_> = ranked.iter().filter(|c| c.kind == "file").collect();
        assert_eq!(files[0].file_path, "crates/app/src/stage.rs");
        assert_eq!(files[1].file_path, "crates/app/src/old_stage.rs");
        assert_eq!(files[1].git_status, "M");
        assert!(!ranked.iter().any(|c| c.file_path.ends_with("other.rs")));
    }

    #[test]
    fn compose_prompt_populates_paths_only_on_file_image_and_quote_badges() {
        let composed = compose_prompt(
            &ComposePromptRequest {
                provider: ProviderKind::ClaudeCode,
                raw_text: "/plan check this".into(),
                explicit_directive: None,
                host_id: Some("h1".into()),
                cwd: Some("/repo".into()),
                contexts: vec![
                    ComposerContextInput::Image {
                        path: "/Users/test/.orbit/attachments/img.png".into(),
                        filename: "img.png".into(),
                    },
                    ComposerContextInput::WorkspaceFile {
                        path: "crates/app/src/lib.rs".into(),
                    },
                    ComposerContextInput::GitDiff {
                        staged: false,
                        label: "Working Tree Diff".into(),
                        diff_text: None,
                    },
                    ComposerContextInput::TerminalOutput {
                        label: "Terminal output (10 lines)".into(),
                        content: "ok".into(),
                    },
                ],
                quotes: vec![ComposerQuoteInput {
                    path: "src/main.rs".into(),
                    line_number: Some(12),
                    code: "let x = 1;".into(),
                }],
                pastes: vec![ComposerPasteInput {
                    label: "Pasted snippet".into(),
                    filename: None,
                    content: "abc".into(),
                }],
            },
            None,
            None,
        );

        assert_eq!(composed.badges.len(), 7);
        assert_eq!(composed.badges[0].kind, "slash");
        assert_eq!(composed.badges[0].path, None);
        assert_eq!(composed.badges[1].kind, "image");
        assert_eq!(
            composed.badges[1].path.as_deref(),
            Some("/Users/test/.orbit/attachments/img.png")
        );
        assert_eq!(composed.badges[2].kind, "file");
        assert_eq!(
            composed.badges[2].path.as_deref(),
            Some("crates/app/src/lib.rs")
        );
        assert_eq!(composed.badges[3].kind, "git_diff");
        assert_eq!(composed.badges[3].path, None);
        assert_eq!(composed.badges[4].kind, "terminal");
        assert_eq!(composed.badges[4].path, None);
        assert_eq!(composed.badges[5].kind, "quote");
        assert_eq!(composed.badges[5].path.as_deref(), Some("src/main.rs:12"));
        assert_eq!(composed.badges[6].kind, "paste");
        assert_eq!(composed.badges[6].path, None);
    }
}
