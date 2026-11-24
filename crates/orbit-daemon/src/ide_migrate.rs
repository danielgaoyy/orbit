//! Inspect and import named IDE resources without exposing file content in the migration report.
use crate::mcp_servers;
use anyhow::{Context, ensure};
use orbit_protocol::{McpServerScope, MigrationConflict};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize)]
pub struct MigrationItem {
    pub source: String,
    pub kind: String,
    pub name: String,
    pub destination: PathBuf,
    pub decision: &'static str,
}

struct Candidate {
    source: &'static str,
    kind: &'static str,
    name: String,
    path: PathBuf,
    data: Vec<u8>,
    root: PathBuf,
}

fn confined(root: &Path, path: &Path) -> anyhow::Result<()> {
    let mut current = root.to_owned();
    for c in path.strip_prefix(root)?.components() {
        let std::path::Component::Normal(name) = c else {
            anyhow::bail!("invalid migration path")
        };
        current.push(name);
        match current.symlink_metadata() {
            Ok(m) => ensure!(!m.is_symlink(), "symlink in migration path"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn read(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    if let Ok(meta) = path.symlink_metadata() {
        ensure!(meta.is_file(), "expected regular migration input")
    }
    let f = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    ensure!(f.metadata()?.is_file(), "expected regular migration input");
    let mut data = Vec::new();
    f.take(2 * 1024 * 1024 + 1).read_to_end(&mut data)?;
    ensure!(
        data.len() <= 2 * 1024 * 1024,
        "migration file exceeds 2 MiB"
    );
    Ok(Some(data))
}
fn files(root: &Path, dir: &Path, depth: u8, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if depth > 12 || !dir.exists() {
        return Ok(());
    }
    confined(root, dir)?;
    let mut children = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|e| e.file_name());
    for e in children {
        let ty = e.file_type()?;
        if ty.is_dir() {
            files(root, &e.path(), depth + 1, out)?;
        } else if ty.is_file() {
            ensure!(out.len() < 1024, "too many migration files");
            out.push(e.path());
        }
    }
    Ok(())
}

fn collect(
    source: &'static str,
    base: &Path,
    external: &str,
    target: &Path,
    project: bool,
    out: &mut Vec<Candidate>,
) -> anyhow::Result<()> {
    let folder = base.join(external);
    let dest = if project {
        target.join(".orbit")
    } else {
        target.to_owned()
    };
    for kind in ["rules", "skills", "commands"] {
        if source == "vscode" {
            continue;
        }
        let dir = folder.join(kind);
        let mut listed = Vec::new();
        files(base, &dir, 0, &mut listed)?;
        for path in listed {
            let rel = path.strip_prefix(&dir)?;
            let target_file = match kind {
                "skills" => dest.join("skills").join(rel),
                "commands" if path.extension().is_some_and(|v| v == "md") => dest
                    .join("skills")
                    .join(rel.with_extension(""))
                    .join("SKILL.md"),
                "rules"
                    if path
                        .extension()
                        .is_some_and(|v| v == "md" || v == "mdc" || v == "rules") =>
                {
                    dest.join("rules").join(rel.with_extension("md"))
                }
                _ => continue,
            };
            confined(base, &path)?;
            let Some(data) = read(&path)? else { continue };
            ensure!(
                out.len() < 2048
                    && out.iter().map(|c| c.data.len()).sum::<usize>() + data.len()
                        <= 32 * 1024 * 1024,
                "migration inputs exceed 32 MiB or 2048 items"
            );
            out.push(Candidate {
                source,
                kind,
                name: rel.display().to_string(),
                path: target_file,
                root: target.into(),
                data,
            });
        }
    }
    for name in match source {
        "claude" => vec!["CLAUDE.md"],
        "codex" => vec!["AGENTS.md"],
        "antigravity" => vec!["GEMINI.md"],
        _ => Vec::new(),
    } {
        let path = folder.join(name);
        confined(base, &path)?;
        if let Some(data) = read(&path)? {
            out.push(Candidate {
                source,
                kind: "rules",
                name: name.into(),
                path: dest.join("rules").join(name),
                root: target.into(),
                data,
            });
        }
    }
    let mut configs = match source {
        "claude" => {
            let mut list = vec![folder.join("settings.json"), folder.join("mcp.json")];
            if !project {
                list.push(base.join(".claude.json"));
            }
            list
        }
        "codex" => vec![folder.join("config.toml")],
        "antigravity" => vec![folder.join("mcp_config.json")],
        _ => vec![folder.join("mcp.json")],
    };
    for path in configs.drain(..) {
        confined(base, &path)?;
        let Some(bytes) = read(&path)? else { continue };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            tracing::warn!(path = %path.display(), "skipping non-UTF8 IDE migration config");
            continue;
        };
        let servers = if source == "codex" {
            mcp_servers::parse_codex_toml_mcp_servers(text, &path)
        } else {
            let mut v: Value = match json5::from_str(text) {
                Ok(parsed) => parsed,
                Err(err) => {
                    tracing::warn!(path = %path.display(), %err, "skipping malformed IDE migration config");
                    continue;
                }
            };
            if source == "vscode" {
                v = json!({"mcpServers":v.get("servers")});
            }
            mcp_servers::parse_json_mcp_servers_value(&v, McpServerScope::OrbitUser, &path, true)
        };
        for server in servers {
            let v = match mcp_servers::migration_value(&server) {
                Ok(val) => val,
                Err(err) => {
                    tracing::warn!(server = %server.name, %err, "skipping invalid MCP server entry during migration");
                    continue;
                }
            };
            out.push(Candidate {
                source,
                kind: "mcp",
                name: server.name,
                path: target.join(if project {
                    ".mcp.json"
                } else {
                    "mcp-servers.json"
                }),
                data: serde_json::to_vec(&v)?,
                root: target.into(),
            });
        }
    }
    Ok(())
}

pub fn migrate(
    home: &Path,
    user: &Path,
    cwd: Option<&Path>,
    from: Option<&str>,
    apply: bool,
    conflict: MigrationConflict,
) -> anyhow::Result<Vec<MigrationItem>> {
    let selected = from
        .map(|s| s.to_ascii_lowercase().replace([' ', '-'], ""))
        .map(|s| {
            if s == "claudecode" {
                "claude".into()
            } else {
                s
            }
        });
    ensure!(
        selected
            .as_deref()
            .is_none_or(|s| ["claude", "codex", "antigravity", "cursor", "vscode"].contains(&s)),
        "unknown migration source"
    );
    let mut candidates = Vec::new();
    let home = if !apply && !home.exists() {
        std::path::absolute(home)?
    } else {
        home.canonicalize()?
    };
    let user = user.canonicalize()?;
    let cwd = cwd.map(Path::canonicalize).transpose()?;
    let _lock = if apply {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(home.join(".ide-migration.lock"))?;
        file.lock()?;
        Some(file)
    } else {
        None
    };
    for (src, dir) in [
        ("claude", ".claude"),
        ("codex", ".codex"),
        ("antigravity", ".gemini/antigravity"),
        ("cursor", ".cursor"),
        ("vscode", ".vscode"),
    ] {
        if selected.as_deref().is_some_and(|s| s != src) {
            continue;
        }
        if src != "vscode" {
            collect(src, &user, dir, &home, false, &mut candidates)?;
        }
        if matches!(src, "claude" | "cursor" | "vscode")
            && let Some(cwd) = &cwd
        {
            collect(src, cwd, dir, cwd, true, &mut candidates)?;
        }
    }
    let mut report = Vec::new();
    let mut writes = BTreeMap::<PathBuf, Vec<u8>>::new();
    // Select one destination directory for a skill and all its relative resources.
    let mut packages = BTreeMap::<(String, PathBuf), Vec<usize>>::new();
    for (idx, c) in candidates.iter().enumerate() {
        if matches!(c.kind, "skills" | "commands") {
            let base = if c.root == home {
                home.join("skills")
            } else {
                c.root.join(".orbit/skills")
            };
            let mut rel = c.path.strip_prefix(&base)?.components();
            if let Some(first) = rel.next()
                && rel.next().is_some()
            {
                packages
                    .entry((c.source.into(), base.join(first)))
                    .or_default()
                    .push(idx);
            }
        }
    }
    let mut planned = BTreeMap::<PathBuf, Vec<u8>>::new();
    let mut decisions = BTreeMap::<usize, &str>::new();
    for ((source, folder), indexes) in packages {
        let mut chosen = folder.clone();
        let mut suffix = 0;
        loop {
            let mut collision = false;
            for &i in &indexes {
                let c = &candidates[i];
                let target = chosen.join(c.path.strip_prefix(&folder)?);
                confined(&c.root, &target)?;
                let old = match planned.get(&target) {
                    Some(b) => Some(b.clone()),
                    None => read(&target)?,
                };
                if old.is_some_and(|b| b != c.data) {
                    collision = true;
                    break;
                }
            }
            if !collision || conflict == MigrationConflict::Overwrite {
                break;
            }
            if conflict == MigrationConflict::Skip {
                for i in &indexes {
                    decisions.insert(*i, "skip");
                }
                break;
            }
            suffix += 1;
            ensure!(suffix < 1000, "too many skill name conflicts");
            let name = folder
                .file_name()
                .context("invalid skill package")?
                .to_string_lossy();
            chosen = folder.with_file_name(format!("{name}-{source}-{suffix}"));
        }
        for i in indexes {
            let c = &mut candidates[i];
            c.path = chosen.join(c.path.strip_prefix(&folder)?);
            if !decisions.contains_key(&i) {
                planned.insert(c.path.clone(), c.data.clone());
            }
            if suffix > 0 {
                decisions.insert(i, "rename");
            }
        }
    }
    for (i, c) in candidates.into_iter().enumerate() {
        confined(&c.root, &c.path)?;
        let mut name = c.name.clone();
        let mut path = c.path.clone();
        let mcp = c.kind == "mcp";
        let raw = writes
            .get(&path)
            .cloned()
            .map(Some)
            .map_or_else(|| read(&path), Ok)?;
        let mut config = if mcp {
            match raw.as_ref() {
                Some(b) => serde_json::from_slice(b)?,
                None => json!({"mcpServers":{}}),
            }
        } else {
            Value::Null
        };
        ensure!(!mcp || config.is_object(), "invalid MCP configuration");
        if mcp && config.get("mcpServers").is_none() {
            config["mcpServers"] = json!({});
        }
        ensure!(
            !mcp || config["mcpServers"].is_object(),
            "invalid MCP destination"
        );
        let mut decision = decisions.get(&i).copied().unwrap_or("create");
        let mut suffix = 0u32;
        while decision != "skip" {
            let old = if mcp {
                config["mcpServers"]
                    .get(&name)
                    .map(serde_json::to_vec)
                    .transpose()?
            } else {
                writes
                    .get(&path)
                    .cloned()
                    .map(Some)
                    .map_or_else(|| read(&path), Ok)?
            };
            match old {
                None => break,
                Some(ref b) if b == &c.data => {
                    decision = "unchanged";
                    break;
                }
                _ => match conflict {
                    MigrationConflict::Skip => {
                        decision = "skip";
                        break;
                    }
                    MigrationConflict::Overwrite => {
                        decision = "overwrite";
                        break;
                    }
                    MigrationConflict::Rename => {
                        suffix += 1;
                        ensure!(suffix < 1000, "too many conflicts");
                        decision = "rename";
                        let tag = format!("{}-{suffix}", c.source);
                        if mcp {
                            name = format!("{}-{tag}", c.name)
                        } else {
                            let stem = c
                                .path
                                .file_stem()
                                .context("invalid filename")?
                                .to_string_lossy();
                            let ext = c.path.extension().and_then(|s| s.to_str()).unwrap_or("md");
                            path = c.path.with_file_name(format!("{stem}-{tag}.{ext}"));
                            confined(&c.root, &path)?;
                        }
                    }
                },
            }
        }
        if !matches!(decision, "unchanged" | "skip") {
            let data = if mcp {
                config["mcpServers"][&name] = serde_json::from_slice(&c.data)?;
                serde_json::to_vec_pretty(&config)?
            } else {
                c.data
            };
            writes.insert(path.clone(), data);
        }
        report.push(MigrationItem {
            source: c.source.into(),
            kind: c.kind.into(),
            name,
            destination: path,
            decision,
        });
    }
    if apply {
        for (path, bytes) in writes {
            let parent = path.parent().context("missing destination parent")?;
            fs::create_dir_all(parent)?;
            crate::diagnostics::write_private(&path, |f| Ok(f.write_all(&bytes)?))?;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skills_keep_resources_and_imported_secrets_stay_private() -> anyhow::Result<()> {
        let t = tempfile::tempdir()?;
        let user = t.path().join("user");
        let home = t.path().join("orbit");
        let src = user.join(".claude/skills/build");
        let existing = home.join("skills/build");
        fs::create_dir_all(&src)?;
        fs::create_dir_all(&existing)?;
        fs::write(existing.join("SKILL.md"), "old")?;
        fs::write(src.join("SKILL.md"), "new")?;
        fs::write(src.join("example.txt"), "example")?;
        fs::write(
            user.join(".claude/mcp.json"),
            r#"{"mcpServers":{"demo":{"command":"tool","env":{"API_KEY":"sk-secret"}}}}"#,
        )?;
        let result = migrate(
            &home,
            &user,
            None,
            Some("claude"),
            true,
            MigrationConflict::Rename,
        )?;
        assert!(!serde_json::to_string(&result)?.contains("sk-secret"));
        let renamed = home.join("skills/build-claude-1");
        assert_eq!(fs::read(renamed.join("example.txt"))?, b"example");
        assert_eq!(fs::read(existing.join("SKILL.md"))?, b"old");
        assert!(
            migrate(&home, &user, None, None, true, MigrationConflict::Rename)?
                .iter()
                .all(|c| c.decision == "unchanged")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                home.join("mcp-servers.json")
                    .metadata()?
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        Ok(())
    }

    #[test]
    fn five_sources_preview_conflict_and_repeat() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let user = tmp.path().join("user");
        let home = tmp.path().join("orbit");
        let ws = tmp.path().join("ws");
        fs::create_dir_all(&home)?;
        for (root, dir, file, content) in [
            (
                &user,
                ".claude",
                "mcp.json",
                r#"{"mcpServers":{"a":{"command":"claude"}}}"#,
            ),
            (
                &user,
                ".codex",
                "config.toml",
                "[mcp_servers.b]\ncommand='codex'",
            ),
            (
                &user,
                ".gemini/antigravity",
                "mcp_config.json",
                r#"{"mcpServers":{"c":{"command":"agy"}}}"#,
            ),
            (
                &ws,
                ".cursor",
                "mcp.json",
                r#"{"mcpServers":{"d":{"command":"cursor"}}}"#,
            ),
            (
                &ws,
                ".vscode",
                "mcp.json",
                r#"{"servers":{"e":{"command":"vscode"}}}"#,
            ),
        ] {
            fs::create_dir_all(root.join(dir))?;
            fs::write(root.join(dir).join(file), content)?;
        }
        let run = |apply, c| migrate(&home, &user, Some(&ws), None, apply, c);
        assert_eq!(run(false, MigrationConflict::Skip)?.len(), 5);
        assert!(!home.join("mcp-servers.json").exists());
        run(true, MigrationConflict::Skip)?;
        assert!(
            run(true, MigrationConflict::Skip)?
                .iter()
                .all(|i| i.decision == "unchanged")
        );
        fs::write(
            user.join(".claude/mcp.json"),
            r#"{"mcpServers":{"a":{"command":"updated"}}}"#,
        )?;
        for (policy, decision) in [
            (MigrationConflict::Skip, "skip"),
            (MigrationConflict::Rename, "rename"),
            (MigrationConflict::Overwrite, "overwrite"),
        ] {
            assert_eq!(run(true, policy)?[0].decision, decision);
        }
        Ok(())
    }
}
