use serde::Deserialize;

/// Maximum reorg depth before triggering deep fork handling.
/// CKB finalizes after 24 blocks, 36 provides safety margin.
pub const DEEP_FORK_DEPTH: u64 = 36;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
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
    /// Enable PostgreSQL COPY for bulk sync (faster initial sync)
    #[serde(default = "default_use_copy_bulk_sync")]
    pub use_copy_bulk_sync: bool,
    /// Number of connections in the COPY connection pool
    #[serde(default = "default_copy_pool_size")]
    pub copy_pool_size: usize,
    /// Max parallel connections for index rebuild task
    #[serde(default = "default_index_rebuild_parallel")]
    pub index_rebuild_parallel: usize,
    /// Apply PostgreSQL tuning for bulk sync optimization
    #[serde(default)]
    pub apply_pg_tuning: bool,
    /// Flush LiveCellStore to DB every N batches (default 100)
    #[serde(default = "default_live_cell_flush_interval")]
    pub live_cell_flush_interval: u64,
    /// Path to RocksDB live cell store
    #[serde(default = "default_live_cell_db_path")]
    pub live_cell_db_path: String,
    /// Keep all consumed cells in RocksDB during bulk sync to avoid PostgreSQL fallback queries.
    /// Requires ~15GB extra memory for full sync. Disable on low-memory machines (<32GB RAM).
    #[serde(default = "default_bulk_sync_cell_cache")]
    pub bulk_sync_cell_cache: bool,
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

fn default_use_copy_bulk_sync() -> bool {
    true
}

fn default_copy_pool_size() -> usize {
    24
}

fn default_index_rebuild_parallel() -> usize {
    10
}

fn default_live_cell_flush_interval() -> u64 {
    100
}

fn default_live_cell_db_path() -> String {
    "./data/live_cells".to_string()
}

fn default_bulk_sync_cell_cache() -> bool {
    true
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
        assert_eq!(default_pipeline_buffer(), 4);
    }

    #[test]
    fn test_default_copy_pool_size() {
        assert_eq!(default_copy_pool_size(), 24);
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
}
