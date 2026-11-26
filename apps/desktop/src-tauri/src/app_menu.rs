//! Native zoom commands in the macOS application menu.

#[cfg(target_os = "macos")]
mod macos {
    use tauri::menu::{
        AboutMetadata, HELP_SUBMENU_ID, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
        WINDOW_SUBMENU_ID,
    };
    use tauri::{AppHandle, Manager, Wry};

    use crate::zoom::{
        self, ZOOM_DEFAULT_PERCENT, ZOOM_MAX_PERCENT, ZOOM_MIN_PERCENT, ZoomSource, ZoomStep,
    };

    const RESET: &str = "orbit.zoom.reset";
    const ZOOM_IN: &str = "orbit.zoom.in";
    const ZOOM_OUT: &str = "orbit.zoom.out";

    pub struct ZoomMenuItems {
        reset: MenuItem<Wry>,
        zoom_in: MenuItem<Wry>,
        zoom_out: MenuItem<Wry>,
    }

    /// Mirrors Tauri 2.12.6's macOS Menu::default, extending only the View submenu.
    pub fn build_app_menu(app: &AppHandle) -> tauri::Result<(Menu<Wry>, ZoomMenuItems)> {
        let pkg_info = app.package_info();
        let config = app.config();
        let about_metadata = AboutMetadata {
            name: Some(pkg_info.name.clone()),
            version: Some(pkg_info.version.to_string()),
            copyright: config.bundle.copyright.clone(),
            authors: config.bundle.publisher.clone().map(|p| vec![p]),
            ..Default::default()
        };

        let items = ZoomMenuItems {
            reset: MenuItem::with_id(app, RESET, "Actual Size", false, Some("CmdOrCtrl+0"))?,
            zoom_in: MenuItem::with_id(app, ZOOM_IN, "Zoom In", true, Some("CmdOrCtrl+="))?,
            zoom_out: MenuItem::with_id(app, ZOOM_OUT, "Zoom Out", true, Some("CmdOrCtrl+-"))?,
        };
        let window_menu = Submenu::with_id_and_items(
            app,
            WINDOW_SUBMENU_ID,
            "Window",
            true,
            &[
                &PredefinedMenuItem::minimize(app, None)?,
                &PredefinedMenuItem::maximize(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::close_window(app, None)?,
            ],
        )?;
        let help_menu = Submenu::with_id_and_items(app, HELP_SUBMENU_ID, "Help", true, &[])?;
        let menu = Menu::with_items(
            app,
            &[
                &Submenu::with_items(
                    app,
                    pkg_info.name.clone(),
                    true,
                    &[
                        &PredefinedMenuItem::about(app, None, Some(about_metadata))?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::services(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::hide(app, None)?,
                        &PredefinedMenuItem::hide_others(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::quit(app, None)?,
                    ],
                )?,
                &Submenu::with_items(
                    app,
                    "File",
                    true,
                    &[&PredefinedMenuItem::close_window(app, None)?],
                )?,
                &Submenu::with_items(
                    app,
                    "Edit",
                    true,
                    &[
                        &PredefinedMenuItem::undo(app, None)?,
                        &PredefinedMenuItem::redo(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::cut(app, None)?,
                        &PredefinedMenuItem::copy(app, None)?,
                        &PredefinedMenuItem::paste(app, None)?,
                        &PredefinedMenuItem::select_all(app, None)?,
                    ],
                )?,
                &Submenu::with_items(
                    app,
                    "View",
                    true,
                    &[
                        &items.reset,
                        &items.zoom_in,
                        &items.zoom_out,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::fullscreen(app, None)?,
                    ],
                )?,
                &window_menu,
                &help_menu,
            ],
        )?;
        Ok((menu, items))
    }

    pub fn sync(items: &ZoomMenuItems, percent: u16) {
        for (item, enabled) in [
            (&items.reset, percent != ZOOM_DEFAULT_PERCENT),
            (&items.zoom_in, percent < ZOOM_MAX_PERCENT),
            (&items.zoom_out, percent > ZOOM_MIN_PERCENT),
        ] {
            if let Err(error) = item.set_enabled(enabled) {
                tracing::warn!(%error, id = %item.id().as_ref(), "Failed to update desktop zoom menu item");
            }
        }
    }

    pub fn on_menu_event(app: &AppHandle, event: MenuEvent) {
        let direction = match event.id.as_ref() {
            RESET => None,
            ZOOM_IN => Some(ZoomStep::In),
            ZOOM_OUT => Some(ZoomStep::Out),
            _ => return,
        };
        // A native accelerator can also fire while an external browser popout has focus.
        if app
            .webview_windows()
            .values()
            .any(|w| !zoom::is_orbit_ui_window(w.label()) && w.is_focused().unwrap_or(false))
        {
            return;
        }
        let next = direction.map_or(ZOOM_DEFAULT_PERCENT, |d| {
            zoom::step(zoom::get_ui_zoom(app.state()).zoom_percent, d)
        });
        zoom::apply(app, next, ZoomSource::Menu);
    }
}

#[cfg(target_os = "macos")]
pub use macos::{ZoomMenuItems, build_app_menu, on_menu_event, sync};

#[cfg(not(target_os = "macos"))]
pub fn on_menu_event(_: &tauri::AppHandle, _: tauri::menu::MenuEvent) {}
