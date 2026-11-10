# rfc9162-tiles

[Crate docs](../api/rfc9162_tiles/index.html). The module documentation includes a tested example that appends one record to a `MemoryStore` and checks the inclusion proof.

## Stores and the log

| Item | Role |
| --- | --- |
| [`TileStore`](../api/rfc9162_tiles/store/trait.TileStore.html) | `get`, `put`, `create_new`, `delete`, `rename`, `list` |
| [`PosixStore`](../api/rfc9162_tiles/store/struct.PosixStore.html) | a directory; writes are temp file, fsync, rename |
| [`MemoryStore`](../api/rfc9162_tiles/store/struct.MemoryStore.html) | in-process files; clones share the map |
| [`PackStore`](../api/rfc9162_tiles/pack/struct.PackStore.html) | one append-only file; [`open`](../api/rfc9162_tiles/pack/struct.PackStore.html#method.open), [`open_shared`](../api/rfc9162_tiles/pack/struct.PackStore.html#method.open_shared), [`open_read`](../api/rfc9162_tiles/pack/struct.PackStore.html#method.open_read), [`compact`](../api/rfc9162_tiles/pack/struct.PackStore.html#method.compact) |
| [`Writer`](../api/rfc9162_tiles/log/struct.Writer.html) | appends entries and publishes checkpoints |
| [`Reader`](../api/rfc9162_tiles/log/struct.Reader.html) | reads entries, proofs and keys at one tree size |
| [`TreeState`](../api/rfc9162_tiles/log/struct.TreeState.html) | size and root written at `.state/treeState` |
| [`Layout`](../api/rfc9162_tiles/layout/struct.Layout.html) | tile widths for a tree size |
| [`Error`](../api/rfc9162_tiles/error/enum.Error.html) | I/O, corrupt bytes, format, lock, entry length, range, signature |

[`index::no_keys`](../api/rfc9162_tiles/index/fn.no_keys.html) is a key function that returns no extra keys. [`index::identity`](../api/rfc9162_tiles/index/fn.identity.html) is `SHA-256(entry)`.

## Checkpoints

| Item | Role |
| --- | --- |
| [`Checkpoint`](../api/rfc9162_tiles/checkpoint/struct.Checkpoint.html) | origin, size, root, extensions |
| [`Note`](../api/rfc9162_tiles/checkpoint/struct.Note.html) | checkpoint text plus signature lines |
| [`Signer`](../api/rfc9162_tiles/checkpoint/struct.Signer.html) | Ed25519 log key; [`from_pem`](../api/rfc9162_tiles/checkpoint/struct.Signer.html#method.from_pem) reads PKCS#8 |
| [`Verifier`](../api/rfc9162_tiles/checkpoint/struct.Verifier.html) | public key and the `name+id+base64` line |
| [`checkpoint::verify`](../api/rfc9162_tiles/checkpoint/fn.verify.html) | checks log signatures against trusted keys |
| [`Cosigner`](../api/rfc9162_tiles/checkpoint/struct.Cosigner.html) | witness key |
| [`verify_cosignatures`](../api/rfc9162_tiles/checkpoint/fn.verify_cosignatures.html) | checks cosignature/v1 lines |

The [tiles how-to](../howto/rfc9162-tiles.md) uses these types. [Tiles](../topics/tiles.md) and [checkpoints](../topics/checkpoints.md) explain the layout and the note.
