//! An authenticated, indexed, append-only log over [`rfc9162`] and
//! [`rfc9162_tiles`]: the contract, the store that keeps it, and a sidecar
//! over a project tree.
//!
//! `rfc9162` is the tree; `rfc9162-tiles` stores it with key sidecars that
//! the checkpoint does not sign, finds a key by filtered scan, and detects a
//! torn tail without proving the recovery. This crate adds, as a contract
//! ([`AuthenticatedLog`]) and a store ([`Store`]) behind it:
//!
//! - every index structure is itself logged, so the checkpoint that signs
//!   the entries signs the index ([`RunManifest::manifest_index`]);
//! - a lookup answers with the entry and with the run that located it, each
//!   carrying an inclusion path ([`Located`]);
//! - keys are sorted into runs with a range, so a prefix or range query is
//!   a seek through [`RunManifest::key_lo`] and [`RunManifest::key_hi`];
//! - compaction writes a new run and logs it; the runs it replaces stay in
//!   the log and stay provable ([`Compaction`]);
//! - recovery replays the tail past the last signed checkpoint and proves the
//!   result consistent with it ([`Recovery`]).
//!
//! [`Sidecar`] sits beside a project tree the way `.git` does: every change
//! to a path is an entry whose bytes are a content-addressed blob in the
//! same store, a snapshot is a signed checkpoint, and over a
//! [`rfc9162_tiles::PackStore`] the whole history is one file.

#![forbid(unsafe_code)]

pub mod sidecar;
pub mod store;

use std::ops::RangeInclusive;

pub use sidecar::{Change, Found, Op, Sidecar, Synced};
pub use store::{Keys, Store};

/// A sealed, sorted run of lookup keys over a contiguous range of entries,
/// recorded in the log as an entry of its own so the checkpoint signs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunManifest {
    /// The entries the run indexes, by log index.
    pub entries: RangeInclusive<u64>,
    /// The smallest key in the run.
    pub key_lo: Vec<u8>,
    /// The largest key in the run.
    pub key_hi: Vec<u8>,
    /// `SHA-256` of the run's key file (sorted `(key, index)` pairs and the
    /// filter), the bytes a reader checks before trusting a seek.
    pub run_hash: [u8; 32],
    /// The log index of the entry that records this manifest, once logged.
    /// A run with no manifest index is sealed on disk and not yet in the
    /// tree; a checkpoint does not cover it.
    pub manifest_index: Option<u64>,
}

/// An inclusion path for one entry at one tree size, as [`rfc9162`] checks
/// it: the sibling hashes from the leaf to the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inclusion {
    /// The entry's log index.
    pub index: u64,
    /// The tree size the path proves against.
    pub tree_size: u64,
    /// Sibling hashes, leaf to root.
    pub path: Vec<[u8; 32]>,
}

/// What a lookup returns: the entry the key names and the run that located
/// it, both with inclusion paths against the same tree size, so the reader
/// proves the answer and the index that gave it with one root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// The entry's inclusion path.
    pub entry: Inclusion,
    /// The inclusion path of the run manifest the seek went through. `None`
    /// when the entry sits in the unsealed tail, found by scan.
    pub run: Option<Inclusion>,
}

/// One compaction: the runs it replaced and the run it wrote, each a logged
/// manifest. The replaced runs remain in the log; a reader at an older
/// checkpoint still proves through them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compaction {
    /// Manifest indices of the runs merged.
    pub replaced: Vec<u64>,
    /// Manifest index of the run written.
    pub written: u64,
}

/// What recovery proved after a crash: the checkpoint it started from, the
/// entries replayed beyond it, and that the recovered tree is consistent
/// with the checkpoint by [`rfc9162::verify_consistency`].
///
/// The checkpoint recovery starts from is the largest size a trusted key
/// signed, read from a record the store's writer cannot reach: a store that
/// can rewrite its own tree state or hand back an older signed checkpoint
/// would otherwise shorten the log and keep every proof valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// Tree size the last signed checkpoint covered.
    pub checkpoint_size: u64,
    /// Entries found beyond it and replayed into the tree.
    pub replayed: u64,
    /// Entries found beyond it that could not be replayed (torn, or past a
    /// gap) and were quarantined.
    pub quarantined: u64,
    /// The consistency path from the checkpoint's tree to the recovered one.
    pub consistency: Vec<[u8; 32]>,
}

/// The operations the log offers. Each returns the crate's [`Error`] on a
/// store that cannot answer; a reader that distrusts the store checks the
/// paths these types carry against a checkpoint it trusts.
pub trait AuthenticatedLog {
    /// Append one entry; returns its log index.
    fn append(&mut self, entry: &[u8]) -> Result<u64, Error>;

    /// Every entry carrying `key`, each with the run that located it. An
    /// empty answer is proven, not assumed: the logged runs' entry ranges
    /// cover every index below the checkpoint size except the scanned
    /// tail, or the answer is [`Error::Tree`], since a withheld run file
    /// would otherwise hide an entry without doctoring any bytes.
    fn find(&self, key: &[u8]) -> Result<Vec<Located>, Error>;

    /// Every entry whose keys fall in `range`, in key order, with their runs.
    fn range(&self, range: RangeInclusive<&[u8]>) -> Result<Vec<Located>, Error>;

    /// Seal the unsealed tail into a run and log its manifest.
    fn seal(&mut self) -> Result<RunManifest, Error>;

    /// Merge runs into one and log the result.
    fn compact(&mut self) -> Result<Compaction, Error>;

    /// Replay past the last checkpoint and prove the result consistent.
    fn recover(&mut self) -> Result<Recovery, Error>;
}

/// What can go wrong between a caller and the store.
#[derive(Debug)]
pub enum Error {
    /// The store answered with bytes that do not hash to what the log
    /// committed to.
    Tampered {
        /// What was being checked.
        what: &'static str,
    },
    /// The store could not be read or written.
    Store(std::io::Error),
    /// The tree or a proof is malformed.
    Tree(&'static str),
    /// The tile store refused in its own terms (a lock held, an entry too
    /// long, a signature), with its message.
    Tiles(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tampered { what } => write!(f, "{what} does not match what the log committed to"),
            Self::Store(e) => write!(f, "store: {e}"),
            Self::Tree(what) => write!(f, "tree: {what}"),
            Self::Tiles(what) => write!(f, "tiles: {what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Store(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_outside_the_tree_has_no_manifest_index() {
        let run = RunManifest {
            entries: 0..=255,
            key_lo: b"module=A".to_vec(),
            key_hi: b"pkg=Z".to_vec(),
            run_hash: rfc9162::leaf_hash(b"run bytes"),
            manifest_index: None,
        };
        assert!(run.manifest_index.is_none());
        assert_eq!(run.entries.clone().count(), 256);
    }

    #[test]
    fn errors_name_what_failed() {
        let e = Error::Tampered { what: "run file" };
        assert_eq!(
            e.to_string(),
            "run file does not match what the log committed to"
        );
        let e: Error = std::io::Error::other("disk full").into();
        assert!(e.to_string().starts_with("store: "));
    }
}
