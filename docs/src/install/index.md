# Install

The crates need Rust 1.85 or newer, which is the `rust-version` in the workspace manifest. This repository's `rust-toolchain.toml` pins 1.97.1 for development.

The published crates are version 0.10.0. They release together. `rfc9162-tiles` depends on `rfc9162` at that same version, and `rfc9162-log` depends on both, so install one version of each.

| Platform | Installer |
| --- | --- |
| [Linux](linux.md) | rustup's shell script, then `cargo add` |
| [macOS](macos.md) | rustup's shell script, after the Xcode command-line tools |
| [Windows](windows.md) | `winget install Rustlang.Rustup`, or `rustup-init.exe` |
| [From a checkout](source.md) | clone the repository and build with the pinned toolchain |

After the toolchain is on `PATH`:

```text
cargo add rfc9162@0.10.0
cargo add rfc9162-tiles@0.10.0
cargo add rfc9162-log@0.10.0
```

The sidecar command comes with `rfc9162-log`:

```text
cargo install rfc9162-log --version 0.10.0 --locked
```

That installs the binary `rfc9162-sidecar`. The [log how-to](../howto/rfc9162-log.md) runs it. From a checkout, `cargo run -p rfc9162-log --bin rfc9162-sidecar` builds the same binary from the sources in the tree.

The [getting started](../getting-started.md) program needs only `rfc9162`.
