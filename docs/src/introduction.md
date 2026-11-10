# Introduction

The workspace holds three crates that share one version and release together.

| Crate | What it does |
| --- | --- |
| [`rfc9162`](howto/rfc9162.md) | Leaf hashes, interior hashes, the root, inclusion proofs, consistency proofs, and the [`Head`](reference/rfc9162.md) a caller signs. |
| [`rfc9162-tiles`](howto/rfc9162-tiles.md) | The same tree stored as [C2SP tlog-tiles](https://c2sp.org/tlog-tiles): hash tiles, entry bundles, a manifest, and an Ed25519 [checkpoint](topics/checkpoints.md). |
| [`rfc9162-log`](howto/rfc9162-log.md) | An index of sorted runs whose manifests are log entries, so the checkpoint covers the index. [`Sidecar`](reference/rfc9162-log.md) keeps a directory's history in that log. |

`rfc9162-tiles` requires `rfc9162` at the same version. `rfc9162-log` requires both.

## What a reader checks

A log entry is an opaque byte string. Its leaf hash is `SHA-256(0x00 || entry)`. Interior nodes are `SHA-256(0x01 || left || right)`. The shape of the tree, and the proofs over it, are fixed by RFC 9162 section 2.1. The [Merkle tree](topics/merkle-tree.md) page draws that shape. [Inclusion proofs](topics/inclusion.md) and [consistency proofs](topics/consistency.md) show which nodes a proof carries.

The flat crate returns those hashes. The tile crate stores the entries and assembles the same proof bytes from tiles. A proof from `Reader::inclusion` compares equal to `rfc9162::inclusion_proof` over the same leaves. The [tutorial](tutorials/build-a-log.md) runs that comparison.

## Specifications

- [RFC 9162](https://www.rfc-editor.org/rfc/rfc9162), Certificate Transparency Version 2.0. The tree rules are section 2.1. The hashing matches RFC 6962, and the crate tests include that RFC's published roots.
- [C2SP tlog-tiles](https://c2sp.org/tlog-tiles), the tile and entry-bundle layout.
- [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint), the checkpoint body.
- [C2SP signed-note](https://c2sp.org/signed-note), the signature envelope and key identifiers.
- [C2SP tlog-cosignature](https://c2sp.org/tlog-cosignature), witness cosignature lines.

## Where to go

[Getting started](getting-started.md) is a single program over the hash tree. The [tutorial](tutorials/build-a-log.md) stores the log and signs it. The [how-to guides](howto/index.md) are one page per crate. The [API reference](reference/index.md) is generated rustdoc.
