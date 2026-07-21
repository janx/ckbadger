use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rustc_hash::{FxHashMap, FxHashSet};

use anyhow::Result;
use anyhow::{anyhow, bail};
use ckb_types::utilities::compact_to_difficulty as ckb_compact_to_difficulty;
use ckbadger_common::TokenBalance;
use ckbadger_store::keys;
use ckbadger_store::store::CF_TOKEN_TRANSFERS;
use ckbadger_store::types::{
    decode_live_cell_marker, BulkBuildSessionMarker, CachedBlockHeader,
    CellDistributionTrackerState, ConsumedCellMeta, DailyActivityStats, DailyAddressCohort,
    DailyCellDistribution, DailyHodlWave, DaoDailySnapshot, DaoLatestStatistics, DaoTopDepositors,
    HodlTrackerState, LiveCellInfo, LockScriptEntry, ObjectStandard, ScriptDailyDelta,
    SporeTypeIndex, SyncStatus, TokenTransferRecord, TxActions, TxIndexEntry,
    BIT_CELL_SENTINEL_COLLECTION, DID_CKB_SENTINEL_COLLECTION, DOTBIT_SENTINEL_COLLECTION,
    SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::{
    AddressBalance, CkbadgerStore, ScriptInfo, CF_ADDR_TXS, CF_BLOCK_HASH_INDEX, CF_BLOCK_HEADERS,
    CF_CELLS, CF_CELL_BY_DATA_HASH, CF_CELL_BY_LOCK, CF_CELL_BY_LOCK_CODE, CF_CELL_BY_TYPE,
    CF_CELL_BY_TYPE_CODE, CF_CONSUMED_CELLS, CF_IDENTITY_COLLECTION_ACTIVITIES, CF_LIVE_CELLS,
    CF_LOCK_SCRIPTS, CF_OBJECT_COLLECTION_ACTIVITIES, CF_STATS_CHAIN, CF_STATS_HODL, CF_TX_ACTIONS,
    CF_TX_HASH_MAP, CF_TX_INDEX,
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
pub(crate) mod block_buffer;
pub(crate) mod facts;
pub(crate) mod interner;
pub(crate) mod live_cells;
pub(crate) mod materialize;
pub(crate) mod memory_guard;
pub(crate) mod owners;
pub(crate) mod prefetch;
pub(crate) mod sampler;
pub(crate) mod sequencer;

use crate::sync::bottleneck::{self, BatchSignals, BottleneckController};

#[derive(Debug, Default, PartialEq, Eq)]
struct PreparedFinalizeArtifacts {
    activity_sealed_rows: Vec<materialize::MaterializedRow>,
    chain_sealed_rows: Vec<materialize::MaterializedRow>,
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
        persist_bulk_sync_completion_status(
            indexer.writer.store().as_ref(),
            indexer.progress.target(),
        )?;
        indexer.finalize_bulk_sync_perf_completed();
        info!(
            run_id = %indexer.run_id,
            current_block = indexer.progress.current(),
            target_block = indexer.progress.target(),
            threshold = indexer.config.bulk_sync_threshold,
            "Bulk build stage finalized; exiting cleanly so near-tip sync starts with a reclaimed heap"
        );
        Ok(())
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
        let mem_profile = indexer.writer.store().memory_profile();
        let memory_guard = memory_guard::BulkMemoryGuard::new(
            indexer.config.bulk_memory_budget_gb,
            mem_profile.system_ram_bytes,
        )?;
        info!(
            run_id = %indexer.run_id,
            process_memory_budget_bytes = memory_guard.limit_bytes(),
            configured_process_memory_budget_gb = ?indexer.config.bulk_memory_budget_gb,
            "Bulk build whole-process memory guard enabled"
        );
        let prefetch_depth =
            bottleneck::prefetch_channel_depth(mem_profile.system_ram_bytes) as usize;
        let flush_depth = bottleneck::flush_channel_depth(mem_profile.system_ram_bytes) as usize;
        // Max = available cores.  Fetch threads are temporary (std::thread::scope),
        // so no persistent over-subscription.  The controller shrinks this when
        // build-bound to reduce overlap contention.
        let max_fetch_threads = std::thread::available_parallelism()
            .map(|n| n.get().max(2) as u32)
            .unwrap_or(4);
        let mut controller = BottleneckController::new(
            200_000, // 200K initial cell target
            max_fetch_threads,
            mem_profile.max_background_jobs,
            mem_profile.system_ram_bytes,
        );
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
        let (threads_tx, threads_rx) = tokio::sync::watch::channel(controller.fetch_threads());
        let mut prefetch = prefetch::PrefetchChannelHandle::new(
            prefetch_depth,
            ckb_store.clone(),
            prefetch_start,
            initial_handoff,
            threads_rx,
        );
        let chunk_rx = prefetch.take_receiver();
        let mut buffer = block_buffer::BlockBufferHandle::new(chunk_rx);
        // Bounded flush channel: the build loop sends PendingFlush into
        // a channel. A dedicated worker converts rows to WriteBatch via
        // prepare_flush and commits to RocksDB. Build only blocks when the
        // channel is full, eliminating the flush bubble when flush_ms > build_ms.
        let flush_queue_budget_bytes = (memory_guard.limit_bytes() / 8).max(64 * 1024 * 1024);
        let flush_channel = materialize::FlushChannelHandle::new(
            flush_depth,
            flush_queue_budget_bytes,
            indexer.writer.store().clone(),
            indexer.append_only_store.clone(),
        )?;
        // Initial 0.0 is semantically correct (no flush yet) but always
        // overwritten by flush_channel.last_flush_ms() before first read.
        #[allow(unused_assignments)]
        let mut prev_flush_ms: f64 = 0.0;
        let mut cumulative_history_rows: usize = 0;
        let mut cumulative_sealed_rows: usize = 0;
        let mut _flush_send_count: usize = 0;
        let mut last_bottleneck: Option<String> = None;
        let mut last_owner_memory = runtime.memory_breakdown_bytes();

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

            let batch_span = tracing::info_span!(
                "bulk_batch",
                batch_index = batch_count,
                start_block = current_block,
                end_block = tracing::field::Empty,
            );

            // Fill buffer to match cell target.  fill_to_cell_budget seeds the
            // buffer with one chunk first (if empty) to get real cell density,
            // then pulls enough chunks for target_cells.
            let recv_started = Instant::now();
            match buffer.fill_to_cell_budget(controller.target_cells()).await {
                Ok(true) => {}
                Ok(false) => {
                    info!("block buffer exhausted, ending bulk build loop");
                    break;
                }
                Err(e) => {
                    return Err(e.context("prefetch error during bulk build"));
                }
            }
            let prefetch_recv_elapsed = recv_started.elapsed();

            let process_memory =
                memory_guard.checkpoint("before_batch", current_block, &last_owner_memory)?;
            let safe_max_batch_bytes = memory_guard.safe_batch_input_bytes(
                process_memory,
                controller.max_batch_bytes(),
                current_block,
                &last_owner_memory,
            )?;

            // Enter the batch span for the synchronous build + record section.
            // Dropped explicitly before the next .await (flush_channel.send).
            let _batch_guard = batch_span.enter();

            // Drain by cell count with byte safety cap.
            let drained = buffer.drain_by_cells(controller.target_cells(), safe_max_batch_bytes);
            let batch_block_count = drained.len() as u64;
            let batch_cells: u64 = drained.iter().map(|b| b.cell_count).sum();
            let batch_bytes: u64 = drained.iter().map(|b| b.block_bytes as u64).sum();

            // Extract raw blocks for apply_blocks.
            let raw_blocks: Vec<_> = drained.into_iter().map(|b| b.raw).collect();

            let build_started = Instant::now();
            let (batch_stats, build_timings, pending_flush) = runtime.apply_blocks(
                &raw_blocks,
                indexer.config.is_mainnet(),
                &token_info_cache,
            )?;
            let build_elapsed = build_started.elapsed();
            let memory_accounting_started = Instant::now();
            last_owner_memory = runtime.memory_breakdown_bytes();
            let memory_accounting_elapsed = memory_accounting_started.elapsed();
            memory_guard.checkpoint("after_batch_build", current_block, &last_owner_memory)?;

            // Read the most recent flush_ms from the worker (non-blocking).
            prev_flush_ms = flush_channel.last_flush_ms();
            let critical_stage_ms = (build_elapsed.as_secs_f64() * 1000.0).max(prev_flush_ms);

            // Capture row counts before send() moves the data.
            let pending_flush_row_count = (
                pending_flush.history_rows.len(),
                pending_flush.sealed_rows.len(),
            );

            // Drop the batch span guard before the next .await point.
            drop(_batch_guard);

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
                    "bulk build batch missing last block number: current_block={} batch_block_count={}",
                    current_block,
                    batch_block_count
                )
            })?;
            let last_block_u64 = u64::try_from(last_block_number).map_err(|_| {
                anyhow!(
                    "bulk build last block number is negative: last_block_number={}",
                    last_block_number
                )
            })?;
            batch_span.record("end_block", last_block_u64);
            indexer.progress.record_batch(
                last_block_u64,
                batch_stats.block_count,
                batch_stats.tx_count,
            );

            let snap = sampler.latest();
            let disk_state = snap.disk_state.clone();
            let mut sample = BatchSample::new(
                batch_stats.block_count,
                build_elapsed.as_secs_f64(),
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
            sample.start_block = current_block;
            sample.end_block = last_block_u64;
            sample.batch_index = batch_count;
            sample.bottleneck = last_bottleneck.clone();
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
            sample.fetch_ms = 0.0; // fetch is decoupled via block buffer
            sample.facts_ms = build_timings.facts_ms;
            sample.resolve_ms = build_timings.resolve_ms;
            sample.reduce_ms = build_timings.reduce_ms;
            sample.history_ms = build_timings.history_ms;
            sample.address_reduce_ms = build_timings.address_reduce_ms;
            sample.activity_stats_ms = build_timings.activity_stats_ms;
            sample.interner_gc_ms = build_timings.interner_gc_ms;
            sample.memory_accounting_ms = memory_accounting_elapsed.as_secs_f64() * 1000.0;
            sample.facts_par_iter_ms = build_timings.facts_breakdown.par_iter_ms;
            sample.facts_merge_ms = build_timings.facts_breakdown.merge_ms;
            sample.facts_serial_equivalent_ms = build_timings.facts_breakdown.serial_equivalent_ms;
            sample.facts_intern_slow_path_count =
                build_timings.facts_breakdown.intern_slow_path_count;
            sample.facts_intern_total_count = build_timings.facts_breakdown.intern_total_count;
            sample.facts_cell_count = build_timings.facts_breakdown.cell_count;
            sample.flush_ms = prev_flush_ms;
            sample.flush_wait_ms = flush_wait_elapsed.as_secs_f64() * 1000.0;
            sample.flush_channel_depth = flush_depth as u64;
            sample.flush_channel_pending = flush_channel_pending;
            sample.prefetch_recv_ms = prefetch_recv_elapsed.as_secs_f64() * 1000.0;
            sample.prefetch_depth = prefetch_depth as u64;
            sample.owner_memory_bytes = last_owner_memory.clone();
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
            indexer.bulk_build_perf.record_batch_bytes(batch_bytes);
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
                0.0, // fetch_ms: fetch is decoupled via block buffer
                build_elapsed.as_secs_f64() * 1000.0,
                owner_mem_total,
                sample.live_cell_count,
                sample.cells,
                sample.inputs,
                cumulative_history_rows as u64,
                cumulative_sealed_rows as u64,
                batch_block_count,
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
                buffer.channel_len() as u64,
                prefetch_depth as u64,
                flush_channel_pending,
                flush_depth as u64,
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
                batch_block_count,
                batch_bytes,
                txs = batch_stats.tx_count,
                current_block = last_block_u64,
                target_block = chain_tip,
                remaining_blocks = indexer.progress.blocks_remaining(),
                progress_pct = format!("{:.1}%", progress_pct),
                build_ms = format!("{:.1}", build_elapsed.as_secs_f64() * 1000.0),
                critical_stage_ms = format!("{:.1}", critical_stage_ms),
                facts_ms = format!("{:.1}", build_timings.facts_ms),
                resolve_ms = format!("{:.1}", build_timings.resolve_ms),
                reduce_ms = format!("{:.1}", build_timings.reduce_ms),
                history_ms = format!("{:.1}", build_timings.history_ms),
                address_reduce_ms = format!("{:.1}", build_timings.address_reduce_ms),
                activity_stats_ms = format!("{:.1}", build_timings.activity_stats_ms),
                interner_gc_ms = format!("{:.1}", build_timings.interner_gc_ms),
                memory_accounting_ms =
                    format!("{:.1}", memory_accounting_elapsed.as_secs_f64() * 1000.0),
                prev_flush_ms = format!("{:.1}", prev_flush_ms),
                "Bulk build materialized batch"
            );

            if let Some(output) = controller.observe(&BatchSignals {
                prefetch_recv_ms: prefetch_recv_elapsed.as_secs_f64() * 1000.0,
                build_ms: build_elapsed.as_secs_f64() * 1000.0,
                flush_wait_ms: flush_wait_elapsed.as_secs_f64() * 1000.0,
                l0_files: snap.l0_files,
                actual_cells: batch_cells,
            }) {
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
                    target_cells = output.target_cells,
                    max_batch_bytes = output.max_batch_bytes,
                    fetch_threads = output.fetch_threads,
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
                    output.fetch_threads,
                    output.bg_jobs,
                    output.target_cells,
                );

                last_bottleneck = Some(output.bottleneck.to_string());
            }

            // Periodic memory summary every 10 batches
            if batch_count.is_multiple_of(10) {
                let total_mb: u64 = last_owner_memory.values().sum::<u64>() / (1024 * 1024);
                let live_cells = runtime.sequencer.live_count();
                let interner_entries = runtime.interner.len();
                let interner_slots = runtime.interner.slot_len();
                let interner_free_slots = runtime.interner.free_len();
                let written_lock_scripts = runtime.interner.lock_script_written_count();
                info!(
                    total_memory_mb = total_mb,
                    live_cells,
                    interner_entries,
                    interner_slots,
                    interner_free_slots,
                    written_lock_scripts,
                    batch_count,
                    "Bulk build memory snapshot"
                );
            }
        }

        // ── Finalize: decomposed into 13 sub-phases with progress reporting ──
        // The progress monitor (entry.rs, 10s polling) reads these atomics and
        // publishes to RocksDB so the TUI can display a finalize checklist.
        let finalize_started = Instant::now();
        memory_guard.checkpoint(
            "before_finalize",
            indexer.progress.current(),
            &last_owner_memory,
        )?;

        // Drop the buffer handle (and its receiver) to signal prefetch to stop.
        drop(buffer);
        let prefetch_stats = prefetch.close_and_wait().await?;
        info!(
            total_fetches = prefetch_stats.total_fetches,
            total_blocks = prefetch_stats.total_blocks,
            exit_reason = ?prefetch_stats.exit_reason,
            "Prefetch worker finished"
        );

        // Phase 0: close channel and drain all queued flushes.
        // No span here — contains .await points (flush_drain.wait()).
        indexer
            .bulk_build_perf
            .record_finalize_step(1, finalize_started.elapsed());
        let flush_drain = flush_channel.begin_shutdown();
        // Genesis-derived burn adjustment for knowledge_size. Fail-fast if the
        // baseline was never derived (single calculation path, no fallback).
        let virtual_occupied = indexer
            .writer
            .store()
            .get_genesis_baseline()?
            .ok_or_else(|| {
                anyhow!("genesis baseline not derived; cannot finalize bulk-build knowledge_size")
            })?
            .virtual_occupied;
        let prepared_finalize = match runtime.prepare_finalize_artifacts(virtual_occupied) {
            Ok(prepared) => prepared,
            Err(err) => {
                let _ = flush_drain.wait().await;
                return Err(err);
            }
        };
        // Run flush drain and materialize phases concurrently.
        // Safety: they write to disjoint CF sets.
        //   Flush drain: CF_CELLS (append-only), CF_BLOCK_HEADERS, CF_TX_INDEX,
        //     CF_ADDR_TXS, CF_CONSUMED_CELLS, CF_ACTIVITIES, CF_ADDR_ACTIVITIES,
        //     and other per-block history CFs.
        //   Materialize: CF_STATS_*, CF_ADDR_STATS, CF_LIVE_CELLS, CF_SCRIPT_INFO,
        //     CF_SCRIPT_VERSIONS, CF_SCRIPT_FAMILIES, CF_TOKEN_INFO, CF_DAO_*,
        //     CF_ADDR_BALANCE, CF_FIBER_*, CF_SPORE_*, CF_CLUSTER_*, and other
        //     sealed-aggregate / final-snapshot CFs.
        // If a future change adds a CF to both paths, concurrent writes will
        // corrupt data silently.
        let domain_store_arc = indexer.writer.store().clone();
        let append_only_arc = Arc::clone(&indexer.append_only_store);
        let perf_stats = indexer.bulk_build_perf.clone();
        let finalize_started_copy = finalize_started;

        let BulkBuildRuntimeState {
            owners,
            sequencer,
            interner,
            hodl_tracker,
            cell_dist_tracker,
            ..
        } = runtime;

        let materialize_handle = tokio::task::spawn_blocking(move || {
            materialize_finalize_phases(
                domain_store_arc.as_ref(),
                append_only_arc.as_ref(),
                prepared_finalize,
                owners,
                sequencer,
                interner,
                hodl_tracker,
                cell_dist_tracker,
                &perf_stats,
                finalize_started_copy,
            )
        });

        let (drain_result, materialize_result) =
            tokio::join!(flush_drain.wait(), materialize_handle,);

        let flush_stats = drain_result?;
        let materialize_report = materialize_result
            .map_err(|e| anyhow!("materialize finalize task panicked: {e}"))??;

        materializer.add_external_counts(
            flush_stats.total_history_rows,
            flush_stats.total_sealed_rows,
            flush_stats.flush_count,
        );
        materializer.merge_report(materialize_report);

        info!(
            "flush pipeline: prepare={:.1}s commit={:.1}s flushes={} rows={}",
            flush_stats.total_prepare_ms / 1000.0,
            flush_stats.total_commit_ms / 1000.0,
            flush_stats.flush_count,
            flush_stats.total_history_rows + flush_stats.total_sealed_rows,
        );

        // Phase 11: memtable flush
        {
            let _guard = tracing::info_span!("bulk_finalize", phase = 12, label = "memtable_flush")
                .entered();
            indexer
                .bulk_build_perf
                .record_finalize_step(12, finalize_started.elapsed());
            flush_bulk_build_materialized_state(
                indexer.writer.store().as_ref(),
                indexer.writer.append_only_store(),
            )?;
        }

        // Phase 12: sync status + cleanup
        {
            let _guard =
                tracing::info_span!("bulk_finalize", phase = 13, label = "sync_cleanup").entered();
            indexer
                .bulk_build_perf
                .record_finalize_step(13, finalize_started.elapsed());
            sync_totals.finalize_success(indexer.writer.store().as_ref(), false)?;
            indexer.writer.store().clear_bulk_build_session_marker()?;
            indexer.writer.refresh_latest_dao_statistics()?;
        }

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
    interner_gc_ms: f64,
}

/// Rows produced by `apply_blocks` that need to be flushed to RocksDB.
/// Designed to be `Send` so it can be moved into `spawn_blocking`.
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

/// Executes finalize phases 1-11: writes sealed aggregates, final snapshot,
/// owner data, and metadata to RocksDB. Returns a MaterializationReport
/// for merging into the main accounting.
///
/// Designed to be `Send` so it can run in `tokio::task::spawn_blocking`
/// concurrently with flush drain.
#[allow(clippy::too_many_arguments)]
fn materialize_finalize_phases(
    domain_store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    prepared: PreparedFinalizeArtifacts,
    owners: CoreOwners,
    sequencer: sequencer::BulkSequencer,
    interner: interner::IdentityInterner,
    hodl_tracker: crate::db::writer::hodl_wave::HodlWaveTracker,
    cell_dist_tracker: crate::db::writer::cell_distribution::CellDistributionTracker,
    perf_stats: &crate::sync::diagnostics::BulkBuildPerfStats,
    finalize_started: Instant,
) -> Result<materialize::MaterializationReport> {
    let mut materializer = materialize::Materializer::new(domain_store, append_only_store);

    // Step 2: activity stats (sealed aggregates)
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 2, label = "activity_stats").entered();
        perf_stats.record_finalize_step(2, finalize_started.elapsed());
        materializer.stream_sealed_aggregate_rows(&prepared.activity_sealed_rows)?;
    }

    // Step 3: chain stats (sealed aggregates)
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 3, label = "chain_stats").entered();
        perf_stats.record_finalize_step(3, finalize_started.elapsed());
        materializer.stream_sealed_aggregate_rows(&prepared.chain_sealed_rows)?;
    }

    // Step 4: final snapshot (live cell markers + index CFs)
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 4, label = "final_snapshot").entered();
        perf_stats.record_finalize_step(4, finalize_started.elapsed());
        let frozen = interner.snapshot_for_reads();
        materializer.materialize_final_snapshot_bounded(|sink| {
            emit_final_snapshot_rows(&sequencer, &frozen, |row| sink.push(row))
        })?;
    }

    // Steps 5-6: emit each owner sequentially through byte-bounded batches.
    // Building every owner's full row vector in parallel duplicated the whole
    // reducer state at the exact point where bulk sync already had peak RSS.
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 5, label = "owners_build").entered();
        perf_stats.record_finalize_step(5, finalize_started.elapsed());
        perf_stats.record_finalize_step(6, finalize_started.elapsed());
        owners.materialize_all(&mut materializer)?;
    }

    // Step 11: metadata (HODL + cell distribution tracker state)
    {
        let _guard = tracing::info_span!("bulk_finalize", phase = 11, label = "metadata").entered();
        perf_stats.record_finalize_step(11, finalize_started.elapsed());
        let mut meta_batch = ckbadger_store::batch::StoreBatch::new(domain_store);
        meta_batch.put_hodl_tracker_state(&hodl_tracker.to_state());
        meta_batch.put_cell_dist_tracker_state(&cell_dist_tracker.to_state());
        if !meta_batch.is_empty() {
            meta_batch.commit()?;
        }
    }

    Ok(materializer.finish())
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
    ) -> Result<FxHashMap<[u8; 32], owners::address::AddressTxDelta>> {
        let address_deltas = self.address.apply_tx_with_deltas(tx, ctx)?;
        self.script.apply_tx(tx, ctx)?;
        self.token.apply_tx(tx, ctx)?;
        self.dao.apply_tx(tx, ctx)?;
        self.fiber.apply_tx(tx, ctx)?;
        self.object.apply_tx(tx, ctx)?;
        Ok(address_deltas)
    }

    fn materialize_all(self, materializer: &mut materialize::Materializer<'_>) -> Result<()> {
        let Self {
            mut address,
            mut script,
            mut token,
            mut dao,
            mut fiber,
            mut object,
        } = self;

        address.flush_sealed(materializer)?;
        address.materialize_final(materializer)?;
        drop(address);

        script.flush_sealed(materializer)?;
        script.materialize_final(materializer)?;
        drop(script);

        token.flush_sealed(materializer)?;
        token.materialize_final(materializer)?;
        drop(token);

        dao.flush_sealed(materializer)?;
        dao.materialize_final(materializer)?;
        drop(dao);

        fiber.flush_sealed(materializer)?;
        fiber.materialize_final(materializer)?;
        drop(fiber);

        object.flush_sealed(materializer)?;
        object.materialize_final(materializer)?;
        Ok(())
    }
}

#[derive(Default)]
struct ActivityStatsAccumulator {
    daily_stats: FxHashMap<String, DailyActivityStats>,
    daily_addrs: FxHashMap<String, FxHashSet<[u8; 32]>>,
    hourly_stats: FxHashMap<String, DailyActivityStats>,
    hourly_addrs: FxHashMap<String, FxHashSet<[u8; 32]>>,
    mtp_timestamps_ms: VecDeque<i64>,
    sealed_through_ms: Option<i64>,
    last_block_number: Option<i64>,
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

    /// Accumulate activity stats directly from in-memory TxActions.
    /// Replaces the old `apply_history_rows` which deserialized bundles
    /// from bincode MaterializedRows — a serialize→deserialize roundtrip
    /// costing ~410ms/batch at steady state.
    ///
    /// Chrono cache: all txs in the same block share one timestamp, so we
    /// cache the formatted date/hour strings and only reformat on timestamp
    /// change (~47K format calls per batch instead of ~123K).
    fn apply_batch(
        &mut self,
        blocks: &[facts::BlockFacts],
        tx_actions_list: &[TxActions],
    ) -> Result<Vec<materialize::MaterializedRow>> {
        if blocks.is_empty() {
            if tx_actions_list.is_empty() {
                return Ok(Vec::new());
            }
            bail!(
                "activity accumulator received actions without blocks: actions={}",
                tx_actions_list.len()
            );
        }

        let mut expected_number = self
            .last_block_number
            .map(|number| {
                number.checked_add(1).ok_or_else(|| {
                    anyhow!(
                        "activity accumulator block number overflow after block={}",
                        number
                    )
                })
            })
            .transpose()?;
        let mut block_timestamps = FxHashMap::default();
        for block in blocks {
            if block.timestamp_ms < 0 {
                bail!(
                    "activity accumulator received negative block timestamp: block={} timestamp_ms={}",
                    block.number,
                    block.timestamp_ms
                );
            }
            if let Some(expected) = expected_number {
                if block.number != expected {
                    bail!(
                        "activity accumulator block discontinuity: expected={} actual={} previous={:?}",
                        expected,
                        block.number,
                        self.last_block_number
                    );
                }
            }
            if block_timestamps
                .insert(block.number, block.timestamp_ms)
                .is_some()
            {
                bail!(
                    "activity accumulator received duplicate block number: block={}",
                    block.number
                );
            }
            expected_number = Some(block.number.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "activity accumulator block number overflow at block={}",
                    block.number
                )
            })?);
        }

        // A bucket already below the previous batch's MTP can never receive a
        // valid future CKB block. Treat such input as a chain/order invariant
        // violation instead of silently recreating an already-written row.
        if let Some(watermark) = self.sealed_through_ms {
            for actions in tx_actions_list {
                for (kind, end_ms) in [
                    (
                        "hourly",
                        activity_bucket_end_ms(actions.timestamp, ACTIVITY_HOUR_MS)?,
                    ),
                    (
                        "daily",
                        activity_bucket_end_ms(actions.timestamp, ACTIVITY_DAY_MS)?,
                    ),
                ] {
                    if end_ms <= watermark {
                        bail!(
                            "already sealed activity bucket received a late action: kind={} block={} tx_index={} timestamp_ms={} bucket_end_ms={} mtp_watermark_ms={}",
                            kind,
                            actions.block_number,
                            actions.tx_index,
                            actions.timestamp,
                            end_ms,
                            watermark
                        );
                    }
                }
            }
        }

        for actions in tx_actions_list {
            let expected_timestamp = block_timestamps.get(&actions.block_number).ok_or_else(|| {
                anyhow!(
                    "activity action references block outside current batch: block={} tx_index={} batch_first={} batch_last={}",
                    actions.block_number,
                    actions.tx_index,
                    blocks.first().expect("non-empty checked").number,
                    blocks.last().expect("non-empty checked").number
                )
            })?;
            if actions.timestamp != *expected_timestamp {
                bail!(
                    "activity action timestamp differs from owning block: block={} tx_index={} action_timestamp_ms={} block_timestamp_ms={}",
                    actions.block_number,
                    actions.tx_index,
                    actions.timestamp,
                    expected_timestamp
                );
            }
        }

        self.accumulate_tx_actions(tx_actions_list)?;

        let mut next_watermark = self.sealed_through_ms;
        for block in blocks {
            self.mtp_timestamps_ms.push_back(block.timestamp_ms);
            if self.mtp_timestamps_ms.len() > CKB_MEDIAN_TIME_BLOCK_COUNT {
                self.mtp_timestamps_ms.pop_front();
            }
            let timestamps = self.mtp_timestamps_ms.make_contiguous();
            let median = ckb_median_time_ms(timestamps)?;
            if let Some(previous) = next_watermark {
                if median < previous {
                    bail!(
                        "CKB median-time watermark regressed: block={} previous_mtp_ms={} current_mtp_ms={} window={:?}",
                        block.number,
                        previous,
                        median,
                        timestamps
                    );
                }
            }
            next_watermark = Some(median);
            self.last_block_number = Some(block.number);
        }

        let watermark = next_watermark.expect("non-empty block batch has a median");
        let rows = self.drain_sealed_rows(watermark)?;
        self.sealed_through_ms = Some(watermark);
        Ok(rows)
    }

    fn accumulate_tx_actions(&mut self, tx_actions_list: &[TxActions]) -> Result<()> {
        let mut cached_ts = i64::MIN;
        let mut cached_date = String::new();
        let mut cached_hour = String::new();

        for tx_actions in tx_actions_list {
            if tx_actions.timestamp != cached_ts {
                cached_ts = tx_actions.timestamp;
                cached_date = ckbadger_common::block_date_from_ms(tx_actions.timestamp)
                    .format("%Y%m%d")
                    .to_string();
                cached_hour = ckbadger_common::block_datetime_from_ms(tx_actions.timestamp)
                    .format("%Y%m%d%H")
                    .to_string();
            }

            crate::db::BatchWriter::accumulate_tx_activity_stats(
                tx_actions,
                self.daily_stats.entry(cached_date.clone()).or_default(),
            );
            crate::db::BatchWriter::accumulate_tx_activity_stats(
                tx_actions,
                self.hourly_stats.entry(cached_hour.clone()).or_default(),
            );

            if !tx_actions.is_cellbase {
                for participant in &tx_actions.participants {
                    if participant.lock_hash.len() == 32 {
                        let mut lock_hash = [0u8; 32];
                        lock_hash.copy_from_slice(&participant.lock_hash);
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
        }

        Ok(())
    }

    #[cfg(test)]
    fn apply_tx_actions(&mut self, tx_actions_list: &[TxActions]) -> Result<()> {
        self.accumulate_tx_actions(tx_actions_list)
    }

    fn drain_sealed_rows(
        &mut self,
        watermark_ms: i64,
    ) -> Result<Vec<materialize::MaterializedRow>> {
        let mut rows = drain_activity_bucket_rows(
            &mut self.hourly_stats,
            &mut self.hourly_addrs,
            watermark_ms,
            true,
        )?;
        rows.extend(drain_activity_bucket_rows(
            &mut self.daily_stats,
            &mut self.daily_addrs,
            watermark_ms,
            false,
        )?);
        Ok(rows)
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
            if let Some(addrs) = self.daily_addrs.get(&date) {
                let mut sorted: Vec<[u8; 32]> = addrs.iter().copied().collect();
                sorted.sort_unstable();
                let flat: Vec<u8> = sorted.iter().flat_map(|h| h.iter().copied()).collect();
                rows.push(materialize::MaterializedRow::new(
                    CF_STATS_CHAIN,
                    keys::encode_stats_key(
                        keys::stats_prefix::ACTIVITY_DAILY_ADDR_SET,
                        date.as_bytes(),
                    ),
                    flat,
                ));
            }
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
            if let Some(addrs) = self.hourly_addrs.get(&hour) {
                let mut sorted: Vec<[u8; 32]> = addrs.iter().copied().collect();
                sorted.sort_unstable();
                let flat: Vec<u8> = sorted.iter().flat_map(|h| h.iter().copied()).collect();
                rows.push(materialize::MaterializedRow::new(
                    CF_STATS_CHAIN,
                    keys::encode_stats_key(
                        keys::stats_prefix::ACTIVITY_HOURLY_ADDR_SET,
                        hour.as_bytes(),
                    ),
                    flat,
                ));
            }
        }

        Ok(rows)
    }
}

const CKB_MEDIAN_TIME_BLOCK_COUNT: usize = 37;
const ACTIVITY_HOUR_MS: i64 = 60 * 60 * 1_000;
const ACTIVITY_DAY_MS: i64 = 24 * ACTIVITY_HOUR_MS;
const CKB_UTC8_OFFSET_MS: i64 = ckbadger_common::CKB_UTC8_OFFSET as i64 * 1_000;

fn ckb_median_time_ms(timestamps: &[i64]) -> Result<i64> {
    if timestamps.is_empty() {
        bail!("cannot calculate CKB median time from an empty timestamp window");
    }
    if timestamps.len() > CKB_MEDIAN_TIME_BLOCK_COUNT {
        bail!(
            "CKB median-time window exceeds consensus limit: len={} max={}",
            timestamps.len(),
            CKB_MEDIAN_TIME_BLOCK_COUNT
        );
    }
    // Consensus fixes the window at at most 37 headers, so keep the working
    // set on the stack. This preserves the exact upper-median rule without a
    // heap allocation for every block in a full-chain replay.
    let mut ordered = [0i64; CKB_MEDIAN_TIME_BLOCK_COUNT];
    ordered[..timestamps.len()].copy_from_slice(timestamps);
    let middle = timestamps.len() / 2;
    let (_, median, _) = ordered[..timestamps.len()].select_nth_unstable(middle);
    Ok(*median)
}

fn activity_bucket_end_ms(timestamp_ms: i64, bucket_ms: i64) -> Result<i64> {
    let shifted = timestamp_ms
        .checked_add(CKB_UTC8_OFFSET_MS)
        .ok_or_else(|| {
            anyhow!(
                "activity bucket timestamp overflow while applying UTC+8 offset: timestamp_ms={}",
                timestamp_ms
            )
        })?;
    let next_bucket = shifted
        .div_euclid(bucket_ms)
        .checked_add(1)
        .ok_or_else(|| {
            anyhow!(
                "activity bucket index overflow: timestamp_ms={}",
                timestamp_ms
            )
        })?;
    next_bucket
        .checked_mul(bucket_ms)
        .and_then(|end| end.checked_sub(CKB_UTC8_OFFSET_MS))
        .ok_or_else(|| {
            anyhow!(
                "activity bucket end overflow: timestamp_ms={} bucket_ms={}",
                timestamp_ms,
                bucket_ms
            )
        })
}

fn activity_bucket_end_from_key(bucket: &str, hourly: bool) -> Result<i64> {
    let local_start_ms = if hourly {
        if bucket.len() != 10 {
            bail!(
                "invalid hourly activity bucket length: bucket={} len={} expected=10",
                bucket,
                bucket.len()
            );
        }
        let date = chrono::NaiveDate::parse_from_str(&bucket[..8], "%Y%m%d")
            .map_err(|err| anyhow!("invalid hourly activity bucket date {}: {}", bucket, err))?;
        let hour = bucket[8..]
            .parse::<u32>()
            .map_err(|err| anyhow!("invalid hourly activity bucket hour {}: {}", bucket, err))?;
        date.and_hms_opt(hour, 0, 0)
            .ok_or_else(|| anyhow!("invalid hour in hourly activity bucket {}", bucket))?
            .and_utc()
            .timestamp_millis()
    } else {
        chrono::NaiveDate::parse_from_str(bucket, "%Y%m%d")
            .map_err(|err| anyhow!("invalid daily activity bucket {}: {}", bucket, err))?
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow!("invalid midnight for daily activity bucket {}", bucket))?
            .and_utc()
            .timestamp_millis()
    };
    local_start_ms
        .checked_add(if hourly {
            ACTIVITY_HOUR_MS
        } else {
            ACTIVITY_DAY_MS
        })
        .and_then(|end| end.checked_sub(CKB_UTC8_OFFSET_MS))
        .ok_or_else(|| anyhow!("activity bucket end overflow for key={}", bucket))
}

fn drain_activity_bucket_rows(
    stats_by_bucket: &mut FxHashMap<String, DailyActivityStats>,
    addrs_by_bucket: &mut FxHashMap<String, FxHashSet<[u8; 32]>>,
    watermark_ms: i64,
    hourly: bool,
) -> Result<Vec<materialize::MaterializedRow>> {
    for bucket in addrs_by_bucket.keys() {
        if !stats_by_bucket.contains_key(bucket) {
            bail!(
                "activity address set has no matching stats bucket: kind={} bucket={}",
                if hourly { "hourly" } else { "daily" },
                bucket
            );
        }
    }

    let mut sealed_buckets = stats_by_bucket
        .keys()
        .filter_map(
            |bucket| match activity_bucket_end_from_key(bucket, hourly) {
                Ok(end_ms) if end_ms <= watermark_ms => Some(Ok(bucket.clone())),
                Ok(_) => None,
                Err(err) => Some(Err(err)),
            },
        )
        .collect::<Result<Vec<_>>>()?;
    sealed_buckets.sort();

    let mut rows = Vec::with_capacity(sealed_buckets.len() * 2);
    for bucket in sealed_buckets {
        let mut stats = stats_by_bucket.remove(&bucket).ok_or_else(|| {
            anyhow!(
                "sealed activity stats bucket disappeared during drain: kind={} bucket={}",
                if hourly { "hourly" } else { "daily" },
                bucket
            )
        })?;
        let addrs = addrs_by_bucket.remove(&bucket);
        stats.unique_address_count = addrs.as_ref().map_or(Ok(0), |set| {
            checked_unique_address_count(set.len(), &bucket)
        })?;
        let stats_prefix = if hourly {
            keys::stats_prefix::ACTIVITY_HOURLY
        } else {
            keys::stats_prefix::ACTIVITY_DAILY
        };
        rows.push(materialize::MaterializedRow::new(
            CF_STATS_CHAIN,
            keys::encode_stats_key(stats_prefix, bucket.as_bytes()),
            bincode::serialize(&stats)?,
        ));
        if let Some(addrs) = addrs {
            let mut sorted = addrs.into_iter().collect::<Vec<_>>();
            sorted.sort_unstable();
            let mut flat = Vec::with_capacity(sorted.len() * 32);
            for hash in sorted {
                flat.extend_from_slice(&hash);
            }
            let addr_prefix = if hourly {
                keys::stats_prefix::ACTIVITY_HOURLY_ADDR_SET
            } else {
                keys::stats_prefix::ACTIVITY_DAILY_ADDR_SET
            };
            rows.push(materialize::MaterializedRow::new(
                CF_STATS_CHAIN,
                keys::encode_stats_key(addr_prefix, bucket.as_bytes()),
                flat,
            ));
        }
    }
    Ok(rows)
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
    /// Per-day: (sum_difficulty, block_count, total_uncles)
    daily_block_stats: FxHashMap<chrono::NaiveDate, (i128, i32, i32)>,
    /// Last DAO field seen per day (for knowledge_size).
    daily_dao_fields: FxHashMap<chrono::NaiveDate, [u8; 32]>,
    /// Per-day block time accumulation: (sum_ms, count)
    daily_block_times: FxHashMap<chrono::NaiveDate, (i64, i32)>,
    /// Block time distribution buckets (seconds → count).
    block_time_dist: FxHashMap<i32, i32>,
    /// Epoch time distribution buckets (minutes → count).
    epoch_time_dist: FxHashMap<i32, i32>,
    /// Per-epoch stats: epoch_number → (start_block, end_block, length, start_ts_ms, end_ts_ms, tx_count).
    epoch_stats: FxHashMap<i64, (i64, i64, i32, i64, i64, i32)>,
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
        let epoch_count = self.epoch_stats.len();
        (daily_count * 100 + dist_count * 16 + epoch_count * 48) as u64
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

            // --- DailyBlockStats (accumulate difficulty, not compact_target) ---
            let block_entry = self.daily_block_stats.entry(block_date).or_default();
            let difficulty_u256 = ckb_compact_to_difficulty(block.compact_target);
            let difficulty_u64: u64 = difficulty_u256.to_string().parse().map_err(|_| {
                anyhow!(
                    "difficulty exceeds u64 range: block={}, date={}, compact_target={:#x}, difficulty={}",
                    block.number, block_date, block.compact_target, difficulty_u256
                )
            })?;
            block_entry.0 += difficulty_u64 as i128;
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
                        let duration_ms = block.timestamp_ms - prev_start_ts;
                        let epoch_duration_minutes = duration_ms as f64 / 60_000.0;
                        let bucket_minutes = epoch_duration_minutes.round() as i32;
                        if bucket_minutes <= 0 {
                            anyhow::bail!(
                                "epoch time distribution: invalid bucket_minutes={} \
                                 for epoch {} (prev_epoch={}, duration_ms={}, \
                                 block={}, prev_start_ts={})",
                                bucket_minutes,
                                block.epoch_number,
                                prev_epoch_num,
                                duration_ms,
                                block.number,
                                prev_start_ts,
                            );
                        }
                        *self.epoch_time_dist.entry(bucket_minutes).or_default() += 1;
                    }
                }
            }
            if block.epoch_index == 0 {
                self.prev_epoch = Some((block.epoch_number, block.timestamp_ms));
            }

            // --- Per-epoch stats ---
            let epoch_entry = self
                .epoch_stats
                .entry(block.epoch_number)
                .or_insert_with(|| {
                    (
                        block.number,
                        block.number,
                        block.epoch_length,
                        block.timestamp_ms,
                        block.timestamp_ms,
                        0,
                    )
                });
            epoch_entry.1 = block.number; // end_block
            epoch_entry.4 = block.timestamp_ms; // end_ts_ms
            epoch_entry.5 += block.transactions_count; // tx_count
        }
        Ok(())
    }

    /// Build sealed aggregate rows for `CF_STATS_CHAIN`.
    ///
    /// `virtual_occupied` is the genesis-derived burn adjustment (from
    /// `GenesisBaseline::virtual_occupied`) used by the knowledge_size calc.
    fn build_rows(&self, virtual_occupied: i128) -> Result<Vec<materialize::MaterializedRow>> {
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
                .and_then(|dao| crate::db::writer::calculate_knowledge_size(dao, virtual_occupied));

            let (block_time_sum_ms, block_time_count) = self
                .daily_block_times
                .get(date)
                .map(|(sum, count)| (*sum, *count))
                .unwrap_or((0, 0));

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
                block_time_sum_ms,
                block_time_count,
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
            let (sum_difficulty, count, uncles) = self.daily_block_stats[date];
            let avg_difficulty = if count > 0 {
                (sum_difficulty / count as i128) as f64
            } else {
                0.0
            };

            let (block_time_sum_ms, block_time_count) = self
                .daily_block_times
                .get(date)
                .map(|(sum, count)| (*sum, *count))
                .unwrap_or((0, 0));

            let stats = ckbadger_store::types::DailyBlockStats {
                avg_difficulty,
                block_count: count,
                total_uncles: uncles,
                block_time_sum_ms,
                block_time_count,
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

        // --- Per-epoch stats ---
        for (&epoch_number, &(start_block, end_block, length, start_ts_ms, end_ts_ms, tx_count)) in
            &self.epoch_stats
        {
            let start_timestamp =
                chrono::DateTime::from_timestamp(start_ts_ms / 1000, 0).unwrap_or_default();
            let end_timestamp =
                chrono::DateTime::from_timestamp(end_ts_ms / 1000, 0).unwrap_or_default();
            let blocks_count = (end_block - start_block + 1) as i32;
            let is_complete = blocks_count >= length;
            let stats = ckbadger_store::types::EpochStats {
                epoch_number,
                start_block,
                end_block: Some(end_block),
                blocks_count,
                length,
                start_timestamp,
                end_timestamp: if is_complete {
                    Some(end_timestamp)
                } else {
                    None
                },
                transactions_count: tx_count,
            };
            rows.push(materialize::MaterializedRow::new(
                CF_STATS_CHAIN,
                keys::encode_stats_key(keys::stats_prefix::EPOCH, &epoch_number.to_be_bytes()),
                bincode::serialize(&stats)?,
            ));
        }

        Ok(rows)
    }
}

struct BulkBuildRuntimeState {
    interner: interner::IdentityInterner,
    interner_liveness: interner::IdentityLiveness,
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
            interner_liveness: interner::IdentityLiveness::default(),
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
            "interner_liveness".to_string(),
            self.interner_liveness.estimated_bytes(),
        );
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
            std::mem::size_of_val(&self.hodl_live_cells_by_lock) as u64
                + self.hodl_live_cells_by_lock.capacity() as u64
                    * std::mem::size_of::<(crate::sync::types::InternId, i32)>() as u64
                + self.hodl_live_cells_by_lock.len() as u64
                    * (std::mem::size_of::<crate::sync::types::InternId>()
                        + std::mem::size_of::<i32>()) as u64,
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
        self.interner_liveness
            .ensure_slots(self.interner.slot_len());
        let resolved = self
            .sequencer
            .resolve_with_liveness(&arena, &mut self.interner_liveness)?;
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
            interner,
            interner_liveness,
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
            || -> Result<(
                HistoryBuildResult,
                std::time::Duration,
                std::time::Duration,
                Vec<materialize::MaterializedRow>,
            )> {
                let history_started = Instant::now();
                let history = build_history_batches(
                    &arena,
                    &resolved,
                    &frozen,
                    interner,
                    is_mainnet,
                    token_info_cache,
                )?;
                let history_elapsed = history_started.elapsed();

                // activity_stats depends only on history.tx_actions_list, not on any reducer state.
                let activity_stats_started = Instant::now();
                let activity_sealed_rows =
                    activity_stats.apply_batch(&arena.blocks, &history.tx_actions_list)?;
                let activity_stats_elapsed = activity_stats_started.elapsed();

                Ok((
                    history,
                    history_elapsed,
                    activity_stats_elapsed,
                    activity_sealed_rows,
                ))
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
        let (history, history_elapsed, activity_stats_elapsed, activity_sealed_rows) = left_result?;
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

        // Collect sealed rows (pure data, no store dependency).
        let mut sealed_rows = hodl_sealed_rows;
        sealed_rows.extend(cell_dist_sealed_rows);
        sealed_rows.extend(activity_sealed_rows);

        let pending = PendingFlush {
            history_rows: history.rows,
            sealed_rows,
        };

        let batch_stats = BatchExecutionStats {
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
        };

        // Identity bytes are needed through history/reducer construction only. Release the
        // frozen view and all batch-local ID holders before reclaiming identities whose live-cell
        // reference count reached zero. Reclaim also invalidates the matching lock-script write
        // marker so a reused ID can never suppress a different script row.
        drop(resolved);
        drop(frozen);
        drop(arena);
        let interner_gc_started = Instant::now();
        let reclaim_candidates = interner_liveness.drain_zero_candidates();
        let reclaim_candidate_count = reclaim_candidates.len();
        let reclaim_stats = interner.reclaim_zero_ref_identities(&reclaim_candidates)?;
        let interner_gc_elapsed = interner_gc_started.elapsed();
        tracing::debug!(
            candidates = reclaim_candidate_count,
            reclaimed_identities = reclaim_stats.identities,
            reclaimed_payload_bytes = reclaim_stats.payload_bytes,
            invalidated_lock_script_markers = reclaim_stats.invalidated_lock_script_markers,
            active_identities = interner.len(),
            free_identity_slots = interner.free_len(),
            interner_gc_ms = format!("{:.1}", interner_gc_elapsed.as_secs_f64() * 1000.0),
            "Bulk build reclaimed unused interned identities"
        );

        let timings = BatchBuildTimings {
            facts_ms: facts_elapsed.as_secs_f64() * 1000.0,
            facts_breakdown,
            resolve_ms: resolve_elapsed.as_secs_f64() * 1000.0,
            reduce_ms: reduce_elapsed.as_secs_f64() * 1000.0,
            history_ms: history_elapsed.as_secs_f64() * 1000.0,
            address_reduce_ms: address_elapsed.as_secs_f64() * 1000.0,
            activity_stats_ms: activity_stats_elapsed.as_secs_f64() * 1000.0,
            interner_gc_ms: interner_gc_elapsed.as_secs_f64() * 1000.0,
        };

        Ok((batch_stats, timings, pending))
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
        self.interner_liveness
            .ensure_slots(self.interner.slot_len());
        let resolved = self
            .sequencer
            .resolve_with_liveness(&arena, &mut self.interner_liveness)?;
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
            interner,
            interner_liveness,
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
            || -> Result<(
                HistoryBuildResult,
                std::time::Duration,
                std::time::Duration,
                Vec<materialize::MaterializedRow>,
            )> {
                let history_started = Instant::now();
                let history = build_history_batches(
                    &arena,
                    &resolved,
                    &frozen,
                    interner,
                    is_mainnet,
                    token_info_cache,
                )?;
                let history_elapsed = history_started.elapsed();
                let activity_stats_started = Instant::now();
                let activity_sealed_rows =
                    activity_stats.apply_batch(&arena.blocks, &history.tx_actions_list)?;
                let activity_stats_elapsed = activity_stats_started.elapsed();
                Ok((
                    history,
                    history_elapsed,
                    activity_stats_elapsed,
                    activity_sealed_rows,
                ))
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
        let (history, history_elapsed, activity_stats_elapsed, activity_sealed_rows) = left_result?;
        mid_result?;
        let (hodl_sealed_rows, cell_dist_sealed_rows, address_elapsed) = right_result?;
        owners
            .object
            .apply_object_activity_count_deltas(&history.object_activity_count_deltas)?;
        owners
            .object
            .apply_identity_activity_count_deltas(&history.identity_activity_count_deltas)?;
        let reduce_elapsed = reduce_started.elapsed();

        // Collect sealed rows (pure data, no store dependency).
        let mut sealed_rows = hodl_sealed_rows;
        sealed_rows.extend(cell_dist_sealed_rows);
        sealed_rows.extend(activity_sealed_rows);

        let batch_stats = BatchExecutionStats {
            last_block_number: Some(last_block.number),
            last_block_hash: Some(last_block.hash.to_vec()),
            block_count: u64::try_from(arena.blocks.len()).map_err(|_| {
                anyhow!(
                    "bulk build block count exceeds u64 range while applying hex block batch: blocks={}",
                    arena.blocks.len()
                )
            })?,
            tx_count,
            cells_created,
            cells_consumed: consumed_cells,
        };
        let pending = PendingFlush {
            history_rows: history.rows,
            sealed_rows,
        };

        drop(resolved);
        drop(frozen);
        drop(arena);
        let interner_gc_started = Instant::now();
        let reclaim_candidates = interner_liveness.drain_zero_candidates();
        let reclaim_candidate_count = reclaim_candidates.len();
        let reclaim_stats = interner.reclaim_zero_ref_identities(&reclaim_candidates)?;
        let interner_gc_elapsed = interner_gc_started.elapsed();
        tracing::debug!(
            candidates = reclaim_candidate_count,
            reclaimed_identities = reclaim_stats.identities,
            reclaimed_payload_bytes = reclaim_stats.payload_bytes,
            invalidated_lock_script_markers = reclaim_stats.invalidated_lock_script_markers,
            active_identities = interner.len(),
            free_identity_slots = interner.free_len(),
            interner_gc_ms = format!("{:.1}", interner_gc_elapsed.as_secs_f64() * 1000.0),
            "Bulk build reclaimed unused interned identities"
        );

        Ok((
            batch_stats,
            BatchBuildTimings {
                facts_ms: facts_elapsed.as_secs_f64() * 1000.0,
                facts_breakdown,
                resolve_ms: resolve_elapsed.as_secs_f64() * 1000.0,
                reduce_ms: reduce_elapsed.as_secs_f64() * 1000.0,
                history_ms: history_elapsed.as_secs_f64() * 1000.0,
                address_reduce_ms: address_elapsed.as_secs_f64() * 1000.0,
                activity_stats_ms: activity_stats_elapsed.as_secs_f64() * 1000.0,
                interner_gc_ms: interner_gc_elapsed.as_secs_f64() * 1000.0,
            },
            pending,
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
        // Genesis-derived burn adjustment for knowledge_size. Fail-fast if the
        // baseline was never derived (single calculation path, no fallback).
        let virtual_occupied = domain_store
            .get_genesis_baseline()?
            .ok_or_else(|| {
                anyhow!("genesis baseline not derived; cannot finalize bulk-build knowledge_size")
            })?
            .virtual_occupied;
        let prepared_finalize = self.prepare_finalize_artifacts(virtual_occupied)?;
        let BulkBuildRuntimeState {
            owners,
            sequencer,
            interner,
            hodl_tracker,
            cell_dist_tracker,
            ..
        } = self;

        materializer.stream_sealed_aggregate_rows(&prepared_finalize.activity_sealed_rows)?;
        materializer.stream_sealed_aggregate_rows(&prepared_finalize.chain_sealed_rows)?;
        let frozen = interner.snapshot_for_reads();
        materializer.materialize_final_snapshot_bounded(|sink| {
            emit_final_snapshot_rows(&sequencer, &frozen, |row| sink.push(row))
        })?;

        owners.materialize_all(materializer)?;

        let mut meta_batch = ckbadger_store::batch::StoreBatch::new(domain_store);
        meta_batch.put_hodl_tracker_state(&hodl_tracker.to_state());
        meta_batch.put_cell_dist_tracker_state(&cell_dist_tracker.to_state());
        if !meta_batch.is_empty() {
            meta_batch.commit()?;
        }
        Ok(())
    }

    fn prepare_finalize_artifacts(
        &self,
        virtual_occupied: i128,
    ) -> Result<PreparedFinalizeArtifacts> {
        Ok(PreparedFinalizeArtifacts {
            activity_sealed_rows: self.activity_stats.build_rows()?,
            chain_sealed_rows: self.chain_stats.build_rows(virtual_occupied)?,
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
    balances: &FxHashMap<[u8; 32], owners::address::CompactAddressBalance>,
    deltas: &FxHashMap<[u8; 32], owners::address::AddressTxDelta>,
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
    tx_actions_list: Vec<ckbadger_store::types::TxActions>,
}

/// Pure-data payload sent to the flush channel. Contains materialized rows
/// that the flush worker converts to WriteBatch via `prepare_flush`.
pub(crate) struct PendingFlush {
    pub(crate) history_rows: Vec<materialize::MaterializedRow>,
    pub(crate) sealed_rows: Vec<materialize::MaterializedRow>,
}

impl PendingFlush {
    pub(crate) fn allocated_bytes(&self) -> Result<usize> {
        self.history_rows
            .iter()
            .chain(&self.sealed_rows)
            .try_fold(
                self.history_rows
                    .capacity()
                    .checked_mul(std::mem::size_of::<materialize::MaterializedRow>())
                    .and_then(|bytes| {
                        self.sealed_rows
                            .capacity()
                            .checked_mul(std::mem::size_of::<materialize::MaterializedRow>())
                            .and_then(|sealed| bytes.checked_add(sealed))
                    })
                    .ok_or_else(|| anyhow!("pending flush row-vector capacity overflow"))?,
                |total, row| {
                    total
                        .checked_add(row.key.capacity())
                        .and_then(|bytes| bytes.checked_add(row.value.capacity()))
                        .ok_or_else(|| {
                            anyhow!(
                                "pending flush allocated byte count overflow: cf={} key_capacity={} value_capacity={}",
                                row.cf_name,
                                row.key.capacity(),
                                row.value.capacity()
                            )
                        })
                },
            )
    }
}

struct BlockHistoryRows {
    rows: Vec<materialize::MaterializedRow>,
    lock_script_rows: Vec<(crate::sync::types::InternId, materialize::MaterializedRow)>,
    object_activity_count_deltas: FxHashMap<Vec<u8>, i64>,
    identity_activity_count_deltas: FxHashMap<Vec<u8>, i64>,
    tx_actions_list: Vec<ckbadger_store::types::TxActions>,
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
    pub tx_actions_map: HashMap<Vec<u8>, TxActions>,
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

/// Seed the mainnet genesis economic baseline into a test domain store.
///
/// Production derives the baseline at startup (block 0) before the sync loop,
/// so the bulk-build finalize path can read `virtual_occupied` for the
/// knowledge_size calc. These test-session helpers drive finalize directly
/// without that startup step, so they must seed it first (fail-fast otherwise).
/// Values mirror mainnet genesis: 33.6B issued, 8.4B burnt, 6/10 occupied ratio.
fn seed_test_genesis_baseline(domain_store: &CkbadgerStore) -> Result<()> {
    domain_store.set_genesis_baseline(&ckbadger_store::GenesisBaseline {
        total_issuance: 3_360_000_000_000_000_000,
        burnt: 840_000_000_000_000_000,
        virtual_occupied: 504_000_000_000_000_000,
    })
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
        seed_test_genesis_baseline(domain_store.as_ref())?;
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
            let history_count = pending.history_rows.len();
            let sealed_count = pending.sealed_rows.len();
            let prepared = materialize::prepare_flush(
                domain_store.as_ref(),
                append_store.as_ref(),
                pending.history_rows,
                pending.sealed_rows,
            )?;
            materializer.add_external_counts(history_count, sealed_count, 1);
            if !prepared.append_batch.is_empty() {
                append_store.write_batch_no_wal_bulk(prepared.append_batch)?;
            }
            if !prepared.domain_batch.is_empty() {
                domain_store.write_batch_no_wal_bulk(prepared.domain_batch)?;
            }
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
        seed_test_genesis_baseline(domain_store.as_ref())?;
        start_bulk_build_session_marker(domain_store.as_ref(), "bulk-build-test-session", 0)?;
        let mut materializer =
            materialize::Materializer::new(domain_store.as_ref(), append_store.as_ref());
        for batch in block_batches {
            let (batch_stats, _timings, pending) =
                runtime.apply_blocks_hex(batch, true, &FxHashMap::default())?;
            let history_count = pending.history_rows.len();
            let sealed_count = pending.sealed_rows.len();
            let prepared = materialize::prepare_flush(
                domain_store.as_ref(),
                append_store.as_ref(),
                pending.history_rows,
                pending.sealed_rows,
            )?;
            materializer.add_external_counts(history_count, sealed_count, 1);
            if !prepared.append_batch.is_empty() {
                append_store.write_batch_no_wal_bulk(prepared.append_batch)?;
            }
            if !prepared.domain_batch.is_empty() {
                domain_store.write_batch_no_wal_bulk(prepared.domain_batch)?;
            }
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
    let (block_headers, block_numbers_by_hash, txs_by_hash, tx_actions_map) =
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
        tx_actions_map,
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

#[allow(clippy::too_many_arguments)]
fn build_history_batches(
    arena: &facts::FactsArena,
    resolved: &[facts::ResolvedTxFacts<'_>],
    interner: &interner::FrozenIdentityView,
    identity_interner: &interner::IdentityInterner,
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

    // par_iter: one rayon task per block — maximum parallelism, no WriteBatch overhead.
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

    // Merge via Vec::extend (fast pointer moves, no WriteBatch left-fold).
    let estimated_rows = arena.txs.len().saturating_mul(6);
    let mut all_rows: Vec<materialize::MaterializedRow> = Vec::with_capacity(estimated_rows);
    let mut all_object_deltas: FxHashMap<Vec<u8>, i64> = FxHashMap::default();
    let mut all_identity_deltas: FxHashMap<Vec<u8>, i64> = FxHashMap::default();
    let mut all_tx_actions: Vec<ckbadger_store::types::TxActions> = Vec::new();
    for result in block_results {
        let block_rows = result?;
        all_rows.extend(block_rows.rows);

        // Cross-batch dedup: the interner keeps one exact write bit beside each
        // reusable InternId slot. Reclaim clears the bit before the ID can be
        // reused, so a new identity can never inherit stale dedup state.
        for (id, row) in block_rows.lock_script_rows {
            if identity_interner.mark_lock_script_written(id)? {
                all_rows.push(row);
            }
        }

        all_tx_actions.extend(block_rows.tx_actions_list);
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
        tx_actions_list: all_tx_actions,
    })
}

fn semantic_tag_to_bit(tag: facts::CellSemanticTag) -> u16 {
    tag.to_bit()
}

/// Build the TxActions list for a block. Shared by the up-front computation
/// (whose participant tags drive `AddrTxValue.tags`) and the CF_TX_ACTIONS
/// materialization later in the same function. Cellbase txs are included in
/// the returned list because activity stats accumulation walks the full list;
/// CF_TX_ACTIONS itself excludes cellbase at materialize time.
fn build_tx_actions_list_for_bulk(
    block_txs: &[facts::TxFacts],
    block_resolved: &[facts::ResolvedTxFacts<'_>],
    interner: &interner::FrozenIdentityView,
    detectors: &[Box<dyn crate::db::writer::activities::ProtocolDetector>],
) -> Result<Vec<ckbadger_store::types::TxActions>> {
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
                        previous_tx_hash: &input.outpoint.tx_hash,
                        previous_output_index: input.outpoint.index,
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
                        bit_cell_identity_id: match input.protocol_facts.as_ref() {
                            Some(facts::CellProtocolFacts::BitCell(bit_cell)) => {
                                Some(bit_cell.identity_id.as_slice())
                            }
                            _ => None,
                        },
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

    crate::db::writer::activities::build_tx_actions_for_block(&tx_views, detectors)
}

/// Serialize into a pre-allocated Vec, avoiding realloc overhead of `bincode::serialize`
/// which starts with a small buffer and grows. Pre-computes exact size first.
///
/// Trade-off: traverses the value twice (once for size, once for serialization).
/// Net positive for larger structs where avoiding multiple Vec reallocations
/// outweighs the sizing pass.
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
    _token_info_cache: &FxHashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Result<BlockHistoryRows> {
    let mut rows = Vec::with_capacity(block_txs.len().saturating_mul(6));
    let mut object_activity_count_deltas: FxHashMap<Vec<u8>, i64> = FxHashMap::default();
    let mut identity_activity_count_deltas: FxHashMap<Vec<u8>, i64> = FxHashMap::default();

    // Block header + hash index (2 rows per block).
    let header = CachedBlockHeader {
        hash: block.hash.to_vec(),
        parent_hash: block.parent_hash.to_vec(),
        timestamp: block.timestamp_ms,
        epoch_number: block.epoch_number,
        epoch_index: block.epoch_index,
        epoch_length: block.epoch_length,
        dao: block.dao.to_vec(),
        transactions_count: block.transactions_count,
        uncles_count: block.uncles_count,
        cycles: None,
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

    // Compute TxActions for the block up-front. The addr_tx materialization below
    // reads each participant's `tags` from this list to populate `AddrTxValue.tags`,
    // letting filtered scans of CF_ADDR_TXS skip non-matching entries without
    // multi_get-ing CF_TX_ACTIONS. CF_TX_ACTIONS rows themselves are still pushed
    // later in this function, alongside the in-memory list returned for stats.
    let tx_actions_list =
        build_tx_actions_list_for_bulk(block_txs, block_resolved, interner, detectors)?;
    if tx_actions_list.len() != block_txs.len() {
        bail!(
            "bulk build TxActions count mismatch with block_txs: block={} tx_actions={} block_txs={}",
            block.number,
            tx_actions_list.len(),
            block_txs.len()
        );
    }
    // Per-tx-position participant tag lookup, keyed by 32-byte lock_hash.
    // Length-prefixed by tx position to keep lookup O(1) per (tx, lock_hash) pair.
    let participant_tags: Vec<FxHashMap<&[u8], u16>> = tx_actions_list
        .iter()
        .map(|actions| {
            actions
                .participants
                .iter()
                .map(|p| (p.lock_hash.as_slice(), p.tags))
                .collect()
        })
        .collect();

    // Per-tx: tx_index, tx_hash_map, addr_txs, consumed_cells.
    for (tx_position, (tx, resolved_tx)) in block_txs.iter().zip(block_resolved).enumerate() {
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

        let mut stags: u16 = 0;
        for cell in resolved_tx.cells.iter() {
            stags |= semantic_tag_to_bit(cell.semantic_tag);
        }
        for input in &resolved_tx.resolved_inputs {
            stags |= semantic_tag_to_bit(input.semantic_tag);
        }
        let entry = TxIndexEntry {
            is_cellbase: tx.is_cellbase,
            timestamp: tx.timestamp_ms,
            inputs_count: tx.inputs_count,
            outputs_count: tx.outputs_count,
            fee: resolved_tx_fee(tx, resolved_tx)?,
            tx_size: tx.tx_size,
            cycles: tx.cycles,
            semantic_tags: stags,
        };
        let tx_location = keys::encode_composite(&[
            &keys::encode_block_num(tx.block_number),
            &keys::encode_tx_idx(tx.tx_index),
        ]);
        rows.push(materialize::MaterializedRow::new(
            CF_TX_INDEX,
            tx_location.clone(),
            bincode_serialize_presized(&entry)?,
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_TX_HASH_MAP,
            tx.hash.to_vec(),
            tx_location,
        ));

        // Compute per-address capacity change (output_cap - input_cap for each lock).
        let mut per_addr: FxHashMap<crate::sync::types::InternId, (i64, i64, bool, bool)> =
            FxHashMap::default();
        // Tuple: (output_cap_sum, input_cap_sum, has_outputs, has_inputs)
        for output in resolved_tx.cells.iter() {
            let e = per_addr.entry(output.lock_script_hash_id).or_default();
            e.0 = e.0.checked_add(output.capacity).ok_or_else(|| {
                anyhow::anyhow!(
                    "output capacity sum overflow for addr in tx block={}",
                    tx.block_number
                )
            })?;
            e.2 = true;
        }
        for input in &resolved_tx.resolved_inputs {
            let e = per_addr.entry(input.lock_script_hash_id).or_default();
            e.1 = e.1.checked_add(input.capacity).ok_or_else(|| {
                anyhow::anyhow!(
                    "input capacity sum overflow for addr in tx block={}",
                    tx.block_number
                )
            })?;
            e.3 = true;
        }
        let tags_for_tx = &participant_tags[tx_position];
        for (lock_hash_id, (out_cap, in_cap, has_out, has_in)) in per_addr {
            let capacity_change = out_cap.checked_sub(in_cap).ok_or_else(|| {
                anyhow::anyhow!(
                    "capacity_change overflow: out={} in={} block={}",
                    out_cap,
                    in_cap,
                    tx.block_number
                )
            })?;
            let lock_hash_bytes = interner.resolve_bytes(lock_hash_id);
            // ParticipantDelta is emitted for every (lock_hash) that touched inputs
            // or outputs of this tx, which is exactly the same set we iterate here,
            // so a missing entry is a real invariant violation, not a normal case.
            let tags = *tags_for_tx.get(lock_hash_bytes).ok_or_else(|| {
                anyhow::anyhow!(
                    "missing participant tags for addr_tx: block={}, tx_idx={}, lock_hash=0x{}",
                    tx.block_number,
                    tx.tx_index,
                    hex::encode(lock_hash_bytes)
                )
            })?;
            let value =
                ckbadger_store::types::AddrTxValue::new(capacity_change, has_in, has_out, tags);
            let encoded_value = bincode_serialize_presized(&value)?;
            rows.push(materialize::MaterializedRow::new(
                CF_ADDR_TXS,
                keys::encode_addr_tx_key(lock_hash_bytes, tx.block_number, tx.tx_index, &tx.hash),
                encoded_value,
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

    // CF_TX_ACTIONS materialization from the tx_actions_list computed at the top
    // of this function. Cellbase txs are excluded from CF_TX_ACTIONS — the API
    // filters them at read time (activities.rs:795) and they are never displayed.
    // Activity stats accumulation uses the returned in-memory list, not CF_TX_ACTIONS.
    for tx_actions in &tx_actions_list {
        if !tx_actions.is_cellbase {
            rows.push(materialize::MaterializedRow::new(
                CF_TX_ACTIONS,
                keys::encode_tx_actions_key(
                    tx_actions.block_number,
                    tx_actions.tx_index,
                    &tx_actions.tx_hash,
                ),
                bincode_serialize_presized(tx_actions)?,
            ));
        }
    }

    // Object/identity collection activities for this block's txs.
    {
        let mut object_activity_acc =
            crate::db::writer::object_activity_acc::ObjectCollectionActivityAccumulator::new();
        let mut identity_activity_acc =
            crate::db::writer::object_activity_acc::ObjectCollectionActivityAccumulator::new();

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
                    facts::CellProtocolFacts::BitCell(bit_cell) => {
                        identity_activity_acc.record(
                            &BIT_CELL_SENTINEL_COLLECTION,
                            &tx.tx_hash,
                            &bit_cell.identity_id,
                            &tx.block_hash,
                            tx.block_number,
                            tx.tx_index,
                            tx.timestamp_ms,
                            false,
                        );
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
                    facts::CellProtocolFacts::BitCell(bit_cell) => {
                        identity_activity_acc.record(
                            &BIT_CELL_SENTINEL_COLLECTION,
                            &tx.tx_hash,
                            &bit_cell.identity_id,
                            &tx.block_hash,
                            tx.block_number,
                            tx.tx_index,
                            tx.timestamp_ms,
                            true,
                        );
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
                    keys::encode_object_collection_activity_key(
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
                keys::encode_object_collection_activity_key(
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
                keys::encode_object_collection_activity_key(
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

    // Cell payloads (CF_CELLS) + data_hash index (CF_CELL_BY_DATA_HASH)
    // + lock script mapping (CF_LOCK_SCRIPTS) for this block's cells.
    //
    // Lock script rows are collected separately (with their InternId) so the
    // serial merge phase can perform exact cross-batch dedup via the interner's
    // dense per-slot write markers. Per-block dedup still happens here via
    // `seen_lock_ids` to avoid redundant serialization.
    let mut seen_lock_ids = rustc_hash::FxHashSet::default();
    let mut lock_script_rows = Vec::new();
    for tx in block_txs {
        for cell in &arena_cells[tx.output_range.clone()] {
            let outpoint_key =
                keys::encode_outpoint(&cell.outpoint.tx_hash, cell_outpoint_index_i16(cell)?);
            rows.push(materialize::MaterializedRow::new(
                CF_CELLS,
                outpoint_key.to_vec(),
                bincode_serialize_presized(&cell_facts_to_live_cell_info(cell, interner))?,
            ));

            // Lock script mapping — dedup within block, cross-batch dedup in merge.
            if seen_lock_ids.insert(cell.lock_script_hash_id) {
                let lock_hash = interner.resolve_bytes(cell.lock_script_hash_id).to_vec();
                let entry = LockScriptEntry {
                    code_hash: interner.resolve_bytes(cell.lock_code_hash_id).to_vec(),
                    hash_type: cell.lock_hash_type,
                    args: interner.resolve_bytes(cell.lock_args_id).to_vec(),
                };
                lock_script_rows.push((
                    cell.lock_script_hash_id,
                    materialize::MaterializedRow::new(
                        CF_LOCK_SCRIPTS,
                        lock_hash,
                        bincode_serialize_presized(&entry)?,
                    ),
                ));
            }

            if let Some(data_hash) = &cell.data_hash {
                rows.push(materialize::MaterializedRow::new(
                    CF_CELL_BY_DATA_HASH,
                    keys::encode_cell_index_key(
                        data_hash,
                        cell.created_at_block,
                        &cell.outpoint.tx_hash,
                        cell_outpoint_index_i16(cell)?,
                    ),
                    vec![],
                ));
            }
        }
    }

    Ok(BlockHistoryRows {
        rows,
        lock_script_rows,
        object_activity_count_deltas,
        identity_activity_count_deltas,
        tx_actions_list,
    })
}

#[cfg(test)]
fn build_object_collection_activity_rows(
    resolved: &[facts::ResolvedTxFacts<'_>],
    object_activity_count_deltas: &mut FxHashMap<Vec<u8>, i64>,
    identity_activity_count_deltas: &mut FxHashMap<Vec<u8>, i64>,
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut object_activity_acc =
        crate::db::writer::object_activity_acc::ObjectCollectionActivityAccumulator::new();
    let mut identity_activity_acc =
        crate::db::writer::object_activity_acc::ObjectCollectionActivityAccumulator::new();
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
                facts::CellProtocolFacts::BitCell(bit_cell) => {
                    identity_activity_acc.record(
                        &BIT_CELL_SENTINEL_COLLECTION,
                        &tx.tx_hash,
                        &bit_cell.identity_id,
                        &tx.block_hash,
                        tx.block_number,
                        tx.tx_index,
                        tx.timestamp_ms,
                        false,
                    );
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
                facts::CellProtocolFacts::BitCell(bit_cell) => {
                    identity_activity_acc.record(
                        &BIT_CELL_SENTINEL_COLLECTION,
                        &tx.tx_hash,
                        &bit_cell.identity_id,
                        &tx.block_hash,
                        tx.block_number,
                        tx.tx_index,
                        tx.timestamp_ms,
                        true,
                    );
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
                keys::encode_object_collection_activity_key(
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
            keys::encode_object_collection_activity_key(
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
            keys::encode_object_collection_activity_key(
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
    tx_actions_list: &[TxActions],
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut accumulator = ActivityStatsAccumulator::default();
    accumulator.apply_tx_actions(tx_actions_list)?;
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
        Box::new(crate::db::writer::rgbpp_detector::RgbppDetector::new())
            as Box<dyn crate::db::writer::activities::ProtocolDetector>,
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

fn emit_final_snapshot_rows<F>(
    sequencer: &sequencer::BulkSequencer,
    interner: &interner::FrozenIdentityView,
    mut emit: F,
) -> Result<()>
where
    F: FnMut(materialize::MaterializedRow) -> Result<()>,
{
    for (outpoint, slot) in sequencer.live_slots() {
        let outpoint_index = live_slot_outpoint_index_i16(outpoint)?;
        emit(materialize::MaterializedRow::new(
            CF_LIVE_CELLS,
            keys::encode_outpoint(&outpoint.tx_hash, outpoint_index).to_vec(),
            slot.created_at_block.to_le_bytes().to_vec(),
        ))?;
        emit(materialize::MaterializedRow::new(
            CF_CELL_BY_LOCK,
            keys::encode_cell_index_key(
                interner.resolve_bytes(slot.lock_script_hash_id),
                slot.created_at_block,
                &outpoint.tx_hash,
                outpoint_index,
            ),
            Vec::new(),
        ))?;
        emit(materialize::MaterializedRow::new(
            CF_CELL_BY_LOCK_CODE,
            keys::encode_cell_index_key(
                interner.resolve_bytes(slot.lock_code_hash_id),
                slot.created_at_block,
                &outpoint.tx_hash,
                outpoint_index,
            ),
            Vec::new(),
        ))?;
        if let Some(type_script_hash_id) = slot.type_script_hash_id {
            emit(materialize::MaterializedRow::new(
                CF_CELL_BY_TYPE,
                keys::encode_cell_index_key(
                    interner.resolve_bytes(type_script_hash_id),
                    slot.created_at_block,
                    &outpoint.tx_hash,
                    outpoint_index,
                ),
                Vec::new(),
            ))?;
        }
        if let Some(type_code_hash_id) = slot.type_code_hash_id {
            emit(materialize::MaterializedRow::new(
                CF_CELL_BY_TYPE_CODE,
                keys::encode_cell_index_key(
                    interner.resolve_bytes(type_code_hash_id),
                    slot.created_at_block,
                    &outpoint.tx_hash,
                    outpoint_index,
                ),
                Vec::new(),
            ))?;
        }
    }

    Ok(())
}

#[cfg(test)]
fn build_final_snapshot_rows(
    sequencer: &sequencer::BulkSequencer,
    interner: &interner::FrozenIdentityView,
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut rows = Vec::new();
    emit_final_snapshot_rows(sequencer, interner, |row| {
        rows.push(row);
        Ok(())
    })?;
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

    // For DAO withdrawal-completion inputs, the output capacity exceeds the raw
    // input capacity because the output includes DAO compensation (interest).
    // Add compensation to the input side so the fee reflects the actual miner fee.
    let total_dao_compensation = resolved_tx
        .resolved_inputs
        .iter()
        .try_fold(0i64, |acc, input| {
            let compensation = match (&input.dao_state, &input.dao_compensation_ars) {
                (
                    Some(facts::DaoCellState::WithdrawRequest { .. }),
                    Some(facts::DaoCompensationArs {
                        deposit_ar,
                        withdraw_request_ar,
                    }),
                ) => crate::db::writer::dao::calculate_dao_compensation_from_ar(
                    input.capacity,
                    *deposit_ar,
                    *withdraw_request_ar,
                )
                .map_err(|e| {
                    anyhow!(
                        "bulk build DAO compensation error while materializing tx index: tx=0x{} block={} tx_index={} outpoint=0x{}:{}: {e}",
                        hex::encode(tx.hash),
                        tx.block_number,
                        tx.tx_index,
                        hex::encode(input.outpoint.tx_hash),
                        input.outpoint.index
                    )
                })?,
                (Some(facts::DaoCellState::WithdrawRequest { .. }), None) => {
                    return Err(anyhow!(
                        "bulk build missing DAO compensation ARs while materializing tx index: tx=0x{} block={} tx_index={} outpoint=0x{}:{}",
                        hex::encode(tx.hash),
                        tx.block_number,
                        tx.tx_index,
                        hex::encode(input.outpoint.tx_hash),
                        input.outpoint.index
                    ));
                }
                _ => 0i64,
            };
            acc.checked_add(compensation).ok_or_else(|| {
                anyhow!(
                    "bulk build DAO compensation overflow while materializing tx index: tx=0x{} block={} tx_index={}",
                    hex::encode(tx.hash),
                    tx.block_number,
                    tx.tx_index
                )
            })
        })?;

    let effective_input = total_input_capacity
        .checked_add(total_dao_compensation)
        .ok_or_else(|| {
            anyhow!(
                "bulk build effective input capacity overflow while materializing tx index: tx=0x{} block={} tx_index={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index
            )
        })?;

    effective_input
        .checked_sub(total_output_capacity)
        .ok_or_else(|| {
            anyhow!(
                "bulk build negative fee while materializing tx index: tx=0x{} block={} tx_index={} inputs={} dao_compensation={} outputs={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index,
                total_input_capacity,
                total_dao_compensation,
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

fn live_slot_outpoint_index_i16(outpoint: &facts::OutPointKey) -> Result<i16> {
    i16::try_from(outpoint.index).map_err(|_| {
        anyhow!(
            "bulk build live outpoint index exceeds i16 while materializing live cells: tx=0x{} output_index={}",
            hex::encode(outpoint.tx_hash),
            outpoint.index
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
    HashMap<Vec<u8>, TxActions>,
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

    let mut tx_actions_map = HashMap::new();
    let activity_iter = domain_store.iterator_cf(domain_store.cf_tx_actions(), IteratorMode::Start);
    for item in activity_iter {
        let (key, value) = item?;
        let actions: TxActions = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize TxActions in bulk artifact snapshot helper: key=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        tx_actions_map.insert(key.to_vec(), actions);
    }

    Ok((
        block_headers,
        block_numbers_by_hash,
        txs_by_hash,
        tx_actions_map,
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
    let mut token_holders: HashMap<Vec<u8>, HashMap<Vec<u8>, TokenBalance>> = HashMap::new();
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
    let mut addr_tokens: HashMap<Vec<u8>, HashMap<Vec<u8>, TokenBalance>> = HashMap::new();
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
    let dotbit_agg = domain_store.get_identity_collection_aggregate(&DOTBIT_SENTINEL_COLLECTION)?;
    let bit_cell_agg =
        domain_store.get_identity_collection_aggregate(&BIT_CELL_SENTINEL_COLLECTION)?;
    let mut identities_by_collection = HashMap::new();
    for collection_id in [
        &DID_CKB_SENTINEL_COLLECTION,
        &DOTBIT_SENTINEL_COLLECTION,
        &BIT_CELL_SENTINEL_COLLECTION,
    ] {
        let mut identity_ids =
            domain_store.list_identity_ids_by_collection(collection_id, None, usize::MAX)?;
        identity_ids.sort();
        if !identity_ids.is_empty() {
            identities_by_collection.insert(collection_id.to_vec(), identity_ids);
        }
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
    let dotbit_owner_counts = domain_store
        .list_identity_owner_counts(&DOTBIT_SENTINEL_COLLECTION)?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let bit_cell_owner_counts = domain_store
        .list_identity_owner_counts(&BIT_CELL_SENTINEL_COLLECTION)?
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
        dotbit_agg,
        bit_cell_agg,
        identities_by_collection,
        spores_by_cluster,
        did_owner_counts,
        dotbit_owner_counts,
        bit_cell_owner_counts,
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
    use crate::parser::bit_cell::BIT_CELL_CODE_HASH_TESTNET;
    use crate::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET;
    use crate::parser::spore::{CLUSTER_CODE_HASH_MAINNET_V2, SPORE_CODE_HASH_MAINNET_V2};
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
        TokenTransferRecord, TxActions, BIT_CELL_SENTINEL_COLLECTION,
    };
    use ckbadger_store::{
        keys, CF_ADDR_TXS, CF_IDENTITY_COLLECTION_ACTIVITIES, CF_OBJECT_COLLECTION_ACTIVITIES,
        CF_TX_ACTIONS,
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

    fn create_bit_cell_type_script() -> Script {
        Script {
            code_hash: BIT_CELL_CODE_HASH_TESTNET.to_string(),
            hash_type: "type".to_string(),
            args: "0x".to_string(),
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
                    capacity: format!("0x{:x}", 200_00000000u64),
                    lock: fixture_lock_script(&format!("0x{}", "03".repeat(20))),
                    type_: Some(create_bit_cell_type_script()),
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
                "0x000000003c00000010000000240000002c000000a7d4860aaf1dc83daedf75d6022811d2c2ae250b1b46fc69000000000c00000032303234303530372e626974".to_string(),
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

        let root = unique_temp_test_dir("bulk-build-addr-tx-test");
        std::fs::create_dir_all(&root).expect("create root");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("create domain");
        std::fs::create_dir_all(&append_path).expect("create append");
        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("open domain");
        let append_store = CkbadgerStore::open_append_only(&append_path).expect("open append");

        let history = build_history_batches(
            &arena,
            &resolved,
            &frozen,
            &interner,
            true,
            &FxHashMap::default(),
        )
        .expect("history batches");

        // Convert rows to WriteBatch and commit to stores, then scan for CF_ADDR_TXS keys.
        let prepared =
            materialize::prepare_flush(&domain_store, &append_store, history.rows, Vec::new())
                .expect("prepare flush");
        domain_store
            .write_batch_no_wal_bulk(prepared.domain_batch)
            .expect("commit domain");
        append_store
            .write_batch_no_wal_bulk(prepared.append_batch)
            .expect("commit append");

        let expected = [
            keys::encode_addr_tx_key(&lock_a_hash, 14_000_888, 0, &create_tx_hash),
            keys::encode_addr_tx_key(&lock_a_hash, 14_000_888, 1, &split_tx_hash),
            keys::encode_addr_tx_key(&lock_b_hash, 14_000_888, 1, &split_tx_hash),
        ];

        let mut actual_keys: HashSet<Vec<u8>> = HashSet::new();
        for item in domain_store.iterator_cf(domain_store.cf(CF_ADDR_TXS), IteratorMode::Start) {
            let (key, _) = item.expect("iterator item");
            actual_keys.insert(key.to_vec());
        }

        assert_eq!(actual_keys.len(), expected.len());
        for key in expected {
            assert!(actual_keys.contains(&key));
        }
        let _ = std::fs::remove_dir_all(&root);
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

        let root = unique_temp_test_dir("bulk-build-token-transfer-test");
        std::fs::create_dir_all(&root).expect("create root");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("create domain");
        std::fs::create_dir_all(&append_path).expect("create append");
        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("open domain");
        let append_store = CkbadgerStore::open_append_only(&append_path).expect("open append");

        let history = build_history_batches(
            &arena,
            &resolved,
            &frozen,
            &interner,
            true,
            &FxHashMap::default(),
        )
        .expect("history batches");
        let prepared =
            materialize::prepare_flush(&domain_store, &append_store, history.rows, Vec::new())
                .expect("prepare flush");
        domain_store
            .write_batch_no_wal_bulk(prepared.domain_batch)
            .expect("commit domain");
        append_store
            .write_batch_no_wal_bulk(prepared.append_batch)
            .expect("commit append");

        let token_records: HashMap<Vec<u8>, TokenTransferRecord> = domain_store
            .iterator_cf(domain_store.cf(CF_TOKEN_TRANSFERS), IteratorMode::Start)
            .map(|item| {
                let (key, value) = item.expect("iterator item");
                (
                    key.to_vec(),
                    bincode::deserialize(&value).expect("deserialize token transfer"),
                )
            })
            .collect();
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(token_records.len(), 2);

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
        let history = build_history_batches(
            &arena,
            &resolved,
            &frozen,
            &interner,
            true,
            &FxHashMap::default(),
        )
        .expect("history batches");
        // Convert rows to WriteBatch and write to stores (no-WAL) to populate memtables.
        let prepared =
            materialize::prepare_flush(&domain_store, &append_store, history.rows, Vec::new())
                .expect("prepare flush");
        if !prepared.append_batch.is_empty() {
            append_store
                .write_batch_no_wal_bulk(prepared.append_batch)
                .expect("commit append");
        }
        if !prepared.domain_batch.is_empty() {
            domain_store
                .write_batch_no_wal_bulk(prepared.domain_batch)
                .expect("commit domain");
        }

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
                    max_supply: None,
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
    fn build_history_rows_materializes_ckb_tx_actions_in_tx_order() {
        let block = bulk_build_addr_tx_fixture();
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

        let root = unique_temp_test_dir("bulk-build-activity-test");
        std::fs::create_dir_all(&root).expect("create root");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("create domain");
        std::fs::create_dir_all(&append_path).expect("create append");
        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("open domain");
        let append_store = CkbadgerStore::open_append_only(&append_path).expect("open append");

        let result = build_history_batches(
            &arena,
            &resolved,
            &frozen,
            &interner,
            true,
            &FxHashMap::default(),
        )
        .expect("history batches");
        let prepared =
            materialize::prepare_flush(&domain_store, &append_store, result.rows, Vec::new())
                .expect("prepare flush");
        domain_store
            .write_batch_no_wal_bulk(prepared.domain_batch)
            .expect("commit domain");
        append_store
            .write_batch_no_wal_bulk(prepared.append_batch)
            .expect("commit append");

        let activity_rows: Vec<(Vec<u8>, TxActions)> = domain_store
            .iterator_cf(domain_store.cf(CF_TX_ACTIONS), IteratorMode::Start)
            .map(|item| {
                let (key, value) = item.expect("iterator item");
                (
                    key.to_vec(),
                    bincode::deserialize(&value).expect("deserialize TxActions"),
                )
            })
            .collect();

        // Only non-cellbase tx materialized to CF_TX_ACTIONS.
        assert_eq!(activity_rows.len(), 1);
        let split_key = keys::encode_tx_actions_key(14_000_888, 1, &split_tx_hash);
        let (ref actual_key, ref split_actions) = activity_rows[0];
        assert_eq!(*actual_key, split_key);
        assert_eq!(split_actions.tx_hash, split_tx_hash);
        assert!(!split_actions.is_cellbase);
        assert_eq!(split_actions.participants.len(), 2);

        let participant_a = split_actions
            .participants
            .iter()
            .find(|p| p.lock_hash == lock_a_hash)
            .expect("participant a");
        assert_eq!(participant_a.ckb_delta, -100_00000000);

        let participant_b = split_actions
            .participants
            .iter()
            .find(|p| p.lock_hash == lock_b_hash)
            .expect("participant b");
        assert!(participant_b.ckb_delta > 0);

        // tx_actions_list must still include cellbase for activity stats accumulation.
        assert_eq!(result.tx_actions_list.len(), 2);
        assert!(result.tx_actions_list.iter().any(|a| a.is_cellbase));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn build_history_rows_excludes_cellbase_from_tx_actions_cf() {
        let block = bulk_build_addr_tx_fixture();
        let interner = interner::IdentityInterner::default();
        let (arena, _) =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &interner)
                .expect("facts arena");
        let mut seq = sequencer::BulkSequencer::default();
        let resolved = seq.resolve(&arena).expect("resolved txs");
        let frozen = interner.snapshot_for_reads();

        let root = unique_temp_test_dir("bulk-build-cellbase-exclude-test");
        std::fs::create_dir_all(&root).expect("create root");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("create domain");
        std::fs::create_dir_all(&append_path).expect("create append");
        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("open domain");
        let append_store = CkbadgerStore::open_append_only(&append_path).expect("open append");

        let result = build_history_batches(
            &arena,
            &resolved,
            &frozen,
            &interner,
            true,
            &FxHashMap::default(),
        )
        .expect("history batches");
        let prepared =
            materialize::prepare_flush(&domain_store, &append_store, result.rows, Vec::new())
                .expect("prepare flush");
        domain_store
            .write_batch_no_wal_bulk(prepared.domain_batch)
            .expect("commit domain");
        append_store
            .write_batch_no_wal_bulk(prepared.append_batch)
            .expect("commit append");

        // CF_TX_ACTIONS rows should NOT include cellbase.
        let tx_action_rows: Vec<TxActions> = domain_store
            .iterator_cf(domain_store.cf(CF_TX_ACTIONS), IteratorMode::Start)
            .map(|item| {
                let (_key, value) = item.expect("iterator item");
                bincode::deserialize(&value).expect("deserialize tx_actions")
            })
            .collect();

        // tx_actions_list should include ALL txs (including cellbase) for stats accumulation.
        let cellbase_in_list = result.tx_actions_list.iter().any(|a| a.is_cellbase);
        let cellbase_in_rows = tx_action_rows.iter().any(|a| a.is_cellbase);

        assert!(
            cellbase_in_list,
            "tx_actions_list must include cellbase for stats accumulation"
        );
        assert!(
            !cellbase_in_rows,
            "CF_TX_ACTIONS rows must NOT include cellbase"
        );

        // Non-cellbase tx count should match
        let non_cellbase_in_list = result
            .tx_actions_list
            .iter()
            .filter(|a| !a.is_cellbase)
            .count();
        assert_eq!(tx_action_rows.len(), non_cellbase_in_list);
        let _ = std::fs::remove_dir_all(&root);
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
        let append_store =
            CkbadgerStore::open_append_only(&append_path).expect("open append-only store");
        let history = build_history_batches(
            &arena,
            &resolved,
            &frozen,
            &interner,
            true,
            &FxHashMap::default(),
        )
        .expect("history batches");
        let sealed_rows =
            build_sealed_aggregate_rows(&history.tx_actions_list).expect("sealed rows");
        let final_snapshot_rows =
            build_final_snapshot_rows(&sequencer, &frozen).expect("final snapshot rows");

        // Convert rows to WriteBatch and commit to stores.
        let prepared =
            materialize::prepare_flush(&domain_store, &append_store, history.rows, Vec::new())
                .expect("prepare flush");
        domain_store
            .write_batch_no_wal_bulk(prepared.domain_batch)
            .expect("commit domain");
        append_store
            .write_batch_no_wal_bulk(prepared.append_batch)
            .expect("commit append");

        let open_tx_actions: TxActions = domain_store
            .iterator_cf(domain_store.cf(CF_TX_ACTIONS), IteratorMode::Start)
            .map(|item| {
                let (_key, value) = item.expect("iterator item");
                bincode::deserialize::<TxActions>(&value).expect("deserialize TxActions")
            })
            .find(|actions| !actions.is_cellbase)
            .expect("non-cellbase TxActions");
        assert!(
            !open_tx_actions.protocol_actions.is_empty(),
            "fiber protocol actions should be at TX level"
        );
        assert_eq!(open_tx_actions.protocol_actions[0].protocol, "fiber");
        assert_eq!(open_tx_actions.protocol_actions[0].action, "channel_open");

        for tx in &resolved {
            owners.apply_tx(tx, &ctx).expect("apply core owners");
        }
        let mut materializer = materialize::Materializer::new(&domain_store, &append_store);
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
    fn build_history_rows_materializes_spore_and_bit_cell_identity_activities() {
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

        let root = unique_temp_test_dir("bulk-build-spore-bit-cell-activity-test");
        std::fs::create_dir_all(&root).expect("create root");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("create domain");
        std::fs::create_dir_all(&append_path).expect("create append");
        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("open domain");
        let append_store = CkbadgerStore::open_append_only(&append_path).expect("open append");

        let history = build_history_batches(
            &arena,
            &resolved,
            &frozen,
            &interner,
            true,
            &FxHashMap::default(),
        )
        .expect("history batches");
        let prepared =
            materialize::prepare_flush(&domain_store, &append_store, history.rows, Vec::new())
                .expect("prepare flush");
        domain_store
            .write_batch_no_wal_bulk(prepared.domain_batch)
            .expect("commit domain");
        append_store
            .write_batch_no_wal_bulk(prepared.append_batch)
            .expect("commit append");

        let object_rows: std::collections::HashMap<Vec<u8>, ObjectCollectionActivityEntry> =
            domain_store
                .iterator_cf(
                    domain_store.cf(CF_OBJECT_COLLECTION_ACTIVITIES),
                    IteratorMode::Start,
                )
                .map(|item| {
                    let (key, value) = item.expect("iterator item");
                    (
                        key.to_vec(),
                        bincode::deserialize(&value)
                            .expect("deserialize object collection activity"),
                    )
                })
                .collect();
        let identity_rows: std::collections::HashMap<Vec<u8>, ObjectCollectionActivityEntry> =
            domain_store
                .iterator_cf(
                    domain_store.cf(CF_IDENTITY_COLLECTION_ACTIVITIES),
                    IteratorMode::Start,
                )
                .map(|item| {
                    let (key, value) = item.expect("iterator item");
                    (
                        key.to_vec(),
                        bincode::deserialize(&value)
                            .expect("deserialize identity collection activity"),
                    )
                })
                .collect();
        let _ = std::fs::remove_dir_all(&root);

        let cluster_mint_key = keys::encode_object_collection_activity_key(
            &cluster_id,
            14_001_000,
            0,
            &create_block_hash,
            &create_tx_hash,
        );
        let cluster_transfer_key = keys::encode_object_collection_activity_key(
            &cluster_id,
            14_001_001,
            1,
            &transfer_block_hash,
            &transfer_tx_hash,
        );
        let bit_cell_mint_key = keys::encode_object_collection_activity_key(
            &BIT_CELL_SENTINEL_COLLECTION,
            14_001_000,
            0,
            &create_block_hash,
            &create_tx_hash,
        );
        let bit_cell_burn_key = keys::encode_object_collection_activity_key(
            &BIT_CELL_SENTINEL_COLLECTION,
            14_001_001,
            1,
            &transfer_block_hash,
            &transfer_tx_hash,
        );

        assert_eq!(object_rows.len(), 2);
        assert_eq!(identity_rows.len(), 2);

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

        let bit_cell_mint = identity_rows
            .get(bit_cell_mint_key.as_slice())
            .expect(".bit Cell mint activity");
        assert_eq!(bit_cell_mint.tx_hash, create_tx_hash);
        assert_eq!(bit_cell_mint.block_hash, create_block_hash);
        assert_eq!(bit_cell_mint.actions.len(), 1);
        assert!(matches!(bit_cell_mint.actions[0], AssetAction::Mint));

        let bit_cell_burn = identity_rows
            .get(bit_cell_burn_key.as_slice())
            .expect(".bit Cell burn activity");
        assert_eq!(bit_cell_burn.tx_hash, transfer_tx_hash);
        assert_eq!(bit_cell_burn.block_hash, transfer_block_hash);
        assert_eq!(bit_cell_burn.actions.len(), 1);
        assert!(matches!(bit_cell_burn.actions[0], AssetAction::Burn));
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

        let mint_key = keys::encode_object_collection_activity_key(
            &DOTBIT_SENTINEL_COLLECTION,
            300,
            0,
            &[0xa0; 32],
            &[0x31; 32],
        );
        let transfer_key = keys::encode_object_collection_activity_key(
            &DOTBIT_SENTINEL_COLLECTION,
            301,
            0,
            &[0xa1; 32],
            &[0x32; 32],
        );
        let recycle_key = keys::encode_object_collection_activity_key(
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
        let mint_key = keys::encode_object_collection_activity_key(
            &class_id,
            400,
            0,
            &[0xb0; 32],
            &[0x41; 32],
        );
        let mint = object_rows.get(mint_key.as_slice()).expect("mnft mint");
        assert_eq!(mint.actions.len(), 1);
        assert!(matches!(mint.actions[0], AssetAction::Mint));

        // Verify transfer
        let transfer_key = keys::encode_object_collection_activity_key(
            &class_id,
            401,
            0,
            &[0xb1; 32],
            &[0x42; 32],
        );
        let transfer = object_rows
            .get(transfer_key.as_slice())
            .expect("mnft transfer");
        assert_eq!(transfer.actions.len(), 1);
        assert!(matches!(transfer.actions[0], AssetAction::Transfer));

        // Verify burn
        let burn_key = keys::encode_object_collection_activity_key(
            &class_id,
            402,
            0,
            &[0xb2; 32],
            &[0x43; 32],
        );
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
            parent_hash: vec![0u8; 32],
            timestamp: 1710000000000,
            epoch_number: 100,
            epoch_index: 5,
            epoch_length: 1800,
            dao: vec![0x00; 32],
            transactions_count: 42,
            uncles_count: 0,
            cycles: None,
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
            semantic_tags: 0,
        };
        let standard = bincode::serialize(&entry).unwrap();
        let presized = bincode_serialize_presized(&entry).unwrap();
        assert_eq!(standard, presized);
    }

    #[test]
    fn test_bincode_serialize_presized_empty_vec_field() {
        let header = CachedBlockHeader {
            hash: vec![],
            parent_hash: vec![0u8; 32],
            timestamp: 0,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 0,
            dao: vec![],
            transactions_count: 0,
            uncles_count: 0,
            cycles: None,
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

        // Mainnet genesis virtual-occupied; both paths must use the same value.
        let virtual_occupied: i128 = 504_000_000_000_000_000;
        let direct_activity_rows = runtime.activity_stats.build_rows().unwrap();
        let direct_chain_rows = runtime.chain_stats.build_rows(virtual_occupied).unwrap();
        let frozen = runtime.interner.snapshot_for_reads();
        let direct_snapshot_rows = build_final_snapshot_rows(&runtime.sequencer, &frozen).unwrap();

        let prepared = runtime
            .prepare_finalize_artifacts(virtual_occupied)
            .unwrap();
        assert_eq!(prepared.activity_sealed_rows, direct_activity_rows);
        assert_eq!(prepared.chain_sealed_rows, direct_chain_rows);
        assert!(!direct_snapshot_rows.is_empty());
    }

    #[test]
    fn apply_tx_actions_accumulates_daily_and_hourly_stats() {
        let tx_actions = TxActions {
            tx_hash: vec![0x11; 32],
            block_hash: vec![0x22; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1_700_000_000_000, // 2023-11-14 22:13:20 UTC
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ckbadger_store::types::ParticipantDelta {
                lock_hash: vec![0x33; 32],
                ckb_delta: 100_00000000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            }],
        };

        let mut acc = ActivityStatsAccumulator::default();
        acc.apply_tx_actions(&[tx_actions]).unwrap();

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
    fn apply_tx_actions_excludes_coinbase_from_unique_addrs() {
        let tx_actions = TxActions {
            tx_hash: vec![0x11; 32],
            block_hash: vec![0x22; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1_700_000_000_000,
            is_cellbase: true,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ckbadger_store::types::ParticipantDelta {
                lock_hash: vec![0x33; 32],
                ckb_delta: 100_00000000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            }],
        };

        let mut acc = ActivityStatsAccumulator::default();
        acc.apply_tx_actions(&[tx_actions]).unwrap();

        let date_key = ckbadger_common::block_date_from_ms(1_700_000_000_000)
            .format("%Y%m%d")
            .to_string();
        let daily = acc.daily_stats.get(&date_key).expect("daily stats");
        assert_eq!(daily.coinbase_count, 1);
        assert_eq!(daily.transfer_count, 0);
        assert!(!acc.daily_addrs.contains_key(&date_key) || acc.daily_addrs[&date_key].is_empty());
    }

    #[test]
    fn ckb_median_time_uses_the_upper_value_for_an_even_window() {
        assert_eq!(ckb_median_time_ms(&[10, 20]).unwrap(), 20);
        assert_eq!(ckb_median_time_ms(&[30, 10, 20]).unwrap(), 20);
    }

    #[test]
    fn activity_stats_seal_only_buckets_older_than_ckb_mtp() {
        let action = TxActions {
            tx_hash: vec![0x11; 32],
            block_hash: vec![0x22; 32],
            block_number: 0,
            tx_index: 0,
            timestamp: 0,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ckbadger_store::types::ParticipantDelta {
                lock_hash: vec![0x33; 32],
                ckb_delta: 1,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            }],
        };
        let block = |number, timestamp_ms| facts::BlockFacts {
            number,
            timestamp_ms,
            ..Default::default()
        };

        let mut acc = ActivityStatsAccumulator::default();
        assert!(acc
            .apply_batch(&[block(0, 0)], std::slice::from_ref(&action))
            .unwrap()
            .is_empty());

        // [0, 2h, 3h] has MTP=2h. The action's UTC+8 08:00 hourly
        // bucket ended at 1h, while its daily bucket is still open.
        let sealed = acc
            .apply_batch(&[block(1, 7_200_000), block(2, 10_800_000)], &[])
            .unwrap();
        assert_eq!(sealed.len(), 2, "hourly stats + hourly address set");
        let hour_key = ckbadger_common::block_datetime_from_ms(0)
            .format("%Y%m%d%H")
            .to_string();
        let day_key = ckbadger_common::block_date_from_ms(0)
            .format("%Y%m%d")
            .to_string();
        assert!(!acc.hourly_stats.contains_key(&hour_key));
        assert!(!acc.hourly_addrs.contains_key(&hour_key));
        assert!(acc.daily_stats.contains_key(&day_key));

        let err = acc
            .apply_batch(&[block(3, 14_400_000)], &[action])
            .unwrap_err()
            .to_string();
        assert!(err.contains("already sealed activity bucket"), "{err}");
        assert!(err.contains("block=0"), "{err}");
    }

    #[test]
    fn incremental_activity_sealing_matches_one_shot_materialization_exactly() {
        let action = TxActions {
            tx_hash: vec![0x41; 32],
            block_hash: vec![0x42; 32],
            block_number: 0,
            tx_index: 0,
            timestamp: 0,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ckbadger_store::types::ParticipantDelta {
                lock_hash: vec![0x43; 32],
                ckb_delta: 123,
                used_delta: 45,
                item_deltas: vec![],
                tags: 0,
            }],
        };
        let block = |number, timestamp_ms| facts::BlockFacts {
            number,
            timestamp_ms,
            ..Default::default()
        };

        let mut one_shot = ActivityStatsAccumulator::default();
        one_shot
            .apply_tx_actions(std::slice::from_ref(&action))
            .unwrap();
        let mut expected = one_shot.build_rows().unwrap();

        let mut incremental = ActivityStatsAccumulator::default();
        let mut actual = incremental
            .apply_batch(&[block(0, 0)], std::slice::from_ref(&action))
            .unwrap();
        actual.extend(
            incremental
                .apply_batch(&[block(1, 7_200_000), block(2, 10_800_000)], &[])
                .unwrap(),
        );
        actual.extend(incremental.build_rows().unwrap());

        let sort_rows = |rows: &mut Vec<materialize::MaterializedRow>| {
            rows.sort_by(|a, b| {
                (a.cf_name, a.key.as_slice(), a.value.as_slice()).cmp(&(
                    b.cf_name,
                    b.key.as_slice(),
                    b.value.as_slice(),
                ))
            });
        };
        sort_rows(&mut expected);
        sort_rows(&mut actual);
        assert_eq!(actual, expected);
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
                    parent_hash: [0u8; 32],
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
                    parent_hash: [0u8; 32],
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

        // DailyBlockStats: both blocks on same day (sum of difficulties)
        let block_stats = acc.daily_block_stats.get(&date).expect("daily block stats");
        let expected_difficulty: u64 = ckb_compact_to_difficulty(0x1a08a97e_u32)
            .to_string()
            .parse()
            .unwrap();
        assert_eq!(
            block_stats.0,
            expected_difficulty as i128 * 2,
            "sum_difficulty"
        );
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

        let rows = acc.build_rows(0).unwrap();

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

    #[test]
    fn chain_stats_accumulator_epoch_stats() {
        let mut acc = ChainStatsAccumulator::default();

        let arena = facts::FactsArena {
            blocks: vec![
                facts::BlockFacts {
                    number: 1000,
                    epoch_number: 5,
                    epoch_index: 0,
                    epoch_length: 1800,
                    timestamp_ms: 1_700_000_000_000,
                    transactions_count: 3,
                    ..Default::default()
                },
                facts::BlockFacts {
                    number: 1001,
                    epoch_number: 5,
                    epoch_index: 1,
                    epoch_length: 1800,
                    timestamp_ms: 1_700_000_010_000,
                    transactions_count: 2,
                    ..Default::default()
                },
                facts::BlockFacts {
                    number: 2800,
                    epoch_number: 6,
                    epoch_index: 0,
                    epoch_length: 1750,
                    // 240 minutes (4 hours) after epoch 5 start
                    timestamp_ms: 1_700_000_000_000 + 14_400_000,
                    transactions_count: 1,
                    ..Default::default()
                },
            ],
            txs: vec![],
            cells: vec![],
        };

        let resolved: Vec<facts::ResolvedTxFacts<'_>> = vec![];
        acc.apply_blocks(&arena, &resolved).unwrap();

        // Epoch 5: blocks 1000-1001, tx_count=5, length=1800
        let e5 = acc.epoch_stats.get(&5).expect("epoch 5 stats");
        assert_eq!(e5.0, 1000); // start_block
        assert_eq!(e5.1, 1001); // end_block
        assert_eq!(e5.2, 1800); // length
        assert_eq!(e5.3, 1_700_000_000_000); // start_ts_ms
        assert_eq!(e5.4, 1_700_000_010_000); // end_ts_ms
        assert_eq!(e5.5, 5); // tx_count

        // Epoch 6: single block 2800, tx_count=1, length=1750
        let e6 = acc.epoch_stats.get(&6).expect("epoch 6 stats");
        assert_eq!(e6.0, 2800);
        assert_eq!(e6.1, 2800);
        assert_eq!(e6.2, 1750);
        assert_eq!(e6.5, 1);

        // build_rows should produce EpochStats rows
        let rows = acc.build_rows(0).unwrap();
        let epoch_rows: Vec<_> = rows
            .iter()
            .filter(|r| {
                r.cf_name == CF_STATS_CHAIN && r.key.first() == Some(&keys::stats_prefix::EPOCH)
            })
            .collect();
        assert_eq!(epoch_rows.len(), 2);

        // Deserialize and verify epoch 5
        let stats5: ckbadger_store::types::EpochStats = bincode::deserialize(
            &epoch_rows
                .iter()
                .find(|r| {
                    let epoch_bytes = &r.key[1..];
                    i64::from_be_bytes(epoch_bytes.try_into().unwrap()) == 5
                })
                .unwrap()
                .value,
        )
        .unwrap();
        assert_eq!(stats5.epoch_number, 5);
        assert_eq!(stats5.start_block, 1000);
        assert_eq!(stats5.end_block, Some(1001));
        assert_eq!(stats5.blocks_count, 2);
        assert_eq!(stats5.length, 1800);
        assert_eq!(stats5.transactions_count, 5);
        // Epoch 5 is incomplete (2/1800 blocks), so end_timestamp must be None
        assert!(stats5.end_timestamp.is_none());
    }

    #[test]
    fn chain_stats_complete_epoch_has_end_timestamp() {
        let mut acc = ChainStatsAccumulator::default();

        // Create a complete epoch: 3 blocks with length=3
        let arena = facts::FactsArena {
            blocks: vec![
                facts::BlockFacts {
                    number: 100,
                    epoch_number: 2,
                    epoch_index: 0,
                    epoch_length: 3,
                    timestamp_ms: 1_700_000_000_000,
                    transactions_count: 1,
                    ..Default::default()
                },
                facts::BlockFacts {
                    number: 101,
                    epoch_number: 2,
                    epoch_index: 1,
                    epoch_length: 3,
                    timestamp_ms: 1_700_000_010_000,
                    transactions_count: 1,
                    ..Default::default()
                },
                facts::BlockFacts {
                    number: 102,
                    epoch_number: 2,
                    epoch_index: 2,
                    epoch_length: 3,
                    timestamp_ms: 1_700_000_020_000,
                    transactions_count: 1,
                    ..Default::default()
                },
            ],
            txs: vec![],
            cells: vec![],
        };

        let resolved: Vec<facts::ResolvedTxFacts<'_>> = vec![];
        acc.apply_blocks(&arena, &resolved).unwrap();

        let rows = acc.build_rows(0).unwrap();
        let epoch_rows: Vec<_> = rows
            .iter()
            .filter(|r| {
                r.cf_name == CF_STATS_CHAIN && r.key.first() == Some(&keys::stats_prefix::EPOCH)
            })
            .collect();
        assert_eq!(epoch_rows.len(), 1);

        let stats: ckbadger_store::types::EpochStats =
            bincode::deserialize(&epoch_rows[0].value).unwrap();
        assert_eq!(stats.epoch_number, 2);
        assert_eq!(stats.blocks_count, 3);
        assert_eq!(stats.length, 3);
        // Complete epoch (3/3 blocks) must have end_timestamp
        assert!(stats.end_timestamp.is_some());
    }

    #[test]
    fn chain_stats_epoch_time_dist_rejects_zero_bucket() {
        let mut acc = ChainStatsAccumulator::default();

        // Epoch 5 starts, then epoch 6 starts with the same timestamp → 0-minute bucket
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
                    // Same timestamp → 0-minute duration
                    timestamp_ms: 1_700_000_000_000,
                    ..Default::default()
                },
            ],
            txs: vec![],
            cells: vec![],
        };

        let resolved: Vec<facts::ResolvedTxFacts<'_>> = vec![];
        let result = acc.apply_blocks(&arena, &resolved);
        assert!(result.is_err(), "should reject 0-minute epoch duration");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("invalid bucket_minutes=0"),
            "error should mention invalid bucket_minutes: {}",
            err_msg
        );
    }

    #[test]
    fn build_history_rows_deduplicates_lock_scripts_across_calls() {
        let block = bulk_build_addr_tx_fixture();
        let interner = interner::IdentityInterner::default();
        let (arena, _) = crate::sync::pipeline::build_bulk_facts_arena_from_blocks(
            std::slice::from_ref(&block),
            &interner,
        )
        .expect("facts arena");
        let mut seq = sequencer::BulkSequencer::default();
        let resolved = seq.resolve(&arena).expect("resolved txs");
        let frozen = interner.snapshot_for_reads();

        let root = unique_temp_test_dir("bulk-build-lock-dedup-test");
        std::fs::create_dir_all(&root).expect("create root");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("create domain");
        std::fs::create_dir_all(&append_path).expect("create append");
        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("open domain");
        let append_store = CkbadgerStore::open_append_only(&append_path).expect("open append");

        // First call — should emit lock script rows.
        let result1 = build_history_batches(
            &arena,
            &resolved,
            &frozen,
            &interner,
            true,
            &FxHashMap::default(),
        )
        .expect("first build");

        // Convert rows, commit, and count lock script entries in the store.
        let prepared1 =
            materialize::prepare_flush(&domain_store, &append_store, result1.rows, Vec::new())
                .expect("prepare flush 1");
        domain_store
            .write_batch_no_wal_bulk(prepared1.domain_batch)
            .expect("commit domain");
        append_store
            .write_batch_no_wal_bulk(prepared1.append_batch)
            .expect("commit append");

        let lock_count_after_first: usize = domain_store
            .iterator_cf(domain_store.cf(CF_LOCK_SCRIPTS), IteratorMode::Start)
            .count();
        assert!(
            lock_count_after_first > 0,
            "first call should emit lock script rows"
        );
        let marker_count_after_first = interner.lock_script_written_count();
        assert_eq!(marker_count_after_first, lock_count_after_first);

        // Second call with the same arena/resolved/frozen and interner markers —
        // should emit zero NEW lock script rows since all lock_hash_ids are
        // already marked written.
        let result2 = build_history_batches(
            &arena,
            &resolved,
            &frozen,
            &interner,
            true,
            &FxHashMap::default(),
        )
        .expect("second build");

        // Convert rows and commit second batch.
        let prepared2 =
            materialize::prepare_flush(&domain_store, &append_store, result2.rows, Vec::new())
                .expect("prepare flush 2");
        domain_store
            .write_batch_no_wal_bulk(prepared2.domain_batch)
            .expect("commit domain 2");
        append_store
            .write_batch_no_wal_bulk(prepared2.append_batch)
            .expect("commit append 2");

        // Lock script count should not have grown (same keys, even if overwritten).
        let lock_count_after_second: usize = domain_store
            .iterator_cf(domain_store.cf(CF_LOCK_SCRIPTS), IteratorMode::Start)
            .count();
        assert_eq!(
            lock_count_after_second, lock_count_after_first,
            "second call should not add new lock script entries"
        );
        assert_eq!(
            interner.lock_script_written_count(),
            marker_count_after_first,
            "marker count should not grow"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn build_final_rows_matches_flush_sealed_and_materialize_final() {
        use super::materialize::Materializer;
        use super::owners::BulkReducer;

        let mut runtime = BulkBuildRuntimeState::default();
        let block = bulk_build_addr_tx_fixture();
        runtime
            .apply_blocks_hex(std::slice::from_ref(&block), true, &FxHashMap::default())
            .unwrap();

        let root = super::unique_temp_test_dir("bulk-build-final-rows");
        std::fs::create_dir_all(&root).unwrap();
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).unwrap();
        std::fs::create_dir_all(&append_path).unwrap();
        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("open domain");
        let append_store = CkbadgerStore::open_append_only(&append_path).expect("open append-only");

        // -- AddressOwner --
        let addr_final = runtime.owners.address.build_final_rows().unwrap();
        assert!(addr_final.sealed_rows.is_empty());
        {
            let mut mat = Materializer::new(&domain_store, &append_store);
            runtime.owners.address.materialize_final(&mut mat).unwrap();
            let report = mat.finish();
            assert_eq!(
                addr_final.snapshot_rows.len(),
                report.final_snapshot_rows,
                "address snapshot row count"
            );
        }

        // -- FiberOwner --
        let fiber_final = runtime.owners.fiber.build_final_rows().unwrap();
        assert!(fiber_final.sealed_rows.is_empty());
        {
            let mut mat = Materializer::new(&domain_store, &append_store);
            runtime.owners.fiber.materialize_final(&mut mat).unwrap();
            let report = mat.finish();
            assert_eq!(
                fiber_final.snapshot_rows.len(),
                report.final_snapshot_rows,
                "fiber snapshot row count"
            );
        }

        // -- DaoOwner --
        let dao_final = runtime.owners.dao.build_final_rows().unwrap();
        {
            let mut mat = Materializer::new(&domain_store, &append_store);
            runtime.owners.dao.flush_sealed(&mut mat).unwrap();
            runtime.owners.dao.materialize_final(&mut mat).unwrap();
            let report = mat.finish();
            assert_eq!(
                dao_final.sealed_rows.len(),
                report.sealed_aggregate_rows,
                "dao sealed row count"
            );
            assert_eq!(
                dao_final.snapshot_rows.len(),
                report.final_snapshot_rows,
                "dao snapshot row count"
            );
        }

        // -- ObjectOwner --
        let object_final = runtime.owners.object.build_final_rows().unwrap();
        {
            let mut mat = Materializer::new(&domain_store, &append_store);
            runtime.owners.object.flush_sealed(&mut mat).unwrap();
            runtime.owners.object.materialize_final(&mut mat).unwrap();
            let report = mat.finish();
            assert_eq!(
                object_final.sealed_rows.len(),
                report.sealed_aggregate_rows,
                "object sealed row count"
            );
            assert_eq!(
                object_final.snapshot_rows.len(),
                report.final_snapshot_rows,
                "object snapshot row count"
            );
        }

        // -- ScriptOwner --
        let script_final = runtime
            .owners
            .script
            .build_final_rows(&domain_store)
            .unwrap();
        {
            let mut mat = Materializer::new(&domain_store, &append_store);
            runtime.owners.script.flush_sealed(&mut mat).unwrap();
            runtime.owners.script.materialize_final(&mut mat).unwrap();
            let report = mat.finish();
            assert_eq!(
                script_final.sealed_rows.len(),
                report.sealed_aggregate_rows,
                "script sealed row count"
            );
            assert_eq!(
                script_final.snapshot_rows.len(),
                report.final_snapshot_rows,
                "script snapshot row count"
            );
        }

        // -- TokenOwner (needs domain store) --
        let token_final = runtime
            .owners
            .token
            .build_final_rows(&domain_store)
            .unwrap();
        {
            let mut mat = Materializer::new(&domain_store, &append_store);
            runtime.owners.token.flush_sealed(&mut mat).unwrap();
            runtime.owners.token.materialize_final(&mut mat).unwrap();
            let report = mat.finish();
            assert_eq!(
                token_final.sealed_rows.len(),
                report.sealed_aggregate_rows,
                "token sealed row count"
            );
            assert_eq!(
                token_final.snapshot_rows.len(),
                report.final_snapshot_rows,
                "token snapshot row count"
            );
        }

        // Verify at least some owners produced non-empty rows.
        assert!(
            !addr_final.snapshot_rows.is_empty(),
            "address owner should produce snapshot rows from fixture"
        );
        assert!(
            !script_final.snapshot_rows.is_empty(),
            "script owner should produce snapshot rows from fixture"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_resolved_tx_fee_includes_dao_compensation() {
        // Scenario: DAO withdrawal-completion transaction.
        //
        // Input:  200 CKB DAO WithdrawRequest cell
        //         deposit_ar = 10_000_000_000_000_000 (10^16)
        //         withdraw_ar = 10_100_000_000_000_000 (1.01 * 10^16, i.e. 1% yield)
        //
        // The DAO compensation on a 200 CKB cell:
        //   free_capacity = 200_00000000 - 102_00000000 = 98_00000000
        //   gross = 98_00000000 * 10_100_000_000_000_000 / 10_000_000_000_000_000 = 98_98000000
        //   compensation = 98_98000000 - 98_00000000 = 98000000  (0.98 CKB)
        //
        // Output: plain cell with capacity = input + compensation - miner_fee
        //   miner_fee = 1_000  (1000 shannons)
        //   output_capacity = 200_00000000 + 98000000 - 1_000 = 200_97999000

        let deposit_ar: u64 = 10_000_000_000_000_000;
        let withdraw_ar: u64 = 10_100_000_000_000_000;
        let input_capacity: i64 = 200_00000000;
        let miner_fee: i64 = 1_000;

        let compensation = ckbadger_common::dao::calculate_dao_compensation_from_ar(
            input_capacity,
            deposit_ar,
            withdraw_ar,
        )
        .expect("compensation should be valid");

        let output_capacity = input_capacity + compensation - miner_fee;

        // Build a minimal TxFacts (non-cellbase).
        let tx = facts::TxFacts {
            hash: [0xab; 32],
            block_number: 1000,
            block_hash: [0x01; 32],
            timestamp_ms: 0,
            block_dao_ar: withdraw_ar,
            tx_index: 1,
            is_cellbase: false,
            inputs_count: 1,
            outputs_count: 1,
            tx_size: 100,
            cycles: None,
            dotbit_action: None,
            input_outpoints: vec![],
            output_range: 0..1,
        };

        // Build a minimal ResolvedInputFacts for the WithdrawRequest input.
        let dummy_intern = InternId::new(0);
        let resolved_input = ResolvedInputFacts {
            outpoint: facts::OutPointKey::new([0xcc; 32], 0),
            created_at_block: 900,
            created_by_block_dao_ar: deposit_ar,
            capacity: input_capacity,
            occupied_capacity: 102_00000000,
            udt_amount: None,
            lock_script_hash_id: dummy_intern,
            lock_code_hash_id: dummy_intern,
            lock_hash_type: 1,
            lock_args_id: dummy_intern,
            type_script_hash_id: None,
            type_code_hash_id: None,
            type_hash_type: None,
            type_args_id: None,
            data_size: 0,
            data_hash: None,
            semantic_tag: facts::CellSemanticTag::Dao,
            dao_state: Some(facts::DaoCellState::WithdrawRequest {
                deposit_block_number: 900,
            }),
            dao_compensation_ars: Some(facts::DaoCompensationArs {
                deposit_ar,
                withdraw_request_ar: withdraw_ar,
            }),
            protocol_facts: None,
        };

        // Build a minimal output CellFacts.
        let output_cell = facts::CellFacts {
            outpoint: facts::OutPointKey::new([0xab; 32], 0),
            created_at_block: 1000,
            created_by_block_dao_ar: withdraw_ar,
            capacity: output_capacity,
            lock_script_hash_id: dummy_intern,
            lock_code_hash_id: dummy_intern,
            lock_hash_type: 1,
            lock_args_id: dummy_intern,
            type_script_hash_id: None,
            type_code_hash_id: None,
            type_hash_type: None,
            type_args_id: None,
            occupied_capacity: 61_00000000,
            data_size: 0,
            data: vec![],
            data_hash: None,
            udt_amount: None,
            semantic_tag: facts::CellSemanticTag::Plain,
            dao_state: None,
            protocol_facts: None,
        };

        let resolved_tx = facts::ResolvedTxFacts {
            tx_hash: tx.hash,
            block_number: tx.block_number,
            block_hash: tx.block_hash,
            timestamp_ms: tx.timestamp_ms,
            block_dao_ar: tx.block_dao_ar,
            tx_index: tx.tx_index,
            dotbit_action: None,
            resolved_inputs: vec![resolved_input],
            cells: std::borrow::Cow::Owned(vec![output_cell]),
        };

        let fee = resolved_tx_fee(&tx, &resolved_tx)
            .expect("resolved_tx_fee should succeed for DAO withdrawal-completion");

        assert_eq!(
            fee, miner_fee,
            "fee should equal the actual miner fee ({miner_fee}), not the raw input-output diff"
        );
    }

    #[test]
    fn activity_stats_build_rows_emits_addr_set_entries() {
        let tx_actions = TxActions {
            tx_hash: vec![0x11; 32],
            block_hash: vec![0x22; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1_700_000_000_000, // 2023-11-14 22:13:20 UTC
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ckbadger_store::types::ParticipantDelta {
                lock_hash: vec![0x33; 32],
                ckb_delta: 100_00000000,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            }],
        };

        let mut acc = ActivityStatsAccumulator::default();
        acc.apply_tx_actions(&[tx_actions]).unwrap();

        let rows = acc.build_rows().unwrap();

        let date_key = ckbadger_common::block_date_from_ms(1_700_000_000_000)
            .format("%Y%m%d")
            .to_string();
        let hour_key = ckbadger_common::block_datetime_from_ms(1_700_000_000_000)
            .format("%Y%m%d%H")
            .to_string();

        // Expect: ACTIVITY_DAILY row, ACTIVITY_DAILY_ADDR_SET row,
        //         ACTIVITY_HOURLY row, ACTIVITY_HOURLY_ADDR_SET row
        assert_eq!(
            rows.len(),
            4,
            "expected 4 rows: daily + daily_addr_set + hourly + hourly_addr_set"
        );

        // Verify ACTIVITY_DAILY_ADDR_SET key and value
        let daily_addr_set_key = keys::encode_stats_key(
            keys::stats_prefix::ACTIVITY_DAILY_ADDR_SET,
            date_key.as_bytes(),
        );
        let daily_addr_row = rows
            .iter()
            .find(|r| r.key == daily_addr_set_key)
            .expect("ACTIVITY_DAILY_ADDR_SET row must exist");
        assert_eq!(daily_addr_row.cf_name, CF_STATS_CHAIN);
        // Value is flat sorted 32-byte hashes; 1 participant => 32 bytes
        assert_eq!(daily_addr_row.value.len(), 32);
        assert_eq!(&daily_addr_row.value[..32], &[0x33; 32]);

        // Verify ACTIVITY_HOURLY_ADDR_SET key and value
        let hourly_addr_set_key = keys::encode_stats_key(
            keys::stats_prefix::ACTIVITY_HOURLY_ADDR_SET,
            hour_key.as_bytes(),
        );
        let hourly_addr_row = rows
            .iter()
            .find(|r| r.key == hourly_addr_set_key)
            .expect("ACTIVITY_HOURLY_ADDR_SET row must exist");
        assert_eq!(hourly_addr_row.cf_name, CF_STATS_CHAIN);
        assert_eq!(hourly_addr_row.value.len(), 32);
        assert_eq!(&hourly_addr_row.value[..32], &[0x33; 32]);
    }
}
