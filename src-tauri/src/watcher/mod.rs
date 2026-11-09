//! Clipboard watchers. Each platform implementation (phase 3: `macos`,
//! `windows`, `x11`, `wayland_data_control`, `fallback`) turns native change
//! notifications into [`ClipboardSnapshot`]s and hands them to a sink.

use std::sync::Arc;

use crate::privacy::ClipboardSnapshot;

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("clipboard watching is not supported here: {0}")]
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
