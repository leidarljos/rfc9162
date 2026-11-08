//! The store against the contract: proofs on every answer, logged manifests,
//! compaction that keeps replaced runs provable, tamper detection, recovery.

use std::sync::Arc;

use rfc9162::{hex, leaf_hash, verify_inclusion};
use rfc9162_log::{AuthenticatedLog, Error, Inclusion, Keys, Store};
use rfc9162_tiles::{MemoryStore, PosixStore, Reader, TileStore};

/// Keys are the whitespace-separated fields of an entry.
fn fields() -> Keys {
    Arc::new(|entry: &[u8]| {
        entry
            .split(|b| *b == b' ')
            .filter(|f| !f.is_empty())
            .map(<[u8]>::to_vec)
            .collect()
    })
}

fn entry(i: u64) -> Vec<u8> {
    format!("module=m{} pkg=p{} target=t{}", i, i % 3, i % 2).into_bytes()
}

fn check(store: &Store<MemoryStore>, inc: &Inclusion, bytes: &[u8]) {
    assert_eq!(inc.tree_size, store.size());
    assert!(
        verify_inclusion(
            &leaf_hash(bytes),
            inc.index as usize,
            inc.tree_size as usize,
            &inc.path,
            &hex(&store.root()),
        ),
        "inclusion of {} fails",
        inc.index
    );
}

fn filled(n: u64) -> Store<MemoryStore> {
    let mut store = Store::open(MemoryStore::new(), fields()).expect("open");
    for i in 0..n {
        assert_eq!(store.append(&entry(i)).expect("append"), i);
    }
    store
}

#[test]
fn the_tail_is_found_by_scan_with_no_run() {
    let store = filled(5);
    let hits = store.find(b"module=m3").expect("find");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].entry.index, 3);
    assert!(hits[0].run.is_none());
    check(&store, &hits[0].entry, &entry(3));
    let by_hash = store
        .find(&rfc9162_tiles::index::identity(&entry(4)))
        .expect("find");
    assert_eq!(by_hash[0].entry.index, 4);
    assert!(store.find(b"module=m9").expect("find").is_empty());
}

#[test]
fn a_seal_logs_the_manifest_and_a_lookup_proves_entry_and_run() {
    let mut store = filled(6);
    let run = store.seal().expect("seal");
    assert_eq!(run.entries, 0..=5);
    assert_eq!(run.manifest_index, Some(6));
    assert_eq!(store.size(), 7, "the manifest is an entry of the log");
    assert_eq!(store.live_runs(), std::slice::from_ref(&run));
    assert!(run.key_lo < run.key_hi);

    let hits = store.find(b"pkg=p1").expect("find");
    assert_eq!(
        hits.iter().map(|h| h.entry.index).collect::<Vec<_>>(),
        vec![1, 4]
    );
    for hit in &hits {
        check(&store, &hit.entry, &entry(hit.entry.index));
        let run_inc = hit.run.as_ref().expect("located through a run");
        assert_eq!(run_inc.index, 6);
        let manifest = Reader::open(store.writer().store(), rfc9162_tiles::index::no_keys())
            .expect("reader")
            .get(6)
            .expect("get")
            .expect("manifest entry");
        assert!(manifest.starts_with(rfc9162_log::store::MANIFEST_MAGIC));
        check(&store, run_inc, &manifest);
    }
    assert!(store
        .find(b"pkg=p7")
        .expect("negative find is proven")
        .is_empty());
    assert!(
        matches!(store.seal(), Err(Error::Tree(_))),
        "nothing to seal"
    );
}

#[test]
fn entries_after_a_seal_come_from_the_tail_until_the_next_seal() {
    let mut store = filled(4);
    store.seal().expect("seal");
    store.append(&entry(10)).expect("append");
    let hits = store.find(b"module=m10").expect("find");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].entry.index, 5);
    assert!(hits[0].run.is_none());
    let run = store.seal().expect("second seal");
    assert_eq!(
        run.entries,
        4..=5,
        "the first manifest sits in the second run's range"
    );
    assert!(store.find(b"module=m10").expect("find")[0].run.is_some());
    assert!(store.find(b"module=m1").expect("find")[0].run.is_some());
}

#[test]
fn a_range_query_walks_runs_and_tail_in_key_order() {
    let mut store = filled(3);
    store.seal().expect("seal");
    store.append(b"module=a0 pkg=zz").expect("append");
    let hits = store
        .range(b"module=".as_slice()..=b"module=~".as_slice())
        .expect("range");
    let keys: Vec<u64> = hits.iter().map(|h| h.entry.index).collect();
    assert_eq!(keys, vec![4, 0, 1, 2], "a0 sorts before m0");
    assert!(hits[0].run.is_none());
    assert!(hits[1].run.is_some());
}

#[test]
fn compaction_merges_live_runs_and_keeps_the_replaced_ones_provable() {
    let mut store = filled(3);
    let first = store.seal().expect("seal");
    for i in 3..6 {
        store.append(&entry(i)).expect("append");
    }
    let second = store.seal().expect("seal");
    let done = store.compact().expect("compact");
    assert_eq!(
        done.replaced,
        vec![
            first.manifest_index.unwrap(),
            second.manifest_index.unwrap()
        ]
    );
    assert_eq!(store.live_runs().len(), 1);
    assert_eq!(
        store.live_runs()[0].entries,
        0..=6,
        "the second manifest sits after the merged range"
    );
    assert_eq!(store.live_runs()[0].manifest_index, Some(done.written));
    let hit = &store.find(b"module=m4").expect("find")[0];
    assert_eq!(hit.run.as_ref().unwrap().index, done.written);
    // the replaced run files are still there and still verify against the
    // manifest entries that stay in the tree
    let old = store
        .writer()
        .store()
        .get("runs/00000000000000000000-00000000000000000002")
        .expect("get");
    assert!(old.is_some());
    assert!(matches!(store.compact(), Err(Error::Tree(_))));
}

#[test]
fn a_doctored_run_file_is_refused() {
    let mut store = filled(4);
    let run = store.seal().expect("seal");
    let path = format!("runs/{:020}-{:020}", run.entries.start(), run.entries.end());
    let mut bytes = store
        .writer()
        .store()
        .get(&path)
        .expect("get")
        .expect("run file");
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    store.writer().store().put(&path, &bytes).expect("put");
    assert!(matches!(
        store.find(b"module=m1"),
        Err(Error::Tampered { what: "run file" })
    ));
    store.writer().store().delete(&path).expect("delete");
    assert!(matches!(
        store.find(b"module=m1"),
        Err(Error::Tampered { what: "run file" })
    ));
}

#[test]
fn an_entry_may_not_pose_as_a_manifest() {
    let mut store = filled(1);
    let mut fake = rfc9162_log::store::MANIFEST_MAGIC.to_vec();
    fake.extend_from_slice(b"junk");
    assert!(matches!(store.append(&fake), Err(Error::Tree(_))));
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rfc9162-log-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[test]
fn reopening_rebuilds_the_live_set_from_the_log_when_the_cache_is_gone() {
    let dir = scratch("reopen");
    let (size, root, runs) = {
        let mut store =
            Store::open(PosixStore::open(&dir).expect("posix"), fields()).expect("open");
        for i in 0..5 {
            store.append(&entry(i)).expect("append");
        }
        store.seal().expect("seal");
        store.append(&entry(5)).expect("append");
        store.seal().expect("seal");
        (store.size(), store.root(), store.live_runs().to_vec())
    };
    let posix = PosixStore::open(&dir).expect("posix");
    posix.delete("runs/live").expect("drop the cache");
    let store = Store::open(posix, fields()).expect("reopen");
    assert_eq!(store.live_runs(), runs.as_slice());
    assert_eq!((store.size(), store.root()), (size, root));
    assert!(store.find(b"module=m5").expect("find")[0].run.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn recovery_proves_the_tree_consistent_with_the_trusted_checkpoint() {
    let dir = scratch("recover");
    let (size, root) = {
        let mut store =
            Store::open(PosixStore::open(&dir).expect("posix"), fields()).expect("open");
        for i in 0..3 {
            store.append(&entry(i)).expect("append");
        }
        (store.size(), store.root())
    };
    {
        let mut store =
            Store::open(PosixStore::open(&dir).expect("posix"), fields()).expect("open");
        store.append(&entry(3)).expect("append");
        store.seal().expect("seal");
    }
    let mut store =
        Store::open_trusting(PosixStore::open(&dir).expect("posix"), fields(), size, root)
            .expect("open");
    let done = store.recover().expect("recover");
    assert_eq!(done.checkpoint_size, 3);
    assert_eq!(done.replayed, 2, "the entry and the manifest");
    assert_eq!(done.quarantined, 0);
    assert!(!done.consistency.is_empty());
    drop(store);
    let mut wrong = Store::open_trusting(
        PosixStore::open(&dir).expect("posix"),
        fields(),
        size,
        [7u8; 32],
    )
    .expect("open");
    assert!(matches!(
        wrong.recover(),
        Err(Error::Tampered { what: "tree state" })
    ));
    drop(wrong);
    let mut none = Store::open(PosixStore::open(&dir).expect("posix"), fields()).expect("open");
    assert!(matches!(none.recover(), Err(Error::Tree(_))));
    let _ = std::fs::remove_dir_all(&dir);
}
