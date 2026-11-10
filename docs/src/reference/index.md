# API reference

The generated rustdoc is part of this site:

- [`rfc9162`](../api/rfc9162/index.html)
- [`rfc9162-tiles`](../api/rfc9162_tiles/index.html)
- [`rfc9162-log`](../api/rfc9162_log/index.html)

Build it locally with:

```text
cargo doc --locked --workspace --no-deps --open
```

`scripts/build-docs.sh` runs that command and copies `target/doc` to `docs/book/api`. The pages below are a map of the public items. The rustdoc pages are the full signatures, the crate-level examples and the type documentation.

| Page | Crate |
| --- | --- |
| [rfc9162](rfc9162.md) | hashes, proofs, `Head` |
| [rfc9162-tiles](rfc9162-tiles.md) | stores, reader, writer, checkpoints |
| [rfc9162-log](rfc9162-log.md) | `Store`, `Sidecar`, runs |
| [rfc9162-sidecar](sidecar.md) | the command-line binary |

The [changelog](../changelog.md) is the release history. [Contributing](../contributing.md) is how a change lands.

<p class="copyright">Copyright (c) rfc9162 developers</p>
