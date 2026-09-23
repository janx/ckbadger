use ckbadger_common::{CachedProposal, MemoryStatsData, SyncProgressData, SyncStatusData};
use ckbadger_store::{CkbadgerStore, HeartbeatTick};
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Clone)]
pub struct CacheInvalidator {
    store: Arc<CkbadgerStore>,
}

impl CacheInvalidator {
    pub fn new(store: Arc<CkbadgerStore>) -> Self {
        Self { store }
    }

    /// Chart cache invalidation is now a no-op.
    /// The API in-memory cache uses TTL-based expiry, so charts expire naturally.
    pub async fn invalidate_chart_caches(&self) {
        // No-op: TTL-based cache handles expiry
    }

    pub fn is_enabled(&self) -> bool {
        true
    }

    /// Persist one progress-loop tick: the runtime heartbeat, sync progress and
    /// — when this tick resampled them — memory stats, in a SINGLE store write.
    ///
    /// The loop runs every 3 s per network; three separate writes per tick were
    /// three more rewrites of `sync_meta`, the CF whose per-block volume drove
    /// the mainnet flush storm. `memory_stats = None` leaves the previous
    /// sample in place.
    // The tick's fields are exactly the keys it writes; grouping them into a
    // second near-copy of `ckbadger_store::HeartbeatTick` would add a type
    // without adding meaning.
    #[allow(clippy::too_many_arguments)]
    pub async fn publish_heartbeat_tick(
        &self,
        run_id: &str,
        current_block: i64,
        target_block: i64,
        stage: Option<&str>,
        oom_events: Option<u64>,
        oom_kill_events: Option<u64>,
        sync_progress: &SyncProgressData,
        memory_stats: Option<&MemoryStatsData>,
    ) {
        let sync_progress_bytes = match serde_json::to_vec(sync_progress) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("Failed to serialize sync progress: {}", e);
                return;
            }
        };
        let memory_stats_bytes = match memory_stats.map(serde_json::to_vec).transpose() {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("Failed to serialize memory stats: {}", e);
                return;
            }
        };
        if let Err(e) = self.store.commit_heartbeat_tick(HeartbeatTick {
            run_id,
            current_block,
            target_block,
            stage,
            oom_events,
            oom_kill_events,
            sync_progress: &sync_progress_bytes,
            memory_stats: memory_stats_bytes.as_deref(),
        }) {
            warn!(run_id, "Failed to persist heartbeat tick: {}", e);
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
            total_transactions: total_tx,
            total_cells,
            total_live_cells,
            total_addresses: 0,
            last_synced_at: sync.last_synced_at,
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
        for proposal in proposals {
            if let Err(e) = self.store.put_pending_proposal(proposal) {
                warn!(
                    "Failed to write pending proposal {}: {}",
                    proposal.proposal_id, e
                );
            }
        }
    }

    pub async fn remove_committed_proposals(&self, proposal_ids: &[String]) {
        for proposal_id in proposal_ids {
            if let Err(e) = self.store.delete_pending_proposal(proposal_id) {
                warn!("Failed to delete committed proposal {}: {}", proposal_id, e);
            }
        }
    }

    pub async fn cleanup_expired_proposals(&self, current_tip: i64) {
        match self.store.delete_expired_proposals(current_tip) {
            Ok(count) if count > 0 => {
                info!("Cleaned up {} expired proposals", count);
            }
            Err(e) => {
                warn!("Failed to cleanup expired proposals: {}", e);
            }
            _ => {}
        }
    }

    pub async fn get_pending_proposals(&self) -> Vec<CachedProposal> {
        match self.store.get_all_pending_proposals() {
            Ok(mut proposals) => {
                proposals.sort_by(|a, b| {
                    b.proposed_at_block
                        .cmp(&a.proposed_at_block)
                        .then(a.proposed_at_index.cmp(&b.proposed_at_index))
                });
                proposals
            }
            Err(e) => {
                warn!("Failed to read pending proposals: {}", e);
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sync_progress_fixture() -> SyncProgressData {
        SyncProgressData {
            current_block: 0,
            target_block: 0,
            last_batch_blocks: None,
            blocks_per_second: 0.0,
            ema_blocks_per_second: 0.0,
            txs_per_second: None,
            ema_txs_per_second: None,
            eta_seconds: None,
            eta_formatted: String::new(),
            progress_percentage: 0.0,
            updated_at: 0,
            startup_phase: None,
            is_direct_db_read: false,
            db_write_ms: None,
            db_commit_ms: None,
            rpc_fetch_ms: None,
            pipeline: None,
            pipeline_reset_epoch: None,
            pipeline_reset_reason: None,
            bulk_build: None,
        }
    }

    fn make_test_invalidator() -> CacheInvalidator {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        // Leak tempdir to keep it alive for the test
        std::mem::forget(dir);
        CacheInvalidator::new(store)
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
            bulk_build: None,
        };
        store.mark_runtime_run_start("run-cache-1", 1000).unwrap();
        invalidator
            .publish_heartbeat_tick(
                "run-cache-1",
                1000,
                10000,
                Some("tip_sync"),
                None,
                None,
                &data,
                None,
            )
            .await;

        // Verify it was written
        let stored = store.get_sync_progress().unwrap();
        assert!(stored.is_some());
        let parsed: SyncProgressData = serde_json::from_slice(&stored.unwrap()).unwrap();
        assert_eq!(parsed.current_block, 1000);
        assert_eq!(parsed.target_block, 10000);

        let runtime = store.get_runtime_status().unwrap();
        assert_eq!(runtime.last_heartbeat_block, 1000);
        assert_eq!(runtime.last_heartbeat_target_block, 10000);
        // A tick without a memory sample leaves the slot untouched.
        assert!(store.get_memory_stats().unwrap().is_none());
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
        store.mark_runtime_run_start("run-cache-2", 0).unwrap();
        invalidator
            .publish_heartbeat_tick(
                "run-cache-2",
                0,
                0,
                None,
                None,
                None,
                &sync_progress_fixture(),
                Some(&data),
            )
            .await;

        let stored = store.get_memory_stats().unwrap();
        assert!(stored.is_some());
        let parsed: MemoryStatsData = serde_json::from_slice(&stored.unwrap()).unwrap();
        assert_eq!(parsed.live_cells_count, 1000);
    }
}
