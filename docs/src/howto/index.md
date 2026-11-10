# How-to guides

Each page is one crate. The tasks assume the crates are [installed](../install/index.md) and that the terms on the [topic pages](../topics/index.md) are available when a step needs them.

| Guide | Tasks |
| --- | --- |
| [rfc9162](rfc9162.md) | Hash a tree, accept a matching proof, check a proof against other inputs, encode the head. |
| [rfc9162-tiles](rfc9162-tiles.md) | Append to a directory, look up an identity hash, keep the log in one pack file, sign a checkpoint and a cosignature. |
| [rfc9162-log](rfc9162-log.md) | Seal runs, find and range, compact, recover, split the index, and record a directory with the sidecar. |

The [tutorial](../tutorials/build-a-log.md) is the narrative that joins the first use of each crate. These pages are the tasks on their own. The programs live in `docs/examples` and the transcripts are what those programs print.
