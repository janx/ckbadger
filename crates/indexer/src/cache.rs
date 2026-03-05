use ckbadger_common::{CachedProposal, MemoryStatsData, SyncProgressData, SyncStatusData};
use ckbadger_store::CkbadgerStore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

use ckbadger_common::PROPOSAL_WINDOW_FARTHEST;

#[derive(Clone)]
pub struct CacheInvalidator {
    store: Arc<CkbadgerStore>,
    proposals: Arc<Mutex<HashMap<String, CachedProposal>>>,
}

impl CacheInvalidator {
    pub fn new(store: Arc<CkbadgerStore>) -> Self {
        Self {
            store,
            proposals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Chart cache invalidation is now a no-op.
    /// The API in-memory cache uses TTL-based expiry, so charts expire naturally.
    pub async fn invalidate_chart_caches(&self) {
        // No-op: TTL-based cache handles expiry
    }

    pub fn is_enabled(&self) -> bool {
        true
    }

    pub async fn publish_sync_progress(&self, data: &SyncProgressData) {
        match serde_json::to_vec(data) {
            Ok(bytes) => {
                if let Err(e) = self.store.put_sync_progress(&bytes) {
                    warn!("Failed to write sync progress to store: {}", e);
                }
            }
            Err(e) => {
                warn!("Failed to serialize sync progress: {}", e);
            }
        }
    }

    pub async fn get_sync_status(&self) -> Option<SyncStatusData> {
        // Build SyncStatusData from the store's SyncStatus
        let sync = self.store.get_sync_status().ok()?;
        let tip = sync.tip_block_number;
        let total_tx = sync.total_transactions;
        let total_cells = sync.total_cells_created;
        let total_live_cells = sync.total_cells_created - sync.total_cells_consumed;

        Some(SyncStatusData {
            tip_block_number: tip,
            tip_block_hash: format!("0x{}", hex::encode(&sync.tip_block_hash)),
            derived_tip_block_number: Some(sync.derived_tip_block_number),
            total_transactions: total_tx,
            total_cells,
            total_live_cells,
            total_addresses: 0,
            last_synced_at: sync.last_synced_at,
            derived_last_synced_at: Some(sync.derived_last_synced_at),
            derived_sync_in_progress: sync.derived_sync_in_progress,
            sync_started_at: sync.sync_started_at,
            sync_started_block: sync.sync_started_block,
            sync_ema_rate: sync.sync_ema_rate,
            bulk_sync_completed_at: sync.bulk_sync_completed_at,
            bulk_sync_completed_block: sync.bulk_sync_completed_block,
        })
    }

    pub async fn set_sync_status(&self, _data: &SyncStatusData) {
        // SyncStatusData is derived from store's SyncStatus.
        // The indexer already writes SyncStatus to the store directly.
        // This method is kept for API compatibility but is a no-op.
    }

    pub async fn update_sync_status<F>(&self, updater: F) -> Option<SyncStatusData>
    where
        F: FnOnce(&mut SyncStatusData),
    {
        let mut status = self.get_sync_status().await.unwrap_or_default();
        updater(&mut status);
        // Note: the actual sync status is managed via store.update_sync_status().
        // This just returns the updated copy for the caller.
        Some(status)
    }

    pub async fn cache_proposals(&self, proposals: &[CachedProposal]) {
        if proposals.is_empty() {
            return;
        }

        if let Ok(mut map) = self.proposals.lock() {
            for proposal in proposals {
                map.insert(proposal.proposal_id.clone(), proposal.clone());
            }
        }
    }

    pub async fn remove_committed_proposals(&self, proposal_ids: &[String]) {
        if proposal_ids.is_empty() {
            return;
        }

        if let Ok(mut map) = self.proposals.lock() {
            for proposal_id in proposal_ids {
                map.remove(proposal_id);
            }
        }
    }

    pub async fn cleanup_expired_proposals(&self, current_tip: i64) {
        if let Ok(mut map) = self.proposals.lock() {
            let expired: Vec<String> = map
                .iter()
                .filter(|(_, cached)| {
                    let expiry_block = cached.proposed_at_block + PROPOSAL_WINDOW_FARTHEST as i64;
                    current_tip > expiry_block
                })
                .map(|(id, _)| id.clone())
                .collect();

            if !expired.is_empty() {
                info!("Cleaning up {} expired proposals", expired.len());
                for id in &expired {
                    map.remove(id);
                }
            }
        }
    }

    pub async fn get_pending_proposals(&self) -> Vec<CachedProposal> {
        let Ok(map) = self.proposals.lock() else {
            return Vec::new();
        };

        let mut result: Vec<CachedProposal> = map.values().cloned().collect();
        result.sort_by(|a, b| {
            b.proposed_at_block
                .cmp(&a.proposed_at_block)
                .then(a.proposed_at_index.cmp(&b.proposed_at_index))
        });
        result
    }

    pub async fn publish_memory_stats(&self, data: &MemoryStatsData) {
        match serde_json::to_vec(data) {
            Ok(bytes) => {
                if let Err(e) = self.store.put_memory_stats(&bytes) {
                    warn!("Failed to write memory stats to store: {}", e);
                }
            }
            Err(e) => {
                warn!("Failed to serialize memory stats: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_invalidator() -> CacheInvalidator {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        // Leak tempdir to keep it alive for the test
        std::mem::forget(dir);
        CacheInvalidator::new(store)
    }

    #[tokio::test]
    async fn test_cache_invalidator_is_always_enabled() {
        let invalidator = make_test_invalidator();
        assert!(invalidator.is_enabled());
    }

    #[tokio::test]
    async fn test_invalidate_does_not_panic() {
        let invalidator = make_test_invalidator();
        invalidator.invalidate_chart_caches().await;
    }

    #[tokio::test]
    async fn test_publish_sync_progress_writes_to_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let invalidator = CacheInvalidator::new(store.clone());

        let data = SyncProgressData {
            current_block: 1000,
            target_block: 10000,
            last_batch_blocks: Some(64),
            blocks_per_second: 100.0,
            ema_blocks_per_second: 95.0,
            txs_per_second: Some(2_000.0),
            ema_txs_per_second: Some(1_900.0),
            eta_seconds: Some(90.0),
            eta_formatted: "1m 30s".to_string(),
            progress_percentage: 10.0,
            updated_at: 1234567890,
            startup_phase: None,
            is_direct_db_read: false,
            db_write_ms: None,
            db_commit_ms: None,
            rpc_fetch_ms: None,
            pipeline: None,
            pipeline_reset_epoch: None,
            pipeline_reset_reason: None,
            adaptive_target_batch_txs: None,
            adaptive_inflight_limit: None,
            adaptive_min_target_batch_txs: None,
            adaptive_cooldown_steps: None,
            adaptive_last_reason: None,
            adaptive_adjustment_seq: None,
            adaptive_backoff_streak: None,
            adaptive_last_adjusted_at: None,
        };
        invalidator.publish_sync_progress(&data).await;

        // Verify it was written
        let stored = store.get_sync_progress().unwrap();
        assert!(stored.is_some());
        let parsed: SyncProgressData = serde_json::from_slice(&stored.unwrap()).unwrap();
        assert_eq!(parsed.current_block, 1000);
        assert_eq!(parsed.target_block, 10000);
    }

    #[tokio::test]
    async fn test_get_sync_status_returns_data() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());

        // Set up some sync status in the store
        store
            .set_sync_status(&ckbadger_store::types::SyncStatus {
                tip_block_number: 12345,
                tip_block_hash: vec![0xab; 32],
                total_transactions: 1000,
                total_cells_created: 500,
                total_cells_consumed: 200,
                last_synced_at: 1700000000,
                sync_started_at: Some(1699999000),
                sync_started_block: 10000,
                sync_ema_rate: Some(77.7),
                bulk_sync_completed_at: Some(1700000100),
                bulk_sync_completed_block: Some(12345),
                ..Default::default()
            })
            .unwrap();

        let invalidator = CacheInvalidator::new(store);
        let status = invalidator.get_sync_status().await;
        assert!(status.is_some());
        let status = status.unwrap();
        assert_eq!(status.tip_block_number, 12345);
        assert_eq!(status.total_transactions, 1000);
        assert_eq!(status.total_cells, 500);
        assert_eq!(status.total_live_cells, 300);
        assert_eq!(status.sync_started_at, Some(1699999000));
        assert_eq!(status.sync_ema_rate, Some(77.7));
        assert_eq!(status.bulk_sync_completed_block, Some(12345));
    }

    #[tokio::test]
    async fn test_proposals_cache_roundtrip() {
        let invalidator = make_test_invalidator();

        let proposals = vec![
            CachedProposal::new_minimal("abc123".to_string(), 100, 0),
            CachedProposal::new_minimal("def456".to_string(), 101, 1),
        ];
        invalidator.cache_proposals(&proposals).await;

        let pending = invalidator.get_pending_proposals().await;
        assert_eq!(pending.len(), 2);

        // Remove one
        invalidator
            .remove_committed_proposals(&["abc123".to_string()])
            .await;
        let pending = invalidator.get_pending_proposals().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].proposal_id, "def456");
    }

    #[tokio::test]
    async fn test_cleanup_expired_proposals() {
        let invalidator = make_test_invalidator();

        let proposals = vec![
            CachedProposal::new_minimal("old".to_string(), 50, 0),
            CachedProposal::new_minimal("new".to_string(), 1000, 1),
        ];
        invalidator.cache_proposals(&proposals).await;

        // Current tip at 100 should expire the proposal from block 50 (expiry = 50 + 10 = 60)
        invalidator.cleanup_expired_proposals(100).await;

        let pending = invalidator.get_pending_proposals().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].proposal_id, "new");
    }

    #[tokio::test]
    async fn test_publish_memory_stats_writes_to_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let invalidator = CacheInvalidator::new(store.clone());

        let data = MemoryStatsData {
            live_cells_count: 1000,
            updated_at: 1700000000,
            ..Default::default()
        };
        invalidator.publish_memory_stats(&data).await;

        let stored = store.get_memory_stats().unwrap();
        assert!(stored.is_some());
        let parsed: MemoryStatsData = serde_json::from_slice(&stored.unwrap()).unwrap();
        assert_eq!(parsed.live_cells_count, 1000);
    }
}
