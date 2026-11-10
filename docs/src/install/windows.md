# Install on Windows

Install rustup with winget:

```text
winget install Rustlang.Rustup
```

The same installer is `rustup-init.exe` from <https://rustup.rs>. Open a new terminal so `cargo` is on `PATH`, then:

```text
rustup show
cargo add rfc9162@0.10.0
cargo add rfc9162-tiles@0.10.0
cargo add rfc9162-log@0.10.0
```

Use the same version of each crate.

The sidecar binary:

```text
cargo install rfc9162-log --version 0.10.0 --locked
```

`cargo install` places `rfc9162-sidecar.exe` in `%USERPROFILE%\.cargo\bin`. The [getting started](../getting-started.md) program is the next step. The [log how-to](../howto/rfc9162-log.md) shows the sidecar commands; the paths in that transcript are from a POSIX shell, and the flags are the same on Windows.
