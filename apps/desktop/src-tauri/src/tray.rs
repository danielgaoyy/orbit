//! Native menu bar / system tray integration for Orbit Desktop.

use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, Wry};

pub const TRAY_ID: &str = "orbit-main-tray";
pub const TRAY_NEW_CONVERSATION_EVENT: &str = "orbit://tray-new-conversation";

#[cfg(target_os = "macos")]
const MACOS_TRAY_TEMPLATE_ICON: tauri::image::Image<'static> =
    tauri::include_image!("./icons/tray-template@2x.png");

const MENU_ID_STATUS: &str = "tray_status";
const MENU_ID_SHOW: &str = "tray_show";
const MENU_ID_NEW_CONVERSATION: &str = "tray_new_conversation";
const MENU_ID_QUIT: &str = "tray_quit";

#[derive(Default)]
pub struct TrayMenuState {
    status_item: Mutex<Option<MenuItem<Wry>>>,
}

/// Format a concise, restrained status summary for the native system tray label & tooltip.
pub fn format_tray_status_label(running_count: u32, pending_approvals: u32) -> String {
    match (running_count, pending_approvals) {
        (0, 0) => "Orbit — Idle".to_string(),
        (0, 1) => "Orbit — 1 approval needed".to_string(),
        (0, p) => format!("Orbit — {p} approvals needed"),
        (1, 0) => "Orbit — 1 agent running".to_string(),
        (r, 0) => format!("Orbit — {r} agents running"),
        (r, 1) => format!("Orbit — {r} running · 1 approval"),
        (r, p) => format!("Orbit — {r} running · {p} approvals"),
    }
}

/// Unminimize, show, and focus the primary Orbit Desktop window.
pub fn focus_main_window_impl(app: &AppHandle) -> Result<(), String> {
    let win = app
        .get_webview_window("main")
        .or_else(|| app.webview_windows().into_values().next())
        .ok_or_else(|| "main window not found".to_string())?;
    let _ = win.unminimize();
    let _ = win.show();
    win.set_focus()
        .map_err(|e| format!("failed to focus window: {e}"))?;
    Ok(())
}

/// Initialize the native menu bar / system tray icon without ever panicking if the host
/// environment lacks a tray daemon or bundled icon.
pub fn setup_system_tray(app: &App) {
    let initial_label = format_tray_status_label(0, 0);

    let Ok(status_item) =
        MenuItem::with_id(app, MENU_ID_STATUS, &initial_label, false, None::<&str>)
    else {
        tracing::warn!("Failed to create Orbit tray status item; skipping system tray");
        return;
    };
    let Ok(sep_top) = PredefinedMenuItem::separator(app) else {
        return;
    };
    let Ok(show_item) = MenuItem::with_id(app, MENU_ID_SHOW, "Show Orbit", true, None::<&str>)
    else {
        return;
    };
    let Ok(new_conv_item) = MenuItem::with_id(
        app,
        MENU_ID_NEW_CONVERSATION,
        "New Conversation",
        true,
        None::<&str>,
    ) else {
        return;
    };
    let Ok(sep_bottom) = PredefinedMenuItem::separator(app) else {
        return;
    };
    let Ok(quit_item) = MenuItem::with_id(app, MENU_ID_QUIT, "Quit", true, None::<&str>) else {
        return;
    };

    let Ok(menu) = Menu::with_items(
        app,
        &[
            &status_item,
            &sep_top,
            &show_item,
            &new_conv_item,
            &sep_bottom,
            &quit_item,
        ],
    ) else {
        tracing::warn!("Failed to build Orbit tray menu; skipping system tray");
        return;
    };

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(&initial_label)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_ID_SHOW => {
                let _ = focus_main_window_impl(app);
            }
            MENU_ID_NEW_CONVERSATION => {
                let _ = focus_main_window_impl(app);
                let _ = app.emit(TRAY_NEW_CONVERSATION_EVENT, ());
            }
            MENU_ID_QUIT => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = focus_main_window_impl(tray.app_handle());
            }
        });

    #[cfg(target_os = "macos")]
    {
        // Monochrome template glyph (black + alpha, 36px tall = 18pt @2x); AppKit tints it
        // for light/dark menu bars and the highlighted state. Source: docs/assets/brand/.
        builder = builder
            .icon(MACOS_TRAY_TEMPLATE_ICON)
            .icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    } else {
        tracing::warn!("Default window icon unavailable; skipping system tray icon");
        return;
    }

    match builder.build(app) {
        Ok(_tray) => {
            if let Some(tray_state) = app.try_state::<TrayMenuState>()
                && let Ok(mut guard) = tray_state.status_item.lock()
            {
                *guard = Some(status_item);
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "Failed to initialize native system tray; continuing without tray");
        }
    }
}

/// Update the live system tray tooltip and status menu item cleanly.
pub fn update_tray_status_impl(
    app: &AppHandle,
    tray_state: &TrayMenuState,
    running_count: u32,
    pending_approvals: u32,
) -> Result<(), String> {
    let label = format_tray_status_label(running_count, pending_approvals);

    if let Ok(guard) = tray_state.status_item.lock()
        && let Some(item) = guard.as_ref()
    {
        let _ = item.set_text(&label);
    }

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(&label));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_restrained_tray_status_labels() {
        assert_eq!(format_tray_status_label(0, 0), "Orbit — Idle");
        assert_eq!(format_tray_status_label(1, 0), "Orbit — 1 agent running");
        assert_eq!(format_tray_status_label(3, 0), "Orbit — 3 agents running");
        assert_eq!(format_tray_status_label(0, 1), "Orbit — 1 approval needed");
        assert_eq!(format_tray_status_label(0, 2), "Orbit — 2 approvals needed");
        assert_eq!(
            format_tray_status_label(2, 1),
            "Orbit — 2 running · 1 approval"
        );
        assert_eq!(
            format_tray_status_label(2, 4),
            "Orbit — 2 running · 4 approvals"
        );
    }
}
