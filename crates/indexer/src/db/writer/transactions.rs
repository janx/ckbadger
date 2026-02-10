use anyhow::Result;
use chrono::{DateTime, Utc};

use super::BatchWriter;

impl BatchWriter {
    pub async fn insert_transactions_batch(
        &self,
        txs: &[(
            &[u8],
            i64,
            &[u8],
            i32,
            i32,
            i16,
            i16,
            i16,
            i16,
            i16,
            i64,
            i64,
            i64,
            Option<i32>,
            Option<i64>,
            bool,
            DateTime<Utc>,
        )],
    ) -> Result<()> {
        if txs.is_empty() {
            return Ok(());
        }

        let hashes: Vec<&[u8]> = txs.iter().map(|t| t.0).collect();
        let block_numbers: Vec<i64> = txs.iter().map(|t| t.1).collect();
        let tx_indices: Vec<i32> = txs.iter().map(|t| t.3).collect();
        let inputs_counts: Vec<i16> = txs.iter().map(|t| t.5).collect();
        let outputs_counts: Vec<i16> = txs.iter().map(|t| t.6).collect();
        let fees: Vec<i64> = txs.iter().map(|t| t.12).collect();
        let tx_sizes: Vec<Option<i32>> = txs.iter().map(|t| t.13).collect();
        let cycles: Vec<Option<i64>> = txs.iter().map(|t| t.14).collect();
        let is_cellbases: Vec<bool> = txs.iter().map(|t| t.15).collect();
        let timestamps: Vec<DateTime<Utc>> = txs.iter().map(|t| t.16).collect();

        // Write only to transactions_index (lightweight index table)
        sqlx::query(
            r#"
            INSERT INTO transactions_index (
                hash, block_number, tx_index, is_cellbase, timestamp,
                inputs_count, outputs_count, fee, tx_size, cycles
            )
            SELECT * FROM UNNEST(
                $1::bytea[], $2::bigint[], $3::int[], $4::bool[], $5::timestamptz[],
                $6::smallint[], $7::smallint[], $8::bigint[], $9::int[], $10::bigint[]
            )
            ON CONFLICT (block_number, hash) DO NOTHING
            "#,
        )
        .bind(&hashes)
        .bind(&block_numbers)
        .bind(&tx_indices)
        .bind(&is_cellbases)
        .bind(&timestamps)
        .bind(&inputs_counts)
        .bind(&outputs_counts)
        .bind(&fees)
        .bind(&tx_sizes)
        .bind(&cycles)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
