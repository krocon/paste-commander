//! Global shortcuts: picker + quick paste.
//!
//! macOS, Windows, X11: `tauri-plugin-global-shortcut`.
//! Wayland: that plugin is X11-only, so we use the XDG `GlobalShortcuts`
//! portal (GNOME 48+, KDE, …) via ashpd. The portal lets the user confirm or
//! change the binding; our accelerator is only the *preferred* trigger.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use tauri_plugin_notification::NotificationExt;

use crate::i18n;
use crate::paste::{self, CopyReason, PasteOutcome};
use crate::platform::DisplayServer;
use crate::settings::{QUICK_PASTE_SLOTS, Settings};
use crate::state::AppState;
use crate::windows;

/// Emitted with `Vec<ShortcutStatus>` whenever registrations change.
pub const SHORTCUTS_CHANGED: &str = "shortcuts-changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Action {
    Picker,
    QuickPaste { position: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub action: Action,
    pub accelerator: String,
    /// Why registration failed (taken by another app, invalid, no portal, …).
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Hotkeys {
    /// Plugin shortcut id → action.
    bindings: Mutex<HashMap<u32, Action>>,
    status: Mutex<Vec<ShortcutStatus>>,
    #[cfg(target_os = "linux")]
    portal_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl Hotkeys {
    pub fn status(&self) -> Vec<ShortcutStatus> {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn set_status(&self, status: Vec<ShortcutStatus>) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = status;
    }
}

/// Desired bindings from the settings.
pub fn desired(settings: &Settings) -> Vec<(Action, String)> {
    let mut out = Vec::new();
    if let Some(picker) = &settings.shortcuts.picker {
        out.push((Action::Picker, picker.clone()));
    }
    for (i, slot) in settings
        .shortcuts
        .quick_paste
        .iter()
        .enumerate()
        .take(QUICK_PASTE_SLOTS)
    {
        if let Some(acc) = slot {
            out.push((Action::QuickPaste { position: i + 2 }, acc.clone()));
        }
    }
    out
}

/// Handler installed on the plugin builder; looks the shortcut up by id.
pub fn plugin_handler<R: Runtime>(app: &AppHandle<R>, shortcut: &Shortcut, event: ShortcutEvent) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let action = state
        .hotkeys
        .bindings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&shortcut.id())
        .copied();
    if let Some(action) = action {
        dispatch(app, action, event.state == ShortcutState::Pressed);
    }
}

/// Picker reacts on press (snappy); quick paste on release, so the user's
/// modifiers are up before we synthesize ⌘V / Ctrl+V.
fn dispatch<R: Runtime>(app: &AppHandle<R>, action: Action, pressed: bool) {
    match (action, pressed) {
        (Action::Picker, true) => {
            if let Err(e) = windows::toggle_picker(app) {
                log::error!("toggle picker failed: {e}");
            }
        }
        (Action::QuickPaste { position }, false) => match paste::quick_paste(app, position) {
            Ok(PasteOutcome::Copied { reason }) => notify_copied(app, reason),
            Ok(_) => {}
            Err(e) => log::error!("quick paste {position} failed: {e}"),
        },
        _ => {}
    }
}

/// "Copied – press ⌘V" when we could not paste ourselves.
pub fn notify_copied<R: Runtime>(app: &AppHandle<R>, reason: CopyReason) {
    let s = i18n::strings(app.state::<AppState>().lang());
    let body = match reason {
        CopyReason::NoAccessibility => s.copied_no_accessibility,
        CopyReason::Unsupported => s.copied_press_paste,
    };
    if let Err(e) = app
        .notification()
        .builder()
        .title("Paste Commander")
        .body(body)
        .show()
    {
        log::warn!("notification failed: {e}");
    }
}

/// (Re-)registers all shortcuts from the settings.
pub fn apply<R: Runtime>(app: &AppHandle<R>, settings: &Settings) {
    let state = app.state::<AppState>();
    let wanted = desired(settings);
    let display = state.platform().display_server;

    #[cfg(target_os = "linux")]
    if display == DisplayServer::Wayland {
        portal::apply(app, wanted);
        return;
    }
    let _ = display;

    let gs = app.global_shortcut();
    if let Err(e) = gs.unregister_all() {
        log::warn!("unregister shortcuts: {e}");
    }
    let mut bindings = HashMap::new();
    let mut status = Vec::new();
    for (action, accelerator) in wanted {
        let error = match accelerator.parse::<Shortcut>() {
            Err(e) => Some(format!("invalid shortcut: {e}")),
            Ok(shortcut) => match bindings.entry(shortcut.id()) {
                Entry::Occupied(_) => Some("assigned twice".to_owned()),
                Entry::Vacant(slot) => match gs.register(shortcut) {
                    Ok(()) => {
                        slot.insert(action);
                        None
                    }
                    Err(e) => Some(e.to_string()),
                },
            },
        };
        if let Some(e) = &error {
            log::warn!("shortcut {accelerator} for {action:?}: {e}");
        }
        status.push(ShortcutStatus {
            action,
            accelerator,
            error,
        });
    }
    *state
        .hotkeys
        .bindings
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = bindings;
    state.hotkeys.set_status(status.clone());
    let _ = app.emit(SHORTCUTS_CHANGED, status);
}

/// While the settings window records a shortcut, ours must not fire.
pub fn suspend<R: Runtime>(app: &AppHandle<R>, suspended: bool) {
    let state = app.state::<AppState>();
    if state.platform().display_server == DisplayServer::Wayland {
        return; // portal shortcuts are owned by the desktop
    }
    if suspended {
        let _ = app.global_shortcut().unregister_all();
        state
            .hotkeys
            .bindings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    } else {
        apply(app, &state.settings());
    }
}

#[cfg(target_os = "linux")]
mod portal {
    use super::*;
    use ashpd::desktop::CreateSessionOptions;
    use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    /// "Ctrl+Alt+Shift+V" → "CTRL+ALT+SHIFT+v" (XDG shortcuts spec syntax).
    pub(super) fn to_portal_trigger(accelerator: &str) -> String {
        let mut parts: Vec<String> = accelerator.split('+').map(str::to_owned).collect();
        let key = parts.pop().unwrap_or_default();
        let mut out: Vec<String> = parts
            .iter()
            .map(|m| match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => "CTRL".into(),
                "alt" | "option" => "ALT".into(),
                "shift" => "SHIFT".into(),
                "super" | "cmd" | "command" | "meta" => "LOGO".into(),
                other => other.to_ascii_uppercase(),
            })
            .collect();
        out.push(if key.len() == 1 {
            key.to_ascii_lowercase()
        } else {
            key
        });
        out.join("+")
    }

    fn id_of(action: Action) -> String {
        match action {
            Action::Picker => "picker".into(),
            Action::QuickPaste { position } => format!("quick-paste-{position}"),
        }
    }

    fn action_of(id: &str) -> Option<Action> {
        if id == "picker" {
            return Some(Action::Picker);
        }
        id.strip_prefix("quick-paste-")?
            .parse()
            .ok()
            .map(|position| Action::QuickPaste { position })
    }

    pub(super) fn apply<R: Runtime>(app: &AppHandle<R>, wanted: Vec<(Action, String)>) {
        let state = app.state::<AppState>();
        let mut task = state
            .hotkeys
            .portal_task
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(old) = task.take() {
            old.abort(); // drops the old session
        }
        let app = app.clone();
        *task = Some(tauri::async_runtime::spawn(async move {
            let result = run(&app, &wanted).await;
            if let Err(e) = result {
                log::warn!("GlobalShortcuts portal unavailable: {e}");
                let status = wanted
                    .into_iter()
                    .map(|(action, accelerator)| ShortcutStatus {
                        action,
                        accelerator,
                        error: Some(format!("XDG GlobalShortcuts portal: {e}")),
                    })
                    .collect::<Vec<_>>();
                let state = app.state::<AppState>();
                state.hotkeys.set_status(status.clone());
                let _ = app.emit(SHORTCUTS_CHANGED, status);
            }
        }));
    }

    async fn run<R: Runtime>(app: &AppHandle<R>, wanted: &[(Action, String)]) -> ashpd::Result<()> {
        let s = i18n::strings(app.state::<AppState>().lang());
        let portal = GlobalShortcuts::new().await?;
        let session = portal
            .create_session(CreateSessionOptions::default())
            .await?;
        let triggers: Vec<String> = wanted
            .iter()
            .map(|(_, acc)| to_portal_trigger(acc))
            .collect();
        let shortcuts: Vec<NewShortcut> = wanted
            .iter()
            .zip(&triggers)
            .map(|((action, _), trigger)| {
                let description = match action {
                    Action::Picker => s.open_history.to_owned(),
                    Action::QuickPaste { position } => {
                        format!("{} {position}", s.quick_paste_entry)
                    }
                };
                NewShortcut::new(id_of(*action), description)
                    .preferred_trigger(Some(trigger.as_str()))
            })
            .collect();
        portal
            .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
            .await?
            .response()?;

        let status: Vec<ShortcutStatus> = wanted
            .iter()
            .map(|(action, accelerator)| ShortcutStatus {
                action: *action,
                accelerator: accelerator.clone(),
                error: None,
            })
            .collect();
        app.state::<AppState>().hotkeys.set_status(status.clone());
        let _ = app.emit(SHORTCUTS_CHANGED, status);

        let mut activated = portal.receive_activated().await?;
        let mut deactivated = portal.receive_deactivated().await?;
        loop {
            tokio::select! {
                Some(ev) = activated.next() => {
                    if let Some(a) = action_of(ev.shortcut_id()) { dispatch(app, a, true); }
                }
                Some(ev) = deactivated.next() => {
                    if let Some(a) = action_of(ev.shortcut_id()) { dispatch(app, a, false); }
                }
                else => break,
            }
        }
        drop(session);
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn portal_trigger_syntax() {
            assert_eq!(to_portal_trigger("Ctrl+Alt+V"), "CTRL+ALT+v");
            assert_eq!(to_portal_trigger("Ctrl+Alt+Shift+2"), "CTRL+ALT+SHIFT+2");
            assert_eq!(to_portal_trigger("Super+F9"), "LOGO+F9");
        }

        #[test]
        fn ids_round_trip() {
            for a in [Action::Picker, Action::QuickPaste { position: 7 }] {
                assert_eq!(action_of(&id_of(a)), Some(a));
            }
            assert_eq!(action_of("nope"), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_maps_slots_to_positions() {
        let s = Settings::default();
        let d = desired(&s);
        assert_eq!(d[0].0, Action::Picker);
        assert_eq!(d[1].0, Action::QuickPaste { position: 2 });
        assert_eq!(d[2].0, Action::QuickPaste { position: 3 });
        assert_eq!(d.len(), 3);
    }

    #[test]
    fn default_accelerators_parse() {
        for (_, acc) in desired(&Settings::default()) {
            assert!(acc.parse::<Shortcut>().is_ok(), "{acc}");
        }
    }
}
