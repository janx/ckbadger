use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ckbadger_common::{BulkBuildProgressData, PipelineProgressData};
use dashmap::DashMap;
use serde::Serialize;
use tracing::info;

use crate::runtime_diag::{CgroupMemorySnapshot, FlightEvent};

use super::helpers::duration_from_millis;
use super::indexer::next_start_block_from_db_tip;
use super::types::CachedCellInfo;

// ── Constants ───────────────────────────────────────────────────────────

pub(crate) const FLIGHT_RECORDER_CAPACITY: usize = 200;

pub(crate) const CELL_CACHE_CAPACITY: usize = 200_000;

pub(crate) const CHART_INVALIDATION_MAX_LIVE_LAG: u64 = 100;

pub(crate) const PARSER_UNRESOLVED_MAX_RETRIES: usize = 240;

/// Chunk size for parser cell-info batch lookups. Splitting a large batch
/// into bounded chunks (~512 keys) lets us emit per-chunk tracing, cooperate
/// with the timeout budget, and avoid worst-case multi_get blowups on blocks
/// with thousands of inputs.
pub(crate) const PARSER_CELL_LOOKUP_CHUNK_SIZE: usize = 512;

/// Per-key time budget for parser cell-info batch reads (ms). The total
/// budget is `max(MIN_MS, n_keys * PER_KEY_MS).min(MAX_MS)`. 30 ms/key is
/// generous (real reads should be sub-millisecond on cached data, single-
/// digit ms on cold disk reads); the cap protects against pathological
/// stalls without re-introducing the original 30 s hard ceiling that
/// could not accommodate large blocks (e.g. 4112-input block 19212685).
pub(crate) const PARSER_CELL_LOOKUP_PER_KEY_MS: u64 = 30;
pub(crate) const PARSER_CELL_LOOKUP_MIN_MS: u64 = 60_000;
pub(crate) const PARSER_CELL_LOOKUP_MAX_MS: u64 = 600_000;

/// A single chunk that exceeds this latency triggers a WARN log so slow
/// reads stand out without spamming the log on every chunk.
pub(crate) const PARSER_CELL_LOOKUP_SLOW_CHUNK_MS: u128 = 1_000;

// ── ParserCellLookupStats ───────────────────────────────────────────────
//
// Lifetime counters of the parser cell-info lookup path. These are sampled
// (delta-per-period) by the health monitor to detect read-side degradation
// long before the user notices a stall.

#[derive(Default)]
pub(crate) struct ParserCellLookupStats {
    pub(crate) chunks_total: AtomicU64,
    pub(crate) slow_chunks_total: AtomicU64,
    pub(crate) timeouts_total: AtomicU64,
    pub(crate) keys_total: AtomicU64,
    pub(crate) elapsed_us_total: AtomicU64,
}

impl ParserCellLookupStats {
    pub(crate) fn record_chunk(&self, keys: usize, elapsed: Duration, slow: bool) {
        self.chunks_total.fetch_add(1, Ordering::Relaxed);
        self.keys_total.fetch_add(keys as u64, Ordering::Relaxed);
        self.elapsed_us_total
            .fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);
        if slow {
            self.slow_chunks_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_timeout(&self) {
        self.timeouts_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> ParserCellLookupSnapshot {
        ParserCellLookupSnapshot {
            chunks_total: self.chunks_total.load(Ordering::Relaxed),
            slow_chunks_total: self.slow_chunks_total.load(Ordering::Relaxed),
            timeouts_total: self.timeouts_total.load(Ordering::Relaxed),
            keys_total: self.keys_total.load(Ordering::Relaxed),
            elapsed_us_total: self.elapsed_us_total.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ParserCellLookupSnapshot {
    pub(crate) chunks_total: u64,
    pub(crate) slow_chunks_total: u64,
    pub(crate) timeouts_total: u64,
    pub(crate) keys_total: u64,
    pub(crate) elapsed_us_total: u64,
}

// ── IncidentReport ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct IncidentReport {
    pub(crate) incident_id: String,
    pub(crate) run_id: String,
    pub(crate) created_at: i64,
    pub(crate) reason: String,
    pub(crate) detail: String,
    pub(crate) startup_phase: Option<String>,
    pub(crate) pipeline_reset_epoch: u64,
    pub(crate) sync_tip_block: i64,
    pub(crate) sync_tip_hash: String,
    pub(crate) cgroup_memory: CgroupMemorySnapshot,
    pub(crate) recent_events: Vec<FlightEvent>,
}

// ── PerfStats ───────────────────────────────────────────────────────────

#[derive(Default)]
pub(crate) struct PerfStats {
    pub(crate) fetch_us: AtomicU64,
    pub(crate) db_stage_write_us: AtomicU64,
    pub(crate) db_commit_us: AtomicU64,
    pub(crate) last_fetch_us: AtomicU64,
    pub(crate) last_db_stage_write_us: AtomicU64,
    pub(crate) last_db_commit_us: AtomicU64,
    pub(crate) blocks_count: AtomicU64,
    // Per-phase decomposition of the writer step. Lets the health monitor
    // attribute db_stage_write_ms across precompute (CPU pre-batch),
    // build (CPU batch assembly), commit (I/O rocksdb write), and the
    // remainder of finalize (CPU bookkeeping). These are accumulated
    // across batches inside a sampling window and rotated by
    // `report_and_reset` into the corresponding `last_*` slots.
    pub(crate) precompute_us: AtomicU64,
    pub(crate) build_us: AtomicU64,
    pub(crate) finalize_us: AtomicU64,
    pub(crate) last_precompute_us: AtomicU64,
    pub(crate) last_build_us: AtomicU64,
    pub(crate) last_finalize_us: AtomicU64,
    // Commit window split (P3.2). `db_commit_us` above stays the wide window
    // (== commit_phase_total); these four are its non-overlapping parts.
    pub(crate) commit_prepare_us: AtomicU64,
    pub(crate) script_rollup_us: AtomicU64,
    pub(crate) append_only_commit_us: AtomicU64,
    pub(crate) domain_commit_us: AtomicU64,
    pub(crate) last_commit_prepare_us: AtomicU64,
    pub(crate) last_script_rollup_us: AtomicU64,
    pub(crate) last_append_only_commit_us: AtomicU64,
    pub(crate) last_domain_commit_us: AtomicU64,
    /// Monotonic counter bumped when the writer ENTERS a phase. The progress
    /// watchdog compares it across ticks: a committed tip that has not moved
    /// while this keeps climbing is a long batch in flight, not a stall.
    pub(crate) writer_phase_seq: AtomicU64,
}

impl PerfStats {
    #[cfg(test)]
    pub(crate) fn add_fetch(&self, duration: Duration) {
        self.fetch_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    pub(crate) fn add_db_write(&self, duration: Duration) {
        self.db_stage_write_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    pub(crate) fn add_db_commit(&self, duration: Duration) {
        self.db_commit_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Atomically accumulate per-phase write metrics from a completed
    /// `write_parsed_batch` invocation. All durations are in milliseconds
    /// (matching the source `BatchWriteMetrics`).
    pub(crate) fn add_write_phase_ms(&self, precompute_ms: f64, build_ms: f64, finalize_ms: f64) {
        let to_us = |ms: f64| ms.max(0.0).round() as u64 * 1000;
        self.precompute_us
            .fetch_add(to_us(precompute_ms), Ordering::Relaxed);
        self.build_us.fetch_add(to_us(build_ms), Ordering::Relaxed);
        self.finalize_us
            .fetch_add(to_us(finalize_ms), Ordering::Relaxed);
    }

    /// Bump the writer-phase heartbeat. Called when a writer phase STARTS.
    pub(crate) fn mark_writer_phase(&self) {
        self.writer_phase_seq.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn writer_phase_seq(&self) -> u64 {
        self.writer_phase_seq.load(Ordering::Relaxed)
    }

    /// Accumulate the five-part commit window split of one completed
    /// `write_parsed_batch`. All durations are in milliseconds.
    pub(crate) fn add_commit_phase_ms(
        &self,
        commit_prepare_ms: f64,
        script_rollup_ms: f64,
        append_only_commit_synced_ms: f64,
        domain_commit_ms: f64,
    ) {
        let to_us = |ms: f64| (ms.max(0.0) * 1000.0).round() as u64;
        self.commit_prepare_us
            .fetch_add(to_us(commit_prepare_ms), Ordering::Relaxed);
        self.script_rollup_us
            .fetch_add(to_us(script_rollup_ms), Ordering::Relaxed);
        self.append_only_commit_us
            .fetch_add(to_us(append_only_commit_synced_ms), Ordering::Relaxed);
        self.domain_commit_us
            .fetch_add(to_us(domain_commit_ms), Ordering::Relaxed);
    }

    /// Commit-window decomposition for the health monitor. Falls back to the
    /// latest completed window when the current accumulator is empty.
    /// Returns (commit_prepare_ms, script_rollup_ms, append_only_commit_ms,
    /// domain_commit_ms).
    pub(crate) fn commit_phase_snapshot_ms(&self) -> (f64, f64, f64, f64) {
        let pick = |current: &AtomicU64, last: &AtomicU64| {
            let v = current.load(Ordering::Relaxed);
            let v = if v > 0 {
                v
            } else {
                last.load(Ordering::Relaxed)
            };
            v as f64 / 1000.0
        };
        (
            pick(&self.commit_prepare_us, &self.last_commit_prepare_us),
            pick(&self.script_rollup_us, &self.last_script_rollup_us),
            pick(
                &self.append_only_commit_us,
                &self.last_append_only_commit_us,
            ),
            pick(&self.domain_commit_us, &self.last_domain_commit_us),
        )
    }

    pub(crate) fn report_and_reset(&self) {
        let blocks = self.blocks_count.swap(0, Ordering::Relaxed);
        if blocks == 0 {
            return;
        }
        let fetch_us = self.fetch_us.swap(0, Ordering::Relaxed);
        let db_stage_us = self.db_stage_write_us.swap(0, Ordering::Relaxed);
        let db_commit_us = self.db_commit_us.swap(0, Ordering::Relaxed);
        let precompute_us = self.precompute_us.swap(0, Ordering::Relaxed);
        let build_us = self.build_us.swap(0, Ordering::Relaxed);
        let finalize_us = self.finalize_us.swap(0, Ordering::Relaxed);
        let commit_prepare_us = self.commit_prepare_us.swap(0, Ordering::Relaxed);
        let script_rollup_us = self.script_rollup_us.swap(0, Ordering::Relaxed);
        let append_only_commit_us = self.append_only_commit_us.swap(0, Ordering::Relaxed);
        let domain_commit_us = self.domain_commit_us.swap(0, Ordering::Relaxed);
        self.last_fetch_us.store(fetch_us, Ordering::Relaxed);
        self.last_db_stage_write_us
            .store(db_stage_us, Ordering::Relaxed);
        self.last_db_commit_us
            .store(db_commit_us, Ordering::Relaxed);
        self.last_precompute_us
            .store(precompute_us, Ordering::Relaxed);
        self.last_build_us.store(build_us, Ordering::Relaxed);
        self.last_finalize_us.store(finalize_us, Ordering::Relaxed);
        self.last_commit_prepare_us
            .store(commit_prepare_us, Ordering::Relaxed);
        self.last_script_rollup_us
            .store(script_rollup_us, Ordering::Relaxed);
        self.last_append_only_commit_us
            .store(append_only_commit_us, Ordering::Relaxed);
        self.last_domain_commit_us
            .store(domain_commit_us, Ordering::Relaxed);

        let fetch_ms = fetch_us as f64 / 1000.0;
        let db_stage_ms = db_stage_us as f64 / 1000.0;
        let db_commit_ms = db_commit_us as f64 / 1000.0;
        info!(
            blocks,
            fetch_ms = format!("{:.1}", fetch_ms),
            db_stage_ms = format!("{:.1}", db_stage_ms),
            // db_commit_ms is the wide commit window; the four fields after it
            // are its non-overlapping parts (P3.2).
            db_commit_ms = format!("{:.1}", db_commit_ms),
            commit_prepare_ms = format!("{:.1}", commit_prepare_us as f64 / 1000.0),
            script_rollup_ms = format!("{:.1}", script_rollup_us as f64 / 1000.0),
            append_only_commit_synced_ms = format!("{:.1}", append_only_commit_us as f64 / 1000.0),
            domain_commit_ms = format!("{:.1}", domain_commit_us as f64 / 1000.0),
            precompute_ms = format!("{:.1}", precompute_us as f64 / 1000.0),
            build_ms = format!("{:.1}", build_us as f64 / 1000.0),
            finalize_ms = format!("{:.1}", finalize_us as f64 / 1000.0),
            "Batch perf"
        );
    }

    /// Snapshot current accumulated values, falling back to the latest completed batch.
    /// Returns (fetch_ms, db_stage_write_ms, db_commit_ms).
    pub(crate) fn snapshot_ms(&self) -> (f64, f64, f64) {
        let rpc = self.fetch_us.load(Ordering::Relaxed);
        let db_stage = self.db_stage_write_us.load(Ordering::Relaxed);
        let db_commit = self.db_commit_us.load(Ordering::Relaxed);
        let rpc = if rpc > 0 {
            rpc
        } else {
            self.last_fetch_us.load(Ordering::Relaxed)
        };
        let db_stage = if db_stage > 0 {
            db_stage
        } else {
            self.last_db_stage_write_us.load(Ordering::Relaxed)
        };
        let db_commit = if db_commit > 0 {
            db_commit
        } else {
            self.last_db_commit_us.load(Ordering::Relaxed)
        };
        (
            rpc as f64 / 1000.0,
            db_stage as f64 / 1000.0,
            db_commit as f64 / 1000.0,
        )
    }

    /// Per-phase decomposition snapshot for the health monitor. Falls back
    /// to the latest completed window when the current accumulator is
    /// empty so post-restart polls still return useful values.
    /// Returns (precompute_ms, build_ms, finalize_ms).
    pub(crate) fn write_phase_snapshot_ms(&self) -> (f64, f64, f64) {
        let pre = self.precompute_us.load(Ordering::Relaxed);
        let build = self.build_us.load(Ordering::Relaxed);
        let fin = self.finalize_us.load(Ordering::Relaxed);
        let pre = if pre > 0 {
            pre
        } else {
            self.last_precompute_us.load(Ordering::Relaxed)
        };
        let build = if build > 0 {
            build
        } else {
            self.last_build_us.load(Ordering::Relaxed)
        };
        let fin = if fin > 0 {
            fin
        } else {
            self.last_finalize_us.load(Ordering::Relaxed)
        };
        (
            pre as f64 / 1000.0,
            build as f64 / 1000.0,
            fin as f64 / 1000.0,
        )
    }
}

// ── PipelinePerfStats ───────────────────────────────────────────────────

#[derive(Default)]
pub(crate) struct PipelinePerfStats {
    pub(crate) last_fetch_us: AtomicU64,
    pub(crate) last_parse_us: AtomicU64,
    pub(crate) last_write_us: AtomicU64,
    pub(crate) last_write_commit_us: AtomicU64,
    pub(crate) last_writer_wait_us: AtomicU64,
    pub(crate) fetch_queue_depth: AtomicU64,
    pub(crate) fetch_queue_capacity: AtomicU64,
    pub(crate) parse_queue_depth: AtomicU64,
    pub(crate) parse_queue_capacity: AtomicU64,
    pub(crate) writer_queue_depth: AtomicU64,
    pub(crate) writer_queue_capacity: AtomicU64,
}

impl PipelinePerfStats {
    pub(crate) fn set_queue_capacities(&self, fetch_capacity: usize, parse_capacity: usize) {
        self.fetch_queue_capacity
            .store(fetch_capacity as u64, Ordering::Relaxed);
        self.parse_queue_capacity
            .store(parse_capacity as u64, Ordering::Relaxed);
        self.writer_queue_capacity
            .store(parse_capacity as u64, Ordering::Relaxed);
    }

    pub(crate) fn record_fetch(
        &self,
        duration: Duration,
        queue_depth: usize,
        queue_capacity: usize,
    ) {
        self.last_fetch_us
            .store(duration.as_micros() as u64, Ordering::Relaxed);
        self.fetch_queue_depth
            .store(queue_depth as u64, Ordering::Relaxed);
        self.fetch_queue_capacity
            .store(queue_capacity as u64, Ordering::Relaxed);
    }

    pub(crate) fn record_parse(
        &self,
        duration: Duration,
        queue_depth: usize,
        queue_capacity: usize,
    ) {
        self.last_parse_us
            .store(duration.as_micros() as u64, Ordering::Relaxed);
        self.parse_queue_depth
            .store(queue_depth as u64, Ordering::Relaxed);
        self.parse_queue_capacity
            .store(queue_capacity as u64, Ordering::Relaxed);
    }

    pub(crate) fn record_write(
        &self,
        duration: Duration,
        commit_ms: f64,
        writer_wait_ms: f64,
        queue_depth: usize,
        queue_capacity: usize,
    ) {
        self.last_write_us
            .store(duration.as_micros() as u64, Ordering::Relaxed);
        self.last_write_commit_us.store(
            duration_from_millis(commit_ms).as_micros() as u64,
            Ordering::Relaxed,
        );
        self.last_writer_wait_us.store(
            Duration::from_secs_f64((writer_wait_ms.max(0.0)) / 1000.0).as_micros() as u64,
            Ordering::Relaxed,
        );
        self.writer_queue_depth
            .store(queue_depth as u64, Ordering::Relaxed);
        self.writer_queue_capacity
            .store(queue_capacity as u64, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> Option<PipelineProgressData> {
        let fetch_us = self.last_fetch_us.load(Ordering::Relaxed);
        let parse_us = self.last_parse_us.load(Ordering::Relaxed);
        let write_us = self.last_write_us.load(Ordering::Relaxed);
        let write_commit_us = self.last_write_commit_us.load(Ordering::Relaxed);
        let wait_us = self.last_writer_wait_us.load(Ordering::Relaxed);
        let fetch_depth = self.fetch_queue_depth.load(Ordering::Relaxed);
        let fetch_capacity = self.fetch_queue_capacity.load(Ordering::Relaxed);
        let parse_depth = self.parse_queue_depth.load(Ordering::Relaxed);
        let parse_capacity = self.parse_queue_capacity.load(Ordering::Relaxed);
        let writer_depth = self.writer_queue_depth.load(Ordering::Relaxed);
        let writer_capacity = self.writer_queue_capacity.load(Ordering::Relaxed);

        if fetch_us == 0
            && parse_us == 0
            && write_us == 0
            && write_commit_us == 0
            && wait_us == 0
            && fetch_capacity == 0
            && parse_capacity == 0
            && writer_capacity == 0
        {
            return None;
        }

        Some(PipelineProgressData {
            fetch_ms: if fetch_us > 0 {
                Some(fetch_us as f64 / 1000.0)
            } else {
                None
            },
            parse_ms: if parse_us > 0 {
                Some(parse_us as f64 / 1000.0)
            } else {
                None
            },
            write_ms: if write_us > 0 {
                Some(write_us as f64 / 1000.0)
            } else {
                None
            },
            commit_ms: if write_commit_us > 0 {
                Some(write_commit_us as f64 / 1000.0)
            } else {
                None
            },
            writer_wait_ms: if wait_us > 0 {
                Some(wait_us as f64 / 1000.0)
            } else {
                None
            },
            fetch_queue_depth: Some(fetch_depth),
            fetch_queue_capacity: if fetch_capacity > 0 {
                Some(fetch_capacity)
            } else {
                None
            },
            parse_queue_depth: Some(parse_depth),
            parse_queue_capacity: if parse_capacity > 0 {
                Some(parse_capacity)
            } else {
                None
            },
            writer_queue_depth: Some(writer_depth),
            writer_queue_capacity: if writer_capacity > 0 {
                Some(writer_capacity)
            } else {
                None
            },
        })
    }
}

// ── BulkBuildPerfStats ──────────────────────────────────────────────────

/// Lock-free shared state for bulk-build engine metrics.
/// Written by the bulk-build batch loop, read by the progress monitor thread.
#[derive(Default)]
pub(crate) struct BulkBuildPerfStats {
    // Stage timings stored as microseconds (f64 ms * 1000 → u64 us)
    last_facts_us: AtomicU64,
    last_resolve_us: AtomicU64,
    last_reduce_us: AtomicU64,
    last_history_us: AtomicU64,
    last_address_reduce_us: AtomicU64,
    last_activity_stats_us: AtomicU64,
    last_flush_us: AtomicU64,
    last_fetch_us: AtomicU64,
    last_build_us: AtomicU64,
    last_flush_wait_us: AtomicU64,
    last_prefetch_recv_us: AtomicU64,
    // In-memory state. The owner total is published only from a batch that
    // walked the owner maps; on the others the snapshot reports it absent.
    owner_memory_bytes: AtomicU64,
    owner_memory_fresh: AtomicBool,
    live_cell_count: AtomicU64,
    // Per-batch volume
    cells_created: AtomicU64,
    cells_consumed: AtomicU64,
    // Cumulative materialization
    cumulative_history_rows: AtomicU64,
    cumulative_sealed_rows: AtomicU64,
    // Batch sizing
    batch_block_count: AtomicU64,
    batch_count: AtomicU64,
    // tx_density stored as f64 bits
    tx_density_bits: AtomicU64,
    // Finalize progress: 0 = not finalizing, 1-13 = step (1-indexed).
    finalize_step: AtomicU8,
    finalize_elapsed_us: AtomicU64,
    // Adaptive EMA controller state
    ms_per_block_ema_bits: AtomicU64,
    controllable_ms_us: AtomicU64,
    target_iteration_ms_us: AtomicU64,
    // Facts phase breakdown
    last_facts_par_iter_us: AtomicU64,
    last_facts_merge_us: AtomicU64,
    last_facts_serial_equivalent_us: AtomicU64,
    last_facts_intern_slow_path_count: AtomicU64,
    last_facts_intern_total_count: AtomicU64,
    last_facts_cell_count: AtomicU64,
    // Prefetch channel pipeline state
    prefetch_channel_pending: AtomicU64,
    prefetch_channel_capacity: AtomicU64,
    // Flush channel pipeline state
    flush_channel_pending: AtomicU64,
    flush_channel_capacity: AtomicU64,
    // Live disk telemetry from sampler
    last_disk_util_bits: AtomicU64,
    last_disk_await_bits: AtomicU64,
    last_disk_qd_bits: AtomicU64,
    last_disk_wr_mb_s_bits: AtomicU64,
    last_disk_wr_iops_bits: AtomicU64,
    last_disk_state_code: AtomicU8,
    last_disk_telemetry_valid: AtomicBool,
    // Bottleneck controller state
    controller_bottleneck_code: AtomicU8,
    controller_recv_ema_bits: AtomicU64,
    controller_build_ema_bits: AtomicU64,
    controller_wait_ema_bits: AtomicU64,
    controller_l0_ema_bits: AtomicU64,
    controller_fetch_threads: AtomicU64,
    controller_bg_jobs: AtomicU64,
    controller_target_cells: AtomicU64,
    batch_bytes: AtomicU64,
}

impl BulkBuildPerfStats {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_batch(
        &self,
        facts_ms: f64,
        resolve_ms: f64,
        reduce_ms: f64,
        history_ms: f64,
        address_reduce_ms: f64,
        activity_stats_ms: f64,
        flush_ms: f64,
        fetch_ms: f64,
        build_ms: f64,
        owner_memory_bytes: Option<u64>,
        live_cell_count: u64,
        cells_created: u64,
        cells_consumed: u64,
        cumulative_history_rows: u64,
        cumulative_sealed_rows: u64,
        batch_block_count: u64,
        batch_count: u64,
        tx_density: f64,
        ms_per_block_ema: f64,
        controllable_ms: f64,
        target_iteration_ms: f64,
        facts_par_iter_ms: f64,
        facts_merge_ms: f64,
        facts_serial_equivalent_ms: f64,
        facts_intern_slow_path_count: u64,
        facts_intern_total_count: u64,
        facts_cell_count: u64,
        flush_wait_ms: f64,
        prefetch_recv_ms: f64,
        prefetch_channel_pending: u64,
        prefetch_channel_capacity: u64,
        flush_channel_pending: u64,
        flush_channel_capacity: u64,
    ) {
        self.last_facts_us
            .store(ms_to_us(facts_ms), Ordering::Relaxed);
        self.last_resolve_us
            .store(ms_to_us(resolve_ms), Ordering::Relaxed);
        self.last_reduce_us
            .store(ms_to_us(reduce_ms), Ordering::Relaxed);
        self.last_history_us
            .store(ms_to_us(history_ms), Ordering::Relaxed);
        self.last_address_reduce_us
            .store(ms_to_us(address_reduce_ms), Ordering::Relaxed);
        self.last_activity_stats_us
            .store(ms_to_us(activity_stats_ms), Ordering::Relaxed);
        self.last_flush_us
            .store(ms_to_us(flush_ms), Ordering::Relaxed);
        self.last_fetch_us
            .store(ms_to_us(fetch_ms), Ordering::Relaxed);
        self.last_build_us
            .store(ms_to_us(build_ms), Ordering::Relaxed);
        if let Some(owner_memory_bytes) = owner_memory_bytes {
            self.owner_memory_bytes
                .store(owner_memory_bytes, Ordering::Relaxed);
        }
        self.owner_memory_fresh
            .store(owner_memory_bytes.is_some(), Ordering::Relaxed);
        self.live_cell_count
            .store(live_cell_count, Ordering::Relaxed);
        self.cells_created.store(cells_created, Ordering::Relaxed);
        self.cells_consumed.store(cells_consumed, Ordering::Relaxed);
        self.cumulative_history_rows
            .store(cumulative_history_rows, Ordering::Relaxed);
        self.cumulative_sealed_rows
            .store(cumulative_sealed_rows, Ordering::Relaxed);
        self.batch_block_count
            .store(batch_block_count, Ordering::Relaxed);
        self.batch_count.store(batch_count, Ordering::Relaxed);
        self.tx_density_bits
            .store(tx_density.to_bits(), Ordering::Relaxed);
        self.ms_per_block_ema_bits
            .store(ms_per_block_ema.to_bits(), Ordering::Relaxed);
        self.controllable_ms_us
            .store(ms_to_us(controllable_ms), Ordering::Relaxed);
        self.target_iteration_ms_us
            .store(ms_to_us(target_iteration_ms), Ordering::Relaxed);
        self.last_facts_par_iter_us
            .store(ms_to_us(facts_par_iter_ms), Ordering::Relaxed);
        self.last_facts_merge_us
            .store(ms_to_us(facts_merge_ms), Ordering::Relaxed);
        self.last_facts_serial_equivalent_us
            .store(ms_to_us(facts_serial_equivalent_ms), Ordering::Relaxed);
        self.last_facts_intern_slow_path_count
            .store(facts_intern_slow_path_count, Ordering::Relaxed);
        self.last_facts_intern_total_count
            .store(facts_intern_total_count, Ordering::Relaxed);
        self.last_facts_cell_count
            .store(facts_cell_count, Ordering::Relaxed);
        self.last_flush_wait_us
            .store(ms_to_us(flush_wait_ms), Ordering::Relaxed);
        self.last_prefetch_recv_us
            .store(ms_to_us(prefetch_recv_ms), Ordering::Relaxed);
        self.prefetch_channel_pending
            .store(prefetch_channel_pending, Ordering::Relaxed);
        self.prefetch_channel_capacity
            .store(prefetch_channel_capacity, Ordering::Relaxed);
        self.flush_channel_pending
            .store(flush_channel_pending, Ordering::Relaxed);
        self.flush_channel_capacity
            .store(flush_channel_capacity, Ordering::Relaxed);
    }

    pub(crate) fn record_disk_telemetry(
        &self,
        state: Option<&str>,
        util_pct: Option<f64>,
        await_ms: Option<f64>,
        avg_queue_depth: Option<f64>,
        write_mb_s: Option<f64>,
        write_iops: Option<f64>,
    ) {
        self.last_disk_util_bits
            .store(opt_f64_to_bits(util_pct), Ordering::Relaxed);
        self.last_disk_await_bits
            .store(opt_f64_to_bits(await_ms), Ordering::Relaxed);
        self.last_disk_qd_bits
            .store(opt_f64_to_bits(avg_queue_depth), Ordering::Relaxed);
        self.last_disk_wr_mb_s_bits
            .store(opt_f64_to_bits(write_mb_s), Ordering::Relaxed);
        self.last_disk_wr_iops_bits
            .store(opt_f64_to_bits(write_iops), Ordering::Relaxed);
        let state_code = encode_disk_state(state);
        self.last_disk_state_code
            .store(state_code, Ordering::Relaxed);
        self.last_disk_telemetry_valid
            .store(state.is_some(), Ordering::Relaxed);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_controller(
        &self,
        bottleneck_code: u8,
        recv_ema: f64,
        build_ema: f64,
        wait_ema: f64,
        l0_ema: f64,
        fetch_threads: u32,
        bg_jobs: i32,
        target_cells: u64,
    ) {
        self.controller_bottleneck_code
            .store(bottleneck_code, Ordering::Relaxed);
        self.controller_recv_ema_bits
            .store(recv_ema.to_bits(), Ordering::Relaxed);
        self.controller_build_ema_bits
            .store(build_ema.to_bits(), Ordering::Relaxed);
        self.controller_wait_ema_bits
            .store(wait_ema.to_bits(), Ordering::Relaxed);
        self.controller_l0_ema_bits
            .store(l0_ema.to_bits(), Ordering::Relaxed);
        self.controller_fetch_threads
            .store(fetch_threads as u64, Ordering::Relaxed);
        self.controller_bg_jobs
            .store(bg_jobs as u64, Ordering::Relaxed);
        self.controller_target_cells
            .store(target_cells, Ordering::Relaxed);
    }

    pub(crate) fn record_batch_bytes(&self, batch_bytes: u64) {
        self.batch_bytes.store(batch_bytes, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> Option<BulkBuildProgressData> {
        let batch_count = self.batch_count.load(Ordering::Relaxed);
        if batch_count == 0 {
            return None;
        }
        let finalize_step_raw = self.finalize_step.load(Ordering::Relaxed);
        let (finalize_phase, finalize_step, finalize_steps_total, finalize_elapsed_ms) =
            if finalize_step_raw > 0 {
                (
                    Some(finalize_step_label(finalize_step_raw)),
                    Some(finalize_step_raw - 1), // 0-indexed for TUI
                    Some(FINALIZE_TOTAL_STEPS),
                    Some(us_to_ms(self.finalize_elapsed_us.load(Ordering::Relaxed))),
                )
            } else {
                (None, None, None, None)
            };
        let (
            disk_state,
            disk_util_pct,
            disk_await_ms,
            disk_avg_queue_depth,
            disk_write_mb_s,
            disk_write_iops,
        ) = if self.last_disk_telemetry_valid.load(Ordering::Relaxed) {
            (
                decode_disk_state(self.last_disk_state_code.load(Ordering::Relaxed))
                    .map(str::to_string),
                opt_f64_from_bits(self.last_disk_util_bits.load(Ordering::Relaxed)),
                opt_f64_from_bits(self.last_disk_await_bits.load(Ordering::Relaxed)),
                opt_f64_from_bits(self.last_disk_qd_bits.load(Ordering::Relaxed)),
                opt_f64_from_bits(self.last_disk_wr_mb_s_bits.load(Ordering::Relaxed)),
                opt_f64_from_bits(self.last_disk_wr_iops_bits.load(Ordering::Relaxed)),
            )
        } else {
            (None, None, None, None, None, None)
        };
        Some(BulkBuildProgressData {
            facts_ms: Some(us_to_ms(self.last_facts_us.load(Ordering::Relaxed))),
            resolve_ms: Some(us_to_ms(self.last_resolve_us.load(Ordering::Relaxed))),
            reduce_ms: Some(us_to_ms(self.last_reduce_us.load(Ordering::Relaxed))),
            history_ms: Some(us_to_ms(self.last_history_us.load(Ordering::Relaxed))),
            address_reduce_ms: Some(us_to_ms(
                self.last_address_reduce_us.load(Ordering::Relaxed),
            )),
            activity_stats_ms: Some(us_to_ms(
                self.last_activity_stats_us.load(Ordering::Relaxed),
            )),
            flush_ms: Some(us_to_ms(self.last_flush_us.load(Ordering::Relaxed))),
            flush_wait_ms: Some(us_to_ms(self.last_flush_wait_us.load(Ordering::Relaxed))),
            prefetch_recv_ms: Some(us_to_ms(self.last_prefetch_recv_us.load(Ordering::Relaxed))),
            fetch_ms: Some(us_to_ms(self.last_fetch_us.load(Ordering::Relaxed))),
            build_ms: Some(us_to_ms(self.last_build_us.load(Ordering::Relaxed))),
            disk_state,
            disk_util_pct,
            disk_await_ms,
            disk_avg_queue_depth,
            disk_write_mb_s,
            disk_write_iops,
            owner_memory_bytes: self
                .owner_memory_fresh
                .load(Ordering::Relaxed)
                .then(|| self.owner_memory_bytes.load(Ordering::Relaxed)),
            live_cell_count: Some(self.live_cell_count.load(Ordering::Relaxed)),
            cells_created: Some(self.cells_created.load(Ordering::Relaxed)),
            cells_consumed: Some(self.cells_consumed.load(Ordering::Relaxed)),
            cumulative_history_rows: Some(self.cumulative_history_rows.load(Ordering::Relaxed)),
            cumulative_sealed_rows: Some(self.cumulative_sealed_rows.load(Ordering::Relaxed)),
            batch_block_count: Some(self.batch_block_count.load(Ordering::Relaxed)),
            batch_count: Some(batch_count),
            tx_density: Some(f64::from_bits(self.tx_density_bits.load(Ordering::Relaxed))),
            finalize_phase,
            finalize_step,
            finalize_steps_total,
            finalize_elapsed_ms,
            ms_per_block_ema: Some(f64::from_bits(
                self.ms_per_block_ema_bits.load(Ordering::Relaxed),
            )),
            controllable_ms: Some(us_to_ms(self.controllable_ms_us.load(Ordering::Relaxed))),
            target_iteration_ms: Some(us_to_ms(
                self.target_iteration_ms_us.load(Ordering::Relaxed),
            )),
            facts_par_iter_ms: Some(us_to_ms(
                self.last_facts_par_iter_us.load(Ordering::Relaxed),
            )),
            facts_merge_ms: Some(us_to_ms(self.last_facts_merge_us.load(Ordering::Relaxed))),
            facts_serial_equivalent_ms: Some(us_to_ms(
                self.last_facts_serial_equivalent_us.load(Ordering::Relaxed),
            )),
            facts_intern_slow_path_count: Some(
                self.last_facts_intern_slow_path_count
                    .load(Ordering::Relaxed),
            ),
            facts_intern_total_count: Some(
                self.last_facts_intern_total_count.load(Ordering::Relaxed),
            ),
            facts_cell_count: Some(self.last_facts_cell_count.load(Ordering::Relaxed)),
            prefetch_channel_pending: Some(self.prefetch_channel_pending.load(Ordering::Relaxed)),
            prefetch_channel_capacity: Some(self.prefetch_channel_capacity.load(Ordering::Relaxed)),
            flush_channel_pending: Some(self.flush_channel_pending.load(Ordering::Relaxed)),
            flush_channel_capacity: Some(self.flush_channel_capacity.load(Ordering::Relaxed)),
            controller_bottleneck: {
                let code = self.controller_bottleneck_code.load(Ordering::Relaxed);
                if code > 0 {
                    Some(code)
                } else {
                    None
                }
            },
            controller_recv_ema: Some(f64::from_bits(
                self.controller_recv_ema_bits.load(Ordering::Relaxed),
            )),
            controller_build_ema: Some(f64::from_bits(
                self.controller_build_ema_bits.load(Ordering::Relaxed),
            )),
            controller_wait_ema: Some(f64::from_bits(
                self.controller_wait_ema_bits.load(Ordering::Relaxed),
            )),
            controller_l0_ema: Some(f64::from_bits(
                self.controller_l0_ema_bits.load(Ordering::Relaxed),
            )),
            controller_fetch_threads: Some(
                self.controller_fetch_threads.load(Ordering::Relaxed) as u32
            ),
            controller_bg_jobs: Some(self.controller_bg_jobs.load(Ordering::Relaxed) as i32),
            target_cells: Some(self.controller_target_cells.load(Ordering::Relaxed))
                .filter(|&v| v > 0),
            batch_bytes: Some(self.batch_bytes.load(Ordering::Relaxed)).filter(|&v| v > 0),
        })
    }

    pub(crate) fn record_finalize_step(&self, step: u8, elapsed: std::time::Duration) {
        self.finalize_step.store(step, Ordering::Relaxed);
        self.finalize_elapsed_us.store(
            (elapsed.as_secs_f64() * 1_000_000.0) as u64,
            Ordering::Relaxed,
        );
    }

    pub(crate) fn clear_finalize(&self) {
        self.finalize_step.store(0, Ordering::Relaxed);
        self.finalize_elapsed_us.store(0, Ordering::Relaxed);
    }
}

const FINALIZE_TOTAL_STEPS: u8 = 13;

fn finalize_step_label(step: u8) -> String {
    match step {
        1 => "drain_flush",
        2 => "activity_stats",
        3 => "chain_stats",
        4 => "final_snapshot",
        5 => "owners_build",
        6 => "owners_write",
        7 => "unused",
        8 => "unused",
        9 => "unused",
        10 => "unused",
        11 => "metadata",
        12 => "memtable_flush",
        13 => "sync_status",
        _ => "unknown",
    }
    .to_string()
}

fn ms_to_us(ms: f64) -> u64 {
    (ms * 1000.0) as u64
}

fn us_to_ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

fn opt_f64_to_bits(value: Option<f64>) -> u64 {
    value.unwrap_or(f64::NAN).to_bits()
}

fn opt_f64_from_bits(bits: u64) -> Option<f64> {
    let value = f64::from_bits(bits);
    if value.is_nan() {
        None
    } else {
        Some(value)
    }
}

fn encode_disk_state(state: Option<&str>) -> u8 {
    match state {
        None => 0,
        Some("idle") => 1,
        Some("active") => 2,
        Some("saturated") => 3,
        Some("unavailable") => 4,
        Some(other) => panic!("unknown disk telemetry state: {other}"),
    }
}

fn decode_disk_state(code: u8) -> Option<&'static str> {
    match code {
        1 => Some("idle"),
        2 => Some("active"),
        3 => Some("saturated"),
        4 => Some("unavailable"),
        _ => None,
    }
}

// ── RepeatedWarningTracker ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub(crate) struct RepeatedWarningSnapshot {
    pub(crate) total_count: u64,
    pub(crate) suppressed_since_last_emit: u64,
    pub(crate) first_seen_secs_ago: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RepeatedWarningState {
    first_seen_at: Instant,
    last_emit_at: Instant,
    total_count: u64,
    suppressed_since_last_emit: u64,
}

#[derive(Default)]
pub(crate) struct RepeatedWarningTracker {
    states: std::sync::Mutex<HashMap<&'static str, RepeatedWarningState>>,
}

impl RepeatedWarningTracker {
    pub(crate) fn record(
        &self,
        key: &'static str,
        min_emit_interval: Duration,
    ) -> Option<RepeatedWarningSnapshot> {
        self.record_at(key, min_emit_interval, Instant::now())
    }

    /// `record` at a given instant, so callers' cadences can be tested
    /// without sleeping.
    pub(crate) fn record_at(
        &self,
        key: &'static str,
        min_emit_interval: Duration,
        now: Instant,
    ) -> Option<RepeatedWarningSnapshot> {
        let mut states = match self.states.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let entry = states.entry(key).or_insert(RepeatedWarningState {
            first_seen_at: now,
            last_emit_at: now,
            total_count: 0,
            suppressed_since_last_emit: 0,
        });
        entry.total_count = entry.total_count.saturating_add(1);

        if now.duration_since(entry.last_emit_at) >= min_emit_interval || entry.total_count == 1 {
            let snapshot = RepeatedWarningSnapshot {
                total_count: entry.total_count,
                suppressed_since_last_emit: entry.suppressed_since_last_emit,
                first_seen_secs_ago: now.duration_since(entry.first_seen_at).as_secs(),
            };
            entry.last_emit_at = now;
            entry.suppressed_since_last_emit = 0;
            Some(snapshot)
        } else {
            entry.suppressed_since_last_emit = entry.suppressed_since_last_emit.saturating_add(1);
            None
        }
    }
}

// ── Queue / memory helper functions ─────────────────────────────────────

pub(crate) fn queue_fill_percentage(depth: Option<u64>, capacity: Option<u64>) -> Option<f64> {
    match (depth, capacity) {
        (Some(d), Some(c)) if c > 0 => Some((d as f64 / c as f64) * 100.0),
        _ => None,
    }
}

pub(crate) fn should_trim_cell_cache(cache_len: usize) -> bool {
    cache_len > CELL_CACHE_CAPACITY * 2
}

pub(crate) fn evict_committed_cell_cache_entries(
    cell_cache: &DashMap<([u8; 32], i16), CachedCellInfo>,
    committed_tip: i64,
) -> usize {
    if committed_tip < 0 {
        return 0;
    }
    let before = cell_cache.len();
    cell_cache.retain(|_, v| v.created_at_block > committed_tip);
    before.saturating_sub(cell_cache.len())
}

// ── Pipeline idle / predicate helpers ───────────────────────────────────

pub(crate) fn should_abort_pipeline_on_idle_timeout(
    parser_finished: bool,
    fetcher_finished: bool,
) -> bool {
    parser_finished || fetcher_finished
}

pub(crate) fn should_invalidate_chart_caches_for_lag(blocks_remaining: u64) -> bool {
    blocks_remaining <= CHART_INVALIDATION_MAX_LIVE_LAG
}

/// Attempt cadence for unresolved inputs while the retry budget counts: the
/// first attempt and every 10th. The give-up attempt logs its own error. A
/// batch's attempts can outnumber its budgeted retries (frozen attempts count
/// as attempts), so nothing here is keyed on the budget limit.
pub(crate) fn should_log_unresolved_retry(attempt: usize) -> bool {
    attempt == 1 || attempt.is_multiple_of(10)
}

/// What the parser should do after one unresolved-input attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnresolvedRetryDecision {
    Retry,
    GiveUp,
}

/// Why a parser batch still has inputs that neither the batch, the parser
/// cache nor the store can resolve, judged against the store's committed tip
/// exactly as the writer judges a batch it receives (position, then parent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnresolvedInputCause {
    /// The store's tip is below the block before this batch: an earlier batch
    /// is still on its way through the writer.
    WriterBehind,
    /// The batch is the next one the writer expects, but its first block is
    /// not the stored tip's child: the node has replaced the tip. Inputs
    /// created only by the replacement block can never come from writer
    /// progress; only the writer's reorg handling can resolve them. Carries
    /// the stored tip's hash, which this classification requires to exist.
    OrphanedTip { stored_tip_hash: Vec<u8> },
    /// The batch extends the stored tip and nothing is in flight, yet inputs
    /// are missing: a real hole, which the retry budget exists to catch.
    MissingInputs,
    /// The store already holds this batch's first block: the batch is stale
    /// and the pipeline reset that discards it is pending.
    WriterPastBatch,
}

impl UnresolvedInputCause {
    /// Stable name for log fields.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            UnresolvedInputCause::WriterBehind => "writer_behind",
            UnresolvedInputCause::OrphanedTip { .. } => "orphaned_tip",
            UnresolvedInputCause::MissingInputs => "missing_inputs",
            UnresolvedInputCause::WriterPastBatch => "writer_past_batch",
        }
    }

    /// What the parser is waiting for while it retries under this cause.
    pub(crate) fn waiting_for(&self) -> &'static str {
        match self {
            UnresolvedInputCause::WriterBehind => {
                "the writer to commit the batches before this one"
            }
            UnresolvedInputCause::OrphanedTip { .. } => {
                "the writer's reorg check of the orphaned store tip"
            }
            UnresolvedInputCause::MissingInputs => {
                "nothing in flight: the inputs are missing from the store the batch extends"
            }
            UnresolvedInputCause::WriterPastBatch => {
                "a pipeline reset: the writer already holds this batch's first block"
            }
        }
    }
}

/// Classify one failed resolution attempt of the batch starting at
/// `start_block`, whose first block's parent is `first_parent_hash`, against
/// the store tip `(db_tip, db_tip_hash)` read through the writer's own source
/// (`Repository::get_sync_tip`).
pub(crate) fn classify_unresolved_inputs(
    db_tip: i64,
    db_tip_hash: &Option<Vec<u8>>,
    start_block: u64,
    first_parent_hash: &[u8],
) -> anyhow::Result<UnresolvedInputCause> {
    let expected_start = next_start_block_from_db_tip(
        db_tip,
        db_tip_hash,
        "parser unresolved-input classification",
    )?;
    Ok(match start_block.cmp(&expected_start) {
        std::cmp::Ordering::Greater => UnresolvedInputCause::WriterBehind,
        std::cmp::Ordering::Less => UnresolvedInputCause::WriterPastBatch,
        std::cmp::Ordering::Equal => match db_tip_hash {
            Some(tip_hash) if tip_hash.as_slice() != first_parent_hash => {
                UnresolvedInputCause::OrphanedTip {
                    stored_tip_hash: tip_hash.clone(),
                }
            }
            _ => UnresolvedInputCause::MissingInputs,
        },
    })
}

/// Advance the parser's unresolved-input retry budget by one attempt.
///
/// The budget exists to detect inputs that will never resolve. A reorg
/// rollback republishes the very cells the parser may be waiting on, in one
/// atomic write of the block journal (bounded by the reorg depth), so it does
/// not outlast the budget.
///
/// One cause makes "unresolved" expected and indefinite, and attempts made
/// under it carry no evidence either way, so they must not consume the
/// budget: an orphaned tip waits for the writer's reorg check, which the
/// parser has requested. Counting those attempts was the v0.8.2 crash loop:
/// the parser spent its budget on inputs only the replacement block creates,
/// and the writer, which never received a batch, never compared the tip with
/// the node.
pub(crate) fn advance_unresolved_retry_budget(
    retry_count: &mut usize,
    cause: &UnresolvedInputCause,
) -> UnresolvedRetryDecision {
    match cause {
        UnresolvedInputCause::OrphanedTip { .. } => {
            return UnresolvedRetryDecision::Retry;
        }
        UnresolvedInputCause::WriterBehind
        | UnresolvedInputCause::MissingInputs
        | UnresolvedInputCause::WriterPastBatch => {}
    }
    *retry_count += 1;
    if *retry_count >= PARSER_UNRESOLVED_MAX_RETRIES {
        UnresolvedRetryDecision::GiveUp
    } else {
        UnresolvedRetryDecision::Retry
    }
}

/// How long the parser stays quiet between warnings about one frozen-budget
/// wait: as long as the writer's own deep-fork pause warning.
pub(crate) const PARSER_FROZEN_BUDGET_WARN_INTERVAL: Duration = Duration::from_secs(120);

/// How the parser reports one failed resolution attempt.
#[derive(Debug, Clone, Copy)]
pub(crate) enum UnresolvedRetryLog {
    /// The budget counts: warn on the attempt cadence. Only a real hole is
    /// worth asking the node about; inputs that an earlier in-flight batch or
    /// a pending reset will settle are not.
    Counting { rpc_probe: bool },
    /// The budget is frozen: warn at most once per
    /// `PARSER_FROZEN_BUDGET_WARN_INTERVAL` and never ask the node. The
    /// missing outputs are in a block the store has not indexed, which the
    /// node cannot explain, and the wait lasts as long as the writer needs —
    /// indefinitely when a deep fork pauses sync.
    Frozen(RepeatedWarningSnapshot),
}

/// Decide whether, and how, attempt number `attempt` under `cause` is logged.
/// `frozen_waits` throttles the frozen cause by time; it lives as long as
/// the parser task.
pub(crate) fn plan_unresolved_retry_log(
    cause: &UnresolvedInputCause,
    attempt: usize,
    frozen_waits: &RepeatedWarningTracker,
    now: Instant,
) -> Option<UnresolvedRetryLog> {
    match cause {
        UnresolvedInputCause::OrphanedTip { .. } => frozen_waits
            .record_at(cause.name(), PARSER_FROZEN_BUDGET_WARN_INTERVAL, now)
            .map(UnresolvedRetryLog::Frozen),
        UnresolvedInputCause::MissingInputs => should_log_unresolved_retry(attempt)
            .then_some(UnresolvedRetryLog::Counting { rpc_probe: true }),
        UnresolvedInputCause::WriterBehind | UnresolvedInputCause::WriterPastBatch => {
            should_log_unresolved_retry(attempt)
                .then_some(UnresolvedRetryLog::Counting { rpc_probe: false })
        }
    }
}

pub(crate) fn should_log_pipeline_idle_timeout(consecutive_idle_timeouts: u64) -> bool {
    consecutive_idle_timeouts <= 3 || consecutive_idle_timeouts.is_multiple_of(10)
}

// ── Fetcher start position ──────────────────────────────────────────────

/// Where the fetcher continues after a batch it sent: the block after the
/// batch, and the pipeline epoch the batch was tagged with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FetchCursor {
    pub(crate) epoch: u64,
    pub(crate) next_block: u64,
}

/// Where one fetch cycle starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FetchStart {
    /// At this block, right after the previous batch of the same epoch.
    Continue(u64),
    /// Right after the store's tip, read anew: there is no cursor (the first
    /// cycle, a failed fetch, a resume from a rebuild pause), or the cursor
    /// belongs to an earlier epoch, so a pipeline reset happened since it was
    /// computed and the position it holds is the one the reset discarded.
    FromDbTip,
}

/// Decide where the fetch cycle that loaded `cycle_epoch` starts.
///
/// The fetcher loads the epoch once per cycle and tags the cycle's batch with
/// that same value, so the start position and the batch's epoch always come
/// from one load. A reset that lands after the load leaves the batch with the
/// old epoch, which the parser and the writer drop, and the next cycle finds
/// its cursor from the old epoch and starts from the store's tip. A cursor
/// from a later epoch than the cycle's means the epoch went backwards, which
/// no reset does: an error.
pub(crate) fn plan_fetch_start(
    cycle_epoch: u64,
    cursor: Option<FetchCursor>,
) -> anyhow::Result<FetchStart> {
    match cursor {
        None => Ok(FetchStart::FromDbTip),
        Some(cursor) if cursor.epoch == cycle_epoch => Ok(FetchStart::Continue(cursor.next_block)),
        Some(cursor) if cursor.epoch < cycle_epoch => Ok(FetchStart::FromDbTip),
        Some(cursor) => anyhow::bail!(
            "pipeline epoch went backwards: the fetcher's cursor (next_block={}) was computed \
             under epoch {}, but this fetch cycle loaded epoch {}",
            cursor.next_block,
            cursor.epoch,
            cycle_epoch
        ),
    }
}

// ── Parser → writer reorg-check request ─────────────────────────────────

/// What a parser batch stuck on an orphaned tip tells the writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrphanedTipReport {
    /// The pipeline epoch the batch belongs to.
    pub(crate) batch_epoch: u64,
    pub(crate) start_block: u64,
    /// The parent hash of the batch's first block.
    pub(crate) first_parent_hash: Vec<u8>,
}

/// What the writer finds in the reorg-check request slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReorgCheckRequest {
    Empty,
    /// Made for a batch of the current pipeline epoch: run the check.
    Current(OrphanedTipReport),
    /// Made for a batch of an earlier epoch. The reset that ended that epoch
    /// discarded the batch, so the request means nothing any more.
    Stale(OrphanedTipReport),
}

/// The parser's request for a writer reorg check: at most one report, the
/// latest. The report carries its batch's epoch, so a request can never act
/// in an epoch other than its own.
#[derive(Debug, Default)]
pub(crate) struct ReorgCheckRequestSlot(std::sync::Mutex<Option<OrphanedTipReport>>);

impl ReorgCheckRequestSlot {
    /// Parser side. The parser repeats the request on every attempt; each
    /// repeat replaces the pending report with an identical one.
    pub(crate) fn request(&self, report: OrphanedTipReport) {
        *self.lock() = Some(report);
    }

    /// Writer side: take the pending request, emptying the slot.
    pub(crate) fn take(&self, current_epoch: u64) -> ReorgCheckRequest {
        match self.lock().take() {
            None => ReorgCheckRequest::Empty,
            Some(report) if report.batch_epoch == current_epoch => {
                ReorgCheckRequest::Current(report)
            }
            Some(report) => ReorgCheckRequest::Stale(report),
        }
    }

    pub(crate) fn clear(&self) {
        *self.lock() = None;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<OrphanedTipReport>> {
        // Every critical section is a single `Option` store or take, which
        // cannot panic, so a poisoned lock still guards a whole value.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

// ── Writer reorg checks that find no divergence ─────────────────────────

/// How many reorg checks in a row may find the node's block at `db_tip`
/// equal to the stored one while a batch at `db_tip + 1` names another
/// parent. One is a race that the following pipeline reset settles: the node
/// switched forks between the fetch and the check. Ten in a row on the same
/// tip mean the node's block at `db_tip + 1` does not extend its own block at
/// `db_tip`, or the batch's parent hash is read wrongly: a broken invariant
/// that no number of resets can repair.
pub(crate) const MAX_CONSECUTIVE_NO_DIVERGENCE: u32 = 10;

/// The batch behind one no-divergence result.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NoDivergenceEvidence<'a> {
    /// Which writer check saw the parent mismatch.
    pub(crate) source: &'static str,
    pub(crate) pipeline_epoch: u64,
    pub(crate) start_block: u64,
    pub(crate) first_parent_hash: &'a [u8],
}

/// Consecutive no-divergence results for one store tip. The stale-fork
/// branch and the parser-requested check share one streak.
#[derive(Debug, Default)]
pub(crate) struct NoDivergenceStreak {
    tip: Option<(i64, Vec<u8>)>,
    count: u32,
}

impl NoDivergenceStreak {
    /// Record one check that found the node's block at `db_tip` to be
    /// `stored_hash`. Returns the streak length for this tip, or the error
    /// that stops the pipeline once it reaches `MAX_CONSECUTIVE_NO_DIVERGENCE`.
    pub(crate) fn record(
        &mut self,
        db_tip: i64,
        stored_hash: &[u8],
        evidence: &NoDivergenceEvidence<'_>,
    ) -> anyhow::Result<u32> {
        let same_tip = matches!(
            &self.tip,
            Some((tip, hash)) if *tip == db_tip && hash.as_slice() == stored_hash
        );
        if same_tip {
            self.count += 1;
        } else {
            self.tip = Some((db_tip, stored_hash.to_vec()));
            self.count = 1;
        }
        if self.count >= MAX_CONSECUTIVE_NO_DIVERGENCE {
            anyhow::bail!(
                "pipeline stopped: {count} consecutive reorg checks found no divergence for the \
                 same store tip (db_tip={db_tip}, stored_hash=0x{stored_hash}), while the \
                 {source} batch starting at block {start_block} names parent 0x{parent} \
                 (pipeline_epoch={epoch}). A consistent node cannot keep serving a block at \
                 db_tip + 1 that does not extend its own block at db_tip: inspect the node's \
                 chain store and the block parser's parent-hash extraction before restarting",
                count = self.count,
                stored_hash = hex::encode(stored_hash),
                source = evidence.source,
                start_block = evidence.start_block,
                parent = hex::encode(evidence.first_parent_hash),
                epoch = evidence.pipeline_epoch,
            );
        }
        Ok(self.count)
    }

    /// A divergence was handled or a batch was written: the streak is over.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

// ── Worker exit reason helpers ──────────────────────────────────────────

pub(crate) fn record_worker_exit_reason(
    slot: &Arc<std::sync::Mutex<Option<String>>>,
    reason: impl Into<String>,
) {
    if let Ok(mut guard) = slot.lock() {
        if guard.is_none() {
            *guard = Some(reason.into());
        }
    }
}

pub(crate) fn get_worker_exit_reason(
    slot: &Arc<std::sync::Mutex<Option<String>>>,
) -> Option<String> {
    slot.lock().ok().and_then(|guard| guard.clone())
}

pub(crate) fn format_pipeline_worker_termination_message(
    parser_finished: bool,
    fetcher_finished: bool,
    parser_exit_reason: Option<&str>,
    fetcher_exit_reason: Option<&str>,
) -> String {
    let parser_reason = parser_exit_reason.unwrap_or("unknown");
    let fetcher_reason = fetcher_exit_reason.unwrap_or("unknown");
    format!(
        "parser_finished={}, fetcher_finished={}, parser_reason={}, fetcher_reason={}",
        parser_finished, fetcher_finished, parser_reason, fetcher_reason
    )
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::Ordering;

    #[test]
    fn test_perf_snapshot_uses_last_batch_after_reset() {
        let perf = PerfStats::default();
        perf.add_fetch(Duration::from_millis(120));
        perf.add_db_write(Duration::from_millis(340));
        perf.add_db_commit(Duration::from_millis(210));
        perf.blocks_count.fetch_add(10, Ordering::Relaxed);
        perf.report_and_reset();

        let (rpc_ms, db_stage_ms, db_commit_ms) = perf.snapshot_ms();
        assert!((rpc_ms - 120.0).abs() < 0.001);
        assert!((db_stage_ms - 340.0).abs() < 0.001);
        assert!((db_commit_ms - 210.0).abs() < 0.001);
    }

    #[test]
    fn test_perf_snapshot_prefers_current_accumulator_over_last_batch() {
        let perf = PerfStats::default();

        perf.add_fetch(Duration::from_millis(500));
        perf.add_db_write(Duration::from_millis(700));
        perf.add_db_commit(Duration::from_millis(420));
        perf.blocks_count.fetch_add(10, Ordering::Relaxed);
        perf.report_and_reset();

        perf.add_fetch(Duration::from_millis(150));
        perf.add_db_write(Duration::from_millis(250));
        perf.add_db_commit(Duration::from_millis(90));

        let (rpc_ms, db_stage_ms, db_commit_ms) = perf.snapshot_ms();
        assert!((rpc_ms - 150.0).abs() < 0.001);
        assert!((db_stage_ms - 250.0).abs() < 0.001);
        assert!((db_commit_ms - 90.0).abs() < 0.001);
    }

    #[test]
    fn test_pipeline_perf_snapshot_returns_none_when_empty() {
        let perf = PipelinePerfStats::default();
        assert!(perf.snapshot().is_none());
    }

    #[test]
    fn test_pipeline_perf_snapshot_contains_stage_metrics() {
        let perf = PipelinePerfStats::default();
        perf.set_queue_capacities(16, 16);
        perf.record_fetch(Duration::from_millis(20), 3, 16);
        perf.record_parse(Duration::from_millis(40), 7, 16);
        perf.record_write(Duration::from_millis(80), 33.0, 12.0, 6, 16);

        let snapshot = perf.snapshot().expect("pipeline snapshot should exist");
        assert_eq!(snapshot.fetch_ms, Some(20.0));
        assert_eq!(snapshot.parse_ms, Some(40.0));
        assert_eq!(snapshot.write_ms, Some(80.0));
        assert_eq!(snapshot.commit_ms, Some(33.0));
        let wait = snapshot
            .writer_wait_ms
            .expect("writer wait should be present");
        assert!((wait - 12.0).abs() < 0.001);
        assert_eq!(snapshot.fetch_queue_depth, Some(3));
        assert_eq!(snapshot.parse_queue_depth, Some(7));
        assert_eq!(snapshot.parse_queue_capacity, Some(16));
        assert_eq!(snapshot.writer_queue_depth, Some(6));
        assert_eq!(snapshot.writer_queue_capacity, Some(16));
    }

    #[test]
    fn test_queue_fill_percentage() {
        assert_eq!(queue_fill_percentage(Some(5), Some(10)), Some(50.0));
        assert_eq!(queue_fill_percentage(Some(1), Some(0)), None);
        assert_eq!(queue_fill_percentage(None, Some(10)), None);
        assert_eq!(queue_fill_percentage(Some(1), None), None);
    }

    #[test]
    fn test_should_trim_cell_cache_threshold() {
        assert!(!should_trim_cell_cache(CELL_CACHE_CAPACITY * 2));
        assert!(should_trim_cell_cache(CELL_CACHE_CAPACITY * 2 + 1));
    }

    #[test]
    fn test_evict_committed_cell_cache_entries_only_removes_committed() {
        use super::super::types::CachedCellInfo;

        fn dummy_cached_cell_info(created_at_block: i64) -> CachedCellInfo {
            CachedCellInfo::new(
                ckbadger_store::LiveCellInfo {
                    capacity: 1,
                    lock_code_hash: [2u8; 32],
                    lock_hash_type: 1,
                    lock_args: vec![],
                    type_code_hash: None,
                    type_hash_type: None,
                    type_args: None,
                    data_size: 0,
                    udt_amount: None,
                    data_hash: None,
                },
                created_at_block,
            )
        }

        let cache = dashmap::DashMap::new();
        cache.insert(([0x11; 32], 0), dummy_cached_cell_info(100));
        cache.insert(([0x22; 32], 1), dummy_cached_cell_info(101));
        cache.insert(([0x33; 32], 2), dummy_cached_cell_info(102));

        let evicted = evict_committed_cell_cache_entries(&cache, 101);
        assert_eq!(evicted, 2);
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key(&([0x33; 32], 2)));

        let evicted_noop = evict_committed_cell_cache_entries(&cache, -1);
        assert_eq!(evicted_noop, 0);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_should_abort_pipeline_on_idle_timeout_when_parser_exits() {
        assert!(should_abort_pipeline_on_idle_timeout(true, false));
    }

    #[test]
    fn test_should_abort_pipeline_on_idle_timeout_when_fetcher_exits() {
        assert!(should_abort_pipeline_on_idle_timeout(false, true));
        assert!(!should_abort_pipeline_on_idle_timeout(false, false));
    }

    #[test]
    fn test_should_invalidate_chart_caches_for_lag_only_near_tip() {
        assert!(should_invalidate_chart_caches_for_lag(0));
        assert!(should_invalidate_chart_caches_for_lag(
            CHART_INVALIDATION_MAX_LIVE_LAG
        ));
        assert!(!should_invalidate_chart_caches_for_lag(
            CHART_INVALIDATION_MAX_LIVE_LAG + 1
        ));
    }

    #[test]
    fn test_should_log_unresolved_retry_policy() {
        assert!(should_log_unresolved_retry(1));
        assert!(!should_log_unresolved_retry(2));
        assert!(should_log_unresolved_retry(10));
        assert!(should_log_unresolved_retry(PARSER_UNRESOLVED_MAX_RETRIES));
        // Past the budget limit (a frozen budget) the cadence stays 1-in-10.
        assert!(!should_log_unresolved_retry(
            PARSER_UNRESOLVED_MAX_RETRIES + 1
        ));
        assert!(should_log_unresolved_retry(
            PARSER_UNRESOLVED_MAX_RETRIES * 10
        ));
    }

    #[test]
    fn test_unresolved_retry_budget_gives_up_only_after_max_attempts() {
        let mut retry_count = 0usize;
        for _ in 0..(PARSER_UNRESOLVED_MAX_RETRIES - 1) {
            assert_eq!(
                advance_unresolved_retry_budget(
                    &mut retry_count,
                    &UnresolvedInputCause::MissingInputs
                ),
                UnresolvedRetryDecision::Retry
            );
        }
        assert_eq!(
            advance_unresolved_retry_budget(&mut retry_count, &UnresolvedInputCause::MissingInputs),
            UnresolvedRetryDecision::GiveUp
        );
        assert_eq!(retry_count, PARSER_UNRESOLVED_MAX_RETRIES);
    }

    /// Regression test for the v0.8.2 testnet crash loop: the store's tip
    /// was orphaned, the parser spent its whole budget on inputs that only
    /// the replacement block creates, and the pipeline died before the writer
    /// ever compared the tip with the node. While the parser waits for the
    /// writer's reorg check, attempts must not consume the budget.
    #[test]
    fn test_unresolved_retry_budget_frozen_while_orphaned_tip_awaits_writer_reorg() {
        assert_budget_frozen_then_fully_available(orphaned_tip());
    }

    fn assert_budget_frozen_then_fully_available(frozen: UnresolvedInputCause) {
        let mut retry_count = 0usize;
        for _ in 0..(PARSER_UNRESOLVED_MAX_RETRIES * 10) {
            assert_eq!(
                advance_unresolved_retry_budget(&mut retry_count, &frozen),
                UnresolvedRetryDecision::Retry
            );
        }
        assert_eq!(
            retry_count, 0,
            "{frozen:?} attempts must not consume budget"
        );

        // Once the writer has acted, the full budget is still available.
        for _ in 0..(PARSER_UNRESOLVED_MAX_RETRIES - 1) {
            assert_eq!(
                advance_unresolved_retry_budget(
                    &mut retry_count,
                    &UnresolvedInputCause::MissingInputs
                ),
                UnresolvedRetryDecision::Retry
            );
        }
        assert_eq!(
            advance_unresolved_retry_budget(&mut retry_count, &UnresolvedInputCause::MissingInputs),
            UnresolvedRetryDecision::GiveUp
        );
    }

    /// Unchanged behaviour: waiting on an earlier in-flight batch, and a batch
    /// the writer has already passed (a reset is pending), both still count.
    #[test]
    fn test_unresolved_retry_budget_counts_writer_behind_and_writer_past_batch() {
        for cause in [
            UnresolvedInputCause::WriterBehind,
            UnresolvedInputCause::WriterPastBatch,
            UnresolvedInputCause::MissingInputs,
        ] {
            let mut retry_count = 0usize;
            assert_eq!(
                advance_unresolved_retry_budget(&mut retry_count, &cause),
                UnresolvedRetryDecision::Retry
            );
            assert_eq!(retry_count, 1, "{} consumes budget", cause.name());
        }
    }

    const STORED_TIP_HASH: [u8; 32] = [0x11; 32];
    const OTHER_HASH: [u8; 32] = [0x22; 32];

    fn orphaned_tip() -> UnresolvedInputCause {
        UnresolvedInputCause::OrphanedTip {
            stored_tip_hash: STORED_TIP_HASH.to_vec(),
        }
    }

    fn classify(
        db_tip: i64,
        start_block: u64,
        first_parent_hash: [u8; 32],
    ) -> UnresolvedInputCause {
        classify_unresolved_inputs(
            db_tip,
            &Some(STORED_TIP_HASH.to_vec()),
            start_block,
            &first_parent_hash,
        )
        .expect("non-negative tip classifies")
    }

    #[test]
    fn test_classify_orphaned_tip_when_the_next_batch_is_not_the_tips_child() {
        assert_eq!(classify(100, 101, OTHER_HASH), orphaned_tip());
    }

    #[test]
    fn test_classify_missing_inputs_when_the_next_batch_extends_the_tip() {
        assert_eq!(
            classify(100, 101, STORED_TIP_HASH),
            UnresolvedInputCause::MissingInputs
        );
    }

    /// Only the batch right after the tip can be judged against the tip's
    /// hash; any later batch waits for the earlier ones, whatever its parent.
    #[test]
    fn test_classify_writer_behind_when_an_earlier_batch_is_still_in_flight() {
        for (start_block, parent) in [
            (102, OTHER_HASH),
            (102, STORED_TIP_HASH),
            (5_000, OTHER_HASH),
        ] {
            assert_eq!(
                classify(100, start_block, parent),
                UnresolvedInputCause::WriterBehind,
                "start_block={start_block}"
            );
        }
    }

    #[test]
    fn test_classify_writer_past_batch_when_the_store_already_holds_its_start() {
        for (db_tip, parent) in [(101, OTHER_HASH), (101, STORED_TIP_HASH), (250, OTHER_HASH)] {
            assert_eq!(
                classify(db_tip, 101, parent),
                UnresolvedInputCause::WriterPastBatch,
                "db_tip={db_tip}"
            );
        }
    }

    /// An empty store has no tip hash to compare: the genesis batch's missing
    /// inputs are a real hole, any later batch waits for genesis.
    #[test]
    fn test_classify_empty_store() {
        assert_eq!(
            classify_unresolved_inputs(0, &None, 0, &OTHER_HASH).unwrap(),
            UnresolvedInputCause::MissingInputs
        );
        assert_eq!(
            classify_unresolved_inputs(0, &None, 1, &OTHER_HASH).unwrap(),
            UnresolvedInputCause::WriterBehind
        );
    }

    #[test]
    fn test_classify_rejects_a_negative_store_tip() {
        let err = classify_unresolved_inputs(-1, &Some(STORED_TIP_HASH.to_vec()), 0, &OTHER_HASH)
            .unwrap_err();
        assert!(err.to_string().contains("negative block number"), "{err}");
    }

    /// A frozen wait warns once, then at most once per interval however many
    /// attempts it makes, and never asks the node.
    #[test]
    fn test_frozen_budget_warns_at_most_once_per_interval_without_node_probe() {
        let cause = orphaned_tip();
        let waits = RepeatedWarningTracker::default();
        let start = Instant::now();
        let at = |secs: u64| start + Duration::from_secs(secs);
        let interval = PARSER_FROZEN_BUDGET_WARN_INTERVAL.as_secs();

        // `Frozen` carries no probe: a frozen wait never asks the node.
        assert!(
            matches!(
                plan_unresolved_retry_log(&cause, 1, &waits, at(0)),
                Some(UnresolvedRetryLog::Frozen(_))
            ),
            "{}: the first frozen attempt warns",
            cause.name()
        );
        // One attempt every 500 ms until just before the interval ends,
        // including attempt numbers the counting cadence would log.
        for (attempt, half_secs) in (2..).zip(1..(interval * 2)) {
            let now = start + Duration::from_millis(half_secs * 500);
            assert!(
                plan_unresolved_retry_log(&cause, attempt, &waits, now).is_none(),
                "{}: attempt {attempt} inside the interval stays quiet",
                cause.name()
            );
        }
        match plan_unresolved_retry_log(&cause, 10_000, &waits, at(interval)) {
            Some(UnresolvedRetryLog::Frozen(repeat)) => {
                assert_eq!(repeat.suppressed_since_last_emit, interval * 2 - 1);
                assert_eq!(repeat.first_seen_secs_ago, interval);
            }
            other => panic!(
                "{}: expected the next frozen warn, got {other:?}",
                cause.name()
            ),
        }
        assert!(plan_unresolved_retry_log(&cause, 10_001, &waits, at(interval + 1)).is_none());
    }

    /// Counting causes keep the attempt cadence; only a real hole asks the node.
    #[test]
    fn test_counting_budget_warns_on_attempt_cadence_and_probes_only_missing_inputs() {
        let waits = RepeatedWarningTracker::default();
        let now = Instant::now();
        for (cause, rpc_probe) in [
            (UnresolvedInputCause::MissingInputs, true),
            (UnresolvedInputCause::WriterBehind, false),
            (UnresolvedInputCause::WriterPastBatch, false),
        ] {
            for attempt in [1, 10, 240, 2_400] {
                match plan_unresolved_retry_log(&cause, attempt, &waits, now) {
                    Some(UnresolvedRetryLog::Counting { rpc_probe: probe }) => {
                        assert_eq!(probe, rpc_probe, "{} attempt {attempt}", cause.name());
                    }
                    other => panic!("{} attempt {attempt}: {other:?}", cause.name()),
                }
            }
            for attempt in [2, 9, 241] {
                assert!(plan_unresolved_retry_log(&cause, attempt, &waits, now).is_none());
            }
        }
    }

    fn report(batch_epoch: u64) -> OrphanedTipReport {
        OrphanedTipReport {
            batch_epoch,
            start_block: 102,
            first_parent_hash: OTHER_HASH.to_vec(),
        }
    }

    #[test]
    fn test_take_reorg_check_request_when_none_was_made() {
        let slot = ReorgCheckRequestSlot::default();
        assert_eq!(slot.take(0), ReorgCheckRequest::Empty);
    }

    #[test]
    fn test_reorg_check_request_is_served_once_in_its_own_epoch() {
        let slot = ReorgCheckRequestSlot::default();
        slot.request(report(3));
        // The parser repeats the request every attempt; it stays one request.
        slot.request(report(3));
        assert_eq!(slot.take(3), ReorgCheckRequest::Current(report(3)));
        assert_eq!(slot.take(3), ReorgCheckRequest::Empty);
    }

    /// A pipeline reset discards the batch that asked. Its request must not
    /// reach the next epoch, even when the parser stores it after the reset.
    #[test]
    fn test_reorg_check_request_from_an_earlier_epoch_is_stale() {
        let slot = ReorgCheckRequestSlot::default();
        slot.request(report(3));
        assert_eq!(slot.take(4), ReorgCheckRequest::Stale(report(3)));
        assert_eq!(slot.take(4), ReorgCheckRequest::Empty);
    }

    /// `run_pipeline` clears the slot when it starts.
    #[test]
    fn test_reorg_check_request_clear_drops_a_pending_request() {
        let slot = ReorgCheckRequestSlot::default();
        slot.request(report(0));
        slot.clear();
        assert_eq!(slot.take(0), ReorgCheckRequest::Empty);
    }

    fn cursor(epoch: u64, next_block: u64) -> Option<FetchCursor> {
        Some(FetchCursor { epoch, next_block })
    }

    #[test]
    fn test_fetch_start_continues_after_the_previous_batch_of_the_same_epoch() {
        assert_eq!(
            plan_fetch_start(5, cursor(5, 103)).unwrap(),
            FetchStart::Continue(103)
        );
    }

    /// The fetcher epoch race (review finding 2): the reset flag was taken at
    /// the top of a cycle and the epoch loaded later, after the node refresh
    /// and the tip read. A reset landing in between sent a batch from the
    /// stale position 103 tagged with the new epoch 6, which the parser
    /// accepted. A cursor computed under epoch 5 must never start a cycle of
    /// epoch 6, however many resets came between.
    #[test]
    fn test_fetch_start_after_a_reset_reads_the_db_tip_instead_of_the_stale_position() {
        for cycle_epoch in [6, 7, 1_000] {
            assert_eq!(
                plan_fetch_start(cycle_epoch, cursor(5, 103)).unwrap(),
                FetchStart::FromDbTip,
                "cursor of epoch 5, cycle of epoch {cycle_epoch}"
            );
        }
    }

    /// No cursor: the first cycle of a run, after a failed fetch, or after a
    /// resume from a rebuild pause.
    #[test]
    fn test_fetch_start_without_a_cursor_reads_the_db_tip() {
        for cycle_epoch in [0, 6] {
            assert_eq!(
                plan_fetch_start(cycle_epoch, None).unwrap(),
                FetchStart::FromDbTip
            );
        }
    }

    /// A reset after the cycle's load: the batch goes out under the epoch the
    /// cycle loaded (5), which the parser drops once the epoch is 6, and the
    /// cursor it leaves (epoch 5) makes the next cycle start from the DB tip.
    #[test]
    fn test_fetch_start_after_a_reset_that_lands_after_the_cycle_loaded_its_epoch() {
        let cycle_epoch = 5;
        assert_eq!(
            plan_fetch_start(cycle_epoch, cursor(5, 103)).unwrap(),
            FetchStart::Continue(103)
        );
        let after_batch = cursor(cycle_epoch, 111);
        // The reset bumps the epoch to 6 while the batch is in flight.
        assert_eq!(
            plan_fetch_start(6, after_batch).unwrap(),
            FetchStart::FromDbTip
        );
    }

    #[test]
    fn test_fetch_start_rejects_a_cursor_from_a_later_epoch() {
        let err = plan_fetch_start(6, cursor(7, 103)).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("pipeline epoch went backwards"),
            "{message}"
        );
        assert!(message.contains("next_block=103"), "{message}");
        assert!(message.contains("epoch 7"), "{message}");
        assert!(message.contains("loaded epoch 6"), "{message}");
    }

    fn evidence(source: &'static str) -> NoDivergenceEvidence<'static> {
        NoDivergenceEvidence {
            source,
            pipeline_epoch: 9,
            start_block: 102,
            first_parent_hash: &OTHER_HASH,
        }
    }

    /// Ten checks in a row on one tip stop the pipeline with everything an
    /// operator needs to find the broken side.
    #[test]
    fn test_no_divergence_streak_stops_at_the_bound_with_full_context() {
        let mut streak = NoDivergenceStreak::default();
        for expected in 1..MAX_CONSECUTIVE_NO_DIVERGENCE {
            assert_eq!(
                streak
                    .record(101, &STORED_TIP_HASH, &evidence("stale-fork"))
                    .unwrap(),
                expected
            );
        }
        let message = streak
            .record(101, &STORED_TIP_HASH, &evidence("stale-fork"))
            .unwrap_err()
            .to_string();
        for expected in [
            format!("{MAX_CONSECUTIVE_NO_DIVERGENCE} consecutive reorg checks found no divergence"),
            format!("db_tip=101, stored_hash=0x{}", hex::encode(STORED_TIP_HASH)),
            format!(
                "the stale-fork batch starting at block 102 names parent 0x{}",
                hex::encode(OTHER_HASH)
            ),
            "pipeline_epoch=9".to_string(),
        ] {
            assert!(
                message.contains(&expected),
                "missing `{expected}` in: {message}"
            );
        }
    }

    /// Progress (a written batch) or a handled divergence ends the streak,
    /// and so does a different store tip.
    #[test]
    fn test_no_divergence_streak_restarts_on_reset_and_on_a_new_tip() {
        let mut streak = NoDivergenceStreak::default();
        let checks = MAX_CONSECUTIVE_NO_DIVERGENCE - 1;
        for _ in 0..checks {
            streak
                .record(101, &STORED_TIP_HASH, &evidence("stale-fork"))
                .unwrap();
        }
        streak.reset();
        for expected in 1..=checks {
            assert_eq!(
                streak
                    .record(101, &STORED_TIP_HASH, &evidence("stale-fork"))
                    .unwrap(),
                expected
            );
        }
        // Another tip height, then the same height with another hash.
        assert_eq!(
            streak
                .record(
                    102,
                    &STORED_TIP_HASH,
                    &evidence("parser-reported orphaned tip")
                )
                .unwrap(),
            1
        );
        assert_eq!(
            streak
                .record(102, &OTHER_HASH, &evidence("parser-reported orphaned tip"))
                .unwrap(),
            1
        );
    }

    #[test]
    fn test_should_log_pipeline_idle_timeout_policy() {
        assert!(should_log_pipeline_idle_timeout(1));
        assert!(should_log_pipeline_idle_timeout(2));
        assert!(should_log_pipeline_idle_timeout(3));
        assert!(!should_log_pipeline_idle_timeout(4));
        assert!(should_log_pipeline_idle_timeout(10));
        assert!(should_log_pipeline_idle_timeout(20));
    }

    #[test]
    fn test_record_worker_exit_reason_keeps_first_reason() {
        let slot = Arc::new(std::sync::Mutex::new(None));
        record_worker_exit_reason(&slot, "first failure");
        record_worker_exit_reason(&slot, "second failure");
        assert_eq!(
            get_worker_exit_reason(&slot).as_deref(),
            Some("first failure")
        );
    }

    #[test]
    fn test_format_pipeline_worker_termination_message_includes_context() {
        let msg = format_pipeline_worker_termination_message(
            true,
            false,
            Some("parser exploded"),
            Some("fetcher okay"),
        );
        assert!(msg.contains("parser_finished=true"));
        assert!(msg.contains("fetcher_finished=false"));
        assert!(msg.contains("parser_reason=parser exploded"));
        assert!(msg.contains("fetcher_reason=fetcher okay"));
    }

    #[test]
    fn test_repeated_warning_tracker_suppresses_and_aggregates() {
        let tracker = RepeatedWarningTracker::default();

        let first = tracker
            .record("pipeline_idle_timeout", Duration::from_secs(60))
            .expect("first warning should emit");
        assert_eq!(first.total_count, 1);
        assert_eq!(first.suppressed_since_last_emit, 0);

        let second = tracker.record("pipeline_idle_timeout", Duration::from_secs(60));
        assert!(second.is_none(), "second warning should be suppressed");

        let third = tracker
            .record("pipeline_idle_timeout", Duration::from_secs(0))
            .expect("forced emit should flush suppressed count");
        assert_eq!(third.total_count, 3);
        assert_eq!(third.suppressed_since_last_emit, 1);
    }

    #[test]
    fn test_repeated_warning_tracker_isolated_by_key() {
        let tracker = RepeatedWarningTracker::default();
        assert!(tracker
            .record("pipeline_idle_timeout", Duration::from_secs(60))
            .is_some());
        assert!(tracker
            .record("pipeline_batch_mismatch", Duration::from_secs(60))
            .is_some());
    }

    #[test]
    fn test_bulk_build_perf_snapshot_returns_none_when_empty() {
        let perf = BulkBuildPerfStats::default();
        assert!(perf.snapshot().is_none());
    }

    #[test]
    fn test_bulk_build_perf_snapshot_returns_data_after_record() {
        let perf = BulkBuildPerfStats::default();
        perf.record_batch(
            45.2,                // facts_ms
            35.8,                // resolve_ms
            28.1,                // reduce_ms
            18.5,                // history_ms
            8.3,                 // address_reduce_ms
            5.1,                 // activity_stats_ms
            52.0,                // flush_ms
            120.5,               // fetch_ms
            141.0,               // build_ms
            Some(1_800_000_000), // owner_memory_bytes
            12_345_678,          // live_cell_count
            5_000,               // cells_created
            3_000,               // cells_consumed
            45_230,              // cumulative_history_rows
            12_890,              // cumulative_sealed_rows
            8_500,               // batch_block_count
            1,                   // batch_count
            4.7,                 // tx_density
            0.042,               // ms_per_block_ema
            1380.0,              // controllable_ms
            1500.0,              // target_iteration_ms
            0.0,                 // facts_par_iter_ms
            0.0,                 // facts_merge_ms
            0.0,                 // facts_serial_equivalent_ms
            0,                   // facts_intern_slow_path_count
            0,                   // facts_intern_total_count
            0,                   // facts_cell_count
            0.0,                 // flush_wait_ms
            0.0,                 // prefetch_recv_ms
            2,                   // prefetch_channel_pending
            4,                   // prefetch_channel_capacity
            1,                   // flush_channel_pending
            4,                   // flush_channel_capacity
        );

        let snap = perf.snapshot().expect("should have data after record");
        // us<->ms round-trip loses sub-microsecond precision; verify ≤0.001ms error
        assert!((snap.facts_ms.unwrap() - 45.2).abs() < 0.01);
        assert!((snap.resolve_ms.unwrap() - 35.8).abs() < 0.01);
        assert!((snap.reduce_ms.unwrap() - 28.1).abs() < 0.01);
        assert_eq!(snap.owner_memory_bytes, Some(1_800_000_000));
        assert_eq!(snap.live_cell_count, Some(12_345_678));
        assert_eq!(snap.cells_created, Some(5_000));
        assert_eq!(snap.cells_consumed, Some(3_000));
        assert_eq!(snap.batch_block_count, Some(8_500));
        assert_eq!(snap.batch_count, Some(1));
        assert!((snap.tx_density.unwrap() - 4.7).abs() < f64::EPSILON);
        assert!((snap.ms_per_block_ema.unwrap() - 0.042).abs() < f64::EPSILON);
        assert!((snap.controllable_ms.unwrap() - 1380.0).abs() < 0.01);
        assert!((snap.target_iteration_ms.unwrap() - 1500.0).abs() < 0.01);
        assert_eq!(snap.flush_channel_pending, Some(1));
        assert_eq!(snap.flush_channel_capacity, Some(4));
    }

    /// D-9: the live owner-memory gauge is either this batch's walk or absent;
    /// a batch without a walk does not carry the previous value forward.
    #[test]
    fn owner_memory_gauge_is_absent_after_a_batch_without_a_walk() {
        let perf = BulkBuildPerfStats::default();
        let record = |owner_memory_bytes: Option<u64>, batch_count: u64| {
            perf.record_batch(
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                owner_memory_bytes,
                10,
                1,
                1,
                1,
                1,
                1,
                batch_count,
                1.0,
                1.0,
                1.0,
                1.0,
                0.0,
                0.0,
                0.0,
                0,
                0,
                0,
                0.0,
                0.0,
                0,
                1,
                0,
                1,
            );
            perf.snapshot().expect("recorded").owner_memory_bytes
        };
        assert_eq!(record(Some(7_000), 1), Some(7_000));
        assert_eq!(record(None, 2), None);
        assert_eq!(record(Some(9_000), 3), Some(9_000));
    }

    #[test]
    fn test_snapshot_includes_facts_breakdown() {
        let perf = BulkBuildPerfStats::default();
        perf.record_batch(
            45.2,
            35.8,
            28.1,
            18.5,
            8.3,
            5.1,
            52.0,
            120.5,
            141.0,
            Some(1_800_000_000),
            12_345_678,
            5_000,
            3_000,
            45_230,
            12_890,
            8_500,
            1,
            4.7,
            0.042,
            1380.0,
            1500.0,
            // facts breakdown:
            40.0,   // facts_par_iter_ms
            5.2,    // facts_merge_ms
            280.0,  // facts_serial_equivalent_ms
            1_200,  // facts_intern_slow_path_count
            42_000, // facts_intern_total_count
            28_000, // facts_cell_count
            0.0,    // flush_wait_ms
            0.0,    // prefetch_recv_ms
            1,      // prefetch_channel_pending
            4,      // prefetch_channel_capacity
            2,      // flush_channel_pending
            4,      // flush_channel_capacity
        );
        let snap = perf.snapshot().unwrap();
        assert!((snap.facts_par_iter_ms.unwrap() - 40.0).abs() < 0.01);
        assert!((snap.facts_merge_ms.unwrap() - 5.2).abs() < 0.01);
        assert!((snap.facts_serial_equivalent_ms.unwrap() - 280.0).abs() < 0.01);
        assert_eq!(snap.facts_intern_slow_path_count, Some(1_200));
        assert_eq!(snap.facts_intern_total_count, Some(42_000));
        assert_eq!(snap.facts_cell_count, Some(28_000));
    }

    #[test]
    fn test_bulk_build_perf_includes_wait_fields() {
        let stats = BulkBuildPerfStats::default();
        assert!(stats.snapshot().is_none());

        stats.record_batch(
            10.0,
            5.0,
            8.0,
            3.0,
            2.0,
            1.0, // facts, resolve, reduce, history, addr_reduce, activity_stats
            50.0,
            200.0,
            3000.0, // flush, fetch, build
            Some(1000),
            500,
            100,
            80, // owner_mem, live_cells, created, consumed
            1000,
            500, // cumulative_history, cumulative_sealed
            5000,
            1, // batch_block_count, batch_count
            2.5,
            0.8,
            3100.0,
            1500.0, // tx_density, ms_per_block_ema, controllable, target_iteration
            8.0,
            2.0,
            40.0, // facts_par_iter, facts_merge, facts_serial_equiv
            5,
            100,
            200, // facts_intern_slow, facts_intern_total, facts_cell_count
            15.0,
            3.5, // flush_wait_ms, prefetch_recv_ms
            2,
            4, // prefetch_channel_pending, prefetch_channel_capacity
            3,
            4, // flush_channel_pending, flush_channel_capacity
        );

        let snap = stats
            .snapshot()
            .expect("snapshot should be Some after record_batch");
        let fw = snap.flush_wait_ms.expect("flush_wait_ms should be Some");
        assert!(
            (fw - 15.0).abs() < 0.01,
            "flush_wait_ms: expected ~15.0, got {fw}"
        );
        let pc = snap
            .prefetch_recv_ms
            .expect("prefetch_recv_ms should be Some");
        assert!(
            (pc - 3.5).abs() < 0.01,
            "prefetch_recv_ms: expected ~3.5, got {pc}"
        );
        assert_eq!(snap.flush_channel_pending, Some(3));
        assert_eq!(snap.flush_channel_capacity, Some(4));
    }

    #[test]
    fn bulk_build_progress_snapshot_includes_disk_pressure_fields() {
        let stats = BulkBuildPerfStats::default();
        stats.record_batch(
            10.0,
            5.0,
            8.0,
            3.0,
            2.0,
            1.0, // facts, resolve, reduce, history, addr_reduce, activity_stats
            50.0,
            200.0,
            3000.0, // flush, fetch, build
            Some(1000),
            500,
            100,
            80, // owner_mem, live_cells, created, consumed
            1000,
            500, // cumulative_history, cumulative_sealed
            5000,
            1, // batch_block_count, batch_count
            2.5,
            0.8,
            3100.0,
            1500.0, // tx_density, ms_per_block_ema, controllable, target_iteration
            8.0,
            2.0,
            40.0, // facts_par_iter, facts_merge, facts_serial_equiv
            5,
            100,
            200, // facts_intern_slow, facts_intern_total, facts_cell_count
            15.0,
            3.5, // flush_wait_ms, prefetch_recv_ms
            2,
            4, // prefetch_channel_pending, prefetch_channel_capacity
            3,
            4, // flush_channel_pending, flush_channel_capacity
        );

        let before = stats
            .snapshot()
            .expect("snapshot should exist after batch record");
        assert_eq!(before.disk_state, None);
        assert_eq!(before.disk_util_pct, None);
        assert_eq!(before.disk_await_ms, None);
        assert_eq!(before.disk_avg_queue_depth, None);
        assert_eq!(before.disk_write_mb_s, None);
        assert_eq!(before.disk_write_iops, None);

        stats.record_disk_telemetry(
            Some("saturated"),
            Some(94.5),
            Some(18.25),
            Some(2.75),
            Some(712.0),
            Some(18_400.0),
        );

        let after = stats
            .snapshot()
            .expect("snapshot should still exist after disk update");
        assert_eq!(after.disk_state.as_deref(), Some("saturated"));
        assert_eq!(after.disk_util_pct, Some(94.5));
        assert_eq!(after.disk_await_ms, Some(18.25));
        assert_eq!(after.disk_avg_queue_depth, Some(2.75));
        assert_eq!(after.disk_write_mb_s, Some(712.0));
        assert_eq!(after.disk_write_iops, Some(18_400.0));
    }

    #[test]
    fn bulk_build_progress_snapshot_threads_unavailable_disk_state() {
        let stats = BulkBuildPerfStats::default();
        stats.record_batch(
            10.0,
            5.0,
            8.0,
            3.0,
            2.0,
            1.0, // facts, resolve, reduce, history, addr_reduce, activity_stats
            50.0,
            200.0,
            3000.0, // flush, fetch, build
            Some(1000),
            500,
            100,
            80, // owner_mem, live_cells, created, consumed
            1000,
            500, // cumulative_history, cumulative_sealed
            5000,
            1, // batch_block_count, batch_count
            2.5,
            0.8,
            3100.0,
            1500.0, // tx_density, ms_per_block_ema, controllable, target_iteration
            8.0,
            2.0,
            40.0, // facts_par_iter, facts_merge, facts_serial_equiv
            5,
            100,
            200, // facts_intern_slow, facts_intern_total, facts_cell_count
            15.0,
            3.5, // flush_wait_ms, prefetch_recv_ms
            2,
            4, // prefetch_channel_pending, prefetch_channel_capacity
            3,
            4, // flush_channel_pending, flush_channel_capacity
        );

        stats.record_disk_telemetry(Some("unavailable"), None, None, None, None, None);

        let snap = stats
            .snapshot()
            .expect("snapshot should exist after batch record");
        assert_eq!(snap.disk_state.as_deref(), Some("unavailable"));
        assert_eq!(snap.disk_util_pct, None);
        assert_eq!(snap.disk_await_ms, None);
        assert_eq!(snap.disk_avg_queue_depth, None);
        assert_eq!(snap.disk_write_mb_s, None);
        assert_eq!(snap.disk_write_iops, None);
    }

    #[test]
    fn bulk_build_progress_snapshot_keeps_absent_disk_telemetry_absent() {
        let stats = BulkBuildPerfStats::default();
        stats.record_batch(
            10.0,
            5.0,
            8.0,
            3.0,
            2.0,
            1.0, // facts, resolve, reduce, history, addr_reduce, activity_stats
            50.0,
            200.0,
            3000.0, // flush, fetch, build
            Some(1000),
            500,
            100,
            80, // owner_mem, live_cells, created, consumed
            1000,
            500, // cumulative_history, cumulative_sealed
            5000,
            1, // batch_block_count, batch_count
            2.5,
            0.8,
            3100.0,
            1500.0, // tx_density, ms_per_block_ema, controllable, target_iteration
            8.0,
            2.0,
            40.0, // facts_par_iter, facts_merge, facts_serial_equiv
            5,
            100,
            200, // facts_intern_slow, facts_intern_total, facts_cell_count
            15.0,
            3.5, // flush_wait_ms, prefetch_recv_ms
            2,
            4, // prefetch_channel_pending, prefetch_channel_capacity
            3,
            4, // flush_channel_pending, flush_channel_capacity
        );

        stats.record_disk_telemetry(None, None, None, None, None, None);

        let snap = stats
            .snapshot()
            .expect("snapshot should exist after batch record");
        assert_eq!(snap.disk_state, None);
        assert_eq!(snap.disk_util_pct, None);
        assert_eq!(snap.disk_await_ms, None);
        assert_eq!(snap.disk_avg_queue_depth, None);
        assert_eq!(snap.disk_write_mb_s, None);
        assert_eq!(snap.disk_write_iops, None);
    }

    #[test]
    fn test_ms_to_us_and_back() {
        assert_eq!(us_to_ms(ms_to_us(123.456)), 123.456);
        assert_eq!(ms_to_us(0.0), 0);
        assert_eq!(us_to_ms(0), 0.0);
    }

    #[test]
    fn test_finalize_step_label_covers_all_steps() {
        assert_eq!(finalize_step_label(1), "drain_flush");
        assert_eq!(finalize_step_label(3), "chain_stats");
        assert_eq!(finalize_step_label(5), "owners_build");
        assert_eq!(finalize_step_label(13), "sync_status");
        assert_eq!(finalize_step_label(0), "unknown");
        assert_eq!(finalize_step_label(14), "unknown");
    }

    #[test]
    fn test_record_finalize_step_updates_atomics() {
        let stats = BulkBuildPerfStats::default();
        let elapsed = std::time::Duration::from_millis(1500);
        stats.record_finalize_step(4, elapsed);
        assert_eq!(stats.finalize_step.load(Ordering::Relaxed), 4);
        let stored_us = stats.finalize_elapsed_us.load(Ordering::Relaxed);
        assert!((us_to_ms(stored_us) - 1500.0).abs() < 1.0);
    }

    #[test]
    fn test_clear_finalize_resets_atomics() {
        let stats = BulkBuildPerfStats::default();
        stats.record_finalize_step(7, std::time::Duration::from_secs(10));
        stats.clear_finalize();
        assert_eq!(stats.finalize_step.load(Ordering::Relaxed), 0);
        assert_eq!(stats.finalize_elapsed_us.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_snapshot_includes_finalize_fields_when_active() {
        let stats = BulkBuildPerfStats::default();
        // Must have batch_count > 0 for snapshot to return Some.
        stats.batch_count.store(1, Ordering::Relaxed);
        stats.record_finalize_step(5, std::time::Duration::from_millis(2500));
        let snap = stats.snapshot().unwrap();
        assert_eq!(snap.finalize_phase.as_deref(), Some("owners_build"));
        assert_eq!(snap.finalize_step, Some(4)); // 0-indexed
        assert_eq!(snap.finalize_steps_total, Some(13));
        assert!((snap.finalize_elapsed_ms.unwrap() - 2500.0).abs() < 1.0);
    }

    #[test]
    fn test_snapshot_finalize_fields_none_when_not_finalizing() {
        let stats = BulkBuildPerfStats::default();
        stats.batch_count.store(1, Ordering::Relaxed);
        let snap = stats.snapshot().unwrap();
        assert!(snap.finalize_phase.is_none());
        assert!(snap.finalize_step.is_none());
        assert!(snap.finalize_steps_total.is_none());
        assert!(snap.finalize_elapsed_ms.is_none());
    }
}
