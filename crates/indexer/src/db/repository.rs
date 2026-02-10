use std::sync::Arc;

use anyhow::Result;

use ckbadger_store::CkbadgerStore;

use crate::cache::CacheInvalidator;

// Re-export DeepForkInfo from ckbadger_store
pub use ckbadger_store::types::DeepForkInfo;

#[derive(Clone)]
pub struct Repository {
    store: Arc<CkbadgerStore>,
    cache_invalidator: Option<CacheInvalidator>,
}

impl Repository {
    pub fn new(store: Arc<CkbadgerStore>) -> Self {
        Self {
            store,
            cache_invalidator: None,
        }
    }

    pub fn with_cache(store: Arc<CkbadgerStore>, cache_invalidator: CacheInvalidator) -> Self {
        Self {
            store,
            cache_invalidator: Some(cache_invalidator),
        }
    }

    pub fn store(&self) -> &Arc<CkbadgerStore> {
        &self.store
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

        // Fallback to store
        let (num, hash) = self.store.get_sync_tip()?;
        if num > 0 {
            Ok((num, hash))
        } else {
            Ok((0, None))
        }
    }

    pub async fn update_sync_tip(
        &self,
        block_number: i64,
        block_hash: &[u8],
        tx_count_delta: i64,
    ) -> Result<()> {
        // Update store
        self.store
            .update_sync_tip(block_number, block_hash, tx_count_delta)?;

        // Update cache
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

    pub fn get_block_hash_at_height(&self, height: i64) -> Result<Option<Vec<u8>>> {
        let header = self.store.get_block_header(height)?;
        Ok(header.map(|h| h.hash))
    }

    pub fn get_block_transaction_count(&self, block_number: i64) -> Result<Option<i32>> {
        let header = self.store.get_block_header(block_number)?;
        Ok(header.map(|h| h.transactions_count))
    }

    pub fn has_unresolved_deep_fork(&self) -> Result<bool> {
        self.store.has_unresolved_deep_fork()
    }

    pub fn get_deep_fork_info(&self) -> Result<Option<DeepForkInfo>> {
        self.store.get_deep_fork_info()
    }
}
