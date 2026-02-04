use anyhow::Result;
use tracing::{info, warn};

use super::BatchWriter;

impl BatchWriter {
    pub async fn update_sync_status(
        &self,
        block_number: i64,
        block_hash: &[u8],
        tx_count: i64,
        cells_created: i64,
        cells_consumed: i64,
        new_addresses: i64,
        ema_rate: Option<f64>,
    ) -> Result<()> {
        if let Some(cache) = &self.cache_invalidator {
            let hash_hex = format!("0x{}", hex::encode(block_hash));
            cache
                .update_sync_status(|status| {
                    status.update_batch(
                        block_number,
                        &hash_hex,
                        tx_count,
                        cells_created,
                        cells_consumed,
                        new_addresses,
                        ema_rate,
                    );
                })
                .await;
        }
        Ok(())
    }

    pub async fn find_last_consistent_block(&self) -> Result<Option<i64>> {
        let row: Option<(Option<i64>, Option<i64>)> = sqlx::query_as(
            r#"
            SELECT 
                (SELECT MAX(number) FROM blocks) as max_block,
                (SELECT MAX(block_number) FROM transactions) as max_tx_block
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((Some(max_block), Some(max_tx_block))) => {
                if max_block > max_tx_block {
                    warn!(
                        "Data inconsistency detected: blocks up to {} but transactions only up to {}",
                        max_block, max_tx_block
                    );
                    Ok(Some(max_tx_block))
                } else {
                    Ok(Some(max_block))
                }
            }
            Some((Some(max_block), None)) => {
                warn!(
                    "Data inconsistency: blocks exist up to {} but no transactions found",
                    max_block
                );
                Ok(Some(-1))
            }
            Some((None, _)) => Ok(None),
            None => Ok(None),
        }
    }

    pub async fn init_sync_start(&self, start_block: i64, is_bulk_sync: bool) -> Result<()> {
        let next_block = start_block + 1;
        info!(
            "Cleaning up any partial data from block {} onwards before sync start",
            next_block
        );

        sqlx::query("DELETE FROM transaction_inputs WHERE tx_block_number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM transaction_cell_deps WHERE tx_block_number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM live_cells WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM cells WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM transactions WHERE block_number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM block_proposals WHERE block_number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM blocks WHERE number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        // Clean up derived/parsed tables (must match cleanup_batch_range)
        sqlx::query("DELETE FROM activities WHERE block_number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM udt_cells WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM dao_deposits WHERE deposit_block_number >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM spore_cells WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM spore_clusters WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM mnft_tokens WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM mnft_classes WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM mnft_issuers WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM dotbit_accounts WHERE created_at_block >= $1")
            .bind(next_block)
            .execute(&self.pool)
            .await?;

        if let Some(cache) = &self.cache_invalidator {
            cache
                .update_sync_status(|status| {
                    status.init_sync_start(start_block, is_bulk_sync);
                })
                .await;
        }

        info!(
            "Partial data cleanup complete, starting sync from block {}",
            next_block
        );
        Ok(())
    }

    pub async fn cleanup_batch_range(&self, start_block: i64, end_block: i64) -> Result<()> {
        info!(
            "Cleaning up partial batch data for blocks {} to {}",
            start_block, end_block
        );

        sqlx::query(
            "DELETE FROM transaction_inputs WHERE tx_block_number >= $1 AND tx_block_number <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM transaction_cell_deps WHERE tx_block_number >= $1 AND tx_block_number <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM live_cells WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query("DELETE FROM cells WHERE created_at_block >= $1 AND created_at_block <= $2")
            .bind(start_block)
            .bind(end_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM transactions WHERE block_number >= $1 AND block_number <= $2")
            .bind(start_block)
            .bind(end_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM block_proposals WHERE block_number >= $1 AND block_number <= $2")
            .bind(start_block)
            .bind(end_block)
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM activities WHERE block_number >= $1 AND block_number <= $2")
            .bind(start_block)
            .bind(end_block)
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "DELETE FROM udt_cells WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM dao_deposits WHERE deposit_block_number >= $1 AND deposit_block_number <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM spore_cells WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM spore_clusters WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM mnft_tokens WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM mnft_classes WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM mnft_issuers WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "DELETE FROM dotbit_accounts WHERE created_at_block >= $1 AND created_at_block <= $2",
        )
        .bind(start_block)
        .bind(end_block)
        .execute(&self.pool)
        .await?;

        info!(
            "Batch cleanup complete for blocks {} to {}",
            start_block, end_block
        );
        Ok(())
    }
}
