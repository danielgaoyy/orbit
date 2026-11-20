//! Zero-configuration project & Git repository discovery + interactive remote directory browser.
//!
//! Supports both automatic discovery (including `~/workspace`, `~/go/src`, `~/.paseo/projects`,
//! `~/.claude.json`, and Orbit's own workspace history) AND interactive remote/local folder browsing
//! (`list_directory_entries`) so users can add any single or multi-directory Project with clicks!

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use orbit_protocol::{DirectoryEntry, DiscoveredProject, NestedGitRepo};
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
            let nested_git_repos = if is_git_repo {
                Vec::new()
            } else {
                discover_nested_git_repos(&canonical, &[], 2, 16)
            };
            out.insert(
                key.clone(),
                DiscoveredProject {
                    path: key,
                    name,
                    git_branch,
                    is_git_repo,
                    last_modified_unix: mtime,
                    nested_git_repos,
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

/// Fast zero-subprocess inspection of whether `cwd` is inside a Git repository and any nested
/// Git repositories contained within `cwd` or its `extra_dirs`.
pub fn inspect_project_git_repos(
    cwd: &Path,
    extra_dirs: &[PathBuf],
) -> (bool, Option<String>, Vec<NestedGitRepo>) {
    let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let (is_git, branch) = find_enclosing_git_head(&canonical);
    let repos = if is_git && extra_dirs.is_empty() {
        Vec::new()
    } else {
        discover_nested_git_repos(&canonical, extra_dirs, 3, 24)
    };
    (is_git, branch, repos)
}

/// Discover nested Git repositories within `root` (up to `max_depth` levels) and any explicit `extra_dirs`.
pub fn discover_nested_git_repos(
    root: &Path,
    extra_dirs: &[PathBuf],
    max_depth: usize,
    limit: usize,
) -> Vec<NestedGitRepo> {
    let Ok(canon_root) = std::fs::canonicalize(root) else {
        return Vec::new();
    };
    if !canon_root.is_dir() {
        return Vec::new();
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<NestedGitRepo> = Vec::new();

    // 1. Explicit multi-directory project members first
    for extra in extra_dirs {
        if out.len() >= limit {
            break;
        }
        let Ok(canon_extra) = std::fs::canonicalize(extra) else {
            continue;
        };
        if !canon_extra.is_dir() || canon_extra == canon_root {
            continue;
        }
        let (is_git, branch) = find_enclosing_git_head(&canon_extra);
        if is_git {
            let path_s = canon_extra.to_string_lossy().to_string();
            if seen.insert(path_s.clone()) {
                let name = canon_extra
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("repo")
                    .to_string();
                let relative_path = canon_extra
                    .strip_prefix(&canon_root)
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| name.clone());
                out.push(NestedGitRepo {
                    path: path_s,
                    relative_path,
                    name,
                    git_branch: branch,
                });
            }
        } else {
            walk_nested_git_repos(
                &canon_extra,
                &canon_extra,
                max_depth.min(2),
                limit,
                &mut seen,
                &mut out,
            );
        }
    }

    // 2. Direct & nested child Git repositories inside `root`
    walk_nested_git_repos(
        &canon_root,
        &canon_root,
        max_depth,
        limit,
        &mut seen,
        &mut out,
    );
    out.sort_by_key(|a| a.relative_path.to_lowercase());
    out
}

fn walk_nested_git_repos(
    root: &Path,
    dir: &Path,
    remaining_depth: usize,
    limit: usize,
    seen: &mut HashSet<String>,
    out: &mut Vec<NestedGitRepo>,
) {
    if remaining_depth == 0 || out.len() >= limit {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= limit {
            break;
        }
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
        let (is_git, branch) = read_fast_git_head(&path);
        if is_git {
            let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            let path_s = canonical.to_string_lossy().to_string();
            if seen.insert(path_s.clone()) {
                let relative_path = path
                    .strip_prefix(root)
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| name.to_string());
                out.push(NestedGitRepo {
                    path: path_s,
                    relative_path,
                    name: name.to_string(),
                    git_branch: branch,
                });
            }
            // Do not descend into an already discovered Git repo
            continue;
        }
        if remaining_depth > 1 {
            walk_nested_git_repos(root, &path, remaining_depth - 1, limit, seen, out);
        }
    }
}

fn find_enclosing_git_head(dir: &Path) -> (bool, Option<String>) {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|p| std::fs::canonicalize(p).ok());
    let mut cur = Some(dir);
    for _ in 0..12 {
        let Some(p) = cur else {
            break;
        };
        if home.as_deref() == Some(p) {
            break;
        }
        let (is_git, branch) = read_fast_git_head(p);
        if is_git {
            return (true, branch);
        }
        cur = p.parent();
    }
    (false, None)
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
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
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

        // Inspect parent non-git folder -> reports is_git_repo=false and discovers nested sub_repo
        let (parent_is_git, _, nested) = inspect_project_git_repos(&tmp, &[]);
        assert!(!parent_is_git);
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].relative_path, "sub_repo");
        assert_eq!(nested[0].git_branch.as_deref(), Some("feat/clean-ui"));

        // Inspect nested git repo directly -> reports is_git_repo=true
        let (sub_is_git, sub_branch, _) = inspect_project_git_repos(&tmp.join("sub_repo"), &[]);
        assert!(sub_is_git);
        assert_eq!(sub_branch.as_deref(), Some("feat/clean-ui"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
