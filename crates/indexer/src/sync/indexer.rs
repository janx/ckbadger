#![allow(dead_code)]

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::cache::{CacheInvalidator, CellInfoCache};
use crate::config::Config;
use crate::db::writer::{
    BatchWriter, BlockRow, CellInputRow, CellOutputRow, CellStateRow, TransactionRow,
};
use crate::db::{ClickHouseClient, LiveCellInfo, MemoryStats};
use crate::rpc::{BlockView, Script, TransactionView};
use crate::state::CanonVersionManager;

use super::SyncProgress;

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct MaxVersionRow {
    max_version: u64,
}

pub struct Indexer {
    client: ClickHouseClient,
    batch_writer: BatchWriter,
    canon_version_mgr: CanonVersionManager,
    cell_cache: CellInfoCache,
    progress: Arc<SyncProgress>,
    cache_invalidator: CacheInvalidator,
    memory_stats: MemoryStats,
    fast_sync_mode: bool,
}

pub struct IndexerConfig {
    pub clickhouse_url: String,
    pub clickhouse_database: String,
    pub cell_cache_capacity: usize,
    pub fast_sync_mode: bool,
    pub redis_url: Option<String>,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            clickhouse_url: "http://localhost:8123".to_string(),
            clickhouse_database: "ckbadger".to_string(),
            cell_cache_capacity: 1_000_000,
            fast_sync_mode: true,
            redis_url: None,
        }
    }
}

impl Indexer {
    pub async fn new(config: IndexerConfig) -> Result<Self> {
        let ch_config = crate::db::ClickHouseConfig::new(&config.clickhouse_url, &config.clickhouse_database);
        let client = ClickHouseClient::new(ch_config);
        
        client.ping().await.context("Failed to connect to ClickHouse")?;
        
        let max_version = Self::fetch_max_canon_version(&client).await?;
        let canon_version_mgr = CanonVersionManager::recover_from_db(max_version);
        
        let batch_writer = BatchWriter::with_fast_sync_mode(client.clone(), config.fast_sync_mode);
        let cell_cache = CellInfoCache::new(config.cell_cache_capacity);
        let cache_invalidator = CacheInvalidator::new(config.redis_url.as_deref()).await;
        
        Ok(Self {
            client,
            batch_writer,
            canon_version_mgr,
            cell_cache,
            progress: Arc::new(SyncProgress::new(0, 0)),
            cache_invalidator,
            memory_stats: MemoryStats::default(),
            fast_sync_mode: config.fast_sync_mode,
        })
    }

    pub async fn from_legacy_config(config: Config) -> Result<Self> {
        let cache_invalidator = CacheInvalidator::new(config.redis_url.as_deref()).await;
        
        let ch_config = crate::db::ClickHouseConfig::from_env()?;
        let client = ClickHouseClient::new(ch_config);
        
        client.ping().await.context("Failed to connect to ClickHouse")?;
        
        let max_version = Self::fetch_max_canon_version(&client).await?;
        let canon_version_mgr = CanonVersionManager::recover_from_db(max_version);
        
        let batch_writer = BatchWriter::with_fast_sync_mode(client.clone(), config.fast_sync_mode);
        let cell_cache = CellInfoCache::new(1_000_000);
        
        Ok(Self {
            client,
            batch_writer,
            canon_version_mgr,
            cell_cache,
            progress: Arc::new(SyncProgress::new(0, 0)),
            cache_invalidator,
            memory_stats: MemoryStats::default(),
            fast_sync_mode: config.fast_sync_mode,
        })
    }

    async fn fetch_max_canon_version(client: &ClickHouseClient) -> Result<Option<u64>> {
        let rows: Vec<MaxVersionRow> = client
            .query_all("SELECT max(canon_version) as max_version FROM canonical_blocks")
            .await?;
        Ok(rows.first().and_then(|r| if r.max_version == 0 { None } else { Some(r.max_version) }))
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
        false
    }

    pub async fn run(&self) -> Result<()> {
        Ok(())
    }

    pub async fn connect_block(&self, block: &BlockView) -> Result<()> {
        let canon_version = self.canon_version_mgr.next();
        let block_number = parse_hex_u64(&block.header.number)?;
        let block_hash = parse_hex_bytes32(&block.header.hash)?;
        let block_timestamp_ms = parse_hex_u64(&block.header.timestamp)? as i64;

        let block_row = self.extract_block_row(block, block_number, &block_hash, block_timestamp_ms)?;
        
        let mut transaction_rows = Vec::with_capacity(block.transactions.len());
        let mut cell_output_rows = Vec::new();
        let mut cell_input_rows = Vec::new();
        let mut cell_state_rows = Vec::new();

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
                let data = tx.outputs_data.get(output_index).map(|s| s.as_str()).unwrap_or("0x");
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
                self.cell_cache.insert(tx_hash.to_vec(), output_index as i16, cell_info);
            }

            if !is_cellbase {
                for (input_index, input) in tx.inputs.iter().enumerate() {
                    let prev_tx_hash = parse_hex_bytes32(&input.previous_output.tx_hash)?;
                    let prev_output_index = parse_hex_u32(&input.previous_output.index)? as u16;
                    
                    if let Some(cell_info) = self.cell_cache.get(&prev_tx_hash, prev_output_index as i16) {
                        let type_script_hash = cell_info.type_script_hash
                            .as_ref()
                            .map(|v| to_bytes32(v))
                            .unwrap_or([0u8; 32]);
                        let type_code_hash = cell_info.type_code_hash
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

        self.batch_writer.write_blocks(&[block_row]).await?;
        self.batch_writer.write_transactions(&transaction_rows).await?;
        self.batch_writer.write_cell_outputs(&cell_output_rows).await?;
        self.batch_writer.write_cell_inputs(&cell_input_rows).await?;
        self.batch_writer.write_cell_states(&cell_state_rows).await?;

        self.batch_writer
            .write_canonical_blocks(&[(block_number, block_hash.to_vec(), canon_version)])
            .await?;

        self.progress.update_current(block_number);

        Ok(())
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
            total_difficulty: [0u8; 32],
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
            
            let data = tx.outputs_data.get(output_index).map(|s| s.as_str()).unwrap_or("0x");
            let data_bytes = parse_hex_bytes(data)?;
            let data_hash = blake2b_hash(&data_bytes);
            
            let lock_script_hash = compute_script_hash(&output.lock)?;
            let (type_code_hash, type_hash_type, type_args, type_script_hash) = match &output.type_ {
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
    let bytes = hex::decode(hex).context("Invalid hex bytes16")?;
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
