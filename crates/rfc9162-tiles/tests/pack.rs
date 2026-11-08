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
    let mut file = std::fs::OpenOptions::new().write(true).open(&pack).unwrap();
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
