use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::clipboard::ClipboardWriter;
use crate::history::{self, History, Location};
use crate::hotkeys::Hotkeys;
use crate::i18n::Lang;
use crate::ingest::{self, IngestContext, IngestOutcome, SelfWriteGuard};
use crate::paste::{FocusTarget, PasteService};
use crate::platform::PlatformInfo;
use crate::privacy::{ClipboardSnapshot, RejectReason};
use crate::settings::Settings;
use crate::watcher::{self, Sink, WatcherHandle};

/// Emitted (without payload) whenever entries change; the picker refetches.
pub const HISTORY_CHANGED: &str = "history-changed";

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub paused: AtomicBool,
    platform: Mutex<PlatformInfo>,
    pub history: Mutex<History>,
    /// Marks our own clipboard writes so the watcher ignores their echo.
    pub self_writes: SelfWriteGuard,
    pub clipboard: ClipboardWriter,
    pub paste: PasteService,
    pub hotkeys: Hotkeys,
    /// The last external copy was concealed or from an excluded app; such a
    /// clipboard is never restored after pasting.
    pub last_copy_was_private: AtomicBool,
    previous_focus: Mutex<Option<FocusTarget>>,
    watcher: Mutex<Option<WatcherHandle>>,
    history_file: PathBuf,
}

/// `<app data dir>/history.sqlite3`
pub fn history_file<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    Ok(app.path().app_data_dir()?.join("history.sqlite3"))
}

impl AppState {
    pub fn new<R: Runtime>(app: &AppHandle<R>, settings: Settings, history_file: PathBuf) -> Self {
        let platform = PlatformInfo::detect();
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
            paste: PasteService::start(app.clone(), platform.display_server),
            platform: Mutex::new(platform),
            history: Mutex::new(history),
            self_writes: SelfWriteGuard::default(),
            clipboard: ClipboardWriter::default(),
            hotkeys: Hotkeys::default(),
            last_copy_was_private: AtomicBool::new(false),
            previous_focus: Mutex::new(None),
            watcher: Mutex::new(None),
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

    /// Snapshot of the platform info with the current permission status.
    pub fn platform(&self) -> PlatformInfo {
        let mut info = lock(&self.platform).clone();
        info.refresh_permissions();
        info
    }

    pub fn set_previous_focus(&self, target: Option<FocusTarget>) {
        *lock(&self.previous_focus) = target;
    }

    pub fn take_previous_focus(&self) -> Option<FocusTarget> {
        lock(&self.previous_focus).take()
    }

    /// Starts the clipboard watcher; every change goes through [`Self::handle_snapshot`].
    pub fn start_watcher<R: Runtime>(&self, app: &AppHandle<R>) {
        let sink: Sink = {
            let app = app.clone();
            Arc::new(move |snapshot| {
                let state = app.state::<AppState>();
                state.handle_snapshot(&app, &snapshot);
            })
        };
        let display = lock(&self.platform).display_server;
        let outcome = match watcher::start_best(display, sink) {
            Ok((handle, backend)) => {
                *lock(&self.watcher) = Some(handle);
                Ok(backend)
            }
            Err(e) => Err(e.to_string()),
        };
        lock(&self.platform).record_watcher(outcome);
    }

    /// Stops the watcher thread (on exit).
    pub fn stop_watcher(&self) {
        drop(lock(&self.watcher).take());
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

    /// Entry point for watchers; runs on the watcher thread.
    pub fn handle_snapshot<R: Runtime>(&self, app: &AppHandle<R>, snapshot: &ClipboardSnapshot) {
        let excluded = lock(&self.settings).excluded_apps.clone();
        let ctx = IngestContext {
            paused: self.is_paused(),
            excluded_apps: &excluded,
            now_ms: history::now_ms(),
        };
        let outcome = ingest::ingest(snapshot, &mut self.history(), &self.self_writes, ctx);
        match outcome {
            Ok(IngestOutcome::Stored(_)) => {
                self.last_copy_was_private.store(false, Ordering::Relaxed);
                if outcome.as_ref().is_ok_and(|o| o.changed_history()) {
                    let _ = app.emit(HISTORY_CHANGED, ());
                }
            }
            Ok(IngestOutcome::Rejected(reason)) => {
                let private = !matches!(reason, RejectReason::NoText);
                self.last_copy_was_private.store(private, Ordering::Relaxed);
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
