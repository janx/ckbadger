use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub use crate::response::SyncStatusResponse as SyncStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum BroadcastMessage {
    #[serde(rename = "new_block", rename_all = "camelCase")]
    NewBlock {
        number: i64,
        hash: String,
        timestamp: String,
        transactions_count: i32,
        epoch_number: i64,
        epoch_index: i32,
        epoch_length: i32,
        avg_block_time: String,
        estimated_epoch_time: String,
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

pub struct WsManager {
    block_sender: broadcast::Sender<BroadcastMessage>,
    tx_sender: broadcast::Sender<BroadcastMessage>,
    reorg_sender: broadcast::Sender<BroadcastMessage>,
    activity_sender: broadcast::Sender<BroadcastMessage>,
    address_subscriptions: Arc<RwLock<HashMap<String, broadcast::Sender<BroadcastMessage>>>>,
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
            address_subscriptions: Arc::new(RwLock::new(HashMap::new())),
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

    pub async fn subscribe_address(
        &self,
        lock_hash: String,
    ) -> broadcast::Receiver<BroadcastMessage> {
        let mut subs = self.address_subscriptions.write().await;
        if let Some(sender) = subs.get(&lock_hash) {
            sender.subscribe()
        } else {
            let (sender, receiver) = broadcast::channel(256);
            subs.insert(lock_hash, sender);
            receiver
        }
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
