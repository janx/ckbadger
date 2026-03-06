use anyhow::{bail, Result};
use ckbadger_store::StoreRuntimeConfig;
use serde::Deserialize;

/// Maximum reorg depth before triggering deep fork handling.
/// CKB finalizes after 24 blocks, 36 provides safety margin.
pub const DEEP_FORK_DEPTH: u64 = 36;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Path to ckbadger domain RocksDB data directory.
    pub domain_data_path: String,
    /// Path to ckbadger append-only RocksDB data directory.
    pub append_only_data_path: String,
    /// Root directory for bulk-sync perf artifacts.
    #[serde(default)]
    pub bulk_sync_perf_output_root: String,
    pub ckb_rpc_url: String,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub start_block: Option<u64>,
    #[serde(default = "default_parallel_fetch_size")]
    pub parallel_fetch_size: usize,
    #[serde(default = "default_pipeline_enabled")]
    pub pipeline_enabled: bool,
    #[serde(default = "default_pipeline_buffer")]
    pub pipeline_buffer: usize,
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
    /// Used after unclean shutdowns to reconcile append-only aggregates.
    #[serde(default = "default_force_startup_cleanup")]
    pub force_startup_cleanup: bool,
    #[serde(default)]
    pub store_runtime_config: StoreRuntimeConfig,
}

fn default_batch_size() -> usize {
    10000
}

fn default_poll_interval_ms() -> u64 {
    1000
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
    pub fn validate(&self) -> Result<()> {
        let ckb_data_path = self
            .ckb_data_path
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        if ckb_data_path.is_empty() {
            bail!("config: ckb_data_path is required and must not be blank");
        }
        if self.batch_size == 0 {
            bail!("config: batch_size must be > 0");
        }
        if self.pipeline_buffer == 0 {
            bail!("config: pipeline_buffer must be > 0");
        }
        if self.parallel_fetch_size == 0 {
            bail!("config: parallel_fetch_size must be > 0");
        }
        if self.store_runtime_config.memory_budget_gb == Some(0) {
            bail!("config: store.memory_budget_gb must be > 0 when set");
        }
        Ok(())
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

    fn make_valid_config() -> Config {
        Config {
            domain_data_path: "/tmp/test".to_string(),
            append_only_data_path: "/tmp/test-ao".to_string(),
            bulk_sync_perf_output_root: String::new(),
            ckb_rpc_url: "http://localhost:8114".to_string(),
            batch_size: 10000,
            poll_interval_ms: 1000,
            start_block: None,
            parallel_fetch_size: 64,
            pipeline_enabled: true,
            pipeline_buffer: 16,
            bulk_sync_threshold: 72,
            fast_sync_mode: true,
            ckb_data_path: Some("/var/lib/ckb/data/db".to_string()),
            token_labels_path: "docs/token-labels".to_string(),
            force_startup_cleanup: false,
            store_runtime_config: StoreRuntimeConfig::default(),
        }
    }

    #[test]
    fn test_validate_accepts_valid_config() {
        make_valid_config().validate().unwrap();
    }

    #[test]
    fn test_validate_rejects_missing_ckb_data_path() {
        let mut config = make_valid_config();
        config.ckb_data_path = None;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("ckb_data_path is required"));
    }

    #[test]
    fn test_validate_rejects_blank_ckb_data_path() {
        let mut config = make_valid_config();
        config.ckb_data_path = Some("   ".to_string());
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("ckb_data_path is required"));
    }

    #[test]
    fn test_validate_rejects_zero_batch_size() {
        let mut config = make_valid_config();
        config.batch_size = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("batch_size must be > 0"));
    }

    #[test]
    fn test_validate_rejects_zero_pipeline_buffer() {
        let mut config = make_valid_config();
        config.pipeline_buffer = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("pipeline_buffer must be > 0"));
    }

    #[test]
    fn test_validate_rejects_zero_parallel_fetch_size() {
        let mut config = make_valid_config();
        config.parallel_fetch_size = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("parallel_fetch_size must be > 0"));
    }

    #[test]
    fn test_validate_rejects_zero_store_memory_budget_override() {
        let mut config = make_valid_config();
        config.store_runtime_config.memory_budget_gb = Some(0);
        let err = config.validate().unwrap_err();
        assert!(err
            .to_string()
            .contains("store.memory_budget_gb must be > 0"));
    }
}
