//! Wire types for the Orbit Skills Ecosystem & Managed Skills CRUD manager.

use serde::{Deserialize, Serialize};

/// Discovery source / ownership origin of a `SKILL.md` skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    OrbitUser,
    OrbitProject,
    ClaudeUser,
    ClaudeProject,
    CodexUser,
    AntigravityUser,
    Builtin,
}

impl SkillSource {
    /// Stable lowercase identifier used in skill IDs and wire projections.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrbitUser => "orbit_user",
            Self::OrbitProject => "orbit_project",
            Self::ClaudeUser => "claude_user",
            Self::ClaudeProject => "claude_project",
            Self::CodexUser => "codex_user",
            Self::AntigravityUser => "antigravity_user",
            Self::Builtin => "builtin",
        }
    }

    /// Whether skills from this source are directly editable and deletable by Orbit.
    pub const fn is_editable(self) -> bool {
        matches!(self, Self::OrbitUser | Self::OrbitProject)
    }
}

/// Target storage scope when creating, editing, or importing an Orbit-managed skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillWriteScope {
    User,
    Project,
}

/// Unified metadata projection for any discovered or managed `SKILL.md` skill on a host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedSkillRecord {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub path: String,
    pub enabled: bool,
    pub editable: bool,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub has_scripts: bool,
    #[serde(default)]
    pub has_references: bool,
    #[serde(default)]
    pub approx_tokens: u32,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    #[serde(default)]
    pub is_command: bool,
}

/// Strip optional YAML frontmatter (`---\n...\n---`) from a Markdown command template
/// and substitute `$ARGUMENTS` placeholders cleanly.
///
/// If the template does not contain `$ARGUMENTS` and `arguments` is non-empty,
/// appends the trimmed arguments after a blank line so caller arguments are never dropped.
pub fn expand_command_arguments(template_content: &str, arguments: &str) -> String {
    let normalized = template_content
        .strip_prefix('\u{feff}')
        .unwrap_or(template_content);
    let body = if let Some(rest) = normalized
        .strip_prefix("---\n")
        .or_else(|| normalized.strip_prefix("---\r\n"))
    {
        let mut offset = 0usize;
        let mut body_start = None;
        for line in rest.split_inclusive('\n') {
            let stripped = line.trim_end_matches(['\r', '\n']);
            if stripped == "---" || stripped == "..." {
                body_start = Some(offset + line.len());
                break;
            }
            offset += line.len();
        }
        body_start.map_or(normalized, |start| &rest[start..])
    } else {
        normalized
    };

    let clean_body = body.trim();
    let clean_args = arguments.trim();
    if clean_body.contains("$ARGUMENTS") {
        clean_body
            .replace("$ARGUMENTS", clean_args)
            .trim()
            .to_string()
    } else if clean_args.is_empty() {
        clean_body.to_string()
    } else if clean_body.is_empty() {
        clean_args.to_string()
    } else {
        format!("{clean_body}\n\n{clean_args}")
    }
}
