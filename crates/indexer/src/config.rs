#![allow(dead_code)]

use serde::Deserialize;

/// Maximum reorg depth before triggering deep fork handling.
/// CKB finalizes after 24 blocks, 36 provides safety margin.
pub const DEEP_FORK_DEPTH: u64 = 36;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_clickhouse_url")]
    pub clickhouse_url: String,
    #[serde(default = "default_clickhouse_database")]
    pub clickhouse_database: String,
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
    /// Max parallel connections for index rebuild task
    #[serde(default = "default_index_rebuild_parallel")]
    pub index_rebuild_parallel: usize,
    /// Use Inserter API for cell writes instead of COPY statements
    #[serde(default = "default_use_inserter_api")]
    pub use_inserter_api: bool,
}

fn default_clickhouse_url() -> String {
    "http://localhost:8123".to_string()
}

fn default_clickhouse_database() -> String {
    "ckbadger".to_string()
}

fn default_batch_size() -> usize {
    // Batch size for bulk operations: larger batches reduce per-batch overhead
    // and improve throughput, but increase memory usage (~100KB per block).
    // 10000 blocks ≈ 1GB per batch. Can be overridden via BATCH_SIZE env var.
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
    // Pipeline buffer: number of batches that can be queued between stages.
    // With batch_size=10000, each buffer slot uses ~1GB. 4 slots = ~4GB max.
    // Can be overridden via PIPELINE_BUFFER env var.
    4
}

fn default_bulk_sync_threshold() -> u64 {
    DEEP_FORK_DEPTH * 2
}

fn default_fast_sync_mode() -> bool {
    true
}

fn default_index_rebuild_parallel() -> usize {
    10
}

fn default_use_inserter_api() -> bool {
    false
}

impl Config {
    pub fn from_env() -> Result<Self, config::ConfigError> {
        config::Config::builder()
            .add_source(
                config::Environment::default()
                    .try_parsing(true)
                    .separator("__"),
            )
            .set_default("clickhouse_url", default_clickhouse_url())?
            .set_default("clickhouse_database", default_clickhouse_database())?
            .set_default("batch_size", default_batch_size() as i64)?
            .set_default("poll_interval_ms", default_poll_interval_ms() as i64)?
            .set_default("confirmations", default_confirmations() as i64)?
            .set_default("parallel_fetch_size", default_parallel_fetch_size() as i64)?
            .set_default("pipeline_enabled", default_pipeline_enabled())?
            .set_default("pipeline_buffer", default_pipeline_buffer() as i64)?
            .set_default("bulk_sync_threshold", default_bulk_sync_threshold() as i64)?
            .set_default("fast_sync_mode", default_fast_sync_mode())?
            .set_default(
                "index_rebuild_parallel",
                default_index_rebuild_parallel() as i64,
            )?
            .set_default("use_inserter_api", default_use_inserter_api())?
            .set_override_option("ckb_rpc_url", std::env::var("CKB_RPC_URL").ok())?
            .set_override_option(
                "use_inserter_api",
                std::env::var("USE_INSERTER_API")
                    .ok()
                    .map(|v| v.to_lowercase() == "true"),
            )?
            .build()?
            .try_deserialize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_clickhouse_url() {
        assert_eq!(default_clickhouse_url(), "http://localhost:8123");
    }

    #[test]
    fn test_default_clickhouse_database() {
        assert_eq!(default_clickhouse_database(), "ckbadger");
    }

    #[test]
    fn test_default_batch_size() {
        assert_eq!(default_batch_size(), 10000);
    }

    #[test]
    fn test_default_pipeline_buffer() {
        assert_eq!(default_pipeline_buffer(), 4);
    }

    #[test]
    fn test_default_parallel_fetch_size() {
        assert_eq!(default_parallel_fetch_size(), 64);
    }

    #[test]
    fn test_pipeline_memory_budget() {
        let batch_size = default_batch_size();
        let buffer = default_pipeline_buffer();
        let bytes_per_block = 100 * 1024; // ~100KB
        let max_memory_gb = (batch_size * buffer * bytes_per_block) / (1024 * 1024 * 1024);
        assert!(
            max_memory_gb <= 20,
            "Pipeline memory budget should be <= 20GB, got {max_memory_gb}GB"
        );
    }

    #[test]
    fn test_bulk_sync_threshold_is_twice_deep_fork_depth() {
        assert_eq!(default_bulk_sync_threshold(), DEEP_FORK_DEPTH * 2);
        assert_eq!(default_bulk_sync_threshold(), 72);
    }

    #[test]
    fn test_bulk_sync_threshold_exceeds_finalization() {
        // CKB finalizes after 24 blocks. bulk_sync_threshold must exceed this
        // to ensure reorg checks are only skipped for finalized blocks.
        const CKB_FINALIZATION_DEPTH: u64 = 24;
        assert!(
            default_bulk_sync_threshold() > CKB_FINALIZATION_DEPTH,
            "bulk_sync_threshold ({}) must exceed CKB finalization depth ({})",
            default_bulk_sync_threshold(),
            CKB_FINALIZATION_DEPTH
        );
    }

    #[test]
    fn test_default_use_inserter_api() {
        assert!(!default_use_inserter_api());
    }
}
