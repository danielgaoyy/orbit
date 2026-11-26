//! Main window geometry and zoom persistence for Orbit Desktop.
//!
//! Persists logical geometry, maximized state, and native zoom atomically to
//! `$ORBIT_HOME/desktop-window-state.json` using debounced non-blocking background
//! writes on move/resize and an immediate non-blocking flush on window close request.
//!
//! On startup, restores saved geometry while enforcing minimum bounds (`680x560`) and
//! validating visibility against `window.available_monitors()`, falling back cleanly
//! to a centered primary-monitor placement whenever an external monitor was detached
//! or saved coordinates are off-screen.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use orbit_client_core::desktop_ops::resolve_orbit_home_dir;
use serde::{Deserialize, Serialize};
use tauri::{
    App, AppHandle, LogicalPosition, LogicalSize, Manager, Monitor, WebviewWindow, WindowEvent,
};

use crate::zoom::{self, PersistZoom, ZOOM_DEFAULT_PERCENT};

pub const WINDOW_STATE_FILENAME: &str = "desktop-window-state.json";
pub const MIN_WINDOW_WIDTH: u32 = 680;
pub const MIN_WINDOW_HEIGHT: u32 = 560;
const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);
const MIN_VISIBLE_HORIZONTAL_PX: i64 = 120;
const MIN_VISIBLE_VERTICAL_PX: i64 = 80;
const TITLEBAR_GRAB_MARGIN_PX: i64 = 40;

static NEXT_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

/// Logical main-window geometry persisted in `$ORBIT_HOME/desktop-window-state.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default = "default_zoom_percent")]
    pub zoom_percent: u16,
}

fn default_zoom_percent() -> u16 {
    ZOOM_DEFAULT_PERCENT
}

/// Logical work-area bounds of an attached display monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Startup geometry after enforcing minimum dimensions and display bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedWindowState {
    pub state: WindowState,
    pub fell_back_to_center: bool,
}

/// Resolve the default path `$ORBIT_HOME/desktop-window-state.json`.
pub fn default_window_state_path() -> Option<PathBuf> {
    resolve_orbit_home_dir().map(|dir| dir.join(WINDOW_STATE_FILENAME))
}

/// Clamp `width` and `height` to Orbit Desktop's minimum usable window dimensions.
pub fn clamp_min_size(mut state: WindowState) -> WindowState {
    state.width = state.width.max(MIN_WINDOW_WIDTH);
    state.height = state.height.max(MIN_WINDOW_HEIGHT);
    state
}

/// Geometry events retain the tracked zoom, and maximized events retain the normal frame.
pub fn merge_captured_window_state(
    previous: Option<WindowState>,
    captured: WindowState,
) -> WindowState {
    let Some(previous) = previous else {
        return clamp_min_size(captured);
    };
    if captured.maximized {
        return clamp_min_size(WindowState {
            maximized: true,
            ..previous
        });
    }
    clamp_min_size(WindowState {
        zoom_percent: previous.zoom_percent,
        ..captured
    })
}

fn clamp_size_to_monitor(width: u32, height: u32, monitor: MonitorRect) -> (u32, u32) {
    let mut w = width.max(MIN_WINDOW_WIDTH);
    let mut h = height.max(MIN_WINDOW_HEIGHT);
    if monitor.width >= MIN_WINDOW_WIDTH {
        w = w.min(monitor.width);
    }
    if monitor.height >= MIN_WINDOW_HEIGHT {
        h = h.min(monitor.height);
    }
    (w, h)
}

fn intersection_dimensions(state: WindowState, monitor: MonitorRect) -> (i64, i64) {
    let win_left = i64::from(state.x);
    let win_top = i64::from(state.y);
    let win_right = win_left + i64::from(state.width);
    let win_bottom = win_top + i64::from(state.height);

    let mon_left = i64::from(monitor.x);
    let mon_top = i64::from(monitor.y);
    let mon_right = mon_left + i64::from(monitor.width);
    let mon_bottom = mon_top + i64::from(monitor.height);

    let overlap_w = (win_right.min(mon_right) - win_left.max(mon_left)).max(0);
    let overlap_h = (win_bottom.min(mon_bottom) - win_top.max(mon_top)).max(0);
    (overlap_w, overlap_h)
}

fn is_visible_on_monitor(state: WindowState, monitor: MonitorRect) -> Option<i64> {
    if monitor.width == 0 || monitor.height == 0 {
        return None;
    }
    let (overlap_w, overlap_h) = intersection_dimensions(state, monitor);
    let min_w = MIN_VISIBLE_HORIZONTAL_PX
        .min(i64::from(state.width) / 2)
        .max(1);
    let min_h = MIN_VISIBLE_VERTICAL_PX
        .min(i64::from(state.height) / 2)
        .max(1);
    if overlap_w < min_w || overlap_h < min_h {
        return None;
    }

    // Ensure the draggable top titlebar edge sits within the monitor's usable vertical range.
    let win_top = i64::from(state.y);
    let mon_top = i64::from(monitor.y);
    let mon_bottom = mon_top + i64::from(monitor.height);
    let max_top = (mon_bottom - TITLEBAR_GRAB_MARGIN_PX).max(mon_top);
    if win_top < mon_top || win_top > max_top {
        return None;
    }

    Some(overlap_w.saturating_mul(overlap_h))
}

fn center_on_monitor(state: WindowState, monitor: MonitorRect) -> WindowState {
    let (width, height) = clamp_size_to_monitor(state.width, state.height, monitor);
    let offset_x = i64::from(monitor.width.saturating_sub(width)) / 2;
    let offset_y = i64::from(monitor.height.saturating_sub(height)) / 2;
    let x = i64::from(monitor.x)
        .saturating_add(offset_x)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let y = i64::from(monitor.y)
        .saturating_add(offset_y)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    WindowState {
        x,
        y,
        width,
        height,
        ..state
    }
}

/// Validate and clamp a loaded `WindowState` against currently attached display monitors.
///
/// - Enforces `width >= 680` and `height >= 560`.
/// - Preserves position when the window's titlebar and sufficient area fall inside any
///   attached monitor (clamping oversized dimensions to that monitor's work area).
/// - Falls back cleanly to a centered placement on the primary monitor whenever saved
///   coordinates are off-screen (e.g. after unplugging an external display).
pub fn resolve_restored_window_state(
    saved: WindowState,
    monitors: &[MonitorRect],
    primary_monitor_index: usize,
) -> ResolvedWindowState {
    let clamped = clamp_min_size(saved);
    if monitors.is_empty() {
        return ResolvedWindowState {
            state: clamped,
            fell_back_to_center: true,
        };
    }

    let mut best_match: Option<(i64, MonitorRect)> = None;
    for &monitor in monitors {
        if let Some(area) = is_visible_on_monitor(clamped, monitor) {
            let replace = match best_match {
                Some((best_area, _)) => area > best_area,
                None => true,
            };
            if replace {
                best_match = Some((area, monitor));
            }
        }
    }

    if let Some((_, monitor)) = best_match {
        let (width, height) = clamp_size_to_monitor(clamped.width, clamped.height, monitor);
        return ResolvedWindowState {
            state: WindowState {
                width,
                height,
                ..clamped
            },
            fell_back_to_center: false,
        };
    }

    let primary = monitors
        .get(primary_monitor_index)
        .copied()
        .unwrap_or(monitors[0]);
    ResolvedWindowState {
        state: center_on_monitor(clamped, primary),
        fell_back_to_center: true,
    }
}

/// Load a persisted `WindowState` from disk, returning `None` gracefully when missing or corrupt.
pub fn load_window_state(path: &Path) -> Option<WindowState> {
    let raw = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "Failed to read desktop window state; using default geometry"
            );
            return None;
        }
    };

    match serde_json::from_str::<WindowState>(&raw) {
        Ok(state) => Some(clamp_min_size(WindowState {
            zoom_percent: zoom::snap(state.zoom_percent),
            ..state
        })),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "Corrupt desktop window state JSON; ignoring and falling back to defaults"
            );
            None
        }
    }
}

/// Atomically persist `WindowState` to `path` via temp-file + fsync + rename.
pub fn save_window_state(path: &Path, state: &WindowState) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("window state path cannot be empty".to_string());
    }
    if path.file_name().is_none() {
        return Err(format!(
            "window state path `{}` does not specify a file name",
            path.display()
        ));
    }
    if path.is_dir() {
        return Err(format!(
            "window state path `{}` is a directory",
            path.display()
        ));
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "failed to create window state directory `{}`: {e}",
                parent.display()
            )
        })?;
    }

    let clamped = clamp_min_size(*state);
    let payload = serde_json::to_vec_pretty(&clamped)
        .map_err(|e| format!("failed to serialize window state: {e}"))?;

    let seq = NEXT_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!(".desktop-window-state.tmp.{}.{seq}", std::process::id());
    let tmp_path = path.with_file_name(tmp_name);

    let write_res = (|| -> Result<(), String> {
        let mut open_opts = fs::OpenOptions::new();
        open_opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open_opts.mode(0o600);
        }
        let mut file = open_opts.open(&tmp_path).map_err(|e| {
            format!(
                "failed to create temporary window state file `{}`: {e}",
                tmp_path.display()
            )
        })?;
        file.write_all(&payload)
            .map_err(|e| format!("failed to write temporary window state: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("failed to flush temporary window state: {e}"))?;
        drop(file);

        fs::rename(&tmp_path, path).map_err(|e| {
            format!(
                "failed to atomically replace window state `{}`: {e}",
                path.display()
            )
        })?;
        Ok(())
    })();

    if write_res.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    write_res
}

fn sanitize_scale_factor(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn monitor_to_logical_rect(monitor: &Monitor) -> MonitorRect {
    let scale = sanitize_scale_factor(monitor.scale_factor());
    let area = monitor.work_area();
    let pos: LogicalPosition<f64> = area.position.to_logical(scale);
    let size: LogicalSize<f64> = area.size.to_logical(scale);
    MonitorRect {
        x: pos.x.round() as i32,
        y: pos.y.round() as i32,
        width: size.width.round().max(0.0) as u32,
        height: size.height.round().max(0.0) as u32,
    }
}

fn collect_monitors(window: &WebviewWindow) -> (Vec<MonitorRect>, usize) {
    let available = match window.available_monitors() {
        Ok(list) => list,
        Err(err) => {
            tracing::warn!(error = %err, "Failed to query available monitors");
            return (Vec::new(), 0);
        }
    };

    let primary = window.primary_monitor().ok().flatten();
    let mut primary_index = 0usize;
    let mut rects = Vec::with_capacity(available.len());

    for (idx, mon) in available.iter().enumerate() {
        if let Some(ref p) = primary
            && mon.position() == p.position()
            && mon.size() == p.size()
            && mon.name() == p.name()
        {
            primary_index = idx;
        }
        rects.push(monitor_to_logical_rect(mon));
    }

    (rects, primary_index)
}

fn capture_window_state(window: &WebviewWindow) -> Option<WindowState> {
    if window.is_minimized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return None;
    }

    let maximized = window.is_maximized().unwrap_or(false);
    let scale = sanitize_scale_factor(window.scale_factor().unwrap_or(1.0));
    let pos = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    if size.width == 0 || size.height == 0 {
        return None;
    }

    let logical_pos: LogicalPosition<f64> = pos.to_logical(scale);
    let logical_size: LogicalSize<f64> = size.to_logical(scale);
    let width = (logical_size.width.round().max(0.0) as u32).max(MIN_WINDOW_WIDTH);
    let height = (logical_size.height.round().max(0.0) as u32).max(MIN_WINDOW_HEIGHT);

    Some(WindowState {
        x: logical_pos.x.round() as i32,
        y: logical_pos.y.round() as i32,
        width,
        height,
        maximized,
        zoom_percent: ZOOM_DEFAULT_PERCENT,
    })
}

struct TrackerInner {
    // A zoom change can precede the first usable (non-minimized) geometry capture.
    zoom_percent: u16,
    latest: Option<WindowState>,
    last_saved: Option<WindowState>,
    dirty: bool,
    deadline: Option<Instant>,
    worker_active: bool,
}

struct WindowStateTracker {
    path: PathBuf,
    inner: Mutex<TrackerInner>,
    write_lock: Mutex<()>,
}

impl WindowStateTracker {
    fn new(
        path: PathBuf,
        initial: Option<WindowState>,
        last_saved: Option<WindowState>,
    ) -> Arc<Self> {
        Arc::new(Self {
            path,
            inner: Mutex::new(TrackerInner {
                zoom_percent: initial.map_or(ZOOM_DEFAULT_PERCENT, |s| s.zoom_percent),
                latest: initial,
                last_saved,
                dirty: false,
                deadline: None,
                worker_active: false,
            }),
            write_lock: Mutex::new(()),
        })
    }

    fn record_geometry(&self, captured: WindowState) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.latest = Some(merge_captured_window_state(
            guard.latest,
            WindowState {
                zoom_percent: guard.zoom_percent,
                ..captured
            },
        ));
    }

    fn set_zoom_percent(self: &Arc<Self>, percent: u16) {
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.zoom_percent = percent;
            if let Some(state) = &mut guard.latest {
                state.zoom_percent = percent;
            }
        }
        self.schedule_debounced_save();
    }

    fn schedule_debounced_save(self: &Arc<Self>) {
        let should_spawn_worker = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.dirty = true;
            guard.deadline = Some(Instant::now() + SAVE_DEBOUNCE);
            if guard.worker_active {
                false
            } else {
                guard.worker_active = true;
                true
            }
        };

        if should_spawn_worker {
            let tracker = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                tracker.run_debounce_worker().await;
            });
        }
    }

    async fn run_debounce_worker(self: Arc<Self>) {
        loop {
            let sleep_duration = {
                let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                if !guard.dirty {
                    guard.worker_active = false;
                    return;
                }
                let now = Instant::now();
                match guard.deadline {
                    Some(dl) if dl > now => Some(dl.saturating_duration_since(now)),
                    _ => {
                        guard.dirty = false;
                        guard.deadline = None;
                        None
                    }
                }
            };

            if let Some(duration) = sleep_duration {
                tokio::time::sleep(duration).await;
            } else {
                self.persist_latest_async().await;
            }
        }
    }

    fn flush_non_blocking(self: &Arc<Self>) {
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.dirty = false;
            guard.deadline = None;
        }
        let tracker = Arc::clone(self);
        tauri::async_runtime::spawn(async move { tracker.persist_latest_async().await });
    }

    async fn persist_latest_async(self: &Arc<Self>) {
        let tracker = Arc::clone(self);
        let res = tauri::async_runtime::spawn_blocking(move || tracker.persist_latest()).await;
        match res {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!(error = %err, "Failed to persist desktop window state"),
            Err(err) => tracing::warn!(error = %err, "Desktop window persistence task failed"),
        }
    }

    fn persist_latest(&self) -> Result<(), String> {
        // Debounce and close can race. Select the snapshot after taking the I/O lock
        // so an older geometry write cannot finish after a newer zoom write.
        let _writer = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot = {
            let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.latest.filter(|s| Some(*s) != guard.last_saved)
        };
        if let Some(state) = snapshot {
            save_window_state(&self.path, &state)?;
            self.inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .last_saved = Some(state);
        }
        Ok(())
    }
}

/// Restore persisted geometry on the `"main"` window and attach debounced non-blocking
/// move/resize & close-request persistence listeners.
pub fn setup_main_window_state(app: &App) -> u16 {
    let Some(window) = app.get_webview_window("main") else {
        tracing::warn!("Main webview window not found; skipping window state setup");
        return ZOOM_DEFAULT_PERCENT;
    };
    let Some(path) = default_window_state_path() else {
        tracing::warn!("Could not resolve ORBIT_HOME; skipping window geometry persistence");
        zoom::set_window_zoom(&window, ZOOM_DEFAULT_PERCENT);
        return ZOOM_DEFAULT_PERCENT;
    };

    let saved = load_window_state(&path);
    let percent = saved.map_or(ZOOM_DEFAULT_PERCENT, |s| s.zoom_percent);
    zoom::set_window_zoom(&window, percent);

    let (initial_state, last_saved) = if let Some(saved) = saved {
        let (monitors, primary_idx) = collect_monitors(&window);
        let resolved = resolve_restored_window_state(saved, &monitors, primary_idx);

        let _ = window.set_size(LogicalSize::new(
            f64::from(resolved.state.width),
            f64::from(resolved.state.height),
        ));
        if monitors.is_empty() && resolved.fell_back_to_center {
            let _ = window.center();
        } else {
            let _ = window.set_position(LogicalPosition::new(
                f64::from(resolved.state.x),
                f64::from(resolved.state.y),
            ));
        }
        if resolved.state.maximized {
            let _ = window.maximize();
        }

        let persisted = if resolved.fell_back_to_center {
            None
        } else {
            Some(resolved.state)
        };
        (Some(resolved.state), persisted)
    } else {
        (capture_window_state(&window), None)
    };

    let tracker = WindowStateTracker::new(path, initial_state, last_saved);
    app.manage(Arc::clone(&tracker));
    let win_for_events = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            if let Some(captured) = capture_window_state(&win_for_events) {
                tracker.record_geometry(captured);
                tracker.schedule_debounced_save();
            }
        }
        WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => {
            if let Some(captured) = capture_window_state(&win_for_events) {
                tracker.record_geometry(captured);
            }
            tracker.flush_non_blocking();
        }
        _ => {}
    });
    percent
}

pub fn zoom_persistence(app: &App) -> Option<PersistZoom> {
    let tracker = Arc::clone(app.try_state::<Arc<WindowStateTracker>>()?.inner());
    Some(Arc::new(move |p| tracker.set_zoom_percent(p)))
}

/// A Quit can end the event loop while a debounce timer or close flush is pending.
pub fn flush_on_exit(app: &AppHandle) {
    if let Some(tracker) = app.try_state::<Arc<WindowStateTracker>>()
        && let Err(error) = tracker.persist_latest()
    {
        tracing::warn!(%error, "Failed to flush desktop window state on exit");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_dir(tag: &str) -> PathBuf {
        let seq = NEXT_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "orbit-window-state-test-{tag}-{}-{seq}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn json_roundtrip_and_atomic_file_persistence() {
        let dir = unique_test_dir("roundtrip");
        let path = dir.join(WINDOW_STATE_FILENAME);

        let original = WindowState {
            x: 180,
            y: 96,
            width: 1420,
            height: 880,
            maximized: false,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        save_window_state(&path, &original).expect("should write window state atomically");
        let loaded = load_window_state(&path).expect("should load persisted window state");
        assert_eq!(loaded, original);

        let maximized = WindowState {
            x: -1200,
            y: 44,
            width: 1600,
            height: 940,
            maximized: true,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        save_window_state(&path, &maximized).expect("should atomically replace window state");
        let loaded_max = load_window_state(&path).expect("should load updated maximized state");
        assert_eq!(loaded_max, maximized);

        // Verify no leftover temporary files exist alongside the target file.
        let entries: Vec<String> = fs::read_dir(&dir)
            .expect("test dir should be readable")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries, vec![WINDOW_STATE_FILENAME.to_string()]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn loads_legacy_and_noncanonical_zoom() {
        let dir = unique_test_dir("zoom-load");
        let path = dir.join(WINDOW_STATE_FILENAME);
        for (zoom, expected) in [(None, 100), (Some(135), 140), (Some(999), 150)] {
            let mut json = serde_json::json!({ "x": 10, "y": 20, "width": 1000, "height": 700 });
            if let Some(p) = zoom {
                json["zoom_percent"] = p.into();
            }
            fs::write(&path, json.to_string()).expect("write window state fixture");
            assert_eq!(
                load_window_state(&path)
                    .expect("load window state")
                    .zoom_percent,
                expected
            );
        }
        fs::remove_dir_all(dir).expect("remove window state fixture");
    }

    #[test]
    fn geometry_after_zoom_persists_new_frame_and_preserves_zoom() {
        let dir = unique_test_dir("zoom-geometry");
        let path = dir.join(WINDOW_STATE_FILENAME);
        let initial = WindowState {
            x: 0,
            y: 55,
            width: 1280,
            height: 840,
            maximized: false,
            zoom_percent: 100,
        };
        let tracker = WindowStateTracker::new(path.clone(), Some(initial), Some(initial));
        let captured = WindowState {
            x: 70,
            height: 900,
            ..initial
        };
        tracker.set_zoom_percent(130);
        tracker.record_geometry(captured);
        tracker.schedule_debounced_save();

        let expected = WindowState {
            zoom_percent: 130,
            ..captured
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while load_window_state(&path) != Some(expected) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(load_window_state(&path), Some(expected));

        // A maximized close capture also carries the native frame's default zoom.
        tracker.record_geometry(WindowState {
            x: 0,
            width: 1920,
            maximized: true,
            ..captured
        });
        tracker.persist_latest().expect("flush final frame");
        assert_eq!(
            load_window_state(&path),
            Some(WindowState {
                maximized: true,
                ..expected
            })
        );
        fs::remove_dir_all(dir).expect("remove saved window state");
    }

    #[test]
    fn zoom_can_precede_the_first_geometry_capture() {
        let dir = unique_test_dir("first-geometry");
        let path = dir.join(WINDOW_STATE_FILENAME);
        let tracker = WindowStateTracker::new(path.clone(), None, None);
        tracker.set_zoom_percent(120);
        tracker.record_geometry(WindowState {
            x: 0,
            y: 0,
            width: 1000,
            height: 700,
            maximized: false,
            zoom_percent: 100,
        });
        tracker.persist_latest().expect("persist first frame");
        assert_eq!(
            load_window_state(&path)
                .expect("load first frame")
                .zoom_percent,
            120
        );
        fs::remove_dir_all(dir).expect("remove saved window state");
    }

    #[test]
    fn corrupt_or_missing_file_falls_back_gracefully() {
        let dir = unique_test_dir("corrupt");

        let missing_path = dir.join("missing.json");
        assert_eq!(load_window_state(&missing_path), None);

        let truncated_path = dir.join("truncated.json");
        fs::write(&truncated_path, r#"{"x": 100, "y": 50, "width":"#)
            .expect("should write truncated json fixture");
        assert_eq!(load_window_state(&truncated_path), None);

        let wrong_shape_path = dir.join("wrong-shape.json");
        fs::write(&wrong_shape_path, r#"{"x": "not-a-number", "y": 0}"#)
            .expect("should write wrong-shape json fixture");
        assert_eq!(load_window_state(&wrong_shape_path), None);

        let empty_path = dir.join("empty.json");
        fs::write(&empty_path, "").expect("should write empty file fixture");
        assert_eq!(load_window_state(&empty_path), None);

        // Passing a directory path must not panic or succeed.
        assert_eq!(load_window_state(&dir), None);
        assert!(
            save_window_state(
                &dir,
                &WindowState {
                    x: 0,
                    y: 0,
                    width: 1000,
                    height: 700,
                    maximized: false,
                    zoom_percent: ZOOM_DEFAULT_PERCENT,
                },
            )
            .is_err()
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clamps_minimum_window_dimensions() {
        let tiny = WindowState {
            x: 100,
            y: 120,
            width: 320,
            height: 240,
            maximized: false,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        let clamped = clamp_min_size(tiny);
        assert_eq!(clamped.width, MIN_WINDOW_WIDTH);
        assert_eq!(clamped.height, MIN_WINDOW_HEIGHT);
        assert_eq!(clamped.x, 100);
        assert_eq!(clamped.y, 120);

        let dir = unique_test_dir("clamp-file");
        let path = dir.join(WINDOW_STATE_FILENAME);
        fs::write(
            &path,
            r#"{"x":40,"y":50,"width":0,"height":10,"maximized":false}"#,
        )
        .expect("should write undersized fixture");
        let loaded = load_window_state(&path).expect("undersized json should load and clamp");
        assert_eq!(loaded.width, MIN_WINDOW_WIDTH);
        assert_eq!(loaded.height, MIN_WINDOW_HEIGHT);

        // Oversized saved window also clamps cleanly to fit within the monitor while >= min size.
        let laptop = [MonitorRect {
            x: 0,
            y: 25,
            width: 1440,
            height: 875,
        }];
        let huge = WindowState {
            x: 20,
            y: 40,
            width: 2400,
            height: 1400,
            maximized: false,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        let resolved = resolve_restored_window_state(huge, &laptop, 0);
        assert!(!resolved.fell_back_to_center);
        assert_eq!(resolved.state.width, 1440);
        assert_eq!(resolved.state.height, 875);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validates_multi_monitor_bounds_and_centers_when_external_display_disconnects() {
        let primary = MonitorRect {
            x: 0,
            y: 25,
            width: 1440,
            height: 875,
        };
        let right_external = MonitorRect {
            x: 1440,
            y: 0,
            width: 2560,
            height: 1440,
        };
        let left_external = MonitorRect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        };

        // 1. Window on attached right external display stays right where the user left it.
        let on_right_ext = WindowState {
            x: 1620,
            y: 120,
            width: 1300,
            height: 820,
            maximized: false,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        let dual_monitors = [primary, right_external];
        let kept = resolve_restored_window_state(on_right_ext, &dual_monitors, 0);
        assert!(!kept.fell_back_to_center);
        assert_eq!(kept.state, on_right_ext);

        // 2. After unplugging the right external display, window is centered on the primary display.
        let single_monitor = [primary];
        let recovered = resolve_restored_window_state(on_right_ext, &single_monitor, 0);
        assert!(recovered.fell_back_to_center);
        assert_eq!(recovered.state.width, 1300);
        assert_eq!(recovered.state.height, 820);
        assert_eq!(recovered.state.x, 70); // (1440 - 1300) / 2
        assert_eq!(recovered.state.y, 52); // 25 + (875 - 820) / 2

        // 3. Window on negative-coordinate left external display stays when attached, centers when detached.
        let on_left_ext = WindowState {
            x: -1500,
            y: 80,
            width: 1200,
            height: 760,
            maximized: true,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        let left_attached = [left_external, primary];
        let kept_left = resolve_restored_window_state(on_left_ext, &left_attached, 1);
        assert!(!kept_left.fell_back_to_center);
        assert_eq!(kept_left.state, on_left_ext);

        let left_detached = resolve_restored_window_state(on_left_ext, &single_monitor, 0);
        assert!(left_detached.fell_back_to_center);
        assert_eq!(left_detached.state.x, 120); // (1440 - 1200) / 2
        assert_eq!(left_detached.state.y, 82); // 25 + (875 - 760) / 2
        assert!(left_detached.state.maximized);

        // 4. Barely peeking or titlebar-above-screen window falls back to centered primary monitor.
        let off_top = WindowState {
            x: 200,
            y: -120,
            width: 1000,
            height: 700,
            maximized: false,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        assert!(resolve_restored_window_state(off_top, &single_monitor, 0).fell_back_to_center);

        let barely_peeking_right = WindowState {
            x: 1410,
            y: 100,
            width: 1000,
            height: 700,
            maximized: false,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        assert!(
            resolve_restored_window_state(barely_peeking_right, &single_monitor, 0)
                .fell_back_to_center
        );

        // 5. Empty monitor list clamps minimum size and requests OS centering.
        let no_monitors = resolve_restored_window_state(
            WindowState {
                x: 9999,
                y: 9999,
                width: 400,
                height: 300,
                maximized: false,
                zoom_percent: ZOOM_DEFAULT_PERCENT,
            },
            &[],
            0,
        );
        assert!(no_monitors.fell_back_to_center);
        assert_eq!(no_monitors.state.width, MIN_WINDOW_WIDTH);
        assert_eq!(no_monitors.state.height, MIN_WINDOW_HEIGHT);
    }

    #[test]
    fn maximizing_preserves_previous_normal_frame() {
        let normal = WindowState {
            x: 120,
            y: 80,
            width: 1280,
            height: 800,
            maximized: false,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        let max_event = WindowState {
            x: 0,
            y: 25,
            width: 1920,
            height: 1055,
            maximized: true,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        let merged = merge_captured_window_state(Some(normal), max_event);
        assert_eq!(
            merged,
            WindowState {
                x: 120,
                y: 80,
                width: 1280,
                height: 800,
                maximized: true,
                zoom_percent: ZOOM_DEFAULT_PERCENT,
            }
        );

        let unmax_event = WindowState {
            x: 160,
            y: 90,
            width: 1100,
            height: 720,
            maximized: false,
            zoom_percent: ZOOM_DEFAULT_PERCENT,
        };
        assert_eq!(
            merge_captured_window_state(Some(merged), unmax_event),
            unmax_event
        );
    }
}
