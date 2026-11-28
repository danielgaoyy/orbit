use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) const MAX_MEMORY_CWD_CACHE_ENTRIES: usize = 2048;
pub(crate) const MAX_KNOWN_AMBIGUOUS_MEMORIES: usize = 2048;
pub(crate) const MAX_DELETED_MEMORY_HISTORY: usize = 2048;
pub(crate) const MAX_WORKSPACE_ALIAS_ENTRIES: usize = 2048;
pub(crate) const MAX_TRACKED_MEMORY_HOSTS: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CachedMemoryCwd {
    Unique(String),
    Ambiguous,
}

#[derive(Clone, Debug)]
struct CachedMemoryCwdEntry {
    state: CachedMemoryCwd,
    seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DeletedWorkspace {
    /// Canonical `workspace_root` proven by a daemon list/confirm response.
    Canonical(String),
    /// Raw request `cwd` passed directly to explicit 3-arg delete without prior daemon resolution.
    UnresolvedRaw(String),
}

#[derive(Clone, Debug)]
struct HostWorkspaceHistory {
    first_cwd: String,
    has_multiple_workspaces: bool,
    had_lru_eviction: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostTwoArgDegradeReason {
    MultiWorkspaceCacheEviction,
    AmbiguityHistoryCapacityExceeded,
    DeletedHistoryCapacityExceeded,
    WorkspaceAliasCapacityExceeded,
}

impl HostTwoArgDegradeReason {
    fn error_message(self) -> &'static str {
        match self {
            Self::MultiWorkspaceCacheEviction => {
                "project memory workspace cwd is ambiguous after multi-workspace cache eviction on this host; provide explicit cwd"
            }
            Self::AmbiguityHistoryCapacityExceeded => {
                "project memory ambiguity history capacity (2048) exceeded on this host; provide explicit cwd"
            }
            Self::DeletedHistoryCapacityExceeded => {
                "project memory deletion history capacity (2048) exceeded on this host; provide explicit cwd"
            }
            Self::WorkspaceAliasCapacityExceeded => {
                "project memory workspace alias capacity (2048) exceeded on this host; provide explicit cwd"
            }
        }
    }
}

/// Strictly bounded LRU `(host_id, memory_id) -> CachedMemoryCwd` cache with fail-closed
/// multi-workspace ambiguity protection and authoritative daemon workspace-root correlation.
///
/// Contract (5 bounded tables, each capped at `2048`):
/// - `entries`: Main LRU (`2048`). New distinct keys evict only the globally oldest live entry.
/// - `known_ambiguous`: Independent bounded set (`2048`) retaining multi-workspace same-ID
///   conflicts across main-LRU eviction and single-side re-listing; overflow degrades the evicted
///   entry's host with `AmbiguityHistoryCapacityExceeded`.
/// - `deleted_memory_history`: Independent bounded LRU (`2048`) recording deleted
///   `(host_id, memory_id) -> DeletedWorkspace`. Re-listing the same ID in a different workspace
///   transitions immediately to `Ambiguous` + `known_ambiguous`; overflow degrades the evicted
///   entry's host with `DeletedHistoryCapacityExceeded`.
/// - `request_cwd_to_workspace_root`: Independent bounded LRU (`2048`) mapping
///   `(host_id, raw_request_cwd) -> canonical_workspace_root` from daemon responses so explicit
///   deletes using slash/symlink aliases resolve to the authoritative workspace root; overflow
///   degrades the evicted entry's host with `WorkspaceAliasCapacityExceeded`.
/// - `host_history`: Bounded per-host tracker (`2048` hosts). A host with at least 2 distinct
///   workspaces and any main-LRU eviction degrades with `MultiWorkspaceCacheEviction`. Exceeding
///   `2048` distinct hosts sets `global_two_arg_degraded` with a distinct host-tracking capacity
///   error. Explicit 3-arg calls with a non-blank `cwd` bypass 2-arg cache lookup and
///   degradation limits and forward directly to the daemon, which remains authoritative for the
///   operation outcome (for example, returning a daemon error for an unknown memory ID).
#[derive(Default)]
pub(crate) struct BoundedMemoryCwdCache {
    entries: HashMap<(String, String), CachedMemoryCwdEntry>,
    order: VecDeque<((String, String), u64)>,
    next_seq: u64,
    known_ambiguous: HashSet<(String, String)>,
    known_ambiguous_order: VecDeque<(String, String)>,
    deleted_memory_history: HashMap<(String, String), (DeletedWorkspace, u64)>,
    deleted_memory_order: VecDeque<((String, String), u64)>,
    deleted_next_seq: u64,
    request_cwd_to_workspace_root: HashMap<(String, String), (String, u64)>,
    alias_order: VecDeque<((String, String), u64)>,
    alias_next_seq: u64,
    host_history: HashMap<String, HostWorkspaceHistory>,
    legacy_two_arg_degraded_hosts: HashMap<String, HostTwoArgDegradeReason>,
    global_two_arg_degraded: bool,
}

impl BoundedMemoryCwdCache {
    fn mark_host_degraded(&mut self, host_id: &str, reason: HostTwoArgDegradeReason) {
        if self.legacy_two_arg_degraded_hosts.contains_key(host_id) {
            return;
        }
        if self.legacy_two_arg_degraded_hosts.len() >= MAX_TRACKED_MEMORY_HOSTS {
            self.global_two_arg_degraded = true;
            return;
        }
        self.legacy_two_arg_degraded_hosts
            .insert(host_id.to_string(), reason);
    }

    fn record_known_ambiguous(&mut self, key: &(String, String)) {
        if self.known_ambiguous.contains(key) {
            return;
        }
        while self.known_ambiguous.len() >= MAX_KNOWN_AMBIGUOUS_MEMORIES {
            let Some(oldest) = self.known_ambiguous_order.pop_front() else {
                break;
            };
            if self.known_ambiguous.remove(&oldest) {
                self.mark_host_degraded(
                    &oldest.0,
                    HostTwoArgDegradeReason::AmbiguityHistoryCapacityExceeded,
                );
            }
        }
        self.known_ambiguous.insert(key.clone());
        self.known_ambiguous_order.push_back(key.clone());
    }

    fn note_host_workspace(&mut self, host_id: &str, canonical_or_raw_cwd: &str) {
        if let Some(hist) = self.host_history.get_mut(host_id) {
            if hist.first_cwd != canonical_or_raw_cwd {
                hist.has_multiple_workspaces = true;
            }
            if hist.has_multiple_workspaces && hist.had_lru_eviction {
                self.mark_host_degraded(
                    host_id,
                    HostTwoArgDegradeReason::MultiWorkspaceCacheEviction,
                );
            }
            return;
        }
        if self.host_history.len() >= MAX_TRACKED_MEMORY_HOSTS {
            self.global_two_arg_degraded = true;
            return;
        }
        self.host_history.insert(
            host_id.to_string(),
            HostWorkspaceHistory {
                first_cwd: canonical_or_raw_cwd.to_string(),
                has_multiple_workspaces: false,
                had_lru_eviction: false,
            },
        );
    }

    fn note_host_lru_eviction(&mut self, evicted_host: &str) {
        if let Some(hist) = self.host_history.get_mut(evicted_host) {
            hist.had_lru_eviction = true;
            if hist.has_multiple_workspaces {
                self.mark_host_degraded(
                    evicted_host,
                    HostTwoArgDegradeReason::MultiWorkspaceCacheEviction,
                );
            }
        } else {
            self.mark_host_degraded(
                evicted_host,
                HostTwoArgDegradeReason::MultiWorkspaceCacheEviction,
            );
        }
    }

    pub(crate) fn record_workspace_alias(
        &mut self,
        host_id: &str,
        raw_request_cwd: &str,
        canonical_workspace_root: &str,
    ) {
        if raw_request_cwd.trim().is_empty() || canonical_workspace_root.trim().is_empty() {
            return;
        }
        self.note_host_workspace(host_id, canonical_workspace_root);
        let key = (host_id.to_string(), raw_request_cwd.to_string());
        self.alias_next_seq = self.alias_next_seq.wrapping_add(1);
        let seq = self.alias_next_seq;

        if let Some((prev_root, prev_seq)) = self.request_cwd_to_workspace_root.get_mut(&key) {
            if prev_root.as_str() != canonical_workspace_root {
                *prev_root = canonical_workspace_root.to_string();
            }
            *prev_seq = seq;
            self.alias_order.push_back((key, seq));
            self.maybe_compact_alias_order();
            return;
        }

        while self.request_cwd_to_workspace_root.len() >= MAX_WORKSPACE_ALIAS_ENTRIES {
            let Some((oldest_key, oldest_seq)) = self.alias_order.pop_front() else {
                break;
            };
            let is_live = self
                .request_cwd_to_workspace_root
                .get(&oldest_key)
                .is_some_and(|(_, cur_seq)| *cur_seq == oldest_seq);
            if is_live {
                self.request_cwd_to_workspace_root.remove(&oldest_key);
                self.mark_host_degraded(
                    &oldest_key.0,
                    HostTwoArgDegradeReason::WorkspaceAliasCapacityExceeded,
                );
            }
        }

        self.request_cwd_to_workspace_root
            .insert(key.clone(), (canonical_workspace_root.to_string(), seq));
        self.alias_order.push_back((key, seq));
        self.maybe_compact_alias_order();
    }

    pub(crate) fn record(&mut self, host_id: &str, memory_id: &str, canonical_cwd: &str) {
        self.note_host_workspace(host_id, canonical_cwd);
        let key = (host_id.to_string(), memory_id.to_string());
        let deleted_in_other_workspace =
            self.deleted_memory_history
                .get(&key)
                .is_some_and(|(deleted_ws, _)| match deleted_ws {
                    DeletedWorkspace::Canonical(prev_root) => prev_root != canonical_cwd,
                    DeletedWorkspace::UnresolvedRaw(raw_deleted) => raw_deleted != canonical_cwd,
                });
        if deleted_in_other_workspace {
            self.record_known_ambiguous(&key);
        }
        self.next_seq = self.next_seq.wrapping_add(1);
        let seq = self.next_seq;

        if let Some(existing) = self.entries.get_mut(&key) {
            let became_ambiguous = match &existing.state {
                CachedMemoryCwd::Unique(prev_cwd) if prev_cwd == canonical_cwd => false,
                CachedMemoryCwd::Unique(_) => true,
                CachedMemoryCwd::Ambiguous => false,
            };
            if became_ambiguous || deleted_in_other_workspace || self.known_ambiguous.contains(&key)
            {
                existing.state = CachedMemoryCwd::Ambiguous;
            }
            existing.seq = seq;
            self.order.push_back((key.clone(), seq));
            if became_ambiguous {
                self.record_known_ambiguous(&key);
            }
            self.maybe_compact_order();
            return;
        }

        while self.entries.len() >= MAX_MEMORY_CWD_CACHE_ENTRIES {
            let Some((oldest_key, oldest_seq)) = self.order.pop_front() else {
                break;
            };
            let is_live = self
                .entries
                .get(&oldest_key)
                .is_some_and(|entry| entry.seq == oldest_seq);
            if is_live {
                self.entries.remove(&oldest_key);
                self.note_host_lru_eviction(&oldest_key.0);
            }
        }

        let state = if deleted_in_other_workspace || self.known_ambiguous.contains(&key) {
            CachedMemoryCwd::Ambiguous
        } else {
            CachedMemoryCwd::Unique(canonical_cwd.to_string())
        };
        self.entries
            .insert(key.clone(), CachedMemoryCwdEntry { state, seq });
        self.order.push_back((key, seq));
        self.maybe_compact_order();
    }

    pub(crate) fn resolve_legacy_two_arg(
        &self,
        host_id: &str,
        memory_id: &str,
    ) -> Result<String, String> {
        let key = (host_id.to_string(), memory_id.to_string());
        if self.known_ambiguous.contains(&key) {
            return Err(
                "project memory workspace cwd is ambiguous across multiple workspaces; provide explicit cwd"
                    .to_string(),
            );
        }
        if let Some(reason) = self.legacy_two_arg_degraded_hosts.get(host_id) {
            return Err(reason.error_message().to_string());
        }
        if self.global_two_arg_degraded {
            return Err(
                "project memory host tracking capacity (2048 hosts) saturated; provide explicit cwd"
                    .to_string(),
            );
        }
        match self.entries.get(&key).map(|entry| &entry.state) {
            Some(CachedMemoryCwd::Unique(cwd)) => Ok(cwd.clone()),
            Some(CachedMemoryCwd::Ambiguous) => Err(
                "project memory workspace cwd is ambiguous across multiple workspaces; provide explicit cwd"
                    .to_string(),
            ),
            None => Err(
                "project memory workspace cwd unknown; list project memories first or provide cwd"
                    .to_string(),
            ),
        }
    }

    /// Records a daemon-confirmed deletion for `(host_id, memory_id)`.
    ///
    /// - Resolves the authoritative canonical workspace root via `request_cwd_to_workspace_root`
    ///   (or the 2-arg `entries` lookup) whenever available, falling back to `UnresolvedRaw` if
    ///   never resolved via daemon list.
    /// - Unconditionally revokes any active `Unique` entry in `entries` so stale implicit 2-arg
    ///   write permission can NEVER survive an explicit alias/slash delete (and flags ambiguity if
    ///   proven to target a different workspace than the cached `Unique` entry).
    pub(crate) fn record_deleted_for_workspace(
        &mut self,
        host_id: &str,
        memory_id: &str,
        raw_request_cwd: &str,
        was_explicit_cwd: bool,
    ) {
        let key = (host_id.to_string(), memory_id.to_string());
        let deleted_workspace = if !was_explicit_cwd {
            DeletedWorkspace::Canonical(raw_request_cwd.to_string())
        } else if let Some((canonical_root, _)) = self
            .request_cwd_to_workspace_root
            .get(&(host_id.to_string(), raw_request_cwd.to_string()))
        {
            DeletedWorkspace::Canonical(canonical_root.clone())
        } else if self
            .entries
            .get(&key)
            .is_some_and(|entry| match &entry.state {
                CachedMemoryCwd::Unique(cached_root) => cached_root == raw_request_cwd,
                CachedMemoryCwd::Ambiguous => false,
            })
            || self
                .deleted_memory_history
                .get(&key)
                .is_some_and(|(prev_ws, _)| match prev_ws {
                    DeletedWorkspace::Canonical(prev_root) => prev_root == raw_request_cwd,
                    DeletedWorkspace::UnresolvedRaw(_) => false,
                })
        {
            DeletedWorkspace::Canonical(raw_request_cwd.to_string())
        } else {
            DeletedWorkspace::UnresolvedRaw(raw_request_cwd.to_string())
        };

        let history_cwd = match &deleted_workspace {
            DeletedWorkspace::Canonical(root) => root.as_str(),
            DeletedWorkspace::UnresolvedRaw(raw) => raw.as_str(),
        };
        self.note_host_workspace(host_id, history_cwd);

        // Revoke active `Unique` mapping unconditionally; flag ambiguity if proven cross-workspace.
        let cross_workspace_with_cached =
            self.entries
                .get(&key)
                .is_some_and(|entry| match (&entry.state, &deleted_workspace) {
                    (
                        CachedMemoryCwd::Unique(cached_root),
                        DeletedWorkspace::Canonical(del_root),
                    ) => cached_root != del_root,
                    (
                        CachedMemoryCwd::Unique(cached_root),
                        DeletedWorkspace::UnresolvedRaw(raw_del),
                    ) => cached_root != raw_del,
                    _ => false,
                });
        if cross_workspace_with_cached {
            self.record_known_ambiguous(&key);
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.state = CachedMemoryCwd::Ambiguous;
            }
        } else if self
            .entries
            .get(&key)
            .is_some_and(|entry| matches!(entry.state, CachedMemoryCwd::Unique(_)))
        {
            self.entries.remove(&key);
            self.maybe_compact_order();
        }

        self.deleted_next_seq = self.deleted_next_seq.wrapping_add(1);
        let seq = self.deleted_next_seq;

        if let Some((prev_ws, prev_seq)) = self.deleted_memory_history.get_mut(&key) {
            let cross_workspace = *prev_ws != deleted_workspace;
            *prev_seq = seq;
            self.deleted_memory_order.push_back((key.clone(), seq));
            if cross_workspace {
                self.record_known_ambiguous(&key);
            }
            self.maybe_compact_deleted_order();
            return;
        }

        while self.deleted_memory_history.len() >= MAX_DELETED_MEMORY_HISTORY {
            let Some((oldest_key, oldest_seq)) = self.deleted_memory_order.pop_front() else {
                break;
            };
            let is_live = self
                .deleted_memory_history
                .get(&oldest_key)
                .is_some_and(|(_, cur_seq)| *cur_seq == oldest_seq);
            if is_live {
                self.deleted_memory_history.remove(&oldest_key);
                self.mark_host_degraded(
                    &oldest_key.0,
                    HostTwoArgDegradeReason::DeletedHistoryCapacityExceeded,
                );
            }
        }

        self.deleted_memory_history
            .insert(key.clone(), (deleted_workspace, seq));
        self.deleted_memory_order.push_back((key, seq));
        self.maybe_compact_deleted_order();
    }

    #[cfg(test)]
    pub(crate) fn evict_for_workspace(&mut self, host_id: &str, memory_id: &str, exact_cwd: &str) {
        let key = (host_id.to_string(), memory_id.to_string());
        let should_remove = self
            .entries
            .get(&key)
            .is_some_and(|entry| match &entry.state {
                CachedMemoryCwd::Unique(cached_cwd) => cached_cwd == exact_cwd,
                CachedMemoryCwd::Ambiguous => false,
            });
        if should_remove {
            self.entries.remove(&key);
            self.maybe_compact_order();
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    fn maybe_compact_order(&mut self) {
        if self.order.len() <= MAX_MEMORY_CWD_CACHE_ENTRIES.saturating_mul(4) {
            return;
        }
        let entries = &self.entries;
        self.order
            .retain(|(k, seq)| entries.get(k).is_some_and(|entry| entry.seq == *seq));
    }

    fn maybe_compact_deleted_order(&mut self) {
        if self.deleted_memory_order.len() <= MAX_DELETED_MEMORY_HISTORY.saturating_mul(4) {
            return;
        }
        let history = &self.deleted_memory_history;
        self.deleted_memory_order
            .retain(|(k, seq)| history.get(k).is_some_and(|(_, cur_seq)| *cur_seq == *seq));
    }

    fn maybe_compact_alias_order(&mut self) {
        if self.alias_order.len() <= MAX_WORKSPACE_ALIAS_ENTRIES.saturating_mul(4) {
            return;
        }
        let aliases = &self.request_cwd_to_workspace_root;
        self.alias_order
            .retain(|(k, seq)| aliases.get(k).is_some_and(|(_, cur_seq)| *cur_seq == *seq));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_alias_delete_revokes_unique_and_same_dir_relist_avoids_false_ambiguity() {
        let mut cache = BoundedMemoryCwdCache::default();
        // Simulate listing `/tmp/east/` where daemon canonical root is `/private/tmp/east`
        cache.record_workspace_alias("h1", "/tmp/east/", "/private/tmp/east");
        cache.record("h1", "m1", "/private/tmp/east");
        assert_eq!(
            cache.resolve_legacy_two_arg("h1", "m1").expect("cached"),
            "/private/tmp/east"
        );

        // Explicit delete via alias `/tmp/east/` revokes active `Unique` entry unconditionally
        cache.record_deleted_for_workspace("h1", "m1", "/tmp/east/", true);
        let err = cache
            .resolve_legacy_two_arg("h1", "m1")
            .expect_err("explicit alias delete must revoke cached 2-arg permission");
        assert!(err.contains("unknown"), "got: {err}");

        // Re-listing same dir `/tmp/east/` restores `Unique("/private/tmp/east")` without false ambiguity
        cache.record_workspace_alias("h1", "/tmp/east/", "/private/tmp/east");
        cache.record("h1", "m1", "/private/tmp/east");
        assert_eq!(
            cache
                .resolve_legacy_two_arg("h1", "m1")
                .expect("same-dir relist must not falsely mark ambiguous"),
            "/private/tmp/east"
        );

        // Unlisted explicit delete targeting another workspace while `m1` is cached fails closed
        cache.record_deleted_for_workspace("h1", "m1", "/tmp/west", true);
        cache.record("h1", "m1", "/tmp/west");
        let ambig = cache
            .resolve_legacy_two_arg("h1", "m1")
            .expect_err("cross-workspace delete must mark ambiguous");
        assert!(ambig.contains("ambiguous"), "got: {ambig}");
    }

    #[test]
    fn per_host_degrade_reasons_are_distinct_across_all_bounded_tables() {
        // 1. DeletedHistoryCapacityExceeded (single-workspace host, 2049 explicit deletes)
        let mut del_cache = BoundedMemoryCwdCache::default();
        for i in 0..2049 {
            del_cache.record_deleted_for_workspace("h-del", &format!("d-{i}"), "/repo", true);
        }
        del_cache.record("h-del", "fresh", "/repo");
        let del_err = del_cache
            .resolve_legacy_two_arg("h-del", "fresh")
            .expect_err("deleted history overflow");
        assert!(
            del_err.contains("deletion history capacity"),
            "got: {del_err}"
        );
        assert!(!del_err.contains("multi-workspace"), "got: {del_err}");

        // 2. WorkspaceAliasCapacityExceeded (2049 aliases on one host)
        let mut alias_cache = BoundedMemoryCwdCache::default();
        for i in 0..2049 {
            alias_cache.record_workspace_alias("h-alias", &format!("/link/{i}"), "/private/repo");
        }
        alias_cache.record("h-alias", "fresh", "/private/repo");
        let alias_err = alias_cache
            .resolve_legacy_two_arg("h-alias", "fresh")
            .expect_err("workspace alias overflow");
        assert!(
            alias_err.contains("workspace alias capacity"),
            "got: {alias_err}"
        );
        assert!(!alias_err.contains("multi-workspace"), "got: {alias_err}");

        // 3. AmbiguityHistoryCapacityExceeded (`h-ambig` evicted from `known_ambiguous` without main-LRU eviction)
        let mut ambig_cache = BoundedMemoryCwdCache::default();
        ambig_cache.record("h-ambig", "a-0", "/east");
        ambig_cache.record("h-ambig", "a-0", "/west");
        for i in 0..2048 {
            let id = format!("other-{i}");
            ambig_cache.record("h-other", &id, "/east");
            ambig_cache.record("h-other", &id, "/west");
            if i % 1024 == 0 {
                // Keep `h-ambig`'s entry hot in `entries` so ONLY `known_ambiguous` evicts `h-ambig`
                ambig_cache.record("h-ambig", "a-0", "/east");
            }
        }
        ambig_cache.record("h-ambig", "fresh", "/east");
        let ambig_err = ambig_cache
            .resolve_legacy_two_arg("h-ambig", "fresh")
            .expect_err("ambiguity history overflow");
        assert!(
            ambig_err.contains("ambiguity history capacity"),
            "got: {ambig_err}"
        );
        assert!(!ambig_err.contains("multi-workspace"), "got: {ambig_err}");

        // 4. MultiWorkspaceCacheEviction (2 workspaces + main LRU eviction)
        let mut lru_cache = BoundedMemoryCwdCache::default();
        lru_cache.record("h-lru", "e0", "/east");
        for i in 0..2050 {
            lru_cache.record("h-lru", &format!("w-{i}"), "/west");
        }
        let lru_err = lru_cache
            .resolve_legacy_two_arg("h-lru", "w-2049")
            .expect_err("multi-workspace LRU eviction");
        assert!(
            lru_err.contains("multi-workspace cache eviction"),
            "got: {lru_err}"
        );
    }
}
