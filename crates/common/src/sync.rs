use serde::{Deserialize, Serialize};

pub const SYNC_PROGRESS_REDIS_KEY: &str = "sync:progress";

#[derive(Debug, Clone, Serialize, Deserialize)]
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
