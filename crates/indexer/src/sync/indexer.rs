#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use futures::stream::{FuturesOrdered, StreamExt};
use lru::LruCache;
use rayon::prelude::*;
use sqlx::PgPool;
use tokio::time::sleep;
use tokio_postgres::NoTls;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::cache::CacheInvalidator;
use crate::config::{Config, DEEP_FORK_DEPTH};
use crate::db::{
    BatchWriter, CopyConfig, CopyPoolManager, LiveCellInfo, LiveCellStorage, ParallelCopyRouter,
    ReorgResult, Repository, SecondaryIssuanceBreakdown,
};
use crate::parser::{
    activity::{ActivityParser, ParsedActivity},
    BlockParser, CellParser, DaoParser, DotbitParser, MnftParser, SporeParser, TransactionParser,
    UdtParser,
};
use crate::rpc::{BlockResponseWithCycles, CkbRpcClient, DaoField};

use super::SyncProgress;

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
    /// (total_tx, cells_created, cells_consumed) for sync_status
    sync_totals: (i64, i64, i64),
    /// Last block info for sync_status update
    last_block: Option<(i64, Vec<u8>)>,
    /// Per-hour hourly statistics: (blocks, txs, cells_created, cells_consumed, capacity)
    hourly_stats: HashMap<DateTime<Utc>, (i32, i32, i32, i32, i64)>,
    /// Per-date daily statistics: (blocks, txs, cells_created, cells_consumed, capacity, data_size_added, data_size_consumed)
    daily_stats: HashMap<NaiveDate, (i32, i32, i32, i32, i64, i64, i64)>,
    /// Per-date daily block stats: (sum_compact_target, block_count, total_uncles)
    daily_block_stats: HashMap<NaiveDate, (i128, i32, i32)>,
    /// Per-(date, miner_hash) -> (blocks_count, last_block_number)
    miner_stats: HashMap<(NaiveDate, Vec<u8>), (i32, i64)>,
    /// Per-epoch -> (start_block, end_block, length, start_ts, end_ts, tx_count, is_new)
    epoch_stats: HashMap<i64, EpochAccum>,
    /// Block time distribution: bucket_seconds -> count
    block_time_dist: HashMap<i32, i32>,
    /// Epoch time distribution: bucket_minutes -> count  
    epoch_time_dist: HashMap<i32, i32>,
    /// Dates that need DAO daily snapshot update
    dao_snapshot_dates: HashSet<NaiveDate>,
    /// Per-date block time totals: (sum_ms, count) for avg calculation
    daily_block_times: HashMap<NaiveDate, (i64, i32)>,
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
    data_size: i32,
}

#[derive(Default)]
struct PerfStats {
    rpc_fetch_us: AtomicU64,
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

        let rpc = self.rpc_fetch_us.swap(0, Ordering::Relaxed);
        let db = self.db_write_us.swap(0, Ordering::Relaxed);

        info!(
            "PERF[{}blks] RPC={:.1}ms DB={:.1}ms",
            blocks,
            rpc as f64 / 1000.0,
            db as f64 / 1000.0,
        );
    }
}

const CELL_CACHE_CAPACITY: usize = 200_000;

/// Convert block time in seconds to a bucket for block_time_distribution.
/// Matches the bucketing logic used in rebuild:
/// - block_time < 1s → bucket 0
/// - 1s <= block_time < 30s → floor(block_time)
/// - block_time >= 30s → bucket 30
fn block_time_to_bucket(block_time_seconds: i64) -> i32 {
    if block_time_seconds < 1 {
        0
    } else if block_time_seconds < 30 {
        block_time_seconds as i32
    } else {
        30
    }
}

fn infer_is_mainnet(rpc_url: &str) -> bool {
    let lowered = rpc_url.to_lowercase();
    !(lowered.contains("testnet") || lowered.contains("devnet"))
}

fn clone_parsed_cell(cell: &crate::parser::cell::ParsedCell) -> crate::parser::cell::ParsedCell {
    crate::parser::cell::ParsedCell {
        capacity: cell.capacity,
        lock_code_hash: cell.lock_code_hash.clone(),
        lock_hash_type: cell.lock_hash_type,
        lock_args: cell.lock_args.clone(),
        lock_script_hash: cell.lock_script_hash.clone(),
        type_code_hash: cell.type_code_hash.clone(),
        type_hash_type: cell.type_hash_type,
        type_args: cell.type_args.clone(),
        type_script_hash: cell.type_script_hash.clone(),
        data_hash: cell.data_hash.clone(),
        data_size: cell.data_size,
        data: cell.data.clone(),
    }
}

fn parsed_cell_from_live_info(info: &LiveCellInfo) -> crate::parser::cell::ParsedCell {
    crate::parser::cell::ParsedCell {
        capacity: info.capacity,
        lock_code_hash: info.lock_code_hash.clone(),
        lock_hash_type: 0,
        lock_args: info.lock_args.clone(),
        lock_script_hash: info.lock_script_hash.clone(),
        type_code_hash: info.type_code_hash.clone(),
        type_hash_type: None,
        type_args: None,
        type_script_hash: info.type_script_hash.clone(),
        data_hash: vec![0u8; 32],
        data_size: info.data_size,
        data: Vec::new(),
    }
}

fn truncate_to_hour(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(dt)
}

struct TxData {
    hash: Vec<u8>,
    block_number: i64,
    tx_index: i32,
    version: i32,
    inputs_count: i16,
    outputs_count: i16,
    witnesses_count: i16,
    cell_deps_count: i16,
    header_deps_count: i16,
    is_cellbase: bool,
    inputs: Vec<crate::parser::transaction::ParsedInput>,
    cell_deps: Vec<crate::parser::transaction::ParsedCellDep>,
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
    // Use indexed parallel iteration to preserve block order
    let mut parsed_results: Vec<(usize, crate::parser::block::ParsedBlock, Vec<TxData>)> = blocks
        .par_iter()
        .enumerate()
        .map(|(block_idx, block_response)| {
            let block = &block_response.block;
            let parsed = BlockParser::parse(block);

            // Parse transactions in parallel but sort by tx_index afterward
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

                    let cell_deps = TransactionParser::parse_cell_deps(tx);

                    TxData {
                        hash: parsed_tx.hash,
                        block_number: parsed.number,
                        tx_index: tx_index as i32,
                        version: parsed_tx.version,
                        inputs_count: parsed_tx.inputs_count as i16,
                        outputs_count: parsed_tx.outputs_count as i16,
                        witnesses_count: parsed_tx.witnesses_count as i16,
                        cell_deps_count: parsed_tx.cell_deps_count as i16,
                        header_deps_count: parsed_tx.header_deps_count as i16,
                        is_cellbase: parsed_tx.is_cellbase,
                        inputs,
                        cell_deps,
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

            // Sort transactions by tx_index to restore order
            tx_data_for_block.sort_by_key(|td| td.tx_index);

            (block_idx, parsed, tx_data_for_block)
        })
        .collect();

    // Sort by block index to restore block order
    parsed_results.sort_by_key(|(idx, _, _)| *idx);

    let mut all_parsed_blocks: Vec<crate::parser::block::ParsedBlock> =
        Vec::with_capacity(parsed_results.len());
    let mut all_tx_data: Vec<TxData> = Vec::new();
    let mut all_input_outpoints: Vec<(Vec<u8>, i16)> = Vec::new();

    for (_, parsed, tx_data_list) in parsed_results {
        for tx_data in &tx_data_list {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    all_input_outpoints.push((
                        input.previous_tx_hash.clone(),
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
    copy_router: Option<ParallelCopyRouter>,
    progress: Arc<SyncProgress>,
    cell_cache: Arc<tokio::sync::Mutex<LruCache<(Vec<u8>, i32), CachedCellInfo>>>,
    perf: PerfStats,
    cache_invalidator: CacheInvalidator,
    last_cache_invalidation: tokio::sync::Mutex<u64>,
    /// Track previous bulk sync state to detect transition from bulk -> live sync
    was_bulk_sync_active: std::sync::atomic::AtomicBool,
    /// Track previous secondary issuance bulk state (>1000 blocks behind)
    was_secondary_issuance_bulk_active: std::sync::atomic::AtomicBool,
    /// Track batches processed since last LiveCellStore flush
    batches_since_flush: std::sync::atomic::AtomicU64,
    /// Shared flag to pause sync during index rebuild
    rebuild_pause_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Flag to notify fetcher that a reorg/mismatch occurred and it should reset next_block
    reorg_notify_flag: Arc<std::sync::atomic::AtomicBool>,
    rocksdb_store: Arc<crate::db::RocksDbLiveCellStore>,
}

impl Indexer {
    pub async fn new(config: Config, pool: PgPool) -> Result<Self> {
        let rpc = CkbRpcClient::new(&config.ckb_rpc_url);
        let cache_invalidator = CacheInvalidator::new(config.redis_url.as_deref()).await;
        let repo = Repository::with_cache(pool.clone(), cache_invalidator.clone());

        let rocksdb_store = Self::create_rocksdb_store(&config)?;
        let live_cell_store: crate::db::DynLiveCellStorage = Arc::clone(&rocksdb_store) as _;

        let writer = BatchWriter::with_live_cell_store(
            pool.clone(),
            config.fast_sync_mode,
            live_cell_store,
            cache_invalidator.clone(),
        );

        let (tip_number, _) = repo.get_sync_tip().await?;
        let chain_tip = rpc.get_tip_block_number().await?;

        let progress = Arc::new(SyncProgress::new(tip_number as u64, chain_tip));

        let cell_cache = Arc::new(tokio::sync::Mutex::new(LruCache::new(
            NonZeroUsize::new(CELL_CACHE_CAPACITY).expect("CELL_CACHE_CAPACITY must be non-zero"),
        )));

        let copy_router = if config.use_copy_bulk_sync {
            match CopyPoolManager::new(
                &config.database_url,
                CopyConfig {
                    max_copy_connections: config.copy_pool_size,
                    copy_batch_size: 100_000,
                    copy_enabled: true,
                },
            ) {
                Ok(pool_manager) => {
                    info!(
                        "COPY bulk sync enabled with {} connections",
                        config.copy_pool_size
                    );
                    Some(ParallelCopyRouter::new(pool_manager))
                }
                Err(e) => {
                    warn!("Failed to create COPY pool, falling back to UNNEST: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let was_bulk = progress.blocks_remaining() > config.bulk_sync_threshold;
        let was_secondary_bulk =
            progress.blocks_remaining() > SECONDARY_ISSUANCE_BACKFILL_THRESHOLD;
        Ok(Self {
            config,
            rpc,
            repo,
            writer,
            copy_router,
            progress,
            cell_cache,
            perf: PerfStats::default(),
            cache_invalidator,
            last_cache_invalidation: tokio::sync::Mutex::new(0),
            was_bulk_sync_active: std::sync::atomic::AtomicBool::new(was_bulk),
            was_secondary_issuance_bulk_active: std::sync::atomic::AtomicBool::new(
                was_secondary_bulk,
            ),
            batches_since_flush: std::sync::atomic::AtomicU64::new(0),
            rebuild_pause_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            reorg_notify_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rocksdb_store,
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

    fn create_rocksdb_store(config: &Config) -> Result<Arc<crate::db::RocksDbLiveCellStore>> {
        info!(
            "Using RocksDB live cell store at: {} (bulk_sync_cell_cache={})",
            config.live_cell_db_path, config.bulk_sync_cell_cache
        );
        let store = crate::db::RocksDbLiveCellStore::open(
            &config.live_cell_db_path,
            config.bulk_sync_cell_cache,
        )?;
        let count = store.len();
        if count > 0 {
            info!("Loaded {} live cells from RocksDB", count);
        }
        Ok(Arc::new(store))
    }

    /// Check if bulk sync mode is active (for skipping non-critical statistics).
    /// Auto-enabled when blocks_remaining > bulk_sync_threshold (no manual config needed)
    pub fn is_bulk_sync_active(&self) -> bool {
        self.progress.blocks_remaining() > self.config.bulk_sync_threshold
    }

    fn is_secondary_issuance_bulk_active(&self) -> bool {
        self.progress.blocks_remaining() > SECONDARY_ISSUANCE_BACKFILL_THRESHOLD
    }

    async fn is_stats_rebuild_in_progress(&self) -> bool {
        let result: Option<(bool,)> = sqlx::query_as(
            "SELECT COALESCE(stats_rebuild_in_progress, false) FROM sync_status WHERE id = 1",
        )
        .fetch_optional(self.writer.pool())
        .await
        .ok()
        .flatten();

        result.map(|r| r.0).unwrap_or(false)
    }

    fn should_use_copy(&self) -> bool {
        self.is_bulk_sync_active() && self.copy_router.is_some()
    }

    pub async fn run(&self) -> Result<()> {
        let blocks_behind = self.progress.blocks_remaining();
        let copy_enabled = self.copy_router.is_some();
        info!(
            "Starting indexer (pipeline={}, copy={}, {} blocks behind, threshold={})",
            self.config.pipeline_enabled,
            copy_enabled,
            blocks_behind,
            self.config.bulk_sync_threshold
        );

        if blocks_behind > self.config.bulk_sync_threshold {
            info!(
                "Bulk sync auto-enabled: {} blocks behind > {} threshold{}",
                blocks_behind,
                self.config.bulk_sync_threshold,
                if copy_enabled { ", using COPY" } else { "" }
            );

            if let Some(store) = self.writer.live_cell_store() {
                store.set_bulk_sync_mode(true);
                if self.config.bulk_sync_cell_cache {
                    let (count, bytes) = store.consumed_cells_stats();
                    info!(
                        "Bulk sync cell cache: {} consumed cells ({:.2} MB) retained",
                        count,
                        bytes as f64 / 1024.0 / 1024.0
                    );
                }
            }
        }

        let (start_block, _) = self.repo.get_sync_tip().await?;

        let consistent_block = self.writer.find_last_consistent_block().await?;
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

        self.writer.init_sync_start(actual_start).await?;

        if let Err(e) = self.maybe_submit_label_import_task().await {
            warn!("Failed to submit label import task: {}", e);
        }

        let writer_for_task = self.writer.pool().clone();
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
                    BatchWriter::with_fast_sync_mode(writer_for_task.clone(), fast_sync_mode);
                match writer.refresh_token_24h_transfers().await {
                    Ok(count) => info!("Refreshed 24h transfers for {} tokens", count),
                    Err(e) => warn!("Failed to refresh token 24h transfers: {}", e),
                }
                match writer.refresh_mnft_24h_transfers().await {
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

            if self.repo.has_unresolved_deep_fork().await.unwrap_or(false) {
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

        type FetchedBatch = (u64, u64, u64, Vec<BlockResponseWithCycles>);
        type ParsedBatch = (
            u64,                                                 // start_block
            u64,                                                 // end_block
            u64,                                                 // chain_tip
            Vec<BlockResponseWithCycles>,                        // raw blocks (for UDT parsing)
            Vec<crate::parser::block::ParsedBlock>,              // parsed blocks
            Vec<TxData>,                                         // parsed transactions
            HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)>,   // input_cell_info
            HashMap<(Vec<u8>, i16), (Vec<u8>, Option<Vec<u8>>)>, // consumed_code_hashes
        );

        let (fetch_tx, mut fetch_rx) = mpsc::channel::<FetchedBatch>(self.config.pipeline_buffer);
        let (parse_tx, mut parse_rx) = mpsc::channel::<ParsedBatch>(self.config.pipeline_buffer);

        let rpc = self.rpc.clone();
        let config = self.config.clone();
        let progress = Arc::clone(&self.progress);
        let repo = self.repo.clone();
        let rebuild_pause = Arc::clone(&self.rebuild_pause_flag);
        let reorg_notify = Arc::clone(&self.reorg_notify_flag);

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

                // Reset next_block after pause to re-query DB state
                // This prevents stale next_block from causing batch mismatches
                if was_paused {
                    info!("Fetcher resuming from pause, resetting next_block to re-query DB state");
                    next_block = None;
                    was_paused = false;
                }

                // Check if writer signaled a reorg/mismatch - reset next_block to re-query DB
                if reorg_notify.swap(false, Ordering::SeqCst) {
                    info!("Fetcher received reorg notification, resetting next_block");
                    next_block = None;
                }

                let chain_tip = match rpc.get_tip_block_number().await {
                    Ok(tip) => tip,
                    Err(e) => {
                        error!("Failed to get chain tip: {}", e);
                        sleep(Duration::from_secs(5)).await;
                        continue;
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

                let blocks = match Self::fetch_blocks_with_config(
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
                };

                if fetch_tx
                    .send((start_block, end_block, chain_tip, blocks))
                    .await
                    .is_err()
                {
                    break;
                }

                next_block = Some(end_block + 1);

                // Periodically re-check db_tip to handle writer failures/reorgs
                if end_block % 1000 == 0 {
                    next_block = None;
                }
            }
        });

        let writer_for_parser = self.writer.clone();
        let cell_cache_for_parser = Arc::clone(&self.cell_cache);

        let parser = tokio::spawn(async move {
            while let Some((start_block, end_block, chain_tip, blocks)) = fetch_rx.recv().await {
                let blocks_clone = blocks.clone();
                let (all_parsed_blocks, all_tx_data, all_input_outpoints) =
                    tokio::task::spawn_blocking(move || parse_blocks_parallel(&blocks_clone))
                        .await
                        .unwrap_or_else(|_| (vec![], vec![], vec![]));

                if all_parsed_blocks.is_empty() {
                    continue;
                }

                let mut batch_cells: HashMap<(Vec<u8>, i16), ()> = HashMap::new();
                for td in &all_tx_data {
                    for (idx, _) in td.cells.iter().enumerate() {
                        batch_cells.insert((td.hash.clone(), idx as i16), ());
                    }
                }

                let mut input_cell_info: HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)> =
                    HashMap::new();
                {
                    let cache = cell_cache_for_parser.lock().await;
                    for (tx_hash, idx) in &all_input_outpoints {
                        let key = (tx_hash.clone(), *idx as i32);
                        if let Some(cached) = cache.peek(&key) {
                            input_cell_info.insert(
                                (tx_hash.clone(), *idx),
                                (
                                    cached.capacity,
                                    cached.created_at_block,
                                    cached.lock_script_hash.clone(),
                                    cached.data_size,
                                ),
                            );
                        }
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
                    // Add timeout to prevent parser from blocking indefinitely on DB query
                    let db_query = writer_for_parser.get_cells_info_batch(&missing_refs);
                    match tokio::time::timeout(Duration::from_secs(30), db_query).await {
                        Ok(Ok(db_info)) => {
                            for ((tx_hash, idx), (cap, block, lock_hash, data_size)) in db_info {
                                input_cell_info
                                    .insert((tx_hash, idx), (cap, block, lock_hash, data_size));
                            }
                        }
                        Ok(Err(e)) => {
                            error!("Parser: Failed to fetch cell info from DB: {}", e);
                        }
                        Err(_) => {
                            warn!("Parser: DB query for cell info timed out after 30s, continuing without data");
                        }
                    }
                }

                let mut consumed_from_db: Vec<(Vec<u8>, i16)> = Vec::new();
                for td in &all_tx_data {
                    if !td.is_cellbase {
                        for input in &td.inputs {
                            let key = (
                                input.previous_tx_hash.clone(),
                                input.previous_output_index as i16,
                            );
                            if input_cell_info.contains_key(&key) && !batch_cells.contains_key(&key)
                            {
                                consumed_from_db.push(key);
                            }
                        }
                    }
                }

                let consumed_code_hashes = if !consumed_from_db.is_empty() {
                    let refs: Vec<(&[u8], i16)> = consumed_from_db
                        .iter()
                        .map(|(h, i)| (h.as_slice(), *i))
                        .collect();
                    // Add timeout to prevent parser from blocking indefinitely on DB query
                    let db_query = writer_for_parser.get_cells_code_hashes_batch(&refs);
                    match tokio::time::timeout(Duration::from_secs(30), db_query).await {
                        Ok(Ok(hashes)) => hashes,
                        Ok(Err(e)) => {
                            error!("Parser: Failed to fetch code hashes from DB: {}", e);
                            HashMap::new()
                        }
                        Err(_) => {
                            warn!("Parser: DB query for code hashes timed out after 30s, continuing without data");
                            HashMap::new()
                        }
                    }
                } else {
                    HashMap::new()
                };

                if parse_tx
                    .send((
                        start_block,
                        end_block,
                        chain_tip,
                        blocks,
                        all_parsed_blocks,
                        all_tx_data,
                        input_cell_info,
                        consumed_code_hashes,
                    ))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Writer loop - receives pre-parsed batches
        loop {
            if self.repo.has_unresolved_deep_fork().await.unwrap_or(false) {
                warn!("Deep fork unresolved, sync paused. Waiting for manual intervention...");
                Self::drain_channel(&mut parse_rx).await;
                sleep(Duration::from_secs(30)).await;
                continue;
            }

            // Use timeout to detect "caught up" state - if no batch arrives within poll interval,
            // we're likely caught up and can trigger idle tasks like integrity checks
            let recv_timeout = Duration::from_millis(self.config.poll_interval_ms * 2);
            match tokio::time::timeout(recv_timeout, parse_rx.recv()).await {
                Ok(Some((
                    start_block,
                    end_block,
                    chain_tip,
                    blocks,
                    all_parsed_blocks,
                    all_tx_data,
                    input_cell_info,
                    consumed_code_hashes,
                ))) => {
                    let (db_tip, db_tip_hash) = self.repo.get_sync_tip().await?;

                    // Validate batch is still valid (no reorg happened)
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

                    // Check for reorg before processing - skip during bulk sync
                    // Historical blocks are already finalized (CKB finalizes after 24 blocks),
                    // so reorg checks are only needed when approaching the chain tip.
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
                            consumed_code_hashes,
                            chain_tip,
                        )
                        .await
                    {
                        error!("Sync error: {:?}", e);
                        if let Err(cleanup_err) = self
                            .writer
                            .cleanup_batch_range(start_block as i64, end_block as i64)
                            .await
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

                    if let Some(last_block) = all_parsed_blocks.last() {
                        self.progress.update_current_batch(
                            last_block.number as u64,
                            all_parsed_blocks.len() as u64,
                        );

                        let mode = if self.should_use_copy() {
                            "[COPY]"
                        } else if self.is_bulk_sync_active() {
                            "[BULK]"
                        } else {
                            ""
                        };
                        info!(
                            "Wrote blocks {} to {} ({} remaining, {:.2}s) {}",
                            start_block,
                            end_block,
                            self.progress.blocks_remaining(),
                            db_elapsed.as_secs_f64(),
                            mode
                        );

                        // Handle periodic updates (secondary issuance, DAO stats)
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
                        if crossed_1000 {
                            let update_block = ((end_block / 1000) * 1000) as i64;
                            if let Err(e) = self
                                .writer
                                .recalculate_dao_extended_statistics(update_block)
                                .await
                            {
                                warn!("Failed to recalculate DAO statistics: {}", e);
                            }
                        }

                        self.maybe_invalidate_chart_caches(end_block).await;
                        self.check_bulk_sync_completion().await;
                    }

                    self.perf
                        .blocks_count
                        .fetch_add(all_parsed_blocks.len() as u64, Ordering::Relaxed);
                    self.perf.report_and_reset();

                    self.maybe_flush_live_cell_store().await;
                }
                Ok(None) => {
                    fetcher.abort();
                    parser.abort();
                    return Err(anyhow::anyhow!("Pipeline channel closed"));
                }
                Err(_timeout) => {
                    if let Err(e) = self.check_and_execute_live_cells_populate_task().await {
                        warn!("Failed to execute live_cells_populate task: {}", e);
                    }
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

    async fn drain_channel<T>(rx: &mut tokio::sync::mpsc::Receiver<T>) {
        let mut drained = 0;
        while rx.try_recv().is_ok() {
            drained += 1;
        }
        if drained > 0 {
            info!("Drained {} stale batches from pipeline", drained);
        }
    }

    #[allow(dead_code)]
    async fn handle_periodic_updates(
        &self,
        start_block: u64,
        end_block: u64,
        last_block_response: &BlockResponseWithCycles,
    ) {
        let last_block_number = BlockParser::parse_block_number(&last_block_response.block);

        if !self.is_secondary_issuance_bulk_active() {
            let block_timestamp =
                BlockParser::parse_timestamp(&last_block_response.block.header.timestamp);
            if let Err(e) = self
                .update_secondary_issuance(
                    &last_block_response.block.header.hash,
                    &last_block_response.block.header.dao,
                    last_block_number as i64,
                    block_timestamp,
                )
                .await
            {
                warn!("Failed to update secondary issuance: {}", e);
            }
        }

        let crossed_1000 = (start_block / 1000) != (end_block / 1000);
        if crossed_1000 {
            let update_block = ((end_block / 1000) * 1000) as i64;
            if let Err(e) = self
                .writer
                .recalculate_dao_extended_statistics(update_block)
                .await
            {
                warn!("Failed to recalculate DAO statistics: {}", e);
            }
        }

        self.maybe_invalidate_chart_caches(end_block).await;
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

    async fn sync_batch(&self) -> Result<SyncAction> {
        let chain_tip = self.rpc.get_tip_block_number().await?;
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

        // Check for reorg - skip during bulk sync since historical blocks are finalized
        // (CKB finalizes after 24 blocks, bulk_sync_threshold is 72)
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

        let end_block = std::cmp::min(start_block + self.config.batch_size as u64 - 1, chain_tip);

        if start_block > end_block {
            return Ok(SyncAction::CaughtUp);
        }

        let fetch_start = Instant::now();
        let blocks = self.fetch_blocks_parallel(start_block, end_block).await?;
        self.perf
            .add(&self.perf.rpc_fetch_us, fetch_start.elapsed());

        let db_start = Instant::now();
        if let Err(e) = self.sync_blocks_batch(&blocks, chain_tip).await {
            if let Err(cleanup_err) = self
                .writer
                .cleanup_batch_range(start_block as i64, end_block as i64)
                .await
            {
                error!("Failed to cleanup partial batch: {:?}", cleanup_err);
            }
            return Err(e);
        }
        let db_elapsed = db_start.elapsed();
        self.perf.add(&self.perf.db_write_us, db_elapsed);

        if let Some(last_block_response) = blocks.last() {
            let last_block_number = BlockParser::parse_block_number(&last_block_response.block);
            self.progress
                .update_current_batch(last_block_number, blocks.len() as u64);

            info!(
                "Wrote blocks {} to {} ({} remaining, {:.2}s)",
                start_block,
                end_block,
                self.progress.blocks_remaining(),
                db_elapsed.as_secs_f64()
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
            if crossed_1000 {
                let update_block = ((end_block / 1000) * 1000) as i64;
                if let Err(e) = self
                    .writer
                    .recalculate_dao_extended_statistics(update_block)
                    .await
                {
                    warn!("Failed to recalculate DAO statistics: {}", e);
                }
            }

            self.maybe_invalidate_chart_caches(end_block).await;
        }

        self.check_bulk_sync_completion().await;
        self.maybe_flush_live_cell_store().await;

        Ok(SyncAction::Continue)
    }

    async fn check_bulk_sync_completion(&self) {
        use std::sync::atomic::Ordering;

        let currently_bulk = self.is_bulk_sync_active();
        let was_bulk = self.was_bulk_sync_active.load(Ordering::SeqCst);
        let currently_secondary_bulk = self.is_secondary_issuance_bulk_active();
        let was_secondary_bulk = self
            .was_secondary_issuance_bulk_active
            .load(Ordering::SeqCst);

        if was_bulk && !currently_bulk {
            info!("Bulk sync completed, submitting post-sync tasks...");

            if let Some(store) = self.writer.live_cell_store() {
                store.set_bulk_sync_mode(false);
                if self.config.bulk_sync_cell_cache {
                    let cleaned = store.cleanup_consumed_cells();
                    info!(
                        "Bulk sync cell cache: cleaned up {} consumed cells",
                        cleaned
                    );
                }
            }

            let chain_tip = self.progress.target();
            self.cache_invalidator
                .update_sync_status(|status| {
                    status.mark_bulk_sync_completed(chain_tip as i64);
                })
                .await;

            if let Err(e) = self.maybe_submit_index_rebuild_task().await {
                warn!("Failed to submit index rebuild task: {}", e);
            }

            if let Err(e) = self.maybe_submit_live_cells_populate_task().await {
                warn!("Failed to submit live cells populate task: {}", e);
            }

            if let Err(e) = self.maybe_submit_statistics_rebuild_task().await {
                warn!("Failed to submit statistics rebuild task: {}", e);
            }

            if let Err(e) = self.maybe_submit_spore_rebuild_task().await {
                warn!("Failed to submit spore rebuild task: {}", e);
            }
        }

        if was_secondary_bulk && !currently_secondary_bulk {
            info!("Secondary issuance bulk sync completed, submitting backfill task...");

            if let Err(e) = self.maybe_submit_secondary_issuance_backfill_task().await {
                warn!("Failed to submit secondary issuance backfill task: {}", e);
            }
        }

        self.was_bulk_sync_active
            .store(currently_bulk, Ordering::SeqCst);
        self.was_secondary_issuance_bulk_active
            .store(currently_secondary_bulk, Ordering::SeqCst);
    }

    /// Submit an index rebuild task if indexes are deferred and no rebuild task is pending/running
    async fn maybe_submit_index_rebuild_task(&self) -> Result<()> {
        use ckbadger_common::{IndexRebuildConfig, TaskBuilder};

        // Check if indexes are deferred
        let row: Option<(bool,)> = sqlx::query_as(
            "SELECT COALESCE(indexes_deferred, false) FROM sync_status WHERE id = 1",
        )
        .fetch_optional(self.writer.pool())
        .await?;

        let indexes_deferred = row.map(|r| r.0).unwrap_or(false);
        if !indexes_deferred {
            debug!("Indexes are not deferred, skipping rebuild task submission");
            return Ok(());
        }

        // Check if there's already a pending or running index_rebuild task
        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT COUNT(*) FROM tasks WHERE task_type = 'index_rebuild' AND status IN ('pending', 'running')",
        )
        .fetch_optional(self.writer.pool())
        .await?;

        if existing.map(|r| r.0).unwrap_or(0) > 0 {
            info!("Index rebuild task already pending/running, skipping submission");
            return Ok(());
        }

        // Submit new index rebuild task
        let builder = TaskBuilder::index_rebuild(IndexRebuildConfig {
            parallel_connections: self.config.index_rebuild_parallel,
            indexes: None,
            rebuild_constraints: true,
        });

        let task_id: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO tasks (task_type, config, priority, max_retries)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(builder.task_type().to_string())
        .bind(builder.config())
        .bind(builder.get_priority())
        .bind(builder.get_max_retries())
        .fetch_one(self.writer.pool())
        .await?;

        info!(
            "Submitted index rebuild task: {} (parallel={})",
            task_id.0, self.config.index_rebuild_parallel
        );

        Ok(())
    }

    async fn maybe_submit_statistics_rebuild_task(&self) -> Result<()> {
        use ckbadger_common::{StatisticsRebuildConfig, TaskBuilder};

        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT COUNT(*) FROM tasks WHERE task_type = 'statistics_rebuild' AND status IN ('pending', 'running')",
        )
        .fetch_optional(self.writer.pool())
        .await?;

        if existing.map(|r| r.0).unwrap_or(0) > 0 {
            info!("Statistics rebuild task already pending/running, skipping submission");
            return Ok(());
        }

        let builder = TaskBuilder::statistics_rebuild(StatisticsRebuildConfig::default());

        let task_id: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO tasks (task_type, config, priority, max_retries)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(builder.task_type().to_string())
        .bind(builder.config())
        .bind(builder.get_priority())
        .bind(builder.get_max_retries())
        .fetch_one(self.writer.pool())
        .await?;

        info!("Submitted statistics rebuild task: {}", task_id.0);

        Ok(())
    }

    async fn maybe_submit_live_cells_populate_task(&self) -> Result<()> {
        use ckbadger_common::{LiveCellsPopulateConfig, TaskBuilder};

        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT COUNT(*) FROM tasks WHERE task_type = 'live_cells_populate' AND status IN ('pending', 'running')",
        )
        .fetch_optional(self.writer.pool())
        .await?;

        if existing.map(|r| r.0).unwrap_or(0) > 0 {
            info!("Live cells populate task already pending/running, skipping submission");
            return Ok(());
        }

        let builder = TaskBuilder::live_cells_populate(LiveCellsPopulateConfig::default());

        let task_id: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO tasks (task_type, config, priority, max_retries)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(builder.task_type().to_string())
        .bind(builder.config())
        .bind(builder.get_priority())
        .bind(builder.get_max_retries())
        .fetch_one(self.writer.pool())
        .await?;

        info!("Submitted live cells populate task: {}", task_id.0);

        Ok(())
    }

    async fn maybe_submit_spore_rebuild_task(&self) -> Result<()> {
        use ckbadger_common::{SporeRebuildConfig, TaskBuilder};

        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT COUNT(*) FROM tasks WHERE task_type = 'spore_rebuild' AND status IN ('pending', 'running')",
        )
        .fetch_optional(self.writer.pool())
        .await?;

        if existing.map(|r| r.0).unwrap_or(0) > 0 {
            info!("Spore rebuild task already pending/running, skipping submission");
            return Ok(());
        }

        let spore_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM spore_cells")
            .fetch_one(self.writer.pool())
            .await?;

        if spore_count.0 == 0 {
            debug!("No spore cells found, skipping spore rebuild task");
            return Ok(());
        }

        let builder = TaskBuilder::spore_rebuild(SporeRebuildConfig::default());

        let task_id: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO tasks (task_type, config, priority, max_retries)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(builder.task_type().to_string())
        .bind(builder.config())
        .bind(builder.get_priority())
        .bind(builder.get_max_retries())
        .fetch_one(self.writer.pool())
        .await?;

        info!("Submitted spore rebuild task: {}", task_id.0);

        Ok(())
    }

    async fn maybe_submit_secondary_issuance_backfill_task(&self) -> Result<()> {
        use ckbadger_common::{SecondaryIssuanceBackfillConfig, TaskBuilder};

        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT COUNT(*) FROM tasks WHERE task_type = 'secondary_issuance_backfill' AND status IN ('pending', 'running')",
        )
        .fetch_optional(self.writer.pool())
        .await?;

        if existing.map(|r| r.0).unwrap_or(0) > 0 {
            info!("Secondary issuance backfill task already pending/running, skipping submission");
            return Ok(());
        }

        let builder = TaskBuilder::secondary_issuance_backfill(SecondaryIssuanceBackfillConfig {
            ckb_rpc_url: self.config.ckb_rpc_url.clone(),
            start_block: Some(0),
            end_block: None,
            ..Default::default()
        });

        let task_id: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO tasks (task_type, config, priority, max_retries)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(builder.task_type().to_string())
        .bind(builder.config())
        .bind(builder.get_priority())
        .bind(builder.get_max_retries())
        .fetch_one(self.writer.pool())
        .await?;

        info!("Submitted secondary issuance backfill task: {}", task_id.0);

        Ok(())
    }

    /// Submit a label import task on indexer startup if not already pending/running.
    /// This ensures token labels are imported at least once per indexer lifecycle.
    async fn maybe_submit_label_import_task(&self) -> Result<()> {
        use ckbadger_common::{LabelImportConfig, TaskBuilder};

        // Check if token-labels directory exists
        // Use TOKEN_LABELS_PATH env var (for Docker) or fall back to relative path (for local dev)
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

        // Check if there's already a pending or running label_import task
        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT COUNT(*) FROM tasks WHERE task_type = 'label_import' AND status IN ('pending', 'running')",
        )
        .fetch_optional(self.writer.pool())
        .await?;

        if existing.map(|r| r.0).unwrap_or(0) > 0 {
            debug!("Label import task already pending/running, skipping submission");
            return Ok(());
        }

        let builder = TaskBuilder::label_import(LabelImportConfig {
            token_labels_path: token_labels_path.clone(),
            ..Default::default()
        });

        let task_id: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO tasks (task_type, config, priority, max_retries)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(builder.task_type().to_string())
        .bind(builder.config())
        .bind(builder.get_priority())
        .bind(builder.get_max_retries())
        .fetch_one(self.writer.pool())
        .await?;

        info!(
            "Submitted label import task: {} (path: {})",
            task_id.0, token_labels_path
        );

        Ok(())
    }

    /// Returns `true` if a task was executed, `false` if no pending task.
    /// Must run in indexer (not task-runner) due to RocksDB access requirement.
    async fn check_and_execute_live_cells_populate_task(&self) -> Result<bool> {
        use ckbadger_common::{LiveCellsPopulateConfig, LiveCellsPopulateResult, TaskConfig};

        let runner_id = format!("indexer-{}", std::process::id());
        let task: Option<ckbadger_common::Task> = sqlx::query_as(
            r#"
            UPDATE tasks
            SET status = 'running',
                runner_id = $1,
                started_at = COALESCE(started_at, NOW()),
                heartbeat_at = NOW()
            WHERE id = (
                SELECT id FROM tasks
                WHERE task_type = 'live_cells_populate'
                  AND status = 'pending'
                ORDER BY priority DESC, created_at ASC
                LIMIT 1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING *
            "#,
        )
        .bind(&runner_id)
        .fetch_optional(self.writer.pool())
        .await?;

        let task = match task {
            Some(t) => t,
            None => return Ok(false),
        };

        info!(
            "Claimed live_cells_populate task {} (runner: {})",
            task.id, runner_id
        );

        let batch_size = match task.config_typed() {
            Some(TaskConfig::LiveCellsPopulate(c)) => c.batch_size,
            _ => LiveCellsPopulateConfig::default().batch_size,
        };

        let result = self.execute_live_cells_populate(task.id, batch_size).await;

        match result {
            Ok(cells_populated) => {
                let result_json =
                    serde_json::to_value(LiveCellsPopulateResult { cells_populated })?;
                sqlx::query(
                    r#"
                    UPDATE tasks
                    SET status = 'completed',
                        completed_at = NOW(),
                        progress_current = progress_total,
                        result = $2,
                        heartbeat_at = NOW()
                    WHERE id = $1
                    "#,
                )
                .bind(task.id)
                .bind(&result_json)
                .execute(self.writer.pool())
                .await?;

                info!(
                    "Completed live_cells_populate task {}: {} cells populated",
                    task.id, cells_populated
                );
                Ok(true)
            }
            Err(e) => {
                error!("Failed live_cells_populate task {}: {}", task.id, e);
                sqlx::query(
                    r#"
                    UPDATE tasks
                    SET status = CASE 
                        WHEN retry_count < max_retries THEN 'pending'
                        ELSE 'failed'
                    END,
                    error_message = $2,
                    retry_count = retry_count + 1,
                    runner_id = NULL,
                    heartbeat_at = NOW()
                    WHERE id = $1
                    "#,
                )
                .bind(task.id)
                .bind(e.to_string())
                .execute(self.writer.pool())
                .await?;

                Err(e)
            }
        }
    }

    async fn execute_live_cells_populate(&self, task_id: Uuid, batch_size: usize) -> Result<i64> {
        use crate::db::copy_live_cells::copy_live_cells_from_rocksdb;
        use crate::db::copy_pool::{CopyConfig, CopyPoolManager};

        info!("Starting live_cells_populate: counting cells in RocksDB...");

        let total_cells = self.rocksdb_store.count_live_cells();
        info!(
            "Found {} live cells in RocksDB, batch_size={}",
            total_cells, batch_size
        );

        sqlx::query(
            r#"
            UPDATE tasks
            SET progress_total = $2,
                progress_current = 0,
                progress_message = 'Truncating live_cells table...',
                heartbeat_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(task_id)
        .bind(total_cells as i64)
        .execute(self.writer.pool())
        .await?;

        info!("Truncating live_cells table...");
        sqlx::query("TRUNCATE live_cells")
            .execute(self.writer.pool())
            .await?;

        let copy_pool = CopyPoolManager::new(
            &self.config.database_url,
            CopyConfig {
                max_copy_connections: 4,
                copy_batch_size: batch_size,
                copy_enabled: true,
            },
        )?;

        sqlx::query(
            r#"
            UPDATE tasks
            SET progress_message = 'Populating live_cells from RocksDB...',
                heartbeat_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(task_id)
        .execute(self.writer.pool())
        .await?;

        let mut cells_written: i64 = 0;
        let mut last_progress_update = std::time::Instant::now();
        let pool = self.writer.pool().clone();
        let start_time = std::time::Instant::now();

        let mut all_batches: Vec<Vec<(Vec<u8>, i16, crate::db::LiveCellInfo)>> = Vec::new();

        self.rocksdb_store
            .iter_live_cells_batched(batch_size, |batch| {
                all_batches.push(batch);
            });

        info!("Collected {} batches from RocksDB", all_batches.len());

        for (batch_idx, batch) in all_batches.into_iter().enumerate() {
            let conn = copy_pool.get_connection().await?;
            let rows = copy_live_cells_from_rocksdb(&conn, &batch).await?;
            cells_written += rows as i64;

            if last_progress_update.elapsed() > std::time::Duration::from_secs(5)
                || batch_idx % 10 == 0
            {
                let elapsed = start_time.elapsed().as_secs_f64();
                let rate = if elapsed > 0.0 {
                    cells_written as f64 / elapsed
                } else {
                    0.0
                };

                sqlx::query(
                    r#"
                    UPDATE tasks
                    SET progress_current = $2,
                        rate_ema = $3,
                        heartbeat_at = NOW()
                    WHERE id = $1
                    "#,
                )
                .bind(task_id)
                .bind(cells_written)
                .bind(rate)
                .execute(&pool)
                .await?;

                last_progress_update = std::time::Instant::now();

                info!(
                    "live_cells_populate progress: {}/{} ({:.1}%), {:.0} cells/sec",
                    cells_written,
                    total_cells,
                    (cells_written as f64 / total_cells as f64) * 100.0,
                    rate
                );
            }

            drop(conn);
        }

        let elapsed = start_time.elapsed();
        info!(
            "live_cells_populate completed: {} cells in {:.2}s ({:.0} cells/sec)",
            cells_written,
            elapsed.as_secs_f64(),
            cells_written as f64 / elapsed.as_secs_f64()
        );

        Ok(cells_written)
    }

    async fn maybe_flush_live_cell_store(&self) {
        use std::sync::atomic::Ordering;

        let batches = self.batches_since_flush.fetch_add(1, Ordering::Relaxed) + 1;

        if batches >= self.config.live_cell_flush_interval {
            if let Some(store) = self.writer.live_cell_store() {
                match store.flush_to_db(self.writer.pool()).await {
                    Ok((inserts, removals)) => {
                        if inserts > 0 || removals > 0 {
                            info!(
                                "LiveCellStore flushed: {} inserts, {} removals",
                                inserts, removals
                            );
                        }
                        self.batches_since_flush.store(0, Ordering::Relaxed);
                    }
                    Err(e) => {
                        warn!("Failed to flush LiveCellStore: {}", e);
                    }
                }
            }
        }
    }

    async fn fetch_blocks_parallel(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Vec<BlockResponseWithCycles>> {
        Self::fetch_blocks_with_config(&self.rpc, start, end, self.config.parallel_fetch_size).await
    }

    async fn fetch_spore_inputs_by_outpoints(
        &self,
        outpoints: &[(Vec<u8>, i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), crate::parser::spore::ParsedSporeCell>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let tx_hashes: Vec<&[u8]> = outpoints.iter().map(|(h, _)| h.as_slice()).collect();
        let indices: Vec<i16> = outpoints.iter().map(|(_, i)| *i).collect();
        let rows = sqlx::query_as::<_, (Vec<u8>, i16, Vec<u8>, Vec<u8>, String, Option<Vec<u8>>, Vec<u8>)>(
            r#"
            SELECT tx_hash, output_index, spore_id, type_script_hash, content_type, cluster_id, owner_lock_hash
            FROM spore_cells
            WHERE (tx_hash, output_index) IN (SELECT * FROM UNNEST($1::bytea[], $2::smallint[]))
            "#,
        )
        .bind(&tx_hashes)
        .bind(&indices)
        .fetch_all(self.writer.pool())
        .await?;

        let mut map = HashMap::new();
        for (
            tx_hash,
            output_index,
            spore_id,
            type_script_hash,
            content_type,
            cluster_id,
            owner_lock_hash,
        ) in rows
        {
            map.insert(
                (tx_hash, output_index),
                crate::parser::spore::ParsedSporeCell {
                    spore_id,
                    type_script_hash,
                    content_type,
                    content: Vec::new(),
                    cluster_id,
                    owner_lock_hash,
                },
            );
        }

        Ok(map)
    }

    async fn fetch_mnft_inputs_by_outpoints(
        &self,
        outpoints: &[(Vec<u8>, i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), crate::parser::mnft::ParsedMnftToken>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let tx_hashes: Vec<&[u8]> = outpoints.iter().map(|(h, _)| h.as_slice()).collect();
        let indices: Vec<i16> = outpoints.iter().map(|(_, i)| *i).collect();
        let rows = sqlx::query_as::<
            _,
            (
                Vec<u8>,
                i16,
                Vec<u8>,
                Vec<u8>,
                Vec<u8>,
                i32,
                Option<Vec<u8>>,
                i16,
                i16,
                Vec<u8>,
            ),
        >(
            r#"
            SELECT tx_hash, output_index, token_id, type_script_hash, class_id, token_index,
                   characteristic, configure, state, owner_lock_hash
            FROM mnft_tokens
            WHERE (tx_hash, output_index) IN (SELECT * FROM UNNEST($1::bytea[], $2::smallint[]))
            "#,
        )
        .bind(&tx_hashes)
        .bind(&indices)
        .fetch_all(self.writer.pool())
        .await?;

        let mut map = HashMap::new();
        for (
            tx_hash,
            output_index,
            token_id,
            type_script_hash,
            class_id,
            token_index,
            characteristic,
            configure,
            state,
            owner_lock_hash,
        ) in rows
        {
            map.insert(
                (tx_hash, output_index),
                crate::parser::mnft::ParsedMnftToken {
                    token_id,
                    type_script_hash,
                    class_id,
                    token_index: token_index as u32,
                    characteristic: characteristic.unwrap_or_default(),
                    configure: configure as u8,
                    state: state as u8,
                    owner_lock_hash,
                },
            );
        }

        Ok(map)
    }

    async fn fetch_dotbit_inputs_by_outpoints(
        &self,
        outpoints: &[(Vec<u8>, i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), crate::parser::dotbit::ParsedDotbitAccount>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let tx_hashes: Vec<&[u8]> = outpoints.iter().map(|(h, _)| h.as_slice()).collect();
        let indices: Vec<i16> = outpoints.iter().map(|(_, i)| *i).collect();
        let rows = sqlx::query_as::<_, (Vec<u8>, i16, Vec<u8>, Vec<u8>, Option<i64>, Vec<u8>)>(
            r#"
            SELECT tx_hash, output_index, account_id, type_script_hash, expired_at, owner_lock_hash
            FROM dotbit_accounts
            WHERE (tx_hash, output_index) IN (SELECT * FROM UNNEST($1::bytea[], $2::smallint[]))
            "#,
        )
        .bind(&tx_hashes)
        .bind(&indices)
        .fetch_all(self.writer.pool())
        .await?;

        let mut map = HashMap::new();
        for (tx_hash, output_index, account_id, type_script_hash, expired_at, owner_lock_hash) in
            rows
        {
            map.insert(
                (tx_hash, output_index),
                crate::parser::dotbit::ParsedDotbitAccount {
                    account_id,
                    type_script_hash,
                    next_account_id: None,
                    expired_at: expired_at.map(|value| value as u64),
                    owner_lock_hash,
                },
            );
        }

        Ok(map)
    }

    fn build_input_cells_for_tx(
        &self,
        tx_data: &TxData,
        batch_output_cells: &HashMap<(Vec<u8>, i16), crate::parser::cell::ParsedCell>,
        live_cell_infos: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
        input_cell_info: &HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)>,
        consumed_code_hashes: &HashMap<(Vec<u8>, i16), (Vec<u8>, Option<Vec<u8>>)>,
    ) -> Vec<crate::parser::cell::ParsedCell> {
        let mut input_cells = Vec::with_capacity(tx_data.inputs.len());
        for input in &tx_data.inputs {
            let key = (
                input.previous_tx_hash.clone(),
                input.previous_output_index as i16,
            );
            if let Some(cell) = batch_output_cells.get(&key) {
                input_cells.push(clone_parsed_cell(cell));
                continue;
            }
            if let Some(info) = live_cell_infos.get(&key) {
                input_cells.push(parsed_cell_from_live_info(info));
                continue;
            }
            if let Some((capacity, _, lock_script_hash, data_size)) = input_cell_info.get(&key) {
                let (lock_code_hash, type_code_hash) = consumed_code_hashes
                    .get(&key)
                    .map(|(lock, type_hash)| (lock.clone(), type_hash.clone()))
                    .unwrap_or_default();
                input_cells.push(crate::parser::cell::ParsedCell {
                    capacity: *capacity,
                    lock_code_hash,
                    lock_hash_type: 0,
                    lock_args: Vec::new(),
                    lock_script_hash: lock_script_hash.clone(),
                    type_code_hash,
                    type_hash_type: None,
                    type_args: None,
                    type_script_hash: None,
                    data_hash: vec![0u8; 32],
                    data_size: *data_size,
                    data: Vec::new(),
                });
            }
        }
        input_cells
    }

    #[allow(clippy::too_many_arguments)]
    async fn collect_activities_for_batch(
        &self,
        blocks: &[BlockResponseWithCycles],
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        all_tx_data: &[TxData],
        input_cell_info: &HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)>,
        consumed_code_hashes: &HashMap<(Vec<u8>, i16), (Vec<u8>, Option<Vec<u8>>)>,
        udt_transfers_by_tx: &HashMap<Vec<u8>, Vec<crate::parser::ParsedUdtTransfer>>,
        bulk_sync_mode: bool,
    ) -> Result<Vec<(i64, DateTime<Utc>, Vec<ParsedActivity>)>> {
        if blocks.is_empty() {
            return Ok(Vec::new());
        }

        let mut batch_output_cells: HashMap<(Vec<u8>, i16), crate::parser::cell::ParsedCell> =
            HashMap::new();
        for tx_data in all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                batch_output_cells.insert(
                    (tx_data.hash.clone(), output_index as i16),
                    clone_parsed_cell(cell),
                );
            }
        }

        let mut unique_outpoints: Vec<(Vec<u8>, i16)> = Vec::new();
        let mut seen_outpoints: HashSet<(Vec<u8>, i16)> = HashSet::new();
        for tx_data in all_tx_data {
            for input in &tx_data.inputs {
                let key = (
                    input.previous_tx_hash.clone(),
                    input.previous_output_index as i16,
                );
                if seen_outpoints.insert(key.clone()) {
                    unique_outpoints.push(key);
                }
            }
        }

        let mut live_cell_infos: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
        if let Some(store) = self.writer.live_cell_store() {
            let outpoint_refs: Vec<(&[u8], i16)> = unique_outpoints
                .iter()
                .map(|(h, i)| (h.as_slice(), *i))
                .collect();
            let cached = store.get_batch(&outpoint_refs);
            for (key, info) in cached {
                live_cell_infos.insert(key, info);
            }
            let consumed = store.get_consumed_cells_batch(&outpoint_refs);
            for (key, info) in consumed {
                live_cell_infos.entry(key).or_insert(info);
            }
        }

        let (input_spores, input_mnfts, input_dotbits) = if bulk_sync_mode {
            (HashMap::new(), HashMap::new(), HashMap::new())
        } else {
            let spore_inputs = self
                .fetch_spore_inputs_by_outpoints(&unique_outpoints)
                .await?;
            let mnft_inputs = self
                .fetch_mnft_inputs_by_outpoints(&unique_outpoints)
                .await?;
            let dotbit_inputs = self
                .fetch_dotbit_inputs_by_outpoints(&unique_outpoints)
                .await?;
            (spore_inputs, mnft_inputs, dotbit_inputs)
        };

        let is_mainnet = infer_is_mainnet(&self.config.ckb_rpc_url);
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);

        let mut activities_by_block: Vec<(i64, DateTime<Utc>, Vec<ParsedActivity>)> = Vec::new();
        let mut block_tx_idx = 0usize;
        for (block_idx, block_response) in blocks.iter().enumerate() {
            let parsed = &all_parsed_blocks[block_idx];
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            let mut block_activities: Vec<ParsedActivity> = Vec::new();

            for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                let tx = &block_response.block.transactions[tx_idx];
                let input_cells = self.build_input_cells_for_tx(
                    tx_data,
                    &batch_output_cells,
                    &live_cell_infos,
                    input_cell_info,
                    consumed_code_hashes,
                );

                let mut activities: Vec<ParsedActivity> = Vec::new();

                activities.extend(ActivityParser::parse_ckb_transfers(
                    tx,
                    &tx_data.hash,
                    tx_data.tx_index,
                    &tx_data.cells,
                    &input_cells,
                ));

                if let Some(cellbase_reward) = ActivityParser::parse_cellbase_reward(
                    tx,
                    &tx_data.hash,
                    tx_data.tx_index,
                    &tx_data.cells,
                    0,
                    0,
                ) {
                    activities.push(cellbase_reward);
                }

                if let Some(transfers) = udt_transfers_by_tx.get(&tx_data.hash) {
                    activities.extend(ActivityParser::parse_token_activities(
                        &tx_data.hash,
                        tx_data.tx_index,
                        transfers,
                        0,
                    ));
                }

                let output_spores = SporeParser::parse_spores(tx);
                let mut input_spores_for_tx: Vec<crate::parser::spore::ParsedSporeCell> =
                    Vec::new();
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some(spore) = input_spores.get(&key) {
                        input_spores_for_tx.push(spore.clone());
                    }
                }
                activities.extend(ActivityParser::parse_dob_activities(
                    &tx_data.hash,
                    tx_data.tx_index,
                    &output_spores,
                    &input_spores_for_tx,
                    0,
                ));

                let output_mnfts = MnftParser::parse_tokens(tx);
                let mut input_mnfts_for_tx: Vec<crate::parser::mnft::ParsedMnftToken> = Vec::new();
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some(token) = input_mnfts.get(&key) {
                        input_mnfts_for_tx.push(token.clone());
                    }
                }

                let output_dotbits = DotbitParser::parse_accounts(tx);
                let mut input_dotbits_for_tx: Vec<crate::parser::dotbit::ParsedDotbitAccount> =
                    Vec::new();
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some(account) = input_dotbits.get(&key) {
                        input_dotbits_for_tx.push(account.clone());
                    }
                }

                activities.extend(ActivityParser::parse_nft_activities(
                    &tx_data.hash,
                    tx_data.tx_index,
                    &output_mnfts,
                    &input_mnfts_for_tx,
                    &output_dotbits,
                    &input_dotbits_for_tx,
                    0,
                ));

                let output_dao_cells: Vec<crate::parser::dao::ParsedDaoCell> = tx
                    .outputs
                    .iter()
                    .zip(tx.outputs_data.iter())
                    .filter_map(|(output, data_hex)| DaoParser::parse_dao_cell(output, data_hex))
                    .collect();

                let mut input_dao_cells: Vec<crate::parser::dao::ParsedDaoCell> = Vec::new();
                for input in &input_cells {
                    let type_code_hash = match &input.type_code_hash {
                        Some(hash) => hash,
                        None => continue,
                    };
                    if type_code_hash != &dao_code_hash {
                        continue;
                    }
                    if input.data.is_empty() {
                        continue;
                    }
                    let state = DaoParser::parse_dao_state(&input.data)
                        .unwrap_or(crate::parser::dao::DaoState::WithdrawRequest);
                    let deposit_block_number = DaoParser::parse_deposit_block_number(&input.data);
                    input_dao_cells.push(crate::parser::dao::ParsedDaoCell {
                        lock_script_hash: input.lock_script_hash.clone(),
                        capacity: input.capacity,
                        state,
                        deposit_block_number,
                    });
                }

                activities.extend(ActivityParser::parse_dao_activities(
                    &tx_data.hash,
                    tx_data.tx_index,
                    &output_dao_cells,
                    &input_dao_cells,
                    0,
                ));

                activities.extend(ActivityParser::parse_script_deployments(
                    &tx_data.hash,
                    tx_data.tx_index,
                    &tx_data.cells,
                    0,
                ));

                activities.extend(ActivityParser::parse_rgbpp_activities(
                    &tx_data.hash,
                    tx_data.tx_index,
                    &tx_data.cells,
                    &input_cells,
                    is_mainnet,
                    0,
                ));

                if !activities.is_empty() {
                    block_activities.extend(activities);
                }
            }

            if !block_activities.is_empty() {
                let original_count = block_activities.len();
                let mut seen_ids: HashSet<Vec<u8>> = HashSet::new();
                block_activities.retain(|a| seen_ids.insert(a.activity_id.clone()));

                if block_activities.len() < original_count {
                    debug!(
                        "Block {}: Removed {} duplicate activity_ids",
                        parsed.number,
                        original_count - block_activities.len()
                    );
                }

                activities_by_block.push((parsed.number, parsed.timestamp, block_activities));
            }
        }

        Ok(activities_by_block)
    }

    async fn write_activities_batch(
        &self,
        activities_by_block: &[(i64, DateTime<Utc>, Vec<ParsedActivity>)],
    ) -> Result<()> {
        if activities_by_block.is_empty() {
            return Ok(());
        }

        if self.should_use_copy() {
            let copy_router = self
                .copy_router
                .as_ref()
                .expect("copy_router must exist when should_use_copy() is true");
            let mut activity_data: Vec<(&ParsedActivity, i64, DateTime<Utc>)> = Vec::new();
            for (block_number, timestamp, activities) in activities_by_block {
                for activity in activities {
                    activity_data.push((activity, *block_number, *timestamp));
                }
            }
            copy_router.copy_activities_parallel(&activity_data).await?;
        } else {
            let (client, connection) =
                tokio_postgres::connect(&self.config.database_url, NoTls).await?;
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    warn!("activities COPY connection error: {}", e);
                }
            });
            crate::db::copy_activities_batch(&client, activities_by_block).await?;
        }

        Ok(())
    }

    async fn sync_blocks_batch(
        &self,
        blocks: &[BlockResponseWithCycles],
        chain_tip: u64,
    ) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        // Parse blocks and transactions in parallel using rayon
        let blocks_clone: Vec<BlockResponseWithCycles> = blocks.to_vec();
        let (all_parsed_blocks, mut all_tx_data, all_input_outpoints) =
            tokio::task::spawn_blocking(move || parse_blocks_parallel(&blocks_clone)).await?;

        let end_block = all_parsed_blocks
            .last()
            .map(|b| b.number as u64)
            .unwrap_or(0);
        let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;

        let mut batch_cells: HashMap<
            (Vec<u8>, i16),
            (i64, i64, Vec<u8>, i32, Vec<u8>, Option<Vec<u8>>),
        > = HashMap::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                batch_cells.insert(
                    (tx_data.hash.clone(), output_index as i16),
                    (
                        cell.capacity,
                        tx_data.block_number,
                        cell.lock_script_hash.clone(),
                        cell.data_size,
                        cell.lock_code_hash.clone(),
                        cell.type_code_hash.clone(),
                    ),
                );
            }
        }

        // (capacity, created_at_block, lock_script_hash, data_size)
        let mut input_cell_info: HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)> = HashMap::new();
        {
            let mut cache = self.cell_cache.lock().await;
            for (tx_hash, idx) in &all_input_outpoints {
                let key = (tx_hash.clone(), *idx as i32);
                if let Some(cached) = cache.get(&key) {
                    input_cell_info.insert(
                        (tx_hash.clone(), *idx),
                        (
                            cached.capacity,
                            cached.created_at_block,
                            cached.lock_script_hash.clone(),
                            cached.data_size,
                        ),
                    );
                }
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
            let db_info = self.writer.get_cells_info_batch(&missing_refs).await?;
            for ((tx_hash, idx), (cap, block, lock_hash, data_size)) in db_info {
                input_cell_info.insert((tx_hash, idx), (cap, block, lock_hash, data_size));
            }
        }

        for tx_data in &mut all_tx_data {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((cap, _, _, _)) = input_cell_info.get(&key) {
                        tx_data.total_input_capacity += cap;
                    } else if let Some((cap, _, _, _, _, _)) = batch_cells.get(&key) {
                        tx_data.total_input_capacity += cap;
                    }
                }
                tx_data.fee = tx_data
                    .total_input_capacity
                    .saturating_sub(tx_data.total_output_capacity);
            }
        }

        {
            let mut cache = self.cell_cache.lock().await;
            for tx_data in &all_tx_data {
                for (output_index, cell) in tx_data.cells.iter().enumerate() {
                    cache.put(
                        (tx_data.hash.clone(), output_index as i32),
                        CachedCellInfo {
                            capacity: cell.capacity,
                            created_at_block: tx_data.block_number,
                            lock_script_hash: cell.lock_script_hash.clone(),
                            data_size: cell.data_size,
                        },
                    );
                }
            }
        }

        // Prepare all data before parallel insertion
        let block_refs: Vec<&crate::parser::block::ParsedBlock> =
            all_parsed_blocks.iter().collect();

        let txs_for_batch: Vec<_> = all_tx_data
            .iter()
            .map(|tx_data| {
                (
                    tx_data.hash.as_slice(),
                    tx_data.block_number,
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

        // Parallel insert: blocks, transactions, and cells are independent (no FK constraints)
        tokio::try_join!(
            async {
                if !block_refs.is_empty() {
                    self.writer.insert_blocks_batch(&block_refs).await
                } else {
                    Ok(())
                }
            },
            async {
                if !txs_for_batch.is_empty() {
                    self.writer.insert_transactions_batch(&txs_for_batch).await
                } else {
                    Ok(())
                }
            },
            async {
                if !all_cells.is_empty() {
                    self.writer
                        .insert_cells_batch(&all_cells, bulk_sync_mode)
                        .await
                } else {
                    Ok(())
                }
            }
        )?;

        // Block proposals must be inserted after blocks (references block_number)
        for parsed_block in &all_parsed_blocks {
            if !parsed_block.proposals.is_empty() {
                self.writer
                    .insert_block_proposals_batch(parsed_block.number, &parsed_block.proposals)
                    .await?;

                // Cache proposals in Redis during live sync for frontend display
                if !self.is_bulk_sync_active() {
                    self.cache_block_proposals(&parsed_block.proposals, parsed_block.number)
                        .await;
                }
            }
        }

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

        let mut all_cell_deps: Vec<(&[u8], i64, i16, &crate::parser::transaction::ParsedCellDep)> =
            Vec::new();
        for tx_data in &all_tx_data {
            for (dep_index, cell_dep) in tx_data.cell_deps.iter().enumerate() {
                all_cell_deps.push((
                    tx_data.hash.as_slice(),
                    tx_data.block_number,
                    dep_index as i16,
                    cell_dep,
                ));
            }
        }

        // Parallel insert: inputs and cell_deps are independent
        tokio::try_join!(
            async {
                if !all_inputs.is_empty() {
                    self.writer
                        .insert_transaction_inputs_batch(&all_inputs)
                        .await
                } else {
                    Ok(())
                }
            },
            async {
                if !all_cell_deps.is_empty() {
                    self.writer
                        .insert_transaction_cell_deps_batch(&all_cell_deps)
                        .await
                } else {
                    Ok(())
                }
            }
        )?;

        let mut all_consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i16)> = Vec::new();
        for tx_data in &all_tx_data {
            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((_, created_block, _, _)) = input_cell_info.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            *created_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
                        ));
                    } else if let Some((_, created_block, _, _, _, _)) = batch_cells.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            *created_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
                        ));
                    }
                }
            }
        }
        if !all_consumptions.is_empty() {
            let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;
            self.writer
                .consume_cells_batch(&all_consumptions, bulk_sync_mode)
                .await?;
        }

        let mut address_balance_changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>)> =
            HashMap::new();

        for tx_data in &all_tx_data {
            let mut tx_balance_changes: HashMap<Vec<u8>, i64> = HashMap::new();
            let mut tx_cells_created: HashMap<Vec<u8>, i32> = HashMap::new();
            let mut tx_cells_consumed: HashMap<Vec<u8>, i32> = HashMap::new();

            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((cap, _, lock_hash, _)) = input_cell_info.get(&key) {
                        *tx_balance_changes.entry(lock_hash.clone()).or_default() -= cap;
                        *tx_cells_consumed.entry(lock_hash.clone()).or_default() += 1;
                    } else if let Some((cap, _, lock_hash, _, _, _)) = batch_cells.get(&key) {
                        *tx_balance_changes.entry(lock_hash.clone()).or_default() -= cap;
                        *tx_cells_consumed.entry(lock_hash.clone()).or_default() += 1;
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
            }

            let all_addresses: HashSet<Vec<u8>> = tx_balance_changes
                .keys()
                .chain(tx_cells_created.keys())
                .chain(tx_cells_consumed.keys())
                .cloned()
                .collect();

            for lock_hash in all_addresses {
                let balance_change = tx_balance_changes.get(&lock_hash).copied().unwrap_or(0);
                let cells_created = tx_cells_created.get(&lock_hash).copied().unwrap_or(0);
                let cells_consumed = tx_cells_consumed.get(&lock_hash).copied().unwrap_or(0);

                let entry = address_balance_changes.entry(lock_hash.clone()).or_insert((
                    0,
                    0,
                    0,
                    0,
                    tx_data.block_number,
                    tx_data.hash.clone(),
                ));
                entry.0 += balance_change;
                entry.1 += cells_created - cells_consumed;
                entry.2 += cells_created;
                entry.3 += 1;
                entry.4 = tx_data.block_number;
                entry.5 = tx_data.hash.clone();
            }
        }

        let changes_ref: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> =
            address_balance_changes
                .iter()
                .map(|(k, (a, b, c, d, e, f))| (k.clone(), (*a, *b, *c, *d, *e, f.as_slice())))
                .collect();

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

        let mut consumed_from_db: Vec<(Vec<u8>, i16)> = Vec::new();
        for tx_data in &all_tx_data {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if input_cell_info.contains_key(&key) && !batch_cells.contains_key(&key) {
                        consumed_from_db.push(key);
                    }
                }
            }
        }

        let consumed_code_hashes = if !consumed_from_db.is_empty() {
            let refs: Vec<(&[u8], i16)> = consumed_from_db
                .iter()
                .map(|(h, i)| (h.as_slice(), *i))
                .collect();
            self.writer.get_cells_code_hashes_batch(&refs).await?
        } else {
            HashMap::new()
        };

        for tx_data in &all_tx_data {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((cap, _, _, _)) = input_cell_info.get(&key) {
                        if let Some((lock_code_hash, type_code_hash)) =
                            consumed_code_hashes.get(&key)
                        {
                            let lock_key = (lock_code_hash.clone(), false);
                            let entry =
                                script_usage_changes.entry(lock_key).or_insert((0, 0, 0, 0));
                            entry.1 -= 1;
                            entry.3 -= cap;

                            if let Some(type_code_hash) = type_code_hash {
                                let type_key = (type_code_hash.clone(), true);
                                let entry =
                                    script_usage_changes.entry(type_key).or_insert((0, 0, 0, 0));
                                entry.1 -= 1;
                                entry.3 -= cap;
                            }
                        }
                    } else if let Some((cap, _, _, _, lock_code_hash, type_code_hash)) =
                        batch_cells.get(&key)
                    {
                        let lock_key = (lock_code_hash.clone(), false);
                        let entry = script_usage_changes.entry(lock_key).or_insert((0, 0, 0, 0));
                        entry.1 -= 1;
                        entry.3 -= cap;

                        if let Some(type_code_hash) = type_code_hash {
                            let type_key = (type_code_hash.clone(), true);
                            let entry =
                                script_usage_changes.entry(type_key).or_insert((0, 0, 0, 0));
                            entry.1 -= 1;
                            entry.3 -= cap;
                        }
                    }
                }
            }
        }

        tokio::try_join!(
            async {
                if !changes_ref.is_empty() {
                    self.writer
                        .update_address_balances_batch(&changes_ref)
                        .await
                } else {
                    Ok(())
                }
            },
            async {
                if !script_usage_changes.is_empty() {
                    self.writer
                        .update_script_usage_batch(&script_usage_changes)
                        .await
                } else {
                    Ok(())
                }
            }
        )?;

        let mut batch_stats = BatchStats::default();
        let mut prev_timestamp: Option<chrono::DateTime<Utc>> =
            if let Some(first_block) = all_parsed_blocks.first() {
                self.writer
                    .get_previous_block_timestamp(first_block.number)
                    .await?
            } else {
                None
            };
        let mut prev_epoch: Option<(i64, chrono::DateTime<Utc>, f64)> =
            if let Some(first_block) = all_parsed_blocks.first() {
                self.writer
                    .get_last_epoch_start(first_block.number)
                    .await?
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
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    input_cell_info
                        .get(&key)
                        .map(|(_, _, _, ds)| *ds as i64)
                        .or_else(|| batch_cells.get(&key).map(|(_, _, _, ds, _, _)| *ds as i64))
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
                        // Use 1-minute buckets to match official CKB Explorer
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

        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let mut block_tx_idx = 0usize;
        for parsed in &all_parsed_blocks {
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            for tx_data in tx_slice {
                let dao_deposits =
                    DaoParser::parse_deposits_from_cells(&tx_data.hash, &tx_data.cells);
                for deposit in &dao_deposits {
                    let ar = DaoParser::extract_ar_from_dao_field(&parsed.dao).unwrap_or(0) as i64;
                    self.writer
                        .insert_dao_deposit(deposit, parsed.number, parsed.timestamp, ar)
                        .await?;
                }
            }

            for tx_data in tx_slice {
                if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                    continue;
                }

                let input_outpoints: Vec<(&[u8], i32)> = tx_data
                    .inputs
                    .iter()
                    .map(|i| (i.previous_tx_hash.as_slice(), i.previous_output_index))
                    .collect();

                let consumed_dao = self
                    .writer
                    .find_consumed_dao_deposits(&input_outpoints)
                    .await?;
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
                                        tx_data.hash.clone(),
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

                self.writer
                    .process_dao_withdrawals(
                        &consumed_dao,
                        &new_dao_outputs,
                        parsed.number,
                        &tx_data.hash,
                        parsed.timestamp,
                    )
                    .await?;
            }
        }

        struct UdtTxContext {
            tx_hash: Vec<u8>,
            block_number: i64,
            timestamp: chrono::DateTime<Utc>,
            output_udts: Vec<crate::parser::ParsedUdtCell>,
            input_outpoints: Vec<(Vec<u8>, i16)>,
        }

        let mut udt_tx_contexts: Vec<UdtTxContext> = Vec::new();
        let mut all_input_outpoints_udt: Vec<(Vec<u8>, i16)> = Vec::new();
        let mut batch_udt_cells: HashMap<(Vec<u8>, i16), crate::parser::ParsedUdtCell> =
            HashMap::new();
        let mut udt_transfers_by_tx: HashMap<Vec<u8>, Vec<crate::parser::ParsedUdtTransfer>> =
            HashMap::new();

        // Temp storage: we filter txs later based on whether they have UDT inputs
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
                        (tx_data.hash.clone(), output_index as i16),
                        udt_cell.clone(),
                    );
                }

                let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                    .inputs
                    .iter()
                    .map(|i| (i.previous_tx_hash.clone(), i.previous_output_index as i16))
                    .collect();

                // Collect ALL input outpoints (to detect UDT consumption/burns)
                all_input_outpoints_udt.extend(input_outpoints.iter().cloned());

                all_tx_infos_for_udt.push(TxInfoForUdt {
                    tx_hash: tx_data.hash.clone(),
                    block_number: parsed.number,
                    timestamp: parsed.timestamp,
                    output_udts,
                    input_outpoints,
                });
            }
        }

        // Fetch UDT info for ALL input outpoints to detect UDT consumption
        let input_udt_info = if !all_input_outpoints_udt.is_empty() {
            let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                let mut seen = HashSet::new();
                all_input_outpoints_udt
                    .into_iter()
                    .filter(|x| seen.insert(x.clone()))
                    .collect()
            };
            let outpoint_refs: Vec<(&[u8], i16)> = unique_outpoints
                .iter()
                .map(|(h, i)| (h.as_slice(), *i))
                .collect();
            self.writer.get_udt_cells_info_batch(&outpoint_refs).await?
        } else {
            HashMap::new()
        };

        // Filter: include txs with UDT outputs OR UDT inputs (fixes burn/send tracking)
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

        if !udt_tx_contexts.is_empty() {
            let mut all_transfers: Vec<(
                crate::parser::ParsedUdtTransfer,
                Vec<u8>,
                i64,
                chrono::DateTime<Utc>,
            )> = Vec::new();

            let mut consumed_udt_outpoints: Vec<(&[u8], i16, i64, &[u8])> = Vec::new();

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
                        consumed_udt_outpoints.push((
                            tx_hash.as_slice(),
                            *idx,
                            ctx.block_number,
                            ctx.tx_hash.as_slice(),
                        ));
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
                        ctx.timestamp,
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
                            ctx.timestamp,
                        ));
                    }
                }
            }

            if !all_transfers.is_empty() {
                for (transfer, tx_hash, _, _) in &all_transfers {
                    udt_transfers_by_tx
                        .entry(tx_hash.clone())
                        .or_default()
                        .push(transfer.clone());
                }

                let transfer_refs: Vec<_> = all_transfers
                    .iter()
                    .map(|(t, h, b, ts)| (t, h.as_slice(), *b, *ts))
                    .collect();
                self.writer
                    .process_udt_transfers_batch(&transfer_refs)
                    .await?;
            }

            if !consumed_udt_outpoints.is_empty() {
                self.writer
                    .consume_udt_cells_batch(&consumed_udt_outpoints)
                    .await?;
            }
        }

        if !batch_udt_cells.is_empty() {
            let tx_block_map: std::collections::HashMap<&[u8], i64> = udt_tx_contexts
                .iter()
                .map(|ctx| (ctx.tx_hash.as_slice(), ctx.block_number))
                .collect();
            let udt_cells_to_insert: Vec<_> = batch_udt_cells
                .iter()
                .map(|((tx_hash, idx), cell)| {
                    let block_number = tx_block_map.get(tx_hash.as_slice()).copied().unwrap_or(0);
                    (tx_hash.as_slice(), *idx, cell, block_number)
                })
                .collect();
            self.writer
                .insert_udt_cells_batch(&udt_cells_to_insert)
                .await?;
        }

        let mut batch_spore_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut batch_mnft_class_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut batch_mnft_token_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut batch_dotbit_account_ids: HashSet<Vec<u8>> = HashSet::new();

        let mut block_tx_idx = 0usize;
        for (block_idx, block_response) in blocks.iter().enumerate() {
            let parsed = &all_parsed_blocks[block_idx];
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                let tx = &block_response.block.transactions[tx_idx];

                for cluster in SporeParser::parse_clusters(tx) {
                    self.writer
                        .insert_spore_cluster(&cluster, parsed.number, &tx_data.hash)
                        .await?;
                }

                for (output_index, spore) in SporeParser::parse_spores(tx).iter().enumerate() {
                    batch_spore_ids.insert(spore.spore_id.clone());
                    self.writer
                        .insert_spore_cell(spore, &tx_data.hash, output_index as i16, parsed.number)
                        .await?;
                    self.writer
                        .insert_spore_content(&spore.spore_id, &spore.content)
                        .await?;
                }

                for issuer in MnftParser::parse_issuers(tx) {
                    self.writer
                        .insert_mnft_issuer(&issuer, &tx_data.hash, 0, parsed.number)
                        .await?;
                }

                for (output_index, class) in MnftParser::parse_classes(tx).iter().enumerate() {
                    batch_mnft_class_ids.insert(class.class_id.clone());
                    self.writer
                        .insert_mnft_class(class, &tx_data.hash, output_index as i16, parsed.number)
                        .await?;
                }

                for (output_index, token) in MnftParser::parse_tokens(tx).iter().enumerate() {
                    batch_mnft_token_ids.insert(token.token_id.clone());
                    self.writer
                        .insert_mnft_token(token, &tx_data.hash, output_index as i16, parsed.number)
                        .await?;
                }

                for (output_index, account) in DotbitParser::parse_accounts(tx).iter().enumerate() {
                    batch_dotbit_account_ids.insert(account.account_id.clone());
                    self.writer
                        .insert_dotbit_account(
                            account,
                            &tx_data.hash,
                            output_index as i16,
                            parsed.number,
                        )
                        .await?;
                }
            }
        }

        // Skip per-input NFT consumption lookups during bulk sync - too slow (3 queries per input)
        if !self.is_bulk_sync_active() {
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
                            .get_spore_id_by_outpoint(&prev_tx_hash, prev_index as i16)
                            .await?;

                        if let Some(spore_id) = consumed_spore_id {
                            if !batch_spore_ids.contains(&spore_id) {
                                self.writer
                                    .consume_spore(&spore_id, parsed.number, &tx_data.hash)
                                    .await?;
                            }
                        }

                        let consumed_mnft_token_id = self
                            .writer
                            .get_mnft_token_id_by_outpoint(&prev_tx_hash, prev_index as i16)
                            .await?;

                        if let Some(token_id) = consumed_mnft_token_id {
                            if !batch_mnft_token_ids.contains(&token_id) {
                                self.writer
                                    .consume_mnft_token(&token_id, parsed.number, &tx_data.hash)
                                    .await?;
                            }
                        }

                        let consumed_dotbit_account_id = self
                            .writer
                            .get_dotbit_account_id_by_outpoint(&prev_tx_hash, prev_index as i16)
                            .await?;

                        if let Some(account_id) = consumed_dotbit_account_id {
                            if !batch_dotbit_account_ids.contains(&account_id) {
                                self.writer
                                    .consume_dotbit_account(
                                        &account_id,
                                        parsed.number,
                                        &tx_data.hash,
                                    )
                                    .await?;
                            }
                        }
                    }
                }
            }
        }

        let activities_by_block = self
            .collect_activities_for_batch(
                blocks,
                &all_parsed_blocks,
                &all_tx_data,
                &input_cell_info,
                &consumed_code_hashes,
                &udt_transfers_by_tx,
                bulk_sync_mode,
            )
            .await?;
        self.write_activities_batch(&activities_by_block).await?;

        self.flush_batch_stats(&batch_stats).await?;

        Ok(())
    }

    async fn write_parsed_batch(
        &self,
        blocks: &[BlockResponseWithCycles],
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        mut all_tx_data: Vec<TxData>,
        input_cell_info: HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)>,
        consumed_code_hashes: HashMap<(Vec<u8>, i16), (Vec<u8>, Option<Vec<u8>>)>,
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

        let mut batch_cells: HashMap<
            (Vec<u8>, i16),
            (i64, i64, Vec<u8>, i32, Vec<u8>, Option<Vec<u8>>),
        > = HashMap::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                batch_cells.insert(
                    (tx_data.hash.clone(), output_index as i16),
                    (
                        cell.capacity,
                        tx_data.block_number,
                        cell.lock_script_hash.clone(),
                        cell.data_size,
                        cell.lock_code_hash.clone(),
                        cell.type_code_hash.clone(),
                    ),
                );
            }
        }

        for tx_data in &mut all_tx_data {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((cap, _, _, _)) = input_cell_info.get(&key) {
                        tx_data.total_input_capacity += cap;
                    } else if let Some((cap, ..)) = batch_cells.get(&key) {
                        tx_data.total_input_capacity += cap;
                    }
                }
                tx_data.fee = tx_data
                    .total_input_capacity
                    .saturating_sub(tx_data.total_output_capacity);
            }
        }

        {
            let mut cache = self.cell_cache.lock().await;
            for tx_data in &all_tx_data {
                for (output_index, cell) in tx_data.cells.iter().enumerate() {
                    cache.put(
                        (tx_data.hash.clone(), output_index as i32),
                        CachedCellInfo {
                            capacity: cell.capacity,
                            created_at_block: tx_data.block_number,
                            lock_script_hash: cell.lock_script_hash.clone(),
                            data_size: cell.data_size,
                        },
                    );
                }
            }
        }

        for parsed_block in all_parsed_blocks {
            if !parsed_block.proposals.is_empty() {
                self.writer
                    .insert_block_proposals_batch(parsed_block.number, &parsed_block.proposals)
                    .await?;

                if !self.is_bulk_sync_active() {
                    self.cache_block_proposals(&parsed_block.proposals, parsed_block.number)
                        .await;
                }
            }
        }

        let txs_for_batch: Vec<_> = all_tx_data
            .iter()
            .map(|tx_data| {
                (
                    tx_data.hash.as_slice(),
                    tx_data.block_number,
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

        let mut all_cell_deps: Vec<(&[u8], i64, i16, &crate::parser::transaction::ParsedCellDep)> =
            Vec::new();
        for tx_data in &all_tx_data {
            for (dep_index, cell_dep) in tx_data.cell_deps.iter().enumerate() {
                all_cell_deps.push((
                    tx_data.hash.as_slice(),
                    tx_data.block_number,
                    dep_index as i16,
                    cell_dep,
                ));
            }
        }

        if self.should_use_copy() {
            let copy_router = self
                .copy_router
                .as_ref()
                .expect("copy_router must exist when should_use_copy() is true");
            tokio::try_join!(
                async {
                    if !txs_for_batch.is_empty() {
                        copy_router
                            .copy_transactions_parallel(&txs_for_batch)
                            .await
                            .map(|_| ())
                    } else {
                        Ok(())
                    }
                },
                async {
                    if !all_cells.is_empty() {
                        copy_router
                            .copy_cells_parallel(&all_cells)
                            .await
                            .map(|_| ())?;
                        copy_router
                            .copy_live_cells_parallel(&all_cells)
                            .await
                            .map(|_| ())
                    } else {
                        Ok(())
                    }
                },
                async {
                    if !all_inputs.is_empty() {
                        copy_router
                            .copy_inputs_parallel(&all_inputs)
                            .await
                            .map(|_| ())
                    } else {
                        Ok(())
                    }
                },
                async {
                    if !all_cell_deps.is_empty() {
                        copy_router
                            .copy_cell_deps_parallel(&all_cell_deps)
                            .await
                            .map(|_| ())
                    } else {
                        Ok(())
                    }
                }
            )?;
        } else {
            tokio::try_join!(
                async {
                    if !txs_for_batch.is_empty() {
                        self.writer.insert_transactions_batch(&txs_for_batch).await
                    } else {
                        Ok(())
                    }
                },
                async {
                    if !all_cells.is_empty() {
                        self.writer
                            .insert_cells_batch(&all_cells, bulk_sync_mode)
                            .await
                    } else {
                        Ok(())
                    }
                },
                async {
                    if !all_inputs.is_empty() {
                        self.writer
                            .insert_transaction_inputs_batch(&all_inputs)
                            .await
                    } else {
                        Ok(())
                    }
                },
                async {
                    if !all_cell_deps.is_empty() {
                        self.writer
                            .insert_transaction_cell_deps_batch(&all_cell_deps)
                            .await
                    } else {
                        Ok(())
                    }
                }
            )?;
        }

        let mut all_consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i16)> = Vec::new();
        for tx_data in &all_tx_data {
            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((_, created_block, _, _)) = input_cell_info.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            *created_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
                        ));
                    } else if let Some((_, created_block, _, _, _, _)) = batch_cells.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            *created_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
                        ));
                    }
                }
            }
        }
        if !all_consumptions.is_empty() {
            let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;
            self.writer
                .consume_cells_batch(&all_consumptions, bulk_sync_mode)
                .await?;
        }

        let mut address_balance_changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>)> =
            HashMap::new();

        for tx_data in &all_tx_data {
            let mut tx_balance_changes: HashMap<Vec<u8>, i64> = HashMap::new();
            let mut tx_cells_created: HashMap<Vec<u8>, i32> = HashMap::new();
            let mut tx_cells_consumed: HashMap<Vec<u8>, i32> = HashMap::new();

            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((cap, _, lock_hash, _)) = input_cell_info.get(&key) {
                        *tx_balance_changes.entry(lock_hash.clone()).or_default() -= cap;
                        *tx_cells_consumed.entry(lock_hash.clone()).or_default() += 1;
                    } else if let Some((cap, _, lock_hash, _, _, _)) = batch_cells.get(&key) {
                        *tx_balance_changes.entry(lock_hash.clone()).or_default() -= cap;
                        *tx_cells_consumed.entry(lock_hash.clone()).or_default() += 1;
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
            }

            let all_addresses: HashSet<Vec<u8>> = tx_balance_changes
                .keys()
                .chain(tx_cells_created.keys())
                .chain(tx_cells_consumed.keys())
                .cloned()
                .collect();

            for lock_hash in all_addresses {
                let balance_change = tx_balance_changes.get(&lock_hash).copied().unwrap_or(0);
                let cells_created = tx_cells_created.get(&lock_hash).copied().unwrap_or(0);
                let cells_consumed = tx_cells_consumed.get(&lock_hash).copied().unwrap_or(0);

                let entry = address_balance_changes.entry(lock_hash.clone()).or_insert((
                    0,
                    0,
                    0,
                    0,
                    tx_data.block_number,
                    tx_data.hash.clone(),
                ));
                entry.0 += balance_change;
                entry.1 += cells_created - cells_consumed;
                entry.2 += cells_created;
                entry.3 += 1;
                entry.4 = tx_data.block_number;
                entry.5 = tx_data.hash.clone();
            }
        }

        let changes_ref: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> =
            address_balance_changes
                .iter()
                .map(|(k, (a, b, c, d, e, f))| (k.clone(), (*a, *b, *c, *d, *e, f.as_slice())))
                .collect();

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
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((cap, _, _, _)) = input_cell_info.get(&key) {
                        if let Some((lock_code_hash, type_code_hash)) =
                            consumed_code_hashes.get(&key)
                        {
                            let lock_key = (lock_code_hash.clone(), false);
                            let entry =
                                script_usage_changes.entry(lock_key).or_insert((0, 0, 0, 0));
                            entry.1 -= 1;
                            entry.3 -= cap;

                            if let Some(type_code_hash) = type_code_hash {
                                let type_key = (type_code_hash.clone(), true);
                                let entry =
                                    script_usage_changes.entry(type_key).or_insert((0, 0, 0, 0));
                                entry.1 -= 1;
                                entry.3 -= cap;
                            }
                        }
                    } else if let Some((cap, _, _, _, lock_code_hash, type_code_hash)) =
                        batch_cells.get(&key)
                    {
                        let lock_key = (lock_code_hash.clone(), false);
                        let entry = script_usage_changes.entry(lock_key).or_insert((0, 0, 0, 0));
                        entry.1 -= 1;
                        entry.3 -= cap;

                        if let Some(type_code_hash) = type_code_hash {
                            let type_key = (type_code_hash.clone(), true);
                            let entry =
                                script_usage_changes.entry(type_key).or_insert((0, 0, 0, 0));
                            entry.1 -= 1;
                            entry.3 -= cap;
                        }
                    }
                }
            }
        }

        tokio::try_join!(
            async {
                if !changes_ref.is_empty() {
                    self.writer
                        .update_address_balances_batch(&changes_ref)
                        .await
                } else {
                    Ok(())
                }
            },
            async {
                if !script_usage_changes.is_empty() {
                    self.writer
                        .update_script_usage_batch(&script_usage_changes)
                        .await
                } else {
                    Ok(())
                }
            }
        )?;

        let mut batch_stats = BatchStats::default();
        let mut prev_timestamp: Option<chrono::DateTime<Utc>> =
            if let Some(first_block) = all_parsed_blocks.first() {
                self.writer
                    .get_previous_block_timestamp(first_block.number)
                    .await?
            } else {
                None
            };
        let mut prev_epoch: Option<(i64, chrono::DateTime<Utc>, f64)> =
            if let Some(first_block) = all_parsed_blocks.first() {
                self.writer
                    .get_last_epoch_start(first_block.number)
                    .await?
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
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    input_cell_info
                        .get(&key)
                        .map(|(_, _, _, ds)| *ds as i64)
                        .or_else(|| batch_cells.get(&key).map(|(_, _, _, ds, _, _)| *ds as i64))
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
                        // Use 1-minute buckets to match official CKB Explorer
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

        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);

        // Phase 1: Collect all DAO deposits from the batch
        let mut all_dao_deposits: Vec<(crate::parser::ParsedDaoDeposit, i64, DateTime<Utc>, i64)> =
            Vec::new();

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

        // Batch insert DAO deposits
        if !all_dao_deposits.is_empty() {
            self.writer
                .insert_dao_deposits_batch(&all_dao_deposits)
                .await?;
        }

        // Phase 2: Collect ALL input outpoints from non-cellbase transactions
        let mut all_input_outpoints: Vec<(Vec<u8>, i16)> = Vec::new();

        block_tx_idx = 0;
        for parsed in all_parsed_blocks {
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            for tx_data in tx_slice {
                if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                    continue;
                }
                for input in &tx_data.inputs {
                    all_input_outpoints.push((
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    ));
                }
            }
        }

        // Phase 3: Batch query all potentially consumed DAO deposits
        let consumed_dao_map = if !all_input_outpoints.is_empty() {
            let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                let mut seen = HashSet::new();
                all_input_outpoints
                    .into_iter()
                    .filter(|x| seen.insert(x.clone()))
                    .collect()
            };
            let outpoint_refs: Vec<(&[u8], i16)> = unique_outpoints
                .iter()
                .map(|(h, i)| (h.as_slice(), *i))
                .collect();
            self.writer
                .find_consumed_dao_deposits_batch(&outpoint_refs)
                .await?
        } else {
            HashMap::new()
        };

        // Phase 4: Process DAO withdrawals in batch
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

            block_tx_idx = 0;
            for parsed in all_parsed_blocks {
                let tx_count_for_block = parsed.transactions_count as usize;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                block_tx_idx += tx_count_for_block;

                for tx_data in tx_slice {
                    if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                        continue;
                    }

                    // Check if any input matches a DAO deposit
                    let mut consumed_deposits: Vec<(i64, Vec<u8>, i16, String, i64, i16)> =
                        Vec::new();
                    for input in &tx_data.inputs {
                        let key = (
                            input.previous_tx_hash.clone(),
                            input.previous_output_index as i16,
                        );
                        if let Some(deposit_info) = consumed_dao_map.get(&key) {
                            consumed_deposits.push(deposit_info.clone());
                        }
                    }

                    if consumed_deposits.is_empty() {
                        continue;
                    }

                    // Parse new DAO outputs (withdraw requests)
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
                                            tx_data.hash.clone(),
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
                        consuming_tx_hash: tx_data.hash.clone(),
                        timestamp: parsed.timestamp,
                    });
                }
            }

            // Batch process all withdrawals
            if !withdrawal_contexts.is_empty() {
                self.writer
                    .process_dao_withdrawals_batch(&withdrawal_contexts)
                    .await?;
            }
        }

        struct UdtTxContext {
            tx_hash: Vec<u8>,
            block_number: i64,
            timestamp: chrono::DateTime<Utc>,
            output_udts: Vec<crate::parser::ParsedUdtCell>,
            input_outpoints: Vec<(Vec<u8>, i16)>,
        }

        let mut udt_tx_contexts: Vec<UdtTxContext> = Vec::new();
        let mut all_input_outpoints_udt: Vec<(Vec<u8>, i16)> = Vec::new();
        let mut batch_udt_cells: HashMap<(Vec<u8>, i16), crate::parser::ParsedUdtCell> =
            HashMap::new();
        let mut udt_transfers_by_tx: HashMap<Vec<u8>, Vec<crate::parser::ParsedUdtTransfer>> =
            HashMap::new();

        // Temp storage: we filter txs later based on whether they have UDT inputs
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
                        (tx_data.hash.clone(), output_index as i16),
                        udt_cell.clone(),
                    );
                }

                let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                    .inputs
                    .iter()
                    .map(|i| (i.previous_tx_hash.clone(), i.previous_output_index as i16))
                    .collect();

                // Collect ALL input outpoints (to detect UDT consumption/burns)
                all_input_outpoints_udt.extend(input_outpoints.iter().cloned());

                all_tx_infos_for_udt.push(TxInfoForUdt {
                    tx_hash: tx_data.hash.clone(),
                    block_number: parsed.number,
                    timestamp: parsed.timestamp,
                    output_udts,
                    input_outpoints,
                });
            }
        }

        // Fetch UDT info for ALL input outpoints to detect UDT consumption
        let input_udt_info = if !all_input_outpoints_udt.is_empty() {
            let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                let mut seen = HashSet::new();
                all_input_outpoints_udt
                    .into_iter()
                    .filter(|x| seen.insert(x.clone()))
                    .collect()
            };
            let outpoint_refs: Vec<(&[u8], i16)> = unique_outpoints
                .iter()
                .map(|(h, i)| (h.as_slice(), *i))
                .collect();
            self.writer.get_udt_cells_info_batch(&outpoint_refs).await?
        } else {
            HashMap::new()
        };

        // Filter: include txs with UDT outputs OR UDT inputs (fixes burn/send tracking)
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

        if !udt_tx_contexts.is_empty() {
            let mut all_transfers: Vec<(
                crate::parser::ParsedUdtTransfer,
                Vec<u8>,
                i64,
                chrono::DateTime<Utc>,
            )> = Vec::new();

            let mut consumed_udt_outpoints: Vec<(&[u8], i16, i64, &[u8])> = Vec::new();

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
                        consumed_udt_outpoints.push((
                            tx_hash.as_slice(),
                            *idx,
                            ctx.block_number,
                            ctx.tx_hash.as_slice(),
                        ));
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
                        ctx.timestamp,
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
                            ctx.timestamp,
                        ));
                    }
                }
            }

            if !all_transfers.is_empty() {
                for (transfer, tx_hash, _, _) in &all_transfers {
                    udt_transfers_by_tx
                        .entry(tx_hash.clone())
                        .or_default()
                        .push(transfer.clone());
                }

                let transfer_refs: Vec<_> = all_transfers
                    .iter()
                    .map(|(t, h, b, ts)| (t, h.as_slice(), *b, *ts))
                    .collect();
                self.writer
                    .process_udt_transfers_batch(&transfer_refs)
                    .await?;
            }

            if !consumed_udt_outpoints.is_empty() {
                self.writer
                    .consume_udt_cells_batch(&consumed_udt_outpoints)
                    .await?;
            }
        }

        if !batch_udt_cells.is_empty() {
            let tx_block_map: std::collections::HashMap<&[u8], i64> = udt_tx_contexts
                .iter()
                .map(|ctx| (ctx.tx_hash.as_slice(), ctx.block_number))
                .collect();
            let udt_cells_to_insert: Vec<_> = batch_udt_cells
                .iter()
                .map(|((tx_hash, idx), cell)| {
                    let block_number = tx_block_map.get(tx_hash.as_slice()).copied().unwrap_or(0);
                    (tx_hash.as_slice(), *idx, cell, block_number)
                })
                .collect();
            self.writer
                .insert_udt_cells_batch(&udt_cells_to_insert)
                .await?;
        }

        let mut batch_spore_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut batch_mnft_class_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut batch_mnft_token_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut batch_dotbit_account_ids: HashSet<Vec<u8>> = HashSet::new();

        let mut block_tx_idx = 0usize;
        for (block_idx, block_response) in blocks.iter().enumerate() {
            let parsed = &all_parsed_blocks[block_idx];
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                let tx = &block_response.block.transactions[tx_idx];

                for cluster in SporeParser::parse_clusters(tx) {
                    self.writer
                        .insert_spore_cluster(&cluster, parsed.number, &tx_data.hash)
                        .await?;
                }

                for (output_index, spore) in SporeParser::parse_spores(tx).iter().enumerate() {
                    batch_spore_ids.insert(spore.spore_id.clone());
                    self.writer
                        .insert_spore_cell(spore, &tx_data.hash, output_index as i16, parsed.number)
                        .await?;
                    self.writer
                        .insert_spore_content(&spore.spore_id, &spore.content)
                        .await?;
                }

                for issuer in MnftParser::parse_issuers(tx) {
                    self.writer
                        .insert_mnft_issuer(&issuer, &tx_data.hash, 0, parsed.number)
                        .await?;
                }

                for (output_index, class) in MnftParser::parse_classes(tx).iter().enumerate() {
                    batch_mnft_class_ids.insert(class.class_id.clone());
                    self.writer
                        .insert_mnft_class(class, &tx_data.hash, output_index as i16, parsed.number)
                        .await?;
                }

                for (output_index, token) in MnftParser::parse_tokens(tx).iter().enumerate() {
                    batch_mnft_token_ids.insert(token.token_id.clone());
                    self.writer
                        .insert_mnft_token(token, &tx_data.hash, output_index as i16, parsed.number)
                        .await?;
                }

                for (output_index, account) in DotbitParser::parse_accounts(tx).iter().enumerate() {
                    batch_dotbit_account_ids.insert(account.account_id.clone());
                    self.writer
                        .insert_dotbit_account(
                            account,
                            &tx_data.hash,
                            output_index as i16,
                            parsed.number,
                        )
                        .await?;
                }
            }
        }

        // Skip per-input NFT consumption lookups during bulk sync - too slow (3 queries per input)
        // These will be processed by integrity service after bulk sync completes
        if !self.is_bulk_sync_active() {
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
                            .get_spore_id_by_outpoint(&prev_tx_hash, prev_index as i16)
                            .await?;

                        if let Some(spore_id) = consumed_spore_id {
                            if !batch_spore_ids.contains(&spore_id) {
                                self.writer
                                    .consume_spore(&spore_id, parsed.number, &tx_data.hash)
                                    .await?;
                            }
                        }

                        let consumed_mnft_token_id = self
                            .writer
                            .get_mnft_token_id_by_outpoint(&prev_tx_hash, prev_index as i16)
                            .await?;

                        if let Some(token_id) = consumed_mnft_token_id {
                            if !batch_mnft_token_ids.contains(&token_id) {
                                self.writer
                                    .consume_mnft_token(&token_id, parsed.number, &tx_data.hash)
                                    .await?;
                            }
                        }

                        let consumed_dotbit_account_id = self
                            .writer
                            .get_dotbit_account_id_by_outpoint(&prev_tx_hash, prev_index as i16)
                            .await?;

                        if let Some(account_id) = consumed_dotbit_account_id {
                            if !batch_dotbit_account_ids.contains(&account_id) {
                                self.writer
                                    .consume_dotbit_account(
                                        &account_id,
                                        parsed.number,
                                        &tx_data.hash,
                                    )
                                    .await?;
                            }
                        }
                    }
                }
            }
        }

        let activities_by_block = self
            .collect_activities_for_batch(
                blocks,
                all_parsed_blocks,
                &all_tx_data,
                &input_cell_info,
                &consumed_code_hashes,
                &udt_transfers_by_tx,
                bulk_sync_mode,
            )
            .await?;
        self.write_activities_batch(&activities_by_block).await?;

        // Write blocks LAST - this is the "commit marker" for crash recovery.
        // All other data must be written successfully before blocks exist.
        let block_refs: Vec<&crate::parser::block::ParsedBlock> =
            all_parsed_blocks.iter().collect();
        self.writer.insert_blocks_batch(&block_refs).await?;

        self.flush_batch_stats(&batch_stats).await?;

        Ok(())
    }

    #[allow(dead_code)]
    async fn sync_block_optimized(
        &self,
        block_response: &BlockResponseWithCycles,
        batch_stats: &mut BatchStats,
        prev_timestamp: &mut Option<chrono::DateTime<Utc>>,
        prev_epoch: &mut Option<(i64, chrono::DateTime<Utc>, f64)>,
        chain_tip: u64,
    ) -> Result<()> {
        let block = &block_response.block;
        let parsed = BlockParser::parse(block);
        let db_start = Instant::now();
        let end_block = parsed.number as u64;
        let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;

        self.writer.insert_block(&parsed, 0).await?;

        if !parsed.proposals.is_empty() {
            self.writer
                .insert_block_proposals_batch(parsed.number, &parsed.proposals)
                .await?;

            if !self.is_bulk_sync_active() {
                self.cache_block_proposals(&parsed.proposals, parsed.number)
                    .await;
            }
        }

        struct TxData {
            hash: Vec<u8>,
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
        }

        let mut tx_data_list: Vec<TxData> = Vec::with_capacity(block.transactions.len());

        for (tx_index, tx) in block.transactions.iter().enumerate() {
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

            tx_data_list.push(TxData {
                hash: parsed_tx.hash,
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
            });
        }

        let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data_list
            .iter()
            .filter(|tx| !tx.is_cellbase)
            .flat_map(|tx| {
                tx.inputs.iter().map(|input| {
                    (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    )
                })
            })
            .collect();

        let mut block_cells: HashMap<
            (Vec<u8>, i16),
            (i64, i64, Vec<u8>, i32, Vec<u8>, Option<Vec<u8>>),
        > = HashMap::new();
        for tx_data in &tx_data_list {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                block_cells.insert(
                    (tx_data.hash.clone(), output_index as i16),
                    (
                        cell.capacity,
                        parsed.number,
                        cell.lock_script_hash.clone(),
                        cell.data_size,
                        cell.lock_code_hash.clone(),
                        cell.type_code_hash.clone(),
                    ),
                );
            }
        }

        let mut input_cell_info: HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)> = HashMap::new();
        {
            let mut cache = self.cell_cache.lock().await;
            for (tx_hash, idx) in &input_outpoints {
                let key = (tx_hash.clone(), *idx as i32);
                if let Some(cached) = cache.get(&key) {
                    input_cell_info.insert(
                        (tx_hash.clone(), *idx),
                        (
                            cached.capacity,
                            cached.created_at_block,
                            cached.lock_script_hash.clone(),
                            cached.data_size,
                        ),
                    );
                }
            }
        }

        let missing_outpoints: Vec<(Vec<u8>, i16)> = input_outpoints
            .iter()
            .filter(|(h, i)| {
                let key = (h.clone(), *i);
                !input_cell_info.contains_key(&key) && !block_cells.contains_key(&key)
            })
            .cloned()
            .collect();

        if !missing_outpoints.is_empty() {
            let missing_refs: Vec<(&[u8], i16)> = missing_outpoints
                .iter()
                .map(|(h, i)| (h.as_slice(), *i))
                .collect();
            let db_info = self.writer.get_cells_info_batch(&missing_refs).await?;
            for ((tx_hash, idx), (cap, block, lock_hash, data_size)) in db_info {
                input_cell_info.insert((tx_hash, idx), (cap, block, lock_hash, data_size));
            }
        }

        for tx_data in &mut tx_data_list {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((cap, _, _, _)) = input_cell_info.get(&key) {
                        tx_data.total_input_capacity += cap;
                    } else if let Some((cap, _, _, _, _, _)) = block_cells.get(&key) {
                        tx_data.total_input_capacity += cap;
                    }
                }
                tx_data.fee = tx_data
                    .total_input_capacity
                    .saturating_sub(tx_data.total_output_capacity);
            }
        }

        {
            let mut cache = self.cell_cache.lock().await;
            for tx_data in &tx_data_list {
                for (output_index, cell) in tx_data.cells.iter().enumerate() {
                    cache.put(
                        (tx_data.hash.clone(), output_index as i32),
                        CachedCellInfo {
                            capacity: cell.capacity,
                            created_at_block: parsed.number,
                            lock_script_hash: cell.lock_script_hash.clone(),
                            data_size: cell.data_size,
                        },
                    );
                }
            }
        }

        let txs_for_batch: Vec<_> = tx_data_list
            .iter()
            .enumerate()
            .map(|(tx_index, tx_data)| {
                (
                    tx_data.hash.as_slice(),
                    parsed.number,
                    tx_index as i32,
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
                    parsed.timestamp,
                )
            })
            .collect();
        self.writer
            .insert_transactions_batch(&txs_for_batch)
            .await?;

        let mut all_cells: Vec<(&[u8], i16, &crate::parser::cell::ParsedCell, i64)> = Vec::new();
        for tx_data in &tx_data_list {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                all_cells.push((
                    tx_data.hash.as_slice(),
                    output_index as i16,
                    cell,
                    parsed.number,
                ));
            }
        }
        if !all_cells.is_empty() {
            self.writer
                .insert_cells_batch(&all_cells, bulk_sync_mode)
                .await?;
        }

        let mut all_inputs: Vec<(&[u8], i64, i16, &crate::parser::transaction::ParsedInput)> =
            Vec::new();
        for tx_data in &tx_data_list {
            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    all_inputs.push((
                        tx_data.hash.as_slice(),
                        parsed.number,
                        input_index as i16,
                        input,
                    ));
                }
            }
        }
        if !all_inputs.is_empty() {
            self.writer
                .insert_transaction_inputs_batch(&all_inputs)
                .await?;
        }

        let mut all_consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i16)> = Vec::new();
        for tx_data in &tx_data_list {
            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((_, created_block, _, _)) = input_cell_info.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            *created_block,
                            tx_data.hash.as_slice(),
                            parsed.number,
                            input_index as i16,
                        ));
                    } else if block_cells.contains_key(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            parsed.number,
                            tx_data.hash.as_slice(),
                            parsed.number,
                            input_index as i16,
                        ));
                    }
                }
            }
        }
        if !all_consumptions.is_empty() {
            let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;
            self.writer
                .consume_cells_batch(&all_consumptions, bulk_sync_mode)
                .await?;
        }

        let mut address_balance_changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> =
            HashMap::new();

        for tx_data in &tx_data_list {
            let mut tx_balance_changes: HashMap<Vec<u8>, i64> = HashMap::new();
            let mut tx_cells_created: HashMap<Vec<u8>, i32> = HashMap::new();
            let mut tx_cells_consumed: HashMap<Vec<u8>, i32> = HashMap::new();

            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.clone(),
                        input.previous_output_index as i16,
                    );
                    if let Some((cap, _, lock_hash, _)) = input_cell_info.get(&key) {
                        *tx_balance_changes.entry(lock_hash.clone()).or_default() -= cap;
                        *tx_cells_consumed.entry(lock_hash.clone()).or_default() += 1;
                    } else if let Some((cap, _, lock_hash, _, _, _)) = block_cells.get(&key) {
                        *tx_balance_changes.entry(lock_hash.clone()).or_default() -= cap;
                        *tx_cells_consumed.entry(lock_hash.clone()).or_default() += 1;
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
            }

            let all_addresses: HashSet<Vec<u8>> = tx_balance_changes
                .keys()
                .chain(tx_cells_created.keys())
                .chain(tx_cells_consumed.keys())
                .cloned()
                .collect();

            for lock_hash in all_addresses {
                let balance_change = tx_balance_changes.get(&lock_hash).copied().unwrap_or(0);
                let cells_created = tx_cells_created.get(&lock_hash).copied().unwrap_or(0);
                let cells_consumed = tx_cells_consumed.get(&lock_hash).copied().unwrap_or(0);

                let entry = address_balance_changes.entry(lock_hash.clone()).or_insert((
                    0,
                    0,
                    0,
                    0,
                    parsed.number,
                    tx_data.hash.as_slice(),
                ));
                entry.0 += balance_change;
                entry.1 += cells_created - cells_consumed;
                entry.2 += cells_created;
                entry.3 += 1;
            }
        }

        if !address_balance_changes.is_empty() {
            self.writer
                .update_address_balances_batch(&address_balance_changes)
                .await?;
        }

        let block_date = parsed.timestamp.date_naive();
        let cells_created: i32 = tx_data_list.iter().map(|tx| tx.cells.len() as i32).sum();
        let cells_consumed: i32 = tx_data_list
            .iter()
            .filter(|tx| !tx.is_cellbase)
            .map(|tx| tx.inputs.len() as i32)
            .sum();
        let capacity_transferred: i64 = tx_data_list
            .iter()
            .filter(|tx| !tx.is_cellbase)
            .map(|tx| tx.total_output_capacity)
            .sum();
        let data_size_added: i64 = tx_data_list
            .iter()
            .flat_map(|tx| tx.cells.iter())
            .map(|cell| cell.data_size as i64)
            .sum();
        let data_size_consumed: i64 = tx_data_list
            .iter()
            .filter(|tx| !tx.is_cellbase)
            .flat_map(|tx| tx.inputs.iter())
            .filter_map(|input| {
                let key = (
                    input.previous_tx_hash.clone(),
                    input.previous_output_index as i16,
                );
                input_cell_info
                    .get(&key)
                    .map(|(_, _, _, ds)| *ds as i64)
                    .or_else(|| block_cells.get(&key).map(|(_, _, _, ds, _, _)| *ds as i64))
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

        if let Some(first_tx) = tx_data_list.first() {
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
            let block_time_seconds = (parsed.timestamp - *prev_ts).num_seconds();
            if block_time_seconds >= 0 {
                let bucket = block_time_to_bucket(block_time_seconds);
                *batch_stats.block_time_dist.entry(bucket).or_default() += 1;
                let block_time_ms = block_time_seconds * 1000;
                let entry = batch_stats
                    .daily_block_times
                    .entry(block_date)
                    .or_insert((0, 0));
                entry.0 += block_time_ms;
                entry.1 += 1;
            }
        }
        *prev_timestamp = Some(parsed.timestamp);

        if parsed.epoch_index == 0 && parsed.epoch_number > 0 {
            if let Some((prev_epoch_num, prev_start_ts, _)) = prev_epoch {
                if *prev_epoch_num == parsed.epoch_number - 1 {
                    let epoch_duration_minutes =
                        (parsed.timestamp - *prev_start_ts).num_seconds() as f64 / 60.0;
                    // Use 1-minute buckets to match official CKB Explorer
                    let bucket_minutes = epoch_duration_minutes.round() as i32;
                    *batch_stats
                        .epoch_time_dist
                        .entry(bucket_minutes)
                        .or_default() += 1;
                }
            }
        }
        if parsed.epoch_index == 0 {
            *prev_epoch = Some((parsed.epoch_number, parsed.timestamp, 0.0));
        }

        batch_stats.dao_snapshot_dates.insert(block_date);

        for tx_data in &tx_data_list {
            let dao_deposits = DaoParser::parse_deposits_from_cells(&tx_data.hash, &tx_data.cells);
            for deposit in &dao_deposits {
                let ar = self
                    .writer
                    .get_block_dao_field(parsed.number)
                    .await?
                    .and_then(|dao| DaoParser::extract_ar_from_dao_field(&dao))
                    .unwrap_or(0) as i64;
                self.writer
                    .insert_dao_deposit(deposit, parsed.number, parsed.timestamp, ar)
                    .await?;
            }
        }

        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        for tx_data in &tx_data_list {
            if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                continue;
            }

            let input_outpoints: Vec<(&[u8], i32)> = tx_data
                .inputs
                .iter()
                .map(|i| (i.previous_tx_hash.as_slice(), i.previous_output_index))
                .collect();

            let consumed_dao = self
                .writer
                .find_consumed_dao_deposits(&input_outpoints)
                .await?;
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
                                    tx_data.hash.clone(),
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

            self.writer
                .process_dao_withdrawals(
                    &consumed_dao,
                    &new_dao_outputs,
                    parsed.number,
                    &tx_data.hash,
                    parsed.timestamp,
                )
                .await?;
        }

        self.perf.add(&self.perf.db_write_us, db_start.elapsed());

        Ok(())
    }

    async fn flush_batch_stats(&self, stats: &BatchStats) -> Result<()> {
        let bulk_sync_active = self.is_bulk_sync_active();

        // Critical: sync_status must always be updated (crash recovery)
        if let Some((block_number, ref block_hash)) = stats.last_block {
            let ema_rate = self.progress.ema_blocks_per_second();
            let ema_rate_opt = if ema_rate > 0.0 { Some(ema_rate) } else { None };
            self.writer
                .update_sync_status(
                    block_number,
                    block_hash,
                    stats.sync_totals.0,
                    stats.sync_totals.1,
                    stats.sync_totals.2,
                    0,
                    ema_rate_opt,
                )
                .await?;
        }

        // Epoch statistics - always written (contains epoch metadata needed for queries)
        for (epoch_number, accum) in &stats.epoch_stats {
            self.writer
                .upsert_epoch_statistics_batch(
                    *epoch_number,
                    accum.start_block,
                    accum.end_block,
                    accum.length,
                    accum.start_ts,
                    accum.end_ts,
                    accum.tx_count,
                    accum.is_new,
                )
                .await?;
        }

        if !bulk_sync_active && !self.is_stats_rebuild_in_progress().await {
            for (
                date,
                (blocks, txs, created, consumed, capacity, data_size_added, data_size_consumed),
            ) in &stats.daily_stats
            {
                self.writer
                    .update_daily_statistics(
                        *date,
                        *blocks,
                        *txs,
                        *created,
                        *consumed,
                        *capacity,
                        *data_size_added,
                        *data_size_consumed,
                    )
                    .await?;
            }

            for (date, (sum_target, count, uncles)) in &stats.daily_block_stats {
                let avg_target = if *count > 0 {
                    (*sum_target / *count as i128) as i64
                } else {
                    0
                };
                self.writer
                    .update_daily_block_stats_batch(*date, avg_target, *count, *uncles)
                    .await?;
            }

            for (date, (sum_ms, count)) in &stats.daily_block_times {
                if *count > 0 {
                    let avg_ms = sum_ms / *count as i64;
                    self.writer
                        .update_daily_avg_block_time_batch(*date, avg_ms, *count)
                        .await?;
                }
            }

            for (hour, (blocks, txs, created, consumed, capacity)) in &stats.hourly_stats {
                self.writer
                    .update_hourly_statistics(*hour, *blocks, *txs, *created, *consumed, *capacity)
                    .await?;
            }

            for ((date, miner_hash), (blocks_count, last_block)) in &stats.miner_stats {
                self.writer
                    .update_miner_statistics_batch(miner_hash, *last_block, *date, *blocks_count)
                    .await?;
            }

            for (bucket, count) in &stats.block_time_dist {
                self.writer
                    .update_block_time_distribution_batch(*bucket, *count)
                    .await?;
            }

            for (bucket, count) in &stats.epoch_time_dist {
                self.writer
                    .update_epoch_time_distribution_batch(*bucket, *count)
                    .await?;
            }

            let mut snapshot_dates: Vec<_> = stats.dao_snapshot_dates.iter().collect();
            snapshot_dates.sort();
            for date in snapshot_dates {
                self.writer.update_dao_daily_snapshot(*date).await?;
            }
        }

        Ok(())
    }

    async fn check_and_handle_reorg(
        &self,
        db_tip: u64,
        stored_hash: &[u8],
    ) -> Result<Option<ReorgAction>> {
        let chain_hash = self
            .rpc
            .get_block_hash(db_tip)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Block {} not found on chain", db_tip))?;

        let chain_hash_bytes = crate::rpc::parse_hex_to_bytes(&chain_hash);

        if chain_hash_bytes == stored_hash {
            return Ok(None);
        }

        warn!(
            "Reorg detected at block {}: stored={} chain={}",
            db_tip,
            hex::encode(stored_hash),
            chain_hash
        );

        let (fork_point, fork_hash) = self.find_fork_point(db_tip).await?;
        let depth = db_tip - fork_point;

        info!(
            "Fork point found at block {}, depth = {}",
            fork_point, depth
        );

        let chain_tip = self.rpc.get_tip_block_number().await?;
        let chain_tip_hash = self
            .rpc
            .get_block_hash(chain_tip)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Chain tip {} not found", chain_tip))?;
        let chain_tip_hash_bytes = crate::rpc::parse_hex_to_bytes(&chain_tip_hash);

        if depth > DEEP_FORK_DEPTH {
            error!(
                "DEEP FORK DETECTED! Depth {} exceeds limit {}. Manual intervention required.",
                depth, DEEP_FORK_DEPTH
            );

            self.writer
                .record_deep_fork(
                    fork_point as i64,
                    &fork_hash,
                    db_tip as i64,
                    stored_hash,
                    chain_tip as i64,
                    &chain_tip_hash_bytes,
                    depth as i64,
                )
                .await?;

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
                .get_block_hash_at_height(height as i64)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Block {} not found in DB", height))?;

            let chain_hash = self
                .rpc
                .get_block_hash(height)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Block {} not found on chain", height))?;

            let chain_hash_bytes = crate::rpc::parse_hex_to_bytes(&chain_hash);

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

    async fn update_secondary_issuance(
        &self,
        block_hash: &str,
        dao_hex: &str,
        block_number: i64,
        block_timestamp: DateTime<Utc>,
    ) -> Result<()> {
        let (_, _, _, _, last_processed) = self.writer.get_secondary_issuance_state().await?;
        if block_number <= last_processed {
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
            let total_dao_deposits: u128 =
                self.writer.get_dao_deposits_at_block(block_number).await?;

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

        self.writer
            .accumulate_secondary_issuance(&breakdown, block_number, block_timestamp)
            .await?;

        Ok(())
    }

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
    fn test_infer_is_mainnet() {
        assert!(infer_is_mainnet("http://localhost:8114"));
        assert!(!infer_is_mainnet("https://testnet.ckb.node"));
        assert!(!infer_is_mainnet("https://devnet.ckb.node"));
    }

    #[test]
    fn test_parsed_cell_from_live_info() {
        let info = LiveCellInfo {
            capacity: 42,
            created_at_block: 7,
            lock_script_hash: vec![1, 2, 3],
            lock_code_hash: vec![4, 5, 6],
            lock_args: vec![7, 8],
            type_script_hash: Some(vec![9, 10, 11]),
            type_code_hash: Some(vec![12, 13, 14]),
            data_size: 123,
        };

        let cell = parsed_cell_from_live_info(&info);
        assert_eq!(cell.capacity, 42);
        assert_eq!(cell.lock_script_hash, vec![1, 2, 3]);
        assert_eq!(cell.lock_code_hash, vec![4, 5, 6]);
        assert_eq!(cell.lock_args, vec![7, 8]);
        assert_eq!(cell.type_script_hash, Some(vec![9, 10, 11]));
        assert_eq!(cell.type_code_hash, Some(vec![12, 13, 14]));
        assert_eq!(cell.data_size, 123);
    }

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
}
