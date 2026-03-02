use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use chrono::Utc;
use tracing::info;

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys::sync_meta_keys;
use ckbadger_store::types::{DeepForkInfo, ReorgEvent};

use super::BatchWriter;

static REORG_EVENT_SEQ: AtomicU64 = AtomicU64::new(0);

fn next_reorg_event_key() -> String {
    let ts_ms = Utc::now().timestamp_millis();
    let seq = REORG_EVENT_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("reorg:{}:{}", ts_ms, seq)
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
        let event = ReorgEvent {
            detected_at: Utc::now().timestamp(),
            rollback_from: fork_point + 1,
            rollback_to: fork_point,
            depth: depth as i32,
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
            depth: depth as i32,
            fork_point,
        });
        let status_bytes = bincode::serialize(&status)?;

        let mut batch = StoreBatch::new(self.store.as_ref());
        batch.put_sync_meta(event_key.as_bytes(), &event_bytes);
        batch.put_sync_meta(sync_meta_keys::SYNC_STATUS, &status_bytes);
        batch.commit()?;

        Ok(())
    }

    pub async fn execute_reorg(
        &self,
        fork_point: i64,
        fork_hash: &[u8],
        old_tip: i64,
        _old_tip_hash: &[u8],
        _new_tip: i64,
        _new_tip_hash: &[u8],
    ) -> Result<ReorgResult> {
        let depth = (old_tip - fork_point) as i32;

        // Use the store's atomic rollback which handles all CFs
        self.store.rollback_to_block(fork_point)?;

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
            orphaned_txs: 0, // Not tracked in RocksDB model
        })
    }

    pub fn clear_deep_fork_flag(&self) -> Result<()> {
        self.store.clear_deep_fork()?;
        Ok(())
    }

    pub fn resolve_deep_fork(
        &self,
        _action: &str,
        _resolved_by: Option<&str>,
        _notes: Option<&str>,
    ) -> Result<()> {
        self.clear_deep_fork_flag()
    }
}

pub struct ReorgResult {
    pub depth: i32,
    pub orphaned_blocks: i32,
    pub orphaned_txs: i32,
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
    fn test_record_deep_fork_writes_event_and_sync_status_together() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        writer
            .record_deep_fork(100, &[], 120, &[0x11; 32], 130, &[0x22; 32], 20)
            .unwrap();

        assert_eq!(reorg_event_count(store.as_ref()), 1);
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let key = keys::encode_block_num(1);
        store
            .put_cf(store.cf_block_headers(), &key, b"invalid-header-payload")
            .unwrap();

        let result = writer.execute_reorg(0, &[0xAA; 32], 1, &[], 1, &[]).await;
        assert!(result.is_err());
        assert_eq!(reorg_event_count(store.as_ref()), 0);
    }
}
