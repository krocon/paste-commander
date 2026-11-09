//! Picker (pre-created, only shown/hidden) and settings (created on demand) windows.

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::error::Result;
use crate::i18n;
use crate::state::AppState;

pub const PICKER: &str = "picker";
pub const SETTINGS: &str = "settings";
/// Sent to the picker each time it is shown: reload list, reset search.
pub const PICKER_SHOWN: &str = "picker-shown";

/// Centers the picker on the monitor under the mouse and focuses it.
pub fn show_picker<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let Some(picker) = app.get_webview_window(PICKER) else {
        log::error!("picker window missing");
        return Ok(());
    };
    if !picker.is_visible()? {
        // Remember who had focus, so pasting can hand it back.
        let state = app.state::<AppState>();
        let display = state.platform().display_server;
        state.set_previous_focus(crate::paste::focus::capture(display));
        center_on_cursor_monitor(app, &picker)?;
    }
    picker.show()?;
    picker.set_focus()?;
    picker.emit(PICKER_SHOWN, ())?;
    Ok(())
}

/// Picker shortcut: opens the picker, or closes it if it is already in front.
pub fn toggle_picker<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    match app.get_webview_window(PICKER) {
        Some(p) if p.is_visible()? && p.is_focused()? => hide_picker(app),
        _ => show_picker(app),
    }
}

pub fn hide_picker<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    if let Some(picker) = app.get_webview_window(PICKER) {
        picker.hide()?;
    }
    Ok(())
}

fn center_on_cursor_monitor<R: Runtime>(app: &AppHandle<R>, win: &WebviewWindow<R>) -> Result<()> {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return Ok(());
    };

    let area = monitor.work_area();
    let size = win.outer_size()?;
    let x = area.position.x + (area.size.width as i32 - size.width as i32) / 2;
    // Slightly above center reads better for a launcher-style window.
    let y = area.position.y + (area.size.height as i32 - size.height as i32) / 3;
    win.set_position(PhysicalPosition::new(x, y))?;
    Ok(())
}

pub fn open_settings<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    if let Some(win) = app.get_webview_window(SETTINGS) {
        win.unminimize()?;
        win.show()?;
        win.set_focus()?;
        return Ok(());
    }

    let title = i18n::strings(app.state::<AppState>().lang()).settings_title;
    let win = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(760.0, 580.0)
        .min_inner_size(600.0, 440.0)
        .center()
        .focused(true)
        .build()?;

    // Accessory apps are not activated when a window opens; bring us to front.
    #[cfg(target_os = "macos")]
    app.show()?;
    win.set_focus()?;
    Ok(())
}

/// Window-event hook registered on the builder.
pub fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    if window.label() == PICKER {
        match event {
            WindowEvent::Focused(false) => {
                let _ = window.hide();
            }
            // The picker lives for the whole session: closing just hides it.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        }
    }
}
