//! Clipboard history backed by SQLite.
//!
//! SQLite over JSON: every copy is a small atomic write instead of rewriting
//! the whole file, pins/limits are single statements, `secure_delete` overwrites
//! removed (possibly sensitive) text, and phase 2 can store images as BLOBs.
//!
//! Ordering uses a monotonically increasing `seq` instead of timestamps, so
//! "re-copied moves to the top" stays exact even within the same millisecond.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

#[cfg(test)]
mod tests;

/// Larger clipboard texts (log dumps, base64 blobs) are not stored.
pub const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
/// Characters sent to the picker per entry; full text stays in Rust.
pub const PREVIEW_CHARS: usize = 300;

const SCHEMA_VERSION: i32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("database schema {0} is newer than this app supports")]
    FutureSchema(i32),
}

pub type Result<T> = std::result::Result<T, HistoryError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Text,
}

impl EntryKind {
    fn as_str(self) -> &'static str {
        match self {
            EntryKind::Text => "text",
        }
    }
}

/// A full history entry, used inside Rust (paste, quick paste).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub kind: EntryKind,
    pub content: String,
    pub source_app: Option<String>,
    /// Unix milliseconds of the first copy.
    pub created_at: i64,
    /// Unix milliseconds of the most recent copy/paste.
    pub copied_at: i64,
    pub pinned: bool,
}

/// What the picker gets: a bounded preview instead of the full text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub id: i64,
    pub kind: EntryKind,
    pub preview: String,
    /// Full length in characters, so the UI can show "…" / size hints.
    pub chars: usize,
    pub source_app: Option<String>,
    pub created_at: i64,
    pub copied_at: i64,
    pub pinned: bool,
}

impl From<Entry> for EntrySummary {
    fn from(e: Entry) -> Self {
        let chars = e.content.chars().count();
        let preview = if chars > PREVIEW_CHARS {
            e.content.chars().take(PREVIEW_CHARS).collect()
        } else {
            e.content
        };
        Self {
            id: e.id,
            kind: e.kind,
            preview,
            chars,
            source_app: e.source_app,
            created_at: e.created_at,
            copied_at: e.copied_at,
            pinned: e.pinned,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NewEntry<'a> {
    pub content: &'a str,
    pub source_app: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreReason {
    Blank,
    TooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    Inserted(i64),
    /// Same text already existed; it moved to the top.
    Bumped(i64),
    Ignored(IgnoreReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// "Keep history across restarts" disabled.
    Memory,
    File(PathBuf),
}

pub struct History {
    conn: Connection,
    location: Location,
    limit: u32,
}

const COLUMNS: &str = "id, kind, content, source_app, created_at, copied_at, pinned";

fn row_to_entry(row: &Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        kind: EntryKind::Text, // only kind so far; column exists for phase 2
        content: row.get(2)?,
        source_app: row.get(3)?,
        created_at: row.get(4)?,
        copied_at: row.get(5)?,
        pinned: row.get(6)?,
    })
}

/// FNV-1a: stable across Rust versions (unlike `DefaultHasher`), good enough to
/// narrow the duplicate lookup; equality is still checked on the full text.
fn content_hash(s: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h as i64
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl History {
    pub fn open(location: Location, limit: u32) -> Result<Self> {
        let conn = match &location {
            Location::Memory => Connection::open_in_memory()?,
            Location::File(path) => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let conn = Connection::open(path)?;
                restrict_permissions(path);
                conn
            }
        };
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA secure_delete = ON;
             PRAGMA foreign_keys = ON;",
        )?;
        migrate(&conn)?;
        let mut history = Self {
            conn,
            location,
            limit,
        };
        history.trim()?;
        Ok(history)
    }

    /// Opens the file; if it is unreadable, moves it aside and starts fresh so a
    /// corrupt database never keeps the app from starting.
    pub fn open_or_recover(location: Location, limit: u32) -> Self {
        match Self::open(location.clone(), limit) {
            Ok(h) => h,
            Err(e) => {
                log::error!("history database unusable: {e}");
                if let Location::File(path) = &location {
                    let aside = path.with_extension(format!("corrupt-{}", now_ms()));
                    if std::fs::rename(path, &aside).is_ok() {
                        log::warn!("moved broken database to {}", aside.display());
                        if let Ok(h) = Self::open(location.clone(), limit) {
                            return h;
                        }
                    }
                }
                log::warn!("falling back to in-memory history");
                Self::open(Location::Memory, limit).expect("in-memory SQLite must open")
            }
        }
    }

    pub fn location(&self) -> &Location {
        &self.location
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }

    pub fn add(&mut self, entry: NewEntry<'_>, now: i64) -> Result<AddOutcome> {
        if entry.content.trim().is_empty() {
            return Ok(AddOutcome::Ignored(IgnoreReason::Blank));
        }
        if entry.content.len() > MAX_TEXT_BYTES {
            return Ok(AddOutcome::Ignored(IgnoreReason::TooLarge));
        }

        let hash = content_hash(entry.content);
        let tx = self.conn.transaction()?;
        let seq: i64 = tx.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM entries", [], |r| {
            r.get(0)
        })?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM entries WHERE hash = ?1 AND content = ?2",
                params![hash, entry.content],
                |r| r.get(0),
            )
            .optional()?;

        let outcome = match existing {
            Some(id) => {
                tx.execute(
                    "UPDATE entries SET seq = ?1, copied_at = ?2,
                            source_app = COALESCE(?3, source_app)
                     WHERE id = ?4",
                    params![seq, now, entry.source_app, id],
                )?;
                AddOutcome::Bumped(id)
            }
            None => {
                tx.execute(
                    "INSERT INTO entries (kind, content, hash, source_app, created_at, copied_at, seq, pinned)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, 0)",
                    params![EntryKind::Text.as_str(), entry.content, hash, entry.source_app, now, seq],
                )?;
                AddOutcome::Inserted(tx.last_insert_rowid())
            }
        };
        tx.commit()?;
        self.trim()?;
        Ok(outcome)
    }

    /// All entries, most recent first.
    pub fn list(&self) -> Result<Vec<EntrySummary>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT {COLUMNS} FROM entries ORDER BY seq DESC"))?;
        let rows = stmt.query_map([], row_to_entry)?;
        rows.map(|r| r.map(EntrySummary::from).map_err(Into::into))
            .collect()
    }

    /// Case-insensitive substring search over the full text and source app.
    /// Done in Rust because SQLite's `lower()` only folds ASCII.
    pub fn search(&self, query: &str) -> Result<Vec<EntrySummary>> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return self.list();
        }
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT {COLUMNS} FROM entries ORDER BY seq DESC"))?;
        let rows = stmt.query_map([], row_to_entry)?;
        let mut out = Vec::new();
        for entry in rows {
            let entry = entry?;
            let hit = entry.content.to_lowercase().contains(&needle)
                || entry
                    .source_app
                    .as_deref()
                    .is_some_and(|a| a.to_lowercase().contains(&needle));
            if hit {
                out.push(entry.into());
            }
        }
        Ok(out)
    }

    pub fn get(&self, id: i64) -> Result<Option<Entry>> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM entries WHERE id = ?1"),
                [id],
                row_to_entry,
            )
            .optional()
            .map_err(Into::into)
    }

    /// 1-based position in recency order (1 = most recent), for quick paste.
    pub fn nth_recent(&self, position: usize) -> Result<Option<Entry>> {
        let Some(offset) = position.checked_sub(1) else {
            return Ok(None);
        };
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM entries ORDER BY seq DESC LIMIT 1 OFFSET ?1"),
                [offset as i64],
                row_to_entry,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Moves an entry to the top without creating a new one (used after pasting it).
    pub fn touch(&mut self, id: i64, now: i64) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE entries SET seq = (SELECT COALESCE(MAX(seq), 0) + 1 FROM entries), copied_at = ?1
             WHERE id = ?2",
            params![now, id],
        )?;
        Ok(n > 0)
    }

    pub fn delete(&mut self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM entries WHERE id = ?1", [id])?
            > 0)
    }

    pub fn set_pinned(&mut self, id: i64, pinned: bool) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE entries SET pinned = ?1 WHERE id = ?2",
            params![pinned, id],
        )? > 0;
        if changed && !pinned {
            self.trim()?;
        }
        Ok(changed)
    }

    /// Returns the number of removed entries.
    pub fn clear(&mut self, keep_pinned: bool) -> Result<usize> {
        let sql = if keep_pinned {
            "DELETE FROM entries WHERE pinned = 0"
        } else {
            "DELETE FROM entries"
        };
        let n = self.conn.execute(sql, [])?;
        // Make sure deleted text does not linger in the WAL file.
        if matches!(self.location, Location::File(_)) {
            self.conn
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        Ok(n)
    }

    /// Returns the number of entries dropped by the new limit.
    pub fn set_limit(&mut self, limit: u32) -> Result<usize> {
        self.limit = limit;
        self.trim()
    }

    #[cfg(test)]
    pub fn len(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Copies all entries into a fresh history at `to` (used when "keep history
    /// across restarts" is toggled). The caller swaps it in and then calls
    /// [`History::discard`] on the old one.
    pub fn copy_to(&self, to: Location) -> Result<History> {
        let mut target = History::open(to, self.limit)?;
        let tx = target.conn.transaction()?;
        tx.execute("DELETE FROM entries", [])?;
        {
            let mut src = self.conn.prepare(
                "SELECT kind, content, hash, source_app, created_at, copied_at, seq, pinned FROM entries",
            )?;
            let mut insert = tx.prepare(
                "INSERT INTO entries (kind, content, hash, source_app, created_at, copied_at, seq, pinned)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            let mut rows = src.query([])?;
            while let Some(r) = rows.next()? {
                insert.execute(params![
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, bool>(7)?,
                ])?;
            }
        }
        tx.commit()?;
        Ok(target)
    }

    /// Closes the database and, for file storage, deletes it with its WAL/SHM files.
    pub fn discard(self) -> Result<()> {
        let location = self.location.clone();
        drop(self.conn);
        if let Location::File(path) = location {
            for suffix in ["", "-wal", "-shm"] {
                let mut p = path.clone().into_os_string();
                p.push(suffix);
                match std::fs::remove_file(PathBuf::from(p)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Ok(())
    }

    /// Drops the oldest unpinned entries beyond the limit. Pinned entries never count.
    fn trim(&mut self) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM entries WHERE pinned = 0 AND id NOT IN
               (SELECT id FROM entries WHERE pinned = 0 ORDER BY seq DESC LIMIT ?1)",
            [self.limit],
        )?)
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(HistoryError::FutureSchema(version));
    }
    if version < 1 {
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS entries (
                 id         INTEGER PRIMARY KEY,
                 kind       TEXT    NOT NULL DEFAULT 'text',
                 content    TEXT    NOT NULL,
                 hash       INTEGER NOT NULL,
                 source_app TEXT,
                 created_at INTEGER NOT NULL,
                 copied_at  INTEGER NOT NULL,
                 seq        INTEGER NOT NULL,
                 pinned     INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS entries_hash ON entries(hash);
             CREATE INDEX IF NOT EXISTS entries_seq  ON entries(seq);
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    }
    Ok(())
}

/// Clipboard contents are private: owner-only file on Unix. On Windows the
/// per-user app data directory already has a user-only ACL.
fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            log::warn!("could not restrict permissions of {}: {e}", path.display());
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}
