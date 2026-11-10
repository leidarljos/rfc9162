# Install on Linux

Install rustup from the script published at <https://rustup.rs>:

```text
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Open a new shell, or source `$HOME/.cargo/env`, then check the toolchain:

```text
rustup show
rustc --version
cargo --version
```

With the toolchain this repository pins, the version commands print:

```text
rustc 1.97.1 (8bab26f4f 2026-07-14)
cargo 1.97.1 (c980f4866 2026-06-30)
```

A default stable toolchain from rustup is enough for the crates when it is Rust 1.85 or newer. Building this repository selects 1.97.1 through `rust-toolchain.toml`.

Add the crates:

```text
cargo new ct-log
cd ct-log
cargo add rfc9162@0.10.0
```

`cargo add rfc9162@0.10.0` resolves the published crate and writes it into `Cargo.toml`. Continue with [getting started](../getting-started.md).

To install the sidecar binary from crates.io:

```text
cargo install rfc9162-log --version 0.10.0 --locked
```

With no arguments the binary exits 2 and prints:

```text
rfc9162-sidecar: a command is needed
usage: rfc9162-sidecar COMMAND --dir DIR [...]; commands: init sync snapshot history tree diff blob include
```

The [log how-to](../howto/rfc9162-log.md) has a full transcript.
