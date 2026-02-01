use anyhow::Result;
use std::collections::HashMap;

use super::BatchWriter;

impl BatchWriter {
    pub async fn update_address_balances_batch(
        &self,
        changes: &HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])>,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let lock_hashes: Vec<&[u8]> = changes.keys().map(|k| k.as_slice()).collect();
        let balance_changes: Vec<i64> = changes.values().map(|(b, _, _, _, _, _)| *b).collect();
        let live_cell_changes: Vec<i32> = changes.values().map(|(_, l, _, _, _, _)| *l).collect();
        let total_cell_changes: Vec<i32> = changes.values().map(|(_, _, t, _, _, _)| *t).collect();
        let tx_counts: Vec<i64> = changes.values().map(|(_, _, _, c, _, _)| *c).collect();
        let block_numbers: Vec<i64> = changes.values().map(|(_, _, _, _, n, _)| *n).collect();
        let tx_hashes: Vec<&[u8]> = changes.values().map(|(_, _, _, _, _, h)| *h).collect();

        sqlx::query(
            r#"
            WITH input AS (
                SELECT lock_hash, balance_delta, live_delta, total_delta, tx_delta, block_num, tx_hash
                FROM UNNEST($1::bytea[], $2::bigint[], $3::int[], $4::int[], $5::bigint[], $6::bigint[], $7::bytea[])
                AS t(lock_hash, balance_delta, live_delta, total_delta, tx_delta, block_num, tx_hash)
            )
            MERGE INTO address_balances ab
            USING input i ON ab.lock_script_hash = i.lock_hash
            WHEN MATCHED THEN UPDATE SET
                balance = ab.balance + i.balance_delta,
                live_cells_count = GREATEST(0, ab.live_cells_count + i.live_delta),
                total_cells_count = ab.total_cells_count + i.total_delta,
                transactions_count = ab.transactions_count + i.tx_delta,
                last_activity_block = i.block_num,
                last_activity_tx = i.tx_hash,
                updated_at = NOW()
            WHEN NOT MATCHED THEN INSERT (
                lock_script_hash, balance, live_cells_count, total_cells_count,
                transactions_count, first_seen_block, first_seen_tx,
                last_activity_block, last_activity_tx
            ) VALUES (
                i.lock_hash,
                i.balance_delta,
                GREATEST(0, i.live_delta),
                GREATEST(0, i.total_delta),
                i.tx_delta,
                i.block_num,
                i.tx_hash,
                i.block_num,
                i.tx_hash
            )
            "#,
        )
        .bind(&lock_hashes)
        .bind(&balance_changes)
        .bind(&live_cell_changes)
        .bind(&total_cell_changes)
        .bind(&tx_counts)
        .bind(&block_numbers)
        .bind(&tx_hashes)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_script_usage_batch(
        &self,
        changes: &HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)>,
    ) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        let code_hashes: Vec<&[u8]> = changes.keys().map(|(h, _)| h.as_slice()).collect();
        let script_kinds: Vec<&str> = changes
            .keys()
            .map(|(_, is_type)| if *is_type { "type" } else { "lock" })
            .collect();
        let cells_count_deltas: Vec<i64> = changes.values().map(|(c, _, _, _)| *c).collect();
        let live_cells_deltas: Vec<i64> = changes.values().map(|(_, l, _, _)| *l).collect();
        let capacity_deltas: Vec<i64> = changes.values().map(|(_, _, c, _)| *c).collect();
        let live_capacity_deltas: Vec<i64> = changes.values().map(|(_, _, _, l)| *l).collect();

        sqlx::query(
            r#"
            INSERT INTO script_usage_stats (
                code_hash, script_kind, cells_count, live_cells_count, capacity_sum, live_capacity_sum
            )
            SELECT code_hash, script_kind, cells_delta, live_delta, cap_delta, live_cap_delta
            FROM UNNEST($1::bytea[], $2::text[], $3::bigint[], $4::bigint[], $5::numeric[], $6::numeric[])
            AS t(code_hash, script_kind, cells_delta, live_delta, cap_delta, live_cap_delta)
            ON CONFLICT (code_hash, script_kind) DO UPDATE SET
                cells_count = script_usage_stats.cells_count + EXCLUDED.cells_count,
                live_cells_count = script_usage_stats.live_cells_count + EXCLUDED.live_cells_count,
                capacity_sum = script_usage_stats.capacity_sum + EXCLUDED.capacity_sum,
                live_capacity_sum = script_usage_stats.live_capacity_sum + EXCLUDED.live_capacity_sum,
                updated_at = NOW()
            "#,
        )
        .bind(&code_hashes)
        .bind(&script_kinds)
        .bind(&cells_count_deltas)
        .bind(&live_cells_deltas)
        .bind(&capacity_deltas)
        .bind(&live_capacity_deltas)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
