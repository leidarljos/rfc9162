# Contributing

## Checks

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are what CI runs, on the toolchain `rust-toolchain.toml` pins.
CI also runs `cargo publish --dry-run` for each crate, so a manifest that cannot be packaged fails before a tag exists.

## Changelog entries

Do not edit `CHANGELOG.md` in a pull request.
Add one fragment per change under `changelog.d/` instead, named `<PR number>.<type>.md`.
The type is one of `feature`, `bugfix`, `doc`, `removal` or `misc`.
A change with no pull request yet can use `+<branch>.<type>.md`.
Write the entry as one Markdown bullet body, one sentence per line, with no leading dash.
At release time `towncrier build --version X.Y.Z` collects the fragments into `CHANGELOG.md` and removes them.
CI runs `towncrier build --draft --version V`, with V the workspace version in `Cargo.toml`, and fails a fragment whose name is not one of those types.

## Versions

The version is written by hand once, in `[workspace.package]` in `Cargo.toml`.
The `rfc9162` pin under `[workspace.dependencies]` is a cogapp block that follows it: after changing the version run `cog -r @cogfiles.txt` (cogapp, not cocogitto; both install a command named `cog`).
CI runs `cog --check @cogfiles.txt` and fails a pin that has drifted.

## Releases

Both crates release together at the workspace version.

1. Set the version in `Cargo.toml` and run `cog -r @cogfiles.txt`.
2. `towncrier build --version X.Y.Z`, then commit the manifest, the lockfile and the changelog.
3. Tag `vX.Y.Z` and push the tag.

The publish workflow checks that the tag is the workspace version, then publishes `rfc9162` and `rfc9162-tiles` in that order.
The release workflow writes a GitHub release whose notes are that version's changelog section.
