//! An indexed log: seal, find, range, compact, recover.

use std::sync::Arc;

use rfc9162_log::{AuthenticatedLog, Keys, Store};
use rfc9162_tiles::MemoryStore;

fn keys() -> Keys {
    Arc::new(|entry: &[u8]| {
        entry
            .split(|byte| *byte == b' ')
            .filter(|field| !field.is_empty())
            .map(<[u8]>::to_vec)
            .collect()
    })
}

fn proves(log: &Store<MemoryStore>, index: u64, path: &[[u8; 32]], tree_size: u64) -> bool {
    let bytes = log
        .index()
        .expect("index")
        .reader()
        .get(index)
        .expect("get")
        .expect("present");
    rfc9162::verify_inclusion(
        &rfc9162::leaf_hash(&bytes),
        usize::try_from(index).unwrap(),
        usize::try_from(tree_size).unwrap(),
        path,
        &rfc9162::hex(&log.root()),
    )
}

fn main() {
    // ANCHOR: seal
    let mut log = Store::open(MemoryStore::new(), keys()).expect("log");
    let petsc = log.append(b"module=petsc pkg=petsc").expect("append");
    let openmpi = log.append(b"module=openmpi pkg=openmpi").expect("append");
    println!("appended {petsc} and {openmpi}, size {}", log.size());
    let run = log.seal().expect("seal");
    println!(
        "sealed {}..={} manifest {}",
        run.entries.start(),
        run.entries.end(),
        run.manifest_index.expect("logged"),
    );
    // ANCHOR_END: seal

    // ANCHOR: find
    let found = log.find(b"module=petsc").expect("find");
    let hit = &found[0];
    println!(
        "find module=petsc index {} run {} inclusion {}",
        hit.entry.index,
        hit.run.is_some(),
        proves(&log, hit.entry.index, &hit.entry.path, hit.entry.tree_size)
    );
    let absent = log.find(b"module=absent").expect("find");
    println!("find module=absent -> {}", absent.len());
    let range = log
        .range(&b"module=o"[..]..=&b"module=petsc"[..])
        .expect("range");
    let indices: Vec<String> = range
        .iter()
        .map(|hit| hit.entry.index.to_string())
        .collect();
    println!(
        "range module=o..=module=petsc indices {}",
        indices.join(" ")
    );
    // ANCHOR_END: find

    // ANCHOR: compact
    log.append(b"module=fftw pkg=fftw").expect("append");
    let second = log.seal().expect("seal");
    println!("second manifest {}", second.manifest_index.expect("logged"));
    let compaction = log.compact().expect("compact");
    println!(
        "compact replaced {:?} written {}",
        compaction.replaced, compaction.written
    );
    let again = log.find(b"module=fftw").expect("find");
    println!(
        "find after compact inclusion {}",
        proves(
            &log,
            again[0].entry.index,
            &again[0].entry.path,
            again[0].entry.tree_size,
        )
    );
    // ANCHOR_END: compact

    // ANCHOR: recover
    let mut recovered =
        Store::open_trusting(MemoryStore::new(), keys(), 0, rfc9162::root(&[])).expect("trust");
    recovered.append(b"module=petsc pkg=petsc").expect("append");
    let recovery = recovered.recover().expect("recover");
    println!(
        "recover checkpoint {} replayed {} quarantined {}",
        recovery.checkpoint_size, recovery.replayed, recovery.quarantined
    );
    // ANCHOR_END: recover

    // ANCHOR: split
    let mut split =
        Store::open_split(MemoryStore::new(), MemoryStore::new(), keys()).expect("split");
    split.append(b"module=petsc pkg=petsc").expect("append");
    split.seal().expect("seal");
    println!(
        "split data size {} index size {}",
        split.size(),
        split.index_writer().expect("index log").size()
    );
    // ANCHOR_END: split
}
