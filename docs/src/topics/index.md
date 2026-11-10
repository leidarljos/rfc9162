# Topics

These pages explain the objects the crates implement. The [how-to guides](../howto/index.md) use them. The [API reference](../reference/index.md) is the function list.

- [The Merkle tree](merkle-tree.md) is the shape of every root.
- [Inclusion proofs](inclusion.md) tie one leaf to a root.
- [Consistency proofs](consistency.md) tie an older root to a later root that appends.
- [Tiles, bundles and packs](tiles.md) are how `rfc9162-tiles` stores that tree.
- [Runs and recovery](runs.md) are how `rfc9162-log` puts the index inside the log.
- [Checkpoints and cosignatures](checkpoints.md) are the signed heads.
