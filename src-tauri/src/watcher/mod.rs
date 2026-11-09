//! Clipboard watchers. Each platform implementation turns native change
//! notifications into [`ClipboardSnapshot`]s and hands them to a sink.
//!
//! | Platform        | Implementation                                   |
//! |-----------------|--------------------------------------------------|
//! | macOS           | `macos`: `NSPasteboard.changeCount` polling      |
//! | Windows         | `windows`: `AddClipboardFormatListener`          |
//! | Linux X11       | `x11`: XFixes selection events                   |
//! | Linux Wayland   | `wayland`: ext/wlr data-control                  |
//! | GNOME Wayland   | fallback: `x11` against XWayland (best effort)   |

use std::sync::Arc;

use crate::platform::DisplayServer;
use crate::privacy::ClipboardSnapshot;

#[cfg(all(test, target_os = "linux"))]
mod linux_it;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod x11;

/// Which watcher ended up running; drives the platform status page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The native mechanism of this platform/session.
    Native(&'static str),
    /// Data-control missing on Wayland: XWayland only sees copies that reach
    /// X11 (typically while an X11 window has focus).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    XWaylandFallback,
}

/// Starts the best available watcher for this session.
pub fn start_best(
    display: DisplayServer,
    sink: Sink,
) -> Result<(WatcherHandle, Backend), WatchError> {
    let _ = display;
    #[cfg(target_os = "macos")]
    return start(Box::new(macos::MacosWatcher), sink).map(|(h, n)| (h, Backend::Native(n)));

    #[cfg(target_os = "windows")]
    return start(Box::new(windows::WindowsWatcher), sink).map(|(h, n)| (h, Backend::Native(n)));

    #[cfg(target_os = "linux")]
    return match display {
        DisplayServer::Wayland => match start(Box::new(wayland::WaylandWatcher), sink.clone()) {
            Ok((h, n)) => Ok((h, Backend::Native(n))),
            Err(e) => {
                log::warn!("Wayland data-control unavailable ({e}); trying XWayland");
                start(Box::new(x11::X11Watcher::xwayland()), sink)
                    .map(|(h, _)| (h, Backend::XWaylandFallback))
            }
        },
        _ => start(Box::new(x11::X11Watcher::new()), sink).map(|(h, n)| (h, Backend::Native(n))),
    };

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = sink;
        Err(WatchError::Unsupported("no clipboard watcher for this OS"))
    }
}

#[cfg_attr(
    not(any(target_os = "macos", target_os = "windows", target_os = "linux")),
    allow(dead_code)
)]
fn start(
    watcher: Box<dyn ClipboardWatcher>,
    sink: Sink,
) -> Result<(WatcherHandle, &'static str), WatchError> {
    let name = watcher.name();
    let handle = watcher.start(sink)?;
    log::info!("clipboard watcher started: {name}");
    Ok((handle, name))
}

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("clipboard watching is not supported here: {0}")]
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Unsupported(&'static str),
    #[error("watcher failed: {0}")]
    Failed(String),
}

/// Receives every observed clipboard change. Called from the watcher's thread.
pub type Sink = Arc<dyn Fn(ClipboardSnapshot) + Send + Sync>;

pub trait ClipboardWatcher: Send {
    /// Short identifier for logs and the platform status page.
    fn name(&self) -> &'static str;

    /// Starts watching on a background thread. Watching stops when the returned
    /// handle is dropped.
    fn start(self: Box<Self>, sink: Sink) -> Result<WatcherHandle, WatchError>;
}

/// Stops its watcher on drop.
pub struct WatcherHandle {
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl WatcherHandle {
    pub fn new(stop: impl FnOnce() + Send + 'static) -> Self {
        Self {
            stop: Some(Box::new(stop)),
        }
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

/// Test double: snapshots pushed into the returned sender are delivered to the
/// sink on a background thread, just like a real watcher would.
#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::mpsc;

    pub struct FakeWatcher {
        rx: mpsc::Receiver<ClipboardSnapshot>,
    }

    impl FakeWatcher {
        pub fn new() -> (Self, mpsc::Sender<ClipboardSnapshot>) {
            let (tx, rx) = mpsc::channel();
            (Self { rx }, tx)
        }
    }

    impl ClipboardWatcher for FakeWatcher {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn start(self: Box<Self>, sink: Sink) -> Result<WatcherHandle, WatchError> {
            // The thread ends when all senders are dropped.
            let thread = std::thread::spawn(move || {
                for snapshot in self.rx {
                    sink(snapshot);
                }
            });
            Ok(WatcherHandle::new(move || {
                let _ = thread.join();
            }))
        }
    }
}
