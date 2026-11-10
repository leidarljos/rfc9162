# rfc9162-log

[Crate docs](../api/rfc9162_log/index.html). The module documentation includes a tested example that appends `alpha`, seals a run, finds the entry by its identity hash and checks the inclusion proof.

## The log

| Item | Role |
| --- | --- |
| [`Store`](../api/rfc9162_log/store/struct.Store.html) | [`open`](../api/rfc9162_log/store/struct.Store.html#method.open), [`open_split`](../api/rfc9162_log/store/struct.Store.html#method.open_split), [`open_trusting`](../api/rfc9162_log/store/struct.Store.html#method.open_trusting) |
| [`AuthenticatedLog`](../api/rfc9162_log/trait.AuthenticatedLog.html) | `append`, `find`, `range`, `seal`, `compact`, `recover` |
| [`RunManifest`](../api/rfc9162_log/struct.RunManifest.html) | entry range, key bounds, run hash, manifest index |
| [`Located`](../api/rfc9162_log/struct.Located.html) | inclusion path of the entry and, when sealed, of the run manifest |
| [`Inclusion`](../api/rfc9162_log/struct.Inclusion.html) | index, tree size, sibling hashes |
| [`Compaction`](../api/rfc9162_log/struct.Compaction.html) | manifest indexes replaced, and the one written |
| [`Recovery`](../api/rfc9162_log/struct.Recovery.html) | checkpoint size, replayed entries, quarantined paths, consistency path |
| [`Error`](../api/rfc9162_log/enum.Error.html) | tampered bytes, store I/O, tree, or a tile-store error |
| [`store::MANIFEST_MAGIC`](../api/rfc9162_log/store/constant.MANIFEST_MAGIC.html) | `rfc9162-log/manifest/v2\n` |
| [`store::RUN_MAGIC_V3`](../api/rfc9162_log/store/constant.RUN_MAGIC_V3.html) | `rfc9162-log/run/v3\n`, the bytes the writer produces |

## The sidecar

| Item | Role |
| --- | --- |
| [`Sidecar`](../api/rfc9162_log/sidecar/struct.Sidecar.html) | `put`, `delete`, `history`, `under`, `diff`, `tree_at`, `sync_dir`, `snapshot`, `include_checkpoint`, `compact` |
| [`Change`](../api/rfc9162_log/sidecar/struct.Change.html) | the JSON entry: `sidecar`, `op`, `path`, and the fields the operation uses |
| [`Op`](../api/rfc9162_log/sidecar/enum.Op.html) | `put`, `delete`, `checkpoint` |
| [`Found`](../api/rfc9162_log/sidecar/struct.Found.html) | a change, its index, and the `Located` proof |
| [`Synced`](../api/rfc9162_log/sidecar/struct.Synced.html) | paths added, changed, deleted, and left unchanged by `sync_dir` |

The binary is [rfc9162-sidecar](sidecar.md). The [log how-to](../howto/rfc9162-log.md) runs the library and the binary. [Runs and recovery](../topics/runs.md) describes the index.
