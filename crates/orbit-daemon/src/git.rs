//! Native Git status, unified diff inspection, and isolated worktree management.

use std::path::{Path, PathBuf};

use orbit_protocol::{GitFileEntry, GitStatusSnapshot};
use tokio::process::Command;

/// Inspect git repository status using `git status --porcelain=v2 --branch`.
pub async fn inspect_git_status(cwd: &Path) -> GitStatusSnapshot {
    let cwd_str = cwd.to_string_lossy().to_string();
    let output = Command::new("git")
        .args(["status", "--porcelain=v2", "--branch"])
        .current_dir(cwd)
        .output()
        .await;

    let Ok(out) = output else {
        return empty_non_git(cwd_str);
    };
    if !out.status.success() {
        return empty_non_git(cwd_str);
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut branch = None;
    let mut upstream = None;
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut files = Vec::new();

    for line in stdout.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            if head != "(detached)" {
                branch = Some(head.trim().to_string());
            }
        } else if let Some(up) = line.strip_prefix("# branch.upstream ") {
            upstream = Some(up.trim().to_string());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for part in ab.split_whitespace() {
                if let Some(a) = part.strip_prefix('+') {
                    ahead = a.parse().unwrap_or(0);
                } else if let Some(b) = part.strip_prefix('-') {
                    behind = b.parse().unwrap_or(0);
                }
            }
        } else if let Some(untracked_path) = line.strip_prefix("? ") {
            files.push(GitFileEntry {
                path: untracked_path.trim().to_string(),
                status: "??".into(),
                staged: false,
                untracked: true,
            });
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            // Format: `1 XY sub mH mI mW hH hI path` or `2 XY sub mH mI mW hH hI Xscore path\torigPath`
            let parts: Vec<&str> = line.splitn(9, ' ').collect();
            if parts.len() >= 9 {
                let xy = parts[1];
                let raw_tail = parts[8];
                let path = if line.starts_with("2 ") {
                    // Rename entry has `Xscore path\torigPath` in tail
                    raw_tail
                        .split_once(' ')
                        .map(|(_, rest)| rest.split('\t').next().unwrap_or(rest))
                        .unwrap_or(raw_tail)
                } else {
                    raw_tail
                };
                let mut chars = xy.chars();
                let x = chars.next().unwrap_or('.');
                let y = chars.next().unwrap_or('.');
                if x != '.' {
                    files.push(GitFileEntry {
                        path: path.to_string(),
                        status: x.to_string(),
                        staged: true,
                        untracked: false,
                    });
                }
                if y != '.' {
                    files.push(GitFileEntry {
                        path: path.to_string(),
                        status: y.to_string(),
                        staged: false,
                        untracked: false,
                    });
                }
            }
        }
    }

    GitStatusSnapshot {
        cwd: cwd_str,
        is_git_repo: true,
        branch,
        upstream,
        ahead,
        behind,
        files,
    }
}

/// Compute unified diff for an entire workspace or a single file (including untracked file preview).
pub async fn compute_git_diff(
    cwd: &Path,
    rel_path: Option<&str>,
    staged: bool,
) -> anyhow::Result<String> {
    let mut args = vec!["diff", "--no-color"];
    if staged {
        args.push("--cached");
    }
    if let Some(p) = rel_path {
        args.push("--");
        args.push(p);
    }

    let out = Command::new("git")
        .args(&args)
        .current_dir(cwd)
        .output()
        .await?;

    let diff_text = String::from_utf8_lossy(&out.stdout).to_string();
    if !diff_text.trim().is_empty() || staged {
        return Ok(diff_text);
    }

    // If diff is empty for a requested untracked file, synthesize an added-file unified diff
    if let Some(p) = rel_path {
        let candidate = cwd.join(p);
        if candidate.is_file()
            && let Ok(content) = tokio::fs::read_to_string(&candidate).await
        {
            let mut synth = format!("--- /dev/null\n+++ b/{p}\n");
            let lines: Vec<&str> = content.lines().collect();
            synth.push_str(&format!("@@ -0,0 +1,{} @@\n", lines.len()));
            for l in lines.into_iter().take(500) {
                synth.push('+');
                synth.push_str(l);
                synth.push('\n');
            }
            return Ok(synth);
        }
    }

    Ok(diff_text)
}

/// Create an isolated git worktree under `$ORBIT_HOME/worktrees/<repo>-<branch>`.
pub async fn create_isolated_worktree(
    orbit_home: &Path,
    source_cwd: &Path,
    branch_name: &str,
    base_ref: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let sanitized: String = branch_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let repo_name = source_cwd
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo");

    let target_dir = orbit_home
        .join("worktrees")
        .join(format!("{repo_name}-{sanitized}"));
    if let Some(parent) = target_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut cmd = Command::new("git");
    cmd.arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(branch_name)
        .arg(&target_dir);
    if let Some(base) = base_ref {
        cmd.arg(base);
    }
    let out = cmd.current_dir(source_cwd).output().await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git worktree add failed: {stderr}");
    }

    Ok(target_dir)
}

fn empty_non_git(cwd: String) -> GitStatusSnapshot {
    GitStatusSnapshot {
        cwd,
        is_git_repo: false,
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        files: Vec::new(),
    }
}
