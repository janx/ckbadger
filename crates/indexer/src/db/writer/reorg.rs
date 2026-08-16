use anyhow::{anyhow, Result};
use chrono::Utc;
use tracing::info;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys::{self, sync_meta_keys};
use ckbadger_store::types::{DeepForkInfo, ReorgEvent, ReorgEventKind};
use ckbadger_store::CkbadgerStore;

use super::BatchWriter;

/// Build the history key for an event detected now.
///
/// The millisecond field orders the history and the uuid keeps two events
/// detected in the same millisecond distinct.
fn next_reorg_event_key() -> Result<String> {
    keys::encode_reorg_event_key(
        Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4().as_bytes(),
    )
}

impl BatchWriter {
    pub fn record_deep_fork(
        &self,
        fork_point: i64,
        fork_hash: &[u8],
        db_tip: i64,
        db_tip_hash: &[u8],
        chain_tip: i64,
        chain_tip_hash: &[u8],
        depth: i64,
    ) -> Result<()> {
        // Store the reorg event
        let depth_i32 = i32::try_from(depth)
            .map_err(|_| anyhow!("reorg depth exceeds i32 range: depth={}", depth))?;
        let event = ReorgEvent {
            detected_at: Utc::now().timestamp(),
            kind: ReorgEventKind::Deep,
            fork_point,
            fork_point_hash: fork_hash.to_vec(),
            old_tip: db_tip,
            old_tip_hash: db_tip_hash.to_vec(),
            new_tip: chain_tip,
            new_tip_hash: chain_tip_hash.to_vec(),
            depth: depth_i32,
            // A deep fork pauses sync instead of rolling back, so nothing is
            // orphaned yet.
            orphaned_blocks: 0,
            orphaned_txs: 0,
        };
        let event_key = next_reorg_event_key()?;
        let event_bytes = bincode::serialize(&event)?;
        let mut status = self.store.get_sync_status()?;
        status.deep_fork_detected = true;
        status.deep_fork_info = Some(DeepForkInfo {
            db_tip,
            db_tip_hash: db_tip_hash.to_vec(),
            chain_tip,
            chain_tip_hash: chain_tip_hash.to_vec(),
            depth: depth_i32,
            fork_point,
        });
        let status_bytes = bincode::serialize(&status)?;

        let mut batch = StoreBatch::new(self.store.as_ref());
        batch.put_sync_meta(event_key.as_bytes(), &event_bytes);
        batch.put_sync_meta(sync_meta_keys::SYNC_STATUS, &status_bytes);
        // The local node has already reported that this tip is no longer
        // canonical. Keep the API unavailable until an operator resolves the
        // deep fork instead of serving a stale summary under a canonical label.
        batch.delete_live_cell_summary_current()?;
        batch.commit()?;

        Ok(())
    }

    pub async fn execute_reorg(
        &self,
        append_store: &CkbadgerStore,
        fork_point: i64,
        fork_hash: &[u8],
        old_tip: i64,
        old_tip_hash: &[u8],
        new_tip: i64,
        new_tip_hash: &[u8],
    ) -> Result<ReorgResult> {
        let depth = i32::try_from(old_tip - fork_point).map_err(|_| {
            anyhow!(
                "reorg depth exceeds i32 range: old_tip={}, fork_point={}",
                old_tip,
                fork_point
            )
        })?;

        // Validate the retained recovery source before undo-log replay. The
        // old summary stays verifiable by hash during this first phase;
        // rollback_to_block withdraws it before staging canonical cell changes.
        let summary_initialized = self.store.is_live_cell_summary_initialized()?;
        let current_summary = self.store.get_live_cell_summary()?;
        if summary_initialized && current_summary.is_none() {
            return Err(anyhow!(
                "initialized live-cell summary is missing before reorg: old_tip={} old_tip_hash=0x{}",
                old_tip,
                hex::encode(old_tip_hash),
            ));
        }
        if !summary_initialized && current_summary.is_some() {
            return Err(anyhow!(
                "live-cell summary current record exists without initialized marker before reorg"
            ));
        }
        if let Some(current) = current_summary {
            let history = self
                .store
                .get_live_cell_summary_at(current.tip_block_number)?
                .ok_or_else(|| {
                    anyhow!(
                        "missing live-cell summary history before reorg: block={} hash=0x{}",
                        current.tip_block_number,
                        hex::encode(current.tip_block_hash),
                    )
                })?;
            if history != current
                || current.tip_block_number != old_tip
                || current.tip_block_hash.as_slice() != old_tip_hash
            {
                return Err(anyhow!(
                    "live-cell summary does not match reorg source tip: current={:?} history={:?} old_tip={} old_tip_hash=0x{}",
                    current,
                    history,
                    old_tip,
                    hex::encode(old_tip_hash),
                ));
            }
        }
        // Revert domain mutations from undo-log first so that entity data
        // (Spore, mNFT, dotbit) is restored to pre-fork state before the
        // multi-stage rollback rebuilds aggregates from it.
        // Extract TxContext entries before the undo log deletes them, so the
        // subsequent cell rollback can use targeted lookups instead of full
        // CF scans.
        let undo_result = self.store.rollback_via_undo_log(append_store, fork_point)?;
        // Domain rollback for canonical mutable state (cells, blocks, stats,
        // aggregates rebuilt from now-correct entity data).
        let rollback = self.store.rollback_to_block_with_tx_contexts(
            fork_point,
            Some(append_store),
            undo_result.tx_contexts,
        )?;
        // Flush memtables after rollback so that the subsequent refresh reads
        // (script rollups, DAO statistics) hit sorted SSTs instead of
        // triggering O(N log N) VectorRep sorts on the un-flushed memtable.
        self.store.flush_all_memtables()?;
        append_store.flush_all_memtables()?;
        // Re-derive script version/family rollups from the corrected reference info.
        self.refresh_script_reference_rollups()?;
        // Advance the DAO singleton stats (latest stats + top depositors) to the
        // post-rollback tip. Rollback deliberately leaves the pre-rollback row
        // in place — it is never deleted — so the read path always has a value
        // and this refresh overwrites it rather than filling a gap.
        self.refresh_latest_dao_statistics()?;

        // Record reorg event and clear deep fork flag in one sync_meta batch.
        let orphaned_blocks = i64::try_from(rollback.blocks_removed).map_err(|_| {
            anyhow!(
                "rolled-back block count exceeds i64 range: blocks_removed={}",
                rollback.blocks_removed
            )
        })?;
        let orphaned_txs = i64::try_from(rollback.txs_removed).map_err(|_| {
            anyhow!(
                "rolled-back transaction count exceeds i64 range: txs_removed={}",
                rollback.txs_removed
            )
        })?;
        let event = ReorgEvent {
            detected_at: Utc::now().timestamp(),
            kind: ReorgEventKind::Automatic,
            fork_point,
            fork_point_hash: fork_hash.to_vec(),
            old_tip,
            old_tip_hash: old_tip_hash.to_vec(),
            new_tip,
            new_tip_hash: new_tip_hash.to_vec(),
            depth,
            orphaned_blocks,
            orphaned_txs,
        };
        let event_key = next_reorg_event_key()?;
        let event_bytes = bincode::serialize(&event)?;
        let mut status = self.store.get_sync_status()?;
        status.deep_fork_detected = false;
        status.deep_fork_info = None;
        let status_bytes = bincode::serialize(&status)?;
        let mut batch = StoreBatch::new(self.store.as_ref());
        batch.put_sync_meta(event_key.as_bytes(), &event_bytes);
        batch.put_sync_meta(sync_meta_keys::SYNC_STATUS, &status_bytes);
        batch.commit()?;

        // Update cache
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
            depth,
            orphaned_blocks: i32::try_from(orphaned_blocks).map_err(|_| {
                anyhow!(
                    "rolled-back block count exceeds i32 range: orphaned_blocks={}",
                    orphaned_blocks
                )
            })?,
        })
    }
}

pub struct ReorgResult {
    pub depth: i32,
    pub orphaned_blocks: i32,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ckbadger_store::keys;
    use ckbadger_store::store::CkbadgerStore;
    use ckbadger_store::types::{LiveCellSummary, ReorgEventKind, SyncStatus};
    use ckbadger_store::StoreBatch;

    use crate::db::writer::BatchWriter;

    use super::next_reorg_event_key;

    fn reorg_event_count(store: &CkbadgerStore) -> usize {
        store
            .iterator_cf(store.cf_sync_meta(), rocksdb::IteratorMode::Start)
            .flatten()
            .filter(|(key, _)| key.starts_with(keys::REORG_EVENT_KEY_PREFIX))
            .count()
    }

    #[test]
    fn test_next_reorg_event_key_is_unique() {
        let first = next_reorg_event_key().unwrap();
        let second = next_reorg_event_key().unwrap();
        assert_ne!(first, second);
        assert!(first.starts_with("reorg:"));
        assert!(second.starts_with("reorg:"));
    }

    #[test]
    fn test_next_reorg_event_key_contains_uuid_segment() {
        let key = next_reorg_event_key().unwrap();
        assert!(key.starts_with("reorg:"));
        let parts: Vec<&str> = key.splitn(3, ':').collect();
        assert_eq!(parts.len(), 3, "expected format reorg:timestamp:uuid");
        assert_eq!(
            parts[1].len(),
            keys::REORG_EVENT_MS_DIGITS,
            "millisecond field must be fixed width so key order is chronological"
        );
        assert!(parts[1].chars().all(|c| c.is_ascii_digit()));
        assert_eq!(parts[2].len(), 32, "UUID segment should be 32 hex chars");
        assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Fixed-width millisecond fields mean lexicographic key order is
    /// chronological order, which is what the history listing pages on.
    #[test]
    fn test_reorg_event_keys_sort_chronologically() {
        let early = keys::encode_reorg_event_key(999_999_999_999, &[0xFF; 16]).unwrap();
        let late = keys::encode_reorg_event_key(1_700_000_000_000, &[0x00; 16]).unwrap();
        assert!(early < late, "{early} should sort before {late}");
    }

    #[test]
    fn test_next_reorg_event_key_no_collision_across_simulated_restarts() {
        let key1 = next_reorg_event_key().unwrap();
        let key2 = next_reorg_event_key().unwrap();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_record_deep_fork_writes_event_and_sync_status_together() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let summary = LiveCellSummary {
            tip_block_number: 120,
            tip_block_hash: [0x11; 32],
            dao: 1,
            typed_non_dao: 0,
            plain: 2,
            data_bearing: 1,
        };
        let mut seed = StoreBatch::new(store.as_ref());
        seed.put_live_cell_summary_snapshots(&[summary]).unwrap();
        seed.commit().unwrap();
        store
            .set_sync_status(&SyncStatus {
                tip_block_number: 120,
                tip_block_hash: vec![0x11; 32],
                total_cells_created: 4,
                total_cells_consumed: 1,
                ..Default::default()
            })
            .unwrap();

        writer
            .record_deep_fork(100, &[0x33; 32], 120, &[0x11; 32], 130, &[0x22; 32], 20)
            .unwrap();

        assert_eq!(reorg_event_count(store.as_ref()), 1);
        let latest = store
            .get_latest_reorg_event()
            .unwrap()
            .expect("latest reorg event should exist");
        assert_eq!(latest.event.kind, ReorgEventKind::Deep);
        assert_eq!(latest.event.fork_point, 100);
        assert_eq!(latest.event.fork_point_hash, vec![0x33; 32]);
        assert_eq!(latest.event.old_tip, 120);
        assert_eq!(latest.event.old_tip_hash, vec![0x11; 32]);
        assert_eq!(latest.event.new_tip, 130);
        assert_eq!(latest.event.new_tip_hash, vec![0x22; 32]);
        assert_eq!(latest.event.depth, 20);
        assert_eq!(latest.event.orphaned_blocks, 0);

        let status = store.get_sync_status().unwrap();
        assert!(status.deep_fork_detected);
        let info = status.deep_fork_info.unwrap();
        assert_eq!(info.fork_point, 100);
        assert_eq!(info.db_tip, 120);
        assert_eq!(info.chain_tip, 130);
        assert_eq!(info.depth, 20);
        assert_eq!(store.get_live_cell_summary().unwrap(), None);
        assert_eq!(store.get_live_cell_summary_at(120).unwrap(), Some(summary));
    }

    #[tokio::test]
    async fn test_execute_reorg_does_not_persist_event_when_rollback_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let key = keys::encode_block_num(1);
        store
            .put_cf(store.cf_block_headers(), &key, b"invalid-header-payload")
            .unwrap();

        let result = writer
            .execute_reorg(store.as_ref(), 0, &[0xAA; 32], 1, &[], 1, &[])
            .await;
        assert!(result.is_err());
        assert_eq!(reorg_event_count(store.as_ref()), 0);
    }
}
