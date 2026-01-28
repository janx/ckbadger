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
use tracing::{debug, error, info, warn};

use crate::cache::CacheInvalidator;
use crate::config::Config;
use crate::db::{
    BatchWriter, CopyConfig, CopyPoolManager, ParallelCopyRouter, ReorgResult, Repository,
    SecondaryIssuanceBreakdown,
};
use crate::integrity::IntegrityServiceHandle;
use crate::parser::{
    BlockParser, CellParser, DaoParser, DotbitParser, MnftParser, SporeParser, TransactionParser,
    UdtParser,
};
use crate::rpc::{BlockResponseWithCycles, CkbRpcClient, DaoField};

use super::SyncProgress;

const REORG_LIMIT: u64 = 36;

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

pub struct Indexer {
    config: Config,
    rpc: CkbRpcClient,
    repo: Repository,
    writer: BatchWriter,
    copy_router: Option<ParallelCopyRouter>,
    progress: Arc<SyncProgress>,
    cell_cache: Arc<tokio::sync::Mutex<LruCache<(Vec<u8>, i32), CachedCellInfo>>>,
    perf: PerfStats,
    integrity_handle: Option<IntegrityServiceHandle>,
    cache_invalidator: CacheInvalidator,
    last_cache_invalidation: tokio::sync::Mutex<u64>,
}

impl Indexer {
    pub async fn new(
        config: Config,
        pool: PgPool,
        integrity_handle: Option<IntegrityServiceHandle>,
    ) -> Result<Self> {
        let rpc = CkbRpcClient::new(&config.ckb_rpc_url);
        let repo = Repository::new(pool.clone());
        let writer = BatchWriter::with_fast_sync_mode(pool, config.fast_sync_mode);

        let (tip_number, _) = repo.get_sync_tip().await?;
        let chain_tip = rpc.get_tip_block_number().await?;

        let progress = Arc::new(SyncProgress::new(tip_number as u64, chain_tip));
        let cache_invalidator = CacheInvalidator::new(config.redis_url.as_deref()).await;

        let cell_cache = Arc::new(tokio::sync::Mutex::new(LruCache::new(
            NonZeroUsize::new(CELL_CACHE_CAPACITY).unwrap(),
        )));

        let copy_router = if config.use_copy_bulk_sync {
            match CopyPoolManager::new(
                &config.database_url,
                CopyConfig {
                    max_copy_connections: config.copy_pool_size,
                    copy_batch_size: 50_000,
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

        Ok(Self {
            config,
            rpc,
            repo,
            writer,
            copy_router,
            progress,
            cell_cache,
            perf: PerfStats::default(),
            integrity_handle,
            cache_invalidator,
            last_cache_invalidation: tokio::sync::Mutex::new(0),
        })
    }

    pub fn progress(&self) -> Arc<SyncProgress> {
        Arc::clone(&self.progress)
    }

    /// Check if bulk sync mode is active (for skipping non-critical statistics).
    /// Auto-enabled when blocks_remaining > bulk_sync_threshold (no manual config needed)
    fn is_bulk_sync_active(&self) -> bool {
        self.progress.blocks_remaining() > self.config.bulk_sync_threshold
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
        }

        let (start_block, _) = self.repo.get_sync_tip().await?;
        self.writer.init_sync_start(start_block).await?;

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
            if self.repo.has_unresolved_deep_fork().await.unwrap_or(false) {
                warn!("Deep fork unresolved, sync paused. Waiting for manual intervention...");
                sleep(Duration::from_secs(30)).await;
                continue;
            }

            match self.sync_batch().await {
                Ok(SyncAction::CaughtUp) => {
                    self.trigger_missing_cycles_fix_when_idle().await;
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

        type FetchedBatch = (u64, u64, Vec<BlockResponseWithCycles>);
        type ParsedBatch = (
            u64,                                                 // start_block
            u64,                                                 // end_block
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

        let fetcher = tokio::spawn(async move {
            let mut next_block: Option<u64> = None;

            loop {
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
                    sleep(Duration::from_millis(config.poll_interval_ms)).await;
                    continue;
                }

                let end_block =
                    std::cmp::min(start_block + config.batch_size as u64 - 1, chain_tip);

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
                    .send((start_block, end_block, blocks))
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
            while let Some((start_block, end_block, blocks)) = fetch_rx.recv().await {
                let blocks_clone = blocks.clone();
                let (all_parsed_blocks, all_tx_data, all_input_outpoints) =
                    tokio::task::spawn_blocking(move || parse_blocks_parallel(&blocks_clone))
                        .await
                        .unwrap_or_else(|_| (vec![], vec![], vec![]));

                if all_parsed_blocks.is_empty() {
                    continue;
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
                    .filter(|(h, i)| !input_cell_info.contains_key(&(h.clone(), *i)))
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
                    match writer_for_parser.get_cells_info_batch(&missing_refs).await {
                        Ok(db_info) => {
                            for ((tx_hash, idx), (cap, block, lock_hash, data_size)) in db_info {
                                input_cell_info
                                    .insert((tx_hash, idx), (cap, block, lock_hash, data_size));
                            }
                        }
                        Err(e) => {
                            error!("Parser: Failed to fetch cell info from DB: {}", e);
                        }
                    }
                }

                let mut consumed_from_db: Vec<(Vec<u8>, i16)> = Vec::new();
                let mut batch_cells: HashMap<(Vec<u8>, i16), ()> = HashMap::new();
                for td in &all_tx_data {
                    for (idx, _) in td.cells.iter().enumerate() {
                        batch_cells.insert((td.hash.clone(), idx as i16), ());
                    }
                }
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
                    match writer_for_parser.get_cells_code_hashes_batch(&refs).await {
                        Ok(hashes) => hashes,
                        Err(e) => {
                            error!("Parser: Failed to fetch code hashes from DB: {}", e);
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
                        Self::drain_channel(&mut parse_rx).await;
                        continue;
                    }

                    // Check for reorg before processing
                    if let Some(ref stored_hash) = db_tip_hash {
                        if db_tip > 0 {
                            match self
                                .check_and_handle_reorg(db_tip as u64, stored_hash)
                                .await?
                            {
                                Some(ReorgAction::Handled(_)) => {
                                    info!("Reorg handled, draining stale batches");
                                    Self::drain_channel(&mut parse_rx).await;
                                    continue;
                                }
                                Some(ReorgAction::DeepForkPaused) => {
                                    warn!("Deep fork detected, sync paused");
                                    Self::drain_channel(&mut parse_rx).await;
                                    sleep(Duration::from_secs(30)).await;
                                    continue;
                                }
                                None => {}
                            }
                        }
                    }

                    let mode = if self.should_use_copy() {
                        "[COPY]"
                    } else if self.is_bulk_sync_active() {
                        "[BULK]"
                    } else {
                        ""
                    };
                    info!(
                        "Syncing blocks {} to {} ({} remaining, {:.2} blocks/sec) {}",
                        start_block,
                        end_block,
                        self.progress.blocks_remaining(),
                        self.progress.blocks_per_second(),
                        mode
                    );

                    let db_start = Instant::now();
                    if let Err(e) = self
                        .write_parsed_batch(
                            &blocks,
                            &all_parsed_blocks,
                            all_tx_data,
                            input_cell_info,
                            consumed_code_hashes,
                        )
                        .await
                    {
                        error!("Sync error: {}", e);
                        Self::drain_channel(&mut parse_rx).await;
                        sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                    self.perf.add(&self.perf.db_write_us, db_start.elapsed());

                    if let Some(last_block) = all_parsed_blocks.last() {
                        self.progress.update_current_batch(
                            last_block.number as u64,
                            all_parsed_blocks.len() as u64,
                        );

                        // Handle periodic updates (secondary issuance, DAO stats)
                        let crossed_50 = (start_block / 50) != (end_block / 50);
                        if crossed_50 {
                            if let Err(e) = self
                                .update_secondary_issuance(
                                    &hex::encode(&last_block.hash),
                                    &hex::encode(&last_block.dao),
                                    last_block.number,
                                    last_block.timestamp,
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
                    // No batch received within timeout - we're likely caught up
                    self.trigger_missing_cycles_fix_when_idle().await;
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

        let crossed_50 = (start_block / 50) != (end_block / 50);
        if crossed_50 {
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

        let end_block = std::cmp::min(start_block + self.config.batch_size as u64 - 1, chain_tip);

        if start_block > end_block {
            return Ok(SyncAction::CaughtUp);
        }

        info!(
            "Syncing blocks {} to {} ({} remaining, {:.2} blocks/sec)",
            start_block,
            end_block,
            self.progress.blocks_remaining(),
            self.progress.blocks_per_second()
        );

        let fetch_start = Instant::now();
        let blocks = self.fetch_blocks_parallel(start_block, end_block).await?;
        self.perf
            .add(&self.perf.rpc_fetch_us, fetch_start.elapsed());

        let db_start = Instant::now();
        self.sync_blocks_batch(&blocks).await?;
        self.perf.add(&self.perf.db_write_us, db_start.elapsed());

        if let Some(last_block_response) = blocks.last() {
            let last_block_number = BlockParser::parse_block_number(&last_block_response.block);
            self.progress
                .update_current_batch(last_block_number, blocks.len() as u64);
        }
        self.perf
            .blocks_count
            .fetch_add(blocks.len() as u64, Ordering::Relaxed);

        self.perf.report_and_reset();

        if let Some(last_block_response) = blocks.last() {
            let last_block_number = BlockParser::parse_block_number(&last_block_response.block);

            let crossed_50 = (start_block / 50) != (end_block / 50);
            if crossed_50 {
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

        Ok(SyncAction::Continue)
    }

    async fn trigger_missing_cycles_fix_when_idle(&self) {
        let blocks_remaining = self.progress.blocks_remaining();
        if blocks_remaining > self.config.bulk_sync_threshold {
            return;
        }
        if let Some(ref handle) = self.integrity_handle {
            if !handle.is_running().await {
                handle
                    .trigger(crate::integrity::IntegrityCheck::AllMissingCycles)
                    .await;
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

    async fn sync_blocks_batch(&self, blocks: &[BlockResponseWithCycles]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        // Parse blocks and transactions in parallel using rayon
        let blocks_clone: Vec<BlockResponseWithCycles> = blocks.to_vec();
        let (all_parsed_blocks, mut all_tx_data, all_input_outpoints) =
            tokio::task::spawn_blocking(move || parse_blocks_parallel(&blocks_clone)).await?;

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
            .filter(|(h, i)| !input_cell_info.contains_key(&(h.clone(), *i)))
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

        // (capacity, created_at_block, lock_script_hash, data_size)
        let mut batch_cells: HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)> = HashMap::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                batch_cells.insert(
                    (tx_data.hash.clone(), output_index as i16),
                    (
                        cell.capacity,
                        tx_data.block_number,
                        cell.lock_script_hash.clone(),
                        cell.data_size,
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
                    } else if let Some((cap, _, _, _)) = batch_cells.get(&key) {
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

        let block_refs: Vec<&crate::parser::block::ParsedBlock> =
            all_parsed_blocks.iter().collect();
        self.writer.insert_blocks_batch(&block_refs).await?;

        for parsed_block in &all_parsed_blocks {
            if !parsed_block.proposals.is_empty() {
                self.writer
                    .insert_block_proposals_batch(parsed_block.number, &parsed_block.proposals)
                    .await?;
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
        if !txs_for_batch.is_empty() {
            self.writer
                .insert_transactions_batch(&txs_for_batch)
                .await?;
        }

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
        if !all_cells.is_empty() {
            self.writer.insert_cells_batch(&all_cells).await?;
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
                    } else if let Some((_, _, _, _)) = batch_cells.get(&key) {
                        let created_block = all_tx_data
                            .iter()
                            .find(|td| td.hash == input.previous_tx_hash)
                            .map(|td| td.block_number)
                            .unwrap_or(tx_data.block_number);
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            created_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
                        ));
                    }
                }
            }
        }
        if !all_consumptions.is_empty() {
            self.writer.consume_cells_batch(&all_consumptions).await?;
        }

        let mut address_balance_changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>)> =
            HashMap::new();
        let mut address_tx_records: Vec<(Vec<u8>, Vec<u8>, i64, i16, i64, chrono::DateTime<Utc>)> =
            Vec::new();

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
                    } else if let Some((cap, _, lock_hash, _)) = batch_cells.get(&key) {
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

                let tx_type: i16 = match balance_change.cmp(&0) {
                    std::cmp::Ordering::Greater => 1,
                    std::cmp::Ordering::Less => 2,
                    std::cmp::Ordering::Equal => 3,
                };

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

                address_tx_records.push((
                    lock_hash.clone(),
                    tx_data.hash.clone(),
                    tx_data.block_number,
                    tx_type,
                    balance_change,
                    tx_data.timestamp,
                ));
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
                    } else if let Some((cap, _, _, _)) = batch_cells.get(&key) {
                        if let Some(src_tx) = all_tx_data
                            .iter()
                            .find(|td| td.hash == input.previous_tx_hash)
                        {
                            if let Some(cell) =
                                src_tx.cells.get(input.previous_output_index as usize)
                            {
                                let lock_key = (cell.lock_code_hash.clone(), false);
                                let entry =
                                    script_usage_changes.entry(lock_key).or_insert((0, 0, 0, 0));
                                entry.1 -= 1;
                                entry.3 -= cap;

                                if let Some(ref type_code_hash) = cell.type_code_hash {
                                    let type_key = (type_code_hash.clone(), true);
                                    let entry = script_usage_changes
                                        .entry(type_key)
                                        .or_insert((0, 0, 0, 0));
                                    entry.1 -= 1;
                                    entry.3 -= cap;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Parallel writes: address balances, address txs, script usage are independent
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
                if !address_tx_records.is_empty() {
                    self.writer
                        .insert_address_transactions_batch(&address_tx_records)
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
                        .or_else(|| batch_cells.get(&key).map(|(_, _, _, ds)| *ds as i64))
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
                        .entry(block_time_seconds as i32)
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
                        let bucket_minutes = ((epoch_duration_minutes / 2.0).floor() as i32) * 2;
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
            tx_index: i32,
            timestamp: chrono::DateTime<Utc>,
            output_udts: Vec<crate::parser::ParsedUdtCell>,
            input_outpoints: Vec<(Vec<u8>, i16)>,
        }

        let mut udt_tx_contexts: Vec<UdtTxContext> = Vec::new();
        let mut all_input_outpoints_udt: Vec<(Vec<u8>, i16)> = Vec::new();
        let mut batch_udt_cells: HashMap<(Vec<u8>, i16), crate::parser::ParsedUdtCell> =
            HashMap::new();

        // Temp storage: we filter txs later based on whether they have UDT inputs
        struct TxInfoForUdt {
            tx_hash: Vec<u8>,
            block_number: i64,
            tx_index: i32,
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
                    tx_index: tx_idx as i32,
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
                    tx_index: tx_info.tx_index,
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
                let transfer_refs: Vec<_> = all_transfers
                    .iter()
                    .map(|(t, h, b, ts)| (t, h.as_slice(), *b, *ts))
                    .collect();
                self.writer
                    .process_udt_transfers_batch(&transfer_refs)
                    .await?;

                let mut address_asset_records: Vec<(
                    Vec<u8>,         // lock_script_hash
                    Vec<u8>,         // tx_hash
                    i64,             // block_number
                    i32,             // tx_index
                    i16,             // event_index
                    String,          // asset_category
                    String,          // asset_type
                    Option<Vec<u8>>, // asset_id
                    i16,             // direction
                    Option<Vec<u8>>, // peer_lock_hash
                    Option<String>,  // amount
                    Option<String>,  // event_type
                    DateTime<Utc>,   // timestamp
                )> = Vec::new();

                for (idx, (transfer, tx_hash, block_number, timestamp)) in
                    all_transfers.iter().enumerate()
                {
                    let tx_index = udt_tx_contexts
                        .iter()
                        .find(|ctx| ctx.tx_hash == *tx_hash)
                        .map(|ctx| ctx.tx_index)
                        .unwrap_or(0);

                    let standard_str = match transfer.standard {
                        crate::parser::UdtStandard::Sudt => "sudt",
                        crate::parser::UdtStandard::Xudt => "xudt",
                    };

                    if let Some(ref from_lock) = transfer.from_lock_hash {
                        address_asset_records.push((
                            from_lock.clone(),
                            tx_hash.clone(),
                            *block_number,
                            tx_index,
                            (idx * 2) as i16,
                            "token".to_string(),
                            standard_str.to_string(),
                            Some(transfer.type_script_hash.clone()),
                            2, // out
                            Some(transfer.to_lock_hash.clone()),
                            Some(transfer.amount.to_string()),
                            None,
                            *timestamp,
                        ));
                    }

                    if !transfer.to_lock_hash.is_empty() {
                        address_asset_records.push((
                            transfer.to_lock_hash.clone(),
                            tx_hash.clone(),
                            *block_number,
                            tx_index,
                            (idx * 2 + 1) as i16,
                            "token".to_string(),
                            standard_str.to_string(),
                            Some(transfer.type_script_hash.clone()),
                            1, // in
                            transfer.from_lock_hash.clone(),
                            Some(transfer.amount.to_string()),
                            None,
                            *timestamp,
                        ));
                    }
                }

                if !address_asset_records.is_empty() {
                    self.writer
                        .insert_address_asset_transfers_batch(&address_asset_records)
                        .await?;
                }
            }

            if !consumed_udt_outpoints.is_empty() {
                self.writer
                    .consume_udt_cells_batch(&consumed_udt_outpoints)
                    .await?;
            }
        }

        if !batch_udt_cells.is_empty() {
            let udt_cells_to_insert: Vec<_> = batch_udt_cells
                .iter()
                .map(|((tx_hash, idx), cell)| {
                    let block_number = udt_tx_contexts
                        .iter()
                        .find(|ctx| ctx.tx_hash == *tx_hash)
                        .map(|ctx| ctx.block_number)
                        .unwrap_or(0);
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

        struct NewSporeInfo {
            spore_id: Vec<u8>,
            owner_lock_hash: Vec<u8>,
            cluster_id: Option<Vec<u8>>,
            content_type: String,
            tx_hash: Vec<u8>,
            block_number: i64,
            tx_index: i32,
            timestamp: DateTime<Utc>,
        }
        let mut new_spores: Vec<NewSporeInfo> = Vec::new();

        struct NewMnftInfo {
            token_id: Vec<u8>,
            owner_lock_hash: Vec<u8>,
            class_id: Vec<u8>,
            issuer_id: Vec<u8>,
            name: Option<String>,
            tx_hash: Vec<u8>,
            block_number: i64,
            tx_index: i32,
            timestamp: DateTime<Utc>,
        }
        let mut new_mnft_tokens: Vec<NewMnftInfo> = Vec::new();

        struct NewDotbitInfo {
            account_id: Vec<u8>,
            owner_lock_hash: Vec<u8>,
            account_name: String,
            tx_hash: Vec<u8>,
            block_number: i64,
            tx_index: i32,
            timestamp: DateTime<Utc>,
        }
        let mut new_dotbit_accounts: Vec<NewDotbitInfo> = Vec::new();

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
                    new_spores.push(NewSporeInfo {
                        spore_id: spore.spore_id.clone(),
                        owner_lock_hash: spore.owner_lock_hash.clone(),
                        cluster_id: spore.cluster_id.clone(),
                        content_type: spore.content_type.clone(),
                        tx_hash: tx_data.hash.clone(),
                        block_number: parsed.number,
                        tx_index: tx_idx as i32,
                        timestamp: parsed.timestamp,
                    });
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
                    new_mnft_tokens.push(NewMnftInfo {
                        token_id: token.token_id.clone(),
                        owner_lock_hash: token.owner_lock_hash.clone(),
                        class_id: token.class_id.clone(),
                        issuer_id: token.class_id[0..20].to_vec(),
                        name: None,
                        tx_hash: tx_data.hash.clone(),
                        block_number: parsed.number,
                        tx_index: tx_idx as i32,
                        timestamp: parsed.timestamp,
                    });
                    self.writer
                        .insert_mnft_token(token, &tx_data.hash, output_index as i16, parsed.number)
                        .await?;
                }

                for (output_index, account) in DotbitParser::parse_accounts(tx).iter().enumerate() {
                    batch_dotbit_account_ids.insert(account.account_id.clone());
                    new_dotbit_accounts.push(NewDotbitInfo {
                        account_id: account.account_id.clone(),
                        owner_lock_hash: account.owner_lock_hash.clone(),
                        account_name: format!("0x{}", hex::encode(&account.account_id)),
                        tx_hash: tx_data.hash.clone(),
                        block_number: parsed.number,
                        tx_index: tx_idx as i32,
                        timestamp: parsed.timestamp,
                    });
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

        let mut dob_address_records: Vec<(
            Vec<u8>,
            Vec<u8>,
            i64,
            i32,
            i16,
            String,
            String,
            Option<Vec<u8>>,
            i16,
            Option<Vec<u8>>,
            Option<String>,
            Option<String>,
            DateTime<Utc>,
        )> = Vec::new();

        let skip_nft_transfer_tracking = self.is_bulk_sync_active();

        for spore_info in &new_spores {
            let prev_owner = if skip_nft_transfer_tracking {
                None
            } else {
                self.writer
                    .get_spore_owner_by_id(&spore_info.spore_id)
                    .await?
            };

            let is_mint = prev_owner.is_none()
                || prev_owner
                    .as_ref()
                    .map(|o| o == &spore_info.owner_lock_hash)
                    .unwrap_or(false);

            let dob_type = if spore_info.content_type.starts_with("dob/") {
                &spore_info.content_type
            } else {
                "spore"
            };

            let event_type = if is_mint { "mint" } else { "transfer" };

            self.writer
                .insert_dob_transfer(
                    &spore_info.spore_id,
                    spore_info.cluster_id.as_deref(),
                    dob_type,
                    &spore_info.tx_hash,
                    spore_info.block_number,
                    prev_owner.as_deref(),
                    &spore_info.owner_lock_hash,
                    event_type,
                    Some(&spore_info.content_type),
                    spore_info.timestamp,
                )
                .await?;

            if let Some(ref from_lock) = prev_owner {
                if from_lock != &spore_info.owner_lock_hash {
                    dob_address_records.push((
                        from_lock.clone(),
                        spore_info.tx_hash.clone(),
                        spore_info.block_number,
                        spore_info.tx_index,
                        0,
                        "dob".to_string(),
                        dob_type.to_string(),
                        Some(spore_info.spore_id.clone()),
                        2,
                        Some(spore_info.owner_lock_hash.clone()),
                        Some("1".to_string()),
                        None,
                        spore_info.timestamp,
                    ));
                }
            }

            dob_address_records.push((
                spore_info.owner_lock_hash.clone(),
                spore_info.tx_hash.clone(),
                spore_info.block_number,
                spore_info.tx_index,
                1,
                "dob".to_string(),
                dob_type.to_string(),
                Some(spore_info.spore_id.clone()),
                1,
                prev_owner.clone(),
                Some("1".to_string()),
                if is_mint {
                    Some("mint".to_string())
                } else {
                    None
                },
                spore_info.timestamp,
            ));
        }

        let mut nft_address_records: Vec<(
            Vec<u8>,
            Vec<u8>,
            i64,
            i32,
            i16,
            String,
            String,
            Option<Vec<u8>>,
            i16,
            Option<Vec<u8>>,
            Option<String>,
            Option<String>,
            DateTime<Utc>,
        )> = Vec::new();

        for mnft_info in &new_mnft_tokens {
            let prev_owner = self
                .writer
                .get_mnft_token_owner_by_id(&mnft_info.token_id)
                .await?;

            let is_mint = prev_owner.is_none()
                || prev_owner
                    .as_ref()
                    .map(|o| o == &mnft_info.owner_lock_hash)
                    .unwrap_or(false);

            let event_type = if is_mint { "mint" } else { "transfer" };

            self.writer
                .insert_nft_transfer(
                    &mnft_info.token_id,
                    "mnft",
                    Some(&mnft_info.issuer_id),
                    Some(&mnft_info.class_id),
                    &mnft_info.tx_hash,
                    mnft_info.block_number,
                    prev_owner.as_deref(),
                    &mnft_info.owner_lock_hash,
                    event_type,
                    mnft_info.name.as_deref(),
                    mnft_info.timestamp,
                )
                .await?;

            if let Some(ref from_lock) = prev_owner {
                if from_lock != &mnft_info.owner_lock_hash {
                    nft_address_records.push((
                        from_lock.clone(),
                        mnft_info.tx_hash.clone(),
                        mnft_info.block_number,
                        mnft_info.tx_index,
                        0,
                        "nft".to_string(),
                        "mnft".to_string(),
                        Some(mnft_info.token_id.clone()),
                        2,
                        Some(mnft_info.owner_lock_hash.clone()),
                        Some("1".to_string()),
                        None,
                        mnft_info.timestamp,
                    ));
                }
            }

            nft_address_records.push((
                mnft_info.owner_lock_hash.clone(),
                mnft_info.tx_hash.clone(),
                mnft_info.block_number,
                mnft_info.tx_index,
                1,
                "nft".to_string(),
                "mnft".to_string(),
                Some(mnft_info.token_id.clone()),
                1,
                prev_owner.clone(),
                Some("1".to_string()),
                if is_mint {
                    Some("mint".to_string())
                } else {
                    None
                },
                mnft_info.timestamp,
            ));
        }

        for dotbit_info in &new_dotbit_accounts {
            let prev_owner = self
                .writer
                .get_dotbit_owner_by_id(&dotbit_info.account_id)
                .await?;

            let is_register = prev_owner.is_none()
                || prev_owner
                    .as_ref()
                    .map(|o| o == &dotbit_info.owner_lock_hash)
                    .unwrap_or(false);

            let event_type = if is_register { "register" } else { "transfer" };

            self.writer
                .insert_nft_transfer(
                    &dotbit_info.account_id,
                    "dotbit",
                    None,
                    None,
                    &dotbit_info.tx_hash,
                    dotbit_info.block_number,
                    prev_owner.as_deref(),
                    &dotbit_info.owner_lock_hash,
                    event_type,
                    Some(&dotbit_info.account_name),
                    dotbit_info.timestamp,
                )
                .await?;

            if let Some(ref from_lock) = prev_owner {
                if from_lock != &dotbit_info.owner_lock_hash {
                    nft_address_records.push((
                        from_lock.clone(),
                        dotbit_info.tx_hash.clone(),
                        dotbit_info.block_number,
                        dotbit_info.tx_index,
                        0,
                        "nft".to_string(),
                        "dotbit".to_string(),
                        Some(dotbit_info.account_id.clone()),
                        2,
                        Some(dotbit_info.owner_lock_hash.clone()),
                        None,
                        None,
                        dotbit_info.timestamp,
                    ));
                }
            }

            nft_address_records.push((
                dotbit_info.owner_lock_hash.clone(),
                dotbit_info.tx_hash.clone(),
                dotbit_info.block_number,
                dotbit_info.tx_index,
                1,
                "nft".to_string(),
                "dotbit".to_string(),
                Some(dotbit_info.account_id.clone()),
                1,
                prev_owner.clone(),
                None,
                if is_register {
                    Some("register".to_string())
                } else {
                    None
                },
                dotbit_info.timestamp,
            ));
        }

        if !dob_address_records.is_empty() {
            self.writer
                .insert_address_asset_transfers_batch(&dob_address_records)
                .await?;
        }

        if !nft_address_records.is_empty() {
            self.writer
                .insert_address_asset_transfers_batch(&nft_address_records)
                .await?;
        }

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
    ) -> Result<()> {
        if all_parsed_blocks.is_empty() {
            return Ok(());
        }

        let mut batch_cells: HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)> = HashMap::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                batch_cells.insert(
                    (tx_data.hash.clone(), output_index as i16),
                    (
                        cell.capacity,
                        tx_data.block_number,
                        cell.lock_script_hash.clone(),
                        cell.data_size,
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
                    } else if let Some((cap, _, _, _)) = batch_cells.get(&key) {
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

        let block_refs: Vec<&crate::parser::block::ParsedBlock> =
            all_parsed_blocks.iter().collect();
        self.writer.insert_blocks_batch(&block_refs).await?;

        for parsed_block in all_parsed_blocks {
            if !parsed_block.proposals.is_empty() {
                self.writer
                    .insert_block_proposals_batch(parsed_block.number, &parsed_block.proposals)
                    .await?;
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
            let copy_router = self.copy_router.as_ref().unwrap();
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
            if !txs_for_batch.is_empty() {
                self.writer
                    .insert_transactions_batch(&txs_for_batch)
                    .await?;
            }

            if !all_cells.is_empty() {
                self.writer.insert_cells_batch(&all_cells).await?;
            }

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
                    } else if batch_cells.contains_key(&key) {
                        let created_block = all_tx_data
                            .iter()
                            .find(|td| td.hash == input.previous_tx_hash)
                            .map(|td| td.block_number)
                            .unwrap_or(tx_data.block_number);
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            created_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
                        ));
                    }
                }
            }
        }
        if !all_consumptions.is_empty() {
            self.writer.consume_cells_batch(&all_consumptions).await?;
        }

        let mut address_balance_changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>)> =
            HashMap::new();
        let mut address_tx_records: Vec<(Vec<u8>, Vec<u8>, i64, i16, i64, chrono::DateTime<Utc>)> =
            Vec::new();

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
                    } else if let Some((cap, _, lock_hash, _)) = batch_cells.get(&key) {
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

                let tx_type: i16 = match balance_change.cmp(&0) {
                    std::cmp::Ordering::Greater => 1,
                    std::cmp::Ordering::Less => 2,
                    std::cmp::Ordering::Equal => 3,
                };

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

                address_tx_records.push((
                    lock_hash.clone(),
                    tx_data.hash.clone(),
                    tx_data.block_number,
                    tx_type,
                    balance_change,
                    tx_data.timestamp,
                ));
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
                    } else if let Some((cap, _, _, _)) = batch_cells.get(&key) {
                        if let Some(src_tx) = all_tx_data
                            .iter()
                            .find(|td| td.hash == input.previous_tx_hash)
                        {
                            if let Some(cell) =
                                src_tx.cells.get(input.previous_output_index as usize)
                            {
                                let lock_key = (cell.lock_code_hash.clone(), false);
                                let entry =
                                    script_usage_changes.entry(lock_key).or_insert((0, 0, 0, 0));
                                entry.1 -= 1;
                                entry.3 -= cap;

                                if let Some(ref type_code_hash) = cell.type_code_hash {
                                    let type_key = (type_code_hash.clone(), true);
                                    let entry = script_usage_changes
                                        .entry(type_key)
                                        .or_insert((0, 0, 0, 0));
                                    entry.1 -= 1;
                                    entry.3 -= cap;
                                }
                            }
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
                if !address_tx_records.is_empty() {
                    self.writer
                        .insert_address_transactions_batch(&address_tx_records)
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
                        .or_else(|| batch_cells.get(&key).map(|(_, _, _, ds)| *ds as i64))
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
                        .entry(block_time_seconds as i32)
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
                        let bucket_minutes = ((epoch_duration_minutes / 2.0).floor() as i32) * 2;
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
        for parsed in all_parsed_blocks {
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
            tx_index: i32,
            timestamp: chrono::DateTime<Utc>,
            output_udts: Vec<crate::parser::ParsedUdtCell>,
            input_outpoints: Vec<(Vec<u8>, i16)>,
        }

        let mut udt_tx_contexts: Vec<UdtTxContext> = Vec::new();
        let mut all_input_outpoints_udt: Vec<(Vec<u8>, i16)> = Vec::new();
        let mut batch_udt_cells: HashMap<(Vec<u8>, i16), crate::parser::ParsedUdtCell> =
            HashMap::new();

        // Temp storage: we filter txs later based on whether they have UDT inputs
        struct TxInfoForUdt {
            tx_hash: Vec<u8>,
            block_number: i64,
            tx_index: i32,
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
                    tx_index: tx_idx as i32,
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
                    tx_index: tx_info.tx_index,
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
                let transfer_refs: Vec<_> = all_transfers
                    .iter()
                    .map(|(t, h, b, ts)| (t, h.as_slice(), *b, *ts))
                    .collect();
                self.writer
                    .process_udt_transfers_batch(&transfer_refs)
                    .await?;

                let mut address_asset_records: Vec<(
                    Vec<u8>,
                    Vec<u8>,
                    i64,
                    i32,
                    i16,
                    String,
                    String,
                    Option<Vec<u8>>,
                    i16,
                    Option<Vec<u8>>,
                    Option<String>,
                    Option<String>,
                    DateTime<Utc>,
                )> = Vec::new();

                for (idx, (transfer, tx_hash, block_number, timestamp)) in
                    all_transfers.iter().enumerate()
                {
                    let tx_index = udt_tx_contexts
                        .iter()
                        .find(|c| &c.tx_hash == tx_hash)
                        .map(|ctx| ctx.tx_index)
                        .unwrap_or(0);

                    let standard_str = match transfer.standard {
                        crate::parser::UdtStandard::Sudt => "sudt",
                        crate::parser::UdtStandard::Xudt => "xudt",
                    };

                    if let Some(ref from_lock) = transfer.from_lock_hash {
                        address_asset_records.push((
                            from_lock.clone(),
                            tx_hash.clone(),
                            *block_number,
                            tx_index,
                            (idx * 2) as i16,
                            "token".to_string(),
                            standard_str.to_string(),
                            Some(transfer.type_script_hash.clone()),
                            2,
                            Some(transfer.to_lock_hash.clone()),
                            Some(transfer.amount.to_string()),
                            None,
                            *timestamp,
                        ));
                    }

                    if !transfer.to_lock_hash.is_empty() {
                        address_asset_records.push((
                            transfer.to_lock_hash.clone(),
                            tx_hash.clone(),
                            *block_number,
                            tx_index,
                            (idx * 2 + 1) as i16,
                            "token".to_string(),
                            standard_str.to_string(),
                            Some(transfer.type_script_hash.clone()),
                            1,
                            transfer.from_lock_hash.clone(),
                            Some(transfer.amount.to_string()),
                            None,
                            *timestamp,
                        ));
                    }
                }

                if !address_asset_records.is_empty() {
                    self.writer
                        .insert_address_asset_transfers_batch(&address_asset_records)
                        .await?;
                }
            }

            if !consumed_udt_outpoints.is_empty() {
                self.writer
                    .consume_udt_cells_batch(&consumed_udt_outpoints)
                    .await?;
            }
        }

        if !batch_udt_cells.is_empty() {
            let udt_cells_to_insert: Vec<_> = batch_udt_cells
                .iter()
                .map(|((tx_hash, idx), cell)| {
                    let block_number = udt_tx_contexts
                        .iter()
                        .find(|ctx| ctx.tx_hash == *tx_hash)
                        .map(|ctx| ctx.block_number)
                        .unwrap_or(0);
                    (tx_hash.as_slice(), *idx, cell, block_number)
                })
                .collect();
            self.writer
                .insert_udt_cells_batch(&udt_cells_to_insert)
                .await?;
        }

        struct NewSporeInfo {
            spore_id: Vec<u8>,
            owner_lock_hash: Vec<u8>,
            cluster_id: Option<Vec<u8>>,
            content_type: String,
            tx_hash: Vec<u8>,
            block_number: i64,
            tx_index: i32,
            timestamp: DateTime<Utc>,
        }
        let mut batch_spore_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut new_spores: Vec<NewSporeInfo> = Vec::new();

        struct NewMnftInfo {
            token_id: Vec<u8>,
            owner_lock_hash: Vec<u8>,
            class_id: Vec<u8>,
            issuer_id: Vec<u8>,
            name: Option<String>,
            tx_hash: Vec<u8>,
            block_number: i64,
            tx_index: i32,
            timestamp: DateTime<Utc>,
        }
        let mut batch_mnft_class_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut batch_mnft_token_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut new_mnft_tokens: Vec<NewMnftInfo> = Vec::new();

        struct NewDotbitInfo {
            account_id: Vec<u8>,
            owner_lock_hash: Vec<u8>,
            account_name: String,
            tx_hash: Vec<u8>,
            block_number: i64,
            tx_index: i32,
            timestamp: DateTime<Utc>,
        }
        let mut batch_dotbit_account_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut new_dotbit_accounts: Vec<NewDotbitInfo> = Vec::new();

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
                    new_spores.push(NewSporeInfo {
                        spore_id: spore.spore_id.clone(),
                        owner_lock_hash: spore.owner_lock_hash.clone(),
                        cluster_id: spore.cluster_id.clone(),
                        content_type: spore.content_type.clone(),
                        tx_hash: tx_data.hash.clone(),
                        block_number: parsed.number,
                        tx_index: tx_idx as i32,
                        timestamp: parsed.timestamp,
                    });
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
                    new_mnft_tokens.push(NewMnftInfo {
                        token_id: token.token_id.clone(),
                        owner_lock_hash: token.owner_lock_hash.clone(),
                        class_id: token.class_id.clone(),
                        issuer_id: token.class_id[0..20].to_vec(),
                        name: None,
                        tx_hash: tx_data.hash.clone(),
                        block_number: parsed.number,
                        tx_index: tx_idx as i32,
                        timestamp: parsed.timestamp,
                    });
                    self.writer
                        .insert_mnft_token(token, &tx_data.hash, output_index as i16, parsed.number)
                        .await?;
                }

                for (output_index, account) in DotbitParser::parse_accounts(tx).iter().enumerate() {
                    batch_dotbit_account_ids.insert(account.account_id.clone());
                    new_dotbit_accounts.push(NewDotbitInfo {
                        account_id: account.account_id.clone(),
                        owner_lock_hash: account.owner_lock_hash.clone(),
                        account_name: format!("0x{}", hex::encode(&account.account_id)),
                        tx_hash: tx_data.hash.clone(),
                        block_number: parsed.number,
                        tx_index: tx_idx as i32,
                        timestamp: parsed.timestamp,
                    });
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

        let mut dob_address_records: Vec<(
            Vec<u8>,
            Vec<u8>,
            i64,
            i32,
            i16,
            String,
            String,
            Option<Vec<u8>>,
            i16,
            Option<Vec<u8>>,
            Option<String>,
            Option<String>,
            DateTime<Utc>,
        )> = Vec::new();

        let skip_nft_transfer_tracking = self.is_bulk_sync_active();

        for spore_info in &new_spores {
            let prev_owner = if skip_nft_transfer_tracking {
                None
            } else {
                self.writer
                    .get_spore_owner_by_id(&spore_info.spore_id)
                    .await?
            };

            let is_mint = prev_owner.is_none()
                || prev_owner
                    .as_ref()
                    .map(|o| o == &spore_info.owner_lock_hash)
                    .unwrap_or(false);

            let dob_type = if spore_info.content_type.starts_with("dob/") {
                &spore_info.content_type
            } else {
                "spore"
            };

            let event_type = if is_mint { "mint" } else { "transfer" };

            self.writer
                .insert_dob_transfer(
                    &spore_info.spore_id,
                    spore_info.cluster_id.as_deref(),
                    dob_type,
                    &spore_info.tx_hash,
                    spore_info.block_number,
                    prev_owner.as_deref(),
                    &spore_info.owner_lock_hash,
                    event_type,
                    Some(&spore_info.content_type),
                    spore_info.timestamp,
                )
                .await?;

            if let Some(ref from_lock) = prev_owner {
                if from_lock != &spore_info.owner_lock_hash {
                    dob_address_records.push((
                        from_lock.clone(),
                        spore_info.tx_hash.clone(),
                        spore_info.block_number,
                        spore_info.tx_index,
                        0,
                        "dob".to_string(),
                        dob_type.to_string(),
                        Some(spore_info.spore_id.clone()),
                        2,
                        Some(spore_info.owner_lock_hash.clone()),
                        Some("1".to_string()),
                        None,
                        spore_info.timestamp,
                    ));
                }
            }

            dob_address_records.push((
                spore_info.owner_lock_hash.clone(),
                spore_info.tx_hash.clone(),
                spore_info.block_number,
                spore_info.tx_index,
                1,
                "dob".to_string(),
                dob_type.to_string(),
                Some(spore_info.spore_id.clone()),
                1,
                prev_owner.clone(),
                Some("1".to_string()),
                if is_mint {
                    Some("mint".to_string())
                } else {
                    None
                },
                spore_info.timestamp,
            ));
        }

        let mut nft_address_records: Vec<(
            Vec<u8>,
            Vec<u8>,
            i64,
            i32,
            i16,
            String,
            String,
            Option<Vec<u8>>,
            i16,
            Option<Vec<u8>>,
            Option<String>,
            Option<String>,
            DateTime<Utc>,
        )> = Vec::new();

        for mnft_info in &new_mnft_tokens {
            let prev_owner = self
                .writer
                .get_mnft_token_owner_by_id(&mnft_info.token_id)
                .await?;

            let is_mint = prev_owner.is_none()
                || prev_owner
                    .as_ref()
                    .map(|o| o == &mnft_info.owner_lock_hash)
                    .unwrap_or(false);

            let event_type = if is_mint { "mint" } else { "transfer" };

            self.writer
                .insert_nft_transfer(
                    &mnft_info.token_id,
                    "mnft",
                    Some(&mnft_info.issuer_id),
                    Some(&mnft_info.class_id),
                    &mnft_info.tx_hash,
                    mnft_info.block_number,
                    prev_owner.as_deref(),
                    &mnft_info.owner_lock_hash,
                    event_type,
                    mnft_info.name.as_deref(),
                    mnft_info.timestamp,
                )
                .await?;

            if let Some(ref from_lock) = prev_owner {
                if from_lock != &mnft_info.owner_lock_hash {
                    nft_address_records.push((
                        from_lock.clone(),
                        mnft_info.tx_hash.clone(),
                        mnft_info.block_number,
                        mnft_info.tx_index,
                        0,
                        "nft".to_string(),
                        "mnft".to_string(),
                        Some(mnft_info.token_id.clone()),
                        2,
                        Some(mnft_info.owner_lock_hash.clone()),
                        Some("1".to_string()),
                        None,
                        mnft_info.timestamp,
                    ));
                }
            }

            nft_address_records.push((
                mnft_info.owner_lock_hash.clone(),
                mnft_info.tx_hash.clone(),
                mnft_info.block_number,
                mnft_info.tx_index,
                1,
                "nft".to_string(),
                "mnft".to_string(),
                Some(mnft_info.token_id.clone()),
                1,
                prev_owner.clone(),
                Some("1".to_string()),
                if is_mint {
                    Some("mint".to_string())
                } else {
                    None
                },
                mnft_info.timestamp,
            ));
        }

        for dotbit_info in &new_dotbit_accounts {
            let prev_owner = self
                .writer
                .get_dotbit_owner_by_id(&dotbit_info.account_id)
                .await?;

            let is_register = prev_owner.is_none()
                || prev_owner
                    .as_ref()
                    .map(|o| o == &dotbit_info.owner_lock_hash)
                    .unwrap_or(false);

            let event_type = if is_register { "register" } else { "transfer" };

            self.writer
                .insert_nft_transfer(
                    &dotbit_info.account_id,
                    "dotbit",
                    None,
                    None,
                    &dotbit_info.tx_hash,
                    dotbit_info.block_number,
                    prev_owner.as_deref(),
                    &dotbit_info.owner_lock_hash,
                    event_type,
                    Some(&dotbit_info.account_name),
                    dotbit_info.timestamp,
                )
                .await?;

            if let Some(ref from_lock) = prev_owner {
                if from_lock != &dotbit_info.owner_lock_hash {
                    nft_address_records.push((
                        from_lock.clone(),
                        dotbit_info.tx_hash.clone(),
                        dotbit_info.block_number,
                        dotbit_info.tx_index,
                        0,
                        "nft".to_string(),
                        "dotbit".to_string(),
                        Some(dotbit_info.account_id.clone()),
                        2,
                        Some(dotbit_info.owner_lock_hash.clone()),
                        None,
                        None,
                        dotbit_info.timestamp,
                    ));
                }
            }

            nft_address_records.push((
                dotbit_info.owner_lock_hash.clone(),
                dotbit_info.tx_hash.clone(),
                dotbit_info.block_number,
                dotbit_info.tx_index,
                1,
                "nft".to_string(),
                "dotbit".to_string(),
                Some(dotbit_info.account_id.clone()),
                1,
                prev_owner.clone(),
                None,
                if is_register {
                    Some("register".to_string())
                } else {
                    None
                },
                dotbit_info.timestamp,
            ));
        }

        if !dob_address_records.is_empty() {
            self.writer
                .insert_address_asset_transfers_batch(&dob_address_records)
                .await?;
        }

        if !nft_address_records.is_empty() {
            self.writer
                .insert_address_asset_transfers_batch(&nft_address_records)
                .await?;
        }

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
    ) -> Result<()> {
        let block = &block_response.block;
        let parsed = BlockParser::parse(block);
        let db_start = Instant::now();

        self.writer.insert_block(&parsed, 0).await?;

        if !parsed.proposals.is_empty() {
            self.writer
                .insert_block_proposals_batch(parsed.number, &parsed.proposals)
                .await?;
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

        // (capacity, created_at_block, lock_script_hash, data_size)
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
            .filter(|(h, i)| !input_cell_info.contains_key(&(h.clone(), *i)))
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

        // (capacity, created_at_block, lock_script_hash, data_size)
        let mut block_cells: HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)> = HashMap::new();
        for tx_data in &tx_data_list {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                block_cells.insert(
                    (tx_data.hash.clone(), output_index as i16),
                    (
                        cell.capacity,
                        parsed.number,
                        cell.lock_script_hash.clone(),
                        cell.data_size,
                    ),
                );
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
                    } else if let Some((cap, _, _, _)) = block_cells.get(&key) {
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
            self.writer.insert_cells_batch(&all_cells).await?;
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
            self.writer.consume_cells_batch(&all_consumptions).await?;
        }

        let mut address_balance_changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> =
            HashMap::new();
        let mut address_tx_records: Vec<(Vec<u8>, Vec<u8>, i64, i16, i64, chrono::DateTime<Utc>)> =
            Vec::new();

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
                    } else if let Some((cap, _, lock_hash, _)) = block_cells.get(&key) {
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

            let all_addresses: std::collections::HashSet<Vec<u8>> = tx_balance_changes
                .keys()
                .chain(tx_cells_created.keys())
                .chain(tx_cells_consumed.keys())
                .cloned()
                .collect();

            for lock_hash in all_addresses {
                let balance_change = tx_balance_changes.get(&lock_hash).copied().unwrap_or(0);
                let cells_created = tx_cells_created.get(&lock_hash).copied().unwrap_or(0);
                let cells_consumed = tx_cells_consumed.get(&lock_hash).copied().unwrap_or(0);

                let tx_type: i16 = match balance_change.cmp(&0) {
                    std::cmp::Ordering::Greater => 1,
                    std::cmp::Ordering::Less => 2,
                    std::cmp::Ordering::Equal => 3,
                };

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

                address_tx_records.push((
                    lock_hash.clone(),
                    tx_data.hash.clone(),
                    parsed.number,
                    tx_type,
                    balance_change,
                    parsed.timestamp,
                ));
            }
        }

        if !address_balance_changes.is_empty() {
            self.writer
                .update_address_balances_batch(&address_balance_changes)
                .await?;
        }

        if !address_tx_records.is_empty() {
            self.writer
                .insert_address_transactions_batch(&address_tx_records)
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
                    .or_else(|| block_cells.get(&key).map(|(_, _, _, ds)| *ds as i64))
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
                *batch_stats
                    .block_time_dist
                    .entry(block_time_seconds as i32)
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
        *prev_timestamp = Some(parsed.timestamp);

        if parsed.epoch_index == 0 && parsed.epoch_number > 0 {
            if let Some((prev_epoch_num, prev_start_ts, _)) = prev_epoch {
                if *prev_epoch_num == parsed.epoch_number - 1 {
                    let epoch_duration_minutes =
                        (parsed.timestamp - *prev_start_ts).num_seconds() as f64 / 60.0;
                    let bucket_minutes = ((epoch_duration_minutes / 2.0).floor() as i32) * 2;
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
            self.writer
                .update_sync_status(
                    block_number,
                    block_hash,
                    stats.sync_totals.0,
                    stats.sync_totals.1,
                    stats.sync_totals.2,
                    0,
                )
                .await?;
        }

        // Core statistics - always written
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

        for (date, (sum_ms, count)) in &stats.daily_block_times {
            if *count > 0 {
                let avg_ms = sum_ms / *count as i64;
                self.writer
                    .update_daily_avg_block_time_batch(*date, avg_ms, *count)
                    .await?;
            }
        }

        // Non-critical statistics - skipped during bulk sync (can be recalculated)
        if !bulk_sync_active {
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
        }

        let mut snapshot_dates: Vec<_> = stats.dao_snapshot_dates.iter().collect();
        snapshot_dates.sort();
        for date in snapshot_dates {
            self.writer.update_dao_daily_snapshot(*date).await?;
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

        if depth > REORG_LIMIT {
            error!(
                "DEEP FORK DETECTED! Depth {} exceeds limit {}. Manual intervention required.",
                depth, REORG_LIMIT
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
            depth, REORG_LIMIT
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
}
