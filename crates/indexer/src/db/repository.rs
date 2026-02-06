#![allow(dead_code)]

use anyhow::Result;

use crate::cache::CacheInvalidator;
use crate::db::DbPool;

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
    pool: DbPool,
    cache_invalidator: Option<CacheInvalidator>,
}

impl Repository {
    pub fn new(pool: DbPool) -> Self {
        Self {
            pool,
            cache_invalidator: None,
        }
    }

    pub fn with_cache(pool: DbPool, cache_invalidator: CacheInvalidator) -> Self {
        Self {
            pool,
            cache_invalidator: Some(cache_invalidator),
        }
    }

    pub fn pool(&self) -> &DbPool {
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

        Ok((0, None))
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

    pub async fn get_block_hash_at_height(&self, _height: i64) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    pub async fn delete_block(&self, _block_number: i64) -> Result<()> {
        Ok(())
    }

    pub async fn restore_cells_consumed_at_block(&self, _block_number: i64) -> Result<()> {
        Ok(())
    }

    pub async fn delete_cells_created_at_block(&self, _block_number: i64) -> Result<()> {
        Ok(())
    }

    pub async fn get_block_transaction_count(&self, _block_number: i64) -> Result<Option<i32>> {
        Ok(None)
    }

    pub async fn has_unresolved_deep_fork(&self) -> Result<bool> {
        Ok(false)
    }

    pub async fn get_deep_fork_info(&self) -> Result<Option<DeepForkInfo>> {
        Ok(None)
    }
}
