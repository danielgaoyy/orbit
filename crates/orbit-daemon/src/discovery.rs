//! Zero-configuration project & Git repository discovery on the host filesystem.
//!
//! Eliminates manual `cwd` path typing ("fill-in-the-blank") in Orbit clients by automatically
//! discovering active Git repositories and workspaces under common developer directories and
//! Orbit's persisted session history.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use orbit_protocol::DiscoveredProject;
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
];

const IGNORED_DIR_NAMES: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    "vendor",
    "Library",
    "Applications",
    "Downloads",
    "Movies",
    "Music",
    "Pictures",
    "Public",
    "Trash",
];

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

    // 2. Scan common workspace roots under $HOME (1~2 levels deep, skipping heavy/system dirs)
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for rel in CANDIDATE_WORKSPACE_ROOTS {
            let root = home.join(rel);
            if root.is_dir() {
                scan_workspace_root(&root, 2, &mut by_path);
            }
        }
        // Also check direct children of $HOME that are themselves Git repositories
        scan_direct_git_children(&home, &mut by_path);

        // Ensure at least $HOME is available if the machine is completely fresh
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
    list.truncate(48);
    list
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
            // Also include immediate project folders inside ~/workspace even if not git-initialized yet,
            // while still checking one level deeper for nested git repos (e.g. ~/workspace/org/repo).
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
    fn discovers_seeded_workspace_cleanly() {
        let store = OrbitStore::open_in_memory().expect("in-memory store");
        let tmp =
            std::env::temp_dir().join(format!("orbit_disc_test_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(tmp.join(".git")).expect("mkdir .git");
        std::fs::write(tmp.join(".git/HEAD"), "ref: refs/heads/feat/clean-ui\n")
            .expect("write HEAD");

        store
            .upsert_workspace(&orbit_protocol::WorkspaceSummary {
                workspace_id: "ws_disc".into(),
                title: "demo".into(),
                cwd: tmp.to_string_lossy().to_string(),
                git_branch: None,
                is_worktree: false,
                updated_at: chrono::Utc::now(),
            })
            .expect("upsert");

        let found = discover_host_projects(&store);
        assert!(
            found
                .iter()
                .any(|p| p.git_branch.as_deref() == Some("feat/clean-ui"))
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
