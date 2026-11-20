//! Bundled Orbit Orchestration Skills embedded directly into the single Rust binary (`include_str!`)
//! with transactional sync across Claude Code, Codex, Antigravity, and generic `~/.agents/skills`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use orbit_protocol::OrchestrationSkillInfo;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub struct BundledSkill {
    pub id: &'static str,
    pub name: &'static str,
    pub slash_command: &'static str,
    pub description: &'static str,
    pub content: &'static str,
}

pub const BUNDLED_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        id: "orbit",
        name: "Orbit Reference",
        slash_command: "/orbit",
        description: "Core reference for orchestrating agents, worktrees, terminals, schedules, heartbeats, and connected hosts.",
        content: include_str!("../skills/orbit/SKILL.md"),
    },
    BundledSkill {
        id: "orbit-handoff",
        name: "Task Handoff",
        slash_command: "/orbit-handoff",
        description: "Transfer the active task and structured briefing to a specialist agent or isolated worktree.",
        content: include_str!("../skills/orbit-handoff/SKILL.md"),
    },
    BundledSkill {
        id: "orbit-committee",
        name: "Committee Planning",
        slash_command: "/orbit-committee",
        description: "Fan out a complex problem to two independent model families, synthesize the plan, implement, and review.",
        content: include_str!("../skills/orbit-committee/SKILL.md"),
    },
    BundledSkill {
        id: "orbit-advisor",
        name: "Advisor",
        slash_command: "/orbit-advisor",
        description: "Get an independent read-only second opinion or diff review from another model without editing files.",
        content: include_str!("../skills/orbit-advisor/SKILL.md"),
    },
];

const MANAGED_MARKER_FILE: &str = ".orbit-managed.json";

#[derive(Debug, Serialize, Deserialize)]
struct ManagedMarker {
    skill_id: String,
    sha256: String,
}

fn content_sha256(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Target skill root directories discovered under the user's home directory.
pub fn resolve_skill_target_roots(user_home: &Path) -> Vec<(&'static str, PathBuf)> {
    vec![
        ("claude", user_home.join(".claude").join("skills")),
        ("codex", user_home.join(".codex").join("skills")),
        (
            "antigravity",
            user_home
                .join(".gemini")
                .join("antigravity-cli")
                .join("skills"),
        ),
        ("agents", user_home.join(".agents").join("skills")),
    ]
}

/// Inspect installation and freshness status of all bundled orchestration skills.
pub fn inspect_orchestration_skills(user_home: &Path) -> Vec<OrchestrationSkillInfo> {
    let targets = resolve_skill_target_roots(user_home);
    BUNDLED_SKILLS
        .iter()
        .map(|skill| {
            let expected_hash = content_sha256(skill.content);
            let mut installed_targets = Vec::new();
            let mut outdated = false;

            for (label, root) in &targets {
                let skill_dir = root.join(skill.id);
                let md_path = skill_dir.join("SKILL.md");
                if md_path.is_file() {
                    installed_targets.push((*label).to_string());
                    if let Ok(actual) = std::fs::read_to_string(&md_path)
                        && content_sha256(&actual) != expected_hash
                    {
                        outdated = true;
                    }
                }
            }

            OrchestrationSkillInfo {
                id: skill.id.to_string(),
                name: skill.name.to_string(),
                slash_command: skill.slash_command.to_string(),
                description: skill.description.to_string(),
                installed: !installed_targets.is_empty(),
                installed_targets,
                outdated,
            }
        })
        .collect()
}

/// Synchronize the selected set of bundled orchestration skills onto the host's skill directories.
/// Never overwrites or removes a skill directory that lacks Orbit's `.orbit-managed.json` marker.
pub fn sync_orchestration_skills(
    user_home: &Path,
    selected_ids: &[String],
) -> anyhow::Result<Vec<OrchestrationSkillInfo>> {
    let selected: HashSet<&str> = selected_ids.iter().map(String::as_str).collect();
    let targets = resolve_skill_target_roots(user_home);

    for skill in BUNDLED_SKILLS {
        let should_install = selected.contains(skill.id);
        let expected_hash = content_sha256(skill.content);

        for (_, root) in &targets {
            let skill_dir = root.join(skill.id);
            let md_path = skill_dir.join("SKILL.md");
            let marker_path = skill_dir.join(MANAGED_MARKER_FILE);

            if should_install {
                // Only write if directory does not exist OR is Orbit-managed
                if skill_dir.exists() && !marker_path.exists() {
                    continue;
                }
                std::fs::create_dir_all(&skill_dir)?;
                let tmp_md = skill_dir.join(".SKILL.md.tmp");
                std::fs::write(&tmp_md, skill.content)?;
                std::fs::rename(&tmp_md, &md_path)?;

                let marker = ManagedMarker {
                    skill_id: skill.id.to_string(),
                    sha256: expected_hash.clone(),
                };
                let tmp_marker = skill_dir.join(".orbit-managed.json.tmp");
                std::fs::write(&tmp_marker, serde_json::to_string_pretty(&marker)?)?;
                std::fs::rename(&tmp_marker, &marker_path)?;
            } else if marker_path.exists() {
                let _ = std::fs::remove_dir_all(&skill_dir);
            }
        }
    }

    Ok(inspect_orchestration_skills(user_home))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_updates_and_uninstalls_managed_skills_safely() {
        let tmp = std::env::temp_dir().join(format!(
            "orbit_skills_test_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&tmp).expect("mkdir");

        // User-owned unmanaged custom skill with same name must never be clobbered
        let unmanaged_dir = tmp.join(".claude").join("skills").join("orbit-advisor");
        std::fs::create_dir_all(&unmanaged_dir).expect("mkdir custom");
        std::fs::write(unmanaged_dir.join("SKILL.md"), "MY CUSTOM ADVISOR").expect("write custom");

        let synced = sync_orchestration_skills(&tmp, &["orbit".into(), "orbit-advisor".into()])
            .expect("sync install");
        let orbit_info = synced.iter().find(|s| s.id == "orbit").expect("orbit");
        assert!(orbit_info.installed);
        assert_eq!(orbit_info.installed_targets.len(), 4);
        assert!(!orbit_info.outdated);

        // Verify custom claude skill remained untouched
        let custom_after =
            std::fs::read_to_string(unmanaged_dir.join("SKILL.md")).expect("read custom");
        assert_eq!(custom_after, "MY CUSTOM ADVISOR");

        // Uninstall all managed skills
        let cleared = sync_orchestration_skills(&tmp, &[]).expect("sync uninstall");
        assert!(!cleared.iter().find(|s| s.id == "orbit").expect("orbit").installed);
        // Custom unmanaged directory still preserved
        assert!(unmanaged_dir.join("SKILL.md").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
