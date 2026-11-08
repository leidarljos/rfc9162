//! Baseline timings: `find` through the logged runs against `find` through the
//! tiles sidecars, on one log of N entries in a directory.
//!
//!     cargo run --release -p rfc9162-log --example bench -- <dir> <entries> [seal_every]
//!
//! Entries look like `module=m<i> pkg=p<i mod 97> target=t<i mod 4>`, keys are
//! the fields. 1000 lookups of `module=` keys drawn evenly over the log, each
//! answered through both paths; prints the median and the 99th percentile in
//! microseconds, and the bytes of runs and sidecars.

use std::sync::Arc;
use std::time::Instant;

use rfc9162_log::{AuthenticatedLog, Keys, Store};
use rfc9162_tiles::{PosixStore, Reader, TileStore};

fn keys() -> Keys {
    Arc::new(|entry: &[u8]| {
        entry
            .split(|b| *b == b' ')
            .filter(|f| !f.is_empty())
            .map(<[u8]>::to_vec)
            .collect()
    })
}

fn tiles_keys() -> rfc9162_tiles::KeyFn {
    let k = keys();
    Box::new(move |entry: &[u8]| {
        if entry.starts_with(rfc9162_log::store::MANIFEST_MAGIC) {
            Vec::new()
        } else {
            k(entry)
        }
    })
}

fn entry(i: u64) -> Vec<u8> {
    format!("module=m{i} pkg=p{} target=t{}", i % 97, i % 4).into_bytes()
}

fn percentiles(mut us: Vec<u128>) -> (u128, u128) {
    us.sort_unstable();
    let p = |q: f64| us[((us.len() - 1) as f64 * q) as usize];
    (p(0.5), p(0.99))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).expect("dir");
    let n: u64 = args.get(2).expect("entries").parse().expect("entries");
    let seal_every: u64 = args
        .get(3)
        .map_or(10_000, |s| s.parse().expect("seal_every"));
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("dir");

    let fill = Instant::now();
    let mut store = Store::open(PosixStore::open(dir).expect("posix"), keys()).expect("open");
    for i in 0..n {
        store.append(&entry(i)).expect("append");
        if (i + 1) % seal_every == 0 {
            store.seal().expect("seal");
        }
    }
    if n % seal_every != 0 {
        store.seal().expect("seal");
    }
    let fill = fill.elapsed();
    let runs = store.live_runs().len();

    let compact = Instant::now();
    let compacted = if runs > 1 { store.compact().ok() } else { None };
    let compact = compact.elapsed();

    let probes: Vec<Vec<u8>> = (0..1000u64)
        .map(|k| format!("module=m{}", k * n / 1000).into_bytes())
        .collect();

    let mut store_us = Vec::with_capacity(probes.len());
    for key in &probes {
        let t = Instant::now();
        let hits = store.find(key).expect("find");
        store_us.push(t.elapsed().as_micros());
        assert_eq!(hits.len(), 1, "{}", String::from_utf8_lossy(key));
    }
    let (s50, s99) = percentiles(store_us);

    let reader = Reader::open(store.writer().store(), tiles_keys()).expect("reader");
    let mut tiles_us = Vec::with_capacity(probes.len());
    for key in &probes {
        let t = Instant::now();
        let hits = reader.find(key).expect("find");
        tiles_us.push(t.elapsed().as_micros());
        assert_eq!(hits.len(), 1);
    }
    let (t50, t99) = percentiles(tiles_us);

    let bytes = |prefix: &str| -> usize {
        let s = store.writer().store();
        s.list(prefix)
            .expect("list")
            .iter()
            .map(|p| s.get(p).expect("get").map_or(0, |b| b.len()))
            .sum()
    };
    println!(
        "entries={n} seal_every={seal_every} runs_before_compaction={runs} compacted={} fill_ms={} compact_ms={}",
        compacted.is_some(),
        fill.as_millis(),
        compact.as_millis()
    );
    println!("store_find_us p50={s50} p99={s99}");
    println!("tiles_find_us p50={t50} p99={t99}");
    println!("run_bytes={} tile_bytes={}", bytes("runs"), bytes("tile"));
}
