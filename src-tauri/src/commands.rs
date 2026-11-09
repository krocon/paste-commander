//! Tauri commands exposed to the webview. Keep `build.rs` and
//! `capabilities/default.json` in sync when adding one.

use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_autostart::ManagerExt;

use crate::error::{Error, Result};
use crate::history::EntrySummary;
use crate::platform::PlatformInfo;
use crate::settings::Settings;
use crate::state::{AppState, HISTORY_CHANGED};
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

    if new.history_limit != old.history_limit || new.persist_history != old.persist_history {
        state.reconfigure_history(&new)?;
        app.emit(HISTORY_CHANGED, ())?;
    }

    if new.language != old.language {
        tray::refresh(&app)?;
    }
    app.emit(SETTINGS_CHANGED, &new)?;
    Ok(new)
}

#[tauri::command]
pub fn get_platform_info(state: State<'_, AppState>) -> PlatformInfo {
    state.platform()
}

#[tauri::command]
pub fn hide_picker<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    windows::hide_picker(&app)
}

#[tauri::command]
pub fn open_settings<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    windows::open_settings(&app)
}

/// Most recent first. A non-empty `query` filters case-insensitively over the
/// full text (the summaries themselves only carry a preview).
#[tauri::command]
pub fn get_history(state: State<'_, AppState>, query: Option<String>) -> Result<Vec<EntrySummary>> {
    let history = state.history();
    Ok(match query.as_deref() {
        Some(q) if !q.trim().is_empty() => history.search(q)?,
        _ => history.list()?,
    })
}

#[tauri::command]
pub fn delete_entry<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: i64,
) -> Result<bool> {
    let deleted = state.history().delete(id)?;
    if deleted {
        app.emit(HISTORY_CHANGED, ())?;
    }
    Ok(deleted)
}

#[tauri::command]
pub fn set_pinned<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: i64,
    pinned: bool,
) -> Result<bool> {
    let changed = state.history().set_pinned(id, pinned)?;
    if changed {
        app.emit(HISTORY_CHANGED, ())?;
    }
    Ok(changed)
}

/// Removes all unpinned entries (same as the tray action).
#[tauri::command]
pub fn clear_history<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> Result<usize> {
    let removed = state.history().clear(true)?;
    app.emit(HISTORY_CHANGED, ())?;
    Ok(removed)
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
