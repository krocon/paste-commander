//! Native strings (tray menu, window titles). The webview has its own catalog.

use crate::settings::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
}

impl Lang {
    pub fn resolve(setting: Language) -> Self {
        match setting {
            Language::De => Lang::De,
            Language::En => Lang::En,
            Language::System => Self::from_locale(sys_locale::get_locale().as_deref()),
        }
    }

    fn from_locale(locale: Option<&str>) -> Self {
        match locale {
            Some(l) if l.to_lowercase().starts_with("de") => Lang::De,
            _ => Lang::En,
        }
    }
}

pub struct Strings {
    pub open_history: &'static str,
    pub clear_history: &'static str,
    pub pause: &'static str,
    pub settings: &'static str,
    pub quit: &'static str,
    pub settings_title: &'static str,
    pub tooltip_paused: &'static str,
}

pub fn strings(lang: Lang) -> &'static Strings {
    match lang {
        Lang::De => &Strings {
            open_history: "Historie öffnen",
            clear_history: "Historie leeren",
            pause: "Pausieren",
            settings: "Einstellungen …",
            quit: "Paste Commander beenden",
            settings_title: "Paste Commander – Einstellungen",
            tooltip_paused: "Paste Commander (pausiert)",
        },
        Lang::En => &Strings {
            open_history: "Open History",
            clear_history: "Clear History",
            pause: "Pause",
            settings: "Settings…",
            quit: "Quit Paste Commander",
            settings_title: "Paste Commander – Settings",
            tooltip_paused: "Paste Commander (paused)",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_mapping() {
        assert_eq!(Lang::from_locale(Some("de-DE")), Lang::De);
        assert_eq!(Lang::from_locale(Some("de_AT")), Lang::De);
        assert_eq!(Lang::from_locale(Some("en-US")), Lang::En);
        assert_eq!(Lang::from_locale(Some("fr-FR")), Lang::En);
        assert_eq!(Lang::from_locale(None), Lang::En);
    }
}
