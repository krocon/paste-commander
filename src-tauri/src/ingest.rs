//! The path every clipboard change takes: pause → own write? → privacy → history.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::history::{AddOutcome, History, NewEntry, Result};
use crate::privacy::{self, ClipboardSnapshot, RejectReason};

// The paste service (phase 5) is the only caller of `SelfWriteGuard::expect`.
/// How long after our own clipboard write a matching change is attributed to us.
#[allow(dead_code)]
const SELF_WRITE_TTL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestOutcome {
    Stored(AddOutcome),
    Rejected(RejectReason),
    Paused,
    /// Caused by Paste Commander putting an entry on the clipboard.
    OwnWrite,
}

impl IngestOutcome {
    /// Whether the visible history changed (UI needs a refresh).
    pub fn changed_history(self) -> bool {
        matches!(
            self,
            IngestOutcome::Stored(AddOutcome::Inserted(_) | AddOutcome::Bumped(_))
        )
    }
}

/// Remembers the text we are about to place on the clipboard so the watcher's
/// echo of it does not create or reorder entries.
#[derive(Default)]
pub struct SelfWriteGuard {
    pending: Mutex<Option<(String, Instant)>>,
}

#[allow(dead_code)]
impl SelfWriteGuard {
    pub fn expect(&self, text: &str) {
        self.expect_at(text, Instant::now());
    }

    fn expect_at(&self, text: &str, now: Instant) {
        *self.lock() = Some((text.to_owned(), now + SELF_WRITE_TTL));
    }

    /// Consumes the expectation if `text` matches and it has not expired.
    fn take_if_match(&self, text: &str, now: Instant) -> bool {
        let mut pending = self.lock();
        match pending.as_ref() {
            Some((expected, deadline)) if now <= *deadline && expected == text => {
                *pending = None;
                true
            }
            Some((_, deadline)) if now > *deadline => {
                *pending = None;
                false
            }
            _ => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(String, Instant)>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct IngestContext<'a> {
    pub paused: bool,
    pub excluded_apps: &'a [String],
    pub now_ms: i64,
}

pub fn ingest(
    snapshot: &ClipboardSnapshot,
    history: &mut History,
    guard: &SelfWriteGuard,
    ctx: IngestContext<'_>,
) -> Result<IngestOutcome> {
    ingest_at(snapshot, history, guard, ctx, Instant::now())
}

fn ingest_at(
    snapshot: &ClipboardSnapshot,
    history: &mut History,
    guard: &SelfWriteGuard,
    ctx: IngestContext<'_>,
    now: Instant,
) -> Result<IngestOutcome> {
    if ctx.paused {
        return Ok(IngestOutcome::Paused);
    }
    if let Some(text) = &snapshot.text
        && guard.take_if_match(text, now)
    {
        return Ok(IngestOutcome::OwnWrite);
    }
    if let Err(reason) = privacy::evaluate(snapshot, ctx.excluded_apps) {
        return Ok(IngestOutcome::Rejected(reason));
    }
    let text = snapshot.text.as_deref().unwrap_or_default();
    let source = snapshot
        .source
        .as_ref()
        .map(|s| s.name.as_deref().unwrap_or(&s.id));
    let outcome = history.add(
        NewEntry {
            content: text,
            source_app: source,
        },
        ctx.now_ms,
    )?;
    Ok(IngestOutcome::Stored(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{IgnoreReason, Location};
    use crate::privacy::{ClipboardFormat, SourceApp, macos};
    use crate::watcher::fake::FakeWatcher;
    use crate::watcher::{ClipboardWatcher, Sink};
    use std::sync::{Arc, Mutex};

    fn text(t: &str) -> ClipboardSnapshot {
        ClipboardSnapshot {
            text: Some(t.into()),
            ..Default::default()
        }
    }

    fn ctx(excluded: &[String]) -> IngestContext<'_> {
        IngestContext {
            paused: false,
            excluded_apps: excluded,
            now_ms: 1,
        }
    }

    fn history() -> History {
        History::open(Location::Memory, 50).unwrap()
    }

    #[test]
    fn stores_plain_text_with_source_name() {
        let mut h = history();
        let snap = ClipboardSnapshot {
            text: Some("hi".into()),
            formats: vec![],
            source: Some(SourceApp {
                id: "com.apple.TextEdit".into(),
                name: Some("TextEdit".into()),
            }),
        };
        let out = ingest(&snap, &mut h, &SelfWriteGuard::default(), ctx(&[])).unwrap();
        assert!(matches!(
            out,
            IngestOutcome::Stored(AddOutcome::Inserted(_))
        ));
        assert_eq!(h.list().unwrap()[0].source_app.as_deref(), Some("TextEdit"));
    }

    #[test]
    fn paused_stores_nothing() {
        let mut h = history();
        let c = IngestContext {
            paused: true,
            excluded_apps: &[],
            now_ms: 1,
        };
        assert_eq!(
            ingest(&text("x"), &mut h, &SelfWriteGuard::default(), c).unwrap(),
            IngestOutcome::Paused
        );
        assert_eq!(h.len().unwrap(), 0);
    }

    #[test]
    fn concealed_content_is_rejected() {
        let mut h = history();
        let snap = ClipboardSnapshot {
            text: Some("hunter2".into()),
            formats: vec![ClipboardFormat::named(macos::CONCEALED)],
            source: None,
        };
        let out = ingest(&snap, &mut h, &SelfWriteGuard::default(), ctx(&[])).unwrap();
        assert_eq!(out, IngestOutcome::Rejected(RejectReason::Concealed));
        assert_eq!(h.len().unwrap(), 0);
    }

    #[test]
    fn blank_text_is_ignored_and_not_a_change() {
        let mut h = history();
        let out = ingest(&text("   "), &mut h, &SelfWriteGuard::default(), ctx(&[])).unwrap();
        assert_eq!(
            out,
            IngestOutcome::Stored(AddOutcome::Ignored(IgnoreReason::Blank))
        );
        assert!(!out.changed_history());
    }

    #[test]
    fn own_write_is_swallowed_once() {
        let mut h = history();
        let guard = SelfWriteGuard::default();
        guard.expect("pasted");
        assert_eq!(
            ingest(&text("pasted"), &mut h, &guard, ctx(&[])).unwrap(),
            IngestOutcome::OwnWrite
        );
        assert_eq!(h.len().unwrap(), 0);
        // The user copying the same text again later is a real copy.
        assert!(
            ingest(&text("pasted"), &mut h, &guard, ctx(&[]))
                .unwrap()
                .changed_history()
        );
    }

    #[test]
    fn own_write_expectation_expires_and_ignores_other_text() {
        let mut h = history();
        let guard = SelfWriteGuard::default();
        let t0 = Instant::now();
        guard.expect_at("mine", t0);
        assert!(
            ingest_at(&text("other"), &mut h, &guard, ctx(&[]), t0)
                .unwrap()
                .changed_history()
        );
        let late = t0 + SELF_WRITE_TTL + Duration::from_millis(1);
        assert!(
            ingest_at(&text("mine"), &mut h, &guard, ctx(&[]), late)
                .unwrap()
                .changed_history()
        );
    }

    /// End-to-end with the fake watcher: snapshots flow from a background
    /// thread through the same sink a real watcher would use.
    #[test]
    fn fake_watcher_feeds_history() {
        let history = Arc::new(Mutex::new(history()));
        let guard = Arc::new(SelfWriteGuard::default());
        let excluded = vec!["KeePassXC".to_string()];

        let sink: Sink = {
            let history = history.clone();
            let guard = guard.clone();
            Arc::new(move |snap| {
                let mut h = history.lock().unwrap();
                ingest(&snap, &mut h, &guard, ctx(&excluded)).unwrap();
            })
        };

        let (watcher, tx) = FakeWatcher::new();
        let handle = Box::new(watcher).start(sink).unwrap();

        tx.send(text("first")).unwrap();
        tx.send(text("second")).unwrap();
        tx.send(text("first")).unwrap(); // duplicate → moves up
        tx.send(ClipboardSnapshot {
            text: Some("secret".into()),
            formats: vec![],
            source: Some(SourceApp {
                id: "keepassxc".into(),
                name: None,
            }),
        })
        .unwrap();
        tx.send(text("")).unwrap();
        drop(tx);
        drop(handle); // joins the watcher thread

        let list = history.lock().unwrap().list().unwrap();
        let texts: Vec<_> = list.iter().map(|e| e.preview.as_str()).collect();
        assert_eq!(texts, ["first", "second"]);
    }
}
