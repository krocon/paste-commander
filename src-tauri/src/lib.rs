//! Paste Commander – clipboard history for macOS, Windows and Linux.
//!
//! All logic lives in Rust; the Angular webview only renders and calls commands.
//! Planned modules (later phases): `watcher`, `privacy`, `history`, `paste`, `hotkeys`.

mod commands;
mod error;
mod i18n;
mod platform;
mod settings;
mod state;
mod tray;
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
            let state = AppState::new(settings);
            log::info!("platform: {:?}", state.platform);
            app.manage(state);

            tray::create(handle)?;
            Ok(())
        })
        .on_window_event(windows::on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::update_settings,
            commands::get_platform_info,
            commands::hide_picker,
            commands::open_settings,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Paste Commander");

    app.run(|_app, event| {
        // Background app: closing the last window must not quit. Only an explicit
        // `exit` (tray "Quit") carries an exit code.
        if let RunEvent::ExitRequested {
            api, code: None, ..
        } = event
        {
            api.prevent_exit();
        }
    });
}
