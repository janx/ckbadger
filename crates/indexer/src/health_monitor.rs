//! Long-running health monitor for live sync.
//!
//! Samples key indexer metrics every minute, persists one row per hour to
//! a CSV file, and emits a debounced WARN if the DB write stage shows
//! sustained degradation (the failure mode that produced the original
//! 4112-input parser stall on block 19212685).
//!
//! Output file is derived from the indexer's `bulk_sync_perf_output_root`:
//! the parent directory gains a `live-sync-health.csv` file. The CSV is
//! append-only; on first run the header is written.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::time::{interval, Instant};
use tracing::{info, warn};

use crate::sync::{Indexer, ParserCellLookupSnapshot};

/// Sampling cadence. One sample per minute is plenty for trend analysis
/// while being negligible cost.
const SAMPLE_INTERVAL_SECS: u64 = 60;

/// CSV row cadence. We keep one hour of in-memory samples; once full, the
/// average is written and the buffer rotates.
const SAMPLES_PER_HOUR: usize = 60;

/// If the trailing 60-minute average of `db_stage_write_ms` exceeds this,
/// emit a WARN. Live sync at the chain tip should be sub-second; 1000 ms
/// is ~10× nominal and a clear signal that read/write performance is
/// degrading toward the original stall mode.
const DEGRADATION_DB_STAGE_WARN_MS: f64 = 1000.0;

/// If more than this many slow chunks accrue in a single sampling window
/// (one minute), emit a WARN. PR-1's chunk path normally produces zero
/// slow chunks per minute at the tip; >10 is anomalous.
const DEGRADATION_SLOW_CHUNK_PER_MIN: u64 = 10;

/// Minimum gap between repeated WARN emissions to avoid log flooding when
/// the system is stuck in a degraded state.
const WARN_DEBOUNCE_SECS: u64 = 600;

#[derive(Debug, Clone, Copy)]
struct Sample {
    db_stage_write_ms: f64,
    db_commit_ms: f64,
    block_cache_mb: u64,
    l0_files: u64,
    l0_max: u64,
    sst_size_gb: f64,
    chunks_delta: u64,
    slow_chunks_delta: u64,
    timeouts_delta: u64,
    keys_delta: u64,
    elapsed_us_delta: u64,
    current_block: u64,
    target_block: u64,
}

/// Spawn the health monitor as a long-running background task. Returns
/// immediately; the task survives until the process exits.
pub fn spawn(indexer: Arc<Indexer>, bulk_sync_perf_output_root: &str) {
    let csv_path = derive_csv_path(bulk_sync_perf_output_root);
    tokio::spawn(async move {
        if let Err(e) = run(indexer, csv_path).await {
            warn!(error = %e, "live-sync health monitor exited unexpectedly");
        }
    });
}

fn derive_csv_path(bulk_sync_perf_output_root: &str) -> PathBuf {
    let root = PathBuf::from(bulk_sync_perf_output_root);
    // Place CSV alongside the bulk-sync perf root (e.g. /workdir/perf/).
    let parent = root.parent().map(PathBuf::from).unwrap_or(root);
    parent.join("live-sync-health.csv")
}

async fn run(indexer: Arc<Indexer>, csv_path: PathBuf) -> anyhow::Result<()> {
    if let Some(parent) = csv_path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
    }
    ensure_header(&csv_path).await?;

    info!(path = %csv_path.display(), "live-sync health monitor started");

    let mut ticker = interval(Duration::from_secs(SAMPLE_INTERVAL_SECS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut buffer: Vec<Sample> = Vec::with_capacity(SAMPLES_PER_HOUR);
    let mut last_lookup_snap: ParserCellLookupSnapshot = indexer.parser_cell_lookup_snapshot();
    let mut last_warn_at: Option<Instant> = None;

    loop {
        ticker.tick().await;

        let curr_lookup = indexer.parser_cell_lookup_snapshot();
        let lookup_delta = delta(&last_lookup_snap, &curr_lookup);
        last_lookup_snap = curr_lookup;

        let (_fetch_ms, db_stage_ms, db_commit_ms) = indexer.perf_snapshot_ms();
        let memory = indexer.get_memory_stats();
        let progress = indexer.progress();
        let sample = Sample {
            db_stage_write_ms: db_stage_ms,
            db_commit_ms,
            block_cache_mb: memory.rocksdb_block_cache_bytes / (1024 * 1024),
            l0_files: memory.l0_files_count,
            l0_max: memory.l0_files_max,
            sst_size_gb: memory.sst_files_size as f64 / (1024.0 * 1024.0 * 1024.0),
            chunks_delta: lookup_delta.chunks_total,
            slow_chunks_delta: lookup_delta.slow_chunks_total,
            timeouts_delta: lookup_delta.timeouts_total,
            keys_delta: lookup_delta.keys_total,
            elapsed_us_delta: lookup_delta.elapsed_us_total,
            current_block: progress.current(),
            target_block: progress.target(),
        };

        // Per-minute degradation alert (debounced).
        check_degradation(&sample, &mut last_warn_at);

        buffer.push(sample);
        if buffer.len() >= SAMPLES_PER_HOUR {
            if let Err(e) = write_hourly_row(&csv_path, &buffer).await {
                warn!(error = %e, "failed to write live-sync-health.csv row");
            }
            buffer.clear();
        }
    }
}

fn delta(
    prev: &ParserCellLookupSnapshot,
    curr: &ParserCellLookupSnapshot,
) -> ParserCellLookupSnapshot {
    ParserCellLookupSnapshot {
        chunks_total: curr.chunks_total.saturating_sub(prev.chunks_total),
        slow_chunks_total: curr
            .slow_chunks_total
            .saturating_sub(prev.slow_chunks_total),
        timeouts_total: curr.timeouts_total.saturating_sub(prev.timeouts_total),
        keys_total: curr.keys_total.saturating_sub(prev.keys_total),
        elapsed_us_total: curr.elapsed_us_total.saturating_sub(prev.elapsed_us_total),
    }
}

fn check_degradation(sample: &Sample, last_warn_at: &mut Option<Instant>) {
    let mut reasons: Vec<String> = Vec::new();
    if sample.db_stage_write_ms >= DEGRADATION_DB_STAGE_WARN_MS {
        reasons.push(format!(
            "db_stage_write_ms={:.0} >= {:.0}",
            sample.db_stage_write_ms, DEGRADATION_DB_STAGE_WARN_MS
        ));
    }
    if sample.slow_chunks_delta > DEGRADATION_SLOW_CHUNK_PER_MIN {
        reasons.push(format!(
            "slow_chunks_per_min={} > {}",
            sample.slow_chunks_delta, DEGRADATION_SLOW_CHUNK_PER_MIN
        ));
    }
    if sample.timeouts_delta > 0 {
        reasons.push(format!("parser_timeouts_per_min={}", sample.timeouts_delta));
    }
    if reasons.is_empty() {
        return;
    }
    let now = Instant::now();
    let should_emit = match *last_warn_at {
        None => true,
        Some(t) => now.duration_since(t) >= Duration::from_secs(WARN_DEBOUNCE_SECS),
    };
    if !should_emit {
        return;
    }
    *last_warn_at = Some(now);
    warn!(
        db_stage_write_ms = sample.db_stage_write_ms,
        db_commit_ms = sample.db_commit_ms,
        block_cache_mb = sample.block_cache_mb,
        l0_files = sample.l0_files,
        slow_chunks_per_min = sample.slow_chunks_delta,
        parser_timeouts_per_min = sample.timeouts_delta,
        reasons = reasons.join(","),
        "live-sync health: degradation detected"
    );
}

async fn ensure_header(path: &PathBuf) -> anyhow::Result<()> {
    if tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(());
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    f.write_all(
        b"timestamp,current_block,target_block,db_stage_write_ms_avg,db_commit_ms_avg,\
          block_cache_mb_avg,l0_files_avg,l0_max_peak,sst_size_gb_last,\
          chunks_per_hour,slow_chunks_per_hour,timeouts_per_hour,\
          keys_per_hour,avg_us_per_chunk\n",
    )
    .await?;
    Ok(())
}

async fn write_hourly_row(path: &PathBuf, buffer: &[Sample]) -> anyhow::Result<()> {
    let n = buffer.len() as f64;
    if n == 0.0 {
        return Ok(());
    }
    let avg_db_stage = buffer.iter().map(|s| s.db_stage_write_ms).sum::<f64>() / n;
    let avg_db_commit = buffer.iter().map(|s| s.db_commit_ms).sum::<f64>() / n;
    let avg_block_cache = buffer.iter().map(|s| s.block_cache_mb).sum::<u64>() as f64 / n;
    let avg_l0 = buffer.iter().map(|s| s.l0_files).sum::<u64>() as f64 / n;
    let peak_l0_max = buffer.iter().map(|s| s.l0_max).max().unwrap_or(0);
    let last_sst_gb = buffer.last().map(|s| s.sst_size_gb).unwrap_or(0.0);
    let chunks_per_hour: u64 = buffer.iter().map(|s| s.chunks_delta).sum();
    let slow_per_hour: u64 = buffer.iter().map(|s| s.slow_chunks_delta).sum();
    let timeouts_per_hour: u64 = buffer.iter().map(|s| s.timeouts_delta).sum();
    let keys_per_hour: u64 = buffer.iter().map(|s| s.keys_delta).sum();
    let elapsed_us_per_hour: u64 = buffer.iter().map(|s| s.elapsed_us_delta).sum();
    let avg_us_per_chunk = if chunks_per_hour > 0 {
        elapsed_us_per_hour as f64 / chunks_per_hour as f64
    } else {
        0.0
    };
    let last = buffer.last().unwrap();
    let row = format!(
        "{},{},{},{:.1},{:.1},{:.0},{:.1},{},{:.2},{},{},{},{},{:.0}\n",
        Utc::now().to_rfc3339(),
        last.current_block,
        last.target_block,
        avg_db_stage,
        avg_db_commit,
        avg_block_cache,
        avg_l0,
        peak_l0_max,
        last_sst_gb,
        chunks_per_hour,
        slow_per_hour,
        timeouts_per_hour,
        keys_per_hour,
        avg_us_per_chunk,
    );
    let mut f = OpenOptions::new().append(true).open(path).await?;
    f.write_all(row.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_csv_path_uses_parent_dir() {
        let p = derive_csv_path("/workdir/perf/bulk-sync");
        assert_eq!(p, PathBuf::from("/workdir/perf/live-sync-health.csv"));
    }

    #[test]
    fn derive_csv_path_handles_no_parent() {
        // Edge case: relative single-segment path falls back to that path's dir.
        let p = derive_csv_path("bulk-sync");
        assert_eq!(p, PathBuf::from("live-sync-health.csv"));
    }

    #[test]
    fn delta_handles_counter_resets_safely() {
        // saturating_sub guards against the (unlikely) case of a fresh
        // counter going backwards; we should never panic.
        let prev = ParserCellLookupSnapshot {
            chunks_total: 100,
            slow_chunks_total: 5,
            timeouts_total: 0,
            keys_total: 50_000,
            elapsed_us_total: 1_000_000,
        };
        let curr = ParserCellLookupSnapshot {
            chunks_total: 50, // backwards
            slow_chunks_total: 10,
            timeouts_total: 1,
            keys_total: 60_000,
            elapsed_us_total: 1_500_000,
        };
        let d = delta(&prev, &curr);
        assert_eq!(d.chunks_total, 0);
        assert_eq!(d.slow_chunks_total, 5);
        assert_eq!(d.timeouts_total, 1);
        assert_eq!(d.keys_total, 10_000);
        assert_eq!(d.elapsed_us_total, 500_000);
    }
}
