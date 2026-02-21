use anyhow::{bail, Result};
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
        // Persist sync status in RocksDB so restart/fallback paths do not rely on Redis.
        self.store.update_sync_status(|status| {
            status.tip_block_number = block_number;
            status.tip_block_hash = block_hash.to_vec();
            status.total_transactions += tx_count;
            status.total_cells_created += cells_created;
            status.total_cells_consumed += cells_consumed;
            status.last_synced_at = chrono::Utc::now().timestamp();
        })?;

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
        if start_block < -1 {
            bail!(
                "invalid startup sync tip: start_block={} (expected >= -1)",
                start_block
            );
        }
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

        // Align persistent sync tip to the startup tip to avoid stale sync_status metadata.
        let tip_number = if start_block < 0 { 0 } else { start_block };
        let tip_hash = if start_block >= 0 {
            self.store.get_block_header(start_block)?.map(|h| h.hash)
        } else {
            None
        };
        self.store.update_sync_status(|status| {
            status.tip_block_number = tip_number;
            match &tip_hash {
                Some(hash) => status.tip_block_hash = hash.clone(),
                None if tip_number == 0 => status.tip_block_hash.clear(),
                None => {}
            }
        })?;

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

    pub fn needs_startup_cleanup(&self, start_block: i64) -> Result<bool> {
        self.has_partial_data_after_block(start_block)
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

    #[test]
    fn test_needs_startup_cleanup_reports_partial_data() {
        let (_dir, store, writer) = setup();
        assert!(!writer.needs_startup_cleanup(0).unwrap());

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &make_header(0x21, 1_700_000_020_000));
        batch.commit().unwrap();

        assert!(writer.needs_startup_cleanup(0).unwrap());
    }

    #[test]
    fn test_update_sync_status_persists_sync_meta_in_store() {
        let (_dir, store, writer) = setup();
        let first_hash = vec![0xAB; 32];
        let second_hash = vec![0xCD; 32];

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            writer
                .update_sync_status(42, &first_hash, 10, 20, 4, 1, Some(123.0))
                .await
                .unwrap();
            writer
                .update_sync_status(43, &second_hash, 3, 8, 2, 1, None)
                .await
                .unwrap();
        });

        let status = store.get_sync_status().unwrap();
        assert_eq!(status.tip_block_number, 43);
        assert_eq!(status.tip_block_hash, second_hash);
        assert_eq!(status.total_transactions, 13);
        assert_eq!(status.total_cells_created, 28);
        assert_eq!(status.total_cells_consumed, 6);
        assert!(status.last_synced_at > 0);
    }

    #[test]
    fn test_init_sync_start_errors_when_start_block_below_minus_one() {
        let (_dir, _store, writer) = setup();
        let err = writer.init_sync_start(-2, false).unwrap_err();
        assert!(err.to_string().contains("expected >= -1"));
    }
}
