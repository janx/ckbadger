use anyhow::Result;
use clickhouse::Row;
use serde::Serialize;

use super::clickhouse::ClickHouseClient;

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
pub struct ClickHouseWriter {
    client: ClickHouseClient,
}

impl ClickHouseWriter {
    /// Create a new ClickHouse writer with the given client.
    pub fn new(client: ClickHouseClient) -> Self {
        Self { client }
    }

    /// Insert a batch of blocks into the blocks table.
    ///
    /// # Arguments
    ///
    /// * `blocks` - Vector of BlockRow instances to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_blocks_batch(&self, blocks: Vec<BlockRow>) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("blocks")?;
        for block in blocks {
            insert.write(&block).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Insert a batch of transactions into the transactions table.
    ///
    /// # Arguments
    ///
    /// * `transactions` - Vector of TransactionRow instances to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_transactions_batch(&self, transactions: Vec<TransactionRow>) -> Result<()> {
        if transactions.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("transactions")?;
        for tx in transactions {
            insert.write(&tx).await?;
        }
        insert.end().await?;

        Ok(())
    }

    /// Insert a batch of cells into the cells table.
    ///
    /// # Arguments
    ///
    /// * `cells` - Vector of CellRow instances to insert
    ///
    /// # Errors
    ///
    /// Returns an error if the insert operation fails.
    pub async fn insert_cells_batch(&self, cells: Vec<CellRow>) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.client().insert("cells")?;
        for cell in cells {
            insert.write(&cell).await?;
        }
        insert.end().await?;

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

        let mut insert = self.client.client().insert("cell_consumptions")?;
        for consumption in consumptions {
            insert.write(&consumption).await?;
        }
        insert.end().await?;

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
}

// ============================================================================
// ROW STRUCTS (matching ClickHouse schema)
// ============================================================================

/// Block row matching the blocks table schema.
///
/// All hash fields use Vec<u8> for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct BlockRow {
    // Block identification
    pub number: u64,
    pub hash: Vec<u8>,
    pub parent_hash: Vec<u8>,
    pub timestamp: u32, // DateTime in ClickHouse (Unix timestamp)

    // Block header fields
    pub version: u32,
    pub compact_target: u64,
    pub nonce: Vec<u8>,

    // Merkle roots
    pub transactions_root: Vec<u8>,
    pub proposals_hash: Vec<u8>,
    pub extra_hash: Vec<u8>,
    pub uncles_hash: Vec<u8>,

    // Epoch information
    pub epoch_number: u64,
    pub epoch_index: u32,
    pub epoch_length: u32,

    // DAO field
    pub dao: Vec<u8>,

    // Block statistics
    pub transactions_count: u32,
    pub proposals_count: u32,
    pub uncles_count: u32,

    // Optional fields
    pub extension: Option<String>,
    pub miner_lock_hash: Option<Vec<u8>>,
    pub miner_message: Option<String>,

    // Difficulty tracking
    pub total_difficulty: String,
}

/// Transaction row matching the transactions table schema.
///
/// All hash fields use Vec<u8> for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct TransactionRow {
    // Transaction identification
    pub hash: Vec<u8>,
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
/// All hash fields use Vec<u8> for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct CellRow {
    // Cell identification (OutPoint)
    pub tx_hash: Vec<u8>,
    pub output_index: u16,
    pub created_at_block: u64,

    // Cell capacity
    pub capacity: u64,

    // Lock script (required)
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: u8,
    pub lock_args: String,
    pub lock_script_hash: Vec<u8>,

    // Type script (optional)
    pub type_code_hash: Option<Vec<u8>>,
    pub type_hash_type: Option<u8>,
    pub type_args: Option<String>,
    pub type_script_hash: Option<Vec<u8>>,

    // Cell data
    pub data_hash: Vec<u8>,
    pub data_size: u32,
    pub data: Option<String>,
}

/// Cell consumption row matching the cell_consumptions table schema.
///
/// All hash fields use Vec<u8> for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct CellConsumptionRow {
    // Cell identification (OutPoint being consumed)
    pub tx_hash: Vec<u8>,
    pub output_index: u16,

    // Consumption metadata
    pub consumed_at_block: u64,
    pub consumed_by_tx: Vec<u8>,
    pub consumed_at_index: u16,
}

/// Live cell row matching the live_cells table schema.
///
/// All hash fields use Vec<u8> for binary serialization (FixedString(32) in ClickHouse).
/// Uses ReplacingMergeTree with sign column for efficient live cell queries.
#[derive(Debug, Clone, Serialize, Row)]
pub struct LiveCellRow {
    // OutPoint (PRIMARY KEY)
    pub tx_hash: Vec<u8>,
    pub output_index: u16,

    // Essential cell data
    pub capacity: u64,
    pub lock_script_hash: Vec<u8>,
    pub type_script_hash: Option<Vec<u8>>,
    pub created_at_block: u64,

    // ReplacingMergeTree metadata
    pub sign: i8,     // 1 = created (live), -1 = consumed (dead)
    pub version: u64, // Block number (for deduplication, higher wins)
}

/// DAO deposit row matching the dao_deposits table schema.
///
/// All hash fields use Vec<u8> for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct DaoDepositRow {
    // Cell identification (OutPoint)
    pub tx_hash: Vec<u8>,
    pub output_index: u16,

    // Depositor information
    pub depositor_lock_hash: Vec<u8>,

    // Deposit metadata
    pub capacity: u64,
    pub deposit_block: u64,
    pub deposit_timestamp: u32,
    pub deposit_ar: u64,
}

/// DAO withdrawal row matching the dao_withdrawals table schema.
///
/// All hash fields use Vec<u8> for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct DaoWithdrawalRow {
    // Original deposit identification
    pub deposit_tx: Vec<u8>,
    pub deposit_index: u16,

    // Withdraw request metadata
    pub withdraw_request_tx: Vec<u8>,
    pub withdraw_request_block: u64,
    pub withdraw_request_timestamp: u32,
    pub withdraw_request_ar: u64,

    // Withdraw completion metadata (NULL until completed)
    pub withdraw_completion_tx: Option<Vec<u8>>,
    pub withdraw_completion_block: Option<u64>,
    pub withdraw_completion_timestamp: Option<u32>,
    pub compensation: Option<u64>,
}

/// Token transfer row matching the token_transfers table schema.
///
/// All hash fields use Vec<u8> for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct TokenTransferRow {
    // Token identification
    pub type_script_hash: Vec<u8>,

    // Transfer participants
    pub from_lock_hash: Option<Vec<u8>>,
    pub to_lock_hash: Option<Vec<u8>>,

    // Transfer metadata
    pub amount: String,
    pub block_number: u64,
    pub tx_hash: Vec<u8>,
    pub tx_index: u32,
    pub timestamp: u32,
}

/// Spore cell row matching the spore_cells table schema.
///
/// All hash fields use Vec<u8> for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct SporeCellRow {
    // Cell identification (OutPoint)
    pub tx_hash: Vec<u8>,
    pub output_index: u16,

    // Spore identification
    pub spore_id: Vec<u8>,
    pub cluster_id: Option<Vec<u8>>,

    // Spore metadata
    pub content_type: String,
    pub content_size: u32,
    pub content: Option<String>,

    // Ownership
    pub owner_lock_hash: Vec<u8>,

    // Lifecycle metadata
    pub created_at_block: u64,
    pub created_at_timestamp: u32,
    pub consumed_at_block: Option<u64>,
    pub consumed_by_tx: Option<Vec<u8>>,
}

/// Spore transfer row matching the spore_transfers table schema.
///
/// All hash fields use Vec<u8> for binary serialization (FixedString(32) in ClickHouse).
#[derive(Debug, Clone, Serialize, Row)]
pub struct SporeTransferRow {
    // Spore identification (OutPoint)
    pub tx_hash: Vec<u8>,
    pub output_index: u16,
    pub spore_id: Vec<u8>,

    // Transfer participants
    pub from_lock_hash: Option<Vec<u8>>,
    pub to_lock_hash: Option<Vec<u8>>,

    // Transfer metadata
    pub block_number: u64,
    pub transfer_tx: Vec<u8>,
    pub timestamp: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_row_creation() {
        let block = BlockRow {
            number: 12345,
            hash: vec![0u8; 32],
            parent_hash: vec![1u8; 32],
            timestamp: 1704067200,
            version: 0,
            compact_target: 0x1a08a97e,
            nonce: vec![0x78, 0x56, 0x34, 0x12],
            transactions_root: vec![2u8; 32],
            proposals_hash: vec![3u8; 32],
            extra_hash: vec![4u8; 32],
            uncles_hash: vec![5u8; 32],
            epoch_number: 40,
            epoch_index: 6,
            epoch_length: 1800,
            dao: vec![6u8; 32],
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
            hash: vec![0u8; 32],
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
            tx_hash: vec![0u8; 32],
            output_index: 0,
            created_at_block: 12345,
            capacity: 10000000000,
            lock_code_hash: vec![1u8; 32],
            lock_hash_type: 1,
            lock_args: "0x1234".to_string(),
            lock_script_hash: vec![2u8; 32],
            type_code_hash: Some(vec![3u8; 32]),
            type_hash_type: Some(1),
            type_args: Some("0x5678".to_string()),
            type_script_hash: Some(vec![4u8; 32]),
            data_hash: vec![5u8; 32],
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
            tx_hash: vec![0u8; 32],
            output_index: 0,
            consumed_at_block: 12346,
            consumed_by_tx: vec![1u8; 32],
            consumed_at_index: 1,
        };

        assert_eq!(consumption.tx_hash.len(), 32);
        assert_eq!(consumption.consumed_at_block, 12346);
        assert_eq!(consumption.consumed_by_tx.len(), 32);
    }

    #[test]
    fn test_live_cell_row_creation() {
        let live_cell = LiveCellRow {
            tx_hash: vec![0u8; 32],
            output_index: 0,
            capacity: 10000000000,
            lock_script_hash: vec![1u8; 32],
            type_script_hash: Some(vec![2u8; 32]),
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
            tx_hash: vec![0u8; 32],
            output_index: 0,
            capacity: 10000000000,
            lock_script_hash: vec![1u8; 32],
            type_script_hash: Some(vec![2u8; 32]),
            created_at_block: 12345,
            sign: -1, // Consumed
            version: 12346,
        };

        assert_eq!(consumption.sign, -1);
        assert_eq!(consumption.version, 12346);
    }
}
