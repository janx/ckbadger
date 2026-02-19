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
        if self.has_partial_data_after_block(start_block)? {
            info!(
                "Cleaning up any partial data from block {} onwards before sync start",
                next_block
            );

            // Use the store's rollback mechanism to clean up everything
            self.store.rollback_to_block(start_block)?;
            info!(
                "Partial data cleanup complete, starting sync from block {}",
                next_block
            );
        } else {
            info!(
                "No partial data detected after block {}, skipping startup rollback",
                start_block
            );
        }

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

    fn has_partial_data_after_block(&self, start_block: i64) -> Result<bool> {
        let start_key = keys::encode_block_num(start_block + 1);

        let header_iter = self.store.iterator_cf(
            self.store.cf_block_headers(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        if header_iter.flatten().next().is_some() {
            return Ok(true);
        }

        let tx_iter = self.store.iterator_cf(
            self.store.cf_tx_index(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        for item in tx_iter.flatten().take(1) {
            let (key, _) = item;
            if key.len() >= 8 && keys::decode_block_num(&key[..8]) > start_block {
                return Ok(true);
            }
        }

        let issuance_iter = self.store.iterator_cf(
            self.store.cf_block_issuance(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        if issuance_iter.flatten().next().is_some() {
            return Ok(true);
        }

        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ckbadger_store::{AddressBalance, CachedBlockHeader, CkbadgerStore, StoreBatch};
    use tempfile::TempDir;

    use super::BatchWriter;

    fn setup() -> (TempDir, Arc<CkbadgerStore>, BatchWriter) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());
        (dir, store, writer)
    }

    #[test]
    fn test_init_sync_start_keeps_rebuild_flags_in_bulk_sync() {
        let (_dir, store, writer) = setup();
        store
            .update_sync_status(|status| {
                status.dao_daily_snapshots_rebuilt = true;
                status.address_balances_rebuilt_from_live_cells = true;
            })
            .unwrap();

        writer.init_sync_start(0, true).unwrap();

        let status = store.get_sync_status().unwrap();
        assert!(status.dao_daily_snapshots_rebuilt);
        assert!(status.address_balances_rebuilt_from_live_cells);
    }

    #[test]
    fn test_init_sync_start_keeps_rebuild_flags_for_non_bulk_sync() {
        let (_dir, store, writer) = setup();
        store
            .update_sync_status(|status| {
                status.dao_daily_snapshots_rebuilt = true;
                status.address_balances_rebuilt_from_live_cells = true;
            })
            .unwrap();

        writer.init_sync_start(0, false).unwrap();

        let status = store.get_sync_status().unwrap();
        assert!(status.dao_daily_snapshots_rebuilt);
        assert!(status.address_balances_rebuilt_from_live_cells);
    }

    fn make_header(hash_byte: u8, ts_ms: i64) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![hash_byte; 32],
            timestamp: ts_ms,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        }
    }

    #[test]
    fn test_init_sync_start_skips_cleanup_when_no_partial_data() {
        let (_dir, store, writer) = setup();
        let lock_hash = vec![0xAA; 32];
        store
            .put_addr_balance_direct(
                &lock_hash,
                &AddressBalance {
                    balance: 123,
                    ..Default::default()
                },
            )
            .unwrap();

        writer.init_sync_start(0, false).unwrap();

        assert!(store.get_addr_balance(&lock_hash).unwrap().is_some());
    }

    #[test]
    fn test_init_sync_start_cleans_when_partial_data_exists() {
        let (_dir, store, writer) = setup();
        let lock_hash = vec![0xBB; 32];
        store
            .put_addr_balance_direct(
                &lock_hash,
                &AddressBalance {
                    balance: 456,
                    ..Default::default()
                },
            )
            .unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &make_header(0x10, 1_700_000_000_000));
        batch.put_block_header(1, &make_header(0x11, 1_700_000_010_000));
        batch.commit().unwrap();

        writer.init_sync_start(0, false).unwrap();

        assert!(store.get_block_header(1).unwrap().is_none());
        assert!(store.get_addr_balance(&lock_hash).unwrap().is_none());
    }
}
