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
    // 2000 blocks ≈ 200MB per batch. Can be overridden via BATCH_SIZE env var.
    2000
}

fn default_poll_interval_ms() -> u64 {
    1000
}

fn default_confirmations() -> u64 {
    0
}

fn default_parallel_fetch_size() -> usize {
    32
}

fn default_pipeline_enabled() -> bool {
    true
}

fn default_pipeline_buffer() -> usize {
    // Pipeline buffer: number of batches that can be queued between stages.
    // With batch_size=2000, each buffer slot uses ~200MB. 16 slots = ~3.2GB max.
    // Larger buffers allow better pipelining when writer is temporarily slow.
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
    8
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
