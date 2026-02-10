use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::db::CachedBlockHeader;
use crate::parser::block::ParsedBlock;

use super::BatchWriter;

impl BatchWriter {
    /// Insert a single block into blocks_index.
    pub async fn insert_block(&self, block: &ParsedBlock, _total_difficulty: i64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO blocks_index (
                number, hash, timestamp, tx_count, proposals_count, uncles_count,
                epoch_number, epoch_index, epoch_length, compact_target, dao
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (number) DO UPDATE SET
                hash = EXCLUDED.hash,
                timestamp = EXCLUDED.timestamp,
                tx_count = EXCLUDED.tx_count
            "#,
        )
        .bind(block.number)
        .bind(&block.hash)
        .bind(block.timestamp)
        .bind(block.transactions_count)
        .bind(block.proposals_count)
        .bind(block.uncles_count)
        .bind(block.epoch_number)
        .bind(block.epoch_index)
        .bind(block.epoch_length)
        .bind(block.compact_target)
        .bind(&block.dao)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert multiple blocks into blocks_index in a single batch operation.
    pub async fn insert_blocks_batch(&self, blocks: &[&ParsedBlock]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let numbers: Vec<i64> = blocks.iter().map(|b| b.number).collect();
        let hashes: Vec<&[u8]> = blocks.iter().map(|b| b.hash.as_slice()).collect();
        let timestamps: Vec<DateTime<Utc>> = blocks.iter().map(|b| b.timestamp).collect();
        let transactions_counts: Vec<i32> = blocks.iter().map(|b| b.transactions_count).collect();
        let proposals_counts: Vec<i32> = blocks.iter().map(|b| b.proposals_count).collect();
        let uncles_counts: Vec<i32> = blocks.iter().map(|b| b.uncles_count).collect();
        let epoch_numbers: Vec<i64> = blocks.iter().map(|b| b.epoch_number).collect();
        let epoch_indices: Vec<i32> = blocks.iter().map(|b| b.epoch_index).collect();
        let epoch_lengths: Vec<i32> = blocks.iter().map(|b| b.epoch_length).collect();
        let compact_targets: Vec<i64> = blocks.iter().map(|b| b.compact_target).collect();
        let daos: Vec<&[u8]> = blocks.iter().map(|b| b.dao.as_slice()).collect();

        sqlx::query(
            r#"
            INSERT INTO blocks_index (
                number, hash, timestamp, tx_count, proposals_count, uncles_count,
                epoch_number, epoch_index, epoch_length, compact_target, dao
            )
            SELECT * FROM UNNEST(
                $1::bigint[], $2::bytea[], $3::timestamptz[], $4::int[], $5::int[], $6::int[],
                $7::bigint[], $8::int[], $9::int[], $10::bigint[], $11::bytea[]
            )
            ON CONFLICT (number) DO UPDATE SET
                hash = EXCLUDED.hash,
                timestamp = EXCLUDED.timestamp,
                tx_count = EXCLUDED.tx_count
            "#,
        )
        .bind(&numbers)
        .bind(&hashes)
        .bind(&timestamps)
        .bind(&transactions_counts)
        .bind(&proposals_counts)
        .bind(&uncles_counts)
        .bind(&epoch_numbers)
        .bind(&epoch_indices)
        .bind(&epoch_lengths)
        .bind(&compact_targets)
        .bind(&daos)
        .execute(&self.pool)
        .await?;

        if let Some(store) = &self.live_cell_store {
            for block in blocks {
                store.insert_block_header(
                    block.number,
                    CachedBlockHeader {
                        hash: block.hash.clone(),
                        timestamp: block.timestamp.timestamp_millis(),
                        epoch_number: block.epoch_number,
                        epoch_index: block.epoch_index,
                        epoch_length: block.epoch_length,
                        dao: block.dao.clone(),
                        transactions_count: block.transactions_count,
                    },
                );
            }
        }

        Ok(())
    }
}
