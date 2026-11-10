//! One pack file for the same tile log, then a compaction of that file.

use rfc9162_tiles::index::no_keys;
use rfc9162_tiles::{PackStore, Reader, Writer};

fn main() {
    // ANCHOR: pack
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("log.pack");
    let store = PackStore::open(&path).expect("pack");
    {
        let mut writer = Writer::open(&store, no_keys()).expect("writer");
        writer.append(b"one").expect("append");
        writer.append(b"two").expect("append");
        println!(
            "size {} root {}",
            writer.size(),
            rfc9162::hex(&writer.root())
        );
    }
    let reader = Reader::open(&store, no_keys()).expect("reader");
    let second = reader.get(1).expect("get").expect("present");
    println!("get 1 = {}", String::from_utf8(second).expect("utf-8"));
    println!("pack bytes {}", store.len().expect("len"));
    // ANCHOR_END: pack

    // ANCHOR: compact
    let compacted = store.compact().expect("compact");
    println!(
        "compact before {} after {} paths {}",
        compacted.before, compacted.after, compacted.paths
    );
    let shared_path = dir.path().join("shared.pack");
    let shared = PackStore::open_shared(&shared_path).expect("shared");
    let mut writer = Writer::open(&shared, no_keys()).expect("writer");
    let index = writer.append(b"shared").expect("append");
    println!("shared append {index}");
    // ANCHOR_END: compact
}
