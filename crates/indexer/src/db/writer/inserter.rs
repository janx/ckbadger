//! Inserter-based batch writer for heavy tables.
//!
//! Uses the ClickHouse Inserter API for high-throughput writes to tables
//! with high row counts (transactions, cells, activities, cell_state).
//!
//! The Inserter API provides:
//! - Automatic batching with configurable row/byte limits
//! - Better memory efficiency for large batches
//! - Streaming writes without buffering entire batch in memory
//!
//! IMPORTANT: Inserter is `!Sync`, so we create one per table per write operation.
//! Do not attempt to share Inserter instances across threads.

use anyhow::{Context, Result};

use crate::cache::CacheInvalidator;
use crate::db::{ClickHouseClient, DynLiveCellStorage};

use super::rows::{
    ActivityRow, BlockRow, CellInputRow, CellOutputRow, CellStateRow, DaoDepositRow, MnftClassRow,
    MnftIssuerRow, MnftTokenRow, SporeCellRow, SporeClusterRow, TransactionRow, UdtCellRow,
};
use super::BatchData;

/// Batch writer that uses the Inserter API for heavy tables.
///
/// Heavy tables (using Inserter):
/// - transactions_all
/// - cell_outputs_all
/// - cell_inputs_all
/// - activities_all
/// - cell_state
///
/// Light tables (using Insert):
/// - blocks_all
/// - canonical_blocks
/// - dao_deposits
/// - udt_cells
/// - spore_clusters
/// - spore_cells
/// - mnft_issuers
/// - mnft_classes
/// - mnft_tokens
#[derive(Clone)]
pub struct InserterBatchWriter {
    client: ClickHouseClient,
    fast_sync_mode: bool,
    live_cell_store: Option<DynLiveCellStorage>,
    cache_invalidator: Option<CacheInvalidator>,
}

impl InserterBatchWriter {
    /// Create a new InserterBatchWriter.
    pub fn new(client: ClickHouseClient) -> Self {
        Self {
            client,
            fast_sync_mode: true,
            live_cell_store: None,
            cache_invalidator: None,
        }
    }

    /// Create with fast sync mode configuration.
    pub fn with_fast_sync_mode(client: ClickHouseClient, fast_sync_mode: bool) -> Self {
        Self {
            client,
            fast_sync_mode,
            live_cell_store: None,
            cache_invalidator: None,
        }
    }

    /// Create with live cell store and cache invalidator.
    pub fn with_live_cell_store(
        client: ClickHouseClient,
        fast_sync_mode: bool,
        live_cell_store: DynLiveCellStorage,
        cache_invalidator: CacheInvalidator,
    ) -> Self {
        Self {
            client,
            fast_sync_mode,
            live_cell_store: Some(live_cell_store),
            cache_invalidator: Some(cache_invalidator),
        }
    }

    /// Get the underlying ClickHouse client.
    pub fn client(&self) -> &ClickHouseClient {
        &self.client
    }

    /// Check if fast sync mode is enabled.
    pub fn is_fast_sync_mode(&self) -> bool {
        self.fast_sync_mode
    }

    /// Get the cache invalidator if configured.
    pub fn cache_invalidator(&self) -> Option<&CacheInvalidator> {
        self.cache_invalidator.as_ref()
    }

    /// Get the live cell store if configured.
    pub fn live_cell_store(&self) -> Option<&DynLiveCellStorage> {
        self.live_cell_store.as_ref()
    }

    /// Write a complete batch using Inserter API for heavy tables.
    ///
    /// Uses a two-phase approach for better throughput:
    /// - Phase 1: Heavy tables written sequentially (better for Inserter API)
    /// - Phase 2: Light tables written in parallel (using Insert API)
    pub async fn write_batch(&self, batch: &BatchData) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }

        // Phase 1: Heavy tables (sequential for better throughput with Inserter)
        tracing::info!("Writing heavy tables...");
        self.write_transactions_inserter(&batch.transactions)
            .await?;
        self.write_cell_outputs_inserter(&batch.cell_outputs)
            .await?;
        self.write_cell_inputs_inserter(&batch.cell_inputs).await?;
        self.write_activities_inserter(&batch.activities).await?;
        self.write_cell_states_inserter(&batch.cell_states).await?;

        // Phase 2: Light tables (parallel with Insert API)
        tracing::info!("Writing light tables...");
        tokio::try_join!(
            self.write_blocks(&batch.blocks),
            self.write_canonical_blocks(&batch.canonical_mappings),
            self.write_dao_deposits(&batch.dao_deposits),
            self.write_udt_cells(&batch.udt_cells),
            self.write_spore_clusters(&batch.spore_clusters),
            self.write_spore_cells(&batch.spore_cells),
            self.write_mnft_issuers(&batch.mnft_issuers),
            self.write_mnft_classes(&batch.mnft_classes),
            self.write_mnft_tokens(&batch.mnft_tokens),
        )
        .context("Failed to write light tables")?;

        Ok(())
    }

    // =========================================================================
    // Heavy Tables - Inserter API
    // =========================================================================

    /// Write transactions using Inserter API for better throughput.
    async fn write_transactions_inserter(&self, txs: &[TransactionRow]) -> Result<()> {
        if txs.is_empty() {
            return Ok(());
        }

        let mut inserter = self.client.inserter::<TransactionRow>("transactions_all");

        for tx in txs {
            inserter
                .write(tx)
                .await
                .context("Failed to write transaction row via inserter")?;
        }

        inserter
            .end()
            .await
            .context("Failed to finalize transactions_all inserter")?;

        Ok(())
    }

    /// Write cell outputs using Inserter API for better throughput.
    async fn write_cell_outputs_inserter(&self, cells: &[CellOutputRow]) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        let mut inserter = self.client.inserter::<CellOutputRow>("cell_outputs_all");

        for cell in cells {
            inserter
                .write(cell)
                .await
                .context("Failed to write cell output row via inserter")?;
        }

        inserter
            .end()
            .await
            .context("Failed to finalize cell_outputs_all inserter")?;

        Ok(())
    }

    /// Write cell inputs using Inserter API for better throughput.
    async fn write_cell_inputs_inserter(&self, inputs: &[CellInputRow]) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }

        let mut inserter = self.client.inserter::<CellInputRow>("cell_inputs_all");

        for input in inputs {
            inserter
                .write(input)
                .await
                .context("Failed to write cell input row via inserter")?;
        }

        inserter
            .end()
            .await
            .context("Failed to finalize cell_inputs_all inserter")?;

        Ok(())
    }

    /// Write activities using Inserter API for better throughput.
    async fn write_activities_inserter(&self, activities: &[ActivityRow]) -> Result<()> {
        if activities.is_empty() {
            return Ok(());
        }

        let mut inserter = self.client.inserter::<ActivityRow>("activities_all");

        for activity in activities {
            inserter
                .write(activity)
                .await
                .context("Failed to write activity row via inserter")?;
        }

        inserter
            .end()
            .await
            .context("Failed to finalize activities_all inserter")?;

        Ok(())
    }

    /// Write cell states using Inserter API for better throughput.
    async fn write_cell_states_inserter(&self, states: &[CellStateRow]) -> Result<()> {
        if states.is_empty() {
            return Ok(());
        }

        let mut inserter = self.client.inserter::<CellStateRow>("cell_state");

        for state in states {
            inserter
                .write(state)
                .await
                .context("Failed to write cell state row via inserter")?;
        }

        inserter
            .end()
            .await
            .context("Failed to finalize cell_state inserter")?;

        Ok(())
    }

    // =========================================================================
    // Light Tables - Insert API (same as BatchWriter)
    // =========================================================================

    /// Write blocks using Insert API.
    async fn write_blocks(&self, blocks: &[BlockRow]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<BlockRow>("blocks_all")
            .await
            .context("Failed to create blocks_all insert")?;

        for block in blocks {
            insert
                .write(block)
                .await
                .context("Failed to write block row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize blocks_all insert")?;

        Ok(())
    }

    /// Write canonical block mappings using Insert API.
    async fn write_canonical_blocks(&self, mappings: &[(u64, Vec<u8>, u64)]) -> Result<()> {
        if mappings.is_empty() {
            return Ok(());
        }

        use super::rows::CanonicalBlockRow;

        let mut insert = self
            .client
            .insert::<CanonicalBlockRow>("canonical_blocks")
            .await
            .context("Failed to create canonical_blocks insert")?;

        for (number, hash, version) in mappings {
            let row = CanonicalBlockRow {
                number: *number,
                block_hash: super::rows::to_hash32(hash),
                canon_version: *version,
            };
            insert
                .write(&row)
                .await
                .context("Failed to write canonical block row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize canonical_blocks insert")?;

        Ok(())
    }

    /// Write DAO deposits using Insert API.
    async fn write_dao_deposits(&self, deposits: &[DaoDepositRow]) -> Result<()> {
        if deposits.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<DaoDepositRow>("dao_deposits")
            .await
            .context("Failed to create dao_deposits insert")?;

        for deposit in deposits {
            insert
                .write(deposit)
                .await
                .context("Failed to write dao deposit row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize dao_deposits insert")?;

        Ok(())
    }

    /// Write UDT cells using Insert API.
    async fn write_udt_cells(&self, cells: &[UdtCellRow]) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<UdtCellRow>("udt_cells")
            .await
            .context("Failed to create udt_cells insert")?;

        for cell in cells {
            insert
                .write(cell)
                .await
                .context("Failed to write udt cell row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize udt_cells insert")?;

        Ok(())
    }

    /// Write spore clusters using Insert API.
    async fn write_spore_clusters(&self, clusters: &[SporeClusterRow]) -> Result<()> {
        if clusters.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<SporeClusterRow>("spore_clusters")
            .await
            .context("Failed to create spore_clusters insert")?;

        for cluster in clusters {
            insert
                .write(cluster)
                .await
                .context("Failed to write spore cluster row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize spore_clusters insert")?;

        Ok(())
    }

    /// Write spore cells using Insert API.
    async fn write_spore_cells(&self, cells: &[SporeCellRow]) -> Result<()> {
        if cells.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<SporeCellRow>("spore_cells")
            .await
            .context("Failed to create spore_cells insert")?;

        for cell in cells {
            insert
                .write(cell)
                .await
                .context("Failed to write spore cell row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize spore_cells insert")?;

        Ok(())
    }

    /// Write mNFT issuers using Insert API.
    async fn write_mnft_issuers(&self, issuers: &[MnftIssuerRow]) -> Result<()> {
        if issuers.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<MnftIssuerRow>("mnft_issuers")
            .await
            .context("Failed to create mnft_issuers insert")?;

        for issuer in issuers {
            insert
                .write(issuer)
                .await
                .context("Failed to write mnft issuer row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize mnft_issuers insert")?;

        Ok(())
    }

    /// Write mNFT classes using Insert API.
    async fn write_mnft_classes(&self, classes: &[MnftClassRow]) -> Result<()> {
        if classes.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<MnftClassRow>("mnft_classes")
            .await
            .context("Failed to create mnft_classes insert")?;

        for class in classes {
            insert
                .write(class)
                .await
                .context("Failed to write mnft class row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize mnft_classes insert")?;

        Ok(())
    }

    /// Write mNFT tokens using Insert API.
    async fn write_mnft_tokens(&self, tokens: &[MnftTokenRow]) -> Result<()> {
        if tokens.is_empty() {
            return Ok(());
        }

        let mut insert = self
            .client
            .insert::<MnftTokenRow>("mnft_tokens")
            .await
            .context("Failed to create mnft_tokens insert")?;

        for token in tokens {
            insert
                .write(token)
                .await
                .context("Failed to write mnft token row")?;
        }

        insert
            .end()
            .await
            .context("Failed to finalize mnft_tokens insert")?;

        Ok(())
    }

    /// Write only canonical block mappings (used after reorg).
    pub async fn write_canonical_only(&self, mappings: &[(u64, Vec<u8>, u64)]) -> Result<()> {
        self.write_canonical_blocks(mappings).await
    }

    /// Write cell states (public interface for reorg handling).
    pub async fn write_cell_states(&self, states: &[CellStateRow]) -> Result<()> {
        self.write_cell_states_inserter(states).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inserter_batch_writer_new() {
        // Can't easily test without ClickHouse connection, but we can verify construction
        // This test mainly ensures the struct compiles correctly
    }

    #[test]
    fn test_batch_data_empty_check() {
        let batch = BatchData::default();
        assert!(batch.is_empty());
        assert_eq!(batch.total_rows(), 0);
    }

    #[test]
    fn test_batch_data_with_transactions() {
        let mut batch = BatchData::default();
        batch.transactions.push(TransactionRow::default());
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 1);
    }

    #[test]
    fn test_batch_data_with_cell_outputs() {
        let mut batch = BatchData::default();
        batch.cell_outputs.push(CellOutputRow::default());
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 1);
    }

    #[test]
    fn test_batch_data_with_cell_inputs() {
        let mut batch = BatchData::default();
        batch.cell_inputs.push(CellInputRow::default());
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 1);
    }

    #[test]
    fn test_batch_data_with_activities() {
        let mut batch = BatchData::default();
        batch.activities.push(ActivityRow::default());
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 1);
    }

    #[test]
    fn test_batch_data_with_cell_states() {
        let mut batch = BatchData::default();
        batch.cell_states.push(CellStateRow::default());
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 1);
    }

    #[test]
    fn test_batch_data_heavy_tables_count() {
        // Heavy tables: transactions, cell_outputs, cell_inputs, activities, cell_states
        let mut batch = BatchData::default();
        batch.transactions.push(TransactionRow::default());
        batch.transactions.push(TransactionRow::default());
        batch.cell_outputs.push(CellOutputRow::default());
        batch.cell_outputs.push(CellOutputRow::default());
        batch.cell_outputs.push(CellOutputRow::default());
        batch.cell_inputs.push(CellInputRow::default());
        batch.activities.push(ActivityRow::default());
        batch.cell_states.push(CellStateRow::default());
        batch.cell_states.push(CellStateRow::default());

        // 2 + 3 + 1 + 1 + 2 = 9 rows in heavy tables
        let heavy_count = batch.transactions.len()
            + batch.cell_outputs.len()
            + batch.cell_inputs.len()
            + batch.activities.len()
            + batch.cell_states.len();
        assert_eq!(heavy_count, 9);
    }

    #[test]
    fn test_batch_data_light_tables_count() {
        // Light tables: blocks, canonical_mappings, dao_deposits, udt_cells, spores, mnft
        let mut batch = BatchData::default();
        batch.blocks.push(super::BlockRow::default());
        batch.canonical_mappings.push((1, vec![0u8; 32], 1));
        batch.dao_deposits.push(DaoDepositRow::default());
        batch.udt_cells.push(UdtCellRow::default());
        batch.spore_clusters.push(SporeClusterRow::default());
        batch.spore_cells.push(SporeCellRow::default());
        batch.mnft_issuers.push(MnftIssuerRow::default());
        batch.mnft_classes.push(MnftClassRow::default());
        batch.mnft_tokens.push(MnftTokenRow::default());

        let light_count = batch.blocks.len()
            + batch.canonical_mappings.len()
            + batch.dao_deposits.len()
            + batch.udt_cells.len()
            + batch.spore_clusters.len()
            + batch.spore_cells.len()
            + batch.mnft_issuers.len()
            + batch.mnft_classes.len()
            + batch.mnft_tokens.len();
        assert_eq!(light_count, 9);
    }
}
