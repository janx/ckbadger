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
    #[serde(default)]
    pub bulk_sync_mode: bool,
    #[serde(default = "default_bulk_sync_threshold")]
    pub bulk_sync_threshold: u64,
}

fn default_batch_size() -> usize {
    1000
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
    6
}

fn default_bulk_sync_threshold() -> u64 {
    1000
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

    #[cfg(test)]
    pub fn test_config() -> Self {
        Self {
            database_url: "postgres://localhost/test".to_string(),
            ckb_rpc_url: "http://localhost:8114".to_string(),
            batch_size: default_batch_size(),
            poll_interval_ms: default_poll_interval_ms(),
            start_block: None,
            confirmations: default_confirmations(),
            parallel_fetch_size: default_parallel_fetch_size(),
            pipeline_enabled: default_pipeline_enabled(),
            pipeline_buffer: default_pipeline_buffer(),
            redis_url: None,
            bulk_sync_mode: false,
            bulk_sync_threshold: default_bulk_sync_threshold(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_bulk_sync_threshold() {
        assert_eq!(default_bulk_sync_threshold(), 1000);
    }

    #[test]
    fn test_config_bulk_sync_defaults() {
        let config = Config::test_config();
        assert!(!config.bulk_sync_mode);
        assert_eq!(config.bulk_sync_threshold, 1000);
    }

    #[test]
    fn test_config_with_bulk_sync_enabled() {
        let mut config = Config::test_config();
        config.bulk_sync_mode = true;
        config.bulk_sync_threshold = 5000;

        assert!(config.bulk_sync_mode);
        assert_eq!(config.bulk_sync_threshold, 5000);
    }

    #[test]
    fn test_bulk_sync_active_calculation() {
        let config = Config::test_config();
        let threshold = config.bulk_sync_threshold;

        fn is_bulk_sync_active(
            bulk_sync_mode: bool,
            blocks_remaining: u64,
            threshold: u64,
        ) -> bool {
            bulk_sync_mode && blocks_remaining > threshold
        }

        assert!(!is_bulk_sync_active(false, 10000, threshold));
        assert!(!is_bulk_sync_active(true, 500, threshold));
        assert!(!is_bulk_sync_active(true, 1000, threshold));
        assert!(is_bulk_sync_active(true, 1001, threshold));
        assert!(is_bulk_sync_active(true, 10000, threshold));
    }
}
