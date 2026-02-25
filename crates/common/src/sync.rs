use serde::{Deserialize, Serialize};

pub const SYNC_STATUS_REDIS_KEY: &str = "sync:status";
pub const SYNC_PROGRESS_REDIS_KEY: &str = "sync:progress";
pub const MEMORY_STATS_REDIS_KEY: &str = "memory:stats";

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

    pub fn init_sync_start(&mut self, start_block: i64, is_bulk_sync: bool) {
        if is_bulk_sync {
            let should_start_new_bulk_session = self.sync_started_at.is_none()
                || self.bulk_sync_completed_at.is_some()
                || start_block < self.sync_started_block;

            if should_start_new_bulk_session {
                // New bulk session:
                // - no prior start recorded
                // - prior bulk already completed
                // - rollback behind previously recorded start
                self.sync_started_at = Some(chrono::Utc::now().timestamp());
                self.sync_started_block = start_block;
                self.bulk_sync_completed_at = None;
                self.bulk_sync_completed_block = None;
            }
        } else {
            self.sync_started_block = start_block;
        }
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgressData {
    pub current_block: u64,
    pub target_block: u64,
    /// Most recently committed batch size in blocks.
    #[serde(default)]
    pub last_batch_blocks: Option<u64>,
    pub blocks_per_second: f64,
    pub ema_blocks_per_second: f64,
    /// Current tx throughput in tx/s for committed writer batches.
    #[serde(default)]
    pub txs_per_second: Option<f64>,
    /// EMA tx throughput in tx/s for committed writer batches.
    #[serde(default)]
    pub ema_txs_per_second: Option<f64>,
    pub eta_seconds: Option<f64>,
    pub eta_formatted: String,
    pub progress_percentage: f64,
    pub updated_at: i64,
    /// Optional startup phase while indexer performs pre-sync initialization.
    #[serde(default)]
    pub startup_phase: Option<String>,
    /// True when reading blocks directly from CKB's RocksDB instead of JSON-RPC.
    #[serde(default)]
    pub is_direct_db_read: bool,
    /// DB write stage total time in ms for the last batch (from PerfStats).
    #[serde(default)]
    pub db_write_ms: Option<f64>,
    /// Pure RocksDB commit time in ms for the last batch (from PerfStats).
    #[serde(default)]
    pub db_commit_ms: Option<f64>,
    /// RPC fetch time in ms for the last batch (from PerfStats).
    #[serde(default)]
    pub rpc_fetch_ms: Option<f64>,
    /// Detailed pipeline stage timings and queue depth, when pipeline mode is enabled.
    #[serde(default)]
    pub pipeline: Option<PipelineProgressData>,
    /// Current pipeline reset epoch (increments on each reset).
    #[serde(default)]
    pub pipeline_reset_epoch: Option<u64>,
    /// Last known pipeline reset reason.
    #[serde(default)]
    pub pipeline_reset_reason: Option<String>,
    /// Adaptive target transactions per batch in bulk sync.
    #[serde(default)]
    pub adaptive_target_batch_txs: Option<u64>,
    /// Adaptive inflight batch limit in bulk sync.
    #[serde(default)]
    pub adaptive_inflight_limit: Option<u64>,
    /// Adaptive minimum target transactions per batch floor in bulk sync.
    #[serde(default)]
    pub adaptive_min_target_batch_txs: Option<u64>,
    /// Remaining cooldown steps before adaptive step-up is allowed.
    #[serde(default)]
    pub adaptive_cooldown_steps: Option<u64>,
    /// Last adaptive controller reason, when available.
    #[serde(default)]
    pub adaptive_last_reason: Option<String>,
    /// Monotonic adaptive adjustment sequence number.
    #[serde(default)]
    pub adaptive_adjustment_seq: Option<u64>,
    /// Consecutive adaptive backoff count.
    #[serde(default)]
    pub adaptive_backoff_streak: Option<u64>,
    /// Unix timestamp when adaptive controller last adjusted.
    #[serde(default)]
    pub adaptive_last_adjusted_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PipelineProgressData {
    /// Fetcher stage duration in ms for the most recent batch.
    pub fetch_ms: Option<f64>,
    /// Parser stage duration in ms for the most recent batch.
    pub parse_ms: Option<f64>,
    /// Writer stage duration in ms for the most recent batch.
    pub write_ms: Option<f64>,
    /// Writer stage pure RocksDB commit time in ms for the most recent batch.
    #[serde(default)]
    pub commit_ms: Option<f64>,
    /// Time writer spent waiting for parsed data in ms.
    pub writer_wait_ms: Option<f64>,
    /// Current queue depth between fetcher -> parser.
    pub fetch_queue_depth: Option<u64>,
    /// Queue capacity between fetcher -> parser.
    pub fetch_queue_capacity: Option<u64>,
    /// Current queue depth between parser -> writer.
    pub parse_queue_depth: Option<u64>,
    /// Queue capacity between parser -> writer.
    pub parse_queue_capacity: Option<u64>,
    /// Current queue depth observed by writer (parser -> writer channel).
    pub writer_queue_depth: Option<u64>,
    /// Queue capacity observed by writer (parser -> writer channel).
    pub writer_queue_capacity: Option<u64>,
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

/// Memory statistics for key indexer components.
/// Published to Redis for monitoring by TUI and other tools.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStatsData {
    /// Number of live (unspent) cells in RocksDB
    pub live_cells_count: u64,
    /// Number of consumed cells retained for reorg support
    pub consumed_cells_count: u64,
    /// Estimated storage bytes used by consumed_cells column family
    pub consumed_cells_bytes: u64,
    /// Source used to estimate consumed_cells_bytes: live/sst/mem/none
    #[serde(default)]
    pub consumed_cells_bytes_source: String,

    /// RocksDB memtable (write buffer) memory usage
    pub rocksdb_memtable_bytes: u64,
    /// RocksDB block cache usage
    pub rocksdb_block_cache_bytes: u64,
    /// RocksDB table readers memory estimate
    pub rocksdb_table_readers_bytes: u64,
    /// Total RocksDB memory usage
    pub rocksdb_total_bytes: u64,

    /// Number of block headers cached
    pub block_headers_count: u64,

    /// Whether bulk sync cell cache is enabled (retains all consumed cells)
    pub bulk_sync_cell_cache_enabled: bool,
    /// Whether currently in bulk sync mode (>1000 blocks behind)
    pub bulk_sync_mode: bool,

    /// Estimated bytes pending compaction
    #[serde(default)]
    pub compaction_pending_bytes: u64,
    /// Number of currently running compactions
    #[serde(default)]
    pub num_running_compactions: u64,
    /// Total SST file size on disk (all CFs)
    #[serde(default)]
    pub sst_files_size: u64,
    /// Total L0 files across all CFs (sum)
    #[serde(default)]
    pub l0_files_count: u64,
    /// Max L0 files in any single CF (the actual write stall trigger)
    #[serde(default)]
    pub l0_files_max: u64,
    /// Name of the CF with the most L0 files
    #[serde(default)]
    pub l0_worst_cf: String,
    /// Total immutable memtables across all CFs (waiting for flush)
    #[serde(default)]
    pub immutable_memtables: u64,
    /// Top column families by estimated live data size: (name, bytes)
    #[serde(default)]
    pub top_cf_sizes: Vec<(String, u64)>,

    /// Chain-level statistics (from SyncStatusData)
    #[serde(default)]
    pub total_transactions: i64,
    #[serde(default)]
    pub total_cells: i64,
    #[serde(default)]
    pub total_live_cells: i64,
    #[serde(default)]
    pub total_addresses: i64,

    /// Unix timestamp when this data was collected
    pub updated_at: i64,
}

impl MemoryStatsData {
    pub fn new() -> Self {
        Self {
            updated_at: chrono::Utc::now().timestamp(),
            ..Default::default()
        }
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
        };

        let json = serde_json::to_string(&status).unwrap();
        let parsed: SyncStatusData = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.tip_block_number, status.tip_block_number);
        assert_eq!(parsed.tip_block_hash, status.tip_block_hash);
    }

    #[test]
    fn test_init_sync_start_keeps_bulk_timing_for_resumed_bulk_session() {
        let mut status = SyncStatusData {
            sync_started_at: Some(123),
            sync_started_block: 10,
            bulk_sync_completed_at: None,
            bulk_sync_completed_block: None,
            ..Default::default()
        };

        status.init_sync_start(200, true);

        assert_eq!(status.sync_started_at, Some(123));
        assert_eq!(status.sync_started_block, 10);
        assert_eq!(status.bulk_sync_completed_at, None);
        assert_eq!(status.bulk_sync_completed_block, None);
    }

    #[test]
    fn test_init_sync_start_resets_bulk_timing_for_new_bulk_run_after_completion() {
        let mut status = SyncStatusData {
            sync_started_at: Some(123),
            sync_started_block: 10,
            bulk_sync_completed_at: Some(456),
            bulk_sync_completed_block: Some(999),
            ..Default::default()
        };

        status.init_sync_start(100, true);

        assert_eq!(status.sync_started_block, 100);
        assert!(status.sync_started_at.is_some());
        assert_ne!(status.sync_started_at, Some(123));
        assert_eq!(status.bulk_sync_completed_at, None);
        assert_eq!(status.bulk_sync_completed_block, None);
    }

    #[test]
    fn test_init_sync_start_non_bulk_does_not_clear_bulk_timing() {
        let mut status = SyncStatusData {
            sync_started_at: Some(123),
            sync_started_block: 10,
            bulk_sync_completed_at: Some(456),
            bulk_sync_completed_block: Some(999),
            ..Default::default()
        };

        status.init_sync_start(200, false);

        assert_eq!(status.sync_started_block, 200);
        assert_eq!(status.sync_started_at, Some(123));
        assert_eq!(status.bulk_sync_completed_at, Some(456));
        assert_eq!(status.bulk_sync_completed_block, Some(999));
    }

    #[test]
    fn test_init_sync_start_resets_bulk_timing_when_rolled_back_before_start() {
        let mut status = SyncStatusData {
            sync_started_at: Some(123),
            sync_started_block: 1000,
            bulk_sync_completed_at: None,
            bulk_sync_completed_block: None,
            ..Default::default()
        };

        status.init_sync_start(500, true);

        assert_eq!(status.sync_started_block, 500);
        assert!(status.sync_started_at.is_some());
        assert_ne!(status.sync_started_at, Some(123));
        assert_eq!(status.bulk_sync_completed_at, None);
        assert_eq!(status.bulk_sync_completed_block, None);
    }

    #[test]
    fn test_format_duration_smart() {
        assert_eq!(format_duration_smart(30.0), "30s");
        assert_eq!(format_duration_smart(90.0), "1m 30s");
        assert_eq!(format_duration_smart(3700.0), "1h 1m");
        assert_eq!(format_duration_smart(90000.0), "1d 1h");
    }

    #[test]
    fn test_memory_stats_serialization() {
        let stats = MemoryStatsData {
            live_cells_count: 45_000_000,
            consumed_cells_count: 12_000_000,
            consumed_cells_bytes: 14_000_000_000,
            consumed_cells_bytes_source: "live".to_string(),
            rocksdb_memtable_bytes: 1_000_000_000,
            rocksdb_block_cache_bytes: 512_000_000,
            rocksdb_table_readers_bytes: 100_000_000,
            rocksdb_total_bytes: 1_612_000_000,
            block_headers_count: 6_000_000,
            bulk_sync_cell_cache_enabled: true,
            bulk_sync_mode: true,
            compaction_pending_bytes: 500_000,
            num_running_compactions: 2,
            sst_files_size: 10_000_000_000,
            l0_files_count: 15,
            l0_files_max: 5,
            l0_worst_cf: "live_cells".to_string(),
            immutable_memtables: 3,
            top_cf_sizes: vec![
                ("live_cells".to_string(), 3_000_000_000),
                ("consumed_cells".to_string(), 2_500_000_000),
            ],
            total_transactions: 50_000_000,
            total_cells: 100_000_000,
            total_live_cells: 45_000_000,
            total_addresses: 2_000_000,
            updated_at: 1700000000,
        };

        let json = serde_json::to_string(&stats).unwrap();
        let parsed: MemoryStatsData = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.live_cells_count, stats.live_cells_count);
        assert_eq!(parsed.rocksdb_total_bytes, stats.rocksdb_total_bytes);
        assert_eq!(parsed.bulk_sync_mode, stats.bulk_sync_mode);
        assert_eq!(parsed.sst_files_size, stats.sst_files_size);
        assert_eq!(parsed.consumed_cells_bytes_source, "live");
        assert_eq!(parsed.top_cf_sizes.len(), 2);
        assert_eq!(parsed.total_transactions, 50_000_000);
    }

    #[test]
    fn test_memory_stats_deserialize_without_source_field() {
        let mut value = serde_json::to_value(MemoryStatsData::default()).unwrap();
        value["liveCellsCount"] = serde_json::json!(1);
        value["consumedCellsCount"] = serde_json::json!(2);
        value["consumedCellsBytes"] = serde_json::json!(3);
        if let Some(obj) = value.as_object_mut() {
            obj.remove("consumedCellsBytesSource");
        }
        let parsed: MemoryStatsData = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.consumed_cells_bytes_source, "");
    }

    #[test]
    fn test_sync_progress_pipeline_serialization() {
        let progress = SyncProgressData {
            current_block: 1000,
            target_block: 2000,
            last_batch_blocks: Some(512),
            blocks_per_second: 500.0,
            ema_blocks_per_second: 450.0,
            txs_per_second: Some(12_000.0),
            ema_txs_per_second: Some(11_100.0),
            eta_seconds: Some(2.0),
            eta_formatted: "2s".to_string(),
            progress_percentage: 50.0,
            updated_at: 1700000000,
            startup_phase: Some("rollback_cleanup".to_string()),
            is_direct_db_read: false,
            db_write_ms: Some(120.0),
            db_commit_ms: Some(50.0),
            rpc_fetch_ms: Some(45.0),
            pipeline: Some(PipelineProgressData {
                fetch_ms: Some(45.0),
                parse_ms: Some(80.0),
                write_ms: Some(120.0),
                commit_ms: Some(50.0),
                writer_wait_ms: Some(15.0),
                fetch_queue_depth: Some(2),
                fetch_queue_capacity: Some(16),
                parse_queue_depth: Some(5),
                parse_queue_capacity: Some(16),
                writer_queue_depth: Some(4),
                writer_queue_capacity: Some(16),
            }),
            pipeline_reset_epoch: Some(3),
            pipeline_reset_reason: Some("pipeline batch mismatch".to_string()),
            adaptive_target_batch_txs: Some(40_000),
            adaptive_inflight_limit: Some(3),
            adaptive_min_target_batch_txs: Some(10_000),
            adaptive_cooldown_steps: Some(2),
            adaptive_last_reason: Some("pressure_backoff".to_string()),
            adaptive_adjustment_seq: Some(42),
            adaptive_backoff_streak: Some(3),
            adaptive_last_adjusted_at: Some(1_700_000_123),
        };

        let json = serde_json::to_string(&progress).unwrap();
        let parsed: SyncProgressData = serde_json::from_str(&json).unwrap();

        let pipeline = parsed.pipeline.expect("pipeline should be present");
        assert_eq!(pipeline.fetch_ms, Some(45.0));
        assert_eq!(pipeline.parse_ms, Some(80.0));
        assert_eq!(pipeline.write_ms, Some(120.0));
        assert_eq!(pipeline.commit_ms, Some(50.0));
        assert_eq!(pipeline.fetch_queue_depth, Some(2));
        assert_eq!(pipeline.parse_queue_capacity, Some(16));
        assert_eq!(pipeline.writer_queue_depth, Some(4));
        assert_eq!(parsed.startup_phase.as_deref(), Some("rollback_cleanup"));
        assert_eq!(parsed.pipeline_reset_epoch, Some(3));
        assert_eq!(
            parsed.pipeline_reset_reason.as_deref(),
            Some("pipeline batch mismatch")
        );
        assert_eq!(parsed.last_batch_blocks, Some(512));
        assert_eq!(parsed.txs_per_second, Some(12_000.0));
        assert_eq!(parsed.ema_txs_per_second, Some(11_100.0));
        assert_eq!(parsed.adaptive_target_batch_txs, Some(40_000));
        assert_eq!(parsed.adaptive_inflight_limit, Some(3));
        assert_eq!(parsed.adaptive_min_target_batch_txs, Some(10_000));
        assert_eq!(parsed.adaptive_cooldown_steps, Some(2));
        assert_eq!(
            parsed.adaptive_last_reason.as_deref(),
            Some("pressure_backoff")
        );
        assert_eq!(parsed.adaptive_adjustment_seq, Some(42));
        assert_eq!(parsed.adaptive_backoff_streak, Some(3));
        assert_eq!(parsed.adaptive_last_adjusted_at, Some(1_700_000_123));
    }

    #[test]
    fn test_sync_progress_deserialize_without_adaptive_fields() {
        let mut value = serde_json::to_value(SyncProgressData {
            current_block: 1000,
            target_block: 2000,
            last_batch_blocks: Some(128),
            blocks_per_second: 500.0,
            ema_blocks_per_second: 450.0,
            txs_per_second: Some(12_000.0),
            ema_txs_per_second: Some(11_100.0),
            eta_seconds: Some(2.0),
            eta_formatted: "2s".to_string(),
            progress_percentage: 50.0,
            updated_at: 1700000000,
            startup_phase: None,
            is_direct_db_read: false,
            db_write_ms: None,
            db_commit_ms: None,
            rpc_fetch_ms: None,
            pipeline: None,
            pipeline_reset_epoch: Some(1),
            pipeline_reset_reason: Some("batch write failed".to_string()),
            adaptive_target_batch_txs: Some(1),
            adaptive_inflight_limit: Some(2),
            adaptive_min_target_batch_txs: Some(1),
            adaptive_cooldown_steps: Some(1),
            adaptive_last_reason: Some("healthy_step_up".to_string()),
            adaptive_adjustment_seq: Some(1),
            adaptive_backoff_streak: Some(0),
            adaptive_last_adjusted_at: Some(1),
        })
        .unwrap();
        if let Some(obj) = value.as_object_mut() {
            obj.remove("pipelineResetEpoch");
            obj.remove("pipelineResetReason");
            obj.remove("lastBatchBlocks");
            obj.remove("txsPerSecond");
            obj.remove("emaTxsPerSecond");
            obj.remove("adaptiveTargetBatchTxs");
            obj.remove("adaptiveInflightLimit");
            obj.remove("adaptiveMinTargetBatchTxs");
            obj.remove("adaptiveCooldownSteps");
            obj.remove("adaptiveLastReason");
            obj.remove("adaptiveAdjustmentSeq");
            obj.remove("adaptiveBackoffStreak");
            obj.remove("adaptiveLastAdjustedAt");
        }

        let parsed: SyncProgressData = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.pipeline_reset_epoch, None);
        assert_eq!(parsed.pipeline_reset_reason, None);
        assert_eq!(parsed.last_batch_blocks, None);
        assert_eq!(parsed.txs_per_second, None);
        assert_eq!(parsed.ema_txs_per_second, None);
        assert_eq!(parsed.adaptive_target_batch_txs, None);
        assert_eq!(parsed.adaptive_inflight_limit, None);
        assert_eq!(parsed.adaptive_min_target_batch_txs, None);
        assert_eq!(parsed.adaptive_cooldown_steps, None);
        assert_eq!(parsed.adaptive_last_reason, None);
        assert_eq!(parsed.adaptive_adjustment_seq, None);
        assert_eq!(parsed.adaptive_backoff_streak, None);
        assert_eq!(parsed.adaptive_last_adjusted_at, None);
    }
}
