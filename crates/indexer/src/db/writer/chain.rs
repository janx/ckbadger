use anyhow::Result;
use chrono::{DateTime, Utc};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{CachedBlockHeader, TxIndexEntry};

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

    pub fn insert_transactions_batch(
        &self,
        txs: &[(
            &[u8],         // hash
            i64,           // block_number
            &[u8],         // block_hash (unused now, kept for API compat)
            i32,           // tx_index
            i32,           // _header_deps_count
            i16,           // inputs_count
            i16,           // outputs_count
            i16,           // _cell_deps_count
            i16,           // _witnesses_count
            i16,           // _proposals_count
            i64,           // _total_output_capacity
            i64,           // _total_input_capacity
            i64,           // fee
            Option<i32>,   // tx_size
            Option<i64>,   // cycles
            bool,          // is_cellbase
            DateTime<Utc>, // timestamp
        )],
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if txs.is_empty() {
            return Ok(());
        }

        for tx in txs {
            let entry = TxIndexEntry {
                is_cellbase: tx.15,
                timestamp: tx.16.timestamp_millis(),
                inputs_count: tx.5,
                outputs_count: tx.6,
                fee: tx.12,
                tx_size: tx.13.ok_or_else(|| {
                    anyhow::anyhow!("missing tx_size for transaction 0x{}", hex::encode(tx.0))
                })?,
                cycles: tx.14,
            };
            batch.put_tx_index(tx.1, tx.3, &entry);
            batch.put_tx_hash_map(tx.0, tx.1, tx.3);
        }

        Ok(())
    }
}
