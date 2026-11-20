//! Zero-configuration project & Git repository discovery + interactive remote directory browser.
//!
//! Supports both automatic discovery (including `~/workspace`, `~/go/src`, `~/.paseo/projects`,
//! `~/.claude.json`, and Orbit's own workspace history) AND interactive remote/local folder browsing
//! (`list_directory_entries`) so users can add any single or multi-directory Project with clicks!

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use orbit_protocol::{DirectoryEntry, DiscoveredProject};
use orbit_store::OrbitStore;

const CANDIDATE_WORKSPACE_ROOTS: &[&str] = &[
    "workspace",
    "workspaces",
    "projects",
    "Projects",
    "Developer",
    "dev",
    "src",
    "code",
    "repos",
    "github",
    "go/src",
    "artifacts",
    "deliveries",
    "opt",
];

const IGNORED_DIR_NAMES: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    "vendor",
    "Library",
    "Applications",
    "Movies",
    "Music",
    "Pictures",
    "Public",
    "Trash",
];

/// Browse subdirectories inside `requested_path` (defaults to `$HOME` when `None` or `"~"`).
pub fn list_directory_entries(
    requested_path: Option<&str>,
) -> (String, Option<String>, Vec<DirectoryEntry>) {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));

    let target = match requested_path.map(str::trim) {
        None | Some("") | Some("~") => home.clone(),
        Some(rel) if rel.starts_with("~/") => home.join(&rel[2..]),
        Some(abs) => PathBuf::from(abs),
    };

    let canonical = if target.is_dir() {
        std::fs::canonicalize(&target).unwrap_or(target)
    } else {
        std::fs::canonicalize(&home).unwrap_or(home)
    };

    let current_path = canonical.to_string_lossy().to_string();
    let parent_path = canonical
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && *p != canonical.as_path())
        .map(|p| p.to_string_lossy().to_string());

    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&canonical) {
        for item in rd.flatten() {
            let path = item.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            let is_git_repo = path.join(".git").exists();
            entries.push(DirectoryEntry {
                name: name.to_string(),
                path: path.to_string_lossy().to_string(),
                is_git_repo,
            });
        }
    }

    entries.sort_by(|a, b| {
        b.is_git_repo
            .cmp(&a.is_git_repo)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    (current_path, parent_path, entries)
}

/// Discover selectable projects on the current host sorted by recent activity (`last_modified_unix` DESC).
pub fn discover_host_projects(store: &OrbitStore) -> Vec<DiscoveredProject> {
    let mut by_path: HashMap<String, DiscoveredProject> = HashMap::new();

    // 1. Seed from Orbit's own recent workspaces and agents (highest recency boost)
    if let Ok(workspaces) = store.list_workspaces() {
        for ws in workspaces {
            let p = PathBuf::from(&ws.cwd);
            if p.is_dir() {
                insert_candidate(
                    &mut by_path,
                    &p,
                    Some(ws.updated_at.timestamp()),
                    ws.git_branch,
                );
            }
        }
    }
    if let Ok(agents) = store.list_agents(None) {
        for ag in agents {
            let p = PathBuf::from(&ag.cwd);
            if p.is_dir() {
                insert_candidate(&mut by_path, &p, Some(ag.updated_at.timestamp()), None);
            }
        }
    }

    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        // 2. Also import any projects configured in Paseo (`~/.paseo/projects/projects.json`) or Claude (`~/.claude.json`)
        import_paseo_projects(&home, &mut by_path);
        import_claude_projects(&home, &mut by_path);

        // 3. Scan common workspace roots under $HOME (1~2 levels deep)
        for rel in CANDIDATE_WORKSPACE_ROOTS {
            let root = home.join(rel);
            if root.is_dir() {
                scan_workspace_root(&root, 2, &mut by_path);
            }
        }

        // 4. Include direct non-hidden subdirectories of $HOME that are Git repos (excluding $HOME/.git itself)
        scan_direct_git_children(&home, &mut by_path);

        if by_path.is_empty() && home.is_dir() {
            insert_candidate(&mut by_path, &home, None, None);
        }
    }

    let mut list: Vec<DiscoveredProject> = by_path.into_values().collect();
    list.sort_by(|a, b| {
        b.last_modified_unix
            .cmp(&a.last_modified_unix)
            .then_with(|| b.is_git_repo.cmp(&a.is_git_repo))
            .then_with(|| a.name.cmp(&b.name))
    });
    list.truncate(60);
    list
}

fn import_paseo_projects(home: &Path, out: &mut HashMap<String, DiscoveredProject>) {
    let path = home.join(".paseo/projects/projects.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) else {
        return;
    };
    for item in arr {
        if item.get("archivedAt").is_some_and(|v| !v.is_null()) {
            continue;
        }
        if let Some(root_str) = item.get("rootPath").and_then(|v| v.as_str()) {
            let p = PathBuf::from(root_str);
            if p.is_dir() {
                insert_candidate(out, &p, None, None);
            }
        }
    }
}

fn import_claude_projects(home: &Path, out: &mut HashMap<String, DiscoveredProject>) {
    let path = home.join(".claude.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let Some(projects_obj) = val.get("projects").and_then(|v| v.as_object()) else {
        return;
    };
    for key in projects_obj.keys().take(30) {
        let p = PathBuf::from(key);
        if p.is_dir() && p != home {
            insert_candidate(out, &p, None, None);
        }
    }
}

fn scan_direct_git_children(dir: &Path, out: &mut HashMap<String, DiscoveredProject>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.starts_with('.') || IGNORED_DIR_NAMES.contains(&name) {
            continue;
        }
        if path.is_dir() && path.join(".git").exists() {
            insert_candidate(out, &path, None, None);
        }
    }
}

fn scan_workspace_root(
    dir: &Path,
    remaining_depth: u8,
    out: &mut HashMap<String, DiscoveredProject>,
) {
    if remaining_depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.starts_with('.') || IGNORED_DIR_NAMES.contains(&name) {
            continue;
        }
        if !path.is_dir() {
            continue;
        }

        if path.join(".git").exists() {
            insert_candidate(out, &path, None, None);
        } else if remaining_depth > 1 {
            insert_candidate(out, &path, None, None);
            scan_direct_git_children(&path, out);
        }
    }
}

fn insert_candidate(
    out: &mut HashMap<String, DiscoveredProject>,
    raw_path: &Path,
    boosted_ts: Option<i64>,
    known_branch: Option<String>,
) {
    let canonical = std::fs::canonicalize(raw_path).unwrap_or_else(|_| raw_path.to_path_buf());
    let key = canonical.to_string_lossy().to_string();
    let name = canonical
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("workspace")
        .to_string();

    let (is_git_repo, detected_branch) = read_fast_git_head(&canonical);
    let mtime = boosted_ts.unwrap_or_else(|| read_dir_mtime(&canonical));
    let git_branch = known_branch.or(detected_branch);

    match out.get_mut(&key) {
        Some(existing) => {
            if mtime > existing.last_modified_unix {
                existing.last_modified_unix = mtime;
            }
            if existing.git_branch.is_none() {
                existing.git_branch = git_branch;
            }
            existing.is_git_repo |= is_git_repo;
        }
        None => {
            out.insert(
                key.clone(),
                DiscoveredProject {
                    path: key,
                    name,
                    git_branch,
                    is_git_repo,
                    last_modified_unix: mtime,
                },
            );
        }
    }
}

fn read_dir_mtime(path: &Path) -> i64 {
    let git_index = path.join(".git").join("index");
    if let Ok(meta) = std::fs::metadata(&git_index)
        && let Ok(mod_time) = meta.modified()
        && let Ok(dur) = mod_time.duration_since(UNIX_EPOCH)
    {
        return dur.as_secs() as i64;
    }
    if let Ok(meta) = std::fs::metadata(path)
        && let Ok(mod_time) = meta.modified()
        && let Ok(dur) = mod_time.duration_since(UNIX_EPOCH)
    {
        return dur.as_secs() as i64;
    }
    0
}

fn read_fast_git_head(repo_dir: &Path) -> (bool, Option<String>) {
    let dot_git = repo_dir.join(".git");
    if dot_git.is_dir() {
        return (true, parse_head_file(&dot_git.join("HEAD")));
    }
    if dot_git.is_file()
        && let Ok(raw) = std::fs::read_to_string(&dot_git)
        && let Some(gitdir_str) = raw.lines().next().and_then(|l| l.strip_prefix("gitdir:"))
    {
        let gitdir = PathBuf::from(gitdir_str.trim());
        let resolved = if gitdir.is_absolute() {
            gitdir
        } else {
            repo_dir.join(gitdir)
        };
        return (true, parse_head_file(&resolved.join("HEAD")));
    }
    (false, None)
}

fn parse_head_file(head_path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(head_path).ok()?;
    let trimmed = raw.trim();
    if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
        Some(branch.to_string())
    } else if trimmed.len() >= 7 {
        Some(trimmed[..7].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_seeded_workspace_and_lists_subdirs() {
        let store = OrbitStore::open_in_memory().expect("in-memory store");
        let tmp =
            std::env::temp_dir().join(format!("orbit_disc_test_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(tmp.join("sub_repo/.git")).expect("mkdir .git");
        std::fs::write(
            tmp.join("sub_repo/.git/HEAD"),
            "ref: refs/heads/feat/clean-ui\n",
        )
        .expect("write HEAD");

        let sub_str = tmp.join("sub_repo").to_string_lossy().to_string();
        store
            .upsert_workspace(&orbit_protocol::WorkspaceSummary {
                workspace_id: "ws_disc".into(),
                title: "demo".into(),
                cwd: sub_str,
                git_branch: None,
                is_worktree: false,
                base_ref: None,
                parent_workspace_id: None,
                source_cwd: None,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                background: false,
                archived_at: None,
                updated_at: chrono::Utc::now(),
            })
            .expect("upsert");

        let found = discover_host_projects(&store);
        assert!(
            found
                .iter()
                .any(|p| p.git_branch.as_deref() == Some("feat/clean-ui"))
        );

        let (_, _, entries) = list_directory_entries(Some(&tmp.to_string_lossy()));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "sub_repo");
        assert!(entries[0].is_git_repo);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
