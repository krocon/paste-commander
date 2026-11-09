//! Paste Commander – clipboard history for macOS, Windows and Linux.
//!
//! All logic lives in Rust; the Angular webview only renders and calls commands.

mod clipboard;
mod commands;
mod error;
mod history;
mod hotkeys;
mod i18n;
mod ingest;
mod paste;
mod platform;
mod privacy;
mod settings;
mod state;
mod tray;
mod watcher;
mod windows;

use tauri::{Manager, RunEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use crate::settings::Settings;
use crate::state::AppState;

pub fn run() {
    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch just opens the settings window.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Err(e) = windows::open_settings(app) {
                log::error!("failed to open settings for second instance: {e}");
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(hotkeys::plugin_handler)
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            // No Dock icon / app switcher entry; we live in the menu bar.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle();
            let mut settings = Settings::load(handle)?;
            // The OS is the source of truth for login items (user may have removed it).
            if let Ok(enabled) = handle.autolaunch().is_enabled() {
                settings.launch_at_login = enabled;
            }
            let state = AppState::new(handle, settings, state::history_file(handle)?);
            app.manage(state);
            let state = app.state::<AppState>();
            state.start_watcher(handle);
            hotkeys::apply(handle, &state.settings());

            // First launch: explain what the app needs. On macOS without
            // Accessibility the settings window opens on the platform page.
            if settings::take_first_run(handle)? && !paste::accessibility_trusted() {
                windows::open_settings(handle)?;
            }
            log::info!("platform: {:?}", state.platform());

            tray::create(handle)?;
            windows::configure_picker(handle);
            Ok(())
        })
        .on_window_event(windows::on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::update_settings,
            commands::get_platform_info,
            commands::hide_picker,
            commands::open_settings,
            commands::get_history,
            commands::delete_entry,
            commands::set_pinned,
            commands::clear_history,
            commands::paste_entry,
            commands::open_accessibility_settings,
            commands::get_shortcut_status,
            commands::suspend_shortcuts,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Paste Commander");

    app.run(|app, event| match event {
        // Background app: closing the last window must not quit. Only an explicit
        // `exit` (tray "Quit") carries an exit code.
        RunEvent::ExitRequested {
            api, code: None, ..
        } => api.prevent_exit(),
        RunEvent::Exit => on_exit(app),
        _ => {}
    });
}

/// "Clear history on quit" keeps pinned entries: pins are explicit "keep this".
fn on_exit<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    state.stop_watcher();
    let settings = state.settings();
    if settings.persist_history && settings.clear_history_on_exit {
        match state.history().clear(true) {
            Ok(n) => log::info!("cleared {n} entries on exit"),
            Err(e) => log::error!("clearing history on exit failed: {e}"),
        }
    }
}
