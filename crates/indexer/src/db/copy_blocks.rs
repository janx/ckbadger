use anyhow::Result;
use bytes::Bytes;
use tokio_postgres::Client;

use crate::db::copy_format::BinaryCopyBuffer;
use crate::parser::block::ParsedBlock;

/// Blocks table has 19 columns for COPY (excluding extension, miner_lock_hash, miner_message, reward which are nullable/optional):
/// number, hash, parent_hash, timestamp, version, compact_target,
/// transactions_count, proposals_count, uncles_count,
/// epoch_number, epoch_index, epoch_length,
/// dao, nonce, extra_hash, proposals_hash, transactions_root, uncles_hash,
/// total_difficulty
const BLOCKS_COLUMN_COUNT: i16 = 19;

pub struct CopyBlocksWriter {
    buffer: BinaryCopyBuffer,
    row_count: usize,
}

impl CopyBlocksWriter {
    pub fn new() -> Self {
        Self {
            buffer: BinaryCopyBuffer::new(BLOCKS_COLUMN_COUNT),
            row_count: 0,
        }
    }

    pub fn add_block(&mut self, block: &ParsedBlock, total_difficulty: i64) {
        self.buffer.start_row();

        // number BIGINT NOT NULL
        self.buffer.write_i64(block.number);
        // hash BYTEA NOT NULL
        self.buffer.write_bytea(&block.hash);
        // parent_hash BYTEA NOT NULL
        self.buffer.write_bytea(&block.parent_hash);
        // timestamp TIMESTAMPTZ NOT NULL
        self.buffer.write_timestamptz(block.timestamp);
        // version INTEGER NOT NULL
        self.buffer.write_i32(block.version);
        // compact_target BIGINT NOT NULL
        self.buffer.write_i64(block.compact_target);
        // transactions_count INTEGER NOT NULL
        self.buffer.write_i32(block.transactions_count);
        // proposals_count INTEGER NOT NULL
        self.buffer.write_i32(block.proposals_count);
        // uncles_count INTEGER NOT NULL
        self.buffer.write_i32(block.uncles_count);
        // epoch_number BIGINT NOT NULL
        self.buffer.write_i64(block.epoch_number);
        // epoch_index INTEGER NOT NULL
        self.buffer.write_i32(block.epoch_index);
        // epoch_length INTEGER NOT NULL
        self.buffer.write_i32(block.epoch_length);
        // dao BYTEA NOT NULL
        self.buffer.write_bytea(&block.dao);
        // nonce BYTEA NOT NULL
        self.buffer.write_bytea(&block.nonce);
        // extra_hash BYTEA NOT NULL
        self.buffer.write_bytea(&block.extra_hash);
        // proposals_hash BYTEA NOT NULL
        self.buffer.write_bytea(&block.proposals_hash);
        // transactions_root BYTEA NOT NULL
        self.buffer.write_bytea(&block.transactions_root);
        // uncles_hash BYTEA NOT NULL
        self.buffer.write_bytea(&block.uncles_hash);
        // total_difficulty NUMERIC(40,0) NOT NULL (stored as i64)
        self.buffer.write_i64(total_difficulty);

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

impl Default for CopyBlocksWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute COPY for blocks
///
/// # Arguments
/// * `client` - tokio-postgres client from CopyPoolManager
/// * `blocks` - Slice of (ParsedBlock, total_difficulty) tuples
///
/// # Returns
/// Number of rows inserted
pub async fn copy_blocks(client: &Client, blocks: &[(&ParsedBlock, i64)]) -> Result<u64> {
    if blocks.is_empty() {
        return Ok(0);
    }

    let mut writer = CopyBlocksWriter::new();
    for (block, total_difficulty) in blocks {
        writer.add_block(block, *total_difficulty);
    }

    let data = writer.finish();

    let sink = client
        .copy_in("COPY blocks (number, hash, parent_hash, timestamp, version, compact_target, transactions_count, proposals_count, uncles_count, epoch_number, epoch_index, epoch_length, dao, nonce, extra_hash, proposals_hash, transactions_root, uncles_hash, total_difficulty) FROM STDIN WITH (FORMAT BINARY)")
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
    fn test_copy_blocks_writer_creates_buffer() {
        let writer = CopyBlocksWriter::new();
        let data = writer.finish();
        // Should have header (19) + trailer (2) = 21 bytes minimum
        assert!(data.len() >= 21);
    }

    #[test]
    fn test_copy_blocks_writer_is_empty() {
        let writer = CopyBlocksWriter::new();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }

    #[test]
    fn test_copy_blocks_writer_add_block() {
        let mut writer = CopyBlocksWriter::new();
        let block = create_test_block(12345);

        writer.add_block(&block, 0);

        assert!(!writer.is_empty());
        assert_eq!(writer.row_count(), 1);

        let data = writer.finish();
        // Header (19) + column_count (2) + data + trailer (2)
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_blocks_writer_multiple_blocks() {
        let mut writer = CopyBlocksWriter::new();
        let block1 = create_test_block(12345);
        let block2 = create_test_block(12346);

        writer.add_block(&block1, 100);
        writer.add_block(&block2, 200);

        assert_eq!(writer.row_count(), 2);

        let data = writer.finish();
        assert!(data.len() > 21);
    }

    #[test]
    fn test_copy_blocks_writer_default() {
        let writer = CopyBlocksWriter::default();
        assert!(writer.is_empty());
        assert_eq!(writer.row_count(), 0);
    }

    #[test]
    fn test_copy_blocks_writer_different_difficulties() {
        let mut writer = CopyBlocksWriter::new();
        let block = create_test_block(1);

        writer.add_block(&block, i64::MAX);

        assert_eq!(writer.row_count(), 1);
        let data = writer.finish();
        assert!(data.len() > 21);
    }
}
