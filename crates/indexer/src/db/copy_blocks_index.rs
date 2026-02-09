use anyhow::Result;
use bytes::Bytes;
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;
use crate::parser::block::ParsedBlock;

/// blocks_index has 12 columns:
/// number, hash, timestamp, tx_count, proposals_count, uncles_count,
/// epoch_number, epoch_index, epoch_length, compact_target, miner_lock_hash, dao
const BLOCKS_INDEX_COLUMN_COUNT: i16 = 12;

pub struct CopyBlocksIndexWriter {
    buffer: BinaryCopyBuffer,
    row_count: usize,
}

impl CopyBlocksIndexWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(BLOCKS_INDEX_COLUMN_COUNT),
            row_count: 0,
        }
    }

    pub fn add_block(&mut self, block: &ParsedBlock, miner_lock_hash: Option<&[u8]>) {
        self.buffer.start_row();

        self.buffer.write_i64(block.number);
        self.buffer.write_bytea(&block.hash);
        self.buffer.write_timestamptz(block.timestamp);
        self.buffer.write_i32(block.transactions_count);
        self.buffer.write_i32(block.proposals_count);
        self.buffer.write_i32(block.uncles_count);
        self.buffer.write_i64(block.epoch_number);
        self.buffer.write_i32(block.epoch_index);
        self.buffer.write_i32(block.epoch_length);
        self.buffer.write_i64(block.compact_target);
        self.buffer.write_bytea_opt(miner_lock_hash);
        self.buffer.write_bytea(&block.dao);

        self.row_count += 1;
    }

    pub fn finish(self) -> Bytes {
        self.buffer.finish().freeze()
    }

    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }
}

impl Default for CopyBlocksIndexWriter {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn copy_blocks_index(
    client: &Client,
    blocks: &[(&ParsedBlock, Option<&[u8]>)],
) -> Result<u64> {
    if blocks.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyBlocksIndexWriter::new();
    for (block, miner_lock_hash) in blocks {
        writer.add_block(block, *miner_lock_hash);
    }

    let data = writer.finish();

    let sink = client
        .copy_in(
            "COPY blocks_index (number, hash, timestamp, tx_count, proposals_count, uncles_count, \
             epoch_number, epoch_index, epoch_length, compact_target, miner_lock_hash, dao) \
             FROM STDIN WITH (FORMAT BINARY)",
        )
        .await?;

    use futures::SinkExt;
    use std::pin::pin;

    let mut sink = pin!(sink);
    sink.send(data).await?;
    let rows = sink.finish().await?;

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn create_test_block(number: i64) -> ParsedBlock {
        ParsedBlock {
            number,
            hash: vec![0x01; 32],
            parent_hash: vec![0x02; 32],
            timestamp: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            version: 0,
            compact_target: 0x1a08a97e,
            transactions_count: 5,
            proposals_count: 10,
            uncles_count: 0,
            epoch_number: 100,
            epoch_index: 50,
            epoch_length: 1800,
            dao: vec![0x03; 32],
            nonce: vec![0x04; 16],
            extra_hash: vec![0x05; 32],
            proposals_hash: vec![0x06; 32],
            transactions_root: vec![0x07; 32],
            uncles_hash: vec![0x08; 32],
            proposals: vec![],
        }
    }

    #[test]
    fn test_copy_blocks_index_writer() {
        let mut writer = CopyBlocksIndexWriter::new();
        let block = create_test_block(12345);
        writer.add_block(&block, None);
        assert_eq!(writer.row_count(), 1);
        let data = writer.finish();
        assert!(data.len() > 21);
    }
}
