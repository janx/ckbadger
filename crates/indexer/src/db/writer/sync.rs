use anyhow::{bail, Result};
use tracing::{info, warn};

use ckbadger_store::keys;

use super::BatchWriter;

const STARTUP_CONTINUITY_SAMPLE_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupContinuityProbe {
    pub startup_tip: i64,
    pub header_tip: Option<i64>,
    pub tx_floor: Option<i64>,
    pub tx_tip: Option<i64>,
    pub first_header_gap: Option<i64>,
    pub recent_window_start: i64,
    pub recent_window_end: i64,
    pub missing_header_sample: Vec<i64>,
    pub missing_tx_block0_sample: Vec<i64>,
    pub full_header_gap_scan: bool,
}

impl StartupContinuityProbe {
    pub fn has_inconsistency(&self) -> bool {
        self.first_header_gap.is_some()
            || self
                .header_tip
                .zip(self.tx_tip)
                .is_some_and(|(header_tip, tx_tip)| header_tip != tx_tip)
            || self.tx_floor.is_some_and(|tx_floor| tx_floor > 0)
            || !self.missing_header_sample.is_empty()
            || !self.missing_tx_block0_sample.is_empty()
    }
}

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
            status.derived_tip_block_number = block_number;
            status.total_transactions += tx_count;
            status.total_cells_created += cells_created;
            status.total_cells_consumed += cells_consumed;
            let now = chrono::Utc::now().timestamp();
            status.last_synced_at = now;
            status.derived_last_synced_at = now;
            status.derived_sync_in_progress = false;
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
        let max_tx_block = self.tx_index_boundary_block(rocksdb::IteratorMode::End);

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

    pub fn probe_startup_continuity(
        &self,
        startup_tip: i64,
        window_size: i64,
        include_full_header_gap_scan: bool,
    ) -> Result<StartupContinuityProbe> {
        if startup_tip < -1 {
            bail!(
                "invalid startup tip for continuity probe: startup_tip={} (expected >= -1)",
                startup_tip
            );
        }
        if window_size <= 0 {
            bail!(
                "invalid continuity probe window_size={} (expected > 0)",
                window_size
            );
        }

        let header_tip = self.store.get_sync_tip_block()?.map(|(num, _)| num);
        let tx_floor = self.tx_index_boundary_block(rocksdb::IteratorMode::Start);
        let tx_tip = self.tx_index_boundary_block(rocksdb::IteratorMode::End);

        let mut missing_header_sample = Vec::new();
        let mut missing_tx_block0_sample = Vec::new();
        let (recent_window_start, recent_window_end) = match header_tip {
            Some(header_tip) if header_tip >= 0 => {
                let window_end = std::cmp::min(startup_tip.max(0), header_tip);
                let window_start = (window_end - window_size + 1).max(0);
                for block_num in window_start..=window_end {
                    if self.store.get_block_header(block_num)?.is_none() {
                        if missing_header_sample.len() < STARTUP_CONTINUITY_SAMPLE_LIMIT {
                            missing_header_sample.push(block_num);
                        }
                        continue;
                    }
                    if self.store.get_tx_index(block_num, 0)?.is_none()
                        && missing_tx_block0_sample.len() < STARTUP_CONTINUITY_SAMPLE_LIMIT
                    {
                        missing_tx_block0_sample.push(block_num);
                    }
                }
                (window_start, window_end)
            }
            _ => (0, -1),
        };

        let first_header_gap = if include_full_header_gap_scan {
            self.store.find_first_block_header_gap()?
        } else {
            None
        };

        Ok(StartupContinuityProbe {
            startup_tip,
            header_tip,
            tx_floor,
            tx_tip,
            first_header_gap,
            recent_window_start,
            recent_window_end,
            missing_header_sample,
            missing_tx_block0_sample,
            full_header_gap_scan: include_full_header_gap_scan,
        })
    }

    pub fn init_sync_start(&self, start_block: i64, is_bulk_sync: bool) -> Result<()> {
        self.init_sync_start_with_options(start_block, is_bulk_sync, false)
    }

    pub fn init_sync_start_with_options(
        &self,
        start_block: i64,
        is_bulk_sync: bool,
        force_cleanup: bool,
    ) -> Result<()> {
        if start_block < -1 {
            bail!(
                "invalid startup sync tip: start_block={} (expected >= -1)",
                start_block
            );
        }
        let next_block = start_block + 1;
        let has_partial_data = self.has_partial_data_after_block(start_block)?;
        let cleanup_reason = if force_cleanup && has_partial_data {
            "forced_and_partial_data_detected"
        } else if force_cleanup {
            "forced_after_unclean_shutdown"
        } else if has_partial_data {
            "partial_data_detected"
        } else {
            "no_cleanup_needed"
        };
        info!(
            start_block,
            next_block, force_cleanup, has_partial_data, cleanup_reason, "Startup cleanup decision"
        );
        if force_cleanup || has_partial_data {
            info!(
                start_block,
                next_block,
                force_cleanup,
                has_partial_data,
                cleanup_reason,
                "Cleaning up partial data before sync start"
            );

            // Use the store's rollback mechanism to clean up everything
            let rollback_target =
                if start_block >= 0 && self.store.get_block_header(start_block)?.is_none() {
                    warn!(
                        start_block,
                        "Startup cleanup tip header missing; rolling back to -1 for full cleanup"
                    );
                    -1
                } else {
                    start_block
                };
            self.store.rollback_to_block(rollback_target)?;
            info!(
                start_block,
                rollback_target, next_block, cleanup_reason, "Startup cleanup complete"
            );
        } else {
            info!(
                start_block,
                next_block, cleanup_reason, "Skipping startup rollback cleanup"
            );
        }

        // Align persistent sync tip to the startup tip to avoid stale sync_status metadata.
        let (tip_number, tip_hash) = if let Some((num, header)) = self.store.get_sync_tip_block()? {
            (num, Some(header.hash))
        } else {
            (0, None)
        };
        self.store.update_sync_status(|status| {
            status.tip_block_number = tip_number;
            match &tip_hash {
                Some(hash) => status.tip_block_hash = hash.clone(),
                None if tip_number == 0 => status.tip_block_hash.clear(),
                None => {}
            }
            status.derived_sync_in_progress = is_bulk_sync;
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
        self.needs_startup_cleanup_with_force(start_block, false)
    }

    pub fn needs_startup_cleanup_with_force(
        &self,
        start_block: i64,
        force_cleanup: bool,
    ) -> Result<bool> {
        if force_cleanup {
            return Ok(true);
        }
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

        if self.store.has_undo_log_entries_after(start_block)? {
            return Ok(true);
        }

        Ok(false)
    }

    fn tx_index_boundary_block(&self, mode: rocksdb::IteratorMode) -> Option<i64> {
        let iter = self.store.iterator_cf(self.store.cf_tx_index(), mode);
        for item in iter.flatten().take(1) {
            let (key, _) = item;
            if key.len() >= 8 {
                return Some(keys::decode_block_num(&key[..8]));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ckbadger_store::{
        AddressBalance, CachedBlockHeader, CkbadgerStore, StoreBatch, TxIndexEntry,
    };
    use tempfile::TempDir;

    use super::BatchWriter;

    fn setup() -> (TempDir, Arc<CkbadgerStore>, BatchWriter) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());
        (dir, store, writer)
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

    fn make_tx_index_entry() -> TxIndexEntry {
        TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 1,
            tx_size: 1,
            cycles: None,
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
    fn test_needs_startup_cleanup_reports_pending_undo_log_entries() {
        let (_dir, store, writer) = setup();
        assert!(!writer.needs_startup_cleanup(0).unwrap());

        let mut batch = StoreBatch::new(&store);
        batch.put_reorg_undo_log_by_block(
            2,
            0,
            &ckbadger_store::types::UndoLogEntry::TxContext(ckbadger_store::types::UndoTxContext {
                tx_hash: vec![0xAA; 32],
                outputs_count: 0,
                inputs: vec![],
            }),
        );
        batch.commit().unwrap();

        assert!(writer.needs_startup_cleanup(0).unwrap());
        assert!(!writer.needs_startup_cleanup(2).unwrap());
    }

    #[test]
    fn test_probe_startup_continuity_reports_clean_state() {
        let (_dir, store, writer) = setup();

        let mut batch = StoreBatch::new(&store);
        for block in 0..=2 {
            batch.put_block_header(block, &make_header(0x40 + block as u8, 1_700_000_000_000));
            batch.put_tx_index(block, 0, &make_tx_index_entry());
        }
        batch.commit().unwrap();

        let probe = writer.probe_startup_continuity(2, 32, true).unwrap();
        assert_eq!(probe.header_tip, Some(2));
        assert_eq!(probe.tx_floor, Some(0));
        assert_eq!(probe.tx_tip, Some(2));
        assert_eq!(probe.first_header_gap, None);
        assert!(probe.missing_header_sample.is_empty());
        assert!(probe.missing_tx_block0_sample.is_empty());
        assert!(!probe.has_inconsistency());
    }

    #[test]
    fn test_probe_startup_continuity_detects_missing_header_and_tx() {
        let (_dir, store, writer) = setup();

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &make_header(0x50, 1_700_000_000_000));
        batch.put_block_header(2, &make_header(0x52, 1_700_000_020_000));
        batch.put_tx_index(0, 0, &make_tx_index_entry());
        batch.commit().unwrap();

        let probe = writer.probe_startup_continuity(2, 32, true).unwrap();
        assert_eq!(probe.header_tip, Some(2));
        assert_eq!(probe.tx_floor, Some(0));
        assert_eq!(probe.tx_tip, Some(0));
        assert_eq!(probe.first_header_gap, Some(1));
        assert_eq!(probe.missing_header_sample, vec![1]);
        assert_eq!(probe.missing_tx_block0_sample, vec![2]);
        assert!(probe.has_inconsistency());
    }

    #[test]
    fn test_needs_startup_cleanup_with_force_reports_true_without_partial_data() {
        let (_dir, _store, writer) = setup();
        assert!(writer.needs_startup_cleanup_with_force(0, true).unwrap());
    }

    #[test]
    fn test_init_sync_start_forces_cleanup_without_partial_data() {
        let (_dir, store, writer) = setup();
        let lock_hash = vec![0xCC; 32];
        store
            .put_addr_balance_direct(
                &lock_hash,
                &AddressBalance {
                    balance: 789,
                    ..Default::default()
                },
            )
            .unwrap();

        writer.init_sync_start_with_options(0, false, true).unwrap();

        assert!(store.get_addr_balance(&lock_hash).unwrap().is_none());
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
