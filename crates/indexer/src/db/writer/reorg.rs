use anyhow::Result;
use chrono::Utc;
use tracing::info;

use ckbadger_store::types::{DeepForkInfo, ReorgEvent};

use super::BatchWriter;

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
        let event_key = format!("reorg:{}", Utc::now().timestamp_millis());
        let event_bytes = bincode::serialize(&event)?;
        let prefixed_key = ckbadger_store::keys::encode_sync_meta_stats_key(event_key.as_bytes());
        self.store
            .put_cf(self.store.cf_stats(), &prefixed_key, &event_bytes)?;

        // Update sync status with deep fork info
        self.store.set_deep_fork(DeepForkInfo {
            db_tip,
            db_tip_hash: db_tip_hash.to_vec(),
            chain_tip,
            chain_tip_hash: chain_tip_hash.to_vec(),
            depth: depth as i32,
            fork_point,
        })?;

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

        // Record the reorg event
        let event = ReorgEvent {
            detected_at: Utc::now().timestamp(),
            rollback_from: fork_point + 1,
            rollback_to: fork_point,
            depth,
        };
        let event_key = format!("reorg:{}", Utc::now().timestamp_millis());
        let event_bytes = bincode::serialize(&event)?;
        let prefixed_key = ckbadger_store::keys::encode_sync_meta_stats_key(event_key.as_bytes());
        self.store
            .put_cf(self.store.cf_stats(), &prefixed_key, &event_bytes)?;

        // Use the store's atomic rollback which handles all CFs
        self.store.rollback_to_block(fork_point)?;

        // Clear deep fork flag
        self.clear_deep_fork_flag()?;

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
