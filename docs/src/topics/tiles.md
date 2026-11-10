# Tiles, bundles and packs

`rfc9162-tiles` stores the [Merkle tree](merkle-tree.md) in the [C2SP tlog-tiles](https://c2sp.org/tlog-tiles) layout. A tlog-tiles client can read the hash tiles and the entry bundles. The root over the tiles is `rfc9162::root` over the same leaves, at every size. Proofs assembled from the tiles are the bytes the flat functions return. The [tutorial](../tutorials/build-a-log.md) compares them.

The index sidecars and the manifest are this crate's addition. A client that only reads tlog-tiles skips those paths.

## Paths

```text
tile/<L>/<N>[.p/<W>]      hash tiles
tile/entries/<N>[.p/<W>]  entry bundles
tile/index/<N>            key sidecar of sealed bundle N
manifest                  JSON rows for sealed bundles
checkpoint                signed note
.state/treeState          size and root, written last
.state/treeState.lock     writer lock
.state/publish.lock       held while a checkpoint is published
.state/torn-<utc>/        files open found past the tree state
```

`<N>` is the index in 3-digit groups, every group but the last prefixed with `x`. Index 1234067 is `x001/x234/067`. A full tile omits `.p/<W>`. A partial tile of width W is `<N>.p/<W>`.

A hash tile holds up to 256 hashes. Level 0 holds leaf hashes. Hash `i` of tile `n` at level `l` is the RFC 9162 root over the leaves in that range. A full tile has 256 hashes. A partial tile has 1 to 255. The level above is built from full tiles. An entry bundle holds the same indexes as the level-0 tile: big-endian `u16` lengths followed by the entry bytes. An entry longer than 65535 bytes is refused.

A tree of 70000 entries is 273 full level-0 tiles, a partial of width 112, one full level-1 tile, a partial of width 17, and a partial level-2 tile of width 1. `Layout` computes those widths, and `docs/examples/tests/shape.rs` checks them.

The right edge of the tree keeps one partial per level: the widest. Narrower partials of that tile are removed on each write. When the tile fills, the partial goes away and the full tile remains.

## Write order

A write is a temporary file, an fsync and a rename. The order is entry bundles, hash tiles, sidecars for any bundle that just filled, the manifest, then `.state/treeState`. A reader that sees the tree state sees the tiles that agree with it. On open, a writer moves files that sit past the tree state into `.state/torn-<utc>/` and reports them from `Writer::quarantined`.

`Writer` holds `.state/treeState.lock` from open until drop. The lock file's body is the process id. A lock left by a crashed writer is removed by the caller. `PackStore::break_lock` removes a pack lock left the same way.

## Keys

Every entry's identity key is `SHA-256(entry)`. The key function passed to `Writer::open` may add more keys. A sealed bundle gets `tile/index/<N>`: a split-block Bloom filter at 10 bits per key, then the sorted `(key, offset)` pairs. The manifest row stores the bundle index, the key range and the sidecar's SHA-256. `Reader::find` uses the ranges and the filter, then checks each candidate against the entry bytes. The unsealed tail is scanned. A missing sidecar is rebuilt from its bundle to the same bytes.

With fewer than 256 entries there is no sealed bundle, so the directory from the [tiles how-to](../howto/rfc9162-tiles.md) contains the tree state, one partial hash tile and one partial bundle.

## One file

`PackStore` stores the same paths as records in one append-only file. Each record is a path, its bytes, and a SHA-256 trailer, so a torn tail is ignored. Bodies that shrink under DEFLATE are stored compressed. Entry bundles shrink. Hash tiles stay the same size. `compact` writes a fresh pack with one record per live path and the live table at the front, then renames it over the old file. The tree and the proofs are over entry bytes, so compaction leaves them unchanged.

`PackStore::open` holds `<pack>.lock` for the life of the value. `open_shared` holds it only during each change, and `open_shared_waiting` sets that wait. `open_read` takes no lock. The [tiles how-to](../howto/rfc9162-tiles.md) compacts a two-entry pack from 888 bytes to 532.
