use anyhow::Result;
use chrono::Utc;
use tracing::info;

use super::BatchWriter;

impl BatchWriter {
    pub async fn record_deep_fork(
        &self,
        fork_point: i64,
        fork_hash: &[u8],
        db_tip: i64,
        db_tip_hash: &[u8],
        chain_tip: i64,
        chain_tip_hash: &[u8],
        depth: i64,
    ) -> Result<i32> {
        let event_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO reorg_events (
                fork_point_number, fork_point_hash,
                old_tip_number, old_tip_hash,
                new_tip_number, new_tip_hash,
                depth, event_type
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'deep')
            RETURNING id
            "#,
        )
        .bind(fork_point)
        .bind(fork_hash)
        .bind(db_tip)
        .bind(db_tip_hash)
        .bind(chain_tip)
        .bind(chain_tip_hash)
        .bind(depth as i32)
        .fetch_one(&self.pool)
        .await?;

        sqlx::query(
            r#"
            UPDATE sync_status SET
                deep_fork_detected = TRUE,
                deep_fork_at = NOW(),
                deep_fork_db_tip = $1,
                deep_fork_db_tip_hash = $2,
                deep_fork_chain_tip = $3,
                deep_fork_chain_tip_hash = $4,
                deep_fork_depth = $5,
                deep_fork_fork_point = $6
            WHERE id = 1
            "#,
        )
        .bind(db_tip)
        .bind(db_tip_hash)
        .bind(chain_tip)
        .bind(chain_tip_hash)
        .bind(depth as i32)
        .bind(fork_point)
        .execute(&self.pool)
        .await?;

        Ok(event_id)
    }

    pub async fn execute_reorg(
        &self,
        fork_point: i64,
        fork_hash: &[u8],
        old_tip: i64,
        old_tip_hash: &[u8],
        new_tip: i64,
        new_tip_hash: &[u8],
    ) -> Result<ReorgResult> {
        let mut tx = self.pool.begin().await?;
        let rollback_from = fork_point + 1;
        let depth = (old_tip - fork_point) as i32;

        let event_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO reorg_events (
                fork_point_number, fork_point_hash,
                old_tip_number, old_tip_hash,
                new_tip_number, new_tip_hash,
                depth, event_type
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'auto')
            RETURNING id
            "#,
        )
        .bind(fork_point)
        .bind(fork_hash)
        .bind(old_tip)
        .bind(old_tip_hash)
        .bind(new_tip)
        .bind(new_tip_hash)
        .bind(depth)
        .fetch_one(&mut *tx)
        .await?;

        let orphaned_blocks: i64 = sqlx::query_scalar(
            r#"
            WITH archived AS (
                INSERT INTO orphaned_blocks (
                    reorg_event_id, number, hash, parent_hash,
                    timestamp, transactions_count, miner_lock_hash
                )
                SELECT $1, number, hash, parent_hash,
                       timestamp, transactions_count, miner_lock_hash
                FROM blocks
                WHERE number >= $2
                RETURNING 1
            )
            SELECT COUNT(*) FROM archived
            "#,
        )
        .bind(event_id)
        .bind(rollback_from)
        .fetch_one(&mut *tx)
        .await?;

        let orphaned_txs: i64 = sqlx::query_scalar(
            r#"
            WITH archived AS (
                INSERT INTO orphaned_transactions (
                    reorg_event_id, hash, block_number, block_hash,
                    tx_index, inputs_count, outputs_count, total_capacity
                )
                SELECT $1, t.hash, t.block_number, b.hash,
                       t.tx_index, t.inputs_count, t.outputs_count, t.total_output_capacity
                FROM transactions t
                JOIN blocks b ON t.block_number = b.number
                WHERE t.block_number >= $2
                RETURNING 1
            )
            SELECT COUNT(*) FROM archived
            "#,
        )
        .bind(event_id)
        .bind(rollback_from)
        .fetch_one(&mut *tx)
        .await?;

        // Rollback statistics before deleting blocks/cells (need the data for calculation)
        self.rollback_statistics(&mut tx, rollback_from).await?;

        sqlx::query(
            r#"
            UPDATE cells SET
                status = 0,
                consumed_at_block = NULL,
                consumed_by_tx = NULL,
                consumed_at_index = NULL
            WHERE consumed_at_block >= $1
            "#,
        )
        .bind(rollback_from)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM cells WHERE created_at_block >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM activities WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM cell_flows WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM transaction_inputs WHERE tx_block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM transaction_cell_deps WHERE tx_block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM transactions WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM tx_block_map WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM block_proposals WHERE block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM blocks WHERE number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            r#"
            UPDATE dao_deposits SET
                withdraw_request_tx = NULL,
                withdraw_request_block = NULL,
                withdraw_request_timestamp = NULL,
                withdraw_request_ar = NULL,
                status = 0
            WHERE withdraw_request_block >= $1 AND status = 1
            "#,
        )
        .bind(rollback_from)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE dao_deposits SET
                withdraw_tx = NULL,
                withdraw_block = NULL,
                withdraw_timestamp = NULL,
                compensation = NULL,
                status = CASE 
                    WHEN withdraw_request_tx IS NOT NULL THEN 1 
                    ELSE 0 
                END
            WHERE withdraw_block >= $1
            "#,
        )
        .bind(rollback_from)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM dao_deposits WHERE deposit_block_number >= $1")
            .bind(rollback_from)
            .execute(&mut *tx)
            .await?;

        self.rollback_token_statistics(&mut tx, rollback_from)
            .await?;

        sqlx::query(
            r#"
            UPDATE sync_status SET
                last_reorg_at = NOW(),
                last_reorg_depth = $1,
                deep_fork_detected = FALSE,
                deep_fork_at = NULL,
                deep_fork_db_tip = NULL,
                deep_fork_db_tip_hash = NULL,
                deep_fork_chain_tip = NULL,
                deep_fork_chain_tip_hash = NULL,
                deep_fork_depth = NULL,
                deep_fork_fork_point = NULL
            WHERE id = 1
            "#,
        )
        .bind(depth)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE reorg_events SET
                orphaned_blocks_count = $2,
                orphaned_txs_count = $3
            WHERE id = $1
            "#,
        )
        .bind(event_id)
        .bind(orphaned_blocks as i32)
        .bind(orphaned_txs as i32)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        if let Some(cache) = &self.cache_invalidator {
            let hash_hex = format!("0x{}", hex::encode(fork_hash));
            cache
                .update_sync_status(|status| {
                    status.tip_block_number = fork_point;
                    status.tip_block_hash = hash_hex;
                    status.last_synced_at = Utc::now().timestamp();
                })
                .await;
        }

        info!(
            "Reorg completed: fork_point={}, depth={}, orphaned_blocks={}, orphaned_txs={}",
            fork_point, depth, orphaned_blocks, orphaned_txs
        );

        Ok(ReorgResult {
            event_id,
            depth,
            orphaned_blocks: orphaned_blocks as i32,
            orphaned_txs: orphaned_txs as i32,
        })
    }

    async fn rollback_token_statistics(
        &self,
        _tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        _rollback_from: i64,
    ) -> Result<()> {
        Ok(())
    }

    async fn rollback_statistics(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        rollback_from: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            WITH rollback_hourly AS (
                SELECT 
                    date_trunc('hour', timestamp) AS hour,
                    COUNT(*)::int AS blocks_count,
                    SUM(transactions_count)::int AS transactions_count
                FROM blocks 
                WHERE number >= $1
                GROUP BY date_trunc('hour', timestamp)
            )
            UPDATE hourly_statistics h SET 
                blocks_count = GREATEST(h.blocks_count - r.blocks_count, 0),
                transactions_count = GREATEST(h.transactions_count - r.transactions_count, 0),
                updated_at = NOW()
            FROM rollback_hourly r 
            WHERE h.hour = r.hour
            "#,
        )
        .bind(rollback_from)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            WITH rollback_hourly_cells AS (
                SELECT 
                    date_trunc('hour', b.timestamp) AS hour,
                    COUNT(*) FILTER (WHERE c.created_at_block >= $1)::int AS cells_created,
                    COUNT(*) FILTER (WHERE c.consumed_at_block >= $1)::int AS cells_consumed
                FROM blocks b
                LEFT JOIN cells c ON c.created_at_block = b.number OR c.consumed_at_block = b.number
                WHERE b.number >= $1
                GROUP BY date_trunc('hour', b.timestamp)
            )
            UPDATE hourly_statistics h SET 
                cells_created = GREATEST(h.cells_created - COALESCE(r.cells_created, 0), 0),
                cells_consumed = GREATEST(h.cells_consumed - COALESCE(r.cells_consumed, 0), 0)
            FROM rollback_hourly_cells r 
            WHERE h.hour = r.hour
            "#,
        )
        .bind(rollback_from)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            WITH rollback_daily AS (
                SELECT 
                    timestamp::date AS date,
                    COUNT(*)::int AS blocks_count,
                    SUM(transactions_count)::int AS transactions_count
                FROM blocks 
                WHERE number >= $1
                GROUP BY timestamp::date
            )
            UPDATE daily_statistics d SET 
                blocks_count = GREATEST(d.blocks_count - r.blocks_count, 0),
                transactions_count = GREATEST(d.transactions_count - r.transactions_count, 0),
                updated_at = NOW()
            FROM rollback_daily r 
            WHERE d.date = r.date
            "#,
        )
        .bind(rollback_from)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            WITH rollback_daily_cells AS (
                SELECT 
                    b.timestamp::date AS date,
                    COUNT(*) FILTER (WHERE c.created_at_block >= $1)::int AS cells_created,
                    COUNT(*) FILTER (WHERE c.consumed_at_block >= $1)::int AS cells_consumed,
                    COALESCE(SUM(c.data_size) FILTER (WHERE c.created_at_block >= $1), 0)::bigint AS data_created,
                    COALESCE(SUM(c.data_size) FILTER (WHERE c.consumed_at_block >= $1), 0)::bigint AS data_consumed
                FROM blocks b
                LEFT JOIN cells c ON c.created_at_block = b.number OR c.consumed_at_block = b.number
                WHERE b.number >= $1
                GROUP BY b.timestamp::date
            )
            UPDATE daily_statistics d SET 
                cells_created = GREATEST(d.cells_created - COALESCE(r.cells_created, 0), 0),
                cells_consumed = GREATEST(d.cells_consumed - COALESCE(r.cells_consumed, 0), 0),
                total_live_cells = d.total_live_cells - COALESCE(r.cells_created, 0) + COALESCE(r.cells_consumed, 0),
                total_data_size = d.total_data_size - COALESCE(r.data_created, 0) + COALESCE(r.data_consumed, 0)
            FROM rollback_daily_cells r 
            WHERE d.date = r.date
            "#,
        )
        .bind(rollback_from)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            WITH rollback_miner AS (
                SELECT 
                    timestamp::date AS date,
                    miner_lock_hash,
                    COUNT(*)::int AS blocks_count
                FROM blocks 
                WHERE number >= $1 AND miner_lock_hash IS NOT NULL
                GROUP BY timestamp::date, miner_lock_hash
            )
            UPDATE miner_statistics m SET 
                blocks_count = GREATEST(m.blocks_count - r.blocks_count, 0)
            FROM rollback_miner r 
            WHERE m.date = r.date AND m.miner_lock_hash = r.miner_lock_hash
            "#,
        )
        .bind(rollback_from)
        .execute(&mut **tx)
        .await?;

        info!(
            "Statistics rollback completed for blocks >= {}",
            rollback_from
        );
        Ok(())
    }

    pub async fn clear_deep_fork_flag(&self) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sync_status SET
                deep_fork_detected = FALSE,
                deep_fork_at = NULL,
                deep_fork_db_tip = NULL,
                deep_fork_db_tip_hash = NULL,
                deep_fork_chain_tip = NULL,
                deep_fork_chain_tip_hash = NULL,
                deep_fork_depth = NULL,
                deep_fork_fork_point = NULL
            WHERE id = 1
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn resolve_deep_fork(
        &self,
        action: &str,
        resolved_by: Option<&str>,
        notes: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE reorg_events SET
                event_type = 'resolved',
                resolved_at = NOW(),
                resolved_by = $2,
                resolution_action = $1,
                resolution_notes = $3
            WHERE event_type = 'deep' AND resolved_at IS NULL
            "#,
        )
        .bind(action)
        .bind(resolved_by)
        .bind(notes)
        .execute(&self.pool)
        .await?;

        self.clear_deep_fork_flag().await?;

        Ok(())
    }
}

pub struct ReorgResult {
    pub event_id: i32,
    pub depth: i32,
    pub orphaned_blocks: i32,
    pub orphaned_txs: i32,
}
