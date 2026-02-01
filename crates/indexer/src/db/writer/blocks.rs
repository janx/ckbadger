use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::db::CachedBlockHeader;
use crate::parser::block::ParsedBlock;

use super::BatchWriter;

impl BatchWriter {
    pub async fn insert_block(&self, block: &ParsedBlock, total_difficulty: i64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO blocks (
                number, hash, parent_hash, timestamp, version, compact_target,
                transactions_count, proposals_count, uncles_count,
                epoch_number, epoch_index, epoch_length,
                dao, nonce, extra_hash, proposals_hash, transactions_root, uncles_hash,
                total_difficulty
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
            ON CONFLICT (number) DO UPDATE SET
                hash = EXCLUDED.hash,
                parent_hash = EXCLUDED.parent_hash,
                timestamp = EXCLUDED.timestamp,
                transactions_count = EXCLUDED.transactions_count
            "#,
        )
        .bind(block.number)
        .bind(&block.hash)
        .bind(&block.parent_hash)
        .bind(block.timestamp)
        .bind(block.version)
        .bind(block.compact_target)
        .bind(block.transactions_count)
        .bind(block.proposals_count)
        .bind(block.uncles_count)
        .bind(block.epoch_number)
        .bind(block.epoch_index)
        .bind(block.epoch_length)
        .bind(&block.dao)
        .bind(&block.nonce)
        .bind(&block.extra_hash)
        .bind(&block.proposals_hash)
        .bind(&block.transactions_root)
        .bind(&block.uncles_hash)
        .bind(total_difficulty)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert multiple blocks in a single batch operation
    pub async fn insert_blocks_batch(&self, blocks: &[&ParsedBlock]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let numbers: Vec<i64> = blocks.iter().map(|b| b.number).collect();
        let hashes: Vec<&[u8]> = blocks.iter().map(|b| b.hash.as_slice()).collect();
        let parent_hashes: Vec<&[u8]> = blocks.iter().map(|b| b.parent_hash.as_slice()).collect();
        let timestamps: Vec<DateTime<Utc>> = blocks.iter().map(|b| b.timestamp).collect();
        let versions: Vec<i32> = blocks.iter().map(|b| b.version).collect();
        let compact_targets: Vec<i64> = blocks.iter().map(|b| b.compact_target).collect();
        let transactions_counts: Vec<i32> = blocks.iter().map(|b| b.transactions_count).collect();
        let proposals_counts: Vec<i32> = blocks.iter().map(|b| b.proposals_count).collect();
        let uncles_counts: Vec<i32> = blocks.iter().map(|b| b.uncles_count).collect();
        let epoch_numbers: Vec<i64> = blocks.iter().map(|b| b.epoch_number).collect();
        let epoch_indices: Vec<i32> = blocks.iter().map(|b| b.epoch_index).collect();
        let epoch_lengths: Vec<i32> = blocks.iter().map(|b| b.epoch_length).collect();
        let daos: Vec<&[u8]> = blocks.iter().map(|b| b.dao.as_slice()).collect();
        let nonces: Vec<&[u8]> = blocks.iter().map(|b| b.nonce.as_slice()).collect();
        let extra_hashes: Vec<&[u8]> = blocks.iter().map(|b| b.extra_hash.as_slice()).collect();
        let proposals_hashes: Vec<&[u8]> =
            blocks.iter().map(|b| b.proposals_hash.as_slice()).collect();
        let transactions_roots: Vec<&[u8]> = blocks
            .iter()
            .map(|b| b.transactions_root.as_slice())
            .collect();
        let uncles_hashes: Vec<&[u8]> = blocks.iter().map(|b| b.uncles_hash.as_slice()).collect();
        let total_difficulties: Vec<i64> = vec![0; blocks.len()];

        sqlx::query(
            r#"
            INSERT INTO blocks (
                number, hash, parent_hash, timestamp, version, compact_target,
                transactions_count, proposals_count, uncles_count,
                epoch_number, epoch_index, epoch_length,
                dao, nonce, extra_hash, proposals_hash, transactions_root, uncles_hash,
                total_difficulty
            )
            SELECT * FROM UNNEST(
                $1::bigint[], $2::bytea[], $3::bytea[], $4::timestamptz[], $5::int[], $6::bigint[],
                $7::int[], $8::int[], $9::int[],
                $10::bigint[], $11::int[], $12::int[],
                $13::bytea[], $14::bytea[], $15::bytea[], $16::bytea[], $17::bytea[], $18::bytea[],
                $19::bigint[]
            )
            ON CONFLICT (number) DO UPDATE SET
                hash = EXCLUDED.hash,
                parent_hash = EXCLUDED.parent_hash,
                timestamp = EXCLUDED.timestamp,
                transactions_count = EXCLUDED.transactions_count
            "#,
        )
        .bind(&numbers)
        .bind(&hashes)
        .bind(&parent_hashes)
        .bind(&timestamps)
        .bind(&versions)
        .bind(&compact_targets)
        .bind(&transactions_counts)
        .bind(&proposals_counts)
        .bind(&uncles_counts)
        .bind(&epoch_numbers)
        .bind(&epoch_indices)
        .bind(&epoch_lengths)
        .bind(&daos)
        .bind(&nonces)
        .bind(&extra_hashes)
        .bind(&proposals_hashes)
        .bind(&transactions_roots)
        .bind(&uncles_hashes)
        .bind(&total_difficulties)
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
