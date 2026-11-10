# rfc9162

Rust crates for an append-only Merkle log hashed as [RFC 9162](https://www.rfc-editor.org/rfc/rfc9162) (Certificate Transparency Version 2.0). A log gives each reader an inclusion proof for one entry and a consistency proof between two signed tree heads.

The guide in [`docs/`](docs/src/index.md) is an mdBook: [installation](docs/src/install/index.md), a [getting-started program](docs/src/getting-started.md), a [tutorial](docs/src/tutorials/build-a-log.md), [how-to guides](docs/src/howto/index.md), [topic pages](docs/src/topics/index.md) and the rustdoc [API reference](docs/src/reference/index.md). `scripts/build-docs.sh` builds the site.

| Crate | Role |
| --- | --- |
| [`rfc9162`](crates/rfc9162) | Leaf hashes, the root, inclusion and consistency proofs, and the `Head` a caller signs. |
| [`rfc9162-tiles`](crates/rfc9162-tiles) | Tiled storage in the C2SP tlog-tiles layout, key sidecars, and Ed25519 signed checkpoints. |
| [`rfc9162-log`](crates/rfc9162-log) | Sorted runs whose manifests are log entries, so a checkpoint covers the index, plus the `rfc9162-sidecar` binary. |

## Install

The three crates share one version. Install the same version of each.

```text
cargo add rfc9162@0.10.0
cargo add rfc9162-tiles@0.10.0
cargo add rfc9162-log@0.10.0
```

`rfc9162-log` installs the sidecar binary:

```text
cargo install rfc9162-log --version 0.10.0 --locked
```

## Specifications

- RFC 9162, Certificate Transparency Version 2.0: <https://www.rfc-editor.org/rfc/rfc9162>
- C2SP tlog-tiles: <https://c2sp.org/tlog-tiles>
- C2SP tlog-checkpoint: <https://c2sp.org/tlog-checkpoint>
- C2SP signed-note: <https://c2sp.org/signed-note>
- C2SP tlog-cosignature: <https://c2sp.org/tlog-cosignature>

The `rfc9162` tests carry the RFC 6962 test vectors, which RFC 9162 keeps.

## Development

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`CONTRIBUTING.md` has the changelog fragments, the version pin and the release steps. The crates were split out of [deedar](https://github.com/leidarljos/deedar). MIT.

Copyright (c) rfc9162 developers
