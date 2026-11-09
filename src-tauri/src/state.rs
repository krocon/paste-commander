use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::history::{self, History, Location};
use crate::i18n::Lang;
use crate::ingest::{self, IngestContext, IngestOutcome, SelfWriteGuard};
use crate::platform::PlatformInfo;
use crate::privacy::ClipboardSnapshot;
use crate::settings::Settings;

/// Emitted (without payload) whenever entries change; the picker refetches.
pub const HISTORY_CHANGED: &str = "history-changed";

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub paused: AtomicBool,
    pub platform: PlatformInfo,
    pub history: Mutex<History>,
    /// Lets the paste service (phase 5) mark its own clipboard writes.
    #[allow(dead_code)]
    pub self_writes: SelfWriteGuard,
    history_file: PathBuf,
}

/// `<app data dir>/history.sqlite3`
pub fn history_file<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    Ok(app.path().app_data_dir()?.join("history.sqlite3"))
}

impl AppState {
    pub fn new(settings: Settings, history_file: PathBuf) -> Self {
        let location = if settings.persist_history {
            Location::File(history_file.clone())
        } else {
            // Persistence was switched off earlier; make sure nothing is left behind.
            remove_db_files(&history_file);
            Location::Memory
        };
        let history = History::open_or_recover(location, settings.history_limit);
        Self {
            settings: Mutex::new(settings),
            paused: AtomicBool::new(false),
            platform: PlatformInfo::detect(),
            history: Mutex::new(history),
            self_writes: SelfWriteGuard::default(),
            history_file,
        }
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn lang(&self) -> Lang {
        Lang::resolve(lock(&self.settings).language)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn history(&self) -> MutexGuard<'_, History> {
        lock(&self.history)
    }

    /// Applies limit and persistence changes to the open history.
    pub fn reconfigure_history(&self, settings: &Settings) -> history::Result<()> {
        let mut history = self.history();
        if history.limit() != settings.history_limit {
            let dropped = history.set_limit(settings.history_limit)?;
            log::info!(
                "history limit {} (dropped {dropped})",
                settings.history_limit
            );
        }
        let target = if settings.persist_history {
            Location::File(self.history_file.clone())
        } else {
            Location::Memory
        };
        if *history.location() != target {
            let new = history.copy_to(target)?;
            let old = std::mem::replace(&mut *history, new);
            old.discard()?;
            log::info!("history storage switched to {:?}", history.location());
        }
        Ok(())
    }

    /// Entry point for watchers (phase 3).
    #[allow(dead_code)]
    pub fn handle_snapshot<R: Runtime>(&self, app: &AppHandle<R>, snapshot: &ClipboardSnapshot) {
        let excluded = lock(&self.settings).excluded_apps.clone();
        let ctx = IngestContext {
            paused: self.is_paused(),
            excluded_apps: &excluded,
            now_ms: history::now_ms(),
        };
        let outcome = ingest::ingest(snapshot, &mut self.history(), &self.self_writes, ctx);
        match outcome {
            Ok(o) if o.changed_history() => {
                let _ = app.emit(HISTORY_CHANGED, ());
            }
            Ok(IngestOutcome::Rejected(reason)) => {
                log::debug!("clipboard change rejected: {reason:?}")
            }
            Ok(_) => {}
            Err(e) => log::error!("storing clipboard entry failed: {e}"),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn remove_db_files(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(p));
    }
}
