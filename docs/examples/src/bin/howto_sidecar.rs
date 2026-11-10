//! Library sidecar: put, snapshot, history, sync a directory, include a child.

use std::collections::BTreeMap;

use ed25519_dalek::SigningKey;
use rfc9162_log::Sidecar;
use rfc9162_tiles::{MemoryStore, Signer};

fn signer(origin: &str, seed: u8) -> Signer {
    Signer::new(origin, SigningKey::from_bytes(&[seed; 32])).expect("signer")
}

fn meta(actor: &str) -> BTreeMap<String, String> {
    let mut meta = BTreeMap::new();
    meta.insert("actor".to_string(), actor.to_string());
    meta
}

fn main() {
    // ANCHOR: put
    let mut sidecar = Sidecar::open(MemoryStore::new()).expect("sidecar");
    let index = sidecar
        .put("ssh/sshd_config", b"Port 22\n", meta("root"))
        .expect("put");
    println!("put {index}");
    let child_signer = signer("example.org/demo", 7);
    let note = sidecar
        .snapshot(&child_signer, "example.org/demo")
        .expect("snapshot");
    println!("snapshot bytes {}", note.to_text().len());
    let found = sidecar.history("ssh/sshd_config").expect("history");
    let item = &found[0];
    let bytes = sidecar
        .log()
        .index()
        .expect("index")
        .reader()
        .get(item.index)
        .expect("get")
        .expect("present");
    println!("entry {}", String::from_utf8(bytes.clone()).expect("utf-8"));
    println!(
        "history {} inclusion {} run {}",
        item.change.path,
        rfc9162::verify_inclusion(
            &rfc9162::leaf_hash(&bytes),
            usize::try_from(item.index).unwrap(),
            usize::try_from(item.located.entry.tree_size).unwrap(),
            &item.located.entry.path,
            &rfc9162::hex(&sidecar.root()),
        ),
        item.located.run.is_some()
    );
    let blob = sidecar
        .blob(item.change.blob.as_deref().expect("blob"))
        .expect("blob")
        .expect("stored");
    println!("blob {}", String::from_utf8(blob).expect("utf-8"));
    // ANCHOR_END: put

    // ANCHOR: sync
    let root = tempfile::tempdir().expect("temp dir");
    let project = root.path().join("project");
    std::fs::create_dir_all(project.join("ssh")).expect("mkdir");
    std::fs::write(project.join("ssh/sshd_config"), b"Port 22\n").expect("write");
    std::fs::write(project.join("motd"), b"hello\n").expect("write");
    let mut synced_log = Sidecar::open(MemoryStore::new()).expect("sidecar");
    let synced = synced_log
        .sync_dir(&project, &[], &meta("root"))
        .expect("sync");
    println!(
        "sync added {} changed {} deleted {} unchanged {}",
        synced.added.len(),
        synced.changed.len(),
        synced.deleted.len(),
        synced.unchanged
    );
    std::fs::write(project.join("ssh/sshd_config"), b"Port 2222\n").expect("rewrite");
    std::fs::remove_file(project.join("motd")).expect("remove");
    let again = synced_log
        .sync_dir(&project, &[], &meta("root"))
        .expect("sync");
    println!(
        "resync added {} changed {} deleted {}",
        again.added.len(),
        again.changed.len(),
        again.deleted.len()
    );
    let tree = synced_log.tree_at(synced_log.size()).expect("tree");
    println!("tree");
    for (path, (index, change)) in &tree {
        println!(
            "  {path} index {index} bytes {}",
            change.size.expect("size")
        );
    }
    // ANCHOR_END: sync

    // ANCHOR: include
    let mut child = Sidecar::open(MemoryStore::new()).expect("child");
    child
        .put("modules/petsc.lua", b"root = '/opt/petsc'\n", meta("stack"))
        .expect("put");
    let child_note = child
        .snapshot(&signer("example.org/stack", 8), "example.org/stack")
        .expect("child snapshot");
    let mut parent = Sidecar::open(MemoryStore::new()).expect("parent");
    let included = parent
        .include_checkpoint(
            "stacks/2026",
            &child_note,
            &[signer("example.org/stack", 8).verifier()],
            meta("site"),
        )
        .expect("include");
    println!("included stacks/2026 at {included}");
    let checkpoints = parent.checkpoints("stacks/2026").expect("checkpoints");
    println!(
        "child origin {} size {}",
        checkpoints[0].1.origin, checkpoints[0].1.size
    );
    // ANCHOR_END: include
}
