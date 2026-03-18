use anyhow::Result;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{CachedBlockHeader, TxIndexEntry};

use crate::parser::block::ParsedBlock;
use crate::sync::types::TxData;

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
                hash: block.hash.clone(),
                timestamp: block.timestamp.timestamp_millis(),
                epoch_number: block.epoch_number,
                epoch_index: block.epoch_index,
                epoch_length: block.epoch_length,
                dao: block.dao.to_vec(),
                transactions_count: block.transactions_count,
            };
            batch.put_block_header(block.number, &header);
        }

        Ok(())
    }

    pub(crate) fn insert_transactions_batch(
        &self,
        txs: &[&TxData],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if txs.is_empty() {
            return Ok(());
        }

        for tx in txs {
            let entry = TxIndexEntry {
                is_cellbase: tx.is_cellbase,
                timestamp: tx.timestamp.timestamp_millis(),
                inputs_count: tx.inputs_count,
                outputs_count: tx.outputs_count,
                fee: tx.fee,
                tx_size: tx.tx_size,
                cycles: tx.cycles,
            };
            batch.put_tx_index(tx.block_number, tx.tx_index, &entry);
            batch.put_tx_hash_map(&tx.hash, tx.block_number, tx.tx_index);
        }

        Ok(())
    }
}
