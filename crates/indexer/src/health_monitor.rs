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

use std::path::{Path, PathBuf};
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
    // Flush-side signals — added to verify WBM tuning actually reduces
    // flush frequency (the suspected dominant cost in per-block commit).
    num_running_flushes: u64,
    mem_table_flush_pending: u64,
    active_memtable_mb: u64,
    wbm_usage_mb: u64,
    wbm_budget_mb: u64,
    // Per-phase decomposition of the writer step. We accumulate these
    // alongside db_stage / db_commit and report the CPU build cost vs
    // the I/O commit cost separately so the next investigation step
    // (after the WBM-not-the-bottleneck finding) has direct attribution.
    precompute_ms: f64,
    build_ms: f64,
    finalize_ms: f64,
    // Commit window split (P3.2). `db_commit_ms` above IS the wide window
    // (`commit_phase_total_ms` in the writer's own metrics); these four are its
    // non-overlapping parts and sum to at most it. There is deliberately no
    // second column for the total.
    commit_prepare_ms: f64,
    script_rollup_ms: f64,
    append_only_commit_synced_ms: f64,
    domain_commit_ms: f64,
    // Flush-storm outcome signals (P3.3).
    sst_files_total: u64,
    manifest_bytes: u64,
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
    let csv_path = ensure_header(&csv_path).await?;

    info!(
        path = %csv_path.display(),
        schema = CSV_SCHEMA_VERSION,
        "live-sync health monitor started"
    );

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
        let (precompute_ms, build_ms, finalize_ms) = indexer.perf_write_phase_snapshot_ms();
        let (commit_prepare_ms, script_rollup_ms, append_only_commit_synced_ms, domain_commit_ms) =
            indexer.perf_commit_phase_snapshot_ms();
        let memory = indexer.get_memory_stats();
        let flush = indexer.flush_stats();
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
            num_running_flushes: flush.num_running_flushes,
            mem_table_flush_pending: flush.mem_table_flush_pending,
            active_memtable_mb: flush.active_memtable_bytes / (1024 * 1024),
            wbm_usage_mb: flush.wbm_usage_bytes / (1024 * 1024),
            wbm_budget_mb: flush.wbm_budget_bytes / (1024 * 1024),
            precompute_ms,
            build_ms,
            finalize_ms,
            commit_prepare_ms,
            script_rollup_ms,
            append_only_commit_synced_ms,
            domain_commit_ms,
            sst_files_total: memory.sst_files_total,
            manifest_bytes: memory.manifest_bytes,
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

/// CSV schema version. Bump together with [`csv_header`] whenever the column
/// set changes: an existing file whose first line is a different header is
/// never appended to, the monitor starts a schema-suffixed file instead.
///
/// There is deliberately no flush-ROUND column. RocksDB exposes no cumulative
/// flush counter, so flush rounds are counted from the `flush_started` events
/// in the RocksDB LOG; `sst_files_last` and `manifest_mb_last` are the exact
/// standing consequences of flush frequency, and `flush_pending_peak` /
/// `flush_observed_in_window` remain the sampled activity signals.
const CSV_SCHEMA_VERSION: u32 = 3;

/// The one definition of the column set. Every row is produced by
/// [`format_hourly_row`] from the same list, and a unit test pins the two to
/// the same column count.
fn csv_header() -> &'static str {
    "schema=3,timestamp,current_block,target_block,db_stage_write_ms_avg,db_commit_ms_avg,\
     block_cache_mb_avg,l0_files_avg,l0_max_peak,sst_size_gb_last,\
     chunks_per_hour,slow_chunks_per_hour,timeouts_per_hour,\
     keys_per_hour,avg_us_per_chunk,\
     flush_pending_peak,active_memtable_mb_avg,wbm_usage_mb_avg,wbm_budget_mb_last,\
     flush_observed_in_window,\
     precompute_ms_avg,build_ms_avg,finalize_ms_avg,\
     commit_prepare_ms_avg,script_rollup_ms_avg,append_only_commit_synced_ms_avg,\
     domain_commit_ms_avg,\
     sst_files_last,manifest_mb_last\n"
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CsvTarget {
    /// The file already carries this exact header: append rows to it.
    Append(PathBuf),
    /// No file with this header yet: create it and write the header first.
    Create(PathBuf),
}

impl CsvTarget {
    fn path(&self) -> &PathBuf {
        match self {
            Self::Append(path) | Self::Create(path) => path,
        }
    }
}

/// Decide which file this process writes rows into, given the first line of
/// the base file (`None` when it does not exist).
///
/// Rows of two different column sets must never share a file, so a foreign
/// header sends this run to `<stem>.schema<N>.csv` instead of appending.
fn csv_target_path(base: &Path, existing_first_line: Option<&str>) -> CsvTarget {
    match existing_first_line {
        None => CsvTarget::Create(base.to_path_buf()),
        Some(line) if line.trim_end() == csv_header().trim_end() => {
            CsvTarget::Append(base.to_path_buf())
        }
        Some(_) => {
            let stem = base
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "live-sync-health".to_string());
            let extension = base
                .extension()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "csv".to_string());
            CsvTarget::Create(
                base.with_file_name(format!("{stem}.schema{CSV_SCHEMA_VERSION}.{extension}")),
            )
        }
    }
}

async fn first_line_of(path: &Path) -> anyhow::Result<Option<String>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            Ok(Some(
                text.split('\n').next().unwrap_or_default().to_string(),
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Resolve the file this run appends to, creating it with the current header
/// when needed. Returns the resolved path.
async fn ensure_header(path: &Path) -> anyhow::Result<PathBuf> {
    let first_line = first_line_of(path).await?;
    let target = csv_target_path(path, first_line.as_deref());
    let resolved = target.path().clone();
    if let CsvTarget::Create(create_path) = &target {
        // A schema-suffixed file that already exists must carry this exact
        // header — otherwise the column set changed without a version bump and
        // mixing would silently corrupt the series.
        if let Some(existing) = first_line_of(create_path).await? {
            if existing.trim_end() != csv_header().trim_end() {
                anyhow::bail!(
                    "live-sync-health CSV {} exists with a different header for schema {}; \
                     bump CSV_SCHEMA_VERSION instead of mixing column sets",
                    create_path.display(),
                    CSV_SCHEMA_VERSION
                );
            }
            return Ok(resolved);
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(create_path)
            .await?;
        f.write_all(csv_header().as_bytes()).await?;
    }
    Ok(resolved)
}

async fn write_hourly_row(path: &Path, buffer: &[Sample]) -> anyhow::Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }
    let row = format_hourly_row(buffer);
    let mut f = OpenOptions::new().append(true).open(path).await?;
    f.write_all(row.as_bytes()).await?;
    Ok(())
}

/// Render one hourly row. Split out from the write so the column count can be
/// pinned against [`csv_header`] in a unit test.
fn format_hourly_row(buffer: &[Sample]) -> String {
    let n = buffer.len() as f64;
    assert!(n > 0.0, "format_hourly_row requires at least one sample");
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
    let flush_pending_peak = buffer
        .iter()
        .map(|s| s.mem_table_flush_pending)
        .max()
        .unwrap_or(0);
    let avg_active_mt = buffer.iter().map(|s| s.active_memtable_mb).sum::<u64>() as f64 / n;
    let avg_wbm_usage = buffer.iter().map(|s| s.wbm_usage_mb).sum::<u64>() as f64 / n;
    let last_wbm_budget = buffer.last().map(|s| s.wbm_budget_mb).unwrap_or(0);
    // "Flush observed" = at least one sample saw num_running_flushes > 0.
    // With 60-CF atomic flush the per-minute sample window catches each
    // flush wave with high probability. This is a minute-resolution
    // signal of "did we flush in this hour" — useful for verifying that
    // the WBM cap change actually stretched flush intervals.
    let flush_observed: u64 = buffer.iter().filter(|s| s.num_running_flushes > 0).count() as u64;
    let avg_precompute = buffer.iter().map(|s| s.precompute_ms).sum::<f64>() / n;
    let avg_build = buffer.iter().map(|s| s.build_ms).sum::<f64>() / n;
    let avg_finalize = buffer.iter().map(|s| s.finalize_ms).sum::<f64>() / n;
    let avg_commit_prepare = buffer.iter().map(|s| s.commit_prepare_ms).sum::<f64>() / n;
    let avg_script_rollup = buffer.iter().map(|s| s.script_rollup_ms).sum::<f64>() / n;
    let avg_append_only_commit = buffer
        .iter()
        .map(|s| s.append_only_commit_synced_ms)
        .sum::<f64>()
        / n;
    let avg_domain_commit = buffer.iter().map(|s| s.domain_commit_ms).sum::<f64>() / n;
    let last = buffer.last().expect("buffer is non-empty");
    format!(
        "{},{},{},{},{:.1},{:.1},{:.0},{:.1},{},{:.2},{},{},{},{},{:.0},{},{:.0},{:.0},{},{},\
         {:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{},{}\n",
        CSV_SCHEMA_VERSION,
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
        flush_pending_peak,
        avg_active_mt,
        avg_wbm_usage,
        last_wbm_budget,
        flush_observed,
        avg_precompute,
        avg_build,
        avg_finalize,
        avg_commit_prepare,
        avg_script_rollup,
        avg_append_only_commit,
        avg_domain_commit,
        last.sst_files_total,
        last.manifest_bytes / (1024 * 1024),
    )
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
    fn csv_header_declares_the_schema_version() {
        assert!(
            csv_header().starts_with(&format!("schema={CSV_SCHEMA_VERSION},")),
            "the header line must declare its schema: {}",
            csv_header()
        );
    }

    #[test]
    fn csv_row_column_count_matches_the_header() {
        let sample = Sample {
            db_stage_write_ms: 1.0,
            db_commit_ms: 2.0,
            block_cache_mb: 3,
            l0_files: 4,
            l0_max: 5,
            sst_size_gb: 6.0,
            chunks_delta: 7,
            slow_chunks_delta: 8,
            timeouts_delta: 9,
            keys_delta: 10,
            elapsed_us_delta: 11,
            current_block: 12,
            target_block: 13,
            num_running_flushes: 14,
            mem_table_flush_pending: 15,
            active_memtable_mb: 16,
            wbm_usage_mb: 17,
            wbm_budget_mb: 18,
            precompute_ms: 19.0,
            build_ms: 20.0,
            finalize_ms: 21.0,
            commit_prepare_ms: 22.0,
            script_rollup_ms: 23.0,
            append_only_commit_synced_ms: 24.0,
            domain_commit_ms: 25.0,
            sst_files_total: 27,
            manifest_bytes: 28 * 1024 * 1024,
        };
        let row = format_hourly_row(&[sample]);
        assert_eq!(
            row.trim_end().split(',').count(),
            csv_header().trim_end().split(',').count(),
            "row: {row}"
        );
    }

    /// An existing file written by an older column set must never gain rows
    /// with a different shape: the monitor starts a schema-suffixed file
    /// instead of mixing.
    #[test]
    fn an_old_header_rotates_to_a_schema_suffixed_file() {
        let base = PathBuf::from("/workdir/perf/live-sync-health.csv");
        assert_eq!(
            csv_target_path(&base, None),
            CsvTarget::Create(base.clone())
        );
        assert_eq!(
            csv_target_path(&base, Some(csv_header())),
            CsvTarget::Append(base.clone())
        );
        assert_eq!(
            csv_target_path(&base, Some("timestamp,current_block,target_block\n")),
            CsvTarget::Create(PathBuf::from(format!(
                "/workdir/perf/live-sync-health.schema{CSV_SCHEMA_VERSION}.csv"
            )))
        );
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
