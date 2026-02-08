#![allow(dead_code)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::cache::{CacheInvalidator, CellInfoCache};
use crate::config::Config;
use crate::db::writer::{
    u64_to_u256_bytes, ActivityRow, BatchData, BlockRow, CellInputRow, CellOutputRow, CellStateRow,
    DaoDepositRow, DynBatchWriter, MnftClassRow, MnftIssuerRow, MnftTokenRow, SporeCellRow,
    SporeClusterRow, TransactionRow, UdtCellRow, EMPTY_HASH,
};
use crate::db::{ClickHouseClient, LiveCellInfo, MemoryStats};
use crate::parser::dao::{DaoParser, DAO_CODE_HASH};
use crate::parser::{MnftParser, SporeParser, UdtParser};
use crate::rpc::{BlockView, CkbRpcClient, Script, TransactionView};
use crate::state::CanonVersionManager;

use super::SyncProgress;

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct MaxVersionRow {
    max_version: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct MaxBlockRow {
    max_block: u64,
}

#[derive(Debug, Clone, Default)]
pub struct DisconnectResult {
    pub cells_restored: usize,
    pub cells_invalidated: usize,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct CellOutputQueryRow {
    tx_hash: [u8; 32],
    output_index: u16,
    capacity: u64,
    lock_script_hash: [u8; 32],
    type_script_hash: [u8; 32],
    lock_code_hash: [u8; 32],
    type_code_hash: [u8; 32],
    data_size: u32,
    block_number: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct CellInputQueryRow {
    previous_tx_hash: [u8; 32],
    previous_output_index: u16,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct CellStateQueryRow {
    tx_hash: [u8; 32],
    output_index: u16,
    capacity: u64,
    lock_script_hash: [u8; 32],
    type_script_hash: [u8; 32],
    lock_code_hash: [u8; 32],
    type_code_hash: [u8; 32],
    data_size: u32,
    created_at_block: u64,
}

pub struct Indexer {
    client: ClickHouseClient,
    batch_writer: DynBatchWriter,
    canon_version_mgr: CanonVersionManager,
    cell_cache: CellInfoCache,
    progress: Arc<SyncProgress>,
    cache_invalidator: CacheInvalidator,
    memory_stats: MemoryStats,
    fast_sync_mode: bool,
    rpc_client: CkbRpcClient,
    poll_interval_ms: u64,
    batch_size: usize,
}

pub struct IndexerConfig {
    pub clickhouse_url: String,
    pub clickhouse_database: String,
    pub cell_cache_capacity: usize,
    pub fast_sync_mode: bool,
    pub redis_url: Option<String>,
    pub ckb_rpc_url: String,
    pub poll_interval_ms: u64,
    pub batch_size: usize,
    pub use_inserter_api: bool,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            clickhouse_url: "http://localhost:8123".to_string(),
            clickhouse_database: "ckbadger".to_string(),
            cell_cache_capacity: 1_000_000,
            fast_sync_mode: true,
            redis_url: None,
            ckb_rpc_url: "http://localhost:8114".to_string(),
            poll_interval_ms: 1000,
            batch_size: 100,
            use_inserter_api: false,
        }
    }
}

impl Indexer {
    pub async fn new(config: IndexerConfig) -> Result<Self> {
        let ch_config =
            crate::db::ClickHouseConfig::new(&config.clickhouse_url, &config.clickhouse_database);
        let client = ClickHouseClient::new(ch_config);

        client
            .ping()
            .await
            .context("Failed to connect to ClickHouse")?;

        let max_version = Self::fetch_max_canon_version(&client).await?;
        let canon_version_mgr = CanonVersionManager::recover_from_db(max_version);

        let batch_writer = if config.use_inserter_api {
            info!("Using Inserter API for batch writes");
            DynBatchWriter::inserter(client.clone(), config.fast_sync_mode)
        } else {
            info!("Using standard Insert API for batch writes");
            DynBatchWriter::standard(client.clone(), config.fast_sync_mode)
        };
        let cell_cache = CellInfoCache::new(config.cell_cache_capacity);
        let cache_invalidator = CacheInvalidator::new(config.redis_url.as_deref()).await;
        let rpc_client = CkbRpcClient::new(&config.ckb_rpc_url);

        Ok(Self {
            client,
            batch_writer,
            canon_version_mgr,
            cell_cache,
            progress: Arc::new(SyncProgress::new(0, 0)),
            cache_invalidator,
            memory_stats: MemoryStats::default(),
            fast_sync_mode: config.fast_sync_mode,
            rpc_client,
            poll_interval_ms: config.poll_interval_ms,
            batch_size: config.batch_size,
        })
    }

    pub async fn from_legacy_config(config: Config) -> Result<Self> {
        let cache_invalidator = CacheInvalidator::new(config.redis_url.as_deref()).await;

        let ch_config = crate::db::ClickHouseConfig::from_env()?;
        let client = ClickHouseClient::new(ch_config);

        client
            .ping()
            .await
            .context("Failed to connect to ClickHouse")?;

        let max_version = Self::fetch_max_canon_version(&client).await?;
        let canon_version_mgr = CanonVersionManager::recover_from_db(max_version);

        let batch_writer = if config.use_inserter_api {
            info!("Using Inserter API for batch writes");
            DynBatchWriter::inserter(client.clone(), config.fast_sync_mode)
        } else {
            info!("Using standard Insert API for batch writes");
            DynBatchWriter::standard(client.clone(), config.fast_sync_mode)
        };
        let cell_cache = CellInfoCache::new(1_000_000);
        let rpc_client = CkbRpcClient::new(&config.ckb_rpc_url);

        Ok(Self {
            client,
            batch_writer,
            canon_version_mgr,
            cell_cache,
            progress: Arc::new(SyncProgress::new(0, 0)),
            cache_invalidator,
            memory_stats: MemoryStats::default(),
            fast_sync_mode: config.fast_sync_mode,
            rpc_client,
            poll_interval_ms: config.poll_interval_ms,
            batch_size: config.batch_size,
        })
    }

    async fn fetch_max_canon_version(client: &ClickHouseClient) -> Result<Option<u64>> {
        let rows: Vec<MaxVersionRow> = client
            .query_all("SELECT max(canon_version) as max_version FROM canonical_blocks")
            .await?;
        Ok(rows.first().and_then(|r| {
            if r.max_version == 0 {
                None
            } else {
                Some(r.max_version)
            }
        }))
    }

    pub fn progress(&self) -> Arc<SyncProgress> {
        Arc::clone(&self.progress)
    }

    pub fn cache_invalidator(&self) -> &CacheInvalidator {
        &self.cache_invalidator
    }

    pub fn get_memory_stats(&self) -> MemoryStats {
        self.memory_stats.clone()
    }

    pub fn is_bulk_sync_active(&self) -> bool {
        self.progress.blocks_remaining() > 72
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting indexer sync loop");

        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        ctrlc::set_handler(move || {
            warn!("Received shutdown signal, stopping indexer...");
            shutdown_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .context("Failed to set Ctrl-C handler")?;

        let mut last_log_time = Instant::now();
        let log_interval = Duration::from_secs(10);

        loop {
            if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
                info!("Shutdown signal received, stopping sync loop");
                break;
            }

            let tip_block = match self.rpc_client.get_tip_block_number().await {
                Ok(tip) => tip,
                Err(e) => {
                    warn!("Failed to get tip block number: {}", e);
                    tokio::time::sleep(Duration::from_millis(self.poll_interval_ms)).await;
                    continue;
                }
            };

            self.progress.update_target(tip_block);

            let synced_block = self.get_synced_block_number().await?;

            if synced_block >= tip_block {
                tokio::time::sleep(Duration::from_millis(self.poll_interval_ms)).await;
                continue;
            }

            let start_block = synced_block + 1;

            if start_block > 0 {
                if let Err(e) = self.check_and_handle_reorg(start_block).await {
                    warn!("Reorg handling failed: {}", e);
                    tokio::time::sleep(Duration::from_millis(self.poll_interval_ms)).await;
                    continue;
                }
            }

            let end_block = std::cmp::min(start_block + self.batch_size as u64 - 1, tip_block);
            let blocks_to_fetch: Vec<u64> = (start_block..=end_block).collect();

            let block_responses = match self.rpc_client.get_blocks_batch(&blocks_to_fetch).await {
                Ok(blocks) => blocks,
                Err(e) => {
                    warn!(
                        "Failed to fetch blocks {}-{}: {}",
                        start_block, end_block, e
                    );
                    tokio::time::sleep(Duration::from_millis(self.poll_interval_ms)).await;
                    continue;
                }
            };

            let blocks: Vec<_> = block_responses
                .into_iter()
                .filter_map(|r| r.map(|resp| resp.block))
                .collect();

            if blocks.is_empty() {
                warn!(
                    "No blocks returned for range {}..={}",
                    start_block, end_block
                );
                tokio::time::sleep(Duration::from_millis(self.poll_interval_ms)).await;
                continue;
            }

            let blocks_count = blocks.len();

            if let Err(e) = self.sync_blocks_batch(&blocks).await {
                warn!(
                    "Failed to process batch {}-{}: {}",
                    start_block, end_block, e
                );
                tokio::time::sleep(Duration::from_millis(self.poll_interval_ms)).await;
                continue;
            }

            self.progress
                .update_current_batch(end_block, blocks_count as u64);

            if last_log_time.elapsed() >= log_interval {
                let current = self.progress.current();
                let target = self.progress.target();
                let percentage = self.progress.progress_percentage();
                let rate = self.progress.blocks_per_second();
                let ema_rate = self.progress.ema_blocks_per_second();
                let eta = self.progress.eta_formatted();

                info!(
                    "Progress: {:.2}% ({}/{}) - {:.1} blocks/sec (EMA: {:.1}) - ETA: {}",
                    percentage, current, target, rate, ema_rate, eta
                );

                let sync_progress_data = ckbadger_common::SyncProgressData {
                    current_block: current,
                    target_block: target,
                    blocks_per_second: rate,
                    ema_blocks_per_second: ema_rate,
                    eta_seconds: self.progress.eta_seconds(),
                    eta_formatted: eta,
                    progress_percentage: percentage,
                    updated_at: chrono::Utc::now().timestamp(),
                };
                self.cache_invalidator
                    .publish_sync_progress(&sync_progress_data)
                    .await;

                last_log_time = Instant::now();
            }

            info!(
                "Wrote blocks {} to {} ({} blocks)",
                start_block,
                start_block + blocks_count as u64 - 1,
                blocks_count
            );
        }

        info!("Indexer sync loop stopped gracefully");
        Ok(())
    }

    async fn get_synced_block_number(&self) -> Result<u64> {
        let rows: Vec<MaxBlockRow> = self
            .client
            .query_all("SELECT max(number) as max_block FROM blocks_all")
            .await?;

        Ok(rows.first().map(|r| r.max_block).unwrap_or(0))
    }

    async fn get_indexed_block_hash(&self, block_number: u64) -> Result<Option<[u8; 32]>> {
        #[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
        struct BlockHashRow {
            hash: [u8; 32],
        }
        let query = format!(
            "SELECT hash FROM blocks_all WHERE number = {} LIMIT 1",
            block_number
        );
        let rows: Vec<BlockHashRow> = self.client.query_all(&query).await?;
        Ok(rows.first().map(|r| r.hash))
    }

    async fn check_and_handle_reorg(&self, next_block: u64) -> Result<()> {
        let prev_block = next_block - 1;

        let indexed_hash = match self.get_indexed_block_hash(prev_block).await? {
            Some(h) => h,
            None => return Ok(()),
        };

        let canonical_hash = match self.rpc_client.get_block_hash(prev_block).await? {
            Some(h) => parse_hex_bytes32(&h)?,
            None => return Ok(()),
        };

        if indexed_hash == canonical_hash {
            return Ok(());
        }

        warn!(
            "Reorg detected at block {}: indexed={}, canonical={}",
            prev_block,
            hex::encode(indexed_hash),
            hex::encode(canonical_hash)
        );

        let fork_point = self.find_fork_point(prev_block).await?;
        info!(
            "Fork point at block {}, disconnecting {} blocks",
            fork_point,
            prev_block - fork_point
        );

        for block_num in (fork_point + 1..=prev_block).rev() {
            let result = self.disconnect_block(block_num).await?;
            info!(
                "Disconnected block {}: {} invalidated, {} restored",
                block_num, result.cells_invalidated, result.cells_restored
            );
        }

        Ok(())
    }

    async fn find_fork_point(&self, start_block: u64) -> Result<u64> {
        let min_block = start_block.saturating_sub(72);

        for check_block in (min_block..start_block).rev() {
            let indexed_hash = match self.get_indexed_block_hash(check_block).await? {
                Some(h) => h,
                None => return Ok(check_block),
            };

            let canonical_hash = match self.rpc_client.get_block_hash(check_block).await? {
                Some(h) => parse_hex_bytes32(&h)?,
                None => return Ok(check_block),
            };

            if indexed_hash == canonical_hash {
                return Ok(check_block);
            }
        }

        warn!(
            "Fork point not found within 72 blocks, using block {}",
            min_block
        );
        Ok(min_block)
    }

    pub async fn connect_block(&self, block: &BlockView) -> Result<()> {
        let canon_version = self.canon_version_mgr.next();
        let block_number = parse_hex_u64(&block.header.number)?;
        let block_hash = parse_hex_bytes32(&block.header.hash)?;
        let block_timestamp_ms = parse_hex_u64(&block.header.timestamp)? as i64;

        let block_row =
            self.extract_block_row(block, block_number, &block_hash, block_timestamp_ms)?;

        let mut transaction_rows = Vec::with_capacity(block.transactions.len());
        let mut cell_output_rows = Vec::new();
        let mut cell_input_rows = Vec::new();
        let mut cell_state_rows = Vec::new();
        let mut dao_deposits = Vec::new();

        let dao_code_hash = parse_hex_bytes32(DAO_CODE_HASH)?;
        let dao_field = parse_hex_bytes32(&block.header.dao)?;
        let block_ar = DaoParser::extract_ar_from_dao_field(&dao_field).unwrap_or(0);

        for (tx_index, tx) in block.transactions.iter().enumerate() {
            let tx_hash = parse_hex_bytes32(&tx.hash)?;
            let is_cellbase = tx_index == 0;

            let (tx_row, outputs, inputs) = self.extract_transaction_data(
                tx,
                &tx_hash,
                block_number,
                &block_hash,
                tx_index as u32,
                block_timestamp_ms,
                is_cellbase,
            )?;

            transaction_rows.push(tx_row);
            cell_output_rows.extend(outputs);
            cell_input_rows.extend(inputs);

            for (output_index, output) in tx.outputs.iter().enumerate() {
                let data = tx
                    .outputs_data
                    .get(output_index)
                    .map(|s| s.as_str())
                    .unwrap_or("0x");
                let data_bytes = parse_hex_bytes(data)?;

                let lock_script_hash = compute_script_hash(&output.lock)?;
                let lock_code_hash = parse_hex_bytes32(&output.lock.code_hash)?;
                let type_script_hash = match &output.type_ {
                    Some(type_script) => compute_script_hash(type_script)?,
                    None => [0u8; 32],
                };
                let type_code_hash = match &output.type_ {
                    Some(type_script) => parse_hex_bytes32(&type_script.code_hash)?,
                    None => [0u8; 32],
                };
                let capacity = parse_hex_u64(&output.capacity)?;

                let live_state = CellStateRow::new_live(
                    tx_hash,
                    output_index as u16,
                    canon_version,
                    capacity,
                    lock_script_hash,
                    type_script_hash,
                    lock_code_hash,
                    type_code_hash,
                    data_bytes.len() as u32,
                    block_number,
                );
                cell_state_rows.push(live_state);

                let cell_info = LiveCellInfo {
                    capacity: capacity as i64,
                    created_at_block: block_number as i64,
                    lock_script_hash: lock_script_hash.to_vec(),
                    lock_code_hash: lock_code_hash.to_vec(),
                    lock_args: parse_hex_bytes(&output.lock.args)?,
                    type_script_hash: if type_script_hash == [0u8; 32] {
                        None
                    } else {
                        Some(type_script_hash.to_vec())
                    },
                    type_code_hash: if type_code_hash == [0u8; 32] {
                        None
                    } else {
                        Some(type_code_hash.to_vec())
                    },
                    data_size: data_bytes.len() as i32,
                };
                self.cell_cache
                    .insert(tx_hash.to_vec(), output_index as i16, cell_info);

                if type_code_hash == dao_code_hash
                    && data_bytes.len() == 8
                    && data_bytes == [0u8; 8]
                {
                    let dao_row = DaoDepositRow::new_deposit(
                        tx_hash,
                        output_index as u16,
                        canon_version,
                        lock_script_hash,
                        capacity,
                        block_number,
                        block_timestamp_ms,
                        block_ar,
                    );
                    dao_deposits.push(dao_row);
                }
            }

            if !is_cellbase {
                for (input_index, input) in tx.inputs.iter().enumerate() {
                    let prev_tx_hash = parse_hex_bytes32(&input.previous_output.tx_hash)?;
                    let prev_output_index = parse_hex_u32(&input.previous_output.index)? as u16;

                    if let Some(cell_info) =
                        self.cell_cache.get(&prev_tx_hash, prev_output_index as i16)
                    {
                        let type_script_hash = cell_info
                            .type_script_hash
                            .as_ref()
                            .map(|v| to_bytes32(v))
                            .unwrap_or([0u8; 32]);
                        let type_code_hash = cell_info
                            .type_code_hash
                            .as_ref()
                            .map(|v| to_bytes32(v))
                            .unwrap_or([0u8; 32]);

                        let consumed_state = CellStateRow::new_consumed(
                            prev_tx_hash,
                            prev_output_index,
                            canon_version,
                            tx_hash,
                            block_number,
                            input_index as u16,
                            cell_info.capacity as u64,
                            to_bytes32(&cell_info.lock_script_hash),
                            type_script_hash,
                            to_bytes32(&cell_info.lock_code_hash),
                            type_code_hash,
                            cell_info.data_size as u32,
                            cell_info.created_at_block as u64,
                        );
                        cell_state_rows.push(consumed_state);
                    }
                }
            }
        }

        let activities =
            self.generate_activities(block, block_number, &block_hash, block_timestamp_ms)?;

        let (udt_cells, spore_clusters, spore_cells, mnft_issuers, mnft_classes, mnft_tokens) =
            self.parse_asset_data(
                block,
                block_number,
                &block_hash,
                block_timestamp_ms,
                canon_version,
            )?;

        let batch_data = BatchData {
            blocks: vec![block_row],
            transactions: transaction_rows,
            cell_outputs: cell_output_rows,
            cell_inputs: cell_input_rows,
            activities,
            cell_states: cell_state_rows,
            dao_deposits,
            canonical_mappings: vec![(block_number, block_hash.to_vec(), canon_version)],
            udt_cells,
            spore_clusters,
            spore_cells,
            mnft_issuers,
            mnft_classes,
            mnft_tokens,
        };

        self.batch_writer.write_batch(&batch_data).await?;
        self.progress.update_current(block_number);

        Ok(())
    }

    /// Sync multiple blocks in a single batch for improved throughput.
    pub async fn sync_blocks_batch(&self, blocks: &[BlockView]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let canon_version = self.canon_version_mgr.next();

        let mut all_block_rows = Vec::with_capacity(blocks.len());
        let mut all_tx_rows = Vec::new();
        let mut all_output_rows = Vec::new();
        let mut all_input_rows = Vec::new();
        let mut all_cell_state_rows = Vec::new();
        let mut all_activities = Vec::new();
        let mut all_dao_deposits = Vec::new();
        let mut all_canonical_mappings = Vec::with_capacity(blocks.len());
        let mut all_udt_cells = Vec::new();
        let mut all_spore_clusters = Vec::new();
        let mut all_spore_cells = Vec::new();
        let mut all_mnft_issuers = Vec::new();
        let mut all_mnft_classes = Vec::new();
        let mut all_mnft_tokens = Vec::new();

        let dao_code_hash = parse_hex_bytes32(DAO_CODE_HASH)?;

        for block in blocks {
            let block_number = parse_hex_u64(&block.header.number)?;
            let block_hash = parse_hex_bytes32(&block.header.hash)?;
            let block_timestamp_ms = parse_hex_u64(&block.header.timestamp)? as i64;

            let block_row =
                self.extract_block_row(block, block_number, &block_hash, block_timestamp_ms)?;
            all_block_rows.push(block_row);

            for (tx_index, tx) in block.transactions.iter().enumerate() {
                let tx_hash = parse_hex_bytes32(&tx.hash)?;
                let is_cellbase = tx_index == 0;

                let (tx_row, outputs, inputs) = self.extract_transaction_data(
                    tx,
                    &tx_hash,
                    block_number,
                    &block_hash,
                    tx_index as u32,
                    block_timestamp_ms,
                    is_cellbase,
                )?;

                all_tx_rows.push(tx_row);
                all_output_rows.extend(outputs);
                all_input_rows.extend(inputs);

                for (output_index, output) in tx.outputs.iter().enumerate() {
                    let data = tx
                        .outputs_data
                        .get(output_index)
                        .map(|s| s.as_str())
                        .unwrap_or("0x");
                    let data_bytes = parse_hex_bytes(data)?;

                    let lock_script_hash = compute_script_hash(&output.lock)?;
                    let lock_code_hash = parse_hex_bytes32(&output.lock.code_hash)?;
                    let type_script_hash = match &output.type_ {
                        Some(type_script) => compute_script_hash(type_script)?,
                        None => [0u8; 32],
                    };
                    let type_code_hash = match &output.type_ {
                        Some(type_script) => parse_hex_bytes32(&type_script.code_hash)?,
                        None => [0u8; 32],
                    };
                    let capacity = parse_hex_u64(&output.capacity)?;

                    let live_state = CellStateRow::new_live(
                        tx_hash,
                        output_index as u16,
                        canon_version,
                        capacity,
                        lock_script_hash,
                        type_script_hash,
                        lock_code_hash,
                        type_code_hash,
                        data_bytes.len() as u32,
                        block_number,
                    );
                    all_cell_state_rows.push(live_state);

                    let cell_info = LiveCellInfo {
                        capacity: capacity as i64,
                        created_at_block: block_number as i64,
                        lock_script_hash: lock_script_hash.to_vec(),
                        lock_code_hash: lock_code_hash.to_vec(),
                        lock_args: parse_hex_bytes(&output.lock.args)?,
                        type_script_hash: if type_script_hash == [0u8; 32] {
                            None
                        } else {
                            Some(type_script_hash.to_vec())
                        },
                        type_code_hash: if type_code_hash == [0u8; 32] {
                            None
                        } else {
                            Some(type_code_hash.to_vec())
                        },
                        data_size: data_bytes.len() as i32,
                    };
                    self.cell_cache
                        .insert(tx_hash.to_vec(), output_index as i16, cell_info);

                    if type_code_hash == dao_code_hash && data_bytes.len() == 8 {
                        let dao_field = parse_hex_bytes32(&block.header.dao)?;
                        let deposit_ar =
                            DaoParser::extract_ar_from_dao_field(&dao_field).unwrap_or(0);

                        if data_bytes == [0u8; 8] {
                            let dao_row = DaoDepositRow::new_deposit(
                                tx_hash,
                                output_index as u16,
                                canon_version,
                                lock_script_hash,
                                capacity,
                                block_number,
                                block_timestamp_ms,
                                deposit_ar,
                            );
                            all_dao_deposits.push(dao_row);
                        }
                    }
                }

                if !is_cellbase {
                    for (input_index, input) in tx.inputs.iter().enumerate() {
                        let prev_tx_hash = parse_hex_bytes32(&input.previous_output.tx_hash)?;
                        let prev_output_index = parse_hex_u32(&input.previous_output.index)? as u16;

                        if let Some(cell_info) =
                            self.cell_cache.get(&prev_tx_hash, prev_output_index as i16)
                        {
                            let type_script_hash = cell_info
                                .type_script_hash
                                .as_ref()
                                .map(|v| to_bytes32(v))
                                .unwrap_or([0u8; 32]);
                            let type_code_hash = cell_info
                                .type_code_hash
                                .as_ref()
                                .map(|v| to_bytes32(v))
                                .unwrap_or([0u8; 32]);

                            let consumed_state = CellStateRow::new_consumed(
                                prev_tx_hash,
                                prev_output_index,
                                canon_version,
                                tx_hash,
                                block_number,
                                input_index as u16,
                                cell_info.capacity as u64,
                                to_bytes32(&cell_info.lock_script_hash),
                                type_script_hash,
                                to_bytes32(&cell_info.lock_code_hash),
                                type_code_hash,
                                cell_info.data_size as u32,
                                cell_info.created_at_block as u64,
                            );
                            all_cell_state_rows.push(consumed_state);
                        }
                    }
                }
            }

            let block_activities =
                self.generate_activities(block, block_number, &block_hash, block_timestamp_ms)?;
            all_activities.extend(block_activities);

            let (udt_cells, spore_clusters, spore_cells, mnft_issuers, mnft_classes, mnft_tokens) =
                self.parse_asset_data(
                    block,
                    block_number,
                    &block_hash,
                    block_timestamp_ms,
                    canon_version,
                )?;
            all_udt_cells.extend(udt_cells);
            all_spore_clusters.extend(spore_clusters);
            all_spore_cells.extend(spore_cells);
            all_mnft_issuers.extend(mnft_issuers);
            all_mnft_classes.extend(mnft_classes);
            all_mnft_tokens.extend(mnft_tokens);

            all_canonical_mappings.push((block_number, block_hash.to_vec(), canon_version));
        }

        let batch_data = BatchData {
            blocks: all_block_rows,
            transactions: all_tx_rows,
            cell_outputs: all_output_rows,
            cell_inputs: all_input_rows,
            activities: all_activities,
            cell_states: all_cell_state_rows,
            dao_deposits: all_dao_deposits,
            canonical_mappings: all_canonical_mappings,
            udt_cells: all_udt_cells,
            spore_clusters: all_spore_clusters,
            spore_cells: all_spore_cells,
            mnft_issuers: all_mnft_issuers,
            mnft_classes: all_mnft_classes,
            mnft_tokens: all_mnft_tokens,
        };

        self.batch_writer.write_batch(&batch_data).await?;

        if let Some(last_block) = blocks.last() {
            let last_block_number = parse_hex_u64(&last_block.header.number)?;
            self.progress.update_current(last_block_number);
        }

        Ok(())
    }

    pub async fn disconnect_block(&self, block_number: u64) -> Result<DisconnectResult> {
        let mut cells_invalidated = 0usize;
        let mut cells_restored = 0usize;
        let mut cell_state_rows = Vec::new();

        let outputs_query = format!(
            "SELECT tx_hash, output_index, capacity, lock_script_hash, type_script_hash, \
             lock_code_hash, type_code_hash, data_size, block_number \
             FROM cell_outputs_all WHERE block_number = {}",
            block_number
        );
        let outputs_created: Vec<CellOutputQueryRow> =
            self.client.query_all(&outputs_query).await?;

        for output in &outputs_created {
            let canon_version = self.canon_version_mgr.next();

            let invalidated_state = CellStateRow::new_invalidated(
                output.tx_hash,
                output.output_index,
                canon_version,
                output.capacity,
                output.lock_script_hash,
                output.type_script_hash,
                output.lock_code_hash,
                output.type_code_hash,
                output.data_size,
                output.block_number,
            );
            cell_state_rows.push(invalidated_state);

            self.cell_cache
                .invalidate(&output.tx_hash, output.output_index as i16);
            cells_invalidated += 1;
        }

        let inputs_query = format!(
            "SELECT previous_tx_hash, previous_output_index \
             FROM cell_inputs_all WHERE tx_block_number = {}",
            block_number
        );
        let inputs_consumed: Vec<CellInputQueryRow> = self.client.query_all(&inputs_query).await?;

        for input in &inputs_consumed {
            let state_query = format!(
                "SELECT tx_hash, output_index, capacity, lock_script_hash, type_script_hash, \
                 lock_code_hash, type_code_hash, data_size, created_at_block \
                 FROM cell_state FINAL \
                 WHERE tx_hash = unhex('{}') AND output_index = {} \
                 AND is_present = 1 \
                 LIMIT 1",
                hex::encode(input.previous_tx_hash),
                input.previous_output_index
            );
            let cell_states: Vec<CellStateQueryRow> = self.client.query_all(&state_query).await?;

            if let Some(cell_state) = cell_states.first() {
                let canon_version = self.canon_version_mgr.next();

                let restored_state = CellStateRow::new_restored(
                    cell_state.tx_hash,
                    cell_state.output_index,
                    canon_version,
                    cell_state.capacity,
                    cell_state.lock_script_hash,
                    cell_state.type_script_hash,
                    cell_state.lock_code_hash,
                    cell_state.type_code_hash,
                    cell_state.data_size,
                    cell_state.created_at_block,
                );
                cell_state_rows.push(restored_state);

                self.cell_cache
                    .invalidate(&input.previous_tx_hash, input.previous_output_index as i16);
                cells_restored += 1;
            }
        }

        if !cell_state_rows.is_empty() {
            self.batch_writer
                .write_cell_states(&cell_state_rows)
                .await?;
        }

        Ok(DisconnectResult {
            cells_restored,
            cells_invalidated,
        })
    }

    fn generate_activities(
        &self,
        block: &BlockView,
        block_number: u64,
        _block_hash: &[u8; 32],
        timestamp_ms: i64,
    ) -> Result<Vec<ActivityRow>> {
        let mut activities = Vec::new();
        let mut activity_index = 0u16;

        for (tx_index, tx) in block.transactions.iter().enumerate() {
            let tx_hash = parse_hex_bytes32(&tx.hash)?;
            let is_cellbase = tx_index == 0;

            if is_cellbase {
                let cellbase_activity = self.generate_cellbase_activity(
                    tx,
                    &tx_hash,
                    block_number,
                    tx_index as u32,
                    activity_index,
                    timestamp_ms,
                )?;
                if let Some(activity) = cellbase_activity {
                    activities.push(activity);
                    activity_index += 1;
                }
            } else {
                let transfer_activities = self.generate_transfer_activities(
                    tx,
                    &tx_hash,
                    block_number,
                    tx_index as u32,
                    &mut activity_index,
                    timestamp_ms,
                )?;
                activities.extend(transfer_activities);
            }
        }

        Ok(activities)
    }

    fn generate_cellbase_activity(
        &self,
        tx: &TransactionView,
        tx_hash: &[u8; 32],
        block_number: u64,
        tx_index: u32,
        activity_index: u16,
        timestamp_ms: i64,
    ) -> Result<Option<ActivityRow>> {
        if tx.outputs.is_empty() {
            return Ok(None);
        }

        let first_output = &tx.outputs[0];
        let capacity = parse_hex_u64(&first_output.capacity)?;
        let to_lock_hash = compute_script_hash(&first_output.lock)?;

        let activity_id = generate_activity_id(tx_hash, "CELLBASE_REWARD", activity_index);

        Ok(Some(ActivityRow {
            activity_id,
            activity_type: "CELLBASE_REWARD".to_string(),
            activity_category: "cellbase".to_string(),
            block_number,
            tx_hash: *tx_hash,
            tx_index,
            activity_index,
            from_lock_hash: [0u8; 32],
            to_lock_hash,
            amount: clickhouse::types::UInt256::from_le_bytes(u64_to_u256_bytes(capacity)),
            asset_id: [0u8; 32],
            metadata: String::new(),
            timestamp: timestamp_ms,
        }))
    }

    fn generate_transfer_activities(
        &self,
        tx: &TransactionView,
        tx_hash: &[u8; 32],
        block_number: u64,
        tx_index: u32,
        activity_index: &mut u16,
        timestamp_ms: i64,
    ) -> Result<Vec<ActivityRow>> {
        let mut activities = Vec::new();

        let mut input_capacities: std::collections::HashMap<[u8; 32], u64> =
            std::collections::HashMap::new();
        for input in &tx.inputs {
            let prev_tx_hash = parse_hex_bytes32(&input.previous_output.tx_hash)?;
            let prev_output_index = parse_hex_u32(&input.previous_output.index)? as i16;

            if let Some(cell_info) = self.cell_cache.get(&prev_tx_hash, prev_output_index) {
                let lock_hash = to_bytes32(&cell_info.lock_script_hash);
                *input_capacities.entry(lock_hash).or_insert(0) += cell_info.capacity as u64;
            }
        }

        let mut output_capacities: std::collections::HashMap<[u8; 32], u64> =
            std::collections::HashMap::new();
        for output in &tx.outputs {
            let capacity = parse_hex_u64(&output.capacity)?;
            let lock_hash = compute_script_hash(&output.lock)?;
            *output_capacities.entry(lock_hash).or_insert(0) += capacity;
        }

        for (from_lock_hash, input_amount) in &input_capacities {
            let output_amount = output_capacities.get(from_lock_hash).copied().unwrap_or(0);

            if output_amount < *input_amount {
                let transfer_amount = input_amount - output_amount;

                for (to_lock_hash, to_amount) in &output_capacities {
                    if to_lock_hash == from_lock_hash {
                        continue;
                    }

                    let activity_id =
                        generate_activity_id(tx_hash, "CKB_TRANSFER", *activity_index);

                    activities.push(ActivityRow {
                        activity_id,
                        activity_type: "CKB_TRANSFER".to_string(),
                        activity_category: "ckb".to_string(),
                        block_number,
                        tx_hash: *tx_hash,
                        tx_index,
                        activity_index: *activity_index,
                        from_lock_hash: *from_lock_hash,
                        to_lock_hash: *to_lock_hash,
                        amount: clickhouse::types::UInt256::from_le_bytes(u64_to_u256_bytes(
                            transfer_amount.min(*to_amount),
                        )),
                        asset_id: [0u8; 32],
                        metadata: String::new(),
                        timestamp: timestamp_ms,
                    });

                    *activity_index += 1;
                    break;
                }
            }
        }

        Ok(activities)
    }

    fn extract_block_row(
        &self,
        block: &BlockView,
        block_number: u64,
        block_hash: &[u8; 32],
        timestamp_ms: i64,
    ) -> Result<BlockRow> {
        let header = &block.header;

        let epoch_info = parse_epoch(&header.epoch)?;

        Ok(BlockRow {
            number: block_number,
            hash: *block_hash,
            parent_hash: parse_hex_bytes32(&header.parent_hash)?,
            timestamp: timestamp_ms,
            version: parse_hex_u32(&header.version)?,
            compact_target: parse_hex_u64(&header.compact_target)?,
            transactions_count: block.transactions.len() as u32,
            proposals_count: block.proposals.len() as u32,
            uncles_count: block.uncles.len() as u8,
            epoch_number: epoch_info.0,
            epoch_index: epoch_info.1,
            epoch_length: epoch_info.2,
            dao: parse_hex_bytes32(&header.dao)?,
            nonce: parse_hex_bytes16(&header.nonce)?,
            extra_hash: parse_hex_bytes32(&header.extra_hash)?,
            extension: String::new(),
            proposals_hash: parse_hex_bytes32(&header.proposals_hash)?,
            transactions_root: parse_hex_bytes32(&header.transactions_root)?,
            uncles_hash: parse_hex_bytes32_with_default(&header.extra_hash, [0u8; 32]),
            miner_lock_hash: [0u8; 32],
            miner_message: String::new(),
            total_difficulty: clickhouse::types::UInt256::from_le_bytes([0u8; 32]),
            reward: 0,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn extract_transaction_data(
        &self,
        tx: &TransactionView,
        tx_hash: &[u8; 32],
        block_number: u64,
        block_hash: &[u8; 32],
        tx_index: u32,
        timestamp_ms: i64,
        is_cellbase: bool,
    ) -> Result<(TransactionRow, Vec<CellOutputRow>, Vec<CellInputRow>)> {
        let mut outputs = Vec::with_capacity(tx.outputs.len());
        let mut inputs = Vec::with_capacity(tx.inputs.len());

        let mut total_output_capacity = 0u64;
        for (output_index, output) in tx.outputs.iter().enumerate() {
            let capacity = parse_hex_u64(&output.capacity)?;
            total_output_capacity += capacity;

            let data = tx
                .outputs_data
                .get(output_index)
                .map(|s| s.as_str())
                .unwrap_or("0x");
            let data_bytes = parse_hex_bytes(data)?;
            let data_hash = blake2b_hash(&data_bytes);

            let lock_script_hash = compute_script_hash(&output.lock)?;
            let (type_code_hash, type_hash_type, type_args, type_script_hash) = match &output.type_
            {
                Some(type_script) => (
                    parse_hex_bytes32(&type_script.code_hash)?,
                    parse_hash_type(&type_script.hash_type),
                    type_script.args.clone(),
                    compute_script_hash(type_script)?,
                ),
                None => ([0u8; 32], 0, String::new(), [0u8; 32]),
            };

            let output_row = CellOutputRow {
                tx_hash: *tx_hash,
                output_index: output_index as u16,
                block_number,
                block_hash: *block_hash,
                capacity,
                lock_code_hash: parse_hex_bytes32(&output.lock.code_hash)?,
                lock_hash_type: parse_hash_type(&output.lock.hash_type),
                lock_args: output.lock.args.clone(),
                lock_script_hash,
                type_code_hash,
                type_hash_type,
                type_args,
                type_script_hash,
                data_hash,
                data_size: data_bytes.len() as u32,
                data: truncate_data_preview(data, 512),
            };
            outputs.push(output_row);
        }

        for (input_index, input) in tx.inputs.iter().enumerate() {
            let input_row = CellInputRow {
                tx_hash: *tx_hash,
                tx_block_number: block_number,
                input_index: input_index as u16,
                previous_tx_hash: parse_hex_bytes32(&input.previous_output.tx_hash)?,
                previous_output_index: parse_hex_u32(&input.previous_output.index)? as u16,
                since: parse_hex_u64(&input.since)?,
            };
            inputs.push(input_row);
        }

        let tx_row = TransactionRow {
            hash: *tx_hash,
            block_number,
            block_hash: *block_hash,
            tx_index,
            version: parse_hex_u32(&tx.version)?,
            inputs_count: tx.inputs.len() as u16,
            outputs_count: tx.outputs.len() as u16,
            witnesses_count: tx.witnesses.len() as u16,
            cell_deps_count: tx.cell_deps.len() as u16,
            header_deps_count: tx.header_deps.len() as u16,
            total_input_capacity: 0,
            total_output_capacity,
            fee: 0,
            tx_size: 0,
            cycles: 0,
            is_cellbase: if is_cellbase { 1 } else { 0 },
            timestamp: timestamp_ms,
        };

        Ok((tx_row, outputs, inputs))
    }

    #[allow(clippy::type_complexity)]
    fn parse_asset_data(
        &self,
        block: &BlockView,
        block_number: u64,
        _block_hash: &[u8; 32],
        timestamp_ms: i64,
        canon_version: u64,
    ) -> Result<(
        Vec<UdtCellRow>,
        Vec<SporeClusterRow>,
        Vec<SporeCellRow>,
        Vec<MnftIssuerRow>,
        Vec<MnftClassRow>,
        Vec<MnftTokenRow>,
    )> {
        let mut udt_cells = Vec::new();
        let mut spore_clusters = Vec::new();
        let mut spore_cells = Vec::new();
        let mut mnft_issuers = Vec::new();
        let mut mnft_classes = Vec::new();
        let mut mnft_tokens = Vec::new();

        for (tx_index, tx) in block.transactions.iter().enumerate() {
            if tx_index == 0 {
                continue;
            }

            let tx_hash = parse_hex_bytes32(&tx.hash)?;

            for (output_index, output) in tx.outputs.iter().enumerate() {
                let data_hex = tx
                    .outputs_data
                    .get(output_index)
                    .map(|s| s.as_str())
                    .unwrap_or("0x");

                if let Some(udt) = UdtParser::parse_udt_cell(output, data_hex) {
                    let lock_script_hash = to_bytes32(&udt.lock_script_hash);
                    let type_script_hash = to_bytes32(&udt.type_script_hash);
                    let type_code_hash = to_bytes32(&udt.type_code_hash);

                    udt_cells.push(UdtCellRow::new_live(
                        tx_hash,
                        output_index as u16,
                        canon_version,
                        type_script_hash,
                        type_code_hash,
                        udt.type_hash_type as u8,
                        format!("0x{}", hex::encode(&udt.type_args)),
                        lock_script_hash,
                        udt.amount,
                        udt.standard.as_str(),
                        block_number,
                    ));
                }

                if let Some(cluster) = SporeParser::parse_cluster_cell(output, data_hex) {
                    let cluster_id = to_bytes32(&cluster.cluster_id);
                    let type_script_hash = to_bytes32(&cluster.type_script_hash);
                    let owner_lock_hash = to_bytes32(&cluster.owner_lock_hash);

                    spore_clusters.push(SporeClusterRow::new(
                        cluster_id,
                        canon_version,
                        type_script_hash,
                        cluster.name.unwrap_or_default(),
                        cluster.description.unwrap_or_default(),
                        owner_lock_hash,
                        block_number,
                        tx_hash,
                        timestamp_ms,
                    ));
                }

                if let Some(spore) = SporeParser::parse_spore_cell(output, data_hex) {
                    let spore_id = to_bytes32(&spore.spore_id);
                    let type_script_hash = to_bytes32(&spore.type_script_hash);
                    let owner_lock_hash = to_bytes32(&spore.owner_lock_hash);
                    let cluster_id = spore
                        .cluster_id
                        .as_ref()
                        .map(|c| to_bytes32(c))
                        .unwrap_or(EMPTY_HASH);

                    spore_cells.push(SporeCellRow::new_live(
                        spore_id,
                        canon_version,
                        type_script_hash,
                        tx_hash,
                        output_index as u16,
                        cluster_id,
                        spore.content_type,
                        spore.content.len() as u32,
                        owner_lock_hash,
                        block_number,
                        timestamp_ms,
                    ));
                }

                if let Some(issuer) = MnftParser::parse_issuer_cell(output, data_hex) {
                    let mut issuer_id = [0u8; 20];
                    issuer_id.copy_from_slice(&issuer.issuer_id[..20.min(issuer.issuer_id.len())]);
                    let type_script_hash = to_bytes32(&issuer.type_script_hash);
                    let owner_lock_hash = to_bytes32(&issuer.owner_lock_hash);

                    mnft_issuers.push(MnftIssuerRow {
                        issuer_id,
                        canon_version,
                        type_script_hash,
                        tx_hash,
                        output_index: output_index as u16,
                        name: issuer.name.unwrap_or_default(),
                        info: issuer.info.map(|b| hex::encode(&b)).unwrap_or_default(),
                        class_count: issuer.class_count,
                        set_count: issuer.set_count,
                        owner_lock_hash,
                        is_live: 1,
                        created_at_block: block_number,
                        created_at_tx: tx_hash,
                        consumed_at_block: 0,
                        consumed_by_tx: EMPTY_HASH,
                        created_at: timestamp_ms,
                        updated_at: timestamp_ms,
                    });
                }

                if let Some(class) = MnftParser::parse_class_cell(output, data_hex) {
                    let mut issuer_id = [0u8; 20];
                    issuer_id.copy_from_slice(&class.issuer_id[..20.min(class.issuer_id.len())]);
                    let type_script_hash = to_bytes32(&class.type_script_hash);
                    let owner_lock_hash = to_bytes32(&class.owner_lock_hash);

                    mnft_classes.push(MnftClassRow {
                        class_id: format!("0x{}", hex::encode(&class.class_id)),
                        canon_version,
                        type_script_hash,
                        issuer_id,
                        name: class.name.unwrap_or_default(),
                        description: class.description.unwrap_or_default(),
                        renderer: class.renderer.unwrap_or_default(),
                        total: class.total,
                        issued: class.issued,
                        holders_count: 0,
                        transfers_count: 0,
                        transfers_24h: 0,
                        owner_lock_hash,
                        is_live: 1,
                        created_at_block: block_number,
                        created_at_tx: tx_hash,
                        consumed_at_block: 0,
                        consumed_by_tx: EMPTY_HASH,
                        created_at: timestamp_ms,
                        updated_at: timestamp_ms,
                    });
                }

                if let Some(token) = MnftParser::parse_token_cell(output, data_hex) {
                    let type_script_hash = to_bytes32(&token.type_script_hash);
                    let owner_lock_hash = to_bytes32(&token.owner_lock_hash);

                    mnft_tokens.push(MnftTokenRow {
                        token_id: format!("0x{}", hex::encode(&token.token_id)),
                        canon_version,
                        type_script_hash,
                        tx_hash,
                        output_index: output_index as u16,
                        class_id: format!("0x{}", hex::encode(&token.class_id)),
                        token_index: token.token_index,
                        characteristic: hex::encode(&token.characteristic),
                        configure: token.configure,
                        state: token.state,
                        owner_lock_hash,
                        is_live: 1,
                        created_at_block: block_number,
                        created_at_tx: tx_hash,
                        consumed_at_block: 0,
                        consumed_by_tx: EMPTY_HASH,
                        created_at: timestamp_ms,
                        updated_at: timestamp_ms,
                    });
                }
            }
        }

        Ok((
            udt_cells,
            spore_clusters,
            spore_cells,
            mnft_issuers,
            mnft_classes,
            mnft_tokens,
        ))
    }
}

fn parse_hex_u64(hex: &str) -> Result<u64> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    u64::from_str_radix(hex, 16).context("Invalid hex u64")
}

fn parse_hex_u32(hex: &str) -> Result<u32> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    u32::from_str_radix(hex, 16).context("Invalid hex u32")
}

fn parse_hex_bytes(hex: &str) -> Result<Vec<u8>> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    if hex.is_empty() {
        return Ok(Vec::new());
    }
    hex::decode(hex).context("Invalid hex bytes")
}

fn parse_hex_bytes32(hex: &str) -> Result<[u8; 32]> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    let bytes = hex::decode(hex).context("Invalid hex bytes32")?;
    if bytes.len() != 32 {
        anyhow::bail!("Expected 32 bytes, got {}", bytes.len());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn parse_hex_bytes32_with_default(hex: &str, default: [u8; 32]) -> [u8; 32] {
    parse_hex_bytes32(hex).unwrap_or(default)
}

fn parse_hex_bytes16(hex: &str) -> Result<[u8; 16]> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    // Left-pad with zeros to 32 hex characters (16 bytes)
    let padded = format!("{:0>32}", hex);
    let bytes = hex::decode(&padded).context("Invalid hex bytes16")?;
    if bytes.len() != 16 {
        anyhow::bail!("Expected 16 bytes, got {}", bytes.len());
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn parse_epoch(epoch_hex: &str) -> Result<(u64, u32, u32)> {
    let epoch = parse_hex_u64(epoch_hex)?;
    let length = ((epoch >> 40) & 0xFFFF) as u32;
    let index = ((epoch >> 24) & 0xFFFF) as u32;
    let number = epoch & 0xFFFFFF;
    Ok((number, index, length))
}

fn parse_hash_type(hash_type: &str) -> u8 {
    match hash_type {
        "data" => 0,
        "type" => 1,
        "data1" => 2,
        "data2" => 4,
        _ => 0,
    }
}

fn compute_script_hash(script: &Script) -> Result<[u8; 32]> {
    let code_hash = parse_hex_bytes(&script.code_hash)?;
    let hash_type = parse_hash_type(&script.hash_type);
    let args = parse_hex_bytes(&script.args)?;

    let mut serialized = Vec::with_capacity(4 + 4 + 4 + 4 + code_hash.len() + 1 + args.len());

    let total_size = 4 + 4 + 4 + 4 + code_hash.len() + 1 + args.len();
    serialized.extend_from_slice(&(total_size as u32).to_le_bytes());
    serialized.extend_from_slice(&(16u32).to_le_bytes());
    serialized.extend_from_slice(&((16 + code_hash.len()) as u32).to_le_bytes());
    serialized.extend_from_slice(&((16 + code_hash.len() + 1) as u32).to_le_bytes());
    serialized.extend_from_slice(&code_hash);
    serialized.push(hash_type);
    serialized.extend_from_slice(&args);

    Ok(blake2b_hash(&serialized))
}

fn blake2b_hash(data: &[u8]) -> [u8; 32] {
    use ckb_hash::new_blake2b;
    let mut hasher = new_blake2b();
    hasher.update(data);
    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);
    hash
}

fn to_bytes32(slice: &[u8]) -> [u8; 32] {
    let mut arr = [0u8; 32];
    let len = slice.len().min(32);
    arr[..len].copy_from_slice(&slice[..len]);
    arr
}

fn truncate_data_preview(data: &str, max_bytes: usize) -> String {
    let hex = data.strip_prefix("0x").unwrap_or(data);
    let max_hex_chars = max_bytes * 2;
    if hex.len() <= max_hex_chars {
        data.to_string()
    } else {
        format!("0x{}", &hex[..max_hex_chars])
    }
}

fn generate_activity_id(tx_hash: &[u8; 32], activity_type: &str, activity_index: u16) -> [u8; 32] {
    use ckb_hash::new_blake2b;
    let mut hasher = new_blake2b();
    hasher.update(tx_hash);
    hasher.update(activity_type.as_bytes());
    hasher.update(&activity_index.to_le_bytes());
    let mut result = [0u8; 32];
    hasher.finalize(&mut result);
    result
}
