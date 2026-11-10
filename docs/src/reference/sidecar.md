# rfc9162-sidecar

The binary in the `rfc9162-log` crate. Its rustdoc page is [`rfc9162_sidecar`](../api/rfc9162_sidecar/index.html). The pack for `--dir DIR` is `DIR/.sidecar/pack`. Each command opens that pack, holds the writer lock while it runs, and prints one line per result. Exit status 0 means the command finished. Exit status 2 means the command line is rejected. Exit status 1 means the store or a proof failed.

```text
rfc9162-sidecar init     --dir DIR
rfc9162-sidecar sync     --dir DIR [--skip NAME]... [--meta K=V]...
rfc9162-sidecar snapshot --dir DIR --key KEY.pem --origin ORIGIN
rfc9162-sidecar history  --dir DIR PATH
rfc9162-sidecar tree     --dir DIR [SIZE]
rfc9162-sidecar diff     --dir DIR FROM TO
rfc9162-sidecar blob     --dir DIR HEX [--out FILE]
rfc9162-sidecar include  --dir DIR PATH --note FILE --trusted KEYLINE... [--meta K=V]...
```

`--meta` repeats. The value is `K=V` and is stored on the change. `--skip` repeats and names a top-level directory entry that `sync` leaves alone. The binary always skips `.sidecar`. `--key` is a PKCS#8 PEM Ed25519 key (`-----BEGIN PRIVATE KEY-----`). `--trusted` repeats and is a verifier line, `name+keyid+base64`. `--note` is the signed checkpoint text of a child log.

| Command | Output |
| --- | --- |
| `init` | `sidecar at DIR/.sidecar size SIZE` |
| `sync` | one `added`, `changed` or `deleted` line per path, then `synced: N added, N changed, N deleted, N unchanged; entries START..END; size SIZE` |
| `snapshot` | `snapshot ORIGIN size SIZE root HEX` |
| `history` | one tab-separated line per change of `PATH`, oldest first |
| `tree` | one tab-separated line per live path at `SIZE`, or at the current size when `SIZE` is omitted |
| `diff` | changes with index in `FROM..TO`, `TO` clamped to the current size |
| `blob` | the bytes of blob `HEX` on stdout, or written to `--out` |
| `include` | `included PATH at INDEX` |

A history, tree or diff line for a put is:

```text
INDEX  put  PATH  BLOBHEX  SIZE
```

A delete omits the blob and the size. A checkpoint line carries the child's origin. The fields are separated by tabs. The [log how-to](../howto/rfc9162-log.md) includes a transcript of each command.

`snapshot` seals the unsealed tail when there is one, then signs the log. `include` verifies the note against the trusted keys. A child note with a smaller size, or the same size and another root, is refused.
