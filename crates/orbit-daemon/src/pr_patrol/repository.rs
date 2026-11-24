//! Read immutable Git objects. No checkout, external diff driver, or textconv is needed.
use super::scanner::deny_unsafe;
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub(super) const FILE_LIMIT: usize = 512 * 1024;
const RULE_LIMIT: usize = 16 * 1024;

pub(super) async fn command(
    dir: &Path,
    program: &str,
    args: &[&str],
    input: Option<&[u8]>,
    limit: usize,
) -> Result<Vec<u8>> {
    let task = async {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(dir)
            .env("GIT_LITERAL_PATHSPECS", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GH_PROMPT_DISABLED", "1")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut output = child
            .stdout
            .take()
            .context("missing stdout")?
            .take((limit + 1) as u64);
        let mut bytes = Vec::new();
        let write = async {
            if let Some(mut stdin) = child.stdin.take()
                && let Some(input) = input
            {
                stdin.write_all(input).await?;
            }
            Ok::<_, std::io::Error>(())
        };
        tokio::try_join!(write, output.read_to_end(&mut bytes))?;
        ensure!(
            bytes.len() <= limit,
            "{program} output exceeds {limit} bytes"
        );
        ensure!(
            child.wait().await?.success(),
            "{program} {} failed",
            args.first().unwrap_or(&"")
        );
        Ok(bytes)
    };
    tokio::time::timeout(Duration::from_secs(30), task)
        .await
        .context("Review subprocess timed out")?
}

async fn git(dir: &Path, args: &[&str], limit: usize) -> Result<Vec<u8>> {
    command(dir, "git", args, None, limit).await
}

pub(super) async fn oid(dir: &Path, name: &str) -> Result<String> {
    ensure!(
        !name.is_empty()
            && !name.starts_with('-')
            && name.len() <= 1024
            && !name.chars().any(char::is_control),
        "Invalid Git ref"
    );
    let bytes = git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{name}^{{commit}}"),
        ],
        128,
    )
    .await
    .with_context(|| format!("Cannot resolve commit {name}"))?;
    Ok(std::str::from_utf8(&bytes)?.trim().to_owned())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PullRequest {
    pub base_ref_oid: String,
    pub head_ref_oid: String,
    pub url: String,
}

pub(super) struct AsyncTempDir {
    dir: Option<tempfile::TempDir>,
}

impl AsyncTempDir {
    pub(super) async fn new() -> Result<Self> {
        let dir = tokio::task::spawn_blocking(tempfile::tempdir)
            .await
            .context("Tempdir creation task failed")??;
        Ok(Self { dir: Some(dir) })
    }

    pub(super) fn path(&self) -> &Path {
        self.dir
            .as_ref()
            .map_or_else(|| Path::new(""), tempfile::TempDir::path)
    }

    pub(super) async fn close(mut self) {
        if let Some(dir) = self.dir.take() {
            let _ = tokio::task::spawn_blocking(move || drop(dir)).await;
        }
    }
}

impl Drop for AsyncTempDir {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn_blocking(move || drop(dir));
            } else {
                drop(dir);
            }
        }
    }
}

async fn resolve_git_objects_dir(root: &Path) -> Result<PathBuf> {
    let bytes = git(root, &["rev-parse", "--git-common-dir"], 4096).await?;
    let raw = std::str::from_utf8(&bytes)?.trim();
    ensure!(!raw.is_empty(), "Empty git common directory");
    let common = PathBuf::from(raw);
    let objects = if common.is_absolute() {
        common.join("objects")
    } else {
        root.join(common).join("objects")
    };
    tokio::task::spawn_blocking(move || -> Result<PathBuf> {
        let canonical = std::fs::canonicalize(&objects).with_context(|| {
            format!(
                "Cannot canonicalize Git objects directory {}",
                objects.display()
            )
        })?;
        ensure!(
            canonical.is_dir(),
            "Git objects path is not a directory: {}",
            canonical.display()
        );
        Ok(canonical)
    })
    .await
    .context("Git objects resolution task failed")?
}

fn format_alternates_entry(objects_dir: &Path) -> Result<String> {
    let raw = objects_dir
        .to_str()
        .context("Git objects path is not valid UTF-8")?;
    #[cfg(windows)]
    let normalized = raw.strip_prefix(r"\\?\").unwrap_or(raw).replace('\\', "/");
    #[cfg(not(windows))]
    let normalized = raw.to_string();
    ensure!(
        !normalized.chars().any(|c| matches!(c, '\n' | '\r' | '\0')),
        "Invalid control character in Git objects path"
    );
    if normalized.starts_with('#') || normalized.starts_with('"') {
        let escaped = normalized.replace('\\', "\\\\").replace('"', "\\\"");
        Ok(format!("\"{escaped}\"\n"))
    } else {
        Ok(format!("{normalized}\n"))
    }
}

pub(super) async fn init_bare_object_store_with_alternates(root: &Path) -> Result<AsyncTempDir> {
    let objects_dir = resolve_git_objects_dir(root).await?;
    let tmp = AsyncTempDir::new().await?;
    if let Err(err) = async {
        git(tmp.path(), &["init", "--bare", "--quiet"], 1024).await?;
        let alternates_path = tmp.path().join("objects").join("info").join("alternates");
        tokio::task::spawn_blocking(move || -> Result<()> {
            if let Some(parent) = alternates_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let entry = format_alternates_entry(&objects_dir)?;
            std::fs::write(&alternates_path, entry)?;
            Ok(())
        })
        .await
        .context("Writing Git alternates task failed")??;
        Ok::<_, anyhow::Error>(())
    }
    .await
    {
        tmp.close().await;
        return Err(err);
    }
    Ok(tmp)
}

impl PullRequest {
    pub async fn lookup(root: &Path, number: u64) -> Result<Self> {
        let bytes = command(
            root,
            "gh",
            &[
                "pr",
                "view",
                &number.to_string(),
                "--json",
                "baseRefOid,headRefOid,url",
            ],
            None,
            4096,
        )
        .await?;
        let pr: Self = serde_json::from_slice(&bytes)?;
        ensure!(
            [&pr.head_ref_oid, &pr.base_ref_oid]
                .iter()
                .all(|oid| oid.len() == 40 && oid.bytes().all(|b| b.is_ascii_hexdigit())),
            "Invalid GitHub commit ID"
        );
        Ok(pr)
    }

    pub fn slug(&self) -> Result<String> {
        let (owner, repo) = crate::git_collab::parse_github_owner_repo_from_pr_url(&self.url)
            .context("Invalid GitHub PR URL")?;
        ensure!(
            self.url.starts_with("https://github.com/"),
            "Expected github.com PR URL"
        );
        Ok(format!("{owner}/{repo}"))
    }

    // When a fork head is absent locally, fetch into a disposable object store. Never check out PR files.
    pub async fn object_store(&self, root: &Path, number: u64) -> Result<Option<AsyncTempDir>> {
        if oid(root, &self.head_ref_oid).await.is_ok()
            && oid(root, &self.base_ref_oid).await.is_ok()
        {
            return Ok(None);
        }
        let tmp = init_bare_object_store_with_alternates(root).await?;
        let remote = format!("https://github.com/{}.git", self.slug()?);
        let head_ref = format!("refs/pull/{number}/head");
        let need_base = oid(tmp.path(), &self.base_ref_oid).await.is_err();
        let mut fetch_args = vec![
            "-c",
            "credential.helper=",
            "-c",
            "credential.helper=!gh auth git-credential",
            "fetch",
            "--quiet",
            "--no-tags",
            &remote,
        ];
        if need_base {
            fetch_args.push(&self.base_ref_oid);
        }
        fetch_args.push(&head_ref);
        if let Err(err) = git(tmp.path(), &fetch_args, 1024).await {
            tmp.close().await;
            return Err(err);
        }
        Ok(Some(tmp))
    }
}

#[derive(Default)]
pub(super) struct File {
    pub lines: Vec<String>,
    pub added: BTreeMap<u32, String>,
    pub test_ranges: Vec<std::ops::RangeInclusive<u32>>,
    pub rust_signals: BTreeSet<(u32, super::scanner::Rule)>,
}

pub(super) struct ContextData {
    pub files: BTreeMap<String, File>,
    pub rules: String,
    pub hunks: String,
    pub skipped: usize,
}

fn parse_hunk_range(token: &str, prefix: char) -> Result<(u32, u32)> {
    let body = token.strip_prefix(prefix).context("Invalid diff hunk")?;
    match body.split_once(',') {
        Some((start, count)) => Ok((start.parse()?, count.parse()?)),
        None => Ok((body.parse()?, 1)),
    }
}

fn added_lines(patch: &str) -> Result<BTreeMap<u32, String>> {
    let mut line: Option<u32> = None;
    let mut old_rem: u32 = 0;
    let mut new_rem: u32 = 0;
    let mut added = BTreeMap::new();
    for raw in patch.split_terminator('\n') {
        if raw.starts_with("diff --git ")
            || (old_rem == 0
                && new_rem == 0
                && (raw.starts_with("--- ") || raw.starts_with("+++ ")))
        {
            line = None;
            old_rem = 0;
            new_rem = 0;
        } else if raw.starts_with("@@ ") {
            let mut parts = raw.split_whitespace();
            let _at = parts.next();
            let old_tok = parts.next().context("Invalid diff hunk")?;
            let new_tok = parts.next().context("Invalid diff hunk")?;
            let (_, old_count) = parse_hunk_range(old_tok, '-')?;
            let (new_start, new_count) = parse_hunk_range(new_tok, '+')?;
            old_rem = old_count;
            new_rem = new_count;
            line = (new_count > 0).then_some(new_start);
        } else if old_rem > 0 || new_rem > 0 {
            if let Some(text) = raw.strip_prefix('+') {
                if let Some(n) = line {
                    added.insert(n, text.trim_end_matches('\r').to_owned());
                    line = n.checked_add(1);
                }
                new_rem = new_rem.saturating_sub(1);
            } else if raw.starts_with(' ') {
                if let Some(n) = line {
                    line = n.checked_add(1);
                }
                old_rem = old_rem.saturating_sub(1);
                new_rem = new_rem.saturating_sub(1);
            } else if raw.starts_with('-') {
                old_rem = old_rem.saturating_sub(1);
            }
        }
    }
    Ok(added)
}

pub(super) fn append_bounded(dst: &mut String, text: &str, limit: usize) {
    let n = text.floor_char_boundary(limit.saturating_sub(dst.len()).min(text.len()));
    dst.push_str(&text[..n]);
}

pub(super) async fn assemble(root: &Path, base: &str, head: &str) -> Result<ContextData> {
    let mb = git(root, &["merge-base", base, head], 128).await?;
    let ancestor = std::str::from_utf8(&mb)?.trim();
    let paths = git(
        root,
        &[
            "diff",
            "--no-ext-diff",
            "--find-renames",
            "-l256",
            "--diff-filter=AMTR",
            "--raw",
            "--no-abbrev",
            "-z",
            ancestor,
            head,
            "--",
        ],
        128 * 1024,
    )
    .await?;
    let mut data = ContextData {
        files: BTreeMap::new(),
        rules: String::new(),
        hunks: String::new(),
        skipped: 0,
    };
    let rule_paths = git(
        root,
        &[
            "ls-tree",
            "-r",
            "--name-only",
            "-z",
            ancestor,
            "--",
            "AGENTS.md",
            "REVIEW.md",
            ".orbit/rules",
            "Cargo.toml",
        ],
        64 * 1024,
    )
    .await?;
    let mut deny = false;
    for path in rule_paths
        .split(|b| *b == 0)
        .filter_map(|b| std::str::from_utf8(b).ok())
    {
        if !matches!(path, "AGENTS.md" | "REVIEW.md" | "Cargo.toml")
            && !(path.starts_with(".orbit/rules/")
                && Path::new(path).parent() == Some(Path::new(".orbit/rules"))
                && path.ends_with(".md"))
        {
            continue;
        }
        if data.rules.len() >= RULE_LIMIT && path != "Cargo.toml" {
            continue;
        }
        let object = format!("{ancestor}:{path}");
        let size = blob_size(root, &object).await?;
        if size > FILE_LIMIT {
            continue;
        }
        let bytes = git(root, &["cat-file", "blob", &object], FILE_LIMIT).await?;
        let text = String::from_utf8_lossy(&bytes);
        if path == "Cargo.toml" {
            deny = deny_unsafe(&text);
            continue;
        }
        append_bounded(&mut data.rules, &format!("\n{path}\n{text}"), RULE_LIMIT);
    }
    // A PR can introduce the lint with its first manifest. A removed base policy
    // still applies to the review, just like the other base-commit rules.
    if !deny {
        let manifest = git(root, &["ls-tree", head, "--", "Cargo.toml"], 128).await?;
        if manifest.starts_with(b"100644 blob ") || manifest.starts_with(b"100755 blob ") {
            let object = format!("{head}:Cargo.toml");
            if blob_size(root, &object).await? <= FILE_LIMIT {
                let bytes = git(root, &["cat-file", "blob", &object], FILE_LIMIT).await?;
                deny = deny_unsafe(&String::from_utf8_lossy(&bytes));
            }
        }
    }
    let mut total = 0;
    let mut total_lines = 0;
    let mut fields = paths.split(|b| *b == 0).filter(|p| !p.is_empty());
    while let Some(raw) = fields.next() {
        ensure!(
            data.files.len() + data.skipped < 256,
            "Review exceeds 256 changed files; narrow the commit range"
        );
        let header = std::str::from_utf8(raw)?
            .split_ascii_whitespace()
            .collect::<Vec<_>>();
        let [_, mode, original, object, status] = header.as_slice() else {
            bail!("Malformed raw diff header");
        };
        let mut name = fields.next().context("Missing path in raw diff")?;
        let renamed = status.starts_with('R');
        if renamed {
            name = fields
                .next()
                .context("Missing rename destination in raw diff")?;
        }
        if !matches!(*mode, "100644" | "100755") {
            data.skipped += 1;
            continue;
        }

        let Ok(path) = std::str::from_utf8(name) else {
            data.skipped += 1;
            continue;
        };
        if path.chars().any(char::is_control) {
            data.skipped += 1;
            continue;
        }
        let size = blob_size(root, object).await?;
        if size > FILE_LIMIT || total + size > 8 * 1024 * 1024 {
            data.skipped += 1;
            continue;
        }
        let bytes = git(root, &["cat-file", "blob", object], FILE_LIMIT).await?;
        let Ok(content) = String::from_utf8(bytes) else {
            data.skipped += 1;
            continue;
        };
        let line_count = content.lines().count();
        if content.contains('\0') || total_lines + line_count > 120_000 {
            data.skipped += 1;
            continue;
        }
        total += size;
        total_lines += line_count;
        let mut args = vec![
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--text",
            "--no-renames",
            "--unified=5",
        ];
        if renamed {
            // GitHub anchors renames against the old blob, including when the checkout is elsewhere.
            args.extend_from_slice(&[original, object]);
        } else {
            args.extend_from_slice(&[ancestor, head, "--", path]);
        }
        let patch = git(root, &args, 2 * FILE_LIMIT + 64 * 1024).await?;
        let patch = String::from_utf8_lossy(&patch);
        let added = added_lines(&patch)?;
        let mut file = File {
            added,
            ..File::default()
        };
        if path.ends_with(".rs") {
            super::scanner::rust_context(&content, deny, &mut file);
        }
        file.lines = content.lines().map(str::to_owned).collect();
        append_bounded(&mut data.hunks, &format!("\n{path}\n{patch}"), 64 * 1024);
        data.files.insert(path.into(), file);
    }
    Ok(data)
}

async fn blob_size(root: &Path, object: &str) -> Result<usize> {
    let bytes = git(root, &["cat-file", "-s", object], 64).await?;
    Ok(std::str::from_utf8(&bytes)?.trim().parse()?)
}

pub(super) async fn default_base(root: &Path) -> Result<String> {
    for name in ["origin/HEAD", "main", "master"] {
        if let Ok(commit) = oid(root, name).await {
            return Ok(commit);
        }
    }
    bail!("Cannot determine base branch; supply --base")
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_git(dir: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
        let mut opts = vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.hooksPath=/dev/null",
        ];
        opts.extend_from_slice(args);
        command(dir, "git", &opts, input, 64 * 1024).await
    }

    #[test]
    fn typechange_multi_section_diff_never_inserts_line_zero() -> Result<()> {
        let symlink_to_file = "\
diff --git a/src/entry.rs b/src/entry.rs
deleted file mode 120000
index 1234567..0000000
--- a/src/entry.rs
+++ /dev/null
@@ -1 +0,0 @@
-target.rs
\\ No newline at end of file
diff --git a/src/entry.rs b/src/entry.rs
new file mode 100644
index 0000000..abcdef0
--- /dev/null
+++ b/src/entry.rs
@@ -0,0 +1,2 @@
+pub fn alpha() {}
+pub fn beta() {}
";
        let added = added_lines(symlink_to_file)?;
        assert!(!added.contains_key(&0), "spurious line 0: {added:?}");
        assert_eq!(
            added,
            BTreeMap::from([
                (1, "pub fn alpha() {}".to_string()),
                (2, "pub fn beta() {}".to_string()),
            ])
        );

        let file_to_symlink = "\
diff --git a/src/entry.rs b/src/entry.rs
deleted file mode 100644
index abcdef0..0000000
--- a/src/entry.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-pub fn alpha() {}
-pub fn beta() {}
diff --git a/src/entry.rs b/src/entry.rs
new file mode 120000
index 0000000..1234567
--- /dev/null
+++ b/src/entry.rs
@@ -0,0 +1 @@
+target.rs
\\ No newline at end of file
";
        let symlink_added = added_lines(file_to_symlink)?;
        assert!(!symlink_added.contains_key(&0));
        assert_eq!(
            symlink_added,
            BTreeMap::from([(1, "target.rs".to_string())])
        );

        // Section boundary without `diff --git ` headers must also reset cleanly.
        let header_only_sections = "\
--- a/src/entry.rs
+++ /dev/null
@@ -1 +0,0 @@
-old_target
--- /dev/null
+++ b/src/entry.rs
@@ -0,0 +1 @@
+fn clean() {}
";
        let header_added = added_lines(header_only_sections)?;
        assert!(!header_added.contains_key(&0));
        assert_eq!(
            header_added,
            BTreeMap::from([(1, "fn clean() {}".to_string())])
        );

        // Source lines inside a hunk that look like `--- ` / `+++ ` are preserved accurately.
        let tricky_hunk = "\
diff --git a/query.sql b/query.sql
index 1111111..2222222 100644
--- a/query.sql
+++ b/query.sql
@@ -1,2 +1,2 @@
--- old sql comment
+++ added line
 SELECT 1;
";
        let tricky_added = added_lines(tricky_hunk)?;
        assert_eq!(
            tricky_added,
            BTreeMap::from([(1, "++ added line".to_string())])
        );
        Ok(())
    }

    #[tokio::test]
    async fn assemble_handles_real_git_typechange_symlink_to_regular_file() -> Result<()> {
        let tmp = AsyncTempDir::new().await?;
        let root = tmp.path();
        test_git(root, &["init", "--quiet"], None).await?;
        // Stage a gitlink/symlink blob (mode 120000) directly in the index so this works cross-platform.
        let sym_blob = test_git(
            root,
            &["hash-object", "-w", "--stdin"],
            Some(b"old_target.rs"),
        )
        .await?;
        let sym_oid = std::str::from_utf8(&sym_blob)?.trim().to_string();
        test_git(
            root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("120000,{sym_oid},entry.rs"),
            ],
            None,
        )
        .await?;
        test_git(root, &["commit", "-qm", "base symlink"], None).await?;
        let base = oid(root, "HEAD").await?;

        // Replace symlink with a regular file (produces `T` typechange diff).
        let file_blob = test_git(
            root,
            &["hash-object", "-w", "--stdin"],
            Some(b"pub fn one() {}\npub fn two() {}\n"),
        )
        .await?;
        let file_oid = std::str::from_utf8(&file_blob)?.trim().to_string();
        test_git(
            root,
            &[
                "update-index",
                "--cacheinfo",
                &format!("100644,{file_oid},entry.rs"),
            ],
            None,
        )
        .await?;
        test_git(root, &["commit", "-qm", "replace with regular file"], None).await?;
        let head = oid(root, "HEAD").await?;

        let data = assemble(root, &base, &head).await?;
        let entry = data.files.get("entry.rs").context("missing entry.rs")?;
        assert!(
            !entry.added.contains_key(&0),
            "real git typechange diff must never emit line 0: {:?}",
            entry.added
        );
        assert_eq!(
            entry.added,
            BTreeMap::from([
                (1, "pub fn one() {}".to_string()),
                (2, "pub fn two() {}".to_string()),
            ])
        );
        tmp.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn bare_object_store_writes_alternates_and_cleans_up_async() -> Result<()> {
        let repo_tmp = AsyncTempDir::new().await?;
        let root = repo_tmp.path();
        test_git(root, &["init", "--quiet"], None).await?;
        std::fs::write(root.join("lib.rs"), "pub fn base() -> u32 { 1 }\n")?;
        test_git(root, &["add", "lib.rs"], None).await?;
        test_git(root, &["commit", "-qm", "base"], None).await?;
        let base = oid(root, "HEAD").await?;

        let store = init_bare_object_store_with_alternates(root).await?;
        let store_path = store.path().to_path_buf();
        let alternates_path = store_path.join("objects").join("info").join("alternates");
        assert!(
            alternates_path.is_file(),
            "objects/info/alternates must exist"
        );
        let alt_text = std::fs::read_to_string(&alternates_path)?;
        let expected_objects = std::fs::canonicalize(root.join(".git").join("objects"))?;
        let actual_objects = std::fs::canonicalize(Path::new(alt_text.trim()))?;
        assert!(
            alt_text.trim().ends_with("objects") && actual_objects == expected_objects,
            "unexpected alternates content: {alt_text:?} vs {}",
            expected_objects.display()
        );
        // Bare object store must immediately resolve the main repo's commits via alternates.
        assert_eq!(oid(store.path(), &base).await?, base);

        store.close().await;
        assert!(
            !store_path.exists(),
            "AsyncTempDir::close must remove the bare store directory"
        );
        repo_tmp.close().await;
        Ok(())
    }
}
