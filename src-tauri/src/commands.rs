//! Tauri commands exposed to the webview. Keep `build.rs` and
//! `capabilities/default.json` in sync when adding one.

use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_autostart::ManagerExt;

use crate::error::{Error, Result};
use crate::platform::PlatformInfo;
use crate::settings::Settings;
use crate::state::AppState;
use crate::{tray, windows};

pub const SETTINGS_CHANGED: &str = "settings-changed";

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn update_settings<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings> {
    let new = settings.sanitized();
    let old = state.settings();

    if new.launch_at_login != old.launch_at_login {
        apply_autostart(&app, new.launch_at_login)?;
    }

    new.save(&app)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = new.clone();

    if new.language != old.language {
        tray::refresh(&app)?;
    }
    app.emit(SETTINGS_CHANGED, &new)?;
    Ok(new)
}

#[tauri::command]
pub fn get_platform_info(state: State<'_, AppState>) -> PlatformInfo {
    state.platform.clone()
}

#[tauri::command]
pub fn hide_picker<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    windows::hide_picker(&app)
}

#[tauri::command]
pub fn open_settings<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    windows::open_settings(&app)
}

pub fn apply_autostart<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<()> {
    let launcher = app.autolaunch();
    let res = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    res.map_err(|e| Error::Autostart(e.to_string()))
}
