# rfc9162-tiles

Tiled storage for an [`rfc9162`](https://docs.rs/rfc9162) log, laid out as [C2SP tlog-tiles](https://c2sp.org/tlog-tiles): hash tiles of 256 hashes, entry bundles of 256 length-prefixed entries, partial tiles for the unsealed tail, and the tree `rfc9162` hashes. A proof assembled from tiles is the byte string `rfc9162::inclusion_proof` and `rfc9162::consistency_proof` return over the flat leaf slice.

The repository guide has the [tasks](../../docs/src/howto/rfc9162-tiles.md), the [layout](../../docs/src/topics/tiles.md) and the [checkpoint format](../../docs/src/topics/checkpoints.md).

```rust
use rfc9162_tiles::index::no_keys;
use rfc9162_tiles::{MemoryStore, Reader, Writer};

let store = MemoryStore::new();
let mut writer = Writer::open(store.clone(), no_keys())?;
let index = writer.append(b"first record")?;
drop(writer);

let reader = Reader::open(store, no_keys())?;
assert_eq!(reader.get(index)?, Some(b"first record".to_vec()));
let proof = reader.inclusion(index)?.unwrap();
assert!(rfc9162::verify_inclusion(
    &rfc9162::leaf_hash(b"first record"),
    index as usize,
    reader.size() as usize,
    &proof,
    &rfc9162::hex(&reader.root()),
));
# Ok::<(), rfc9162_tiles::Error>(())
```

## Layout

```text
tile/<L>/<N>[.p/<W>]      hash tiles, N as x-prefixed 3-digit elements (1234067 -> x001/x234/067)
tile/entries/<N>[.p/<W>]  entry bundles, big-endian u16 length-prefixed entries
tile/index/<N>            key sidecar of sealed bundle N: Bloom filter and sorted (key, offset) pairs
manifest                  JSON rows: bundle index, key range, sidecar hash
checkpoint                C2SP checkpoint, an Ed25519 signed note
.state/treeState          size and hex root the tiles are complete to, written last
.state/treeState.lock     the writer lock; .state/publish.lock guards the checkpoint
.state/torn-<utc>/        files open found beyond the tree state
```

A write is a temporary file, fsync and rename. The order is bundles, hash tiles, sidecars, manifest, tree state. A tail tile keeps the widest partial, the one the tree state names. A tree of 70000 entries is 273 full level-0 tiles, a partial of width 112, one full level-1 tile, a partial of width 17 and a partial level-2 tile of width 1.

## One file

`PackStore` keeps those paths as records in one append-only file. Each record ends in a SHA-256, so a torn tail is ignored. Bodies that shrink under DEFLATE are stored compressed. `compact` rewrites the pack with one record per live path. `open` holds `<pack>.lock` until drop. `open_shared` holds it for each change. `open_read` takes no lock.

## Keys and checkpoints

`Writer::open` takes a key function, `Fn(&[u8]) -> Vec<Vec<u8>>`. `SHA-256(entry)` is always a key. `Reader::find` returns the indexes of entries that carry a key, through the manifest, the filters and the sidecars, then a scan of the unsealed tail.

`Writer::checkpoint` signs the tree state as a C2SP checkpoint. `Signer` reads a PKCS#8 PEM Ed25519 key. The key id is `SHA-256(name || 0x0A || 0x01 || public key)[:4]`. `Reader::open_checkpoint` opens a log at a checkpoint a trusted key signed. `checkpoint::verify_cosignatures` checks witness cosignature/v1 lines.

Released with [rfc9162](https://github.com/leidarljos/rfc9162) at the same version. MIT.
