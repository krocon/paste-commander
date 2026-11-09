use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::i18n::Lang;
use crate::platform::PlatformInfo;
use crate::settings::Settings;

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub paused: AtomicBool,
    pub platform: PlatformInfo,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: Mutex::new(settings),
            paused: AtomicBool::new(false),
            platform: PlatformInfo::detect(),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn lang(&self) -> Lang {
        Lang::resolve(
            self.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .language,
        )
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
}
