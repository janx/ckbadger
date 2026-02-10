use anyhow::Result;

use super::BatchWriter;

impl BatchWriter {
    /// Transaction inputs are no longer stored in a separate table.
    /// Input data is derived from the cell consumption (consume_cells_batch).
    /// This method is kept for API compatibility but is a no-op.
    pub fn insert_transaction_inputs_batch(
        &self,
        _inputs: &[(&[u8], i64, i16, &crate::parser::transaction::ParsedInput)],
    ) -> Result<()> {
        // No-op: input data is captured via cell consumption in cells.rs
        Ok(())
    }

    /// Block proposals are read directly from CKB node's RocksDB.
    /// No need to store them separately.
    pub fn insert_block_proposals_batch(
        &self,
        _block_number: i64,
        _proposals: &[Vec<u8>],
    ) -> Result<()> {
        // No-op: proposals are available via ckb-store-reader
        Ok(())
    }

    /// Cell flows are captured via activities.
    pub fn insert_cell_flows_batch(
        &self,
        _flows: &[(i64, &[u8], i16, i16, &[u8], i64, i32, Option<&[u8]>)],
    ) -> Result<()> {
        // No-op: cell flow data is captured in activities
        Ok(())
    }

    pub fn insert_proposals_batch(&self, _proposals: &[(i64, i16, &[u8])]) -> Result<()> {
        // No-op: proposals are available via ckb-store-reader
        Ok(())
    }
}
