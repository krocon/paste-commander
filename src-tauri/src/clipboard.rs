//! Writing to (and reading back from) the system clipboard.
//!
//! One long-lived `arboard::Clipboard`: on X11/Wayland the instance keeps
//! serving the selection to other apps, so it must outlive each write.
//! Every write is announced to the [`SelfWriteGuard`] first, so the watcher's
//! echo of our own write never creates or reorders history entries.

use std::sync::{Mutex, MutexGuard};

use crate::ingest::SelfWriteGuard;

#[derive(Default)]
pub struct ClipboardWriter {
    inner: Mutex<Option<arboard::Clipboard>>,
}

impl ClipboardWriter {
    fn lock(&self) -> Result<MutexGuard<'_, Option<arboard::Clipboard>>, String> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
        }
        Ok(guard)
    }

    pub fn set_text(&self, text: &str, guard: &SelfWriteGuard) -> Result<(), String> {
        guard.expect(text);
        let mut clipboard = self.lock()?;
        clipboard
            .as_mut()
            .expect("initialized by lock()")
            .set_text(text)
            .map_err(|e| e.to_string())
    }

    /// Current clipboard text, if any (used to restore it after pasting).
    pub fn get_text(&self) -> Option<String> {
        let mut clipboard = self.lock().ok()?;
        clipboard.as_mut()?.get_text().ok()
    }
}
