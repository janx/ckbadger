use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::broadcast;

pub use crate::response::SyncStatusResponse as SyncStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum BroadcastMessage {
    /// A block that just entered the store. Carries only facts read off that
    /// block plus the sync status.
    ///
    /// Rolling network statistics (average block time, epoch ETA) are
    /// deliberately absent: they are windowed aggregates anchored at the chain
    /// tip, not properties of a block, and `/statistics/network` is their single
    /// computation path. Deriving them per pushed block once produced a
    /// single-interval value (3.5s..33.2s block to block) that overwrote the
    /// window average in the client cache.
    #[serde(rename = "new_block", rename_all = "camelCase")]
    NewBlock {
        number: i64,
        hash: String,
        timestamp: String,
        transactions_count: i32,
        epoch_number: i64,
        epoch_index: i32,
        epoch_length: i32,
        sync_status: Box<SyncStatus>,
    },
    #[serde(rename = "new_transaction", rename_all = "camelCase")]
    NewTransaction {
        hash: String,
        block_number: i64,
        inputs_count: i32,
        outputs_count: i32,
        fee: String,
        timestamp: String,
    },
    #[serde(rename = "reorg", rename_all = "camelCase")]
    Reorg {
        depth: i32,
        old_tip: i64,
        new_tip: i64,
        fork_point: i64,
        orphaned_blocks: i32,
        orphaned_txs: i32,
        timestamp: String,
    },
    #[serde(rename = "deep_fork", rename_all = "camelCase")]
    DeepFork {
        detected: bool,
        depth: i32,
        db_tip: i64,
        chain_tip: i64,
        fork_point: i64,
        timestamp: String,
    },
    #[serde(rename = "latest_activities")]
    LatestActivities { activities: Vec<serde_json::Value> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    pub action: String,
    pub channel: Option<String>,
    pub lock_hash: Option<String>,
}

/// Maximum number of concurrent WebSocket connections.
const MAX_WS_CONNECTIONS: usize = 1024;

pub struct WsManager {
    block_sender: broadcast::Sender<BroadcastMessage>,
    tx_sender: broadcast::Sender<BroadcastMessage>,
    reorg_sender: broadcast::Sender<BroadcastMessage>,
    activity_sender: broadcast::Sender<BroadcastMessage>,
    active_connections: AtomicUsize,
}

impl WsManager {
    pub fn new() -> Self {
        let (block_sender, _) = broadcast::channel(1024);
        let (tx_sender, _) = broadcast::channel(1024);
        let (reorg_sender, _) = broadcast::channel(64);
        let (activity_sender, _) = broadcast::channel(256);

        Self {
            block_sender,
            tx_sender,
            reorg_sender,
            activity_sender,
            active_connections: AtomicUsize::new(0),
        }
    }

    pub fn subscribe_blocks(&self) -> broadcast::Receiver<BroadcastMessage> {
        self.block_sender.subscribe()
    }

    pub fn subscribe_transactions(&self) -> broadcast::Receiver<BroadcastMessage> {
        self.tx_sender.subscribe()
    }

    pub fn subscribe_reorgs(&self) -> broadcast::Receiver<BroadcastMessage> {
        self.reorg_sender.subscribe()
    }

    /// Try to acquire a connection slot. Returns `true` if accepted, `false` if at capacity.
    pub fn try_acquire_connection(&self) -> bool {
        let mut current = self.active_connections.load(Ordering::Relaxed);
        loop {
            if current >= MAX_WS_CONNECTIONS {
                return false;
            }
            match self.active_connections.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    /// Release a connection slot when a WebSocket disconnects.
    pub fn release_connection(&self) {
        self.active_connections.fetch_sub(1, Ordering::AcqRel);
    }

    /// Current number of active WebSocket connections.
    pub fn active_connection_count(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }

    pub fn broadcast_block(&self, msg: BroadcastMessage) {
        let _ = self.block_sender.send(msg);
    }

    pub fn broadcast_transaction(&self, msg: BroadcastMessage) {
        let _ = self.tx_sender.send(msg);
    }

    pub fn broadcast_reorg(&self, msg: BroadcastMessage) {
        let _ = self.reorg_sender.send(msg);
    }

    pub fn subscribe_activities(&self) -> broadcast::Receiver<BroadcastMessage> {
        self.activity_sender.subscribe()
    }

    pub fn broadcast_activities(&self, msg: BroadcastMessage) {
        let _ = self.activity_sender.send(msg);
    }
}

impl Default for WsManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_limit_accepts_within_capacity() {
        let mgr = WsManager::new();
        assert!(mgr.try_acquire_connection());
        assert_eq!(mgr.active_connection_count(), 1);
    }

    #[test]
    fn test_connection_limit_rejects_at_capacity() {
        let mgr = WsManager::new();
        for _ in 0..MAX_WS_CONNECTIONS {
            assert!(mgr.try_acquire_connection());
        }
        assert!(!mgr.try_acquire_connection());
        assert_eq!(mgr.active_connection_count(), MAX_WS_CONNECTIONS);
    }

    #[test]
    fn test_release_connection_frees_slot() {
        let mgr = WsManager::new();
        for _ in 0..MAX_WS_CONNECTIONS {
            mgr.try_acquire_connection();
        }
        assert!(!mgr.try_acquire_connection());

        mgr.release_connection();
        assert_eq!(mgr.active_connection_count(), MAX_WS_CONNECTIONS - 1);
        assert!(mgr.try_acquire_connection());
    }
}
