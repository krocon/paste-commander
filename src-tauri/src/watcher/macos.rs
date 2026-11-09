//! macOS: NSPasteboard has no change notification, so we poll `changeCount`.
//! Reading an integer 4× per second costs effectively nothing; the pasteboard
//! contents are only read when the count actually changes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString, NSWorkspace};

use super::{ClipboardWatcher, Sink, WatchError, WatcherHandle};
use crate::privacy::{ClipboardFormat, ClipboardSnapshot, SourceApp};

const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub struct MacosWatcher;

impl ClipboardWatcher for MacosWatcher {
    fn name(&self) -> &'static str {
        "macos-changecount"
    }

    fn start(self: Box<Self>, sink: Sink) -> Result<WatcherHandle, WatchError> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let thread = std::thread::Builder::new()
            .name("clipboard-watcher".into())
            .spawn(move || {
                // Start from the current state: what is on the clipboard at launch
                // is not a new copy.
                let mut last = autoreleasepool(|_| NSPasteboard::generalPasteboard().changeCount());
                while !stop_thread.load(Ordering::Relaxed) {
                    std::thread::sleep(POLL_INTERVAL);
                    let snapshot = autoreleasepool(|_| {
                        let pb = NSPasteboard::generalPasteboard();
                        let count = pb.changeCount();
                        if count == last {
                            return None;
                        }
                        last = count;
                        Some(read_snapshot(&pb))
                    });
                    if let Some(s) = snapshot {
                        sink(s);
                    }
                }
            })
            .map_err(|e| WatchError::Failed(e.to_string()))?;

        Ok(WatcherHandle::new(move || {
            stop.store(true, Ordering::Relaxed);
            let _ = thread.join();
        }))
    }
}

fn read_snapshot(pb: &NSPasteboard) -> ClipboardSnapshot {
    let formats = pb
        .types()
        .map(|types| {
            types
                .iter()
                .map(|t| ClipboardFormat::named(t.to_string()))
                .collect()
        })
        .unwrap_or_default();
    // SAFETY: AppKit-provided constant, valid for the process lifetime.
    let string_type = unsafe { NSPasteboardTypeString };
    let text = pb.stringForType(string_type).map(|s| s.to_string());
    ClipboardSnapshot {
        text,
        formats,
        source: frontmost_app(),
    }
}

/// The frontmost app is the best available proxy for "who copied": the
/// pasteboard does not record its writer.
///
/// NSWorkspace updates `frontmostApplication` from notifications handled on
/// the main run loop, so it is read there. Async + timeout instead of
/// `exec_sync`: on quit the main thread joins this watcher thread, and a sync
/// dispatch would deadlock.
fn frontmost_app() -> Option<SourceApp> {
    let (tx, rx) = std::sync::mpsc::channel();
    dispatch2::DispatchQueue::main().exec_async(move || {
        let _ = tx.send(autoreleasepool(|_| frontmost_app_on_main()));
    });
    rx.recv_timeout(Duration::from_millis(250)).ok().flatten()
}

fn frontmost_app_on_main() -> Option<SourceApp> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let name = app.localizedName().map(|n| n.to_string());
    let id = app
        .bundleIdentifier()
        .map(|b| b.to_string())
        .or_else(|| name.clone())?;
    Some(SourceApp { id, name })
}
