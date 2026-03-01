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
        // Authoritative source for sync progression is the persisted block_headers CF.
        // Cache/Redis can be stale and must never drive writer/fetcher start height.
        if let Some((num, header)) = self.store.get_sync_tip_block()? {
            return Ok((num, Some(header.hash)));
        }

        // Legacy fallback for older stores that may only have sync_status.
        let (num, hash) = self.store.get_sync_tip()?;
        if num > 0 || hash.is_some() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::CachedBlockHeader;
    use tempfile::TempDir;

    fn make_header(block_number: i64) -> CachedBlockHeader {
        let mut hash = vec![0u8; 32];
        hash[..8].copy_from_slice(&block_number.to_le_bytes());
        CachedBlockHeader {
            block_number,
            hash,
            timestamp: block_number,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0u8; 32],
            transactions_count: 1,
        }
    }

    struct TestCtx {
        _dir: TempDir,
        repo: Repository,
        store: Arc<CkbadgerStore>,
    }

    fn setup() -> TestCtx {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let repo = Repository::new(store.clone());
        TestCtx {
            _dir: dir,
            repo,
            store,
        }
    }

    #[tokio::test]
    async fn test_get_sync_tip_uses_block_headers_over_sync_status() {
        let ctx = setup();

        let mut batch = StoreBatch::new(&ctx.store);
        for n in 0..=10 {
            batch.put_block_header(n, &make_header(n));
        }
        batch.commit().unwrap();

        // Inject a stale/ahead sync_status tip that should be ignored.
        ctx.store
            .update_sync_status(|status| {
                status.tip_block_number = 1000;
                status.tip_block_hash = vec![0xAB; 32];
            })
            .unwrap();

        let (tip, hash) = ctx.repo.get_sync_tip().await.unwrap();
        assert_eq!(tip, 10);
        assert_eq!(hash, Some(make_header(10).hash));
    }

    #[tokio::test]
    async fn test_get_sync_tip_falls_back_to_sync_status_when_no_headers() {
        let ctx = setup();

        ctx.store
            .update_sync_status(|status| {
                status.tip_block_number = 7;
                status.tip_block_hash = vec![0xCD; 32];
            })
            .unwrap();

        let (tip, hash) = ctx.repo.get_sync_tip().await.unwrap();
        assert_eq!(tip, 7);
        assert_eq!(hash, Some(vec![0xCD; 32]));
    }
}
