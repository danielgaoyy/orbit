//! Device-local native zoom shared by the main window and detached terminals.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use crate::DETACHED_TERMINAL_WINDOW_PREFIX;

pub const ZOOM_MIN_PERCENT: u16 = 80;
pub const ZOOM_MAX_PERCENT: u16 = 150;
pub const ZOOM_STEP_PERCENT: u16 = 10;
pub const ZOOM_DEFAULT_PERCENT: u16 = 100;
pub const ZOOM_CHANGED_EVENT: &str = "orbit://zoom-changed";

pub(crate) fn snap(percent: u16) -> u16 {
    let rounded = percent.saturating_add(ZOOM_STEP_PERCENT / 2) / ZOOM_STEP_PERCENT;
    (rounded * ZOOM_STEP_PERCENT).clamp(ZOOM_MIN_PERCENT, ZOOM_MAX_PERCENT)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ZoomStep {
    In,
    Out,
}

impl TryFrom<i8> for ZoomStep {
    type Error = &'static str;

    fn try_from(delta: i8) -> Result<Self, Self::Error> {
        match delta {
            1 => Ok(Self::In),
            -1 => Ok(Self::Out),
            _ => Err("invalid zoom delta"),
        }
    }
}

pub(crate) fn step(percent: u16, direction: ZoomStep) -> u16 {
    let current = snap(percent);
    snap(match direction {
        ZoomStep::In => current + ZOOM_STEP_PERCENT,
        ZoomStep::Out => current - ZOOM_STEP_PERCENT,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct UiZoomState {
    pub zoom_percent: u16,
    pub min_percent: u16,
    pub max_percent: u16,
    pub step_percent: u16,
}

impl UiZoomState {
    fn new(zoom_percent: u16) -> Self {
        Self {
            zoom_percent,
            min_percent: ZOOM_MIN_PERCENT,
            max_percent: ZOOM_MAX_PERCENT,
            step_percent: ZOOM_STEP_PERCENT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomSource {
    Startup,
    Menu,
    Shortcut,
    Settings,
}

impl ZoomSource {
    fn validate_command(self) -> Result<(), String> {
        match self {
            Self::Shortcut | Self::Settings => Ok(()),
            Self::Startup | Self::Menu => Err("invalid zoom source".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UiZoomChangedPayload {
    #[serde(flatten)]
    pub state: UiZoomState,
    pub source: ZoomSource,
}

pub type PersistZoom = Arc<dyn Fn(u16) + Send + Sync>;

pub struct ZoomController {
    percent: Mutex<u16>,
    persist: Option<PersistZoom>,
}

#[derive(Debug, PartialEq, Eq)]
enum ZoomEffect {
    Apply,
    Notify,
    None,
}

impl ZoomController {
    pub fn new(percent: u16, persist: Option<PersistZoom>) -> Self {
        Self {
            percent: Mutex::new(snap(percent)),
            persist,
        }
    }

    fn current(&self) -> u16 {
        *self.percent.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn update(
        &self,
        next: impl FnOnce(u16) -> u16,
        source: ZoomSource,
    ) -> (UiZoomState, ZoomEffect) {
        let mut percent = self.percent.lock().unwrap_or_else(|e| e.into_inner());
        let next = next(*percent);
        let effect = if next != *percent {
            *percent = next;
            ZoomEffect::Apply
        } else if matches!(source, ZoomSource::Menu | ZoomSource::Shortcut)
            && matches!(next, ZOOM_MIN_PERCENT | ZOOM_MAX_PERCENT)
        {
            ZoomEffect::Notify
        } else {
            ZoomEffect::None
        };
        (UiZoomState::new(next), effect)
    }

    fn apply_change(
        &self,
        app: &AppHandle,
        next: impl FnOnce(u16) -> u16,
        source: ZoomSource,
    ) -> UiZoomState {
        // Native menu events and synchronous commands run on the UI thread. Release
        // the percent lock before calling into the window system or event listeners.
        let (state, effect) = self.update(next, source);
        if effect == ZoomEffect::None {
            return state;
        }
        if effect == ZoomEffect::Apply {
            for (label, window) in app.webview_windows() {
                if is_orbit_ui_window(&label) {
                    set_window_zoom(&window, state.zoom_percent);
                }
            }
            #[cfg(target_os = "macos")]
            if let Some(items) = app.try_state::<crate::app_menu::ZoomMenuItems>() {
                crate::app_menu::sync(&items, state.zoom_percent);
            }
            if let Some(persist) = &self.persist {
                persist(state.zoom_percent);
            }
        }
        if let Err(error) = app.emit(ZOOM_CHANGED_EVENT, UiZoomChangedPayload { state, source }) {
            tracing::warn!(%error, "Failed to broadcast desktop zoom");
        }
        state
    }
}

pub fn is_orbit_ui_window(label: &str) -> bool {
    label == "main" || label.starts_with(DETACHED_TERMINAL_WINDOW_PREFIX)
}

pub(crate) fn set_window_zoom(window: &WebviewWindow, percent: u16) {
    if let Err(error) = window.set_zoom(f64::from(percent) / 100.0) {
        tracing::warn!(%error, window = window.label(), percent, "Failed to set desktop zoom");
    }
}

pub fn inherit(window: &WebviewWindow) {
    if is_orbit_ui_window(window.label()) {
        // The detached window is built on a worker. Read the current value on the UI
        // thread so a simultaneous shortcut cannot be overwritten by an older value.
        let target = window.clone();
        if let Err(error) = window.run_on_main_thread(move || {
            set_window_zoom(&target, target.state::<ZoomController>().current());
        }) {
            tracing::warn!(%error, window = window.label(), "Failed to inherit desktop zoom");
        }
    }
}

#[tauri::command]
pub fn get_ui_zoom(state: State<'_, ZoomController>) -> UiZoomState {
    UiZoomState::new(state.current())
}

#[tauri::command]
pub fn step_ui_zoom(
    app: AppHandle,
    state: State<'_, ZoomController>,
    delta: i8,
    source: ZoomSource,
) -> Result<UiZoomState, String> {
    source.validate_command()?;
    let direction = ZoomStep::try_from(delta)?;
    Ok(state.apply_change(&app, |p| step(p, direction), source))
}

#[tauri::command]
pub fn set_ui_zoom(
    app: AppHandle,
    _state: State<'_, ZoomController>,
    zoom_percent: u16,
    source: ZoomSource,
) -> Result<UiZoomState, String> {
    source.validate_command()?;
    Ok(apply(&app, zoom_percent, source))
}

pub fn apply(app: &AppHandle, next: u16, source: ZoomSource) -> UiZoomState {
    app.state::<ZoomController>()
        .apply_change(app, |_| snap(next), source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_nearest_step_and_clamps_all_u16_inputs() {
        for (input, expected) in [
            (0, 80),
            (79, 80),
            (84, 80),
            (85, 90),
            (100, 100),
            (149, 150),
            (151, 150),
            (u16::MAX, 150),
        ] {
            assert_eq!(snap(input), expected, "input {input}");
        }
    }

    #[test]
    fn steps_from_snapped_values_and_stops_at_the_limits() {
        assert_eq!(step(135, ZoomStep::Out), 130);
        assert_eq!(step(135, ZoomStep::In), 150);
        assert_eq!(step(80, ZoomStep::Out), 80);
        assert_eq!(step(150, ZoomStep::In), 150);
        for delta in [i8::MIN, -2, 0, 2, i8::MAX] {
            assert_eq!(ZoomStep::try_from(delta), Err("invalid zoom delta"));
        }
        assert_eq!(ZoomStep::try_from(-1), Ok(ZoomStep::Out));
        assert_eq!(ZoomStep::try_from(1), Ok(ZoomStep::In));
    }

    #[test]
    fn only_changed_values_apply_and_only_user_shortcuts_and_menus_notify_at_limits() {
        let controller = ZoomController::new(100, None);
        for source in [
            ZoomSource::Menu,
            ZoomSource::Shortcut,
            ZoomSource::Settings,
            ZoomSource::Startup,
        ] {
            assert_eq!(controller.update(|_| 100, source).1, ZoomEffect::None);
        }
        for limit in [150, 80] {
            assert_eq!(
                controller.update(|_| limit, ZoomSource::Settings).1,
                ZoomEffect::Apply
            );
            assert_eq!(controller.current(), limit);
            for source in [ZoomSource::Menu, ZoomSource::Shortcut] {
                assert_eq!(controller.update(|_| limit, source).1, ZoomEffect::Notify);
            }
            for source in [ZoomSource::Settings, ZoomSource::Startup] {
                assert_eq!(controller.update(|_| limit, source).1, ZoomEffect::None);
            }
        }
    }

    #[test]
    fn wire_sources_round_trip_and_only_frontend_sources_are_accepted() {
        for (source, wire, allowed) in [
            (ZoomSource::Startup, "startup", false),
            (ZoomSource::Menu, "menu", false),
            (ZoomSource::Shortcut, "shortcut", true),
            (ZoomSource::Settings, "settings", true),
        ] {
            let value = serde_json::to_value(source).expect("serialize zoom source");
            assert_eq!(value, wire);
            assert_eq!(
                serde_json::from_value::<ZoomSource>(value).expect("deserialize zoom source"),
                source
            );
            assert_eq!(source.validate_command().is_ok(), allowed);
        }
        let payload = UiZoomChangedPayload {
            state: UiZoomState::new(130),
            source: ZoomSource::Shortcut,
        };
        assert_eq!(
            serde_json::to_value(payload).expect("serialize zoom event"),
            serde_json::json!({
                "zoom_percent": 130, "min_percent": 80, "max_percent": 150, "step_percent": 10, "source": "shortcut"
            })
        );
    }

    #[test]
    fn only_main_and_detached_terminal_windows_take_part() {
        assert!(is_orbit_ui_window("main"));
        assert!(is_orbit_ui_window("orbit-terminal-x"));
        assert!(!is_orbit_ui_window("orbit-browser-x"));
        assert!(!is_orbit_ui_window(""));
        assert!(!is_orbit_ui_window("main-preview"));
    }
}
