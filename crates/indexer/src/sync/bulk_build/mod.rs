use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rustc_hash::{FxHashMap, FxHashSet};

use anyhow::Result;
use anyhow::{anyhow, bail};
use ckbadger_store::keys;
use ckbadger_store::store::CF_TOKEN_TRANSFERS;
use ckbadger_store::types::{
    decode_live_cell_marker, BulkBuildSessionMarker, CachedBlockHeader,
    CellDistributionTrackerState, ConsumedCellMeta, DailyActivityStats, DailyAddressCohort,
    DailyCellDistribution, DailyHodlWave, DaoDailySnapshot, DaoLatestStatistics, DaoTopDepositors,
    HodlTrackerState, LiveCellInfo, ObjectStandard, ScriptDailyDelta, SporeTypeIndex, SyncStatus,
    TokenTransferRecord, TxActivityBundle, TxIndexEntry, DID_CKB_SENTINEL_COLLECTION,
    DOTBIT_SENTINEL_COLLECTION, SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::{
    AddressBalance, CkbadgerStore, ScriptInfo, CF_ACTIVITIES, CF_ADDR_TXS, CF_BLOCK_HASH_INDEX,
    CF_BLOCK_HEADERS, CF_CELLS, CF_CELL_BY_DATA_HASH, CF_CELL_BY_LOCK, CF_CELL_BY_LOCK_CODE,
    CF_CELL_BY_TYPE, CF_CELL_BY_TYPE_CODE, CF_CONSUMED_CELLS, CF_IDENTITY_COLLECTION_ACTIVITIES,
    CF_LIVE_CELLS, CF_OBJECT_COLLECTION_ACTIVITIES, CF_STATS_CHAIN, CF_STATS_HODL, CF_TX_HASH_MAP,
    CF_TX_INDEX,
};
use rayon::prelude::*;
use rocksdb::IteratorMode;
use tracing::info;

use super::indexer::{
    finalize_bulk_stage_handoff_state, persist_bulk_sync_completion_status,
    take_bulk_sync_completion_transition, Indexer,
};
use crate::bulk_sync_perf::BatchSample;
use crate::parser::{ParsedUdtCell, UdtParser, UdtStandard};
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::owners::BulkReducer;

pub(crate) mod accounting;
pub(crate) mod binary_facts;
pub(crate) mod facts;
pub(crate) mod interner;
pub(crate) mod live_cells;
pub(crate) mod materialize;
pub(crate) mod owners;
pub(crate) mod prefetch;
pub(crate) mod sampler;
pub(crate) mod sequencer;

use crate::sync::bottleneck::{BatchSignals, BottleneckController};

const BULK_BUILD_MIN_BLOCK_SPAN: u64 = 10_000;

#[derive(Debug, Default, PartialEq, Eq)]
struct PreparedFinalizeArtifacts {
    activity_sealed_rows: Vec<materialize::MaterializedRow>,
    chain_sealed_rows: Vec<materialize::MaterializedRow>,
    final_snapshot_rows: Vec<materialize::MaterializedRow>,
}

#[derive(Default)]
pub(crate) struct BulkBuildEngine;

impl BulkBuildEngine {
    pub(crate) async fn run(indexer: &Indexer) -> Result<()> {
        let start_block = i64::try_from(indexer.progress.current()).map_err(|_| {
            anyhow!(
                "bulk build start block exceeds i64 range: current_block={}",
                indexer.progress.current()
            )
        })?;
        start_bulk_build_session_marker(
            indexer.writer.store().as_ref(),
            &indexer.run_id,
            start_block,
        )?;

        info!(
            run_id = %indexer.run_id,
            start_block,
            threshold = indexer.config.bulk_sync_threshold,
            "Bulk build engine route selected; materializing bulk stage before pipeline handoff"
        );
        Self::run_bulk_stage_until_pipeline_handoff(indexer).await?;
        let handoff_tip = i64::try_from(indexer.progress.current()).map_err(|_| {
            anyhow!(
                "bulk build handoff tip exceeds i64 range: current_block={}",
                indexer.progress.current()
            )
        })?;
        indexer.reconcile_hodl_tracker_with_tip(handoff_tip)?;
        indexer.reconcile_cell_dist_tracker_with_tip(handoff_tip)?;
        info!(
            run_id = %indexer.run_id,
            current_block = indexer.progress.current(),
            target_block = indexer.progress.target(),
            threshold = indexer.config.bulk_sync_threshold,
            "Bulk build stage finalized; handing off to pipeline for near-tip/live sync"
        );
        indexer.run_pipeline().await
    }

    async fn run_bulk_stage_until_pipeline_handoff(indexer: &Indexer) -> Result<()> {
        let ckb_store = indexer
            .ckb_store()
            .ok_or_else(|| anyhow!("bulk build requires direct CKB RocksDB reader"))?;
        let mut runtime = BulkBuildRuntimeState::default();
        let mut sync_totals = BulkBuildSyncTotals::default();
        let mut materializer = materialize::Materializer::new(
            indexer.writer.store().as_ref(),
            indexer.append_only_store.as_ref(),
        );
        let disk_device = crate::sys_info::detect_disk_device(&indexer.config.domain_data_path);
        let sampler = sampler::BackgroundSampler::new(
            indexer.writer.store().clone(),
            std::time::Duration::from_millis(200),
            disk_device,
        );
        let token_info_cache = preload_token_info_cache(indexer.writer.store().as_ref())?;
        let configured_batch_size = u64::try_from(indexer.config.batch_size).map_err(|_| {
            anyhow!(
                "bulk build batch_size exceeds u64 range: batch_size={}",
                indexer.config.batch_size
            )
        })?;
        let mem_profile = indexer.writer.store().memory_profile();
        // Max = available cores.  Fetch threads are temporary (std::thread::scope),
        // so no persistent over-subscription.  The controller shrinks this when
        // build-bound to reduce overlap contention.
        let max_fetch_threads = std::thread::available_parallelism()
            .map(|n| n.get().max(2) as u32)
            .unwrap_or(4);
        let mut controller = BottleneckController::new(
            configured_batch_size,
            max_fetch_threads,
            mem_profile.max_background_jobs,
            mem_profile.system_ram_bytes,
        );
        let channel_depth = controller.channel_depth() as usize;
        let mut batch_block_span = controller.batch_span();
        let mut batch_count: u64 = 0;
        // Compute initial handoff_target for the prefetch worker.
        let initial_chain_tip = ckb_store
            .tip_number()
            .ok_or_else(|| anyhow!("failed to get chain tip from CKB RocksDB for prefetch init"))?;
        let initial_handoff = initial_chain_tip.saturating_sub(indexer.config.bulk_sync_threshold);
        let prefetch_start = if indexer.progress.current() == 0 {
            0
        } else {
            indexer.progress.current() + 1
        };
        let (ahead_tx, ahead_rx) = tokio::sync::watch::channel(controller.prefetch_ahead());
        let (threads_tx, threads_rx) = tokio::sync::watch::channel(controller.fetch_threads());
        let mut prefetch = prefetch::PrefetchChannelHandle::new(
            channel_depth,
            ckb_store.clone(),
            prefetch_start,
            initial_handoff,
            configured_batch_size,
            ahead_rx,
            threads_rx,
        );
        // Bounded flush channel: the build loop sends PendingFlush into
        // a channel. A dedicated worker drains it serially, committing
        // each batch to RocksDB. Build only blocks when the channel is
        // full, eliminating the flush bubble when flush_ms > build_ms.
        let flush_channel = materialize::FlushChannelHandle::new(
            channel_depth,
            indexer.writer.store().clone(),
            indexer.append_only_store.clone(),
        );
        // Initial 0.0 is semantically correct (no flush yet) but always
        // overwritten by flush_channel.last_flush_ms() before first read.
        #[allow(unused_assignments)]
        let mut prev_flush_ms: f64 = 0.0;
        let mut cumulative_history_rows: usize = 0;
        let mut cumulative_sealed_rows: usize = 0;
        let mut _flush_send_count: usize = 0;

        loop {
            ckb_store.refresh()?;
            let chain_tip = ckb_store.tip_number().ok_or_else(|| {
                anyhow!("failed to get chain tip from CKB RocksDB during bulk build")
            })?;
            indexer.progress.update_target(chain_tip);

            let current_block = indexer.progress.current();
            let blocks_remaining = chain_tip.saturating_sub(current_block);
            if blocks_remaining <= indexer.config.bulk_sync_threshold {
                break;
            }

            // Receive next batch from prefetch pipeline.
            let recv_started = Instant::now();
            let prefetch_result = match prefetch.recv().await {
                Ok(result) => result,
                Err(e) => {
                    info!(error = %e, "prefetch channel closed, ending bulk build loop");
                    break;
                }
            };
            let prefetch_recv_elapsed = recv_started.elapsed();
            let prefetch_channel_pending = prefetch.pending() as u64;
            let prefetch_channel_capacity = prefetch.capacity() as u64;

            let (blocks, fetch_elapsed, effective_end) = (
                prefetch_result.blocks,
                prefetch_result.fetch_elapsed,
                prefetch_result.effective_end,
            );

            let build_started = Instant::now();
            let (batch_stats, build_timings, pending_flush) =
                runtime.apply_blocks(&blocks, indexer.config.is_mainnet(), &token_info_cache)?;
            let build_elapsed = build_started.elapsed();

            // Read the most recent flush_ms from the worker (non-blocking).
            prev_flush_ms = flush_channel.last_flush_ms();
            let critical_stage_ms = (fetch_elapsed.as_secs_f64() * 1000.0)
                .max(build_elapsed.as_secs_f64() * 1000.0)
                .max(prev_flush_ms);

            // Capture row counts before send() moves the data.
            let pending_flush_row_count = (
                pending_flush.history_rows.len(),
                pending_flush.sealed_rows.len(),
            );

            // Send to flush channel.  Blocks when channel is full (natural
            // backpressure).  Channel depth is memory-budget-derived.
            let flush_wait_started = Instant::now();
            flush_channel.send(pending_flush).await?;
            let flush_wait_elapsed = flush_wait_started.elapsed();
            let flush_channel_pending = flush_channel.pending() as u64;

            cumulative_history_rows += pending_flush_row_count.0;
            cumulative_sealed_rows += pending_flush_row_count.1;
            _flush_send_count += 1;
            sync_totals.record_batch(&batch_stats)?;

            let last_block_number = batch_stats.last_block_number.ok_or_else(|| {
                anyhow!(
                    "bulk build batch missing last block number: current_block={} effective_end={}",
                    current_block,
                    effective_end
                )
            })?;
            let last_block_u64 = u64::try_from(last_block_number).map_err(|_| {
                anyhow!(
                    "bulk build last block number is negative: last_block_number={}",
                    last_block_number
                )
            })?;
            indexer.progress.record_batch(
                last_block_u64,
                batch_stats.block_count,
                batch_stats.tx_count,
            );

            let snap = sampler.latest();
            let disk_state = snap.disk_state.clone();
            let mut sample = BatchSample::new(
                batch_stats.block_count,
                fetch_elapsed.as_secs_f64() + build_elapsed.as_secs_f64(),
                0.0,
                snap.compaction_pending_mb,
                snap.l0_files,
                snap.imm_memtables,
                chrono::Utc::now()
                    .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                    .to_string(),
                snap.load_avg_1m,
                snap.mem_available_mb,
                snap.disk_read_mb,
                snap.disk_write_mb,
            );
            sample.engine = "bulk_build".to_string();
            sample.disk_read_mb_s = snap.disk_read_mb_s;
            sample.disk_write_mb_s = snap.disk_write_mb_s;
            sample.disk_read_iops = snap.disk_read_iops;
            sample.disk_write_iops = snap.disk_write_iops;
            sample.disk_util_pct = snap.disk_util_pct;
            sample.disk_await_ms = snap.disk_await_ms;
            sample.disk_avg_queue_depth = snap.disk_avg_queue_depth;
            sample.disk_in_flight = snap.disk_in_flight;
            sample.disk_state = disk_state.clone();
            sample.txs = batch_stats.tx_count;
            sample.cells = u64::try_from(batch_stats.cells_created).map_err(|_| {
                anyhow!(
                    "bulk build created cell count is negative while recording perf sample: cells_created={}",
                    batch_stats.cells_created
                )
            })?;
            sample.inputs = u64::try_from(batch_stats.cells_consumed).map_err(|_| {
                anyhow!(
                    "bulk build consumed cell count is negative while recording perf sample: cells_consumed={}",
                    batch_stats.cells_consumed
                )
            })?;
            sample.build_ms = build_elapsed.as_secs_f64() * 1000.0;
            sample.fetch_ms = fetch_elapsed.as_secs_f64() * 1000.0;
            sample.facts_ms = build_timings.facts_ms;
            sample.resolve_ms = build_timings.resolve_ms;
            sample.reduce_ms = build_timings.reduce_ms;
            sample.history_ms = build_timings.history_ms;
            sample.address_reduce_ms = build_timings.address_reduce_ms;
            sample.activity_stats_ms = build_timings.activity_stats_ms;
            sample.facts_par_iter_ms = build_timings.facts_breakdown.par_iter_ms;
            sample.facts_merge_ms = build_timings.facts_breakdown.merge_ms;
            sample.facts_serial_equivalent_ms = build_timings.facts_breakdown.serial_equivalent_ms;
            sample.facts_intern_slow_path_count =
                build_timings.facts_breakdown.intern_slow_path_count;
            sample.facts_intern_total_count = build_timings.facts_breakdown.intern_total_count;
            sample.facts_cell_count = build_timings.facts_breakdown.cell_count;
            sample.flush_ms = prev_flush_ms;
            sample.flush_wait_ms = flush_wait_elapsed.as_secs_f64() * 1000.0;
            sample.flush_channel_depth = controller.channel_depth();
            sample.flush_channel_pending = flush_channel_pending;
            sample.prefetch_recv_ms = prefetch_recv_elapsed.as_secs_f64() * 1000.0;
            sample.prefetch_depth = controller.prefetch_ahead();
            sample.owner_memory_bytes = runtime.memory_breakdown_bytes();
            sample.live_cell_count = runtime.sequencer.live_count() as u64;
            // Cumulative row counts: tracks rows sent to flush channel.
            sample.cumulative_history_rows = cumulative_history_rows as u64;
            sample.cumulative_sealed_rows = cumulative_sealed_rows as u64;
            sample.cumulative_snapshot_rows = 0; // snapshots are written at finalize

            // Compute tx_density for both TUI publishing and adaptive sizing below.
            let tx_density = if batch_stats.block_count > 0 && batch_stats.tx_count > 0 {
                batch_stats.tx_count as f64 / batch_stats.block_count as f64
            } else {
                0.0
            };

            // Publish bulk-build metrics to shared atomics for progress monitor -> TUI.
            // Must happen before record_bulk_sync_perf_batch_sample moves sample.
            let owner_mem_total: u64 = sample.owner_memory_bytes.values().sum();
            indexer.bulk_build_perf.record_disk_telemetry(
                disk_state.as_deref(),
                snap.disk_util_pct,
                snap.disk_await_ms,
                snap.disk_avg_queue_depth,
                snap.disk_write_mb_s,
                snap.disk_write_iops,
            );
            indexer.bulk_build_perf.record_batch(
                build_timings.facts_ms,
                build_timings.resolve_ms,
                build_timings.reduce_ms,
                build_timings.history_ms,
                build_timings.address_reduce_ms,
                build_timings.activity_stats_ms,
                prev_flush_ms,
                fetch_elapsed.as_secs_f64() * 1000.0,
                build_elapsed.as_secs_f64() * 1000.0,
                owner_mem_total,
                sample.live_cell_count,
                sample.cells,
                sample.inputs,
                cumulative_history_rows as u64,
                cumulative_sealed_rows as u64,
                batch_block_span,
                batch_count + 1, // batch_count is incremented below
                tx_density,
                0.0, // ms_per_block_ema (not tracked by throughput controller)
                critical_stage_ms,
                0.0, // target_iteration_ms (not applicable)
                build_timings.facts_breakdown.par_iter_ms,
                build_timings.facts_breakdown.merge_ms,
                build_timings.facts_breakdown.serial_equivalent_ms,
                build_timings.facts_breakdown.intern_slow_path_count,
                build_timings.facts_breakdown.intern_total_count,
                build_timings.facts_breakdown.cell_count,
                flush_wait_elapsed.as_secs_f64() * 1000.0,
                prefetch_recv_elapsed.as_secs_f64() * 1000.0,
                prefetch_channel_pending,
                prefetch_channel_capacity,
                flush_channel_pending,
                controller.channel_depth(),
            );

            indexer.record_bulk_sync_perf_batch_sample(sample);

            batch_count += 1;
            let progress_pct = if chain_tip > 0 {
                last_block_u64 as f64 / chain_tip as f64 * 100.0
            } else {
                0.0
            };
            info!(
                run_id = %indexer.run_id,
                end_block = effective_end,
                blocks = batch_stats.block_count,
                txs = batch_stats.tx_count,
                current_block = last_block_u64,
                target_block = chain_tip,
                remaining_blocks = indexer.progress.blocks_remaining(),
                progress_pct = format!("{:.1}%", progress_pct),
                fetch_ms = format!("{:.1}", fetch_elapsed.as_secs_f64() * 1000.0),
                build_ms = format!("{:.1}", build_elapsed.as_secs_f64() * 1000.0),
                critical_stage_ms = format!("{:.1}", critical_stage_ms),
                facts_ms = format!("{:.1}", build_timings.facts_ms),
                resolve_ms = format!("{:.1}", build_timings.resolve_ms),
                reduce_ms = format!("{:.1}", build_timings.reduce_ms),
                history_ms = format!("{:.1}", build_timings.history_ms),
                address_reduce_ms = format!("{:.1}", build_timings.address_reduce_ms),
                activity_stats_ms = format!("{:.1}", build_timings.activity_stats_ms),
                prev_flush_ms = format!("{:.1}", prev_flush_ms),
                "Bulk build materialized batch"
            );

            if let Some(output) = controller.observe(&BatchSignals {
                prefetch_recv_ms: prefetch_recv_elapsed.as_secs_f64() * 1000.0,
                build_ms: build_elapsed.as_secs_f64() * 1000.0,
                flush_wait_ms: flush_wait_elapsed.as_secs_f64() * 1000.0,
                l0_files: snap.l0_files,
                actual_blocks: batch_stats.block_count,
                history_rows: pending_flush_row_count.0,
                flush_channel_pending,
                flush_channel_capacity: controller.channel_depth(),
            }) {
                batch_block_span = output.batch_span;
                prefetch.update_span(batch_block_span);
                let _ = ahead_tx.send(output.prefetch_ahead);
                let _ = threads_tx.send(output.fetch_threads);

                if let Some(new_bg_jobs) = controller.bg_jobs_if_changed() {
                    if let Err(e) = indexer.writer.store().set_max_background_jobs(new_bg_jobs) {
                        tracing::warn!(
                            error = %e, new_bg_jobs,
                            "Failed to adjust RocksDB background jobs"
                        );
                    }
                }

                tracing::debug!(
                    bottleneck = %output.bottleneck,
                    batch_span = output.batch_span,
                    prefetch_ahead = output.prefetch_ahead,
                    fetch_threads = output.fetch_threads,
                    channel_depth = controller.channel_depth(),
                    bg_jobs = output.bg_jobs,
                    recv_ema = format!("{:.1}", output.recv_ema),
                    build_ema = format!("{:.1}", output.build_ema),
                    wait_ema = format!("{:.1}", output.wait_ema),
                    l0_ema = format!("{:.1}", output.l0_ema),
                    "Bottleneck controller adjusted"
                );

                indexer.bulk_build_perf.record_controller(
                    output.bottleneck.to_code(),
                    output.recv_ema,
                    output.build_ema,
                    output.wait_ema,
                    output.l0_ema,
                    output.prefetch_ahead,
                    output.fetch_threads,
                    output.bg_jobs,
                    controller.rows_per_block_ema(),
                    output.flush_fill_ema,
                    controller.max_history_rows(),
                );
            }

            // Periodic memory summary every 10 batches
            if batch_count.is_multiple_of(10) {
                let mem = runtime.memory_breakdown_bytes();
                let total_mb: u64 = mem.values().sum::<u64>() / (1024 * 1024);
                let live_cells = runtime.sequencer.live_count();
                let interner_entries = runtime.interner.len();
                info!(
                    total_memory_mb = total_mb,
                    live_cells, interner_entries, batch_count, "Bulk build memory snapshot"
                );
            }
        }

        // ── Finalize: decomposed into 13 sub-phases with progress reporting ──
        // The progress monitor (entry.rs, 10s polling) reads these atomics and
        // publishes to RocksDB so the TUI can display a finalize checklist.
        let finalize_started = Instant::now();

        // Shut down prefetch worker before draining flush channel.
        let prefetch_stats = prefetch.close_and_wait().await?;
        info!(
            total_fetches = prefetch_stats.total_fetches,
            total_blocks = prefetch_stats.total_blocks,
            ahead_gate_count = prefetch_stats.ahead_gate_count,
            exit_reason = ?prefetch_stats.exit_reason,
            "Prefetch worker finished"
        );

        // Phase 0: close channel and drain all queued flushes.
        indexer
            .bulk_build_perf
            .record_finalize_step(1, finalize_started.elapsed());
        let flush_drain = flush_channel.begin_shutdown();
        let prepared_finalize = match runtime.prepare_finalize_artifacts() {
            Ok(prepared) => prepared,
            Err(err) => {
                let _ = flush_drain.wait().await;
                return Err(err);
            }
        };
        let flush_stats = flush_drain.wait().await?;
        materializer.add_external_counts(
            flush_stats.total_history_rows,
            flush_stats.total_sealed_rows,
            flush_stats.flush_count,
        );

        // Destructure runtime to get owned fields for explicit sub-phase control.
        let BulkBuildRuntimeState {
            owners,
            hodl_tracker,
            cell_dist_tracker,
            ..
        } = runtime;

        // Phase 1: activity stats (daily + hourly aggregates)
        indexer
            .bulk_build_perf
            .record_finalize_step(2, finalize_started.elapsed());
        materializer.stream_sealed_aggregate_rows(&prepared_finalize.activity_sealed_rows)?;

        // Phase 2: chain stats (hash rate, difficulty, uncle rate, epoch time, etc.)
        indexer
            .bulk_build_perf
            .record_finalize_step(3, finalize_started.elapsed());
        materializer.stream_sealed_aggregate_rows(&prepared_finalize.chain_sealed_rows)?;

        // Phase 3: final snapshot (live cell markers + index CFs)
        indexer
            .bulk_build_perf
            .record_finalize_step(4, finalize_started.elapsed());
        materializer.materialize_final_snapshot(&prepared_finalize.final_snapshot_rows)?;

        // Phases 4-9: owners (flush_sealed + materialize_final per owner)
        let mut owners = owners;

        indexer
            .bulk_build_perf
            .record_finalize_step(5, finalize_started.elapsed());
        owners.address.flush_sealed(&mut materializer)?;
        owners.address.materialize_final(&mut materializer)?;

        indexer
            .bulk_build_perf
            .record_finalize_step(6, finalize_started.elapsed());
        owners.script.flush_sealed(&mut materializer)?;
        owners.script.materialize_final(&mut materializer)?;

        indexer
            .bulk_build_perf
            .record_finalize_step(7, finalize_started.elapsed());
        owners.token.flush_sealed(&mut materializer)?;
        owners.token.materialize_final(&mut materializer)?;

        indexer
            .bulk_build_perf
            .record_finalize_step(8, finalize_started.elapsed());
        owners.dao.flush_sealed(&mut materializer)?;
        owners.dao.materialize_final(&mut materializer)?;

        indexer
            .bulk_build_perf
            .record_finalize_step(9, finalize_started.elapsed());
        owners.fiber.flush_sealed(&mut materializer)?;
        owners.fiber.materialize_final(&mut materializer)?;

        indexer
            .bulk_build_perf
            .record_finalize_step(10, finalize_started.elapsed());
        owners.object.flush_sealed(&mut materializer)?;
        owners.object.materialize_final(&mut materializer)?;

        // Phase 10: metadata (HODL + cell distribution tracker state)
        indexer
            .bulk_build_perf
            .record_finalize_step(11, finalize_started.elapsed());
        let mut meta_batch =
            ckbadger_store::batch::StoreBatch::new(indexer.writer.store().as_ref());
        meta_batch.put_hodl_tracker_state(&hodl_tracker.to_state());
        meta_batch.put_cell_dist_tracker_state(&cell_dist_tracker.to_state());
        if !meta_batch.is_empty() {
            meta_batch.commit()?;
        }

        // Phase 11: memtable flush
        indexer
            .bulk_build_perf
            .record_finalize_step(12, finalize_started.elapsed());
        flush_bulk_build_materialized_state(
            indexer.writer.store().as_ref(),
            indexer.writer.append_only_store(),
        )?;

        // Phase 12: sync status + cleanup
        indexer
            .bulk_build_perf
            .record_finalize_step(13, finalize_started.elapsed());
        sync_totals.finalize_success(indexer.writer.store().as_ref(), false)?;
        indexer.writer.refresh_latest_dao_statistics()?;
        indexer.writer.store().clear_bulk_build_session_marker()?;

        let finalize_elapsed = finalize_started.elapsed();
        indexer.bulk_build_perf.clear_finalize();
        info!(
            run_id = %indexer.run_id,
            finalize_ms = format!("{:.1}", finalize_elapsed.as_secs_f64() * 1000.0),
            batch_count,
            "Bulk build finalize completed"
        );

        indexer.record_bulk_sync_perf_finalize_seconds(finalize_elapsed.as_secs_f64());

        let previous_bulk_sync_allowed = finalize_bulk_stage_handoff_state(
            &indexer.bulk_sync_allowed,
            &indexer.was_bulk_sync_active,
        );
        info!(
            run_id = %indexer.run_id,
            previous_bulk_sync_allowed,
            "Bulk build stage handoff disabled bulk re-entry before pipeline takeover"
        );
        let report = materializer.finish();
        indexer.record_bulk_sync_perf_materialization_report(report);
        sampler.shutdown();
        Ok(())
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct BatchExecutionStats {
    last_block_number: Option<i64>,
    last_block_hash: Option<Vec<u8>>,
    block_count: u64,
    tx_count: u64,
    cells_created: i64,
    cells_consumed: i64,
}

impl BatchExecutionStats {
    fn is_empty(&self) -> bool {
        self.block_count == 0
    }
}

#[derive(Debug, Default, Clone)]
struct BatchBuildTimings {
    facts_ms: f64,
    facts_breakdown: binary_facts::FactsTimingBreakdown,
    resolve_ms: f64,
    reduce_ms: f64,
    history_ms: f64,
    address_reduce_ms: f64,
    activity_stats_ms: f64,
}

/// Rows produced by `apply_blocks` that need to be flushed to RocksDB.
/// Designed to be `Send` so it can be moved into `spawn_blocking`.
pub(crate) struct PendingFlush {
    pub(crate) history_rows: Vec<materialize::MaterializedRow>,
    pub(crate) sealed_rows: Vec<materialize::MaterializedRow>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct BulkBuildSyncTotals {
    last_block_number: Option<i64>,
    last_block_hash: Option<Vec<u8>>,
    total_transactions: i64,
    total_cells_created: i64,
    total_cells_consumed: i64,
}

impl BulkBuildSyncTotals {
    fn record_batch(&mut self, stats: &BatchExecutionStats) -> Result<()> {
        if stats.is_empty() {
            return Ok(());
        }

        self.last_block_number = stats.last_block_number;
        self.last_block_hash = stats.last_block_hash.clone();
        self.total_transactions = checked_add_sync_total(
            "total_transactions",
            self.total_transactions,
            i64::try_from(stats.tx_count).map_err(|_| {
                anyhow!(
                    "bulk build tx_count exceeds i64 range while recording batch sync totals: tx_count={}",
                    stats.tx_count
                )
            })?,
            self.last_block_number.unwrap_or_default(),
        )?;
        self.total_cells_created = checked_add_sync_total(
            "total_cells_created",
            self.total_cells_created,
            stats.cells_created,
            self.last_block_number.unwrap_or_default(),
        )?;
        self.total_cells_consumed = checked_add_sync_total(
            "total_cells_consumed",
            self.total_cells_consumed,
            stats.cells_consumed,
            self.last_block_number.unwrap_or_default(),
        )?;
        Ok(())
    }

    fn finalize_success(
        self,
        store: &CkbadgerStore,
        mark_bulk_sync_completed: bool,
    ) -> Result<SyncStatus> {
        let mut status = store.get_sync_status()?;

        if let Some(last_block_number) = self.last_block_number {
            status.tip_block_number = last_block_number;
        }
        if let Some(last_block_hash) = self.last_block_hash {
            status.tip_block_hash = last_block_hash;
        }

        status.total_transactions = checked_add_sync_total(
            "total_transactions",
            status.total_transactions,
            self.total_transactions,
            status.tip_block_number,
        )?;
        status.total_cells_created = checked_add_sync_total(
            "total_cells_created",
            status.total_cells_created,
            self.total_cells_created,
            status.tip_block_number,
        )?;
        status.total_cells_consumed = checked_add_sync_total(
            "total_cells_consumed",
            status.total_cells_consumed,
            self.total_cells_consumed,
            status.tip_block_number,
        )?;
        status.last_synced_at = chrono::Utc::now().timestamp();
        if mark_bulk_sync_completed {
            status.mark_bulk_sync_completed(status.tip_block_number);
        }
        store.set_sync_status(&status)?;
        Ok(status)
    }
}

fn checked_add_sync_total(label: &str, current: i64, delta: i64, block_number: i64) -> Result<i64> {
    current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "bulk build sync total overflow: field={} current={} delta={} block={}",
            label,
            current,
            delta,
            block_number
        )
    })
}

fn start_bulk_build_session_marker(
    store: &CkbadgerStore,
    run_id: &str,
    start_block: i64,
) -> Result<BulkBuildSessionMarker> {
    let marker = BulkBuildSessionMarker {
        run_id: run_id.to_string(),
        started_at: chrono::Utc::now().timestamp(),
        start_block,
    };
    store.set_bulk_build_session_marker(Some(&marker))?;
    Ok(marker)
}

fn flush_bulk_build_materialized_state(
    domain_store: &CkbadgerStore,
    append_store: &CkbadgerStore,
) -> Result<()> {
    append_store.flush_all_memtables().map_err(|e| {
        anyhow!(
            "failed to flush append-only bulk build memtables before sync status persistence: {}",
            e
        )
    })?;
    domain_store.flush_all_memtables().map_err(|e| {
        anyhow!(
            "failed to flush domain bulk build memtables before sync status persistence: {}",
            e
        )
    })?;
    Ok(())
}

#[derive(Default)]
struct CoreOwners {
    address: owners::address::AddressOwner,
    script: owners::script::ScriptOwner,
    token: owners::token::TokenOwner,
    dao: owners::dao::DaoOwner,
    fiber: owners::fiber::FiberOwner,
    object: owners::object::ObjectOwner,
}

impl CoreOwners {
    fn estimated_bytes_by_owner(&self) -> HashMap<String, u64> {
        HashMap::from([
            ("owner.address".to_string(), self.address.estimated_bytes()),
            ("owner.script".to_string(), self.script.estimated_bytes()),
            ("owner.token".to_string(), self.token.estimated_bytes()),
            ("owner.dao".to_string(), self.dao.estimated_bytes()),
            ("owner.fiber".to_string(), self.fiber.estimated_bytes()),
            ("owner.object".to_string(), self.object.estimated_bytes()),
        ])
    }

    #[cfg(test)]
    fn apply_tx(
        &mut self,
        tx: &facts::ResolvedTxFacts<'_>,
        ctx: &owners::ReducerContext<'_>,
    ) -> Result<()> {
        self.apply_tx_and_return_address_deltas(tx, ctx).map(|_| ())
    }

    #[cfg(test)]
    fn apply_tx_and_return_address_deltas(
        &mut self,
        tx: &facts::ResolvedTxFacts<'_>,
        ctx: &owners::ReducerContext<'_>,
    ) -> Result<FxHashMap<Vec<u8>, owners::address::AddressTxDelta>> {
        let address_deltas = self.address.apply_tx_with_deltas(tx, ctx)?;
        self.script.apply_tx(tx, ctx)?;
        self.token.apply_tx(tx, ctx)?;
        self.dao.apply_tx(tx, ctx)?;
        self.fiber.apply_tx(tx, ctx)?;
        self.object.apply_tx(tx, ctx)?;
        Ok(address_deltas)
    }

    fn materialize_all(&mut self, materializer: &mut materialize::Materializer<'_>) -> Result<()> {
        self.address.flush_sealed(materializer)?;
        self.script.flush_sealed(materializer)?;
        self.token.flush_sealed(materializer)?;
        self.dao.flush_sealed(materializer)?;
        self.fiber.flush_sealed(materializer)?;
        self.object.flush_sealed(materializer)?;

        self.address.materialize_final(materializer)?;
        self.script.materialize_final(materializer)?;
        self.token.materialize_final(materializer)?;
        self.dao.materialize_final(materializer)?;
        self.fiber.materialize_final(materializer)?;
        self.object.materialize_final(materializer)?;
        Ok(())
    }
}

#[derive(Default)]
struct ActivityStatsAccumulator {
    daily_stats: FxHashMap<String, DailyActivityStats>,
    daily_addrs: FxHashMap<String, FxHashSet<[u8; 32]>>,
    hourly_stats: FxHashMap<String, DailyActivityStats>,
    hourly_addrs: FxHashMap<String, FxHashSet<[u8; 32]>>,
}

impl ActivityStatsAccumulator {
    fn estimated_bytes(&self) -> u64 {
        crate::sync::bulk_build::accounting::hash_map_serialized_bytes(&self.daily_stats)
            + crate::sync::bulk_build::accounting::hash_map_bytes(
                &self.daily_addrs,
                |date, addrs| {
                    crate::sync::bulk_build::accounting::serialized_bytes(date)
                        + crate::sync::bulk_build::accounting::hash_set_serialized_bytes(addrs)
                },
            )
            + crate::sync::bulk_build::accounting::hash_map_serialized_bytes(&self.hourly_stats)
            + crate::sync::bulk_build::accounting::hash_map_bytes(
                &self.hourly_addrs,
                |hour, addrs| {
                    crate::sync::bulk_build::accounting::serialized_bytes(hour)
                        + crate::sync::bulk_build::accounting::hash_set_serialized_bytes(addrs)
                },
            )
    }

    /// Accumulate activity stats directly from in-memory bundles.
    /// Replaces the old `apply_history_rows` which deserialized bundles
    /// from bincode MaterializedRows — a serialize→deserialize roundtrip
    /// costing ~410ms/batch at steady state.
    ///
    /// Chrono cache: all txs in the same block share one timestamp, so we
    /// cache the formatted date/hour strings and only reformat on timestamp
    /// change (~47K format calls per batch instead of ~123K).
    fn apply_bundles(&mut self, bundles: &[TxActivityBundle]) -> Result<()> {
        let mut cached_ts = i64::MIN;
        let mut cached_date = String::new();
        let mut cached_hour = String::new();

        for bundle in bundles {
            if bundle.timestamp != cached_ts {
                cached_ts = bundle.timestamp;
                cached_date = ckbadger_common::block_date_from_ms(bundle.timestamp)
                    .format("%Y%m%d")
                    .to_string();
                cached_hour = ckbadger_common::block_datetime_from_ms(bundle.timestamp)
                    .format("%Y%m%d%H")
                    .to_string();
            }

            for owner in &bundle.owners {
                crate::db::BatchWriter::accumulate_owner_activity_stats(
                    bundle.is_cellbase,
                    owner,
                    self.daily_stats.entry(cached_date.clone()).or_default(),
                );
                crate::db::BatchWriter::accumulate_owner_activity_stats(
                    bundle.is_cellbase,
                    owner,
                    self.hourly_stats.entry(cached_hour.clone()).or_default(),
                );

                if !bundle.is_cellbase && owner.lock_hash.len() == 32 {
                    let mut lock_hash = [0u8; 32];
                    lock_hash.copy_from_slice(&owner.lock_hash);
                    self.daily_addrs
                        .entry(cached_date.clone())
                        .or_default()
                        .insert(lock_hash);
                    self.hourly_addrs
                        .entry(cached_hour.clone())
                        .or_default()
                        .insert(lock_hash);
                }
            }
        }

        Ok(())
    }

    fn build_rows(&self) -> Result<Vec<materialize::MaterializedRow>> {
        let mut rows = Vec::with_capacity(self.daily_stats.len() + self.hourly_stats.len());

        let mut daily_entries = self
            .daily_stats
            .iter()
            .map(|(date, stats)| (date.clone(), stats.clone()))
            .collect::<Vec<_>>();
        daily_entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (date, mut stats) in daily_entries {
            stats.unique_address_count = self
                .daily_addrs
                .get(&date)
                .map_or(Ok(0), |set| checked_unique_address_count(set.len(), &date))?;
            rows.push(materialize::MaterializedRow::new(
                CF_STATS_CHAIN,
                keys::encode_stats_key(keys::stats_prefix::ACTIVITY_DAILY, date.as_bytes()),
                bincode::serialize(&stats)?,
            ));
        }

        let mut hourly_entries = self
            .hourly_stats
            .iter()
            .map(|(hour, stats)| (hour.clone(), stats.clone()))
            .collect::<Vec<_>>();
        hourly_entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (hour, mut stats) in hourly_entries {
            stats.unique_address_count = self
                .hourly_addrs
                .get(&hour)
                .map_or(Ok(0), |set| checked_unique_address_count(set.len(), &hour))?;
            rows.push(materialize::MaterializedRow::new(
                CF_STATS_CHAIN,
                keys::encode_stats_key(keys::stats_prefix::ACTIVITY_HOURLY, hour.as_bytes()),
                bincode::serialize(&stats)?,
            ));
        }

        Ok(rows)
    }
}

fn checked_unique_address_count(len: usize, bucket: &str) -> Result<u32> {
    u32::try_from(len).map_err(|_| {
        anyhow!(
            "unique_address_count exceeds u32 range while building bulk activity stats: bucket={} len={}",
            bucket,
            len
        )
    })
}

/// Accumulates chain-level daily statistics during bulk-build.
///
/// Covers `DailyStats`, `DailyBlockStats`, block-time distribution, and
/// epoch-time distribution — the same data that `SyncBatch::finalize()`
/// writes during live sync.
#[derive(Default)]
#[allow(clippy::type_complexity)]
struct ChainStatsAccumulator {
    /// Per-day: (blocks, txs, cells_created, cells_consumed, capacity_transferred,
    ///           used_cap_created, used_cap_consumed, data_size_added, data_size_consumed)
    daily_stats: FxHashMap<chrono::NaiveDate, (i32, i32, i32, i32, i128, i128, i128, i64, i64)>,
    /// Per-day: (sum_compact_target, block_count, total_uncles)
    daily_block_stats: FxHashMap<chrono::NaiveDate, (i128, i32, i32)>,
    /// Last DAO field seen per day (for knowledge_size).
    daily_dao_fields: FxHashMap<chrono::NaiveDate, [u8; 32]>,
    /// Per-day block time accumulation: (sum_ms, count)
    daily_block_times: FxHashMap<chrono::NaiveDate, (i64, i32)>,
    /// Block time distribution buckets (seconds → count).
    block_time_dist: FxHashMap<i32, i32>,
    /// Epoch time distribution buckets (minutes → count).
    epoch_time_dist: FxHashMap<i32, i32>,
    /// Timestamp of the previous block (for inter-block time deltas).
    prev_timestamp_ms: Option<i64>,
    /// Previous epoch: (epoch_number, epoch_start_timestamp_ms).
    prev_epoch: Option<(i64, i64)>,
}

impl ChainStatsAccumulator {
    fn estimated_bytes(&self) -> u64 {
        // Rough estimate: ~100 bytes per entry across all maps
        let daily_count = self.daily_stats.len()
            + self.daily_block_stats.len()
            + self.daily_dao_fields.len()
            + self.daily_block_times.len();
        let dist_count = self.block_time_dist.len() + self.epoch_time_dist.len();
        (daily_count * 100 + dist_count * 16) as u64
    }

    /// Accumulate chain statistics from a batch of blocks.
    ///
    /// Requires the resolved tx facts to compute per-block consumed cell stats
    /// (occupied_capacity and data_size of consumed inputs).
    fn apply_blocks(
        &mut self,
        arena: &facts::FactsArena,
        resolved: &[facts::ResolvedTxFacts<'_>],
    ) -> Result<()> {
        for block in &arena.blocks {
            let block_date = ckbadger_common::block_date_from_ms(block.timestamp_ms);

            // --- DailyStats fields ---
            let mut cells_created: i32 = 0;
            let mut cells_consumed: i32 = 0;
            let mut capacity_transferred: i128 = 0;
            let mut used_cap_created: i128 = 0;
            let mut used_cap_consumed: i128 = 0;
            let mut data_size_added: i64 = 0;
            let mut data_size_consumed: i64 = 0;

            for tx in &resolved[block.tx_range.clone()] {
                let tx_cells_created = i32::try_from(tx.cells.len()).map_err(|_| {
                    anyhow!(
                        "chain stats: output count exceeds i32: block={} tx_index={}",
                        block.number,
                        tx.tx_index
                    )
                })?;
                cells_created += tx_cells_created;

                for cell in tx.cells.iter() {
                    used_cap_created += i128::from(cell.occupied_capacity);
                    data_size_added += cell.data_size as i64;
                }

                if tx.tx_index > 0 {
                    // Non-cellbase
                    let tx_consumed = i32::try_from(tx.resolved_inputs.len()).map_err(|_| {
                        anyhow!(
                            "chain stats: input count exceeds i32: block={} tx_index={}",
                            block.number,
                            tx.tx_index
                        )
                    })?;
                    cells_consumed += tx_consumed;
                    capacity_transferred += tx
                        .cells
                        .iter()
                        .map(|c| i128::from(c.capacity))
                        .sum::<i128>();

                    for input in &tx.resolved_inputs {
                        used_cap_consumed += i128::from(input.occupied_capacity);
                        data_size_consumed += input.data_size as i64;
                    }
                }
            }

            let entry = self.daily_stats.entry(block_date).or_default();
            entry.0 += 1; // blocks
            entry.1 += block.transactions_count; // txs
            entry.2 += cells_created;
            entry.3 += cells_consumed;
            entry.4 = entry.4.checked_add(capacity_transferred).ok_or_else(|| {
                anyhow!(
                    "chain stats: daily capacity_transferred overflow: date={} block={}",
                    block_date,
                    block.number
                )
            })?;
            entry.5 = entry.5.checked_add(used_cap_created).ok_or_else(|| {
                anyhow!(
                    "chain stats: daily used_cap_created overflow: date={} block={}",
                    block_date,
                    block.number
                )
            })?;
            entry.6 = entry.6.checked_add(used_cap_consumed).ok_or_else(|| {
                anyhow!(
                    "chain stats: daily used_cap_consumed overflow: date={} block={}",
                    block_date,
                    block.number
                )
            })?;
            entry.7 += data_size_added;
            entry.8 += data_size_consumed;

            // --- DAO field (last per day wins) ---
            self.daily_dao_fields.insert(block_date, block.dao);

            // --- DailyBlockStats ---
            let block_entry = self.daily_block_stats.entry(block_date).or_default();
            block_entry.0 += block.compact_target as i128;
            block_entry.1 += 1;
            block_entry.2 += block.uncles_count;

            // --- Inter-block time ---
            if let Some(prev_ts) = self.prev_timestamp_ms {
                let delta_ms = block.timestamp_ms - prev_ts;
                if delta_ms >= 0 {
                    let delta_seconds = delta_ms / 1000;
                    *self
                        .block_time_dist
                        .entry(crate::sync::dao_helpers::block_time_to_bucket(
                            delta_seconds,
                        ))
                        .or_default() += 1;

                    let bt_entry = self.daily_block_times.entry(block_date).or_insert((0, 0));
                    bt_entry.0 += delta_ms;
                    bt_entry.1 += 1;
                }
            }
            self.prev_timestamp_ms = Some(block.timestamp_ms);

            // --- Epoch time distribution ---
            if block.epoch_index == 0 && block.epoch_number > 0 {
                if let Some((prev_epoch_num, prev_start_ts)) = self.prev_epoch {
                    if prev_epoch_num == block.epoch_number - 1 {
                        let epoch_duration_minutes =
                            (block.timestamp_ms - prev_start_ts) as f64 / 60_000.0;
                        let bucket_minutes = epoch_duration_minutes.round() as i32;
                        *self.epoch_time_dist.entry(bucket_minutes).or_default() += 1;
                    }
                }
            }
            if block.epoch_index == 0 {
                self.prev_epoch = Some((block.epoch_number, block.timestamp_ms));
            }
        }
        Ok(())
    }

    /// Build sealed aggregate rows for `CF_STATS_CHAIN`.
    fn build_rows(&self) -> Result<Vec<materialize::MaterializedRow>> {
        let mut rows = Vec::new();

        // --- DailyStats (cumulative totals threaded forward) ---
        let mut sorted_dates: Vec<_> = self.daily_stats.keys().copied().collect();
        sorted_dates.sort();

        let mut cum_live: i64 = 0;
        let mut cum_dead: i64 = 0;
        let mut cum_all: i64 = 0;
        let mut cum_data_size: i64 = 0;

        for date in &sorted_dates {
            let (
                blocks,
                txs,
                created,
                consumed,
                capacity,
                occ_created,
                occ_consumed,
                data_added,
                data_consumed,
            ) = self.daily_stats[date];

            cum_live += (created - consumed) as i64;
            cum_dead += consumed as i64;
            cum_all += created as i64;
            cum_data_size += data_added - data_consumed;

            let knowledge_size = self
                .daily_dao_fields
                .get(date)
                .and_then(|dao| crate::db::writer::calculate_knowledge_size(dao));

            let avg_block_time_ms = self.daily_block_times.get(date).and_then(|(sum, count)| {
                if *count > 0 {
                    Some(*sum / *count as i64)
                } else {
                    None
                }
            });

            let stats = ckbadger_store::types::DailyStats {
                blocks_count: blocks,
                transactions_count: txs,
                cells_created: created,
                cells_consumed: consumed,
                capacity_transferred: capacity,
                used_capacity_created: occ_created,
                used_capacity_consumed: occ_consumed,
                total_live_cells: cum_live,
                total_dead_cells: cum_dead,
                total_all_cells: cum_all,
                total_data_size: cum_data_size,
                knowledge_size,
                avg_block_time_ms,
            };
            rows.push(materialize::MaterializedRow::new(
                CF_STATS_CHAIN,
                keys::encode_stats_key(
                    keys::stats_prefix::DAILY,
                    date.format("%Y%m%d").to_string().as_bytes(),
                ),
                bincode::serialize(&stats)?,
            ));
        }

        // --- DailyBlockStats ---
        let mut sorted_block_dates: Vec<_> = self.daily_block_stats.keys().copied().collect();
        sorted_block_dates.sort();
        for date in &sorted_block_dates {
            let (sum_target, count, uncles) = self.daily_block_stats[date];
            let avg_compact_target = if count > 0 {
                let avg_i64 = i64::try_from(sum_target / count as i128).map_err(|_| {
                    anyhow!(
                        "chain stats: daily avg compact target exceeds i64: date={} sum={} count={}",
                        date,
                        sum_target,
                        count
                    )
                })?;
                avg_i64 as f64
            } else {
                0.0
            };

            let avg_block_time_ms = self
                .daily_block_times
                .get(date)
                .and_then(|(sum, bt_count)| {
                    if *bt_count > 0 {
                        Some(*sum / *bt_count as i64)
                    } else {
                        None
                    }
                });

            let stats = ckbadger_store::types::DailyBlockStats {
                avg_compact_target,
                block_count: count,
                total_uncles: uncles,
                avg_block_time_ms,
            };
            rows.push(materialize::MaterializedRow::new(
                CF_STATS_CHAIN,
                keys::encode_stats_key(
                    keys::stats_prefix::DAILY_BLOCK,
                    date.format("%Y%m%d").to_string().as_bytes(),
                ),
                bincode::serialize(&stats)?,
            ));
        }

        // --- Block time distribution ---
        for (bucket, count) in &self.block_time_dist {
            rows.push(materialize::MaterializedRow::new(
                CF_STATS_CHAIN,
                keys::encode_stats_key(keys::stats_prefix::BLOCK_TIME_DIST, &bucket.to_be_bytes()),
                count.to_le_bytes().to_vec(),
            ));
        }

        // --- Epoch time distribution ---
        for (bucket, count) in &self.epoch_time_dist {
            rows.push(materialize::MaterializedRow::new(
                CF_STATS_CHAIN,
                keys::encode_stats_key(keys::stats_prefix::EPOCH_TIME_DIST, &bucket.to_be_bytes()),
                count.to_le_bytes().to_vec(),
            ));
        }

        Ok(rows)
    }
}

struct BulkBuildRuntimeState {
    interner: interner::IdentityInterner,
    sequencer: sequencer::BulkSequencer,
    owners: CoreOwners,
    activity_stats: ActivityStatsAccumulator,
    chain_stats: ChainStatsAccumulator,
    hodl_tracker: crate::db::writer::hodl_wave::HodlWaveTracker,
    cell_dist_tracker: crate::db::writer::cell_distribution::CellDistributionTracker,
    hodl_live_cells_by_lock: FxHashMap<crate::sync::types::InternId, i32>,
}

impl Default for BulkBuildRuntimeState {
    fn default() -> Self {
        Self {
            interner: interner::IdentityInterner::with_capacity(8192),
            sequencer: sequencer::BulkSequencer::default(),
            owners: CoreOwners::default(),
            activity_stats: ActivityStatsAccumulator::default(),
            chain_stats: ChainStatsAccumulator::default(),
            hodl_tracker: crate::db::writer::hodl_wave::HodlWaveTracker::new(),
            cell_dist_tracker: crate::db::writer::cell_distribution::CellDistributionTracker::new(),
            hodl_live_cells_by_lock: FxHashMap::default(),
        }
    }
}

impl BulkBuildRuntimeState {
    fn memory_breakdown_bytes(&self) -> HashMap<String, u64> {
        let mut breakdown = self.owners.estimated_bytes_by_owner();
        breakdown.insert("live_cells".to_string(), self.sequencer.live_cells_bytes());
        breakdown.insert("interner".to_string(), self.interner.estimated_bytes());
        breakdown.insert(
            "activity_stats".to_string(),
            self.activity_stats.estimated_bytes(),
        );
        breakdown.insert(
            "chain_stats".to_string(),
            self.chain_stats.estimated_bytes(),
        );
        breakdown.insert(
            "hodl_live_cells_by_lock".to_string(),
            crate::sync::bulk_build::accounting::hash_map_bytes(
                &self.hodl_live_cells_by_lock,
                |lock_hash_id, live_count| {
                    std::mem::size_of_val(lock_hash_id) as u64
                        + std::mem::size_of_val(live_count) as u64
                },
            ),
        );
        breakdown
    }

    fn apply_blocks(
        &mut self,
        blocks: &[binary_facts::RawCkbBlock],
        is_mainnet: bool,
        token_info_cache: &FxHashMap<Vec<u8>, (Option<String>, Option<u8>)>,
    ) -> Result<(BatchExecutionStats, BatchBuildTimings, PendingFlush)> {
        if blocks.is_empty() {
            return Ok((
                BatchExecutionStats::default(),
                BatchBuildTimings::default(),
                PendingFlush {
                    history_rows: Vec::new(),
                    sealed_rows: Vec::new(),
                },
            ));
        }

        let facts_started = Instant::now();
        let (arena, facts_breakdown) =
            binary_facts::build_bulk_facts_arena_from_raw_blocks(blocks, &self.interner)?;
        let facts_elapsed = facts_started.elapsed();

        // Snapshot interner for zero-copy reads during resolve/reduce phases.
        let frozen = self.interner.snapshot_for_reads();

        let resolve_started = Instant::now();
        let resolved = self.sequencer.resolve(&arena)?;
        let resolve_elapsed = resolve_started.elapsed();

        let tx_count = u64::try_from(arena.txs.len()).map_err(|_| {
            anyhow!(
                "bulk build tx count exceeds u64 range while applying block batch: txs={}",
                arena.txs.len()
            )
        })?;
        let cells_created = i64::try_from(arena.cells.len()).map_err(|_| {
            anyhow!(
                "bulk build created cell count exceeds i64 range while applying block batch: cells={}",
                arena.cells.len()
            )
        })?;
        let consumed_cells = i64::try_from(
            resolved
                .iter()
                .map(|tx| tx.resolved_inputs.len())
                .sum::<usize>(),
        )
        .map_err(|_| {
            anyhow!("bulk build consumed cell count exceeds i64 range while applying block batch")
        })?;
        let reduce_started = Instant::now();
        let last_block = arena
            .blocks
            .last()
            .ok_or_else(|| anyhow!("bulk build arena missing blocks for non-empty batch"))?;

        // Destructure self to split borrows for rayon::join overlap.
        let BulkBuildRuntimeState {
            owners,
            cell_dist_tracker,
            hodl_tracker,
            hodl_live_cells_by_lock,
            activity_stats,
            chain_stats,
            ..
        } = self;
        let ctx = owners::ReducerContext::new(&frozen);

        // 3-way parallel tree via nested rayon::join:
        //   LEFT:   history → activity_stats (activity_stats depends only on history output)
        //   MIDDLE: chain_stats (reads only immutable arena + resolved)
        //   RIGHT:  hodl → rayon::join(address+cell_dist, 5 independent reducers)
        //
        // Each branch captures disjoint &mut fields. Shared &arena/&resolved/&frozen are immutable.
        let (left_result, (mid_result, right_result)) = rayon::join(
            // LEFT: history materialization → activity stats accumulation
            || -> Result<(HistoryBuildResult, std::time::Duration, std::time::Duration)> {
                let history_started = Instant::now();
                let history =
                    build_history_rows(&arena, &resolved, &frozen, is_mainnet, token_info_cache)?;
                let history_elapsed = history_started.elapsed();

                // activity_stats depends only on history.activity_bundles, not on any reducer state.
                let activity_stats_started = Instant::now();
                activity_stats.apply_bundles(&history.activity_bundles)?;
                let activity_stats_elapsed = activity_stats_started.elapsed();

                Ok((history, history_elapsed, activity_stats_elapsed))
            },
            || {
                rayon::join(
                    // MIDDLE: chain-level daily statistics (reads only immutable arena + resolved)
                    || -> Result<()> {
                        chain_stats.apply_blocks(&arena, &resolved)
                    },
                    // RIGHT: all reducers (mutable state, disjoint from LEFT and MIDDLE)
                    || -> Result<(
                        Vec<materialize::MaterializedRow>,
                        Vec<materialize::MaterializedRow>,
                        std::time::Duration,
                    )> {
                        let hodl_sealed_rows = apply_hodl_tracker_batch_standalone(
                            hodl_tracker,
                            hodl_live_cells_by_lock,
                            &arena,
                            &resolved,
                        )?;

                        // Destructure owners to split borrows: address+cell_dist runs in
                        // parallel with 5 independent reducers that don't read address state.
                        let CoreOwners {
                            ref mut address,
                            ref mut script,
                            ref mut token,
                            ref mut dao,
                            ref mut fiber,
                            ref mut object,
                        } = *owners;

                        let address_started = Instant::now();

                        let (addr_result, reducers_result) = rayon::join(
                            // LEFT-inner: address reducer + cell_dist_tracker (serial, interdependent)
                            || -> Result<Vec<materialize::MaterializedRow>> {
                                let mut cell_dist_sealed_rows = Vec::new();
                                for block in &arena.blocks {
                                    let block_date =
                                        ckbadger_common::block_date_from_ms(block.timestamp_ms);
                                    cell_dist_tracker
                                        .record_block_date(block.number, block_date);

                                    for tx in &resolved[block.tx_range.clone()] {
                                        for input in &tx.resolved_inputs {
                                            cell_dist_tracker
                                                .cell_consumed(input.occupied_capacity)?;
                                        }
                                        for cell in tx.cells.iter() {
                                            cell_dist_tracker
                                                .cell_created(cell.occupied_capacity);
                                        }

                                        let address_deltas =
                                            address.apply_tx_with_deltas(tx, &ctx)?;
                                        apply_cell_dist_cohort_deltas(
                                            cell_dist_tracker,
                                            address.balances(),
                                            &address_deltas,
                                            tx,
                                        )?;
                                    }

                                    if let Some((snapshot_date, snapshot)) =
                                        cell_dist_tracker.maybe_snapshot(block_date)
                                    {
                                        let date_str =
                                            snapshot_date.format("%Y%m%d").to_string();
                                        let cohort = cell_dist_tracker.cohort_snapshot();
                                        cell_dist_sealed_rows.push(
                                            materialize::MaterializedRow::new(
                                                CF_STATS_HODL,
                                                keys::encode_stats_key(
                                                    keys::stats_prefix::CELL_DISTRIBUTION,
                                                    date_str.as_bytes(),
                                                ),
                                                bincode::serialize(&snapshot)?,
                                            ),
                                        );
                                        cell_dist_sealed_rows.push(
                                            materialize::MaterializedRow::new(
                                                CF_STATS_HODL,
                                                keys::encode_stats_key(
                                                    keys::stats_prefix::ADDR_COHORT,
                                                    date_str.as_bytes(),
                                                ),
                                                bincode::serialize(&cohort)?,
                                            ),
                                        );
                                    }
                                }
                                Ok(cell_dist_sealed_rows)
                            },
                            // RIGHT-inner: 5 independent reducers via nested rayon::join.
                            // Each reads immutable ResolvedTxFacts and writes only its own state.
                            || -> Result<()> {
                                let (r_left, r_right) = rayon::join(
                                    || -> Result<()> {
                                        for block in &arena.blocks {
                                            for tx in &resolved[block.tx_range.clone()] {
                                                script.apply_tx(tx, &ctx)?;
                                            }
                                        }
                                        for block in &arena.blocks {
                                            for tx in &resolved[block.tx_range.clone()] {
                                                token.apply_tx(tx, &ctx)?;
                                            }
                                        }
                                        Ok(())
                                    },
                                    || -> Result<()> {
                                        for block in &arena.blocks {
                                            for tx in &resolved[block.tx_range.clone()] {
                                                dao.apply_tx(tx, &ctx)?;
                                            }
                                            dao.record_block(block)?;
                                        }
                                        let (r_fiber, r_object) = rayon::join(
                                            || -> Result<()> {
                                                for block in &arena.blocks {
                                                    for tx in
                                                        &resolved[block.tx_range.clone()]
                                                    {
                                                        fiber.apply_tx(tx, &ctx)?;
                                                    }
                                                }
                                                Ok(())
                                            },
                                            || -> Result<()> {
                                                for block in &arena.blocks {
                                                    for tx in
                                                        &resolved[block.tx_range.clone()]
                                                    {
                                                        object.apply_tx(tx, &ctx)?;
                                                    }
                                                }
                                                Ok(())
                                            },
                                        );
                                        r_fiber?;
                                        r_object?;
                                        Ok(())
                                    },
                                );
                                r_left?;
                                r_right?;
                                Ok(())
                            },
                        );
                        let cell_dist_sealed_rows = addr_result?;
                        reducers_result?;

                        let address_elapsed = address_started.elapsed();
                        Ok((hodl_sealed_rows, cell_dist_sealed_rows, address_elapsed))
                    },
                )
            },
        );
        let (history, history_elapsed, activity_stats_elapsed) = left_result?;
        mid_result?;
        let (hodl_sealed_rows, cell_dist_sealed_rows, address_elapsed) = right_result?;

        // Post-overlap: activity deltas need both history + object reducer done.
        owners
            .object
            .apply_object_activity_count_deltas(&history.object_activity_count_deltas)?;
        owners
            .object
            .apply_identity_activity_count_deltas(&history.identity_activity_count_deltas)?;
        let reduce_elapsed = reduce_started.elapsed();

        // Collect all sealed rows into a single vec for the pending flush.
        let mut all_sealed = hodl_sealed_rows;
        all_sealed.extend(cell_dist_sealed_rows);

        let pending = PendingFlush {
            history_rows: history.rows,
            sealed_rows: all_sealed,
        };

        let timings = BatchBuildTimings {
            facts_ms: facts_elapsed.as_secs_f64() * 1000.0,
            facts_breakdown,
            resolve_ms: resolve_elapsed.as_secs_f64() * 1000.0,
            reduce_ms: reduce_elapsed.as_secs_f64() * 1000.0,
            history_ms: history_elapsed.as_secs_f64() * 1000.0,
            address_reduce_ms: address_elapsed.as_secs_f64() * 1000.0,
            activity_stats_ms: activity_stats_elapsed.as_secs_f64() * 1000.0,
        };

        Ok((BatchExecutionStats {
            last_block_number: Some(last_block.number),
            last_block_hash: Some(last_block.hash.to_vec()),
            block_count: u64::try_from(arena.blocks.len()).map_err(|_| {
                anyhow!(
                    "bulk build block count exceeds u64 range while applying block batch: blocks={}",
                    arena.blocks.len()
                )
            })?,
            tx_count,
            cells_created,
            cells_consumed: consumed_cells,
        }, timings, pending))
    }

    /// Apply blocks from hex-based RPC fixtures (used by test helpers and integration tests).
    fn apply_blocks_hex(
        &mut self,
        blocks: &[BlockResponseWithCycles],
        is_mainnet: bool,
        token_info_cache: &FxHashMap<Vec<u8>, (Option<String>, Option<u8>)>,
    ) -> Result<(BatchExecutionStats, BatchBuildTimings, PendingFlush)> {
        if blocks.is_empty() {
            return Ok((
                BatchExecutionStats::default(),
                BatchBuildTimings::default(),
                PendingFlush {
                    history_rows: Vec::new(),
                    sealed_rows: Vec::new(),
                },
            ));
        }
        let facts_started = Instant::now();
        let (arena, facts_breakdown) =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(blocks, &self.interner)?;
        let facts_elapsed = facts_started.elapsed();
        let frozen = self.interner.snapshot_for_reads();
        let resolve_started = Instant::now();
        let resolved = self.sequencer.resolve(&arena)?;
        let resolve_elapsed = resolve_started.elapsed();

        let tx_count = u64::try_from(arena.txs.len()).map_err(|_| {
            anyhow!(
                "bulk build tx count exceeds u64 range while applying block batch: txs={}",
                arena.txs.len()
            )
        })?;
        let cells_created = i64::try_from(arena.cells.len()).map_err(|_| {
            anyhow!(
                "bulk build created cell count exceeds i64 range: cells={}",
                arena.cells.len()
            )
        })?;
        let consumed_cells = i64::try_from(
            resolved
                .iter()
                .map(|tx| tx.resolved_inputs.len())
                .sum::<usize>(),
        )
        .map_err(|_| anyhow!("bulk build consumed cell count exceeds i64 range"))?;
        let reduce_started = Instant::now();
        let last_block = arena
            .blocks
            .last()
            .ok_or_else(|| anyhow!("bulk build arena missing blocks for non-empty batch"))?;
        let BulkBuildRuntimeState {
            owners,
            cell_dist_tracker,
            hodl_tracker,
            hodl_live_cells_by_lock,
            activity_stats,
            chain_stats,
            ..
        } = self;
        let ctx = owners::ReducerContext::new(&frozen);

        // 3-way parallel tree (same structure as apply_blocks):
        //   LEFT:   history → activity_stats
        //   MIDDLE: chain_stats
        //   RIGHT:  hodl → rayon::join(address+cell_dist, 5 independent reducers)
        let (left_result, (mid_result, right_result)) = rayon::join(
            || -> Result<(HistoryBuildResult, std::time::Duration, std::time::Duration)> {
                let history_started = Instant::now();
                let history =
                    build_history_rows(&arena, &resolved, &frozen, is_mainnet, token_info_cache)?;
                let history_elapsed = history_started.elapsed();
                let activity_stats_started = Instant::now();
                activity_stats.apply_bundles(&history.activity_bundles)?;
                let activity_stats_elapsed = activity_stats_started.elapsed();
                Ok((history, history_elapsed, activity_stats_elapsed))
            },
            || {
                rayon::join(
                || -> Result<()> { chain_stats.apply_blocks(&arena, &resolved) },
                || -> Result<(Vec<materialize::MaterializedRow>, Vec<materialize::MaterializedRow>, std::time::Duration)> {
                    let hodl_sealed_rows = apply_hodl_tracker_batch_standalone(hodl_tracker, hodl_live_cells_by_lock, &arena, &resolved)?;
                    let CoreOwners { ref mut address, ref mut script, ref mut token, ref mut dao, ref mut fiber, ref mut object } = *owners;
                    let address_started = Instant::now();
                    let (addr_result, reducers_result) = rayon::join(
                        || -> Result<Vec<materialize::MaterializedRow>> {
                            let mut cell_dist_sealed_rows = Vec::new();
                            for block in &arena.blocks {
                                let block_date = ckbadger_common::block_date_from_ms(block.timestamp_ms);
                                cell_dist_tracker.record_block_date(block.number, block_date);
                                for tx in &resolved[block.tx_range.clone()] {
                                    for input in &tx.resolved_inputs { cell_dist_tracker.cell_consumed(input.occupied_capacity)?; }
                                    for cell in tx.cells.iter() { cell_dist_tracker.cell_created(cell.occupied_capacity); }
                                    let address_deltas = address.apply_tx_with_deltas(tx, &ctx)?;
                                    apply_cell_dist_cohort_deltas(cell_dist_tracker, address.balances(), &address_deltas, tx)?;
                                }
                                if let Some((snapshot_date, snapshot)) = cell_dist_tracker.maybe_snapshot(block_date) {
                                    let date_str = snapshot_date.format("%Y%m%d").to_string();
                                    let cohort = cell_dist_tracker.cohort_snapshot();
                                    cell_dist_sealed_rows.push(materialize::MaterializedRow::new(ckbadger_store::CF_STATS_HODL, ckbadger_store::keys::encode_stats_key(ckbadger_store::keys::stats_prefix::CELL_DISTRIBUTION, date_str.as_bytes()), bincode::serialize(&snapshot)?));
                                    cell_dist_sealed_rows.push(materialize::MaterializedRow::new(ckbadger_store::CF_STATS_HODL, ckbadger_store::keys::encode_stats_key(ckbadger_store::keys::stats_prefix::ADDR_COHORT, date_str.as_bytes()), bincode::serialize(&cohort)?));
                                }
                            }
                            Ok(cell_dist_sealed_rows)
                        },
                        || -> Result<()> {
                            let (r_left, r_right) = rayon::join(
                                || -> Result<()> { for block in &arena.blocks { for tx in &resolved[block.tx_range.clone()] { script.apply_tx(tx, &ctx)?; } } for block in &arena.blocks { for tx in &resolved[block.tx_range.clone()] { token.apply_tx(tx, &ctx)?; } } Ok(()) },
                                || -> Result<()> { for block in &arena.blocks { for tx in &resolved[block.tx_range.clone()] { dao.apply_tx(tx, &ctx)?; } dao.record_block(block)?; } let (r_fiber, r_object) = rayon::join(|| -> Result<()> { for block in &arena.blocks { for tx in &resolved[block.tx_range.clone()] { fiber.apply_tx(tx, &ctx)?; } } Ok(()) }, || -> Result<()> { for block in &arena.blocks { for tx in &resolved[block.tx_range.clone()] { object.apply_tx(tx, &ctx)?; } } Ok(()) }); r_fiber?; r_object?; Ok(()) },
                            );
                            r_left?; r_right?; Ok(())
                        },
                    );
                    let cell_dist_sealed_rows = addr_result?;
                    reducers_result?;
                    let address_elapsed = address_started.elapsed();
                    Ok((hodl_sealed_rows, cell_dist_sealed_rows, address_elapsed))
                },
            )
            },
        );
        let (history, history_elapsed, activity_stats_elapsed) = left_result?;
        mid_result?;
        let (hodl_sealed_rows, cell_dist_sealed_rows, address_elapsed) = right_result?;
        owners
            .object
            .apply_object_activity_count_deltas(&history.object_activity_count_deltas)?;
        owners
            .object
            .apply_identity_activity_count_deltas(&history.identity_activity_count_deltas)?;
        let reduce_elapsed = reduce_started.elapsed();
        let mut all_sealed = hodl_sealed_rows;
        all_sealed.extend(cell_dist_sealed_rows);
        Ok((
            BatchExecutionStats {
                last_block_number: Some(last_block.number),
                last_block_hash: Some(last_block.hash.to_vec()),
                block_count: u64::try_from(arena.blocks.len()).unwrap_or(0),
                tx_count,
                cells_created,
                cells_consumed: consumed_cells,
            },
            BatchBuildTimings {
                facts_ms: facts_elapsed.as_secs_f64() * 1000.0,
                facts_breakdown,
                resolve_ms: resolve_elapsed.as_secs_f64() * 1000.0,
                reduce_ms: reduce_elapsed.as_secs_f64() * 1000.0,
                history_ms: history_elapsed.as_secs_f64() * 1000.0,
                address_reduce_ms: address_elapsed.as_secs_f64() * 1000.0,
                activity_stats_ms: activity_stats_elapsed.as_secs_f64() * 1000.0,
            },
            PendingFlush {
                history_rows: history.rows,
                sealed_rows: all_sealed,
            },
        ))
    }

    /// Finalize all in-memory state to RocksDB. Used by test helpers.
    /// Production code uses the decomposed sub-phase sequence in
    /// `run_bulk_stage_until_pipeline_handoff` which reports finalize
    /// progress via `BulkBuildPerfStats` atomics.
    fn finalize(
        self,
        domain_store: &CkbadgerStore,
        materializer: &mut materialize::Materializer<'_>,
    ) -> Result<()> {
        let prepared_finalize = self.prepare_finalize_artifacts()?;
        let BulkBuildRuntimeState {
            owners,
            hodl_tracker,
            cell_dist_tracker,
            ..
        } = self;

        materializer.stream_sealed_aggregate_rows(&prepared_finalize.activity_sealed_rows)?;
        materializer.stream_sealed_aggregate_rows(&prepared_finalize.chain_sealed_rows)?;
        materializer.materialize_final_snapshot(&prepared_finalize.final_snapshot_rows)?;

        let mut owners = owners;
        owners.materialize_all(materializer)?;

        let mut meta_batch = ckbadger_store::batch::StoreBatch::new(domain_store);
        meta_batch.put_hodl_tracker_state(&hodl_tracker.to_state());
        meta_batch.put_cell_dist_tracker_state(&cell_dist_tracker.to_state());
        if !meta_batch.is_empty() {
            meta_batch.commit()?;
        }
        Ok(())
    }

    fn prepare_finalize_artifacts(&self) -> Result<PreparedFinalizeArtifacts> {
        let frozen = self.interner.snapshot_for_reads();
        Ok(PreparedFinalizeArtifacts {
            activity_sealed_rows: self.activity_stats.build_rows()?,
            chain_sealed_rows: self.chain_stats.build_rows()?,
            final_snapshot_rows: build_final_snapshot_rows(&self.sequencer, &frozen)?,
        })
    }
}

/// Standalone hodl tracker batch processing. Extracted from `BulkBuildRuntimeState`
/// to allow split borrows in `rayon::join` (hodl fields borrowed separately from other fields).
fn apply_hodl_tracker_batch_standalone(
    hodl_tracker: &mut crate::db::writer::hodl_wave::HodlWaveTracker,
    hodl_live_cells_by_lock: &mut FxHashMap<crate::sync::types::InternId, i32>,
    arena: &facts::FactsArena,
    resolved: &[facts::ResolvedTxFacts<'_>],
) -> Result<Vec<materialize::MaterializedRow>> {
    if arena.txs.len() != resolved.len() {
        bail!(
            "bulk build hodl tracker tx count mismatch: facts_txs={} resolved_txs={}",
            arena.txs.len(),
            resolved.len()
        );
    }

    let mut sealed_rows = Vec::new();
    for block in &arena.blocks {
        let block_date = ckbadger_common::block_date_from_ms(block.timestamp_ms);
        hodl_tracker.record_block_date(block.number, block_date);

        for tx in &resolved[block.tx_range.clone()] {
            for input in &tx.resolved_inputs {
                update_hodl_holder_count(
                    hodl_tracker,
                    hodl_live_cells_by_lock,
                    input.lock_script_hash_id,
                    -1,
                    tx,
                )?;
                hodl_tracker.cell_consumed(input.created_at_block, input.capacity)?;
            }
            for cell in tx.cells.iter() {
                update_hodl_holder_count(
                    hodl_tracker,
                    hodl_live_cells_by_lock,
                    cell.lock_script_hash_id,
                    1,
                    tx,
                )?;
                hodl_tracker.cell_created(block_date, cell.capacity);
            }
        }

        if let Some((snapshot_date, snapshot)) = hodl_tracker.maybe_snapshot(block_date) {
            let date_str = snapshot_date.format("%Y%m%d").to_string();
            sealed_rows.push(materialize::MaterializedRow::new(
                CF_STATS_HODL,
                keys::encode_stats_key(keys::stats_prefix::HODL_WAVE, date_str.as_bytes()),
                bincode::serialize(&snapshot)?,
            ));
        }
    }

    Ok(sealed_rows)
}

fn update_hodl_holder_count(
    hodl_tracker: &mut crate::db::writer::hodl_wave::HodlWaveTracker,
    hodl_live_cells_by_lock: &mut FxHashMap<crate::sync::types::InternId, i32>,
    lock_hash_id: crate::sync::types::InternId,
    delta: i32,
    tx: &facts::ResolvedTxFacts<'_>,
) -> Result<()> {
    let old_live = hodl_live_cells_by_lock
        .get(&lock_hash_id)
        .copied()
        .unwrap_or(0);
    let new_live = old_live.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "hodl live cell count overflow: tx=0x{} block={} tx_index={} lock_hash_id={:?} old_live={} delta={}",
            hex::encode(tx.tx_hash),
            tx.block_number,
            tx.tx_index,
            lock_hash_id,
            old_live,
            delta
        )
    })?;
    if new_live < 0 {
        bail!(
            "hodl live cell count underflow: tx=0x{} block={} tx_index={} lock_hash_id={:?} old_live={} delta={}",
            hex::encode(tx.tx_hash),
            tx.block_number,
            tx.tx_index,
            lock_hash_id,
            old_live,
            delta
        );
    }

    hodl_tracker.update_holder_count(old_live, new_live)?;
    if new_live == 0 {
        hodl_live_cells_by_lock.remove(&lock_hash_id);
    } else {
        hodl_live_cells_by_lock.insert(lock_hash_id, new_live);
    }
    Ok(())
}

fn apply_cell_dist_cohort_deltas(
    tracker: &mut crate::db::writer::cell_distribution::CellDistributionTracker,
    balances: &FxHashMap<Vec<u8>, AddressBalance>,
    deltas: &FxHashMap<Vec<u8>, owners::address::AddressTxDelta>,
    tx: &facts::ResolvedTxFacts<'_>,
) -> Result<()> {
    for (lock_hash, delta) in deltas {
        let balance = balances.get(lock_hash).ok_or_else(|| {
            anyhow!(
                "missing address balance after applying tx deltas for cell distribution tracker: lock_hash=0x{}, block={}, tx=0x{}, tx_index={}",
                hex::encode(lock_hash),
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        tracker
            .apply_cohort_delta(
                balance.first_seen_block,
                delta.used_capacity_delta,
                delta.balance_delta,
            )
            .map_err(|e| {
                anyhow!(
                    "failed to apply cell distribution cohort delta: lock_hash=0x{}, first_seen_block={}, block={}, tx=0x{}, tx_index={}, error={}",
                    hex::encode(lock_hash),
                    balance.first_seen_block,
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    e
                )
            })?;
    }

    Ok(())
}

struct HistoryBuildResult {
    rows: Vec<materialize::MaterializedRow>,
    object_activity_count_deltas: FxHashMap<Vec<u8>, i64>,
    identity_activity_count_deltas: FxHashMap<Vec<u8>, i64>,
    activity_bundles: Vec<ckbadger_store::types::TxActivityBundle>,
}

struct BlockHistoryRows {
    rows: Vec<materialize::MaterializedRow>,
    object_activity_count_deltas: FxHashMap<Vec<u8>, i64>,
    identity_activity_count_deltas: FxHashMap<Vec<u8>, i64>,
    activity_bundles: Vec<ckbadger_store::types::TxActivityBundle>,
}

#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct CoreOwnerStateSnapshot {
    pub address_balances: HashMap<Vec<u8>, AddressBalance>,
    pub script_infos: HashMap<Vec<u8>, ScriptInfo>,
    pub token_state: owners::token::TokenStateSnapshot,
    pub dao_state: owners::dao::DaoStateSnapshot,
    pub object_state: owners::object::ObjectStateSnapshot,
}

#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct BulkArtifactSnapshot {
    pub report: materialize::MaterializationReport,
    pub sync_status: SyncStatus,
    pub bulk_build_session_marker: Option<BulkBuildSessionMarker>,
    pub hodl_tracker_state: Option<HodlTrackerState>,
    pub cell_dist_tracker_state: Option<CellDistributionTrackerState>,
    pub hodl_waves: HashMap<String, DailyHodlWave>,
    pub cell_distribution_snapshots: HashMap<String, DailyCellDistribution>,
    pub address_cohort_snapshots: HashMap<String, DailyAddressCohort>,
    pub block_headers: HashMap<i64, CachedBlockHeader>,
    pub block_numbers_by_hash: HashMap<Vec<u8>, i64>,
    pub txs_by_hash: HashMap<Vec<u8>, (i64, i32, TxIndexEntry)>,
    pub activity_bundles: HashMap<Vec<u8>, TxActivityBundle>,
    pub daily_activity_stats: HashMap<String, DailyActivityStats>,
    pub hourly_activity_stats: HashMap<String, DailyActivityStats>,
    pub dao_daily_snapshots: HashMap<String, DaoDailySnapshot>,
    pub latest_dao_statistics: Option<DaoLatestStatistics>,
    pub dao_top_depositors: Option<DaoTopDepositors>,
    pub script_daily_deltas: HashMap<(Vec<u8>, bool), HashMap<u32, ScriptDailyDelta>>,
    pub cell_payloads: HashMap<Vec<u8>, LiveCellInfo>,
    pub live_cells: HashMap<Vec<u8>, i64>,
    pub consumed_cells: HashMap<Vec<u8>, ConsumedCellMeta>,
    pub cell_by_lock: HashSet<Vec<u8>>,
    pub cell_by_type: HashSet<Vec<u8>>,
    pub cell_by_lock_code: HashSet<Vec<u8>>,
    pub cell_by_type_code: HashSet<Vec<u8>>,
    pub cell_by_data_hash: HashSet<Vec<u8>>,
    pub core: CoreOwnerStateSnapshot,
}

#[doc(hidden)]
pub(crate) fn materialize_core_owner_state_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<CoreOwnerStateSnapshot> {
    Ok(materialize_bulk_artifacts_for_test(blocks)?.core)
}

#[doc(hidden)]
pub(crate) fn materialize_bulk_artifacts_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<BulkArtifactSnapshot> {
    materialize_bulk_artifacts_from_block_batches_for_test_impl(&[blocks])
}

#[doc(hidden)]
pub(crate) fn materialize_bulk_artifacts_from_batches_for_test(
    batches: &[Vec<BlockResponseWithCycles>],
) -> Result<BulkArtifactSnapshot> {
    let batch_slices = batches.iter().map(Vec::as_slice).collect::<Vec<_>>();
    materialize_bulk_artifacts_from_block_batches_for_test_impl(&batch_slices)
}

#[doc(hidden)]
pub(crate) fn materialize_bulk_stage_for_test(
    blocks: &[BlockResponseWithCycles],
    chain_tip: u64,
    bulk_sync_threshold: u64,
) -> Result<BulkArtifactSnapshot> {
    run_bulk_stage_test_session(
        blocks,
        chain_tip,
        bulk_sync_threshold,
        "bulk-build-stage-handoff",
        |domain_store, append_store, state| {
            collect_bulk_artifact_snapshot(
                domain_store,
                append_store,
                state.report,
                state.sync_status,
            )
        },
    )
}

#[doc(hidden)]
pub(crate) fn materialize_bulk_stage_then_complete_sync_status_for_test(
    blocks: &[BlockResponseWithCycles],
    chain_tip: u64,
    bulk_sync_threshold: u64,
) -> Result<SyncStatus> {
    run_bulk_stage_test_session(
        blocks,
        chain_tip,
        bulk_sync_threshold,
        "bulk-build-stage-completion",
        |domain_store, _append_store, state| {
            apply_pipeline_sync_status_for_test(domain_store, &blocks[state.processed_blocks..])?;

            let bulk_sync_allowed = std::sync::atomic::AtomicBool::new(true);
            let was_bulk_sync_active = std::sync::atomic::AtomicBool::new(false);
            finalize_bulk_stage_handoff_state(&bulk_sync_allowed, &was_bulk_sync_active);
            if take_bulk_sync_completion_transition(&was_bulk_sync_active, false) {
                persist_bulk_sync_completion_status(domain_store, chain_tip)?;
            }

            domain_store.get_sync_status()
        },
    )
}

struct BulkStageTestState {
    processed_blocks: usize,
    report: materialize::MaterializationReport,
    sync_status: SyncStatus,
}

fn run_bulk_stage_test_session<T, F>(
    blocks: &[BlockResponseWithCycles],
    chain_tip: u64,
    bulk_sync_threshold: u64,
    temp_root_label: &str,
    finish: F,
) -> Result<T>
where
    F: FnOnce(&CkbadgerStore, &CkbadgerStore, BulkStageTestState) -> Result<T>,
{
    let mut runtime = BulkBuildRuntimeState::default();
    let mut sync_totals = BulkBuildSyncTotals::default();

    let root = unique_temp_test_dir(temp_root_label);
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let snapshot = {
        let domain_store = Arc::new(CkbadgerStore::open_domain(&domain_path)?);
        let append_store = Arc::new(CkbadgerStore::open_append_only(&append_path)?);
        domain_store.update_sync_status(|status| status.init_sync_start(0, true))?;
        start_bulk_build_session_marker(domain_store.as_ref(), "bulk-build-test-session", 0)?;
        let mut materializer =
            materialize::Materializer::new(domain_store.as_ref(), append_store.as_ref());
        let mut current_block = 0u64;
        let mut processed_blocks = 0usize;

        for (block_idx, block) in blocks.iter().enumerate() {
            if chain_tip.saturating_sub(current_block) <= bulk_sync_threshold {
                break;
            }

            let (batch_stats, _timings, pending) = runtime.apply_blocks_hex(
                std::slice::from_ref(block),
                true,
                &FxHashMap::default(),
            )?;
            materializer.stream_history_rows(&pending.history_rows)?;
            materializer.stream_sealed_aggregate_rows(&pending.sealed_rows)?;
            sync_totals.record_batch(&batch_stats)?;
            let last_block_number = batch_stats
                .last_block_number
                .ok_or_else(|| anyhow!("bulk stage test batch missing last block number"))?;
            current_block = u64::try_from(last_block_number).map_err(|_| {
                anyhow!(
                    "bulk stage test batch returned negative last block number: last_block_number={}",
                    last_block_number
                )
            })?;
            processed_blocks = block_idx + 1;
        }

        runtime.finalize(domain_store.as_ref(), &mut materializer)?;
        flush_bulk_build_materialized_state(domain_store.as_ref(), append_store.as_ref())?;
        let sync_status = sync_totals.finalize_success(domain_store.as_ref(), false)?;
        crate::db::writer::BatchWriter::new(domain_store.clone(), append_store.clone())
            .refresh_latest_dao_statistics()?;
        domain_store.clear_bulk_build_session_marker()?;
        let report = materializer.finish();

        finish(
            domain_store.as_ref(),
            append_store.as_ref(),
            BulkStageTestState {
                processed_blocks,
                report,
                sync_status,
            },
        )?
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(snapshot)
}

fn apply_pipeline_sync_status_for_test(
    store: &CkbadgerStore,
    blocks: &[BlockResponseWithCycles],
) -> Result<()> {
    if blocks.is_empty() {
        return Ok(());
    }

    let (parsed_blocks, all_tx_data, _) = super::batch::parse_blocks_parallel(blocks)?;
    let last_block = parsed_blocks.last().ok_or_else(|| {
        anyhow!("pipeline completion test helper received non-empty blocks without parsed blocks")
    })?;
    let tx_count = i64::try_from(all_tx_data.len()).map_err(|_| {
        anyhow!(
            "pipeline completion test helper tx count exceeds i64 range: txs={}",
            all_tx_data.len()
        )
    })?;
    let cells_created = i64::try_from(all_tx_data.iter().map(|tx| tx.cells.len()).sum::<usize>())
        .map_err(|_| {
        anyhow!("pipeline completion test helper created cell count exceeds i64 range")
    })?;
    let cells_consumed = i64::try_from(
        all_tx_data
            .iter()
            .filter(|tx| !tx.is_cellbase)
            .map(|tx| tx.inputs.len())
            .sum::<usize>(),
    )
    .map_err(|_| {
        anyhow!("pipeline completion test helper consumed cell count exceeds i64 range")
    })?;

    let mut status = store.get_sync_status()?;
    status.tip_block_number = last_block.number;
    status.tip_block_hash = last_block.hash.clone();
    status.total_transactions = checked_add_sync_total(
        "total_transactions",
        status.total_transactions,
        tx_count,
        last_block.number,
    )?;
    status.total_cells_created = checked_add_sync_total(
        "total_cells_created",
        status.total_cells_created,
        cells_created,
        last_block.number,
    )?;
    status.total_cells_consumed = checked_add_sync_total(
        "total_cells_consumed",
        status.total_cells_consumed,
        cells_consumed,
        last_block.number,
    )?;
    status.last_synced_at = chrono::Utc::now().timestamp();
    store.set_sync_status(&status)
}

fn materialize_bulk_artifacts_from_block_batches_for_test_impl(
    block_batches: &[&[BlockResponseWithCycles]],
) -> Result<BulkArtifactSnapshot> {
    let mut runtime = BulkBuildRuntimeState::default();
    let mut sync_totals = BulkBuildSyncTotals::default();

    let root = unique_temp_test_dir("bulk-build-core-owners");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let snapshot = {
        let domain_store = Arc::new(CkbadgerStore::open_domain(&domain_path)?);
        let append_store = Arc::new(CkbadgerStore::open_append_only(&append_path)?);
        domain_store.update_sync_status(|status| status.init_sync_start(0, true))?;
        start_bulk_build_session_marker(domain_store.as_ref(), "bulk-build-test-session", 0)?;
        let mut materializer =
            materialize::Materializer::new(domain_store.as_ref(), append_store.as_ref());
        for batch in block_batches {
            let (batch_stats, _timings, pending) =
                runtime.apply_blocks_hex(batch, true, &FxHashMap::default())?;
            materializer.stream_history_rows(&pending.history_rows)?;
            materializer.stream_sealed_aggregate_rows(&pending.sealed_rows)?;
            sync_totals.record_batch(&batch_stats)?;
        }
        runtime.finalize(domain_store.as_ref(), &mut materializer)?;
        flush_bulk_build_materialized_state(domain_store.as_ref(), append_store.as_ref())?;
        let sync_status = sync_totals.finalize_success(domain_store.as_ref(), true)?;
        crate::db::writer::BatchWriter::new(domain_store.clone(), append_store.clone())
            .refresh_latest_dao_statistics()?;
        domain_store.clear_bulk_build_session_marker()?;
        let report = materializer.finish();
        collect_bulk_artifact_snapshot(
            domain_store.as_ref(),
            append_store.as_ref(),
            report,
            sync_status,
        )?
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(snapshot)
}

fn collect_bulk_artifact_snapshot(
    domain_store: &CkbadgerStore,
    append_store: &CkbadgerStore,
    report: materialize::MaterializationReport,
    sync_status: SyncStatus,
) -> Result<BulkArtifactSnapshot> {
    let core = collect_core_owner_state_snapshot(domain_store)?;
    let (block_headers, block_numbers_by_hash, txs_by_hash, activity_bundles) =
        collect_history_snapshot(domain_store)?;
    let (daily_activity_stats, hourly_activity_stats) =
        collect_activity_stats_snapshot(domain_store)?;
    let (dao_daily_snapshots, latest_dao_statistics, dao_top_depositors) =
        collect_dao_stats_snapshot(domain_store)?;
    let script_daily_deltas = collect_script_daily_deltas_snapshot(domain_store)?;
    let (hodl_waves, cell_distribution_snapshots, address_cohort_snapshots) =
        collect_hodl_stats_snapshot(domain_store)?;
    let (
        cell_payloads,
        live_cells,
        consumed_cells,
        cell_by_lock,
        cell_by_type,
        cell_by_lock_code,
        cell_by_type_code,
        cell_by_data_hash,
    ) = collect_cell_snapshot(domain_store, append_store)?;
    let bulk_build_session_marker = domain_store.get_bulk_build_session_marker()?;
    let hodl_tracker_state = domain_store.get_hodl_tracker_state()?;
    let cell_dist_tracker_state = domain_store.get_cell_dist_tracker_state()?;

    Ok(BulkArtifactSnapshot {
        report,
        sync_status,
        bulk_build_session_marker,
        hodl_tracker_state,
        cell_dist_tracker_state,
        hodl_waves,
        cell_distribution_snapshots,
        address_cohort_snapshots,
        block_headers,
        block_numbers_by_hash,
        txs_by_hash,
        activity_bundles,
        daily_activity_stats,
        hourly_activity_stats,
        dao_daily_snapshots,
        latest_dao_statistics,
        dao_top_depositors,
        script_daily_deltas,
        cell_payloads,
        live_cells,
        consumed_cells,
        cell_by_lock,
        cell_by_type,
        cell_by_lock_code,
        cell_by_type_code,
        cell_by_data_hash,
        core,
    })
}

#[allow(clippy::type_complexity)]
fn collect_dao_stats_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<(
    HashMap<String, DaoDailySnapshot>,
    Option<DaoLatestStatistics>,
    Option<DaoTopDepositors>,
)> {
    let dao_daily_snapshots = domain_store
        .list_dao_daily_snapshots()?
        .into_iter()
        .map(|snapshot| (snapshot.date.clone(), snapshot))
        .collect::<HashMap<_, _>>();
    let latest_dao_statistics = domain_store.get_latest_dao_statistics()?;
    let dao_top_depositors = domain_store.get_dao_top_depositors()?;
    Ok((
        dao_daily_snapshots,
        latest_dao_statistics,
        dao_top_depositors,
    ))
}

#[allow(clippy::type_complexity)]
fn collect_script_daily_deltas_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<HashMap<(Vec<u8>, bool), HashMap<u32, ScriptDailyDelta>>> {
    let iter = domain_store.iterator_cf(domain_store.cf_stats_script(), IteratorMode::Start);
    let mut script_daily_deltas: HashMap<(Vec<u8>, bool), HashMap<u32, ScriptDailyDelta>> =
        HashMap::new();

    for item in iter {
        let (key, value) = item?;
        if key.len() != keys::SCRIPT_DAILY_KEY_SIZE
            || key.first().copied() != Some(keys::STATS_PREFIX_SCRIPT_DAILY)
        {
            continue;
        }

        let (code_hash, is_type, date) = keys::decode_script_daily_key(&key);
        let delta: ScriptDailyDelta = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize script daily delta in bulk artifact snapshot helper: code_hash=0x{}, is_type={}, date={}, error={}",
                hex::encode(&code_hash),
                is_type,
                date,
                e
            )
        })?;
        script_daily_deltas
            .entry((code_hash, is_type))
            .or_default()
            .insert(date, delta);
    }

    Ok(script_daily_deltas)
}

fn build_history_rows(
    arena: &facts::FactsArena,
    resolved: &[facts::ResolvedTxFacts<'_>],
    interner: &interner::FrozenIdentityView,
    is_mainnet: bool,
    token_info_cache: &FxHashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Result<HistoryBuildResult> {
    if arena.txs.len() != resolved.len() {
        bail!(
            "bulk build history tx count mismatch: facts_txs={} resolved_txs={}",
            arena.txs.len(),
            resolved.len()
        );
    }

    let detectors = build_activity_protocol_detectors(resolved, interner, is_mainnet)?;

    let block_results: Vec<Result<BlockHistoryRows>> = arena
        .blocks
        .par_iter()
        .map(|block| {
            let block_txs = &arena.txs[block.tx_range.clone()];
            let block_resolved = &resolved[block.tx_range.clone()];
            build_history_rows_for_block(
                block,
                block_txs,
                block_resolved,
                &arena.cells,
                interner,
                &detectors,
                token_info_cache,
            )
        })
        .collect();

    // Merge results preserving block order.
    let estimated_total =
        arena.blocks.len() * 2 + arena.txs.len() * 2 + arena.cells.len() * 2 + arena.txs.len();
    let mut all_rows = Vec::with_capacity(estimated_total);
    let mut all_object_deltas: FxHashMap<Vec<u8>, i64> = FxHashMap::default();
    let mut all_identity_deltas: FxHashMap<Vec<u8>, i64> = FxHashMap::default();
    let mut all_bundles: Vec<ckbadger_store::types::TxActivityBundle> = Vec::new();
    for result in block_results {
        let block_rows = result?;
        all_rows.extend(block_rows.rows);
        all_bundles.extend(block_rows.activity_bundles);
        for (k, v) in block_rows.object_activity_count_deltas {
            let entry = all_object_deltas.entry(k).or_insert(0);
            *entry = entry
                .checked_add(v)
                .ok_or_else(|| anyhow!("object activity delta overflow during parallel merge"))?;
        }
        for (k, v) in block_rows.identity_activity_count_deltas {
            let entry = all_identity_deltas.entry(k).or_insert(0);
            *entry = entry
                .checked_add(v)
                .ok_or_else(|| anyhow!("identity activity delta overflow during parallel merge"))?;
        }
    }

    Ok(HistoryBuildResult {
        rows: all_rows,
        object_activity_count_deltas: all_object_deltas,
        identity_activity_count_deltas: all_identity_deltas,
        activity_bundles: all_bundles,
    })
}

/// Serialize into a pre-allocated Vec, avoiding realloc overhead of `bincode::serialize`
/// which starts with a small buffer and grows. Pre-computes exact size first.
///
/// Trade-off: traverses the value twice (once for size, once for serialization).
/// Net positive for larger structs where avoiding multiple Vec reallocations
/// outweighs the sizing pass. Used in the hot `par_iter` path of
/// `build_history_rows_for_block` where the allocation saving is amplified
/// across rayon threads.
fn bincode_serialize_presized<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    let size = bincode::serialized_size(value)
        .map_err(|e| anyhow!("bincode size estimation failed: {}", e))?;
    let mut buf = Vec::with_capacity(size as usize);
    bincode::serialize_into(&mut buf, value)
        .map_err(|e| anyhow!("bincode serialize_into failed: {}", e))?;
    Ok(buf)
}

#[allow(clippy::too_many_arguments)]
fn build_history_rows_for_block(
    block: &facts::BlockFacts,
    block_txs: &[facts::TxFacts],
    block_resolved: &[facts::ResolvedTxFacts<'_>],
    arena_cells: &[facts::CellFacts],
    interner: &interner::FrozenIdentityView,
    detectors: &[Box<dyn crate::db::writer::activities::ProtocolDetector>],
    token_info_cache: &FxHashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Result<BlockHistoryRows> {
    let input_count: usize = block_resolved
        .iter()
        .map(|tx| tx.resolved_inputs.len())
        .sum();
    let cell_count: usize = block_txs.iter().map(|tx| tx.output_range.len()).sum();
    let estimated_rows = 2 // block header + hash index
        + block_txs.len() * 4 // tx_index + tx_hash_map + ~2 addr_txs
        + input_count // consumed_cells
        + cell_count * 2 // cells + possible data_hash
        + block_txs.len(); // activity bundles estimate
    let mut rows = Vec::with_capacity(estimated_rows);
    let mut object_activity_count_deltas: FxHashMap<Vec<u8>, i64> = FxHashMap::default();
    let mut identity_activity_count_deltas: FxHashMap<Vec<u8>, i64> = FxHashMap::default();

    // Block header + hash index (2 rows per block).
    let header = CachedBlockHeader {
        hash: block.hash.to_vec(),
        timestamp: block.timestamp_ms,
        epoch_number: block.epoch_number,
        epoch_index: block.epoch_index,
        epoch_length: block.epoch_length,
        dao: block.dao.to_vec(),
        transactions_count: block.transactions_count,
    };
    rows.push(materialize::MaterializedRow::new(
        CF_BLOCK_HEADERS,
        keys::encode_block_num(block.number).to_vec(),
        bincode_serialize_presized(&header)?,
    ));
    rows.push(materialize::MaterializedRow::new(
        CF_BLOCK_HASH_INDEX,
        block.hash.to_vec(),
        block.number.to_le_bytes().to_vec(),
    ));

    if block_txs.len() != block_resolved.len() {
        bail!(
            "bulk build history tx count mismatch within block: block={} facts_txs={} resolved_txs={}",
            block.number,
            block_txs.len(),
            block_resolved.len()
        );
    }

    // Per-tx: tx_index, tx_hash_map, addr_txs, consumed_cells.
    for (tx, resolved_tx) in block_txs.iter().zip(block_resolved) {
        if tx.hash != resolved_tx.tx_hash
            || tx.block_number != resolved_tx.block_number
            || tx.tx_index != resolved_tx.tx_index
        {
            bail!(
                "bulk build history tx alignment mismatch: facts_tx=0x{} facts_block={} facts_tx_index={} resolved_tx=0x{} resolved_block={} resolved_tx_index={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index,
                hex::encode(resolved_tx.tx_hash),
                resolved_tx.block_number,
                resolved_tx.tx_index
            );
        }

        let entry = TxIndexEntry {
            is_cellbase: tx.is_cellbase,
            timestamp: tx.timestamp_ms,
            inputs_count: tx.inputs_count,
            outputs_count: tx.outputs_count,
            fee: resolved_tx_fee(tx, resolved_tx)?,
            tx_size: tx.tx_size,
            cycles: tx.cycles,
        };
        let tx_location = keys::encode_composite(&[
            &keys::encode_block_num(tx.block_number),
            &keys::encode_tx_idx(tx.tx_index),
        ]);
        rows.push(materialize::MaterializedRow::new(
            CF_TX_INDEX,
            tx_location.to_vec(),
            bincode_serialize_presized(&entry)?,
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_TX_HASH_MAP,
            tx.hash.to_vec(),
            tx_location.to_vec(),
        ));

        let mut touched_lock_hash_ids = FxHashSet::default();
        for output in resolved_tx.cells.iter() {
            touched_lock_hash_ids.insert(output.lock_script_hash_id);
        }
        for input in &resolved_tx.resolved_inputs {
            touched_lock_hash_ids.insert(input.lock_script_hash_id);
        }
        for lock_hash_id in touched_lock_hash_ids {
            rows.push(materialize::MaterializedRow::new(
                CF_ADDR_TXS,
                keys::encode_addr_tx_key(
                    interner.resolve_bytes(lock_hash_id),
                    tx.block_number,
                    tx.tx_index,
                    &tx.hash,
                ),
                Vec::new(),
            ));
        }

        if tx.is_cellbase {
            continue;
        }

        for input in &resolved_tx.resolved_inputs {
            rows.push(materialize::MaterializedRow::new(
                CF_CONSUMED_CELLS,
                keys::encode_outpoint(
                    &input.outpoint.tx_hash,
                    resolved_input_outpoint_index_i16(input)?,
                )
                .to_vec(),
                bincode_serialize_presized(&ConsumedCellMeta {
                    created_at_block: input.created_at_block,
                    consumed_at_block: tx.block_number,
                    consumed_by_tx: Some(tx.hash.to_vec()),
                })?,
            ));
        }
    }

    // Token transfers for this block's txs (block-local transfer_idx).
    {
        let mut transfer_idx: FxHashMap<(Vec<u8>, i64), i32> = FxHashMap::default();
        for tx in block_resolved {
            let input_udts = tx
                .resolved_inputs
                .iter()
                .filter_map(|input| parsed_udt_cell_from_input(input, interner, tx).transpose())
                .collect::<Result<Vec<_>>>()?;
            let output_udts = tx
                .cells
                .iter()
                .filter_map(|cell| parsed_udt_cell_from_output(cell, interner, tx).transpose())
                .collect::<Result<Vec<_>>>()?;

            for transfer in UdtParser::build_transfers_from_cells(&input_udts, &output_udts) {
                let idx = transfer_idx
                    .entry((transfer.type_script_hash.clone(), tx.block_number))
                    .or_insert(0);
                let record = TokenTransferRecord {
                    tx_hash: tx.tx_hash.to_vec(),
                    block_number: tx.block_number,
                    from_lock_hash: transfer.from_lock_hash.clone(),
                    to_lock_hash: transfer.to_lock_hash.clone(),
                    amount: transfer.amount,
                    is_mint: transfer.is_mint,
                    is_burn: transfer.is_burn,
                    timestamp: tx.timestamp_ms,
                };
                rows.push(materialize::MaterializedRow::new(
                    CF_TOKEN_TRANSFERS,
                    keys::encode_token_transfer_key(
                        &transfer.type_script_hash,
                        tx.block_number,
                        *idx,
                    ),
                    bincode_serialize_presized(&record)?,
                ));
                *idx = idx.checked_add(1).ok_or_else(|| {
                    anyhow!(
                        "token transfer index overflow in bulk build history rows: type_hash=0x{} block={}",
                        hex::encode(&transfer.type_script_hash),
                        tx.block_number
                    )
                })?;
            }
        }
    }

    // Activity bundles for this block.
    let mut activity_bundles;
    {
        if block_txs.len() != block_resolved.len() {
            bail!(
                "bulk build activity tx count mismatch within block: block={} facts_txs={} resolved_txs={}",
                block.number,
                block_txs.len(),
                block_resolved.len()
            );
        }

        let mut block_inputs = Vec::with_capacity(block_txs.len());
        let mut block_outputs = Vec::with_capacity(block_txs.len());
        for (tx, resolved_tx) in block_txs.iter().zip(block_resolved) {
            if tx.hash != resolved_tx.tx_hash
                || tx.block_number != resolved_tx.block_number
                || tx.tx_index != resolved_tx.tx_index
            {
                bail!(
                    "bulk build activity tx alignment mismatch: facts_tx=0x{} facts_block={} facts_tx_index={} resolved_tx=0x{} resolved_block={} resolved_tx_index={}",
                    hex::encode(tx.hash),
                    tx.block_number,
                    tx.tx_index,
                    hex::encode(resolved_tx.tx_hash),
                    resolved_tx.block_number,
                    resolved_tx.tx_index
                );
            }

            block_outputs.push(
                resolved_tx
                    .cells
                    .iter()
                    .map(|cell| crate::db::writer::activities::OutputCellView {
                        capacity: cell.capacity,
                        lock_code_hash: interner.resolve_bytes(cell.lock_code_hash_id),
                        lock_hash_type: cell.lock_hash_type,
                        lock_args: interner.resolve_bytes(cell.lock_args_id),
                        lock_script_hash: interner.resolve_bytes(cell.lock_script_hash_id),
                        type_code_hash: cell.type_code_hash_id.map(|id| interner.resolve_bytes(id)),
                        type_hash_type: cell.type_hash_type,
                        type_args: cell.type_args_id.map(|id| interner.resolve_bytes(id)),
                        type_script_hash: cell
                            .type_script_hash_id
                            .map(|id| interner.resolve_bytes(id)),
                        data_hash: cell.data_hash.as_ref().map_or(&[], |h| h.as_slice()),
                        data_size: cell.data_size,
                        data: &cell.data,
                    })
                    .collect::<Vec<_>>(),
            );
            block_inputs.push(
                resolved_tx
                    .resolved_inputs
                    .iter()
                    .map(|input| -> Result<crate::db::writer::activities::InputCellView<'_>> {
                        let (is_dao_withdraw_request, dao_compensation) = match (
                            input.dao_state,
                            input.dao_compensation_ars,
                        ) {
                            (
                                Some(facts::DaoCellState::WithdrawRequest { .. }),
                                Some(facts::DaoCompensationArs {
                                    deposit_ar,
                                    withdraw_request_ar,
                                }),
                            ) => (
                                true,
                                Some(crate::db::writer::dao::calculate_dao_compensation_from_ar(
                                    input.capacity, deposit_ar, withdraw_request_ar,
                                )?),
                            ),
                            (Some(facts::DaoCellState::WithdrawRequest { .. }), None) => {
                                bail!(
                                    "missing DAO compensation ARs while building bulk DAO activity input: outpoint=0x{}:{}",
                                    hex::encode(input.outpoint.tx_hash),
                                    input.outpoint.index
                                );
                            }
                            _ => (false, None),
                        };
                        Ok(crate::db::writer::activities::InputCellView {
                            lock_script_hash: interner.resolve_bytes(input.lock_script_hash_id),
                            lock_code_hash: interner.resolve_bytes(input.lock_code_hash_id),
                            lock_hash_type: input.lock_hash_type,
                            lock_args: interner.resolve_bytes(input.lock_args_id),
                            capacity: input.capacity,
                            occupied_capacity: input.occupied_capacity,
                            type_code_hash: input.type_code_hash_id.map(|id| interner.resolve_bytes(id)),
                            type_hash_type: input.type_hash_type,
                            type_script_hash: input.type_script_hash_id.map(|id| interner.resolve_bytes(id)),
                            type_args: input.type_args_id.map(|id| interner.resolve_bytes(id)),
                            udt_amount: input.udt_amount,
                            data: &[],
                            is_dao_withdraw_request,
                            dao_compensation,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
        }

        let tx_views = block_txs
            .iter()
            .zip(block_inputs)
            .zip(block_outputs)
            .map(
                |((tx, inputs), outputs)| crate::db::writer::activities::TxView {
                    tx_hash: &tx.hash,
                    block_hash: &tx.block_hash,
                    tx_index: tx.tx_index,
                    block_number: tx.block_number,
                    timestamp: tx.timestamp_ms,
                    is_cellbase: tx.is_cellbase,
                    inputs,
                    outputs,
                },
            )
            .collect::<Vec<_>>();

        let bundles =
            crate::db::writer::activities::build_activity_bundles_for_block_with_detectors(
                &tx_views,
                token_info_cache,
                detectors,
            )?;
        activity_bundles = Vec::with_capacity(bundles.len());
        for bundle in bundles {
            rows.push(materialize::MaterializedRow::new(
                CF_ACTIVITIES,
                keys::encode_tx_activity_bundle_key(
                    bundle.block_number,
                    bundle.tx_index,
                    &bundle.tx_hash,
                ),
                bincode_serialize_presized(&bundle)?,
            ));
            activity_bundles.push(bundle);
        }
    }

    // Object/identity collection activities for this block's txs.
    {
        let mut object_activity_acc =
            crate::db::writer::nft_activity_acc::ObjectCollectionActivityAccumulator::new();
        let mut identity_activity_acc =
            crate::db::writer::nft_activity_acc::ObjectCollectionActivityAccumulator::new();

        for tx in block_resolved {
            let mut dotbit_created_account_ids = FxHashSet::default();
            let mut dotbit_consumed_account_ids = FxHashSet::default();

            for input in &tx.resolved_inputs {
                let Some(protocol) = input.protocol_facts.as_ref() else {
                    continue;
                };
                match protocol {
                    facts::CellProtocolFacts::Spore(spore) if !spore.is_did => {
                        let collection_id = spore
                            .cluster_id
                            .map(|id| id.to_vec())
                            .unwrap_or_else(|| SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
                        object_activity_acc.record(
                            &collection_id,
                            &tx.tx_hash,
                            &spore.spore_id,
                            &tx.block_hash,
                            tx.block_number,
                            tx.tx_index,
                            tx.timestamp_ms,
                            false,
                        );
                    }
                    facts::CellProtocolFacts::MnftToken(token) => {
                        object_activity_acc.record(
                            &token.class_id,
                            &tx.tx_hash,
                            &token.token_id,
                            &tx.block_hash,
                            tx.block_number,
                            tx.tx_index,
                            tx.timestamp_ms,
                            false,
                        );
                    }
                    facts::CellProtocolFacts::Dotbit(dotbit) => {
                        dotbit_consumed_account_ids.insert(dotbit.account_id.to_vec());
                    }
                    _ => {}
                }
            }

            for cell in tx.cells.iter() {
                let Some(protocol) = cell.protocol_facts.as_ref() else {
                    continue;
                };
                match protocol {
                    facts::CellProtocolFacts::Spore(spore) if spore.is_did => {
                        identity_activity_acc.record(
                            &DID_CKB_SENTINEL_COLLECTION,
                            &tx.tx_hash,
                            &spore.spore_id,
                            &tx.block_hash,
                            tx.block_number,
                            tx.tx_index,
                            tx.timestamp_ms,
                            true,
                        );
                    }
                    facts::CellProtocolFacts::Spore(spore) => {
                        let collection_id = spore
                            .cluster_id
                            .map(|id| id.to_vec())
                            .unwrap_or_else(|| SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
                        object_activity_acc.record(
                            &collection_id,
                            &tx.tx_hash,
                            &spore.spore_id,
                            &tx.block_hash,
                            tx.block_number,
                            tx.tx_index,
                            tx.timestamp_ms,
                            true,
                        );
                    }
                    facts::CellProtocolFacts::MnftToken(token) => {
                        object_activity_acc.record(
                            &token.class_id,
                            &tx.tx_hash,
                            &token.token_id,
                            &tx.block_hash,
                            tx.block_number,
                            tx.tx_index,
                            tx.timestamp_ms,
                            true,
                        );
                    }
                    facts::CellProtocolFacts::Dotbit(dotbit) => {
                        dotbit_created_account_ids.insert(dotbit.account_id.to_vec());
                    }
                    _ => {}
                }
            }

            if let Some(entry) = crate::db::writer::dotbit::build_dotbit_tx_activity_entry(
                tx.dotbit_action.as_deref(),
                &dotbit_created_account_ids,
                &dotbit_consumed_account_ids,
                &tx.tx_hash,
                &tx.block_hash,
                tx.timestamp_ms,
            ) {
                rows.push(materialize::MaterializedRow::new(
                    CF_IDENTITY_COLLECTION_ACTIVITIES,
                    keys::encode_nft_collection_activity_key(
                        &DOTBIT_SENTINEL_COLLECTION,
                        tx.block_number,
                        tx.tx_index,
                        &tx.block_hash,
                        &tx.tx_hash,
                    )
                    .to_vec(),
                    bincode_serialize_presized(&entry)?,
                ));
                let delta = identity_activity_count_deltas
                    .entry(DOTBIT_SENTINEL_COLLECTION.to_vec())
                    .or_insert(0);
                *delta = delta.checked_add(1).ok_or_else(|| {
                    anyhow!(
                        "dotbit identity activity delta overflow in bulk build: block={} tx=0x{}",
                        tx.block_number,
                        hex::encode(tx.tx_hash)
                    )
                })?;
            }
        }

        for resolved_entry in object_activity_acc.into_resolved_entries() {
            let delta = object_activity_count_deltas
                .entry(resolved_entry.collection_id.clone())
                .or_insert(0);
            *delta = delta.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "object collection activity delta overflow in bulk build history rows: collection_id=0x{}",
                    hex::encode(&resolved_entry.collection_id)
                )
            })?;
            rows.push(materialize::MaterializedRow::new(
                CF_OBJECT_COLLECTION_ACTIVITIES,
                keys::encode_nft_collection_activity_key(
                    &resolved_entry.collection_id,
                    resolved_entry.block_number,
                    resolved_entry.tx_idx,
                    &resolved_entry.entry.block_hash,
                    &resolved_entry.entry.tx_hash,
                )
                .to_vec(),
                bincode_serialize_presized(&resolved_entry.entry)?,
            ));
        }

        for resolved_entry in identity_activity_acc.into_resolved_entries() {
            let delta = identity_activity_count_deltas
                .entry(resolved_entry.collection_id.clone())
                .or_insert(0);
            *delta = delta.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "identity collection activity delta overflow in bulk build history rows: collection_id=0x{}",
                    hex::encode(&resolved_entry.collection_id)
                )
            })?;
            rows.push(materialize::MaterializedRow::new(
                CF_IDENTITY_COLLECTION_ACTIVITIES,
                keys::encode_nft_collection_activity_key(
                    &resolved_entry.collection_id,
                    resolved_entry.block_number,
                    resolved_entry.tx_idx,
                    &resolved_entry.entry.block_hash,
                    &resolved_entry.entry.tx_hash,
                )
                .to_vec(),
                bincode_serialize_presized(&resolved_entry.entry)?,
            ));
        }
    }

    // Cell payloads (CF_CELLS) + data_hash index (CF_CELL_BY_DATA_HASH) for this block's cells.
    for tx in block_txs {
        for cell in &arena_cells[tx.output_range.clone()] {
            let outpoint_key =
                keys::encode_outpoint(&cell.outpoint.tx_hash, cell_outpoint_index_i16(cell)?)
                    .to_vec();
            rows.push(materialize::MaterializedRow::new(
                CF_CELLS,
                outpoint_key,
                bincode_serialize_presized(&cell_facts_to_live_cell_info(cell, interner))?,
            ));

            if let Some(data_hash) = &cell.data_hash {
                rows.push(materialize::MaterializedRow::new(
                    CF_CELL_BY_DATA_HASH,
                    keys::encode_cell_index_key(
                        data_hash,
                        cell.created_at_block,
                        &cell.outpoint.tx_hash,
                        cell_outpoint_index_i16(cell)?,
                    ),
                    Vec::new(),
                ));
            }
        }
    }

    Ok(BlockHistoryRows {
        rows,
        object_activity_count_deltas,
        identity_activity_count_deltas,
        activity_bundles,
    })
}

#[cfg(test)]
fn build_object_collection_activity_rows(
    resolved: &[facts::ResolvedTxFacts<'_>],
    object_activity_count_deltas: &mut FxHashMap<Vec<u8>, i64>,
    identity_activity_count_deltas: &mut FxHashMap<Vec<u8>, i64>,
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut object_activity_acc =
        crate::db::writer::nft_activity_acc::ObjectCollectionActivityAccumulator::new();
    let mut identity_activity_acc =
        crate::db::writer::nft_activity_acc::ObjectCollectionActivityAccumulator::new();
    let mut rows = Vec::new();

    for tx in resolved {
        let mut dotbit_created_account_ids = FxHashSet::default();
        let mut dotbit_consumed_account_ids = FxHashSet::default();

        for input in &tx.resolved_inputs {
            let Some(protocol) = input.protocol_facts.as_ref() else {
                continue;
            };
            match protocol {
                facts::CellProtocolFacts::Spore(spore) if !spore.is_did => {
                    let collection_id = spore
                        .cluster_id
                        .map(|id| id.to_vec())
                        .unwrap_or_else(|| SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
                    object_activity_acc.record(
                        &collection_id,
                        &tx.tx_hash,
                        &spore.spore_id,
                        &tx.block_hash,
                        tx.block_number,
                        tx.tx_index,
                        tx.timestamp_ms,
                        false,
                    );
                }
                facts::CellProtocolFacts::MnftToken(token) => {
                    object_activity_acc.record(
                        &token.class_id,
                        &tx.tx_hash,
                        &token.token_id,
                        &tx.block_hash,
                        tx.block_number,
                        tx.tx_index,
                        tx.timestamp_ms,
                        false,
                    );
                }
                facts::CellProtocolFacts::Dotbit(dotbit) => {
                    dotbit_consumed_account_ids.insert(dotbit.account_id.to_vec());
                }
                _ => {}
            }
        }

        for cell in tx.cells.iter() {
            let Some(protocol) = cell.protocol_facts.as_ref() else {
                continue;
            };
            match protocol {
                facts::CellProtocolFacts::Spore(spore) if spore.is_did => {
                    identity_activity_acc.record(
                        &DID_CKB_SENTINEL_COLLECTION,
                        &tx.tx_hash,
                        &spore.spore_id,
                        &tx.block_hash,
                        tx.block_number,
                        tx.tx_index,
                        tx.timestamp_ms,
                        true,
                    );
                }
                facts::CellProtocolFacts::Spore(spore) => {
                    let collection_id = spore
                        .cluster_id
                        .map(|id| id.to_vec())
                        .unwrap_or_else(|| SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
                    object_activity_acc.record(
                        &collection_id,
                        &tx.tx_hash,
                        &spore.spore_id,
                        &tx.block_hash,
                        tx.block_number,
                        tx.tx_index,
                        tx.timestamp_ms,
                        true,
                    );
                }
                facts::CellProtocolFacts::MnftToken(token) => {
                    object_activity_acc.record(
                        &token.class_id,
                        &tx.tx_hash,
                        &token.token_id,
                        &tx.block_hash,
                        tx.block_number,
                        tx.tx_index,
                        tx.timestamp_ms,
                        true,
                    );
                }
                facts::CellProtocolFacts::Dotbit(dotbit) => {
                    dotbit_created_account_ids.insert(dotbit.account_id.to_vec());
                }
                _ => {}
            }
        }

        if let Some(entry) = crate::db::writer::dotbit::build_dotbit_tx_activity_entry(
            tx.dotbit_action.as_deref(),
            &dotbit_created_account_ids,
            &dotbit_consumed_account_ids,
            &tx.tx_hash,
            &tx.block_hash,
            tx.timestamp_ms,
        ) {
            rows.push(materialize::MaterializedRow::new(
                CF_IDENTITY_COLLECTION_ACTIVITIES,
                keys::encode_nft_collection_activity_key(
                    &DOTBIT_SENTINEL_COLLECTION,
                    tx.block_number,
                    tx.tx_index,
                    &tx.block_hash,
                    &tx.tx_hash,
                )
                .to_vec(),
                bincode::serialize(&entry)?,
            ));
            let delta = identity_activity_count_deltas
                .entry(DOTBIT_SENTINEL_COLLECTION.to_vec())
                .or_insert(0);
            *delta = delta.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "dotbit identity activity delta overflow in bulk build: block={} tx=0x{}",
                    tx.block_number,
                    hex::encode(tx.tx_hash)
                )
            })?;
        }
    }

    for resolved_entry in object_activity_acc.into_resolved_entries() {
        let delta = object_activity_count_deltas
            .entry(resolved_entry.collection_id.clone())
            .or_insert(0);
        *delta = delta.checked_add(1).ok_or_else(|| {
            anyhow!(
                "object collection activity delta overflow in test helper: collection_id=0x{}",
                hex::encode(&resolved_entry.collection_id)
            )
        })?;
        rows.push(materialize::MaterializedRow::new(
            CF_OBJECT_COLLECTION_ACTIVITIES,
            keys::encode_nft_collection_activity_key(
                &resolved_entry.collection_id,
                resolved_entry.block_number,
                resolved_entry.tx_idx,
                &resolved_entry.entry.block_hash,
                &resolved_entry.entry.tx_hash,
            )
            .to_vec(),
            bincode::serialize(&resolved_entry.entry)?,
        ));
    }

    for resolved_entry in identity_activity_acc.into_resolved_entries() {
        let delta = identity_activity_count_deltas
            .entry(resolved_entry.collection_id.clone())
            .or_insert(0);
        *delta = delta.checked_add(1).ok_or_else(|| {
            anyhow!(
                "identity collection activity delta overflow in bulk build history rows: collection_id=0x{}",
                hex::encode(&resolved_entry.collection_id)
            )
        })?;
        rows.push(materialize::MaterializedRow::new(
            CF_IDENTITY_COLLECTION_ACTIVITIES,
            keys::encode_nft_collection_activity_key(
                &resolved_entry.collection_id,
                resolved_entry.block_number,
                resolved_entry.tx_idx,
                &resolved_entry.entry.block_hash,
                &resolved_entry.entry.tx_hash,
            )
            .to_vec(),
            bincode::serialize(&resolved_entry.entry)?,
        ));
    }

    Ok(rows)
}

#[allow(clippy::type_complexity)]
/// Pre-load token display info (symbol, decimals) from the store.
/// On a fresh DB this contains only label-imported data which is
/// immutable for the duration of bulk sync. Loading once eliminates
/// the per-batch store read that forced flush ordering.
fn preload_token_info_cache(
    store: &CkbadgerStore,
) -> Result<FxHashMap<Vec<u8>, (Option<String>, Option<u8>)>> {
    let mut cache = FxHashMap::default();
    let cf = store.cf_tokens();
    let iter = store.iterator_cf(cf, IteratorMode::Start);
    for item in iter {
        let (key, value) =
            item.map_err(|e| anyhow!("failed to iterate CF_TOKENS for token info preload: {}", e))?;
        let info: ckbadger_store::types::TokenInfo = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize token info during preload: key=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        let display_name = info.symbol.or(info.name);
        let decimals = match info.decimals {
            Some(value) => Some(u8::try_from(value).map_err(|_| {
                anyhow!(
                    "token decimals out of u8 range during preload: key=0x{} decimals={}",
                    hex::encode(&key),
                    value
                )
            })?),
            None => None,
        };
        cache.insert(key.to_vec(), (display_name, decimals));
    }
    info!(
        token_count = cache.len(),
        "Pre-loaded token info cache for bulk sync"
    );
    Ok(cache)
}

#[cfg(test)]
fn build_sealed_aggregate_rows(
    bundles: &[TxActivityBundle],
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut accumulator = ActivityStatsAccumulator::default();
    accumulator.apply_bundles(bundles)?;
    accumulator.build_rows()
}

fn build_activity_protocol_detectors(
    resolved: &[facts::ResolvedTxFacts<'_>],
    interner: &interner::FrozenIdentityView,
    is_mainnet: bool,
) -> Result<Vec<Box<dyn crate::db::writer::activities::ProtocolDetector>>> {
    let mut lock_code_hashes = HashSet::new();
    let mut type_code_hashes = HashSet::new();

    for tx in resolved {
        for input in &tx.resolved_inputs {
            lock_code_hashes.insert(activity_code_hash(
                interner,
                input.lock_code_hash_id,
                "input lock_code_hash",
                tx,
            )?);
            if let Some(type_code_hash_id) = input.type_code_hash_id {
                type_code_hashes.insert(activity_code_hash(
                    interner,
                    type_code_hash_id,
                    "input type_code_hash",
                    tx,
                )?);
            }
        }

        for cell in tx.cells.iter() {
            lock_code_hashes.insert(activity_code_hash(
                interner,
                cell.lock_code_hash_id,
                "output lock_code_hash",
                tx,
            )?);
            if let Some(type_code_hash_id) = cell.type_code_hash_id {
                type_code_hashes.insert(activity_code_hash(
                    interner,
                    type_code_hash_id,
                    "output type_code_hash",
                    tx,
                )?);
            }
        }
    }

    Ok(vec![
        Box::new(crate::db::writer::rgbpp_detector::RgbppDetector::new(
            is_mainnet,
        )) as Box<dyn crate::db::writer::activities::ProtocolDetector>,
        Box::new(crate::db::writer::fiber_detector::FiberDetector::new(
            is_mainnet,
        )),
        Box::new(crate::db::writer::stablepp_detector::StableppDetector::new(
            is_mainnet,
        )),
        Box::new(crate::db::writer::utxoswap_detector::UtxoSwapDetector::new(
            is_mainnet,
        )),
    ]
    .into_iter()
    .filter(|detector| detector.might_apply_batch(&lock_code_hashes, &type_code_hashes))
    .collect())
}

fn activity_code_hash(
    interner: &interner::FrozenIdentityView,
    id: crate::sync::types::InternId,
    label: &str,
    tx: &facts::ResolvedTxFacts<'_>,
) -> Result<[u8; 32]> {
    interner.resolve_bytes(id).try_into().map_err(|_| {
        anyhow!(
            "invalid {} length while building bulk activities: tx=0x{} block={} tx_index={} len={}",
            label,
            hex::encode(tx.tx_hash),
            tx.block_number,
            tx.tx_index,
            interner.resolve_bytes(id).len()
        )
    })
}

fn parsed_udt_cell_from_output(
    cell: &facts::CellFacts,
    interner: &interner::FrozenIdentityView,
    tx: &facts::ResolvedTxFacts<'_>,
) -> Result<Option<ParsedUdtCell>> {
    parsed_udt_cell_from_parts(
        cell.semantic_tag,
        cell.type_script_hash_id,
        cell.type_code_hash_id,
        cell.type_hash_type,
        cell.type_args_id,
        cell.lock_script_hash_id,
        cell.udt_amount,
        interner,
        &format!(
            "output outpoint=0x{}:{} block={} tx=0x{} tx_index={}",
            hex::encode(cell.outpoint.tx_hash),
            cell.outpoint.index,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        ),
    )
}

fn parsed_udt_cell_from_input(
    input: &facts::ResolvedInputFacts,
    interner: &interner::FrozenIdentityView,
    tx: &facts::ResolvedTxFacts<'_>,
) -> Result<Option<ParsedUdtCell>> {
    parsed_udt_cell_from_parts(
        input.semantic_tag,
        input.type_script_hash_id,
        input.type_code_hash_id,
        input.type_hash_type,
        input.type_args_id,
        input.lock_script_hash_id,
        input.udt_amount,
        interner,
        &format!(
            "input outpoint=0x{}:{} block={} tx=0x{} tx_index={}",
            hex::encode(input.outpoint.tx_hash),
            input.outpoint.index,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn parsed_udt_cell_from_parts(
    semantic_tag: facts::CellSemanticTag,
    type_script_hash_id: Option<crate::sync::types::InternId>,
    type_code_hash_id: Option<crate::sync::types::InternId>,
    type_hash_type: Option<i16>,
    type_args_id: Option<crate::sync::types::InternId>,
    lock_script_hash_id: crate::sync::types::InternId,
    udt_amount: Option<u128>,
    interner: &interner::FrozenIdentityView,
    context: &str,
) -> Result<Option<ParsedUdtCell>> {
    let Some(standard) = udt_standard_for_semantic_tag(semantic_tag) else {
        return Ok(None);
    };

    let type_script_hash_id = type_script_hash_id.ok_or_else(|| {
        anyhow!(
            "missing type_script_hash_id for bulk build token transfer cell: {}",
            context
        )
    })?;
    let type_code_hash_id = type_code_hash_id.ok_or_else(|| {
        anyhow!(
            "missing type_code_hash_id for bulk build token transfer cell: {}",
            context
        )
    })?;
    let type_hash_type = type_hash_type.ok_or_else(|| {
        anyhow!(
            "missing type_hash_type for bulk build token transfer cell: {}",
            context
        )
    })?;
    let type_args_id = type_args_id.ok_or_else(|| {
        anyhow!(
            "missing type_args_id for bulk build token transfer cell: {}",
            context
        )
    })?;
    // xUDT-compatible cells can carry non-amount payloads (e.g. owner-mode cells).
    // These are legitimately tagged Xudt by semantic classification but have no
    // fungible amount — skip them for token transfer processing.
    let Some(amount) = udt_amount else {
        return Ok(None);
    };

    Ok(Some(ParsedUdtCell {
        type_script_hash: interner.resolve_bytes(type_script_hash_id).to_vec(),
        type_code_hash: interner.resolve_bytes(type_code_hash_id).to_vec(),
        type_hash_type,
        type_args: interner.resolve_bytes(type_args_id).to_vec(),
        lock_script_hash: interner.resolve_bytes(lock_script_hash_id).to_vec(),
        amount,
        standard,
    }))
}

fn udt_standard_for_semantic_tag(semantic_tag: facts::CellSemanticTag) -> Option<UdtStandard> {
    match semantic_tag {
        facts::CellSemanticTag::Sudt => Some(UdtStandard::Sudt),
        facts::CellSemanticTag::Xudt => Some(UdtStandard::Xudt),
        _ => None,
    }
}

fn build_final_snapshot_rows(
    sequencer: &sequencer::BulkSequencer,
    interner: &interner::FrozenIdentityView,
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut rows = Vec::with_capacity(sequencer.live_count() * 5);

    for slot in sequencer.live_slots() {
        let outpoint_index = live_slot_outpoint_index_i16(slot)?;
        rows.push(materialize::MaterializedRow::new(
            CF_LIVE_CELLS,
            keys::encode_outpoint(&slot.outpoint.tx_hash, outpoint_index).to_vec(),
            slot.created_at_block.to_le_bytes().to_vec(),
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_CELL_BY_LOCK,
            keys::encode_cell_index_key(
                interner.resolve_bytes(slot.lock_script_hash_id),
                slot.created_at_block,
                &slot.outpoint.tx_hash,
                outpoint_index,
            ),
            Vec::new(),
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_CELL_BY_LOCK_CODE,
            keys::encode_cell_index_key(
                interner.resolve_bytes(slot.lock_code_hash_id),
                slot.created_at_block,
                &slot.outpoint.tx_hash,
                outpoint_index,
            ),
            Vec::new(),
        ));
        if let Some(type_script_hash_id) = slot.type_script_hash_id {
            rows.push(materialize::MaterializedRow::new(
                CF_CELL_BY_TYPE,
                keys::encode_cell_index_key(
                    interner.resolve_bytes(type_script_hash_id),
                    slot.created_at_block,
                    &slot.outpoint.tx_hash,
                    outpoint_index,
                ),
                Vec::new(),
            ));
        }
        if let Some(type_code_hash_id) = slot.type_code_hash_id {
            rows.push(materialize::MaterializedRow::new(
                CF_CELL_BY_TYPE_CODE,
                keys::encode_cell_index_key(
                    interner.resolve_bytes(type_code_hash_id),
                    slot.created_at_block,
                    &slot.outpoint.tx_hash,
                    outpoint_index,
                ),
                Vec::new(),
            ));
        }
    }

    Ok(rows)
}

fn resolved_tx_fee(tx: &facts::TxFacts, resolved_tx: &facts::ResolvedTxFacts<'_>) -> Result<i64> {
    if tx.is_cellbase {
        return Ok(0);
    }

    let total_input_capacity =
        resolved_tx
            .resolved_inputs
            .iter()
            .try_fold(0i64, |acc, input| {
                acc.checked_add(input.capacity).ok_or_else(|| {
                    anyhow!(
                        "bulk build input capacity overflow while materializing tx index: tx=0x{} block={} tx_index={}",
                        hex::encode(tx.hash),
                        tx.block_number,
                        tx.tx_index
                    )
                })
            })?;
    let total_output_capacity = resolved_tx.cells.iter().try_fold(0i64, |acc, cell| {
        acc.checked_add(cell.capacity).ok_or_else(|| {
            anyhow!(
                "bulk build output capacity overflow while materializing tx index: tx=0x{} block={} tx_index={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index
            )
        })
    })?;

    total_input_capacity
        .checked_sub(total_output_capacity)
        .ok_or_else(|| {
            anyhow!(
                "bulk build negative fee while materializing tx index: tx=0x{} block={} tx_index={} inputs={} outputs={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index,
                total_input_capacity,
                total_output_capacity
            )
        })
}

fn cell_facts_to_live_cell_info(
    cell: &facts::CellFacts,
    interner: &interner::FrozenIdentityView,
) -> LiveCellInfo {
    LiveCellInfo {
        capacity: cell.capacity,
        lock_script_hash: interner.resolve_bytes(cell.lock_script_hash_id).to_vec(),
        lock_code_hash: interner.resolve_bytes(cell.lock_code_hash_id).to_vec(),
        lock_hash_type: cell.lock_hash_type,
        lock_args: interner.resolve_bytes(cell.lock_args_id).to_vec(),
        type_script_hash: cell
            .type_script_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_code_hash: cell
            .type_code_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_hash_type: cell.type_hash_type,
        type_args: cell
            .type_args_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        data_size: cell.data_size,
        occupied_capacity: cell.occupied_capacity,
        udt_amount: cell.udt_amount,
        data_hash: cell.data_hash.map(|hash| hash.to_vec()),
    }
}

fn cell_outpoint_index_i16(cell: &facts::CellFacts) -> Result<i16> {
    i16::try_from(cell.outpoint.index).map_err(|_| {
        anyhow!(
            "bulk build cell outpoint index exceeds i16 while materializing cells: tx=0x{} output_index={}",
            hex::encode(cell.outpoint.tx_hash),
            cell.outpoint.index
        )
    })
}

fn live_slot_outpoint_index_i16(slot: &live_cells::LiveCellSlot) -> Result<i16> {
    i16::try_from(slot.outpoint.index).map_err(|_| {
        anyhow!(
            "bulk build live outpoint index exceeds i16 while materializing live cells: tx=0x{} output_index={}",
            hex::encode(slot.outpoint.tx_hash),
            slot.outpoint.index
        )
    })
}

fn resolved_input_outpoint_index_i16(input: &facts::ResolvedInputFacts) -> Result<i16> {
    i16::try_from(input.outpoint.index).map_err(|_| {
        anyhow!(
            "bulk build consumed outpoint index exceeds i16 while materializing consumed cells: tx=0x{} output_index={}",
            hex::encode(input.outpoint.tx_hash),
            input.outpoint.index
        )
    })
}

#[allow(clippy::type_complexity)]
fn collect_history_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<(
    HashMap<i64, CachedBlockHeader>,
    HashMap<Vec<u8>, i64>,
    HashMap<Vec<u8>, (i64, i32, TxIndexEntry)>,
    HashMap<Vec<u8>, TxActivityBundle>,
)> {
    let mut block_headers = HashMap::new();
    let mut block_numbers_by_hash = HashMap::new();
    let block_iter = domain_store.iterator_cf(domain_store.cf_block_headers(), IteratorMode::Start);
    for item in block_iter {
        let (key, value) = item?;
        if key.len() != 8 {
            bail!(
                "invalid block_headers key length in bulk artifact snapshot helper: key_len={}",
                key.len()
            );
        }
        let block_number = keys::decode_block_num(&key);
        let header: CachedBlockHeader = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize CachedBlockHeader in bulk artifact snapshot helper: block_number={} error={}",
                block_number,
                e
            )
        })?;
        let indexed_block_number = domain_store
            .get_block_number_by_hash(&header.hash)?
            .ok_or_else(|| {
                anyhow!(
                    "block_hash_index missing in bulk artifact snapshot helper: block_number={} hash=0x{}",
                    block_number,
                    hex::encode(&header.hash)
                )
            })?;
        if indexed_block_number != block_number {
            bail!(
                "block_hash_index mismatch in bulk artifact snapshot helper: block_number={} indexed_block_number={} hash=0x{}",
                block_number,
                indexed_block_number,
                hex::encode(&header.hash)
            );
        }
        block_numbers_by_hash.insert(header.hash.clone(), indexed_block_number);
        block_headers.insert(block_number, header);
    }

    let mut txs_by_hash = HashMap::new();
    let tx_iter = domain_store.iterator_cf(domain_store.cf_tx_hash_map(), IteratorMode::Start);
    for item in tx_iter {
        let (tx_hash, _value) = item?;
        let tx_entry = domain_store.get_tx_by_hash(&tx_hash)?.ok_or_else(|| {
            anyhow!(
                "tx index missing in bulk artifact snapshot helper: tx_hash=0x{}",
                hex::encode(&tx_hash)
            )
        })?;
        txs_by_hash.insert(tx_hash.to_vec(), tx_entry);
    }

    let mut activity_bundles = HashMap::new();
    let activity_iter = domain_store.iterator_cf(domain_store.cf_activities(), IteratorMode::Start);
    for item in activity_iter {
        let (key, value) = item?;
        let bundle: TxActivityBundle = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize TxActivityBundle in bulk artifact snapshot helper: key=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        activity_bundles.insert(key.to_vec(), bundle);
    }

    Ok((
        block_headers,
        block_numbers_by_hash,
        txs_by_hash,
        activity_bundles,
    ))
}

fn collect_activity_stats_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<(
    HashMap<String, DailyActivityStats>,
    HashMap<String, DailyActivityStats>,
)> {
    let daily_activity_stats = domain_store
        .list_daily_activity_stats()?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let hourly_activity_stats = domain_store
        .list_hourly_activity_stats_since("0000000000")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    Ok((daily_activity_stats, hourly_activity_stats))
}

#[allow(clippy::type_complexity)]
fn collect_hodl_stats_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<(
    HashMap<String, DailyHodlWave>,
    HashMap<String, DailyCellDistribution>,
    HashMap<String, DailyAddressCohort>,
)> {
    let mut hodl_waves = HashMap::new();
    let hodl_iter = domain_store.prefix_iterator_cf(
        domain_store.cf_stats_hodl(),
        &[keys::stats_prefix::HODL_WAVE],
    );
    for item in hodl_iter {
        let (key, value) = item?;
        if !key.starts_with(&[keys::stats_prefix::HODL_WAVE]) {
            break;
        }
        let date = String::from_utf8_lossy(&key[1..]).to_string();
        let wave: DailyHodlWave = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize DailyHodlWave in bulk artifact snapshot helper: key=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        hodl_waves.insert(date, wave);
    }

    let mut cell_distribution_snapshots = HashMap::new();
    let cell_dist_iter = domain_store.prefix_iterator_cf(
        domain_store.cf_stats_hodl(),
        &[keys::stats_prefix::CELL_DISTRIBUTION],
    );
    for item in cell_dist_iter {
        let (key, value) = item?;
        if !key.starts_with(&[keys::stats_prefix::CELL_DISTRIBUTION]) {
            break;
        }
        let date = String::from_utf8_lossy(&key[1..]).to_string();
        let snapshot: DailyCellDistribution = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize DailyCellDistribution in bulk artifact snapshot helper: key=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        cell_distribution_snapshots.insert(date, snapshot);
    }

    let mut address_cohort_snapshots = HashMap::new();
    let cohort_iter = domain_store.prefix_iterator_cf(
        domain_store.cf_stats_hodl(),
        &[keys::stats_prefix::ADDR_COHORT],
    );
    for item in cohort_iter {
        let (key, value) = item?;
        if !key.starts_with(&[keys::stats_prefix::ADDR_COHORT]) {
            break;
        }
        let date = String::from_utf8_lossy(&key[1..]).to_string();
        let snapshot: DailyAddressCohort = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize DailyAddressCohort in bulk artifact snapshot helper: key=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        address_cohort_snapshots.insert(date, snapshot);
    }

    Ok((
        hodl_waves,
        cell_distribution_snapshots,
        address_cohort_snapshots,
    ))
}

#[allow(clippy::type_complexity)]
fn collect_cell_snapshot(
    domain_store: &CkbadgerStore,
    append_store: &CkbadgerStore,
) -> Result<(
    HashMap<Vec<u8>, LiveCellInfo>,
    HashMap<Vec<u8>, i64>,
    HashMap<Vec<u8>, ConsumedCellMeta>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
)> {
    let mut cell_payloads = HashMap::new();
    let cell_iter = append_store.iterator_cf(append_store.cf_cells(), IteratorMode::Start);
    for item in cell_iter {
        let (key, value) = item?;
        let cell: LiveCellInfo = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize LiveCellInfo in bulk artifact snapshot helper: outpoint=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        cell_payloads.insert(key.to_vec(), cell);
    }

    let mut live_cells = HashMap::new();
    let live_iter = domain_store.iterator_cf(domain_store.cf_live_cells(), IteratorMode::Start);
    for item in live_iter {
        let (key, value) = item?;
        let created_at_block = decode_live_cell_marker(&value).ok_or_else(|| {
            anyhow!(
                "invalid live cell marker value in bulk artifact snapshot helper: outpoint=0x{} value_len={}",
                hex::encode(&key),
                value.len()
            )
        })?;
        live_cells.insert(key.to_vec(), created_at_block);
    }

    let mut consumed_cells = HashMap::new();
    let consumed_iter =
        domain_store.iterator_cf(domain_store.cf_consumed_cells(), IteratorMode::Start);
    for item in consumed_iter {
        let (key, value) = item?;
        let consumed: ConsumedCellMeta = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize ConsumedCellMeta in bulk artifact snapshot helper: outpoint=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        consumed_cells.insert(key.to_vec(), consumed);
    }

    let cell_by_lock = collect_index_keys(
        domain_store.iterator_cf(domain_store.cf_cell_by_lock(), IteratorMode::Start),
    )?;
    let cell_by_type = collect_index_keys(
        domain_store.iterator_cf(domain_store.cf_cell_by_type(), IteratorMode::Start),
    )?;
    let cell_by_lock_code = collect_index_keys(
        domain_store.iterator_cf(domain_store.cf_cell_by_lock_code(), IteratorMode::Start),
    )?;
    let cell_by_type_code = collect_index_keys(
        domain_store.iterator_cf(domain_store.cf_cell_by_type_code(), IteratorMode::Start),
    )?;
    let cell_by_data_hash = collect_index_keys(
        domain_store.iterator_cf(domain_store.cf_cell_by_data_hash(), IteratorMode::Start),
    )?;

    Ok((
        cell_payloads,
        live_cells,
        consumed_cells,
        cell_by_lock,
        cell_by_type,
        cell_by_lock_code,
        cell_by_type_code,
        cell_by_data_hash,
    ))
}

fn collect_index_keys<I>(iter: I) -> Result<HashSet<Vec<u8>>>
where
    I: IntoIterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>>,
{
    let mut keys = HashSet::new();
    for item in iter {
        let (key, value) = item?;
        if !value.is_empty() {
            bail!(
                "cell index value must be empty in bulk artifact snapshot helper: key=0x{} value_len={}",
                hex::encode(&key),
                value.len()
            );
        }
        keys.insert(key.to_vec());
    }
    Ok(keys)
}

fn collect_core_owner_state_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<CoreOwnerStateSnapshot> {
    let mut address_balances = HashMap::new();
    let addr_iter = domain_store.iterator_cf(domain_store.cf_addr_balance(), IteratorMode::Start);
    for item in addr_iter {
        let (key, value) = item?;
        let balance: AddressBalance = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize AddressBalance in core owner snapshot helper: lock_hash=0x{}, error={}",
                hex::encode(&key),
                e
            )
        })?;
        address_balances.insert(key.to_vec(), balance);
    }

    let script_infos = domain_store
        .list_script_infos()?
        .into_iter()
        .collect::<HashMap<_, _>>();

    let tokens = domain_store
        .list_tokens()?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let mut token_holders: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>> = HashMap::new();
    let mut token_transfer_counts = HashMap::new();
    let mut token_hourly_transfers = HashMap::new();
    let mut token_daily_deltas = HashMap::new();
    for type_hash in tokens.keys() {
        let holders = domain_store
            .list_token_holders(type_hash, usize::MAX)?
            .into_iter()
            .collect::<HashMap<_, _>>();
        token_holders.insert(type_hash.clone(), holders);

        token_transfer_counts.insert(
            type_hash.clone(),
            domain_store.get_token_transfers_count(type_hash)?,
        );

        let prefix = keys::encode_token_hourly_prefix(type_hash);
        let iter = domain_store.prefix_iterator_cf(domain_store.cf_stats_token(), &prefix);
        let mut hourly = HashMap::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow!(
                    "failed to iterate stats_token hourly rows in core owner snapshot helper: type_hash=0x{}, error={}",
                    hex::encode(type_hash),
                    e
                )
            })?;
            if !key.starts_with(prefix.as_slice()) {
                break;
            }
            if key.len() != 41 {
                bail!(
                    "invalid token hourly key length in core owner snapshot helper: type_hash=0x{}, len={}",
                    hex::encode(type_hash),
                    key.len()
                );
            }
            if value.len() != 8 {
                bail!(
                    "invalid token hourly value length in core owner snapshot helper: type_hash=0x{}, len={}",
                    hex::encode(type_hash),
                    value.len()
                );
            }
            let hour_bucket = i64::from_be_bytes(
                key[33..41]
                    .try_into()
                    .expect("hour bucket slice length must be 8"),
            );
            let count = i64::from_le_bytes(
                value[..8]
                    .try_into()
                    .expect("hourly transfer value length must be 8"),
            );
            hourly.insert(hour_bucket, count);
        }
        if !hourly.is_empty() {
            token_hourly_transfers.insert(type_hash.clone(), hourly);
        }

        let daily_deltas = domain_store
            .list_token_daily_deltas(type_hash)?
            .into_iter()
            .collect::<HashMap<_, _>>();
        if !daily_deltas.is_empty() {
            token_daily_deltas.insert(type_hash.clone(), daily_deltas);
        }
    }
    let mut addr_tokens: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>> = HashMap::new();
    let addr_tokens_iter = domain_store.iterator_cf(
        domain_store.cf_addr_tokens_by_balance(),
        IteratorMode::Start,
    );
    for item in addr_tokens_iter {
        let (key, value) = item?;
        if !value.is_empty() {
            bail!(
                "addr_tokens_by_balance value must be empty in core owner snapshot helper: value_len={}",
                value.len()
            );
        }
        let (lock_hash, balance, type_hash) = keys::decode_addr_token_balance_key(&key);
        addr_tokens
            .entry(lock_hash)
            .or_default()
            .insert(type_hash, balance);
    }
    let token_state = owners::token::TokenStateSnapshot {
        tokens,
        token_holders,
        addr_tokens,
        token_transfer_counts,
        token_hourly_transfers,
        token_daily_deltas,
    };

    let deposits = domain_store
        .list_dao_deposits()?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let page_limit = deposits.len().max(1);
    let mut withdraw_lookup: HashMap<Vec<u8>, HashMap<i16, Vec<u8>>> = HashMap::new();
    for (outpoint_key, entry) in &deposits {
        if let (Some(request_tx), Some(request_output_index)) = (
            entry.withdraw_request_tx.as_ref(),
            entry.withdraw_request_output_index,
        ) {
            let linked = domain_store
                .get_dao_deposit_by_withdraw_tx(request_tx, request_output_index)?
                .ok_or_else(|| {
                    anyhow!(
                        "dao_by_withdraw_tx missing in core owner snapshot helper: request_tx=0x{}, output_index={}",
                        hex::encode(request_tx),
                        request_output_index
                    )
                })?;
            withdraw_lookup
                .entry(request_tx.clone())
                .or_default()
                .insert(request_output_index, linked.clone());
            if linked != *outpoint_key {
                bail!(
                    "dao_by_withdraw_tx mismatch in core owner snapshot helper: request_tx=0x{}, output_index={}",
                    hex::encode(request_tx),
                    request_output_index
                );
            }
        }
    }
    let mut by_status = HashMap::new();
    for status in [0i16, 1, 2] {
        let outpoints = domain_store
            .list_dao_deposits_by_status_paginated(status, page_limit, None)?
            .into_iter()
            .map(|(outpoint, _entry)| outpoint)
            .collect::<Vec<_>>();
        by_status.insert(status, outpoints);
    }
    let mut by_lock: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
    for (outpoint_key, entry) in &deposits {
        let rows = domain_store
            .list_dao_deposits_by_lock_paginated(&entry.lock_script_hash, page_limit, None)?
            .into_iter()
            .map(|(outpoint, _entry)| outpoint)
            .collect::<Vec<_>>();
        if !rows.iter().any(|row| row == outpoint_key) {
            bail!(
                "dao_by_lock_block missing outpoint in core owner snapshot helper: outpoint=0x{}",
                hex::encode(outpoint_key)
            );
        }
        by_lock.insert(entry.lock_script_hash.clone(), rows);
    }
    let dao_state = owners::dao::DaoStateSnapshot {
        deposits,
        withdraw_lookup,
        by_status,
        by_lock,
    };

    let spores = domain_store
        .list_spores(usize::MAX)?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let identities = domain_store
        .list_identities(usize::MAX)?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let cluster_aggs = domain_store
        .list_cluster_aggregates()?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let did_agg = domain_store.get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)?;
    let mut identities_by_collection = HashMap::new();
    let mut did_ids = domain_store.list_identity_ids_by_collection(
        &DID_CKB_SENTINEL_COLLECTION,
        None,
        usize::MAX,
    )?;
    did_ids.sort();
    if !did_ids.is_empty() {
        identities_by_collection.insert(DID_CKB_SENTINEL_COLLECTION.to_vec(), did_ids);
    }
    let mut spores_by_cluster = HashMap::new();
    let mut cluster_owner_counts = HashMap::new();
    for cluster_id in cluster_aggs.keys() {
        let mut members = domain_store
            .list_spores_by_cluster(cluster_id, usize::MAX)?
            .into_iter()
            .map(|(spore_id, _entry)| spore_id)
            .collect::<Vec<_>>();
        members.sort();
        spores_by_cluster.insert(cluster_id.clone(), members);
        let owners = domain_store
            .list_cluster_owner_counts(cluster_id)?
            .into_iter()
            .collect::<HashMap<_, _>>();
        cluster_owner_counts.insert(cluster_id.clone(), owners);
    }
    let did_owner_counts = domain_store
        .list_identity_owner_counts(&DID_CKB_SENTINEL_COLLECTION)?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let mut spore_outpoints = HashMap::new();
    for (spore_id, entry) in &spores {
        if entry.standard != ObjectStandard::Spore {
            continue;
        }
        let mut outpoints = domain_store.list_spore_outpoints_by_spore_id(spore_id)?;
        outpoints.sort();
        spore_outpoints.insert(spore_id.clone(), outpoints);
    }
    let mut spore_type_indexes = HashMap::new();
    let stats_spore_iter =
        domain_store.iterator_cf(domain_store.cf_stats_spore(), IteratorMode::Start);
    for item in stats_spore_iter {
        let (key, value) = item?;
        if key.len() != keys::SPORE_TYPE_INDEX_KEY_SIZE
            || key[0] != keys::STATS_PREFIX_SPORE_TYPE_INDEX
        {
            continue;
        }
        let type_hash = key[1..33].to_vec();
        let index: SporeTypeIndex = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize SporeTypeIndex in core owner snapshot helper: type_hash=0x{}, error={}",
                hex::encode(&type_hash),
                e
            )
        })?;
        spore_type_indexes.insert(type_hash, index);
    }
    let object_state = owners::object::ObjectStateSnapshot {
        spores,
        identities,
        cluster_aggs,
        did_agg,
        identities_by_collection,
        spores_by_cluster,
        did_owner_counts,
        cluster_owner_counts,
        spore_outpoints,
        spore_type_indexes,
        ..owners::object::ObjectStateSnapshot::default()
    };

    Ok(CoreOwnerStateSnapshot {
        address_balances,
        script_infos,
        token_state,
        dao_state,
        object_state,
    })
}

pub(crate) fn unique_temp_test_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    static UNIQUE_TEMP_TEST_DIR_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = UNIQUE_TEMP_TEST_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    build_unique_temp_test_dir(prefix, std::process::id(), nanos, sequence)
}

fn build_unique_temp_test_dir(
    prefix: &str,
    process_id: u32,
    nanos: u128,
    sequence: u64,
) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ckbadger-{}-{}-{}-{}",
        prefix, process_id, nanos, sequence
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET;
    use crate::parser::spore::{
        CLUSTER_CODE_HASH_MAINNET_V2, SPORE_CODE_HASH_MAINNET_DID, SPORE_CODE_HASH_MAINNET_V2,
    };
    use crate::parser::udt::SUDT_CODE_HASH;
    use crate::parser::ScriptParser;
    use crate::rpc::{
        BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
        TransactionView,
    };
    use crate::sync::bulk_build::facts::{
        CellFacts, CellProtocolFacts, CellSemanticTag, DotbitProtocolFacts, MnftTokenProtocolFacts,
        OutPointKey, ResolvedInputFacts, ResolvedTxFacts,
    };
    use crate::sync::types::InternId;
    use ckbadger_store::store::CF_TOKEN_TRANSFERS;
    use ckbadger_store::types::{
        AssetAction, FiberChannelState, ObjectCollectionActivityEntry, TokenInfo,
        TokenTransferRecord, TxActivityBundle, DID_CKB_SENTINEL_COLLECTION,
        DOTBIT_SENTINEL_COLLECTION,
    };
    use ckbadger_store::{
        keys, CF_ACTIVITIES, CF_ADDR_TXS, CF_IDENTITY_COLLECTION_ACTIVITIES,
        CF_OBJECT_COLLECTION_ACTIVITIES,
    };

    fn open_empty_domain_store(name: &str) -> (CkbadgerStore, std::path::PathBuf) {
        let root = unique_temp_test_dir(name);
        std::fs::create_dir_all(&root).expect("create test dir");
        let store = CkbadgerStore::open_domain(&root).expect("open test domain store");
        (store, root)
    }

    #[test]
    fn unique_temp_test_dir_builder_avoids_same_timestamp_collisions() {
        let first = build_unique_temp_test_dir("bulk-build-core-owners", 42, 123456789, 0);
        let second = build_unique_temp_test_dir("bulk-build-core-owners", 42, 123456789, 1);

        assert_ne!(first, second);
    }

    fn fixture_lock_script(args_hex: &str) -> Script {
        Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: args_hex.to_string(),
        }
    }

    fn fixture_header(number: u64, hash_byte: u8) -> HeaderView {
        HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: "0x18c7b3b2b00".to_string(),
            number: format!("0x{number:x}"),
            epoch: "0x7080006000028".to_string(),
            parent_hash: format!("0x{}", "11".repeat(32)),
            transactions_root: format!("0x{}", "22".repeat(32)),
            proposals_hash: format!("0x{}", "33".repeat(32)),
            extra_hash: format!("0x{}", "44".repeat(32)),
            dao: format!("0x{}", "00".repeat(32)),
            nonce: "0x1".to_string(),
            hash: format!("0x{}", format!("{hash_byte:02x}").repeat(32)),
        }
    }

    fn fixture_header_with_timestamp(number: u64, hash_byte: u8, timestamp_ms: u64) -> HeaderView {
        HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: format!("0x{timestamp_ms:x}"),
            number: format!("0x{number:x}"),
            epoch: "0x7080006000028".to_string(),
            parent_hash: format!("0x{}", "11".repeat(32)),
            transactions_root: format!("0x{}", "22".repeat(32)),
            proposals_hash: format!("0x{}", "33".repeat(32)),
            extra_hash: format!("0x{}", "44".repeat(32)),
            dao: format!("0x{}", "00".repeat(32)),
            nonce: "0x1".to_string(),
            hash: format!("0x{}", format!("{hash_byte:02x}").repeat(32)),
        }
    }

    fn bulk_build_addr_tx_fixture() -> BlockResponseWithCycles {
        let lock_a_args = format!("0x{}", "01".repeat(20));
        let lock_b_args = format!("0x{}", "02".repeat(20));
        let create_tx = TransactionView {
            hash: format!("0x{}", "aa".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&lock_a_args),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        let split_tx = TransactionView {
            hash: format!("0x{}", "bb".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: create_tx.hash.clone(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_a_args),
                    type_: None,
                },
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_b_args),
                    type_: None,
                },
            ],
            outputs_data: vec!["0x".to_string(), "0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_000_888, 0x99),
                uncles: vec![],
                transactions: vec![create_tx, split_tx],
                proposals: vec![],
            },
            cycles: None,
        }
    }

    fn fixture_sudt_type_script() -> Script {
        Script {
            code_hash: SUDT_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", "11".repeat(20)),
        }
    }

    fn encode_molecule_bytes(data: &[u8]) -> Vec<u8> {
        let len = data.len() as u32;
        let mut result = len.to_le_bytes().to_vec();
        result.extend_from_slice(data);
        result
    }

    fn create_cluster_type_script(cluster_id: &[u8; 32]) -> Script {
        Script {
            code_hash: CLUSTER_CODE_HASH_MAINNET_V2.to_string(),
            hash_type: "data1".to_string(),
            args: format!("0x{}", hex::encode(cluster_id)),
        }
    }

    fn create_spore_type_script(spore_id: &[u8; 32]) -> Script {
        Script {
            code_hash: SPORE_CODE_HASH_MAINNET_V2.to_string(),
            hash_type: "data1".to_string(),
            args: format!("0x{}", hex::encode(spore_id)),
        }
    }

    fn create_did_type_script(did_id: &[u8; 32]) -> Script {
        Script {
            code_hash: SPORE_CODE_HASH_MAINNET_DID.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", hex::encode(did_id)),
        }
    }

    fn create_cluster_data(name: &str, description: &str) -> Vec<u8> {
        let name_bytes = encode_molecule_bytes(name.as_bytes());
        let description_bytes = encode_molecule_bytes(description.as_bytes());
        let offset_name = 16u32;
        let offset_description = offset_name + name_bytes.len() as u32;
        let offset_end = offset_description + description_bytes.len() as u32;

        let mut data = Vec::new();
        data.extend_from_slice(&offset_end.to_le_bytes());
        data.extend_from_slice(&offset_name.to_le_bytes());
        data.extend_from_slice(&offset_description.to_le_bytes());
        data.extend_from_slice(&offset_end.to_le_bytes());
        data.extend_from_slice(&name_bytes);
        data.extend_from_slice(&description_bytes);
        data
    }

    fn create_spore_data(
        content_type: &str,
        content: &[u8],
        cluster_id: Option<&[u8; 32]>,
    ) -> Vec<u8> {
        let content_type_bytes = encode_molecule_bytes(content_type.as_bytes());
        let content_bytes = encode_molecule_bytes(content);
        let cluster_id_bytes = cluster_id.map(|id| encode_molecule_bytes(id));

        let offset_content_type = 16u32;
        let offset_content = offset_content_type + content_type_bytes.len() as u32;
        let offset_cluster_id = offset_content + content_bytes.len() as u32;
        let total_size = offset_cluster_id
            + cluster_id_bytes
                .as_ref()
                .map(|bytes| bytes.len())
                .unwrap_or(0) as u32;

        let mut data = Vec::new();
        data.extend_from_slice(&total_size.to_le_bytes());
        data.extend_from_slice(&offset_content_type.to_le_bytes());
        data.extend_from_slice(&offset_content.to_le_bytes());
        data.extend_from_slice(&offset_cluster_id.to_le_bytes());
        data.extend_from_slice(&content_type_bytes);
        data.extend_from_slice(&content_bytes);
        if let Some(cluster_id_bytes) = cluster_id_bytes {
            data.extend_from_slice(&cluster_id_bytes);
        }
        data
    }

    fn fixture_fiber_funding_lock_script(args_hex: &str) -> Script {
        Script {
            code_hash: FUNDING_LOCK_CODE_HASH_MAINNET.to_string(),
            hash_type: "type".to_string(),
            args: args_hex.to_string(),
        }
    }

    fn u128_data_hex(amount: u128) -> String {
        format!("0x{}", hex::encode(amount.to_le_bytes()))
    }

    fn bulk_build_token_transfer_fixture() -> BlockResponseWithCycles {
        let lock_a_args = format!("0x{}", "01".repeat(20));
        let lock_b_args = format!("0x{}", "02".repeat(20));
        let sudt_type = fixture_sudt_type_script();
        let create_tx = TransactionView {
            hash: format!("0x{}", "c1".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&lock_a_args),
                type_: Some(sudt_type.clone()),
            }],
            outputs_data: vec![u128_data_hex(200)],
            witnesses: vec!["0x".to_string()],
        };

        let split_tx = TransactionView {
            hash: format!("0x{}", "d2".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: create_tx.hash.clone(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_a_args),
                    type_: Some(sudt_type.clone()),
                },
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_b_args),
                    type_: Some(sudt_type),
                },
            ],
            outputs_data: vec![u128_data_hex(100), u128_data_hex(100)],
            witnesses: vec!["0x".to_string()],
        };

        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_000_889, 0x9a),
                uncles: vec![],
                transactions: vec![create_tx, split_tx],
                proposals: vec![],
            },
            cycles: None,
        }
    }

    fn bulk_build_fiber_open_fixture() -> BlockResponseWithCycles {
        let participant_args = format!("0x{}", "03".repeat(20));
        let funding_args = format!("0x{}", "bb".repeat(20));
        let create_tx = TransactionView {
            hash: format!("0x{}", "f1".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&participant_args),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        let open_tx = TransactionView {
            hash: format!("0x{}", "f2".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: create_tx.hash.clone(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: format!("0x{:x}", 130_00000000u64),
                    lock: fixture_fiber_funding_lock_script(&funding_args),
                    type_: None,
                },
                CellOutput {
                    capacity: format!("0x{:x}", 70_00000000u64),
                    lock: fixture_lock_script(&participant_args),
                    type_: None,
                },
            ],
            outputs_data: vec!["0x".to_string(), "0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_000_990, 0xa7),
                uncles: vec![],
                transactions: vec![create_tx, open_tx],
                proposals: vec![],
            },
            cycles: None,
        }
    }

    fn bulk_build_object_activity_fixture() -> Vec<BlockResponseWithCycles> {
        let cluster_id = [0x11; 32];
        let spore_id = [0x22; 32];
        let did_id = [0x33; 32];

        let create_tx = TransactionView {
            hash: format!("0x{}", "a1".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: format!("0x{:x}", 200_00000000u64),
                    lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
                    type_: Some(create_cluster_type_script(&cluster_id)),
                },
                CellOutput {
                    capacity: format!("0x{:x}", 200_00000000u64),
                    lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
                    type_: Some(create_spore_type_script(&spore_id)),
                },
                CellOutput {
                    capacity: format!("0x{:x}", 150_00000000u64),
                    lock: fixture_lock_script(&format!("0x{}", "03".repeat(20))),
                    type_: Some(create_did_type_script(&did_id)),
                },
            ],
            outputs_data: vec![
                format!(
                    "0x{}",
                    hex::encode(create_cluster_data(
                        "Genesis Cluster",
                        "{\"dob\":{\"ver\":1}}"
                    ))
                ),
                format!(
                    "0x{}",
                    hex::encode(create_spore_data(
                        "image/png",
                        b"spore-content",
                        Some(&cluster_id)
                    ))
                ),
                "0x".to_string(),
            ],
            witnesses: vec!["0x".to_string()],
        };

        let dummy_cellbase = TransactionView {
            hash: format!("0x{}", "b0".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 500_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "09".repeat(20))),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        let transfer_and_burn_tx = TransactionView {
            hash: format!("0x{}", "b1".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![
                CellInput {
                    since: "0x0".to_string(),
                    previous_output: OutPoint {
                        tx_hash: create_tx.hash.clone(),
                        index: "0x1".to_string(),
                    },
                },
                CellInput {
                    since: "0x0".to_string(),
                    previous_output: OutPoint {
                        tx_hash: create_tx.hash.clone(),
                        index: "0x2".to_string(),
                    },
                },
            ],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "02".repeat(20))),
                type_: Some(create_spore_type_script(&spore_id)),
            }],
            outputs_data: vec![format!(
                "0x{}",
                hex::encode(create_spore_data(
                    "image/png",
                    b"spore-content",
                    Some(&cluster_id)
                ))
            )],
            witnesses: vec!["0x".to_string()],
        };

        vec![
            BlockResponseWithCycles {
                block: BlockView {
                    header: fixture_header_with_timestamp(14_001_000, 0x81, 1_700_000_000_000),
                    uncles: vec![],
                    transactions: vec![create_tx],
                    proposals: vec![],
                },
                cycles: None,
            },
            BlockResponseWithCycles {
                block: BlockView {
                    header: fixture_header_with_timestamp(14_001_001, 0x82, 1_700_000_010_000),
                    uncles: vec![],
                    transactions: vec![dummy_cellbase, transfer_and_burn_tx],
                    proposals: vec![],
                },
                cycles: None,
            },
        ]
    }

    #[test]
    fn build_history_rows_materializes_addr_txs_for_unique_touched_locks() {
        let block = bulk_build_addr_tx_fixture();
        let lock_a_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "01".repeat(20)
        )));
        let lock_b_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "02".repeat(20)
        )));
        let create_tx_hash =
            hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
        let split_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("split tx hash");

        let interner = interner::IdentityInterner::default();
        let (arena, _) =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");
        let frozen = interner.snapshot_for_reads();

        let (_test_store, test_root) = open_empty_domain_store("bulk-build-addr-tx-test");
        let addr_rows: Vec<_> =
            build_history_rows(&arena, &resolved, &frozen, true, &FxHashMap::default())
                .expect("history rows")
                .rows
                .into_iter()
                .filter(|row| row.cf_name == CF_ADDR_TXS)
                .collect();

        let expected = [
            keys::encode_addr_tx_key(&lock_a_hash, 14_000_888, 0, &create_tx_hash),
            keys::encode_addr_tx_key(&lock_a_hash, 14_000_888, 1, &split_tx_hash),
            keys::encode_addr_tx_key(&lock_b_hash, 14_000_888, 1, &split_tx_hash),
        ];

        assert_eq!(addr_rows.len(), expected.len());
        let actual_keys: HashSet<Vec<u8>> = addr_rows.iter().map(|row| row.key.clone()).collect();
        assert_eq!(actual_keys.len(), expected.len());
        for key in expected {
            assert!(actual_keys.contains(&key));
        }
        let _ = std::fs::remove_dir_all(&test_root);
    }

    #[test]
    fn build_history_rows_materializes_token_transfer_records_in_tx_order() {
        let block = bulk_build_token_transfer_fixture();
        let lock_a_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "01".repeat(20)
        )));
        let lock_b_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "02".repeat(20)
        )));
        let type_hash = ScriptParser::compute_script_hash(&fixture_sudt_type_script());
        let create_tx_hash =
            hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
        let split_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("split tx hash");

        let interner = interner::IdentityInterner::default();
        let (arena, _) =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");
        let frozen = interner.snapshot_for_reads();

        let (_test_store, test_root) = open_empty_domain_store("bulk-build-token-transfer-test");
        let token_rows: Vec<_> =
            build_history_rows(&arena, &resolved, &frozen, true, &FxHashMap::default())
                .expect("history rows")
                .rows
                .into_iter()
                .filter(|row| row.cf_name == CF_TOKEN_TRANSFERS)
                .collect();
        let _ = std::fs::remove_dir_all(&test_root);

        assert_eq!(token_rows.len(), 2);
        let token_records: HashMap<Vec<u8>, TokenTransferRecord> = token_rows
            .into_iter()
            .map(|row| {
                (
                    row.key,
                    bincode::deserialize(&row.value).expect("deserialize token transfer"),
                )
            })
            .collect();

        let mint_key = keys::encode_token_transfer_key(&type_hash, 14_000_889, 0);
        let transfer_key = keys::encode_token_transfer_key(&type_hash, 14_000_889, 1);
        let mint = token_records.get(&mint_key).expect("mint transfer");
        assert_eq!(mint.tx_hash, create_tx_hash);
        assert_eq!(mint.block_number, 14_000_889);
        assert_eq!(mint.from_lock_hash, None);
        assert_eq!(mint.to_lock_hash, lock_a_hash);
        assert_eq!(mint.amount, 200);
        assert!(mint.is_mint);
        assert!(!mint.is_burn);

        let transfer = token_records.get(&transfer_key).expect("split transfer");
        assert_eq!(transfer.tx_hash, split_tx_hash);
        assert_eq!(transfer.block_number, 14_000_889);
        assert_eq!(transfer.from_lock_hash, Some(lock_a_hash));
        assert_eq!(transfer.to_lock_hash, lock_b_hash);
        assert_eq!(transfer.amount, 100);
        assert!(!transfer.is_mint);
        assert!(!transfer.is_burn);
    }

    #[test]
    fn flush_bulk_build_materialized_state_flushes_domain_and_append_memtables() {
        let block = bulk_build_addr_tx_fixture();
        let interner = interner::IdentityInterner::default();
        let (arena, _) =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");

        let root = unique_temp_test_dir("bulk-build-flush-helper-test");
        std::fs::create_dir_all(&root).expect("create root dir");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("create domain dir");
        std::fs::create_dir_all(&append_path).expect("create append-only dir");

        let frozen = interner.snapshot_for_reads();
        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("open domain store");
        let append_store =
            CkbadgerStore::open_append_only(&append_path).expect("open append-only store");
        let history = build_history_rows(&arena, &resolved, &frozen, true, &FxHashMap::default())
            .expect("history rows");
        let mut materializer = materialize::Materializer::new(&domain_store, &append_store);
        materializer
            .stream_history_rows(&history.rows)
            .expect("stream history rows");

        let domain_stats_before = domain_store.memory_stats();
        let append_stats_before = append_store.memory_stats();
        assert!(
            domain_stats_before.memtable_bytes > 0 || append_stats_before.memtable_bytes > 0,
            "expected pending no-WAL memtable bytes before flush helper"
        );

        flush_bulk_build_materialized_state(&domain_store, &append_store)
            .expect("flush bulk build state");

        let domain_stats_after = domain_store.memory_stats();
        let append_stats_after = append_store.memory_stats();
        assert!(domain_stats_after.sst_files_size > domain_stats_before.sst_files_size);
        assert!(append_stats_after.sst_files_size > append_stats_before.sst_files_size);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn preload_token_info_cache_loads_label_data() {
        let (test_store, test_root) = open_empty_domain_store("bulk-build-preload-token-cache");
        let type_script = fixture_sudt_type_script();
        let type_hash = ScriptParser::compute_script_hash(&type_script);
        let type_args = crate::rpc::parse_hex_to_bytes(&type_script.args);
        test_store
            .put_token_direct(
                &type_hash,
                &TokenInfo {
                    type_code_hash: crate::rpc::parse_hex_to_bytes(&type_script.code_hash),
                    hash_type: 1,
                    type_args,
                    standard: "sUDT".to_string(),
                    name: Some("Seal".to_string()),
                    symbol: Some("SEAL".to_string()),
                    decimals: Some(8),
                    total_supply: Some(200),
                    max_supply: None,
                    holders_count: 1,
                    first_seen_block: 14_000_889,
                    icon_url: None,
                    description: None,
                    transfers_count: 1,
                },
            )
            .expect("put token");

        let cache = preload_token_info_cache(&test_store).expect("preload token info cache");
        assert_eq!(
            cache.get(&type_hash),
            Some(&(Some("SEAL".to_string()), Some(8)))
        );

        let _ = std::fs::remove_dir_all(&test_root);
    }

    #[test]
    fn build_history_rows_materializes_ckb_activity_bundles_in_tx_order() {
        let block = bulk_build_addr_tx_fixture();
        let create_tx_hash =
            hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
        let split_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("split tx hash");
        let lock_a_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "01".repeat(20)
        )));
        let lock_b_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "02".repeat(20)
        )));

        let interner = interner::IdentityInterner::default();
        let (arena, _) =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");
        let frozen = interner.snapshot_for_reads();

        let (_test_store, test_root) = open_empty_domain_store("bulk-build-activity-test");
        let activity_rows: Vec<_> =
            build_history_rows(&arena, &resolved, &frozen, true, &FxHashMap::default())
                .expect("history rows")
                .rows
                .into_iter()
                .filter(|row| row.cf_name == CF_ACTIVITIES)
                .collect();
        let _ = std::fs::remove_dir_all(&test_root);

        assert_eq!(activity_rows.len(), 2);
        let activity_bundles: HashMap<Vec<u8>, TxActivityBundle> = activity_rows
            .into_iter()
            .map(|row| {
                (
                    row.key,
                    bincode::deserialize(&row.value).expect("deserialize tx activity bundle"),
                )
            })
            .collect();

        let create_key = keys::encode_tx_activity_bundle_key(14_000_888, 0, &create_tx_hash);
        let split_key = keys::encode_tx_activity_bundle_key(14_000_888, 1, &split_tx_hash);
        let create_bundle = activity_bundles.get(&create_key).expect("cellbase bundle");
        assert_eq!(create_bundle.tx_hash, create_tx_hash);
        assert!(create_bundle.is_cellbase);
        assert_eq!(create_bundle.owners.len(), 1);

        let split_bundle = activity_bundles.get(&split_key).expect("split bundle");
        assert_eq!(split_bundle.tx_hash, split_tx_hash);
        assert!(!split_bundle.is_cellbase);
        assert_eq!(split_bundle.owners.len(), 2);

        let owner_a = split_bundle
            .owners
            .iter()
            .find(|owner| owner.lock_hash == lock_a_hash)
            .expect("owner a");
        assert_eq!(owner_a.ckb_delta, -100_00000000);
        assert!(owner_a.asset_changes.is_empty());
        assert_eq!(owner_a.peers, vec![lock_b_hash.clone()]);

        let owner_b = split_bundle
            .owners
            .iter()
            .find(|owner| owner.lock_hash == lock_b_hash)
            .expect("owner b");
        assert_eq!(owner_b.ckb_delta, 100_00000000);
        assert!(owner_b.asset_changes.is_empty());
        assert_eq!(owner_b.peers, vec![lock_a_hash]);
    }

    #[test]
    fn bulk_build_memory_breakdown_includes_hodl_live_cells_by_lock() {
        let mut runtime = BulkBuildRuntimeState::default();
        runtime.hodl_live_cells_by_lock.insert(InternId::new(7), 2);
        runtime.hodl_live_cells_by_lock.insert(InternId::new(8), 1);

        let breakdown = runtime.memory_breakdown_bytes();
        assert!(
            breakdown
                .get("hodl_live_cells_by_lock")
                .copied()
                .unwrap_or_default()
                > 0
        );
    }

    #[test]
    fn checked_unique_address_count_rejects_overflow() {
        let overflow_len = usize::try_from(u64::from(u32::MAX) + 1).expect("overflow len");
        let err = checked_unique_address_count(overflow_len, "daily 20260319")
            .expect_err("overflow should fail fast");
        assert!(err.to_string().contains("daily 20260319"));
        assert!(err.to_string().contains("unique_address_count"));
    }

    #[test]
    fn bulk_build_materializes_fiber_channels_via_core_owner_final_state() {
        let block = bulk_build_fiber_open_fixture();
        let open_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("open tx hash");
        let funding_args = hex::decode("bb".repeat(20)).expect("funding args");
        let expected_channel_id = keys::encode_fiber_channel_id(&open_tx_hash, 0);

        let interner = interner::IdentityInterner::default();
        let (arena, _) =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &interner)
                .expect("facts arena");
        let frozen = interner.snapshot_for_reads();
        let mut sequencer = sequencer::BulkSequencer::default();
        let resolved = sequencer.resolve(&arena).expect("resolved txs");
        let mut owners = CoreOwners::default();
        let ctx = owners::ReducerContext::new(&frozen);

        let root = unique_temp_test_dir("bulk-build-fiber-activity");
        std::fs::create_dir_all(&root).expect("create root dir");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("create domain dir");
        std::fs::create_dir_all(&append_path).expect("create append-only dir");

        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("open domain store");
        let history = build_history_rows(&arena, &resolved, &frozen, true, &FxHashMap::default())
            .expect("history rows");
        let sealed_rows =
            build_sealed_aggregate_rows(&history.activity_bundles).expect("sealed rows");
        let final_snapshot_rows =
            build_final_snapshot_rows(&sequencer, &frozen).expect("final snapshot rows");

        let open_bundle = history
            .rows
            .iter()
            .filter(|row| row.cf_name == CF_ACTIVITIES)
            .map(|row| {
                bincode::deserialize::<TxActivityBundle>(&row.value)
                    .expect("deserialize tx activity bundle")
            })
            .find(|bundle| !bundle.is_cellbase)
            .expect("non-cellbase activity bundle");
        let participant_owner = open_bundle
            .owners
            .iter()
            .find(|owner| !owner.protocol_actions.is_empty())
            .expect("fiber participant owner");
        assert_eq!(participant_owner.protocol_actions.len(), 1);
        assert_eq!(participant_owner.protocol_actions[0].protocol, "fiber");
        assert_eq!(participant_owner.protocol_actions[0].action, "channel_open");

        for tx in &resolved {
            owners.apply_tx(tx, &ctx).expect("apply core owners");
        }
        let append_store =
            CkbadgerStore::open_append_only(&append_path).expect("open append-only store");
        let mut materializer = materialize::Materializer::new(&domain_store, &append_store);
        materializer
            .stream_history_rows(&history.rows)
            .expect("stream history rows");
        materializer
            .stream_sealed_aggregate_rows(&sealed_rows)
            .expect("stream sealed rows");
        materializer
            .materialize_final_snapshot(&final_snapshot_rows)
            .expect("materialize final snapshot rows");
        owners
            .materialize_all(&mut materializer)
            .expect("materialize core owners");
        let _ = materializer.finish();

        let channels = domain_store
            .list_fiber_channels(10, None, None)
            .expect("list fiber channels");
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].0, expected_channel_id);
        assert_eq!(channels[0].1.state, FiberChannelState::Open);
        assert_eq!(channels[0].1.capacity, 130_00000000);
        assert_eq!(channels[0].1.funding_lock_args, funding_args);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn build_history_rows_materializes_spore_and_did_collection_activities() {
        let blocks = bulk_build_object_activity_fixture();
        let cluster_id = [0x11; 32];
        let create_block_hash = vec![0x81; 32];
        let transfer_block_hash = vec![0x82; 32];
        let create_tx_hash = vec![0xa1; 32];
        let transfer_tx_hash = vec![0xb1; 32];

        let interner = interner::IdentityInterner::default();
        let (arena, _) =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&blocks, &interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");
        let frozen = interner.snapshot_for_reads();

        let (_test_store, test_root) =
            open_empty_domain_store("bulk-build-spore-did-activity-test");
        let history_rows: Vec<_> =
            build_history_rows(&arena, &resolved, &frozen, true, &FxHashMap::default())
                .expect("history rows")
                .rows
                .into_iter()
                .filter(|row| {
                    row.cf_name == CF_OBJECT_COLLECTION_ACTIVITIES
                        || row.cf_name == CF_IDENTITY_COLLECTION_ACTIVITIES
                })
                .collect();
        let _ = std::fs::remove_dir_all(&test_root);

        let object_rows: std::collections::HashMap<Vec<u8>, ObjectCollectionActivityEntry> =
            history_rows
                .iter()
                .filter(|row| row.cf_name == CF_OBJECT_COLLECTION_ACTIVITIES)
                .map(|row| {
                    (
                        row.key.clone(),
                        bincode::deserialize(&row.value)
                            .expect("deserialize object collection activity"),
                    )
                })
                .collect();
        let identity_rows: std::collections::HashMap<Vec<u8>, ObjectCollectionActivityEntry> =
            history_rows
                .iter()
                .filter(|row| row.cf_name == CF_IDENTITY_COLLECTION_ACTIVITIES)
                .map(|row| {
                    (
                        row.key.clone(),
                        bincode::deserialize(&row.value)
                            .expect("deserialize identity collection activity"),
                    )
                })
                .collect();

        let cluster_mint_key = keys::encode_nft_collection_activity_key(
            &cluster_id,
            14_001_000,
            0,
            &create_block_hash,
            &create_tx_hash,
        );
        let cluster_transfer_key = keys::encode_nft_collection_activity_key(
            &cluster_id,
            14_001_001,
            1,
            &transfer_block_hash,
            &transfer_tx_hash,
        );
        let did_mint_key = keys::encode_nft_collection_activity_key(
            &DID_CKB_SENTINEL_COLLECTION,
            14_001_000,
            0,
            &create_block_hash,
            &create_tx_hash,
        );

        assert_eq!(object_rows.len(), 2);
        assert_eq!(identity_rows.len(), 1);

        let cluster_mint = object_rows
            .get(cluster_mint_key.as_slice())
            .expect("cluster mint activity");
        assert_eq!(cluster_mint.tx_hash, create_tx_hash);
        assert_eq!(cluster_mint.block_hash, create_block_hash);
        assert_eq!(cluster_mint.actions.len(), 1);
        assert!(matches!(cluster_mint.actions[0], AssetAction::Mint));

        let cluster_transfer = object_rows
            .get(cluster_transfer_key.as_slice())
            .expect("cluster transfer activity");
        assert_eq!(cluster_transfer.tx_hash, transfer_tx_hash);
        assert_eq!(cluster_transfer.block_hash, transfer_block_hash);
        assert_eq!(cluster_transfer.actions.len(), 1);
        assert!(matches!(cluster_transfer.actions[0], AssetAction::Transfer));

        let did_mint = identity_rows
            .get(did_mint_key.as_slice())
            .expect("did mint activity");
        assert_eq!(did_mint.tx_hash, create_tx_hash);
        assert_eq!(did_mint.block_hash, create_block_hash);
        assert_eq!(did_mint.actions.len(), 1);
        assert!(matches!(did_mint.actions[0], AssetAction::Mint));
    }

    #[test]
    fn build_object_collection_activity_rows_materializes_dotbit_identity_activities() {
        fn dotbit_output(
            tx_hash_byte: u8,
            lock_hash_id: InternId,
            account_id: [u8; 20],
            account: &str,
        ) -> CellFacts {
            CellFacts {
                outpoint: OutPointKey::new([tx_hash_byte; 32], 0),
                created_at_block: 0,
                created_by_block_dao_ar: 0,
                capacity: 200_00000000,
                lock_script_hash_id: lock_hash_id,
                lock_code_hash_id: InternId::new(401),
                lock_hash_type: 1,
                lock_args_id: InternId::new(402),
                type_script_hash_id: Some(InternId::new(403)),
                type_code_hash_id: Some(InternId::new(404)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(405)),
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Dotbit,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Dotbit(DotbitProtocolFacts {
                    account_id,
                    account: Some(account.to_string()),
                    next_account_id: None,
                    expired_at: Some(1_800_000_000),
                    registered_at: Some(1_700_000_000),
                    status: Some(0),
                })),
            }
        }

        fn dotbit_input(
            tx_hash_byte: u8,
            lock_hash_id: InternId,
            account_id: [u8; 20],
            account: &str,
        ) -> ResolvedInputFacts {
            ResolvedInputFacts {
                outpoint: OutPointKey::new([tx_hash_byte; 32], 0),
                created_at_block: 0,
                created_by_block_dao_ar: 0,
                capacity: 200_00000000,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data_hash: None,
                udt_amount: None,
                lock_script_hash_id: lock_hash_id,
                lock_code_hash_id: InternId::new(401),
                lock_hash_type: 1,
                lock_args_id: InternId::new(402),
                type_script_hash_id: Some(InternId::new(403)),
                type_code_hash_id: Some(InternId::new(404)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(405)),
                semantic_tag: CellSemanticTag::Dotbit,
                dao_state: None,
                dao_compensation_ars: None,
                protocol_facts: Some(CellProtocolFacts::Dotbit(DotbitProtocolFacts {
                    account_id,
                    account: Some(account.to_string()),
                    next_account_id: None,
                    expired_at: Some(1_800_000_000),
                    registered_at: Some(1_700_000_000),
                    status: Some(0),
                })),
            }
        }

        let account_a = [0x51; 20];
        let account_b = [0x61; 20];
        let owner_a = InternId::new(501);
        let owner_b = InternId::new(502);

        let resolved = vec![
            ResolvedTxFacts {
                tx_hash: [0x31; 32],
                block_number: 300,
                block_hash: [0xa0; 32],
                timestamp_ms: 1_700_100_000_000,
                block_dao_ar: 0,
                tx_index: 0,
                dotbit_action: Some("confirm_proposal".to_string()),
                resolved_inputs: Vec::new(),
                cells: vec![dotbit_output(0x31, owner_a, account_a, "alice.bit")].into(),
            },
            ResolvedTxFacts {
                tx_hash: [0x32; 32],
                block_number: 301,
                block_hash: [0xa1; 32],
                timestamp_ms: 1_700_100_360_000,
                block_dao_ar: 0,
                tx_index: 0,
                dotbit_action: Some("transfer_account".to_string()),
                resolved_inputs: vec![dotbit_input(0x31, owner_a, account_a, "alice.bit")],
                cells: vec![dotbit_output(0x32, owner_b, account_a, "alice.bit")].into(),
            },
            ResolvedTxFacts {
                tx_hash: [0x33; 32],
                block_number: 302,
                block_hash: [0xa2; 32],
                timestamp_ms: 1_700_100_720_000,
                block_dao_ar: 0,
                tx_index: 0,
                dotbit_action: Some("recycle_expired_account".to_string()),
                resolved_inputs: vec![dotbit_input(0x32, owner_b, account_a, "alice.bit")],
                cells: Vec::new().into(),
            },
            ResolvedTxFacts {
                tx_hash: [0x34; 32],
                block_number: 303,
                block_hash: [0xa3; 32],
                timestamp_ms: 1_700_101_080_000,
                block_dao_ar: 0,
                tx_index: 0,
                dotbit_action: Some("confirm_proposal".to_string()),
                resolved_inputs: vec![dotbit_input(0x40, owner_a, account_b, "bob.bit")],
                cells: vec![dotbit_output(0x34, owner_a, account_b, "bob.bit")].into(),
            },
        ];

        let mut object_activity_count_deltas = FxHashMap::default();
        let mut identity_activity_count_deltas = FxHashMap::default();
        let rows = build_object_collection_activity_rows(
            &resolved,
            &mut object_activity_count_deltas,
            &mut identity_activity_count_deltas,
        )
        .expect("dotbit collection activity rows");

        let identity_rows: HashMap<Vec<u8>, ObjectCollectionActivityEntry> = rows
            .iter()
            .filter(|row| row.cf_name == CF_IDENTITY_COLLECTION_ACTIVITIES)
            .map(|row| {
                (
                    row.key.clone(),
                    bincode::deserialize(&row.value)
                        .expect("deserialize identity collection activity"),
                )
            })
            .collect();

        let mint_key = keys::encode_nft_collection_activity_key(
            &DOTBIT_SENTINEL_COLLECTION,
            300,
            0,
            &[0xa0; 32],
            &[0x31; 32],
        );
        let transfer_key = keys::encode_nft_collection_activity_key(
            &DOTBIT_SENTINEL_COLLECTION,
            301,
            0,
            &[0xa1; 32],
            &[0x32; 32],
        );
        let recycle_key = keys::encode_nft_collection_activity_key(
            &DOTBIT_SENTINEL_COLLECTION,
            302,
            0,
            &[0xa2; 32],
            &[0x33; 32],
        );

        assert_eq!(identity_rows.len(), 3);
        assert_eq!(
            identity_activity_count_deltas.get(DOTBIT_SENTINEL_COLLECTION.as_slice()),
            Some(&3_i64)
        );

        let mint = identity_rows.get(mint_key.as_slice()).expect("dotbit mint");
        assert_eq!(mint.actions.len(), 1);
        assert!(matches!(mint.actions[0], AssetAction::Mint));

        let transfer = identity_rows
            .get(transfer_key.as_slice())
            .expect("dotbit transfer");
        assert_eq!(transfer.actions.len(), 1);
        assert!(matches!(transfer.actions[0], AssetAction::Transfer));

        let recycle = identity_rows
            .get(recycle_key.as_slice())
            .expect("dotbit recycle");
        assert_eq!(recycle.actions.len(), 1);
        assert!(matches!(recycle.actions[0], AssetAction::Recycle));
    }

    /// Regression: mNFT token collection activities must be recorded during
    /// bulk build. Previously only Spore/Dotbit were handled, leaving mNFT
    /// collection activities empty despite objects/holders being populated.
    #[test]
    fn build_object_collection_activity_rows_materializes_mnft_token_activities() {
        let class_id = vec![0xAA; 24]; // 24-byte mNFT class_id
        let token_id_a = vec![0xBB; 28]; // mNFT token_id
        let token_id_b = vec![0xCC; 28];

        fn mnft_output(
            tx_hash_byte: u8,
            lock_hash_id: InternId,
            class_id: Vec<u8>,
            token_id: Vec<u8>,
        ) -> CellFacts {
            CellFacts {
                outpoint: OutPointKey::new([tx_hash_byte; 32], 0),
                created_at_block: 0,
                created_by_block_dao_ar: 0,
                capacity: 200_00000000,
                lock_script_hash_id: lock_hash_id,
                lock_code_hash_id: InternId::new(301),
                lock_hash_type: 1,
                lock_args_id: InternId::new(302),
                type_script_hash_id: Some(InternId::new(303)),
                type_code_hash_id: Some(InternId::new(304)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(305)),
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Mnft,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::MnftToken(MnftTokenProtocolFacts {
                    token_id,
                    class_id,
                    token_index: 1,
                    characteristic: vec![0; 8],
                    configure: 0b00000011,
                    state: 0,
                })),
            }
        }

        fn mnft_input(
            tx_hash_byte: u8,
            lock_hash_id: InternId,
            class_id: Vec<u8>,
            token_id: Vec<u8>,
        ) -> ResolvedInputFacts {
            ResolvedInputFacts {
                outpoint: OutPointKey::new([tx_hash_byte; 32], 0),
                created_at_block: 0,
                created_by_block_dao_ar: 0,
                capacity: 200_00000000,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data_hash: None,
                udt_amount: None,
                lock_script_hash_id: lock_hash_id,
                lock_code_hash_id: InternId::new(301),
                lock_hash_type: 1,
                lock_args_id: InternId::new(302),
                type_script_hash_id: Some(InternId::new(303)),
                type_code_hash_id: Some(InternId::new(304)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(305)),
                semantic_tag: CellSemanticTag::Mnft,
                dao_state: None,
                dao_compensation_ars: None,
                protocol_facts: Some(CellProtocolFacts::MnftToken(MnftTokenProtocolFacts {
                    token_id,
                    class_id,
                    token_index: 1,
                    characteristic: vec![0; 8],
                    configure: 0b00000011,
                    state: 0,
                })),
            }
        }

        let owner_a = InternId::new(601);
        let owner_b = InternId::new(602);

        let resolved = vec![
            // TX1: Mint token_a
            ResolvedTxFacts {
                tx_hash: [0x41; 32],
                block_number: 400,
                block_hash: [0xb0; 32],
                timestamp_ms: 1_700_200_000_000,
                block_dao_ar: 0,
                tx_index: 0,
                dotbit_action: None,
                resolved_inputs: Vec::new(),
                cells: vec![mnft_output(
                    0x41,
                    owner_a,
                    class_id.clone(),
                    token_id_a.clone(),
                )]
                .into(),
            },
            // TX2: Transfer token_a (consume + create same token)
            ResolvedTxFacts {
                tx_hash: [0x42; 32],
                block_number: 401,
                block_hash: [0xb1; 32],
                timestamp_ms: 1_700_200_360_000,
                block_dao_ar: 0,
                tx_index: 0,
                dotbit_action: None,
                resolved_inputs: vec![mnft_input(
                    0x41,
                    owner_a,
                    class_id.clone(),
                    token_id_a.clone(),
                )],
                cells: vec![mnft_output(
                    0x42,
                    owner_b,
                    class_id.clone(),
                    token_id_a.clone(),
                )]
                .into(),
            },
            // TX3: Burn token_b (consume only)
            ResolvedTxFacts {
                tx_hash: [0x43; 32],
                block_number: 402,
                block_hash: [0xb2; 32],
                timestamp_ms: 1_700_200_720_000,
                block_dao_ar: 0,
                tx_index: 0,
                dotbit_action: None,
                resolved_inputs: vec![mnft_input(
                    0x50,
                    owner_a,
                    class_id.clone(),
                    token_id_b.clone(),
                )],
                cells: Vec::new().into(),
            },
        ];

        let mut object_activity_count_deltas = FxHashMap::default();
        let mut identity_activity_count_deltas = FxHashMap::default();
        let rows = build_object_collection_activity_rows(
            &resolved,
            &mut object_activity_count_deltas,
            &mut identity_activity_count_deltas,
        )
        .expect("mnft collection activity rows");

        // Should produce object rows, not identity rows
        let object_rows: HashMap<Vec<u8>, ObjectCollectionActivityEntry> = rows
            .iter()
            .filter(|row| row.cf_name == CF_OBJECT_COLLECTION_ACTIVITIES)
            .map(|row| {
                (
                    row.key.clone(),
                    bincode::deserialize(&row.value)
                        .expect("deserialize object collection activity"),
                )
            })
            .collect();

        assert_eq!(object_rows.len(), 3, "expected 3 mNFT activity rows");

        // No identity rows for mNFT
        let identity_count = rows
            .iter()
            .filter(|row| row.cf_name == CF_IDENTITY_COLLECTION_ACTIVITIES)
            .count();
        assert_eq!(identity_count, 0);

        // Verify mint
        let mint_key =
            keys::encode_nft_collection_activity_key(&class_id, 400, 0, &[0xb0; 32], &[0x41; 32]);
        let mint = object_rows.get(mint_key.as_slice()).expect("mnft mint");
        assert_eq!(mint.actions.len(), 1);
        assert!(matches!(mint.actions[0], AssetAction::Mint));

        // Verify transfer
        let transfer_key =
            keys::encode_nft_collection_activity_key(&class_id, 401, 0, &[0xb1; 32], &[0x42; 32]);
        let transfer = object_rows
            .get(transfer_key.as_slice())
            .expect("mnft transfer");
        assert_eq!(transfer.actions.len(), 1);
        assert!(matches!(transfer.actions[0], AssetAction::Transfer));

        // Verify burn
        let burn_key =
            keys::encode_nft_collection_activity_key(&class_id, 402, 0, &[0xb2; 32], &[0x43; 32]);
        let burn = object_rows.get(burn_key.as_slice()).expect("mnft burn");
        assert_eq!(burn.actions.len(), 1);
        assert!(matches!(burn.actions[0], AssetAction::Burn));

        // Verify object activity count deltas (regression: was missing before fix)
        assert_eq!(
            object_activity_count_deltas.get(class_id.as_slice()),
            Some(&3),
            "3 mNFT activity entries should produce delta of 3"
        );
        assert!(
            identity_activity_count_deltas.is_empty(),
            "no identity deltas for mNFT"
        );
    }

    /// Regression: bulk build must include genesis block 0 in the first batch.
    /// When block 0 is skipped, cells created in genesis are missing from the
    /// live cell map and later blocks that consume them fail with
    /// "missing live input".
    #[test]
    fn bulk_build_genesis_cells_available_for_consumption_in_later_block() {
        let lock_args = format!("0x{}", "01".repeat(20));

        // Block 0 (genesis): cellbase creates a cell
        let genesis_cellbase = TransactionView {
            hash: format!("0x{}", "e2".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 500_00000000u64),
                lock: fixture_lock_script(&lock_args),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };
        let genesis_block = BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(0, 0x00),
                uncles: vec![],
                transactions: vec![genesis_cellbase.clone()],
                proposals: vec![],
            },
            cycles: None,
        };

        // Block 11: tx consumes genesis cellbase output
        let block11_cellbase = TransactionView {
            hash: format!("0x{}", "fc".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 100_00000000u64),
                lock: fixture_lock_script(&lock_args),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };
        let spend_tx = TransactionView {
            hash: format!("0x{}", "dd".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: genesis_cellbase.hash.clone(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 499_00000000u64),
                lock: fixture_lock_script(&lock_args),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };
        let block_11 = BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(11, 0x0b),
                uncles: vec![],
                transactions: vec![block11_cellbase, spend_tx],
                proposals: vec![],
            },
            cycles: None,
        };

        // Process both blocks through bulk build - this must not error.
        // Before the fix, block 0 was skipped and this would fail with
        // "missing live input" at block 11.
        let interner = interner::IdentityInterner::default();
        let (arena, _) = crate::sync::pipeline::build_bulk_facts_arena_from_blocks(
            &[genesis_block, block_11],
            &interner,
        )
        .expect("facts arena");

        let mut sequencer = sequencer::BulkSequencer::default();
        let resolved = sequencer
            .resolve(&arena)
            .expect("resolve must succeed when genesis is included");

        // Block 0 cellbase: 0 inputs (cellbase), 1 output
        assert!(resolved[0].resolved_inputs.is_empty());
        // Block 11 cellbase: 0 inputs, 1 output
        assert!(resolved[1].resolved_inputs.is_empty());
        // Block 11 spend tx: 1 input (genesis cell), 1 output
        assert_eq!(resolved[2].resolved_inputs.len(), 1);
        assert_eq!(resolved[2].resolved_inputs[0].capacity, 500_00000000);
    }

    #[test]
    fn test_xudt_owner_mode_cell_without_amount_returns_none() {
        let interner = interner::IdentityInterner::default();
        let script_hash_id = interner.intern_bytes(vec![0xaa; 32]);
        let code_hash_id = interner.intern_bytes(vec![0xbb; 32]);
        let args_id = interner.intern_bytes(vec![0xcc; 20]);
        let lock_id = interner.intern_bytes(vec![0xdd; 32]);
        let frozen = interner.snapshot_for_reads();

        // xUDT cell with no amount (owner-mode) should return Ok(None), not error
        let result = parsed_udt_cell_from_parts(
            CellSemanticTag::Xudt,
            Some(script_hash_id),
            Some(code_hash_id),
            Some(1),
            Some(args_id),
            lock_id,
            None, // no udt_amount — owner-mode cell
            &frozen,
            "test xudt owner-mode",
        );
        assert!(result.is_ok(), "should not error on xUDT without amount");
        assert!(
            result.unwrap().is_none(),
            "should return None for owner-mode xUDT"
        );
    }

    #[test]
    fn test_xudt_cell_with_amount_returns_parsed_cell() {
        let interner = interner::IdentityInterner::default();
        let script_hash_id = interner.intern_bytes(vec![0xaa; 32]);
        let code_hash_id = interner.intern_bytes(vec![0xbb; 32]);
        let args_id = interner.intern_bytes(vec![0xcc; 20]);
        let lock_id = interner.intern_bytes(vec![0xdd; 32]);
        let frozen = interner.snapshot_for_reads();

        let result = parsed_udt_cell_from_parts(
            CellSemanticTag::Xudt,
            Some(script_hash_id),
            Some(code_hash_id),
            Some(1),
            Some(args_id),
            lock_id,
            Some(1000),
            &frozen,
            "test xudt with amount",
        );
        let cell = result
            .unwrap()
            .expect("should return Some for xUDT with amount");
        assert_eq!(cell.amount, 1000);
        assert!(matches!(cell.standard, UdtStandard::Xudt));
    }

    #[test]
    fn test_plain_cell_skipped_for_udt_processing() {
        let interner = interner::IdentityInterner::default();
        let lock_id = interner.intern_bytes(vec![0xdd; 32]);
        let frozen = interner.snapshot_for_reads();

        let result = parsed_udt_cell_from_parts(
            CellSemanticTag::Plain,
            None,
            None,
            None,
            None,
            lock_id,
            None,
            &frozen,
            "test plain cell",
        );
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_bincode_serialize_presized_matches_standard() {
        // Verify presized produces identical output to standard bincode::serialize.
        let header = CachedBlockHeader {
            hash: vec![0xaa; 32],
            timestamp: 1710000000000,
            epoch_number: 100,
            epoch_index: 5,
            epoch_length: 1800,
            dao: vec![0x00; 32],
            transactions_count: 42,
        };
        let standard = bincode::serialize(&header).unwrap();
        let presized = bincode_serialize_presized(&header).unwrap();
        assert_eq!(standard, presized);
    }

    #[test]
    fn test_bincode_serialize_presized_small_struct() {
        let entry = TxIndexEntry {
            is_cellbase: false,
            timestamp: 1710000000000,
            inputs_count: 3,
            outputs_count: 2,
            fee: 100_000,
            tx_size: 512,
            cycles: Some(1_000_000),
        };
        let standard = bincode::serialize(&entry).unwrap();
        let presized = bincode_serialize_presized(&entry).unwrap();
        assert_eq!(standard, presized);
    }

    #[test]
    fn test_bincode_serialize_presized_empty_vec_field() {
        let header = CachedBlockHeader {
            hash: vec![],
            timestamp: 0,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 0,
            dao: vec![],
            transactions_count: 0,
        };
        let standard = bincode::serialize(&header).unwrap();
        let presized = bincode_serialize_presized(&header).unwrap();
        assert_eq!(standard, presized);
    }

    #[test]
    fn prepare_finalize_artifacts_matches_direct_finalize_components() {
        let mut runtime = BulkBuildRuntimeState::default();
        let block = bulk_build_addr_tx_fixture();
        runtime
            .apply_blocks_hex(std::slice::from_ref(&block), true, &FxHashMap::default())
            .unwrap();

        let direct_activity_rows = runtime.activity_stats.build_rows().unwrap();
        let direct_chain_rows = runtime.chain_stats.build_rows().unwrap();
        let frozen = runtime.interner.snapshot_for_reads();
        let direct_snapshot_rows = build_final_snapshot_rows(&runtime.sequencer, &frozen).unwrap();

        let prepared = runtime.prepare_finalize_artifacts().unwrap();
        assert_eq!(prepared.activity_sealed_rows, direct_activity_rows);
        assert_eq!(prepared.chain_sealed_rows, direct_chain_rows);
        assert_eq!(prepared.final_snapshot_rows, direct_snapshot_rows);
    }

    #[test]
    fn apply_bundles_accumulates_daily_and_hourly_stats() {
        let bundle = TxActivityBundle {
            tx_hash: vec![0x11; 32],
            block_hash: vec![0x22; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1_700_000_000_000, // 2023-11-14 22:13:20 UTC
            is_cellbase: false,
            owners: vec![ckbadger_store::types::OwnerActivityDelta {
                lock_hash: vec![0x33; 32],
                lock_code_hash: vec![0x44; 32],
                lock_hash_type: 1,
                lock_args: vec![0x55; 20],
                ckb_delta: 100_00000000,
                used_delta: 0,
                has_type_script: false,
                involved_script_code_hashes: vec![vec![0x44; 32]],
                asset_changes: vec![],
                type_calls: None,
                lock_calls: None,
                protocol_actions: vec![],
                peers: vec![],
            }],
        };

        let mut acc = ActivityStatsAccumulator::default();
        acc.apply_bundles(&[bundle]).unwrap();

        let date_key = ckbadger_common::block_date_from_ms(1_700_000_000_000)
            .format("%Y%m%d")
            .to_string();
        let daily = acc.daily_stats.get(&date_key).expect("daily stats");
        assert_eq!(daily.transfer_count, 1, "pure CKB transfer");
        assert_eq!(daily.total_ckb_moved, 100_00000000);
        assert_eq!(daily.coinbase_count, 0);
        assert_eq!(acc.daily_addrs.get(&date_key).unwrap().len(), 1);

        let hour_key = ckbadger_common::block_datetime_from_ms(1_700_000_000_000)
            .format("%Y%m%d%H")
            .to_string();
        assert!(acc.hourly_stats.contains_key(&hour_key));
        assert_eq!(acc.hourly_addrs.get(&hour_key).unwrap().len(), 1);
    }

    #[test]
    fn apply_bundles_excludes_coinbase_from_unique_addrs() {
        let bundle = TxActivityBundle {
            tx_hash: vec![0x11; 32],
            block_hash: vec![0x22; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1_700_000_000_000,
            is_cellbase: true,
            owners: vec![ckbadger_store::types::OwnerActivityDelta {
                lock_hash: vec![0x33; 32],
                lock_code_hash: vec![0x44; 32],
                lock_hash_type: 1,
                lock_args: vec![0x55; 20],
                ckb_delta: 100_00000000,
                used_delta: 0,
                has_type_script: false,
                involved_script_code_hashes: vec![],
                asset_changes: vec![],
                type_calls: None,
                lock_calls: None,
                protocol_actions: vec![],
                peers: vec![],
            }],
        };

        let mut acc = ActivityStatsAccumulator::default();
        acc.apply_bundles(&[bundle]).unwrap();

        let date_key = ckbadger_common::block_date_from_ms(1_700_000_000_000)
            .format("%Y%m%d")
            .to_string();
        let daily = acc.daily_stats.get(&date_key).expect("daily stats");
        assert_eq!(daily.coinbase_count, 1);
        assert_eq!(daily.transfer_count, 0);
        assert!(!acc.daily_addrs.contains_key(&date_key) || acc.daily_addrs[&date_key].is_empty());
    }

    // -----------------------------------------------------------------------
    // ChainStatsAccumulator tests
    // -----------------------------------------------------------------------

    #[test]
    fn chain_stats_accumulator_daily_stats_from_two_blocks() {
        use std::borrow::Cow;

        // Two blocks on the same day, 10 seconds apart.
        // Block 1: timestamp 2023-11-14 22:13:20 UTC, 1 cellbase tx with 1 output
        // Block 2: timestamp 2023-11-14 22:13:30 UTC, 1 cellbase + 1 spend tx
        let ts1: i64 = 1_700_000_000_000;
        let ts2: i64 = 1_700_000_010_000; // +10s

        // Manually construct arena + resolved for direct accumulator test.
        let arena = facts::FactsArena {
            blocks: vec![
                facts::BlockFacts {
                    number: 100,
                    hash: [0x01; 32],
                    timestamp_ms: ts1,
                    epoch_number: 5,
                    epoch_index: 10,
                    epoch_length: 1800,
                    dao: [0x00; 32],
                    compact_target: 0x1a08a97e,
                    uncles_count: 0,
                    transactions_count: 1,
                    tx_range: 0..1,
                },
                facts::BlockFacts {
                    number: 101,
                    hash: [0x02; 32],
                    timestamp_ms: ts2,
                    epoch_number: 5,
                    epoch_index: 11,
                    epoch_length: 1800,
                    dao: [0x00; 32],
                    compact_target: 0x1a08a97e,
                    uncles_count: 1,
                    transactions_count: 2,
                    tx_range: 1..3,
                },
            ],
            txs: vec![
                // Block 100: cellbase
                facts::TxFacts {
                    hash: [0xaa; 32],
                    block_number: 100,
                    block_hash: [0x01; 32],
                    timestamp_ms: ts1,
                    block_dao_ar: 10_000_000_000,
                    tx_index: 0,
                    is_cellbase: true,
                    inputs_count: 1,
                    outputs_count: 1,
                    tx_size: 120,
                    cycles: Some(0),
                    dotbit_action: None,
                    input_outpoints: Vec::new(),
                    output_range: 0..1,
                },
                // Block 101: cellbase
                facts::TxFacts {
                    hash: [0xbb; 32],
                    block_number: 101,
                    block_hash: [0x02; 32],
                    timestamp_ms: ts2,
                    block_dao_ar: 10_000_000_000,
                    tx_index: 0,
                    is_cellbase: true,
                    inputs_count: 1,
                    outputs_count: 1,
                    tx_size: 120,
                    cycles: Some(0),
                    dotbit_action: None,
                    input_outpoints: Vec::new(),
                    output_range: 1..2,
                },
                // Block 101: spend tx (consumes cell from block 100 cellbase)
                facts::TxFacts {
                    hash: [0xcc; 32],
                    block_number: 101,
                    block_hash: [0x02; 32],
                    timestamp_ms: ts2,
                    block_dao_ar: 10_000_000_000,
                    tx_index: 1,
                    is_cellbase: false,
                    inputs_count: 1,
                    outputs_count: 1,
                    tx_size: 200,
                    cycles: Some(1000),
                    dotbit_action: None,
                    input_outpoints: vec![facts::OutPointKey::new([0xaa; 32], 0)],
                    output_range: 2..3,
                },
            ],
            cells: vec![
                // Block 100 cellbase output
                facts::CellFacts {
                    outpoint: facts::OutPointKey::new([0xaa; 32], 0),
                    created_at_block: 100,
                    created_by_block_dao_ar: 10_000_000_000,
                    capacity: 100_00000000,
                    lock_script_hash_id: InternId::new(0),
                    lock_code_hash_id: InternId::new(1),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(2),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 10,
                    data: vec![0; 10],
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: facts::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
                // Block 101 cellbase output
                facts::CellFacts {
                    outpoint: facts::OutPointKey::new([0xbb; 32], 0),
                    created_at_block: 101,
                    created_by_block_dao_ar: 10_000_000_000,
                    capacity: 50_00000000,
                    lock_script_hash_id: InternId::new(0),
                    lock_code_hash_id: InternId::new(1),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(2),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: facts::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
                // Block 101 spend tx output
                facts::CellFacts {
                    outpoint: facts::OutPointKey::new([0xcc; 32], 0),
                    created_at_block: 101,
                    created_by_block_dao_ar: 10_000_000_000,
                    capacity: 99_00000000,
                    lock_script_hash_id: InternId::new(3),
                    lock_code_hash_id: InternId::new(4),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(5),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 5,
                    data: vec![0; 5],
                    data_hash: None,
                    udt_amount: None,
                    semantic_tag: facts::CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
            ],
        };

        // Build resolved tx facts manually (can't use sequencer since we need
        // to keep arena alive for the borrow).
        let resolved_input = facts::ResolvedInputFacts {
            outpoint: facts::OutPointKey::new([0xaa; 32], 0),
            created_at_block: 100,
            created_by_block_dao_ar: 10_000_000_000,
            capacity: 100_00000000,
            occupied_capacity: 61_00000000,
            data_size: 10,
            data_hash: None,
            udt_amount: None,
            lock_script_hash_id: InternId::new(0),
            lock_code_hash_id: InternId::new(1),
            lock_hash_type: 1,
            lock_args_id: InternId::new(2),
            type_script_hash_id: None,
            type_code_hash_id: None,
            type_hash_type: None,
            type_args_id: None,
            semantic_tag: facts::CellSemanticTag::Plain,
            dao_state: None,
            dao_compensation_ars: None,
            protocol_facts: None,
        };

        let resolved: Vec<facts::ResolvedTxFacts<'_>> = vec![
            // Block 100 cellbase
            facts::ResolvedTxFacts {
                tx_hash: [0xaa; 32],
                block_number: 100,
                block_hash: [0x01; 32],
                timestamp_ms: ts1,
                block_dao_ar: 10_000_000_000,
                tx_index: 0,
                dotbit_action: None,
                resolved_inputs: vec![],
                cells: Cow::Borrowed(&arena.cells[0..1]),
            },
            // Block 101 cellbase
            facts::ResolvedTxFacts {
                tx_hash: [0xbb; 32],
                block_number: 101,
                block_hash: [0x02; 32],
                timestamp_ms: ts2,
                block_dao_ar: 10_000_000_000,
                tx_index: 0,
                dotbit_action: None,
                resolved_inputs: vec![],
                cells: Cow::Borrowed(&arena.cells[1..2]),
            },
            // Block 101 spend
            facts::ResolvedTxFacts {
                tx_hash: [0xcc; 32],
                block_number: 101,
                block_hash: [0x02; 32],
                timestamp_ms: ts2,
                block_dao_ar: 10_000_000_000,
                tx_index: 1,
                dotbit_action: None,
                resolved_inputs: vec![resolved_input],
                cells: Cow::Borrowed(&arena.cells[2..3]),
            },
        ];

        let mut acc = ChainStatsAccumulator::default();
        acc.apply_blocks(&arena, &resolved).unwrap();

        // Both blocks are on the same date: 2023-11-14
        let date = ckbadger_common::block_date_from_ms(ts1);
        let daily = acc.daily_stats.get(&date).expect("daily stats entry");

        // blocks: 2
        assert_eq!(daily.0, 2, "blocks_count");
        // txs: block 100 has 1, block 101 has 2
        assert_eq!(daily.1, 3, "transactions_count");
        // cells_created: block 100 cellbase=1, block 101 cellbase=1 + spend=1 = 3
        assert_eq!(daily.2, 3, "cells_created");
        // cells_consumed: only non-cellbase tx in block 101 consumes 1
        assert_eq!(daily.3, 1, "cells_consumed");
        // capacity_transferred: spend tx output = 99 CKB (non-cellbase only)
        assert_eq!(daily.4, 99_00000000i128, "capacity_transferred");
        // used_cap_created: 61 + 61 + 61 = 183 CKB (all cells)
        assert_eq!(daily.5, 183_00000000i128, "used_capacity_created");
        // used_cap_consumed: 61 CKB (the consumed input)
        assert_eq!(daily.6, 61_00000000i128, "used_capacity_consumed");
        // data_size_added: 10 + 0 + 5 = 15
        assert_eq!(daily.7, 15, "data_size_added");
        // data_size_consumed: 10 (the consumed input had data_size=10)
        assert_eq!(daily.8, 10, "data_size_consumed");

        // DailyBlockStats: both blocks on same day
        let block_stats = acc.daily_block_stats.get(&date).expect("daily block stats");
        // sum_compact_target: 0x1a08a97e * 2
        assert_eq!(block_stats.0, 0x1a08a97e_i128 * 2, "sum_compact_target");
        assert_eq!(block_stats.1, 2, "block_count");
        // uncles: block 100=0, block 101=1
        assert_eq!(block_stats.2, 1, "total_uncles");

        // Block time: 10 seconds between blocks
        let bt = acc.daily_block_times.get(&date).expect("block times");
        assert_eq!(bt.0, 10_000, "block_time_sum_ms");
        assert_eq!(bt.1, 1, "block_time_count");

        // Block time distribution: 10s bucket
        assert_eq!(acc.block_time_dist.get(&10), Some(&1));

        // No epoch boundary crossing, so epoch_time_dist should be empty
        assert!(acc.epoch_time_dist.is_empty());
    }

    #[test]
    fn chain_stats_accumulator_build_rows_threads_cumulative_totals() {
        use chrono::NaiveDate;

        let mut acc = ChainStatsAccumulator::default();

        // Day 1: 5 created, 2 consumed
        let day1 = NaiveDate::from_ymd_opt(2023, 11, 14).unwrap();
        acc.daily_stats
            .insert(day1, (10, 20, 5, 2, 1000, 500, 200, 100, 30));

        // Day 2: 3 created, 1 consumed
        let day2 = NaiveDate::from_ymd_opt(2023, 11, 15).unwrap();
        acc.daily_stats
            .insert(day2, (8, 15, 3, 1, 800, 300, 100, 50, 10));

        let rows = acc.build_rows().unwrap();

        // Find daily stats rows (prefix DAILY = 0x01)
        let daily_rows: Vec<_> = rows
            .iter()
            .filter(|r| {
                r.cf_name == CF_STATS_CHAIN
                    && r.key.len() > 1
                    && r.key[0] == ckbadger_store::keys::stats_prefix::DAILY
            })
            .collect();
        assert_eq!(daily_rows.len(), 2, "two daily stats rows");

        // Deserialize day 1
        let stats1: ckbadger_store::types::DailyStats =
            bincode::deserialize(&daily_rows[0].value).unwrap();
        assert_eq!(stats1.total_live_cells, 3); // 5-2
        assert_eq!(stats1.total_dead_cells, 2);
        assert_eq!(stats1.total_all_cells, 5);
        assert_eq!(stats1.total_data_size, 70); // 100-30

        // Deserialize day 2 (cumulative from day 1)
        let stats2: ckbadger_store::types::DailyStats =
            bincode::deserialize(&daily_rows[1].value).unwrap();
        assert_eq!(stats2.total_live_cells, 5); // 3 + (3-1) = 5
        assert_eq!(stats2.total_dead_cells, 3); // 2 + 1
        assert_eq!(stats2.total_all_cells, 8); // 5 + 3
        assert_eq!(stats2.total_data_size, 110); // 70 + (50-10)
    }

    #[test]
    fn chain_stats_accumulator_epoch_time_distribution() {
        let mut acc = ChainStatsAccumulator::default();

        // Simulate epoch boundary: epoch 5 starts at ts=0, epoch 6 starts 240 min later
        let arena = facts::FactsArena {
            blocks: vec![
                facts::BlockFacts {
                    number: 1000,
                    epoch_number: 5,
                    epoch_index: 0,
                    timestamp_ms: 1_700_000_000_000,
                    ..Default::default()
                },
                facts::BlockFacts {
                    number: 2800,
                    epoch_number: 6,
                    epoch_index: 0,
                    // 240 minutes = 14,400,000 ms later
                    timestamp_ms: 1_700_000_000_000 + 14_400_000,
                    ..Default::default()
                },
            ],
            txs: vec![],
            cells: vec![],
        };

        let resolved: Vec<facts::ResolvedTxFacts<'_>> = vec![];
        acc.apply_blocks(&arena, &resolved).unwrap();

        // Epoch 5→6 boundary: 240 minutes
        assert_eq!(acc.epoch_time_dist.get(&240), Some(&1));
    }
}
