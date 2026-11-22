//! Repo-keyed warm detached-HEAD Git worktree reuse pool (`$ORBIT_HOME/worktrees/_pool/<repo-hash>/`).
//!
//! Pre-warms clean detached-HEAD standbys per active Git repository so subsequent worktree
//! provisioning avoids cold `git worktree add` full-tree checkouts. On claim:
//! 1. Runs `git worktree prune` and verifies the standby is detached-HEAD and completely clean.
//! 2. Moves the standby atomically via `git worktree move` into the target worktree directory.
//! 3. Switches branch via fast differential `git checkout -b <branch> <base> --` (or `git checkout <branch> --`).
//! 4. Schedules asynchronous background replenishment up to `max_standby_per_repo`.
//!
//! If the pool is disabled, empty, dirty, or any step fails, callers fall back seamlessly to
//! standard `git worktree add`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use sha2::{Digest, Sha256};
use tokio::process::Command;
use tokio::sync::{Mutex as AsyncMutex, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Default maximum warm detached-HEAD standbys kept per active Git repository.
pub const DEFAULT_MAX_STANDBY_PER_REPO: usize = 2;
/// Hard ceiling for `max_standby_per_repo` to prevent runaway disk usage from misconfiguration.
const MAX_ALLOWED_STANDBY_PER_REPO: usize = 8;
const POOL_DIR_NAME: &str = "_pool";
const REPO_SOURCE_META_FILE: &str = ".repo-source";
const STANDBY_PREFIX: &str = "wt-";

/// Runtime configuration for [`WorktreePool`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePoolConfig {
    /// Enable warm detached-HEAD worktree pooling (default: `true`).
    pub enabled: bool,
    /// Maximum warm standbys kept per active repo (default: [`DEFAULT_MAX_STANDBY_PER_REPO`]).
    pub max_standby_per_repo: usize,
}

impl Default for WorktreePoolConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

impl WorktreePoolConfig {
    /// Build configuration honoring optional `ORBIT_WORKTREE_POOL` (`0`/`false` disables) and
    /// `ORBIT_WORKTREE_POOL_MAX` environment overrides.
    pub fn from_env() -> Self {
        let enabled = std::env::var("ORBIT_WORKTREE_POOL")
            .ok()
            .map(|v| {
                let norm = v.trim().to_ascii_lowercase();
                !matches!(norm.as_str(), "0" | "false" | "off" | "no")
            })
            .unwrap_or(true);
        let max_standby_per_repo = std::env::var("ORBIT_WORKTREE_POOL_MAX")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .map(|n| n.min(MAX_ALLOWED_STANDBY_PER_REPO))
            .unwrap_or(DEFAULT_MAX_STANDBY_PER_REPO);
        Self {
            enabled: enabled && max_standby_per_repo > 0,
            max_standby_per_repo,
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            max_standby_per_repo: 0,
        }
    }

    pub fn with_max_standby(max_standby_per_repo: usize) -> Self {
        let capped = max_standby_per_repo.min(MAX_ALLOWED_STANDBY_PER_REPO);
        Self {
            enabled: capped > 0,
            max_standby_per_repo: capped,
        }
    }
}

/// Snapshot of a single repository's warm standby pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePoolStats {
    pub repo_hash: String,
    pub pool_dir: PathBuf,
    pub available_standbys: usize,
    pub max_standby: usize,
    pub enabled: bool,
}

/// Manages repo-keyed warm detached-HEAD standbys under `$ORBIT_HOME/worktrees/_pool/<repo-hash>/`.
pub struct WorktreePool {
    orbit_home: PathBuf,
    config: WorktreePoolConfig,
    /// Per-repo async serialization lock keyed by `repo_hash`.
    repo_locks: RwLock<HashMap<String, Arc<AsyncMutex<()>>>>,
    /// Deduplicates concurrent background replenishment spawns per `repo_hash`.
    replenishing: RwLock<HashSet<String>>,
    /// Monotonic sequence counter for fencing stale background replenish tasks across cleanups.
    seq_counter: AtomicU64,
    /// Per-repo invalidation generation watermark keyed by `repo_hash`.
    repo_generations: StdMutex<HashMap<String, u64>>,
    shutting_down: AtomicBool,
}

impl WorktreePool {
    pub fn new(orbit_home: PathBuf, config: WorktreePoolConfig) -> Arc<Self> {
        Arc::new(Self {
            orbit_home,
            config,
            repo_locks: RwLock::new(HashMap::new()),
            replenishing: RwLock::new(HashSet::new()),
            seq_counter: AtomicU64::new(1),
            repo_generations: StdMutex::new(HashMap::new()),
            shutting_down: AtomicBool::new(false),
        })
    }

    fn bump_repo_generation(&self, repo_hash: &str) -> u64 {
        let next = self.seq_counter.fetch_add(1, Ordering::AcqRel) + 1;
        let mut gens = self
            .repo_generations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        gens.insert(repo_hash.to_string(), next);
        next
    }

    fn bump_all_repo_generations(&self) {
        let next = self.seq_counter.fetch_add(1, Ordering::AcqRel) + 1;
        let mut gens = self
            .repo_generations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for value in gens.values_mut() {
            *value = next;
        }
    }

    fn is_stale_generation(&self, repo_hash: &str, started_seq: u64) -> bool {
        let gens = self
            .repo_generations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        gens.get(repo_hash)
            .copied()
            .is_some_and(|invalidated_at| invalidated_at > started_seq)
    }

    pub fn with_default_config(orbit_home: PathBuf) -> Arc<Self> {
        Self::new(orbit_home, WorktreePoolConfig::default())
    }

    pub fn config(&self) -> &WorktreePoolConfig {
        &self.config
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
            && self.config.max_standby_per_repo > 0
            && !self.shutting_down.load(Ordering::Acquire)
    }

    pub fn pool_root(&self) -> PathBuf {
        self.orbit_home.join("worktrees").join(POOL_DIR_NAME)
    }

    async fn lock_for_repo(&self, repo_hash: &str) -> Arc<AsyncMutex<()>> {
        if let Some(existing) = self.repo_locks.read().await.get(repo_hash) {
            return Arc::clone(existing);
        }
        let mut guard = self.repo_locks.write().await;
        Arc::clone(
            guard
                .entry(repo_hash.to_string())
                .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
        )
    }

    /// Resolve a deterministic `(repo_hash, pool_dir)` key for `source_cwd` using its canonical
    /// Git common directory so sibling worktrees of the same repository share one pool.
    pub async fn repo_pool_dir(&self, source_cwd: &Path) -> anyhow::Result<(String, PathBuf)> {
        let common_dir = resolve_git_common_dir(source_cwd).await?;
        let repo_hash = hash_repo_key(&common_dir);
        let dir = self.pool_root().join(&repo_hash);
        Ok((repo_hash, dir))
    }

    /// Inspect current standby count for `source_cwd`.
    pub async fn inspect_repo_pool(&self, source_cwd: &Path) -> anyhow::Result<WorktreePoolStats> {
        let (repo_hash, pool_dir) = self.repo_pool_dir(source_cwd).await?;
        let standbys = list_standby_dirs(&pool_dir).await;
        Ok(WorktreePoolStats {
            repo_hash,
            pool_dir,
            available_standbys: standbys.len(),
            max_standby: self.config.max_standby_per_repo,
            enabled: self.is_enabled(),
        })
    }

    /// Pre-warm detached-HEAD standbys for `source_cwd` up to `max_standby_per_repo`.
    /// Returns the number of newly created standbys.
    pub async fn prewarm_repo(&self, source_cwd: &Path) -> anyhow::Result<usize> {
        let started_seq = self.seq_counter.fetch_add(1, Ordering::AcqRel);
        self.prewarm_repo_with_seq(source_cwd, started_seq).await
    }

    async fn prewarm_repo_with_seq(
        &self,
        source_cwd: &Path,
        started_seq: u64,
    ) -> anyhow::Result<usize> {
        if !self.is_enabled() {
            return Ok(0);
        }
        let (repo_hash, pool_dir) = self.repo_pool_dir(source_cwd).await?;
        if self.is_stale_generation(&repo_hash, started_seq) {
            return Ok(0);
        }
        let repo_lock = self.lock_for_repo(&repo_hash).await;
        let _guard = repo_lock.lock().await;

        if !self.is_enabled() || self.is_stale_generation(&repo_hash, started_seq) {
            return Ok(0);
        }

        // Ensure repo actually has a valid HEAD commit before creating detached standbys.
        if resolve_head_sha(source_cwd).await.is_none() {
            return Ok(0);
        }

        let _ = run_git_worktree_prune(source_cwd).await;
        tokio::fs::create_dir_all(&pool_dir).await?;
        write_repo_source_meta(&pool_dir, source_cwd).await;

        // Validate existing standbys: discard dirty/broken ones, trim any excess above cap.
        let mut clean_standbys = Vec::new();
        for candidate in list_standby_dirs(&pool_dir).await {
            if is_standby_clean(&candidate).await {
                clean_standbys.push(candidate);
            } else {
                warn!(
                    standby = %candidate.display(),
                    "Discarding dirty or invalid warm worktree standby during replenish"
                );
                discard_standby_worktree(Some(source_cwd), &candidate).await;
            }
        }
        while clean_standbys.len() > self.config.max_standby_per_repo {
            if let Some(extra) = clean_standbys.pop() {
                discard_standby_worktree(Some(source_cwd), &extra).await;
            }
        }

        let needed = self
            .config
            .max_standby_per_repo
            .saturating_sub(clean_standbys.len());
        let mut created = 0usize;
        for _ in 0..needed {
            if !self.is_enabled() || self.is_stale_generation(&repo_hash, started_seq) {
                break;
            }
            let standby_path = pool_dir.join(format!(
                "{STANDBY_PREFIX}{}",
                &Uuid::new_v4().simple().to_string()[..8]
            ));
            let out = git_cmd(source_cwd)
                .arg("worktree")
                .arg("add")
                .arg("--detach")
                .arg(&standby_path)
                .arg("HEAD")
                .output()
                .await?;
            if out.status.success() {
                if !self.is_enabled() || self.is_stale_generation(&repo_hash, started_seq) {
                    discard_standby_worktree(Some(source_cwd), &standby_path).await;
                    break;
                }
                created += 1;
                debug!(
                    repo_hash = %repo_hash,
                    standby = %standby_path.display(),
                    "Pre-warmed detached-HEAD worktree standby"
                );
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                warn!(
                    repo_hash = %repo_hash,
                    error = %stderr.trim(),
                    "Failed to pre-warm detached-HEAD worktree standby"
                );
                let _ = tokio::fs::remove_dir_all(&standby_path).await;
                let _ = run_git_worktree_prune(source_cwd).await;
                break;
            }
        }
        Ok(created)
    }

    /// Fire-and-forget background pool replenishment for an active repository (deduplicated).
    pub fn schedule_replenish(self: &Arc<Self>, source_cwd: PathBuf) {
        if !self.is_enabled() {
            return;
        }
        let scheduled_seq = self.seq_counter.fetch_add(1, Ordering::AcqRel);
        let pool = Arc::clone(self);
        tokio::spawn(async move {
            let Ok((repo_hash, _)) = pool.repo_pool_dir(&source_cwd).await else {
                return;
            };
            if !pool.is_enabled() || pool.is_stale_generation(&repo_hash, scheduled_seq) {
                return;
            }
            {
                let mut active = pool.replenishing.write().await;
                if !active.insert(repo_hash.clone()) {
                    return;
                }
            }
            let res = pool.prewarm_repo_with_seq(&source_cwd, scheduled_seq).await;
            pool.replenishing.write().await.remove(&repo_hash);
            match res {
                Ok(n) if n > 0 => {
                    info!(
                        repo = %source_cwd.display(),
                        warmed = n,
                        "Replenished warm worktree pool in background"
                    );
                }
                Err(e) => {
                    debug!(
                        repo = %source_cwd.display(),
                        error = %e,
                        "Background worktree pool replenish skipped"
                    );
                }
                _ => {}
            }
        });
    }

    /// Attempt to claim a clean warm standby for `WorktreeCreateMode::BranchOff`, moving it to
    /// `target_dir` and running fast `git checkout -b <branch> <base> --`.
    ///
    /// Returns `Ok(Some(target_dir))` on a warm hit, or `Ok(None)` when the pool is disabled,
    /// empty, dirty, or cannot satisfy the checkout (caller should fall back to `git worktree add`).
    pub async fn try_claim_branch_off(
        &self,
        source_cwd: &Path,
        target_dir: &Path,
        branch: &str,
        base_ref: Option<&str>,
    ) -> anyhow::Result<Option<PathBuf>> {
        if !self.is_enabled()
            || !is_valid_git_ref_name(branch)
            || path_exists_async(target_dir).await
        {
            return Ok(None);
        }
        let Some(resolved_base) = resolve_claim_base_ref(source_cwd, base_ref).await else {
            return Ok(None);
        };
        self.claim_and_checkout(
            source_cwd,
            target_dir,
            &["checkout", "-b", branch, &resolved_base, "--"],
        )
        .await
    }

    /// Attempt to claim a clean warm standby for `CheckoutBranch` or `CheckoutPr`, moving it to
    /// `target_dir` and running `git checkout <branch> --`.
    pub async fn try_claim_checkout_branch(
        &self,
        source_cwd: &Path,
        target_dir: &Path,
        branch: &str,
    ) -> anyhow::Result<Option<PathBuf>> {
        if !self.is_enabled()
            || !is_valid_git_ref_name(branch)
            || path_exists_async(target_dir).await
        {
            return Ok(None);
        }
        self.claim_and_checkout(source_cwd, target_dir, &["checkout", branch, "--"])
            .await
    }

    async fn claim_and_checkout(
        &self,
        source_cwd: &Path,
        target_dir: &Path,
        checkout_args: &[&str],
    ) -> anyhow::Result<Option<PathBuf>> {
        let Ok((repo_hash, pool_dir)) = self.repo_pool_dir(source_cwd).await else {
            return Ok(None);
        };
        let repo_lock = self.lock_for_repo(&repo_hash).await;
        let _guard = repo_lock.lock().await;

        if !self.is_enabled() || path_exists_async(target_dir).await {
            return Ok(None);
        }

        let candidates = list_standby_dirs(&pool_dir).await;
        if candidates.is_empty() {
            return Ok(None);
        }

        let _ = run_git_worktree_prune(source_cwd).await;

        for candidate in candidates {
            if !is_standby_clean(&candidate).await {
                warn!(
                    repo_hash = %repo_hash,
                    standby = %candidate.display(),
                    "Rejecting dirty or non-detached warm worktree standby"
                );
                discard_standby_worktree(Some(source_cwd), &candidate).await;
                continue;
            }

            if let Some(parent) = target_dir.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            let move_out = git_cmd(source_cwd)
                .arg("worktree")
                .arg("move")
                .arg(&candidate)
                .arg(target_dir)
                .output()
                .await;
            let moved_ok = move_out.as_ref().is_ok_and(|o| o.status.success());
            if !moved_ok {
                let err_msg = move_out
                    .as_ref()
                    .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())
                    .unwrap_or_else(|e| e.to_string());
                warn!(
                    standby = %candidate.display(),
                    target = %target_dir.display(),
                    error = %err_msg,
                    "git worktree move failed on pooled standby; discarding and falling back"
                );
                discard_standby_worktree(Some(source_cwd), &candidate).await;
                if path_exists_async(target_dir).await {
                    discard_standby_worktree(Some(source_cwd), target_dir).await;
                }
                continue;
            }

            let co_out = git_cmd(target_dir).args(checkout_args).output().await;
            if co_out.as_ref().is_ok_and(|o| o.status.success()) {
                info!(
                    repo_hash = %repo_hash,
                    target = %target_dir.display(),
                    "Claimed warm worktree from pool"
                );
                return Ok(Some(target_dir.to_path_buf()));
            }

            let co_err = co_out
                .as_ref()
                .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())
                .unwrap_or_else(|e| e.to_string());
            warn!(
                target = %target_dir.display(),
                error = %co_err,
                "Checkout on claimed warm worktree failed; cleaning up for cold fallback"
            );
            discard_standby_worktree(Some(source_cwd), target_dir).await;
            return Ok(None);
        }

        Ok(None)
    }

    /// Prune dirty/invalid standbys for a single repo while keeping clean standbys intact.
    pub async fn prune_repo_pool(&self, source_cwd: &Path) -> usize {
        let Ok((repo_hash, pool_dir)) = self.repo_pool_dir(source_cwd).await else {
            return 0;
        };
        let repo_lock = self.lock_for_repo(&repo_hash).await;
        let _guard = repo_lock.lock().await;

        let _ = run_git_worktree_prune(source_cwd).await;
        let mut removed = 0usize;
        for candidate in list_standby_dirs(&pool_dir).await {
            if !is_standby_clean(&candidate).await {
                discard_standby_worktree(Some(source_cwd), &candidate).await;
                removed += 1;
            }
        }
        removed
    }

    /// Remove all warm standbys for `source_cwd` (e.g. when the final workspace for a repo is archived).
    pub async fn cleanup_repo_pool(&self, source_cwd: &Path) -> usize {
        let Ok((repo_hash, pool_dir)) = self.repo_pool_dir(source_cwd).await else {
            return 0;
        };
        // Bump before and after acquiring the per-repo lock to fence off queued and in-flight replenish tasks.
        self.bump_repo_generation(&repo_hash);
        let repo_lock = self.lock_for_repo(&repo_hash).await;
        let _guard = repo_lock.lock().await;
        self.bump_repo_generation(&repo_hash);
        cleanup_pool_dir_locked(&pool_dir, Some(source_cwd)).await
    }

    /// Sweep `$ORBIT_HOME/worktrees/_pool/` on daemon startup to remove orphaned or dirty standbys
    /// left behind by a previous unclean exit.
    pub async fn prune_stale_standbys(&self) -> usize {
        let root = self.pool_root();
        let Ok(mut entries) = tokio::fs::read_dir(&root).await else {
            return 0;
        };
        let mut removed = 0usize;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let pool_dir = entry.path();
            if !tokio::fs::metadata(&pool_dir)
                .await
                .is_ok_and(|m| m.is_dir())
            {
                continue;
            }
            let Some(repo_hash) = pool_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(ToOwned::to_owned)
            else {
                continue;
            };
            let repo_lock = self.lock_for_repo(&repo_hash).await;
            let _guard = repo_lock.lock().await;

            let src_repo = match read_repo_source_meta(&pool_dir).await {
                Some(p) if path_exists_async(&p).await => Some(p),
                _ => None,
            };
            if src_repo.is_none() {
                removed += cleanup_pool_dir_locked(&pool_dir, None).await;
                continue;
            }
            if let Some(ref src) = src_repo {
                let _ = run_git_worktree_prune(src).await;
            }
            for standby in list_standby_dirs(&pool_dir).await {
                if !is_standby_clean(&standby).await {
                    discard_standby_worktree(src_repo.as_deref(), &standby).await;
                    removed += 1;
                }
            }
        }
        removed
    }

    /// Stop background replenishment and remove all pooled standbys across all repositories on
    /// daemon shutdown.
    pub async fn shutdown_cleanup(&self) -> usize {
        self.shutting_down.store(true, Ordering::Release);
        self.bump_all_repo_generations();
        let root = self.pool_root();
        let Ok(mut entries) = tokio::fs::read_dir(&root).await else {
            return 0;
        };
        let mut removed = 0usize;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let pool_dir = entry.path();
            if !tokio::fs::metadata(&pool_dir)
                .await
                .is_ok_and(|m| m.is_dir())
            {
                continue;
            }
            let Some(repo_hash) = pool_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(ToOwned::to_owned)
            else {
                continue;
            };
            self.bump_repo_generation(&repo_hash);
            let repo_lock = self.lock_for_repo(&repo_hash).await;
            let _guard = repo_lock.lock().await;
            let src_repo = match read_repo_source_meta(&pool_dir).await {
                Some(p) if path_exists_async(&p).await => Some(p),
                _ => None,
            };
            removed += cleanup_pool_dir_locked(&pool_dir, src_repo.as_deref()).await;
        }
        let _ = tokio::fs::remove_dir(&root).await;
        removed
    }
}

// --- Git & Filesystem Helpers ---

fn is_valid_git_ref_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && !name.starts_with('-') && !trimmed.starts_with('-')
}

async fn path_exists_async(path: &Path) -> bool {
    tokio::fs::try_exists(path).await.unwrap_or(false)
}

fn git_cmd(cwd: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        .kill_on_drop(true)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR");
    cmd
}

async fn resolve_git_common_dir(source_cwd: &Path) -> anyhow::Result<PathBuf> {
    let out = git_cmd(source_cwd)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!(
            "not a git repository ({}): {}",
            source_cwd.display(),
            stderr.trim()
        );
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let candidate = PathBuf::from(&raw);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        source_cwd.join(candidate)
    };
    Ok(crate::platform::canonicalize_clean_async(&resolved)
        .await
        .unwrap_or(resolved))
}

fn hash_repo_key(common_dir: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(common_dir.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

async fn resolve_head_sha(source_cwd: &Path) -> Option<String> {
    let out = git_cmd(source_cwd)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() { None } else { Some(sha) }
}

async fn resolve_claim_base_ref(source_cwd: &Path, base_ref: Option<&str>) -> Option<String> {
    if let Some(base) = base_ref.map(str::trim).filter(|b| !b.is_empty()) {
        if !is_valid_git_ref_name(base) {
            return None;
        }
        let verify_arg = format!("{base}^{{commit}}");
        let out = git_cmd(source_cwd)
            .args(["rev-parse", "--verify", &verify_arg])
            .output()
            .await
            .ok()?;
        if out.status.success() {
            return Some(base.to_string());
        }
        return None;
    }
    resolve_head_sha(source_cwd).await
}

async fn run_git_worktree_prune(source_cwd: &Path) -> bool {
    git_cmd(source_cwd)
        .args(["worktree", "prune"])
        .output()
        .await
        .is_ok_and(|o| o.status.success())
}

/// Verify a standby directory is a valid, detached-HEAD, completely clean Git worktree.
async fn is_standby_clean(standby_dir: &Path) -> bool {
    let is_dir = tokio::fs::metadata(standby_dir)
        .await
        .is_ok_and(|m| m.is_dir());
    if !is_dir || !path_exists_async(&standby_dir.join(".git")).await {
        return false;
    }

    // Must be inside a valid git work tree.
    let inside_ok = git_cmd(standby_dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .await
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true");
    if !inside_ok {
        return false;
    }

    // Must be detached HEAD (`symbolic-ref -q HEAD` exits non-zero when detached).
    let is_detached = git_cmd(standby_dir)
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
        .await
        .is_ok_and(|o| !o.status.success());
    if !is_detached {
        return false;
    }

    // Must have zero modified, staged, or untracked files.
    git_cmd(standby_dir)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .await
        .is_ok_and(|o| o.status.success() && o.stdout.is_empty())
}

async fn list_standby_dirs(pool_dir: &Path) -> Vec<PathBuf> {
    let Ok(mut entries) = tokio::fs::read_dir(pool_dir).await else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let matches_prefix = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(STANDBY_PREFIX));
        if matches_prefix && tokio::fs::metadata(&path).await.is_ok_and(|m| m.is_dir()) {
            dirs.push(path);
        }
    }
    dirs.sort();
    dirs
}

async fn write_repo_source_meta(pool_dir: &Path, source_cwd: &Path) {
    let canon = crate::platform::canonicalize_clean_async(source_cwd)
        .await
        .unwrap_or_else(|_| source_cwd.to_path_buf());
    let _ = tokio::fs::write(
        pool_dir.join(REPO_SOURCE_META_FILE),
        canon.to_string_lossy().as_bytes(),
    )
    .await;
}

async fn read_repo_source_meta(pool_dir: &Path) -> Option<PathBuf> {
    let raw = tokio::fs::read_to_string(pool_dir.join(REPO_SOURCE_META_FILE))
        .await
        .ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

async fn discard_standby_worktree(source_cwd: Option<&Path>, standby_dir: &Path) {
    if let Some(src) = source_cwd {
        let _ = git_cmd(src)
            .arg("worktree")
            .arg("remove")
            .arg("--force")
            .arg(standby_dir)
            .output()
            .await;
    }
    if path_exists_async(standby_dir).await {
        let _ = tokio::fs::remove_dir_all(standby_dir).await;
    }
    if let Some(src) = source_cwd {
        let _ = run_git_worktree_prune(src).await;
    }
}

async fn cleanup_pool_dir_locked(pool_dir: &Path, source_cwd: Option<&Path>) -> usize {
    let mut removed = 0usize;
    for standby in list_standby_dirs(pool_dir).await {
        discard_standby_worktree(source_cwd, &standby).await;
        removed += 1;
    }
    if let Some(src) = source_cwd {
        let _ = run_git_worktree_prune(src).await;
    }
    let _ = tokio::fs::remove_dir_all(pool_dir).await;
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit_wt_pool_{tag}_{}",
            &Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&dir).expect("mkdir temp root");
        dir
    }

    async fn run_git(cwd: &Path, args: &[&str]) -> String {
        let out = git_cmd(cwd).args(args).output().await.expect("spawn git");
        assert!(
            out.status.success(),
            "git {:?} failed in {}: {}",
            args,
            cwd.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    async fn init_temp_git_repo(repo_dir: &Path) {
        std::fs::create_dir_all(repo_dir).expect("mkdir repo");
        run_git(repo_dir, &["init", "-b", "main"]).await;
        run_git(
            repo_dir,
            &["config", "user.email", "orbit-test@example.com"],
        )
        .await;
        run_git(repo_dir, &["config", "user.name", "Orbit Test"]).await;
        std::fs::write(repo_dir.join("README.md"), "initial v1\n").expect("write readme");
        run_git(repo_dir, &["add", "README.md"]).await;
        run_git(repo_dir, &["commit", "-m", "initial commit"]).await;
    }

    #[tokio::test]
    async fn prewarm_fast_claim_branch_checkout_and_shutdown_cleanup() {
        let root = temp_root("claim");
        let home = root.join("orbit_home");
        let repo = root.join("demo-repo");
        init_temp_git_repo(&repo).await;

        let pool = WorktreePool::new(home.clone(), WorktreePoolConfig::with_max_standby(2));
        let created = pool.prewarm_repo(&repo).await.expect("prewarm");
        assert_eq!(created, 2);

        let stats = pool.inspect_repo_pool(&repo).await.expect("stats");
        assert_eq!(stats.available_standbys, 2);
        assert!(
            stats
                .pool_dir
                .starts_with(home.join("worktrees").join("_pool"))
        );

        // Advance HEAD on main after pre-warming to verify claimed standby tracks latest HEAD!
        std::fs::write(repo.join("README.md"), "updated v2\n").expect("write v2");
        run_git(&repo, &["add", "README.md"]).await;
        run_git(&repo, &["commit", "-m", "second commit"]).await;
        let latest_head = run_git(&repo, &["rev-parse", "HEAD"]).await;

        // Claim first warm standby for a new branch
        let target = home.join("worktrees").join("demo-repo-feat-fast");
        let claimed = pool
            .try_claim_branch_off(&repo, &target, "feat/fast", None)
            .await
            .expect("claim");
        assert_eq!(claimed.as_deref(), Some(target.as_path()));
        assert_eq!(
            run_git(&target, &["rev-parse", "--abbrev-ref", "HEAD"]).await,
            "feat/fast"
        );
        assert_eq!(run_git(&target, &["rev-parse", "HEAD"]).await, latest_head);
        assert_eq!(
            std::fs::read_to_string(target.join("README.md")).expect("read readme"),
            "updated v2\n"
        );

        // Pool should now have 1 standby remaining
        let stats_after = pool.inspect_repo_pool(&repo).await.expect("stats after");
        assert_eq!(stats_after.available_standbys, 1);

        // Prewarm tops it back up to 2
        let topped = pool.prewarm_repo(&repo).await.expect("replenish");
        assert_eq!(topped, 1);
        assert_eq!(
            pool.inspect_repo_pool(&repo)
                .await
                .expect("stats topped")
                .available_standbys,
            2
        );

        // Shutdown cleanup removes all pooled standbys while preserving active claimed worktree
        let cleaned = pool.shutdown_cleanup().await;
        assert_eq!(cleaned, 2);
        assert!(target.exists());
        assert_eq!(
            pool.inspect_repo_pool(&repo)
                .await
                .expect("stats post shutdown")
                .available_standbys,
            0
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn rejects_dirty_standby_and_falls_back_cleanly() {
        let root = temp_root("dirty");
        let home = root.join("orbit_home");
        let repo = root.join("dirty-repo");
        init_temp_git_repo(&repo).await;

        let pool = WorktreePool::new(home.clone(), WorktreePoolConfig::with_max_standby(2));
        assert_eq!(pool.prewarm_repo(&repo).await.expect("prewarm"), 2);

        let stats = pool.inspect_repo_pool(&repo).await.expect("stats");
        let standbys = list_standby_dirs(&stats.pool_dir).await;
        assert_eq!(standbys.len(), 2);

        // Pollute the first standby with an untracked file so it must be rejected and discarded.
        std::fs::write(standbys[0].join("stray.tmp"), "dirty").expect("dirty standby 0");

        // Claim should discard standbys[0] and cleanly claim standbys[1]
        let target1 = home.join("worktrees").join("dirty-repo-br1");
        let claimed1 = pool
            .try_claim_branch_off(&repo, &target1, "feat/clean-pick", None)
            .await
            .expect("claim 1");
        assert_eq!(claimed1.as_deref(), Some(target1.as_path()));
        assert!(!target1.join("stray.tmp").exists());
        assert!(!standbys[0].exists());

        // Pool is now empty -> next claim returns Ok(None) for graceful cold fallback
        let target2 = home.join("worktrees").join("dirty-repo-br2");
        let claimed2 = pool
            .try_claim_branch_off(&repo, &target2, "feat/fallback", None)
            .await
            .expect("claim 2");
        assert!(claimed2.is_none());
        assert!(!target2.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn disabled_pool_skips_prewarm_and_returns_none_on_claim() {
        let root = temp_root("disabled");
        let home = root.join("orbit_home");
        let repo = root.join("off-repo");
        init_temp_git_repo(&repo).await;

        let pool = WorktreePool::new(home.clone(), WorktreePoolConfig::disabled());
        assert_eq!(pool.prewarm_repo(&repo).await.expect("prewarm off"), 0);
        let target = home.join("worktrees").join("off-repo-br");
        assert!(
            pool.try_claim_branch_off(&repo, &target, "feat/off", None)
                .await
                .expect("claim off")
                .is_none()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn background_replenish_sibling_repo_hash_and_stale_pruning() {
        let root = temp_root("bg_prune");
        let home = root.join("orbit_home");
        let repo = root.join("mesh-repo");
        init_temp_git_repo(&repo).await;

        let pool = WorktreePool::new(home.clone(), WorktreePoolConfig::with_max_standby(2));
        pool.schedule_replenish(repo.clone());

        // Wait (bounded) for background replenishment to fill both standbys
        let mut ready = false;
        for _ in 0..80 {
            if pool
                .inspect_repo_pool(&repo)
                .await
                .expect("inspect")
                .available_standbys
                == 2
            {
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert!(ready, "background replenish should populate 2 standbys");

        // Claim a worktree and verify it resolves the exact same repo_hash as the main repo
        let wt = home.join("worktrees").join("mesh-repo-peer");
        let claimed = pool
            .try_claim_branch_off(&repo, &wt, "feat/peer", None)
            .await
            .expect("claim peer");
        assert_eq!(claimed.as_deref(), Some(wt.as_path()));
        let (hash_main, _) = pool.repo_pool_dir(&repo).await.expect("main hash");
        let (hash_wt, _) = pool.repo_pool_dir(&wt).await.expect("wt hash");
        assert_eq!(hash_main, hash_wt);

        // Create an orphaned pool entry whose source repo no longer exists; prune_stale_standbys cleans it
        let orphan_dir = pool.pool_root().join("deadbeef00001111");
        std::fs::create_dir_all(orphan_dir.join("wt-orphan")).expect("mkdir orphan");
        std::fs::write(
            orphan_dir.join(REPO_SOURCE_META_FILE),
            root.join("deleted-repo").to_string_lossy().as_bytes(),
        )
        .expect("write orphan meta");
        let pruned = pool.prune_stale_standbys().await;
        assert!(pruned >= 1);
        assert!(!orphan_dir.exists());

        // Cleanup repo pool removes remaining warm standbys cleanly
        let removed = pool.cleanup_repo_pool(&repo).await;
        assert_eq!(removed, 1);
        assert_eq!(
            pool.inspect_repo_pool(&repo)
                .await
                .expect("inspect after cleanup")
                .available_standbys,
            0
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn branch_off_never_overwrites_existing_branch_and_rejects_flag_names() {
        let root = temp_root("safe_branch");
        let home = root.join("orbit_home");
        let repo = root.join("safe-repo");
        init_temp_git_repo(&repo).await;

        // Create an existing feature branch with its own unique commit ahead of main.
        run_git(&repo, &["checkout", "-b", "feat/existing"]).await;
        std::fs::write(repo.join("FEATURE.txt"), "precious work\n").expect("write feature");
        run_git(&repo, &["add", "FEATURE.txt"]).await;
        run_git(&repo, &["commit", "-m", "feature commit"]).await;
        let feature_sha = run_git(&repo, &["rev-parse", "HEAD"]).await;
        run_git(&repo, &["checkout", "main"]).await;
        let main_sha = run_git(&repo, &["rev-parse", "HEAD"]).await;
        assert_ne!(feature_sha, main_sha);

        let pool = WorktreePool::new(home.clone(), WorktreePoolConfig::with_max_standby(2));
        assert_eq!(pool.prewarm_repo(&repo).await.expect("prewarm"), 2);

        // Flag-like branch names must be rejected without consuming standbys.
        let flag_target = home.join("worktrees").join("safe-repo-flag");
        assert!(
            pool.try_claim_branch_off(&repo, &flag_target, "--detach", Some("main"))
                .await
                .expect("reject -flag branch_off")
                .is_none()
        );
        assert!(
            pool.try_claim_checkout_branch(&repo, &flag_target, "-b")
                .await
                .expect("reject -flag checkout_branch")
                .is_none()
        );
        assert_eq!(
            pool.inspect_repo_pool(&repo)
                .await
                .expect("stats after flag rejection")
                .available_standbys,
            2
        );

        // Claiming an already-existing branch via BranchOff must fail safely without resetting it!
        let existing_target = home.join("worktrees").join("safe-repo-existing");
        let claimed = pool
            .try_claim_branch_off(&repo, &existing_target, "feat/existing", Some("main"))
            .await
            .expect("claim existing branch");
        assert!(
            claimed.is_none(),
            "BranchOff must not overwrite an existing branch"
        );
        assert!(!existing_target.exists());
        assert_eq!(
            run_git(&repo, &["rev-parse", "feat/existing"]).await,
            feature_sha,
            "existing branch commits must remain completely untouched"
        );

        let _ = pool.shutdown_cleanup().await;
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn cleanup_repo_pool_fences_stale_prewarm_generation() {
        let root = temp_root("gen_fence");
        let home = root.join("orbit_home");
        let repo = root.join("fence-repo");
        init_temp_git_repo(&repo).await;

        let pool = WorktreePool::new(home.clone(), WorktreePoolConfig::with_max_standby(2));
        let stale_seq = pool.seq_counter.fetch_add(1, Ordering::AcqRel);
        let _ = pool.cleanup_repo_pool(&repo).await;

        // A stale prewarm started before cleanup must not repopulate standbys.
        let created = pool
            .prewarm_repo_with_seq(&repo, stale_seq)
            .await
            .expect("stale prewarm");
        assert_eq!(created, 0);
        assert_eq!(
            pool.inspect_repo_pool(&repo)
                .await
                .expect("inspect after stale prewarm")
                .available_standbys,
            0
        );

        // A fresh prewarm started after cleanup succeeds normally.
        assert_eq!(pool.prewarm_repo(&repo).await.expect("fresh prewarm"), 2);
        let _ = pool.shutdown_cleanup().await;
        let _ = std::fs::remove_dir_all(&root);
    }
}
