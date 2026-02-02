//! Task system types for background operations.
//!
//! Defines the core types used by both `ckbadger-task-runner` and `ckbadger-task-tui`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Task status enum matching database values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Paused,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::Running => write!(f, "running"),
            TaskStatus::Completed => write!(f, "completed"),
            TaskStatus::Failed => write!(f, "failed"),
            TaskStatus::Cancelled => write!(f, "cancelled"),
            TaskStatus::Paused => write!(f, "paused"),
        }
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(TaskStatus::Pending),
            "running" => Ok(TaskStatus::Running),
            "completed" => Ok(TaskStatus::Completed),
            "failed" => Ok(TaskStatus::Failed),
            "cancelled" => Ok(TaskStatus::Cancelled),
            "paused" => Ok(TaskStatus::Paused),
            _ => Err(anyhow::anyhow!("Invalid task status: {}", s)),
        }
    }
}

/// Task type enum matching database values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    CyclesBackfill,
    IndexRebuild,
    LabelImport,
    StatisticsRebuild,
    LiveCellsPopulate,
    SporeRebuild,
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskType::CyclesBackfill => write!(f, "cycles_backfill"),
            TaskType::IndexRebuild => write!(f, "index_rebuild"),
            TaskType::LabelImport => write!(f, "label_import"),
            TaskType::StatisticsRebuild => write!(f, "statistics_rebuild"),
            TaskType::LiveCellsPopulate => write!(f, "live_cells_populate"),
            TaskType::SporeRebuild => write!(f, "spore_rebuild"),
        }
    }
}

impl std::str::FromStr for TaskType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cycles_backfill" => Ok(TaskType::CyclesBackfill),
            "index_rebuild" => Ok(TaskType::IndexRebuild),
            "label_import" => Ok(TaskType::LabelImport),
            "statistics_rebuild" => Ok(TaskType::StatisticsRebuild),
            "live_cells_populate" => Ok(TaskType::LiveCellsPopulate),
            "spore_rebuild" => Ok(TaskType::SporeRebuild),
            _ => Err(anyhow::anyhow!("Invalid task type: {}", s)),
        }
    }
}

// ============================================
// Task Configuration Types
// ============================================

/// Configuration for cycles backfill task
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CyclesBackfillConfig {
    /// CKB RPC URL to fetch cycles from
    pub ckb_rpc_url: String,
    /// Start from specific block (None = find missing automatically)
    pub start_block: Option<i64>,
    /// End at specific block (None = current tip)
    pub end_block: Option<i64>,
    /// Batch size for processing
    #[serde(default = "default_cycles_batch_size")]
    pub batch_size: i64,
    /// Number of concurrent RPC requests
    #[serde(default = "default_concurrent_requests")]
    pub concurrent_requests: usize,
}

fn default_cycles_batch_size() -> i64 {
    50
}
fn default_concurrent_requests() -> usize {
    4
}

/// Configuration for index rebuild task
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IndexRebuildConfig {
    /// Parallel connections per partitioned table
    #[serde(default = "default_parallel_connections")]
    pub parallel_connections: usize,
    /// Only rebuild specific indexes (None = all)
    pub indexes: Option<Vec<String>>,
    /// Also rebuild constraints
    #[serde(default = "default_rebuild_constraints")]
    pub rebuild_constraints: bool,
}

fn default_parallel_connections() -> usize {
    10
}
fn default_rebuild_constraints() -> bool {
    true
}

/// Configuration for label import task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelImportConfig {
    #[serde(default = "default_token_labels_path")]
    pub token_labels_path: String,
    #[serde(default = "default_network")]
    pub network: String,
    #[serde(default = "default_true")]
    pub import_udt: bool,
    #[serde(default = "default_true")]
    pub import_scripts: bool,
}

impl Default for LabelImportConfig {
    fn default() -> Self {
        Self {
            token_labels_path: default_token_labels_path(),
            network: default_network(),
            import_udt: true,
            import_scripts: true,
        }
    }
}

fn default_token_labels_path() -> String {
    "docs/token-labels".to_string()
}

fn default_network() -> String {
    "mainnet".to_string()
}
fn default_true() -> bool {
    true
}

/// Configuration for statistics rebuild task
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsRebuildConfig {
    /// Tables to rebuild (None = all 7 tables)
    pub tables: Option<Vec<String>>,
}

/// Configuration for live cells populate task
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LiveCellsPopulateConfig {
    /// Batch size for COPY operations (default: 100,000)
    #[serde(default = "default_populate_batch_size")]
    pub batch_size: usize,
}

fn default_populate_batch_size() -> usize {
    100_000
}

/// Configuration for spore rebuild task
/// Rebuilds spore_cells.is_live status and spore_clusters.spores_count
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SporeRebuildConfig {
    /// Batch size for processing spore cells (default: 10,000)
    #[serde(default = "default_spore_batch_size")]
    pub batch_size: usize,
}

fn default_spore_batch_size() -> usize {
    10_000
}

/// Unified task configuration enum
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskConfig {
    CyclesBackfill(CyclesBackfillConfig),
    IndexRebuild(IndexRebuildConfig),
    LabelImport(LabelImportConfig),
    StatisticsRebuild(StatisticsRebuildConfig),
    LiveCellsPopulate(LiveCellsPopulateConfig),
    SporeRebuild(SporeRebuildConfig),
}

impl TaskConfig {
    pub fn task_type(&self) -> TaskType {
        match self {
            TaskConfig::CyclesBackfill(_) => TaskType::CyclesBackfill,
            TaskConfig::IndexRebuild(_) => TaskType::IndexRebuild,
            TaskConfig::LabelImport(_) => TaskType::LabelImport,
            TaskConfig::StatisticsRebuild(_) => TaskType::StatisticsRebuild,
            TaskConfig::LiveCellsPopulate(_) => TaskType::LiveCellsPopulate,
            TaskConfig::SporeRebuild(_) => TaskType::SporeRebuild,
        }
    }
}

// ============================================
// Task Result/Progress Types
// ============================================

/// Progress details for cycles backfill
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CyclesBackfillResult {
    pub transactions_processed: i64,
    pub cycles_updated: i64,
    pub errors: Vec<String>,
}

/// Progress details for index rebuild
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IndexRebuildResult {
    /// Total indexes to rebuild
    pub total_indexes: usize,
    /// Completed indexes
    pub completed_indexes: usize,
    /// Currently rebuilding index name
    pub current_index: Option<String>,
    /// List of completed indexes with their durations (ms)
    pub completed: Vec<IndexCompletionInfo>,
    /// List of failed indexes with error messages
    pub failed: Vec<IndexFailureInfo>,
    /// Total constraints to rebuild
    pub total_constraints: usize,
    /// Completed constraints
    pub completed_constraints: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexCompletionInfo {
    pub name: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexFailureInfo {
    pub name: String,
    pub error: String,
}

/// Progress details for label import
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LabelImportResult {
    pub udt_labels_imported: i64,
    pub script_labels_imported: i64,
    pub errors: Vec<String>,
}

/// Progress details for statistics rebuild
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsRebuildResult {
    pub completed_tables: Vec<String>,
    pub current_table: Option<String>,
    pub failed: Vec<StatisticsFailureInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsFailureInfo {
    pub table: String,
    pub error: String,
}

/// Progress details for live cells populate
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LiveCellsPopulateResult {
    pub cells_populated: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SporeRebuildResult {
    pub spores_processed: i64,
    pub spores_marked_consumed: i64,
    pub clusters_updated: i64,
}

/// Unified task result enum
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskResult {
    CyclesBackfill(CyclesBackfillResult),
    IndexRebuild(IndexRebuildResult),
    LabelImport(LabelImportResult),
    StatisticsRebuild(StatisticsRebuildResult),
    LiveCellsPopulate(LiveCellsPopulateResult),
    SporeRebuild(SporeRebuildResult),
}

// ============================================
// Main Task Entity
// ============================================

/// Rate sample for EMA calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateSample {
    /// Unix timestamp (seconds)
    pub ts: i64,
    /// Progress value at this timestamp
    pub v: i64,
}

/// Task entity matching the database schema
#[derive(Debug, Clone, FromRow)]
pub struct Task {
    pub id: Uuid,
    pub task_type: String,
    pub status: String,
    pub priority: i32,
    pub config: serde_json::Value,
    pub progress_total: Option<i64>,
    pub progress_current: Option<i64>,
    pub progress_message: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub runner_id: Option<String>,
    pub retry_count: i32,
    pub max_retries: i32,
    pub rate_samples: Option<serde_json::Value>,
    pub rate_ema: Option<f64>,
    pub log_tail: Option<String>,
}

impl Task {
    /// Parse task type from string
    pub fn task_type_enum(&self) -> Option<TaskType> {
        self.task_type.parse().ok()
    }

    /// Parse status from string
    pub fn status_enum(&self) -> Option<TaskStatus> {
        self.status.parse().ok()
    }

    /// Parse config into typed config
    pub fn config_typed(&self) -> Option<TaskConfig> {
        serde_json::from_value(self.config.clone()).ok()
    }

    /// Parse result into typed result
    pub fn result_typed(&self) -> Option<TaskResult> {
        self.result
            .as_ref()
            .and_then(|r| serde_json::from_value(r.clone()).ok())
    }

    /// Calculate progress percentage (0.0 - 100.0)
    pub fn progress_percent(&self) -> f64 {
        match (self.progress_current, self.progress_total) {
            (Some(current), Some(total)) if total > 0 => (current as f64 / total as f64) * 100.0,
            _ => 0.0,
        }
    }

    /// Calculate ETA based on rate_ema
    pub fn eta_seconds(&self) -> Option<f64> {
        let rate = self.rate_ema?;
        let current = self.progress_current?;
        let total = self.progress_total?;

        if rate <= 0.0 || current >= total {
            return None;
        }

        let remaining = total - current;
        Some(remaining as f64 / rate)
    }

    /// Format ETA as human-readable string
    pub fn eta_formatted(&self) -> Option<String> {
        let seconds = self.eta_seconds()?;
        Some(format_duration(seconds as u64))
    }

    /// Get elapsed time since task started (or total duration if completed)
    pub fn elapsed_seconds(&self) -> Option<i64> {
        let started = self.started_at?;
        let end_time = self.completed_at.unwrap_or_else(Utc::now);
        Some(end_time.signed_duration_since(started).num_seconds())
    }

    /// Format elapsed time as human-readable string
    pub fn elapsed_formatted(&self) -> Option<String> {
        self.elapsed_seconds()
            .map(|s| format_duration(s.unsigned_abs()))
    }
}

/// Format seconds as human-readable duration
pub fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        let mins = seconds / 60;
        let secs = seconds % 60;
        if secs > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}m", mins)
        }
    } else {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    }
}

// ============================================
// Task Creation Helpers
// ============================================

/// Builder for creating new tasks
#[derive(Debug, Clone)]
pub struct TaskBuilder {
    task_type: TaskType,
    config: serde_json::Value,
    priority: i32,
    max_retries: i32,
}

impl TaskBuilder {
    pub fn cycles_backfill(config: CyclesBackfillConfig) -> Self {
        Self {
            task_type: TaskType::CyclesBackfill,
            config: serde_json::to_value(TaskConfig::CyclesBackfill(config))
                .expect("CyclesBackfillConfig should be serializable"),
            priority: 0,
            max_retries: 3,
        }
    }

    pub fn index_rebuild(config: IndexRebuildConfig) -> Self {
        Self {
            task_type: TaskType::IndexRebuild,
            config: serde_json::to_value(TaskConfig::IndexRebuild(config))
                .expect("IndexRebuildConfig should be serializable"),
            priority: 10,   // Higher priority by default
            max_retries: 1, // Index rebuilds should not retry by default
        }
    }

    pub fn label_import(config: LabelImportConfig) -> Self {
        Self {
            task_type: TaskType::LabelImport,
            config: serde_json::to_value(TaskConfig::LabelImport(config))
                .expect("LabelImportConfig should be serializable"),
            priority: 0,
            max_retries: 3,
        }
    }

    pub fn statistics_rebuild(config: StatisticsRebuildConfig) -> Self {
        Self {
            task_type: TaskType::StatisticsRebuild,
            config: serde_json::to_value(TaskConfig::StatisticsRebuild(config))
                .expect("StatisticsRebuildConfig should be serializable"),
            priority: 5,
            max_retries: 2,
        }
    }

    pub fn live_cells_populate(config: LiveCellsPopulateConfig) -> Self {
        Self {
            task_type: TaskType::LiveCellsPopulate,
            config: serde_json::to_value(TaskConfig::LiveCellsPopulate(config))
                .expect("LiveCellsPopulateConfig should be serializable"),
            priority: 8,
            max_retries: 1,
        }
    }

    pub fn spore_rebuild(config: SporeRebuildConfig) -> Self {
        Self {
            task_type: TaskType::SporeRebuild,
            config: serde_json::to_value(TaskConfig::SporeRebuild(config))
                .expect("SporeRebuildConfig should be serializable"),
            priority: 6,
            max_retries: 2,
        }
    }

    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn max_retries(mut self, max_retries: i32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn task_type(&self) -> TaskType {
        self.task_type
    }

    pub fn config(&self) -> &serde_json::Value {
        &self.config
    }

    pub fn get_priority(&self) -> i32 {
        self.priority
    }

    pub fn get_max_retries(&self) -> i32 {
        self.max_retries
    }
}

// ============================================
// EMA Rate Calculation
// ============================================

/// EMA rate calculator for smooth progress estimation
pub struct RateCalculator {
    samples: Vec<RateSample>,
    ema: Option<f64>,
    alpha: f64,
    max_samples: usize,
}

impl Default for RateCalculator {
    fn default() -> Self {
        Self::new(0.1, 60)
    }
}

impl RateCalculator {
    /// Create a new rate calculator
    ///
    /// * `alpha` - EMA smoothing factor (0.1 = smooth, 0.3 = responsive)
    /// * `max_samples` - Maximum samples to keep for backup calculation
    pub fn new(alpha: f64, max_samples: usize) -> Self {
        Self {
            samples: Vec::new(),
            ema: None,
            alpha,
            max_samples,
        }
    }

    /// Add a progress sample and update EMA
    pub fn add_sample(&mut self, progress: i64) {
        let now = Utc::now().timestamp();

        if let Some(last) = self.samples.last() {
            let dt = (now - last.ts) as f64;
            if dt > 0.0 {
                let dp = (progress - last.v) as f64;
                let rate = dp / dt;

                self.ema = Some(match self.ema {
                    Some(prev) => self.alpha * rate + (1.0 - self.alpha) * prev,
                    None => rate,
                });
            }
        }

        self.samples.push(RateSample {
            ts: now,
            v: progress,
        });

        if self.samples.len() > self.max_samples {
            self.samples.remove(0);
        }
    }

    /// Get current EMA rate (items/sec)
    pub fn rate(&self) -> Option<f64> {
        self.ema
    }

    /// Get samples for serialization
    pub fn samples(&self) -> &[RateSample] {
        &self.samples
    }

    /// Restore from serialized state
    pub fn restore(
        samples: Vec<RateSample>,
        ema: Option<f64>,
        alpha: f64,
        max_samples: usize,
    ) -> Self {
        Self {
            samples,
            ema,
            alpha,
            max_samples,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_display() {
        assert_eq!(TaskStatus::Pending.to_string(), "pending");
        assert_eq!(TaskStatus::Running.to_string(), "running");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
    }

    #[test]
    fn test_task_status_parse() {
        assert_eq!(
            "pending".parse::<TaskStatus>().unwrap(),
            TaskStatus::Pending
        );
        assert_eq!(
            "running".parse::<TaskStatus>().unwrap(),
            TaskStatus::Running
        );
        assert!("invalid".parse::<TaskStatus>().is_err());
    }

    #[test]
    fn test_task_type_display() {
        assert_eq!(TaskType::CyclesBackfill.to_string(), "cycles_backfill");
        assert_eq!(TaskType::IndexRebuild.to_string(), "index_rebuild");
        assert_eq!(TaskType::LabelImport.to_string(), "label_import");
        assert_eq!(
            TaskType::StatisticsRebuild.to_string(),
            "statistics_rebuild"
        );
        assert_eq!(
            TaskType::LiveCellsPopulate.to_string(),
            "live_cells_populate"
        );
        assert_eq!(TaskType::SporeRebuild.to_string(), "spore_rebuild");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3660), "1h 1m");
        assert_eq!(format_duration(7200), "2h");
    }

    #[test]
    fn test_task_builder() {
        let config = CyclesBackfillConfig {
            ckb_rpc_url: "http://localhost:8114".to_string(),
            ..Default::default()
        };
        let builder = TaskBuilder::cycles_backfill(config)
            .priority(5)
            .max_retries(2);

        assert_eq!(builder.task_type(), TaskType::CyclesBackfill);
        assert_eq!(builder.get_priority(), 5);
        assert_eq!(builder.get_max_retries(), 2);
    }

    #[test]
    fn test_rate_calculator() {
        let mut calc = RateCalculator::new(0.5, 10);

        // Initially no rate
        assert!(calc.rate().is_none());

        // Add first sample (establishes baseline)
        calc.samples.push(RateSample { ts: 0, v: 0 });

        // Simulate adding sample 1 second later with 10 items processed
        calc.samples.push(RateSample { ts: 1, v: 10 });
        let rate = 10.0; // 10 items in 1 second
        calc.ema = Some(rate);

        assert!((calc.rate().unwrap() - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_task_config_serialization() {
        let config = TaskConfig::CyclesBackfill(CyclesBackfillConfig {
            ckb_rpc_url: "http://localhost:8114".to_string(),
            batch_size: 100,
            ..Default::default()
        });

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("cycles_backfill"));
        assert!(json.contains("ckbRpcUrl"));

        let parsed: TaskConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_type(), TaskType::CyclesBackfill);
    }

    #[test]
    fn test_index_rebuild_result() {
        let result = IndexRebuildResult {
            total_indexes: 26,
            completed_indexes: 5,
            current_index: Some("idx_blocks_timestamp".to_string()),
            completed: vec![
                IndexCompletionInfo {
                    name: "idx_1".to_string(),
                    duration_ms: 1000,
                },
                IndexCompletionInfo {
                    name: "idx_2".to_string(),
                    duration_ms: 2000,
                },
            ],
            failed: vec![],
            total_constraints: 5,
            completed_constraints: 0,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("totalIndexes"));
        assert!(json.contains("currentIndex"));
    }

    #[test]
    fn test_statistics_rebuild_builder() {
        let builder = TaskBuilder::statistics_rebuild(StatisticsRebuildConfig::default());
        assert_eq!(builder.task_type(), TaskType::StatisticsRebuild);
        assert_eq!(builder.get_priority(), 5);
    }

    #[test]
    fn test_live_cells_populate_builder() {
        let builder = TaskBuilder::live_cells_populate(LiveCellsPopulateConfig::default());
        assert_eq!(builder.task_type(), TaskType::LiveCellsPopulate);
        assert_eq!(builder.get_priority(), 8);
    }

    #[test]
    fn test_spore_rebuild_builder() {
        let builder = TaskBuilder::spore_rebuild(SporeRebuildConfig::default());
        assert_eq!(builder.task_type(), TaskType::SporeRebuild);
        assert_eq!(builder.get_priority(), 6);
    }
}
