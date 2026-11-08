# rfc9162-log

The contract for the log that grows out of `rfc9162` and `rfc9162-tiles`,
written before its store so the store is argued over types.

What `rfc9162-tiles` gives: tlog-tiles layout, per-bundle key sidecars the
checkpoint does not sign, `find` by Bloom filter and scan, torn-tail
quarantine. What this crate names on top:

| guarantee | type |
|---|---|
| the checkpoint signs the index | `RunManifest::manifest_index`: every sealed run's manifest is a log entry |
| a lookup carries its own proof | `Located`: the entry and the run that found it, each with an inclusion path at one tree size |
| prefix and range queries seek | `RunManifest::key_lo` and `key_hi` over sorted runs; `AuthenticatedLog::range` |
| compaction keeps every proof | `Compaction`: replaced runs stay in the log, the written run is logged |
| recovery is proven | `Recovery`: replay past the checkpoint, consistency path back to it |

Consumers: eb-stack's install ledger (keys such as `module=`, `pkg=`,
`target=`), deedar's deed log, and ljos through deedar; one log per site for
HPC installs, general enough to carry an SBOM.

`publish = false` until the store exists. The design discussion lives on the
owner's tracker beside this repository.
