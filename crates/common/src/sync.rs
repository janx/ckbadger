use serde::{Deserialize, Serialize};

pub const SYNC_STATUS_REDIS_KEY: &str = "sync:status";
pub const SYNC_PROGRESS_REDIS_KEY: &str = "sync:progress";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusData {
    pub tip_block_number: i64,
    #[serde(default)]
    pub tip_block_hash: String,
    pub total_transactions: i64,
    pub total_cells: i64,
    pub total_live_cells: i64,
    pub total_addresses: i64,
    pub last_synced_at: i64,

    pub sync_started_at: Option<i64>,
    pub sync_started_block: i64,
    pub sync_ema_rate: Option<f64>,

    /// Timestamp when bulk sync completed (caught up to chain tip)
    pub bulk_sync_completed_at: Option<i64>,
    /// Chain tip block number when bulk sync completed
    pub bulk_sync_completed_block: Option<i64>,

    #[serde(default)]
    pub indexes_deferred: bool,
    pub indexes_dropped_at: Option<i64>,
    pub indexes_rebuild_started_at: Option<i64>,
    pub indexes_rebuild_completed_at: Option<i64>,
    pub indexes_rebuild_progress: Option<IndexRebuildProgressData>,
}

impl SyncStatusData {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_batch(
        &mut self,
        block_number: i64,
        block_hash: &str,
        tx_count: i64,
        cells_created: i64,
        cells_consumed: i64,
        new_addresses: i64,
        ema_rate: Option<f64>,
    ) {
        self.tip_block_number = block_number;
        self.tip_block_hash = block_hash.to_string();
        self.total_transactions += tx_count;
        self.total_cells += cells_created;
        self.total_live_cells += cells_created - cells_consumed;
        self.total_addresses += new_addresses;
        self.last_synced_at = chrono::Utc::now().timestamp();
        if let Some(rate) = ema_rate {
            self.sync_ema_rate = Some(rate);
        }
    }

    pub fn init_sync_start(&mut self, start_block: i64) {
        self.sync_started_at = Some(chrono::Utc::now().timestamp());
        self.sync_started_block = start_block;
    }

    pub fn mark_bulk_sync_completed(&mut self, chain_tip: i64) {
        if self.bulk_sync_completed_at.is_none() {
            self.bulk_sync_completed_at = Some(chrono::Utc::now().timestamp());
            self.bulk_sync_completed_block = Some(chain_tip);
        }
    }

    pub fn bulk_sync_elapsed_seconds(&self) -> Option<i64> {
        let started = self.sync_started_at?;
        let completed = self
            .bulk_sync_completed_at
            .unwrap_or_else(|| chrono::Utc::now().timestamp());
        Some(completed - started)
    }

    pub fn bulk_sync_total_seconds(&self) -> Option<i64> {
        let started = self.sync_started_at?;
        let completed = self.bulk_sync_completed_at?;
        Some(completed - started)
    }

    pub fn set_indexes_deferred(&mut self, deferred: bool) {
        self.indexes_deferred = deferred;
        if deferred {
            self.indexes_dropped_at = Some(chrono::Utc::now().timestamp());
        }
    }

    pub fn start_index_rebuild(&mut self, total: i32) {
        self.indexes_rebuild_started_at = Some(chrono::Utc::now().timestamp());
        self.indexes_rebuild_progress = Some(IndexRebuildProgressData {
            total,
            completed: 0,
            current_index: None,
            items: vec![],
        });
    }

    pub fn update_index_rebuild_progress(&mut self, progress: IndexRebuildProgressData) {
        self.indexes_rebuild_progress = Some(progress);
    }

    pub fn complete_index_rebuild(&mut self) {
        self.indexes_deferred = false;
        self.indexes_rebuild_completed_at = Some(chrono::Utc::now().timestamp());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IndexRebuildProgressData {
    pub total: i32,
    pub completed: i32,
    pub current_index: Option<String>,
    pub items: Vec<IndexRebuildItemData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRebuildItemData {
    pub name: String,
    pub status: String,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgressData {
    pub current_block: u64,
    pub target_block: u64,
    pub blocks_per_second: f64,
    pub ema_blocks_per_second: f64,
    pub eta_seconds: Option<f64>,
    pub eta_formatted: String,
    pub progress_percentage: f64,
    pub updated_at: i64,
}

pub fn format_duration_smart(total_secs: f64) -> String {
    let total_secs = total_secs.round() as u64;

    if total_secs < 60 {
        return format!("{}s", total_secs);
    }

    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m {}s", minutes, seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_status_update_batch() {
        let mut status = SyncStatusData::new();
        status.update_batch(100, "0xabc", 50, 100, 30, 10, Some(1000.0));

        assert_eq!(status.tip_block_number, 100);
        assert_eq!(status.tip_block_hash, "0xabc");
        assert_eq!(status.total_transactions, 50);
        assert_eq!(status.total_cells, 100);
        assert_eq!(status.total_live_cells, 70);
        assert_eq!(status.total_addresses, 10);
        assert_eq!(status.sync_ema_rate, Some(1000.0));
    }

    #[test]
    fn test_sync_status_serialization() {
        let status = SyncStatusData {
            tip_block_number: 12345,
            tip_block_hash: "0xabc123".to_string(),
            total_transactions: 1000,
            total_cells: 500,
            total_live_cells: 300,
            total_addresses: 100,
            last_synced_at: 1700000000,
            sync_started_at: Some(1699999000),
            sync_started_block: 0,
            sync_ema_rate: Some(500.5),
            bulk_sync_completed_at: None,
            bulk_sync_completed_block: None,
            indexes_deferred: false,
            indexes_dropped_at: None,
            indexes_rebuild_started_at: None,
            indexes_rebuild_completed_at: None,
            indexes_rebuild_progress: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        let parsed: SyncStatusData = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.tip_block_number, status.tip_block_number);
        assert_eq!(parsed.tip_block_hash, status.tip_block_hash);
    }

    #[test]
    fn test_format_duration_smart() {
        assert_eq!(format_duration_smart(30.0), "30s");
        assert_eq!(format_duration_smart(90.0), "1m 30s");
        assert_eq!(format_duration_smart(3700.0), "1h 1m");
        assert_eq!(format_duration_smart(90000.0), "1d 1h");
    }
}
