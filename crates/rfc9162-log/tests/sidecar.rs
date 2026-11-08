//! A sidecar beside a project tree over one pack: puts and deletes read
//! back by path, by directory and by log range, blobs are stored once per
//! content, a snapshot signs the log, and the whole thing is two inodes.

use std::collections::BTreeMap;

use ed25519_dalek::SigningKey;
use rfc9162::{hex, leaf_hash, verify_inclusion};
use rfc9162_log::{Op, Sidecar};
use rfc9162_tiles::{Checkpoint, Note, PackStore, Signer, TileStore};

fn meta(actor: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("actor".to_string(), actor.to_string())])
}

#[test]
fn changes_read_back_by_path_directory_and_range_with_proofs() {
    let dir = tempfile::tempdir().unwrap();
    let store = PackStore::open(dir.path().join("pack")).unwrap();
    let mut sidecar = Sidecar::open(&store).unwrap();
    let a1 = sidecar
        .put("software/A/1.0/bin/a", b"a version one", meta("eb"))
        .unwrap();
    let b1 = sidecar
        .put("software/B/2.0/lib/libb.so", b"libb", meta("eb"))
        .unwrap();
    let a2 = sidecar
        .put("software/A/1.0/bin/a", b"a version two", meta("eb"))
        .unwrap();
    let a_mod = sidecar
        .put("modules/A/1.0.lua", b"module a", meta("eb"))
        .unwrap();
    let gone = sidecar
        .delete("software/B/2.0/lib/libb.so", meta("gc"))
        .unwrap();
    assert_eq!((a1, b1, a2, a_mod, gone), (0, 1, 2, 3, 4));

    // Snapshot: seal the tail into a run, sign the log.
    let signer = Signer::new("example.org/site", SigningKey::from_bytes(&[9u8; 32])).unwrap();
    let note = sidecar.snapshot(&signer, "example.org/site").unwrap();
    let checkpoint = Checkpoint::parse(&Note::parse(&note.to_text()).unwrap().text).unwrap();
    assert_eq!(
        checkpoint.size,
        sidecar.size(),
        "the checkpoint signs the manifest too"
    );
    assert_eq!(checkpoint.root, sidecar.root());
    let root = hex(&sidecar.root());

    // A path's history, each change proven against the signed root.
    let history = sidecar.history("software/A/1.0/bin/a").unwrap();
    assert_eq!(
        history.iter().map(|f| f.index).collect::<Vec<_>>(),
        vec![0, 2]
    );
    for found in &history {
        let bytes = serde_json::to_vec(&found.change).unwrap();
        let entry = &found.located.entry;
        assert!(verify_inclusion(
            &leaf_hash(&bytes),
            entry.index as usize,
            entry.tree_size as usize,
            &entry.path,
            &root
        ));
        assert!(found.located.run.is_some(), "found through a logged run");
    }
    assert_eq!(history[1].change.op, Op::Put);
    assert_eq!(history[1].change.size, Some(13));

    // Everything under a directory, in key order.
    let under_a = sidecar.under("software/A").unwrap();
    assert_eq!(
        under_a.iter().map(|f| f.index).collect::<Vec<_>>(),
        vec![0, 2]
    );
    let under_software = sidecar.under("software").unwrap();
    assert_eq!(
        under_software.iter().map(|f| f.index).collect::<Vec<_>>(),
        vec![0, 2, 1, 4],
        "A's two changes, then B's put and delete"
    );
    assert!(sidecar.under("software/C").unwrap().is_empty());
    assert_eq!(sidecar.under("").unwrap().len(), 5);

    // The tree at a size, and the diff between two.
    let at_3 = sidecar.tree_at(3).unwrap();
    assert_eq!(
        at_3.keys().cloned().collect::<Vec<_>>(),
        vec!["software/A/1.0/bin/a", "software/B/2.0/lib/libb.so"]
    );
    assert_eq!(at_3["software/A/1.0/bin/a"].0, 2);
    let now = sidecar.tree_at(sidecar.size()).unwrap();
    assert_eq!(
        now.keys().cloned().collect::<Vec<_>>(),
        vec!["modules/A/1.0.lua", "software/A/1.0/bin/a"]
    );
    let diff = sidecar.diff(3, sidecar.size()).unwrap();
    assert_eq!(
        diff.iter().map(|(i, c)| (*i, c.op)).collect::<Vec<_>>(),
        vec![(3, Op::Put), (4, Op::Delete)]
    );

    // Blobs: content-addressed, read back and checked.
    let blob = history[1].change.blob.clone().unwrap();
    assert_eq!(
        sidecar.blob(&blob).unwrap().as_deref(),
        Some(&b"a version two"[..])
    );
    assert_eq!(sidecar.blob(&"0".repeat(64)).unwrap(), None);
}

#[test]
fn a_blob_is_stored_once_per_content_and_the_sidecar_is_two_inodes() {
    let dir = tempfile::tempdir().unwrap();
    let store = PackStore::open(dir.path().join("pack")).unwrap();
    {
        let mut sidecar = Sidecar::open(&store).unwrap();
        for i in 0..40 {
            let path = format!("etc/conf.d/{i:02}.conf");
            sidecar
                .put(&path, b"the same content in every file\n", BTreeMap::new())
                .unwrap();
        }
        sidecar
            .put("etc/motd", b"welcome", BTreeMap::new())
            .unwrap();
        assert_eq!(
            store.list("blob").unwrap().len(),
            2,
            "two contents, two blobs"
        );
    }
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.len(), 2, "pack and lock: {files:?}");
    // Reopened, the history is there and a bad path is refused.
    let mut sidecar = Sidecar::open(&store).unwrap();
    assert_eq!(sidecar.size(), 41);
    assert_eq!(sidecar.tree_at(41).unwrap().len(), 41);
    assert!(sidecar.put("../escape", b"x", BTreeMap::new()).is_err());
    assert!(sidecar.put("/abs", b"x", BTreeMap::new()).is_err());
    assert!(sidecar.put("a//b", b"x", BTreeMap::new()).is_err());
}

#[test]
fn a_tampered_blob_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = PackStore::open(dir.path().join("pack")).unwrap();
    let mut sidecar = Sidecar::open(&store).unwrap();
    sidecar.put("f", b"original", BTreeMap::new()).unwrap();
    let blob = sidecar.change(0).unwrap().unwrap().blob.unwrap();
    store.put(&format!("blob/{blob}"), b"replaced").unwrap();
    assert!(matches!(
        sidecar.blob(&blob),
        Err(rfc9162_log::Error::Tampered { what: "blob" })
    ));
}
