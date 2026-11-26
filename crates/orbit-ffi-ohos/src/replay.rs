//! In-memory source for recorded stage deltas in performance builds.
use orbit_client_core::app_core::{CoreEventSubscriber, CoreNotification};
use orbit_client_core::contract::AgentStageDeltaRequest;
use orbit_client_core::stage::{AgentStageDeltaView, StageTurnView};
use serde::de::IgnoredAny;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_FRAMES: usize = 5000;
const FRAME_PERIOD: Duration = Duration::from_millis(25);

#[derive(Default)]
pub(crate) struct Replay {
    subscribers: Vec<Weak<dyn CoreEventSubscriber>>,
    sessions: HashMap<(String, String), Session>,
}

#[derive(Default)]
struct Session {
    turns: Vec<StageTurnView>,
    latest: Option<AgentStageDeltaView>,
    previous_revision: Option<u64>,
    running: bool,
}

impl Session {
    fn apply(&mut self, mut frame: AgentStageDeltaView) {
        let replace = frame.replace_from_turn_index as usize;
        self.turns.truncate(replace);
        self.turns.append(&mut frame.changed_turns);
        if let Some(previous) = self.latest.take() {
            self.previous_revision = Some(previous.revision);
            if frame.aux_unchanged {
                frame.pending_permissions = previous.pending_permissions;
                frame.pending_questions = previous.pending_questions;
                frame.pending_plan_approval = previous.pending_plan_approval;
                frame.insights = previous.insights;
                frame.session_turn_diffs = previous.session_turn_diffs;
            }
        }
        self.latest = Some(frame);
    }

    fn read(&self, req: &AgentStageDeltaRequest) -> Option<AgentStageDeltaView> {
        let mut out = self.latest.clone()?;
        let start = out.window_start_turn_index;
        let window = req
            .turn_limit
            .filter(|&n| n > 0)
            .map_or(start, |n| out.total_turn_count.saturating_sub(n).max(start));
        let unchanged = req.since_revision == out.revision;
        let new_window = req
            .since_window_start_turn_index
            .is_some_and(|w| w != window);
        out.reset = req.since_revision == 0
            || new_window
            || (!unchanged && (out.reset || self.previous_revision != Some(req.since_revision)));
        let offset = if out.reset {
            window
        } else if unchanged {
            out.total_turn_count
        } else {
            (start + out.replace_from_turn_index).max(window)
        };
        out.changed_turns = self.turns[(offset - start) as usize..].to_vec();
        out.window_start_turn_index = window;
        out.replace_from_turn_index = offset - window;
        out.has_older_turns |= window > 0;
        out.aux_unchanged = !out.reset && req.since_aux_revision == Some(out.aux_revision);
        if out.aux_unchanged {
            out.pending_permissions.clear();
            out.pending_questions.clear();
            out.pending_plan_approval = None;
            out.session_turn_diffs.clear();
            out.insights = Default::default();
        }
        Some(out)
    }
}

impl Replay {
    pub(crate) fn subscribe(&mut self, subscriber: &Arc<dyn CoreEventSubscriber>) {
        self.subscribers.retain(|s| s.strong_count() > 0);
        self.subscribers.push(Arc::downgrade(subscriber));
    }

    pub(crate) fn delta(&self, req: &AgentStageDeltaRequest) -> Option<AgentStageDeltaView> {
        self.sessions
            .get(&(req.host_id.clone(), req.agent_id.clone()))?
            .read(req)
    }

    pub(crate) fn start(
        state: &Arc<Mutex<Self>>,
        host: &str,
        agent: &str,
        json: &str,
    ) -> Result<(), String> {
        if json.len() > MAX_BYTES {
            return Err("replay exceeds 8 MiB".into());
        }
        // Count without allocating a DTO per entry until the bound is known.
        let count: Vec<IgnoredAny> = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if count.len() > MAX_FRAMES {
            return Err("replay exceeds 5000 frames".into());
        }
        let mut frames: Vec<AgentStageDeltaView> =
            serde_json::from_str(json).map_err(|e| e.to_string())?;
        let (mut previous, mut window, mut len) = (None, 0, 0);
        for frame in &mut frames {
            if (previous.is_some_and(|rev| frame.revision < rev) && !frame.reset)
                || frame.replace_from_turn_index > len
                || ((previous.is_none() || frame.reset || frame.window_start_turn_index != window)
                    && frame.replace_from_turn_index != 0)
                || u64::from(frame.window_start_turn_index)
                    + u64::from(frame.replace_from_turn_index)
                    + frame.changed_turns.len() as u64
                    != u64::from(frame.total_turn_count)
            {
                return Err(format!(
                    "invalid replay delta at revision {}",
                    frame.revision
                ));
            }
            frame.reset |= previous.is_none() || frame.window_start_turn_index != window;
            previous = Some(frame.revision);
            window = frame.window_start_turn_index;
            len = frame.total_turn_count - window;
            frame.host_id = host.to_owned();
            if let Some(card) = &mut frame.agent {
                card.agent_id = agent.to_owned();
            }
        }
        let key = (host.to_owned(), agent.to_owned());
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        if guard.sessions.get(&key).is_some_and(|s| s.running) {
            return Err("replay already running for this session".into());
        }
        if frames.is_empty() {
            guard.sessions.remove(&key);
            return Ok(());
        }
        if !guard.subscribers.iter().any(|s| s.strong_count() > 0) {
            return Err("subscribe to events before starting replay".into());
        }
        let weak = Arc::downgrade(state);
        let scope = key.clone();
        // Hold the start lock until the worker exists and its session is installed.
        std::thread::Builder::new()
            .name("orbit-stage-replay".into())
            .spawn(move || {
                let mut frames = frames.into_iter().peekable();
                // Anchor playback to the recording clock so scheduler delays do not accumulate.
                let mut next_frame = Instant::now();
                while let Some(frame) = frames.next() {
                    let Some(state) = weak.upgrade() else { return };
                    let event = CoreNotification::TimelineChanged {
                        host_id: scope.0.clone(),
                        agent_id: scope.1.clone(),
                        revision: frame.revision,
                    };
                    let listeners: Vec<_> = {
                        let Ok(mut guard) = state.lock() else { return };
                        let Some(session) = guard.sessions.get_mut(&scope) else {
                            return;
                        };
                        session.apply(frame);
                        session.running = frames.peek().is_some();
                        guard.subscribers.iter().filter_map(Weak::upgrade).collect()
                    };
                    for listener in listeners {
                        listener.on_notification(event.clone());
                    }
                    if frames.peek().is_some() {
                        next_frame += FRAME_PERIOD;
                        std::thread::sleep(next_frame.saturating_duration_since(Instant::now()));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        guard.sessions.insert(
            key,
            Session {
                running: true,
                ..Default::default()
            },
        );
        Ok(())
    }
}
