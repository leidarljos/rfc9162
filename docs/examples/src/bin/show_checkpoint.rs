//! Prints the checkpoint note stored in a pack.

use rfc9162_tiles::layout::CHECKPOINT;
use rfc9162_tiles::{PackStore, TileStore};

fn main() {
    let path = std::env::args().nth(1).expect("pack path");
    let store = PackStore::open_read(path).expect("open pack");
    let bytes = store
        .get(CHECKPOINT)
        .expect("read")
        .expect("checkpoint record");
    print!("{}", String::from_utf8(bytes).expect("utf-8"));
}
