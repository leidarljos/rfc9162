//! The store behind [`AuthenticatedLog`]: sorted runs over [`rfc9162_tiles`],
//! each run's manifest appended to the log itself so the checkpoint that
//! signs the entries signs the index.
//!
//! Layout inside the tile store, beside the tiles:
//!
//! - `runs/<lo>-<hi>`: one sealed run, the sorted `(key, index)` pairs of
//!   entries `lo..=hi` and a Bloom filter over the keys, in the format
//!   `RUN_MAGIC` describes. Its SHA-256 is the `run_hash` the logged manifest
//!   carries, so a reader checks the file before trusting a seek.
//! - `runs/live`: the manifest indices of the runs a lookup goes through, one
//!   per line, oldest first. A cache: when it is missing or stale the live set
//!   is rebuilt from the manifest entries in the log.
//!
//! A manifest entry starts with `MANIFEST_MAGIC`, carries no lookup keys and
//! lies in the entry range of the run sealed after it. Compaction logs a
//! manifest that names the manifest indices it replaces; the replaced runs
//! stay in the store and in the tree, and leave the live set.
//!
//! [`Store`] is the one writer and holds the writer lock. [`Index`] answers
//! lookups at a tree size and root a caller trusts, without the lock, which is
//! how a verifier that holds a signed checkpoint reads the log.

use std::collections::{BTreeSet, HashMap};
use std::ops::RangeInclusive;
use std::sync::{Arc, Mutex};

use rfc9162_tiles::bloom::Bloom;
use rfc9162_tiles::index::{identity, KeyFn};
use rfc9162_tiles::log::{Reader, Writer};
use rfc9162_tiles::store::TileStore;

use crate::{AuthenticatedLog, Compaction, Error, Inclusion, Located, Recovery, RunManifest};

/// First bytes of a logged run manifest.
pub const MANIFEST_MAGIC: &[u8] = b"rfc9162-log/manifest/v1\n";
/// First bytes of a run file.
pub const RUN_MAGIC: &[u8] = b"rfc9162-log/run/v1\n";
const LIVE: &str = "runs/live";
const BITS_PER_KEY: usize = 10;

/// Derives the lookup keys of an entry; shared between the writer's sidecars
/// and the runs. The identity hash is always a key and need not be returned.
pub type Keys = Arc<dyn Fn(&[u8]) -> Vec<Vec<u8>> + Send + Sync>;

/// Decoded runs by manifest index, shared between lookups so a run file is
/// read and checked once per process rather than once per lookup.
pub type RunCache = Arc<Mutex<HashMap<u64, Arc<Run>>>>;

fn run_path(entries: &RangeInclusive<u64>) -> String {
    format!("runs/{:020}-{:020}", entries.start(), entries.end())
}

/// The key function the tiles layer gets: the caller's keys for ordinary
/// entries, none for manifest entries.
pub fn tiles_keys(keys: &Keys) -> KeyFn {
    let keys = Arc::clone(keys);
    Box::new(move |entry: &[u8]| {
        if entry.starts_with(MANIFEST_MAGIC) {
            Vec::new()
        } else {
            keys(entry)
        }
    })
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), Error> {
    let len = u16::try_from(bytes.len()).map_err(|_| Error::Tree("key over 65535 bytes"))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    what: &'static str,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(Error::Tampered { what: self.what })?;
        if end > self.bytes.len() {
            return Err(Error::Tampered { what: self.what });
        }
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u64(&mut self) -> Result<u64, Error> {
        let b = self.take(8)?;
        Ok(u64::from_be_bytes(b.try_into().expect("eight bytes")))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes(b.try_into().expect("four bytes")))
    }
    fn bytes(&mut self) -> Result<&'a [u8], Error> {
        let b = self.take(2)?;
        let len = u16::from_be_bytes(b.try_into().expect("two bytes")) as usize;
        self.take(len)
    }
    fn done(&self) -> Result<(), Error> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(Error::Tampered { what: self.what })
        }
    }
}

/// A logged manifest, as the entry carries it: the run it describes and the
/// manifest indices of the runs it replaced (none for a seal).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Logged {
    run: RunManifest,
    replaced: Vec<u64>,
}

impl Logged {
    fn encode(&self) -> Result<Vec<u8>, Error> {
        let mut out = MANIFEST_MAGIC.to_vec();
        put_u64(&mut out, *self.run.entries.start());
        put_u64(&mut out, *self.run.entries.end());
        put_bytes(&mut out, &self.run.key_lo)?;
        put_bytes(&mut out, &self.run.key_hi)?;
        out.extend_from_slice(&self.run.run_hash);
        let n = u32::try_from(self.replaced.len())
            .map_err(|_| Error::Tree("too many replaced runs"))?;
        out.extend_from_slice(&n.to_be_bytes());
        for index in &self.replaced {
            put_u64(&mut out, *index);
        }
        Ok(out)
    }

    fn decode(bytes: &[u8], manifest_index: u64) -> Result<Self, Error> {
        let what = "manifest entry";
        let body = bytes
            .strip_prefix(MANIFEST_MAGIC)
            .ok_or(Error::Tampered { what })?;
        let mut c = Cursor {
            bytes: body,
            at: 0,
            what,
        };
        let lo = c.u64()?;
        let hi = c.u64()?;
        let key_lo = c.bytes()?.to_vec();
        let key_hi = c.bytes()?.to_vec();
        let run_hash: [u8; 32] = c.take(32)?.try_into().expect("32 bytes");
        let n = c.u32()?;
        let mut replaced = Vec::with_capacity(n as usize);
        for _ in 0..n {
            replaced.push(c.u64()?);
        }
        c.done()?;
        if lo > hi || hi >= manifest_index {
            return Err(Error::Tampered { what });
        }
        Ok(Self {
            run: RunManifest {
                entries: lo..=hi,
                key_lo,
                key_hi,
                run_hash,
                manifest_index: Some(manifest_index),
            },
            replaced,
        })
    }
}

/// A run file, decoded and checked against its manifest.
pub struct Run {
    pairs: Vec<(Vec<u8>, u64)>,
    filter: Bloom,
}

impl Run {
    fn encode(entries: &RangeInclusive<u64>, pairs: &[(Vec<u8>, u64)]) -> Result<Vec<u8>, Error> {
        let mut out = RUN_MAGIC.to_vec();
        put_u64(&mut out, *entries.start());
        put_u64(&mut out, *entries.end());
        let n = u32::try_from(pairs.len()).map_err(|_| Error::Tree("too many pairs in a run"))?;
        out.extend_from_slice(&n.to_be_bytes());
        for (key, index) in pairs {
            put_bytes(&mut out, key)?;
            put_u64(&mut out, *index);
        }
        let keys: Vec<&[u8]> = pairs.iter().map(|(k, _)| k.as_slice()).collect();
        let filter = Bloom::build(&keys, BITS_PER_KEY).to_bytes();
        let len = u32::try_from(filter.len()).map_err(|_| Error::Tree("filter too large"))?;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&filter);
        Ok(out)
    }

    fn decode(bytes: &[u8], expect: &RunManifest) -> Result<Self, Error> {
        let what = "run file";
        if identity(bytes) != expect.run_hash {
            return Err(Error::Tampered { what });
        }
        let body = bytes
            .strip_prefix(RUN_MAGIC)
            .ok_or(Error::Tampered { what })?;
        let mut c = Cursor {
            bytes: body,
            at: 0,
            what,
        };
        let lo = c.u64()?;
        let hi = c.u64()?;
        if lo != *expect.entries.start() || hi != *expect.entries.end() {
            return Err(Error::Tampered { what });
        }
        let n = c.u32()?;
        let mut pairs = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let key = c.bytes()?.to_vec();
            let index = c.u64()?;
            if !expect.entries.contains(&index) {
                return Err(Error::Tampered { what });
            }
            pairs.push((key, index));
        }
        if pairs.windows(2).any(|w| w[0] >= w[1]) {
            return Err(Error::Tampered { what });
        }
        let len = c.u32()? as usize;
        let filter = Bloom::from_bytes(c.take(len)?).map_err(|_| Error::Tampered { what })?;
        c.done()?;
        Ok(Self { pairs, filter })
    }

    fn indices_of(&self, key: &[u8]) -> Vec<u64> {
        if !self.filter.contains(key) {
            return Vec::new();
        }
        let start = self.pairs.partition_point(|(k, _)| k.as_slice() < key);
        self.pairs[start..]
            .iter()
            .take_while(|(k, _)| k.as_slice() == key)
            .map(|(_, i)| *i)
            .collect()
    }

    fn pairs_in(&self, range: &RangeInclusive<&[u8]>) -> Vec<(Vec<u8>, u64)> {
        let start = self
            .pairs
            .partition_point(|(k, _)| k.as_slice() < *range.start());
        self.pairs[start..]
            .iter()
            .take_while(|(k, _)| k.as_slice() <= *range.end())
            .cloned()
            .collect()
    }
}

/// Whether the runs cover `0..=last` without a gap or an overlap.
fn contiguous(runs: &[RunManifest]) -> bool {
    let mut next = 0u64;
    for run in runs {
        if *run.entries.start() != next {
            return false;
        }
        next = run.entries.end() + 1;
    }
    true
}

/// The live runs below `size`: the cache at `runs/live` when every line of it
/// names a manifest entry below `size` and the runs are contiguous, else a
/// rebuild from the manifest entries in `0..size` with the replacements
/// applied. The flag says whether it was rebuilt.
fn load_live<S: TileStore>(
    reader: &Reader<S>,
    size: u64,
) -> Result<(Vec<RunManifest>, bool), Error> {
    if let Some(bytes) = reader.store().get(LIVE)? {
        let text = String::from_utf8_lossy(&bytes);
        let mut runs = Vec::new();
        let mut ok = true;
        for line in text.lines().filter(|l| !l.is_empty()) {
            let Ok(index) = line.parse::<u64>() else {
                ok = false;
                break;
            };
            if index >= size {
                ok = false;
                break;
            }
            match reader.get(index)? {
                Some(entry) if entry.starts_with(MANIFEST_MAGIC) => {
                    runs.push(Logged::decode(&entry, index)?.run);
                }
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && contiguous(&runs) {
            return Ok((runs, false));
        }
    }
    let mut runs: Vec<RunManifest> = Vec::new();
    for index in 0..size {
        let Some(entry) = reader.get(index)? else {
            return Err(Error::Tree("entry missing below the tree size"));
        };
        if !entry.starts_with(MANIFEST_MAGIC) {
            continue;
        }
        let logged = Logged::decode(&entry, index)?;
        runs.retain(|r| {
            !logged
                .replaced
                .contains(&r.manifest_index.unwrap_or(u64::MAX))
        });
        runs.push(logged.run);
        runs.sort_by_key(|r| *r.entries.start());
    }
    if !contiguous(&runs) {
        return Err(Error::Tree("logged runs do not cover the sealed range"));
    }
    Ok((runs, true))
}

fn save_live(store: &impl TileStore, runs: &[RunManifest]) -> Result<(), Error> {
    let mut text = String::new();
    for run in runs {
        let index = run
            .manifest_index
            .ok_or(Error::Tree("live run without a manifest index"))?;
        text.push_str(&index.to_string());
        text.push('\n');
    }
    store.put(LIVE, text.as_bytes())?;
    Ok(())
}

/// Lookups at a tree size and root the caller trusts, without the writer
/// lock. Every answer carries inclusion paths against that size, so a caller
/// holding a signed checkpoint for it proves each hit with the checkpoint's
/// root and nothing from the store.
pub struct Index<S: TileStore> {
    reader: Reader<S>,
    keys: Keys,
    live: Vec<RunManifest>,
    cache: RunCache,
}

impl<S: TileStore> Index<S> {
    /// Opens the lookups at `size` with `root`, with its own run cache.
    pub fn at(store: S, size: u64, root: [u8; 32], keys: Keys) -> Result<Self, Error> {
        Self::with_cache(store, size, root, keys, Arc::default())
    }

    /// Like [`Index::at`], sharing `cache` with other lookups on the same log.
    pub fn with_cache(
        store: S,
        size: u64,
        root: [u8; 32],
        keys: Keys,
        cache: RunCache,
    ) -> Result<Self, Error> {
        let reader = Reader::at(store, size, root, tiles_keys(&keys))?;
        let (live, _) = load_live(&reader, size)?;
        Ok(Self {
            reader,
            keys,
            live,
            cache,
        })
    }

    /// The runs a lookup goes through, oldest first.
    #[must_use]
    pub fn live_runs(&self) -> &[RunManifest] {
        &self.live
    }

    /// The tree size the lookups answer at.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.reader.size()
    }

    /// The tiles reader underneath, for entries and proofs.
    pub fn reader(&self) -> &Reader<S> {
        &self.reader
    }

    fn last_sealed(&self) -> Option<u64> {
        self.live.last().map(|r| *r.entries.end())
    }

    fn run(&self, run: &RunManifest) -> Result<Arc<Run>, Error> {
        let manifest_index = run
            .manifest_index
            .ok_or(Error::Tree("live run without a manifest index"))?;
        if let Some(hit) = self
            .cache
            .lock()
            .map_err(|_| Error::Tree("run cache poisoned"))?
            .get(&manifest_index)
        {
            return Ok(Arc::clone(hit));
        }
        let path = run_path(&run.entries);
        let bytes = self
            .reader
            .store()
            .get(&path)?
            .ok_or(Error::Tampered { what: "run file" })?;
        let decoded = Arc::new(Run::decode(&bytes, run)?);
        self.cache
            .lock()
            .map_err(|_| Error::Tree("run cache poisoned"))?
            .insert(manifest_index, Arc::clone(&decoded));
        Ok(decoded)
    }

    fn inclusion(&self, index: u64) -> Result<Inclusion, Error> {
        let path = self
            .reader
            .inclusion(index)?
            .ok_or(Error::Tree("no inclusion path below the tree size"))?;
        Ok(Inclusion {
            index,
            tree_size: self.reader.size(),
            path,
        })
    }

    fn located(&self, index: u64, run: Option<&RunManifest>) -> Result<Located, Error> {
        let run = match run {
            Some(run) => {
                let manifest_index = run
                    .manifest_index
                    .ok_or(Error::Tree("live run without a manifest index"))?;
                Some(self.inclusion(manifest_index)?)
            }
            None => None,
        };
        Ok(Located {
            entry: self.inclusion(index)?,
            run,
        })
    }

    /// The `(key, index)` pairs of the entries after the last sealed one.
    fn tail_pairs(&self) -> Result<Vec<(Vec<u8>, u64)>, Error> {
        let from = self.last_sealed().map_or(0, |last| last + 1);
        let mut pairs = Vec::new();
        for index in from..self.reader.size() {
            let Some(entry) = self.reader.get(index)? else {
                return Err(Error::Tree("entry missing below the tree size"));
            };
            if entry.starts_with(MANIFEST_MAGIC) {
                continue;
            }
            pairs.push((identity(&entry).to_vec(), index));
            for key in (self.keys)(&entry) {
                pairs.push((key, index));
            }
        }
        Ok(pairs)
    }

    fn require_coverage(&self) -> Result<(), Error> {
        if contiguous(&self.live) {
            Ok(())
        } else {
            Err(Error::Tree("live runs do not cover the sealed range"))
        }
    }

    /// Every entry carrying `key`, each with the run that located it; see
    /// [`AuthenticatedLog::find`].
    pub fn find(&self, key: &[u8]) -> Result<Vec<Located>, Error> {
        self.require_coverage()?;
        let mut found = Vec::new();
        for run in &self.live {
            if key < run.key_lo.as_slice() || key > run.key_hi.as_slice() {
                continue;
            }
            for index in self.run(run)?.indices_of(key) {
                found.push(self.located(index, Some(run))?);
            }
        }
        for (k, index) in self.tail_pairs()? {
            if k == key {
                found.push(self.located(index, None)?);
            }
        }
        Ok(found)
    }

    /// Every entry whose keys fall in `range`, in key order, with their runs;
    /// see [`AuthenticatedLog::range`].
    pub fn range(&self, range: RangeInclusive<&[u8]>) -> Result<Vec<Located>, Error> {
        self.require_coverage()?;
        let mut hits: Vec<(Vec<u8>, u64, Option<usize>)> = Vec::new();
        for (n, run) in self.live.iter().enumerate() {
            if run.key_hi.as_slice() < *range.start() || run.key_lo.as_slice() > *range.end() {
                continue;
            }
            for (k, index) in self.run(run)?.pairs_in(&range) {
                hits.push((k, index, Some(n)));
            }
        }
        for (k, index) in self.tail_pairs()? {
            if k.as_slice() >= *range.start() && k.as_slice() <= *range.end() {
                hits.push((k, index, None));
            }
        }
        hits.sort();
        hits.into_iter()
            .map(|(_, index, run)| self.located(index, run.map(|n| &self.live[n])))
            .collect()
    }
}

/// The store: one writer over a tile store, the live runs, and the size a
/// trusted checkpoint covers.
pub struct Store<S: TileStore> {
    writer: Writer<S>,
    keys: Keys,
    live: Vec<RunManifest>,
    cache: RunCache,
    trusted: Option<(u64, [u8; 32])>,
}

impl<S: TileStore> Store<S> {
    /// Opens the log on `store` with the writer lock, rebuilding the live set
    /// from the logged manifests when the cache does not match them.
    pub fn open(store: S, keys: Keys) -> Result<Self, Error> {
        let writer = Writer::open(store, tiles_keys(&keys))?;
        let mut this = Self {
            writer,
            keys,
            live: Vec::new(),
            cache: Arc::default(),
            trusted: None,
        };
        this.reload_live()?;
        Ok(this)
    }

    /// Like [`Store::open`], trusting `root` as the root at `size`: what
    /// [`AuthenticatedLog::recover`] proves the current tree consistent with.
    /// The pair comes from a signed checkpoint kept where the store's writer
    /// cannot reach it.
    pub fn open_trusting(store: S, keys: Keys, size: u64, root: [u8; 32]) -> Result<Self, Error> {
        let mut this = Self::open(store, keys)?;
        this.trusted = Some((size, root));
        Ok(this)
    }

    /// The runs a lookup goes through, oldest first.
    #[must_use]
    pub fn live_runs(&self) -> &[RunManifest] {
        &self.live
    }

    /// The number of entries in the tree.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.writer.size()
    }

    /// The root of the tree.
    #[must_use]
    pub fn root(&self) -> [u8; 32] {
        self.writer.root()
    }

    /// The underlying writer, for checkpoints.
    pub fn writer(&self) -> &Writer<S> {
        &self.writer
    }

    /// Lookups at the current tree, sharing this store's run cache.
    pub fn index(&self) -> Result<Index<&S>, Error> {
        Index::with_cache(
            self.writer.store(),
            self.writer.size(),
            self.writer.root(),
            Arc::clone(&self.keys),
            Arc::clone(&self.cache),
        )
    }

    /// Appends entries in order and returns their index range; the batch is
    /// written once. Nothing is written when an entry starts with the
    /// manifest magic or is over the tiles entry length.
    pub fn append_batch<I>(&mut self, entries: I) -> Result<std::ops::Range<u64>, Error>
    where
        I: IntoIterator<Item = Vec<u8>>,
    {
        let entries: Vec<Vec<u8>> = entries.into_iter().collect();
        if entries.iter().any(|e| e.starts_with(MANIFEST_MAGIC)) {
            return Err(Error::Tree(
                "an entry may not start with the manifest magic",
            ));
        }
        Ok(self.writer.append_batch(entries)?)
    }

    fn reload_live(&mut self) -> Result<(), Error> {
        let reader = Reader::at(
            self.writer.store(),
            self.writer.size(),
            self.writer.root(),
            tiles_keys(&self.keys),
        )?;
        let (live, rebuilt) = load_live(&reader, self.writer.size())?;
        drop(reader);
        if rebuilt {
            save_live(self.writer.store(), &live)?;
        }
        self.live = live;
        Ok(())
    }

    fn last_sealed(&self) -> Option<u64> {
        self.live.last().map(|r| *r.entries.end())
    }

    fn log_run(
        &mut self,
        entries: RangeInclusive<u64>,
        pairs: Vec<(Vec<u8>, u64)>,
        replaced: Vec<u64>,
    ) -> Result<RunManifest, Error> {
        let bytes = Run::encode(&entries, &pairs)?;
        let path = run_path(&entries);
        if !self.writer.store().create_new(&path, &bytes)? {
            return Err(Error::Tree("a run for that entry range is already stored"));
        }
        let (key_lo, key_hi) = match (pairs.first(), pairs.last()) {
            (Some(first), Some(last)) => (first.0.clone(), last.0.clone()),
            _ => (Vec::new(), Vec::new()),
        };
        let run = RunManifest {
            entries,
            key_lo,
            key_hi,
            run_hash: identity(&bytes),
            manifest_index: None,
        };
        let logged = Logged { run, replaced };
        let manifest_index = self.writer.append(&logged.encode()?)?;
        let mut run = logged.run;
        run.manifest_index = Some(manifest_index);
        Ok(run)
    }
}

impl<S: TileStore> AuthenticatedLog for Store<S> {
    fn append(&mut self, entry: &[u8]) -> Result<u64, Error> {
        Ok(self.append_batch([entry.to_vec()])?.start)
    }

    fn find(&self, key: &[u8]) -> Result<Vec<Located>, Error> {
        self.index()?.find(key)
    }

    fn range(&self, range: RangeInclusive<&[u8]>) -> Result<Vec<Located>, Error> {
        self.index()?.range(range)
    }

    fn seal(&mut self) -> Result<RunManifest, Error> {
        let index = self.index()?;
        index.require_coverage()?;
        let lo = self.last_sealed().map_or(0, |last| last + 1);
        let size = self.writer.size();
        if lo >= size {
            return Err(Error::Tree("nothing to seal"));
        }
        let mut pairs = index.tail_pairs()?;
        drop(index);
        if pairs.is_empty() {
            // Only manifest entries since the last seal: nothing to index.
            // They fall into the range of the next run that has keys.
            return Err(Error::Tree("nothing to seal"));
        }
        pairs.sort();
        pairs.dedup();
        let run = self.log_run(lo..=size - 1, pairs, Vec::new())?;
        self.live.push(run.clone());
        save_live(self.writer.store(), &self.live)?;
        Ok(run)
    }

    fn compact(&mut self) -> Result<Compaction, Error> {
        let index = self.index()?;
        index.require_coverage()?;
        if self.live.len() < 2 {
            return Err(Error::Tree("fewer than two runs to compact"));
        }
        let mut merged: BTreeSet<(Vec<u8>, u64)> = BTreeSet::new();
        let mut replaced = Vec::new();
        for run in &self.live {
            merged.extend(index.run(run)?.pairs.iter().cloned());
            replaced.push(
                run.manifest_index
                    .ok_or(Error::Tree("live run without a manifest index"))?,
            );
        }
        drop(index);
        let lo = *self.live[0].entries.start();
        let hi = *self.live[self.live.len() - 1].entries.end();
        let run = self.log_run(lo..=hi, merged.into_iter().collect(), replaced.clone())?;
        let written = run.manifest_index.expect("logged");
        self.live = vec![run];
        save_live(self.writer.store(), &self.live)?;
        Ok(Compaction { replaced, written })
    }

    fn recover(&mut self) -> Result<Recovery, Error> {
        let (checkpoint_size, checkpoint_root) = self
            .trusted
            .ok_or(Error::Tree("no trusted checkpoint to recover from"))?;
        let size = self.writer.size();
        if checkpoint_size > size {
            return Err(Error::Tampered { what: "tree state" });
        }
        let consistency = if checkpoint_size == 0 {
            Vec::new()
        } else {
            self.index()?
                .reader()
                .consistency(checkpoint_size)?
                .ok_or(Error::Tree("no consistency path from the checkpoint"))?
        };
        let old_size =
            usize::try_from(checkpoint_size).map_err(|_| Error::Tree("size over usize"))?;
        let new_size = usize::try_from(size).map_err(|_| Error::Tree("size over usize"))?;
        if checkpoint_size > 0
            && !rfc9162::verify_consistency(
                old_size,
                &rfc9162::hex(&checkpoint_root),
                new_size,
                &rfc9162::hex(&self.writer.root()),
                &consistency,
            )
        {
            return Err(Error::Tampered { what: "tree state" });
        }
        self.reload_live()?;
        let (quarantined, _) = self.writer.quarantined();
        Ok(Recovery {
            checkpoint_size,
            replayed: size - checkpoint_size,
            quarantined: u64::try_from(quarantined.len()).unwrap_or(u64::MAX),
            consistency,
        })
    }
}

impl From<rfc9162_tiles::Error> for Error {
    fn from(e: rfc9162_tiles::Error) -> Self {
        match e {
            rfc9162_tiles::Error::Io(e) => Self::Store(e),
            rfc9162_tiles::Error::Corrupt { .. } => Self::Tampered { what: "tile store" },
            other => Self::Tiles(other.to_string()),
        }
    }
}
