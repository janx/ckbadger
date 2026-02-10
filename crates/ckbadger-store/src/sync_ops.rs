//! Sync status operations.

use crate::keys::sync_meta_keys;
use crate::store::CkbadgerStore;
use crate::types::{DeepForkInfo, SyncStatus};

impl CkbadgerStore {
    pub fn get_sync_status(&self) -> anyhow::Result<SyncStatus> {
        match self.get_cf(self.cf_sync_meta(), sync_meta_keys::SYNC_STATUS)? {
            Some(value) => Ok(bincode::deserialize(&value)?),
            None => Ok(SyncStatus::default()),
        }
    }

    pub fn set_sync_status(&self, status: &SyncStatus) -> anyhow::Result<()> {
        let value = bincode::serialize(status)?;
        self.put_cf(self.cf_sync_meta(), sync_meta_keys::SYNC_STATUS, &value)
    }

    pub fn update_sync_status<F>(&self, update_fn: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut SyncStatus),
    {
        let mut status = self.get_sync_status()?;
        update_fn(&mut status);
        self.set_sync_status(&status)
    }

    /// Get sync tip (block number and hash) from the sync_status.
    pub fn get_sync_tip(&self) -> anyhow::Result<(i64, Option<Vec<u8>>)> {
        let status = self.get_sync_status()?;
        let hash = if status.tip_block_hash.is_empty() {
            None
        } else {
            Some(status.tip_block_hash)
        };
        Ok((status.tip_block_number, hash))
    }

    /// Update sync tip.
    pub fn update_sync_tip(
        &self,
        block_number: i64,
        block_hash: &[u8],
        tx_count_delta: i64,
    ) -> anyhow::Result<()> {
        self.update_sync_status(|status| {
            status.tip_block_number = block_number;
            status.tip_block_hash = block_hash.to_vec();
            status.total_transactions += tx_count_delta;
            status.last_synced_at = chrono::Utc::now().timestamp();
        })
    }

    /// Check if there's an unresolved deep fork.
    pub fn has_unresolved_deep_fork(&self) -> anyhow::Result<bool> {
        let status = self.get_sync_status()?;
        Ok(status.deep_fork_detected)
    }

    /// Get deep fork info.
    pub fn get_deep_fork_info(&self) -> anyhow::Result<Option<DeepForkInfo>> {
        let status = self.get_sync_status()?;
        if status.deep_fork_detected {
            Ok(status.deep_fork_info)
        } else {
            Ok(None)
        }
    }

    /// Set deep fork detected.
    pub fn set_deep_fork(&self, info: DeepForkInfo) -> anyhow::Result<()> {
        self.update_sync_status(|status| {
            status.deep_fork_detected = true;
            status.deep_fork_info = Some(info);
        })
    }

    /// Clear deep fork.
    pub fn clear_deep_fork(&self) -> anyhow::Result<()> {
        self.update_sync_status(|status| {
            status.deep_fork_detected = false;
            status.deep_fork_info = None;
        })
    }

    /// Check if bulk sync is active by looking at block timestamps.
    pub fn is_bulk_sync_active_by_timestamp(&self) -> anyhow::Result<bool> {
        if let Some((_, header)) = self.get_sync_tip_block()? {
            let now = chrono::Utc::now().timestamp();
            let block_time = header.timestamp / 1000; // ms -> s
            Ok(now - block_time > 3600)
        } else {
            Ok(true)
        }
    }
}
