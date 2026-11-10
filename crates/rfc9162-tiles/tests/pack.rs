//! A packed store keeps every path in one file: it reads back after a
//! reopen, ignores a torn tail, refuses a second writer, lets a reader see a
//! writer's appends, compacts to its live records, compresses bundles, and
//! carries a whole log in a constant number of inodes.

use rfc9162_tiles::index::no_keys;
use rfc9162_tiles::layout::TREE_STATE_LOCK;
use rfc9162_tiles::{PackStore, Reader, TileStore, Writer, PACK_MAGIC};
use std::io::{Seek, SeekFrom, Write};

fn entry(i: u64) -> Vec<u8> {
    format!(
        "{{\"ts\":\"2026-11-08T12:00:{:02}Z\",\"module\":\"PETSc/3.25.0-foss-2026.1\",\"record\":{i}}}",
        i % 60
    )
    .into_bytes()
}

#[test]
fn paths_read_back_after_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    {
        let store = PackStore::open(&pack).unwrap();
        store.put("a/one", b"1").unwrap();
        store.put("a/two", b"2").unwrap();
        store.put("b/three", b"3").unwrap();
        assert!(!store.create_new("a/one", b"x").unwrap());
        assert!(store.create_new("c/four", b"4").unwrap());
        store.put("a/one", b"one again").unwrap();
        assert!(store.delete("a/two").unwrap());
        assert!(!store.delete("a/two").unwrap());
        store.rename("b/three", "b/moved").unwrap();
        assert_eq!(store.list("a").unwrap(), vec!["a/one".to_string()]);
        assert_eq!(
            store.list("").unwrap(),
            vec![
                "a/one".to_string(),
                "b/moved".to_string(),
                "c/four".to_string()
            ]
        );
    }
    assert!(
        !dir.path().join("pack.lock").exists(),
        "the lock goes with the writer"
    );
    let store = PackStore::open_read(&pack).unwrap();
    assert_eq!(
        store.get("a/one").unwrap().as_deref(),
        Some(&b"one again"[..])
    );
    assert_eq!(store.get("a/two").unwrap(), None);
    assert_eq!(store.get("b/three").unwrap(), None);
    assert_eq!(store.get("b/moved").unwrap().as_deref(), Some(&b"3"[..]));
    assert_eq!(store.get("c/four").unwrap().as_deref(), Some(&b"4"[..]));
    assert!(store.put("d", b"no").is_err(), "a reader does not write");
}

#[test]
fn a_torn_tail_is_ignored_and_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let good_len = {
        let store = PackStore::open(&pack).unwrap();
        store.put("kept", b"kept bytes").unwrap();
        let len = store.len().unwrap();
        store.put("torn", b"torn record bytes").unwrap();
        len
    };
    // A crash mid-record: the pack ends inside the second record's head.
    let file = std::fs::OpenOptions::new().write(true).open(&pack).unwrap();
    file.set_len(good_len + 10).unwrap();
    drop(file);
    let reader = PackStore::open_read(&pack).unwrap();
    assert_eq!(
        reader.get("kept").unwrap().as_deref(),
        Some(&b"kept bytes"[..])
    );
    assert_eq!(reader.get("torn").unwrap(), None);
    let store = PackStore::open(&pack).unwrap();
    assert_eq!(store.len().unwrap(), good_len, "a writer drops the tail");
    store.put("after", b"after").unwrap();
    assert_eq!(store.get("after").unwrap().as_deref(), Some(&b"after"[..]));
    assert_eq!(
        store.get("kept").unwrap().as_deref(),
        Some(&b"kept bytes"[..])
    );
}

#[test]
fn a_second_writer_is_refused_until_the_first_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let first = PackStore::open(&pack).unwrap();
    let second = PackStore::open(&pack);
    assert_eq!(
        second.err().map(|e| e.kind()),
        Some(std::io::ErrorKind::WouldBlock)
    );
    drop(first);
    assert!(PackStore::open(&pack).is_ok());
    // A lock a crash left is broken by hand.
    std::fs::write(dir.path().join("pack.lock"), "999999\n").unwrap();
    assert!(matches!(PackStore::open(&pack), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    assert!(PackStore::break_lock(&pack).unwrap());
    assert!(PackStore::open(&pack).is_ok());
}

#[test]
fn a_reader_sees_what_the_writer_appends_and_a_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let writer = PackStore::open(&pack).unwrap();
    writer.put("x", b"first").unwrap();
    let reader = PackStore::open_read(&pack).unwrap();
    assert_eq!(reader.get("x").unwrap().as_deref(), Some(&b"first"[..]));
    writer.put("x", b"second").unwrap();
    writer.put("y", b"y").unwrap();
    assert_eq!(reader.get("x").unwrap().as_deref(), Some(&b"second"[..]));
    assert_eq!(
        reader.list("").unwrap(),
        vec!["x".to_string(), "y".to_string()]
    );
    for i in 0..50 {
        writer.put("x", format!("version {i}").as_bytes()).unwrap();
    }
    let before = writer.len().unwrap();
    let done = writer.compact().unwrap();
    assert_eq!((done.before, done.paths), (before, 2));
    assert!(done.after < before / 4, "{done:?}");
    assert_eq!(writer.len().unwrap(), done.after);
    assert_eq!(
        writer.get("x").unwrap().as_deref(),
        Some(&b"version 49"[..])
    );
    assert_eq!(
        reader.get("x").unwrap().as_deref(),
        Some(&b"version 49"[..])
    );
    assert_eq!(reader.get("y").unwrap().as_deref(), Some(&b"y"[..]));
    writer.put("z", b"z").unwrap();
    assert_eq!(reader.get("z").unwrap().as_deref(), Some(&b"z"[..]));
}

#[test]
fn a_record_that_is_not_a_pack_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    std::fs::write(&pack, b"not a pack at all\n").unwrap();
    assert!(PackStore::open(&pack).is_err());
    assert!(PackStore::open_read(&pack).is_err());
    let mut file = std::fs::OpenOptions::new().write(true).open(&pack).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(PACK_MAGIC).unwrap();
    file.set_len(PACK_MAGIC.len() as u64).unwrap();
    drop(file);
    assert!(PackStore::open_read(&pack).unwrap().is_empty().unwrap());
}

#[test]
fn a_log_over_a_pack_costs_a_constant_number_of_inodes() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let store = PackStore::open(&pack).unwrap();
    let raw: usize = (0..1000).map(|i| entry(i).len()).sum();
    {
        let mut writer = Writer::open(&store, no_keys()).unwrap();
        for i in 0..1000 {
            assert_eq!(writer.append(&entry(i)).unwrap(), i);
        }
        assert!(store.exists(TREE_STATE_LOCK).unwrap());
        assert_eq!(writer.size(), 1000);
    }
    assert!(!store.exists(TREE_STATE_LOCK).unwrap());
    // Three full bundles and the one partial of width 232; the narrower
    // partials the tail passed through are gone, at level 0 as well.
    assert_eq!(store.list("tile/entries").unwrap().len(), 4);
    assert_eq!(store.list("tile/0").unwrap().len(), 4);
    assert_eq!(store.list("tile/1").unwrap().len(), 1);
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.len(), 2, "pack and lock only: {files:?}");
    let reader = Reader::open(&store, no_keys()).unwrap();
    let root = reader.hashes().root().unwrap();
    let leaves: Vec<[u8; 32]> = (0..1000).map(|i| rfc9162::leaf_hash(&entry(i))).collect();
    assert_eq!(root, rfc9162::root(&leaves));
    let bundle = reader.bundle(3).unwrap();
    assert_eq!(bundle.len(), 232);
    assert_eq!(bundle[0], entry(768));
    // Appending one entry at a time rewrites the partial bundle and the
    // partial tiles on every append, so the history is many times the
    // entries until it is compacted away; append_batch writes each once.
    let packed = store.len().unwrap();
    let live = store.live_bytes().unwrap();
    assert!(live < packed / 4, "live {live} of {packed}");
    let done = store.compact().unwrap();
    assert_eq!(done.after, live);
    // The JSON lines shrink under DEFLATE; the hash tiles do not, and the
    // compacted pack is still smaller than the entries alone.
    assert!(
        done.after < raw as u64,
        "pack {} bytes holds {raw} bytes of entries plus tiles and state",
        done.after
    );
    let reader = Reader::open(&store, no_keys()).unwrap();
    assert_eq!(reader.hashes().root().unwrap(), root);
    assert_eq!(reader.bundle(0).unwrap()[5], entry(5));
}

#[test]
fn shared_writers_see_each_others_changes() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let a = PackStore::open_shared(&pack).unwrap();
    let b = PackStore::open_shared(&pack).unwrap();
    a.put("x", b"from a").unwrap();
    b.put("y", b"from b").unwrap();
    assert_eq!(b.get("x").unwrap().as_deref(), Some(&b"from a"[..]));
    assert_eq!(a.get("y").unwrap().as_deref(), Some(&b"from b"[..]));
    b.rename("x", "z").unwrap();
    assert_eq!(a.get("x").unwrap(), None);
    assert_eq!(a.get("z").unwrap().as_deref(), Some(&b"from a"[..]));
    assert!(a.delete("y").unwrap());
    assert!(!b.exists("y").unwrap());
    // No lock is held between changes.
    assert!(!dir.path().join("pack.lock").exists());
}

#[test]
fn concurrent_shared_writers_lose_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let threads: Vec<_> = (0..4)
        .map(|w| {
            let pack = pack.clone();
            std::thread::spawn(move || {
                let store = PackStore::open_shared(&pack).unwrap();
                for i in 0..200 {
                    let body = format!("writer {w} entry {i} {}", "x".repeat(150));
                    store
                        .put(&format!("w{w}/e{i:03}"), body.as_bytes())
                        .unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let reader = PackStore::open_read(&pack).unwrap();
    for w in 0..4 {
        let paths = reader.list(&format!("w{w}")).unwrap();
        assert_eq!(paths.len(), 200, "writer {w}");
        let body = reader.get(&format!("w{w}/e199")).unwrap().unwrap();
        assert!(body.starts_with(format!("writer {w} entry 199 ").as_bytes()));
    }
}

#[test]
fn only_one_shared_writer_creates_a_path() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let created: usize = (0..4)
        .map(|_| {
            let pack = pack.clone();
            std::thread::spawn(move || {
                let store = PackStore::open_shared(&pack).unwrap();
                store.create_new("once", b"first").unwrap() as usize
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|t| t.join().unwrap())
        .sum();
    assert_eq!(created, 1);
}

#[test]
fn a_shared_writer_waits_for_an_exclusive_one_and_then_gives_up() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let exclusive = PackStore::open(&pack).unwrap();
    exclusive.put("a", b"a").unwrap();
    let wait = std::time::Duration::from_millis(100);
    let err = PackStore::open_shared_waiting(&pack, wait).err().unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    drop(exclusive);
    let shared = PackStore::open_shared_waiting(&pack, wait).unwrap();
    assert_eq!(shared.get("a").unwrap().as_deref(), Some(&b"a"[..]));
    shared.put("b", b"b").unwrap();
}

#[test]
fn a_shared_writer_drops_a_crashed_writers_torn_tail() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let a = PackStore::open_shared(&pack).unwrap();
    a.put("kept", b"kept").unwrap();
    // A crashed writer leaves half a record after the last good one.
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&pack)
        .unwrap();
    file.write_all(&[1, 0, 0, 4, 0, 0, 0, 9]).unwrap();
    drop(file);
    let b = PackStore::open_shared(&pack).unwrap();
    b.put("after", b"after").unwrap();
    let reader = PackStore::open_read(&pack).unwrap();
    assert_eq!(reader.get("kept").unwrap().as_deref(), Some(&b"kept"[..]));
    assert_eq!(reader.get("after").unwrap().as_deref(), Some(&b"after"[..]));
}

#[test]
fn a_compaction_by_one_shared_writer_is_followed_by_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    let a = PackStore::open_shared(&pack).unwrap();
    let b = PackStore::open_shared(&pack).unwrap();
    for i in 0..50 {
        a.put(
            "same",
            format!("version {i} {}", "y".repeat(200)).as_bytes(),
        )
        .unwrap();
    }
    b.put("other", b"other").unwrap();
    let before = std::fs::metadata(&pack).unwrap().len();
    let done = a.compact().unwrap();
    assert!(done.after < before);
    b.put("late", b"late").unwrap();
    let reader = PackStore::open_read(&pack).unwrap();
    assert!(reader
        .get("same")
        .unwrap()
        .unwrap()
        .starts_with(b"version 49 "));
    assert_eq!(reader.get("other").unwrap().as_deref(), Some(&b"other"[..]));
    assert_eq!(reader.get("late").unwrap().as_deref(), Some(&b"late"[..]));
}
