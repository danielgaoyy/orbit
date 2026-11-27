//! HarmonyOS 7+ (`API 26` / `aarch64-unknown-linux-ohos`) Native N-API Bridge (`liborbit_ohos.so`).
//!
//! Uses `ohos-rs` (`napi-ohos` + `ThreadsafeFunction`) on HarmonyOS targets to drive ArkTS `@ObservedV2`
//! state models, while remaining 100% buildable and testable on macOS/Linux host toolchains.

mod dto;
mod events;
mod facade;
#[cfg(target_env = "ohos")]
mod napi;
#[cfg(feature = "perf-hooks")]
mod replay;
#[cfg(test)]
mod tests_pr5;

pub use facade::OhosCoreFacade;

#[cfg(test)]
mod tests {
    use super::OhosCoreFacade;
    use crate::events::OhosCoreNotification;
    use crate::facade::parse_mode_str;
    use orbit_client_core::app_core::CoreNotification;
    use orbit_protocol::SessionMode;

    #[test]
    fn mobile_contract_rejects_bad_keys_and_preserves_host_scope() {
        let path = std::env::temp_dir().join(format!("orbit-ohos-key-{}.db", std::process::id()));
        let path = path.to_str().expect("path").to_owned();
        assert!(OhosCoreFacade::new_with_master_key(path.clone(), "%%%".into()).is_err());
        assert!(OhosCoreFacade::new_with_master_key(path.clone(), "AQI=".into()).is_err());
        use base64::Engine as _;
        let core = OhosCoreFacade::new_with_master_key(
            path.clone(),
            base64::engine::general_purpose::STANDARD.encode([7; 32]),
        )
        .expect("key");
        assert_eq!(core.get_fleet_mesh_snapshot_json().expect("mesh"), "{}");
        assert!(
            core.get_session_usage_json("unknown".into(), "agent".into())
                .is_err()
        );
        assert!(
            core.git_stage_paths_json("unknown".into(), "/repo".into(), vec![], true)
                .is_err()
        );
        drop(core);
        assert!(OhosCoreFacade::open(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn ohos_facade_exports_valid_fleet_json() {
        let facade = OhosCoreFacade::open_in_memory().expect("open");
        let json = facade.fleet_overview_json().expect("json");
        assert!(json.contains("\"hosts\":[]"));
        assert!(json.contains("\"projects\":[]"));
    }

    #[test]
    fn ohos_facade_agent_stage_project_and_lifecycle_roundtrip() {
        let facade = OhosCoreFacade::open_in_memory().expect("open");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = orbit_crypto::PairingOffer {
            host_id: "host_ohos".into(),
            hostname: "matebook-fold".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "secret".into(),
        };
        let uri = offer.to_qr_uri().expect("uri");
        let paired_json = facade.pair_host_json(&uri).expect("pair");
        assert!(paired_json.contains("host_ohos"));

        let proj_json = facade
            .upsert_project_json(
                None,
                "host_ohos",
                "orbit-harmony",
                r#"["/workspace/orbit","/workspace/shared"]"#,
            )
            .expect("upsert project");
        assert!(proj_json.contains("orbit-harmony"));

        let space_proj_json = facade
            .upsert_project_json(None, "host_ohos", "project", r#"["/tmp/project "]"#)
            .expect("upsert trailing-space project");
        let space_proj_val: serde_json::Value =
            serde_json::from_str(&space_proj_json).expect("parse space project json");
        assert_eq!(
            space_proj_val["directories"][0].as_str(),
            Some("/tmp/project "),
            "OHOS facade upsert_project_json must preserve trailing space in directory path"
        );

        let opt_id = facade
            .create_agent_full(
                "host_ohos",
                "/workspace/orbit",
                Some(r#"["/workspace/shared"]"#),
                "claude_code",
                None,
                Some("claude-opus-5-5".into()),
                Some("high"),
                "accept_edits",
                "Build HarmonyOS NEXT stage UI",
                Some("/plan Build HarmonyOS NEXT stage UI".into()),
                Some(r#"[{"kind":"slash","label":"/plan"}]"#),
                None,
            )
            .expect("create_agent_full");
        assert!(opt_id.starts_with("opt_"));

        let stage_json = facade
            .agent_stage_json("host_ohos", &opt_id)
            .expect("stage json");
        let stage_val: serde_json::Value = serde_json::from_str(&stage_json).expect("valid json");
        assert_eq!(stage_val["host_id"], "host_ohos");
        assert_eq!(stage_val["turns"].as_array().map(Vec::len), Some(1));

        let delta_json = facade
            .agent_timeline_delta_json("host_ohos", &opt_id, 0)
            .expect("delta json");
        assert!(delta_json.contains("Build HarmonyOS NEXT stage UI"));

        facade.on_app_background();
        facade.on_app_foreground();
        facade.on_network_changed();

        facade
            .remove_project("host_ohos::/workspace/orbit")
            .expect("remove project");
    }

    #[test]
    fn ohos_facade_stage_delta_sidebar_prefs_catalog_and_compact_events() {
        let notif: OhosCoreNotification = CoreNotification::TerminalChanged {
            revision: 9,
            host_id: "h1".into(),
            slot: 2,
            raw_bytes: b"\x1b[31msecret\x1b[0m".to_vec(),
        }
        .into();
        let notif_json = serde_json::to_string(&notif).expect("serialize notif");
        assert!(!notif_json.contains("raw_bytes"));
        assert!(notif_json.contains("\"slot\":2"));

        let facade = OhosCoreFacade::open_in_memory().expect("open");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = orbit_crypto::PairingOffer {
            host_id: "host_ohos_2".into(),
            hostname: "ohos-dev".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "secret".into(),
        };
        facade
            .pair_host_json(&offer.to_qr_uri().expect("uri"))
            .expect("pair");
        let opt_id = facade
            .create_agent_full(
                "host_ohos_2",
                "/repo/orbit",
                None,
                "claude_code",
                None,
                None,
                None,
                "default",
                "Inspect sidebar",
                None,
                None,
                None,
            )
            .expect("create");

        let d0: serde_json::Value = serde_json::from_str(
            &facade
                .agent_stage_delta_json("host_ohos_2", &opt_id, 0)
                .expect("delta0"),
        )
        .expect("json");
        assert_eq!(d0["reset"], true);
        let rev = d0["revision"].as_u64().expect("rev");

        let d_same: serde_json::Value = serde_json::from_str(
            &facade
                .agent_stage_delta_json("host_ohos_2", &opt_id, rev)
                .expect("delta same"),
        )
        .expect("json");
        assert_eq!(d_same["changed_turns"].as_array().map(Vec::len), Some(0));

        assert!(facade.toggle_pin_agent("host_ohos_2", &opt_id));
        facade
            .seed_thread_prefs_json(
                r#"[{"host_id":"host_ohos_2","agent_id":"ag_hist","seen_seq":12,"is_pinned":true}]"#,
            )
            .expect("seed prefs");
        facade.mark_agent_seen("host_ohos_2", "ag_hist", 20);
        facade.mark_agent_unread("host_ohos_2", "ag_hist");
        facade.mark_agent_read("host_ohos_2", "ag_hist");

        let sidebar: serde_json::Value = serde_json::from_str(
            &facade
                .fleet_sidebar_json(
                    Some("recent"),
                    Some("all"),
                    None,
                    Some("host_ohos_2"),
                    Some(&opt_id),
                    Some("/repo/orbit"),
                )
                .expect("sidebar"),
        )
        .expect("json");
        assert_eq!(
            sidebar["pinned_conversations"].as_array().map(Vec::len),
            Some(1)
        );

        let catalogs_json = facade.fallback_provider_catalogs_json().expect("catalogs");
        assert!(catalogs_json.contains("claude_code"));
        let cat_json = facade
            .fallback_provider_catalog_json("claude_code")
            .expect("cat");
        let label = facade
            .format_catalog_model_label(&cat_json, Some("claude-opus-5-5"))
            .expect("label");
        assert_eq!(label, "Opus 5.5");
        assert_eq!(facade.format_effort_label("high"), "High");
        assert!(!facade.default_effort_description("medium").is_empty());

        let term_slot = facade
            .terminal_screen_slot_json("host_ohos_2", 2)
            .expect("slot 2");
        assert!(term_slot.contains("\"rows_data\""));
        let ctx = facade.build_side_question_context("host_ohos_2", &opt_id, Some(500));
        assert!(ctx.contains("Workspace: /repo/orbit"));

        let bridge_json = facade.bridge_info_json().expect("bridge info");
        assert!(bridge_json.contains("\"protocol_version\":2"));

        let slash_json = facade
            .slash_commands_json(Some("harmony"), Some("plan"))
            .expect("slash");
        assert!(slash_json.contains("/plan"));

        let sel_json = facade
            .resolve_model_selection_json(r#"{"provider":"claude_code","desired_model":"sonnet"}"#)
            .expect("model sel");
        assert!(sel_json.contains("\"claude-opus-5-5\""));

        let comp_json = facade
            .compose_prompt_json(
                r#"{"provider":"codex","raw_text":"/plan refactor router","host_id":"host_ohos_2","cwd":"/repo/orbit"}"#,
            )
            .expect("compose");
        assert!(comp_json.contains("<WORKFLOW_INSTRUCTION command=\\\"/plan\\\">"));

        let mention_json = facade
            .rank_mention_candidates_json(
                r#"{"host_id":"host_ohos_2","cwd":"/repo/orbit","query":"diff","limit":5}"#,
            )
            .expect("mention");
        assert!(mention_json.contains("@working-diff"));

        assert_eq!(
            facade
                .detect_diagram_kind("mermaid", "flowchart LR; A-->B")
                .as_deref(),
            Some("mermaid")
        );
        let diagram_json = facade
            .render_diagram_json(
                r#"{"kind":"mermaid","source":"flowchart LR\nA-->B","theme":"dark"}"#,
            )
            .expect("diagram");
        assert!(diagram_json.contains("<svg"));
    }

    #[test]
    fn ohos_facade_parity_automations_board_archive_and_modes() {
        assert_eq!(parse_mode_str("auto"), SessionMode::Auto);
        assert_eq!(parse_mode_str("accept_edits"), SessionMode::AcceptEdits);
        assert_eq!(parse_mode_str("full_access"), SessionMode::FullAccess);

        let facade = OhosCoreFacade::open_in_memory().expect("open");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = orbit_crypto::PairingOffer {
            host_id: "host_parity".into(),
            hostname: "ohos-parity".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "secret".into(),
        };
        facade
            .pair_host_json(&offer.to_qr_uri().expect("uri"))
            .expect("pair");

        let opt_id = facade
            .create_agent_full(
                "host_parity",
                "/workspace/parity",
                None,
                "claude_code",
                None,
                None,
                None,
                "auto",
                "Test parity controls",
                None,
                None,
                None,
            )
            .expect("create");
        facade.set_agent_fast_mode("host_parity", &opt_id, true);
        facade.unarchive_agent("host_parity", &opt_id);
        facade.delete_agent("host_parity", &opt_id);

        // Workspace board & events roundtrip
        facade.fetch_workspace_board("host_parity", "ws_1");
        let board_json = facade
            .workspace_board_json("host_parity", "ws_1")
            .expect("board json");
        assert_eq!(board_json, "null");
        facade.update_workspace_board(
            "host_parity",
            "ws_1",
            "## Goal\nShip HarmonyOS parity".into(),
            None,
            Some("harmony-client".into()),
        );
        facade.fetch_workspace_events("host_parity", "ws_1", Some(20));
        let events_json = facade
            .workspace_events_json("host_parity", "ws_1")
            .expect("events json");
        assert_eq!(events_json, "[]");

        // Schedules CRUD fire-and-forget & runs snapshot
        facade
            .create_schedule_json(
                r#"{"host_id":"host_parity","name":"Nightly Check","cron_expr":"0 9 * * *","prompt":"Run tests","cwd":"/workspace/parity","provider":"claude_code","mode":"auto"}"#,
            )
            .expect("create schedule");
        facade.pause_schedule("host_parity", "sched_1");
        facade.resume_schedule("host_parity", "sched_1");
        facade.run_schedule_once("host_parity", "sched_1");
        facade.delete_schedule("host_parity", "sched_1");
        facade.fetch_schedule_runs("host_parity", "sched_1", Some(10));
        let runs_json = facade
            .schedule_runs_json("host_parity", "sched_1")
            .expect("runs json");
        assert_eq!(runs_json, "[]");

        // Offline RPCs return structured errors cleanly without panicking
        assert!(
            facade
                .list_conditional_timers_json("host_parity", None, true)
                .is_err()
        );
        assert!(
            facade
                .cancel_conditional_timer_json("host_parity", "tmr_1")
                .is_err()
        );
        assert!(facade.list_goal_runs_json("host_parity", None).is_err());
        assert!(
            facade
                .control_goal_run_json(
                    r#"{"host_id":"host_parity","goal_id":"g1","action":"pause"}"#,
                )
                .is_err()
        );
        assert!(
            facade
                .list_monitor_watches_json("host_parity", None)
                .is_err()
        );
        assert!(
            facade
                .stop_monitor_watch_json(r#"{"host_id":"host_parity","monitor_id":"m1"}"#)
                .is_err()
        );
        assert!(facade.list_workflow_runs_json("host_parity", true).is_err());
        assert!(
            facade
                .cancel_workflow_run_json(r#"{"host_id":"host_parity","run_id":"wf1"}"#)
                .is_err()
        );
    }

    #[test]
    fn ohos_facade_mark_agent_read_clears_only_target_conversation() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!(
            "orbit_ohos_mark_read_{}_{}.sqlite",
            std::process::id(),
            nanos
        ));
        let db_str = db_path.to_string_lossy().into_owned();

        // Pair host first so its saved profile exists in the SQLite store.
        {
            let facade = OhosCoreFacade::open(&db_str).expect("open initial");
            let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
            let offer = orbit_crypto::PairingOffer {
                host_id: "host_ohos_read".into(),
                hostname: "read-check-host".into(),
                daemon_pubkey_b64: kp.public_key_b64,
                direct_endpoints: Vec::new(),
                relay_url: None,
                pairing_secret: "secret".into(),
            };
            facade
                .pair_host_json(&offer.to_qr_uri().expect("uri"))
                .expect("pair");
        }

        // Seed two non-optimistic replica agents directly into the client SQLite cache.
        {
            let store = orbit_store::OrbitStore::open(&db_path).expect("open store");
            let now = chrono::Utc::now();
            for (agent_id, title) in [
                ("ag_read_target", "Target Thread"),
                ("ag_unread_peer", "Peer Thread"),
            ] {
                let summary = orbit_protocol::AgentSummary {
                    agent_id: agent_id.into(),
                    workspace_id: "ws_read_1".into(),
                    cwd: "/repo/orbit".into(),
                    title: title.into(),
                    provider: orbit_protocol::ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: orbit_protocol::SessionMode::Default,
                    status: orbit_protocol::AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 12,
                    timeline_epoch: 0,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    profile_id: None,
                    acp_agent_id: None,
                    background: false,
                    auto_archive: false,
                    attention: orbit_protocol::AttentionState::Finished,
                    last_activity_at: Some(now),
                    archived_at: None,
                    labels: std::collections::HashMap::new(),
                    created_at: now,
                    updated_at: now,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    queued_prompts: Vec::new(),
                };
                store
                    .upsert_single_replica_agent("host_ohos_read", &summary)
                    .expect("upsert replica agent");
            }
        }

        // Re-open facade so OrbitAppCore hydrates both cached agents from SQLite.
        let facade = OhosCoreFacade::open(&db_str).expect("reopen facade");
        facade.mark_agent_unread("host_ohos_read", "ag_read_target");
        facade.mark_agent_unread("host_ohos_read", "ag_unread_peer");

        let find_unread = |sidebar_json: &str, target_id: &str| -> Option<bool> {
            let val: serde_json::Value = serde_json::from_str(sidebar_json).expect("sidebar json");
            for group in val["projects"].as_array()? {
                for conv in group["conversations"].as_array()? {
                    if conv["agent"]["agent_id"].as_str() == Some(target_id) {
                        return conv["is_unread"].as_bool();
                    }
                }
            }
            None
        };

        let before = facade
            .fleet_sidebar_json(
                Some("recent"),
                Some("all"),
                None,
                Some("host_ohos_read"),
                None,
                Some("/repo/orbit"),
            )
            .expect("sidebar before");
        assert_eq!(find_unread(&before, "ag_read_target"), Some(true));
        assert_eq!(find_unread(&before, "ag_unread_peer"), Some(true));

        facade.mark_agent_read("host_ohos_read", "ag_read_target");

        let after = facade
            .fleet_sidebar_json(
                Some("recent"),
                Some("all"),
                None,
                Some("host_ohos_read"),
                None,
                Some("/repo/orbit"),
            )
            .expect("sidebar after");
        assert_eq!(find_unread(&after, "ag_read_target"), Some(false));
        assert_eq!(find_unread(&after, "ag_unread_peer"), Some(true));

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(format!("{db_str}-wal"));
        let _ = std::fs::remove_file(format!("{db_str}-shm"));
    }
}
