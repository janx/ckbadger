use anyhow::{anyhow, Result};
use chrono::Utc;
use tracing::info;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys::sync_meta_keys;
use ckbadger_store::types::{DeepForkInfo, ReorgEvent};
use ckbadger_store::CkbadgerStore;

use super::BatchWriter;

fn next_reorg_event_key() -> String {
    let ts_ms = Utc::now().timestamp_millis();
    let uuid_hex = hex::encode(uuid::Uuid::new_v4().as_bytes());
    format!("reorg:{}:{}", ts_ms, uuid_hex)
}

impl BatchWriter {
    pub fn record_deep_fork(
        &self,
        fork_point: i64,
        _fork_hash: &[u8],
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
            rollback_from: fork_point + 1,
            rollback_to: fork_point,
            depth: depth_i32,
        };
        let event_key = next_reorg_event_key();
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
        batch.put_sync_meta(sync_meta_keys::REORG_LATEST_EVENT, &event_bytes);
        batch.put_sync_meta(sync_meta_keys::SYNC_STATUS, &status_bytes);
        batch.commit()?;

        Ok(())
    }

    pub async fn execute_reorg(
        &self,
        append_store: &CkbadgerStore,
        fork_point: i64,
        fork_hash: &[u8],
        old_tip: i64,
        _old_tip_hash: &[u8],
        _new_tip: i64,
        _new_tip_hash: &[u8],
    ) -> Result<ReorgResult> {
        let depth = i32::try_from(old_tip - fork_point).map_err(|_| {
            anyhow!(
                "reorg depth exceeds i32 range: old_tip={}, fork_point={}",
                old_tip,
                fork_point
            )
        })?;

        // Domain rollback for canonical mutable state.
        self.store
            .rollback_to_block_with_append_only_store(fork_point, Some(append_store))?;
        // Revert domain mutations from undo-log and prune append undo entries.
        self.store.rollback_via_undo_log(append_store, fork_point)?;
        // Re-derive script version/family rollups from the corrected reference info.
        self.refresh_script_reference_rollups()?;

        // Record reorg event and clear deep fork flag in one sync_meta batch.
        let event = ReorgEvent {
            detected_at: Utc::now().timestamp(),
            rollback_from: fork_point + 1,
            rollback_to: fork_point,
            depth,
        };
        let event_key = next_reorg_event_key();
        let event_bytes = bincode::serialize(&event)?;
        let mut status = self.store.get_sync_status()?;
        status.deep_fork_detected = false;
        status.deep_fork_info = None;
        let status_bytes = bincode::serialize(&status)?;
        let mut batch = StoreBatch::new(self.store.as_ref());
        batch.put_sync_meta(event_key.as_bytes(), &event_bytes);
        batch.put_sync_meta(sync_meta_keys::REORG_LATEST_EVENT, &event_bytes);
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
            "Reorg completed: fork_point={}, depth={}",
            fork_point, depth
        );

        Ok(ReorgResult {
            depth,
            orphaned_blocks: depth,
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

    use crate::db::writer::BatchWriter;

    use super::next_reorg_event_key;

    fn reorg_event_count(store: &CkbadgerStore) -> usize {
        store
            .iterator_cf(store.cf_sync_meta(), rocksdb::IteratorMode::Start)
            .flatten()
            .filter(|(key, _)| key.starts_with(b"reorg:"))
            .count()
    }

    #[test]
    fn test_next_reorg_event_key_is_unique() {
        let first = next_reorg_event_key();
        let second = next_reorg_event_key();
        assert_ne!(first, second);
        assert!(first.starts_with("reorg:"));
        assert!(second.starts_with("reorg:"));
    }

    #[test]
    fn test_next_reorg_event_key_contains_uuid_segment() {
        let key = next_reorg_event_key();
        assert!(key.starts_with("reorg:"));
        let parts: Vec<&str> = key.splitn(3, ':').collect();
        assert_eq!(parts.len(), 3, "expected format reorg:timestamp:uuid");
        assert_eq!(parts[2].len(), 32, "UUID segment should be 32 hex chars");
        assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_next_reorg_event_key_no_collision_across_simulated_restarts() {
        let key1 = next_reorg_event_key();
        let key2 = next_reorg_event_key();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_record_deep_fork_writes_event_and_sync_status_together() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        writer
            .record_deep_fork(100, &[], 120, &[0x11; 32], 130, &[0x22; 32], 20)
            .unwrap();

        assert_eq!(reorg_event_count(store.as_ref()), 1);
        let latest = store
            .get_cf(
                store.cf_sync_meta(),
                keys::sync_meta_keys::REORG_LATEST_EVENT,
            )
            .unwrap()
            .expect("latest reorg marker should exist");
        let latest_event: ckbadger_store::types::ReorgEvent =
            bincode::deserialize(&latest).unwrap();
        assert_eq!(latest_event.rollback_to, 100);
        assert_eq!(latest_event.depth, 20);

        let status = store.get_sync_status().unwrap();
        assert!(status.deep_fork_detected);
        let info = status.deep_fork_info.unwrap();
        assert_eq!(info.fork_point, 100);
        assert_eq!(info.db_tip, 120);
        assert_eq!(info.chain_tip, 130);
        assert_eq!(info.depth, 20);
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
