use super::OhosCoreFacade;
use orbit_protocol::{MemoryCategory, MemoryStatus, ProjectMemoryItem};

fn sample_memory(id: &str, workspace_root: &str) -> ProjectMemoryItem {
    ProjectMemoryItem {
        id: id.into(),
        workspace_root: workspace_root.into(),
        title: format!("Memory {id}"),
        content: "Use Orbit design tokens.".into(),
        category: MemoryCategory::Convention,
        status: MemoryStatus::Hypothesis,
        confidence: 0.9,
        file_paths: vec!["AGENTS.md".into()],
        tags: vec!["pr6".into()],
        source_provider: Some("claude".into()),
        source_agent_id: Some("ag-1".into()),
        source_path: None,
        access_count: 1,
        created_at_ms: 1_760_000_000_000,
        updated_at_ms: 1_760_000_000_000,
        last_accessed_ms: 1_760_000_000_000,
    }
}

fn pair_offline_host(facade: &OhosCoreFacade, host_id: &str, secret: &str) -> String {
    let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
    let offer = orbit_crypto::PairingOffer {
        host_id: host_id.into(),
        hostname: format!("Host-{host_id}"),
        daemon_pubkey_b64: kp.public_key_b64,
        direct_endpoints: vec!["ws://192.168.10.25:19999/ws".into()],
        relay_url: Some("wss://relay.example.invalid".into()),
        pairing_secret: secret.into(),
    };
    let uri = offer.to_qr_uri().expect("qr uri");
    let paired = facade.pair_host_json(&uri).expect("pair host");
    assert!(paired.contains(host_id));
    offer.daemon_pubkey_b64
}

#[test]
fn pr6_export_host_pairing_uri_roundtrips_and_validates() {
    let facade = OhosCoreFacade::open_in_memory().expect("open facade");

    // Empty / unknown host rejected cleanly
    assert!(facade.export_host_pairing_uri("").is_err());
    assert!(facade.export_host_pairing_uri("   ").is_err());
    assert!(facade.export_host_pairing_uri("missing-host").is_err());

    let expected_pubkey = pair_offline_host(&facade, "host_pr6_pair", "super-secret-pr6");
    let exported_uri = facade
        .export_host_pairing_uri("host_pr6_pair")
        .expect("export uri");
    assert!(exported_uri.starts_with("orbit://pair/"));

    let decoded = orbit_crypto::PairingOffer::from_qr_uri(&exported_uri).expect("decode exported");
    assert_eq!(decoded.host_id, "host_pr6_pair");
    assert_eq!(decoded.daemon_pubkey_b64, expected_pubkey);
    assert_eq!(decoded.pairing_secret, "super-secret-pr6");
    assert_eq!(
        decoded.relay_url.as_deref(),
        Some("wss://relay.example.invalid")
    );
    assert_eq!(
        decoded.direct_endpoints,
        vec!["ws://192.168.10.25:19999/ws"]
    );

    // Exported URI must re-pair cleanly into a fresh client core
    let second = OhosCoreFacade::open_in_memory().expect("open second facade");
    let repaired = second.pair_host_json(&exported_uri).expect("re-pair host");
    assert!(repaired.contains("host_pr6_pair"));
}

#[test]
fn pr6_memory_cwd_cache_preserves_exact_spaces_flags_ambiguity_and_evicts_exact_lru() {
    let facade = OhosCoreFacade::open_in_memory().expect("open facade");

    // Unknown memory without explicit cwd fails deterministically before RPC
    let missing = facade
        .resolve_project_memory_cwd("host-a", "mem-missing", None)
        .expect_err("unknown cwd must fail");
    assert!(missing.contains("cwd"));

    // Record items with exact POSIX trailing spaces & cross-host same IDs
    facade.record_project_memory_cwds(
        "host-a",
        "/fallback/space ",
        &[
            sample_memory("mem-1", "/repo/project "),
            sample_memory("mem-fallback", ""),
        ],
    );
    facade.record_project_memory_cwds(
        "host-b",
        "/fallback/b",
        &[sample_memory("mem-1", "/repo/b")],
    );

    // Exact trailing-space paths are preserved verbatim (never trimmed!)
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-a", "mem-1", None)
            .expect("host-a exact space cwd"),
        "/repo/project "
    );
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-b", "mem-1", None)
            .expect("host-b cwd"),
        "/repo/b"
    );
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-a", "mem-fallback", None)
            .expect("fallback exact space cwd"),
        "/fallback/space "
    );

    // Explicit cwd preserves leading/trailing spaces verbatim and takes precedence over cache
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-a", "mem-1", Some("  /repo/override ".into()))
            .expect("explicit exact cwd"),
        "  /repo/override "
    );
    // Whitespace-only explicit cwd falls back to cache
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-a", "mem-1", Some("   ".into()))
            .expect("blank fallback"),
        "/repo/project "
    );

    // Same (host, id) observed across two distinct workspaces (east & west) MUST become Ambiguous
    facade.record_project_memory_cwds(
        "host-a",
        "/tmp/east",
        &[sample_memory("shared", "/tmp/east")],
    );
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-a", "shared", None)
            .expect("unique east"),
        "/tmp/east"
    );
    facade.record_project_memory_cwds(
        "host-a",
        "/tmp/west",
        &[sample_memory("shared", "/tmp/west")],
    );
    let ambig = facade
        .resolve_project_memory_cwd("host-a", "shared", None)
        .expect_err("cross-workspace same ID must fail as ambiguous");
    assert!(ambig.contains("ambiguous"), "unexpected error: {ambig}");
    // Explicit cwd still resolves deterministically even when cache entry is Ambiguous
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-a", "shared", Some("/tmp/east".into()))
            .expect("explicit east"),
        "/tmp/east"
    );
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-a", "shared", Some("/tmp/west".into()))
            .expect("explicit west"),
        "/tmp/west"
    );

    // Workspace-scoped eviction removes ONLY matching Unique record and never clobbers another workspace
    facade.evict_project_memory_cwd_for_workspace("host-a", "mem-1", "/repo/other");
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-a", "mem-1", None)
            .expect("mismatched workspace eviction must not remove record"),
        "/repo/project "
    );
    facade.evict_project_memory_cwd_for_workspace("host-a", "mem-1", "/repo/project ");
    assert!(
        facade
            .resolve_project_memory_cwd("host-a", "mem-1", None)
            .is_err()
    );
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-b", "mem-1", None)
            .expect("host-b intact"),
        "/repo/b"
    );
}

#[test]
fn pr6_memory_cwd_cache_strictly_bounds_capacity_per_insert_and_preserves_other_hosts() {
    let facade = OhosCoreFacade::open_in_memory().expect("open facade");

    // Seed 2048 distinct entries across host-keep (1 entry at index 0) + host-fill (2047 entries)
    facade.record_project_memory_cwds(
        "host-oldest",
        "/repo/oldest",
        &[sample_memory("oldest-key", "/repo/oldest")],
    );
    let batch_2046: Vec<ProjectMemoryItem> = (0..2046)
        .map(|i| sample_memory(&format!("fill-{i}"), "/repo/fill"))
        .collect();
    facade.record_project_memory_cwds("host-fill", "/repo/fill", &batch_2046);
    facade.record_project_memory_cwds(
        "host-other",
        "/repo/other",
        &[sample_memory("other-pinned", "/repo/other")],
    );
    assert_eq!(facade.memory_cwd_cache_len(), 2048);

    // Duplicate key update refreshes recency and occupies ZERO extra slots
    facade.record_project_memory_cwds(
        "host-other",
        "/repo/other",
        &[sample_memory("other-pinned", "/repo/other")],
    );
    assert_eq!(facade.memory_cwd_cache_len(), 2048);

    // 2049th distinct key evicts ONLY the single oldest entry (`host-oldest::oldest-key`), keeping `host-other`
    facade.record_project_memory_cwds(
        "host-fill",
        "/repo/fill",
        &[sample_memory("fill-2046", "/repo/fill")],
    );
    assert_eq!(facade.memory_cwd_cache_len(), 2048);
    assert!(
        facade
            .resolve_project_memory_cwd("host-oldest", "oldest-key", None)
            .is_err(),
        "oldest entry must be evicted at 2049"
    );
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-other", "other-pinned", None)
            .expect("other host entry must survive single eviction"),
        "/repo/other"
    );

    // Single batch of 2100 items MUST stay hard-capped at 2048 (never 2100!), evicting earliest items
    let facade_single = OhosCoreFacade::open_in_memory().expect("open single facade");
    let batch_2100: Vec<ProjectMemoryItem> = (0..2100)
        .map(|i| sample_memory(&format!("bulk-{i}"), "/repo/bulk"))
        .collect();
    facade_single.record_project_memory_cwds("host-c", "/repo/bulk", &batch_2100);
    assert_eq!(facade_single.memory_cwd_cache_len(), 2048);
    assert!(
        facade_single
            .resolve_project_memory_cwd("host-c", "bulk-0", None)
            .is_err(),
        "bulk-0 must be evicted when 2100 unique keys inserted"
    );
    assert!(
        facade_single
            .resolve_project_memory_cwd("host-c", "bulk-51", None)
            .is_err(),
        "bulk-51 must be evicted when 2100 unique keys inserted"
    );
    assert_eq!(
        facade_single
            .resolve_project_memory_cwd("host-c", "bulk-52", None)
            .expect("bulk-52 must remain as oldest surviving key"),
        "/repo/bulk"
    );
    assert_eq!(
        facade_single
            .resolve_project_memory_cwd("host-c", "bulk-2099", None)
            .expect("bulk-2099 must remain as newest key"),
        "/repo/bulk"
    );

    // Multi-batch across 9 projects × 250 items preserves a refreshed other-host key while
    // failing closed for legacy 2-arg calls on `host-main` (multi-workspace + LRU eviction).
    let facade_multi = OhosCoreFacade::open_in_memory().expect("open multi facade");
    for batch_idx in 0..9 {
        if batch_idx == 8 {
            facade_multi.record_project_memory_cwds(
                "host-keep",
                "/repo/keep",
                &[sample_memory("keep-me", "/repo/keep")],
            );
        }
        let items: Vec<ProjectMemoryItem> = (0..250)
            .map(|i| {
                sample_memory(
                    &format!("p{batch_idx}-m{i}"),
                    &format!("/repo/p{batch_idx}"),
                )
            })
            .collect();
        facade_multi.record_project_memory_cwds(
            "host-main",
            &format!("/repo/p{batch_idx}"),
            &items,
        );
        assert!(facade_multi.memory_cwd_cache_len() <= 2048);
    }
    assert_eq!(facade_multi.memory_cwd_cache_len(), 2048);
    assert_eq!(
        facade_multi
            .resolve_project_memory_cwd("host-keep", "keep-me", None)
            .expect("unaffected host entry must remain resolvable"),
        "/repo/keep"
    );
    assert!(
        facade_multi
            .resolve_project_memory_cwd("host-main", "p0-m0", None)
            .is_err(),
        "earliest batch item must have been evicted"
    );
    let degraded_err = facade_multi
        .resolve_project_memory_cwd("host-main", "p8-m249", None)
        .expect_err("multi-workspace evicted host must fail closed for 2-arg resolution");
    assert!(
        degraded_err.contains("ambiguous"),
        "expected ambiguity fail-closed error, got: {degraded_err}"
    );
    assert_eq!(
        facade_multi
            .resolve_project_memory_cwd("host-main", "p8-m249", Some("/repo/p8".into()))
            .expect("explicit cwd must still resolve on degraded host"),
        "/repo/p8"
    );
}

#[test]
fn pr6_memory_ambiguity_survives_lru_eviction_and_single_side_relist() {
    // Case 1: east + west same ID -> 2160 evictions -> re-list ONLY west -> 2-arg MUST fail closed
    let facade = OhosCoreFacade::open_in_memory().expect("open facade");
    facade.record_project_memory_cwds(
        "host-a",
        "/tmp/east",
        &[sample_memory("original-pair", "/tmp/east")],
    );
    facade.record_project_memory_cwds(
        "host-a",
        "/tmp/west",
        &[sample_memory("original-pair", "/tmp/west")],
    );
    facade.record_project_memory_cwds(
        "host-b",
        "/tmp/b-only",
        &[sample_memory("b-item", "/tmp/b-only")],
    );
    for proj in 0..9 {
        let cwd = format!("/tmp/fill-{proj}");
        let items: Vec<ProjectMemoryItem> = (0..240)
            .map(|i| sample_memory(&format!("f-{proj}-{i}"), &cwd))
            .collect();
        facade.record_project_memory_cwds("host-a", &cwd, &items);
        if proj == 8 {
            // Keep host-b fresh while host-a overflows
            facade.record_project_memory_cwds(
                "host-b",
                "/tmp/b-only",
                &[sample_memory("b-item", "/tmp/b-only")],
            );
        }
    }
    assert_eq!(facade.memory_cwd_cache_len(), 2048);

    // Re-list ONLY west after eviction -> MUST stay ambiguous, never downgrade back to Unique
    facade.record_project_memory_cwds(
        "host-a",
        "/tmp/west",
        &[sample_memory("original-pair", "/tmp/west")],
    );
    let err = facade
        .resolve_project_memory_cwd("host-a", "original-pair", None)
        .expect_err("re-listed west after eviction must stay fail-closed");
    assert!(err.contains("ambiguous"), "unexpected error: {err}");
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-a", "original-pair", Some("/tmp/west".into()))
            .expect("explicit cwd works"),
        "/tmp/west"
    );
    assert_eq!(
        facade
            .resolve_project_memory_cwd("host-b", "b-item", None)
            .expect("host-b unaffected"),
        "/tmp/b-only"
    );

    // Case 2: list ONLY east -> 2160 evictions -> list ONLY west -> 2-arg MUST fail closed
    let facade2 = OhosCoreFacade::open_in_memory().expect("open facade2");
    facade2.record_project_memory_cwds(
        "host-x",
        "/tmp/east",
        &[sample_memory("late-pair", "/tmp/east")],
    );
    let fill_batch: Vec<ProjectMemoryItem> = (0..2100)
        .map(|i| sample_memory(&format!("e-{i}"), "/tmp/east"))
        .collect();
    facade2.record_project_memory_cwds("host-x", "/tmp/east", &fill_batch);
    // Now list west with the same ID after east's entry was evicted from the main LRU
    facade2.record_project_memory_cwds(
        "host-x",
        "/tmp/west",
        &[sample_memory("late-pair", "/tmp/west")],
    );
    let err2 = facade2
        .resolve_project_memory_cwd("host-x", "late-pair", None)
        .expect_err("host with prior eviction observing second workspace must fail closed");
    assert!(err2.contains("ambiguous"), "unexpected error: {err2}");
    assert_eq!(
        facade2
            .resolve_project_memory_cwd("host-x", "late-pair", Some("/tmp/west".into()))
            .expect("explicit cwd works"),
        "/tmp/west"
    );
}

#[test]
fn pr6_deleted_history_overflow_and_host_tracking_saturation_fail_closed_distinctly() {
    // 1. Delete in east -> list west same ID -> fails closed as ambiguous; overflow > 2048 degrades host
    let facade = OhosCoreFacade::open_in_memory().expect("open facade");
    facade.record_project_memory_cwds("host-d", "/tmp/east", &[sample_memory("m1", "/tmp/east")]);
    facade.record_project_memory_deleted_for_workspace("host-d", "m1", "/tmp/east", false);
    assert!(
        facade
            .resolve_project_memory_cwd("host-d", "m1", None)
            .is_err()
    );
    facade.record_project_memory_cwds("host-d", "/tmp/west", &[sample_memory("m1", "/tmp/west")]);
    let replay_err = facade
        .resolve_project_memory_cwd("host-d", "m1", None)
        .expect_err("deleted in east then listed in west must fail closed");
    assert!(replay_err.contains("ambiguous"), "got: {replay_err}");

    let facade_ovf = OhosCoreFacade::open_in_memory().expect("open overflow facade");
    for i in 0..2049 {
        facade_ovf.record_project_memory_deleted_for_workspace(
            "host-ovf",
            &format!("del-{i}"),
            "/tmp/east",
            true,
        );
    }
    facade_ovf.record_project_memory_cwds(
        "host-ovf",
        "/tmp/west",
        &[sample_memory("del-0", "/tmp/west")],
    );
    let ovf_err = facade_ovf
        .resolve_project_memory_cwd("host-ovf", "del-0", None)
        .expect_err("overflowed deletion history must degrade host fail-closed");
    assert!(
        ovf_err.contains("deletion history capacity"),
        "expected distinct deletion history capacity error, got: {ovf_err}"
    );
    assert!(
        !ovf_err.contains("multi-workspace cache eviction"),
        "single-workspace deletion overflow must not claim multi-workspace eviction: {ovf_err}"
    );

    // 2. R3-Nit-1: 2049 distinct single-workspace hosts trigger distinct host-tracking saturation error
    let facade_hosts = OhosCoreFacade::open_in_memory().expect("open hosts facade");
    for h in 0..2048 {
        let host = format!("host-{h}");
        facade_hosts.record_project_memory_cwds(
            &host,
            "/repo/only",
            &[sample_memory("only-id", "/repo/only")],
        );
    }
    assert_eq!(
        facade_hosts
            .resolve_project_memory_cwd("host-2047", "only-id", None)
            .expect("2048th host resolves before saturation"),
        "/repo/only"
    );
    facade_hosts.record_project_memory_cwds(
        "host-2048",
        "/repo/only",
        &[sample_memory("only-id", "/repo/only")],
    );
    let cap_err = facade_hosts
        .resolve_project_memory_cwd("host-2047", "only-id", None)
        .expect_err("2049th distinct host must trigger global host-tracking saturation");
    assert!(
        cap_err.contains("host tracking capacity"),
        "expected distinct host tracking capacity error, got: {cap_err}"
    );
    assert!(
        !cap_err.contains("multi-workspace cache eviction"),
        "must not conflate host tracking saturation with multi-workspace eviction: {cap_err}"
    );
    assert_eq!(
        facade_hosts
            .resolve_project_memory_cwd("host-2047", "only-id", Some("/repo/only".into()))
            .expect("explicit cwd still works under host tracking saturation"),
        "/repo/only"
    );
}

#[test]
fn pr6_all_ten_tier2_async_rpcs_validate_inputs_and_reject_unknown_or_offline_hosts() {
    let facade = OhosCoreFacade::open_in_memory().expect("open facade");
    pair_offline_host(&facade, "host_pr6_offline", "secret-offline");

    // 1. Blank argument validation
    assert!(facade.get_usage_insights_json("", 7).is_err());
    assert!(facade.list_external_mcp_servers_json("  ", None).is_err());
    assert!(
        facade
            .toggle_external_mcp_server_json("host_pr6_offline", " ", true, None)
            .is_err()
    );
    assert!(facade.list_managed_skills_json("", None).is_err());
    assert!(
        facade
            .list_project_memories_json("host_pr6_offline", "   ", 50)
            .is_err()
    );
    assert!(
        facade
            .confirm_project_memory_json("host_pr6_offline", "", Some("/repo".into()))
            .is_err()
    );
    assert!(
        facade
            .delete_project_memory_json("host_pr6_offline", "", Some("/repo".into()))
            .is_err()
    );
    assert!(facade.list_plugins_json("").is_err());
    assert!(
        facade
            .set_plugin_enabled_json("host_pr6_offline", "", true)
            .is_err()
    );
    assert!(facade.get_sandbox_config_json("").is_err());

    // 2. Confirm / Delete without cached or explicit cwd fails immediately
    let no_cwd_confirm = facade
        .confirm_project_memory_json("host_pr6_offline", "mem-unknown", None)
        .expect_err("missing cwd confirm");
    assert!(no_cwd_confirm.contains("cwd"));
    let no_cwd_delete = facade
        .delete_project_memory_json("host_pr6_offline", "mem-unknown", None)
        .expect_err("missing cwd delete");
    assert!(no_cwd_delete.contains("cwd"));

    // 3. Unknown host and disconnected host return errors cleanly without hanging
    for target in ["missing-host", "host_pr6_offline"] {
        assert!(facade.get_usage_insights_json(target, 0).is_err());
        assert!(facade.get_usage_insights_json(target, 365).is_err());
        assert!(facade.list_external_mcp_servers_json(target, None).is_err());
        assert!(
            facade
                .list_external_mcp_servers_json(target, Some("/repo".into()))
                .is_err()
        );
        assert!(
            facade
                .toggle_external_mcp_server_json(target, "orbit_user:fs", false, None)
                .is_err()
        );
        assert!(
            facade
                .list_managed_skills_json(target, Some("/repo".into()))
                .is_err()
        );
        assert!(
            facade
                .list_project_memories_json(target, "/repo", 50)
                .is_err()
        );
        assert!(
            facade
                .confirm_project_memory_json(target, "mem-1", Some("/repo".into()))
                .is_err()
        );
        assert!(
            facade
                .delete_project_memory_json(target, "mem-1", Some("/repo".into()))
                .is_err()
        );
        assert!(facade.list_plugins_json(target).is_err());
        assert!(
            facade
                .set_plugin_enabled_json(target, "orbit-lint", true)
                .is_err()
        );
        assert!(facade.get_sandbox_config_json(target).is_err());
    }
}
