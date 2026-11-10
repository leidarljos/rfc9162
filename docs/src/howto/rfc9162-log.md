# rfc9162-log

`rfc9162-log` is an indexed log on top of the tile store. Keys from the caller's function are sorted into runs. Each run's manifest is appended to the log, so a checkpoint over the log covers the index. [`Sidecar`](../reference/rfc9162-log.md) stores a directory as entries in that log. The `rfc9162-sidecar` binary is the same operations from a shell.

The key function receives entry bytes and returns extra lookup keys. The identity hash is added by the tile layer and can be omitted here.

## Seal a run and look up a key

```rust
{{#rustdoc_include ../../examples/src/bin/howto_log.rs:seal}}
```

`seal` covers entries 0 and 1 and logs the manifest at index 2. `find` returns a [`Located`](../reference/rfc9162-log.md) value: the entry's inclusion path, and the manifest's inclusion path when the entry was inside a sealed run. The entry path verifies against `log.root()` at `tree_size`. A key that no entry carries returns an empty list. `range` walks keys in order from the start key through the end key, inclusive.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_log.rs:find}}
```

`module=openmpi` sorts before `module=petsc`, so the range `module=o` through `module=petsc` returns index 1, then index 0.

## Compact and recover

A second seal makes a second run. `compact` merges the live runs, appends the new manifest, and leaves the replaced manifests in the log. A reader at an older size still has those entries. After compaction, `find` for `module=fftw` still verifies.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_log.rs:compact}}
```

`open_trusting` records a size and root for `recover` to check. Keep that pair outside the store the writer updates. Here the trusted head is the empty tree. One append is replayed, and the consistency path from size 0 is empty.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_log.rs:recover}}
```

`open_split` sends entries to one store and run manifests to another. After one append and one seal, the data log's size is 1 and the index log's size is 1.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_log.rs:split}}
```

```text
{{#include ../output/howto_log.txt}}
```

The replaced manifests are indexes 2 and 4. The written manifest is index 5. See [runs and recovery](../topics/runs.md).

## Record a directory in the library

`Sidecar::put` writes the file bytes once, under `blob/<sha256>`, then appends a JSON entry. `snapshot` seals the tail and signs a checkpoint. `history` returns each change with its proof. The entry JSON below is the bytes that were hashed.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_sidecar.rs:put}}
```

`sync_dir` compares a directory to the tree the log holds. A new file is a put, a changed file is a put, a missing file is a delete, and an unchanged file is left alone. `tree_at` is the latest put for each path that is still present.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_sidecar.rs:sync}}
```

`include_checkpoint` appends a child log's signed note after `checkpoint::verify` accepts it. A later checkpoint for that path has to extend the one already logged: a smaller size, or the same size with another root, is refused. `checkpoints` parses the notes back.

```rust
{{#rustdoc_include ../../examples/src/bin/howto_sidecar.rs:include}}
```

```text
{{#include ../output/howto_sidecar.txt}}
```

The first sync adds two files. The resync changes `ssh/sshd_config` (10 bytes, `Port 2222\n`) and deletes `motd`. The child checkpoint has size 2: the put, then the manifest `snapshot` sealed. The parent logs it at index 0 under `stacks/2026`.

## The sidecar from a shell

`rfc9162-sidecar` keeps the pack at `DIR/.sidecar/pack`. Install it with [cargo install](../install/index.md) or run it from a checkout:

```text
cargo run -q -p rfc9162-log --bin rfc9162-sidecar -- init --dir demo
```

The transcript below is that binary, with a PKCS#8 Ed25519 key whose seed is the byte `7` repeated. The commands run in an empty directory that contains `demo.pem` and, later, `child.pem`. File contents are `demo/ssh/sshd_config` = `Port 22\n`, then `Port 2222\n`; `demo/motd` = `hello\n`, deleted on the second sync; `demo/issue` = `new\n`; `child/modules/petsc.lua` = `root = '/opt/petsc'\n`.

```text
{{#include ../output/sidecar.txt}}
```

`init` creates the pack and prints size 0. The first `sync` appends `motd` and `ssh/sshd_config` at indexes 0 and 1. `snapshot` seals a run, which appends a manifest, so the signed size is 3. The root is `4bc522505fc7ad765e208f1d45c73fb8ddfc3a9225ab3d0e1e5e8e78b8986a92`.

The second sync appends the new and changed paths and the deletion at indexes 3, 4 and 5. Index 2 is the manifest from the snapshot, and `diff` skips manifest entries. `history` prints both puts of `ssh/sshd_config`. `tree` prints the live paths. `blob` writes the bytes of the latest put, `Port 2222\n`.

`include` reads the child's signed note. `show_checkpoint` in `docs/examples` prints the `checkpoint` record from a pack; a caller that already has the note file skips that step. The trusted line is the child's verifier. The command logs the note at index 0 under `stacks/2026`.

The command reference is the [sidecar page](../reference/sidecar.md).
