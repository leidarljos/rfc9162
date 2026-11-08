# Changelog

Versions follow semver at 0.x: a minor bump is a feature, a patch is a fix.
The crates carry the one workspace version and are released together.

<!-- towncrier release notes start -->

## 0.8.0 (2026-11-08)

### Features

- `Sidecar::include_checkpoint` logs a child project's signed checkpoint under a path, verified against trusted keys and refused when it does not extend the child's last logged checkpoint, so a parent log's checkpoint signs its children's; `checkpoints(path)` reads them back.
- `Sidecar::sync_dir` records the regular files under a directory against the tree the log holds, the way etckeeper commits `/etc`: a file the log lacks or holds with other bytes is a put, a path the directory lacks is a delete, an unchanged file is left alone, and every change goes in one batch after its blob.
- `rfc9162-log::Sidecar`: a sidecar log beside a project tree. Every change to a path is a JSON entry keyed `path=` and `blob=`, its bytes a content-addressed blob stored once in the same store before the entry that names it; `history`, `under`, `tree_at`, `diff` read it back with proofs, and `snapshot` seals the tail and signs the log. Over a `PackStore` the whole history is one file and its lock.


## 0.7.0 (2026-11-08)

### Features

- New crate `rfc9162-tiles`: tiled, indexed storage for an `rfc9162` log in the C2SP tlog-tiles layout (256-hash tiles, 256-entry bundles, partial tiles for the tail), a locked writer that writes bundles, tiles, index sidecars, manifest and tree state in that order and quarantines a torn tail on open, a reader that checks every tile against the trusted root and assembles inclusion and consistency proofs identical to the flat crate's, per-bundle key sidecars with a split-block Bloom filter and a manifest for `find`, and C2SP checkpoints as Ed25519 signed notes with cosignature/v1 verification.
- `rfc9162-log` is published: an authenticated, indexed, append-only log over the other two crates. Keys are sorted into runs whose manifests are themselves entries, so the checkpoint signs the index and a lookup returns an inclusion path for the entry and for the run that located it; compaction writes a new run and logs it, the replaced runs stay provable; `Store::open_split` keeps a data tile log of the caller's entries alone and the runs and manifests in an index log beside it with its own checkpoint, so a consumer that signs its own head over the data log is unchanged.
- `rfc9162-tiles::PackStore`: a `TileStore` over one append-only file, so a log costs a constant number of inodes however long its history. Each record is a path with its bytes, a deletion or a rename and ends in a SHA-256, so a torn tail is found and ignored; bodies that shrink under DEFLATE are stored compressed; `compact` rewrites the pack with one record per live path. One writer holds `<pack>.lock`; readers open without a lock, see a writer's appends, and reopen after a compaction.

### Fixes

- `rfc9162-tiles`: a tail tile keeps only its widest partial. Every width a partial bundle or hash tile passed through stayed until the full tile existed, up to 255 files per level per tail tile; the tree state names the widest, which covers the narrower ones, so those are removed with each write.

### Miscellaneous

- The two crates live in their own repository, with the history of the commits that wrote them.
  `rfc9162-tiles` requires `rfc9162` at the one workspace version, and the publish workflow publishes `rfc9162` before `rfc9162-tiles`.


## 0.6.1 (2026-11-05)

### Features

- `rfc9162`: the Merkle tree of RFC 9162 (doi:10.17487/RFC9162) as pure functions over 32-byte leaf hashes.
  Leaf `SHA-256(0x00 || entry)`, node `SHA-256(0x01 || left || right)`, the root over a leaf slice, inclusion and consistency proofs, the checks for both, and a `Head` whose `material(domain)` is what a log signs.
  The crate stores nothing and serialises nothing.
  This release was published from the deedar repository, whose deed log builds on it; the version number is deedar's at the time.
