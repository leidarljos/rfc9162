//! One file for every store path: a packed [`TileStore`], so a log costs a
//! constant number of inodes however long its history.
//!
//! The pack is an append-only file. Each record carries a store path and
//! either its bytes, a deletion, or a rename, and ends in a SHA-256 over the
//! record, so a torn tail is found and ignored. Bytes that shrink under
//! DEFLATE are stored compressed, record by record; hash tiles do not
//! shrink and are stored as they are. A path's current bytes are the last
//! record that names it, so [`PackStore::compact`] rewrites the pack with
//! one record per live path and renames it over the old one.
//!
//! A compaction also writes an index as the pack's first record: every live
//! path with where its body sits, and the length of the pack the index
//! covers. Opening such a pack reads the index and then only the records
//! appended after the region it covers, rather than every record; a record
//! the index named is checked against its digest when it is read. Packs
//! written by format v1 (no index) open by reading every record, and become
//! v2 at their next compaction.
//!
//! One writer at a time: the writer holds `<pack>.lock`, created
//! exclusively and removed when the store is dropped, the way the tile
//! log's own locks work; a crash leaves it, and [`PackStore::break_lock`]
//! removes it once the operator knows no writer is alive. Readers open the
//! pack without a lock and see every record whose digest holds; a reader
//! re-reads the tail before each operation, so it sees what a writer has
//! appended since, and reopens the pack after a compaction.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::store::TileStore;

/// The first bytes of a pack file as this version writes it (format v2,
/// whose first record may be an index).
pub const PACK_MAGIC: &[u8] = b"rfc9162-tiles/pack/v2\n";
/// The first bytes of a pack file in format v1, which still opens.
pub const PACK_MAGIC_V1: &[u8] = b"rfc9162-tiles/pack/v1\n";

const KIND_PUT: u8 = 1;
const KIND_DELETE: u8 = 2;
const KIND_RENAME: u8 = 3;
/// The live table a compaction writes as the first record of a v2 pack.
const KIND_INDEX: u8 = 4;

const CODEC_RAW: u8 = 0;
const CODEC_DEFLATE: u8 = 1;

/// Bodies shorter than this are stored raw: the DEFLATE framing costs more
/// than it saves.
const COMPRESS_FROM: usize = 128;
const DEFLATE_LEVEL: u8 = 6;

/// kind, codec, path length, stored length, raw length.
const HEAD_LEN: usize = 1 + 1 + 2 + 4 + 4;
const DIGEST_LEN: usize = 32;

/// An index record's body: the covered length and the path count.
const INDEX_FIXED: usize = 8 + 4;
/// Per path in the index: path length, record start, body offset, stored,
/// raw, codec.
const INDEX_PER_PATH: usize = 2 + 8 + 8 + 4 + 4 + 1;

/// Bytes the index record of a compacted pack takes for `paths` live paths
/// whose names total `names` bytes.
fn index_len(paths: usize, names: usize) -> u64 {
    (HEAD_LEN + INDEX_FIXED + DIGEST_LEN + paths * INDEX_PER_PATH + names) as u64
}

/// Where a path's bytes sit in the pack.
#[derive(Debug, Clone, Copy)]
struct Slot {
    /// Offset of the record that holds the body; a rename keeps the record,
    /// whose path is then the old one.
    start: u64,
    /// Offset of the stored body.
    offset: u64,
    stored: u32,
    raw: u32,
    codec: u8,
}

struct Inner {
    file: File,
    /// Length of the pack up to and including the last record whose digest
    /// held. Records past it are a torn tail.
    valid_len: u64,
    live: BTreeMap<String, Slot>,
    /// Bytes the live records occupy, body and framing, plus the magic and
    /// the index a compaction writes: the pack's length after a compaction.
    live_bytes: u64,
    #[cfg(unix)]
    inode: u64,
}

/// A [`TileStore`] over one append-only file.
pub struct PackStore {
    path: PathBuf,
    writer: bool,
    inner: Mutex<Inner>,
}

/// What a compaction did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compacted {
    /// Pack length before.
    pub before: u64,
    /// Pack length after.
    pub after: u64,
    /// Live paths carried over.
    pub paths: usize,
}

impl PackStore {
    /// Opens the pack at `path` for writing, creating it when absent, and
    /// takes `<path>.lock`. A pack another writer holds is refused with
    /// [`io::ErrorKind::WouldBlock`].
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        take_lock(&lock_path(&path))?;
        let store = match Self::open_with(path.clone(), true) {
            Ok(store) => store,
            Err(e) => {
                let _ = fs::remove_file(lock_path(&path));
                return Err(e);
            }
        };
        // A writer owns the tail: whatever a crash left past the last good
        // record is dropped before anything is appended after it.
        {
            let inner = store.inner.lock().unwrap();
            let valid = inner.valid_len;
            if inner.file.metadata()?.len() != valid {
                inner.file.set_len(valid)?;
                inner.file.sync_all()?;
            }
        }
        Ok(store)
    }

    /// Opens the pack at `path` for reading, without a lock. The pack must
    /// exist.
    pub fn open_read(path: impl Into<PathBuf>) -> io::Result<Self> {
        Self::open_with(path.into(), false)
    }

    /// Removes a lock a crashed writer left. The caller knows no writer is
    /// alive; nothing here checks.
    pub fn break_lock(path: impl AsRef<Path>) -> io::Result<bool> {
        match fs::remove_file(lock_path(path.as_ref())) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// The pack file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Length of the pack file, including any torn tail.
    pub fn len(&self) -> io::Result<u64> {
        Ok(fs::metadata(&self.path)?.len())
    }

    /// Whether the pack holds no path.
    pub fn is_empty(&self) -> io::Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner)?;
        Ok(inner.live.is_empty())
    }

    /// Bytes the live records occupy, framing, magic and index included: the
    /// pack after a compaction is exactly this long.
    pub fn live_bytes(&self) -> io::Result<u64> {
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner)?;
        Ok(inner.live_bytes)
    }

    /// Rewrites the pack with one record per live path, in path order, and
    /// renames it over the old one. The tree and every proof are untouched:
    /// they are over entry bytes, and every live path keeps its bytes.
    pub fn compact(&self) -> io::Result<Compacted> {
        self.need_writer()?;
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner)?;
        let before = inner.valid_len;
        let temp = self.path.with_extension("compacting");
        let mut out = File::create(&temp)?;
        out.write_all(PACK_MAGIC)?;
        let current: Vec<(String, Slot)> = inner
            .live
            .iter()
            .map(|(path, slot)| (path.clone(), *slot))
            .collect();
        let names: usize = current.iter().map(|(p, _)| p.len()).sum();
        let first = PACK_MAGIC.len() as u64 + index_len(current.len(), names);
        let mut offset = first;
        let mut live = BTreeMap::new();
        let mut records = Vec::with_capacity(current.len());
        for (path, slot) in &current {
            let stored = read_record(&mut inner.file, slot)?;
            let record = encode_stored(KIND_PUT, slot.codec, path, &stored, slot.raw)?;
            let body_at = offset + (HEAD_LEN + path.len()) as u64;
            live.insert(
                path.clone(),
                Slot {
                    start: offset,
                    offset: body_at,
                    stored: slot.stored,
                    raw: slot.raw,
                    codec: slot.codec,
                },
            );
            offset += record.len() as u64;
            records.push(record);
        }
        let index = encode_index(offset, &live)?;
        debug_assert_eq!(PACK_MAGIC.len() as u64 + index.len() as u64, first);
        out.write_all(&index)?;
        for record in &records {
            out.write_all(record)?;
        }
        let live_bytes = offset;
        out.sync_all()?;
        drop(out);
        fs::rename(&temp, &self.path)?;
        sync_parent(&self.path)?;
        let file = OpenOptions::new().read(true).write(true).open(&self.path)?;
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            file.metadata()?.ino()
        };
        *inner = Inner {
            file,
            valid_len: offset,
            live,
            live_bytes,
            #[cfg(unix)]
            inode,
        };
        Ok(Compacted {
            before,
            after: offset,
            paths: inner.live.len(),
        })
    }

    fn open_with(path: PathBuf, writer: bool) -> io::Result<Self> {
        let inner = load(&path, writer)?;
        Ok(Self {
            path,
            writer,
            inner: Mutex::new(inner),
        })
    }

    fn need_writer(&self) -> io::Result<()> {
        if self.writer {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} is open for reading", self.path.display()),
            ))
        }
    }

    /// Reads records a writer appended since, or reopens the pack when a
    /// compaction replaced the file. A writer's view is its own.
    fn refresh(&self, inner: &mut Inner) -> io::Result<()> {
        if self.writer {
            return Ok(());
        }
        let meta = fs::metadata(&self.path)?;
        #[cfg(unix)]
        let replaced = {
            use std::os::unix::fs::MetadataExt;
            meta.ino() != inner.inode
        };
        #[cfg(not(unix))]
        let replaced = meta.len() < inner.valid_len;
        if replaced {
            *inner = load(&self.path, false)?;
            return Ok(());
        }
        if meta.len() > inner.valid_len {
            scan(inner)?;
        }
        Ok(())
    }

    fn append(&self, inner: &mut Inner, kind: u8, path: &str, body: &[u8]) -> io::Result<Slot> {
        self.need_writer()?;
        let (codec, stored) = if kind == KIND_PUT && body.len() >= COMPRESS_FROM {
            let packed = miniz_oxide::deflate::compress_to_vec(body, DEFLATE_LEVEL);
            if packed.len() * 10 < body.len() * 9 {
                (CODEC_DEFLATE, packed)
            } else {
                (CODEC_RAW, body.to_vec())
            }
        } else {
            (CODEC_RAW, body.to_vec())
        };
        let raw = u32::try_from(body.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "body over 4 GiB"))?;
        let record = encode_stored(kind, codec, path, &stored, raw)?;
        let at = inner.valid_len;
        inner.file.seek(SeekFrom::Start(at))?;
        inner.file.write_all(&record)?;
        inner.file.sync_data()?;
        inner.valid_len = at + record.len() as u64;
        Ok(Slot {
            start: at,
            offset: at + (HEAD_LEN + path.len()) as u64,
            stored: stored.len() as u32,
            raw,
            codec,
        })
    }
}

/// Opens the pack file and reads its records.
fn load(path: &Path, writer: bool) -> io::Result<Inner> {
    let file = OpenOptions::new()
        .read(true)
        .write(writer)
        .create(writer)
        .open(path)?;
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        file.metadata()?.ino()
    };
    let mut inner = Inner {
        file,
        valid_len: 0,
        live: BTreeMap::new(),
        live_bytes: PACK_MAGIC.len() as u64 + index_len(0, 0),
        #[cfg(unix)]
        inode,
    };
    let len = inner.file.metadata()?.len();
    if len == 0 {
        if !writer {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is empty, not a pack", path.display()),
            ));
        }
        inner.file.write_all(PACK_MAGIC)?;
        inner.file.sync_all()?;
        inner.valid_len = PACK_MAGIC.len() as u64;
    } else {
        let mut magic = vec![0u8; PACK_MAGIC.len()];
        inner.file.seek(SeekFrom::Start(0))?;
        let v2 = match inner.file.read_exact(&mut magic) {
            Ok(()) if magic == PACK_MAGIC => true,
            Ok(()) if magic == PACK_MAGIC_V1 => false,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} is not a pack", path.display()),
                ))
            }
        };
        inner.valid_len = PACK_MAGIC.len() as u64;
        if v2 {
            read_index(&mut inner, len)?;
        }
        if v2 && inner.valid_len == PACK_MAGIC.len() as u64 {
            skip_unusable_index(&mut inner, len)?;
        }
        scan(&mut inner)?;
    }
    Ok(inner)
}

impl Drop for PackStore {
    fn drop(&mut self) {
        if self.writer {
            let _ = fs::remove_file(lock_path(&self.path));
        }
    }
}

impl TileStore for PackStore {
    fn get(&self, path: &str) -> io::Result<Option<Vec<u8>>> {
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner)?;
        let Some(slot) = inner.live.get(path).copied() else {
            return Ok(None);
        };
        let stored = read_record(&mut inner.file, &slot)?;
        Ok(Some(unpack(&slot, stored)?))
    }

    fn put(&self, path: &str, bytes: &[u8]) -> io::Result<()> {
        check_path(path)?;
        let mut inner = self.inner.lock().unwrap();
        let slot = self.append(&mut inner, KIND_PUT, path, bytes)?;
        record_live(&mut inner, path, slot);
        Ok(())
    }

    fn create_new(&self, path: &str, bytes: &[u8]) -> io::Result<bool> {
        check_path(path)?;
        let mut inner = self.inner.lock().unwrap();
        if inner.live.contains_key(path) {
            return Ok(false);
        }
        let slot = self.append(&mut inner, KIND_PUT, path, bytes)?;
        record_live(&mut inner, path, slot);
        Ok(true)
    }

    fn delete(&self, path: &str) -> io::Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.live.contains_key(path) {
            return Ok(false);
        }
        self.append(&mut inner, KIND_DELETE, path, &[])?;
        drop_live(&mut inner, path);
        Ok(true)
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        check_path(to)?;
        let mut inner = self.inner.lock().unwrap();
        let Some(slot) = inner.live.get(from).copied() else {
            return Err(io::Error::new(io::ErrorKind::NotFound, from.to_string()));
        };
        self.append(&mut inner, KIND_RENAME, from, to.as_bytes())?;
        drop_live(&mut inner, from);
        record_live(&mut inner, to, slot);
        Ok(())
    }

    fn list(&self, prefix: &str) -> io::Result<Vec<String>> {
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner)?;
        Ok(inner
            .live
            .keys()
            .filter(|k| under(k, prefix))
            .cloned()
            .collect())
    }

    fn exists(&self, path: &str) -> io::Result<bool> {
        let mut inner = self.inner.lock().unwrap();
        self.refresh(&mut inner)?;
        Ok(inner.live.contains_key(path))
    }
}

fn under(path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || path
            .strip_prefix(prefix)
            .is_some_and(|r| r.starts_with('/'))
}

fn check_path(path: &str) -> io::Result<()> {
    if path.is_empty()
        || path.len() > u16::MAX as usize
        || path.starts_with('/')
        || path
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("store path {path:?} is not a relative path of plain components"),
        ));
    }
    Ok(())
}

fn lock_path(pack: &Path) -> PathBuf {
    let mut name = pack.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

fn take_lock(lock: &Path) -> io::Result<()> {
    match OpenOptions::new().write(true).create_new(true).open(lock) {
        Ok(mut file) => {
            let _ = writeln!(file, "{}", std::process::id());
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("{} is held by another writer", lock.display()),
        )),
        Err(e) => Err(e),
    }
}

fn sync_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}

/// The framing of a record whose stored body is already encoded.
fn encode_stored(kind: u8, codec: u8, path: &str, stored: &[u8], raw: u32) -> io::Result<Vec<u8>> {
    let path_len = u16::try_from(path.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path over 64 KiB"))?;
    let stored_len = u32::try_from(stored.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "body over 4 GiB"))?;
    let mut out = Vec::with_capacity(HEAD_LEN + path.len() + stored.len() + DIGEST_LEN);
    out.push(kind);
    out.push(codec);
    out.extend_from_slice(&path_len.to_be_bytes());
    out.extend_from_slice(&stored_len.to_be_bytes());
    out.extend_from_slice(&raw.to_be_bytes());
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(stored);
    let digest = Sha256::digest(&out);
    out.extend_from_slice(&digest);
    Ok(out)
}

/// The stored body of the `PUT` record at `slot`, checked against the
/// record's digest and head: a record an index named was not read when the
/// pack opened, so it is checked here.
fn read_record(file: &mut File, slot: &Slot) -> io::Result<Vec<u8>> {
    let corrupt = || io::Error::new(io::ErrorKind::InvalidData, "pack record fails its digest");
    let body_at = usize::try_from(slot.offset - slot.start).map_err(|_| corrupt())?;
    let mut record = vec![0u8; body_at + slot.stored as usize + DIGEST_LEN];
    file.seek(SeekFrom::Start(slot.start))?;
    file.read_exact(&mut record)?;
    let (framed, digest) = record.split_at(record.len() - DIGEST_LEN);
    let path_len = u16::from_be_bytes([framed[2], framed[3]]) as usize;
    let stored_len = u32::from_be_bytes([framed[4], framed[5], framed[6], framed[7]]);
    if Sha256::digest(framed).as_slice() != digest
        || framed[0] != KIND_PUT
        || framed[1] != slot.codec
        || HEAD_LEN + path_len != body_at
        || stored_len != slot.stored
    {
        return Err(corrupt());
    }
    Ok(framed[body_at..].to_vec())
}

fn unpack(slot: &Slot, stored: Vec<u8>) -> io::Result<Vec<u8>> {
    match slot.codec {
        CODEC_RAW => Ok(stored),
        CODEC_DEFLATE => {
            let raw = miniz_oxide::inflate::decompress_to_vec(&stored).map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("pack record: {e:?}"))
            })?;
            if raw.len() != slot.raw as usize {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "pack record inflates to the wrong length",
                ));
            }
            Ok(raw)
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("pack record codec {other}"),
        )),
    }
}

/// Bytes one live path costs a compacted pack: its record and its index row.
fn live_cost(path: &str, slot: &Slot) -> u64 {
    (HEAD_LEN + path.len() + DIGEST_LEN + INDEX_PER_PATH + path.len()) as u64
        + u64::from(slot.stored)
}

fn record_live(inner: &mut Inner, path: &str, slot: Slot) {
    if let Some(old) = inner.live.insert(path.to_string(), slot) {
        inner.live_bytes -= live_cost(path, &old);
    }
    inner.live_bytes += live_cost(path, &slot);
}

fn drop_live(inner: &mut Inner, path: &str) {
    if let Some(old) = inner.live.remove(path) {
        inner.live_bytes -= live_cost(path, &old);
    }
}

/// The index record of a compacted pack: the length it covers and, per live
/// path in path order, where the body sits.
fn encode_index(covered: u64, live: &BTreeMap<String, Slot>) -> io::Result<Vec<u8>> {
    let mut body = Vec::with_capacity(INDEX_FIXED + live.len() * INDEX_PER_PATH);
    body.extend_from_slice(&covered.to_be_bytes());
    let n = u32::try_from(live.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many paths"))?;
    body.extend_from_slice(&n.to_be_bytes());
    for (path, slot) in live {
        let len = u16::try_from(path.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path over 64 KiB"))?;
        body.extend_from_slice(&len.to_be_bytes());
        body.extend_from_slice(path.as_bytes());
        body.extend_from_slice(&slot.start.to_be_bytes());
        body.extend_from_slice(&slot.offset.to_be_bytes());
        body.extend_from_slice(&slot.stored.to_be_bytes());
        body.extend_from_slice(&slot.raw.to_be_bytes());
        body.push(slot.codec);
    }
    let raw = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "index over 4 GiB"))?;
    encode_stored(KIND_INDEX, CODEC_RAW, "", &body, raw)
}

/// Takes the live table from a v2 pack's index record when its digest
/// holds, every slot lies inside the covered region and the covered region
/// lies inside the file; `valid_len` then moves to the end of that region.
/// A missing or unusable index leaves `inner` as it was, and the scan reads
/// every record.
fn read_index(inner: &mut Inner, file_len: u64) -> io::Result<()> {
    let at = inner.valid_len;
    let mut head = [0u8; HEAD_LEN];
    inner.file.seek(SeekFrom::Start(at))?;
    if inner.file.read_exact(&mut head).is_err() || head[0] != KIND_INDEX || head[1] != CODEC_RAW {
        return Ok(());
    }
    let path_len = u16::from_be_bytes([head[2], head[3]]) as usize;
    let stored_len = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize;
    let end = at + (HEAD_LEN + path_len + stored_len + DIGEST_LEN) as u64;
    if path_len != 0 || end > file_len {
        return Ok(());
    }
    let mut record = head.to_vec();
    record.resize(HEAD_LEN + stored_len + DIGEST_LEN, 0);
    inner.file.read_exact(&mut record[HEAD_LEN..])?;
    let (framed, digest) = record.split_at(HEAD_LEN + stored_len);
    if Sha256::digest(framed).as_slice() != digest {
        return Ok(());
    }
    let body = &framed[HEAD_LEN..];
    let Some(table) = parse_index(body, end) else {
        return Ok(());
    };
    let (covered, slots) = table;
    if covered > file_len {
        return Ok(());
    }
    let mut live = BTreeMap::new();
    let mut live_bytes = PACK_MAGIC.len() as u64 + index_len(0, 0);
    for (path, slot) in slots {
        live_bytes += live_cost(&path, &slot);
        live.insert(path, slot);
    }
    inner.live = live;
    inner.live_bytes = live_bytes;
    inner.valid_len = covered;
    Ok(())
}

/// Steps over a first record that says it is an index but could not be
/// used (a digest or a bound that did not hold), by the lengths its head
/// declares, so the scan applies the records after it rather than stopping
/// at it.
fn skip_unusable_index(inner: &mut Inner, file_len: u64) -> io::Result<()> {
    let at = inner.valid_len;
    let mut head = [0u8; HEAD_LEN];
    inner.file.seek(SeekFrom::Start(at))?;
    if inner.file.read_exact(&mut head).is_err() || head[0] != KIND_INDEX {
        return Ok(());
    }
    let path_len = u16::from_be_bytes([head[2], head[3]]) as u64;
    let stored_len = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as u64;
    let end = at + HEAD_LEN as u64 + path_len + stored_len + DIGEST_LEN as u64;
    if end <= file_len {
        inner.valid_len = end;
    }
    Ok(())
}

/// The covered length and the slots of an index body, or `None` when the
/// body is malformed or a slot does not lie between `first` and the covered
/// length.
fn parse_index(body: &[u8], first: u64) -> Option<(u64, Vec<(String, Slot)>)> {
    let covered = u64::from_be_bytes(body.get(..8)?.try_into().ok()?);
    let n = u32::from_be_bytes(body.get(8..12)?.try_into().ok()?) as usize;
    if covered < first || n > body.len() / INDEX_PER_PATH {
        return None;
    }
    let mut at = INDEX_FIXED;
    let mut slots = Vec::with_capacity(n);
    for _ in 0..n {
        let len = u16::from_be_bytes(body.get(at..at + 2)?.try_into().ok()?) as usize;
        at += 2;
        let path = std::str::from_utf8(body.get(at..at + len)?)
            .ok()?
            .to_string();
        at += len;
        let row = body.get(at..at + INDEX_PER_PATH - 2)?;
        at += INDEX_PER_PATH - 2;
        let slot = Slot {
            start: u64::from_be_bytes(row[..8].try_into().ok()?),
            offset: u64::from_be_bytes(row[8..16].try_into().ok()?),
            stored: u32::from_be_bytes(row[16..20].try_into().ok()?),
            raw: u32::from_be_bytes(row[20..24].try_into().ok()?),
            codec: row[24],
        };
        let stop = slot
            .offset
            .checked_add(u64::from(slot.stored) + DIGEST_LEN as u64)?;
        if slot.start < first || slot.offset < slot.start + HEAD_LEN as u64 || stop > covered {
            return None;
        }
        slots.push((path, slot));
    }
    (at == body.len()).then_some((covered, slots))
}

/// Reads records from `valid_len` to the end of the file one at a time,
/// applying each whose digest holds and stopping at the first that does
/// not: a short read, a bad digest, a path that is not UTF-8, or a kind
/// this version does not know.
fn scan(inner: &mut Inner) -> io::Result<()> {
    let mut at = inner.valid_len;
    inner.file.seek(SeekFrom::Start(at))?;
    let mut reader = io::BufReader::new(inner.file.try_clone()?);
    let mut head = [0u8; HEAD_LEN];
    let mut record = Vec::new();
    let mut digest = [0u8; DIGEST_LEN];
    loop {
        if reader.read_exact(&mut head).is_err() {
            break;
        }
        let kind = head[0];
        let codec = head[1];
        let path_len = u16::from_be_bytes([head[2], head[3]]) as usize;
        let stored_len = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let raw = u32::from_be_bytes([head[8], head[9], head[10], head[11]]);
        record.clear();
        record.extend_from_slice(&head);
        record.resize(HEAD_LEN + path_len + stored_len, 0);
        if reader.read_exact(&mut record[HEAD_LEN..]).is_err()
            || reader.read_exact(&mut digest).is_err()
        {
            break;
        }
        if Sha256::digest(&record).as_slice() != digest {
            break;
        }
        let Ok(path) = std::str::from_utf8(&record[HEAD_LEN..HEAD_LEN + path_len]) else {
            break;
        };
        let slot = Slot {
            start: at,
            offset: at + (HEAD_LEN + path_len) as u64,
            stored: stored_len as u32,
            raw,
            codec,
        };
        match kind {
            KIND_PUT => record_live(inner, path, slot),
            KIND_DELETE => drop_live(inner, path),
            KIND_RENAME => {
                let Ok(to) = std::str::from_utf8(&record[HEAD_LEN + path_len..]) else {
                    break;
                };
                let to = to.to_string();
                if let Some(moved) = inner.live.remove(path) {
                    inner.live_bytes -= live_cost(path, &moved);
                    record_live(inner, &to, moved);
                }
            }
            // An index carries no change of its own; when it was not usable
            // the records it describes follow it and are applied here.
            KIND_INDEX => {}
            _ => break,
        }
        at += (HEAD_LEN + path_len + stored_len + DIGEST_LEN) as u64;
    }
    inner.valid_len = at;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(i: usize) -> Vec<u8> {
        format!(
            "{{\"module\":\"PETSc/3.24.{i}-foss-2026.1\",\"pad\":\"{}\"}}",
            "x".repeat(200)
        )
        .into_bytes()
    }

    fn fill(store: &PackStore, n: usize) {
        for i in 0..n {
            store
                .put(&format!("tile/entries/{i:03}"), &body(i))
                .unwrap();
        }
        store.put("tile/entries/000", b"rewritten").unwrap();
        store.delete("tile/entries/001").unwrap();
        store.rename("tile/entries/002", "moved/two").unwrap();
    }

    fn check(store: &PackStore, n: usize) {
        assert_eq!(
            store.get("tile/entries/000").unwrap().as_deref(),
            Some(&b"rewritten"[..])
        );
        assert_eq!(store.get("tile/entries/001").unwrap(), None);
        assert_eq!(store.get("tile/entries/002").unwrap(), None);
        assert_eq!(store.get("moved/two").unwrap(), Some(body(2)));
        for i in 3..n {
            assert_eq!(
                store.get(&format!("tile/entries/{i:03}")).unwrap(),
                Some(body(i))
            );
        }
    }

    #[test]
    fn a_compacted_pack_opens_through_its_index_and_takes_later_records() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("pack");
        let store = PackStore::open(&pack).unwrap();
        fill(&store, 50);
        let live = store.live_bytes().unwrap();
        let done = store.compact().unwrap();
        assert_eq!(done.after, live);
        store.put("after/compaction", b"tail").unwrap();
        store.rename("tile/entries/003", "moved/three").unwrap();
        drop(store);
        let reader = PackStore::open_read(&pack).unwrap();
        {
            let inner = reader.inner.lock().unwrap();
            assert_eq!(inner.valid_len, std::fs::metadata(&pack).unwrap().len());
        }
        assert_eq!(
            reader.get("after/compaction").unwrap().as_deref(),
            Some(&b"tail"[..])
        );
        assert_eq!(reader.get("moved/three").unwrap(), Some(body(3)));
        assert_eq!(reader.get("tile/entries/003").unwrap(), None);
        assert_eq!(reader.get("moved/two").unwrap(), Some(body(2)));
        assert_eq!(reader.get("tile/entries/049").unwrap(), Some(body(49)));
        let writer = PackStore::open(&pack).unwrap();
        let live = writer.live_bytes().unwrap();
        assert_eq!(writer.compact().unwrap().after, live);
        assert_eq!(writer.get("moved/three").unwrap(), Some(body(3)));
    }

    #[test]
    fn the_index_is_what_open_reads() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("pack");
        let store = PackStore::open(&pack).unwrap();
        fill(&store, 20);
        store.compact().unwrap();
        drop(store);
        let mut file = OpenOptions::new().read(true).open(&pack).unwrap();
        let mut inner = Inner {
            file: file.try_clone().unwrap(),
            valid_len: PACK_MAGIC.len() as u64,
            live: BTreeMap::new(),
            live_bytes: 0,
            #[cfg(unix)]
            inode: 0,
        };
        let len = file.seek(SeekFrom::End(0)).unwrap();
        read_index(&mut inner, len).unwrap();
        assert_eq!(inner.valid_len, len);
        assert_eq!(inner.live.len(), 19);
    }

    #[test]
    fn a_damaged_index_falls_back_to_reading_every_record() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("pack");
        let store = PackStore::open(&pack).unwrap();
        fill(&store, 30);
        store.compact().unwrap();
        drop(store);
        let mut bytes = std::fs::read(&pack).unwrap();
        // A byte inside the index body; its digest no longer holds.
        bytes[PACK_MAGIC.len() + HEAD_LEN + 3] ^= 0xff;
        std::fs::write(&pack, &bytes).unwrap();
        let reader = PackStore::open_read(&pack).unwrap();
        check(&reader, 30);
        let writer = PackStore::open(&pack).unwrap();
        assert_eq!(std::fs::metadata(&pack).unwrap().len(), bytes.len() as u64);
        check(&writer, 30);
    }

    #[test]
    fn a_damaged_record_behind_the_index_is_refused_when_read() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("pack");
        let store = PackStore::open(&pack).unwrap();
        fill(&store, 10);
        store.compact().unwrap();
        let slot = store.inner.lock().unwrap().live["tile/entries/005"];
        drop(store);
        let mut bytes = std::fs::read(&pack).unwrap();
        bytes[slot.offset as usize] ^= 0xff;
        std::fs::write(&pack, &bytes).unwrap();
        let reader = PackStore::open_read(&pack).unwrap();
        let err = reader.get("tile/entries/005").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(reader.get("tile/entries/006").unwrap(), Some(body(6)));
    }

    #[test]
    fn a_v1_pack_opens_and_becomes_v2_at_compaction() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("pack");
        let mut bytes = PACK_MAGIC_V1.to_vec();
        for i in 0..5 {
            let raw = body(i);
            bytes.extend(
                encode_stored(
                    KIND_PUT,
                    CODEC_RAW,
                    &format!("p/{i}"),
                    &raw,
                    raw.len() as u32,
                )
                .unwrap(),
            );
        }
        std::fs::write(&pack, &bytes).unwrap();
        let store = PackStore::open(&pack).unwrap();
        for i in 0..5 {
            assert_eq!(store.get(&format!("p/{i}")).unwrap(), Some(body(i)));
        }
        store.put("p/5", &body(5)).unwrap();
        assert!(std::fs::read(&pack).unwrap().starts_with(PACK_MAGIC_V1));
        store.compact().unwrap();
        assert!(std::fs::read(&pack).unwrap().starts_with(PACK_MAGIC));
        drop(store);
        let reader = PackStore::open_read(&pack).unwrap();
        for i in 0..6 {
            assert_eq!(reader.get(&format!("p/{i}")).unwrap(), Some(body(i)));
        }
    }
}
