use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ckbadger_common::{BulkBuildProgressData, PipelineProgressData};
use dashmap::DashMap;
use serde::Serialize;
use tracing::info;

use crate::runtime_diag::{CgroupMemorySnapshot, FlightEvent};

use super::helpers::duration_from_millis;
use super::types::CachedCellInfo;

// ── Constants ───────────────────────────────────────────────────────────

pub(crate) const FLIGHT_RECORDER_CAPACITY: usize = 200;

pub(crate) const CELL_CACHE_CAPACITY: usize = 200_000;

pub(crate) const CHART_INVALIDATION_MAX_LIVE_LAG: u64 = 100;

pub(crate) const PARSER_UNRESOLVED_MAX_RETRIES: usize = 240;

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

    pub(crate) fn report_and_reset(&self) {
        let blocks = self.blocks_count.swap(0, Ordering::Relaxed);
        if blocks == 0 {
            return;
        }
        let fetch_us = self.fetch_us.swap(0, Ordering::Relaxed);
        let db_stage_us = self.db_stage_write_us.swap(0, Ordering::Relaxed);
        let db_commit_us = self.db_commit_us.swap(0, Ordering::Relaxed);
        self.last_fetch_us.store(fetch_us, Ordering::Relaxed);
        self.last_db_stage_write_us
            .store(db_stage_us, Ordering::Relaxed);
        self.last_db_commit_us
            .store(db_commit_us, Ordering::Relaxed);

        let fetch_ms = fetch_us as f64 / 1000.0;
        let db_stage_ms = db_stage_us as f64 / 1000.0;
        let db_commit_ms = db_commit_us as f64 / 1000.0;
        info!(
            blocks,
            fetch_ms = format!("{:.1}", fetch_ms),
            db_stage_ms = format!("{:.1}", db_stage_ms),
            db_commit_ms = format!("{:.1}", db_commit_ms),
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
    last_prefetch_collect_us: AtomicU64,
    // In-memory state
    owner_memory_bytes: AtomicU64,
    live_cell_count: AtomicU64,
    // Per-batch volume
    cells_created: AtomicU64,
    cells_consumed: AtomicU64,
    // Cumulative materialization
    cumulative_history_rows: AtomicU64,
    cumulative_sealed_rows: AtomicU64,
    // Batch sizing
    batch_block_span: AtomicU64,
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
        owner_memory_bytes: u64,
        live_cell_count: u64,
        cells_created: u64,
        cells_consumed: u64,
        cumulative_history_rows: u64,
        cumulative_sealed_rows: u64,
        batch_block_span: u64,
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
        prefetch_collect_ms: f64,
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
        self.owner_memory_bytes
            .store(owner_memory_bytes, Ordering::Relaxed);
        self.live_cell_count
            .store(live_cell_count, Ordering::Relaxed);
        self.cells_created.store(cells_created, Ordering::Relaxed);
        self.cells_consumed.store(cells_consumed, Ordering::Relaxed);
        self.cumulative_history_rows
            .store(cumulative_history_rows, Ordering::Relaxed);
        self.cumulative_sealed_rows
            .store(cumulative_sealed_rows, Ordering::Relaxed);
        self.batch_block_span
            .store(batch_block_span, Ordering::Relaxed);
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
        self.last_prefetch_collect_us
            .store(ms_to_us(prefetch_collect_ms), Ordering::Relaxed);
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
            prefetch_collect_ms: Some(us_to_ms(
                self.last_prefetch_collect_us.load(Ordering::Relaxed),
            )),
            fetch_ms: Some(us_to_ms(self.last_fetch_us.load(Ordering::Relaxed))),
            build_ms: Some(us_to_ms(self.last_build_us.load(Ordering::Relaxed))),
            owner_memory_bytes: Some(self.owner_memory_bytes.load(Ordering::Relaxed)),
            live_cell_count: Some(self.live_cell_count.load(Ordering::Relaxed)),
            cells_created: Some(self.cells_created.load(Ordering::Relaxed)),
            cells_consumed: Some(self.cells_consumed.load(Ordering::Relaxed)),
            cumulative_history_rows: Some(self.cumulative_history_rows.load(Ordering::Relaxed)),
            cumulative_sealed_rows: Some(self.cumulative_sealed_rows.load(Ordering::Relaxed)),
            batch_block_span: Some(self.batch_block_span.load(Ordering::Relaxed)),
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
        5 => "owner:address",
        6 => "owner:script",
        7 => "owner:token",
        8 => "owner:dao",
        9 => "owner:fiber",
        10 => "owner:object",
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
        let now = Instant::now();
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

pub(crate) fn sender_queue_depth<T>(sender: &tokio::sync::mpsc::Sender<T>) -> u64 {
    (sender.max_capacity() - sender.capacity()) as u64
}

pub(crate) fn queue_fill_percentage(depth: Option<u64>, capacity: Option<u64>) -> Option<f64> {
    match (depth, capacity) {
        (Some(d), Some(c)) if c > 0 => Some((d as f64 / c as f64) * 100.0),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct QueuePressureSnapshot {
    pub(crate) parse_queue_pending_txs: u64,
    pub(crate) parse_queue_capacity_txs: u64,
    pub(crate) parse_queue_fill_pct: Option<f64>,
    pub(crate) writer_queue_depth: u64,
    pub(crate) writer_queue_capacity: u64,
    pub(crate) writer_queue_fill_pct: Option<f64>,
}

pub(crate) fn build_queue_pressure_snapshot(
    parse_queue_pending_txs: u64,
    parse_queue_capacity_txs: u64,
    writer_queue_depth: u64,
    writer_queue_capacity: u64,
) -> QueuePressureSnapshot {
    QueuePressureSnapshot {
        parse_queue_pending_txs,
        parse_queue_capacity_txs,
        parse_queue_fill_pct: queue_fill_percentage(
            Some(parse_queue_pending_txs),
            Some(parse_queue_capacity_txs),
        ),
        writer_queue_depth,
        writer_queue_capacity,
        writer_queue_fill_pct: queue_fill_percentage(
            Some(writer_queue_depth),
            Some(writer_queue_capacity),
        ),
    }
}

pub(crate) fn parse_queue_capacity_txs(
    queue_capacity_batches: usize,
    target_batch_txs: u64,
    min_target_batch_txs: u64,
) -> u64 {
    let queue_capacity_batches =
        u64::try_from(queue_capacity_batches).expect("parse queue capacity exceeds u64");
    let per_batch_tx_cap = u64::try_from(super::adaptive::adaptive_sub_batch_tx_cap(
        target_batch_txs,
        min_target_batch_txs,
    ))
    .expect("adaptive sub-batch tx cap exceeds u64");
    queue_capacity_batches
        .checked_mul(per_batch_tx_cap)
        .expect("parse queue tx capacity overflow")
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

pub(crate) fn should_log_unresolved_retry(attempt: usize) -> bool {
    attempt == 1 || attempt.is_multiple_of(10) || attempt >= PARSER_UNRESOLVED_MAX_RETRIES
}

pub(crate) fn should_log_pipeline_idle_timeout(consecutive_idle_timeouts: u64) -> bool {
    consecutive_idle_timeouts <= 3 || consecutive_idle_timeouts.is_multiple_of(10)
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
    fn test_queue_fill_snapshot_keeps_parser_and_writer_pressure_separate() {
        let snapshot = build_queue_pressure_snapshot(320_000, 1_280_000, 3, 8);

        assert_eq!(snapshot.parse_queue_pending_txs, 320_000);
        assert_eq!(snapshot.parse_queue_capacity_txs, 1_280_000);
        assert_eq!(snapshot.parse_queue_fill_pct, Some(25.0));
        assert_eq!(snapshot.writer_queue_depth, 3);
        assert_eq!(snapshot.writer_queue_capacity, 8);
        assert_eq!(snapshot.writer_queue_fill_pct, Some(37.5));
    }

    #[test]
    fn test_compaction_pressure_snapshot_reports_l0_total_and_l0_max() {
        let snapshot = ckbadger_store::store::CompactionPressureSnapshot {
            l0_files_total: 82,
            l0_files_max: 3,
            compaction_pending_bytes: 0,
            immutable_memtables: 0,
        };

        assert_eq!(snapshot.l0_files_total, 82);
        assert_eq!(snapshot.l0_files_max, 3);
    }

    #[test]
    fn test_parse_queue_capacity_txs_uses_sub_batch_cap() {
        use super::super::adaptive::ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS;
        assert_eq!(
            parse_queue_capacity_txs(8, 40_000, ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS),
            320_000
        );
    }

    #[test]
    fn test_parse_queue_capacity_txs_respects_floor() {
        assert_eq!(parse_queue_capacity_txs(4, 2_500, 8_000), 32_000);
    }

    #[test]
    #[should_panic(expected = "parse queue tx capacity overflow")]
    fn test_parse_queue_capacity_txs_panics_on_overflow() {
        use super::super::adaptive::ADAPTIVE_BATCH_MAX_TXS;
        let _ = parse_queue_capacity_txs(usize::MAX, ADAPTIVE_BATCH_MAX_TXS, 10_000);
    }

    #[tokio::test]
    async fn test_sender_queue_depth_tracks_runtime_channel_state() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u8>(4);
        assert_eq!(sender_queue_depth(&tx), 0);
        tx.send(1).await.unwrap();
        tx.send(2).await.unwrap();
        assert_eq!(sender_queue_depth(&tx), 2);
        assert_eq!(rx.recv().await, Some(1));
        assert_eq!(sender_queue_depth(&tx), 1);
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
            CachedCellInfo {
                capacity: 1,
                created_at_block,
                lock_script_hash: vec![1u8; 32],
                lock_code_hash: vec![2u8; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: None,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                data_size: 0,
                occupied_capacity: 1,
                udt_amount: None,
                data_hash: None,
            }
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
            45.2,          // facts_ms
            35.8,          // resolve_ms
            28.1,          // reduce_ms
            18.5,          // history_ms
            8.3,           // address_reduce_ms
            5.1,           // activity_stats_ms
            52.0,          // flush_ms
            120.5,         // fetch_ms
            141.0,         // build_ms
            1_800_000_000, // owner_memory_bytes
            12_345_678,    // live_cell_count
            5_000,         // cells_created
            3_000,         // cells_consumed
            45_230,        // cumulative_history_rows
            12_890,        // cumulative_sealed_rows
            8_500,         // batch_block_span
            1,             // batch_count
            4.7,           // tx_density
            0.042,         // ms_per_block_ema
            1380.0,        // controllable_ms
            1500.0,        // target_iteration_ms
            0.0,           // facts_par_iter_ms
            0.0,           // facts_merge_ms
            0.0,           // facts_serial_equivalent_ms
            0,             // facts_intern_slow_path_count
            0,             // facts_intern_total_count
            0,             // facts_cell_count
            0.0,           // flush_wait_ms
            0.0,           // prefetch_collect_ms
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
        assert_eq!(snap.batch_block_span, Some(8_500));
        assert_eq!(snap.batch_count, Some(1));
        assert!((snap.tx_density.unwrap() - 4.7).abs() < f64::EPSILON);
        assert!((snap.ms_per_block_ema.unwrap() - 0.042).abs() < f64::EPSILON);
        assert!((snap.controllable_ms.unwrap() - 1380.0).abs() < 0.01);
        assert!((snap.target_iteration_ms.unwrap() - 1500.0).abs() < 0.01);
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
            1_800_000_000,
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
            0.0,    // prefetch_collect_ms
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
            10.0, 5.0, 8.0, 3.0, 2.0,
            1.0, // facts, resolve, reduce, history, addr_reduce, activity_stats
            50.0, 200.0, 3000.0, // flush, fetch, build
            1000, 500, 100, 80, // owner_mem, live_cells, created, consumed
            1000, 500, // cumulative_history, cumulative_sealed
            5000, 1, // batch_block_span, batch_count
            2.5, 0.8, 3100.0,
            1500.0, // tx_density, ms_per_block_ema, controllable, target_iteration
            8.0, 2.0, 40.0, // facts_par_iter, facts_merge, facts_serial_equiv
            5, 100, 200, // facts_intern_slow, facts_intern_total, facts_cell_count
            15.0, 3.5, // flush_wait_ms, prefetch_collect_ms
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
            .prefetch_collect_ms
            .expect("prefetch_collect_ms should be Some");
        assert!(
            (pc - 3.5).abs() < 0.01,
            "prefetch_collect_ms: expected ~3.5, got {pc}"
        );
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
        assert_eq!(finalize_step_label(5), "owner:address");
        assert_eq!(finalize_step_label(13), "sync_status");
        assert_eq!(finalize_step_label(0), "unknown");
        assert_eq!(finalize_step_label(14), "unknown");
    }

    #[test]
    fn test_finalize_total_steps_constant() {
        assert_eq!(FINALIZE_TOTAL_STEPS, 13);
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
        assert_eq!(snap.finalize_phase.as_deref(), Some("owner:address"));
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
