# rfc9162-log

An authenticated, indexed, append-only log over [`rfc9162`](https://docs.rs/rfc9162) and [`rfc9162-tiles`](https://docs.rs/rfc9162-tiles). Keys are sorted into runs. Each run's manifest is itself a log entry, so a checkpoint over the log covers the index. A lookup returns the entry and the run that found it, each with an inclusion path at one tree size. Compaction appends a new run and leaves the replaced runs in the log.

[`Sidecar`](https://docs.rs/rfc9162-log/latest/rfc9162_log/struct.Sidecar.html) keeps a directory's history in that log: file bytes are stored once under their SHA-256, and each change is a JSON entry. `rfc9162-sidecar` (`cargo install rfc9162-log`) is the same operations on `DIR/.sidecar/pack`.

The repository guide has the [library tasks and a command transcript](../../docs/src/howto/rfc9162-log.md), the [run format](../../docs/src/topics/runs.md) and the [command reference](../../docs/src/reference/sidecar.md).

```rust
use std::sync::Arc;

use rfc9162_log::{AuthenticatedLog, Store};
use rfc9162_tiles::MemoryStore;

let mut log = Store::open(MemoryStore::new(), Arc::new(|_| Vec::new()))?;
let index = log.append(b"alpha")?;
log.seal()?;
let found = log.find(&rfc9162_tiles::index::identity(b"alpha"))?;
assert_eq!(found[0].entry.index, index);
# Ok::<(), rfc9162_log::Error>(())
```

MIT.
