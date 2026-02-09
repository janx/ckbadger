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
        let block_hashes: Vec<&[u8]> = txs.iter().map(|t| t.2).collect();
        let tx_indices: Vec<i32> = txs.iter().map(|t| t.3).collect();
        let versions: Vec<i32> = txs.iter().map(|t| t.4).collect();
        let inputs_counts: Vec<i16> = txs.iter().map(|t| t.5).collect();
        let outputs_counts: Vec<i16> = txs.iter().map(|t| t.6).collect();
        let witnesses_counts: Vec<i16> = txs.iter().map(|t| t.7).collect();
        let cell_deps_counts: Vec<i16> = txs.iter().map(|t| t.8).collect();
        let header_deps_counts: Vec<i16> = txs.iter().map(|t| t.9).collect();
        let total_input_capacities: Vec<i64> = txs.iter().map(|t| t.10).collect();
        let total_output_capacities: Vec<i64> = txs.iter().map(|t| t.11).collect();
        let fees: Vec<i64> = txs.iter().map(|t| t.12).collect();
        let tx_sizes: Vec<Option<i32>> = txs.iter().map(|t| t.13).collect();
        let cycles: Vec<Option<i64>> = txs.iter().map(|t| t.14).collect();
        let is_cellbases: Vec<bool> = txs.iter().map(|t| t.15).collect();
        let timestamps: Vec<DateTime<Utc>> = txs.iter().map(|t| t.16).collect();

        sqlx::query(
            r#"
            INSERT INTO transactions (
                hash, block_number, block_hash, tx_index, version,
                inputs_count, outputs_count, witnesses_count, cell_deps_count, header_deps_count,
                total_input_capacity, total_output_capacity, fee, tx_size, cycles, is_cellbase, timestamp
            )
            SELECT * FROM UNNEST(
                $1::bytea[], $2::bigint[], $3::bytea[], $4::int[], $5::int[],
                $6::smallint[], $7::smallint[], $8::smallint[], $9::smallint[], $10::smallint[],
                $11::numeric[], $12::numeric[], $13::numeric[], $14::int[], $15::bigint[], $16::bool[], $17::timestamptz[]
            )
            ON CONFLICT (block_number, hash) DO NOTHING
            "#,
        )
        .bind(&hashes)
        .bind(&block_numbers)
        .bind(&block_hashes)
        .bind(&tx_indices)
        .bind(&versions)
        .bind(&inputs_counts)
        .bind(&outputs_counts)
        .bind(&witnesses_counts)
        .bind(&cell_deps_counts)
        .bind(&header_deps_counts)
        .bind(&total_input_capacities)
        .bind(&total_output_capacities)
        .bind(&fees)
        .bind(&tx_sizes)
        .bind(&cycles)
        .bind(&is_cellbases)
        .bind(&timestamps)
        .execute(&self.pool)
        .await?;

        // Also write to transactions_index (lightweight index table)
        sqlx::query(
            r#"
            INSERT INTO transactions_index (
                hash, block_number, tx_index, is_cellbase, timestamp,
                inputs_count, outputs_count, fee, cycles
            )
            SELECT * FROM UNNEST(
                $1::bytea[], $2::bigint[], $3::int[], $4::bool[], $5::timestamptz[],
                $6::smallint[], $7::smallint[], $8::bigint[], $9::bigint[]
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
        .bind(&cycles)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
