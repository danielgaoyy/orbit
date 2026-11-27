use super::OhosCoreFacade;
use crate::facade::validate_ohos_git_discard_path;
use orbit_client_core::render::{
    HighlightedCodeView, MAX_WORKSPACE_HIGHLIGHT_LINES, TerminalScreenDeltaView,
};
use serde_json::Value;

#[test]
fn pr5_highlight_workspace_code_json_enforces_cap_utf8_and_unknown_language() {
    let facade = OhosCoreFacade::open_in_memory().expect("open facade");

    let big_code: String = (1..=320)
        .map(|i| format!("const item_{i}: string = '项目 {i} 🎯';\n"))
        .collect();
    let req = serde_json::json!({
        "language": "ts",
        "code": big_code,
        "max_lines": 999
    })
    .to_string();
    let raw = facade
        .highlight_workspace_code_json(&req)
        .expect("highlight json");
    let view: HighlightedCodeView = serde_json::from_str(&raw).expect("parse highlighted view");
    assert_eq!(view.total_lines, 320);
    assert!(view.truncated);
    assert_eq!(view.lines.len(), MAX_WORKSPACE_HIGHLIGHT_LINES);
    let first_line: String = view.lines[0]
        .tokens
        .iter()
        .map(|t| t.text.as_str())
        .collect();
    assert!(first_line.contains("项目 1 🎯"));

    // Empty input and unknown language fallback
    let empty_raw = facade
        .highlight_workspace_code_json(r#"{"language":"unknown_xyz","code":"","max_lines":50}"#)
        .expect("empty highlight");
    let empty_view: HighlightedCodeView =
        serde_json::from_str(&empty_raw).expect("parse empty view");
    assert_eq!(empty_view.total_lines, 0);
    assert!(!empty_view.truncated);
    assert!(empty_view.lines.is_empty());

    let unknown_raw = facade
        .highlight_workspace_code_json(
            r#"{"language":"unknown_xyz","code":"alpha\nβeta\n","max_lines":0}"#,
        )
        .expect("unknown lang highlight");
    let unknown_view: HighlightedCodeView =
        serde_json::from_str(&unknown_raw).expect("parse unknown view");
    assert_eq!(unknown_view.total_lines, 2);
    assert!(!unknown_view.truncated);
    assert_eq!(unknown_view.lines.len(), 2);
}

#[test]
fn pr5_terminal_screen_delta_slot_json_handles_unknown_and_paired_hosts() {
    let facade = OhosCoreFacade::open_in_memory().expect("open facade");
    assert_eq!(
        facade
            .terminal_screen_delta_slot_json("missing-host", 1, 0)
            .expect("unknown delta"),
        "null"
    );

    let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
    let offer = orbit_crypto::PairingOffer {
        host_id: "host_pr5_term".into(),
        hostname: "TermBox".into(),
        daemon_pubkey_b64: kp.public_key_b64,
        direct_endpoints: Vec::new(),
        relay_url: None,
        pairing_secret: "secret123".into(),
    };
    let uri = offer.to_qr_uri().expect("pair uri");
    let host_json = facade.pair_host_json(&uri).expect("pair host");
    assert!(host_json.contains("host_pr5_term"));

    let delta_raw = facade
        .terminal_screen_delta_slot_json("host_pr5_term", 2, 0)
        .expect("delta slot 2");
    let delta: TerminalScreenDeltaView =
        serde_json::from_str(&delta_raw).expect("parse terminal delta");
    assert!(delta.reset);
    assert_eq!(delta.slot, 2);
    assert_eq!(delta.changed_rows.len(), usize::from(delta.rows));

    let steady_raw = facade
        .terminal_screen_delta_slot_json("host_pr5_term", 2, delta.revision)
        .expect("steady delta");
    let steady: TerminalScreenDeltaView =
        serde_json::from_str(&steady_raw).expect("parse steady delta");
    assert!(!steady.reset);
    assert_eq!(steady.revision, delta.revision);
    assert!(steady.changed_rows.is_empty());
}

#[test]
fn pr5_git_discard_path_guard_preserves_trailing_spaces_and_blocks_traversal() {
    assert!(validate_ohos_git_discard_path("src/main.rs").is_ok());
    assert!(validate_ohos_git_discard_path("docs/trailing space ").is_ok());
    assert!(validate_ohos_git_discard_path(" leading/and trailing  ").is_ok());

    for bad in [
        "",
        "../secret.txt",
        "src/../../etc/passwd",
        "/etc/passwd",
        "~/secret",
        ".git/HEAD",
        "src/.GIT/config",
        "-rf",
        "src\\windows.rs",
        "bad\0file",
        "bad\nfile",
        ".",
    ] {
        assert!(
            validate_ohos_git_discard_path(bad).is_err(),
            "expected invalid discard path to be rejected: {bad:?}"
        );
    }

    let facade = OhosCoreFacade::open_in_memory().expect("open facade");
    let err_traversal = facade
        .git_discard_files_json(
            r#"{"host_id":"h1","cwd":"/repo","paths":["../escape.txt"],"include_untracked":false}"#,
        )
        .expect_err("traversal must fail before RPC");
    assert!(err_traversal.contains("traversal"));

    let err_empty = facade
        .git_discard_files_json(
            r#"{"host_id":"h1","cwd":"/repo","paths":[],"include_untracked":false}"#,
        )
        .expect_err("empty paths must fail");
    assert!(err_empty.contains("at least one path"));

    // Valid trailing-space path passes validation and reaches host lookup (host offline/missing)
    let err_host = facade
        .git_discard_files_json(
            r#"{"host_id":"missing_host","cwd":"/repo","paths":["keep trailing space "],"include_untracked":true}"#,
        )
        .expect_err("unknown host error");
    assert!(
        !err_host.contains("traversal") && !err_host.contains("invalid"),
        "trailing-space path should pass validation, got: {err_host}"
    );
}

#[test]
fn pr5_git_push_empty_remote_and_workspace_v2_scripts_handoff_validation() {
    let facade = OhosCoreFacade::open_in_memory().expect("open facade");

    // Empty host/cwd rejected before RPC
    assert!(
        facade
            .git_push_json(
                r#"{"host_id":"","cwd":"/repo","remote":"","branch":null,"set_upstream":false}"#
            )
            .is_err()
    );
    // Empty remote/branch normalize to None and proceed to host lookup (not JSON parse error)
    let push_err = facade
        .git_push_json(
            r#"{"host_id":"missing_host","cwd":"/repo","remote":"   ","branch":"","set_upstream":true}"#,
        )
        .expect_err("offline host error");
    assert!(
        !push_err.contains("invalid git_push"),
        "empty remote must normalize to default remote, got: {push_err}"
    );

    // Workspace v2 validation & dispatch
    assert!(
        facade
            .create_workspace_v2_json(
                r#"{"host_id":"h1","source_cwd":"/repo","title":"PR-5","isolation":"worktree","worktree_mode":"branch_off","worktree_branch":"feat/pr5","base_ref":"main"}"#
            )
            .is_ok()
    );
    assert!(
        facade
            .create_workspace_v2_json(
                r#"{"host_id":"h1","source_cwd":"/repo","isolation":"local"}"#
            )
            .is_ok()
    );
    assert!(
        facade
            .create_workspace_v2_json(
                r#"{"host_id":"h1","source_cwd":"/repo","isolation":"docker"}"#
            )
            .is_err()
    );
    assert!(
        facade
            .create_workspace_v2_json(
                r#"{"host_id":"h1","source_cwd":"/repo","worktree_mode":"invalid_mode"}"#
            )
            .is_err()
    );

    // Archive / unarchive / rename confirmed validation & offline rejection
    let archive_offline = facade
        .archive_workspace_v2_json("h1", "ws-1", true)
        .expect_err("offline host archive must not report false success");
    assert!(!archive_offline.contains("requires non-empty"));
    assert!(facade.archive_workspace_v2_json("", "ws-1", false).is_err());
    let unarchive_offline = facade
        .unarchive_workspace_json("h1", "ws-1")
        .expect_err("offline host unarchive must not report false success");
    assert!(!unarchive_offline.contains("requires non-empty"));
    assert!(facade.unarchive_workspace_json("h1", "").is_err());
    let rename_offline = facade
        .rename_workspace_confirmed_json("h1", "ws-1", "Renamed")
        .expect_err("offline host rename must not report false success");
    assert!(!rename_offline.contains("requires non-empty"));
    assert!(
        facade
            .rename_workspace_confirmed_json("h1", "ws-1", "   ")
            .is_err()
    );
    assert!(facade.request_workspace_scripts("h1", "ws-1").is_ok());
    assert!(facade.request_workspace_scripts("", "ws-1").is_err());
    assert!(
        facade
            .start_workspace_script_json("h1", "ws-1", "dev")
            .is_ok()
    );
    assert!(
        facade
            .start_workspace_script_json("h1", "ws-1", " ")
            .is_err()
    );
    assert!(
        facade
            .stop_workspace_script_json("h1", "ws-1", "dev")
            .is_ok()
    );
    assert!(facade.stop_workspace_script_json("h1", "", "dev").is_err());

    // Worktree handoff JSON validation
    let handoff_parse_err = facade
        .execute_worktree_handoff_json("{bad-json")
        .expect_err("malformed json");
    assert!(handoff_parse_err.contains("ExecuteWorktreeHandoffRequest"));

    // Board CAS JSON validation (rejects empty host/workspace/malformed JSON before RPC)
    assert!(facade.update_workspace_board_cas_json("{bad").is_err());
    assert!(
        facade
            .update_workspace_board_cas_json(
                r#"{"host_id":"","workspace_id":"ws-1","content":"x","expected_revision":0}"#
            )
            .is_err()
    );
    assert!(
        facade
            .update_workspace_board_cas_json(
                r#"{"host_id":"h1","workspace_id":"","content":"x","expected_revision":0}"#
            )
            .is_err()
    );
}

#[test]
fn pr5_start_terminal_slot_and_minified_highlight_budget_isolation() {
    let facade = OhosCoreFacade::open_in_memory().expect("open facade");
    let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
    let offer = orbit_crypto::PairingOffer {
        host_id: "host_pr5_slot_iso".into(),
        hostname: "SlotIsoBox".into(),
        daemon_pubkey_b64: kp.public_key_b64,
        direct_endpoints: Vec::new(),
        relay_url: None,
        pairing_secret: "test-secret".into(),
    };
    let uri = offer.to_qr_uri().expect("pair uri");
    let host_json = facade.pair_host_json(&uri).expect("pair host");
    let host_val: Value = serde_json::from_str(&host_json).expect("parse host");
    let host_id = host_val["host_id"].as_str().expect("host_id");

    // Seed slot 1 and slot 2 deltas, then invoke start_terminal_slot on slot 2 only:
    // slot 1 revision must remain untouched while slot 2 resets.
    let s1_before: Value = serde_json::from_str(
        &facade
            .terminal_screen_delta_slot_json(host_id, 1, 0)
            .expect("slot 1 init"),
    )
    .expect("slot 1 json");
    let s2_before: Value = serde_json::from_str(
        &facade
            .terminal_screen_delta_slot_json(host_id, 2, 0)
            .expect("slot 2 init"),
    )
    .expect("slot 2 json");
    let rev1 = s1_before["revision"].as_u64().expect("rev1");
    let rev2 = s2_before["revision"].as_u64().expect("rev2");

    facade.start_terminal_slot(host_id, "/tmp", 100, 30, 2);

    let s1_after: Value = serde_json::from_str(
        &facade
            .terminal_screen_delta_slot_json(host_id, 1, rev1)
            .expect("slot 1 after"),
    )
    .expect("slot 1 after json");
    assert_eq!(s1_after["reset"].as_bool(), Some(false));
    assert_eq!(s1_after["changed_rows"].as_array().map(Vec::len), Some(0));

    let s2_after: Value = serde_json::from_str(
        &facade
            .terminal_screen_delta_slot_json(host_id, 2, rev2)
            .expect("slot 2 after"),
    )
    .expect("slot 2 after json");
    assert_eq!(s2_after["reset"].as_bool(), Some(true));
    assert_eq!(s2_after["cols"].as_u64(), Some(100));
    assert_eq!(s2_after["rows"].as_u64(), Some(30));

    // Minified 88 KB single-line JS highlight stays bounded (< 16 KB JSON) and reports truncated=true.
    let minified = "const x=1;function g(y){return x+y;}".repeat(2_500);
    let req = serde_json::json!({
        "language": "js",
        "code": minified,
        "max_lines": 250
    });
    let out_json = facade
        .highlight_workspace_code_json(&req.to_string())
        .expect("highlight minified");
    assert!(
        out_json.len() < 16 * 1024,
        "highlight JSON exceeded 16 KB: {} bytes",
        out_json.len()
    );
    let parsed: Value = serde_json::from_str(&out_json).expect("parse minified");
    assert_eq!(parsed["truncated"].as_bool(), Some(true));
    assert_eq!(parsed["total_lines"].as_u64(), Some(1));

    // R3-P1-3: Pre-clipped 4,096 B minified JS & 4,095 B emoji-boundary identifier with
    // `plain_line_numbers: [1]` bypass syntect regex parsing cleanly in O(1) and emit 1 token.
    let preclipped_minified =
        "const x=1;function g(y){return x+y;}".repeat(120)[..4096].to_string();
    let req_preclipped_min = serde_json::json!({
        "language": "js",
        "code": preclipped_minified,
        "max_lines": 250,
        "plain_line_numbers": [1]
    })
    .to_string();
    let req_preclipped_ident = serde_json::json!({
        "language": "js",
        "code": "a".repeat(4095),
        "max_lines": 250,
        "plain_line_numbers": [1]
    })
    .to_string();
    let req_untagged_ident = serde_json::json!({
        "language": "js",
        "code": "a".repeat(4095),
        "max_lines": 250
    })
    .to_string();

    for (label, req_str, expect_truncated) in [
        ("preclipped_minified_js_4096", &req_preclipped_min, true),
        (
            "preclipped_emoji_boundary_4095",
            &req_preclipped_ident,
            true,
        ),
        ("untagged_overlong_ident_4095", &req_untagged_ident, false),
    ] {
        // Warmup + 25 timed samples
        let _ = facade
            .highlight_workspace_code_json(req_str)
            .expect("warmup highlight");
        let mut samples_ms = Vec::with_capacity(25);
        let mut out_json = String::new();
        for _ in 0..25 {
            let t0 = std::time::Instant::now();
            out_json = facade
                .highlight_workspace_code_json(req_str)
                .expect("highlight");
            samples_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        samples_ms.sort_by(|a, b| a.total_cmp(b));
        let med_ms = samples_ms[samples_ms.len() / 2];
        let max_ms = samples_ms[samples_ms.len() - 1];
        let parsed_case: Value = serde_json::from_str(&out_json).expect("parse case");
        if expect_truncated {
            assert_eq!(parsed_case["truncated"].as_bool(), Some(true));
        }
        assert_eq!(parsed_case["total_lines"].as_u64(), Some(1));
        assert_eq!(
            parsed_case["lines"][0]["tokens"].as_array().map(Vec::len),
            Some(1),
            "{label} should emit a single O(1) plain-text token"
        );
        eprintln!(
            "OHOS_HIGHLIGHT_BENCH {label}: median={med_ms:.3}ms max={max_ms:.3}ms out_bytes={}",
            out_json.len()
        );
    }

    // R4-P1-2: Pre-clipped 250-line mixed file (line 1 is a 4,096 B clipped comment marked in
    // `plain_line_numbers: [1]`, lines 2..250 are regular JS) returns 250 lines with 1 plain token
    // on line 1 AND full JS syntax tokens (>= 3) on line 2!
    let mut mixed_clipped_lines = vec![format!("// {}", "c".repeat(4093))];
    for _ in 0..249 {
        mixed_clipped_lines.push("const val = 42;".to_string());
    }
    let req_mixed = serde_json::json!({
        "language": "js",
        "code": mixed_clipped_lines.join("\n"),
        "max_lines": 250,
        "plain_line_numbers": [1]
    })
    .to_string();
    let out_mixed = facade
        .highlight_workspace_code_json(&req_mixed)
        .expect("highlight mixed 250 lines");
    let parsed_mixed: Value = serde_json::from_str(&out_mixed).expect("parse mixed");
    assert_eq!(parsed_mixed["truncated"].as_bool(), Some(true));
    assert_eq!(parsed_mixed["total_lines_exact"].as_bool(), Some(true));
    assert_eq!(parsed_mixed["lines"].as_array().map(Vec::len), Some(250));
    assert_eq!(
        parsed_mixed["lines"][0]["tokens"].as_array().map(Vec::len),
        Some(1),
        "line 1 must stay single plain-text span"
    );
    assert!(
        parsed_mixed["lines"][1]["tokens"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0)
            >= 3,
        "line 2 must retain real JS syntax tokens"
    );

    // R5-Nit-1: Pre-clipped 250-line payload ending with empty line 250 (`\n\n`) returns 250 lines
    // with line 250 empty (never dropping to 249 lines).
    let mut empty_250_payload = "const a = 12345;\n".repeat(249);
    empty_250_payload.push('\n');
    let req_empty_250 = serde_json::json!({
        "language": "js",
        "code": empty_250_payload,
        "max_lines": 250
    })
    .to_string();
    let out_empty_250 = facade
        .highlight_workspace_code_json(&req_empty_250)
        .expect("highlight empty line 250");
    let parsed_empty_250: Value = serde_json::from_str(&out_empty_250).expect("parse empty 250");
    assert_eq!(
        parsed_empty_250["lines"].as_array().map(Vec::len),
        Some(250)
    );
    assert_eq!(
        parsed_empty_250["lines"][249]["line_number"].as_u64(),
        Some(250)
    );
    assert_eq!(
        parsed_empty_250["lines"][249]["tokens"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    // R5-P1: Pre-clipped 250-line payload where line 2 was 65,536 chars (`plain_line_numbers: [2]`)
    // returns 250 lines with JS syntax tokens on lines 1 and 3 and 1 plain span on line 2.
    let mut mid_65536_payload = String::from("const first = 1;\n");
    mid_65536_payload.push_str("// ");
    mid_65536_payload.push_str(&"c".repeat(4093));
    mid_65536_payload.push('\n');
    mid_65536_payload.push_str(&"const val = 42;\n".repeat(248));
    let req_mid_65536 = serde_json::json!({
        "language": "js",
        "code": mid_65536_payload,
        "max_lines": 250,
        "plain_line_numbers": [2]
    })
    .to_string();
    let out_mid_65536 = facade
        .highlight_workspace_code_json(&req_mid_65536)
        .expect("highlight mid 65536");
    let parsed_mid_65536: Value = serde_json::from_str(&out_mid_65536).expect("parse mid 65536");
    assert_eq!(
        parsed_mid_65536["lines"].as_array().map(Vec::len),
        Some(250)
    );
    assert!(
        parsed_mid_65536["lines"][0]["tokens"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0)
            >= 3
    );
    assert_eq!(
        parsed_mid_65536["lines"][1]["tokens"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert!(
        parsed_mid_65536["lines"][2]["tokens"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0)
            >= 3
    );
}
