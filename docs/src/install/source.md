# Install from a checkout

The repository is <https://github.com/leidarljos/rfc9162>.

```text
git clone https://github.com/leidarljos/rfc9162.git
cd rfc9162
rustup show
cargo test --locked --workspace
```

`rust-toolchain.toml` selects Rust 1.97.1 with rustfmt and clippy. `cargo test --locked --workspace` builds the three crates, the documentation examples and the shape tests those examples lock.

Run the sidecar from the checkout:

```text
cargo run -q -p rfc9162-log --bin rfc9162-sidecar -- init --dir demo
```

That creates `demo/.sidecar/pack` and prints the log size. The [log how-to](../howto/rfc9162-log.md) continues from there.

Build this guide, including rustdoc under `docs/book/api/`:

```text
scripts/build-docs.sh
```

The script needs `mdbook` 0.5.4 on `PATH`. `.github/workflows/docs.yml` installs that release and runs the same script. The workflow builds the site and stops there.

```text
curl -sSL -o mdbook.tar.gz https://github.com/rust-lang/mdBook/releases/download/v0.5.4/mdbook-v0.5.4-x86_64-unknown-linux-gnu.tar.gz
tar -xzf mdbook.tar.gz
```

The macOS and Windows archives are on the same GitHub release, with `x86_64-apple-darwin`, `aarch64-apple-darwin` and `x86_64-pc-windows-msvc` in the file name.
