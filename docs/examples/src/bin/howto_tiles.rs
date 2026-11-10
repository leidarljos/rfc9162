//! A directory log: append, read, prove, look up by the identity hash.

use rfc9162_tiles::index::{identity, no_keys};
use rfc9162_tiles::{PosixStore, Reader, TileStore, Writer};

fn main() {
    // ANCHOR: write
    let dir = tempfile::tempdir().expect("temp dir");
    let store = PosixStore::open(dir.path()).expect("store");
    let mut writer = Writer::open(store.clone(), no_keys()).expect("writer");
    let first = writer.append(b"first record").expect("append");
    let second = writer.append(b"second record").expect("append");
    println!("appended {first} and {second}");
    println!(
        "size {} root {}",
        writer.size(),
        rfc9162::hex(&writer.root())
    );
    drop(writer);
    // ANCHOR_END: write

    // ANCHOR: read
    let reader = Reader::open(store.clone(), no_keys()).expect("reader");
    let record = reader.get(first).expect("get").expect("present");
    println!(
        "get {first} = {}",
        String::from_utf8(record).expect("utf-8")
    );
    let proof = reader
        .inclusion(second)
        .expect("inclusion")
        .expect("in tree");
    println!(
        "inclusion {second} {}",
        rfc9162::verify_inclusion(
            &rfc9162::leaf_hash(b"second record"),
            usize::try_from(second).unwrap(),
            usize::try_from(reader.size()).unwrap(),
            &proof,
            &rfc9162::hex(&reader.root()),
        )
    );
    // ANCHOR_END: read

    // ANCHOR: find
    let hits = reader.find(&identity(b"first record")).expect("find");
    println!("find identity of first record -> {hits:?}");
    let missing = reader.find(&identity(b"absent")).expect("find");
    println!("find identity of absent -> {missing:?}");
    // ANCHOR_END: find

    // ANCHOR: files
    let mut files = store.list("").expect("list");
    files.retain(|path| !path.ends_with(".lock"));
    println!("files");
    for path in files {
        println!("  {path}");
    }
    // ANCHOR_END: files
}
