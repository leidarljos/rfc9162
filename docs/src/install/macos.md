# Install on macOS

Rust needs a system linker. Install the Xcode command-line tools if `clang` is absent:

```text
xcode-select --install
```

Install rustup from the script published at <https://rustup.rs>:

```text
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Open a new shell, or source `$HOME/.cargo/env`, then:

```text
rustup show
cargo add rfc9162@0.10.0
cargo add rfc9162-tiles@0.10.0
cargo add rfc9162-log@0.10.0
```

Use the same version of each crate. The tiles crate requires `rfc9162` at the version it was published with, and the log crate requires both.

The sidecar binary:

```text
cargo install rfc9162-log --version 0.10.0 --locked
```

The [getting started](../getting-started.md) program is the next step. The tile store's locks and renames are covered by the workspace tests on macOS in CI.
