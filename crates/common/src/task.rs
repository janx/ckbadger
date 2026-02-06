//! Task system types for background operations.
//!
//! Defines the core types used by both `ckbadger-task-runner` and `ckbadger-task-tui`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Task status enum matching database values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    CyclesBackfill,
    IndexRebuild,
    LabelImport,
    StatisticsRebuild,
    LiveCellsPopulate,
    SporeRebuild,
    ConsumedAtBackfill,
    SecondaryIssuanceBackfill,
    CellsStatusRebuild,
    ActivitiesRebuild,
    AddressBalancesRebuild,
    TokenRebuild,
    MnftRebuild,
    DotbitRebuild,
    DaoRebuild,
    TxBlockMapRebuild,
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
            TaskType::ConsumedAtBackfill => write!(f, "consumed_at_backfill"),
            TaskType::SecondaryIssuanceBackfill => write!(f, "secondary_issuance_backfill"),
            TaskType::CellsStatusRebuild => write!(f, "cells_status_rebuild"),
            TaskType::ActivitiesRebuild => write!(f, "activities_rebuild"),
            TaskType::AddressBalancesRebuild => write!(f, "address_balances_rebuild"),
            TaskType::TokenRebuild => write!(f, "token_rebuild"),
            TaskType::MnftRebuild => write!(f, "mnft_rebuild"),
            TaskType::DotbitRebuild => write!(f, "dotbit_rebuild"),
            TaskType::DaoRebuild => write!(f, "dao_rebuild"),
            TaskType::TxBlockMapRebuild => write!(f, "tx_block_map_rebuild"),
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
            "consumed_at_backfill" => Ok(TaskType::ConsumedAtBackfill),
            "secondary_issuance_backfill" => Ok(TaskType::SecondaryIssuanceBackfill),
            "cells_status_rebuild" => Ok(TaskType::CellsStatusRebuild),
            "activities_rebuild" => Ok(TaskType::ActivitiesRebuild),
            "address_balances_rebuild" => Ok(TaskType::AddressBalancesRebuild),
            "token_rebuild" => Ok(TaskType::TokenRebuild),
            "mnft_rebuild" => Ok(TaskType::MnftRebuild),
            "dotbit_rebuild" => Ok(TaskType::DotbitRebuild),
            "dao_rebuild" => Ok(TaskType::DaoRebuild),
            "tx_block_map_rebuild" => Ok(TaskType::TxBlockMapRebuild),
            _ => Err(anyhow::anyhow!("Invalid task type: {}", s)),
        }
    }
}

impl TaskType {
    /// Returns true if this task type requires bulk sync to be completed before execution.
    ///
    /// Tasks that depend on having complete/consistent blockchain data should not run
    /// during bulk sync, as they would produce incomplete or incorrect results.
    ///
    /// Safe tasks (can run anytime):
    /// - CyclesBackfill: RPC-based, independent of sync state
    /// - LabelImport: File-based, independent of sync state
    ///
    /// Unsafe tasks (require bulk sync completion):
    /// - IndexRebuild: Would slow down writes by 3-4x during sync
    /// - CellsStatusRebuild: Requires all transaction_inputs written
    /// - LiveCellsPopulate: Requires cache fully populated
    /// - ConsumedAtBackfill: Requires complete transaction history
    /// - SporeRebuild: Requires accurate cell status
    /// - StatisticsRebuild: Requires complete blockchain data
    /// - SecondaryIssuanceBackfill: Requires all blocks to exist
    pub fn requires_bulk_sync_completion(&self) -> bool {
        match self {
            TaskType::CyclesBackfill | TaskType::LabelImport => false,
            TaskType::IndexRebuild
            | TaskType::CellsStatusRebuild
            | TaskType::LiveCellsPopulate
            | TaskType::ConsumedAtBackfill
            | TaskType::SporeRebuild
            | TaskType::StatisticsRebuild
            | TaskType::SecondaryIssuanceBackfill
            | TaskType::ActivitiesRebuild
            | TaskType::AddressBalancesRebuild
            | TaskType::TokenRebuild
            | TaskType::MnftRebuild
            | TaskType::DotbitRebuild
            | TaskType::DaoRebuild
            | TaskType::TxBlockMapRebuild => true,
        }
    }
}

// ============================================
// Task Configuration Types
// ============================================

/// Configuration for cycles backfill task
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for CyclesBackfillConfig {
    fn default() -> Self {
        Self {
            ckb_rpc_url: String::new(),
            start_block: None,
            end_block: None,
            batch_size: default_cycles_batch_size(),
            concurrent_requests: default_concurrent_requests(),
        }
    }
}

fn default_cycles_batch_size() -> i64 {
    50
}
fn default_concurrent_requests() -> usize {
    32
}

/// Configuration for secondary issuance backfill task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryIssuanceBackfillConfig {
    /// CKB RPC URL to fetch economic state from
    pub ckb_rpc_url: String,
    /// Start from specific block (default: 1, skipping genesis which has no economic state)
    pub start_block: Option<i64>,
    /// End at specific block (None = current tip)
    pub end_block: Option<i64>,
    /// Batch size for processing blocks
    #[serde(default = "default_secondary_issuance_batch_size")]
    pub batch_size: i64,
    /// Number of concurrent RPC requests
    #[serde(default = "default_concurrent_requests")]
    pub concurrent_requests: usize,
}

impl Default for SecondaryIssuanceBackfillConfig {
    fn default() -> Self {
        Self {
            ckb_rpc_url: String::new(),
            start_block: None,
            end_block: None,
            batch_size: default_secondary_issuance_batch_size(),
            concurrent_requests: default_concurrent_requests(),
        }
    }
}

fn default_secondary_issuance_batch_size() -> i64 {
    1000
}

/// Configuration for index rebuild task
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for IndexRebuildConfig {
    fn default() -> Self {
        Self {
            parallel_connections: default_parallel_connections(),
            indexes: None,
            rebuild_constraints: default_rebuild_constraints(),
        }
    }
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCellsPopulateConfig {
    /// Batch size for COPY operations (default: 100,000)
    #[serde(default = "default_populate_batch_size")]
    pub batch_size: usize,
}

impl Default for LiveCellsPopulateConfig {
    fn default() -> Self {
        Self {
            batch_size: default_populate_batch_size(),
        }
    }
}

fn default_populate_batch_size() -> usize {
    100_000
}

/// Configuration for spore rebuild task
/// Rebuilds spore_cells.is_live status and spore_clusters.spores_count
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeRebuildConfig {
    /// CKB RPC URL for fetching full cell data (needed for cluster_id extraction)
    #[serde(default)]
    pub ckb_rpc_url: String,
    /// Batch size for processing spore cells (default: 10,000)
    #[serde(default = "default_spore_batch_size")]
    pub batch_size: usize,
}

fn default_spore_batch_size() -> usize {
    10_000
}

impl Default for SporeRebuildConfig {
    fn default() -> Self {
        Self {
            ckb_rpc_url: String::new(),
            batch_size: default_spore_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumedAtBackfillConfig {
    #[serde(default = "default_consumed_batch_size")]
    pub batch_size: i64,
}

fn default_consumed_batch_size() -> i64 {
    100_000
}

impl Default for ConsumedAtBackfillConfig {
    fn default() -> Self {
        Self {
            batch_size: default_consumed_batch_size(),
        }
    }
}

/// Configuration for cells status rebuild task
/// Rebuilds cells.status and consumed_at fields from transaction_inputs table
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellsStatusRebuildConfig {
    /// Batch size for processing blocks (default: 100,000)
    #[serde(default = "default_cells_status_batch_size")]
    pub batch_size: i64,
}

fn default_cells_status_batch_size() -> i64 {
    100_000
}

impl Default for CellsStatusRebuildConfig {
    fn default() -> Self {
        Self {
            batch_size: default_cells_status_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitiesRebuildConfig {
    #[serde(default = "default_activities_batch_size")]
    pub batch_size: i64,
}

fn default_activities_batch_size() -> i64 {
    10_000
}

impl Default for ActivitiesRebuildConfig {
    fn default() -> Self {
        Self {
            batch_size: default_activities_batch_size(),
        }
    }
}

/// Configuration for address balances rebuild task
/// Rebuilds address_balances table from cells table
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddressBalancesRebuildConfig {
    /// Not used currently, but reserved for future batching
    #[serde(default)]
    pub _reserved: Option<bool>,
}

/// Configuration for token rebuild task
/// Rebuilds tokens, token_balances, and udt_cells from cells table
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenRebuildConfig {
    #[serde(default = "default_token_rebuild_batch_size")]
    pub batch_size: i64,
}

fn default_token_rebuild_batch_size() -> i64 {
    10_000
}

impl Default for TokenRebuildConfig {
    fn default() -> Self {
        Self {
            batch_size: default_token_rebuild_batch_size(),
        }
    }
}

/// Configuration for M-NFT rebuild task
/// Rebuilds mnft_issuers, mnft_classes, mnft_tokens from cells table
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MnftRebuildConfig {
    #[serde(default = "default_mnft_rebuild_batch_size")]
    pub batch_size: i64,
}

fn default_mnft_rebuild_batch_size() -> i64 {
    10_000
}

impl Default for MnftRebuildConfig {
    fn default() -> Self {
        Self {
            batch_size: default_mnft_rebuild_batch_size(),
        }
    }
}

/// Configuration for DotBit rebuild task
/// Rebuilds dotbit_accounts from cells table
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DotbitRebuildConfig {
    #[serde(default = "default_dotbit_rebuild_batch_size")]
    pub batch_size: i64,
}

fn default_dotbit_rebuild_batch_size() -> i64 {
    10_000
}

impl Default for DotbitRebuildConfig {
    fn default() -> Self {
        Self {
            batch_size: default_dotbit_rebuild_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoRebuildConfig {
    #[serde(default = "default_dao_rebuild_batch_size")]
    pub batch_size: i64,
}

fn default_dao_rebuild_batch_size() -> i64 {
    10_000
}

impl Default for DaoRebuildConfig {
    fn default() -> Self {
        Self {
            batch_size: default_dao_rebuild_batch_size(),
        }
    }
}

/// Configuration for tx_block_map rebuild task
/// Rebuilds tx_block_map lookup table from transactions table
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TxBlockMapRebuildConfig {
    /// Not used currently, reserved for future batching
    #[serde(default)]
    pub _reserved: Option<bool>,
}

/// Result for tx_block_map rebuild task
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TxBlockMapRebuildResult {
    pub rows_inserted: i64,
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
    ConsumedAtBackfill(ConsumedAtBackfillConfig),
    SecondaryIssuanceBackfill(SecondaryIssuanceBackfillConfig),
    CellsStatusRebuild(CellsStatusRebuildConfig),
    ActivitiesRebuild(ActivitiesRebuildConfig),
    AddressBalancesRebuild(AddressBalancesRebuildConfig),
    TokenRebuild(TokenRebuildConfig),
    MnftRebuild(MnftRebuildConfig),
    DotbitRebuild(DotbitRebuildConfig),
    DaoRebuild(DaoRebuildConfig),
    TxBlockMapRebuild(TxBlockMapRebuildConfig),
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
            TaskConfig::ConsumedAtBackfill(_) => TaskType::ConsumedAtBackfill,
            TaskConfig::SecondaryIssuanceBackfill(_) => TaskType::SecondaryIssuanceBackfill,
            TaskConfig::CellsStatusRebuild(_) => TaskType::CellsStatusRebuild,
            TaskConfig::ActivitiesRebuild(_) => TaskType::ActivitiesRebuild,
            TaskConfig::AddressBalancesRebuild(_) => TaskType::AddressBalancesRebuild,
            TaskConfig::TokenRebuild(_) => TaskType::TokenRebuild,
            TaskConfig::MnftRebuild(_) => TaskType::MnftRebuild,
            TaskConfig::DotbitRebuild(_) => TaskType::DotbitRebuild,
            TaskConfig::DaoRebuild(_) => TaskType::DaoRebuild,
            TaskConfig::TxBlockMapRebuild(_) => TaskType::TxBlockMapRebuild,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConsumedAtBackfillResult {
    pub cells_updated: i64,
    pub blocks_processed: i64,
}

/// Progress details for secondary issuance backfill
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryIssuanceBackfillResult {
    pub blocks_processed: i64,
    pub blocks_total: i64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CellsStatusRebuildResult {
    pub cells_updated: i64,
    pub blocks_processed: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActivitiesRebuildResult {
    pub activities_created: i64,
    pub blocks_processed: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddressBalancesRebuildResult {
    pub addresses_updated: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenRebuildResult {
    pub tokens_created: i64,
    pub balances_updated: i64,
    pub udt_cells_created: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MnftRebuildResult {
    pub issuers_created: i64,
    pub classes_created: i64,
    pub tokens_created: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DotbitRebuildResult {
    pub accounts_created: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DaoRebuildResult {
    pub deposits_populated: i64,
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
    ConsumedAtBackfill(ConsumedAtBackfillResult),
    SecondaryIssuanceBackfill(SecondaryIssuanceBackfillResult),
    CellsStatusRebuild(CellsStatusRebuildResult),
    ActivitiesRebuild(ActivitiesRebuildResult),
    AddressBalancesRebuild(AddressBalancesRebuildResult),
    TokenRebuild(TokenRebuildResult),
    MnftRebuild(MnftRebuildResult),
    DotbitRebuild(DotbitRebuildResult),
    DaoRebuild(DaoRebuildResult),
    TxBlockMapRebuild(TxBlockMapRebuildResult),
}

// ============================================
// Main Task Entity
// ============================================

/// Rate sample for EMA calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateSample {
    /// Unix timestamp (milliseconds)
    pub ts: i64,
    /// Progress value at this timestamp
    pub v: i64,
}

/// Task entity matching the database schema
#[derive(Debug, Clone)]
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

    pub fn consumed_at_backfill(config: ConsumedAtBackfillConfig) -> Self {
        Self {
            task_type: TaskType::ConsumedAtBackfill,
            config: serde_json::to_value(TaskConfig::ConsumedAtBackfill(config))
                .expect("ConsumedAtBackfillConfig should be serializable"),
            priority: 7,
            max_retries: 2,
        }
    }

    pub fn secondary_issuance_backfill(config: SecondaryIssuanceBackfillConfig) -> Self {
        Self {
            task_type: TaskType::SecondaryIssuanceBackfill,
            config: serde_json::to_value(TaskConfig::SecondaryIssuanceBackfill(config))
                .expect("SecondaryIssuanceBackfillConfig should be serializable"),
            priority: 4,
            max_retries: 2,
        }
    }

    pub fn cells_status_rebuild(config: CellsStatusRebuildConfig) -> Self {
        Self {
            task_type: TaskType::CellsStatusRebuild,
            config: serde_json::to_value(TaskConfig::CellsStatusRebuild(config))
                .expect("CellsStatusRebuildConfig should be serializable"),
            priority: 9, // High priority - needed before statistics work correctly
            max_retries: 2,
        }
    }

    pub fn activities_rebuild(config: ActivitiesRebuildConfig) -> Self {
        Self {
            task_type: TaskType::ActivitiesRebuild,
            config: serde_json::to_value(TaskConfig::ActivitiesRebuild(config))
                .expect("ActivitiesRebuildConfig should be serializable"),
            priority: 7, // After bulk sync completes, before statistics
            max_retries: 2,
        }
    }

    pub fn address_balances_rebuild(config: AddressBalancesRebuildConfig) -> Self {
        Self {
            task_type: TaskType::AddressBalancesRebuild,
            config: serde_json::to_value(TaskConfig::AddressBalancesRebuild(config))
                .expect("AddressBalancesRebuildConfig should be serializable"),
            priority: 8, // After cells_status_rebuild (9), before token_rebuild
            max_retries: 2,
        }
    }

    pub fn token_rebuild(config: TokenRebuildConfig) -> Self {
        Self {
            task_type: TaskType::TokenRebuild,
            config: serde_json::to_value(TaskConfig::TokenRebuild(config))
                .expect("TokenRebuildConfig should be serializable"),
            priority: 7, // After bulk sync completes, after address balances
            max_retries: 2,
        }
    }

    pub fn mnft_rebuild(config: MnftRebuildConfig) -> Self {
        Self {
            task_type: TaskType::MnftRebuild,
            config: serde_json::to_value(TaskConfig::MnftRebuild(config))
                .expect("MnftRebuildConfig should be serializable"),
            priority: 6,
            max_retries: 2,
        }
    }

    pub fn dotbit_rebuild(config: DotbitRebuildConfig) -> Self {
        Self {
            task_type: TaskType::DotbitRebuild,
            config: serde_json::to_value(TaskConfig::DotbitRebuild(config))
                .expect("DotbitRebuildConfig should be serializable"),
            priority: 6,
            max_retries: 2,
        }
    }

    pub fn dao_rebuild(config: DaoRebuildConfig) -> Self {
        Self {
            task_type: TaskType::DaoRebuild,
            config: serde_json::to_value(TaskConfig::DaoRebuild(config))
                .expect("DaoRebuildConfig should be serializable"),
            priority: 8,
            max_retries: 2,
        }
    }

    pub fn tx_block_map_rebuild(config: TxBlockMapRebuildConfig) -> Self {
        Self {
            task_type: TaskType::TxBlockMapRebuild,
            config: serde_json::to_value(TaskConfig::TxBlockMapRebuild(config))
                .expect("TxBlockMapRebuildConfig should be serializable"),
            priority: 8,
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
        let now_ms = Utc::now().timestamp_millis();

        if let Some(last) = self.samples.last() {
            let dt_ms = now_ms - last.ts;
            if dt_ms > 0 {
                let dp = (progress - last.v) as f64;
                let dt_secs = dt_ms as f64 / 1000.0;
                let rate = dp / dt_secs;

                self.ema = Some(match self.ema {
                    Some(prev) => self.alpha * rate + (1.0 - self.alpha) * prev,
                    None => rate,
                });
            }
        }

        self.samples.push(RateSample {
            ts: now_ms,
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
        assert_eq!(
            TaskType::SecondaryIssuanceBackfill.to_string(),
            "secondary_issuance_backfill"
        );
        assert_eq!(
            TaskType::CellsStatusRebuild.to_string(),
            "cells_status_rebuild"
        );
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
    fn test_config_defaults_match_serde_defaults() {
        let live_cells = LiveCellsPopulateConfig::default();
        assert_eq!(live_cells.batch_size, 100_000);

        let cycles = CyclesBackfillConfig::default();
        assert_eq!(cycles.batch_size, 50);
        assert_eq!(cycles.concurrent_requests, 32);

        let index = IndexRebuildConfig::default();
        assert_eq!(index.parallel_connections, 10);
        assert!(index.rebuild_constraints);

        let secondary = SecondaryIssuanceBackfillConfig::default();
        assert_eq!(secondary.batch_size, 1000);
        assert_eq!(secondary.concurrent_requests, 32);
    }

    #[test]
    fn test_config_roundtrip_preserves_defaults() {
        let config = LiveCellsPopulateConfig::default();
        let json = serde_json::to_value(&config).unwrap();
        let restored: LiveCellsPopulateConfig = serde_json::from_value(json).unwrap();
        assert_eq!(restored.batch_size, 100_000);
    }

    #[test]
    fn test_spore_rebuild_builder() {
        let builder = TaskBuilder::spore_rebuild(SporeRebuildConfig::default());
        assert_eq!(builder.task_type(), TaskType::SporeRebuild);
        assert_eq!(builder.get_priority(), 6);
    }

    #[test]
    fn test_secondary_issuance_backfill_builder() {
        let builder =
            TaskBuilder::secondary_issuance_backfill(SecondaryIssuanceBackfillConfig::default());
        assert_eq!(builder.task_type(), TaskType::SecondaryIssuanceBackfill);
        assert_eq!(builder.get_priority(), 4);
    }

    #[test]
    fn test_consumed_at_backfill_builder() {
        let builder = TaskBuilder::consumed_at_backfill(ConsumedAtBackfillConfig::default());
        assert_eq!(builder.task_type(), TaskType::ConsumedAtBackfill);
        assert_eq!(builder.get_priority(), 7);
    }

    #[test]
    fn test_consumed_at_backfill_type_display() {
        assert_eq!(
            TaskType::ConsumedAtBackfill.to_string(),
            "consumed_at_backfill"
        );
    }

    #[test]
    fn test_consumed_at_backfill_type_parse() {
        assert_eq!(
            "consumed_at_backfill".parse::<TaskType>().unwrap(),
            TaskType::ConsumedAtBackfill
        );
    }

    #[test]
    fn test_secondary_issuance_backfill_type_parse() {
        assert_eq!(
            "secondary_issuance_backfill".parse::<TaskType>().unwrap(),
            TaskType::SecondaryIssuanceBackfill
        );
    }

    #[test]
    fn test_consumed_at_backfill_config_serialization() {
        let config = TaskConfig::ConsumedAtBackfill(ConsumedAtBackfillConfig { batch_size: 50000 });
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("consumed_at_backfill"));
        assert!(json.contains("batchSize"));

        let parsed: TaskConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_type(), TaskType::ConsumedAtBackfill);
    }

    #[test]
    fn test_secondary_issuance_backfill_config_serialization() {
        let config = TaskConfig::SecondaryIssuanceBackfill(SecondaryIssuanceBackfillConfig {
            ckb_rpc_url: "http://localhost:8114".to_string(),
            start_block: Some(0),
            end_block: Some(100),
            batch_size: 1000,
            concurrent_requests: 4,
        });

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("secondary_issuance_backfill"));
        assert!(json.contains("ckbRpcUrl"));
        assert!(json.contains("startBlock"));

        let parsed: TaskConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_type(), TaskType::SecondaryIssuanceBackfill);
    }

    #[test]
    fn test_cells_status_rebuild_builder() {
        let builder = TaskBuilder::cells_status_rebuild(CellsStatusRebuildConfig::default());
        assert_eq!(builder.task_type(), TaskType::CellsStatusRebuild);
        assert_eq!(builder.get_priority(), 9);
    }

    #[test]
    fn test_cells_status_rebuild_type_display() {
        assert_eq!(
            TaskType::CellsStatusRebuild.to_string(),
            "cells_status_rebuild"
        );
    }

    #[test]
    fn test_cells_status_rebuild_type_parse() {
        assert_eq!(
            "cells_status_rebuild".parse::<TaskType>().unwrap(),
            TaskType::CellsStatusRebuild
        );
    }

    #[test]
    fn test_cells_status_rebuild_config_serialization() {
        let config = TaskConfig::CellsStatusRebuild(CellsStatusRebuildConfig { batch_size: 50000 });
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("cells_status_rebuild"));
        assert!(json.contains("batchSize"));

        let parsed: TaskConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_type(), TaskType::CellsStatusRebuild);
    }

    #[test]
    fn test_requires_bulk_sync_completion_safe_tasks() {
        assert!(!TaskType::CyclesBackfill.requires_bulk_sync_completion());
        assert!(!TaskType::LabelImport.requires_bulk_sync_completion());
    }

    #[test]
    fn test_requires_bulk_sync_completion_unsafe_tasks() {
        assert!(TaskType::IndexRebuild.requires_bulk_sync_completion());
        assert!(TaskType::CellsStatusRebuild.requires_bulk_sync_completion());
        assert!(TaskType::LiveCellsPopulate.requires_bulk_sync_completion());
        assert!(TaskType::ConsumedAtBackfill.requires_bulk_sync_completion());
        assert!(TaskType::SporeRebuild.requires_bulk_sync_completion());
        assert!(TaskType::StatisticsRebuild.requires_bulk_sync_completion());
        assert!(TaskType::SecondaryIssuanceBackfill.requires_bulk_sync_completion());
        assert!(TaskType::AddressBalancesRebuild.requires_bulk_sync_completion());
        assert!(TaskType::TokenRebuild.requires_bulk_sync_completion());
        assert!(TaskType::MnftRebuild.requires_bulk_sync_completion());
        assert!(TaskType::DotbitRebuild.requires_bulk_sync_completion());
        assert!(TaskType::DaoRebuild.requires_bulk_sync_completion());
    }

    #[test]
    fn test_address_balances_rebuild_builder() {
        let builder =
            TaskBuilder::address_balances_rebuild(AddressBalancesRebuildConfig::default());
        assert_eq!(builder.task_type(), TaskType::AddressBalancesRebuild);
        assert_eq!(builder.get_priority(), 8);
    }

    #[test]
    fn test_address_balances_rebuild_type_display() {
        assert_eq!(
            TaskType::AddressBalancesRebuild.to_string(),
            "address_balances_rebuild"
        );
    }

    #[test]
    fn test_address_balances_rebuild_type_parse() {
        assert_eq!(
            "address_balances_rebuild".parse::<TaskType>().unwrap(),
            TaskType::AddressBalancesRebuild
        );
    }

    #[test]
    fn test_address_balances_rebuild_config_serialization() {
        let config = TaskConfig::AddressBalancesRebuild(AddressBalancesRebuildConfig::default());
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("address_balances_rebuild"));

        let parsed: TaskConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_type(), TaskType::AddressBalancesRebuild);
    }

    #[test]
    fn test_token_rebuild_builder() {
        let builder = TaskBuilder::token_rebuild(TokenRebuildConfig::default());
        assert_eq!(builder.task_type(), TaskType::TokenRebuild);
        assert_eq!(builder.get_priority(), 7);
    }

    #[test]
    fn test_token_rebuild_type_display() {
        assert_eq!(TaskType::TokenRebuild.to_string(), "token_rebuild");
    }

    #[test]
    fn test_token_rebuild_type_parse() {
        assert_eq!(
            "token_rebuild".parse::<TaskType>().unwrap(),
            TaskType::TokenRebuild
        );
    }

    #[test]
    fn test_token_rebuild_config_serialization() {
        let config = TaskConfig::TokenRebuild(TokenRebuildConfig { batch_size: 20000 });
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("token_rebuild"));
        assert!(json.contains("batchSize"));

        let parsed: TaskConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_type(), TaskType::TokenRebuild);
    }

    #[test]
    fn test_mnft_rebuild_builder() {
        let builder = TaskBuilder::mnft_rebuild(MnftRebuildConfig::default());
        assert_eq!(builder.task_type(), TaskType::MnftRebuild);
        assert_eq!(builder.get_priority(), 6);
    }

    #[test]
    fn test_mnft_rebuild_type_display() {
        assert_eq!(TaskType::MnftRebuild.to_string(), "mnft_rebuild");
    }

    #[test]
    fn test_mnft_rebuild_type_parse() {
        assert_eq!(
            "mnft_rebuild".parse::<TaskType>().unwrap(),
            TaskType::MnftRebuild
        );
    }

    #[test]
    fn test_mnft_rebuild_config_serialization() {
        let config = TaskConfig::MnftRebuild(MnftRebuildConfig { batch_size: 5000 });
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("mnft_rebuild"));
        assert!(json.contains("batchSize"));

        let parsed: TaskConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_type(), TaskType::MnftRebuild);
    }

    #[test]
    fn test_dotbit_rebuild_builder() {
        let builder = TaskBuilder::dotbit_rebuild(DotbitRebuildConfig::default());
        assert_eq!(builder.task_type(), TaskType::DotbitRebuild);
        assert_eq!(builder.get_priority(), 6);
    }

    #[test]
    fn test_dotbit_rebuild_type_display() {
        assert_eq!(TaskType::DotbitRebuild.to_string(), "dotbit_rebuild");
    }

    #[test]
    fn test_dotbit_rebuild_type_parse() {
        assert_eq!(
            "dotbit_rebuild".parse::<TaskType>().unwrap(),
            TaskType::DotbitRebuild
        );
    }

    #[test]
    fn test_dotbit_rebuild_config_serialization() {
        let config = TaskConfig::DotbitRebuild(DotbitRebuildConfig { batch_size: 5000 });
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("dotbit_rebuild"));
        assert!(json.contains("batchSize"));

        let parsed: TaskConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_type(), TaskType::DotbitRebuild);
    }

    #[test]
    fn test_mnft_dotbit_require_bulk_sync_completion() {
        assert!(TaskType::MnftRebuild.requires_bulk_sync_completion());
        assert!(TaskType::DotbitRebuild.requires_bulk_sync_completion());
    }

    #[test]
    fn test_dao_rebuild_builder() {
        let builder = TaskBuilder::dao_rebuild(DaoRebuildConfig::default());
        assert_eq!(builder.task_type(), TaskType::DaoRebuild);
        assert_eq!(builder.get_priority(), 8);
    }

    #[test]
    fn test_dao_rebuild_type_display() {
        assert_eq!(TaskType::DaoRebuild.to_string(), "dao_rebuild");
    }

    #[test]
    fn test_dao_rebuild_type_parse() {
        assert_eq!(
            "dao_rebuild".parse::<TaskType>().unwrap(),
            TaskType::DaoRebuild
        );
    }

    #[test]
    fn test_dao_rebuild_config_serialization() {
        let config = TaskConfig::DaoRebuild(DaoRebuildConfig { batch_size: 5000 });
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("dao_rebuild"));
        assert!(json.contains("batchSize"));

        let parsed: TaskConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_type(), TaskType::DaoRebuild);
    }

    #[test]
    fn test_dao_rebuild_requires_bulk_sync_completion() {
        assert!(TaskType::DaoRebuild.requires_bulk_sync_completion());
    }
}
