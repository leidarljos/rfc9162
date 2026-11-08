//! A sidecar log beside a project tree: every change to a path is an entry,
//! the bytes it wrote are a content-addressed blob in the same store, and a
//! snapshot is a signed checkpoint.
//!
//! The way `.git` sits beside a working tree and etckeeper keeps `/etc` in
//! one, a [`Sidecar`] keeps a project's history, but the history is an
//! authenticated log: a third party proves any change from the checkpoint
//! alone, nothing needs a working tree or a git binary, and retention is
//! compaction with proofs kept rather than a history rewrite. Over a
//! [`rfc9162_tiles::PackStore`] the whole sidecar is one file and its lock.
//!
//! An entry is one JSON line, [`Change`], and its lookup keys are
//! `path=<path>` and `blob=<hex>`, so a path's history is a point lookup
//! and everything under a directory is a range over the sorted runs. A blob
//! is stored once per content hash at `blob/<hex>` before the entry that
//! names it is appended, so an entry never names a blob the store lacks.
//! Entries are at most the tile log's entry length; file content goes in
//! the blob, not the entry.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use rfc9162_tiles::checkpoint::verify;
use rfc9162_tiles::{Checkpoint, Note, Signer, TileStore, Verifier};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::store::MANIFEST_MAGIC;
use crate::{AuthenticatedLog, Error, Keys, Located, Store};

/// What a change did to its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// The path now holds the blob's bytes.
    Put,
    /// The path is gone.
    Delete,
    /// The path is a child project, and this is its signed checkpoint,
    /// verified against a trusted key before it was logged.
    Checkpoint,
}

/// One change to one path, as the log records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// The format, 1.
    pub sidecar: u8,
    /// What happened.
    pub op: Op,
    /// The project-relative path, `/`-separated.
    pub path: String,
    /// SHA-256 of the bytes, lower-case hex, for a put.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    /// Length of the bytes, for a put.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// The child's checkpoint origin, for a checkpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// The child's signed checkpoint note, as text, for a checkpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Whatever the caller records with the change: an actor, a job, a
    /// reason. Keys and values are stored as given.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, String>,
}

/// A change with its log index and the proof the lookup returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The log index of the change.
    pub index: u64,
    /// The change.
    pub change: Change,
    /// The entry's and its run's inclusion paths.
    pub located: Located,
}

/// The lookup keys of a sidecar entry: `path=` and `blob=`. An entry that is
/// not a sidecar change has no keys.
#[must_use]
pub fn sidecar_keys() -> Keys {
    Arc::new(|entry: &[u8]| {
        let Ok(change) = serde_json::from_slice::<Change>(entry) else {
            return Vec::new();
        };
        let mut keys = vec![format!("path={}", change.path).into_bytes()];
        if let Some(blob) = &change.blob {
            keys.push(format!("blob={blob}").into_bytes());
        }
        if let Some(origin) = &change.origin {
            keys.push(format!("origin={origin}").into_bytes());
        }
        keys
    })
}

fn blob_path(hex: &str) -> String {
    format!("blob/{hex}")
}

fn check_path(path: &str) -> Result<(), Error> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err(Error::Tree(
            "a sidecar path is relative, with no empty, dot or dot-dot component",
        ));
    }
    Ok(())
}

/// The sidecar over a store.
pub struct Sidecar<S: TileStore> {
    log: Store<S>,
}

impl<S: TileStore> Sidecar<S> {
    /// Opens the sidecar on `store`, taking the writer lock.
    pub fn open(store: S) -> Result<Self, Error> {
        Ok(Self {
            log: Store::open(store, sidecar_keys())?,
        })
    }

    /// The log underneath.
    #[must_use]
    pub fn log(&self) -> &Store<S> {
        &self.log
    }

    /// Entries in the log, changes and run manifests together.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.log.size()
    }

    /// The root over the log.
    #[must_use]
    pub fn root(&self) -> [u8; 32] {
        self.log.root()
    }

    /// Records that `path` now holds `bytes`. The blob is written first,
    /// once per content hash; the entry follows.
    pub fn put(
        &mut self,
        path: &str,
        bytes: &[u8],
        meta: BTreeMap<String, String>,
    ) -> Result<u64, Error> {
        check_path(path)?;
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        let hex = rfc9162::hex(&digest);
        let store = self.log.writer().store();
        let at = blob_path(&hex);
        if !store.exists(&at)? {
            store.put(&at, bytes)?;
        }
        let change = Change {
            sidecar: 1,
            op: Op::Put,
            path: path.to_string(),
            blob: Some(hex),
            size: Some(bytes.len() as u64),
            origin: None,
            note: None,
            meta,
        };
        self.append_change(&change)
    }

    /// Records that `path` is gone.
    pub fn delete(&mut self, path: &str, meta: BTreeMap<String, String>) -> Result<u64, Error> {
        check_path(path)?;
        let change = Change {
            sidecar: 1,
            op: Op::Delete,
            path: path.to_string(),
            blob: None,
            size: None,
            origin: None,
            note: None,
            meta,
        };
        self.append_change(&change)
    }

    fn append_change(&mut self, change: &Change) -> Result<u64, Error> {
        let line = serde_json::to_vec(change).map_err(|_| Error::Tree("change does not encode"))?;
        self.log.append(&line)
    }

    /// The bytes of the blob `hex` names, checked against the hash, or
    /// nothing when the store lacks it.
    pub fn blob(&self, hex: &str) -> Result<Option<Vec<u8>>, Error> {
        let Some(bytes) = self.log.writer().store().get(&blob_path(hex))? else {
            return Ok(None);
        };
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if rfc9162::hex(&digest) != hex {
            return Err(Error::Tampered { what: "blob" });
        }
        Ok(Some(bytes))
    }

    /// The change at log index `index`, or nothing when the entry is a run
    /// manifest or out of range.
    pub fn change(&self, index: u64) -> Result<Option<Change>, Error> {
        let index_view = self.log.index()?;
        let Some(bytes) = index_view.reader().get(index)? else {
            return Ok(None);
        };
        Ok(decode(&bytes))
    }

    /// Every change to `path`, oldest first, each with its proof.
    pub fn history(&self, path: &str) -> Result<Vec<Found>, Error> {
        let key = format!("path={path}").into_bytes();
        let located = self.log.find(&key)?;
        self.found(located)
    }

    /// Every change to a path under `dir`, in key order, each with its
    /// proof. `dir` is a directory path without a trailing slash, or the
    /// empty string for every path.
    pub fn under(&self, dir: &str) -> Result<Vec<Found>, Error> {
        let lo = if dir.is_empty() {
            b"path=".to_vec()
        } else {
            format!("path={dir}/").into_bytes()
        };
        let mut hi = lo.clone();
        hi.push(0xff);
        let located = self.log.range(lo.as_slice()..=hi.as_slice())?;
        self.found(located)
    }

    fn found(&self, located: Vec<Located>) -> Result<Vec<Found>, Error> {
        let index_view = self.log.index()?;
        let mut out = Vec::with_capacity(located.len());
        for item in located {
            let index = item.entry.index;
            let bytes = index_view
                .reader()
                .get(index)?
                .ok_or(Error::Tree("a located entry is missing"))?;
            let change = decode(&bytes).ok_or(Error::Tree("a located entry is not a change"))?;
            out.push(Found {
                index,
                change,
                located: item,
            });
        }
        Ok(out)
    }

    /// The changes with log index in `from..to`, in log order: what a
    /// snapshot at `to` holds that one at `from` did not.
    pub fn diff(&self, from: u64, to: u64) -> Result<Vec<(u64, Change)>, Error> {
        let to = to.min(self.log.size());
        let index_view = self.log.index()?;
        let mut out = Vec::new();
        for index in from..to {
            if let Some(bytes) = index_view.reader().get(index)? {
                if let Some(change) = decode(&bytes) {
                    out.push((index, change));
                }
            }
        }
        Ok(out)
    }

    /// The tree as the first `size` entries leave it: each path's latest
    /// put, with the index that made it; a deleted path is absent.
    pub fn tree_at(&self, size: u64) -> Result<BTreeMap<String, (u64, Change)>, Error> {
        let mut tree = BTreeMap::new();
        for (index, change) in self.diff(0, size)? {
            match change.op {
                Op::Put => {
                    tree.insert(change.path.clone(), (index, change));
                }
                Op::Delete => {
                    tree.remove(&change.path);
                }
                Op::Checkpoint => {}
            }
        }
        Ok(tree)
    }

    /// Logs a child project's signed checkpoint under `path`, so this log's
    /// next checkpoint signs the child's. The note has to carry a signature
    /// from one of `trusted`, and it has to extend the child's last logged
    /// checkpoint: a smaller size, or the same size with another root, is
    /// refused as an equivocation. The note is kept whole, so a reader
    /// re-verifies it from the entry.
    pub fn include_checkpoint(
        &mut self,
        path: &str,
        note: &Note,
        trusted: &[Verifier],
        meta: BTreeMap<String, String>,
    ) -> Result<u64, Error> {
        check_path(path)?;
        verify(note, trusted)?;
        let checkpoint = Checkpoint::parse(&note.text)?;
        if let Some((_, last)) = self.checkpoints(path)?.last() {
            if last.origin != checkpoint.origin {
                return Err(Error::Tree("a child checkpoint names another origin"));
            }
            if checkpoint.size < last.size
                || (checkpoint.size == last.size && checkpoint.root != last.root)
            {
                return Err(Error::Tree(
                    "a child checkpoint does not extend the one logged",
                ));
            }
        }
        let change = Change {
            sidecar: 1,
            op: Op::Checkpoint,
            path: path.to_string(),
            blob: None,
            size: Some(checkpoint.size),
            origin: Some(checkpoint.origin.clone()),
            note: Some(note.to_text()),
            meta,
        };
        self.append_change(&change)
    }

    /// The child checkpoints logged under `path`, oldest first, each parsed
    /// from the note the entry keeps.
    pub fn checkpoints(&self, path: &str) -> Result<Vec<(u64, Checkpoint)>, Error> {
        let mut out = Vec::new();
        for found in self.history(path)? {
            if found.change.op != Op::Checkpoint {
                continue;
            }
            let text = found
                .change
                .note
                .as_deref()
                .ok_or(Error::Tree("a checkpoint entry has no note"))?;
            let checkpoint = Checkpoint::parse(&Note::parse(text)?.text)?;
            out.push((found.index, checkpoint));
        }
        Ok(out)
    }

    /// Seals the unsealed tail into a run, when there is one, and signs the
    /// log's size and root as a checkpoint under `origin`.
    pub fn snapshot(&mut self, signer: &Signer, origin: &str) -> Result<Note, Error> {
        match self.log.seal() {
            Ok(_) | Err(Error::Tree("nothing to seal")) => {}
            Err(e) => return Err(e),
        }
        Ok(self.log.writer().checkpoint(signer, origin)?)
    }

    /// Merges the live runs into one and logs it.
    pub fn compact(&mut self) -> Result<crate::Compaction, Error> {
        self.log.compact()
    }
}

fn decode(bytes: &[u8]) -> Option<Change> {
    if bytes.starts_with(MANIFEST_MAGIC) {
        return None;
    }
    serde_json::from_slice(bytes).ok()
}

/// What [`Sidecar::sync_dir`] recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Synced {
    /// Paths the log did not hold.
    pub added: Vec<String>,
    /// Paths whose bytes differ from the log's latest put.
    pub changed: Vec<String>,
    /// Paths the log held that the directory lacks.
    pub deleted: Vec<String>,
    /// Paths whose bytes the log already holds.
    pub unchanged: u64,
    /// The log range the changes went to, empty when nothing changed.
    pub entries: std::ops::Range<u64>,
}

impl<S: TileStore> Sidecar<S> {
    /// Records the regular files under `root` against the tree the log
    /// holds, the way etckeeper commits `/etc`: a file the log lacks or holds
    /// with other bytes is a put, a path the log holds that the directory
    /// lacks is a delete, and a file with the same bytes is left alone.
    /// Every change carries `meta`, and all of them go in one batch after
    /// their blobs. `skip` names entries at the top of `root` that are not
    /// the project's, the sidecar's own directory among them. Symbolic
    /// links are not followed and not recorded.
    pub fn sync_dir(
        &mut self,
        root: &Path,
        skip: &[&str],
        meta: &BTreeMap<String, String>,
    ) -> Result<Synced, Error> {
        let held = self.tree_at(self.log.size())?;
        let mut files = Vec::new();
        walk(root, root, skip, &mut files)?;
        let mut synced = Synced::default();
        let mut lines = Vec::new();
        let store_blobs = |this: &Self, hex: &str, bytes: &[u8]| -> Result<(), Error> {
            let store = this.log.writer().store();
            let at = blob_path(hex);
            if !store.exists(&at)? {
                store.put(&at, bytes)?;
            }
            Ok(())
        };
        let mut seen = std::collections::BTreeSet::new();
        for (path, file) in files {
            let bytes = std::fs::read(&file)?;
            let digest: [u8; 32] = Sha256::digest(&bytes).into();
            let hex = rfc9162::hex(&digest);
            seen.insert(path.clone());
            match held.get(&path) {
                Some((_, change)) if change.blob.as_deref() == Some(hex.as_str()) => {
                    synced.unchanged += 1;
                    continue;
                }
                Some(_) => synced.changed.push(path.clone()),
                None => synced.added.push(path.clone()),
            }
            store_blobs(self, &hex, &bytes)?;
            let change = Change {
                sidecar: 1,
                op: Op::Put,
                path,
                blob: Some(hex),
                size: Some(bytes.len() as u64),
                origin: None,
                note: None,
                meta: meta.clone(),
            };
            lines.push(
                serde_json::to_vec(&change).map_err(|_| Error::Tree("change does not encode"))?,
            );
        }
        for path in held.keys() {
            if !seen.contains(path) {
                synced.deleted.push(path.clone());
                let change = Change {
                    sidecar: 1,
                    op: Op::Delete,
                    path: path.clone(),
                    blob: None,
                    size: None,
                    origin: None,
                    note: None,
                    meta: meta.clone(),
                };
                lines.push(
                    serde_json::to_vec(&change)
                        .map_err(|_| Error::Tree("change does not encode"))?,
                );
            }
        }
        let size = self.log.size();
        synced.entries = if lines.is_empty() {
            size..size
        } else {
            self.log.append_batch(lines)?
        };
        Ok(synced)
    }
}

/// Every regular file under `dir`, as (project path, filesystem path), in
/// path order. Entries named in `skip` at the top level are left out.
fn walk(
    root: &Path,
    dir: &Path,
    skip: &[&str],
    out: &mut Vec<(String, std::path::PathBuf)>,
) -> Result<(), Error> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if dir == root && skip.contains(&name.as_ref()) {
            continue;
        }
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            walk(root, &path, skip, out)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Error::Tree("a file is not under the root"))?;
            let text = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((text, path));
        }
    }
    Ok(())
}
