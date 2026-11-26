use orbit_ohos::OhosCoreFacade;

#[cfg(not(feature = "perf-hooks"))]
#[test]
fn ordinary_build_rejects_replay_before_parsing() {
    let facade = OhosCoreFacade::open_in_memory().expect("in-memory core");
    assert_eq!(
        facade.debug_replay_stage_deltas_json("host", "agent", "invalid"),
        Err(String::from("perf hooks disabled"))
    );
}

#[cfg(feature = "perf-hooks")]
#[test]
fn recorded_frames_drive_scoped_invalidation_and_delta_pulls() {
    use orbit_client_core::stage::AgentStageDeltaView;
    use serde_json::{Value, json};
    use std::{sync::mpsc, time::Duration};

    let facade = OhosCoreFacade::open_in_memory().expect("in-memory core");
    let (tx, rx) = mpsc::channel();
    facade.subscribe(move |event| {
        let _ = tx.send(event);
    });
    let fixture = include_str!("../../../apps/harmony/contract/fixtures/AgentStageDeltaView.json");
    let first: AgentStageDeltaView = serde_json::from_str(fixture).expect("stage fixture");
    let mut second = first.clone();
    second.revision += 1;
    second.reset = false;
    second.replace_from_turn_index = 1;
    second.total_turn_count = 2;
    second.changed_turns[0].turn_index = 1;
    let frames = serde_json::to_string(&[first, second]).expect("recording");
    facade
        .debug_replay_stage_deltas_json("host", "agent", &frames)
        .expect("start replay");
    for revision in [2, 3] {
        let event: Value = serde_json::from_str(
            &rx.recv_timeout(Duration::from_secs(2))
                .expect("timeline callback"),
        )
        .expect("event JSON");
        assert_eq!(
            event,
            json!({"kind":"timeline_changed","host_id":"host","agent_id":"agent","revision":revision})
        );
    }
    let pull = |host, agent, revision| -> AgentStageDeltaView {
        serde_json::from_str(
            &facade
                .agent_stage_delta_json(host, agent, revision)
                .expect("pull"),
        )
        .expect("stage delta")
    };
    assert_eq!(pull("host", "agent", 0).changed_turns.len(), 2);
    assert_eq!(pull("host", "agent", 2).replace_from_turn_index, 1);
    assert!(pull("host", "agent", 3).changed_turns.is_empty());
    assert_eq!(pull("other", "agent", 0).total_turn_count, 0);
    assert_eq!(pull("host", "other", 0).total_turn_count, 0);
    let request = json!({"host_id":"host","agent_id":"agent","turn_limit":1});
    let window: AgentStageDeltaView = serde_json::from_str(
        &facade
            .agent_stage_delta_by_request_json(&request.to_string())
            .expect("windowed pull"),
    )
    .expect("window delta");
    assert_eq!(window.window_start_turn_index, 1);
    assert_eq!(window.changed_turns.len(), 1);
    facade
        .debug_replay_stage_deltas_json("host", "agent", "[]")
        .expect("release scope");
    assert_eq!(pull("host", "agent", 0).total_turn_count, 0);
}

#[cfg(feature = "perf-hooks")]
#[test]
fn rejects_oversized_and_invalid_recordings_before_replay() {
    let facade = OhosCoreFacade::open_in_memory().expect("in-memory core");
    for (json, reason) in [
        (" ".repeat(8 * 1024 * 1024 + 1), "8 MiB"),
        (format!("[{}null]", "null,".repeat(5000)), "5000 frames"),
        (String::from("[{}]"), "missing field"),
    ] {
        let error = facade
            .debug_replay_stage_deltas_json("h", "a", &json)
            .expect_err("must reject recording");
        assert!(error.contains(reason), "{error}");
    }
}

#[cfg(feature = "perf-hooks")]
#[test]
fn stream_500_reaches_the_subscriber_and_retains_all_turns() {
    use orbit_client_core::stage::AgentStageDeltaView;
    use serde_json::Value;
    use std::{fs, path::Path, sync::mpsc, time::Duration};

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/harmony/scripts/perf/fixtures/stream-500.json");
    let recording = fs::read_to_string(path).expect("read stream-500 fixture");
    let facade = OhosCoreFacade::open_in_memory().expect("in-memory core");
    let (tx, rx) = mpsc::channel();
    facade.subscribe(move |event| {
        let _ = tx.send(event);
    });
    facade
        .debug_replay_stage_deltas_json("test-host", "test-agent", &recording)
        .expect("start replay");
    for revision in 200..700 {
        let event = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("next recorded frame");
        let event: Value = serde_json::from_str(&event).expect("timeline event");
        assert_eq!(event["revision"], revision);
    }
    let json = facade
        .agent_stage_delta_json("test-host", "test-agent", 0)
        .expect("reset pull");
    let snapshot: AgentStageDeltaView = serde_json::from_str(&json).expect("reset delta");
    assert_eq!(snapshot.changed_turns.len(), 100);
    assert_eq!(snapshot.revision, 699);
    assert!(
        snapshot
            .changed_turns
            .last()
            .expect("last turn")
            .terminal_reply
            .is_some()
    );
}
