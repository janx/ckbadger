use anyhow::Result;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::CachedBlockHeader;

use crate::parser::block::ParsedBlock;

use super::BatchWriter;

impl BatchWriter {
    /// Insert multiple blocks into the RocksDB store.
    pub fn insert_blocks_batch(
        &self,
        blocks: &[&ParsedBlock],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        for block in blocks {
            let header = CachedBlockHeader {
                block_number: block.number,
                hash: block.hash.clone(),
                timestamp: block.timestamp.timestamp_millis(),
                epoch_number: block.epoch_number,
                epoch_index: block.epoch_index,
                epoch_length: block.epoch_length,
                dao: block.dao.clone(),
                transactions_count: block.transactions_count,
            };
            batch.put_block_header(block.number, &header);
        }

        Ok(())
    }
}
