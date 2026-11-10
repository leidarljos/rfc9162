# Runs and recovery

`rfc9162-tiles` can find a key by scanning sidecars. A checkpoint over that log covers the entries and the hash tree. The sidecar files are rebuilt from the bundles. `rfc9162-log` puts the index into the log: a run file is stored beside the tree, and a manifest entry that names the run is appended, so the next checkpoint covers the index.

## A run

A [`RunManifest`](../reference/rfc9162-log.md) is a contiguous entry range, the lowest and highest keys in the run, the SHA-256 of the run file, and, once the manifest entry is appended, that entry's index. A manifest with no index is on disk and outside the tree.

The run file the writer produces starts with `rfc9162-log/run/v3\n`. Keys are front-coded in blocks of 64, with a block index of first keys, so a lookup decodes the blocks whose range can hold the key. Readers still accept v1 and v2 run files. The manifest entry starts with `rfc9162-log/manifest/v2\n` and records the run, the data-tree size and root at seal time, and the manifest indexes the run replaced.

`seal` takes the entries past the last sealed run, writes the run, and appends the manifest. In the [log how-to](../howto/rfc9162-log.md) the first seal covers entries 0 and 1 and logs the manifest at index 2.

## A lookup

[`find`](../reference/rfc9162-log.md) returns one [`Located`](../reference/rfc9162-log.md) value per hit. `entry` is the inclusion path of the entry. `run` is the inclusion path of the manifest that contained the key. Both use one tree size, so one root checks the answer and the index that produced it. A hit in the unsealed tail has `run: None` and was found by scan.

The logged runs' entry ranges have to cover every index below the checkpoint, aside from the scanned tail. A missing run file is `Error::Tree`. An empty answer is the result of that search.

`range` returns hits whose keys lie in an inclusive byte range, in key order. `module=openmpi` sorts before `module=petsc`, which is why the how-to prints index 1 before index 0.

## Compaction

`compact` reads the live runs, writes one run over their union, and appends that manifest. The replaced manifests stay in the log. Their indexes in the how-to are 2 and 4. The new manifest is index 5. A reader who kept an older checkpoint still proves through the old runs, because those entries are still in the tree.

## Split logs

`Store::open_split` writes caller entries to one tile log and run files plus manifests to another. Each log has its own size, root and checkpoint. A caller that signs a head over the data log alone sees only its entries. The how-to seals one entry and prints data size 1, index size 1.

## Recovery

`recover` starts from a size and root passed to `open_trusting`. That pair is the caller's trusted checkpoint. The method loads the consistency path from that size to the current tree and checks it with `verify_consistency`. It reports how many entries sit past the checkpoint (`replayed`) and how many paths the writer had to quarantine (`quarantined`).

The trusted pair has to come from a record outside the files the store's writer updates. A writer who can replace the tree state, or hand back an older signed checkpoint of their choosing, can present a shorter log that still verifies. Store the signed note where that writer has no write access, and pass that note's size and root to `open_trusting`.

The how-to trusts the empty tree, appends one entry, and prints `replayed 1`.
