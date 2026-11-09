use super::*;

fn mem(limit: u32) -> History {
    History::open(Location::Memory, limit).unwrap()
}

fn add(h: &mut History, text: &str) -> AddOutcome {
    h.add(
        NewEntry {
            content: text,
            source_app: None,
        },
        1_000,
    )
    .unwrap()
}

fn texts(h: &History) -> Vec<String> {
    h.list().unwrap().into_iter().map(|e| e.preview).collect()
}

fn id_of(o: AddOutcome) -> i64 {
    match o {
        AddOutcome::Inserted(id) | AddOutcome::Bumped(id) => id,
        AddOutcome::Ignored(r) => panic!("ignored: {r:?}"),
    }
}

#[test]
fn newest_first() {
    let mut h = mem(50);
    add(&mut h, "a");
    add(&mut h, "b");
    add(&mut h, "c");
    assert_eq!(texts(&h), ["c", "b", "a"]);
}

#[test]
fn duplicate_moves_to_top_without_new_entry() {
    let mut h = mem(50);
    let first = id_of(add(&mut h, "same"));
    add(&mut h, "other");
    let again = add(&mut h, "same");
    assert_eq!(again, AddOutcome::Bumped(first));
    assert_eq!(texts(&h), ["same", "other"]);
    assert_eq!(h.len().unwrap(), 2);
}

#[test]
fn duplicate_keeps_created_at_and_updates_copied_at_and_source() {
    let mut h = mem(50);
    let id = id_of(
        h.add(
            NewEntry {
                content: "x",
                source_app: Some("A"),
            },
            10,
        )
        .unwrap(),
    );
    h.add(
        NewEntry {
            content: "x",
            source_app: Some("B"),
        },
        20,
    )
    .unwrap();
    h.add(
        NewEntry {
            content: "x",
            source_app: None,
        },
        30,
    )
    .unwrap();
    let e = h.get(id).unwrap().unwrap();
    assert_eq!((e.created_at, e.copied_at), (10, 30));
    assert_eq!(
        e.source_app.as_deref(),
        Some("B"),
        "unknown source keeps the last known one"
    );
}

#[test]
fn whitespace_variants_are_distinct_but_blank_is_ignored() {
    let mut h = mem(50);
    assert_eq!(add(&mut h, ""), AddOutcome::Ignored(IgnoreReason::Blank));
    assert_eq!(
        add(&mut h, " \n\t "),
        AddOutcome::Ignored(IgnoreReason::Blank)
    );
    add(&mut h, "a");
    add(&mut h, "a ");
    assert_eq!(h.len().unwrap(), 2, "content is stored verbatim");
}

#[test]
fn oversized_text_is_ignored() {
    let mut h = mem(50);
    let big = "x".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(
        add(&mut h, &big),
        AddOutcome::Ignored(IgnoreReason::TooLarge)
    );
    assert_eq!(h.len().unwrap(), 0);
}

#[test]
fn limit_drops_oldest_unpinned() {
    let mut h = mem(3);
    for t in ["1", "2", "3", "4", "5"] {
        add(&mut h, t);
    }
    assert_eq!(texts(&h), ["5", "4", "3"]);
}

#[test]
fn pinned_entries_survive_limit_and_do_not_count() {
    let mut h = mem(2);
    let pinned = id_of(add(&mut h, "pinned"));
    h.set_pinned(pinned, true).unwrap();
    for t in ["a", "b", "c"] {
        add(&mut h, t);
    }
    assert_eq!(texts(&h), ["c", "b", "pinned"]);
}

#[test]
fn unpinning_reapplies_limit() {
    let mut h = mem(2);
    let old = id_of(add(&mut h, "old"));
    h.set_pinned(old, true).unwrap();
    add(&mut h, "a");
    add(&mut h, "b");
    h.set_pinned(old, false).unwrap();
    assert_eq!(texts(&h), ["b", "a"]);
}

#[test]
fn lowering_limit_trims_immediately() {
    let mut h = mem(10);
    for i in 0..10 {
        add(&mut h, &i.to_string());
    }
    assert_eq!(h.set_limit(4).unwrap(), 6);
    assert_eq!(texts(&h), ["9", "8", "7", "6"]);
}

#[test]
fn clear_can_keep_pins() {
    let mut h = mem(10);
    let p = id_of(add(&mut h, "p"));
    h.set_pinned(p, true).unwrap();
    add(&mut h, "x");
    assert_eq!(h.clear(true).unwrap(), 1);
    assert_eq!(texts(&h), ["p"]);
    assert_eq!(h.clear(false).unwrap(), 1);
    assert_eq!(h.len().unwrap(), 0);
}

#[test]
fn nth_recent_is_one_based() {
    let mut h = mem(10);
    for t in ["a", "b", "c"] {
        add(&mut h, t);
    }
    assert_eq!(h.nth_recent(0).unwrap(), None);
    assert_eq!(h.nth_recent(1).unwrap().unwrap().content, "c");
    assert_eq!(h.nth_recent(3).unwrap().unwrap().content, "a");
    assert_eq!(h.nth_recent(4).unwrap(), None);
}

#[test]
fn touch_moves_to_top() {
    let mut h = mem(10);
    let a = id_of(add(&mut h, "a"));
    add(&mut h, "b");
    assert!(h.touch(a, 5_000).unwrap());
    assert_eq!(texts(&h), ["a", "b"]);
    assert_eq!(h.get(a).unwrap().unwrap().copied_at, 5_000);
    assert!(!h.touch(999, 0).unwrap());
}

#[test]
fn delete_and_pin_report_missing_ids() {
    let mut h = mem(10);
    let a = id_of(add(&mut h, "a"));
    assert!(h.delete(a).unwrap());
    assert!(!h.delete(a).unwrap());
    assert!(!h.set_pinned(a, true).unwrap());
}

#[test]
fn search_is_unicode_case_insensitive_over_full_text() {
    let mut h = mem(10);
    let long = format!("{}ÄRGER am Ende", "x".repeat(PREVIEW_CHARS + 10));
    add(&mut h, &long);
    add(&mut h, "Grüße aus München");
    h.add(
        NewEntry {
            content: "irrelevant",
            source_app: Some("com.apple.Safari"),
        },
        1,
    )
    .unwrap();

    assert_eq!(h.search("münchen").unwrap().len(), 1);
    assert_eq!(
        h.search("ärger").unwrap().len(),
        1,
        "match beyond the preview"
    );
    assert_eq!(h.search("safari").unwrap().len(), 1, "matches source app");
    assert_eq!(h.search("  ").unwrap().len(), 3, "blank query lists all");
    assert!(h.search("nope").unwrap().is_empty());
}

#[test]
fn summary_preview_is_bounded_and_char_safe() {
    let mut h = mem(10);
    let text = "ü".repeat(PREVIEW_CHARS + 5);
    add(&mut h, &text);
    let s = &h.list().unwrap()[0];
    assert_eq!(s.preview.chars().count(), PREVIEW_CHARS);
    assert_eq!(s.chars, PREVIEW_CHARS + 5);
}

#[test]
fn hash_collisions_do_not_merge_entries() {
    // Different text, forced into the same hash bucket by direct SQL.
    let mut h = mem(10);
    let a = id_of(add(&mut h, "a"));
    h.conn
        .execute(
            "UPDATE entries SET hash = ?1 WHERE id = ?2",
            params![content_hash("b"), a],
        )
        .unwrap();
    assert!(matches!(add(&mut h, "b"), AddOutcome::Inserted(_)));
    assert_eq!(h.len().unwrap(), 2);
}

// --- file storage -----------------------------------------------------------

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "pc-test-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn db(&self) -> PathBuf {
        self.0.join("history.sqlite3")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn history_and_pins_survive_reopen() {
    let dir = TempDir::new("reopen");
    {
        let mut h = History::open(Location::File(dir.db()), 10).unwrap();
        let p = id_of(add(&mut h, "pinned"));
        h.set_pinned(p, true).unwrap();
        add(&mut h, "recent");
    }
    let h = History::open(Location::File(dir.db()), 10).unwrap();
    let list = h.list().unwrap();
    assert_eq!(
        list.iter().map(|e| e.preview.as_str()).collect::<Vec<_>>(),
        ["recent", "pinned"]
    );
    assert!(list[1].pinned);
}

#[test]
fn reopening_with_smaller_limit_trims() {
    let dir = TempDir::new("limit");
    {
        let mut h = History::open(Location::File(dir.db()), 10).unwrap();
        for i in 0..10 {
            add(&mut h, &i.to_string());
        }
    }
    let h = History::open(Location::File(dir.db()), 3).unwrap();
    assert_eq!(h.len().unwrap(), 3);
}

#[cfg(unix)]
#[test]
fn database_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("perm");
    let _h = History::open(Location::File(dir.db()), 10).unwrap();
    let mode = std::fs::metadata(dir.db()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn switching_to_memory_copies_entries_and_deletes_file() {
    let dir = TempDir::new("to-mem");
    let mut file = History::open(Location::File(dir.db()), 10).unwrap();
    let p = id_of(add(&mut file, "keep me"));
    file.set_pinned(p, true).unwrap();
    add(&mut file, "newest");

    let mem = file.copy_to(Location::Memory).unwrap();
    file.discard().unwrap();

    assert!(!dir.db().exists());
    let list = mem.list().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].preview, "newest");
    assert!(list[1].pinned);
}

#[test]
fn switching_to_file_persists_memory_entries() {
    let dir = TempDir::new("to-file");
    let mut mem = mem(10);
    add(&mut mem, "a");
    add(&mut mem, "b");
    let file = mem.copy_to(Location::File(dir.db())).unwrap();
    mem.discard().unwrap();
    drop(file);

    let reopened = History::open(Location::File(dir.db()), 10).unwrap();
    assert_eq!(
        reopened
            .list()
            .unwrap()
            .iter()
            .map(|e| e.preview.clone())
            .collect::<Vec<_>>(),
        ["b", "a"]
    );
}

#[test]
fn corrupt_file_is_moved_aside() {
    let dir = TempDir::new("corrupt");
    std::fs::write(
        dir.db(),
        b"definitely not sqlite, just garbage bytes padding padding",
    )
    .unwrap();
    let mut h = History::open_or_recover(Location::File(dir.db()), 10);
    assert_eq!(h.location(), &Location::File(dir.db()));
    add(&mut h, "works");
    let aside = std::fs::read_dir(&dir.0)
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains("corrupt-"));
    assert!(aside);
}
