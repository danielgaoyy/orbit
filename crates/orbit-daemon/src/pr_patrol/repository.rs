//! Read immutable Git objects. No checkout, external diff driver, or textconv is needed.
use super::scanner::deny_unsafe;
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
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
    pub async fn object_store(
        &self,
        root: &Path,
        number: u64,
    ) -> Result<Option<tempfile::TempDir>> {
        if oid(root, &self.head_ref_oid).await.is_ok()
            && oid(root, &self.base_ref_oid).await.is_ok()
        {
            return Ok(None);
        }
        let tmp = tempfile::tempdir()?;
        git(tmp.path(), &["init", "--bare", "--quiet"], 1024).await?;
        let remote = format!("https://github.com/{}.git", self.slug()?);
        git(
            tmp.path(),
            &[
                "-c",
                "credential.helper=",
                "-c",
                "credential.helper=!gh auth git-credential",
                "fetch",
                "--quiet",
                "--no-tags",
                &remote,
                &self.base_ref_oid,
                &format!("refs/pull/{number}/head"),
            ],
            1024,
        )
        .await?;
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

fn added_lines(patch: &str) -> Result<BTreeMap<u32, String>> {
    let mut line = None;
    let mut added = BTreeMap::new();
    for raw in patch.split_terminator('\n') {
        if raw.starts_with("@@ ") {
            let start = raw
                .split_whitespace()
                .nth(2)
                .and_then(|s| s.strip_prefix('+'))
                .and_then(|s| s.split(',').next())
                .context("Invalid diff hunk")?
                .parse::<u32>()?;
            line = Some(start);
        } else if let Some(n) = line {
            if let Some(text) = raw.strip_prefix('+') {
                added.insert(n, text.trim_end_matches('\r').to_owned());
            }
            if raw.starts_with([' ', '+']) {
                line = n.checked_add(1);
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
