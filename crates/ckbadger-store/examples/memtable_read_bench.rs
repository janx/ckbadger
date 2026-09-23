//! Memtable point-read microbenchmark: SkipList vs VectorRep.
//!
//! Task 5.2. This is the repeatable release-build form of the throwaway
//! diagnostic in
//! `work/incident-archive/20260922/sync-root-cause-20260922/memtable-bench.rs`,
//! which produced the 2.471 ms / 21,755 ms pair that motivated moving live sync
//! off VectorRep. Those numbers came from a **debug** build and are evidence
//! only — never a baseline. Run this one for any figure that gets compared.
//!
//! ```bash
//! cargo run --release -p ckbadger-store --example memtable_read_bench
//! ```
//!
//! Fixed inputs, identical for every configuration: 20,000 `TOKEN_DAILY` rows
//! written in one `StoreBatch`, then 512 point reads whose summed
//! `owned_capacity_delta` is asserted equal across every configuration — a
//! configuration that reads faster by reading less would fail the run.
//!
//! Three read regimes per configuration, measured from the same write pass:
//!
//! - `hot` — straight after commit, the data still in the active memtable.
//!   This is the regime live sync actually reads in: every batch reads back
//!   keys it just wrote. VectorRep is unsorted, so a point read here is a
//!   linear scan of the whole memtable.
//! - `flushed` — after `flush_all_memtables()`, i.e. sorted SSTs with bloom
//!   filters. Both memtable kinds converge here, which is why the regression
//!   never showed up in flushed-store measurements.
//! - `reopen` — the store closed and reopened on the same directory, so the
//!   reads go through a cold block cache.
//!
//! The two memtable kinds differ only in `StoreRuntimeConfig.vector_memtable`;
//! every other option, including the 1 GB memory budget, is identical.

use std::path::PathBuf;
use std::time::Instant;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::{keys, CkbadgerStore, StoreRuntimeConfig};

/// Rows written per run.
const WRITE_COUNT: u32 = 20_000;
/// Point reads per regime.
const READ_COUNT: u32 = 512;
/// Runs per (memtable kind, regime).
const RUNS: usize = 5;
/// The single UTC+8 day every row is keyed on.
const DATE: u32 = 20_260_915;
/// Odd multiplier, so `i -> i.wrapping_mul(MIX)` is a bijection on u32 and no
/// two rows can collide onto the same key.
const MIX: u32 = 2_654_435_761;
/// Coprime with `WRITE_COUNT`, so the read set walks the key space instead of a
/// contiguous prefix.
const READ_STRIDE: u32 = 7_919;

fn row_key(i: u32) -> [u8; 32] {
    let mut hash = [0u8; 32];
    hash[..4].copy_from_slice(&i.wrapping_mul(MIX).to_be_bytes());
    hash
}

/// `TokenDailyDelta { owned_capacity_delta, owned_knowledge_delta }` as bincode
/// writes it: two little-endian i128s.
fn row_value(i: u32) -> [u8; 32] {
    let mut value = [0u8; 32];
    value[..16].copy_from_slice(&i128::from(i + 100).to_le_bytes());
    value[16..].copy_from_slice(&1i128.to_le_bytes());
    value
}

/// The checksum every configuration must agree on.
fn expected_checksum() -> i128 {
    (0..READ_COUNT)
        .map(|i| i128::from((i * READ_STRIDE) % WRITE_COUNT + 100))
        .sum()
}

fn scratch_dir(kind: &str, run: usize) -> PathBuf {
    let base = std::env::var("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join(format!(
        "ckbadger-memtable-bench-{kind}-{run}-{}",
        std::process::id()
    ))
}

fn open(path: &PathBuf, vector_memtable: bool) -> anyhow::Result<CkbadgerStore> {
    CkbadgerStore::open_domain_with_runtime(
        path,
        StoreRuntimeConfig {
            memory_budget_gb: Some(1),
            vector_memtable,
            ..Default::default()
        },
    )
}

fn write_rows(store: &CkbadgerStore) -> anyhow::Result<f64> {
    let started = Instant::now();
    let mut batch = StoreBatch::new(store);
    for i in 0..WRITE_COUNT {
        batch.put_stats(
            &keys::encode_token_daily_key(&row_key(i), DATE),
            &row_value(i),
        );
    }
    batch.commit()?;
    Ok(started.elapsed().as_secs_f64() * 1000.0)
}

/// 512 point reads; returns `(elapsed_ms, checksum)`.
fn read_rows(store: &CkbadgerStore) -> anyhow::Result<(f64, i128)> {
    let started = Instant::now();
    let mut checksum = 0i128;
    for i in 0..READ_COUNT {
        let row = (i * READ_STRIDE) % WRITE_COUNT;
        let delta = store
            .get_token_daily_delta(&row_key(row), DATE)?
            .ok_or_else(|| anyhow::anyhow!("seeded row {row} missing"))?;
        checksum += delta.owned_capacity_delta;
    }
    Ok((started.elapsed().as_secs_f64() * 1000.0, checksum))
}

fn median(samples: &[f64]) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN timings"));
    sorted[sorted.len() / 2]
}

fn format_samples(samples: &[f64]) -> String {
    samples
        .iter()
        .map(|ms| format!("{ms:.3}"))
        .collect::<Vec<_>>()
        .join(" ")
}

struct KindResult {
    kind: &'static str,
    write: Vec<f64>,
    hot: Vec<f64>,
    flushed: Vec<f64>,
    reopen: Vec<f64>,
}

fn run_kind(kind: &'static str, vector_memtable: bool) -> anyhow::Result<KindResult> {
    let expected = expected_checksum();
    let mut result = KindResult {
        kind,
        write: Vec::with_capacity(RUNS),
        hot: Vec::with_capacity(RUNS),
        flushed: Vec::with_capacity(RUNS),
        reopen: Vec::with_capacity(RUNS),
    };

    for run in 0..RUNS {
        let path = scratch_dir(kind, run);
        let _ = std::fs::remove_dir_all(&path);

        {
            let store = open(&path, vector_memtable)?;
            result.write.push(write_rows(&store)?);

            let (hot_ms, checksum) = read_rows(&store)?;
            anyhow::ensure!(
                checksum == expected,
                "{kind} run {run} hot checksum {checksum} != {expected}"
            );
            result.hot.push(hot_ms);

            store.flush_all_memtables()?;
            let (flushed_ms, checksum) = read_rows(&store)?;
            anyhow::ensure!(
                checksum == expected,
                "{kind} run {run} flushed checksum {checksum} != {expected}"
            );
            result.flushed.push(flushed_ms);
        }

        let store = open(&path, vector_memtable)?;
        let (reopen_ms, checksum) = read_rows(&store)?;
        anyhow::ensure!(
            checksum == expected,
            "{kind} run {run} reopen checksum {checksum} != {expected}"
        );
        result.reopen.push(reopen_ms);
        drop(store);

        std::fs::remove_dir_all(&path)?;
    }

    Ok(result)
}

fn main() -> anyhow::Result<()> {
    println!(
        "memtable point-read microbenchmark: writes={WRITE_COUNT} reads={READ_COUNT} \
         runs={RUNS} checksum={}",
        expected_checksum()
    );
    println!(
        "profile={}",
        if cfg!(debug_assertions) {
            "debug (NOT a baseline — rerun with --release)"
        } else {
            "release"
        }
    );
    println!();

    let results = [run_kind("skiplist", false)?, run_kind("vector", true)?];

    println!("{:<9} {:<8} {:>10}  runs_ms", "kind", "regime", "median_ms");
    for result in &results {
        for (regime, samples) in [
            ("write", &result.write),
            ("hot", &result.hot),
            ("flushed", &result.flushed),
            ("reopen", &result.reopen),
        ] {
            println!(
                "{:<9} {:<8} {:>10.3}  {}",
                result.kind,
                regime,
                median(samples),
                format_samples(samples)
            );
        }
    }

    Ok(())
}
