//! Test support: drive the real live write path and the production rollback
//! entry point against a domain + append-only store pair, for integration
//! tests (`crates/indexer/tests/rollback_property.rs`). Not an API.
//!
//! The live batch goes through the same steps the pipeline's parser and
//! writer tasks perform — `parse_blocks_parallel`, input resolution against
//! the store, `precompute_live_batch` (the parser's Passes 1–3), then
//! `Indexer::write_parsed_batch` — and the rollback through
//! `BatchWriter::execute_reorg` followed by the pipeline's post-reorg cache
//! and tracker reset.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8};
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use dashmap::DashMap;

use ckbadger_store::{CkbadgerStore, StoreRuntimeConfig};

use crate::config::Config;
use crate::db::writer::cell_distribution::CellDistributionTracker;
use crate::db::writer::hodl_wave::HodlWaveTracker;
use crate::db::{BatchWriter, Repository};
use crate::rpc::{BlockResponseWithCycles, CkbRpcClient};
use crate::runtime_diag::FlightRecorder;

use super::batch::parse_blocks_parallel;
use super::diagnostics::{ParserCellLookupStats, PipelinePerfStats};
use super::helpers::checked_usize_to_i16;
use super::indexer::{decide_startup_sync, Indexer};
use super::pipeline::{precompute_live_batch, LiveBatchPrecompute};
use super::progress::SyncProgress;
use super::shutdown::ShutdownSignal;
use super::types::{BatchWriteMetrics, CachedCellInfo};

/// An `Indexer` over `domain` and `append` that is never started: no RPC, no
/// CKB store reader, no pipeline tasks. Only the write and rollback paths are
/// usable. `startup_tip` / `startup_hash` are the store's tip at construction.
pub fn indexer_with_stores(
    domain: Arc<CkbadgerStore>,
    append: Arc<CkbadgerStore>,
    network: &str,
    startup_tip: i64,
    startup_hash: Option<Vec<u8>>,
) -> Indexer {
    let config = Config {
        domain_data_path: "unused-domain".to_string(),
        append_only_data_path: "unused-append".to_string(),
        bulk_sync_perf_output_root: String::new(),
        build_version: "test".to_string(),
        ckb_rpc_url: "http://127.0.0.1:1".to_string(),
        poll_interval_ms: 1000,
        start_block: None,
        bulk_sync_threshold: 72,
        bulk_memory_budget_gb: None,
        fast_sync_mode: true,
        ckb_db_path: "unused-ckb-db".to_string(),
        metadata_path: None,
        network: network.to_string(),
        force_startup_cleanup: false,
        store_runtime_config: StoreRuntimeConfig::default(),
        decoder_cache_path: "unused-decoder-cache".to_string(),
        dob_decode_dir: "unused-dob-decode".to_string(),
        cycles_request_dir: None,
    };
    let cache_invalidator = crate::cache::CacheInvalidator::new(domain.clone());
    Indexer {
        run_id: "live-test".to_string(),
        config,
        rpc: CkbRpcClient::new("http://127.0.0.1:1"),
        repo: Repository::new(domain.clone()),
        writer: BatchWriter::new(domain.clone(), append.clone()),
        append_only_store: append,
        progress: Arc::new(SyncProgress::new(0, 0)),
        cell_cache: Arc::new(DashMap::<([u8; 32], i16), CachedCellInfo>::new()),
        udt_cell_cache: Arc::new(DashMap::new()),
        perf: super::diagnostics::PerfStats::default(),
        parser_cell_lookup_stats: Arc::new(ParserCellLookupStats::default()),
        pipeline_perf: Arc::new(PipelinePerfStats::default()),
        bulk_build_perf: Arc::new(super::diagnostics::BulkBuildPerfStats::default()),
        adaptive_batch_controller: Arc::new(super::adaptive::LiveBatchController::new()),
        cache_invalidator,
        last_cache_invalidation: tokio::sync::Mutex::new(0),
        was_bulk_sync_active: AtomicBool::new(false),
        bulk_sync_allowed: AtomicBool::new(false),
        startup_decision: decide_startup_sync(0, startup_tip, &startup_hash, 72)
            .expect("live-test startup decision"),
        rebuild_pause_flag: Arc::new(AtomicBool::new(false)),
        pipeline_reset_notify_flag: Arc::new(AtomicBool::new(false)),
        pipeline_reset_reason_code: Arc::new(AtomicU8::new(0)),
        startup_phase: AtomicU8::new(super::helpers::STARTUP_PHASE_NONE),
        pipeline_reset_epoch: Arc::new(AtomicU64::new(0)),
        incident_seq: AtomicU64::new(0),
        flight_recorder: FlightRecorder::new(super::diagnostics::FLIGHT_RECORDER_CAPACITY),
        repeated_warning_tracker: super::diagnostics::RepeatedWarningTracker::default(),
        incident_dir: PathBuf::from("unused-incidents"),
        bulk_sync_perf_run: std::sync::Mutex::new(None),
        shutdown: ShutdownSignal::default(),
        unrecoverable_exit: Arc::new(AtomicBool::new(false)),
        ckb_store: None,
        hodl_tracker: std::sync::Mutex::new(HodlWaveTracker::new()),
        cell_dist_tracker: std::sync::Mutex::new(CellDistributionTracker::new()),
        hourly_retention_requested: Arc::new(AtomicBool::new(false)),
    }
}

/// The label import every start runs before it builds the indexer
/// (`entry.rs`), with the bundled metadata; an entry error fails the test
/// where startup would only log it.
pub fn startup_label_import(domain: &CkbadgerStore, network: &str) -> Result<()> {
    let result = crate::label_import::run_label_import(
        domain,
        &ckbadger_common::LabelImportConfig {
            metadata_path: None,
            network: network.to_string(),
        },
    )?;
    if !result.errors.is_empty() {
        bail!("startup label import errors: {:?}", result.errors);
    }
    Ok(())
}

/// The indexer a restart builds over `domain` and `append`: the startup
/// label import, then a new `Indexer` (empty caches, trackers not loaded) at
/// the store's tip after the store-side startup steps every run performs
/// before its first batch (`Indexer::prepare_store_for_sync`, live mode, no
/// forced cleanup).
pub fn restart(
    domain: Arc<CkbadgerStore>,
    append: Arc<CkbadgerStore>,
    network: &str,
) -> Result<Indexer> {
    startup_label_import(&domain, network)?;
    let (tip, hash) = match domain.get_sync_tip_block()? {
        Some((number, header)) => (number, Some(header.hash)),
        None => (-1, None),
    };
    let indexer = indexer_with_stores(domain, append, network, tip, hash);
    indexer.prepare_store_for_sync(tip, false)?;
    Ok(indexer)
}

/// Write `blocks` as ONE live batch through the real parser precompute and
/// `write_parsed_batch`. Every input resolves from this batch or from the
/// store (never from a parser cache), so a batch after a rollback sees exactly
/// what the store holds.
pub async fn write_live_batch(
    indexer: &Indexer,
    blocks: Vec<BlockResponseWithCycles>,
) -> Result<()> {
    let (parsed_blocks, mut all_tx_data, all_input_outpoints) = parse_blocks_parallel(&blocks)?;
    let (Some(first), Some(last)) = (parsed_blocks.first(), parsed_blocks.last()) else {
        bail!("live test batch has no blocks");
    };
    let start_block = u64::try_from(first.number)?;
    let end_block = u64::try_from(last.number)?;

    let mut batch_outputs: HashSet<(Vec<u8>, i16)> = HashSet::new();
    for tx in &all_tx_data {
        for index in 0..tx.cells.len() {
            batch_outputs.insert((
                tx.hash.to_vec(),
                checked_usize_to_i16(index, "live test batch output index")?,
            ));
        }
    }
    let from_store: Vec<(&[u8], i16)> = all_input_outpoints
        .iter()
        .filter(|outpoint| !batch_outputs.contains(*outpoint))
        .map(|(hash, index)| (hash.as_slice(), *index))
        .collect();
    let input_cell_info = indexer
        .writer
        .get_full_cells_info_batch_chunk(&from_store)?;
    for (hash, index) in &from_store {
        if !input_cell_info.contains_key(&(hash.to_vec(), *index)) {
            bail!(
                "live test batch {}-{}: input 0x{}:{} resolves neither in the batch nor in the store",
                start_block,
                end_block,
                hex::encode(hash),
                index
            );
        }
    }

    let LiveBatchPrecompute {
        input_script_hashes,
        batch_cell_infos,
        address_balance_changes,
        script_usage_changes,
        script_reference_usage_changes,
        script_daily_changes,
        token_daily_changes,
        spore_type_index_changes,
        spore_daily_changes,
        cluster_daily_changes,
        object_type_index_changes,
        object_daily_changes,
        ..
    } = precompute_live_batch(
        indexer.writer.store(),
        &DashMap::new(),
        &mut all_tx_data,
        &input_cell_info,
        start_block,
        end_block,
    )?;

    indexer
        .write_parsed_batch(
            &blocks,
            &parsed_blocks,
            all_tx_data,
            input_cell_info,
            batch_cell_infos,
            input_script_hashes,
            address_balance_changes,
            script_usage_changes,
            script_reference_usage_changes,
            script_daily_changes,
            token_daily_changes,
            spore_type_index_changes,
            spore_daily_changes,
            cluster_daily_changes,
            object_type_index_changes,
            object_daily_changes,
            end_block,
        )
        .await
        .map(|_metrics: BatchWriteMetrics| ())
}

/// Roll the store back to `fork_point` through the live reorg entry point
/// (`BatchWriter::execute_reorg`), then reset the in-memory caches and
/// trackers exactly as the pipeline does after `ReorgAction::Handled`.
pub async fn reorg_to(indexer: &Indexer, fork_point: i64) -> Result<()> {
    let store = indexer.writer.store();
    let status = store.get_sync_status()?;
    let old_tip = status.tip_block_number;
    if fork_point >= old_tip {
        bail!("reorg_to: fork point {fork_point} is not below the tip {old_tip}");
    }
    let fork_hash = store
        .get_block_header(fork_point)?
        .ok_or_else(|| anyhow!("reorg_to: no header at fork point {fork_point}"))?
        .hash;
    indexer
        .writer
        .execute_reorg(
            fork_point,
            &fork_hash,
            old_tip,
            &status.tip_block_hash,
            fork_point,
            &fork_hash,
        )
        .await?;
    indexer.cell_cache.clear();
    indexer.udt_cell_cache.clear();
    let (reorg_tip, _) = indexer.repo.get_sync_tip().await?;
    indexer.reconcile_hodl_tracker_with_tip(reorg_tip)?;
    indexer.reconcile_cell_dist_tracker_with_tip(reorg_tip)?;
    Ok(())
}
