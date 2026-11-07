# rfc9162

Two Rust crates for an append-only Merkle log hashed as RFC 9162
(Certificate Transparency 2.0, doi:10.17487/RFC9162).

| Crate | Carries |
|---|---|
| [`rfc9162`](crates/rfc9162) | the tree over 32-byte leaf hashes: root, inclusion and consistency proofs, the checks for both, and the `Head` a log signs. It stores nothing and serialises nothing. |
| [`rfc9162-tiles`](crates/rfc9162-tiles) | tiled, indexed storage for such a log in the C2SP tlog-tiles layout: hash tiles, entry bundles, per-bundle key sidecars with a Bloom filter, a manifest for `find`, and C2SP checkpoints as Ed25519 signed notes. |

The root over the tiles is `rfc9162::root` over the same leaves at every size, and a proof
assembled from tiles is the bytes the flat crate gives over the leaf slice. Any tlog-tiles client
reads the tree; the index sidecars are this repository's addition and tlog clients ignore them.

## Install

```
cargo add rfc9162
cargo add rfc9162-tiles
```

Both crates carry one version and release together. `rfc9162-tiles` requires the `rfc9162` of
the same release.

## The tree

```rust
use rfc9162::{inclusion_proof, leaf_hash, verify_inclusion, Head};

let leaves: Vec<[u8; 32]> = ["a", "b", "c"].iter().map(|r| leaf_hash(r.as_bytes())).collect();
let head = Head::over(&leaves);
let path = inclusion_proof(&leaves, 1).unwrap();
assert!(verify_inclusion(&leaves[1], 1, head.size, &path, &head.root));
```

A user computes each leaf from the canonical bytes of its own record, keeps its own file format,
and signs `Head::material(domain)` with a domain that names its log.

## The tiles

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

The crate README has the on-disk layout, the key functions behind `Reader::find`, and the
checkpoint and cosignature checks.

## Specifications

- RFC 9162, Certificate Transparency Version 2.0: <https://www.rfc-editor.org/rfc/rfc9162>
- C2SP tlog-tiles, the tile and entry-bundle layout: <https://c2sp.org/tlog-tiles>
- C2SP tlog-checkpoint, the checkpoint body: <https://c2sp.org/tlog-checkpoint>
- C2SP signed-note, the signature envelope and key ids: <https://c2sp.org/signed-note>
- C2SP tlog-cosignature, witness cosignature/v1 lines: <https://c2sp.org/tlog-cosignature>

The `rfc9162` tests carry the RFC 6962 test vectors, which RFC 9162 keeps.

## Development

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`CONTRIBUTING.md` has the changelog fragments, the version pin and the release steps.

Split out of [deedar](https://github.com/leidarljos/deedar), whose deed log builds on
`rfc9162`. MIT.
