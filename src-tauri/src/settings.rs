//! User settings: defaults, validation and persistence via `tauri-plugin-store`.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

use crate::error::Result;

pub const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

pub const HISTORY_LIMIT_MIN: u32 = 10;
pub const HISTORY_LIMIT_MAX: u32 = 500;
pub const HISTORY_LIMIT_DEFAULT: u32 = 50;

/// Quick-paste slots cover history positions 2..=9 (position 1 is what's already
/// on the clipboard, so a shortcut for it would be pointless).
pub const QUICK_PASTE_SLOTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    System,
    De,
    En,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Shortcuts {
    /// Opens the picker window.
    pub picker: Option<String>,
    /// Index 0 pastes history entry 2, index 7 pastes entry 9. `None` = unassigned.
    pub quick_paste: Vec<Option<String>>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        // Strings use the accelerator syntax of `tauri-plugin-global-shortcut`.
        // Windows/Linux avoid Ctrl+Alt+<digit>: Windows reports AltGr as Ctrl+Alt,
        // and AltGr+2/3 type ²/³ on German layouts. Ctrl+Shift+V is "paste" in
        // most Linux terminals, so the picker uses Ctrl+Alt+V there.
        let (picker, quick_prefix) = if cfg!(target_os = "macos") {
            ("Cmd+Shift+V", "Ctrl+Alt")
        } else {
            ("Ctrl+Alt+V", "Ctrl+Alt+Shift")
        };
        let mut quick_paste = vec![None; QUICK_PASTE_SLOTS];
        quick_paste[0] = Some(format!("{quick_prefix}+2"));
        quick_paste[1] = Some(format!("{quick_prefix}+3"));
        Self {
            picker: Some(picker.to_owned()),
            quick_paste,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: Language,
    pub history_limit: u32,
    pub persist_history: bool,
    pub clear_history_on_exit: bool,
    pub restore_clipboard: bool,
    pub launch_at_login: bool,
    /// Application identifiers (bundle id / process name) whose copies are ignored.
    pub excluded_apps: Vec<String>,
    pub shortcuts: Shortcuts,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::System,
            history_limit: HISTORY_LIMIT_DEFAULT,
            persist_history: true,
            clear_history_on_exit: false,
            restore_clipboard: true,
            launch_at_login: false,
            excluded_apps: crate::privacy::DEFAULT_EXCLUDED_APPS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            shortcuts: Shortcuts::default(),
        }
    }
}

impl Settings {
    /// Brings untrusted input (webview, hand-edited store file) into a valid state.
    pub fn sanitized(mut self) -> Self {
        self.history_limit = self
            .history_limit
            .clamp(HISTORY_LIMIT_MIN, HISTORY_LIMIT_MAX);

        let mut seen = std::collections::HashSet::new();
        self.excluded_apps = self
            .excluded_apps
            .into_iter()
            .map(|a| a.trim().to_owned())
            .filter(|a| !a.is_empty() && seen.insert(a.to_lowercase()))
            .collect();

        fn clean(s: Option<String>) -> Option<String> {
            s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
        }
        self.shortcuts.picker = clean(self.shortcuts.picker.take());
        self.shortcuts.quick_paste.resize(QUICK_PASTE_SLOTS, None);
        self.shortcuts.quick_paste = self.shortcuts.quick_paste.into_iter().map(clean).collect();
        self
    }

    pub fn load<R: Runtime>(app: &AppHandle<R>) -> Result<Self> {
        let store = app.store(STORE_FILE)?;
        let settings = match store.get(STORE_KEY) {
            Some(value) => serde_json::from_value::<Settings>(value).unwrap_or_else(|e| {
                log::warn!("invalid settings in store, falling back to defaults: {e}");
                Settings::default()
            }),
            None => Settings::default(),
        };
        Ok(settings.sanitized())
    }

    pub fn save<R: Runtime>(&self, app: &AppHandle<R>) -> Result<()> {
        let store = app.store(STORE_FILE)?;
        store.set(STORE_KEY, serde_json::to_value(self)?);
        store.save()?;
        Ok(())
    }
}

/// True exactly once per installation (first launch), used for onboarding.
pub fn take_first_run<R: Runtime>(app: &AppHandle<R>) -> Result<bool> {
    const KEY: &str = "firstRunDone";
    let store = app.store(STORE_FILE)?;
    if store.get(KEY).is_some() {
        return Ok(false);
    }
    store.set(KEY, true);
    store.save()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let s = Settings::default();
        assert_eq!(s.clone().sanitized(), s);
        assert_eq!(s.shortcuts.quick_paste.len(), QUICK_PASTE_SLOTS);
        assert!(
            s.shortcuts.quick_paste[0]
                .as_deref()
                .unwrap()
                .ends_with("+2")
        );
        assert!(
            s.shortcuts.quick_paste[1]
                .as_deref()
                .unwrap()
                .ends_with("+3")
        );
        assert!(s.shortcuts.quick_paste[2..].iter().all(Option::is_none));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn default_quick_paste_avoids_altgr() {
        // Ctrl+Alt+<digit> == AltGr+<digit> on Windows.
        let s = Shortcuts::default();
        for sc in s.quick_paste.iter().flatten() {
            assert!(sc.contains("Shift"), "{sc} collides with AltGr");
        }
    }

    #[test]
    fn history_limit_is_clamped() {
        let low = Settings {
            history_limit: 1,
            ..Default::default()
        }
        .sanitized();
        let high = Settings {
            history_limit: 9999,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(low.history_limit, HISTORY_LIMIT_MIN);
        assert_eq!(high.history_limit, HISTORY_LIMIT_MAX);
    }

    #[test]
    fn excluded_apps_are_trimmed_and_deduplicated() {
        let s = Settings {
            excluded_apps: vec![
                " com.agilebits.onepassword7 ".into(),
                "".into(),
                "KeePassXC".into(),
                "keepassxc".into(),
            ],
            ..Default::default()
        }
        .sanitized();
        assert_eq!(
            s.excluded_apps,
            vec!["com.agilebits.onepassword7", "KeePassXC"]
        );
    }

    #[test]
    fn quick_paste_is_normalized_to_slot_count() {
        let mut s = Settings::default();
        s.shortcuts.quick_paste = vec![Some("  ".into())];
        s.shortcuts.picker = Some("".into());
        let s = s.sanitized();
        assert_eq!(s.shortcuts.quick_paste, vec![None; QUICK_PASTE_SLOTS]);
        assert_eq!(s.shortcuts.picker, None);
    }

    #[test]
    fn partial_json_falls_back_to_field_defaults() {
        let s: Settings = serde_json::from_str(r#"{"historyLimit": 120}"#).unwrap();
        assert_eq!(s.history_limit, 120);
        assert!(s.persist_history);
        assert_eq!(s.shortcuts, Shortcuts::default());
    }
}
