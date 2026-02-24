use serde::Deserialize;

/// Maximum reorg depth before triggering deep fork handling.
/// CKB finalizes after 24 blocks, 36 provides safety margin.
pub const DEEP_FORK_DEPTH: u64 = 36;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Path to ckbadger-store RocksDB data directory
    pub data_path: String,
    pub ckb_rpc_url: String,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub start_block: Option<u64>,
    #[serde(default = "default_confirmations")]
    pub confirmations: u64,
    #[serde(default = "default_parallel_fetch_size")]
    pub parallel_fetch_size: usize,
    #[serde(default = "default_pipeline_enabled")]
    pub pipeline_enabled: bool,
    #[serde(default = "default_pipeline_buffer")]
    pub pipeline_buffer: usize,
    #[serde(default)]
    pub redis_url: Option<String>,
    #[serde(default = "default_bulk_sync_threshold")]
    pub bulk_sync_threshold: u64,
    #[serde(default = "default_fast_sync_mode")]
    pub fast_sync_mode: bool,
    /// Path to CKB node's RocksDB data directory for direct reads.
    /// When set, the indexer reads blocks directly from CKB's RocksDB instead of via JSON-RPC.
    #[serde(default)]
    pub ckb_data_path: Option<String>,
    /// Path to token-labels repository for label import.
    #[serde(default = "default_token_labels_path")]
    pub token_labels_path: String,
    /// Force startup rollback cleanup before syncing.
    /// Used after unclean shutdowns to reconcile derived aggregates.
    #[serde(default = "default_force_startup_cleanup")]
    pub force_startup_cleanup: bool,
}

fn default_batch_size() -> usize {
    10000
}

fn default_poll_interval_ms() -> u64 {
    1000
}

fn default_confirmations() -> u64 {
    0
}

fn default_parallel_fetch_size() -> usize {
    64
}

fn default_pipeline_enabled() -> bool {
    true
}

fn default_pipeline_buffer() -> usize {
    16
}

fn default_bulk_sync_threshold() -> u64 {
    DEEP_FORK_DEPTH * 2
}

fn default_fast_sync_mode() -> bool {
    true
}

fn default_token_labels_path() -> String {
    "docs/token-labels".to_string()
}

fn default_force_startup_cleanup() -> bool {
    false
}

impl Config {
    pub fn from_env() -> Result<Self, config::ConfigError> {
        config::Config::builder()
            .add_source(config::Environment::default().separator("_"))
            .set_default("batch_size", default_batch_size() as i64)?
            .set_default("poll_interval_ms", default_poll_interval_ms() as i64)?
            .set_default("confirmations", default_confirmations() as i64)?
            .build()?
            .try_deserialize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_batch_size() {
        assert_eq!(default_batch_size(), 10000);
    }

    #[test]
    fn test_default_pipeline_buffer() {
        assert_eq!(default_pipeline_buffer(), 16);
    }

    #[test]
    fn test_default_parallel_fetch_size() {
        assert_eq!(default_parallel_fetch_size(), 64);
    }

    #[test]
    fn test_bulk_sync_threshold_is_twice_deep_fork_depth() {
        assert_eq!(default_bulk_sync_threshold(), DEEP_FORK_DEPTH * 2);
        assert_eq!(default_bulk_sync_threshold(), 72);
    }

    #[test]
    fn test_bulk_sync_threshold_exceeds_finalization() {
        const CKB_FINALIZATION_DEPTH: u64 = 24;
        assert!(
            default_bulk_sync_threshold() > CKB_FINALIZATION_DEPTH,
            "bulk_sync_threshold ({}) must exceed CKB finalization depth ({})",
            default_bulk_sync_threshold(),
            CKB_FINALIZATION_DEPTH
        );
    }

    #[test]
    fn test_default_token_labels_path() {
        assert_eq!(default_token_labels_path(), "docs/token-labels");
    }

    #[test]
    fn test_default_force_startup_cleanup() {
        assert!(!default_force_startup_cleanup());
    }
}
