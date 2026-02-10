use anyhow::Result;
use tracing::{info, warn};

use ckbadger_store::keys;

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

    pub fn find_last_consistent_block(&self) -> Result<Option<i64>> {
        // Get max block from block_headers CF
        let max_block = self.store.get_sync_tip_block()?.map(|(num, _)| num);

        // Get max block from tx_index CF
        let iter = self
            .store
            .iterator_cf(self.store.cf_tx_index(), rocksdb::IteratorMode::End);
        let mut max_tx_block: Option<i64> = None;
        for item in iter.flatten().take(1) {
            let (key, _) = item;
            if key.len() >= 8 {
                max_tx_block = Some(keys::decode_block_num(&key[..8]));
            }
        }

        match (max_block, max_tx_block) {
            (Some(mb), Some(mtb)) => {
                if mb > mtb {
                    warn!(
                        "Data inconsistency detected: blocks up to {} but transactions only up to {}",
                        mb, mtb
                    );
                    Ok(Some(mtb))
                } else {
                    Ok(Some(mb))
                }
            }
            (Some(mb), None) => {
                warn!(
                    "Data inconsistency: blocks exist up to {} but no transactions found",
                    mb
                );
                Ok(Some(-1))
            }
            (None, _) => Ok(None),
        }
    }

    pub fn init_sync_start(&self, start_block: i64, is_bulk_sync: bool) -> Result<()> {
        let next_block = start_block + 1;
        info!(
            "Cleaning up any partial data from block {} onwards before sync start",
            next_block
        );

        // Use the store's rollback mechanism to clean up everything
        self.store.rollback_to_block(start_block)?;

        if let Some(cache) = &self.cache_invalidator {
            let cache = cache.clone();
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    cache
                        .update_sync_status(|status| {
                            status.init_sync_start(start_block, is_bulk_sync);
                        })
                        .await;
                });
            });
        }

        info!(
            "Partial data cleanup complete, starting sync from block {}",
            next_block
        );
        Ok(())
    }

    pub fn cleanup_batch_range(&self, start_block: i64, end_block: i64) -> Result<()> {
        info!(
            "Cleaning up partial batch data for blocks {} to {}",
            start_block, end_block
        );

        // For range cleanup, we rollback to the block before the range
        // then the caller will re-sync from start_block
        self.store.rollback_to_block(start_block - 1)?;

        info!(
            "Batch cleanup complete for blocks {} to {}",
            start_block, end_block
        );
        Ok(())
    }
}
