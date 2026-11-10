//! Build a directory log, check proofs, sign a checkpoint, then index a log.

use std::sync::Arc;

use ed25519_dalek::SigningKey;
use rfc9162_log::{AuthenticatedLog, Store};
use rfc9162_tiles::index::no_keys;
use rfc9162_tiles::{MemoryStore, PosixStore, Reader, Signer, TileStore, Writer};

fn main() {
    // ANCHOR: store
    let dir = tempfile::tempdir().expect("temp dir");
    let store = PosixStore::open(dir.path()).expect("store");
    let mut writer = Writer::open(store.clone(), no_keys()).expect("writer");
    for record in ["alpha", "beta", "gamma"] {
        let index = writer.append(record.as_bytes()).expect("append");
        println!("append {index} {record}");
    }
    let early_size = writer.size();
    let early_root = rfc9162::hex(&writer.root());
    println!("stored size {early_size}");
    println!("stored root {early_root}");
    drop(writer);

    let mut files = store.list("").expect("list");
    files.retain(|path| !path.ends_with(".lock"));
    println!("files");
    for path in &files {
        println!("  {path}");
    }
    // ANCHOR_END: store

    // ANCHOR: read
    let reader = Reader::open(store.clone(), no_keys()).expect("reader");
    let entry = reader.get(1).expect("get").expect("present");
    println!("get 1 = {}", String::from_utf8(entry).expect("utf-8"));
    let proof = reader.inclusion(1).expect("inclusion").expect("in tree");
    let leaves: Vec<[u8; 32]> = ["alpha", "beta", "gamma"]
        .iter()
        .map(|record| rfc9162::leaf_hash(record.as_bytes()))
        .collect();
    let flat = rfc9162::inclusion_proof(&leaves, 1).expect("flat proof");
    println!("tile proof matches flat proof {}", proof == flat);
    println!(
        "verify inclusion {}",
        rfc9162::verify_inclusion(
            &rfc9162::leaf_hash(b"beta"),
            1,
            usize::try_from(reader.size()).unwrap(),
            &proof,
            &rfc9162::hex(&reader.root()),
        )
    );
    // ANCHOR_END: read

    // ANCHOR: grow
    let mut writer = Writer::open(store.clone(), no_keys()).expect("writer");
    let index = writer.append(b"delta").expect("append");
    println!("append {index} delta");
    drop(writer);
    let reader = Reader::open(store.clone(), no_keys()).expect("reader");
    let consistency = reader
        .consistency(early_size)
        .expect("consistency")
        .expect("prefix");
    println!(
        "verify consistency {}",
        rfc9162::verify_consistency(
            usize::try_from(early_size).unwrap(),
            &early_root,
            usize::try_from(reader.size()).unwrap(),
            &rfc9162::hex(&reader.root()),
            &consistency,
        )
    );
    // ANCHOR_END: grow

    // ANCHOR: checkpoint
    let signer =
        Signer::new("example.org/log", SigningKey::from_bytes(&[7u8; 32])).expect("signer");
    let writer = Writer::open(store.clone(), no_keys()).expect("writer");
    let note = writer
        .checkpoint(&signer, "example.org/log")
        .expect("checkpoint");
    println!("checkpoint");
    print!("{}", note.to_text());
    drop(writer);
    let trusted = [signer.verifier()];
    let opened =
        Reader::open_checkpoint(store, "example.org/log", &trusted, no_keys()).expect("open");
    println!("opened at checkpoint size {}", opened.size());
    let again = opened.inclusion(1).expect("inclusion").expect("in tree");
    println!(
        "inclusion at checkpoint {}",
        rfc9162::verify_inclusion(
            &rfc9162::leaf_hash(b"beta"),
            1,
            usize::try_from(opened.size()).unwrap(),
            &again,
            &rfc9162::hex(&opened.root()),
        )
    );
    // ANCHOR_END: checkpoint

    // ANCHOR: index
    let mut log = Store::open(
        MemoryStore::new(),
        Arc::new(|entry: &[u8]| {
            entry
                .split(|byte| *byte == b' ')
                .filter(|field| !field.is_empty())
                .map(<[u8]>::to_vec)
                .collect()
        }),
    )
    .expect("log");
    log.append(b"module=petsc pkg=petsc").expect("append");
    log.append(b"module=openmpi pkg=openmpi").expect("append");
    let run = log.seal().expect("seal");
    println!(
        "sealed {}..={} manifest {}",
        run.entries.start(),
        run.entries.end(),
        run.manifest_index.expect("logged"),
    );
    let found = log.find(b"module=petsc").expect("find");
    let hit = &found[0];
    let bytes = log
        .index()
        .expect("index")
        .reader()
        .get(hit.entry.index)
        .expect("get")
        .expect("present");
    println!(
        "find module=petsc index {} run {} inclusion {}",
        hit.entry.index,
        hit.run.is_some(),
        rfc9162::verify_inclusion(
            &rfc9162::leaf_hash(&bytes),
            usize::try_from(hit.entry.index).unwrap(),
            usize::try_from(hit.entry.tree_size).unwrap(),
            &hit.entry.path,
            &rfc9162::hex(&log.root()),
        )
    );
    // ANCHOR_END: index
}
