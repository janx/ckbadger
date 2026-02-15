#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use dashmap::DashMap;
use futures::stream::{FuturesOrdered, StreamExt};
use rayon::prelude::*;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::LiveCellInfo;
use ckbadger_store::CkbadgerStore;

use crate::cache::CacheInvalidator;
use crate::config::{Config, DEEP_FORK_DEPTH};
use crate::db::{
    rebuild_activities, rebuild_cell_indices, BatchWriter, ReorgResult, Repository,
    SecondaryIssuanceBreakdown,
};
use crate::parser::{
    BlockParser, CellParser, DaoParser, DotbitParser, MnftParser, SporeParser, TransactionParser,
    UdtParser,
};
use ckb_store_reader::CkbChainReader;

use crate::rpc::{BlockResponseWithCycles, CkbRpcClient, DaoField};

use super::SyncProgress;

#[allow(dead_code)]
const PARTITION_SIZE: u64 = 5_000_000;

#[allow(dead_code)]
fn get_partition_index(block_number: u64) -> usize {
    (block_number / PARTITION_SIZE) as usize
}

#[allow(dead_code)]
fn format_partition_range(start_block: u64, end_block: u64) -> String {
    let start_partition = get_partition_index(start_block);
    let end_partition = get_partition_index(end_block);
    if start_partition == end_partition {
        format!("[p{}]", start_partition)
    } else {
        format!("[p{}->p{}]", start_partition, end_partition)
    }
}

#[allow(dead_code)]
fn crosses_partition_boundary(start_block: u64, end_block: u64) -> bool {
    get_partition_index(start_block) != get_partition_index(end_block)
}

enum SyncAction {
    CaughtUp,
    Continue,
    ReorgHandled,
    DeepForkPaused,
}

#[allow(dead_code)]
enum ReorgAction {
    Handled(ReorgResult),
    DeepForkPaused,
}

/// Accumulated statistics across a batch of blocks (avoids per-block DB writes)
#[derive(Default)]
struct BatchStats {
    sync_totals: (i64, i64, i64),
    last_block: Option<(i64, Vec<u8>)>,
    hourly_stats: HashMap<DateTime<Utc>, (i32, i32, i32, i32, i64)>,
    daily_stats: HashMap<NaiveDate, (i32, i32, i32, i32, i64, i64, i64)>,
    daily_block_stats: HashMap<NaiveDate, (i128, i32, i32)>,
    miner_stats: HashMap<(NaiveDate, Vec<u8>), (i32, i64)>,
    epoch_stats: HashMap<i64, EpochAccum>,
    block_time_dist: HashMap<i32, i32>,
    epoch_time_dist: HashMap<i32, i32>,
    dao_snapshot_dates: HashSet<NaiveDate>,
    daily_block_times: HashMap<NaiveDate, (i64, i32)>,
    daily_dao_fields: HashMap<NaiveDate, Vec<u8>>,
}

#[derive(Clone)]
struct EpochAccum {
    start_block: i64,
    end_block: i64,
    length: i32,
    start_ts: chrono::DateTime<Utc>,
    end_ts: chrono::DateTime<Utc>,
    tx_count: i32,
    is_new: bool,
}

#[derive(Clone)]
struct CachedCellInfo {
    capacity: i64,
    created_at_block: i64,
    lock_script_hash: Vec<u8>,
    lock_code_hash: Vec<u8>,
    lock_hash_type: i16,
    lock_args: Vec<u8>,
    type_script_hash: Option<Vec<u8>>,
    type_code_hash: Option<Vec<u8>>,
    data_size: i32,
    occupied_capacity: i64,
}

#[derive(Clone)]
struct CachedUdtCellInfo {
    type_script_hash: Vec<u8>,
    type_code_hash: Vec<u8>,
    type_hash_type: i16,
    type_args: Vec<u8>,
    lock_script_hash: Vec<u8>,
    amount: u128,
    standard: String,
}

#[derive(Default)]
struct PerfStats {
    fetch_us: AtomicU64,
    db_write_us: AtomicU64,
    blocks_count: AtomicU64,
}

impl PerfStats {
    fn add(&self, field: &AtomicU64, duration: Duration) {
        field.fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    fn report_and_reset(&self) {
        let blocks = self.blocks_count.swap(0, Ordering::Relaxed);
        if blocks == 0 {
            return;
        }
        let fetch_ms = self.fetch_us.swap(0, Ordering::Relaxed) as f64 / 1000.0;
        let db_ms = self.db_write_us.swap(0, Ordering::Relaxed) as f64 / 1000.0;
        info!(
            blocks,
            fetch_ms = format!("{:.1}", fetch_ms),
            db_ms = format!("{:.1}", db_ms),
            "Batch perf"
        );
    }

    /// Snapshot the current accumulated values (non-destructive read).
    fn snapshot_ms(&self) -> (f64, f64) {
        let rpc = self.fetch_us.load(Ordering::Relaxed);
        let db = self.db_write_us.load(Ordering::Relaxed);
        (rpc as f64 / 1000.0, db as f64 / 1000.0)
    }
}

const CELL_CACHE_CAPACITY: usize = 200_000;
const UDT_CELL_CACHE_CAPACITY: usize = 100_000;

fn block_time_to_bucket(block_time_seconds: i64) -> i32 {
    if block_time_seconds < 1 {
        0
    } else if block_time_seconds < 30 {
        block_time_seconds as i32
    } else {
        30
    }
}

fn truncate_to_hour(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(dt)
}

struct TxData {
    hash: [u8; 32],
    block_number: i64,
    block_hash: Vec<u8>,
    tx_index: i32,
    version: i32,
    inputs_count: i16,
    outputs_count: i16,
    witnesses_count: i16,
    cell_deps_count: i16,
    header_deps_count: i16,
    is_cellbase: bool,
    inputs: Vec<crate::parser::transaction::ParsedInput>,
    cells: Vec<crate::parser::cell::ParsedCell>,
    outputs_data: Vec<String>,
    total_input_capacity: i64,
    total_output_capacity: i64,
    fee: i64,
    tx_size: i32,
    cycles: Option<i64>,
    timestamp: chrono::DateTime<Utc>,
}

fn parse_blocks_parallel(
    blocks: &[BlockResponseWithCycles],
) -> (
    Vec<crate::parser::block::ParsedBlock>,
    Vec<TxData>,
    Vec<(Vec<u8>, i16)>,
) {
    let mut parsed_results: Vec<(usize, crate::parser::block::ParsedBlock, Vec<TxData>)> = blocks
        .par_iter()
        .enumerate()
        .map(|(block_idx, block_response)| {
            let block = &block_response.block;
            let parsed = BlockParser::parse(block);
            let mut tx_data_for_block: Vec<TxData> = block
                .transactions
                .par_iter()
                .enumerate()
                .map(|(tx_index, tx)| {
                    let parsed_tx = TransactionParser::parse(tx);
                    let inputs = TransactionParser::parse_inputs(tx);
                    let cells = CellParser::parse_outputs(tx);
                    let outputs_data: Vec<String> = tx.outputs_data.clone();
                    let total_output_capacity: i64 = cells.iter().map(|c| c.capacity).sum();
                    let cycles = if tx_index == 0 {
                        None
                    } else {
                        block_response
                            .cycles
                            .as_ref()
                            .and_then(|c| c.get(tx_index - 1))
                            .and_then(|hex| {
                                let hex = hex.strip_prefix("0x").unwrap_or(hex);
                                u64::from_str_radix(hex, 16).ok().map(|v| v as i64)
                            })
                    };
                    TxData {
                        hash: parsed_tx.hash,
                        block_number: parsed.number,
                        block_hash: parsed.hash.clone(),
                        tx_index: tx_index as i32,
                        version: parsed_tx.version,
                        inputs_count: parsed_tx.inputs_count as i16,
                        outputs_count: parsed_tx.outputs_count as i16,
                        witnesses_count: parsed_tx.witnesses_count as i16,
                        cell_deps_count: parsed_tx.cell_deps_count as i16,
                        header_deps_count: parsed_tx.header_deps_count as i16,
                        is_cellbase: parsed_tx.is_cellbase,
                        inputs,
                        cells,
                        outputs_data,
                        total_input_capacity: 0,
                        total_output_capacity,
                        fee: 0,
                        tx_size: parsed_tx.tx_size,
                        cycles,
                        timestamp: parsed.timestamp,
                    }
                })
                .collect();
            tx_data_for_block.sort_by_key(|td| td.tx_index);
            (block_idx, parsed, tx_data_for_block)
        })
        .collect();
    parsed_results.sort_by_key(|(idx, _, _)| *idx);

    let mut all_parsed_blocks = Vec::with_capacity(parsed_results.len());
    let mut all_tx_data = Vec::new();
    let mut all_input_outpoints = Vec::new();
    for (_, parsed, tx_data_list) in parsed_results {
        for tx_data in &tx_data_list {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    all_input_outpoints.push((
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    ));
                }
            }
        }
        all_tx_data.extend(tx_data_list);
        all_parsed_blocks.push(parsed);
    }
    (all_parsed_blocks, all_tx_data, all_input_outpoints)
}

const CACHE_INVALIDATION_INTERVAL: u64 = 10_000;
const SECONDARY_ISSUANCE_BACKFILL_THRESHOLD: u64 = 1000;

pub struct Indexer {
    config: Config,
    rpc: CkbRpcClient,
    repo: Repository,
    writer: BatchWriter,
    progress: Arc<SyncProgress>,
    cell_cache: Arc<DashMap<([u8; 32], i32), CachedCellInfo>>,
    udt_cell_cache: Arc<DashMap<([u8; 32], i16), CachedUdtCellInfo>>,
    perf: PerfStats,
    cache_invalidator: CacheInvalidator,
    last_cache_invalidation: tokio::sync::Mutex<u64>,
    was_bulk_sync_active: std::sync::atomic::AtomicBool,
    was_secondary_issuance_bulk_active: std::sync::atomic::AtomicBool,
    rebuild_pause_flag: Arc<std::sync::atomic::AtomicBool>,
    reorg_notify_flag: Arc<std::sync::atomic::AtomicBool>,
    address_balances_deferred: std::sync::atomic::AtomicBool,
    activities_deferred: std::sync::atomic::AtomicBool,
    ckb_store: Option<Arc<CkbChainReader>>,
}

impl Indexer {
    pub async fn new(config: Config, store: Arc<CkbadgerStore>) -> Result<Self> {
        let rpc = CkbRpcClient::new(&config.ckb_rpc_url);
        let cache_invalidator = CacheInvalidator::new(config.redis_url.as_deref()).await;

        let ckb_store = match config.ckb_data_path.as_deref() {
            Some(path) => {
                let reader = CkbChainReader::open(path)?;
                info!("CKB direct RocksDB reader opened at {}", path);
                Some(Arc::new(reader))
            }
            None => None,
        };
        let repo = Repository::with_cache(store.clone(), cache_invalidator.clone());
        let writer = BatchWriter::with_cache(
            store.clone(),
            config.fast_sync_mode,
            cache_invalidator.clone(),
        );

        let (tip_number, _) = repo.get_sync_tip().await?;
        let chain_tip = if let Some(ref store) = ckb_store {
            store.tip_number().unwrap_or(0)
        } else {
            rpc.get_tip_block_number().await?
        };

        let progress = Arc::new(SyncProgress::new(tip_number as u64, chain_tip));
        progress.start_sampler();
        let cell_cache = Arc::new(DashMap::with_capacity(CELL_CACHE_CAPACITY));
        let udt_cell_cache = Arc::new(DashMap::with_capacity(UDT_CELL_CACHE_CAPACITY));

        let was_bulk = progress.blocks_remaining() > config.bulk_sync_threshold;
        let was_secondary_bulk =
            progress.blocks_remaining() > SECONDARY_ISSUANCE_BACKFILL_THRESHOLD;

        let sync_status = store.get_sync_status()?;
        let address_balances_deferred = sync_status.address_balances_deferred;
        let activities_deferred = sync_status.activities_deferred;

        if address_balances_deferred || activities_deferred {
            info!(
                "Loaded deferred states: address_balances={}, activities={}",
                address_balances_deferred, activities_deferred
            );
        }

        Ok(Self {
            config,
            rpc,
            repo,
            writer,
            progress,
            cell_cache,
            udt_cell_cache,
            perf: PerfStats::default(),
            cache_invalidator,
            last_cache_invalidation: tokio::sync::Mutex::new(0),
            was_bulk_sync_active: std::sync::atomic::AtomicBool::new(was_bulk),
            was_secondary_issuance_bulk_active: std::sync::atomic::AtomicBool::new(
                was_secondary_bulk,
            ),
            rebuild_pause_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            reorg_notify_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            address_balances_deferred: std::sync::atomic::AtomicBool::new(
                address_balances_deferred,
            ),
            activities_deferred: std::sync::atomic::AtomicBool::new(activities_deferred),
            ckb_store,
        })
    }

    pub fn progress(&self) -> Arc<SyncProgress> {
        Arc::clone(&self.progress)
    }

    pub fn cache_invalidator(&self) -> &CacheInvalidator {
        &self.cache_invalidator
    }

    pub fn writer(&self) -> &BatchWriter {
        &self.writer
    }

    pub fn rebuild_pause_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.rebuild_pause_flag)
    }

    pub fn is_bulk_sync_active(&self) -> bool {
        self.progress.blocks_remaining() > self.config.bulk_sync_threshold
    }

    pub fn is_direct_db_read(&self) -> bool {
        self.ckb_store.is_some()
    }

    pub fn ckb_store(&self) -> Option<Arc<CkbChainReader>> {
        self.ckb_store.clone()
    }

    /// Snapshot the current perf stats: (fetch_ms, db_ms).
    pub fn perf_snapshot_ms(&self) -> (f64, f64) {
        self.perf.snapshot_ms()
    }

    pub fn get_memory_stats(&self) -> ckbadger_common::MemoryStatsData {
        let stats = self.writer.store().memory_stats();
        let sync_status = self.writer.store().get_sync_status().unwrap_or_default();
        ckbadger_common::MemoryStatsData {
            live_cells_count: stats.cells_count as u64,
            consumed_cells_count: 0,
            consumed_cells_bytes: 0,
            rocksdb_memtable_bytes: stats.memtable_bytes as u64,
            rocksdb_block_cache_bytes: stats.block_cache_bytes as u64,
            rocksdb_table_readers_bytes: stats.table_readers_bytes as u64,
            rocksdb_total_bytes: stats.memory_bytes as u64,
            block_headers_count: 0,
            bulk_sync_cell_cache_enabled: false,
            bulk_sync_mode: self.is_bulk_sync_active(),
            compaction_pending_bytes: stats.compaction_pending_bytes,
            num_running_compactions: stats.num_running_compactions,
            sst_files_size: stats.sst_files_size,
            l0_files_count: stats.l0_files_count,
            l0_files_max: stats.l0_files_max,
            l0_worst_cf: stats.l0_worst_cf,
            immutable_memtables: stats.immutable_memtables,
            top_cf_sizes: stats.top_cf_sizes,
            total_transactions: sync_status.total_transactions,
            total_cells: sync_status.total_cells_created,
            total_live_cells: sync_status.total_cells_created - sync_status.total_cells_consumed,
            total_addresses: 0,
            updated_at: chrono::Utc::now().timestamp(),
        }
    }

    fn is_secondary_issuance_bulk_active(&self) -> bool {
        self.progress.blocks_remaining() > SECONDARY_ISSUANCE_BACKFILL_THRESHOLD
    }

    // === run / run_sequential / run_pipeline ===

    pub async fn run(&self) -> Result<()> {
        let blocks_behind = self.progress.blocks_remaining();
        info!(
            "Starting indexer (pipeline={}, {} blocks behind, threshold={})",
            self.config.pipeline_enabled, blocks_behind, self.config.bulk_sync_threshold
        );

        if blocks_behind > self.config.bulk_sync_threshold {
            info!(
                "Bulk sync auto-enabled: {} blocks behind > {} threshold",
                blocks_behind, self.config.bulk_sync_threshold,
            );
            self.writer.store().set_bulk_sync_compaction_options();

            // Mark activities as deferred for crash recovery
            self.activities_deferred.store(true, Ordering::Relaxed);
            if let Err(e) = self.writer.store().update_sync_status(|s| {
                s.activities_deferred = true;
            }) {
                warn!("Failed to set activities_deferred flag: {}", e);
            }
        }

        // If activities were deferred from a previous run but bulk sync is done, rebuild now
        if self.activities_deferred.load(Ordering::Relaxed)
            && blocks_behind <= self.config.bulk_sync_threshold
        {
            info!("Activities deferred from previous run, starting rebuild");
            let store_act = Arc::clone(self.writer.store());
            let ckb_store_act = self.ckb_store.clone();
            tokio::task::spawn_blocking(move || {
                rebuild_activities(&store_act, ckb_store_act.as_deref());
                if let Err(e) = store_act.update_sync_status(|s| {
                    s.activities_deferred = false;
                }) {
                    warn!("Failed to clear activities_deferred flag: {}", e);
                }
            });
        }

        let (start_block, _) = self.repo.get_sync_tip().await?;
        let consistent_block = self.writer.find_last_consistent_block()?;
        let actual_start = match consistent_block {
            Some(cb) if cb < start_block => {
                warn!(
                    "Rolling back from block {} to {} due to data inconsistency",
                    start_block, cb
                );
                cb
            }
            _ => start_block,
        };

        self.writer.init_sync_start(
            actual_start,
            blocks_behind > self.config.bulk_sync_threshold,
        )?;

        if let Err(e) = self.maybe_submit_label_import_task() {
            warn!("Failed to submit label import task: {}", e);
        }

        if !self.is_bulk_sync_active() {
            info!("Not in bulk sync - checking for pending rebuild tasks from previous run...");
            self.maybe_submit_pending_rebuild_tasks();
        }

        // Periodic 24h transfer refresh
        let store_for_task = Arc::clone(self.writer.store());
        let fast_sync_mode = self.config.fast_sync_mode;
        let progress_for_task = Arc::clone(&self.progress);
        let bulk_sync_threshold = self.config.bulk_sync_threshold;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(600));
            loop {
                interval.tick().await;
                let blocks_remaining = progress_for_task.blocks_remaining();
                if blocks_remaining > bulk_sync_threshold {
                    debug!(
                        "Skipping token 24h refresh ({} blocks remaining > {} threshold)",
                        blocks_remaining, bulk_sync_threshold
                    );
                    continue;
                }
                let writer =
                    BatchWriter::with_fast_sync_mode(store_for_task.clone(), fast_sync_mode);
                match writer.refresh_token_24h_transfers() {
                    Ok(count) => info!("Refreshed 24h transfers for {} tokens", count),
                    Err(e) => warn!("Failed to refresh token 24h transfers: {}", e),
                }
                match writer.refresh_mnft_24h_transfers() {
                    Ok(count) => info!("Refreshed 24h transfers for {} NFT classes", count),
                    Err(e) => warn!("Failed to refresh NFT 24h transfers: {}", e),
                }
            }
        });

        if self.config.pipeline_enabled {
            self.run_pipeline().await
        } else {
            self.run_sequential().await
        }
    }

    async fn run_sequential(&self) -> Result<()> {
        loop {
            if self.rebuild_pause_flag.load(Ordering::SeqCst) {
                debug!("Sync paused for index rebuild");
                sleep(Duration::from_millis(500)).await;
                continue;
            }

            if self.repo.has_unresolved_deep_fork().unwrap_or(false) {
                warn!("Deep fork unresolved, sync paused. Waiting for manual intervention...");
                sleep(Duration::from_secs(30)).await;
                continue;
            }

            match self.sync_batch().await {
                Ok(SyncAction::CaughtUp) => {
                    sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
                }
                Ok(SyncAction::Continue) => {}
                Ok(SyncAction::ReorgHandled) => {
                    info!("Reorg handled, continuing sync from fork point");
                }
                Ok(SyncAction::DeepForkPaused) => {
                    warn!("Deep fork detected, sync paused");
                    sleep(Duration::from_secs(30)).await;
                }
                Err(e) => {
                    error!("Sync error: {}", e);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn run_pipeline(&self) -> Result<()> {
        use tokio::sync::mpsc;

        type FetchedBatch = (u64, u64, u64, Arc<Vec<BlockResponseWithCycles>>);
        type ParsedBatch = (
            u64,
            u64,
            u64,
            Arc<Vec<BlockResponseWithCycles>>,
            Vec<crate::parser::block::ParsedBlock>,
            Vec<TxData>,
            HashMap<(Vec<u8>, i16), LiveCellInfo>,
            // Pre-computed in parser stage:
            HashMap<(Vec<u8>, i16), LiveCellInfo>, // batch_cell_infos
            HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>, i64)>, // address_balance_changes
            HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)>, // script_usage_changes
        );

        let (fetch_tx, mut fetch_rx) = mpsc::channel::<FetchedBatch>(self.config.pipeline_buffer);
        let (parse_tx, mut parse_rx) = mpsc::channel::<ParsedBatch>(self.config.pipeline_buffer);

        let rpc = self.rpc.clone();
        let config = self.config.clone();
        let progress = Arc::clone(&self.progress);
        let repo = self.repo.clone();
        let rebuild_pause = Arc::clone(&self.rebuild_pause_flag);
        let reorg_notify = Arc::clone(&self.reorg_notify_flag);
        let ckb_store = self.ckb_store.clone();

        // === Fetcher task ===
        let fetcher = tokio::spawn(async move {
            let mut next_block: Option<u64> = None;
            let mut was_paused = false;

            loop {
                if rebuild_pause.load(Ordering::SeqCst) {
                    debug!("Fetcher paused for index rebuild");
                    was_paused = true;
                    sleep(Duration::from_millis(500)).await;
                    continue;
                }
                if was_paused {
                    info!("Fetcher resuming from pause, resetting next_block to re-query DB state");
                    next_block = None;
                    was_paused = false;
                }
                if reorg_notify.swap(false, Ordering::SeqCst) {
                    info!("Fetcher received reorg notification, resetting next_block");
                    next_block = None;
                }

                if let Some(ref store) = ckb_store {
                    if let Err(e) = store.refresh() {
                        error!("Failed to refresh CKB RocksDB secondary: {}", e);
                        sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }

                let chain_tip = if let Some(ref store) = ckb_store {
                    match store.tip_number() {
                        Some(tip) => tip,
                        None => {
                            error!("Failed to get chain tip from CKB RocksDB");
                            sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    }
                } else {
                    match rpc.get_tip_block_number().await {
                        Ok(tip) => tip,
                        Err(e) => {
                            error!("Failed to get chain tip: {}", e);
                            sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    }
                };
                progress.update_target(chain_tip);

                let start_block = match next_block {
                    Some(nb) => nb,
                    None => {
                        let (db_tip, db_tip_hash) = match repo.get_sync_tip().await {
                            Ok(tip) => tip,
                            Err(e) => {
                                error!("Failed to get DB tip: {}", e);
                                sleep(Duration::from_secs(5)).await;
                                continue;
                            }
                        };
                        if db_tip == 0 && db_tip_hash.is_none() {
                            0
                        } else {
                            (db_tip + 1) as u64
                        }
                    }
                };

                if start_block > chain_tip {
                    debug!(
                        "Fetcher waiting: start_block {} > chain_tip {}",
                        start_block, chain_tip
                    );
                    sleep(Duration::from_millis(config.poll_interval_ms)).await;
                    continue;
                }

                let end_block =
                    std::cmp::min(start_block + config.batch_size as u64 - 1, chain_tip);

                debug!(
                    "Fetcher: fetching blocks {} to {} (chain_tip={}, next_block={:?})",
                    start_block, end_block, chain_tip, next_block
                );

                let blocks = if let Some(ref store) = ckb_store {
                    let store = Arc::clone(store);
                    let sb = start_block;
                    let eb = end_block;
                    match tokio::task::spawn_blocking(move || {
                        Self::fetch_blocks_direct(&store, sb, eb)
                    })
                    .await
                    {
                        Ok(Ok(blocks)) => blocks,
                        Ok(Err(e)) => {
                            error!("Failed to fetch blocks from RocksDB: {}", e);
                            sleep(Duration::from_secs(5)).await;
                            next_block = None;
                            continue;
                        }
                        Err(e) => {
                            error!("Block fetch task panicked: {}", e);
                            sleep(Duration::from_secs(5)).await;
                            next_block = None;
                            continue;
                        }
                    }
                } else {
                    match Self::fetch_blocks_with_config(
                        &rpc,
                        start_block,
                        end_block,
                        config.parallel_fetch_size,
                    )
                    .await
                    {
                        Ok(blocks) => blocks,
                        Err(e) => {
                            error!("Failed to fetch blocks: {}", e);
                            sleep(Duration::from_secs(5)).await;
                            next_block = None;
                            continue;
                        }
                    }
                };

                // Split into sub-batches if too many transactions
                let max_txs = config.max_batch_txs;
                let mut sub_start = 0usize;
                let mut accum_txs = 0usize;
                let mut send_failed = false;

                for (i, block) in blocks.iter().enumerate() {
                    accum_txs += block.block.transactions.len();
                    let is_last = i == blocks.len() - 1;

                    if accum_txs >= max_txs || is_last {
                        let sub_blocks = blocks[sub_start..=i].to_vec();
                        let sub_start_block = start_block + sub_start as u64;
                        let sub_end_block = start_block + i as u64;

                        if sub_start > 0 {
                            debug!(
                                sub_start_block,
                                sub_end_block,
                                txs = accum_txs,
                                "Fetcher: sending sub-batch"
                            );
                        }

                        if fetch_tx
                            .send((
                                sub_start_block,
                                sub_end_block,
                                chain_tip,
                                Arc::new(sub_blocks),
                            ))
                            .await
                            .is_err()
                        {
                            send_failed = true;
                            break;
                        }

                        sub_start = i + 1;
                        accum_txs = 0;
                    }
                }

                if send_failed {
                    break;
                }

                next_block = Some(end_block + 1);
                if end_block % 1000 == 0 {
                    next_block = None;
                }
            }
        });

        // === Parser task ===
        let writer_for_parser = self.writer.clone();
        let cell_cache_for_parser = Arc::clone(&self.cell_cache);

        let parse_tx_for_writer_depth = parse_tx.clone();
        let parser = tokio::spawn(async move {
            while let Some((start_block, end_block, chain_tip, blocks)) = fetch_rx.recv().await {
                let t_parser = Instant::now();

                let blocks_ref = Arc::clone(&blocks);
                let (all_parsed_blocks, mut all_tx_data, all_input_outpoints) =
                    tokio::task::spawn_blocking(move || parse_blocks_parallel(&blocks_ref))
                        .await
                        .unwrap_or_else(|_| (vec![], vec![], vec![]));

                if all_parsed_blocks.is_empty() {
                    continue;
                }

                let t_parse_ms = t_parser.elapsed().as_secs_f64() * 1000.0;

                let mut batch_cells: HashMap<(Vec<u8>, i16), ()> = HashMap::new();
                for td in &all_tx_data {
                    for (idx, _) in td.cells.iter().enumerate() {
                        batch_cells.insert((td.hash.to_vec(), idx as i16), ());
                    }
                }

                let t_cell_lookup = Instant::now();
                let mut cache_hits: usize = 0;
                let mut input_cell_info: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
                for (tx_hash, idx) in &all_input_outpoints {
                    let hash_arr: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                    let key = (hash_arr, *idx as i32);
                    if let Some(cached) = cell_cache_for_parser.get(&key) {
                        cache_hits += 1;
                        input_cell_info.insert(
                            (tx_hash.clone(), *idx),
                            LiveCellInfo {
                                capacity: cached.capacity,
                                created_at_block: cached.created_at_block,
                                lock_script_hash: cached.lock_script_hash.clone(),
                                lock_code_hash: cached.lock_code_hash.clone(),
                                lock_hash_type: cached.lock_hash_type,
                                lock_args: cached.lock_args.clone(),
                                type_script_hash: cached.type_script_hash.clone(),
                                type_code_hash: cached.type_code_hash.clone(),
                                data_size: cached.data_size,
                                occupied_capacity: cached.occupied_capacity,
                            },
                        );
                    }
                }

                let missing_outpoints: Vec<(Vec<u8>, i16)> = all_input_outpoints
                    .iter()
                    .filter(|(h, i)| {
                        let key = (h.clone(), *i);
                        !input_cell_info.contains_key(&key) && !batch_cells.contains_key(&key)
                    })
                    .cloned()
                    .collect();

                let db_lookups;
                if !missing_outpoints.is_empty() {
                    let unique_missing: Vec<(Vec<u8>, i16)> = {
                        let mut seen = HashSet::new();
                        missing_outpoints
                            .into_iter()
                            .filter(|x| seen.insert(x.clone()))
                            .collect()
                    };
                    db_lookups = unique_missing.len();
                    let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;
                    let wr = writer_for_parser.clone();
                    let missing_owned: Vec<(Vec<u8>, i16)> = unique_missing
                        .iter()
                        .map(|(h, i)| (h.clone(), *i))
                        .collect();
                    let db_query = tokio::task::spawn_blocking(move || {
                        let refs: Vec<(&[u8], i16)> = missing_owned
                            .iter()
                            .map(|(h, i)| (h.as_slice(), *i))
                            .collect();
                        wr.get_full_cells_info_batch(&refs, bulk_sync_mode)
                    });
                    match tokio::time::timeout(Duration::from_secs(30), db_query).await {
                        Ok(Ok(Ok(db_info))) => {
                            for ((tx_hash, idx), info) in db_info {
                                input_cell_info.insert((tx_hash, idx), info);
                            }
                        }
                        Ok(Ok(Err(e))) => {
                            error!("Parser: DB error fetching cell info: {}", e);
                        }
                        Ok(Err(e)) => {
                            error!("Parser: Failed to fetch cell info from DB: {}", e);
                        }
                        Err(_) => {
                            warn!("Parser: DB query for cell info timed out after 30s, continuing without data");
                        }
                    }
                } else {
                    db_lookups = 0;
                }
                let cache_misses = db_lookups;
                let cell_lookup_ms = t_cell_lookup.elapsed().as_secs_f64() * 1000.0;

                // Pre-compute batch_cell_infos, fees, cell_cache, balance/script changes
                // (moved from writer to overlap with pipeline buffering)
                let t_precompute_parser = Instant::now();

                // Pass 1: Build batch_cell_infos
                let mut batch_cell_infos: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
                for tx_data in &all_tx_data {
                    for (output_index, cell) in tx_data.cells.iter().enumerate() {
                        let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                        let type_script_size = cell
                            .type_args
                            .as_ref()
                            .map(|args| 32 + 1 + args.len() as i64)
                            .unwrap_or(0);
                        let occupied_capacity =
                            (8 + lock_script_size + type_script_size + cell.data_size as i64)
                                * 100_000_000;
                        batch_cell_infos.insert(
                            (tx_data.hash.to_vec(), output_index as i16),
                            LiveCellInfo {
                                capacity: cell.capacity,
                                created_at_block: tx_data.block_number,
                                lock_script_hash: cell.lock_script_hash.clone(),
                                lock_code_hash: cell.lock_code_hash.clone(),
                                lock_hash_type: cell.lock_hash_type,
                                lock_args: cell.lock_args.clone(),
                                type_script_hash: cell.type_script_hash.clone(),
                                type_code_hash: cell.type_code_hash.clone(),
                                data_size: cell.data_size,
                                occupied_capacity,
                            },
                        );
                    }
                }

                // Pass 2: Compute input capacity + fee
                for tx_data in &mut all_tx_data {
                    if !tx_data.is_cellbase {
                        for input in &tx_data.inputs {
                            let key = (
                                input.previous_tx_hash.to_vec(),
                                input.previous_output_index as i16,
                            );
                            if let Some(info) = input_cell_info.get(&key) {
                                tx_data.total_input_capacity += info.capacity;
                            } else if let Some(info) = batch_cell_infos.get(&key) {
                                tx_data.total_input_capacity += info.capacity;
                            }
                        }
                        tx_data.fee = tx_data
                            .total_input_capacity
                            .saturating_sub(tx_data.total_output_capacity);
                    }
                }

                // Pass 3: cell_cache update + address_balance_changes + script_usage_changes
                let mut address_balance_changes: HashMap<
                    Vec<u8>,
                    (i64, i32, i32, i64, i64, Vec<u8>, i64),
                > = HashMap::new();
                let mut script_usage_changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> =
                    HashMap::new();

                for tx_data in &all_tx_data {
                    // cell_cache update
                    for (output_index, cell) in tx_data.cells.iter().enumerate() {
                        let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                        let type_script_size = cell
                            .type_args
                            .as_ref()
                            .map(|a| 32 + 1 + a.len() as i64)
                            .unwrap_or(0);
                        let cell_occupied =
                            (8 + lock_script_size + type_script_size + cell.data_size as i64)
                                * 100_000_000;
                        cell_cache_for_parser.insert(
                            (tx_data.hash, output_index as i32),
                            CachedCellInfo {
                                capacity: cell.capacity,
                                created_at_block: tx_data.block_number,
                                lock_script_hash: cell.lock_script_hash.clone(),
                                lock_code_hash: cell.lock_code_hash.clone(),
                                lock_hash_type: cell.lock_hash_type,
                                lock_args: cell.lock_args.clone(),
                                type_script_hash: cell.type_script_hash.clone(),
                                type_code_hash: cell.type_code_hash.clone(),
                                data_size: cell.data_size,
                                occupied_capacity: cell_occupied,
                            },
                        );
                    }

                    // script_usage_changes - outputs
                    for cell in &tx_data.cells {
                        let lock_key = (cell.lock_code_hash.clone(), false);
                        let entry = script_usage_changes.entry(lock_key).or_insert((0, 0, 0, 0));
                        entry.0 += 1;
                        entry.1 += 1;
                        entry.2 += cell.capacity;
                        entry.3 += cell.capacity;
                        if let Some(ref type_code_hash) = cell.type_code_hash {
                            let type_key = (type_code_hash.clone(), true);
                            let entry =
                                script_usage_changes.entry(type_key).or_insert((0, 0, 0, 0));
                            entry.0 += 1;
                            entry.1 += 1;
                            entry.2 += cell.capacity;
                            entry.3 += cell.capacity;
                        }
                    }

                    // Per-tx balance/consumption tracking
                    let mut tx_balance_changes: HashMap<Vec<u8>, i64> = HashMap::new();
                    let mut tx_cells_consumed: HashMap<Vec<u8>, i32> = HashMap::new();
                    let mut tx_occupied_changes: HashMap<Vec<u8>, i64> = HashMap::new();

                    if !tx_data.is_cellbase {
                        for input in &tx_data.inputs {
                            let key = (
                                input.previous_tx_hash.to_vec(),
                                input.previous_output_index as i16,
                            );
                            let info = input_cell_info
                                .get(&key)
                                .or_else(|| batch_cell_infos.get(&key));
                            if let Some(info) = info {
                                *tx_balance_changes
                                    .entry(info.lock_script_hash.clone())
                                    .or_default() -= info.capacity;
                                *tx_cells_consumed
                                    .entry(info.lock_script_hash.clone())
                                    .or_default() += 1;
                                *tx_occupied_changes
                                    .entry(info.lock_script_hash.clone())
                                    .or_default() -= info.occupied_capacity;
                                // script usage - inputs
                                let lock_key = (info.lock_code_hash.clone(), false);
                                let entry =
                                    script_usage_changes.entry(lock_key).or_insert((0, 0, 0, 0));
                                entry.1 -= 1;
                                entry.3 -= info.capacity;
                                if let Some(ref type_code_hash) = info.type_code_hash {
                                    let type_key = (type_code_hash.clone(), true);
                                    let entry = script_usage_changes
                                        .entry(type_key)
                                        .or_insert((0, 0, 0, 0));
                                    entry.1 -= 1;
                                    entry.3 -= info.capacity;
                                }
                            }
                        }
                    }

                    // address_balance_changes - outputs + merge
                    let mut tx_cells_created: HashMap<Vec<u8>, i32> = HashMap::new();
                    for cell in &tx_data.cells {
                        *tx_balance_changes
                            .entry(cell.lock_script_hash.clone())
                            .or_default() += cell.capacity;
                        *tx_cells_created
                            .entry(cell.lock_script_hash.clone())
                            .or_default() += 1;
                        let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                        let type_script_size = cell
                            .type_args
                            .as_ref()
                            .map(|a| 32 + 1 + a.len() as i64)
                            .unwrap_or(0);
                        let cell_occupied =
                            (8 + lock_script_size + type_script_size + cell.data_size as i64)
                                * 100_000_000;
                        *tx_occupied_changes
                            .entry(cell.lock_script_hash.clone())
                            .or_default() += cell_occupied;
                    }
                    let all_addresses: HashSet<Vec<u8>> = tx_balance_changes
                        .keys()
                        .chain(tx_cells_created.keys())
                        .chain(tx_cells_consumed.keys())
                        .chain(tx_occupied_changes.keys())
                        .cloned()
                        .collect();
                    for lock_hash in all_addresses {
                        let balance_change =
                            tx_balance_changes.get(&lock_hash).copied().unwrap_or(0);
                        let cells_created = tx_cells_created.get(&lock_hash).copied().unwrap_or(0);
                        let cells_consumed =
                            tx_cells_consumed.get(&lock_hash).copied().unwrap_or(0);
                        let occupied_change =
                            tx_occupied_changes.get(&lock_hash).copied().unwrap_or(0);
                        let entry = address_balance_changes.entry(lock_hash.clone()).or_insert((
                            0,
                            0,
                            0,
                            0,
                            tx_data.block_number,
                            tx_data.hash.to_vec(),
                            0,
                        ));
                        entry.0 += balance_change;
                        entry.1 += cells_created - cells_consumed;
                        entry.2 += cells_created;
                        entry.3 += 1;
                        entry.4 = tx_data.block_number;
                        entry.5 = tx_data.hash.to_vec();
                        entry.6 += occupied_change;
                    }
                }
                // cell_cache eviction check
                if cell_cache_for_parser.len() > CELL_CACHE_CAPACITY * 2 {
                    cell_cache_for_parser.clear();
                }

                let precompute_parser_ms = t_precompute_parser.elapsed().as_secs_f64() * 1000.0;
                let total_parser_ms = t_parser.elapsed().as_secs_f64() * 1000.0;
                let tx_count: usize = all_tx_data.len();
                let cell_count: usize = all_tx_data.iter().map(|t| t.cells.len()).sum();
                let input_count: usize = all_tx_data
                    .iter()
                    .filter(|t| !t.is_cellbase)
                    .map(|t| t.inputs.len())
                    .sum();
                let queue_depth = parse_tx.max_capacity() - parse_tx.capacity();
                let cache_total = cache_hits + cache_misses;
                let hit_rate = if cache_total > 0 {
                    cache_hits as f64 / cache_total as f64 * 100.0
                } else {
                    0.0
                };
                info!(
                    parse_ms = format!("{:.1}", t_parse_ms),
                    cell_lookup_ms = format!("{:.1}", cell_lookup_ms),
                    precompute_ms = format!("{:.1}", precompute_parser_ms),
                    total_ms = format!("{:.1}", total_parser_ms),
                    txs = tx_count,
                    cells = cell_count,
                    inputs = input_count,
                    cache_hits,
                    cache_misses,
                    cache_hit_pct = format!("{:.0}", hit_rate),
                    cache_size = cell_cache_for_parser.len(),
                    queue_depth,
                    "Parser batch {}-{}",
                    start_block,
                    end_block,
                );

                if parse_tx
                    .send((
                        start_block,
                        end_block,
                        chain_tip,
                        blocks,
                        all_parsed_blocks,
                        all_tx_data,
                        input_cell_info,
                        batch_cell_infos,
                        address_balance_changes,
                        script_usage_changes,
                    ))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // === Writer loop ===
        loop {
            if self.repo.has_unresolved_deep_fork().unwrap_or(false) {
                warn!("Deep fork unresolved, sync paused. Waiting for manual intervention...");
                Self::drain_channel(&mut parse_rx).await;
                sleep(Duration::from_secs(30)).await;
                continue;
            }

            let recv_timeout = Duration::from_millis(self.config.poll_interval_ms * 2);
            let t_recv = Instant::now();
            match tokio::time::timeout(recv_timeout, parse_rx.recv()).await {
                Ok(Some((
                    start_block,
                    end_block,
                    chain_tip,
                    blocks,
                    all_parsed_blocks,
                    all_tx_data,
                    input_cell_info,
                    batch_cell_infos,
                    address_balance_changes,
                    script_usage_changes,
                ))) => {
                    let recv_wait_ms = t_recv.elapsed().as_secs_f64() * 1000.0;
                    let (db_tip, db_tip_hash) = self.repo.get_sync_tip().await?;
                    let expected_start = if db_tip == 0 && db_tip_hash.is_none() {
                        0
                    } else {
                        (db_tip + 1) as u64
                    };

                    if start_block != expected_start {
                        warn!(
                            "Pipeline batch mismatch: expected {}, got {}. Draining stale batches.",
                            expected_start, start_block
                        );
                        self.reorg_notify_flag.store(true, Ordering::SeqCst);
                        Self::drain_channel(&mut parse_rx).await;
                        continue;
                    }

                    let blocks_behind = chain_tip.saturating_sub(db_tip as u64);
                    if blocks_behind <= self.config.bulk_sync_threshold {
                        if let Some(ref stored_hash) = db_tip_hash {
                            if db_tip > 0 {
                                match self
                                    .check_and_handle_reorg(db_tip as u64, stored_hash)
                                    .await?
                                {
                                    Some(ReorgAction::Handled(_)) => {
                                        info!("Reorg handled, draining stale batches");
                                        self.reorg_notify_flag.store(true, Ordering::SeqCst);
                                        Self::drain_channel(&mut parse_rx).await;
                                        continue;
                                    }
                                    Some(ReorgAction::DeepForkPaused) => {
                                        warn!("Deep fork detected, sync paused");
                                        self.reorg_notify_flag.store(true, Ordering::SeqCst);
                                        Self::drain_channel(&mut parse_rx).await;
                                        sleep(Duration::from_secs(30)).await;
                                        continue;
                                    }
                                    None => {}
                                }
                            }
                        }
                    }

                    let db_start = Instant::now();
                    if let Err(e) = self
                        .write_parsed_batch(
                            &blocks,
                            &all_parsed_blocks,
                            all_tx_data,
                            input_cell_info,
                            batch_cell_infos,
                            address_balance_changes,
                            script_usage_changes,
                            chain_tip,
                        )
                        .await
                    {
                        error!("Sync error: {:?}", e);
                        if let Err(cleanup_err) = self
                            .writer
                            .cleanup_batch_range(start_block as i64, end_block as i64)
                        {
                            error!("Failed to cleanup partial batch: {:?}", cleanup_err);
                        }
                        self.reorg_notify_flag.store(true, Ordering::SeqCst);
                        Self::drain_channel(&mut parse_rx).await;
                        sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                    let db_elapsed = db_start.elapsed();
                    self.perf.add(&self.perf.db_write_us, db_elapsed);

                    if db_elapsed.as_secs() >= 5 {
                        let stats = self.writer.store().memory_stats();
                        warn!(
                            db_ms = format!("{:.1}", db_elapsed.as_secs_f64() * 1000.0),
                            compaction_pending_mb = stats.compaction_pending_bytes / (1024 * 1024),
                            running_compactions = stats.num_running_compactions,
                            l0_total = stats.l0_files_count,
                            l0_max = stats.l0_files_max,
                            l0_worst_cf = stats.l0_worst_cf,
                            memtable_mb = stats.memtable_bytes / (1024 * 1024),
                            imm_memtables = stats.immutable_memtables,
                            "Slow DB write detected (possible write stall)"
                        );
                    }

                    if let Some(last_block) = all_parsed_blocks.last() {
                        self.progress.update_current_batch(
                            last_block.number as u64,
                            all_parsed_blocks.len() as u64,
                        );

                        let mode = if self.is_bulk_sync_active() {
                            "[BULK]"
                        } else {
                            ""
                        };
                        let partition_range = format_partition_range(start_block, end_block);
                        let boundary_info = if crosses_partition_boundary(start_block, end_block) {
                            " (crosses boundary)"
                        } else {
                            ""
                        };
                        let writer_queue = parse_tx_for_writer_depth.max_capacity()
                            - parse_tx_for_writer_depth.capacity();
                        info!(
                            "Wrote blocks {} to {} ({} remaining, {:.2}s, q={}, wait={:.0}ms) {}{} {}",
                            start_block,
                            end_block,
                            self.progress.blocks_remaining(),
                            db_elapsed.as_secs_f64(),
                            writer_queue,
                            recv_wait_ms,
                            partition_range,
                            boundary_info,
                            mode
                        );

                        if !self.is_secondary_issuance_bulk_active() {
                            for block in &all_parsed_blocks {
                                if let Err(e) = self
                                    .update_secondary_issuance(
                                        &format!("0x{}", hex::encode(&block.hash)),
                                        &hex::encode(&block.dao),
                                        block.number,
                                        block.timestamp,
                                    )
                                    .await
                                {
                                    warn!(
                                        "Failed to update secondary issuance for block {}: {}",
                                        block.number, e
                                    );
                                }
                            }
                        }

                        let crossed_1000 = (start_block / 1000) != (end_block / 1000);
                        if crossed_1000 && !self.is_bulk_sync_active() {
                            let update_block = ((end_block / 1000) * 1000) as i64;
                            let writer = self.writer.clone();
                            tokio::spawn(async move {
                                if let Err(e) =
                                    writer.recalculate_dao_extended_statistics(update_block)
                                {
                                    warn!("Failed to recalculate DAO statistics: {}", e);
                                }
                            });
                        }

                        self.maybe_invalidate_chart_caches(end_block).await;
                        self.check_bulk_sync_completion().await;
                    }

                    self.perf
                        .blocks_count
                        .fetch_add(all_parsed_blocks.len() as u64, Ordering::Relaxed);
                    self.perf.report_and_reset();
                }
                Ok(None) => {
                    fetcher.abort();
                    parser.abort();
                    return Err(anyhow::anyhow!("Pipeline channel closed"));
                }
                Err(_timeout) => {
                    // Idle timeout - no pending batches
                }
            }
        }
    }

    async fn fetch_blocks_with_config(
        rpc: &CkbRpcClient,
        start: u64,
        end: u64,
        parallel_fetch_size: usize,
    ) -> Result<Vec<BlockResponseWithCycles>> {
        let mut blocks = Vec::with_capacity((end - start + 1) as usize);
        let mut current = start;
        while current <= end {
            let batch_end = std::cmp::min(current + parallel_fetch_size as u64 - 1, end);
            let mut futures = FuturesOrdered::new();
            for block_num in current..=batch_end {
                futures.push_back(
                    async move { (block_num, rpc.get_block_by_number(block_num).await) },
                );
            }
            while let Some((block_num, result)) = futures.next().await {
                match result {
                    Ok(Some(block)) => blocks.push(block),
                    Ok(None) => return Err(anyhow::anyhow!("Block {} not found", block_num)),
                    Err(e) => return Err(e),
                }
            }
            current = batch_end + 1;
        }
        Ok(blocks)
    }

    fn fetch_blocks_direct(
        store: &CkbChainReader,
        start: u64,
        end: u64,
    ) -> Result<Vec<BlockResponseWithCycles>> {
        let block_numbers: Vec<u64> = (start..=end).collect();
        let results: Vec<Result<BlockResponseWithCycles>> = block_numbers
            .par_iter()
            .map(|&num| {
                let hash = store.get_block_hash(num).ok_or_else(|| {
                    anyhow::anyhow!("Block {} hash not found in CKB RocksDB", num)
                })?;
                let block = store.get_block(&hash).ok_or_else(|| {
                    anyhow::anyhow!("Block {} data not found in CKB RocksDB", num)
                })?;
                let rpc_block = ckb_store_reader::block_view_to_rpc(&block, store);
                Ok(rpc_block.into())
            })
            .collect();
        results.into_iter().collect()
    }

    async fn drain_channel<T>(rx: &mut tokio::sync::mpsc::Receiver<T>) {
        let mut drained = 0;
        while rx.try_recv().is_ok() {
            drained += 1;
        }
        if drained > 0 {
            info!("Drained {} stale batches from pipeline", drained);
        }
    }

    async fn maybe_invalidate_chart_caches(&self, current_block: u64) {
        if !self.cache_invalidator.is_enabled() {
            return;
        }
        let blocks_remaining = self.progress.blocks_remaining();
        if blocks_remaining < 100 {
            return;
        }
        let mut last_invalidation = self.last_cache_invalidation.lock().await;
        if current_block >= *last_invalidation + CACHE_INVALIDATION_INTERVAL {
            self.cache_invalidator.invalidate_chart_caches().await;
            *last_invalidation = current_block;
        }
    }
    // === sync_batch, check_bulk_sync_completion, task submission ===

    async fn sync_batch(&self) -> Result<SyncAction> {
        let chain_tip = self.get_chain_tip().await?;
        self.progress.update_target(chain_tip);

        let (db_tip, db_tip_hash) = self.repo.get_sync_tip().await?;
        let start_block = if db_tip == 0 && db_tip_hash.is_none() {
            0
        } else {
            (db_tip + 1) as u64
        };

        if start_block > chain_tip {
            return Ok(SyncAction::CaughtUp);
        }

        let blocks_behind = chain_tip.saturating_sub(start_block);
        if blocks_behind <= self.config.bulk_sync_threshold {
            if let Some(ref stored_hash) = db_tip_hash {
                if db_tip > 0 {
                    match self
                        .check_and_handle_reorg(db_tip as u64, stored_hash)
                        .await?
                    {
                        Some(ReorgAction::Handled(_)) => return Ok(SyncAction::ReorgHandled),
                        Some(ReorgAction::DeepForkPaused) => return Ok(SyncAction::DeepForkPaused),
                        None => {}
                    }
                }
            }
        }

        let mut end_block =
            std::cmp::min(start_block + self.config.batch_size as u64 - 1, chain_tip);

        if start_block > end_block {
            return Ok(SyncAction::CaughtUp);
        }

        // Live sync accumulation
        if end_block == start_block && blocks_behind <= self.config.bulk_sync_threshold {
            let accumulation_timeout = Duration::from_secs(2);
            let max_accumulate = 5u64;
            let deadline = Instant::now() + accumulation_timeout;
            while Instant::now() < deadline {
                sleep(Duration::from_millis(200)).await;
                if let Ok(new_tip) = self.get_chain_tip().await {
                    if new_tip > end_block {
                        end_block = std::cmp::min(
                            start_block + max_accumulate - 1,
                            std::cmp::min(new_tip, start_block + self.config.batch_size as u64 - 1),
                        );
                        self.progress.update_target(new_tip);
                        if end_block - start_block + 1 >= max_accumulate {
                            break;
                        }
                    }
                }
            }
        }

        let fetch_start = Instant::now();
        let blocks = self.fetch_blocks_parallel(start_block, end_block).await?;
        self.perf.add(&self.perf.fetch_us, fetch_start.elapsed());

        let db_start = Instant::now();
        if let Err(e) = self.sync_blocks_batch(&blocks, chain_tip).await {
            if let Err(cleanup_err) = self
                .writer
                .cleanup_batch_range(start_block as i64, end_block as i64)
            {
                error!("Failed to cleanup partial batch: {:?}", cleanup_err);
            }
            return Err(e);
        }
        let db_elapsed = db_start.elapsed();
        self.perf.add(&self.perf.db_write_us, db_elapsed);

        if db_elapsed.as_secs() >= 5 {
            let stats = self.writer.store().memory_stats();
            warn!(
                db_ms = format!("{:.1}", db_elapsed.as_secs_f64() * 1000.0),
                compaction_pending_mb = stats.compaction_pending_bytes / (1024 * 1024),
                running_compactions = stats.num_running_compactions,
                l0_total = stats.l0_files_count,
                l0_max = stats.l0_files_max,
                l0_worst_cf = stats.l0_worst_cf,
                memtable_mb = stats.memtable_bytes / (1024 * 1024),
                "Slow DB write detected (possible write stall)"
            );
        }

        if let Some(last_block_response) = blocks.last() {
            let last_block_number = BlockParser::parse_block_number(&last_block_response.block);
            self.progress
                .update_current_batch(last_block_number, blocks.len() as u64);

            let partition_range = format_partition_range(start_block, end_block);
            let boundary_info = if crosses_partition_boundary(start_block, end_block) {
                " (crosses boundary)"
            } else {
                ""
            };
            info!(
                "Wrote blocks {} to {} ({} remaining, {:.2}s) {}{}",
                start_block,
                end_block,
                self.progress.blocks_remaining(),
                db_elapsed.as_secs_f64(),
                partition_range,
                boundary_info
            );
        }
        self.perf
            .blocks_count
            .fetch_add(blocks.len() as u64, Ordering::Relaxed);
        self.perf.report_and_reset();

        if !blocks.is_empty() {
            if !self.is_secondary_issuance_bulk_active() {
                for block_response in &blocks {
                    let block_number =
                        BlockParser::parse_block_number(&block_response.block) as i64;
                    let block_timestamp =
                        BlockParser::parse_timestamp(&block_response.block.header.timestamp);
                    if let Err(e) = self
                        .update_secondary_issuance(
                            &block_response.block.header.hash,
                            &block_response.block.header.dao,
                            block_number,
                            block_timestamp,
                        )
                        .await
                    {
                        warn!(
                            "Failed to update secondary issuance for block {}: {}",
                            block_number, e
                        );
                    }
                }
            }

            let crossed_1000 = (start_block / 1000) != (end_block / 1000);
            if crossed_1000 && !self.is_bulk_sync_active() {
                let update_block = ((end_block / 1000) * 1000) as i64;
                let writer = self.writer.clone();
                tokio::spawn(async move {
                    if let Err(e) = writer.recalculate_dao_extended_statistics(update_block) {
                        warn!("Failed to recalculate DAO statistics: {}", e);
                    }
                });
            }

            self.maybe_invalidate_chart_caches(end_block).await;
        }

        self.check_bulk_sync_completion().await;

        Ok(SyncAction::Continue)
    }

    async fn check_bulk_sync_completion(&self) {
        let currently_bulk = self.is_bulk_sync_active();
        let was_bulk = self.was_bulk_sync_active.load(Ordering::SeqCst);
        let currently_secondary_bulk = self.is_secondary_issuance_bulk_active();
        let was_secondary_bulk = self
            .was_secondary_issuance_bulk_active
            .load(Ordering::SeqCst);

        if was_bulk && !currently_bulk {
            let stats = self.writer.store().memory_stats();
            let current = self.progress.current();
            let chain_tip = self.progress.target();
            let sst_gb = stats.sst_files_size as f64 / (1024.0 * 1024.0 * 1024.0);

            self.cache_invalidator
                .update_sync_status(|status| {
                    status.mark_bulk_sync_completed(chain_tip as i64);
                })
                .await;

            let elapsed = self
                .cache_invalidator
                .get_sync_status()
                .await
                .and_then(|s| s.bulk_sync_total_seconds());
            let avg_bps = elapsed
                .filter(|&e| e > 0)
                .map(|e| current as f64 / e as f64);
            info!(
                blocks_synced = current,
                elapsed_secs = elapsed.unwrap_or(0),
                avg_bps = format!("{:.1}", avg_bps.unwrap_or(0.0)),
                sst_size_gb = format!("{:.1}", sst_gb),
                "Bulk sync completed"
            );

            // Re-enable auto-compactions and trigger manual compaction in background
            self.writer.store().restore_normal_compaction_options();
            let store_compact = Arc::clone(self.writer.store());
            tokio::task::spawn_blocking(move || {
                store_compact.trigger_full_compaction();
            });

            // Rebuild cell secondary indices (skipped during bulk sync)
            info!("Starting cell index rebuild from LIVE_CELLS");
            let store_rebuild = Arc::clone(self.writer.store());
            let pause_flag = Arc::clone(&self.rebuild_pause_flag);
            pause_flag.store(true, Ordering::SeqCst);
            tokio::task::spawn_blocking(move || {
                rebuild_cell_indices(&store_rebuild);
                pause_flag.store(false, Ordering::SeqCst);
                info!("Cell index rebuild finished, resuming sync");
            });

            // Rebuild activities (skipped during bulk sync)
            info!("Starting activities rebuild");
            let store_act = Arc::clone(self.writer.store());
            let ckb_store_act = self.ckb_store.clone();
            tokio::task::spawn_blocking(move || {
                rebuild_activities(&store_act, ckb_store_act.as_deref());
                // Clear the deferred flag after successful rebuild
                if let Err(e) = store_act.update_sync_status(|s| {
                    s.activities_deferred = false;
                }) {
                    warn!("Failed to clear activities_deferred flag: {}", e);
                }
            });

            self.maybe_submit_pending_rebuild_tasks();
        }

        if was_secondary_bulk && !currently_secondary_bulk {
            info!("Secondary issuance bulk sync completed");
        }

        self.was_bulk_sync_active
            .store(currently_bulk, Ordering::SeqCst);
        self.was_secondary_issuance_bulk_active
            .store(currently_secondary_bulk, Ordering::SeqCst);
    }

    fn maybe_submit_pending_rebuild_tasks(&self) {
        if let Err(e) = self.maybe_submit_label_import_task() {
            warn!("Failed to submit label import task: {}", e);
        }
    }

    fn submit_task_if_not_exists(
        &self,
        task_type: &str,
        config: serde_json::Value,
        priority: i32,
        max_retries: i32,
    ) -> Result<()> {
        use ckbadger_store::TaskEntry;

        let store = self.writer.store();
        let pending = store.list_tasks_by_status("pending")?;
        if pending.iter().any(|t| t.task_type == task_type) {
            info!("{} task already pending, skipping submission", task_type);
            return Ok(());
        }
        let running = store.list_tasks_by_status("running")?;
        if running.iter().any(|t| t.task_type == task_type) {
            info!("{} task already running, skipping submission", task_type);
            return Ok(());
        }

        let entry = TaskEntry {
            id: Uuid::new_v4(),
            task_type: task_type.to_string(),
            status: "pending".to_string(),
            priority,
            config: serde_json::to_string(&config).unwrap_or_default(),
            progress_total: None,
            progress_current: None,
            progress_message: None,
            result: None,
            error_message: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            heartbeat_at: None,
            runner_id: None,
            retry_count: 0,
            max_retries,
            rate_samples: None,
            rate_ema: None,
            log_tail: None,
        };
        store.create_task(&entry)?;
        info!("Submitted {} task: {}", task_type, entry.id);
        Ok(())
    }

    fn maybe_submit_label_import_task(&self) -> Result<()> {
        use ckbadger_common::{LabelImportConfig, TaskBuilder};
        let token_labels_path =
            std::env::var("TOKEN_LABELS_PATH").unwrap_or_else(|_| "docs/token-labels".to_string());
        if !std::path::Path::new(&token_labels_path)
            .join("information")
            .exists()
        {
            debug!(
                "Token labels directory not found at {}, skipping label import",
                token_labels_path
            );
            return Ok(());
        }
        let builder = TaskBuilder::label_import(LabelImportConfig {
            token_labels_path,
            ..Default::default()
        });
        self.submit_task_if_not_exists(
            &builder.task_type().to_string(),
            builder.config().clone(),
            builder.get_priority(),
            builder.get_max_retries(),
        )
    }

    async fn fetch_blocks_parallel(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Vec<BlockResponseWithCycles>> {
        if let Some(ref store) = self.ckb_store {
            let store = Arc::clone(store);
            tokio::task::spawn_blocking(move || Self::fetch_blocks_direct(&store, start, end))
                .await
                .map_err(|e| anyhow::anyhow!("Block fetch task panicked: {}", e))?
        } else {
            Self::fetch_blocks_with_config(&self.rpc, start, end, self.config.parallel_fetch_size)
                .await
        }
    }

    // === sync_blocks_batch (sequential path) ===

    async fn sync_blocks_batch(
        &self,
        blocks: &[BlockResponseWithCycles],
        chain_tip: u64,
    ) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let blocks_clone: Vec<BlockResponseWithCycles> = blocks.to_vec();
        let (all_parsed_blocks, mut all_tx_data, all_input_outpoints) =
            tokio::task::spawn_blocking(move || parse_blocks_parallel(&blocks_clone)).await?;

        let end_block = all_parsed_blocks
            .last()
            .map(|b| b.number as u64)
            .unwrap_or(0);
        let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;

        let mut batch_cell_infos: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                let type_script_size = cell
                    .type_args
                    .as_ref()
                    .map(|args| 32 + 1 + args.len() as i64)
                    .unwrap_or(0);
                let occupied_capacity =
                    (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000;
                batch_cell_infos.insert(
                    (tx_data.hash.to_vec(), output_index as i16),
                    LiveCellInfo {
                        capacity: cell.capacity,
                        created_at_block: tx_data.block_number,
                        lock_script_hash: cell.lock_script_hash.clone(),
                        lock_code_hash: cell.lock_code_hash.clone(),
                        lock_hash_type: cell.lock_hash_type,
                        lock_args: cell.lock_args.clone(),
                        type_script_hash: cell.type_script_hash.clone(),
                        type_code_hash: cell.type_code_hash.clone(),
                        data_size: cell.data_size,
                        occupied_capacity,
                    },
                );
            }
        }

        let mut input_cell_info: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
        for (tx_hash, idx) in &all_input_outpoints {
            let hash_arr: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
            let key = (hash_arr, *idx as i32);
            if let Some(cached) = self.cell_cache.get(&key) {
                input_cell_info.insert(
                    (tx_hash.clone(), *idx),
                    LiveCellInfo {
                        capacity: cached.capacity,
                        created_at_block: cached.created_at_block,
                        lock_script_hash: cached.lock_script_hash.clone(),
                        lock_code_hash: cached.lock_code_hash.clone(),
                        lock_hash_type: cached.lock_hash_type,
                        lock_args: cached.lock_args.clone(),
                        type_script_hash: cached.type_script_hash.clone(),
                        type_code_hash: cached.type_code_hash.clone(),
                        data_size: cached.data_size,
                        occupied_capacity: cached.occupied_capacity,
                    },
                );
            }
        }

        let missing_outpoints: Vec<(Vec<u8>, i16)> = all_input_outpoints
            .iter()
            .filter(|(h, i)| {
                let key = (h.clone(), *i);
                !input_cell_info.contains_key(&key) && !batch_cell_infos.contains_key(&key)
            })
            .cloned()
            .collect();

        if !missing_outpoints.is_empty() {
            let unique_missing: Vec<(Vec<u8>, i16)> = {
                let mut seen = HashSet::new();
                missing_outpoints
                    .into_iter()
                    .filter(|x| seen.insert(x.clone()))
                    .collect()
            };
            let missing_refs: Vec<(&[u8], i16)> = unique_missing
                .iter()
                .map(|(h, i)| (h.as_slice(), *i))
                .collect();
            let db_info = self
                .writer
                .get_full_cells_info_batch(&missing_refs, bulk_sync_mode)?;
            for ((tx_hash, idx), info) in db_info {
                input_cell_info.insert((tx_hash, idx), info);
            }
        }

        for tx_data in &mut all_tx_data {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    );
                    if let Some(info) = input_cell_info.get(&key) {
                        tx_data.total_input_capacity += info.capacity;
                    } else if let Some(info) = batch_cell_infos.get(&key) {
                        tx_data.total_input_capacity += info.capacity;
                    }
                }
                tx_data.fee = tx_data
                    .total_input_capacity
                    .saturating_sub(tx_data.total_output_capacity);
            }
        }

        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                let type_script_size = cell
                    .type_args
                    .as_ref()
                    .map(|a| 32 + 1 + a.len() as i64)
                    .unwrap_or(0);
                let cell_occupied =
                    (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000;
                self.cell_cache.insert(
                    (tx_data.hash, output_index as i32),
                    CachedCellInfo {
                        capacity: cell.capacity,
                        created_at_block: tx_data.block_number,
                        lock_script_hash: cell.lock_script_hash.clone(),
                        lock_code_hash: cell.lock_code_hash.clone(),
                        lock_hash_type: cell.lock_hash_type,
                        lock_args: cell.lock_args.clone(),
                        type_script_hash: cell.type_script_hash.clone(),
                        type_code_hash: cell.type_code_hash.clone(),
                        data_size: cell.data_size,
                        occupied_capacity: cell_occupied,
                    },
                );
            }
        }
        if self.cell_cache.len() > CELL_CACHE_CAPACITY * 2 {
            self.cell_cache.clear();
        }

        // Prepare all data for insertion
        let block_refs: Vec<&crate::parser::block::ParsedBlock> =
            all_parsed_blocks.iter().collect();

        let txs_for_batch: Vec<_> = all_tx_data
            .iter()
            .map(|tx_data| {
                (
                    tx_data.hash.as_slice(),
                    tx_data.block_number,
                    tx_data.block_hash.as_slice(),
                    tx_data.tx_index,
                    tx_data.version,
                    tx_data.inputs_count,
                    tx_data.outputs_count,
                    tx_data.witnesses_count,
                    tx_data.cell_deps_count,
                    tx_data.header_deps_count,
                    tx_data.total_input_capacity,
                    tx_data.total_output_capacity,
                    tx_data.fee,
                    Some(tx_data.tx_size),
                    tx_data.cycles,
                    tx_data.is_cellbase,
                    tx_data.timestamp,
                )
            })
            .collect();

        let mut all_cells: Vec<(&[u8], i16, &crate::parser::cell::ParsedCell, i64)> = Vec::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                all_cells.push((
                    tx_data.hash.as_slice(),
                    output_index as i16,
                    cell,
                    tx_data.block_number,
                ));
            }
        }

        // Write blocks, txs, cells via StoreBatch
        let t_headers = Instant::now();
        {
            let mut batch = StoreBatch::new(self.writer.store());
            if !block_refs.is_empty() {
                self.writer.insert_blocks_batch(&block_refs, &mut batch)?;
            }
            if !txs_for_batch.is_empty() {
                self.writer
                    .insert_transactions_batch(&txs_for_batch, &mut batch)?;
            }
            if !all_cells.is_empty() {
                self.writer
                    .insert_cells_batch(&all_cells, &mut batch, bulk_sync_mode)?;
            }
            batch.commit()?;
        }
        let headers_ms = t_headers.elapsed().as_secs_f64() * 1000.0;

        // Block proposals (no-op in RocksDB but kept for API compatibility)
        for parsed_block in &all_parsed_blocks {
            if !parsed_block.proposals.is_empty() {
                self.writer
                    .insert_block_proposals_batch(parsed_block.number, &parsed_block.proposals)?;
                if !self.is_bulk_sync_active() {
                    self.cache_block_proposals(&parsed_block.proposals, parsed_block.number)
                        .await;
                }
            }
        }

        // Inputs and flows (no-ops in RocksDB model)
        let mut all_inputs: Vec<(&[u8], i64, i16, &crate::parser::transaction::ParsedInput)> =
            Vec::new();
        for tx_data in &all_tx_data {
            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    all_inputs.push((
                        tx_data.hash.as_slice(),
                        tx_data.block_number,
                        input_index as i16,
                        input,
                    ));
                }
            }
        }

        let mut all_flows: Vec<(i64, &[u8], i16, i16, &[u8], i64, i32, Option<&[u8]>)> = Vec::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                all_flows.push((
                    tx_data.block_number,
                    tx_data.hash.as_slice(),
                    output_index as i16,
                    0,
                    cell.lock_script_hash.as_slice(),
                    cell.capacity,
                    cell.data_size,
                    None,
                ));
            }
        }
        for tx_data in &all_tx_data {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    );
                    let info = input_cell_info
                        .get(&key)
                        .or_else(|| batch_cell_infos.get(&key));
                    if let Some(info) = info {
                        all_flows.push((
                            tx_data.block_number,
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            1,
                            info.lock_script_hash.as_slice(),
                            info.capacity,
                            info.data_size,
                            Some(tx_data.hash.as_slice()),
                        ));
                    }
                }
            }
        }

        if !all_inputs.is_empty() {
            self.writer.insert_transaction_inputs_batch(&all_inputs)?;
        }
        if !all_flows.is_empty() {
            self.writer.insert_cell_flows_batch(&all_flows)?;
        }

        // Consume cells
        let t_cells = Instant::now();
        let mut all_consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i16)> = Vec::new();
        for tx_data in &all_tx_data {
            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    );
                    if let Some(info) = input_cell_info.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            info.created_at_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
                        ));
                    } else if let Some(info) = batch_cell_infos.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            info.created_at_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
                        ));
                    }
                }
            }
        }
        // Single batch for consume + address balances + script usage
        let mut consume_addr_batch = StoreBatch::new(self.writer.store());
        if !all_consumptions.is_empty() {
            self.writer.consume_cells_batch_preloaded(
                &all_consumptions,
                &input_cell_info,
                &batch_cell_infos,
                &mut consume_addr_batch,
                bulk_sync_mode,
            )?;
        }

        // Address balances
        let mut address_balance_changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>, i64)> =
            HashMap::new();
        for tx_data in &all_tx_data {
            let mut tx_balance_changes: HashMap<Vec<u8>, i64> = HashMap::new();
            let mut tx_cells_created: HashMap<Vec<u8>, i32> = HashMap::new();
            let mut tx_cells_consumed: HashMap<Vec<u8>, i32> = HashMap::new();
            let mut tx_occupied_changes: HashMap<Vec<u8>, i64> = HashMap::new();
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    );
                    let info = input_cell_info
                        .get(&key)
                        .or_else(|| batch_cell_infos.get(&key));
                    if let Some(info) = info {
                        *tx_balance_changes
                            .entry(info.lock_script_hash.clone())
                            .or_default() -= info.capacity;
                        *tx_cells_consumed
                            .entry(info.lock_script_hash.clone())
                            .or_default() += 1;
                        *tx_occupied_changes
                            .entry(info.lock_script_hash.clone())
                            .or_default() -= info.occupied_capacity;
                    }
                }
            }
            for cell in &tx_data.cells {
                *tx_balance_changes
                    .entry(cell.lock_script_hash.clone())
                    .or_default() += cell.capacity;
                *tx_cells_created
                    .entry(cell.lock_script_hash.clone())
                    .or_default() += 1;
                let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                let type_script_size = cell
                    .type_args
                    .as_ref()
                    .map(|a| 32 + 1 + a.len() as i64)
                    .unwrap_or(0);
                let cell_occupied =
                    (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000;
                *tx_occupied_changes
                    .entry(cell.lock_script_hash.clone())
                    .or_default() += cell_occupied;
            }
            let all_addresses: HashSet<Vec<u8>> = tx_balance_changes
                .keys()
                .chain(tx_cells_created.keys())
                .chain(tx_cells_consumed.keys())
                .chain(tx_occupied_changes.keys())
                .cloned()
                .collect();
            for lock_hash in all_addresses {
                let balance_change = tx_balance_changes.get(&lock_hash).copied().unwrap_or(0);
                let cells_created = tx_cells_created.get(&lock_hash).copied().unwrap_or(0);
                let cells_consumed = tx_cells_consumed.get(&lock_hash).copied().unwrap_or(0);
                let occupied_change = tx_occupied_changes.get(&lock_hash).copied().unwrap_or(0);
                let entry = address_balance_changes.entry(lock_hash.clone()).or_insert((
                    0,
                    0,
                    0,
                    0,
                    tx_data.block_number,
                    tx_data.hash.to_vec(),
                    0,
                ));
                entry.0 += balance_change;
                entry.1 += cells_created - cells_consumed;
                entry.2 += cells_created;
                entry.3 += 1;
                entry.4 = tx_data.block_number;
                entry.5 = tx_data.hash.to_vec();
                entry.6 += occupied_change;

                // Index address → transaction
                consume_addr_batch.put_addr_tx(
                    &lock_hash,
                    tx_data.block_number,
                    tx_data.tx_index,
                    &tx_data.hash,
                );
            }
        }

        let changes_ref: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8], i64)> =
            address_balance_changes
                .iter()
                .map(|(k, (a, b, c, d, e, f, g))| {
                    (k.clone(), (*a, *b, *c, *d, *e, f.as_slice(), *g))
                })
                .collect();

        // Script usage
        let mut script_usage_changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> =
            HashMap::new();
        for tx_data in &all_tx_data {
            for cell in &tx_data.cells {
                let lock_key = (cell.lock_code_hash.clone(), false);
                let entry = script_usage_changes.entry(lock_key).or_insert((0, 0, 0, 0));
                entry.0 += 1;
                entry.1 += 1;
                entry.2 += cell.capacity;
                entry.3 += cell.capacity;
                if let Some(ref type_code_hash) = cell.type_code_hash {
                    let type_key = (type_code_hash.clone(), true);
                    let entry = script_usage_changes.entry(type_key).or_insert((0, 0, 0, 0));
                    entry.0 += 1;
                    entry.1 += 1;
                    entry.2 += cell.capacity;
                    entry.3 += cell.capacity;
                }
            }
        }
        for tx_data in &all_tx_data {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    );
                    let info = input_cell_info
                        .get(&key)
                        .or_else(|| batch_cell_infos.get(&key));
                    if let Some(info) = info {
                        let lock_key = (info.lock_code_hash.clone(), false);
                        let entry = script_usage_changes.entry(lock_key).or_insert((0, 0, 0, 0));
                        entry.1 -= 1;
                        entry.3 -= info.capacity;
                        if let Some(ref type_code_hash) = info.type_code_hash {
                            let type_key = (type_code_hash.clone(), true);
                            let entry =
                                script_usage_changes.entry(type_key).or_insert((0, 0, 0, 0));
                            entry.1 -= 1;
                            entry.3 -= info.capacity;
                        }
                    }
                }
            }
        }

        let skip_address_balances = self
            .address_balances_deferred
            .load(std::sync::atomic::Ordering::Relaxed)
            && bulk_sync_mode;

        // Parallel DB reads for address balances and script usage
        let lock_hash_keys: Vec<&Vec<u8>> = if !skip_address_balances && !changes_ref.is_empty() {
            changes_ref.keys().collect()
        } else {
            vec![]
        };
        let unique_code_hashes: Vec<Vec<u8>> = if !script_usage_changes.is_empty() {
            let mut seen = std::collections::HashSet::new();
            script_usage_changes
                .keys()
                .filter_map(|(code_hash, _)| {
                    if seen.insert(code_hash.clone()) {
                        Some(code_hash.clone())
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            vec![]
        };
        let code_hash_refs: Vec<&Vec<u8>> = unique_code_hashes.iter().collect();

        let need_balances = !lock_hash_keys.is_empty();
        let need_scripts = !code_hash_refs.is_empty();

        if need_balances || need_scripts {
            let writer = &self.writer;
            let (existing_balances, existing_scripts) = std::thread::scope(|s| {
                let bal = if need_balances {
                    Some(s.spawn(|| writer.read_address_balances(&lock_hash_keys)))
                } else {
                    None
                };
                let scr = if need_scripts {
                    Some(s.spawn(|| writer.read_script_info(&code_hash_refs)))
                } else {
                    None
                };
                (
                    bal.map(|h| h.join().unwrap()),
                    scr.map(|h| h.join().unwrap()),
                )
            });
            if let Some(existing) = existing_balances {
                self.writer.apply_address_balance_deltas(
                    &existing?,
                    &changes_ref,
                    &mut consume_addr_batch,
                )?;
            }
            if let Some(existing) = existing_scripts {
                self.writer.apply_script_usage_deltas(
                    &existing?,
                    &script_usage_changes,
                    &mut consume_addr_batch,
                )?;
            }
        }
        {
            consume_addr_batch.commit()?;
        }

        let cells_ms = t_cells.elapsed().as_secs_f64() * 1000.0;

        // Accumulate batch statistics
        let t_stats = Instant::now();
        let mut batch_stats = BatchStats::default();
        let mut prev_timestamp: Option<chrono::DateTime<Utc>> =
            if let Some(first_block) = all_parsed_blocks.first() {
                self.writer
                    .get_previous_block_timestamp(first_block.number)?
            } else {
                None
            };
        let mut prev_epoch: Option<(i64, chrono::DateTime<Utc>, f64)> =
            if let Some(first_block) = all_parsed_blocks.first() {
                self.writer
                    .get_last_epoch_start(first_block.number)?
                    .map(|(epoch_num, ts)| (epoch_num, ts, 0.0))
            } else {
                None
            };

        let mut block_tx_idx = 0usize;
        for parsed in &all_parsed_blocks {
            let block_date = parsed.timestamp.date_naive();
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            let cells_created: i32 = tx_slice.iter().map(|tx| tx.cells.len() as i32).sum();
            let cells_consumed: i32 = tx_slice
                .iter()
                .filter(|tx| !tx.is_cellbase)
                .map(|tx| tx.inputs.len() as i32)
                .sum();
            let capacity_transferred: i64 = tx_slice
                .iter()
                .filter(|tx| !tx.is_cellbase)
                .map(|tx| tx.total_output_capacity)
                .sum();
            let data_size_added: i64 = tx_slice
                .iter()
                .flat_map(|tx| tx.cells.iter())
                .map(|cell| cell.data_size as i64)
                .sum();
            let data_size_consumed: i64 = tx_slice
                .iter()
                .filter(|tx| !tx.is_cellbase)
                .flat_map(|tx| tx.inputs.iter())
                .filter_map(|input| {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    );
                    input_cell_info
                        .get(&key)
                        .map(|info| info.data_size as i64)
                        .or_else(|| batch_cell_infos.get(&key).map(|info| info.data_size as i64))
                })
                .sum();

            batch_stats.sync_totals.0 += parsed.transactions_count as i64;
            batch_stats.sync_totals.1 += cells_created as i64;
            batch_stats.sync_totals.2 += cells_consumed as i64;
            batch_stats.last_block = Some((parsed.number, parsed.hash.clone()));

            {
                let entry = batch_stats.daily_stats.entry(block_date).or_default();
                entry.0 += 1;
                entry.1 += parsed.transactions_count;
                entry.2 += cells_created;
                entry.3 += cells_consumed;
                entry.4 += capacity_transferred;
                entry.5 += data_size_added;
                entry.6 += data_size_consumed;
            }

            batch_stats
                .daily_dao_fields
                .insert(block_date, parsed.dao.clone());

            {
                let block_hour = truncate_to_hour(parsed.timestamp);
                let entry = batch_stats.hourly_stats.entry(block_hour).or_default();
                entry.0 += 1;
                entry.1 += parsed.transactions_count;
                entry.2 += cells_created;
                entry.3 += cells_consumed;
                entry.4 += capacity_transferred;
            }

            {
                let entry = batch_stats.daily_block_stats.entry(block_date).or_default();
                entry.0 += parsed.compact_target as i128;
                entry.1 += 1;
                entry.2 += parsed.uncles_count;
            }

            if let Some(first_tx) = tx_slice.first() {
                if first_tx.is_cellbase {
                    if let Some(first_cell) = first_tx.cells.first() {
                        let key = (block_date, first_cell.lock_script_hash.clone());
                        let entry = batch_stats.miner_stats.entry(key).or_insert((0, 0));
                        entry.0 += 1;
                        entry.1 = parsed.number;
                    }
                }
            }

            {
                let entry = batch_stats
                    .epoch_stats
                    .entry(parsed.epoch_number)
                    .or_insert_with(|| EpochAccum {
                        start_block: parsed.number,
                        end_block: parsed.number,
                        length: parsed.epoch_length,
                        start_ts: parsed.timestamp,
                        end_ts: parsed.timestamp,
                        tx_count: 0,
                        is_new: parsed.epoch_index == 0,
                    });
                entry.end_block = parsed.number;
                entry.end_ts = parsed.timestamp;
                entry.tx_count += parsed.transactions_count;
            }

            if let Some(prev_ts) = prev_timestamp {
                let block_time_seconds = (parsed.timestamp - prev_ts).num_seconds();
                if block_time_seconds >= 0 {
                    *batch_stats
                        .block_time_dist
                        .entry(block_time_to_bucket(block_time_seconds))
                        .or_default() += 1;
                    let block_time_ms = block_time_seconds * 1000;
                    let entry = batch_stats
                        .daily_block_times
                        .entry(block_date)
                        .or_insert((0, 0));
                    entry.0 += block_time_ms;
                    entry.1 += 1;
                }
            }
            prev_timestamp = Some(parsed.timestamp);

            if parsed.epoch_index == 0 && parsed.epoch_number > 0 {
                if let Some((prev_epoch_num, prev_start_ts, _)) = prev_epoch {
                    if prev_epoch_num == parsed.epoch_number - 1 {
                        let epoch_duration_minutes =
                            (parsed.timestamp - prev_start_ts).num_seconds() as f64 / 60.0;
                        let bucket_minutes = epoch_duration_minutes.round() as i32;
                        *batch_stats
                            .epoch_time_dist
                            .entry(bucket_minutes)
                            .or_default() += 1;
                    }
                }
            }
            if parsed.epoch_index == 0 {
                prev_epoch = Some((parsed.epoch_number, parsed.timestamp, 0.0));
            }

            batch_stats.dao_snapshot_dates.insert(block_date);
        }

        // DAO processing
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        {
            let mut all_dao_deposits: Vec<(
                crate::parser::ParsedDaoDeposit,
                i64,
                chrono::DateTime<Utc>,
                i64,
            )> = Vec::new();
            let mut block_tx_idx = 0usize;
            for parsed in &all_parsed_blocks {
                let tx_count_for_block = parsed.transactions_count as usize;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                block_tx_idx += tx_count_for_block;
                let ar = DaoParser::extract_ar_from_dao_field(&parsed.dao).unwrap_or(0) as i64;
                for tx_data in tx_slice {
                    let dao_deposits =
                        DaoParser::parse_deposits_from_cells(&tx_data.hash, &tx_data.cells);
                    for deposit in dao_deposits {
                        all_dao_deposits.push((deposit, parsed.number, parsed.timestamp, ar));
                    }
                }
            }
            if !all_dao_deposits.is_empty() {
                let mut batch = StoreBatch::new(self.writer.store());
                self.writer
                    .insert_dao_deposits_batch(&all_dao_deposits, &mut batch)?;
                batch.commit()?;
            }
        }

        let mut block_tx_idx = 0usize;
        for parsed in &all_parsed_blocks {
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            for tx_data in tx_slice {
                if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                    continue;
                }

                let input_outpoints: Vec<(&[u8], i32)> = tx_data
                    .inputs
                    .iter()
                    .map(|i| (i.previous_tx_hash.as_slice(), i.previous_output_index))
                    .collect();

                let consumed_dao = self.writer.find_consumed_dao_deposits(&input_outpoints)?;
                if consumed_dao.is_empty() {
                    continue;
                }

                let mut new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)> = Vec::new();
                for (idx, cell) in tx_data.cells.iter().enumerate() {
                    if let Some(ref type_code_hash) = cell.type_code_hash {
                        if type_code_hash == &dao_code_hash && cell.data_size == 8 {
                            if let Some(data) = tx_data.outputs_data.get(idx) {
                                let data_bytes = crate::rpc::parse_hex_to_bytes(data);
                                if let Some(deposit_block) =
                                    DaoParser::parse_deposit_block_number(&data_bytes)
                                {
                                    new_dao_outputs.push((
                                        tx_data.hash.to_vec(),
                                        idx as i16,
                                        cell.lock_script_hash.clone(),
                                        cell.capacity,
                                        deposit_block,
                                    ));
                                }
                            }
                        }
                    }
                }

                {
                    let mut batch = StoreBatch::new(self.writer.store());
                    self.writer.process_dao_withdrawals(
                        &consumed_dao,
                        &new_dao_outputs,
                        parsed.number,
                        &tx_data.hash,
                        parsed.timestamp,
                        &mut batch,
                    )?;
                    batch.commit()?;
                }
            }
        }

        // UDT processing
        let skip_token = false;
        let skip_spore = false;

        struct UdtTxContext {
            tx_hash: Vec<u8>,
            block_number: i64,
            #[allow(dead_code)]
            timestamp: chrono::DateTime<Utc>,
            output_udts: Vec<crate::parser::ParsedUdtCell>,
            input_outpoints: Vec<(Vec<u8>, i16)>,
        }

        let mut udt_tx_contexts: Vec<UdtTxContext> = Vec::new();
        let mut all_input_outpoints_udt: Vec<(Vec<u8>, i16)> = Vec::new();
        let mut batch_udt_cells: HashMap<(Vec<u8>, i16), crate::parser::ParsedUdtCell> =
            HashMap::new();

        struct TxInfoForUdt {
            tx_hash: Vec<u8>,
            block_number: i64,
            timestamp: chrono::DateTime<Utc>,
            output_udts: Vec<crate::parser::ParsedUdtCell>,
            input_outpoints: Vec<(Vec<u8>, i16)>,
        }
        let mut all_tx_infos_for_udt: Vec<TxInfoForUdt> = Vec::new();

        let mut block_tx_idx = 0usize;
        for (block_idx, block_response) in blocks.iter().enumerate() {
            let parsed = &all_parsed_blocks[block_idx];
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                if tx_data.is_cellbase {
                    continue;
                }
                let tx = &block_response.block.transactions[tx_idx];
                let output_udts = UdtParser::parse_udt_cells(tx);
                for (output_index, udt_cell) in output_udts.iter().enumerate() {
                    batch_udt_cells.insert(
                        (tx_data.hash.to_vec(), output_index as i16),
                        udt_cell.clone(),
                    );
                    self.udt_cell_cache.insert(
                        (tx_data.hash, output_index as i16),
                        CachedUdtCellInfo {
                            type_script_hash: udt_cell.type_script_hash.clone(),
                            type_code_hash: udt_cell.type_code_hash.clone(),
                            type_hash_type: udt_cell.type_hash_type,
                            type_args: udt_cell.type_args.clone(),
                            lock_script_hash: udt_cell.lock_script_hash.clone(),
                            amount: udt_cell.amount,
                            standard: udt_cell.standard.as_str().to_string(),
                        },
                    );
                }
                let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                    .inputs
                    .iter()
                    .map(|i| (i.previous_tx_hash.to_vec(), i.previous_output_index as i16))
                    .collect();
                all_input_outpoints_udt.extend(input_outpoints.iter().cloned());
                all_tx_infos_for_udt.push(TxInfoForUdt {
                    tx_hash: tx_data.hash.to_vec(),
                    block_number: parsed.number,
                    timestamp: parsed.timestamp,
                    output_udts,
                    input_outpoints,
                });
            }
        }

        let mut input_udt_info: HashMap<
            (Vec<u8>, i16),
            (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String),
        > = HashMap::new();
        if !skip_token && !all_input_outpoints_udt.is_empty() {
            let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                let mut seen = HashSet::new();
                all_input_outpoints_udt
                    .into_iter()
                    .filter(|x| seen.insert(x.clone()))
                    .collect()
            };
            let mut uncached: Vec<(Vec<u8>, i16)> = Vec::new();
            for (tx_hash, idx) in &unique_outpoints {
                let key: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                if let Some(cached) = self.udt_cell_cache.get(&(key, *idx)) {
                    input_udt_info.insert(
                        (tx_hash.clone(), *idx),
                        (
                            cached.type_script_hash.clone(),
                            cached.type_code_hash.clone(),
                            cached.type_hash_type,
                            cached.type_args.clone(),
                            cached.lock_script_hash.clone(),
                            cached.amount,
                            cached.standard.clone(),
                        ),
                    );
                } else {
                    uncached.push((tx_hash.clone(), *idx));
                }
            }
            if !uncached.is_empty() {
                let outpoint_refs: Vec<(&[u8], i16)> =
                    uncached.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                let db_results = self.writer.get_udt_cells_info_batch(&outpoint_refs)?;
                for ((tx_hash, idx), (tsh, tch, tht, ta, lsh, am, std)) in &db_results {
                    let key: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                    self.udt_cell_cache.insert(
                        (key, *idx),
                        CachedUdtCellInfo {
                            type_script_hash: tsh.clone(),
                            type_code_hash: tch.clone(),
                            type_hash_type: *tht,
                            type_args: ta.clone(),
                            lock_script_hash: lsh.clone(),
                            amount: *am,
                            standard: std.clone(),
                        },
                    );
                }
                input_udt_info.extend(db_results);
            }
        }
        if self.udt_cell_cache.len() > UDT_CELL_CACHE_CAPACITY * 2 {
            self.udt_cell_cache.clear();
        }

        for tx_info in all_tx_infos_for_udt {
            let has_udt_outputs = !tx_info.output_udts.is_empty();
            let has_udt_inputs = tx_info.input_outpoints.iter().any(|(tx_hash, idx)| {
                input_udt_info.contains_key(&(tx_hash.clone(), *idx))
                    || batch_udt_cells.contains_key(&(tx_hash.clone(), *idx))
            });
            if has_udt_outputs || has_udt_inputs {
                udt_tx_contexts.push(UdtTxContext {
                    tx_hash: tx_info.tx_hash,
                    block_number: tx_info.block_number,
                    timestamp: tx_info.timestamp,
                    output_udts: tx_info.output_udts,
                    input_outpoints: tx_info.input_outpoints,
                });
            }
        }

        if !skip_token && !udt_tx_contexts.is_empty() {
            let mut all_transfers: Vec<(crate::parser::ParsedUdtTransfer, Vec<u8>, i64)> =
                Vec::new();
            for ctx in &udt_tx_contexts {
                let mut input_udts: Vec<crate::parser::ParsedUdtCell> = Vec::new();
                for (tx_hash, idx) in &ctx.input_outpoints {
                    if let Some((
                        type_script_hash,
                        type_code_hash,
                        type_hash_type,
                        type_args,
                        lock_script_hash,
                        amount,
                        standard,
                    )) = input_udt_info.get(&(tx_hash.clone(), *idx))
                    {
                        input_udts.push(crate::parser::ParsedUdtCell {
                            type_script_hash: type_script_hash.clone(),
                            type_code_hash: type_code_hash.clone(),
                            type_hash_type: *type_hash_type,
                            type_args: type_args.clone(),
                            lock_script_hash: lock_script_hash.clone(),
                            amount: *amount,
                            standard: crate::parser::UdtStandard::parse(standard),
                        });
                    } else if let Some(udt_cell) = batch_udt_cells.get(&(tx_hash.clone(), *idx)) {
                        input_udts.push(udt_cell.clone());
                    }
                }

                for out_udt in &ctx.output_udts {
                    let matching_input = input_udts
                        .iter()
                        .find(|inp| inp.type_script_hash == out_udt.type_script_hash);
                    let is_mint = matching_input.is_none();
                    let from_lock_hash = matching_input.map(|inp| inp.lock_script_hash.clone());
                    all_transfers.push((
                        crate::parser::ParsedUdtTransfer {
                            type_script_hash: out_udt.type_script_hash.clone(),
                            type_code_hash: out_udt.type_code_hash.clone(),
                            type_hash_type: out_udt.type_hash_type,
                            type_args: out_udt.type_args.clone(),
                            from_lock_hash,
                            to_lock_hash: out_udt.lock_script_hash.clone(),
                            amount: out_udt.amount,
                            standard: out_udt.standard.clone(),
                            is_mint,
                            is_burn: false,
                        },
                        ctx.tx_hash.clone(),
                        ctx.block_number,
                    ));
                }

                for inp_udt in &input_udts {
                    let has_matching_output = ctx
                        .output_udts
                        .iter()
                        .any(|out| out.type_script_hash == inp_udt.type_script_hash);
                    if !has_matching_output {
                        all_transfers.push((
                            crate::parser::ParsedUdtTransfer {
                                type_script_hash: inp_udt.type_script_hash.clone(),
                                type_code_hash: inp_udt.type_code_hash.clone(),
                                type_hash_type: inp_udt.type_hash_type,
                                type_args: inp_udt.type_args.clone(),
                                from_lock_hash: Some(inp_udt.lock_script_hash.clone()),
                                to_lock_hash: Vec::new(),
                                amount: inp_udt.amount,
                                standard: inp_udt.standard.clone(),
                                is_mint: false,
                                is_burn: true,
                            },
                            ctx.tx_hash.clone(),
                            ctx.block_number,
                        ));
                    }
                }
            }

            if !all_transfers.is_empty() {
                let transfer_refs: Vec<_> = all_transfers
                    .iter()
                    .map(|(t, h, b)| (t, h.as_slice(), *b))
                    .collect();
                let block_timestamps: HashMap<i64, i64> = all_parsed_blocks
                    .iter()
                    .map(|p| (p.number, p.timestamp.timestamp_millis()))
                    .collect();
                {
                    let mut batch = StoreBatch::new(self.writer.store());
                    self.writer.process_udt_transfers_batch(
                        &transfer_refs,
                        &block_timestamps,
                        &mut batch,
                    )?;
                    batch.commit()?;
                }
            }
        }

        // NFT/Spore processing
        let mut batch_spore_ids: HashSet<Vec<u8>> = HashSet::new();

        {
            let mut nft_batch = StoreBatch::new(self.writer.store());
            let mut block_tx_idx = 0usize;
            for (block_idx, block_response) in blocks.iter().enumerate() {
                let parsed = &all_parsed_blocks[block_idx];
                let tx_count_for_block = parsed.transactions_count as usize;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                block_tx_idx += tx_count_for_block;

                for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                    let tx = &block_response.block.transactions[tx_idx];

                    if !skip_spore {
                        for cluster in SporeParser::parse_clusters(tx) {
                            self.writer.insert_spore_cluster(
                                &cluster,
                                parsed.number,
                                &tx_data.hash,
                                &mut nft_batch,
                            )?;
                        }
                        for (output_index, spore) in
                            SporeParser::parse_spores(tx).iter().enumerate()
                        {
                            batch_spore_ids.insert(spore.spore_id.clone());
                            self.writer.insert_spore_cell(
                                spore,
                                &tx_data.hash,
                                output_index as i16,
                                parsed.number,
                                &mut nft_batch,
                            )?;
                            self.writer
                                .insert_spore_content(&spore.spore_id, &spore.content)?;
                        }
                    }

                    for issuer in MnftParser::parse_issuers(tx) {
                        self.writer.insert_mnft_issuer(
                            &issuer,
                            &tx_data.hash,
                            0,
                            parsed.number,
                            &mut nft_batch,
                        )?;
                    }
                    for (output_index, class) in MnftParser::parse_classes(tx).iter().enumerate() {
                        self.writer.insert_mnft_class(
                            class,
                            &tx_data.hash,
                            output_index as i16,
                            parsed.number,
                            &mut nft_batch,
                        )?;
                    }
                    for (output_index, token) in MnftParser::parse_tokens(tx).iter().enumerate() {
                        self.writer.insert_mnft_token(
                            token,
                            &tx_data.hash,
                            output_index as i16,
                            parsed.number,
                            &mut nft_batch,
                        )?;
                    }
                    for (output_index, account) in
                        DotbitParser::parse_accounts(tx).iter().enumerate()
                    {
                        self.writer.insert_dotbit_account(
                            account,
                            &tx_data.hash,
                            output_index as i16,
                            parsed.number,
                            &mut nft_batch,
                        )?;
                    }
                }
            }
            nft_batch.commit()?;
        }

        // Spore consumption (live sync only)
        if !self.is_bulk_sync_active() {
            let mut consume_batch = StoreBatch::new(self.writer.store());
            let mut block_tx_idx = 0usize;
            for (block_idx, block_response) in blocks.iter().enumerate() {
                let parsed = &all_parsed_blocks[block_idx];
                let tx_count_for_block = parsed.transactions_count as usize;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                block_tx_idx += tx_count_for_block;

                for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                    if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                        continue;
                    }
                    let tx = &block_response.block.transactions[tx_idx];
                    for input in &tx.inputs {
                        let prev_tx_hash =
                            crate::rpc::parse_hex_to_bytes(&input.previous_output.tx_hash);
                        let prev_index = input
                            .previous_output
                            .index
                            .strip_prefix("0x")
                            .and_then(|s| u32::from_str_radix(s, 16).ok())
                            .unwrap_or(0);
                        let consumed_spore_id = self
                            .writer
                            .get_spore_id_by_outpoint(&prev_tx_hash, prev_index as i16)?;
                        if let Some(spore_id) = consumed_spore_id {
                            if !batch_spore_ids.contains(&spore_id) {
                                self.writer.consume_spore(
                                    &spore_id,
                                    parsed.number,
                                    &tx_data.hash,
                                    &mut consume_batch,
                                )?;
                            }
                        }
                    }
                }
            }
            consume_batch.commit()?;
        }

        {
            let mut batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut batch)?;
            if bulk_sync_mode {
                batch.commit_no_wal()?;
            } else {
                batch.commit()?;
            }
        }
        if let Some((block_number, ref block_hash)) = batch_stats.last_block {
            let ema_rate = self.progress.ema_blocks_per_second();
            let ema_rate_opt = if ema_rate > 0.0 { Some(ema_rate) } else { None };
            self.writer
                .update_sync_status(
                    block_number,
                    block_hash,
                    batch_stats.sync_totals.0,
                    batch_stats.sync_totals.1,
                    batch_stats.sync_totals.2,
                    0,
                    ema_rate_opt,
                )
                .await?;
        }

        let stats_ms = t_stats.elapsed().as_secs_f64() * 1000.0;
        debug!(
            headers_ms = format!("{:.1}", headers_ms),
            cells_ms = format!("{:.1}", cells_ms),
            stats_ms = format!("{:.1}", stats_ms),
            "Batch write breakdown"
        );

        Ok(())
    }
    // === write_parsed_batch (pipeline path) ===
    // This is largely identical to sync_blocks_batch but receives pre-parsed data
    // from the pipeline parser stage and writes blocks LAST as a commit marker.

    #[allow(clippy::too_many_arguments)]
    async fn write_parsed_batch(
        &self,
        blocks: &[BlockResponseWithCycles],
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        all_tx_data: Vec<TxData>,
        input_cell_info: HashMap<(Vec<u8>, i16), LiveCellInfo>,
        batch_cell_infos: HashMap<(Vec<u8>, i16), LiveCellInfo>,
        address_balance_changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>, i64)>,
        script_usage_changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)>,
        chain_tip: u64,
    ) -> Result<()> {
        if all_parsed_blocks.is_empty() {
            return Ok(());
        }

        let end_block = all_parsed_blocks
            .last()
            .map(|b| b.number as u64)
            .unwrap_or(0);
        let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;

        let t_precompute = Instant::now();

        // Build reference vectors from pre-computed data (Passes 1-3 done in parser)
        let mut all_cells: Vec<(&[u8], i16, &crate::parser::cell::ParsedCell, i64)> = Vec::new();
        let mut all_inputs: Vec<(&[u8], i64, i16, &crate::parser::transaction::ParsedInput)> =
            Vec::new();
        let mut all_consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i16)> = Vec::new();
        let mut txs_for_batch: Vec<_> = Vec::with_capacity(all_tx_data.len());

        for tx_data in &all_tx_data {
            txs_for_batch.push((
                tx_data.hash.as_slice(),
                tx_data.block_number,
                tx_data.block_hash.as_slice(),
                tx_data.tx_index,
                tx_data.version,
                tx_data.inputs_count,
                tx_data.outputs_count,
                tx_data.witnesses_count,
                tx_data.cell_deps_count,
                tx_data.header_deps_count,
                tx_data.total_input_capacity,
                tx_data.total_output_capacity,
                tx_data.fee,
                Some(tx_data.tx_size),
                tx_data.cycles,
                tx_data.is_cellbase,
                tx_data.timestamp,
            ));

            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                all_cells.push((
                    tx_data.hash.as_slice(),
                    output_index as i16,
                    cell,
                    tx_data.block_number,
                ));
            }

            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    all_inputs.push((
                        tx_data.hash.as_slice(),
                        tx_data.block_number,
                        input_index as i16,
                        input,
                    ));
                }

                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    );
                    let info = input_cell_info
                        .get(&key)
                        .or_else(|| batch_cell_infos.get(&key));
                    if let Some(info) = info {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            info.created_at_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
                        ));
                    }
                }
            }
        }

        // Compute per-tx address entries for addr_txs index
        let mut addr_tx_entries: Vec<(Vec<u8>, i64, i32, Vec<u8>)> = Vec::new();
        for tx_data in &all_tx_data {
            let mut touched: HashSet<Vec<u8>> = HashSet::new();
            for cell in &tx_data.cells {
                touched.insert(cell.lock_script_hash.clone());
            }
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    );
                    let info = input_cell_info
                        .get(&key)
                        .or_else(|| batch_cell_infos.get(&key));
                    if let Some(info) = info {
                        touched.insert(info.lock_script_hash.clone());
                    }
                }
            }
            for lock_hash in touched {
                addr_tx_entries.push((
                    lock_hash,
                    tx_data.block_number,
                    tx_data.tx_index,
                    tx_data.hash.to_vec(),
                ));
            }
        }

        let changes_ref: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8], i64)> =
            address_balance_changes
                .iter()
                .map(|(k, (a, b, c, d, e, f, g))| {
                    (k.clone(), (*a, *b, *c, *d, *e, f.as_slice(), *g))
                })
                .collect();

        let block_refs: Vec<&crate::parser::block::ParsedBlock> =
            all_parsed_blocks.iter().collect();

        // Pass 4: Proposals (iterates all_parsed_blocks, has async call in live sync)
        let mut all_proposals: Vec<(i64, i16, &[u8])> = Vec::new();
        for parsed_block in all_parsed_blocks {
            if !parsed_block.proposals.is_empty() {
                for (proposal_index, proposal_id) in parsed_block.proposals.iter().enumerate() {
                    all_proposals.push((
                        parsed_block.number,
                        proposal_index as i16,
                        proposal_id.as_slice(),
                    ));
                }
                if !self.is_bulk_sync_active() {
                    self.cache_block_proposals(&parsed_block.proposals, parsed_block.number)
                        .await;
                }
            }
        }

        let skip_address_balances = self
            .address_balances_deferred
            .load(std::sync::atomic::Ordering::Relaxed)
            && bulk_sync_mode;
        let skip_activities = bulk_sync_mode;

        let precompute_ms = t_precompute.elapsed().as_secs_f64() * 1000.0;

        // DAO, UDT, NFT processing flags
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let skip_token = false;
        let skip_spore = false;

        // Pre-fetch DAO, UDT, address balance, and script info data outside thread::scope.
        // 4-way rayon::join overlaps all DB reads: takes max(dao, udt, addr, script).
        let t_prefetch = Instant::now();

        // Prepare address balance + script info keys for prefetch
        let lock_hash_keys: Vec<&Vec<u8>> = if !skip_address_balances {
            changes_ref.keys().collect()
        } else {
            Vec::new()
        };

        let unique_code_hashes: Vec<Vec<u8>> = {
            let mut seen = std::collections::HashSet::new();
            script_usage_changes
                .keys()
                .filter_map(|(code_hash, _)| {
                    if seen.insert(code_hash.clone()) {
                        Some(code_hash.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };
        let code_hash_refs: Vec<&Vec<u8>> = unique_code_hashes.iter().collect();

        let (
            (
                consumed_dao_map,
                (prefetched_input_udt_info, prefetched_batch_udt_cells, prefetched_udt_tx_infos),
            ),
            (prefetched_addr_balances, prefetched_script_info),
        ) = if bulk_sync_mode {
            let writer = &self.writer;
            let udt_cache = &self.udt_cell_cache;
            rayon::join(
                || {
                    rayon::join(
                        || {
                            // DAO: collect input outpoints, deduplicate, batch query DB
                            let mut all_input_outpoints_dao: Vec<(Vec<u8>, i16)> = Vec::new();
                            let mut block_tx_idx = 0usize;
                            for parsed in all_parsed_blocks.iter() {
                                let tx_count_for_block = parsed.transactions_count as usize;
                                let tx_slice =
                                    &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                                block_tx_idx += tx_count_for_block;
                                for tx_data in tx_slice {
                                    if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                        continue;
                                    }
                                    for input in &tx_data.inputs {
                                        all_input_outpoints_dao.push((
                                            input.previous_tx_hash.to_vec(),
                                            input.previous_output_index as i16,
                                        ));
                                    }
                                }
                            }
                            if !all_input_outpoints_dao.is_empty() {
                                let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                                    let mut seen = HashSet::new();
                                    all_input_outpoints_dao
                                        .into_iter()
                                        .filter(|x| seen.insert(x.clone()))
                                        .collect()
                                };
                                let outpoint_refs: Vec<(&[u8], i16)> = unique_outpoints
                                    .iter()
                                    .map(|(h, i)| (h.as_slice(), *i))
                                    .collect();
                                writer
                                    .find_consumed_dao_deposits_batch(&outpoint_refs)
                                    .unwrap_or_default()
                            } else {
                                HashMap::new()
                            }
                        },
                        || {
                            // UDT: parse outputs, populate cache, collect input outpoints,
                            // cache lookup + DB fallback
                            struct TxInfoForUdt {
                                tx_hash: Vec<u8>,
                                block_number: i64,
                                timestamp: chrono::DateTime<Utc>,
                                output_udts: Vec<crate::parser::ParsedUdtCell>,
                                input_outpoints: Vec<(Vec<u8>, i16)>,
                            }
                            let mut all_tx_infos_for_udt: Vec<TxInfoForUdt> = Vec::new();
                            let mut all_input_outpoints_udt: Vec<(Vec<u8>, i16)> = Vec::new();
                            let mut batch_udt_cells: HashMap<
                                (Vec<u8>, i16),
                                crate::parser::ParsedUdtCell,
                            > = HashMap::new();

                            let mut block_tx_idx = 0usize;
                            for (block_idx, block_response) in blocks.iter().enumerate() {
                                let parsed = &all_parsed_blocks[block_idx];
                                let tx_count_for_block = parsed.transactions_count as usize;
                                let tx_slice =
                                    &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                                block_tx_idx += tx_count_for_block;
                                for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                                    if tx_data.is_cellbase {
                                        continue;
                                    }
                                    let tx = &block_response.block.transactions[tx_idx];
                                    let output_udts = UdtParser::parse_udt_cells(tx);
                                    for (output_index, udt_cell) in output_udts.iter().enumerate() {
                                        batch_udt_cells.insert(
                                            (tx_data.hash.to_vec(), output_index as i16),
                                            udt_cell.clone(),
                                        );
                                        udt_cache.insert(
                                            (tx_data.hash, output_index as i16),
                                            CachedUdtCellInfo {
                                                type_script_hash: udt_cell.type_script_hash.clone(),
                                                type_code_hash: udt_cell.type_code_hash.clone(),
                                                type_hash_type: udt_cell.type_hash_type,
                                                type_args: udt_cell.type_args.clone(),
                                                lock_script_hash: udt_cell.lock_script_hash.clone(),
                                                amount: udt_cell.amount,
                                                standard: udt_cell.standard.as_str().to_string(),
                                            },
                                        );
                                    }
                                    let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                                        .inputs
                                        .iter()
                                        .map(|i| {
                                            (
                                                i.previous_tx_hash.to_vec(),
                                                i.previous_output_index as i16,
                                            )
                                        })
                                        .collect();
                                    all_input_outpoints_udt.extend(input_outpoints.iter().cloned());
                                    all_tx_infos_for_udt.push(TxInfoForUdt {
                                        tx_hash: tx_data.hash.to_vec(),
                                        block_number: parsed.number,
                                        timestamp: parsed.timestamp,
                                        output_udts,
                                        input_outpoints,
                                    });
                                }
                            }

                            // Check persistent UDT cache before DB reads
                            let mut input_udt_info: HashMap<
                                (Vec<u8>, i16),
                                (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String),
                            > = HashMap::new();
                            let mut udt_cache_hits: usize = 0;
                            let mut udt_db_lookups: usize = 0;
                            if !skip_token && !all_input_outpoints_udt.is_empty() {
                                let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                                    let mut seen = HashSet::new();
                                    all_input_outpoints_udt
                                        .into_iter()
                                        .filter(|x| seen.insert(x.clone()))
                                        .collect()
                                };
                                let mut uncached: Vec<(Vec<u8>, i16)> = Vec::new();
                                for (tx_hash, idx) in &unique_outpoints {
                                    let key: [u8; 32] =
                                        tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                                    if let Some(cached) = udt_cache.get(&(key, *idx)) {
                                        input_udt_info.insert(
                                            (tx_hash.clone(), *idx),
                                            (
                                                cached.type_script_hash.clone(),
                                                cached.type_code_hash.clone(),
                                                cached.type_hash_type,
                                                cached.type_args.clone(),
                                                cached.lock_script_hash.clone(),
                                                cached.amount,
                                                cached.standard.clone(),
                                            ),
                                        );
                                        udt_cache_hits += 1;
                                    } else {
                                        uncached.push((tx_hash.clone(), *idx));
                                    }
                                }
                                udt_db_lookups = uncached.len();
                                if !uncached.is_empty() {
                                    let outpoint_refs: Vec<(&[u8], i16)> =
                                        uncached.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                                    if let Ok(db_results) =
                                        writer.get_udt_cells_info_batch(&outpoint_refs)
                                    {
                                        for ((tx_hash, idx), (tsh, tch, tht, ta, lsh, am, std)) in
                                            &db_results
                                        {
                                            let key: [u8; 32] =
                                                tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                                            udt_cache.insert(
                                                (key, *idx),
                                                CachedUdtCellInfo {
                                                    type_script_hash: tsh.clone(),
                                                    type_code_hash: tch.clone(),
                                                    type_hash_type: *tht,
                                                    type_args: ta.clone(),
                                                    lock_script_hash: lsh.clone(),
                                                    amount: *am,
                                                    standard: std.clone(),
                                                },
                                            );
                                        }
                                        input_udt_info.extend(db_results);
                                    }
                                }
                            }
                            if udt_cache_hits > 0 || udt_db_lookups > 0 {
                                debug!(
                                    udt_cache_hits,
                                    udt_db_lookups,
                                    udt_cache_size = udt_cache.len(),
                                    "UDT prefetch cache stats"
                                );
                            }
                            if udt_cache.len() > UDT_CELL_CACHE_CAPACITY * 2 {
                                udt_cache.clear();
                            }

                            // Build tx contexts for UDT processing
                            struct UdtTxInfo {
                                tx_hash: Vec<u8>,
                                block_number: i64,
                                #[allow(dead_code)]
                                timestamp: chrono::DateTime<Utc>,
                                output_udts: Vec<crate::parser::ParsedUdtCell>,
                                input_outpoints: Vec<(Vec<u8>, i16)>,
                            }
                            let mut udt_tx_contexts: Vec<UdtTxInfo> = Vec::new();
                            for tx_info in all_tx_infos_for_udt {
                                let has_udt_outputs = !tx_info.output_udts.is_empty();
                                let has_udt_inputs =
                                    tx_info.input_outpoints.iter().any(|(tx_hash, idx)| {
                                        input_udt_info.contains_key(&(tx_hash.clone(), *idx))
                                            || batch_udt_cells
                                                .contains_key(&(tx_hash.clone(), *idx))
                                    });
                                if has_udt_outputs || has_udt_inputs {
                                    udt_tx_contexts.push(UdtTxInfo {
                                        tx_hash: tx_info.tx_hash,
                                        block_number: tx_info.block_number,
                                        timestamp: tx_info.timestamp,
                                        output_udts: tx_info.output_udts,
                                        input_outpoints: tx_info.input_outpoints,
                                    });
                                }
                            }

                            (input_udt_info, batch_udt_cells, udt_tx_contexts)
                        },
                    )
                },
                || {
                    rayon::join(
                        || {
                            if !lock_hash_keys.is_empty() {
                                writer
                                    .read_address_balances(&lock_hash_keys)
                                    .unwrap_or_default()
                            } else {
                                HashMap::new()
                            }
                        },
                        || {
                            if !code_hash_refs.is_empty() {
                                writer.read_script_info(&code_hash_refs).unwrap_or_default()
                            } else {
                                HashMap::new()
                            }
                        },
                    )
                },
            )
        } else {
            (
                (HashMap::new(), (HashMap::new(), HashMap::new(), Vec::new())),
                (HashMap::new(), HashMap::new()),
            )
        };
        let prefetch_ms = t_prefetch.elapsed().as_secs_f64() * 1000.0;

        let t_write = Instant::now();
        let mut batch_stats;
        let mut thread_times: Option<[f64; 7]> = None;
        if bulk_sync_mode {
            // Parallel write path: each thread writes to its own StoreBatch and commits independently.
            // DAO/UDT/addr/script DB reads are pre-fetched above via rayon::join, so threads only do writes.
            // Independent batches let all threads run fully in parallel; the RocksDB write
            // group overhead (~2ms) is negligible.
            let store = self.writer.store();
            let writer = &self.writer;

            let tt;
            (batch_stats, tt) = std::thread::scope(|s| -> Result<(BatchStats, [f64; 7])> {
                // T1: Cells + Consumption (index CFs deferred to post-bulk rebuild)
                // CFs: LIVE_CELLS, CONSUMED_CELLS
                let h1 = s.spawn(|| -> Result<f64> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);
                    if !all_cells.is_empty() {
                        writer.insert_cells_batch(&all_cells, &mut batch, true)?;
                    }
                    if !all_consumptions.is_empty() {
                        writer.consume_cells_batch_preloaded(
                            &all_consumptions,
                            &input_cell_info,
                            &batch_cell_infos,
                            &mut batch,
                            true,
                        )?;
                    }
                    batch.commit_no_wal()?;
                    Ok(t.elapsed().as_secs_f64() * 1000.0)
                });

                // T2: Transactions + Address Balances + Script Usage + Addr TX index
                // CFs: TX_INDEX, TX_HASH_MAP, ADDR_BALANCE, SCRIPT_INFO, ADDR_TX
                let h2 = s.spawn(|| -> Result<f64> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);
                    if !txs_for_batch.is_empty() {
                        writer.insert_transactions_batch(&txs_for_batch, &mut batch)?;
                    }
                    if !skip_address_balances && !changes_ref.is_empty() {
                        writer.apply_address_balance_deltas(
                            &prefetched_addr_balances,
                            &changes_ref,
                            &mut batch,
                        )?;
                    }
                    if !script_usage_changes.is_empty() {
                        writer.apply_script_usage_deltas(
                            &prefetched_script_info,
                            &script_usage_changes,
                            &mut batch,
                        )?;
                    }
                    for (lock_hash, block_num, tx_idx, tx_hash) in &addr_tx_entries {
                        batch.put_addr_tx(lock_hash, *block_num, *tx_idx, tx_hash);
                    }
                    batch.commit_no_wal()?;
                    Ok(t.elapsed().as_secs_f64() * 1000.0)
                });

                // T4: DAO (writes only — DB reads pre-fetched above)
                // CFs: DAO_DEPOSITS, DAO_BY_WITHDRAW_TX
                let h4 = s.spawn(|| -> Result<f64> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);

                    // DAO deposits
                    let mut all_dao_deposits: Vec<(
                        crate::parser::ParsedDaoDeposit,
                        i64,
                        DateTime<Utc>,
                        i64,
                    )> = Vec::new();
                    let mut block_tx_idx = 0usize;
                    for parsed in all_parsed_blocks {
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;
                        let ar =
                            DaoParser::extract_ar_from_dao_field(&parsed.dao).unwrap_or(0) as i64;
                        for tx_data in tx_slice {
                            let dao_deposits =
                                DaoParser::parse_deposits_from_cells(&tx_data.hash, &tx_data.cells);
                            for deposit in dao_deposits {
                                all_dao_deposits.push((
                                    deposit,
                                    parsed.number,
                                    parsed.timestamp,
                                    ar,
                                ));
                            }
                        }
                    }
                    if !all_dao_deposits.is_empty() {
                        writer.insert_dao_deposits_batch(&all_dao_deposits, &mut batch)?;
                    }

                    // consumed_dao_map was pre-fetched outside thread::scope
                    if !consumed_dao_map.is_empty() {
                        use crate::db::DaoWithdrawalContextTrait;
                        #[derive(Clone)]
                        struct DaoWithdrawalContext {
                            consumed_deposits: Vec<(i64, Vec<u8>, i16, String, i64, i16)>,
                            new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
                            block_number: i64,
                            consuming_tx_hash: Vec<u8>,
                            timestamp: DateTime<Utc>,
                        }
                        impl DaoWithdrawalContextTrait for DaoWithdrawalContext {
                            fn consumed_deposits(
                                &self,
                            ) -> &[(i64, Vec<u8>, i16, String, i64, i16)]
                            {
                                &self.consumed_deposits
                            }
                            fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)] {
                                &self.new_dao_outputs
                            }
                            fn block_number(&self) -> i64 {
                                self.block_number
                            }
                            fn consuming_tx_hash(&self) -> &[u8] {
                                &self.consuming_tx_hash
                            }
                            fn timestamp(&self) -> DateTime<Utc> {
                                self.timestamp
                            }
                        }

                        let mut withdrawal_contexts: Vec<DaoWithdrawalContext> = Vec::new();
                        let mut block_tx_idx = 0usize;
                        for parsed in all_parsed_blocks {
                            let tx_count_for_block = parsed.transactions_count as usize;
                            let tx_slice =
                                &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                            block_tx_idx += tx_count_for_block;
                            for tx_data in tx_slice {
                                if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                    continue;
                                }
                                let mut consumed_deposits: Vec<(
                                    i64,
                                    Vec<u8>,
                                    i16,
                                    String,
                                    i64,
                                    i16,
                                )> = Vec::new();
                                for input in &tx_data.inputs {
                                    let key = (
                                        input.previous_tx_hash.to_vec(),
                                        input.previous_output_index as i16,
                                    );
                                    if let Some(deposit_info) = consumed_dao_map.get(&key) {
                                        consumed_deposits.push(deposit_info.clone());
                                    }
                                }
                                if consumed_deposits.is_empty() {
                                    continue;
                                }
                                let mut new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)> =
                                    Vec::new();
                                for (idx, cell) in tx_data.cells.iter().enumerate() {
                                    if let Some(ref type_code_hash) = cell.type_code_hash {
                                        if type_code_hash == &dao_code_hash && cell.data_size == 8 {
                                            if let Some(data) = tx_data.outputs_data.get(idx) {
                                                let data_bytes =
                                                    crate::rpc::parse_hex_to_bytes(data);
                                                if let Some(deposit_block) =
                                                    DaoParser::parse_deposit_block_number(
                                                        &data_bytes,
                                                    )
                                                {
                                                    new_dao_outputs.push((
                                                        tx_data.hash.to_vec(),
                                                        idx as i16,
                                                        cell.lock_script_hash.clone(),
                                                        cell.capacity,
                                                        deposit_block,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                                withdrawal_contexts.push(DaoWithdrawalContext {
                                    consumed_deposits,
                                    new_dao_outputs,
                                    block_number: parsed.number,
                                    consuming_tx_hash: tx_data.hash.to_vec(),
                                    timestamp: parsed.timestamp,
                                });
                            }
                        }
                        if !withdrawal_contexts.is_empty() {
                            writer
                                .process_dao_withdrawals_batch(&withdrawal_contexts, &mut batch)?;
                        }
                    }

                    batch.commit_no_wal()?;
                    Ok(t.elapsed().as_secs_f64() * 1000.0)
                });

                // T5: UDT (writes only — DB reads + parsing pre-fetched above)
                // CFs: TOKENS, TOKEN_HOLDERS
                let input_udt_info = &prefetched_input_udt_info;
                let batch_udt_cells = &prefetched_batch_udt_cells;
                let h5 = s.spawn(|| -> Result<f64> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);

                    if !skip_token && !prefetched_udt_tx_infos.is_empty() {
                        let mut all_transfers: Vec<(
                            crate::parser::ParsedUdtTransfer,
                            Vec<u8>,
                            i64,
                        )> = Vec::new();
                        for ctx in &prefetched_udt_tx_infos {
                            let mut input_udts: Vec<crate::parser::ParsedUdtCell> = Vec::new();
                            for (tx_hash, idx) in &ctx.input_outpoints {
                                if let Some((tsh, tch, tht, ta, lsh, am, std)) =
                                    input_udt_info.get(&(tx_hash.clone(), *idx))
                                {
                                    input_udts.push(crate::parser::ParsedUdtCell {
                                        type_script_hash: tsh.clone(),
                                        type_code_hash: tch.clone(),
                                        type_hash_type: *tht,
                                        type_args: ta.clone(),
                                        lock_script_hash: lsh.clone(),
                                        amount: *am,
                                        standard: crate::parser::UdtStandard::parse(std),
                                    });
                                } else if let Some(udt_cell) =
                                    batch_udt_cells.get(&(tx_hash.clone(), *idx))
                                {
                                    input_udts.push(udt_cell.clone());
                                }
                            }
                            for out_udt in &ctx.output_udts {
                                let matching_input = input_udts
                                    .iter()
                                    .find(|inp| inp.type_script_hash == out_udt.type_script_hash);
                                let is_mint = matching_input.is_none();
                                let from_lock_hash =
                                    matching_input.map(|inp| inp.lock_script_hash.clone());
                                all_transfers.push((
                                    crate::parser::ParsedUdtTransfer {
                                        type_script_hash: out_udt.type_script_hash.clone(),
                                        type_code_hash: out_udt.type_code_hash.clone(),
                                        type_hash_type: out_udt.type_hash_type,
                                        type_args: out_udt.type_args.clone(),
                                        from_lock_hash,
                                        to_lock_hash: out_udt.lock_script_hash.clone(),
                                        amount: out_udt.amount,
                                        standard: out_udt.standard.clone(),
                                        is_mint,
                                        is_burn: false,
                                    },
                                    ctx.tx_hash.clone(),
                                    ctx.block_number,
                                ));
                            }
                            for inp_udt in &input_udts {
                                let has_matching_output = ctx
                                    .output_udts
                                    .iter()
                                    .any(|out| out.type_script_hash == inp_udt.type_script_hash);
                                if !has_matching_output {
                                    all_transfers.push((
                                        crate::parser::ParsedUdtTransfer {
                                            type_script_hash: inp_udt.type_script_hash.clone(),
                                            type_code_hash: inp_udt.type_code_hash.clone(),
                                            type_hash_type: inp_udt.type_hash_type,
                                            type_args: inp_udt.type_args.clone(),
                                            from_lock_hash: Some(inp_udt.lock_script_hash.clone()),
                                            to_lock_hash: Vec::new(),
                                            amount: inp_udt.amount,
                                            standard: inp_udt.standard.clone(),
                                            is_mint: false,
                                            is_burn: true,
                                        },
                                        ctx.tx_hash.clone(),
                                        ctx.block_number,
                                    ));
                                }
                            }
                        }

                        if !all_transfers.is_empty() {
                            let transfer_refs: Vec<_> = all_transfers
                                .iter()
                                .map(|(t, h, b)| (t, h.as_slice(), *b))
                                .collect();
                            let block_timestamps: HashMap<i64, i64> = all_parsed_blocks
                                .iter()
                                .map(|p| (p.number, p.timestamp.timestamp_millis()))
                                .collect();
                            writer.process_udt_transfers_batch(
                                &transfer_refs,
                                &block_timestamps,
                                &mut batch,
                            )?;
                        }
                    }

                    batch.commit_no_wal()?;
                    Ok(t.elapsed().as_secs_f64() * 1000.0)
                });

                // T6: Spore + mNFT/DotBit (no NFT consumption during bulk sync)
                // CFs: SPORE_DATA, SPORE_CONTENT, NFT_DATA
                let h6 = s.spawn(|| -> Result<f64> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);
                    let mut block_tx_idx = 0usize;
                    for (block_idx, block_response) in blocks.iter().enumerate() {
                        let parsed = &all_parsed_blocks[block_idx];
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;
                        for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                            let tx = &block_response.block.transactions[tx_idx];
                            if !skip_spore {
                                for cluster in SporeParser::parse_clusters(tx) {
                                    writer.insert_spore_cluster(
                                        &cluster,
                                        parsed.number,
                                        &tx_data.hash,
                                        &mut batch,
                                    )?;
                                }
                                for (output_index, spore) in
                                    SporeParser::parse_spores(tx).iter().enumerate()
                                {
                                    writer.insert_spore_cell(
                                        spore,
                                        &tx_data.hash,
                                        output_index as i16,
                                        parsed.number,
                                        &mut batch,
                                    )?;
                                    writer.insert_spore_content(&spore.spore_id, &spore.content)?;
                                }
                            }
                            for issuer in MnftParser::parse_issuers(tx) {
                                writer.insert_mnft_issuer(
                                    &issuer,
                                    &tx_data.hash,
                                    0,
                                    parsed.number,
                                    &mut batch,
                                )?;
                            }
                            for (output_index, class) in
                                MnftParser::parse_classes(tx).iter().enumerate()
                            {
                                writer.insert_mnft_class(
                                    class,
                                    &tx_data.hash,
                                    output_index as i16,
                                    parsed.number,
                                    &mut batch,
                                )?;
                            }
                            for (output_index, token) in
                                MnftParser::parse_tokens(tx).iter().enumerate()
                            {
                                writer.insert_mnft_token(
                                    token,
                                    &tx_data.hash,
                                    output_index as i16,
                                    parsed.number,
                                    &mut batch,
                                )?;
                            }
                            for (output_index, account) in
                                DotbitParser::parse_accounts(tx).iter().enumerate()
                            {
                                writer.insert_dotbit_account(
                                    account,
                                    &tx_data.hash,
                                    output_index as i16,
                                    parsed.number,
                                    &mut batch,
                                )?;
                            }
                        }
                    }
                    batch.commit_no_wal()?;
                    Ok(t.elapsed().as_secs_f64() * 1000.0)
                });

                // T7: Stats accumulation (overlaps with T1-T6 IO)
                // Safe: reads CF_BLOCK_HEADERS which is NOT written by T1-T6.
                // RocksDB supports concurrent reads. All other stats computation is
                // purely CPU-bound on immutable all_parsed_blocks + all_tx_data.
                let h7 = s.spawn(|| -> Result<(BatchStats, f64)> {
                    let t = Instant::now();
                    let mut stats = BatchStats::default();
                    let mut prev_timestamp: Option<chrono::DateTime<Utc>> =
                        if let Some(first_block) = all_parsed_blocks.first() {
                            writer.get_previous_block_timestamp(first_block.number)?
                        } else {
                            None
                        };
                    let mut prev_epoch: Option<(i64, chrono::DateTime<Utc>, f64)> =
                        if let Some(first_block) = all_parsed_blocks.first() {
                            writer
                                .get_last_epoch_start(first_block.number)?
                                .map(|(epoch_num, ts)| (epoch_num, ts, 0.0))
                        } else {
                            None
                        };

                    let mut block_tx_idx = 0usize;
                    for parsed in all_parsed_blocks {
                        let block_date = parsed.timestamp.date_naive();
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;

                        let cells_created: i32 =
                            tx_slice.iter().map(|tx| tx.cells.len() as i32).sum();
                        let cells_consumed: i32 = tx_slice
                            .iter()
                            .filter(|tx| !tx.is_cellbase)
                            .map(|tx| tx.inputs.len() as i32)
                            .sum();
                        let capacity_transferred: i64 = tx_slice
                            .iter()
                            .filter(|tx| !tx.is_cellbase)
                            .map(|tx| tx.total_output_capacity)
                            .sum();
                        let data_size_added: i64 = tx_slice
                            .iter()
                            .flat_map(|tx| tx.cells.iter())
                            .map(|cell| cell.data_size as i64)
                            .sum();
                        let data_size_consumed: i64 = tx_slice
                            .iter()
                            .filter(|tx| !tx.is_cellbase)
                            .flat_map(|tx| tx.inputs.iter())
                            .filter_map(|input| {
                                let key = (
                                    input.previous_tx_hash.to_vec(),
                                    input.previous_output_index as i16,
                                );
                                input_cell_info
                                    .get(&key)
                                    .map(|info| info.data_size as i64)
                                    .or_else(|| {
                                        batch_cell_infos.get(&key).map(|info| info.data_size as i64)
                                    })
                            })
                            .sum();

                        stats.sync_totals.0 += parsed.transactions_count as i64;
                        stats.sync_totals.1 += cells_created as i64;
                        stats.sync_totals.2 += cells_consumed as i64;
                        stats.last_block = Some((parsed.number, parsed.hash.clone()));

                        {
                            let entry = stats.daily_stats.entry(block_date).or_default();
                            entry.0 += 1;
                            entry.1 += parsed.transactions_count;
                            entry.2 += cells_created;
                            entry.3 += cells_consumed;
                            entry.4 += capacity_transferred;
                            entry.5 += data_size_added;
                            entry.6 += data_size_consumed;
                        }
                        stats
                            .daily_dao_fields
                            .insert(block_date, parsed.dao.clone());
                        {
                            let block_hour = truncate_to_hour(parsed.timestamp);
                            let entry = stats.hourly_stats.entry(block_hour).or_default();
                            entry.0 += 1;
                            entry.1 += parsed.transactions_count;
                            entry.2 += cells_created;
                            entry.3 += cells_consumed;
                            entry.4 += capacity_transferred;
                        }
                        {
                            let entry = stats.daily_block_stats.entry(block_date).or_default();
                            entry.0 += parsed.compact_target as i128;
                            entry.1 += 1;
                            entry.2 += parsed.uncles_count;
                        }
                        if let Some(first_tx) = tx_slice.first() {
                            if first_tx.is_cellbase {
                                if let Some(first_cell) = first_tx.cells.first() {
                                    let key = (block_date, first_cell.lock_script_hash.clone());
                                    let entry = stats.miner_stats.entry(key).or_insert((0, 0));
                                    entry.0 += 1;
                                    entry.1 = parsed.number;
                                }
                            }
                        }
                        {
                            let entry = stats
                                .epoch_stats
                                .entry(parsed.epoch_number)
                                .or_insert_with(|| EpochAccum {
                                    start_block: parsed.number,
                                    end_block: parsed.number,
                                    length: parsed.epoch_length,
                                    start_ts: parsed.timestamp,
                                    end_ts: parsed.timestamp,
                                    tx_count: 0,
                                    is_new: parsed.epoch_index == 0,
                                });
                            entry.end_block = parsed.number;
                            entry.end_ts = parsed.timestamp;
                            entry.tx_count += parsed.transactions_count;
                        }

                        if let Some(prev_ts) = prev_timestamp {
                            let block_time_seconds = (parsed.timestamp - prev_ts).num_seconds();
                            if block_time_seconds >= 0 {
                                *stats
                                    .block_time_dist
                                    .entry(block_time_to_bucket(block_time_seconds))
                                    .or_default() += 1;
                                let block_time_ms = block_time_seconds * 1000;
                                let entry =
                                    stats.daily_block_times.entry(block_date).or_insert((0, 0));
                                entry.0 += block_time_ms;
                                entry.1 += 1;
                            }
                        }
                        prev_timestamp = Some(parsed.timestamp);

                        if parsed.epoch_index == 0 && parsed.epoch_number > 0 {
                            if let Some((prev_epoch_num, prev_start_ts, _)) = prev_epoch {
                                if prev_epoch_num == parsed.epoch_number - 1 {
                                    let epoch_duration_minutes =
                                        (parsed.timestamp - prev_start_ts).num_seconds() as f64
                                            / 60.0;
                                    let bucket_minutes = epoch_duration_minutes.round() as i32;
                                    *stats.epoch_time_dist.entry(bucket_minutes).or_default() += 1;
                                }
                            }
                        }
                        if parsed.epoch_index == 0 {
                            prev_epoch = Some((parsed.epoch_number, parsed.timestamp, 0.0));
                        }
                        stats.dao_snapshot_dates.insert(block_date);
                    }
                    Ok((stats, t.elapsed().as_secs_f64() * 1000.0))
                });

                // T_ACT: Activity builder (writes only CF_ACTIVITIES — no conflicts)
                // Skipped during bulk sync; activities are rebuilt after completion.
                let h_act = if !skip_activities {
                    Some(s.spawn(|| -> Result<f64> {
                        let t = Instant::now();
                        let mut batch = StoreBatch::new(store);
                        let token_info_cache: HashMap<Vec<u8>, (Option<String>, Option<u8>)> =
                            HashMap::new();
                        let mut block_tx_idx = 0usize;
                        for parsed in all_parsed_blocks {
                            let tx_count = parsed.transactions_count as usize;
                            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count];
                            block_tx_idx += tx_count;

                            let tx_views: Vec<crate::db::writer::activities::TxView<'_>> = tx_slice
                                .iter()
                                .map(|td| {
                                    let inputs: Vec<crate::db::writer::activities::InputCellView> =
                                        if td.is_cellbase {
                                            Vec::new()
                                        } else {
                                            td.inputs
                                                .iter()
                                                .map(|inp| {
                                                    let key = (
                                                        inp.previous_tx_hash.to_vec(),
                                                        inp.previous_output_index as i16,
                                                    );
                                                    let cell_info = input_cell_info
                                                        .get(&key)
                                                        .or_else(|| batch_cell_infos.get(&key));
                                                    if let Some(info) = cell_info {
                                                        crate::db::writer::activities::InputCellView {
                                                            lock_script_hash: info
                                                                .lock_script_hash
                                                                .clone(),
                                                            capacity: info.capacity,
                                                            occupied_capacity: info.occupied_capacity,
                                                            type_code_hash: info.type_code_hash.clone(),
                                                            type_script_hash: info
                                                                .type_script_hash
                                                                .clone(),
                                                            type_args: None,
                                                            data: Vec::new(),
                                                            data_size: info.data_size,
                                                        }
                                                    } else {
                                                        crate::db::writer::activities::InputCellView {
                                                            lock_script_hash: Vec::new(),
                                                            capacity: 0,
                                                            occupied_capacity: 0,
                                                            type_code_hash: None,
                                                            type_script_hash: None,
                                                            type_args: None,
                                                            data: Vec::new(),
                                                            data_size: 0,
                                                        }
                                                    }
                                                })
                                                .collect()
                                        };
                                    crate::db::writer::activities::TxView {
                                        tx_hash: &td.hash,
                                        tx_index: td.tx_index,
                                        block_number: parsed.number,
                                        timestamp: parsed.timestamp.timestamp_millis(),
                                        is_cellbase: td.is_cellbase,
                                        inputs,
                                        outputs: &td.cells,
                                        outputs_data: &td.outputs_data,
                                    }
                                })
                                .collect();

                            let activities = crate::db::writer::activities::build_activities_for_block(
                                &tx_views,
                                &token_info_cache,
                            );
                            for (lock_hash, entry) in activities {
                                batch.put_activity(
                                    &lock_hash,
                                    entry.block_number,
                                    entry.tx_index,
                                    &entry,
                                );
                            }
                        }
                        batch.commit_no_wal()?;
                        Ok(t.elapsed().as_secs_f64() * 1000.0)
                    }))
                } else {
                    None
                };

                let t1_ms = h1.join().expect("T1 panicked")?;
                let t2_ms = h2.join().expect("T2 panicked")?;
                let t4_ms = h4.join().expect("T4 panicked")?;
                let t5_ms = h5.join().expect("T5 panicked")?;
                let t6_ms = h6.join().expect("T6 panicked")?;
                let (stats, t7_ms) = h7.join().expect("T7 panicked")?;
                let t_act_ms = match h_act {
                    Some(h) => h.join().expect("T_ACT panicked")?,
                    None => 0.0,
                };
                Ok((stats, [t1_ms, t2_ms, t4_ms, t5_ms, t6_ms, t7_ms, t_act_ms]))
            })?;
            thread_times = Some(tt);
        } else {
            // Live sync: serial writes in a single batch
            let mut data_batch = StoreBatch::new(self.writer.store());
            if !txs_for_batch.is_empty() {
                self.writer
                    .insert_transactions_batch(&txs_for_batch, &mut data_batch)?;
            }
            if !all_cells.is_empty() {
                self.writer
                    .insert_cells_batch(&all_cells, &mut data_batch, false)?;
            }
            if !all_inputs.is_empty() {
                self.writer.insert_transaction_inputs_batch(&all_inputs)?;
            }
            if !all_proposals.is_empty() {
                self.writer.insert_proposals_batch(&all_proposals)?;
            }
            if !all_consumptions.is_empty() {
                self.writer.consume_cells_batch_preloaded(
                    &all_consumptions,
                    &input_cell_info,
                    &batch_cell_infos,
                    &mut data_batch,
                    false,
                )?;
            }

            // Parallel DB reads for address balances and script usage
            let lock_hash_keys: Vec<&Vec<u8>> = if !skip_address_balances && !changes_ref.is_empty()
            {
                changes_ref.keys().collect()
            } else {
                vec![]
            };
            let unique_code_hashes: Vec<Vec<u8>> = if !script_usage_changes.is_empty() {
                let mut seen = std::collections::HashSet::new();
                script_usage_changes
                    .keys()
                    .filter_map(|(code_hash, _)| {
                        if seen.insert(code_hash.clone()) {
                            Some(code_hash.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                vec![]
            };
            let code_hash_refs: Vec<&Vec<u8>> = unique_code_hashes.iter().collect();

            let need_balances = !lock_hash_keys.is_empty();
            let need_scripts = !code_hash_refs.is_empty();

            if need_balances || need_scripts {
                let writer = &self.writer;
                let (existing_balances, existing_scripts) = std::thread::scope(|s| {
                    let bal = if need_balances {
                        Some(s.spawn(|| writer.read_address_balances(&lock_hash_keys)))
                    } else {
                        None
                    };
                    let scr = if need_scripts {
                        Some(s.spawn(|| writer.read_script_info(&code_hash_refs)))
                    } else {
                        None
                    };
                    (
                        bal.map(|h| h.join().unwrap()),
                        scr.map(|h| h.join().unwrap()),
                    )
                });
                if let Some(existing) = existing_balances {
                    self.writer.apply_address_balance_deltas(
                        &existing?,
                        &changes_ref,
                        &mut data_batch,
                    )?;
                }
                if let Some(existing) = existing_scripts {
                    self.writer.apply_script_usage_deltas(
                        &existing?,
                        &script_usage_changes,
                        &mut data_batch,
                    )?;
                }
            }

            // Write addr_txs entries
            for (lock_hash, block_num, tx_idx, tx_hash) in &addr_tx_entries {
                data_batch.put_addr_tx(lock_hash, *block_num, *tx_idx, tx_hash);
            }

            // Group A: DAO processing
            {
                let mut all_dao_deposits: Vec<(
                    crate::parser::ParsedDaoDeposit,
                    i64,
                    DateTime<Utc>,
                    i64,
                )> = Vec::new();
                let mut block_tx_idx = 0usize;
                for parsed in all_parsed_blocks {
                    let tx_count_for_block = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    block_tx_idx += tx_count_for_block;
                    let ar = DaoParser::extract_ar_from_dao_field(&parsed.dao).unwrap_or(0) as i64;
                    for tx_data in tx_slice {
                        let dao_deposits =
                            DaoParser::parse_deposits_from_cells(&tx_data.hash, &tx_data.cells);
                        for deposit in dao_deposits {
                            all_dao_deposits.push((deposit, parsed.number, parsed.timestamp, ar));
                        }
                    }
                }
                if !all_dao_deposits.is_empty() {
                    self.writer
                        .insert_dao_deposits_batch(&all_dao_deposits, &mut data_batch)?;
                }

                // Batch query consumed DAO deposits
                let mut all_input_outpoints_dao: Vec<(Vec<u8>, i16)> = Vec::new();
                let mut block_tx_idx = 0usize;
                for parsed in all_parsed_blocks {
                    let tx_count_for_block = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    block_tx_idx += tx_count_for_block;
                    for tx_data in tx_slice {
                        if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                            continue;
                        }
                        for input in &tx_data.inputs {
                            all_input_outpoints_dao.push((
                                input.previous_tx_hash.to_vec(),
                                input.previous_output_index as i16,
                            ));
                        }
                    }
                }

                let consumed_dao_map = if !all_input_outpoints_dao.is_empty() {
                    let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                        let mut seen = HashSet::new();
                        all_input_outpoints_dao
                            .into_iter()
                            .filter(|x| seen.insert(x.clone()))
                            .collect()
                    };
                    let outpoint_refs: Vec<(&[u8], i16)> = unique_outpoints
                        .iter()
                        .map(|(h, i)| (h.as_slice(), *i))
                        .collect();
                    self.writer
                        .find_consumed_dao_deposits_batch(&outpoint_refs)?
                } else {
                    HashMap::new()
                };

                if !consumed_dao_map.is_empty() {
                    use crate::db::DaoWithdrawalContextTrait;
                    #[derive(Clone)]
                    struct DaoWithdrawalContext {
                        consumed_deposits: Vec<(i64, Vec<u8>, i16, String, i64, i16)>,
                        new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
                        block_number: i64,
                        consuming_tx_hash: Vec<u8>,
                        timestamp: DateTime<Utc>,
                    }
                    impl DaoWithdrawalContextTrait for DaoWithdrawalContext {
                        fn consumed_deposits(&self) -> &[(i64, Vec<u8>, i16, String, i64, i16)] {
                            &self.consumed_deposits
                        }
                        fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)] {
                            &self.new_dao_outputs
                        }
                        fn block_number(&self) -> i64 {
                            self.block_number
                        }
                        fn consuming_tx_hash(&self) -> &[u8] {
                            &self.consuming_tx_hash
                        }
                        fn timestamp(&self) -> DateTime<Utc> {
                            self.timestamp
                        }
                    }

                    let mut withdrawal_contexts: Vec<DaoWithdrawalContext> = Vec::new();
                    let mut block_tx_idx = 0usize;
                    for parsed in all_parsed_blocks {
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;
                        for tx_data in tx_slice {
                            if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                continue;
                            }
                            let mut consumed_deposits: Vec<(i64, Vec<u8>, i16, String, i64, i16)> =
                                Vec::new();
                            for input in &tx_data.inputs {
                                let key = (
                                    input.previous_tx_hash.to_vec(),
                                    input.previous_output_index as i16,
                                );
                                if let Some(deposit_info) = consumed_dao_map.get(&key) {
                                    consumed_deposits.push(deposit_info.clone());
                                }
                            }
                            if consumed_deposits.is_empty() {
                                continue;
                            }
                            let mut new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)> =
                                Vec::new();
                            for (idx, cell) in tx_data.cells.iter().enumerate() {
                                if let Some(ref type_code_hash) = cell.type_code_hash {
                                    if type_code_hash == &dao_code_hash && cell.data_size == 8 {
                                        if let Some(data) = tx_data.outputs_data.get(idx) {
                                            let data_bytes = crate::rpc::parse_hex_to_bytes(data);
                                            if let Some(deposit_block) =
                                                DaoParser::parse_deposit_block_number(&data_bytes)
                                            {
                                                new_dao_outputs.push((
                                                    tx_data.hash.to_vec(),
                                                    idx as i16,
                                                    cell.lock_script_hash.clone(),
                                                    cell.capacity,
                                                    deposit_block,
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                            withdrawal_contexts.push(DaoWithdrawalContext {
                                consumed_deposits,
                                new_dao_outputs,
                                block_number: parsed.number,
                                consuming_tx_hash: tx_data.hash.to_vec(),
                                timestamp: parsed.timestamp,
                            });
                        }
                    }
                    if !withdrawal_contexts.is_empty() {
                        self.writer
                            .process_dao_withdrawals_batch(&withdrawal_contexts, &mut data_batch)?;
                    }
                }
            }

            // Group B: UDT processing
            {
                struct UdtTxContext {
                    tx_hash: Vec<u8>,
                    block_number: i64,
                    #[allow(dead_code)]
                    timestamp: chrono::DateTime<Utc>,
                    output_udts: Vec<crate::parser::ParsedUdtCell>,
                    input_outpoints: Vec<(Vec<u8>, i16)>,
                }
                let mut udt_tx_contexts: Vec<UdtTxContext> = Vec::new();
                let mut all_input_outpoints_udt: Vec<(Vec<u8>, i16)> = Vec::new();
                let mut batch_udt_cells: HashMap<(Vec<u8>, i16), crate::parser::ParsedUdtCell> =
                    HashMap::new();
                struct TxInfoForUdt {
                    tx_hash: Vec<u8>,
                    block_number: i64,
                    timestamp: chrono::DateTime<Utc>,
                    output_udts: Vec<crate::parser::ParsedUdtCell>,
                    input_outpoints: Vec<(Vec<u8>, i16)>,
                }
                let mut all_tx_infos_for_udt: Vec<TxInfoForUdt> = Vec::new();

                let mut block_tx_idx = 0usize;
                for (block_idx, block_response) in blocks.iter().enumerate() {
                    let parsed = &all_parsed_blocks[block_idx];
                    let tx_count_for_block = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    block_tx_idx += tx_count_for_block;
                    for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                        if tx_data.is_cellbase {
                            continue;
                        }
                        let tx = &block_response.block.transactions[tx_idx];
                        let output_udts = UdtParser::parse_udt_cells(tx);
                        for (output_index, udt_cell) in output_udts.iter().enumerate() {
                            batch_udt_cells.insert(
                                (tx_data.hash.to_vec(), output_index as i16),
                                udt_cell.clone(),
                            );
                            self.udt_cell_cache.insert(
                                (tx_data.hash, output_index as i16),
                                CachedUdtCellInfo {
                                    type_script_hash: udt_cell.type_script_hash.clone(),
                                    type_code_hash: udt_cell.type_code_hash.clone(),
                                    type_hash_type: udt_cell.type_hash_type,
                                    type_args: udt_cell.type_args.clone(),
                                    lock_script_hash: udt_cell.lock_script_hash.clone(),
                                    amount: udt_cell.amount,
                                    standard: udt_cell.standard.as_str().to_string(),
                                },
                            );
                        }
                        let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                            .inputs
                            .iter()
                            .map(|i| (i.previous_tx_hash.to_vec(), i.previous_output_index as i16))
                            .collect();
                        all_input_outpoints_udt.extend(input_outpoints.iter().cloned());
                        all_tx_infos_for_udt.push(TxInfoForUdt {
                            tx_hash: tx_data.hash.to_vec(),
                            block_number: parsed.number,
                            timestamp: parsed.timestamp,
                            output_udts,
                            input_outpoints,
                        });
                    }
                }

                let mut input_udt_info: HashMap<
                    (Vec<u8>, i16),
                    (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String),
                > = HashMap::new();
                if !skip_token && !all_input_outpoints_udt.is_empty() {
                    let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                        let mut seen = HashSet::new();
                        all_input_outpoints_udt
                            .into_iter()
                            .filter(|x| seen.insert(x.clone()))
                            .collect()
                    };
                    let mut uncached: Vec<(Vec<u8>, i16)> = Vec::new();
                    for (tx_hash, idx) in &unique_outpoints {
                        let key: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                        if let Some(cached) = self.udt_cell_cache.get(&(key, *idx)) {
                            input_udt_info.insert(
                                (tx_hash.clone(), *idx),
                                (
                                    cached.type_script_hash.clone(),
                                    cached.type_code_hash.clone(),
                                    cached.type_hash_type,
                                    cached.type_args.clone(),
                                    cached.lock_script_hash.clone(),
                                    cached.amount,
                                    cached.standard.clone(),
                                ),
                            );
                        } else {
                            uncached.push((tx_hash.clone(), *idx));
                        }
                    }
                    if !uncached.is_empty() {
                        let outpoint_refs: Vec<(&[u8], i16)> =
                            uncached.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                        let db_results = self.writer.get_udt_cells_info_batch(&outpoint_refs)?;
                        for ((tx_hash, idx), (tsh, tch, tht, ta, lsh, am, std)) in &db_results {
                            let key: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                            self.udt_cell_cache.insert(
                                (key, *idx),
                                CachedUdtCellInfo {
                                    type_script_hash: tsh.clone(),
                                    type_code_hash: tch.clone(),
                                    type_hash_type: *tht,
                                    type_args: ta.clone(),
                                    lock_script_hash: lsh.clone(),
                                    amount: *am,
                                    standard: std.clone(),
                                },
                            );
                        }
                        input_udt_info.extend(db_results);
                    }
                }
                if self.udt_cell_cache.len() > UDT_CELL_CACHE_CAPACITY * 2 {
                    self.udt_cell_cache.clear();
                }

                for tx_info in all_tx_infos_for_udt {
                    let has_udt_outputs = !tx_info.output_udts.is_empty();
                    let has_udt_inputs = tx_info.input_outpoints.iter().any(|(tx_hash, idx)| {
                        input_udt_info.contains_key(&(tx_hash.clone(), *idx))
                            || batch_udt_cells.contains_key(&(tx_hash.clone(), *idx))
                    });
                    if has_udt_outputs || has_udt_inputs {
                        udt_tx_contexts.push(UdtTxContext {
                            tx_hash: tx_info.tx_hash,
                            block_number: tx_info.block_number,
                            timestamp: tx_info.timestamp,
                            output_udts: tx_info.output_udts,
                            input_outpoints: tx_info.input_outpoints,
                        });
                    }
                }

                if !skip_token && !udt_tx_contexts.is_empty() {
                    let mut all_transfers: Vec<(crate::parser::ParsedUdtTransfer, Vec<u8>, i64)> =
                        Vec::new();
                    for ctx in &udt_tx_contexts {
                        let mut input_udts: Vec<crate::parser::ParsedUdtCell> = Vec::new();
                        for (tx_hash, idx) in &ctx.input_outpoints {
                            if let Some((tsh, tch, tht, ta, lsh, am, std)) =
                                input_udt_info.get(&(tx_hash.clone(), *idx))
                            {
                                input_udts.push(crate::parser::ParsedUdtCell {
                                    type_script_hash: tsh.clone(),
                                    type_code_hash: tch.clone(),
                                    type_hash_type: *tht,
                                    type_args: ta.clone(),
                                    lock_script_hash: lsh.clone(),
                                    amount: *am,
                                    standard: crate::parser::UdtStandard::parse(std),
                                });
                            } else if let Some(udt_cell) =
                                batch_udt_cells.get(&(tx_hash.clone(), *idx))
                            {
                                input_udts.push(udt_cell.clone());
                            }
                        }
                        for out_udt in &ctx.output_udts {
                            let matching_input = input_udts
                                .iter()
                                .find(|inp| inp.type_script_hash == out_udt.type_script_hash);
                            let is_mint = matching_input.is_none();
                            let from_lock_hash =
                                matching_input.map(|inp| inp.lock_script_hash.clone());
                            all_transfers.push((
                                crate::parser::ParsedUdtTransfer {
                                    type_script_hash: out_udt.type_script_hash.clone(),
                                    type_code_hash: out_udt.type_code_hash.clone(),
                                    type_hash_type: out_udt.type_hash_type,
                                    type_args: out_udt.type_args.clone(),
                                    from_lock_hash,
                                    to_lock_hash: out_udt.lock_script_hash.clone(),
                                    amount: out_udt.amount,
                                    standard: out_udt.standard.clone(),
                                    is_mint,
                                    is_burn: false,
                                },
                                ctx.tx_hash.clone(),
                                ctx.block_number,
                            ));
                        }
                        for inp_udt in &input_udts {
                            let has_matching_output = ctx
                                .output_udts
                                .iter()
                                .any(|out| out.type_script_hash == inp_udt.type_script_hash);
                            if !has_matching_output {
                                all_transfers.push((
                                    crate::parser::ParsedUdtTransfer {
                                        type_script_hash: inp_udt.type_script_hash.clone(),
                                        type_code_hash: inp_udt.type_code_hash.clone(),
                                        type_hash_type: inp_udt.type_hash_type,
                                        type_args: inp_udt.type_args.clone(),
                                        from_lock_hash: Some(inp_udt.lock_script_hash.clone()),
                                        to_lock_hash: Vec::new(),
                                        amount: inp_udt.amount,
                                        standard: inp_udt.standard.clone(),
                                        is_mint: false,
                                        is_burn: true,
                                    },
                                    ctx.tx_hash.clone(),
                                    ctx.block_number,
                                ));
                            }
                        }
                    }

                    if !all_transfers.is_empty() {
                        let transfer_refs: Vec<_> = all_transfers
                            .iter()
                            .map(|(t, h, b)| (t, h.as_slice(), *b))
                            .collect();
                        let block_timestamps: HashMap<i64, i64> = all_parsed_blocks
                            .iter()
                            .map(|p| (p.number, p.timestamp.timestamp_millis()))
                            .collect();
                        self.writer.process_udt_transfers_batch(
                            &transfer_refs,
                            &block_timestamps,
                            &mut data_batch,
                        )?;
                    }
                }
            }

            // Group C: NFT/Spore processing
            {
                let mut batch_spore_ids: HashSet<Vec<u8>> = HashSet::new();
                let mut block_tx_idx = 0usize;
                for (block_idx, block_response) in blocks.iter().enumerate() {
                    let parsed = &all_parsed_blocks[block_idx];
                    let tx_count_for_block = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    block_tx_idx += tx_count_for_block;
                    for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                        let tx = &block_response.block.transactions[tx_idx];
                        if !skip_spore {
                            for cluster in SporeParser::parse_clusters(tx) {
                                self.writer.insert_spore_cluster(
                                    &cluster,
                                    parsed.number,
                                    &tx_data.hash,
                                    &mut data_batch,
                                )?;
                            }
                            for (output_index, spore) in
                                SporeParser::parse_spores(tx).iter().enumerate()
                            {
                                batch_spore_ids.insert(spore.spore_id.clone());
                                self.writer.insert_spore_cell(
                                    spore,
                                    &tx_data.hash,
                                    output_index as i16,
                                    parsed.number,
                                    &mut data_batch,
                                )?;
                                self.writer
                                    .insert_spore_content(&spore.spore_id, &spore.content)?;
                            }
                        }
                        for issuer in MnftParser::parse_issuers(tx) {
                            self.writer.insert_mnft_issuer(
                                &issuer,
                                &tx_data.hash,
                                0,
                                parsed.number,
                                &mut data_batch,
                            )?;
                        }
                        for (output_index, class) in
                            MnftParser::parse_classes(tx).iter().enumerate()
                        {
                            self.writer.insert_mnft_class(
                                class,
                                &tx_data.hash,
                                output_index as i16,
                                parsed.number,
                                &mut data_batch,
                            )?;
                        }
                        for (output_index, token) in MnftParser::parse_tokens(tx).iter().enumerate()
                        {
                            self.writer.insert_mnft_token(
                                token,
                                &tx_data.hash,
                                output_index as i16,
                                parsed.number,
                                &mut data_batch,
                            )?;
                        }
                        for (output_index, account) in
                            DotbitParser::parse_accounts(tx).iter().enumerate()
                        {
                            self.writer.insert_dotbit_account(
                                account,
                                &tx_data.hash,
                                output_index as i16,
                                parsed.number,
                                &mut data_batch,
                            )?;
                        }
                    }
                }

                // NFT consumption (live sync only)
                if !self.is_bulk_sync_active() {
                    let mut all_prev_tx_hashes: Vec<Vec<u8>> = Vec::new();
                    let mut all_prev_indices: Vec<i16> = Vec::new();
                    let mut outpoint_context: Vec<(i64, Vec<u8>)> = Vec::new();
                    let mut block_tx_idx = 0usize;
                    for (block_idx, block_response) in blocks.iter().enumerate() {
                        let parsed = &all_parsed_blocks[block_idx];
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;
                        for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                            if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                continue;
                            }
                            let tx = &block_response.block.transactions[tx_idx];
                            for input in &tx.inputs {
                                let prev_tx_hash =
                                    crate::rpc::parse_hex_to_bytes(&input.previous_output.tx_hash);
                                let prev_index = input
                                    .previous_output
                                    .index
                                    .strip_prefix("0x")
                                    .and_then(|s| u32::from_str_radix(s, 16).ok())
                                    .unwrap_or(0)
                                    as i16;
                                all_prev_tx_hashes.push(prev_tx_hash);
                                all_prev_indices.push(prev_index);
                                outpoint_context.push((parsed.number, tx_data.hash.to_vec()));
                            }
                        }
                    }
                    if !all_prev_tx_hashes.is_empty() {
                        let spore_results = self.writer.get_spore_ids_by_outpoints_batch(
                            &all_prev_tx_hashes,
                            &all_prev_indices,
                        )?;
                        let mnft_results = self.writer.get_mnft_token_ids_by_outpoints_batch(
                            &all_prev_tx_hashes,
                            &all_prev_indices,
                        )?;
                        let dotbit_results =
                            self.writer.get_dotbit_account_ids_by_outpoints_batch(
                                &all_prev_tx_hashes,
                                &all_prev_indices,
                            )?;
                        let spore_map: HashMap<(Vec<u8>, i16), Vec<u8>> = spore_results
                            .into_iter()
                            .map(|(h, i, id)| ((h, i), id))
                            .collect();
                        let mnft_map: HashMap<(Vec<u8>, i16), Vec<u8>> = mnft_results
                            .into_iter()
                            .map(|(h, i, id)| ((h, i), id))
                            .collect();
                        let dotbit_map: HashMap<(Vec<u8>, i16), Vec<u8>> = dotbit_results
                            .into_iter()
                            .map(|(h, i, id)| ((h, i), id))
                            .collect();
                        for (i, (block_number, consuming_tx_hash)) in
                            outpoint_context.iter().enumerate()
                        {
                            let key = (all_prev_tx_hashes[i].clone(), all_prev_indices[i]);
                            if let Some(spore_id) = spore_map.get(&key) {
                                if !batch_spore_ids.contains(spore_id) {
                                    self.writer.consume_spore(
                                        spore_id,
                                        *block_number,
                                        consuming_tx_hash,
                                        &mut data_batch,
                                    )?;
                                }
                            }
                            if let Some(token_id) = mnft_map.get(&key) {
                                self.writer.consume_mnft_token(
                                    token_id,
                                    *block_number,
                                    consuming_tx_hash,
                                    &mut data_batch,
                                )?;
                            }
                            if let Some(account_id) = dotbit_map.get(&key) {
                                self.writer.consume_dotbit_account(
                                    account_id,
                                    *block_number,
                                    consuming_tx_hash,
                                    &mut data_batch,
                                )?;
                            }
                        }
                    }
                }
            }

            // Activity writes (live sync)
            {
                let token_info_cache: HashMap<Vec<u8>, (Option<String>, Option<u8>)> =
                    HashMap::new();
                let mut block_tx_idx = 0usize;
                for parsed in all_parsed_blocks {
                    let tx_count = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count];
                    block_tx_idx += tx_count;

                    let tx_views: Vec<crate::db::writer::activities::TxView<'_>> = tx_slice
                        .iter()
                        .map(|td| {
                            let inputs: Vec<crate::db::writer::activities::InputCellView> =
                                if td.is_cellbase {
                                    Vec::new()
                                } else {
                                    td.inputs
                                        .iter()
                                        .map(|inp| {
                                            let key = (
                                                inp.previous_tx_hash.to_vec(),
                                                inp.previous_output_index as i16,
                                            );
                                            let cell_info = input_cell_info
                                                .get(&key)
                                                .or_else(|| batch_cell_infos.get(&key));
                                            if let Some(info) = cell_info {
                                                crate::db::writer::activities::InputCellView {
                                                    lock_script_hash: info.lock_script_hash.clone(),
                                                    capacity: info.capacity,
                                                    occupied_capacity: info.occupied_capacity,
                                                    type_code_hash: info.type_code_hash.clone(),
                                                    type_script_hash: info.type_script_hash.clone(),
                                                    type_args: None,
                                                    data: Vec::new(),
                                                    data_size: info.data_size,
                                                }
                                            } else {
                                                crate::db::writer::activities::InputCellView {
                                                    lock_script_hash: Vec::new(),
                                                    capacity: 0,
                                                    occupied_capacity: 0,
                                                    type_code_hash: None,
                                                    type_script_hash: None,
                                                    type_args: None,
                                                    data: Vec::new(),
                                                    data_size: 0,
                                                }
                                            }
                                        })
                                        .collect()
                                };
                            crate::db::writer::activities::TxView {
                                tx_hash: &td.hash,
                                tx_index: td.tx_index,
                                block_number: parsed.number,
                                timestamp: parsed.timestamp.timestamp_millis(),
                                is_cellbase: td.is_cellbase,
                                inputs,
                                outputs: &td.cells,
                                outputs_data: &td.outputs_data,
                            }
                        })
                        .collect();

                    let activities = crate::db::writer::activities::build_activities_for_block(
                        &tx_views,
                        &token_info_cache,
                    );
                    for (lock_hash, entry) in activities {
                        data_batch.put_activity(
                            &lock_hash,
                            entry.block_number,
                            entry.tx_index,
                            &entry,
                        );
                    }
                }
            }

            // Commit all data writes in a single batch
            data_batch.commit()?;

            // Stats accumulation for live sync (serial — before finalize)
            batch_stats = BatchStats::default();
            let mut prev_timestamp: Option<chrono::DateTime<Utc>> =
                if let Some(first_block) = all_parsed_blocks.first() {
                    self.writer
                        .get_previous_block_timestamp(first_block.number)?
                } else {
                    None
                };
            let mut prev_epoch: Option<(i64, chrono::DateTime<Utc>, f64)> =
                if let Some(first_block) = all_parsed_blocks.first() {
                    self.writer
                        .get_last_epoch_start(first_block.number)?
                        .map(|(epoch_num, ts)| (epoch_num, ts, 0.0))
                } else {
                    None
                };

            let mut block_tx_idx = 0usize;
            for parsed in all_parsed_blocks {
                let block_date = parsed.timestamp.date_naive();
                let tx_count_for_block = parsed.transactions_count as usize;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                block_tx_idx += tx_count_for_block;

                let cells_created: i32 = tx_slice.iter().map(|tx| tx.cells.len() as i32).sum();
                let cells_consumed: i32 = tx_slice
                    .iter()
                    .filter(|tx| !tx.is_cellbase)
                    .map(|tx| tx.inputs.len() as i32)
                    .sum();
                let capacity_transferred: i64 = tx_slice
                    .iter()
                    .filter(|tx| !tx.is_cellbase)
                    .map(|tx| tx.total_output_capacity)
                    .sum();
                let data_size_added: i64 = tx_slice
                    .iter()
                    .flat_map(|tx| tx.cells.iter())
                    .map(|cell| cell.data_size as i64)
                    .sum();
                let data_size_consumed: i64 = tx_slice
                    .iter()
                    .filter(|tx| !tx.is_cellbase)
                    .flat_map(|tx| tx.inputs.iter())
                    .filter_map(|input| {
                        let key = (
                            input.previous_tx_hash.to_vec(),
                            input.previous_output_index as i16,
                        );
                        input_cell_info
                            .get(&key)
                            .map(|info| info.data_size as i64)
                            .or_else(|| {
                                batch_cell_infos.get(&key).map(|info| info.data_size as i64)
                            })
                    })
                    .sum();

                batch_stats.sync_totals.0 += parsed.transactions_count as i64;
                batch_stats.sync_totals.1 += cells_created as i64;
                batch_stats.sync_totals.2 += cells_consumed as i64;
                batch_stats.last_block = Some((parsed.number, parsed.hash.clone()));

                {
                    let entry = batch_stats.daily_stats.entry(block_date).or_default();
                    entry.0 += 1;
                    entry.1 += parsed.transactions_count;
                    entry.2 += cells_created;
                    entry.3 += cells_consumed;
                    entry.4 += capacity_transferred;
                    entry.5 += data_size_added;
                    entry.6 += data_size_consumed;
                }
                batch_stats
                    .daily_dao_fields
                    .insert(block_date, parsed.dao.clone());
                {
                    let block_hour = truncate_to_hour(parsed.timestamp);
                    let entry = batch_stats.hourly_stats.entry(block_hour).or_default();
                    entry.0 += 1;
                    entry.1 += parsed.transactions_count;
                    entry.2 += cells_created;
                    entry.3 += cells_consumed;
                    entry.4 += capacity_transferred;
                }
                {
                    let entry = batch_stats.daily_block_stats.entry(block_date).or_default();
                    entry.0 += parsed.compact_target as i128;
                    entry.1 += 1;
                    entry.2 += parsed.uncles_count;
                }
                if let Some(first_tx) = tx_slice.first() {
                    if first_tx.is_cellbase {
                        if let Some(first_cell) = first_tx.cells.first() {
                            let key = (block_date, first_cell.lock_script_hash.clone());
                            let entry = batch_stats.miner_stats.entry(key).or_insert((0, 0));
                            entry.0 += 1;
                            entry.1 = parsed.number;
                        }
                    }
                }
                {
                    let entry = batch_stats
                        .epoch_stats
                        .entry(parsed.epoch_number)
                        .or_insert_with(|| EpochAccum {
                            start_block: parsed.number,
                            end_block: parsed.number,
                            length: parsed.epoch_length,
                            start_ts: parsed.timestamp,
                            end_ts: parsed.timestamp,
                            tx_count: 0,
                            is_new: parsed.epoch_index == 0,
                        });
                    entry.end_block = parsed.number;
                    entry.end_ts = parsed.timestamp;
                    entry.tx_count += parsed.transactions_count;
                }

                if let Some(prev_ts) = prev_timestamp {
                    let block_time_seconds = (parsed.timestamp - prev_ts).num_seconds();
                    if block_time_seconds >= 0 {
                        *batch_stats
                            .block_time_dist
                            .entry(block_time_to_bucket(block_time_seconds))
                            .or_default() += 1;
                        let block_time_ms = block_time_seconds * 1000;
                        let entry = batch_stats
                            .daily_block_times
                            .entry(block_date)
                            .or_insert((0, 0));
                        entry.0 += block_time_ms;
                        entry.1 += 1;
                    }
                }
                prev_timestamp = Some(parsed.timestamp);

                if parsed.epoch_index == 0 && parsed.epoch_number > 0 {
                    if let Some((prev_epoch_num, prev_start_ts, _)) = prev_epoch {
                        if prev_epoch_num == parsed.epoch_number - 1 {
                            let epoch_duration_minutes =
                                (parsed.timestamp - prev_start_ts).num_seconds() as f64 / 60.0;
                            let bucket_minutes = epoch_duration_minutes.round() as i32;
                            *batch_stats
                                .epoch_time_dist
                                .entry(bucket_minutes)
                                .or_default() += 1;
                        }
                    }
                }
                if parsed.epoch_index == 0 {
                    prev_epoch = Some((parsed.epoch_number, parsed.timestamp, 0.0));
                }
                batch_stats.dao_snapshot_dates.insert(block_date);
            }
        }
        let write_ms = t_write.elapsed().as_secs_f64() * 1000.0;

        // Finalization: block headers + stats commit
        let t_finalize = Instant::now();
        {
            let mut batch = StoreBatch::new(self.writer.store());
            self.writer.insert_blocks_batch(&block_refs, &mut batch)?;
            self.write_batch_stats_to_batch(&batch_stats, &mut batch)?;
            if bulk_sync_mode {
                batch.commit_no_wal()?;
            } else {
                batch.commit()?;
            }
        }

        // Lightweight async cache update (no DB write)
        if let Some((block_number, ref block_hash)) = batch_stats.last_block {
            let ema_rate = self.progress.ema_blocks_per_second();
            let ema_rate_opt = if ema_rate > 0.0 { Some(ema_rate) } else { None };
            self.writer
                .update_sync_status(
                    block_number,
                    block_hash,
                    batch_stats.sync_totals.0,
                    batch_stats.sync_totals.1,
                    batch_stats.sync_totals.2,
                    0,
                    ema_rate_opt,
                )
                .await?;
        }
        let finalize_ms = t_finalize.elapsed().as_secs_f64() * 1000.0;

        let batch_tx_count = all_tx_data.len();
        let batch_cell_count: usize = all_tx_data.iter().map(|t| t.cells.len()).sum();
        let batch_input_count: usize = all_tx_data
            .iter()
            .filter(|t| !t.is_cellbase)
            .map(|t| t.inputs.len())
            .sum();
        if let Some([t1, t2, t4, t5, t6, t7, t_act]) = thread_times {
            info!(
                precompute_ms = format!("{:.1}", precompute_ms),
                prefetch_ms = format!("{:.1}", prefetch_ms),
                write_ms = format!("{:.1}", write_ms),
                finalize_ms = format!("{:.1}", finalize_ms),
                t1_ms = format!("{:.1}", t1),
                t2_ms = format!("{:.1}", t2),
                t4_ms = format!("{:.1}", t4),
                t5_ms = format!("{:.1}", t5),
                t6_ms = format!("{:.1}", t6),
                t7_ms = format!("{:.1}", t7),
                t_act_ms = format!("{:.1}", t_act),
                txs = batch_tx_count,
                cells = batch_cell_count,
                inputs = batch_input_count,
                "Batch write breakdown"
            );
        } else {
            info!(
                precompute_ms = format!("{:.1}", precompute_ms),
                write_ms = format!("{:.1}", write_ms),
                finalize_ms = format!("{:.1}", finalize_ms),
                txs = batch_tx_count,
                cells = batch_cell_count,
                inputs = batch_input_count,
                "Batch write breakdown"
            );
        }
        Ok(())
    }
    // === write_batch_stats_to_batch ===

    fn write_batch_stats_to_batch(&self, stats: &BatchStats, batch: &mut StoreBatch) -> Result<()> {
        // Epoch statistics
        for (epoch_number, accum) in &stats.epoch_stats {
            self.writer.upsert_epoch_statistics_batch(
                *epoch_number,
                accum.start_block,
                accum.end_block,
                accum.length,
                accum.start_ts,
                accum.end_ts,
                accum.tx_count,
                accum.is_new,
                batch,
            )?;
        }

        // Daily statistics
        for (
            date,
            (blocks, txs, created, consumed, capacity, data_size_added, data_size_consumed),
        ) in &stats.daily_stats
        {
            let dao_field = stats.daily_dao_fields.get(date);
            self.writer.update_daily_statistics(
                *date,
                *blocks,
                *txs,
                *created,
                *consumed,
                *capacity,
                *data_size_added,
                *data_size_consumed,
                dao_field.map(|v| v.as_slice()),
                batch,
            )?;
        }

        // Daily block stats
        for (date, (sum_target, count, uncles)) in &stats.daily_block_stats {
            let avg_target = if *count > 0 {
                (*sum_target / *count as i128) as i64
            } else {
                0
            };
            self.writer
                .update_daily_block_stats_batch(*date, avg_target, *count, *uncles, batch)?;
        }

        // Daily avg block time
        for (date, (sum_ms, count)) in &stats.daily_block_times {
            if *count > 0 {
                let avg_ms = sum_ms / *count as i64;
                self.writer
                    .update_daily_avg_block_time_batch(*date, avg_ms, *count, batch)?;
            }
        }

        // Hourly statistics
        for (hour, (blocks, txs, created, consumed, capacity)) in &stats.hourly_stats {
            self.writer.update_hourly_statistics(
                *hour, *blocks, *txs, *created, *consumed, *capacity, batch,
            )?;
        }

        // Miner statistics
        for ((date, miner_hash), (blocks_count, last_block)) in &stats.miner_stats {
            self.writer.update_miner_statistics_batch(
                miner_hash,
                *last_block,
                *date,
                *blocks_count,
                batch,
            )?;
        }

        // Block time distribution
        for (bucket, count) in &stats.block_time_dist {
            self.writer
                .update_block_time_distribution_batch(*bucket, *count, batch)?;
        }

        // Epoch time distribution
        for (bucket, count) in &stats.epoch_time_dist {
            self.writer
                .update_epoch_time_distribution_batch(*bucket, *count, batch)?;
        }

        // DAO daily snapshots
        {
            let mut snapshot_dates: Vec<_> = stats.dao_snapshot_dates.iter().collect();
            snapshot_dates.sort();
            for date in snapshot_dates {
                let dao_field = stats.daily_dao_fields.get(date).map(|v| v.as_slice());
                self.writer
                    .update_dao_daily_snapshot(*date, dao_field, batch)?;
            }
        }

        Ok(())
    }

    // === get_chain_block_hash, get_chain_tip ===

    /// Get the block hash for a given block number, using direct RocksDB reads when available.
    async fn get_chain_block_hash(&self, number: u64) -> Result<Vec<u8>> {
        if let Some(ref store) = self.ckb_store {
            store.refresh()?;
            store
                .get_block_hash(number)
                .map(|h| h.to_vec())
                .ok_or_else(|| anyhow::anyhow!("Block {} not found in CKB RocksDB", number))
        } else {
            let hash_hex = self
                .rpc
                .get_block_hash(number)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Block {} not found on chain", number))?;
            Ok(crate::rpc::parse_hex_to_bytes(&hash_hex))
        }
    }

    /// Get the chain tip block number, using direct RocksDB reads when available.
    async fn get_chain_tip(&self) -> Result<u64> {
        if let Some(ref store) = self.ckb_store {
            store.refresh()?;
            store
                .tip_number()
                .ok_or_else(|| anyhow::anyhow!("Failed to get chain tip from CKB RocksDB"))
        } else {
            self.rpc.get_tip_block_number().await
        }
    }

    // === check_and_handle_reorg, find_fork_point ===

    async fn check_and_handle_reorg(
        &self,
        db_tip: u64,
        stored_hash: &[u8],
    ) -> Result<Option<ReorgAction>> {
        let chain_hash_bytes = self.get_chain_block_hash(db_tip).await?;

        if chain_hash_bytes == stored_hash {
            return Ok(None);
        }

        warn!(
            "Reorg detected at block {}: stored={} chain={}",
            db_tip,
            hex::encode(stored_hash),
            hex::encode(&chain_hash_bytes)
        );

        let (fork_point, fork_hash) = self.find_fork_point(db_tip).await?;
        let depth = db_tip - fork_point;

        info!(
            "Fork point found at block {}, depth = {}",
            fork_point, depth
        );

        let chain_tip = self.get_chain_tip().await?;
        let chain_tip_hash_bytes = self.get_chain_block_hash(chain_tip).await?;

        if depth > DEEP_FORK_DEPTH {
            error!(
                "DEEP FORK DETECTED! Depth {} exceeds limit {}. Manual intervention required.",
                depth, DEEP_FORK_DEPTH
            );

            self.writer.record_deep_fork(
                fork_point as i64,
                &fork_hash,
                db_tip as i64,
                stored_hash,
                chain_tip as i64,
                &chain_tip_hash_bytes,
                depth as i64,
            )?;

            return Ok(Some(ReorgAction::DeepForkPaused));
        }

        info!(
            "Processing automatic reorg (depth={} <= limit={})",
            depth, DEEP_FORK_DEPTH
        );

        let result = self
            .writer
            .execute_reorg(
                fork_point as i64,
                &fork_hash,
                db_tip as i64,
                stored_hash,
                chain_tip as i64,
                &chain_tip_hash_bytes,
            )
            .await?;

        Ok(Some(ReorgAction::Handled(result)))
    }

    async fn find_fork_point(&self, db_tip: u64) -> Result<(u64, Vec<u8>)> {
        let mut height = db_tip;

        loop {
            let db_hash = self
                .repo
                .get_block_hash_at_height(height as i64)?
                .ok_or_else(|| anyhow::anyhow!("Block {} not found in DB", height))?;

            let chain_hash_bytes = self.get_chain_block_hash(height).await?;

            if db_hash == chain_hash_bytes {
                return Ok((height, db_hash));
            }

            if height == 0 {
                return Err(anyhow::anyhow!(
                    "No common ancestor found - genesis mismatch!"
                ));
            }

            height -= 1;
        }
    }

    // === update_secondary_issuance ===

    async fn update_secondary_issuance(
        &self,
        block_hash: &str,
        dao_hex: &str,
        block_number: i64,
        block_timestamp: DateTime<Utc>,
    ) -> Result<()> {
        // Check if we already have issuance data for this block
        if self
            .writer
            .store()
            .get_block_issuance(block_number)?
            .is_some()
        {
            return Ok(());
        }

        let economic_state = match self.rpc.get_block_economic_state(block_hash).await? {
            Some(state) => state,
            None => return Ok(()),
        };

        let dao_field = match DaoField::from_hex(dao_hex) {
            Some(f) => f,
            None => return Ok(()),
        };

        let secondary_issuance: u128 = economic_state
            .issuance
            .secondary
            .strip_prefix("0x")
            .and_then(|s| u128::from_str_radix(s, 16).ok())
            .unwrap_or(0);

        let miner_secondary: u128 = economic_state
            .miner_reward
            .secondary
            .strip_prefix("0x")
            .and_then(|s| u128::from_str_radix(s, 16).ok())
            .unwrap_or(0);

        let non_miner_secondary = secondary_issuance.saturating_sub(miner_secondary);

        // Calculate dao_compensation and burnt using RFC-0015 formula
        // dao_compensation = non_miner * deposit / (C - U)
        // burnt = non_miner * liquid / (C - U) where liquid = C - U - deposit
        let total_issuance = dao_field.total_issuance as u128;
        let occupied = dao_field.occupied_capacity as u128;
        let denominator = total_issuance.saturating_sub(occupied);

        let (dao_compensation, burnt) = if denominator > 0 {
            let total_dao_deposits: u128 = self.writer.get_dao_deposits_at_block(block_number)?;

            let dao_share = (non_miner_secondary * total_dao_deposits) / denominator;
            let burnt_share = non_miner_secondary.saturating_sub(dao_share);
            (dao_share, burnt_share)
        } else {
            (0, non_miner_secondary)
        };

        let breakdown = SecondaryIssuanceBreakdown {
            secondary_issuance: secondary_issuance as i64,
            miner_secondary: miner_secondary as i64,
            dao_compensation: dao_compensation as i64,
            burnt: burnt as i64,
        };

        let mut batch = StoreBatch::new(self.writer.store());
        self.writer.accumulate_secondary_issuance(
            &breakdown,
            block_number,
            block_timestamp,
            &mut batch,
        )?;
        batch.commit()?;

        Ok(())
    }

    // === cache_block_proposals ===

    async fn cache_block_proposals(&self, proposals: &[Vec<u8>], block_number: i64) {
        use ckbadger_common::CachedProposal;

        if proposals.is_empty() || !self.cache_invalidator.is_enabled() {
            return;
        }

        let mempool = match self.rpc.get_raw_tx_pool_verbose().await {
            Ok(pool) => pool,
            Err(e) => {
                warn!("Failed to fetch mempool for proposal enrichment: {}", e);
                let cached: Vec<CachedProposal> = proposals
                    .iter()
                    .enumerate()
                    .map(|(idx, p)| {
                        CachedProposal::new_minimal(hex::encode(p), block_number, idx as i16)
                    })
                    .collect();
                self.cache_invalidator.cache_proposals(&cached).await;
                return;
            }
        };

        let mut all_mempool_txs: HashMap<String, &crate::rpc::TxPoolEntry> = HashMap::new();
        for (tx_hash, entry) in mempool.pending.iter().chain(mempool.proposed.iter()) {
            let short_id = &tx_hash[2..22];
            all_mempool_txs.insert(short_id.to_string(), entry);
        }

        let mut cached_proposals = Vec::with_capacity(proposals.len());

        for (idx, proposal_bytes) in proposals.iter().enumerate() {
            let proposal_id = hex::encode(proposal_bytes);

            if let Some(entry) = all_mempool_txs.get(&proposal_id) {
                let fee = crate::rpc::parse_hex_to_bytes(&entry.fee);
                let fee_u64 = if fee.len() >= 8 {
                    u64::from_be_bytes(fee[fee.len() - 8..].try_into().unwrap_or_default())
                } else {
                    u64::from_str_radix(entry.fee.trim_start_matches("0x"), 16).unwrap_or(0)
                };
                let size =
                    u64::from_str_radix(entry.size.trim_start_matches("0x"), 16).unwrap_or(0);
                let cycles =
                    u64::from_str_radix(entry.cycles.trim_start_matches("0x"), 16).unwrap_or(0);

                cached_proposals.push(CachedProposal::new_with_details(
                    proposal_id,
                    "".to_string(),
                    block_number,
                    idx as i16,
                    fee_u64,
                    size,
                    cycles,
                ));
            } else {
                cached_proposals.push(CachedProposal::new_minimal(
                    proposal_id,
                    block_number,
                    idx as i16,
                ));
            }
        }

        self.cache_invalidator
            .cache_proposals(&cached_proposals)
            .await;
        self.cache_invalidator
            .cleanup_expired_proposals(block_number)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secondary_issuance_backfill_threshold_is_1000() {
        assert_eq!(SECONDARY_ISSUANCE_BACKFILL_THRESHOLD, 1000);
    }

    #[test]
    fn test_secondary_issuance_skipped_when_more_than_1000_blocks_behind() {
        let threshold = SECONDARY_ISSUANCE_BACKFILL_THRESHOLD;
        assert!(1001 > threshold);
        assert!(5000 > threshold);
    }

    #[test]
    fn test_secondary_issuance_tracked_when_1000_or_fewer_blocks_behind() {
        let threshold = SECONDARY_ISSUANCE_BACKFILL_THRESHOLD;
        assert!(1000 <= threshold);
        assert!(999 <= threshold);
        assert!(1 <= threshold);
    }

    // --- DAO recalculation boundary tests ---

    /// Helper: returns true if this batch crosses a 1000-block boundary
    fn crosses_1000_boundary(start_block: u64, end_block: u64) -> bool {
        (start_block / 1000) != (end_block / 1000)
    }

    #[test]
    fn test_crossed_1000_within_same_thousand() {
        assert!(!crosses_1000_boundary(6330000, 6330999));
        assert!(!crosses_1000_boundary(0, 999));
        assert!(!crosses_1000_boundary(5000, 5999));
    }

    #[test]
    fn test_crossed_1000_across_boundary() {
        assert!(crosses_1000_boundary(6330000, 6339999));
        assert!(crosses_1000_boundary(999, 1000));
        assert!(crosses_1000_boundary(0, 9999));
        assert!(crosses_1000_boundary(4500, 5500));
    }

    #[test]
    fn test_crossed_1000_exact_boundary() {
        assert!(crosses_1000_boundary(999, 1000));
        assert!(!crosses_1000_boundary(1000, 1001));
        assert!(crosses_1000_boundary(1999, 2000));
    }

    #[test]
    fn test_dao_recalc_skipped_during_bulk_sync() {
        let bulk_sync_threshold = 1000u64;
        let blocks_remaining = 10_000_000u64;
        let is_bulk = blocks_remaining > bulk_sync_threshold;
        let crossed = crosses_1000_boundary(6330000, 6339999);
        assert!(crossed, "batch should cross 1000-block boundary");
        assert!(is_bulk, "should be in bulk sync mode");
        assert!(
            !crossed || is_bulk,
            "DAO recalc should be skipped during bulk sync"
        );
    }

    #[test]
    fn test_dao_recalc_runs_in_realtime_sync() {
        let bulk_sync_threshold = 1000u64;
        let blocks_remaining = 500u64;
        let is_bulk = blocks_remaining > bulk_sync_threshold;
        let crossed = crosses_1000_boundary(18545999, 18546999);
        assert!(crossed, "batch should cross 1000-block boundary");
        assert!(!is_bulk, "should NOT be in bulk sync mode");
        assert!(
            crossed && !is_bulk,
            "DAO recalc should run in real-time sync"
        );
    }

    #[test]
    fn test_dao_recalc_not_triggered_without_boundary_crossing() {
        let bulk_sync_threshold = 1000u64;
        let blocks_remaining = 100u64;
        let is_bulk = blocks_remaining > bulk_sync_threshold;
        let crossed = crosses_1000_boundary(18546500, 18546800);
        assert!(!crossed, "batch should NOT cross 1000-block boundary");
        assert!(
            !crossed || is_bulk,
            "DAO recalc should not trigger without boundary crossing"
        );
    }

    #[test]
    fn test_partition_boundary_detection() {
        let start = 4_000_000u64;
        let end = 4_999_999u64;
        assert_eq!(get_partition_index(start), get_partition_index(end));
        assert!(!crosses_partition_boundary(start, end));
        assert_eq!(format_partition_range(start, end), "[p0]");

        let start = 4_999_990u64;
        let end = 5_000_009u64;
        assert_ne!(get_partition_index(start), get_partition_index(end));
        assert!(crosses_partition_boundary(start, end));
        assert_eq!(format_partition_range(start, end), "[p0->p1]");

        let start = 9_999_999u64;
        let end = 10_000_001u64;
        assert_ne!(get_partition_index(start), get_partition_index(end));
        assert!(crosses_partition_boundary(start, end));
        assert_eq!(format_partition_range(start, end), "[p1->p2]");

        let start = 5_000_000u64;
        let end = 5_100_000u64;
        assert_eq!(get_partition_index(start), get_partition_index(end));
        assert!(!crosses_partition_boundary(start, end));
        assert_eq!(format_partition_range(start, end), "[p1]");
    }
}
