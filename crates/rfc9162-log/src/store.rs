//! The store behind [`AuthenticatedLog`]: sorted runs over [`rfc9162_tiles`],
//! each run's manifest appended to a log so a signed checkpoint covers the
//! index as well as the entries.
//!
//! Two layouts, one code path:
//!
//! - **Shared**: the manifests are entries of the data log itself. One tree,
//!   one checkpoint signs entries and index. [`Store::open`], [`Index::at`].
//! - **Split**: the manifests are entries of a second tile log, the index
//!   log, each carrying the data tree's size and root it indexes. The data
//!   log holds exactly the caller's entries, which a consumer that already
//!   signs its data leaves needs; the index log has its own checkpoint under
//!   the same key. [`Store::open_split`], [`Index::at_split`].
//!
//! Inside the store that holds the manifests (the data store when shared,
//! the index store when split), beside the tiles:
//!
//! - `runs/<lo>-<hi>`: one sealed run, the sorted `(key, index)` pairs of
//!   data entries `lo..=hi` and a Bloom filter over the keys, in the format
//!   `RUN_MAGIC` describes. Its SHA-256 is the `run_hash` the logged manifest
//!   carries, so a reader checks the file before trusting a seek. The writer
//!   writes format v2: each key is stored as the length it shares with the
//!   key before it and the bytes that follow (front coding), and each index
//!   as a LEB128 varint offset from `lo`. Runs written as v1 (every key and
//!   index in full) stay readable; the manifest binds the file's hash, not
//!   its format.
//! - `runs/live`: the manifest indices of the runs a lookup goes through, one
//!   per line, oldest first. A cache: when it is missing or stale the live set
//!   is rebuilt from the manifest entries in the log.
//!
//! A manifest entry starts with `MANIFEST_MAGIC`, carries no lookup keys and,
//! when shared, lies in the entry range of the run sealed after it.
//! Compaction logs a manifest that names the manifest indices it replaces;
//! the replaced runs stay in the store and in the tree, and leave the live
//! set.
//!
//! [`Store`] is the one writer and holds the writer locks. [`Index`] answers
//! lookups at tree sizes and roots a caller trusts, without the locks, which
//! is how a verifier that holds signed checkpoints reads the log.

use std::collections::{BTreeSet, HashMap};
use std::ops::RangeInclusive;
use std::sync::{Arc, Mutex};

use rfc9162_tiles::bloom::Bloom;
use rfc9162_tiles::index::{identity, KeyFn};
use rfc9162_tiles::log::{Reader, Writer};
use rfc9162_tiles::store::TileStore;

use crate::{AuthenticatedLog, Compaction, Error, Inclusion, Located, Recovery, RunManifest};

/// First bytes of a logged run manifest.
pub const MANIFEST_MAGIC: &[u8] = b"rfc9162-log/manifest/v2\n";
/// First bytes of a run file in format v2 (front-coded, one sequence),
/// which readers accept.
pub const RUN_MAGIC: &[u8] = b"rfc9162-log/run/v2\n";
/// First bytes of a run file as the writer writes it (format v3: front-coded
/// blocks of [`BLOCK_PAIRS`] pairs behind a block index).
pub const RUN_MAGIC_V3: &[u8] = b"rfc9162-log/run/v3\n";
/// Pairs per block in a v3 run. Each block restarts its front coding, so a
/// lookup decodes only the blocks whose key range can hold the key.
pub const BLOCK_PAIRS: usize = 64;
/// First bytes of a run file in format v1, which readers still accept.
pub const RUN_MAGIC_V1: &[u8] = b"rfc9162-log/run/v1\n";
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

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Bytes `a` and `b` share at their start.
fn shared_prefix(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
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
    /// A LEB128 varint of at most ten bytes whose value fits in a `u64`.
    fn varint(&mut self) -> Result<u64, Error> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.take(1)?[0];
            let low = u64::from(byte & 0x7f);
            if shift == 63 && low > 1 {
                return Err(Error::Tampered { what: self.what });
            }
            value |= low << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(Error::Tampered { what: self.what })
    }
    fn done(&self) -> Result<(), Error> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(Error::Tampered { what: self.what })
        }
    }
}

/// A logged manifest, as the entry carries it: the run it describes, the
/// data tree (size and root) at the moment it was sealed, and the manifest
/// indices of the runs it replaced (none for a seal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logged {
    /// The run.
    pub run: RunManifest,
    /// Entries of the data tree when the run was sealed.
    pub data_size: u64,
    /// Root of the data tree when the run was sealed.
    pub data_root: [u8; 32],
    /// Manifest indices of the runs this one replaced.
    pub replaced: Vec<u64>,
}

impl Logged {
    fn encode(&self) -> Result<Vec<u8>, Error> {
        let mut out = MANIFEST_MAGIC.to_vec();
        put_u64(&mut out, *self.run.entries.start());
        put_u64(&mut out, *self.run.entries.end());
        put_bytes(&mut out, &self.run.key_lo)?;
        put_bytes(&mut out, &self.run.key_hi)?;
        out.extend_from_slice(&self.run.run_hash);
        put_u64(&mut out, self.data_size);
        out.extend_from_slice(&self.data_root);
        let n = u32::try_from(self.replaced.len())
            .map_err(|_| Error::Tree("too many replaced runs"))?;
        out.extend_from_slice(&n.to_be_bytes());
        for index in &self.replaced {
            put_u64(&mut out, *index);
        }
        Ok(out)
    }

    /// Decodes the manifest entry at `manifest_index`. `shared` says the
    /// manifest sits in the data tree, where its index has to lie past the
    /// run it describes.
    pub fn decode(bytes: &[u8], manifest_index: u64, shared: bool) -> Result<Self, Error> {
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
        let data_size = c.u64()?;
        let data_root: [u8; 32] = c.take(32)?.try_into().expect("32 bytes");
        let n = c.u32()?;
        let mut replaced = Vec::with_capacity(n as usize);
        for _ in 0..n {
            replaced.push(c.u64()?);
        }
        c.done()?;
        if lo > hi || hi >= data_size || (shared && hi >= manifest_index) {
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
            data_size,
            data_root,
            replaced,
        })
    }
}

/// A run file, checked against its manifest. Runs in format v1 and v2 are
/// decoded whole into `pairs`; a v3 run keeps its bytes and its block index
/// in `blocks` and decodes a block when a lookup reaches it.
pub struct Run {
    pairs: Vec<(Vec<u8>, u64)>,
    blocks: Option<Blocks>,
    filter: Bloom,
}

/// The block index and body of a v3 run.
struct Blocks {
    lo: u64,
    hi: u64,
    body: Vec<u8>,
    index: Vec<BlockRef>,
}

/// One block: its first key and where its bytes sit in the body.
struct BlockRef {
    first: Vec<u8>,
    start: usize,
    end: usize,
    count: usize,
}

impl Blocks {
    /// The pairs of block `b`, checked: the first key matches the index, the
    /// pairs rise strictly and every index lies in the run's entry range.
    fn decode(&self, b: usize) -> Result<Vec<(Vec<u8>, u64)>, Error> {
        let what = "run block";
        let block = &self.index[b];
        let mut c = Cursor {
            bytes: &self.body[block.start..block.end],
            at: 0,
            what,
        };
        let mut pairs: Vec<(Vec<u8>, u64)> = Vec::with_capacity(block.count);
        for _ in 0..block.count {
            let shared = c.varint()?;
            let unshared = c.varint()?;
            let previous = pairs.last().map(|(k, _)| k.as_slice()).unwrap_or(&[]);
            if shared > previous.len() as u64
                || shared.saturating_add(unshared) > u64::from(u16::MAX)
            {
                return Err(Error::Tampered { what });
            }
            let mut key = previous[..shared as usize].to_vec();
            key.extend_from_slice(c.take(unshared as usize)?);
            let index = self
                .lo
                .checked_add(c.varint()?)
                .ok_or(Error::Tampered { what })?;
            if index > self.hi {
                return Err(Error::Tampered { what });
            }
            if let Some(last) = pairs.last() {
                if (&key, index) <= (&last.0, last.1) {
                    return Err(Error::Tampered { what });
                }
            }
            pairs.push((key, index));
        }
        c.done()?;
        match pairs.first() {
            Some((key, _)) if *key == block.first => Ok(pairs),
            _ => Err(Error::Tampered { what }),
        }
    }

    /// Blocks that can hold keys from `lo` on: from the last block whose first
    /// key is below `lo` (the key may start inside it), in order.
    fn from(&self, lo: &[u8]) -> usize {
        self.index
            .partition_point(|b| b.first.as_slice() < lo)
            .saturating_sub(1)
    }
}

impl Run {
    /// A v3 run: the pairs in blocks of [`BLOCK_PAIRS`], each front-coded from
    /// an empty key, behind an index of each block's first key, pair count and
    /// byte length.
    fn encode_blocks(
        entries: &RangeInclusive<u64>,
        pairs: &[(Vec<u8>, u64)],
    ) -> Result<Vec<u8>, Error> {
        let lo = *entries.start();
        let mut bodies: Vec<Vec<u8>> = Vec::new();
        for chunk in pairs.chunks(BLOCK_PAIRS) {
            let mut body = Vec::new();
            let mut previous: &[u8] = &[];
            for (key, index) in chunk {
                if key.len() > usize::from(u16::MAX) {
                    return Err(Error::Tree("key over 65535 bytes"));
                }
                let offset = index
                    .checked_sub(lo)
                    .ok_or(Error::Tree("a run pair below the run's entry range"))?;
                let shared = shared_prefix(previous, key);
                put_varint(&mut body, shared as u64);
                put_varint(&mut body, (key.len() - shared) as u64);
                body.extend_from_slice(&key[shared..]);
                put_varint(&mut body, offset);
                previous = key;
            }
            bodies.push(body);
        }
        let mut out = RUN_MAGIC_V3.to_vec();
        put_u64(&mut out, lo);
        put_u64(&mut out, *entries.end());
        put_varint(&mut out, pairs.len() as u64);
        put_varint(&mut out, bodies.len() as u64);
        for (chunk, body) in pairs.chunks(BLOCK_PAIRS).zip(&bodies) {
            let first = &chunk[0].0;
            put_varint(&mut out, first.len() as u64);
            out.extend_from_slice(first);
            put_varint(&mut out, chunk.len() as u64);
            put_varint(&mut out, body.len() as u64);
        }
        for body in &bodies {
            out.extend_from_slice(body);
        }
        let keys: Vec<&[u8]> = pairs.iter().map(|(k, _)| k.as_slice()).collect();
        let filter = Bloom::build(&keys, BITS_PER_KEY).to_bytes();
        put_varint(&mut out, filter.len() as u64);
        out.extend_from_slice(&filter);
        Ok(out)
    }

    /// Reads a v3 run's header, block index and filter; blocks stay encoded.
    fn decode_blocks(body: &[u8], expect: &RunManifest) -> Result<Self, Error> {
        let what = "run file";
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
        let n = c.varint()?;
        let nblocks = c.varint()?;
        if n > (body.len() as u64) / 3 || nblocks > n.max(1) {
            return Err(Error::Tampered { what });
        }
        let mut heads = Vec::with_capacity(nblocks as usize);
        let mut total = 0u64;
        for _ in 0..nblocks {
            let len = c.varint()?;
            if len > u64::from(u16::MAX) {
                return Err(Error::Tampered { what });
            }
            let first = c.take(len as usize)?.to_vec();
            let count = c.varint()?;
            let bytes = c.varint()?;
            if count == 0 || count > BLOCK_PAIRS as u64 || bytes == 0 {
                return Err(Error::Tampered { what });
            }
            if let Some((previous, _, _)) = heads.last() {
                if first < *previous {
                    return Err(Error::Tampered { what });
                }
            }
            total += count;
            heads.push((first, count as usize, bytes as usize));
        }
        if total != n {
            return Err(Error::Tampered { what });
        }
        let mut index = Vec::with_capacity(heads.len());
        let mut start = 0usize;
        let mut blob = Vec::new();
        for (first, count, bytes) in heads {
            blob.extend_from_slice(c.take(bytes)?);
            index.push(BlockRef {
                first,
                start,
                end: start + bytes,
                count,
            });
            start += bytes;
        }
        let len = usize::try_from(c.varint()?).map_err(|_| Error::Tampered { what })?;
        let filter = Bloom::from_bytes(c.take(len)?).map_err(|_| Error::Tampered { what })?;
        c.done()?;
        Ok(Self {
            pairs: Vec::new(),
            blocks: Some(Blocks {
                lo,
                hi,
                body: blob,
                index,
            }),
            filter,
        })
    }

    /// Every pair of the run, in order.
    fn all_pairs(&self) -> Result<Vec<(Vec<u8>, u64)>, Error> {
        match &self.blocks {
            None => Ok(self.pairs.clone()),
            Some(blocks) => {
                let mut out = Vec::new();
                for b in 0..blocks.index.len() {
                    out.extend(blocks.decode(b)?);
                }
                Ok(out)
            }
        }
    }

    /// A v2 run (one front-coded sequence); the writer writes v3.
    #[cfg(test)]
    fn encode(entries: &RangeInclusive<u64>, pairs: &[(Vec<u8>, u64)]) -> Result<Vec<u8>, Error> {
        let lo = *entries.start();
        let mut out = RUN_MAGIC.to_vec();
        put_u64(&mut out, lo);
        put_u64(&mut out, *entries.end());
        put_varint(&mut out, pairs.len() as u64);
        let mut previous: &[u8] = &[];
        for (key, index) in pairs {
            if key.len() > usize::from(u16::MAX) {
                return Err(Error::Tree("key over 65535 bytes"));
            }
            let offset = index
                .checked_sub(lo)
                .ok_or(Error::Tree("a run pair below the run's entry range"))?;
            let shared = shared_prefix(previous, key);
            put_varint(&mut out, shared as u64);
            put_varint(&mut out, (key.len() - shared) as u64);
            out.extend_from_slice(&key[shared..]);
            put_varint(&mut out, offset);
            previous = key;
        }
        let keys: Vec<&[u8]> = pairs.iter().map(|(k, _)| k.as_slice()).collect();
        let filter = Bloom::build(&keys, BITS_PER_KEY).to_bytes();
        put_varint(&mut out, filter.len() as u64);
        out.extend_from_slice(&filter);
        Ok(out)
    }

    #[cfg(test)]
    fn encode_v1(
        entries: &RangeInclusive<u64>,
        pairs: &[(Vec<u8>, u64)],
    ) -> Result<Vec<u8>, Error> {
        let mut out = RUN_MAGIC_V1.to_vec();
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
        if let Some(body) = bytes.strip_prefix(RUN_MAGIC_V3) {
            return Self::decode_blocks(body, expect);
        }
        let (v2, body) = if let Some(body) = bytes.strip_prefix(RUN_MAGIC) {
            (true, body)
        } else if let Some(body) = bytes.strip_prefix(RUN_MAGIC_V1) {
            (false, body)
        } else {
            return Err(Error::Tampered { what });
        };
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
        let n = if v2 { c.varint()? } else { u64::from(c.u32()?) };
        // A pair takes at least three bytes in v2 and ten in v1, so a count the
        // body cannot hold is refused before anything is allocated for it.
        if n > (body.len() as u64) / 3 {
            return Err(Error::Tampered { what });
        }
        let mut pairs: Vec<(Vec<u8>, u64)> = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let (key, index) = if v2 {
                let shared = c.varint()?;
                let unshared = c.varint()?;
                let previous = pairs.last().map(|(k, _)| k.as_slice()).unwrap_or(&[]);
                if shared > previous.len() as u64
                    || shared.saturating_add(unshared) > u64::from(u16::MAX)
                {
                    return Err(Error::Tampered { what });
                }
                let mut key = previous[..shared as usize].to_vec();
                key.extend_from_slice(c.take(unshared as usize)?);
                let index = lo
                    .checked_add(c.varint()?)
                    .ok_or(Error::Tampered { what })?;
                (key, index)
            } else {
                let key = c.bytes()?.to_vec();
                (key, c.u64()?)
            };
            if !expect.entries.contains(&index) {
                return Err(Error::Tampered { what });
            }
            pairs.push((key, index));
        }
        if pairs.windows(2).any(|w| w[0] >= w[1]) {
            return Err(Error::Tampered { what });
        }
        let len = if v2 { c.varint()? } else { u64::from(c.u32()?) };
        let len = usize::try_from(len).map_err(|_| Error::Tampered { what })?;
        let filter = Bloom::from_bytes(c.take(len)?).map_err(|_| Error::Tampered { what })?;
        c.done()?;
        Ok(Self {
            pairs,
            blocks: None,
            filter,
        })
    }

    /// The entry indices recorded for `key`, as the unit tests read them.
    #[cfg(test)]
    fn indices_of(&self, key: &[u8]) -> Vec<u64> {
        self.lookup(key).expect("a checked run")
    }

    fn lookup(&self, key: &[u8]) -> Result<Vec<u64>, Error> {
        if !self.filter.contains(key) {
            return Ok(Vec::new());
        }
        Ok(self
            .pairs_in(&(key..=key))?
            .into_iter()
            .map(|(_, i)| i)
            .collect())
    }

    fn pairs_in(&self, range: &RangeInclusive<&[u8]>) -> Result<Vec<(Vec<u8>, u64)>, Error> {
        let Some(blocks) = &self.blocks else {
            let start = self
                .pairs
                .partition_point(|(k, _)| k.as_slice() < *range.start());
            return Ok(self.pairs[start..]
                .iter()
                .take_while(|(k, _)| k.as_slice() <= *range.end())
                .cloned()
                .collect());
        };
        let mut out = Vec::new();
        for b in blocks.from(range.start())..blocks.index.len() {
            if blocks.index[b].first.as_slice() > *range.end() {
                break;
            }
            out.extend(
                blocks
                    .decode(b)?
                    .into_iter()
                    .filter(|(k, _)| range.contains(&k.as_slice())),
            );
        }
        Ok(out)
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

/// The live runs whose manifests sit below `size` in the manifest log: the
/// cache at `runs/live` when every line names a manifest entry below `size`
/// and the runs are contiguous, else a rebuild from the manifest entries in
/// `0..size` with the replacements applied. The flag says whether it was
/// rebuilt.
fn load_live<S: TileStore>(
    manifests: &Reader<S>,
    size: u64,
    shared: bool,
) -> Result<(Vec<RunManifest>, bool), Error> {
    if let Some(bytes) = manifests.store().get(LIVE)? {
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
            match manifests.get(index)? {
                Some(entry) if entry.starts_with(MANIFEST_MAGIC) => {
                    runs.push(Logged::decode(&entry, index, shared)?.run);
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
        let Some(entry) = manifests.get(index)? else {
            return Err(Error::Tree("entry missing below the tree size"));
        };
        if !entry.starts_with(MANIFEST_MAGIC) {
            continue;
        }
        let logged = Logged::decode(&entry, index, shared)?;
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

/// Lookups at tree sizes and roots the caller trusts, without the writer
/// locks. Every answer carries inclusion paths against those sizes: the
/// entry's against the data tree, the run manifest's against the tree that
/// holds the manifests (the same tree when shared), so a caller holding the
/// signed checkpoints proves each hit with their roots and nothing from the
/// store.
pub struct Index<S: TileStore> {
    data: Reader<S>,
    manifests: Option<Reader<S>>,
    keys: Keys,
    live: Vec<RunManifest>,
    cache: RunCache,
}

impl<S: TileStore> Index<S> {
    /// Opens the lookups on a shared log at `size` with `root`.
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
        let data = Reader::at(store, size, root, tiles_keys(&keys))?;
        let (live, _) = load_live(&data, size, true)?;
        Ok(Self {
            data,
            manifests: None,
            keys,
            live,
            cache,
        })
    }

    /// Opens the lookups on a split log: the data tree at `data_size` with
    /// `data_root` and the index log at `index_size` with `index_root`.
    #[allow(clippy::too_many_arguments)]
    pub fn at_split(
        data_store: S,
        data_size: u64,
        data_root: [u8; 32],
        index_store: S,
        index_size: u64,
        index_root: [u8; 32],
        keys: Keys,
        cache: RunCache,
    ) -> Result<Self, Error> {
        let data = Reader::at(data_store, data_size, data_root, tiles_keys(&keys))?;
        let manifests = Reader::at(index_store, index_size, index_root, tiles_keys(&keys))?;
        let (live, _) = load_live(&manifests, index_size, false)?;
        // A run that indexes entries past this data tree is not usable here.
        let live = live
            .into_iter()
            .take_while(|run| *run.entries.end() < data_size)
            .collect();
        Ok(Self {
            data,
            manifests: Some(manifests),
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

    /// The data tree size the lookups answer at.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.data.size()
    }

    /// The tiles reader of the data log, for entries and proofs.
    pub fn reader(&self) -> &Reader<S> {
        &self.data
    }

    /// The tiles reader that holds the manifests: the index log's when split,
    /// the data log's when shared.
    pub fn manifest_reader(&self) -> &Reader<S> {
        self.manifests.as_ref().unwrap_or(&self.data)
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
            .manifest_reader()
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

    fn inclusion_in(reader: &Reader<S>, index: u64) -> Result<Inclusion, Error> {
        let path = reader
            .inclusion(index)?
            .ok_or(Error::Tree("no inclusion path below the tree size"))?;
        Ok(Inclusion {
            index,
            tree_size: reader.size(),
            path,
        })
    }

    fn located(&self, index: u64, run: Option<&RunManifest>) -> Result<Located, Error> {
        let run = match run {
            Some(run) => {
                let manifest_index = run
                    .manifest_index
                    .ok_or(Error::Tree("live run without a manifest index"))?;
                Some(Self::inclusion_in(self.manifest_reader(), manifest_index)?)
            }
            None => None,
        };
        Ok(Located {
            entry: Self::inclusion_in(&self.data, index)?,
            run,
        })
    }

    /// The `(key, index)` pairs of the data entries after the last sealed one.
    fn tail_pairs(&self) -> Result<Vec<(Vec<u8>, u64)>, Error> {
        let from = self.last_sealed().map_or(0, |last| last + 1);
        let mut pairs = Vec::new();
        for index in from..self.data.size() {
            let Some(entry) = self.data.get(index)? else {
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
            for index in self.run(run)?.lookup(key)? {
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
            for (k, index) in self.run(run)?.pairs_in(&range)? {
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

/// The store: the data writer, the manifest writer when split, the live
/// runs, and the size a trusted checkpoint covers.
pub struct Store<S: TileStore> {
    data: Writer<S>,
    manifests: Option<Writer<S>>,
    keys: Keys,
    live: Vec<RunManifest>,
    cache: RunCache,
    trusted: Option<(u64, [u8; 32])>,
}

impl<S: TileStore> Store<S> {
    /// Opens a shared log on `store` with the writer lock, rebuilding the
    /// live set from the logged manifests when the cache does not match them.
    pub fn open(store: S, keys: Keys) -> Result<Self, Error> {
        let data = Writer::open(store, tiles_keys(&keys))?;
        let mut this = Self {
            data,
            manifests: None,
            keys,
            live: Vec::new(),
            cache: Arc::default(),
            trusted: None,
        };
        this.reload_live()?;
        Ok(this)
    }

    /// Opens a split log: entries go to `data`, run files and manifests to
    /// `index`. Both writer locks are held.
    pub fn open_split(data: S, index: S, keys: Keys) -> Result<Self, Error> {
        let data = Writer::open(data, tiles_keys(&keys))?;
        let manifests = Writer::open(index, tiles_keys(&keys))?;
        let mut this = Self {
            data,
            manifests: Some(manifests),
            keys,
            live: Vec::new(),
            cache: Arc::default(),
            trusted: None,
        };
        this.reload_live()?;
        Ok(this)
    }

    /// Like [`Store::open`], trusting `root` as the root at `size`: what
    /// [`AuthenticatedLog::recover`] proves the current data tree consistent
    /// with. The pair comes from a signed checkpoint kept where the store's
    /// writer cannot reach it.
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

    /// The number of entries in the data tree.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.data.size()
    }

    /// The root of the data tree.
    #[must_use]
    pub fn root(&self) -> [u8; 32] {
        self.data.root()
    }

    /// The data writer, for checkpoints.
    pub fn writer(&self) -> &Writer<S> {
        &self.data
    }

    /// The index log's writer when split, for its own checkpoint.
    pub fn index_writer(&self) -> Option<&Writer<S>> {
        self.manifests.as_ref()
    }

    fn manifest_writer(&self) -> &Writer<S> {
        self.manifests.as_ref().unwrap_or(&self.data)
    }

    fn manifest_store(&self) -> &S {
        self.manifest_writer().store()
    }

    /// Lookups at the current trees, sharing this store's run cache.
    pub fn index(&self) -> Result<Index<&S>, Error> {
        match &self.manifests {
            None => Index::with_cache(
                self.data.store(),
                self.data.size(),
                self.data.root(),
                Arc::clone(&self.keys),
                Arc::clone(&self.cache),
            ),
            Some(manifests) => Index::at_split(
                self.data.store(),
                self.data.size(),
                self.data.root(),
                manifests.store(),
                manifests.size(),
                manifests.root(),
                Arc::clone(&self.keys),
                Arc::clone(&self.cache),
            ),
        }
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
        Ok(self.data.append_batch(entries)?)
    }

    fn reload_live(&mut self) -> Result<(), Error> {
        let shared = self.manifests.is_none();
        let writer = self.manifest_writer();
        let reader = Reader::at(
            writer.store(),
            writer.size(),
            writer.root(),
            tiles_keys(&self.keys),
        )?;
        let (live, rebuilt) = load_live(&reader, writer.size(), shared)?;
        drop(reader);
        if rebuilt {
            save_live(self.manifest_store(), &live)?;
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
        let bytes = Run::encode_blocks(&entries, &pairs)?;
        let path = run_path(&entries);
        if !self.manifest_store().create_new(&path, &bytes)? {
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
        let logged = Logged {
            run,
            data_size: self.data.size(),
            data_root: self.data.root(),
            replaced,
        };
        let encoded = logged.encode()?;
        let manifest_index = match &mut self.manifests {
            Some(manifests) => manifests.append(&encoded)?,
            None => self.data.append(&encoded)?,
        };
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
        let size = self.data.size();
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
        save_live(self.manifest_store(), &self.live)?;
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
            merged.extend(index.run(run)?.all_pairs()?);
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
        save_live(self.manifest_store(), &self.live)?;
        Ok(Compaction { replaced, written })
    }

    fn recover(&mut self) -> Result<Recovery, Error> {
        let (checkpoint_size, checkpoint_root) = self
            .trusted
            .ok_or(Error::Tree("no trusted checkpoint to recover from"))?;
        let size = self.data.size();
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
                &rfc9162::hex(&self.data.root()),
                &consistency,
            )
        {
            return Err(Error::Tampered { what: "tree state" });
        }
        self.reload_live()?;
        let (quarantined, _) = self.data.quarantined();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs() -> Vec<(Vec<u8>, u64)> {
        let mut pairs: Vec<(Vec<u8>, u64)> = (0u64..300)
            .flat_map(|i| {
                [
                    (
                        format!("module=GROMACS/2026.{}-foss-2026.1", i % 7).into_bytes(),
                        1000 + i,
                    ),
                    (
                        format!("path=software/PETSc/3.24.{i}/lib/libpetsc.so").into_bytes(),
                        1000 + i,
                    ),
                    (
                        format!("target=x86_64/amd/zen{}", 2 + i % 3).into_bytes(),
                        1000 + i,
                    ),
                ]
            })
            .collect();
        pairs.sort();
        pairs
    }

    fn manifest(entries: RangeInclusive<u64>, bytes: &[u8]) -> RunManifest {
        RunManifest {
            entries,
            key_lo: Vec::new(),
            key_hi: Vec::new(),
            run_hash: identity(bytes),
            manifest_index: None,
        }
    }

    #[test]
    fn v2_round_trips_and_answers_lookups() {
        let entries = 1000..=1299;
        let pairs = pairs();
        let bytes = Run::encode(&entries, &pairs).unwrap();
        assert!(bytes.starts_with(RUN_MAGIC));
        let run = Run::decode(&bytes, &manifest(entries, &bytes)).unwrap();
        assert_eq!(run.pairs, pairs);
        assert_eq!(run.indices_of(b"target=x86_64/amd/zen3").len(), 100);
        assert!(run.indices_of(b"target=x86_64/amd/zen5").is_empty());
    }

    #[test]
    fn v1_runs_stay_readable() {
        let entries = 1000..=1299;
        let pairs = pairs();
        let bytes = Run::encode_v1(&entries, &pairs).unwrap();
        let run = Run::decode(&bytes, &manifest(entries, &bytes)).unwrap();
        assert_eq!(run.pairs, pairs);
    }

    #[test]
    fn v2_is_smaller_than_v1_on_shared_prefixes() {
        let entries = 1000..=1299;
        let pairs = pairs();
        let v1 = Run::encode_v1(&entries, &pairs).unwrap().len();
        let v2 = Run::encode(&entries, &pairs).unwrap().len();
        assert!(v2 * 2 < v1, "v2 {v2} bytes, v1 {v1} bytes");
    }

    #[test]
    fn a_shared_length_past_the_previous_key_is_refused() {
        let entries = 0..=0;
        let mut bytes = RUN_MAGIC.to_vec();
        put_u64(&mut bytes, 0);
        put_u64(&mut bytes, 0);
        put_varint(&mut bytes, 1);
        put_varint(&mut bytes, 3);
        put_varint(&mut bytes, 1);
        bytes.push(b'k');
        put_varint(&mut bytes, 0);
        put_varint(&mut bytes, 0);
        let expect = manifest(entries, &bytes);
        assert!(matches!(
            Run::decode(&bytes, &expect),
            Err(Error::Tampered { .. })
        ));
    }

    #[test]
    fn an_index_outside_the_run_is_refused() {
        let entries = 10..=11;
        let bytes = Run::encode(&(10..=20), &[(b"k".to_vec(), 15)]).unwrap();
        let mut patched = RUN_MAGIC.to_vec();
        patched.extend_from_slice(&bytes[RUN_MAGIC.len()..]);
        // Rewrite the header so lo and hi match the manifest while the
        // pair's offset of 5 lands on 15, outside 10..=11.
        patched[RUN_MAGIC.len() + 8..RUN_MAGIC.len() + 16].copy_from_slice(&11u64.to_be_bytes());
        let expect = manifest(entries, &patched);
        assert!(matches!(
            Run::decode(&patched, &expect),
            Err(Error::Tampered { .. })
        ));
    }

    #[test]
    fn an_overlong_varint_is_refused() {
        let mut c = Cursor {
            bytes: &[0xff; 11],
            at: 0,
            what: "test",
        };
        assert!(c.varint().is_err());
        let mut out = Vec::new();
        put_varint(&mut out, u64::MAX);
        let mut c = Cursor {
            bytes: &out,
            at: 0,
            what: "test",
        };
        assert_eq!(c.varint().unwrap(), u64::MAX);
    }
}

#[cfg(test)]
mod block_tests {
    use super::*;

    fn manifest(entries: RangeInclusive<u64>, bytes: &[u8]) -> RunManifest {
        RunManifest {
            entries,
            key_lo: Vec::new(),
            key_hi: Vec::new(),
            run_hash: identity(bytes),
            manifest_index: None,
        }
    }

    /// 1000 entries; "module=hot" recurs 300 times so it spans several blocks.
    fn pairs() -> Vec<(Vec<u8>, u64)> {
        let mut pairs: Vec<(Vec<u8>, u64)> = (0u64..1000)
            .flat_map(|i| {
                let module = if i % 3 == 0 && i < 900 {
                    "module=hot".to_string()
                } else {
                    format!("module=m{i:04}")
                };
                [
                    (module.into_bytes(), i),
                    (format!("target=t{}", i % 4).into_bytes(), i),
                ]
            })
            .collect();
        pairs.sort();
        pairs
    }

    fn v3(pairs: &[(Vec<u8>, u64)]) -> Run {
        let bytes = Run::encode_blocks(&(0..=999), pairs).unwrap();
        assert!(bytes.starts_with(RUN_MAGIC_V3));
        Run::decode(&bytes, &manifest(0..=999, &bytes)).unwrap()
    }

    fn brute(pairs: &[(Vec<u8>, u64)], lo: &[u8], hi: &[u8]) -> Vec<(Vec<u8>, u64)> {
        pairs
            .iter()
            .filter(|(k, _)| k.as_slice() >= lo && k.as_slice() <= hi)
            .cloned()
            .collect()
    }

    #[test]
    fn v3_keeps_blocks_encoded_and_returns_every_pair() {
        let pairs = pairs();
        let run = v3(&pairs);
        let blocks = run.blocks.as_ref().unwrap();
        assert_eq!(blocks.index.len(), pairs.len().div_ceil(BLOCK_PAIRS));
        assert!(run.pairs.is_empty());
        assert_eq!(run.all_pairs().unwrap(), pairs);
    }

    #[test]
    fn a_key_spanning_blocks_is_found_in_full() {
        let pairs = pairs();
        let run = v3(&pairs);
        let expect: Vec<u64> = brute(&pairs, b"module=hot", b"module=hot")
            .into_iter()
            .map(|(_, i)| i)
            .collect();
        assert_eq!(expect.len(), 300);
        assert_eq!(run.lookup(b"module=hot").unwrap(), expect);
        assert!(run.lookup(b"module=absent").unwrap().is_empty());
    }

    #[test]
    fn ranges_match_a_scan_of_the_pairs() {
        let pairs = pairs();
        let run = v3(&pairs);
        for (lo, hi) in [
            (&b"module=m0100"[..], &b"module=m0200"[..]),
            (b"module=", b"module=\xff"),
            (b"target=t1", b"target=t2"),
            (b"a", b"z"),
            (b"zz", b"zzz"),
        ] {
            assert_eq!(run.pairs_in(&(lo..=hi)).unwrap(), brute(&pairs, lo, hi));
        }
    }

    #[test]
    fn a_lookup_decodes_only_the_blocks_it_needs() {
        let pairs = pairs();
        let run = v3(&pairs);
        let blocks = run.blocks.as_ref().unwrap();
        let first = blocks.from(b"target=t3");
        let last = blocks
            .index
            .partition_point(|b| b.first.as_slice() <= &b"target=t3"[..]);
        assert!(last - first < blocks.index.len() / 4, "{first}..{last}");
    }

    #[test]
    fn v2_and_v3_answer_alike() {
        let pairs = pairs();
        let bytes = Run::encode(&(0..=999), &pairs).unwrap();
        let old = Run::decode(&bytes, &manifest(0..=999, &bytes)).unwrap();
        let new = v3(&pairs);
        for key in [&b"module=hot"[..], b"module=m0001", b"target=t0", b"nope"] {
            assert_eq!(old.lookup(key).unwrap(), new.lookup(key).unwrap());
        }
    }

    #[test]
    fn a_tampered_block_is_refused_when_read() {
        let pairs = pairs();
        let bytes = Run::encode_blocks(&(0..=999), &pairs).unwrap();
        let run = Run::decode(&bytes, &manifest(0..=999, &bytes)).unwrap();
        let mut blocks = run.blocks.unwrap();
        let b = blocks.index.len() / 2;
        let at = blocks.index[b].start;
        blocks.body[at] = 0x7f;
        assert!(matches!(blocks.decode(b), Err(Error::Tampered { .. })));
    }

    #[test]
    fn a_count_that_disagrees_with_the_blocks_is_refused() {
        let pairs = pairs();
        let mut bytes = Run::encode_blocks(&(0..=999), &pairs).unwrap();
        // n sits right after magic, lo and hi; 2000 pairs encode as 0xd0 0x0f.
        let at = RUN_MAGIC_V3.len() + 16;
        assert_eq!(&bytes[at..at + 2], &[0xd0, 0x0f]);
        bytes[at] = 0xcf;
        let expect = manifest(0..=999, &bytes);
        assert!(matches!(
            Run::decode(&bytes, &expect),
            Err(Error::Tampered { .. })
        ));
    }
}
