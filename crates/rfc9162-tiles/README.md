# rfc9162-tiles

Tiled, indexed storage for an [rfc9162](https://crates.io/crates/rfc9162) log, laid out as
[C2SP tlog-tiles](https://c2sp.org/tlog-tiles): hash tiles of 256 hashes, entry bundles of 256
length-prefixed entries, partial tiles for the unsealed tail, and the tree `rfc9162` hashes. Any
tlog-tiles client reads the tree. The root over the tiles is `rfc9162::root` over the same leaves at
every size, and a proof assembled from tiles is the bytes `rfc9162::inclusion_proof` and
`rfc9162::consistency_proof` give over the flat leaf slice.

```rust
use rfc9162_tiles::index::no_keys;
use rfc9162_tiles::{PosixStore, Reader, Writer};

let store = PosixStore::open("/var/lib/example-log")?;
let mut writer = Writer::open(store.clone(), no_keys())?;
let index = writer.append(b"first record")?;
drop(writer); // releases .state/treeState.lock

let reader = Reader::open(store, no_keys())?;
assert_eq!(reader.get(index)?, Some(b"first record".to_vec()));
let proof = reader.inclusion(index)?.unwrap();
assert!(rfc9162::verify_inclusion(
    &rfc9162::leaf_hash(b"first record"), index as usize, reader.size() as usize,
    &proof, &rfc9162::hex(&reader.root())));
# Ok::<(), rfc9162_tiles::Error>(())
```

## Layout

```
tile/<L>/<N>[.p/<W>]      hash tiles, N as x-prefixed 3-digit elements (1234067 -> x001/x234/067)
tile/entries/<N>[.p/<W>]  entry bundles, big-endian uint16 length-prefixed entries
tile/index/<N>            key sidecar of sealed bundle N: Bloom filter and sorted (key, offset) pairs
manifest                  JSON: per sealed bundle its index, key range and sidecar hash
checkpoint                C2SP checkpoint, an Ed25519 signed note
.state/treeState          size and hex root the tiles are complete to, written last
.state/treeState.lock     the writer lock; .state/publish.lock guards the checkpoint
.state/torn-<utc>/        files open found beyond the tree state
```

A write is a temporary file, fsync and rename. The order is bundles, hash tiles, sidecars, manifest,
tree state. Partial tiles are removed once their full tile exists. A tree of 70000 entries is 273
full level-0 tiles, a partial of width 112, one full level-1 tile, a partial of width 17 and a
partial level-2 tile of width 1.

## One file instead of a tree

`PackStore` keeps the same paths as records in one append-only file, so a
log costs a constant number of inodes however long it grows: on a shared
filesystem with an inode quota, a tree of 10^6 entries is about 12,000
files as a directory and one as a pack. Each record ends in a SHA-256, so
a torn tail is found and ignored; bodies that shrink under DEFLATE (entry
bundles do, hash tiles do not) are stored compressed; `compact` rewrites
the pack with one record per live path and renames it over the old one.
The tree and every proof are untouched, since they are over entry bytes.
One writer holds `<pack>.lock` and removes it on drop; a crash leaves it
and `PackStore::break_lock` removes it. Readers open the pack without a
lock, see a writer's appends, and reopen after a compaction.

```rust
let store = rfc9162_tiles::PackStore::open(dir.join("pack"))?;
let mut writer = rfc9162_tiles::Writer::open(&store, rfc9162_tiles::index::no_keys())?;
```

## Keys

`Writer::open` and `Reader::open` take a key function, `Fn(&[u8]) -> Vec<Vec<u8>>`, that derives the
lookup keys of an entry from its bytes; `SHA-256(entry)` is always a key. `Reader::find(key)` returns
the indices of every entry carrying the key, through the manifest ranges, the filters and the
sidecars of sealed bundles, then a scan of the unsealed tail. Sidecars are derivable: open rebuilds a
missing one from its bundle to the same bytes.

## Checkpoints

`Writer::checkpoint(&signer, origin)` signs the tree state as a C2SP checkpoint under
`.state/publish.lock` and refuses to replace a published checkpoint for a larger tree. `Signer` reads
a PKCS#8 PEM Ed25519 key; the key id is `SHA-256(name || 0x0A || 0x01 || public key)[:4]`.
`Reader::open_checkpoint(store, origin, &trusted, keys)` opens a log at a checkpoint a trusted key
signed. `checkpoint::verify_cosignatures` checks witness cosignature/v1 lines (type byte `0x04`,
eight-byte big-endian timestamp, Ed25519 over `cosignature/v1`, `time <t>` and the checkpoint lines).

Released with [rfc9162](https://github.com/leidarljos/rfc9162) at the same version; the publish
workflow publishes `rfc9162` before this crate. [deedar](https://github.com/leidarljos/deedar) builds
its deed log on both. MIT.
