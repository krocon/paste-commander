//! Wayland: `ext-data-control-v1` (fallback `wlr-data-control`) via
//! wl-clipboard-rs. Works on KDE Plasma, Sway, Hyprland, COSMIC, …; not on
//! GNOME/Mutter, where `Watcher::new` fails and we fall back to XWayland.

use std::io::Read;
use std::sync::mpsc;
use std::time::Duration;

use wl_clipboard_rs::paste::Seat;
use wl_clipboard_rs::watch::{ClipboardEvent, ClipboardType, Watcher};

use super::{ClipboardWatcher, Sink, WatchError, WatcherHandle};
use crate::history::MAX_TEXT_BYTES;
use crate::privacy::{self, ClipboardFormat, ClipboardSnapshot};

const READ_TIMEOUT: Duration = Duration::from_secs(2);
const TEXT_MIME_TYPES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "STRING",
    "TEXT",
];

pub struct WaylandWatcher;

impl ClipboardWatcher for WaylandWatcher {
    fn name(&self) -> &'static str {
        "wayland-data-control"
    }

    fn start(self: Box<Self>, sink: Sink) -> Result<WatcherHandle, WatchError> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("clipboard-watcher".into())
            .spawn(move || {
                let mut watcher = match Watcher::new(ClipboardType::Regular, Seat::Unspecified) {
                    Ok(w) => w,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(watcher.cancel_handle()));
                if let Err(e) = run(&mut watcher, &sink) {
                    log::error!("Wayland clipboard watcher stopped: {e}");
                }
            })
            .map_err(|e| WatchError::Failed(e.to_string()))?;

        let cancel = match ready_rx.recv() {
            Ok(Ok(cancel)) => cancel,
            Ok(Err(e)) => {
                let _ = thread.join();
                return Err(WatchError::Unsupported(Box::leak(e.into_boxed_str())));
            }
            Err(e) => return Err(WatchError::Failed(e.to_string())),
        };

        Ok(WatcherHandle::new(move || {
            cancel.cancel();
            let _ = thread.join();
        }))
    }
}

fn run(watcher: &mut Watcher, sink: &Sink) -> Result<(), wl_clipboard_rs::paste::Error> {
    // The first event reports the selection present at startup (`Changed`, or
    // `Cleared` if empty), not a new copy.
    let mut first = true;
    while let Some(event) = watcher.next_event()? {
        if std::mem::take(&mut first) {
            continue;
        }
        let ClipboardEvent::Changed {
            mime_types,
            mut offer,
            ..
        } = event
        else {
            continue;
        };

        let mut formats: Vec<ClipboardFormat> =
            mime_types.iter().map(ClipboardFormat::named).collect();
        for marker in privacy::linux::DATA_FORMATS {
            if let Some(f) = formats.iter_mut().find(|f| f.name == marker) {
                f.data = offer
                    .receive(marker)
                    .ok()
                    .and_then(|r| read_with_timeout(r, 64));
            }
        }

        let text = if privacy::marker_verdict(&formats).is_some() {
            None // never transfer secrets
        } else {
            TEXT_MIME_TYPES
                .iter()
                .find(|m| mime_types.iter().any(|t| t == *m))
                .and_then(|m| offer.receive(m).ok())
                .and_then(|r| read_with_timeout(r, MAX_TEXT_BYTES + 1))
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        };

        // Wayland does not expose which client set the selection.
        sink(ClipboardSnapshot {
            text,
            formats,
            source: None,
        });
    }
    Ok(())
}

/// The source client writes into the pipe; a misbehaving client must not block
/// the watcher forever, so reading happens on a helper thread with a timeout.
fn read_with_timeout(reader: impl Read + Send + 'static, max: usize) -> Option<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let result = reader.take(max as u64).read_to_end(&mut buf).map(|_| buf);
        let _ = tx.send(result);
    });
    rx.recv_timeout(READ_TIMEOUT).ok()?.ok()
}
