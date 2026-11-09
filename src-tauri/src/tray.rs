use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::error::Result;
use crate::i18n;
use crate::state::AppState;
use crate::windows;

const TRAY_ID: &str = "main";

mod ids {
    pub const OPEN_HISTORY: &str = "open_history";
    pub const CLEAR_HISTORY: &str = "clear_history";
    pub const PAUSE: &str = "pause";
    pub const SETTINGS: &str = "settings";
    pub const QUIT: &str = "quit";
}

/// Event names the webview listens to.
pub mod events {
    pub const PAUSED_CHANGED: &str = "paused-changed";
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> Result<Menu<R>> {
    let state = app.state::<AppState>();
    let s = i18n::strings(state.lang());
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, ids::OPEN_HISTORY, s.open_history, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &CheckMenuItem::with_id(
                app,
                ids::PAUSE,
                s.pause,
                true,
                state.is_paused(),
                None::<&str>,
            )?,
            &MenuItem::with_id(app, ids::CLEAR_HISTORY, s.clear_history, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, ids::SETTINGS, s.settings, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, ids::QUIT, s.quit, true, None::<&str>)?,
        ],
    )?;
    Ok(menu)
}

pub fn create<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        // Monochrome template icon: macOS tints it for light/dark menu bars.
        .icon_as_template(true)
        .tooltip("Paste Commander")
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()))
        .build(app)?;
    Ok(())
}

/// Rebuilds the menu, e.g. after a language change or pause toggle from elsewhere.
pub fn refresh<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_menu(app)?))?;
        let state = app.state::<AppState>();
        let tooltip = if state.is_paused() {
            i18n::strings(state.lang()).tooltip_paused
        } else {
            "Paste Commander"
        };
        tray.set_tooltip(Some(tooltip))?;
    }
    Ok(())
}

/// Tray "Clear History": pinned entries are deliberate and survive.
fn clear_history<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let removed = app.state::<AppState>().history().clear(true)?;
    log::info!("cleared {removed} history entries");
    app.emit(crate::state::HISTORY_CHANGED, ())?;
    Ok(())
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let result = match id {
        ids::OPEN_HISTORY => windows::show_picker(app),
        ids::CLEAR_HISTORY => clear_history(app),
        ids::PAUSE => {
            let state = app.state::<AppState>();
            let paused = !state.is_paused();
            state.set_paused(paused);
            log::info!(
                "clipboard monitoring {}",
                if paused { "paused" } else { "resumed" }
            );
            app.emit(events::PAUSED_CHANGED, paused)
                .map_err(Into::into)
                .and_then(|_| refresh(app))
        }
        ids::SETTINGS => windows::open_settings(app),
        ids::QUIT => {
            app.exit(0);
            Ok(())
        }
        _ => Ok(()),
    };
    if let Err(e) = result {
        log::error!("tray action {id} failed: {e}");
    }
}
