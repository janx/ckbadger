use serde::Deserialize;

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
    // With batch_size=10000, each buffer slot uses ~1GB. 16 slots = ~16GB max.
    // Can be overridden via PIPELINE_BUFFER env var.
    16
}

fn default_bulk_sync_threshold() -> u64 {
    1000
}

fn default_fast_sync_mode() -> bool {
    true
}

fn default_use_copy_bulk_sync() -> bool {
    true
}

fn default_copy_pool_size() -> usize {
    // Number of parallel COPY connections for bulk insert.
    // More connections allow higher parallelism during bulk sync.
    24
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
}
