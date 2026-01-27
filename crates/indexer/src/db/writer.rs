use anyhow::Result;
#[allow(unused_imports)]
use chrono::{DateTime, NaiveDate, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use std::collections::HashMap;

use super::clickhouse::ClickHouseClient;
#[allow(unused_imports)]
use crate::parser::{
    block::ParsedBlock,
    cell::ParsedCell,
    transaction::{ParsedCellDep, ParsedInput},
    ParsedClusterCell, ParsedDaoDeposit, ParsedDaoWithdrawRequest, ParsedSporeCell,
    ParsedUdtTransfer,
};

/// Secondary issuance breakdown for DAO statistics
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct SecondaryIssuanceBreakdown {
    pub secondary_issuance: i64,
    pub miner_secondary: i64,
    pub dao_compensation: i64,
    pub burnt: i64,
}

/// Result of a blockchain reorganization operation
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ReorgResult {
    pub blocks_deleted: u64,
    pub transactions_deleted: u64,
    pub cells_deleted: u64,
}

/// Type alias for compatibility with sync module
#[allow(dead_code)]
pub type BatchWriter = ClickHouseWriter;

/// Extract accumulated rate (AR) from DAO field (bytes 8-15)
#[allow(dead_code)]
fn extract_ar_from_dao(dao: &[u8]) -> Option<u64> {
    if dao.len() < 16 {
        return None;
    }
    let bytes: [u8; 8] = dao[8..16].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// Extract total issuance from DAO field (bytes 0-7)
#[allow(dead_code)]
fn extract_total_issuance_from_dao(dao: &[u8]) -> Option<u64> {
    if dao.len() < 8 {
        return None;
    }
    let bytes: [u8; 8] = dao[0..8].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// Check if cell data looks like a dep group (4-byte count + N × 36-byte OutPoints)
#[allow(dead_code)]
fn looks_like_dep_group(data: &[u8]) -> bool {
    let size = data.len();
    if !(40..=10000).contains(&size) || !(size - 4).is_multiple_of(36) {
        return false;
    }
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap_or([0; 4])) as usize;
    count > 0 && count <= 256 && count == (size - 4) / 36
}

fn decode_sync_tip_hash(hash: &str) -> Option<Vec<u8>> {
    hex::decode(hash).ok()
}

/// Convert Vec<u8> to [u8; 32] for FixedString(32) columns
fn vec_to_hash32(v: &[u8]) -> [u8; 32] {
    v.try_into().expect("hash must be 32 bytes")
}

/// Convert Option<Vec<u8>> to Option<[u8; 32]> for nullable FixedString(32) columns
fn opt_vec_to_hash32(v: Option<&[u8]>) -> Option<[u8; 32]> {
    v.map(|bytes| bytes.try_into().expect("hash must be 32 bytes"))
}

/// Convert Vec<u8> to [u8; 16] for FixedString(16) columns (nonce)
fn vec_to_hash16(v: &[u8]) -> [u8; 16] {
    v.try_into().expect("nonce must be 16 bytes")
}

/// Convert Vec<u8> to [u8; 10] for FixedString(10) columns (ProposalShortId)
fn vec_to_proposal_id(v: &[u8]) -> [u8; 10] {
    v.try_into().expect("proposal_id must be 10 bytes")
}

#[allow(dead_code)]
const DAO_OCCUPIED_CAPACITY: u64 = 102_00000000;

/// ClickHouse batch writer for core blockchain tables.
///
/// Provides high-performance batch insert operations for blocks, transactions,
/// cells, cell consumptions, and live cells. All hash fields use binary
/// serialization (Vec<u8>) for optimal storage and performance.
///
/// # Performance Characteristics
///
/// - Target throughput: 500K+ rows/s sustained
/// - Batch size: 100K rows optimal (from Phase 0 benchmarks)
/// - Binary hash serialization: 9.8x faster than hex strings
///
/// # Example
///
/// ```no_run
/// use ckbadger_indexer::db::{ClickHouseClient, ClickHouseWriter};
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = ClickHouseClient::new("http://localhost:8123/ckbadger")?;
/// let writer = ClickHouseWriter::new(client);
///
/// // Insert blocks batch
/// let blocks = vec![/* BlockRow instances */];
/// writer.insert_blocks_batch(blocks).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct ClickHouseWriter {
    client: ClickHouseClient,
}

impl ClickHouseWriter {
    /// Create a new ClickHouse writer with the given client.
    pub fn new(client: ClickHouseClient) -> Self {
        Self { client }
    }

    /// Get reference to the underlying ClickHouse client
    pub fn client(&self) -> &ClickHouseClient {
        &self.client
    }

    /// Get reference to the underlying ClickHouse client (alias for compatibility)
    pub fn pool(&self) -> &ClickHouseClient {
        &self.client
    }

    /// Get current sync tip (block number and hash)
    pub async fn get_sync_tip(&self) -> Result<(i64, Option<Vec<u8>>)> {
        #[derive(Row, Deserialize)]
        struct SyncTipRow {
            tip_block_number: i64,
            tip_block_hash: String,
        }

        let row = self
            .client
            .client()
            .query(
                "SELECT tip_block_number, hex(tip_block_hash) as tip_block_hash FROM sync_status WHERE id = 1",
            )
            .fetch_optional::<SyncTipRow>()
            .await?;

        match row {
            Some(r) => Ok((r.tip_block_number, decode_sync_tip_hash(&r.tip_block_hash))),
            None => Ok((0, None)),
        }
    }

    /// Get block hash by height
    pub async fn get_block_hash_at_height(&self, height: i64) -> Result<Option<Vec<u8>>> {
        #[derive(Row, Deserialize)]
        struct BlockHashRow {
            hash: String,
        }

        let query = format!(
            "SELECT hex(hash) as hash FROM blocks WHERE number = {}",
            height
        );
        let row = self
            .client
            .client()
            .query(&query)
            .fetch_optional::<BlockHashRow>()
            .await?;

        Ok(row.and_then(|r| decode_sync_tip_hash(&r.hash)))
    }

    /// Check if there's an unresolved deep fork
    pub async fn has_unresolved_deep_fork(&self) -> Result<bool> {
        // Stub for now - deep fork detection is complex
        // TODO: Implement based on PostgreSQL Repository logic if needed
        Ok(false)
    }

    /// Refresh 24h transfer stats for tokens
    pub async fn refresh_token_24h_transfers(&self) -> Result<u64> {
        // TODO: Implement ClickHouse equivalent once token transfer stats are modeled.
        Ok(0)
    }

    /// Update sync status for the latest block
    pub async fn update_sync_status(
        &self,
        block_number: i64,
        _block_hash: &[u8],
        _tx_count: i64,
        _cells_created: i64,
        _cells_consumed: i64,
        _new_addresses: i64,
    ) -> Result<()> {
        // ReplacingMergeTree deduplicates by version column (updated_at)
        // INSERT new row instead of UPDATE - ClickHouse keeps latest version
        let query = format!(
            "INSERT INTO sync_status (id, tip_block_number, updated_at) VALUES (1, {}, now())",
            block_number
        );
        self.client.client().query(&query).execute().await?;
        Ok(())
    }

    /// Execute a chain reorganization rollback
    pub async fn execute_reorg(
        &self,
        _fork_point: i64,
        _fork_hash: &[u8],
        _old_tip: i64,
        _old_tip_hash: &[u8],
        _new_tip: i64,
        _new_tip_hash: &[u8],
    ) -> Result<ReorgResult> {
        // TODO: Implement ClickHouse reorg handling (archive/delete ranges as needed).
        Ok(ReorgResult {
            blocks_deleted: 0,
            transactions_deleted: 0,
            cells_deleted: 0,
        })
    }

    /// Insert a batch of blocks into the blocks table.
    ///
    /// # Arguments
    ///
    /// * `blocks` - Slice of ParsedBlock references to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_blocks_batch(&self, blocks: &[&ParsedBlock]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let start = std::time::Instant::now();
        let mut insert = self.client.client().insert("blocks")?;
        for parsed_block in blocks {
            let row = Self::parsed_block_to_row(parsed_block, "0".to_string());
            insert.write(&row).await?;
        }
        insert.end().await?;
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 100 {
            tracing::debug!("insert_blocks_batch: {} rows in {:.1}ms", blocks.len(), elapsed.as_secs_f64() * 1000.0);
        }

        Ok(())
    }

    /// Insert a batch of transactions into the transactions table.
    pub async fn insert_transactions_batch(
        &self,
        txs: &[(
            &[u8],
            i64,
            i32,
            i32,
            i16,
            i16,
            i16,
            i16,
            i16,
            i64,
            i64,
            i64,
            Option<i32>,
            Option<i64>,
            bool,
            DateTime<Utc>,
        )],
    ) -> Result<()> {
        if txs.is_empty() {
            return Ok(());
        }

        let start = std::time::Instant::now();
        let mut insert = self.client.client().insert("transactions")?;
        for (
            hash,
            block_number,
            tx_index,
            version,
            inputs_count,
            outputs_count,
            witnesses_count,
            cell_deps_count,
            header_deps_count,
            total_input_capacity,
            total_output_capacity,
            fee,
            tx_size,
            cycles,
            is_cellbase,
            timestamp,
        ) in txs
        {
            let row = TransactionRow {
                hash: vec_to_hash32(hash),
                block_number: *block_number as u64,
                tx_index: *tx_index as u32,
                timestamp: timestamp.timestamp() as u32,
                version: *version as u32,
                inputs_count: *inputs_count as u16,
                outputs_count: *outputs_count as u16,
                witnesses_count: *witnesses_count as u16,
                cell_deps_count: *cell_deps_count as u16,
                header_deps_count: *header_deps_count as u16,
                total_input_capacity: *total_input_capacity as u64,
                total_output_capacity: *total_output_capacity as u64,
                fee: *fee as u64,
                is_cellbase: if *is_cellbase { 1 } else { 0 },
                tx_size: tx_size.map(|s| s as u32),
                cycles: cycles.map(|c| c as u64),
            };
            insert.write(&row).await?;
        }
        insert.end().await?;
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 100 {
            tracing::debug!("insert_transactions_batch: {} rows in {:.1}ms", txs.len(), elapsed.as_secs_f64() * 1000.0);
        }

        Ok(())
    }

    /// Insert a batch of cells into the cells table.
    pub async fn insert_cells_batch(&self, cells: &[(&[u8], i16, &ParsedCell, i64)]) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        let start = std::time::Instant::now();
        let mut insert = self.client.client().insert("cells")?;
        for (tx_hash, output_index, cell, created_at_block) in cells {
            let row = CellRow {
                tx_hash: vec_to_hash32(tx_hash),
                output_index: *output_index as u16,
                created_at_block: *created_at_block as u64,
                capacity: cell.capacity as u64,
                lock_code_hash: vec_to_hash32(&cell.lock_code_hash),
                lock_hash_type: cell.lock_hash_type as u8,
                lock_args: hex::encode(&cell.lock_args),
                lock_script_hash: vec_to_hash32(&cell.lock_script_hash),
                type_code_hash: opt_vec_to_hash32(cell.type_code_hash.as_deref()),
                type_hash_type: cell.type_hash_type.map(|t| t as u8),
                type_args: cell.type_args.as_ref().map(hex::encode),
                type_script_hash: opt_vec_to_hash32(cell.type_script_hash.as_deref()),
                data_hash: vec_to_hash32(&cell.data_hash),
                data_size: cell.data_size as u32,
                data: if cell.data.is_empty() {
                    None
                } else {
                    Some(hex::encode(&cell.data[..cell.data.len().min(512)]))
                },
            };
            insert.write(&row).await?;
        }
        insert.end().await?;
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 100 {
            tracing::debug!("insert_cells_batch: {} rows in {:.1}ms", cells.len(), elapsed.as_secs_f64() * 1000.0);
        }

        Ok(())
    }

    /// Insert a batch of cell consumptions into the cell_consumptions table.
    ///
    /// # Arguments
    ///
    /// * `consumptions` - Vector of CellConsumptionRow instances to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_cell_consumptions_batch(
        &self,
        consumptions: Vec<CellConsumptionRow>,
    ) -> Result<()> {
        if consumptions.is_empty() {
            return Ok(());
        }

        let start = std::time::Instant::now();
        let len = consumptions.len();
        let mut insert = self.client.client().insert("cell_consumptions")?;
        for consumption in consumptions {
            insert.write(&consumption).await?;
        }
        insert.end().await?;
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 100 {
            tracing::debug!("insert_cell_consumptions_batch: {} rows in {:.1}ms", len, elapsed.as_secs_f64() * 1000.0);
        }

        Ok(())
    }

    /// Insert a batch of live cells into the live_cells table.
    ///
    /// # Arguments
    ///
    /// * `live_cells` - Vector of LiveCellRow instances to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_live_cells_batch(&self, live_cells: Vec<LiveCellRow>) -> Result<()> {
        if live_cells.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("live_cells")?;
        for live_cell in live_cells {
            insert.write(&live_cell).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Insert a batch of DAO deposits into the dao_deposits table.
    ///
    /// # Arguments
    ///
    /// * `deposits` - Vector of DaoDepositRow instances to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_dao_deposits_batch(&self, deposits: Vec<DaoDepositRow>) -> Result<()> {
        if deposits.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("dao_deposits")?;
        for deposit in deposits {
            insert.write(&deposit).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Insert a batch of DAO withdrawals into the dao_withdrawals table.
    ///
    /// # Arguments
    ///
    /// * `withdrawals` - Vector of DaoWithdrawalRow instances to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_dao_withdrawals_batch(
        &self,
        withdrawals: Vec<DaoWithdrawalRow>,
    ) -> Result<()> {
        if withdrawals.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("dao_withdrawals")?;
        for withdrawal in withdrawals {
            insert.write(&withdrawal).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Insert a batch of token transfers into the token_transfers table.
    ///
    /// # Arguments
    ///
    /// * `transfers` - Vector of TokenTransferRow instances to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_token_transfers_batch(
        &self,
        transfers: Vec<TokenTransferRow>,
    ) -> Result<()> {
        if transfers.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("token_transfers")?;
        for transfer in transfers {
            insert.write(&transfer).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Insert a batch of Spore cells into the spore_cells table.
    ///
    /// # Arguments
    ///
    /// * `spores` - Vector of SporeCellRow instances to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_spore_cells_batch(&self, spores: Vec<SporeCellRow>) -> Result<()> {
        if spores.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("spore_cells")?;
        for spore in spores {
            insert.write(&spore).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Insert a batch of Spore transfers into the spore_transfers table.
    ///
    /// # Arguments
    ///
    /// * `transfers` - Vector of SporeTransferRow instances to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_spore_transfers_batch(
        &self,
        transfers: Vec<SporeTransferRow>,
    ) -> Result<()> {
        if transfers.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("spore_transfers")?;
        for transfer in transfers {
            insert.write(&transfer).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Convert ParsedBlock to BlockRow for ClickHouse insertion
    fn parsed_block_to_row(parsed: &ParsedBlock, total_difficulty: String) -> BlockRow {
        BlockRow {
            number: parsed.number as u64,
            hash: vec_to_hash32(&parsed.hash),
            parent_hash: vec_to_hash32(&parsed.parent_hash),
            timestamp: parsed.timestamp.timestamp() as u32,
            version: parsed.version as u32,
            compact_target: parsed.compact_target as u64,
            nonce: vec_to_hash16(&parsed.nonce),
            transactions_root: vec_to_hash32(&parsed.transactions_root),
            proposals_hash: vec_to_hash32(&parsed.proposals_hash),
            extra_hash: vec_to_hash32(&parsed.extra_hash),
            uncles_hash: vec_to_hash32(&parsed.uncles_hash),
            epoch_number: parsed.epoch_number as u64,
            epoch_index: parsed.epoch_index as u32,
            epoch_length: parsed.epoch_length as u32,
            dao: vec_to_hash32(&parsed.dao),
            transactions_count: parsed.transactions_count as u32,
            proposals_count: parsed.proposals_count as u32,
            uncles_count: parsed.uncles_count as u32,
            extension: None,
            miner_lock_hash: None,
            miner_message: None,
            total_difficulty,
        }
    }

    /// Insert a single block into the blocks table
    pub async fn insert_block(&self, block: &ParsedBlock, _total_difficulty: i64) -> Result<()> {
        self.insert_blocks_batch(&[block]).await
    }

    /// Initialize sync status at start block
    pub async fn init_sync_start(&self, start_block: i64) -> Result<()> {
        let query = format!(
            "INSERT INTO sync_status (id, tip_block_number, updated_at) VALUES (1, {}, now())",
            start_block
        );
        self.client.client().query(&query).execute().await?;
        Ok(())
    }

    /// Consume cells batch (mark cells as consumed)
    pub async fn consume_cells_batch(
        &self,
        consumptions: &[(&[u8], i16, i64, &[u8], i64, i16)],
    ) -> Result<()> {
        if consumptions.is_empty() {
            return Ok(());
        }

        let rows: Vec<CellConsumptionRow> = consumptions
            .iter()
            .map(
                |(
                    tx_hash,
                    output_index,
                    _created_at_block,
                    consumed_by_tx,
                    consumed_at_block,
                    consumed_at_index,
                )| {
                    CellConsumptionRow {
                        tx_hash: vec_to_hash32(tx_hash),
                        output_index: *output_index as u16,
                        consumed_at_block: *consumed_at_block as u64,
                        consumed_by_tx: vec_to_hash32(consumed_by_tx),
                        consumed_at_index: *consumed_at_index as u16,
                    }
                },
            )
            .collect();

        self.insert_cell_consumptions_batch(rows).await?;

        // Also update live_cells with sign=-1
        let live_cell_updates: Vec<LiveCellRow> = consumptions
            .iter()
            .map(
                |(
                    tx_hash,
                    output_index,
                    _created_at_block,
                    _consumed_by_tx,
                    consumed_at_block,
                    _consumed_at_index,
                )| LiveCellRow {
                    tx_hash: vec_to_hash32(tx_hash),
                    output_index: *output_index as u16,
                    capacity: 0,
                    lock_script_hash: [0u8; 32],
                    type_script_hash: None,
                    created_at_block: 0,
                    sign: -1,
                    version: *consumed_at_block as u64,
                },
            )
            .collect();

        self.insert_live_cells_batch(live_cell_updates).await?;
        Ok(())
    }

    /// Get cell information for a batch of outpoints
    pub async fn get_cells_info_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        let start = std::time::Instant::now();

        // Use tuple IN clause for better ClickHouse performance
        let tuples: Vec<String> = outpoints
            .iter()
            .map(|(tx_hash, idx)| {
                format!("(unhex('{}'), {})", hex::encode(tx_hash), idx)
            })
            .collect();

        let query = format!(
            "SELECT hex(tx_hash) as tx_hash, output_index, capacity, created_at_block, hex(lock_script_hash) as lock_script_hash, data_size 
             FROM cells 
             WHERE (tx_hash, output_index) IN ({})",
            tuples.join(", ")
        );

        #[derive(Row, serde::Deserialize)]
        struct CellInfoRow {
            tx_hash: String,
            output_index: u16,
            capacity: u64,
            created_at_block: u64,
            lock_script_hash: String,
            data_size: u32,
        }

        let rows = self
            .client
            .client()
            .query(&query)
            .fetch_all::<CellInfoRow>()
            .await?;

        let elapsed = start.elapsed();
        if elapsed.as_millis() > 100 {
            tracing::debug!("get_cells_info_batch: {} outpoints, {} rows in {:.1}ms", outpoints.len(), rows.len(), elapsed.as_secs_f64() * 1000.0);
        }

        let mut result = HashMap::new();
        for row in rows {
            let tx_hash = hex::decode(&row.tx_hash).unwrap_or_default();
            let lock_hash = hex::decode(&row.lock_script_hash).unwrap_or_default();
            result.insert(
                (tx_hash, row.output_index as i16),
                (
                    row.capacity as i64,
                    row.created_at_block as i64,
                    lock_hash,
                    row.data_size as i32,
                ),
            );
        }

        Ok(result)
    }

    /// Get code hashes for a batch of cells
    pub async fn get_cells_code_hashes_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (Vec<u8>, Option<Vec<u8>>)>> {
        if outpoints.is_empty() {
            return Ok(HashMap::new());
        }

        // Use tuple IN clause for efficient batch lookup (vs OR conditions)
        let tuples: Vec<String> = outpoints
            .iter()
            .map(|(tx_hash, idx)| format!("(unhex('{}'), {})", hex::encode(tx_hash), idx))
            .collect();

        let query = format!(
            "SELECT hex(tx_hash) as tx_hash, output_index, hex(lock_code_hash) as lock_code_hash, hex(type_code_hash) as type_code_hash 
             FROM cells 
             WHERE (tx_hash, output_index) IN ({})",
            tuples.join(", ")
        );

        #[derive(Row, serde::Deserialize)]
        struct CodeHashRow {
            tx_hash: String,
            output_index: u16,
            lock_code_hash: String,
            type_code_hash: Option<String>,
        }

        let rows = self
            .client
            .client()
            .query(&query)
            .fetch_all::<CodeHashRow>()
            .await?;

        let mut result = HashMap::new();
        for row in rows {
            let tx_hash = hex::decode(&row.tx_hash).unwrap_or_default();
            let lock_code_hash = hex::decode(&row.lock_code_hash).unwrap_or_default();
            let type_code_hash = row.type_code_hash.and_then(|h| hex::decode(&h).ok());
            result.insert(
                (tx_hash, row.output_index as i16),
                (lock_code_hash, type_code_hash),
            );
        }

        Ok(result)
    }

    /// Get UDT cell information for a batch of outpoints
    pub async fn get_udt_cells_info_batch(
        &self,
        _outpoints: &[(&[u8], i16)],
    ) -> Result<HashMap<(Vec<u8>, i16), (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String)>>
    {
        Ok(HashMap::new())
    }

    /// Update DAO daily snapshot for a specific date
    pub async fn update_dao_daily_snapshot(&self, date: NaiveDate) -> Result<()> {
        // Stub implementation - DAO snapshot logic is complex
        // For now, just log that it was called
        tracing::info!("update_dao_daily_snapshot called for date: {}", date);
        Ok(())
    }

    /// Get secondary issuance state
    pub async fn get_secondary_issuance_state(&self) -> Result<(u128, u128, u128, u128, i64)> {
        // Stub implementation - return zeros for now
        Ok((0, 0, 0, 0, 0))
    }

    /// Get DAO deposits at a specific block
    pub async fn get_dao_deposits_at_block(&self, block_number: i64) -> Result<u128> {
        let query = format!(
            "SELECT SUM(capacity) as total FROM dao_deposits WHERE deposit_block <= {}",
            block_number
        );

        #[derive(Row, serde::Deserialize)]
        struct TotalRow {
            total: u64,
        }

        let row = self
            .client
            .client()
            .query(&query)
            .fetch_optional::<TotalRow>()
            .await?;

        Ok(row.map(|r| r.total as u128).unwrap_or(0))
    }

    /// Insert block proposals batch
    pub async fn insert_block_proposals_batch(
        &self,
        block_number: i64,
        proposals: &[Vec<u8>],
    ) -> Result<()> {
        if proposals.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("block_proposals")?;
        for (idx, proposal_hash) in proposals.iter().enumerate() {
            let row = BlockProposalRow {
                block_number: block_number as u64,
                proposal_index: idx as u16,
                proposal_hash: vec_to_proposal_id(proposal_hash),
            };
            insert.write(&row).await?;
        }
        insert.end().await?;
        Ok(())
    }

    /// Insert transaction inputs batch
    pub async fn insert_transaction_inputs_batch(
        &self,
        inputs: &[(&[u8], i64, i16, &ParsedInput)],
    ) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("transaction_inputs")?;
        for (tx_hash, block_number, input_index, input) in inputs {
            let row = TransactionInputRow {
                tx_hash: vec_to_hash32(tx_hash),
                tx_block_number: *block_number as u64,
                input_index: *input_index as u16,
                previous_tx_hash: vec_to_hash32(&input.previous_tx_hash),
                previous_output_index: input.previous_output_index as u16,
                since: input.since as u64,
            };
            insert.write(&row).await?;
        }
        insert.end().await?;
        Ok(())
    }

    /// Insert transaction cell deps batch
    pub async fn insert_transaction_cell_deps_batch(
        &self,
        cell_deps: &[(&[u8], i64, i16, &ParsedCellDep)],
    ) -> Result<()> {
        if cell_deps.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("transaction_cell_deps")?;
        for (tx_hash, block_number, dep_index, cell_dep) in cell_deps {
            let row = TransactionCellDepRow {
                tx_hash: vec_to_hash32(tx_hash),
                tx_block_number: *block_number as u64,
                dep_index: *dep_index as u16,
                dep_tx_hash: vec_to_hash32(&cell_dep.out_point_tx_hash),
                dep_output_index: cell_dep.out_point_index as u16,
                dep_type: if cell_dep.dep_type == 0 {
                    "code".to_string()
                } else {
                    "dep_group".to_string()
                },
            };
            insert.write(&row).await?;
        }
        insert.end().await?;
        Ok(())
    }

    /// Update address balances batch
    pub async fn update_address_balances_batch(
        &self,
        changes: &HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])>,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        Ok(())
    }

    /// Insert address transactions batch
    pub async fn insert_address_transactions_batch(
        &self,
        records: &[(Vec<u8>, Vec<u8>, i64, i16, i64, DateTime<Utc>)],
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("address_transactions")?;
        for (lock_hash, tx_hash, block_number, tx_type, balance_change, timestamp) in records {
            let row = AddressTransactionRow {
                lock_hash: vec_to_hash32(lock_hash),
                tx_hash: vec_to_hash32(tx_hash),
                block_number: *block_number as u64,
                tx_type: *tx_type as i8,
                balance_change: *balance_change,
                timestamp: timestamp.timestamp() as u32,
            };
            insert.write(&row).await?;
        }
        insert.end().await?;
        Ok(())
    }

    /// Update script usage batch
    pub async fn update_script_usage_batch(
        &self,
        changes: &HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)>,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        Ok(())
    }

    /// Insert address asset transfers batch (stub)
    pub async fn insert_address_asset_transfers_batch(
        &self,
        _transfers: &[(
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
        )],
    ) -> Result<()> {
        Ok(())
    }

    /// Insert DAO deposit
    pub async fn insert_dao_deposit(
        &self,
        deposit: &ParsedDaoDeposit,
        block_number: i64,
        timestamp: DateTime<Utc>,
        ar: i64,
    ) -> Result<()> {
        let row = DaoDepositRow {
            tx_hash: vec_to_hash32(&deposit.tx_hash),
            output_index: deposit.output_index as u16,
            depositor_lock_hash: vec_to_hash32(&deposit.lock_script_hash),
            capacity: deposit.capacity as u64,
            deposit_block: block_number as u64,
            deposit_timestamp: timestamp.timestamp() as u32,
            deposit_ar: ar as u64,
        };
        self.insert_dao_deposits_batch(vec![row]).await
    }

    /// Find consumed DAO deposits
    pub async fn find_consumed_dao_deposits(
        &self,
        consumed_cells: &[(&[u8], i32)],
    ) -> Result<Vec<(Vec<u8>, i16, i64, i64)>> {
        if consumed_cells.is_empty() {
            return Ok(vec![]);
        }

        let tuples: Vec<String> = consumed_cells
            .iter()
            .map(|(tx_hash, idx)| format!("(unhex('{}'), {})", hex::encode(tx_hash), idx))
            .collect();

        let query = format!(
            "SELECT hex(tx_hash) as tx_hash, output_index, capacity, deposit_ar 
             FROM dao_deposits 
             WHERE (tx_hash, output_index) IN ({})",
            tuples.join(", ")
        );

        #[derive(Row, serde::Deserialize)]
        struct DaoDepositQueryRow {
            tx_hash: String,
            output_index: u16,
            capacity: u64,
            deposit_ar: u64,
        }

        let rows = self
            .client
            .client()
            .query(&query)
            .fetch_all::<DaoDepositQueryRow>()
            .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    hex::decode(&r.tx_hash).unwrap_or_default(),
                    r.output_index as i16,
                    r.capacity as i64,
                    r.deposit_ar as i64,
                )
            })
            .collect())
    }

    /// Process DAO withdrawals
    pub async fn process_dao_withdrawals(
        &self,
        _consumed_dao: &[(Vec<u8>, i16, i64, i64)],
        _new_dao_outputs: &[(Vec<u8>, i16, Vec<u8>, i64, u64)],
        _block_number: i64,
        _tx_hash: &[u8],
        _timestamp: DateTime<Utc>,
    ) -> Result<()> {
        Ok(())
    }

    /// Recalculate DAO extended statistics
    pub async fn recalculate_dao_extended_statistics(&self, _block_number: i64) -> Result<()> {
        Ok(())
    }

    /// Accumulate secondary issuance
    pub async fn accumulate_secondary_issuance(
        &self,
        _breakdown: &SecondaryIssuanceBreakdown,
        _block_number: i64,
        _block_timestamp: DateTime<Utc>,
    ) -> Result<()> {
        Ok(())
    }

    /// Get block DAO field
    pub async fn get_block_dao_field(&self, block_number: i64) -> Result<Option<Vec<u8>>> {
        let query = format!("SELECT dao FROM blocks WHERE number = {}", block_number);

        #[derive(Row, serde::Deserialize)]
        struct DaoRow {
            dao: Vec<u8>,
        }

        let row = self
            .client
            .client()
            .query(&query)
            .fetch_optional::<DaoRow>()
            .await?;

        Ok(row.map(|r| r.dao))
    }

    /// Process UDT transfers batch (stub)
    pub async fn process_udt_transfers_batch(
        &self,
        _transfers: &[(&ParsedUdtTransfer, &[u8], i64, DateTime<Utc>)],
    ) -> Result<()> {
        Ok(())
    }

    /// Insert UDT cells batch (stub - cells tracked in main cells table)
    pub async fn insert_udt_cells_batch(
        &self,
        _cells: &[(&[u8], i16, &crate::parser::ParsedUdtCell, i64)],
    ) -> Result<()> {
        Ok(())
    }

    /// Consume UDT cells batch (stub - tracked in consume_cells_batch)
    pub async fn consume_udt_cells_batch(&self, _cells: &[(&[u8], i16, i64, &[u8])]) -> Result<()> {
        Ok(())
    }

    /// Insert Spore cluster (stub)
    pub async fn insert_spore_cluster(
        &self,
        _cluster: &ParsedClusterCell,
        _block_number: i64,
        _tx_hash: &[u8],
    ) -> Result<()> {
        Ok(())
    }

    /// Insert Spore cell (stub)
    pub async fn insert_spore_cell(
        &self,
        _spore: &ParsedSporeCell,
        _tx_hash: &[u8],
        _output_index: i16,
        _block_number: i64,
    ) -> Result<()> {
        Ok(())
    }

    /// Insert Spore content (stub)
    pub async fn insert_spore_content(&self, _spore_id: &[u8], _content: &[u8]) -> Result<()> {
        Ok(())
    }

    /// Consume Spore (stub)
    pub async fn consume_spore(
        &self,
        _spore_id: &[u8],
        _consumed_at_block: i64,
        _consumed_by_tx: &[u8],
    ) -> Result<()> {
        Ok(())
    }

    /// Get Spore ID by outpoint (stub)
    pub async fn get_spore_id_by_outpoint(
        &self,
        _tx_hash: &[u8],
        _output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Get Spore owner by ID (stub)
    pub async fn get_spore_owner_by_id(&self, _spore_id: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Insert mNFT issuer (stub)
    pub async fn insert_mnft_issuer(
        &self,
        _issuer: &crate::parser::ParsedMnftIssuer,
        _tx_hash: &[u8],
        _output_index: i16,
        _block_number: i64,
    ) -> Result<()> {
        Ok(())
    }

    /// Insert mNFT class (stub)
    pub async fn insert_mnft_class(
        &self,
        _class: &crate::parser::ParsedMnftClass,
        _tx_hash: &[u8],
        _output_index: i16,
        _block_number: i64,
    ) -> Result<()> {
        Ok(())
    }

    /// Insert mNFT token (stub)
    pub async fn insert_mnft_token(
        &self,
        _token: &crate::parser::ParsedMnftToken,
        _tx_hash: &[u8],
        _output_index: i16,
        _block_number: i64,
    ) -> Result<()> {
        Ok(())
    }

    /// Consume mNFT token (stub)
    pub async fn consume_mnft_token(
        &self,
        _token_id: &[u8],
        _consumed_at_block: i64,
        _consumed_by_tx: &[u8],
    ) -> Result<()> {
        Ok(())
    }

    /// Get mNFT token ID by outpoint (stub)
    pub async fn get_mnft_token_id_by_outpoint(
        &self,
        _tx_hash: &[u8],
        _output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Get mNFT token owner by ID (stub)
    pub async fn get_mnft_token_owner_by_id(&self, _token_id: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Insert DotBit account (stub)
    pub async fn insert_dotbit_account(
        &self,
        _account: &crate::parser::ParsedDotbitAccount,
        _tx_hash: &[u8],
        _output_index: i16,
        _block_number: i64,
    ) -> Result<()> {
        Ok(())
    }

    /// Consume DotBit account (stub)
    pub async fn consume_dotbit_account(
        &self,
        _account_id: &[u8],
        _consumed_at_block: i64,
        _consumed_by_tx: &[u8],
    ) -> Result<()> {
        Ok(())
    }

    /// Get DotBit account ID by outpoint (stub)
    pub async fn get_dotbit_account_id_by_outpoint(
        &self,
        _tx_hash: &[u8],
        _output_index: i16,
    ) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Get DotBit owner by ID (stub)
    pub async fn get_dotbit_owner_by_id(&self, _account_id: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Insert DOB transfer (stub)
    pub async fn insert_dob_transfer(
        &self,
        _dob_id: &[u8],
        _cluster_id: Option<&[u8]>,
        _dob_type: &str,
        _tx_hash: &[u8],
        _block_number: i64,
        _from: Option<&[u8]>,
        _to: &[u8],
        _event_type: &str,
        _content_type: Option<&str>,
        _timestamp: DateTime<Utc>,
    ) -> Result<()> {
        Ok(())
    }

    /// Insert NFT transfer (stub)
    pub async fn insert_nft_transfer(
        &self,
        _nft_id: &[u8],
        _nft_type: &str,
        _issuer_id: Option<&[u8]>,
        _class_id: Option<&[u8]>,
        _tx_hash: &[u8],
        _block_number: i64,
        _from: Option<&[u8]>,
        _to: &[u8],
        _event_type: &str,
        _name: Option<&str>,
        _timestamp: DateTime<Utc>,
    ) -> Result<()> {
        Ok(())
    }

    /// Get previous block timestamp (stub)
    pub async fn get_previous_block_timestamp(
        &self,
        _block_number: i64,
    ) -> Result<Option<DateTime<Utc>>> {
        Ok(None)
    }

    /// Get last epoch start (stub)
    pub async fn get_last_epoch_start(
        &self,
        _epoch_number: i64,
    ) -> Result<Option<(i64, DateTime<Utc>)>> {
        Ok(None)
    }

    /// Update daily statistics (stub)
    pub async fn update_daily_statistics(
        &self,
        _date: NaiveDate,
        _blocks: i32,
        _txs: i32,
        _created: i32,
        _consumed: i32,
        _capacity: i64,
        _data_size_added: i64,
        _data_size_consumed: i64,
    ) -> Result<()> {
        Ok(())
    }

    /// Update hourly statistics (stub)
    pub async fn update_hourly_statistics(
        &self,
        _hour: DateTime<Utc>,
        _blocks: i32,
        _txs: i32,
        _created: i32,
        _consumed: i32,
        _capacity: i64,
    ) -> Result<()> {
        Ok(())
    }

    /// Update daily block stats batch (stub)
    pub async fn update_daily_block_stats_batch(
        &self,
        _date: NaiveDate,
        _avg_target: i64,
        _count: i32,
        _uncles: i32,
    ) -> Result<()> {
        Ok(())
    }

    /// Update daily avg block time batch (stub)
    pub async fn update_daily_avg_block_time_batch(
        &self,
        _date: NaiveDate,
        _avg_ms: i64,
        _count: i32,
    ) -> Result<()> {
        Ok(())
    }

    /// Update block time distribution batch (stub)
    pub async fn update_block_time_distribution_batch(
        &self,
        _bucket: i32,
        _count: i32,
    ) -> Result<()> {
        Ok(())
    }

    /// Update epoch time distribution batch (stub)
    pub async fn update_epoch_time_distribution_batch(
        &self,
        _bucket: i32,
        _count: i32,
    ) -> Result<()> {
        Ok(())
    }

    /// Upsert epoch statistics batch (stub)
    pub async fn upsert_epoch_statistics_batch(
        &self,
        _epoch_number: i64,
        _start_block: i64,
        _end_block: i64,
        _length: i32,
        _start_ts: DateTime<Utc>,
        _end_ts: DateTime<Utc>,
        _tx_count: i32,
        _is_new: bool,
    ) -> Result<()> {
        Ok(())
    }

    /// Update miner statistics batch (stub)
    pub async fn update_miner_statistics_batch(
        &self,
        _miner_hash: &[u8],
        _last_block: i64,
        _date: NaiveDate,
        _blocks_count: i32,
    ) -> Result<()> {
        Ok(())
    }

    /// Record deep fork (stub)
    pub async fn record_deep_fork(
        &self,
        _fork_point: i64,
        _fork_hash: &[u8],
        _old_tip: i64,
        _old_tip_hash: &[u8],
        _new_tip: i64,
        _new_tip_hash: &[u8],
        _blocks_deleted: i64,
    ) -> Result<()> {
        Ok(())
    }
}

// ============================================================================
// ROW STRUCTS (matching ClickHouse schema)
// ============================================================================

/// Block row matching the blocks table schema.
///
/// All hash fields use [u8; 32] for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct BlockRow {
    // Block identification
    pub number: u64,
    pub hash: [u8; 32],
    pub parent_hash: [u8; 32],
    pub timestamp: u32, // DateTime in ClickHouse (Unix timestamp)

    // Block header fields
    pub version: u32,
    pub compact_target: u64,
    pub nonce: [u8; 16],

    // Merkle roots
    pub transactions_root: [u8; 32],
    pub proposals_hash: [u8; 32],
    pub extra_hash: [u8; 32],
    pub uncles_hash: [u8; 32],

    // Epoch information
    pub epoch_number: u64,
    pub epoch_index: u32,
    pub epoch_length: u32,

    // DAO field
    pub dao: [u8; 32],

    // Block statistics
    pub transactions_count: u32,
    pub proposals_count: u32,
    pub uncles_count: u32,

    // Optional fields
    pub extension: Option<String>,
    pub miner_lock_hash: Option<[u8; 32]>,
    pub miner_message: Option<String>,

    // Difficulty tracking
    pub total_difficulty: String,
}

/// Transaction row matching the transactions table schema.
///
/// All hash fields use [u8; 32] for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct TransactionRow {
    // Transaction identification
    pub hash: [u8; 32],
    pub block_number: u64,
    pub tx_index: u32,
    pub timestamp: u32, // DateTime in ClickHouse (Unix timestamp)

    // Transaction structure
    pub version: u32,
    pub inputs_count: u16,
    pub outputs_count: u16,
    pub witnesses_count: u16,
    pub cell_deps_count: u16,
    pub header_deps_count: u16,

    // Capacity tracking
    pub total_input_capacity: u64,
    pub total_output_capacity: u64,
    pub fee: u64,

    // Transaction metadata
    pub is_cellbase: u8,
    pub tx_size: Option<u32>,
    pub cycles: Option<u64>,
}

/// Cell row matching the cells table schema.
///
/// All hash fields use [u8; 32] for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct CellRow {
    // Cell identification (OutPoint)
    pub tx_hash: [u8; 32],
    pub output_index: u16,
    pub created_at_block: u64,

    // Cell capacity
    pub capacity: u64,

    // Lock script (required)
    pub lock_code_hash: [u8; 32],
    pub lock_hash_type: u8,
    pub lock_args: String,
    pub lock_script_hash: [u8; 32],

    // Type script (optional)
    pub type_code_hash: Option<[u8; 32]>,
    pub type_hash_type: Option<u8>,
    pub type_args: Option<String>,
    pub type_script_hash: Option<[u8; 32]>,

    // Cell data
    pub data_hash: [u8; 32],
    pub data_size: u32,
    pub data: Option<String>,
}

/// Cell consumption row matching the cell_consumptions table schema.
///
/// All hash fields use [u8; 32] for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct CellConsumptionRow {
    // Cell identification (OutPoint being consumed)
    pub tx_hash: [u8; 32],
    pub output_index: u16,

    // Consumption metadata
    pub consumed_at_block: u64,
    pub consumed_by_tx: [u8; 32],
    pub consumed_at_index: u16,
}

/// Live cell row matching the live_cells table schema.
///
/// All hash fields use [u8; 32] for binary serialization (FixedString(32) in ClickHouse).
/// Uses ReplacingMergeTree with sign column for efficient live cell queries.
#[derive(Debug, Clone, Serialize, Row)]
pub struct LiveCellRow {
    // OutPoint (PRIMARY KEY)
    pub tx_hash: [u8; 32],
    pub output_index: u16,

    // Essential cell data
    pub capacity: u64,
    pub lock_script_hash: [u8; 32],
    pub type_script_hash: Option<[u8; 32]>,
    pub created_at_block: u64,

    // ReplacingMergeTree metadata
    pub sign: i8,     // 1 = created (live), -1 = consumed (dead)
    pub version: u64, // Block number (for deduplication, higher wins)
}

/// DAO deposit row matching the dao_deposits table schema.
///
/// All hash fields use [u8; 32] for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct DaoDepositRow {
    // Cell identification (OutPoint)
    pub tx_hash: [u8; 32],
    pub output_index: u16,

    // Depositor information
    pub depositor_lock_hash: [u8; 32],

    // Deposit metadata
    pub capacity: u64,
    pub deposit_block: u64,
    pub deposit_timestamp: u32,
    pub deposit_ar: u64,
}

/// DAO withdrawal row matching the dao_withdrawals table schema.
///
/// All hash fields use [u8; 32] for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct DaoWithdrawalRow {
    // Original deposit identification
    pub deposit_tx: [u8; 32],
    pub deposit_index: u16,

    // Withdraw request metadata
    pub withdraw_request_tx: [u8; 32],
    pub withdraw_request_block: u64,
    pub withdraw_request_timestamp: u32,
    pub withdraw_request_ar: u64,

    // Withdraw completion metadata (NULL until completed)
    pub withdraw_completion_tx: Option<[u8; 32]>,
    pub withdraw_completion_block: Option<u64>,
    pub withdraw_completion_timestamp: Option<u32>,
    pub compensation: Option<u64>,
}

/// Token transfer row matching the token_transfers table schema.
///
/// All hash fields use [u8; 32] for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct TokenTransferRow {
    // Token identification
    pub type_script_hash: [u8; 32],

    // Transfer participants
    pub from_lock_hash: Option<[u8; 32]>,
    pub to_lock_hash: Option<[u8; 32]>,

    // Transfer metadata
    pub amount: String,
    pub block_number: u64,
    pub tx_hash: [u8; 32],
    pub tx_index: u32,
    pub timestamp: u32,
}

/// Spore cell row matching the spore_cells table schema.
///
/// All hash fields use [u8; 32] for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct SporeCellRow {
    // Cell identification (OutPoint)
    pub tx_hash: [u8; 32],
    pub output_index: u16,

    // Spore identification
    pub spore_id: [u8; 32],
    pub cluster_id: Option<[u8; 32]>,

    // Spore metadata
    pub content_type: String,
    pub content_size: u32,
    pub content: Option<String>,

    // Ownership
    pub owner_lock_hash: [u8; 32],

    // Lifecycle metadata
    pub created_at_block: u64,
    pub created_at_timestamp: u32,
    pub consumed_at_block: Option<u64>,
    pub consumed_by_tx: Option<[u8; 32]>,
}

/// Spore transfer row matching the spore_transfers table schema.
///
/// All hash fields use [u8; 32] for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct SporeTransferRow {
    // Spore identification (OutPoint)
    pub tx_hash: [u8; 32],
    pub output_index: u16,
    pub spore_id: [u8; 32],

    // Transfer participants
    pub from_lock_hash: Option<[u8; 32]>,
    pub to_lock_hash: Option<[u8; 32]>,

    // Transfer metadata
    pub block_number: u64,
    pub transfer_tx: [u8; 32],
    pub timestamp: u32,
}

/// Block proposal row matching the block_proposals table schema.
#[derive(Debug, Clone, Serialize, Row)]
pub struct BlockProposalRow {
    pub block_number: u64,
    pub proposal_index: u16,
    pub proposal_hash: [u8; 10],
}

/// Transaction input row matching the transaction_inputs table schema.
#[derive(Debug, Clone, Serialize, Row)]
pub struct TransactionInputRow {
    pub tx_hash: [u8; 32],
    pub tx_block_number: u64,
    pub input_index: u16,
    pub previous_tx_hash: [u8; 32],
    pub previous_output_index: u16,
    pub since: u64,
}

/// Transaction cell dep row matching the transaction_cell_deps table schema.
#[derive(Debug, Clone, Serialize, Row)]
pub struct TransactionCellDepRow {
    pub tx_hash: [u8; 32],
    pub tx_block_number: u64,
    pub dep_index: u16,
    pub dep_tx_hash: [u8; 32],
    pub dep_output_index: u16,
    pub dep_type: String,
}

/// Address transaction row matching the address_transactions table schema.
#[derive(Debug, Clone, Serialize, Row)]
pub struct AddressTransactionRow {
    pub lock_hash: [u8; 32],
    pub tx_hash: [u8; 32],
    pub block_number: u64,
    pub tx_type: i8,
    pub balance_change: i64,
    pub timestamp: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_row_creation() {
        let block = BlockRow {
            number: 12345,
            hash: [0u8; 32],
            parent_hash: [1u8; 32],
            timestamp: 1704067200,
            version: 0,
            compact_target: 0x1a08a97e,
            nonce: [0x78, 0x56, 0x34, 0x12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            transactions_root: [2u8; 32],
            proposals_hash: [3u8; 32],
            extra_hash: [4u8; 32],
            uncles_hash: [5u8; 32],
            epoch_number: 40,
            epoch_index: 6,
            epoch_length: 1800,
            dao: [6u8; 32],
            transactions_count: 5,
            proposals_count: 2,
            uncles_count: 0,
            extension: None,
            miner_lock_hash: None,
            miner_message: None,
            total_difficulty: "1000000".to_string(),
        };

        assert_eq!(block.number, 12345);
        assert_eq!(block.hash.len(), 32);
        assert_eq!(block.transactions_count, 5);
    }

    #[test]
    fn test_transaction_row_creation() {
        let tx = TransactionRow {
            hash: [0u8; 32],
            block_number: 12345,
            tx_index: 0,
            timestamp: 1704067200,
            version: 0,
            inputs_count: 2,
            outputs_count: 3,
            witnesses_count: 2,
            cell_deps_count: 1,
            header_deps_count: 0,
            total_input_capacity: 20000000000,
            total_output_capacity: 19999900000,
            fee: 100000,
            is_cellbase: 0,
            tx_size: Some(512),
            cycles: Some(1000000),
        };

        assert_eq!(tx.block_number, 12345);
        assert_eq!(tx.hash.len(), 32);
        assert_eq!(tx.inputs_count, 2);
        assert_eq!(tx.outputs_count, 3);
    }

    #[test]
    fn test_cell_row_creation() {
        let cell = CellRow {
            tx_hash: [0u8; 32],
            output_index: 0,
            created_at_block: 12345,
            capacity: 10000000000,
            lock_code_hash: [1u8; 32],
            lock_hash_type: 1,
            lock_args: "0x1234".to_string(),
            lock_script_hash: [2u8; 32],
            type_code_hash: Some([3u8; 32]),
            type_hash_type: Some(1),
            type_args: Some("0x5678".to_string()),
            type_script_hash: Some([4u8; 32]),
            data_hash: [5u8; 32],
            data_size: 128,
            data: Some("0xabcd".to_string()),
        };

        assert_eq!(cell.tx_hash.len(), 32);
        assert_eq!(cell.output_index, 0);
        assert_eq!(cell.capacity, 10000000000);
        assert!(cell.type_code_hash.is_some());
    }

    #[test]
    fn test_cell_consumption_row_creation() {
        let consumption = CellConsumptionRow {
            tx_hash: [0u8; 32],
            output_index: 0,
            consumed_at_block: 12346,
            consumed_by_tx: [1u8; 32],
            consumed_at_index: 1,
        };

        assert_eq!(consumption.tx_hash.len(), 32);
        assert_eq!(consumption.consumed_at_block, 12346);
        assert_eq!(consumption.consumed_by_tx.len(), 32);
    }

    #[test]
    fn test_decode_sync_tip_hash() {
        assert_eq!(decode_sync_tip_hash("00ff"), Some(vec![0x00, 0xff]));
        assert!(decode_sync_tip_hash("not-hex").is_none());
    }

    #[tokio::test]
    async fn test_has_unresolved_deep_fork_stub() {
        let client =
            ClickHouseClient::new("http://localhost:8123", "default", "", "default").unwrap();
        let writer = ClickHouseWriter::new(client);

        let result = writer.has_unresolved_deep_fork().await;

        assert_eq!(result.unwrap(), false);
    }

    #[test]
    fn test_live_cell_row_creation() {
        let live_cell = LiveCellRow {
            tx_hash: [0u8; 32],
            output_index: 0,
            capacity: 10000000000,
            lock_script_hash: [1u8; 32],
            type_script_hash: Some([2u8; 32]),
            created_at_block: 12345,
            sign: 1,
            version: 12345,
        };

        assert_eq!(live_cell.tx_hash.len(), 32);
        assert_eq!(live_cell.sign, 1);
        assert_eq!(live_cell.version, 12345);
    }

    #[test]
    fn test_live_cell_consumption() {
        let consumption = LiveCellRow {
            tx_hash: [0u8; 32],
            output_index: 0,
            capacity: 10000000000,
            lock_script_hash: [1u8; 32],
            type_script_hash: Some([2u8; 32]),
            created_at_block: 12345,
            sign: -1, // Consumed
            version: 12346,
        };

        assert_eq!(consumption.sign, -1);
        assert_eq!(consumption.version, 12346);
    }
}
