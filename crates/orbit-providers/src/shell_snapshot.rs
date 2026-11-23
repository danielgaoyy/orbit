//! Login-shell startup environment snapshot cache (`$ORBIT_HOME/shell_snapshots/<hash>.json`).
//!
//! Avoids paying interactive login-shell initialization latency (`.zshrc`, `.bashrc`, `nvm`,
//! `pyenv`, `rbenv`, Homebrew, Cargo, Volta, etc.) on every provider CLI, side-question, or
//! workspace hook launch while guarding against common pitfalls:
//!
//! - **Non-interactive & bounded**: Probes run with `stdin=/dev/null`, `stderr=/dev/null`,
//!   anti-prompt guard env vars (preventing 1Password `op` biometric prompts, Oh-My-Zsh
//!   auto-update prompts, Powerlevel10k instant prompt, tmux autostart, and pinentry),
//!   random split markers around `env -0`, and a strict 3-second timeout.
//! - **Secret & volatile hygiene**: Persists neither transient secrets (`*_TOKEN*`, `*_KEY*`,
//!   `*_SECRET*`, `*_PASSWORD*`, `OP_SESSION_*`, URL credentials, etc.) nor shell-ephemeral
//!   variables (`SHLVL`, `PWD`, `OLDPWD`, `_`, `PS1`, `PROMPT`, terminal/session IDs, `ORBIT_*`),
//!   while safely preserving `SSH_AUTH_SOCK` and developer toolchain environment variables.
//! - **Atomic `0600` persistence & automatic invalidation**: Snapshots are written via temp-file +
//!   atomic rename with `0600` permissions (inside a `0700` directory) and automatically
//!   invalidated whenever `$SHELL`, any watched rc file's mtime/size (`~/.zshrc`, `~/.bashrc`,
//!   `~/.profile`, `~/.zprofile`, etc.), or the 30-minute TTL expires.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex as AsyncMutex;
#[cfg(unix)]
use tracing::{debug, warn};
use uuid::Uuid;

/// Current on-disk snapshot schema version.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Default snapshot time-to-live (30 minutes).
pub const DEFAULT_SNAPSHOT_TTL: Duration = Duration::from_secs(30 * 60);

/// Default hard timeout for probing the user's login shell (3 seconds).
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Cooldown after a failed or timed-out shell probe so a broken rc file cannot repeatedly stall.
const PROBE_FAILURE_BACKOFF: Duration = Duration::from_secs(30);

/// Upper bound on stdout bytes read from a single shell probe (1 MiB).
#[cfg(unix)]
const MAX_PROBE_STDOUT_BYTES: usize = 1024 * 1024;

/// Maximum length of a single cached environment variable value (32 KiB).
const MAX_ENV_VALUE_BYTES: usize = 32 * 1024;

/// Maximum number of environment variables stored in a single snapshot.
const MAX_ENV_VARS: usize = 512;

/// Minimum interval between repeated rc-file `stat()` checks on warm in-memory hits.
const DEFAULT_RC_STAT_THROTTLE: Duration = Duration::from_secs(2);

/// Maximum snapshot JSON file size loaded from disk (2 MiB).
const MAX_SNAPSHOT_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Maximum age of orphaned `.tmp` atomic-write artifacts before pruning (5 minutes).
const STALE_TMP_MAX_AGE: Duration = Duration::from_secs(5 * 60);

/// Startup files under `$HOME` whose mtimes/sizes are folded into the snapshot fingerprint.
pub const WATCHED_RC_FILES: &[&str] = &[
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".zlogin",
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".profile",
    ".config/fish/config.fish",
];

/// Probe-only guard environment variables that prevent interactive prompts during shell init.
const PROBE_GUARD_ENVS: &[(&str, &str)] = &[
    ("ORBIT_SHELL_PROBE", "1"),
    ("DISABLE_AUTO_UPDATE", "true"),
    ("ZSH_TMUX_AUTOSTART", "false"),
    ("POWERLEVEL9K_INSTANT_PROMPT", "off"),
    ("OP_BIOMETRIC_UNLOCK_ENABLED", "false"),
    ("SSH_ASKPASS_REQUIRE", "never"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("PINENTRY_USER_DATA", "USE_CURSES=0"),
    ("NONINTERACTIVE", "1"),
    ("TERM", "dumb"),
];

/// Environment variables explicitly safe to preserve even if they contain words like `AUTH`.
const SAFE_ALLOWLIST_KEYS: &[&str] = &[
    "PATH",
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
    "GPG_AGENT_INFO",
    "XDG_RUNTIME_DIR",
];

/// Volatile, session-specific, or terminal-bound variables that must never be cached.
const VOLATILE_EXACT_KEYS: &[&str] = &[
    "SHLVL",
    "PWD",
    "OLDPWD",
    "_",
    "PS1",
    "PS2",
    "PS3",
    "PS4",
    "PROMPT",
    "PROMPT2",
    "PROMPT3",
    "PROMPT4",
    "RPROMPT",
    "RPS1",
    "RPS2",
    "PROMPT_COMMAND",
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "COLORTERM",
    "LINES",
    "COLUMNS",
    "PPID",
    "UID",
    "EUID",
    "RANDOM",
    "SECONDS",
    "LINENO",
    "EPOCHSECONDS",
    "EPOCHREALTIME",
    "HISTCMD",
    "HISTFILE",
    "HISTSIZE",
    "SAVEHIST",
    "PIPESTATUS",
    "FUNCNAME",
    "BASH_SUBSHELL",
    "BASH_EXECUTION_STRING",
    "ZSH_SUBSHELL",
    "ZSH_EVAL_CONTEXT",
    "ZSH_EXECUTION_STRING",
    "SHELL_SESSION_ID",
    "SHELL_SESSION_FILE",
    "SHELL_SESSION_DID_INIT",
    "SHELL_SESSION_DID_HISTORY_CHECK",
    "STY",
    "TMUX",
    "TMUX_PANE",
    "TMUX_TMPDIR",
    "WINDOWID",
    "ITERM_SESSION_ID",
    "ITERM_PROFILE",
    "WEZTERM_PANE",
    "WEZTERM_UNIX_SOCKET",
    "WEZTERM_EXECUTABLE",
    "KITTY_WINDOW_ID",
    "KITTY_PID",
    "ALACRITTY_WINDOW_ID",
    "ALACRITTY_SOCKET",
    "VSCODE_IPC_HOOK_CLI",
    "VSCODE_GIT_IPC_HANDLE",
    "VSCODE_INJECTION",
    "VSCODE_PID",
    "VSCODE_CWD",
    "XPC_FLAGS",
    "XPC_SERVICE_NAME",
    "LaunchInstanceID",
    "SECURITYSESSIONID",
    "__CF_USER_TEXT_ENCODING",
    "__CFBundleIdentifier",
    "Apple_PubSub_Socket_Render",
    "NO_COLOR",
    "FORCE_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "GPG_TTY",
    "AGY_EFFORT",
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EFFORT_LEVEL",
    "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING",
    "CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS",
    "CLAUDE_CODE_SSE_PORT",
    "CODEX_THREAD_ID",
    "CODEX_SANDBOX",
    "CODEX_SANDBOX_NETWORK_DISABLED",
    "CONDA_SHLVL",
];

/// Prefixes for volatile/internal variables that must never be cached.
const VOLATILE_PREFIXES: &[&str] = &["ORBIT_", "BASH_FUNC_", "LC_TERMINAL"];

/// Case-insensitive prefixes for secret/password-manager session variables.
const SENSITIVE_PREFIXES_UPPER: &[&str] = &[
    "OP_SESSION_",
    "OP_SERVICE_ACCOUNT",
    "OP_CONNECT_",
    "ONEPASSWORD_",
    "1PASSWORD_",
    "BW_SESSION",
    "AWS_SESSION_",
    "AWS_SECURITY_",
];

/// Case-insensitive `_`-delimited key segments indicating secrets or transient credentials.
const SENSITIVE_SEGMENTS_UPPER: &[&str] = &[
    "SECRET",
    "SECRETS",
    "PASSWORD",
    "PASSWD",
    "PASSPHRASE",
    "PASS",
    "TOKEN",
    "TOKENS",
    "APIKEY",
    "PRIVATEKEY",
    "COOKIE",
    "COOKIES",
    "BEARER",
    "CREDENTIAL",
    "CREDENTIALS",
    "KEYRING",
    "PIN",
];

/// Adjacent `_`-delimited uppercase key segment pairs indicating secret keys/headers.
const SENSITIVE_SEGMENT_PAIRS_UPPER: &[(&str, &str)] = &[
    ("API", "KEY"),
    ("API", "KEYS"),
    ("PRIVATE", "KEY"),
    ("PRIVATE", "KEYS"),
    ("ACCESS", "KEY"),
    ("ACCESS", "KEYS"),
    ("SECRET", "KEY"),
    ("SECRET", "KEYS"),
    ("SIGNING", "KEY"),
    ("ENCRYPTION", "KEY"),
    ("MASTER", "KEY"),
    ("AUTH", "KEY"),
    ("AUTH", "HEADER"),
    ("AUTH", "CONFIG"),
    ("SESSION", "KEY"),
    ("SESSION", "ID"),
    ("CLIENT", "SECRET"),
];

/// File metadata stamp used to detect modifications to watched shell rc files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RcFileStamp {
    pub mtime_secs: u64,
    pub mtime_nanos: u32,
    pub size: u64,
}

/// On-disk atomic JSON representation persisted under `$ORBIT_HOME/shell_snapshots/<hash>.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedShellSnapshot {
    pub version: u32,
    pub shell: String,
    pub fingerprint: String,
    pub created_at_unix: u64,
    pub rc_mtimes: BTreeMap<String, Option<RcFileStamp>>,
    pub env: BTreeMap<String, String>,
}

/// Diagnostics and cache telemetry for `orbit doctor` and daemon observability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShellSnapshotStats {
    pub hits: u64,
    pub misses: u64,
    pub disk_hits: u64,
    pub probe_errors: u64,
    pub shell: String,
    pub age_secs: Option<u64>,
    pub var_count: usize,
    pub warm: bool,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedMemoryEntry {
    shell: String,
    raw_shell_env: Option<String>,
    fingerprint: String,
    created_at_unix: u64,
    created_at_instant: Option<Instant>,
    last_rc_check_at: Instant,
    env: HashMap<String, String>,
}

#[derive(Debug, Default)]
struct CacheMemoryState {
    entry: Option<CachedMemoryEntry>,
    last_failure_at: Option<Instant>,
}

/// In-memory (`RwLock`) + atomic on-disk (`$ORBIT_HOME/shell_snapshots/<hash>.json`) cache
/// of the user's login shell environment.
pub struct ShellSnapshotCache {
    cache_dir_override: Option<PathBuf>,
    home_dir_override: Option<PathBuf>,
    shell_override: Option<String>,
    ttl: Duration,
    probe_timeout: Duration,
    failure_backoff: Duration,
    rc_stat_throttle: Duration,
    memory: RwLock<CacheMemoryState>,
    refresh_lock: AsyncMutex<()>,
    warmup_in_flight: AtomicBool,
    hits: AtomicU64,
    misses: AtomicU64,
    disk_hits: AtomicU64,
    probe_errors: AtomicU64,
}

static GLOBAL_SNAPSHOT_CACHE: OnceLock<ShellSnapshotCache> = OnceLock::new();
static ORBIT_HOME_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

/// Register the active `$ORBIT_HOME` path (e.g. from `orbit daemon --home <path>` or `orbit doctor`)
/// so the global shell snapshot cache stores snapshots in `<orbit_home>/shell_snapshots/`.
pub fn set_snapshot_orbit_home(orbit_home: &Path) {
    let lock = ORBIT_HOME_OVERRIDE.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = lock.write() {
        *guard = Some(orbit_home.to_path_buf());
    }
}

fn is_orbit_home_configured() -> bool {
    if let Some(lock) = ORBIT_HOME_OVERRIDE.get()
        && let Ok(guard) = lock.read()
        && guard.is_some()
    {
        return true;
    }
    std::env::var_os("ORBIT_HOME").is_some_and(|v| !v.is_empty())
}

/// Access the process-wide singleton [`ShellSnapshotCache`].
pub fn global_shell_snapshot_cache() -> &'static ShellSnapshotCache {
    GLOBAL_SNAPSHOT_CACHE.get_or_init(ShellSnapshotCache::new_global)
}

#[cfg(unix)]
struct WarmupFlagGuard<'a>(&'a AtomicBool);

#[cfg(unix)]
impl Drop for WarmupFlagGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl ShellSnapshotCache {
    fn new_global() -> Self {
        Self {
            cache_dir_override: None,
            home_dir_override: None,
            shell_override: None,
            ttl: DEFAULT_SNAPSHOT_TTL,
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
            failure_backoff: PROBE_FAILURE_BACKOFF,
            rc_stat_throttle: DEFAULT_RC_STAT_THROTTLE,
            memory: RwLock::new(CacheMemoryState::default()),
            refresh_lock: AsyncMutex::new(()),
            warmup_in_flight: AtomicBool::new(false),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            disk_hits: AtomicU64::new(0),
            probe_errors: AtomicU64::new(0),
        }
    }

    /// Create an isolated cache instance with explicit directories, shell, TTL, and timeout.
    pub fn with_config(
        cache_dir: PathBuf,
        home_dir: PathBuf,
        shell: String,
        ttl: Duration,
        probe_timeout: Duration,
    ) -> Self {
        Self {
            cache_dir_override: Some(cache_dir),
            home_dir_override: Some(home_dir),
            shell_override: Some(shell),
            ttl,
            probe_timeout,
            failure_backoff: PROBE_FAILURE_BACKOFF,
            rc_stat_throttle: DEFAULT_RC_STAT_THROTTLE,
            memory: RwLock::new(CacheMemoryState::default()),
            refresh_lock: AsyncMutex::new(()),
            warmup_in_flight: AtomicBool::new(false),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            disk_hits: AtomicU64::new(0),
            probe_errors: AtomicU64::new(0),
        }
    }

    /// Override the rc-file `stat()` throttling window used on warm in-memory hits.
    pub fn with_rc_stat_throttle(mut self, throttle: Duration) -> Self {
        self.rc_stat_throttle = throttle;
        self
    }

    fn resolve_cache_dir(&self) -> PathBuf {
        if let Some(ref dir) = self.cache_dir_override {
            return dir.clone();
        }
        if let Some(lock) = ORBIT_HOME_OVERRIDE.get()
            && let Ok(guard) = lock.read()
            && let Some(ref home) = *guard
        {
            return home.join("shell_snapshots");
        }
        if let Some(env_home) = std::env::var_os("ORBIT_HOME")
            && !env_home.is_empty()
        {
            return PathBuf::from(env_home).join("shell_snapshots");
        }
        resolve_default_home_dir()
            .join(".orbit")
            .join("shell_snapshots")
    }

    fn resolve_home_dir(&self) -> PathBuf {
        self.home_dir_override
            .clone()
            .unwrap_or_else(resolve_default_home_dir)
    }

    fn resolve_shell_path(&self) -> String {
        if let Some(ref shell) = self.shell_override {
            return shell.clone();
        }
        detect_login_shell()
    }

    fn current_raw_shell_env(&self) -> Option<String> {
        if self.shell_override.is_some() {
            None
        } else {
            std::env::var("SHELL").ok()
        }
    }

    fn current_key(&self) -> (String, BTreeMap<String, Option<RcFileStamp>>, String) {
        let shell = self.resolve_shell_path();
        let home = self.resolve_home_dir();
        let rc_mtimes = collect_rc_file_stamps(&home);
        let fingerprint = compute_snapshot_fingerprint(&shell, &rc_mtimes);
        (shell, rc_mtimes, fingerprint)
    }

    fn is_entry_time_fresh(&self, entry: &CachedMemoryEntry, now_unix: u64) -> bool {
        if !is_timestamp_within_ttl(entry.created_at_unix, now_unix, self.ttl) {
            return false;
        }
        entry
            .created_at_instant
            .is_none_or(|inst| inst.elapsed() < self.ttl)
    }

    fn is_entry_fresh(&self, entry: &CachedMemoryEntry, fingerprint: &str, now_unix: u64) -> bool {
        entry.fingerprint == fingerprint && self.is_entry_time_fresh(entry, now_unix)
    }

    /// Zero-syscall fast path: serves a fresh in-memory snapshot without re-statting rc files
    /// when checked within `self.rc_stat_throttle`.
    fn try_memory_hit_throttled(&self) -> Option<HashMap<String, String>> {
        if self.rc_stat_throttle.is_zero() {
            return None;
        }
        let now_unix = now_unix_secs();
        let raw_shell_env = self.current_raw_shell_env();
        let guard = self.memory.read().ok()?;
        let entry = guard.entry.as_ref()?;
        if !self.is_entry_time_fresh(entry, now_unix) {
            return None;
        }
        if entry.last_rc_check_at.elapsed() >= self.rc_stat_throttle {
            return None;
        }
        if self.shell_override.is_none() && entry.raw_shell_env != raw_shell_env {
            return None;
        }
        self.hits.fetch_add(1, Ordering::Relaxed);
        Some(entry.env.clone())
    }

    fn check_memory_with_fingerprint(
        &self,
        fingerprint: &str,
        now_unix: u64,
    ) -> Option<HashMap<String, String>> {
        let matched = {
            let guard = self.memory.read().ok()?;
            let entry = guard.entry.as_ref()?;
            if self.is_entry_fresh(entry, fingerprint, now_unix) {
                Some(entry.env.clone())
            } else {
                None
            }
        };
        if let Some(env) = matched {
            if let Ok(mut guard) = self.memory.write()
                && let Some(ref mut entry) = guard.entry
                && entry.fingerprint == fingerprint
            {
                entry.last_rc_check_at = Instant::now();
            }
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Some(env);
        }
        None
    }

    /// Synchronous fast-path: returns a valid snapshot from memory or on-disk cache immediately
    /// without ever spawning a shell. Returns `None` on cold miss.
    pub fn resolve_env_cached(&self) -> Option<HashMap<String, String>> {
        if self.cache_dir_override.is_none() && is_shell_snapshot_disabled() {
            return None;
        }

        // 1. Throttled in-memory fast-path (avoids redundant rc-file stat calls on hot paths)
        if let Some(env) = self.try_memory_hit_throttled() {
            return Some(env);
        }

        let (shell, rc_mtimes, fingerprint) = self.current_key();
        let now_unix = now_unix_secs();

        // 2. Verified in-memory hit after re-checking rc file stamps
        if let Some(env) = self.check_memory_with_fingerprint(&fingerprint, now_unix) {
            return Some(env);
        }

        // 3. On-disk fast-path (shared across daemon & short-lived CLI processes)
        if let Some(disk_snap) = self.load_from_disk_if_valid(&fingerprint, &shell, &rc_mtimes) {
            let env_map: HashMap<String, String> = disk_snap.env.into_iter().collect();
            let raw_shell_env = self.current_raw_shell_env();
            if let Ok(mut guard) = self.memory.write() {
                guard.entry = Some(CachedMemoryEntry {
                    shell: disk_snap.shell,
                    raw_shell_env,
                    fingerprint: disk_snap.fingerprint,
                    created_at_unix: disk_snap.created_at_unix,
                    created_at_instant: None,
                    last_rc_check_at: Instant::now(),
                    env: env_map.clone(),
                });
                guard.last_failure_at = None;
            }
            self.hits.fetch_add(1, Ordering::Relaxed);
            self.disk_hits.fetch_add(1, Ordering::Relaxed);
            return Some(env_map);
        }

        None
    }

    #[cfg(unix)]
    async fn resolve_key_and_cached_async(
        &self,
    ) -> (
        Option<HashMap<String, String>>,
        String,
        BTreeMap<String, Option<RcFileStamp>>,
        String,
    ) {
        let shell_override = self.shell_override.clone();
        let home_dir = self.resolve_home_dir();
        let (shell, rc_mtimes, fingerprint) = tokio::task::spawn_blocking(move || {
            let shell = shell_override.unwrap_or_else(detect_login_shell);
            let rc_mtimes = collect_rc_file_stamps(&home_dir);
            let fingerprint = compute_snapshot_fingerprint(&shell, &rc_mtimes);
            (shell, rc_mtimes, fingerprint)
        })
        .await
        .unwrap_or_else(|_| self.current_key());

        let now_unix = now_unix_secs();
        if let Some(env) = self.check_memory_with_fingerprint(&fingerprint, now_unix) {
            return (Some(env), shell, rc_mtimes, fingerprint);
        }

        let cache_dir = self.resolve_cache_dir();
        let fp_for_disk = fingerprint.clone();
        let shell_for_disk = shell.clone();
        let rc_for_disk = rc_mtimes.clone();
        let ttl = self.ttl;
        let disk_snap = tokio::task::spawn_blocking(move || {
            load_from_disk_if_valid_in(&cache_dir, &fp_for_disk, &shell_for_disk, &rc_for_disk, ttl)
        })
        .await
        .ok()
        .flatten();

        if let Some(disk_snap) = disk_snap {
            let env_map: HashMap<String, String> = disk_snap.env.into_iter().collect();
            let raw_shell_env = self.current_raw_shell_env();
            if let Ok(mut guard) = self.memory.write() {
                guard.entry = Some(CachedMemoryEntry {
                    shell: disk_snap.shell,
                    raw_shell_env,
                    fingerprint: disk_snap.fingerprint,
                    created_at_unix: disk_snap.created_at_unix,
                    created_at_instant: None,
                    last_rc_check_at: Instant::now(),
                    env: env_map.clone(),
                });
                guard.last_failure_at = None;
            }
            self.hits.fetch_add(1, Ordering::Relaxed);
            self.disk_hits.fetch_add(1, Ordering::Relaxed);
            return (Some(env_map), shell, rc_mtimes, fingerprint);
        }

        (None, shell, rc_mtimes, fingerprint)
    }

    /// Async full resolution: checks memory -> disk -> bounded login-shell probe (with
    /// single-flight deduplication so concurrent callers never spawn redundant shells).
    pub async fn resolve_env(&self) -> HashMap<String, String> {
        if self.cache_dir_override.is_none() && is_shell_snapshot_disabled() {
            return HashMap::new();
        }
        #[cfg(not(unix))]
        {
            return HashMap::new();
        }

        #[cfg(unix)]
        {
            if let Some(cached) = self.try_memory_hit_throttled() {
                return cached;
            }

            let (cached, _, _, _) = self.resolve_key_and_cached_async().await;
            if let Some(cached) = cached {
                return cached;
            }

            // Coalesce concurrent cold probes.
            let _refresh_guard = self.refresh_lock.lock().await;

            if let Some(cached) = self.try_memory_hit_throttled() {
                return cached;
            }

            let (cached, shell, rc_mtimes, fingerprint) = self.resolve_key_and_cached_async().await;
            if let Some(cached) = cached {
                return cached;
            }

            // Respect negative backoff after recent probe failure/timeout.
            if let Ok(guard) = self.memory.read()
                && let Some(failed_at) = guard.last_failure_at
                && failed_at.elapsed() < self.failure_backoff
            {
                return guard
                    .entry
                    .as_ref()
                    .map(|e| e.env.clone())
                    .unwrap_or_default();
            }

            if is_unsupported_non_posix_shell(&shell) {
                debug!(
                    shell = %shell,
                    "Skipping login-shell snapshot probe for unsupported non-POSIX shell"
                );
                return HashMap::new();
            }

            self.misses.fetch_add(1, Ordering::Relaxed);

            let home_dir = self.resolve_home_dir();

            match probe_login_shell_env(&shell, &home_dir, self.probe_timeout).await {
                Ok(env_map) if !env_map.is_empty() => {
                    let created_at_unix = now_unix_secs();
                    let persisted = PersistedShellSnapshot {
                        version: SNAPSHOT_SCHEMA_VERSION,
                        shell: shell.clone(),
                        fingerprint: fingerprint.clone(),
                        created_at_unix,
                        rc_mtimes,
                        env: env_map
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    };

                    let cache_dir = self.resolve_cache_dir();
                    let fp_for_prune = fingerprint.clone();
                    let shell_for_prune = shell.clone();
                    let ttl = self.ttl;
                    let save_res = tokio::task::spawn_blocking(move || {
                        let res = save_to_disk_atomic_in(&cache_dir, &persisted);
                        if res.is_ok() {
                            prune_stale_disk_snapshots_in(
                                &cache_dir,
                                &fp_for_prune,
                                &shell_for_prune,
                                ttl,
                            );
                        }
                        res
                    })
                    .await;

                    match save_res {
                        Ok(Err(err)) => {
                            warn!(error = %err, "Failed to persist shell environment snapshot to disk");
                        }
                        Err(err) => {
                            warn!(error = %err, "Shell snapshot disk persistence task failed");
                        }
                        Ok(Ok(_)) => {}
                    }

                    let raw_shell_env = self.current_raw_shell_env();
                    let now_inst = Instant::now();
                    if let Ok(mut guard) = self.memory.write() {
                        guard.entry = Some(CachedMemoryEntry {
                            shell,
                            raw_shell_env,
                            fingerprint,
                            created_at_unix,
                            created_at_instant: Some(now_inst),
                            last_rc_check_at: now_inst,
                            env: env_map.clone(),
                        });
                        guard.last_failure_at = None;
                    }
                    env_map
                }
                Ok(_) => {
                    self.probe_errors.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut guard) = self.memory.write() {
                        guard.last_failure_at = Some(Instant::now());
                        return guard
                            .entry
                            .as_ref()
                            .map(|e| e.env.clone())
                            .unwrap_or_default();
                    }
                    HashMap::new()
                }
                Err(err) => {
                    self.probe_errors.fetch_add(1, Ordering::Relaxed);
                    warn!(shell = %shell, error = %err, "Login shell environment probe failed; falling back to inherited env");
                    if let Ok(mut guard) = self.memory.write() {
                        guard.last_failure_at = Some(Instant::now());
                        return guard
                            .entry
                            .as_ref()
                            .map(|e| e.env.clone())
                            .unwrap_or_default();
                    }
                    HashMap::new()
                }
            }
        }
    }

    /// Trigger a non-blocking, deduplicated background warm-up when running inside a Tokio runtime.
    pub fn warm_in_background(&'static self) {
        if self.cache_dir_override.is_none() && is_shell_snapshot_disabled() {
            return;
        }
        #[cfg(not(unix))]
        {
            return;
        }
        #[cfg(unix)]
        {
            if let Ok(guard) = self.memory.read()
                && let Some(failed_at) = guard.last_failure_at
                && failed_at.elapsed() < self.failure_backoff
            {
                return;
            }
            let Ok(handle) = tokio::runtime::Handle::try_current() else {
                return;
            };
            if self
                .warmup_in_flight
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return;
            }
            handle.spawn(async move {
                let _flag_guard = WarmupFlagGuard(&self.warmup_in_flight);
                let env = self.resolve_env().await;
                debug!(
                    var_count = env.len(),
                    "Background login shell environment snapshot warm-up finished"
                );
            });
        }
    }

    /// Clear in-memory snapshot, reset failure backoff, and remove persisted snapshot files.
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.memory.write() {
            guard.entry = None;
            guard.last_failure_at = None;
        }
        let cache_dir = self.resolve_cache_dir();
        if let Ok(entries) = std::fs::read_dir(&cache_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_snapshot_artifact = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|ext| ext == "json" || ext == "tmp");
                if is_snapshot_artifact && path.is_file() {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    /// Collect current cache telemetry (`hit/miss/shell/age/var_count`) for diagnostics.
    pub fn stats(&self) -> ShellSnapshotStats {
        let now_unix = now_unix_secs();
        let (shell, age_secs, var_count, warm, fingerprint) = match self.memory.read() {
            Ok(guard) => match guard.entry.as_ref() {
                Some(entry) => {
                    let is_warm = self.is_entry_time_fresh(entry, now_unix);
                    (
                        entry.shell.clone(),
                        Some(now_unix.saturating_sub(entry.created_at_unix)),
                        entry.env.len(),
                        is_warm,
                        Some(entry.fingerprint.clone()),
                    )
                }
                None => (self.resolve_shell_path(), None, 0, false, None),
            },
            Err(_) => (self.resolve_shell_path(), None, 0, false, None),
        };

        ShellSnapshotStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            disk_hits: self.disk_hits.load(Ordering::Relaxed),
            probe_errors: self.probe_errors.load(Ordering::Relaxed),
            shell,
            age_secs,
            var_count,
            warm,
            fingerprint,
        }
    }

    /// Snapshot file path for a given fingerprint hash: `<cache_dir>/<hash>.json`.
    pub fn snapshot_file_path(&self, fingerprint: &str) -> PathBuf {
        self.resolve_cache_dir().join(format!("{fingerprint}.json"))
    }

    /// Load and validate an on-disk snapshot matching `fingerprint`, `shell`, `rc_mtimes`, and TTL.
    pub fn load_from_disk_if_valid(
        &self,
        fingerprint: &str,
        shell: &str,
        rc_mtimes: &BTreeMap<String, Option<RcFileStamp>>,
    ) -> Option<PersistedShellSnapshot> {
        let cache_dir = self.resolve_cache_dir();
        load_from_disk_if_valid_in(&cache_dir, fingerprint, shell, rc_mtimes, self.ttl)
    }

    /// Atomically write a snapshot JSON file with `0600` permissions (and `0700` parent dir on Unix).
    pub fn save_to_disk_atomic(
        &self,
        snapshot: &PersistedShellSnapshot,
    ) -> std::io::Result<PathBuf> {
        let cache_dir = self.resolve_cache_dir();
        save_to_disk_atomic_in(&cache_dir, snapshot)
    }
}

fn load_from_disk_if_valid_in(
    cache_dir: &Path,
    fingerprint: &str,
    shell: &str,
    rc_mtimes: &BTreeMap<String, Option<RcFileStamp>>,
    ttl: Duration,
) -> Option<PersistedShellSnapshot> {
    let path = cache_dir.join(format!("{fingerprint}.json"));
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_SNAPSHOT_FILE_BYTES {
        return None;
    }
    let raw = std::fs::read_to_string(&path).ok()?;
    let mut parsed: PersistedShellSnapshot = serde_json::from_str(&raw).ok()?;
    if parsed.version != SNAPSHOT_SCHEMA_VERSION {
        return None;
    }
    if parsed.shell != shell || parsed.fingerprint != fingerprint || &parsed.rc_mtimes != rc_mtimes
    {
        return None;
    }
    if !is_timestamp_within_ttl(parsed.created_at_unix, now_unix_secs(), ttl) {
        return None;
    }
    // Defense-in-depth: re-apply secret/volatile/probe-guard filter when loading from disk.
    let filtered = filter_snapshot_env(parsed.env.into_iter().collect(), &HashSet::new());
    if filtered.is_empty() {
        return None;
    }
    parsed.env = filtered.into_iter().collect();
    Some(parsed)
}

fn save_to_disk_atomic_in(
    cache_dir: &Path,
    snapshot: &PersistedShellSnapshot,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(cache_dir)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(cache_dir, std::fs::Permissions::from_mode(0o700));
    }

    let final_path = cache_dir.join(format!("{}.json", snapshot.fingerprint));
    let tmp_name = format!(
        ".snap-{}-{}-{}.tmp",
        snapshot.fingerprint,
        std::process::id(),
        &Uuid::new_v4().simple().to_string()[..8]
    );
    let tmp_path = cache_dir.join(tmp_name);
    let payload = serde_json::to_vec_pretty(snapshot).map_err(std::io::Error::other)?;

    let write_result: std::io::Result<()> = (|| {
        let mut file = create_private_file(&tmp_path)?;
        file.write_all(&payload)?;
        file.flush()?;
        let _ = file.sync_all();
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(())
    })();

    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    Ok(final_path)
}

fn prune_stale_disk_snapshots_in(
    cache_dir: &Path,
    keep_fingerprint: &str,
    shell: &str,
    ttl: Duration,
) {
    let keep_name = format!("{keep_fingerprint}.json");
    let now_unix = now_unix_secs();
    let max_stale_ttl = ttl.saturating_mul(2);
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_name.ends_with(".tmp") {
            let is_stale_tmp = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age > STALE_TMP_MAX_AGE);
            if is_stale_tmp {
                let _ = std::fs::remove_file(&path);
            }
            continue;
        }
        if file_name == keep_name || !file_name.ends_with(".json") {
            continue;
        }
        let should_remove = match std::fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file() && m.len() <= MAX_SNAPSHOT_FILE_BYTES)
            .and_then(|_| std::fs::read_to_string(&path).ok())
            .and_then(|raw| serde_json::from_str::<PersistedShellSnapshot>(&raw).ok())
        {
            Some(snap) => {
                snap.shell == shell
                    || !is_timestamp_within_ttl(snap.created_at_unix, now_unix, max_stale_ttl)
            }
            None => true,
        };
        if should_remove {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Resolve the cached login-shell environment asynchronously (memory -> disk -> 3s bounded probe).
pub async fn resolve_login_shell_env() -> HashMap<String, String> {
    global_shell_snapshot_cache().resolve_env().await
}

/// Non-blocking synchronous fast-path returning cached login-shell env (from memory or disk).
/// On cold miss, triggers a deduplicated background warm-up (when `$ORBIT_HOME` is configured and
/// a Tokio runtime is active) and returns `None` immediately so command launches never stall.
pub fn resolve_login_shell_env_cached() -> Option<HashMap<String, String>> {
    let cache = global_shell_snapshot_cache();
    if let Some(env) = cache.resolve_env_cached() {
        return Some(env);
    }
    if cache.cache_dir_override.is_some() || is_orbit_home_configured() {
        cache.warm_in_background();
    }
    None
}

/// Trigger non-blocking background warm-up of the global login-shell snapshot cache.
pub fn warm_login_shell_env_in_background() {
    global_shell_snapshot_cache().warm_in_background();
}

/// Invalidate both in-memory and on-disk shell snapshots.
pub fn invalidate_shell_snapshot() {
    global_shell_snapshot_cache().invalidate();
}

/// Return cache telemetry (`hits`, `misses`, `shell`, `age_secs`, `var_count`, etc.).
pub fn shell_snapshot_stats() -> ShellSnapshotStats {
    global_shell_snapshot_cache().stats()
}

/// Merge cached login-shell `PATH` & non-conflicting environment variables into a
/// `tokio::process::Command` without overwriting any explicit caller env vars.
/// Returns `true` when a warm snapshot was applied.
pub fn apply_cached_shell_env(cmd: &mut tokio::process::Command) -> bool {
    apply_cached_std_shell_env(cmd.as_std_mut())
}

/// Merge cached login-shell `PATH` & non-conflicting environment variables into a
/// `std::process::Command` without overwriting any explicit caller env vars.
/// Returns `true` when a warm snapshot was applied.
pub fn apply_cached_std_shell_env(cmd: &mut std::process::Command) -> bool {
    if let Some(snapshot) = resolve_login_shell_env_cached()
        && !snapshot.is_empty()
    {
        apply_shell_env_to_std_command(cmd, &snapshot);
        return true;
    }
    false
}

/// Apply an explicit snapshot map onto a `tokio::process::Command` without overwriting
/// caller-specified env vars or non-empty inherited process env vars (except merged `PATH`).
pub fn apply_shell_env_to_command(
    cmd: &mut tokio::process::Command,
    snapshot: &HashMap<String, String>,
) {
    apply_shell_env_to_std_command(cmd.as_std_mut(), snapshot);
}

/// Apply an explicit snapshot map onto a `std::process::Command` without overwriting
/// caller-specified env vars (`cmd.env(...)` / `cmd.env_remove(...)`) or non-empty inherited
/// process env vars (except merged `PATH`).
pub fn apply_shell_env_to_std_command(
    cmd: &mut std::process::Command,
    snapshot: &HashMap<String, String>,
) {
    if snapshot.is_empty() {
        return;
    }

    let explicit_keys: HashSet<OsString> = cmd.get_envs().map(|(k, _)| k.to_os_string()).collect();

    if !explicit_keys.contains(OsStr::new("PATH"))
        && let Some(login_path) = snapshot.get("PATH")
        && let Some(merged) = merge_paths(login_path, std::env::var_os("PATH").as_deref())
    {
        cmd.env("PATH", merged);
    }

    for (key, value) in snapshot {
        if key == "PATH" {
            continue;
        }
        if explicit_keys.contains(OsStr::new(key)) {
            continue;
        }
        if std::env::var_os(key).is_some_and(|existing| !existing.is_empty()) {
            continue;
        }
        cmd.env(key, value);
    }
}

/// Merge cached login-shell `PATH` & non-conflicting environment variables into a target map
/// (such as [`orbit_shim::ShimSpawnSpec::env`]) without overwriting any explicit keys already
/// present in `target`. Returns `true` when a warm snapshot was applied.
pub fn merge_cached_shell_env_into_map(target: &mut HashMap<String, String>) -> bool {
    if let Some(snapshot) = resolve_login_shell_env_cached()
        && !snapshot.is_empty()
    {
        merge_shell_env_into_map(&snapshot, target);
        return true;
    }
    false
}

/// Merge a specific snapshot map into `target` without overwriting explicit entries in `target`
/// or duplicating non-empty inherited parent env vars (except merged `PATH`).
pub fn merge_shell_env_into_map(
    snapshot: &HashMap<String, String>,
    target: &mut HashMap<String, String>,
) {
    if snapshot.is_empty() {
        return;
    }

    if !target.contains_key("PATH")
        && let Some(login_path) = snapshot.get("PATH")
        && let Some(merged) = merge_paths(login_path, std::env::var_os("PATH").as_deref())
        && let Some(merged_str) = merged.to_str()
    {
        target.insert("PATH".to_string(), merged_str.to_string());
    }

    for (key, value) in snapshot {
        if key == "PATH" || target.contains_key(key) {
            continue;
        }
        if std::env::var_os(key).is_some_and(|existing| !existing.is_empty()) {
            continue;
        }
        target.insert(key.clone(), value.clone());
    }
}

/// Return cached login-shell `PATH` directories (strictly read-only and non-blocking; never
/// spawns background probe tasks during read-only bootstrap or executable discovery).
pub fn cached_login_shell_path_dirs() -> Vec<PathBuf> {
    let Some(snapshot) = global_shell_snapshot_cache().resolve_env_cached() else {
        return Vec::new();
    };
    let Some(login_path) = snapshot.get("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(login_path)
        .filter(|p| !p.as_os_str().is_empty())
        .collect()
}

fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

fn is_shell_snapshot_disabled() -> bool {
    std::env::var("ORBIT_DISABLE_SHELL_SNAPSHOT").is_ok_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_timestamp_within_ttl(created_at_unix: u64, now_unix: u64, ttl: Duration) -> bool {
    if ttl.is_zero() {
        return false;
    }
    // Reject far-future timestamps caused by clock jumps (>60s skew).
    if created_at_unix > now_unix.saturating_add(60) {
        return false;
    }
    Duration::from_secs(now_unix.saturating_sub(created_at_unix)) < ttl
}

fn resolve_default_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Detect the current user's preferred login shell path.
pub fn detect_login_shell() -> String {
    #[cfg(not(unix))]
    {
        return std::env::var("COMSPEC")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string());
    }

    #[cfg(unix)]
    {
        if let Ok(shell) = std::env::var("SHELL") {
            let trimmed = shell.trim();
            if !trimmed.is_empty() && Path::new(trimmed).is_file() {
                return trimmed.to_string();
            }
        }
        for candidate in ["/bin/zsh", "/bin/bash", "/usr/bin/zsh", "/usr/bin/bash"] {
            if Path::new(candidate).is_file() {
                return candidate.to_string();
            }
        }
        "/bin/sh".to_string()
    }
}

/// Returns `true` for non-POSIX login shells (`csh`, `tcsh`, `nu`, `pwsh`, etc.) that do not
/// accept POSIX `||` / redirection probe scripts and should be skipped cleanly.
pub fn is_unsupported_non_posix_shell(shell_path: &str) -> bool {
    let base = Path::new(shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = base.strip_suffix(".exe").unwrap_or(&base);
    matches!(
        stem,
        "csh" | "tcsh" | "nu" | "nushell" | "pwsh" | "powershell" | "cmd"
    )
}

/// Stat all watched shell startup rc files under `home_dir` (following symlinks).
pub fn collect_rc_file_stamps(home_dir: &Path) -> BTreeMap<String, Option<RcFileStamp>> {
    let mut map = BTreeMap::new();
    for rel in WATCHED_RC_FILES {
        let path = home_dir.join(rel);
        let stamp = std::fs::metadata(&path).ok().and_then(|meta| {
            let modified = meta.modified().ok()?;
            let dur = modified.duration_since(UNIX_EPOCH).ok()?;
            Some(RcFileStamp {
                mtime_secs: dur.as_secs(),
                mtime_nanos: dur.subsec_nanos(),
                size: meta.len(),
            })
        });
        map.insert((*rel).to_string(), stamp);
    }
    map
}

/// Compute a deterministic 32-char SHA-256 hex fingerprint from shell path + rc-file stamps.
pub fn compute_snapshot_fingerprint(
    shell_path: &str,
    rc_mtimes: &BTreeMap<String, Option<RcFileStamp>>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"orbit-shell-snapshot-v");
    hasher.update(SNAPSHOT_SCHEMA_VERSION.to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(shell_path.as_bytes());
    hasher.update(b"\0");
    for (rel, stamp) in rc_mtimes {
        hasher.update(rel.as_bytes());
        hasher.update(b"=");
        match stamp {
            Some(s) => {
                hasher.update(s.mtime_secs.to_le_bytes());
                hasher.update(s.mtime_nanos.to_le_bytes());
                hasher.update(s.size.to_le_bytes());
            }
            None => hasher.update(b"absent"),
        }
        hasher.update(b"\0");
    }
    let digest = hasher.finalize();
    hex_encode(&digest[..16])
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Merge login-shell `PATH` entries ahead of `current_path`, preserving order without duplicates.
pub fn merge_paths(login_path: &str, current_path: Option<&OsStr>) -> Option<OsString> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut merged: Vec<PathBuf> = Vec::new();

    for dir in std::env::split_paths(login_path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        if seen.insert(dir.clone()) {
            merged.push(dir);
        }
    }

    if let Some(cur) = current_path {
        for dir in std::env::split_paths(cur) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            if seen.insert(dir.clone()) {
                merged.push(dir);
            }
        }
    }

    if merged.is_empty() {
        return None;
    }

    std::env::join_paths(merged).ok()
}

#[cfg(unix)]
fn build_probe_command_args(shell_path: &str, nonce: &str) -> Vec<String> {
    let base = Path::new(shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("sh")
        .to_ascii_lowercase();

    // Split marker literals across adjacent quoted strings so the exact marker never appears in
    // `ZSH_EXECUTION_STRING` / `BASH_EXECUTION_STRING`.
    if base == "fish" {
        let script = format!(
            "printf '%s%s' '__ORBIT_SHELL_ENV_START_' '{nonce}__'; \
             command env -0 2>/dev/null; or command env; \
             printf '%s%s' '__ORBIT_SHELL_ENV_END_' '{nonce}__'"
        );
        vec!["-l".to_string(), "-c".to_string(), script]
    } else if base == "zsh" || base == "bash" {
        let script = format!(
            "printf '%s%s' '__ORBIT_SHELL_ENV_START_' '{nonce}__'; \
             command env -0 2>/dev/null || command env; \
             printf '%s%s' '__ORBIT_SHELL_ENV_END_' '{nonce}__'"
        );
        // Pass `+m` (disable job control / MONITOR) so interactive login shells spawned in an
        // isolated background process group (`process_group(0)`) never call `tcsetpgrp()` and
        // stall on `SIGTTOU`.
        vec![
            "-l".to_string(),
            "-i".to_string(),
            "+m".to_string(),
            "-c".to_string(),
            script,
        ]
    } else {
        let script = format!(
            "printf '%s%s' '__ORBIT_SHELL_ENV_START_' '{nonce}__'; \
             command env -0 2>/dev/null || command env; \
             printf '%s%s' '__ORBIT_SHELL_ENV_END_' '{nonce}__'"
        );
        vec!["-l".to_string(), "-c".to_string(), script]
    }
}

#[cfg(unix)]
async fn kill_probe_process_group_and_wait(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id()
        && pid > 0
    {
        // The probe shell was launched in its own process group (`process_group(0)`), so
        // PGID == PID. Terminate the entire process group via safe `/bin/kill` so lingering
        // grandchildren (`sleep`, `nvm`, `brew`, etc.) never leak orphaned.
        let _ = tokio::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[cfg(unix)]
async fn probe_login_shell_env(
    shell_path: &str,
    home_dir: &Path,
    timeout: Duration,
) -> Result<HashMap<String, String>, String> {
    if is_unsupported_non_posix_shell(shell_path) {
        return Ok(HashMap::new());
    }

    let nonce = Uuid::new_v4().simple().to_string();
    let start_marker = format!("__ORBIT_SHELL_ENV_START_{nonce}__");
    let end_marker = format!("__ORBIT_SHELL_ENV_END_{nonce}__");
    let args = build_probe_command_args(shell_path, &nonce);

    let safe_cwd = if home_dir.is_dir() {
        home_dir.to_path_buf()
    } else {
        PathBuf::from("/")
    };

    let mut cmd = tokio::process::Command::new(shell_path);
    cmd.args(&args)
        .current_dir(safe_cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);

    for (k, v) in PROBE_GUARD_ENVS {
        cmd.env(k, v);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn login shell `{shell_path}`: {e}"))?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "login shell stdout pipe unavailable".to_string())?;

    let start_bytes = start_marker.as_bytes();
    let end_bytes = end_marker.as_bytes();

    let read_res = tokio::time::timeout(timeout, async {
        let mut captured = Vec::with_capacity(16 * 1024);
        let mut chunk = [0u8; 4096];
        loop {
            match stdout.read(&mut chunk).await {
                Ok(0) => break,
                Ok(n) => {
                    let remaining = MAX_PROBE_STDOUT_BYTES.saturating_sub(captured.len());
                    if remaining == 0 {
                        break;
                    }
                    let to_append = n.min(remaining);
                    captured.extend_from_slice(&chunk[..to_append]);
                    let window_start = captured.len().saturating_sub(to_append + end_bytes.len());
                    if rfind_subslice(&captured[window_start..], end_bytes).is_some() {
                        break;
                    }
                }
                Err(e) => return Err(format!("reading login shell stdout failed: {e}")),
            }
        }
        Ok(captured)
    })
    .await;

    let raw_output = match read_res {
        Ok(Ok(buf)) => buf,
        Ok(Err(err)) => {
            kill_probe_process_group_and_wait(&mut child).await;
            return Err(err);
        }
        Err(_) => {
            kill_probe_process_group_and_wait(&mut child).await;
            return Err(format!(
                "login shell probe timed out after {}ms",
                timeout.as_millis()
            ));
        }
    };

    // Give the shell a short grace window to exit cleanly on its own before force-killing
    // the entire process group (covers rc scripts that spawned background jobs inheriting stdout).
    if tokio::time::timeout(Duration::from_millis(100), child.wait())
        .await
        .is_err()
    {
        kill_probe_process_group_and_wait(&mut child).await;
    }

    let body = extract_marked_env_bytes(&raw_output, start_bytes, end_bytes)
        .ok_or_else(|| "login shell output missing snapshot markers".to_string())?;

    let parsed = parse_env_output(body);
    Ok(filter_snapshot_env(parsed, &HashSet::new()))
}

/// Extract the payload between `start_marker` and the last `end_marker` in `raw`.
pub fn extract_marked_env_bytes<'a>(
    raw: &'a [u8],
    start_marker: &[u8],
    end_marker: &[u8],
) -> Option<&'a [u8]> {
    let start_pos = find_subslice(raw, start_marker)?;
    let body_start = start_pos + start_marker.len();
    let tail = &raw[body_start..];
    let end_rel = rfind_subslice(tail, end_marker)?;
    Some(&tail[..end_rel])
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn rfind_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(haystack.len());
    }
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

/// Parse NUL-delimited `env -0` output (with automatic newline fallback if no NUL bytes exist).
pub fn parse_env_output(raw: &[u8]) -> HashMap<String, String> {
    if raw.contains(&b'\0') {
        parse_nul_delimited_env(raw)
    } else {
        parse_newline_delimited_env(raw)
    }
}

/// Parse NUL-delimited `env -0` output (`KEY=VALUE\0...`), faithfully preserving multiline values
/// and values containing `=`.
pub fn parse_nul_delimited_env(raw: &[u8]) -> HashMap<String, String> {
    let mut env = HashMap::new();
    for entry in raw.split(|&b| b == b'\0') {
        if entry.is_empty() {
            continue;
        }
        let Some(eq_idx) = entry.iter().position(|&b| b == b'=') else {
            continue;
        };
        let Ok(key) = std::str::from_utf8(&entry[..eq_idx]) else {
            continue;
        };
        if !is_valid_env_key(key) {
            continue;
        }
        let Ok(val) = std::str::from_utf8(&entry[eq_idx + 1..]) else {
            continue;
        };
        env.insert(key.to_string(), val.to_string());
    }
    env
}

fn parse_newline_delimited_env(raw: &[u8]) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let text = String::from_utf8_lossy(raw);
    for line in text.lines() {
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        if !is_valid_env_key(key) {
            continue;
        }
        env.insert(key.to_string(), val.to_string());
    }
    env
}

fn is_valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && !key
            .bytes()
            .any(|b| b == 0 || b == b'=' || b.is_ascii_whitespace())
}

fn is_probe_guard_env_key(key: &str) -> bool {
    PROBE_GUARD_ENVS.iter().any(|(k, _)| *k == key)
}

/// Filter out volatile/ephemeral, probe-guard, and secret-bearing environment variables while
/// preserving safe pointers like `SSH_AUTH_SOCK` and developer toolchain variables.
pub fn filter_snapshot_env(
    raw: HashMap<String, String>,
    probe_injected_keys: &HashSet<String>,
) -> HashMap<String, String> {
    let mut kept = HashMap::new();
    for (key, value) in raw {
        if !is_valid_env_key(&key) {
            continue;
        }
        if value.len() > MAX_ENV_VALUE_BYTES {
            continue;
        }
        if is_probe_guard_env_key(&key) || probe_injected_keys.contains(&key) {
            continue;
        }
        if is_volatile_env_key(&key) {
            continue;
        }
        if !is_safe_allowlisted_key(&key)
            && (is_sensitive_env_key(&key)
                || contains_embedded_url_credentials(&value)
                || contains_inline_private_key(&value))
        {
            continue;
        }
        kept.insert(key, value);
    }

    if kept.len() > MAX_ENV_VARS {
        let mut bounded = HashMap::with_capacity(MAX_ENV_VARS);
        for priority_key in SAFE_ALLOWLIST_KEYS {
            if let Some(v) = kept.remove(*priority_key) {
                bounded.insert((*priority_key).to_string(), v);
            }
        }
        let mut rest: Vec<(String, String)> = kept.into_iter().collect();
        rest.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in rest
            .into_iter()
            .take(MAX_ENV_VARS.saturating_sub(bounded.len()))
        {
            bounded.insert(k, v);
        }
        return bounded;
    }

    kept
}

fn is_safe_allowlisted_key(key: &str) -> bool {
    SAFE_ALLOWLIST_KEYS.contains(&key)
}

/// Returns `true` for shell-ephemeral, prompt, terminal-session, or Orbit-internal variables.
pub fn is_volatile_env_key(key: &str) -> bool {
    if VOLATILE_EXACT_KEYS.contains(&key) {
        return true;
    }
    VOLATILE_PREFIXES
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

/// Returns `true` when `key` looks like a secret, token, password, or credential variable.
pub fn is_sensitive_env_key(key: &str) -> bool {
    if is_safe_allowlisted_key(key) {
        return false;
    }
    let upper = key.to_ascii_uppercase();
    if SENSITIVE_PREFIXES_UPPER
        .iter()
        .any(|prefix| upper.starts_with(prefix))
    {
        return true;
    }
    let segments: Vec<&str> = upper.split('_').filter(|s| !s.is_empty()).collect();
    if segments
        .iter()
        .any(|seg| SENSITIVE_SEGMENTS_UPPER.contains(seg))
    {
        return true;
    }
    segments
        .windows(2)
        .any(|pair| SENSITIVE_SEGMENT_PAIRS_UPPER.contains(&(pair[0], pair[1])))
}

/// Detect credentials embedded in any URL occurrence within `value` (including `user:pass@host`,
/// token-only `https://ghp_xxx@github.com/...`, and multi-URL variables).
fn contains_embedded_url_credentials(value: &str) -> bool {
    let mut cursor = value;
    while let Some(scheme_pos) = cursor.find("://") {
        let rest = &cursor[scheme_pos + 3..];
        let authority_end = rest
            .find(|c: char| {
                matches!(c, '/' | '?' | '#' | ',' | ';' | '"' | '\'') || c.is_ascii_whitespace()
            })
            .unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        if let Some((userinfo, host)) = authority.rsplit_once('@')
            && !userinfo.is_empty()
            && !host.is_empty()
        {
            return true;
        }
        cursor = rest;
    }
    false
}

/// Detect inline PEM/OpenSSH private keys serialized into environment variables.
fn contains_inline_private_key(value: &str) -> bool {
    value.contains("PRIVATE KEY-----")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orbit_{prefix}_{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    #[test]
    fn parses_nul_delimited_env_output_with_banners_multiline_and_equals() {
        let start = b"__ORBIT_SHELL_ENV_START_abc123__";
        let end = b"__ORBIT_SHELL_ENV_END_abc123__";

        let mut raw = Vec::new();
        raw.extend_from_slice(b"Loading nvm v22.0.0...\nWelcome to zsh!\n");
        raw.extend_from_slice(start);
        raw.extend_from_slice(b"PATH=/opt/homebrew/bin:/usr/bin\0");
        raw.extend_from_slice(b"MULTILINE=first line\nsecond line\nthird=line\0");
        raw.extend_from_slice(b"COMPLEX_OPTS=foo=bar=baz\0");
        raw.extend_from_slice(b"EMPTY_VAL=\0");
        raw.extend_from_slice(b"NO_EQUALS_ENTRY\0");
        raw.extend_from_slice(b"BAD KEY=ignored\0");
        raw.extend_from_slice(end);
        raw.extend_from_slice(b"\ntrailing noise\n");

        let body = extract_marked_env_bytes(&raw, start, end).expect("markers extracted");
        let parsed = parse_nul_delimited_env(body);

        assert_eq!(
            parsed.get("PATH").map(String::as_str),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(
            parsed.get("MULTILINE").map(String::as_str),
            Some("first line\nsecond line\nthird=line")
        );
        assert_eq!(
            parsed.get("COMPLEX_OPTS").map(String::as_str),
            Some("foo=bar=baz")
        );
        assert_eq!(parsed.get("EMPTY_VAL").map(String::as_str), Some(""));
        assert!(!parsed.contains_key("NO_EQUALS_ENTRY"));
        assert!(!parsed.contains_key("BAD KEY"));
    }

    #[test]
    fn filters_volatile_and_sensitive_vars_but_keeps_ssh_auth_sock_and_toolchains() {
        let mut raw = HashMap::new();
        // Volatile / session vars (must be dropped)
        raw.insert("SHLVL".into(), "3".into());
        raw.insert("PWD".into(), "/tmp/work".into());
        raw.insert("OLDPWD".into(), "/tmp".into());
        raw.insert("_".into(), "/usr/bin/env".into());
        raw.insert("PS1".into(), "%n@%m %~ %# ".into());
        raw.insert("PROMPT".into(), "$ ".into());
        raw.insert("RPROMPT".into(), "[git]".into());
        raw.insert("TERM_SESSION_ID".into(), "sess-123".into());
        raw.insert("TMUX_PANE".into(), "%1".into());
        raw.insert("BASH_FUNC_myfn%%".into(), "() { echo hi; }".into());
        raw.insert("ORBIT_AGENT_ID".into(), "ag_123".into());
        raw.insert("ORBIT_HOME".into(), "/tmp/orbit".into());
        raw.insert("NO_COLOR".into(), "1".into());
        raw.insert("AGY_EFFORT".into(), "high".into());
        raw.insert("CLAUDE_CODE_ENTRYPOINT".into(), "sdk-ts".into());

        // Sensitive / transient secret vars (must be dropped)
        raw.insert("OPENAI_API_KEY".into(), "sk-test-123".into());
        raw.insert("ANTHROPIC_API_KEY".into(), "sk-ant-123".into());
        raw.insert("GITHUB_TOKEN".into(), "ghp_123".into());
        raw.insert("AWS_ACCESS_KEY_ID".into(), "AKIAEXAMPLE".into());
        raw.insert("AWS_SECRET_ACCESS_KEY".into(), "supersecret".into());
        raw.insert("AWS_SESSION_TOKEN".into(), "sessiontok".into());
        raw.insert("OP_SESSION_myteam".into(), "op-secret".into());
        raw.insert("BW_SESSION".into(), "bw-secret".into());
        raw.insert("DATABASE_PASSWORD".into(), "hunter2".into());
        raw.insert("DB_PASS".into(), "p@ss".into());
        raw.insert("SSH_KEY_PASSPHRASE".into(), "phrase".into());
        raw.insert(
            "HTTPS_PROXY".into(),
            "http://alice:hunter2@proxy.corp:8080".into(),
        );

        // Probe-injected synthetic key (must be dropped when listed in probe_injected)
        raw.insert("DISABLE_AUTO_UPDATE".into(), "true".into());

        // Safe & valuable toolchain/socket vars (must be kept)
        raw.insert("PATH".into(), "/opt/homebrew/bin:/usr/bin:/bin".into());
        raw.insert(
            "SSH_AUTH_SOCK".into(),
            "/private/tmp/com.apple.launchd.xxx/Listeners".into(),
        );
        raw.insert("HOMEBREW_PREFIX".into(), "/opt/homebrew".into());
        raw.insert("NVM_DIR".into(), "/Users/dev/.nvm".into());
        raw.insert("PYENV_ROOT".into(), "/Users/dev/.pyenv".into());
        raw.insert("RBENV_ROOT".into(), "/Users/dev/.rbenv".into());
        raw.insert("CARGO_HOME".into(), "/Users/dev/.cargo".into());
        raw.insert("GOPATH".into(), "/Users/dev/go".into());
        raw.insert("HTTP_PROXY".into(), "http://127.0.0.1:7890".into());
        raw.insert("BYPASS_PROXY".into(), "1".into());

        let mut probe_injected = HashSet::new();
        probe_injected.insert("DISABLE_AUTO_UPDATE".to_string());

        let filtered = filter_snapshot_env(raw, &probe_injected);

        // Verify dropped volatile keys
        for dropped in [
            "SHLVL",
            "PWD",
            "OLDPWD",
            "_",
            "PS1",
            "PROMPT",
            "RPROMPT",
            "TERM_SESSION_ID",
            "TMUX_PANE",
            "BASH_FUNC_myfn%%",
            "ORBIT_AGENT_ID",
            "ORBIT_HOME",
            "NO_COLOR",
            "AGY_EFFORT",
            "CLAUDE_CODE_ENTRYPOINT",
            "DISABLE_AUTO_UPDATE",
        ] {
            assert!(
                !filtered.contains_key(dropped),
                "expected volatile key `{dropped}` to be filtered out"
            );
        }

        // Verify dropped sensitive keys
        for secret in [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "GITHUB_TOKEN",
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "OP_SESSION_myteam",
            "BW_SESSION",
            "DATABASE_PASSWORD",
            "DB_PASS",
            "SSH_KEY_PASSPHRASE",
            "HTTPS_PROXY",
        ] {
            assert!(
                !filtered.contains_key(secret),
                "expected sensitive key `{secret}` to be filtered out"
            );
        }

        // Verify preserved safe & toolchain keys
        for kept in [
            "PATH",
            "SSH_AUTH_SOCK",
            "HOMEBREW_PREFIX",
            "NVM_DIR",
            "PYENV_ROOT",
            "RBENV_ROOT",
            "CARGO_HOME",
            "GOPATH",
            "HTTP_PROXY",
            "BYPASS_PROXY",
        ] {
            assert!(
                filtered.contains_key(kept),
                "expected safe key `{kept}` to be preserved"
            );
        }
    }

    #[test]
    fn atomic_save_and_load_roundtrip_enforces_0600_permissions_and_ttl() {
        let root = unique_temp_dir("snap_roundtrip");
        let cache_dir = root.join("shell_snapshots");
        let home_dir = root.join("home");
        std::fs::create_dir_all(&home_dir).expect("mkdir home");
        std::fs::write(
            home_dir.join(".zshrc"),
            "export PATH=\"/opt/homebrew/bin:$PATH\"\n",
        )
        .expect("write .zshrc");

        let cache = ShellSnapshotCache::with_config(
            cache_dir.clone(),
            home_dir.clone(),
            "/bin/zsh".to_string(),
            Duration::from_secs(1800),
            Duration::from_secs(3),
        );

        let rc_mtimes = collect_rc_file_stamps(&home_dir);
        let fp = compute_snapshot_fingerprint("/bin/zsh", &rc_mtimes);
        let mut env = BTreeMap::new();
        env.insert("PATH".into(), "/opt/homebrew/bin:/usr/bin:/bin".into());
        env.insert("SSH_AUTH_SOCK".into(), "/tmp/agent.sock".into());
        env.insert("NVM_DIR".into(), "/Users/dev/.nvm".into());

        let snap = PersistedShellSnapshot {
            version: SNAPSHOT_SCHEMA_VERSION,
            shell: "/bin/zsh".to_string(),
            fingerprint: fp.clone(),
            created_at_unix: now_unix_secs(),
            rc_mtimes: rc_mtimes.clone(),
            env,
        };

        let saved_path = cache.save_to_disk_atomic(&snap).expect("atomic save");
        assert!(saved_path.is_file());
        assert_eq!(
            saved_path.file_name().and_then(|n| n.to_str()),
            Some(format!("{fp}.json").as_str())
        );

        // No leftover `.tmp` files
        let entries: Vec<PathBuf> = std::fs::read_dir(&cache_dir)
            .expect("read cache_dir")
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(entries.len(), 1);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = std::fs::metadata(&saved_path)
                .expect("stat saved file")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600, "snapshot file must be 0600");

            let dir_mode = std::fs::metadata(&cache_dir)
                .expect("stat cache dir")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(dir_mode, 0o700, "snapshot dir must be 0700");
        }

        let loaded = cache
            .load_from_disk_if_valid(&fp, "/bin/zsh", &rc_mtimes)
            .expect("valid snapshot loads cleanly");
        assert_eq!(loaded, snap);

        // Expired snapshot must be rejected
        let expired = PersistedShellSnapshot {
            created_at_unix: now_unix_secs().saturating_sub(3600),
            ..snap
        };
        cache
            .save_to_disk_atomic(&expired)
            .expect("save expired snap");
        assert!(
            cache
                .load_from_disk_if_valid(&fp, "/bin/zsh", &rc_mtimes)
                .is_none(),
            "expired snapshot must not be loaded"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rc_file_mtime_change_auto_invalidates_memory_and_disk_cache() {
        let root = unique_temp_dir("snap_mtime");
        let cache_dir = root.join("shell_snapshots");
        let home_dir = root.join("home");
        std::fs::create_dir_all(&home_dir).expect("mkdir home");
        let zshrc = home_dir.join(".zshrc");
        let zprofile = home_dir.join(".zprofile");
        let bashrc = home_dir.join(".bashrc");
        let profile = home_dir.join(".profile");
        std::fs::write(&zshrc, "export FOO=1\n").expect("write .zshrc");
        std::fs::write(&zprofile, "export BAR=1\n").expect("write .zprofile");
        std::fs::write(&bashrc, "export BAZ=1\n").expect("write .bashrc");
        std::fs::write(&profile, "export QUX=1\n").expect("write .profile");

        let cache = ShellSnapshotCache::with_config(
            cache_dir,
            home_dir.clone(),
            "/bin/zsh".to_string(),
            Duration::from_secs(1800),
            Duration::from_secs(3),
        )
        .with_rc_stat_throttle(Duration::ZERO);

        let initial_stamps = collect_rc_file_stamps(&home_dir);
        let initial_fp = compute_snapshot_fingerprint("/bin/zsh", &initial_stamps);
        let mut env = BTreeMap::new();
        env.insert("PATH".into(), "/opt/homebrew/bin:/usr/bin".into());
        env.insert("PYENV_ROOT".into(), "/Users/dev/.pyenv".into());

        cache
            .save_to_disk_atomic(&PersistedShellSnapshot {
                version: SNAPSHOT_SCHEMA_VERSION,
                shell: "/bin/zsh".to_string(),
                fingerprint: initial_fp.clone(),
                created_at_unix: now_unix_secs(),
                rc_mtimes: initial_stamps,
                env,
            })
            .expect("seed snapshot");

        // First call loads from disk into memory
        let first = cache.resolve_env_cached().expect("disk hit");
        assert_eq!(
            first.get("PYENV_ROOT").map(String::as_str),
            Some("/Users/dev/.pyenv")
        );
        let stats_after_disk = cache.stats();
        assert_eq!(stats_after_disk.hits, 1);
        assert_eq!(stats_after_disk.disk_hits, 1);
        assert!(stats_after_disk.warm);
        assert_eq!(stats_after_disk.var_count, 2);

        // Second call serves straight from memory
        assert!(cache.resolve_env_cached().is_some());
        let stats_after_mem = cache.stats();
        assert_eq!(stats_after_mem.hits, 2);
        assert_eq!(stats_after_mem.disk_hits, 1);

        // Bump `.zshrc` mtime while keeping exact same file contents & length
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&zshrc)
            .expect("open .zshrc");
        file.set_modified(SystemTime::now() + Duration::from_secs(10))
            .expect("bump mtime");
        drop(file);

        let updated_stamps = collect_rc_file_stamps(&home_dir);
        let updated_fp = compute_snapshot_fingerprint("/bin/zsh", &updated_stamps);
        assert_ne!(
            initial_fp, updated_fp,
            "fingerprint must change when ~/.zshrc mtime changes"
        );
        assert!(
            cache.resolve_env_cached().is_none(),
            "cache must auto-invalidate on ~/.zshrc mtime change"
        );

        // Also verify touching ~/.bashrc, ~/.profile, ~/.zprofile each changes fingerprint
        for rc in [&zprofile, &bashrc, &profile] {
            let before =
                compute_snapshot_fingerprint("/bin/zsh", &collect_rc_file_stamps(&home_dir));
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(rc)
                .expect("open rc");
            f.set_modified(SystemTime::now() + Duration::from_secs(20))
                .expect("bump rc mtime");
            drop(f);
            let after =
                compute_snapshot_fingerprint("/bin/zsh", &collect_rc_file_stamps(&home_dir));
            assert_ne!(
                before,
                after,
                "fingerprint must change for {}",
                rc.display()
            );
        }

        // Creating a previously absent rc file must also change the fingerprint
        let before_create =
            compute_snapshot_fingerprint("/bin/zsh", &collect_rc_file_stamps(&home_dir));
        std::fs::write(home_dir.join(".zshenv"), "export ZSHENV_NEW=1\n").expect("create .zshenv");
        let after_create =
            compute_snapshot_fingerprint("/bin/zsh", &collect_rc_file_stamps(&home_dir));
        assert_ne!(before_create, after_create);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_caller_env_takes_precedence_over_cached_snapshot() {
        let mut snapshot = HashMap::new();
        let joined_snapshot_path =
            std::env::join_paths(["/opt/orbit-login-bin", "/opt/homebrew/bin", "/usr/bin"])
                .expect("join snapshot PATH")
                .to_string_lossy()
                .into_owned();
        snapshot.insert("PATH".into(), joined_snapshot_path);
        snapshot.insert("ORBIT_TEST_SNAP_EXPLICIT".into(), "from_snapshot".into());
        snapshot.insert("ORBIT_TEST_SNAP_ONLY".into(), "snapshot_value".into());
        snapshot.insert(
            "ORBIT_TEST_SNAP_REMOVED".into(),
            "should_not_reappear".into(),
        );

        // 1. std::process::Command: explicit caller `.env(...)` and `.env_remove(...)` win
        let mut std_cmd = std::process::Command::new("/bin/sh");
        std_cmd
            .env("ORBIT_TEST_SNAP_EXPLICIT", "from_caller")
            .env("PATH", "/caller/only/bin")
            .env_remove("ORBIT_TEST_SNAP_REMOVED");
        apply_shell_env_to_std_command(&mut std_cmd, &snapshot);

        let envs: HashMap<OsString, Option<OsString>> = std_cmd
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(OsStr::to_os_string)))
            .collect();

        assert_eq!(
            envs.get(OsStr::new("ORBIT_TEST_SNAP_EXPLICIT")),
            Some(&Some(OsString::from("from_caller")))
        );
        assert_eq!(
            envs.get(OsStr::new("PATH")),
            Some(&Some(OsString::from("/caller/only/bin")))
        );
        assert_eq!(envs.get(OsStr::new("ORBIT_TEST_SNAP_REMOVED")), Some(&None));
        assert_eq!(
            envs.get(OsStr::new("ORBIT_TEST_SNAP_ONLY")),
            Some(&Some(OsString::from("snapshot_value")))
        );

        // 2. tokio::process::Command without explicit PATH merges login PATH ahead of current PATH
        let mut tokio_cmd = tokio::process::Command::new("/bin/sh");
        tokio_cmd.env("ORBIT_TEST_SNAP_EXPLICIT", "tokio_caller");
        apply_shell_env_to_command(&mut tokio_cmd, &snapshot);

        let tokio_envs: HashMap<OsString, Option<OsString>> = tokio_cmd
            .as_std()
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(OsStr::to_os_string)))
            .collect();
        assert_eq!(
            tokio_envs.get(OsStr::new("ORBIT_TEST_SNAP_EXPLICIT")),
            Some(&Some(OsString::from("tokio_caller")))
        );
        assert_eq!(
            tokio_envs.get(OsStr::new("ORBIT_TEST_SNAP_ONLY")),
            Some(&Some(OsString::from("snapshot_value")))
        );
        let merged_path = tokio_envs
            .get(OsStr::new("PATH"))
            .and_then(|v| v.as_ref())
            .and_then(|v| v.to_str())
            .expect("merged PATH present");
        let path_sep = if cfg!(windows) { ';' } else { ':' };
        let expected_prefix = format!("/opt/orbit-login-bin{path_sep}");
        assert!(
            merged_path.starts_with(&expected_prefix) || merged_path == "/opt/orbit-login-bin",
            "login PATH entries should lead merged PATH, got: {merged_path}"
        );

        // 3. Map merge helper for ShimSpawnSpec::env also preserves explicit caller entries
        let mut map_target = HashMap::new();
        map_target.insert("ORBIT_TEST_SNAP_EXPLICIT".into(), "shim_caller".into());
        merge_shell_env_into_map(&snapshot, &mut map_target);
        assert_eq!(
            map_target
                .get("ORBIT_TEST_SNAP_EXPLICIT")
                .map(String::as_str),
            Some("shim_caller")
        );
        assert_eq!(
            map_target.get("ORBIT_TEST_SNAP_ONLY").map(String::as_str),
            Some("snapshot_value")
        );
        assert!(
            map_target
                .get("PATH")
                .is_some_and(|p| p.starts_with(&expected_prefix) || p == "/opt/orbit-login-bin")
        );

        // 4. Verify real child process observes explicit caller precedence end-to-end
        #[cfg(unix)]
        {
            let mut probe_cmd = std::process::Command::new("/bin/sh");
            probe_cmd
                .args([
                    "-c",
                    "printf '%s|%s|%s' \"$ORBIT_TEST_SNAP_EXPLICIT\" \"$ORBIT_TEST_SNAP_ONLY\" \"$PATH\"",
                ])
                .env("ORBIT_TEST_SNAP_EXPLICIT", "live_caller_wins")
                .env("PATH", "/caller/live/bin:/usr/bin:/bin");
            apply_shell_env_to_std_command(&mut probe_cmd, &snapshot);
            let out = probe_cmd.output().expect("spawn /bin/sh");
            assert!(out.status.success());
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert_eq!(
                stdout,
                "live_caller_wins|snapshot_value|/caller/live/bin:/usr/bin:/bin"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn live_probe_captures_env_handles_timeout_and_invalidates_cleanly() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_temp_dir("snap_probe");
        let cache_dir = root.join("shell_snapshots");
        let home_dir = root.join("home");
        std::fs::create_dir_all(&home_dir).expect("mkdir home");

        // 1. Mock fast shell that prints noisy banner + exports custom var + secret + volatile var
        let fast_shell = root.join("mock_fast_shell.sh");
        std::fs::write(
            &fast_shell,
            "#!/bin/sh\n\
             echo 'noisy rc banner before markers'\n\
             export PATH=\"/opt/mock-nvm/bin:/usr/bin:/bin\"\n\
             export MOCK_TOOLCHAIN_HOME=\"/opt/mock-toolchain\"\n\
             export SSH_AUTH_SOCK=\"/tmp/mock-ssh.sock\"\n\
             export MOCK_SECRET_TOKEN=\"must-not-persist\"\n\
             export SHLVL=99\n\
             # Execute the `-c` payload passed as the final argument\n\
             shift $(($# - 1))\n\
             eval \"$1\"\n",
        )
        .expect("write fast shell");
        std::fs::set_permissions(&fast_shell, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fast shell");

        let cache = ShellSnapshotCache::with_config(
            cache_dir.clone(),
            home_dir.clone(),
            fast_shell.to_string_lossy().to_string(),
            Duration::from_secs(1800),
            Duration::from_secs(3),
        );

        let resolved = cache.resolve_env().await;
        assert_eq!(
            resolved.get("MOCK_TOOLCHAIN_HOME").map(String::as_str),
            Some("/opt/mock-toolchain")
        );
        assert_eq!(
            resolved.get("SSH_AUTH_SOCK").map(String::as_str),
            Some("/tmp/mock-ssh.sock")
        );
        assert!(!resolved.contains_key("MOCK_SECRET_TOKEN"));
        assert!(!resolved.contains_key("SHLVL"));

        let st1 = cache.stats();
        assert_eq!(st1.misses, 1);
        assert_eq!(st1.hits, 0);
        assert!(st1.warm);
        assert!(st1.var_count >= 3);

        // Subsequent call hits memory cache without re-probing
        let second = cache.resolve_env().await;
        assert_eq!(
            second.get("MOCK_TOOLCHAIN_HOME"),
            resolved.get("MOCK_TOOLCHAIN_HOME")
        );
        let st2 = cache.stats();
        assert_eq!(st2.hits, 1);
        assert_eq!(st2.misses, 1);

        // Invalidate removes both in-memory entry and on-disk snapshot JSON
        cache.invalidate();
        let st3 = cache.stats();
        assert!(!st3.warm);
        assert_eq!(st3.var_count, 0);
        let remaining_json: Vec<PathBuf> = std::fs::read_dir(&cache_dir)
            .expect("read cache_dir")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect();
        assert!(
            remaining_json.is_empty(),
            "invalidate must delete on-disk snapshots"
        );

        // 2. Mock hanging shell must timeout cleanly within `probe_timeout` and never persist
        let slow_shell = root.join("mock_slow_shell.sh");
        std::fs::write(&slow_shell, "#!/bin/sh\nsleep 10\n").expect("write slow shell");
        std::fs::set_permissions(&slow_shell, std::fs::Permissions::from_mode(0o755))
            .expect("chmod slow shell");

        let slow_cache = ShellSnapshotCache::with_config(
            root.join("slow_snapshots"),
            home_dir,
            slow_shell.to_string_lossy().to_string(),
            Duration::from_secs(1800),
            Duration::from_millis(150),
        );

        let started = Instant::now();
        let timed_out_env = slow_cache.resolve_env().await;
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "hanging shell probe must terminate promptly on timeout"
        );
        assert!(timed_out_env.is_empty());
        assert_eq!(slow_cache.stats().probe_errors, 1);
        assert!(!slow_cache.stats().warm);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn filters_token_urls_multi_urls_pem_keys_and_all_probe_guard_envs() {
        let mut raw = HashMap::new();
        // Token-only URL credential
        raw.insert(
            "GIT_REMOTE_URL".into(),
            "https://ghp_supersecret123@github.com/org/repo.git".into(),
        );
        // Multi-URL where only the second URL carries credentials
        raw.insert(
            "PROXY_LIST".into(),
            "http://public.proxy:8080,https://tok_xyz@private.proxy:8443/path".into(),
        );
        raw.insert(
            "MIRROR_URLS".into(),
            "https://clean.example.com/a https://alice:secret@internal.example.com/b".into(),
        );
        // Inline PEM private key
        raw.insert(
            "DEPLOY_CERT".into(),
            "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA...\n-----END OPENSSH PRIVATE KEY-----"
                .into(),
        );
        raw.insert(
            "TLS_BUNDLE".into(),
            "-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n-----END RSA PRIVATE KEY-----".into(),
        );
        // All PROBE_GUARD_ENVS must be stripped even with an empty probe_injected_keys set
        for (k, v) in PROBE_GUARD_ENVS {
            raw.insert((*k).to_string(), (*v).to_string());
        }
        // Safe URL with `@` in path (not userinfo) must be preserved
        raw.insert(
            "NPM_CONFIG_REGISTRY".into(),
            "https://registry.npmjs.org/@scope/pkg".into(),
        );

        let filtered = filter_snapshot_env(raw, &HashSet::new());
        for dropped in [
            "GIT_REMOTE_URL",
            "PROXY_LIST",
            "MIRROR_URLS",
            "DEPLOY_CERT",
            "TLS_BUNDLE",
        ] {
            assert!(
                !filtered.contains_key(dropped),
                "expected sensitive value `{dropped}` to be stripped"
            );
        }
        for (guard_key, _) in PROBE_GUARD_ENVS {
            assert!(
                !filtered.contains_key(*guard_key),
                "expected probe guard env `{guard_key}` to always be stripped"
            );
        }
        assert_eq!(
            filtered.get("NPM_CONFIG_REGISTRY").map(String::as_str),
            Some("https://registry.npmjs.org/@scope/pkg")
        );
    }

    #[test]
    fn subsecond_ttl_rc_stat_throttle_stats_warm_and_disk_guards() {
        let root = unique_temp_dir("snap_p2");
        let cache_dir = root.join("shell_snapshots");
        let home_dir = root.join("home");
        std::fs::create_dir_all(&home_dir).expect("mkdir home");
        let zshrc = home_dir.join(".zshrc");
        std::fs::write(&zshrc, "export A=1\n").expect("write .zshrc");

        // Sub-second TTL should NOT immediately treat a fresh timestamp as expired.
        let now = now_unix_secs();
        assert!(is_timestamp_within_ttl(
            now,
            now,
            Duration::from_millis(500)
        ));
        assert!(!is_timestamp_within_ttl(
            now.saturating_sub(1),
            now,
            Duration::from_millis(500)
        ));
        assert!(!is_timestamp_within_ttl(now, now, Duration::ZERO));

        // Throttled warm hit skips rc stat within window, respects invalidate, and stats().warm flips false on TTL expiry.
        let cache = ShellSnapshotCache::with_config(
            cache_dir.clone(),
            home_dir.clone(),
            "/bin/zsh".to_string(),
            Duration::from_secs(5),
            Duration::from_secs(3),
        )
        .with_rc_stat_throttle(Duration::from_secs(5));

        let stamps = collect_rc_file_stamps(&home_dir);
        let fp = compute_snapshot_fingerprint("/bin/zsh", &stamps);
        let mut env = BTreeMap::new();
        env.insert("PATH".into(), "/usr/bin:/bin".into());
        cache
            .save_to_disk_atomic(&PersistedShellSnapshot {
                version: SNAPSHOT_SCHEMA_VERSION,
                shell: "/bin/zsh".to_string(),
                fingerprint: fp.clone(),
                created_at_unix: now_unix_secs(),
                rc_mtimes: stamps.clone(),
                env,
            })
            .expect("save snap");

        // Load from disk into memory
        assert!(cache.resolve_env_cached().is_some());
        assert!(cache.stats().warm);

        // Mutate `.zshrc` mtime within throttle window -> warm memory hit still serves immediately
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(&zshrc)
            .expect("open .zshrc");
        f.set_modified(SystemTime::now() + Duration::from_secs(30))
            .expect("bump mtime");
        drop(f);
        assert!(cache.resolve_env_cached().is_some());

        // Explicit invalidate immediately clears even within throttle window
        cache.invalidate();
        assert!(!cache.stats().warm);

        // Oversized (>2 MiB) snapshot file is rejected safely without reading into memory
        std::fs::create_dir_all(&cache_dir).expect("mkdir cache_dir");
        let big_fp = "deadbeef00000000deadbeef00000000";
        let big_file = cache_dir.join(format!("{big_fp}.json"));
        std::fs::write(
            &big_file,
            vec![b' '; (MAX_SNAPSHOT_FILE_BYTES as usize) + 16],
        )
        .expect("write oversized snap");
        assert!(
            cache
                .load_from_disk_if_valid(big_fp, "/bin/zsh", &stamps)
                .is_none()
        );

        // Stale `.tmp` files older than 5 minutes are pruned while fresh `.tmp` files are kept
        let stale_tmp = cache_dir.join(".snap-stale.tmp");
        let fresh_tmp = cache_dir.join(".snap-fresh.tmp");
        std::fs::write(&stale_tmp, b"partial").expect("write stale tmp");
        std::fs::write(&fresh_tmp, b"inflight").expect("write fresh tmp");
        let stale_f = std::fs::OpenOptions::new()
            .write(true)
            .open(&stale_tmp)
            .expect("open stale tmp");
        stale_f
            .set_modified(SystemTime::now() - Duration::from_secs(600))
            .expect("backdate stale tmp");
        drop(stale_f);

        prune_stale_disk_snapshots_in(&cache_dir, &fp, "/bin/zsh", Duration::from_secs(1800));
        assert!(!stale_tmp.exists(), "stale .tmp artifact should be pruned");
        assert!(fresh_tmp.exists(), "fresh .tmp artifact should be kept");

        // stats().warm becomes false once in-memory entry expires
        if let Ok(mut guard) = cache.memory.write() {
            guard.entry = Some(CachedMemoryEntry {
                shell: "/bin/zsh".to_string(),
                raw_shell_env: None,
                fingerprint: fp,
                created_at_unix: now_unix_secs().saturating_sub(10),
                created_at_instant: Some(Instant::now() - Duration::from_secs(10)),
                last_rc_check_at: Instant::now(),
                env: HashMap::from([("PATH".into(), "/usr/bin".into())]),
            });
        }
        assert!(
            !cache.stats().warm,
            "stats().warm must report false once TTL has expired"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn skips_unsupported_non_posix_shells_cleanly() {
        for sh in [
            "/bin/csh",
            "/usr/bin/tcsh",
            "/opt/homebrew/bin/nu",
            "/usr/local/bin/pwsh",
            "pwsh.exe",
        ] {
            assert!(
                is_unsupported_non_posix_shell(sh),
                "expected `{sh}` to be treated as unsupported non-POSIX shell"
            );
        }
        for sh in ["/bin/zsh", "/bin/bash", "/usr/bin/fish", "/bin/sh"] {
            assert!(
                !is_unsupported_non_posix_shell(sh),
                "expected `{sh}` to be supported"
            );
        }

        let root = unique_temp_dir("snap_nonposix");
        let cache = ShellSnapshotCache::with_config(
            root.join("shell_snapshots"),
            root.join("home"),
            "/usr/bin/tcsh".to_string(),
            Duration::from_secs(1800),
            Duration::from_secs(3),
        );
        let env = cache.resolve_env().await;
        assert!(env.is_empty());
        assert_eq!(cache.stats().probe_errors, 0);
        assert!(!cache.stats().warm);

        let _ = std::fs::remove_dir_all(&root);
    }
}
