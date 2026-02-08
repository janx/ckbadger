use anyhow::Result;
use sqlx::PgPool;

use crate::cache::CacheInvalidator;

pub struct DeepForkInfo {
    pub db_tip: i64,
    pub db_tip_hash: Vec<u8>,
    pub chain_tip: i64,
    pub chain_tip_hash: Vec<u8>,
    pub depth: i32,
    pub fork_point: i64,
}

#[derive(Clone)]
pub struct Repository {
    pool: PgPool,
    cache_invalidator: Option<CacheInvalidator>,
}

impl Repository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            cache_invalidator: None,
        }
    }

    pub fn with_cache(pool: PgPool, cache_invalidator: CacheInvalidator) -> Self {
        Self {
            pool,
            cache_invalidator: Some(cache_invalidator),
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn get_sync_tip(&self) -> Result<(i64, Option<Vec<u8>>)> {
        if let Some(cache) = &self.cache_invalidator {
            if let Some(status) = cache.get_sync_status().await {
                if status.tip_block_number > 0 {
                    let hash = if status.tip_block_hash.is_empty() {
                        None
                    } else {
                        hex::decode(status.tip_block_hash.trim_start_matches("0x")).ok()
                    };
                    return Ok((status.tip_block_number, hash));
                }
            }
        }

        let row = sqlx::query_as::<_, (Option<i64>, Option<Vec<u8>>)>(
            "SELECT number, hash FROM blocks ORDER BY number DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((Some(num), hash)) => Ok((num, hash)),
            _ => Ok((0, None)),
        }
    }

    pub async fn update_sync_tip(
        &self,
        block_number: i64,
        block_hash: &[u8],
        tx_count_delta: i64,
    ) -> Result<()> {
        if let Some(cache) = &self.cache_invalidator {
            let hash_hex = format!("0x{}", hex::encode(block_hash));
            cache
                .update_sync_status(|status| {
                    status.tip_block_number = block_number;
                    status.tip_block_hash = hash_hex;
                    status.total_transactions += tx_count_delta;
                    status.last_synced_at = chrono::Utc::now().timestamp();
                })
                .await;
        }
        Ok(())
    }

    pub async fn get_block_hash_at_height(&self, height: i64) -> Result<Option<Vec<u8>>> {
        let row = sqlx::query_as::<_, (Vec<u8>,)>("SELECT hash FROM blocks WHERE number = $1")
            .bind(height)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(hash,)| hash))
    }

    pub async fn delete_block(&self, block_number: i64) -> Result<()> {
        sqlx::query("DELETE FROM blocks WHERE number = $1")
            .bind(block_number)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn restore_cells_consumed_at_block(&self, block_number: i64) -> Result<()> {
        sqlx::query(
            "UPDATE cells SET status = 0, consumed_at_block = NULL, consumed_by_tx = NULL, consumed_at_index = NULL WHERE consumed_at_block = $1",
        )
        .bind(block_number)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_cells_created_at_block(&self, block_number: i64) -> Result<()> {
        sqlx::query("DELETE FROM cells WHERE created_at_block = $1")
            .bind(block_number)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_block_transaction_count(&self, block_number: i64) -> Result<Option<i32>> {
        let row =
            sqlx::query_as::<_, (i32,)>("SELECT transactions_count FROM blocks WHERE number = $1")
                .bind(block_number)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(count,)| count))
    }

    pub async fn has_unresolved_deep_fork(&self) -> Result<bool> {
        let row =
            sqlx::query_as::<_, (bool,)>("SELECT deep_fork_detected FROM sync_status WHERE id = 1")
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0)
    }

    pub async fn get_deep_fork_info(&self) -> Result<Option<DeepForkInfo>> {
        let row = sqlx::query_as::<_, (bool, Option<i64>, Option<Vec<u8>>, Option<i64>, Option<Vec<u8>>, Option<i32>, Option<i64>)>(
            r#"
            SELECT deep_fork_detected, deep_fork_db_tip, deep_fork_db_tip_hash,
                   deep_fork_chain_tip, deep_fork_chain_tip_hash, deep_fork_depth, deep_fork_fork_point
            FROM sync_status WHERE id = 1
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        if row.0 {
            if let (
                Some(db_tip),
                Some(db_tip_hash),
                Some(chain_tip),
                Some(chain_tip_hash),
                Some(depth),
                Some(fork_point),
            ) = (row.1, row.2, row.3, row.4, row.5, row.6)
            {
                return Ok(Some(DeepForkInfo {
                    db_tip,
                    db_tip_hash,
                    chain_tip,
                    chain_tip_hash,
                    depth,
                    fork_point,
                }));
            }
        }
        Ok(None)
    }
}
