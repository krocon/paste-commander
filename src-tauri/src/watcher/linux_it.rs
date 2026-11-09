//! Integration tests against real display servers. Ignored by default; run in
//! a session that has one, e.g. (see `scripts/linux-watcher-tests.sh`):
//!
//! ```text
//! xvfb-run cargo test -- --ignored x11_
//! sway (headless) + cargo test -- --ignored wayland_
//! ```

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use super::wayland::WaylandWatcher;
use super::x11::X11Watcher;
use super::{ClipboardWatcher, Sink};
use crate::privacy::{self, ClipboardSnapshot, RejectReason};

fn channel_sink() -> (Sink, mpsc::Receiver<ClipboardSnapshot>) {
    let (tx, rx) = mpsc::channel();
    let sink: Sink = Arc::new(move |s| {
        let _ = tx.send(s);
    });
    (sink, rx)
}

fn xclip(text: &str) {
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("xclip installed");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    child.wait().unwrap();
}

fn next(rx: &mpsc::Receiver<ClipboardSnapshot>) -> ClipboardSnapshot {
    rx.recv_timeout(Duration::from_secs(5))
        .expect("clipboard change within 5 s")
}

#[test]
#[ignore = "needs an X server and xclip"]
fn x11_captures_copies_including_incr() {
    let (sink, rx) = channel_sink();
    let _handle = Box::new(X11Watcher::new())
        .start(sink)
        .expect("X11 watcher");
    std::thread::sleep(Duration::from_millis(200));

    xclip("hello from x11 – ünïcödé");
    assert_eq!(next(&rx).text.as_deref(), Some("hello from x11 – ünïcödé"));

    // Large enough to force the INCR protocol.
    let big = "0123456789".repeat(100_000);
    xclip(&big);
    assert_eq!(next(&rx).text.map(|t| t.len()), Some(big.len()));
}

fn wl_copy(text: &str, sensitive: bool) {
    use wl_clipboard_rs::copy::{MimeType, Options, Source};
    let mut opts = Options::new();
    opts.sensitive(sensitive);
    opts.copy(
        Source::Bytes(text.as_bytes().to_vec().into_boxed_slice()),
        MimeType::Text,
    )
    .expect("wl copy");
}

#[test]
#[ignore = "needs a Wayland compositor with data-control"]
fn wayland_captures_copies_and_flags_secrets() {
    let (sink, rx) = channel_sink();
    let _handle = Box::new(WaylandWatcher)
        .start(sink)
        .expect("Wayland watcher");
    std::thread::sleep(Duration::from_millis(200));

    wl_copy("hello from wayland", false);
    let plain = next(&rx);
    assert_eq!(plain.text.as_deref(), Some("hello from wayland"));
    assert_eq!(privacy::evaluate(&plain, &[]), Ok(()));

    wl_copy("hunter2", true);
    let secret = next(&rx);
    assert_eq!(
        secret.text, None,
        "secret text must not even be transferred"
    );
    assert_eq!(
        privacy::evaluate(&secret, &[]),
        Err(RejectReason::Concealed)
    );
}
