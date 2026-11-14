<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="branding/logo/logo-dark.svg">
    <img src="branding/logo/logo-light.svg" alt="rfc9162" width="360">
  </picture>
</p>

# rfc9162

Rust crates for an append-only Merkle log hashed as [RFC 9162](https://www.rfc-editor.org/rfc/rfc9162) (Certificate Transparency Version 2.0). A log gives each reader an inclusion proof for one entry and a consistency proof between two signed tree heads.

The guide, <https://stackhands.github.io/rfc9162/>, is an mdBook in [`docs/`](docs/src/index.md): [installation](docs/src/install/index.md), a [getting-started program](docs/src/getting-started.md), a [tutorial](docs/src/tutorials/build-a-log.md), [how-to guides](docs/src/howto/index.md), [topic pages](docs/src/topics/index.md) and the rustdoc [API reference](docs/src/reference/index.md). `scripts/build-docs.sh` builds the site.

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

<!-- stackhands:begin -->
## Part of stackhands

rfc9162 is one of the [stackhands](https://github.com/stackhands) tools for building, installing and maintaining an HPC software stack with EasyBuild on top of EESSI.

| Tool | Job | Docs |
|---|---|---|
| [eb-stack](https://github.com/stackhands/eb-stack) | Solve, shard and plan an EasyBuild build list; admit builds under a scratch inode budget | [docs](https://eb-stack.rgoswami.me/) |
| [eb-staged-rebuild](https://github.com/stackhands/eb-staged-rebuild) | Rebuild an installation in a staged tree and swap it in, so running jobs keep their paths | [docs](https://stackhands.github.io/eb-staged-rebuild/) |
| [eessi-arch-guard](https://github.com/stackhands/eessi-arch-guard) | Reload EESSI in a Slurm task that inherited it for another CPU or GPU | [docs](https://stackhands.github.io/eessi-arch-guard/) |
| [eb-local](https://github.com/stackhands/eb-local) | Install software with EasyBuild on top of EESSI into a directory a user or project owns | [docs](https://stackhands.github.io/eb-local/) |
| [eb-keep-logs](https://github.com/stackhands/eb-keep-logs) | Keep a failed build's EasyBuild log and command output; rotate a pipeline's log tree | [docs](https://stackhands.github.io/eb-keep-logs/) |
| **rfc9162** | Append-only Merkle logs hashed as RFC 9162, with tiled storage and signed checkpoints | [docs](https://stackhands.github.io/rfc9162/) |
| [zig-common](https://github.com/stackhands/zig-common) | The shared Zig library under the Zig tools: system calls, checked files, child processes | [docs](https://stackhands.github.io/zig-common/) |
<!-- stackhands:end -->
