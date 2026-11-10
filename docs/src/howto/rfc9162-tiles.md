# rfc9162-tiles

`rfc9162-tiles` stores an RFC 9162 log as [tlog tiles](../topics/tiles.md). `Writer` appends. `Reader` returns entries and proofs that `rfc9162` checks. `PosixStore` is a directory. `PackStore` is one file. `MemoryStore` keeps the files in the process.

`Writer::open` and `Reader::open` take a key function. [`no_keys`](../reference/rfc9162-tiles.md) adds nothing beyond the identity hash `SHA-256(entry)`, which every entry has. The writer and the reader of one log use the same function.

## Append to a directory and read it back

```rust
{{#rustdoc_include ../../examples/src/bin/howto_tiles.rs:write}}
```

```rust
{{#rustdoc_include ../../examples/src/bin/howto_tiles.rs:read}}
```

Drop the writer before opening another one. The writer holds `.state/treeState.lock` until drop. A second `Writer::open` while that file exists returns `Error::Locked`.

`find` takes a key. The identity hash finds the entry that hashed to it. An identity that no entry has returns an empty list.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_tiles.rs:find}}
```

Two entries produce a partial level-0 tile and a partial bundle, both of width 2:

```rust
{{#rustdoc_include ../../examples/src/bin/howto_tiles.rs:files}}
```

```text
{{#include ../output/howto_tiles.txt}}
```

## Keep the log in one pack file

`PackStore::open` creates an append-only file and holds `<pack>.lock` until the store drops. `Writer` and `Reader` can borrow that store. `compact` rewrites the file so each live path is one record. For two entries the live paths are the tree state, the partial hash tile and the partial bundle.

`open_shared` takes the pack lock for each change, so more than one process can append. The snippet opens a second pack that way and appends one record.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_pack.rs:pack}}
```

```rust
{{#rustdoc_include ../../examples/src/bin/howto_pack.rs:compact}}
```

```text
{{#include ../output/howto_pack.txt}}
```

`compact` on this pack keeps 3 paths and leaves the file at 532 bytes. The length before compact includes a deleted writer lock whose body is the process id, so that length changes from run to run. Readers open with `PackStore::open_read`, which leaves `<pack>.lock` free.

## Sign a checkpoint

`Signer::new` takes the origin and an Ed25519 key. The origin is the key name: non-empty, no spaces, no `+`. `Writer::checkpoint` signs the current size and root and writes the note to `checkpoint`. A published checkpoint for a larger size is left in place.

`checkpoint::verify` checks the note against trusted log keys. `Cosigner` appends a cosignature/v1 line. `verify_cosignatures` checks those lines. The timestamp below is the Unix time 1700000000.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_checkpoint.rs:sign}}
```

```rust
{{#rustdoc_include ../../examples/src/bin/howto_checkpoint.rs:cosign}}
```

The verifier line is what `Reader::open_checkpoint` and the sidecar's `--trusted` flag parse: `name+hex-key-id+base64(type || public key)`.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_checkpoint.rs:pem}}
```

```text
{{#include ../output/howto_checkpoint.txt}}
```

The same signing key under another name has another key id. The [checkpoint topic](../topics/checkpoints.md) describes the note. The fixed seed `7` repeated is a documentation key.

`Signer::from_pem` reads a PKCS#8 PEM file (`-----BEGIN PRIVATE KEY-----`). The [log how-to](rfc9162-log.md) passes that file to `rfc9162-sidecar snapshot --key`.
