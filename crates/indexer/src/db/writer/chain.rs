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
            let compact_target = u32::try_from(block.compact_target).map_err(|_| {
                anyhow::anyhow!(
                    "block compact_target out of u32 range: block={}, compact_target={}",
                    block.number,
                    block.compact_target
                )
            })?;
            let header = CachedBlockHeader {
                hash: block.hash.clone(),
                parent_hash: block.parent_hash.clone(),
                timestamp: block.timestamp.timestamp_millis(),
                epoch_number: block.epoch_number,
                epoch_index: block.epoch_index,
                epoch_length: block.epoch_length,
                dao: block.dao.to_vec(),
                transactions_count: block.transactions_count,
                uncles_count: block.uncles_count,
                proposals_count: block.proposals_count,
                compact_target,
                miner_lock_hash: block.miner_lock_hash.clone(),
                cycles: None,
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
                semantic_tags: tx.semantic_tags,
            };
            batch.put_tx_index(tx.block_number, tx.tx_index, &entry);
            batch.put_tx_hash_map(&tx.hash, tx.block_number, tx.tx_index);
        }

        Ok(())
    }
}
