use anyhow::{Context, Result};

use crate::cache::CacheInvalidator;
use crate::db::{ClickHouseClient, DynLiveCellStorage};

use super::rows::{
    ActivityRow, BlockRow, CellInputRow, CellOutputRow, CellStateRow, DaoDepositRow, MnftClassRow,
    MnftIssuerRow, MnftTokenRow, SporeCellRow, SporeClusterRow, TransactionRow, UdtCellRow,
};

#[derive(Debug, Default)]
pub struct BatchData {
    pub blocks: Vec<BlockRow>,
    pub transactions: Vec<TransactionRow>,
    pub cell_outputs: Vec<CellOutputRow>,
    pub cell_inputs: Vec<CellInputRow>,
    pub activities: Vec<ActivityRow>,
    pub cell_states: Vec<CellStateRow>,
    pub dao_deposits: Vec<DaoDepositRow>,
    pub canonical_mappings: Vec<(u64, Vec<u8>, u64)>,
    pub udt_cells: Vec<UdtCellRow>,
    pub spore_clusters: Vec<SporeClusterRow>,
    pub spore_cells: Vec<SporeCellRow>,
    pub mnft_issuers: Vec<MnftIssuerRow>,
    pub mnft_classes: Vec<MnftClassRow>,
    pub mnft_tokens: Vec<MnftTokenRow>,
}

impl BatchData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
            && self.transactions.is_empty()
            && self.cell_outputs.is_empty()
            && self.cell_inputs.is_empty()
            && self.activities.is_empty()
            && self.cell_states.is_empty()
            && self.dao_deposits.is_empty()
            && self.canonical_mappings.is_empty()
            && self.udt_cells.is_empty()
            && self.spore_clusters.is_empty()
            && self.spore_cells.is_empty()
            && self.mnft_issuers.is_empty()
            && self.mnft_classes.is_empty()
            && self.mnft_tokens.is_empty()
    }

    pub fn total_rows(&self) -> usize {
        self.blocks.len()
            + self.transactions.len()
            + self.cell_outputs.len()
            + self.cell_inputs.len()
            + self.activities.len()
            + self.cell_states.len()
            + self.dao_deposits.len()
            + self.canonical_mappings.len()
            + self.udt_cells.len()
            + self.spore_clusters.len()
            + self.spore_cells.len()
            + self.mnft_issuers.len()
            + self.mnft_classes.len()
            + self.mnft_tokens.len()
    }
}

#[derive(Clone)]
pub struct BatchWriter {
    pub(super) client: ClickHouseClient,
    pub(super) fast_sync_mode: bool,
    pub(super) live_cell_store: Option<DynLiveCellStorage>,
    pub(super) cache_invalidator: Option<CacheInvalidator>,
}

impl BatchWriter {
    pub fn new(client: ClickHouseClient) -> Self {
        Self {
            client,
            fast_sync_mode: true,
            live_cell_store: None,
            cache_invalidator: None,
        }
    }

    pub fn with_fast_sync_mode(client: ClickHouseClient, fast_sync_mode: bool) -> Self {
        Self {
            client,
            fast_sync_mode,
            live_cell_store: None,
            cache_invalidator: None,
        }
    }

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

    pub fn client(&self) -> &ClickHouseClient {
        &self.client
    }

    pub fn cache_invalidator(&self) -> Option<&CacheInvalidator> {
        self.cache_invalidator.as_ref()
    }

    pub fn live_cell_store(&self) -> Option<&DynLiveCellStorage> {
        self.live_cell_store.as_ref()
    }

    pub fn is_fast_sync_mode(&self) -> bool {
        self.fast_sync_mode
    }

    pub async fn write_batch(&self, batch: &BatchData) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }

        tokio::try_join!(
            self.write_blocks(&batch.blocks),
            self.write_transactions(&batch.transactions),
            self.write_cell_outputs(&batch.cell_outputs),
            self.write_cell_inputs(&batch.cell_inputs),
            self.write_activities(&batch.activities),
            self.write_cell_states(&batch.cell_states),
            self.write_dao_deposits(&batch.dao_deposits),
            self.write_canonical_blocks(&batch.canonical_mappings),
            self.write_udt_cells(&batch.udt_cells),
            self.write_spore_clusters(&batch.spore_clusters),
            self.write_spore_cells(&batch.spore_cells),
            self.write_mnft_issuers(&batch.mnft_issuers),
            self.write_mnft_classes(&batch.mnft_classes),
            self.write_mnft_tokens(&batch.mnft_tokens),
        )
        .context("Failed to write batch to ClickHouse")?;

        Ok(())
    }

    /// Write a minimal batch with just blocks, transactions, and cells (no activities/state).
    ///
    /// Used during fast sync when activity/state tracking is deferred.
    pub async fn write_core_batch(
        &self,
        blocks: &[BlockRow],
        transactions: &[TransactionRow],
        cell_outputs: &[CellOutputRow],
        cell_inputs: &[CellInputRow],
    ) -> Result<()> {
        tokio::try_join!(
            self.write_blocks(blocks),
            self.write_transactions(transactions),
            self.write_cell_outputs(cell_outputs),
            self.write_cell_inputs(cell_inputs),
        )
        .context("Failed to write core batch to ClickHouse")?;

        Ok(())
    }

    /// Write only canonical block mappings (used after reorg).
    pub async fn write_canonical_only(&self, mappings: &[(u64, Vec<u8>, u64)]) -> Result<()> {
        self.write_canonical_blocks(mappings).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_data_new_is_empty() {
        let batch = BatchData::new();
        assert!(batch.is_empty());
        assert_eq!(batch.total_rows(), 0);
    }

    #[test]
    fn test_batch_data_with_blocks() {
        let mut batch = BatchData::new();
        batch.blocks.push(BlockRow::default());
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 1);
    }

    #[test]
    fn test_batch_data_with_transactions() {
        let mut batch = BatchData::new();
        batch.transactions.push(TransactionRow::default());
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 1);
    }

    #[test]
    fn test_batch_data_with_cells() {
        let mut batch = BatchData::new();
        batch.cell_outputs.push(CellOutputRow::default());
        batch.cell_inputs.push(CellInputRow::default());
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 2);
    }

    #[test]
    fn test_batch_data_with_activities() {
        let mut batch = BatchData::new();
        batch.activities.push(ActivityRow::default());
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 1);
    }

    #[test]
    fn test_batch_data_with_cell_states() {
        let mut batch = BatchData::new();
        batch.cell_states.push(CellStateRow::default());
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 1);
    }

    #[test]
    fn test_batch_data_with_canonical_mappings() {
        let mut batch = BatchData::new();
        batch.canonical_mappings.push((1, vec![0u8; 32], 1));
        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 1);
    }

    #[test]
    fn test_batch_data_total_rows_mixed() {
        let mut batch = BatchData::new();
        batch.blocks.push(BlockRow::default());
        batch.blocks.push(BlockRow::default());
        batch.transactions.push(TransactionRow::default());
        batch.cell_outputs.push(CellOutputRow::default());
        batch.cell_outputs.push(CellOutputRow::default());
        batch.cell_outputs.push(CellOutputRow::default());
        batch.cell_inputs.push(CellInputRow::default());
        batch.activities.push(ActivityRow::default());
        batch.cell_states.push(CellStateRow::default());
        batch.canonical_mappings.push((1, vec![0u8; 32], 1));

        assert!(!batch.is_empty());
        assert_eq!(batch.total_rows(), 10);
    }

    #[test]
    fn test_batch_data_is_empty_requires_all_empty() {
        let mut batch = BatchData::new();
        assert!(batch.is_empty());

        batch.blocks.push(BlockRow::default());
        assert!(!batch.is_empty());
        batch.blocks.clear();

        batch.transactions.push(TransactionRow::default());
        assert!(!batch.is_empty());
        batch.transactions.clear();

        batch.cell_outputs.push(CellOutputRow::default());
        assert!(!batch.is_empty());
        batch.cell_outputs.clear();

        batch.cell_inputs.push(CellInputRow::default());
        assert!(!batch.is_empty());
        batch.cell_inputs.clear();

        batch.activities.push(ActivityRow::default());
        assert!(!batch.is_empty());
        batch.activities.clear();

        batch.cell_states.push(CellStateRow::default());
        assert!(!batch.is_empty());
        batch.cell_states.clear();

        batch.canonical_mappings.push((1, vec![0u8; 32], 1));
        assert!(!batch.is_empty());
        batch.canonical_mappings.clear();

        assert!(batch.is_empty());
    }
}
