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
}
