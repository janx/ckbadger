//! Core RocksDB store.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tracing::{error, info, warn};

use rocksdb::{
    ColumnFamily, ColumnFamilyDescriptor, DBCompactionStyle, DBCompressionType, IteratorMode,
    Options, UniversalCompactOptions, WriteBatch, WriteBufferManager, DB,
};
use serde::{Deserialize, Serialize};

use crate::keys;
use crate::types::MemoryStats;

/// Type alias for RocksDB iterator items to avoid complex type lint.
pub type KvResult = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>;

const GB: u64 = 1024 * 1024 * 1024;
const MB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreRuntimeConfig {
    pub memory_budget_gb: Option<u64>,
    pub direct_io_reads: bool,
}

impl Default for StoreRuntimeConfig {
    fn default() -> Self {
        Self {
            memory_budget_gb: None,
            direct_io_reads: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompactionPressureSnapshot {
    pub l0_files_total: u64,
    pub l0_files_max: u64,
    pub compaction_pending_bytes: u64,
    pub immutable_memtables: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondaryStoreOwner {
    Api,
    Tui,
    Cli,
}

impl SecondaryStoreOwner {
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Api => "api-secondary",
            Self::Tui => "tui-secondary",
            Self::Cli => "cli-secondary",
        }
    }
}

pub fn secondary_store_path<P: AsRef<Path>>(
    primary_path: P,
    owner: SecondaryStoreOwner,
) -> PathBuf {
    let mut path = primary_path.as_ref().as_os_str().to_os_string();
    path.push(format!("-{}", owner.suffix()));
    PathBuf::from(path)
}

pub fn known_domain_secondary_store_paths<P: AsRef<Path>>(primary_path: P) -> [PathBuf; 3] {
    let primary_path = primary_path.as_ref();
    [
        secondary_store_path(primary_path, SecondaryStoreOwner::Api),
        secondary_store_path(primary_path, SecondaryStoreOwner::Tui),
        secondary_store_path(primary_path, SecondaryStoreOwner::Cli),
    ]
}

pub fn known_append_only_secondary_store_paths<P: AsRef<Path>>(primary_path: P) -> [PathBuf; 1] {
    [secondary_store_path(primary_path, SecondaryStoreOwner::Api)]
}

/// Scales `base` by `scale` factor then clamps to `[min, max]`.
fn scale_clamp(base: u64, scale: f64, min: u64, max: u64) -> usize {
    let value = (base as f64 * scale).round() as u64;
    value.clamp(min, max) as usize
}

/// Auto-detected system memory profile that drives RocksDB memory parameters.
#[derive(Debug, Clone)]
pub struct MemoryProfile {
    pub system_ram_bytes: u64,
    pub cpu_count: usize,
    pub is_secondary: bool,
    pub rocksdb_budget_bytes: usize,
    pub wbm_normal_bytes: usize,
    pub block_cache_normal_bytes: usize,
    pub wbm_bulk_sync_bytes: usize,
    pub block_cache_bulk_sync_bytes: usize,
    pub write_buffer_mega_bytes: usize,
    pub write_buffer_high_bytes: usize,
    pub write_buffer_low_bytes: usize,
    pub write_buffer_hot_cf_bytes: usize,
    pub max_background_jobs: i32,
    pub max_subcompactions: u32,
    pub bulk_max_bytes_for_level_base: u64,
    pub bulk_target_file_size_base: u64,
    pub normal_max_bytes_for_level_base: u64,
    pub normal_target_file_size_base: u64,
    pub severe_compaction_pending_bytes: u64,
    pub moderate_compaction_pending_bytes: u64,
    pub severe_immutable_memtables: u64,
    pub moderate_immutable_memtables: u64,
    pub severe_compaction_pending_bytes_bulk: u64,
    pub moderate_compaction_pending_bytes_bulk: u64,
    pub drain_pending_bytes_threshold: u64,
}

impl MemoryProfile {
    pub fn for_primary() -> Self {
        Self::for_primary_with_config(StoreRuntimeConfig::default())
    }

    pub fn for_primary_with_config(runtime_config: StoreRuntimeConfig) -> Self {
        let (ram, cpus) = detect_system_resources(runtime_config);
        Self::compute(ram, cpus, false)
    }

    pub fn for_secondary() -> Self {
        Self::for_secondary_with_config(StoreRuntimeConfig::default())
    }

    pub fn for_secondary_with_config(runtime_config: StoreRuntimeConfig) -> Self {
        let (ram, cpus) = detect_system_resources(runtime_config);
        Self::compute(ram, cpus, true)
    }

    /// Pure computation from fixed inputs for deterministic unit testing.
    pub fn compute(system_ram_bytes: u64, cpu_count: usize, is_secondary: bool) -> Self {
        let budget = if is_secondary {
            let raw = system_ram_bytes / 4;
            raw.clamp(2 * GB, 16 * GB) as usize
        } else {
            let raw = system_ram_bytes / 2;
            raw.max(2 * GB) as usize
        };

        let budget_u64 = budget as u64;
        let wbm_normal = budget / 2;
        let cache_normal = budget / 2;
        let wbm_bulk = (budget_u64 * 85 / 100) as usize;
        let cache_bulk = budget - wbm_bulk;

        let wbm_scale = wbm_normal as f64 / (8.0 * GB as f64);
        let write_buffer_mega = scale_clamp(256 * MB, wbm_scale, 64 * MB, GB);
        let write_buffer_high = scale_clamp(128 * MB, wbm_scale, 32 * MB, 512 * MB);
        let write_buffer_low = scale_clamp(32 * MB, wbm_scale, 8 * MB, 128 * MB);

        let wbm_bulk_scale = wbm_bulk as f64 / 13_635_534_029.0;
        let write_buffer_hot = scale_clamp(512 * MB, wbm_bulk_scale, 128 * MB, 2 * GB);

        let cpus = cpu_count.max(1);
        let max_background_jobs = cpus.clamp(4, 32) as i32;
        let max_subcompactions = (cpus / 4).clamp(2, 8) as u32;

        let budget_scale = budget_u64 as f64 / (16.0 * GB as f64);
        let bulk_level_base = scale_clamp(2 * GB, budget_scale, 512 * MB, 8 * GB) as u64;
        let bulk_file_base = scale_clamp(256 * MB, budget_scale, 64 * MB, 2 * GB) as u64;
        let normal_level_base = scale_clamp(512 * MB, budget_scale, 128 * MB, 2 * GB) as u64;
        let normal_file_base = scale_clamp(64 * MB, budget_scale, 16 * MB, 256 * MB) as u64;

        let severe_pending = scale_clamp(8 * GB, wbm_scale, 2 * GB, 32 * GB) as u64;
        let moderate_pending = scale_clamp(4 * GB, wbm_scale, GB, 16 * GB) as u64;
        let severe_imm = scale_clamp(60, wbm_scale, 15, 240) as u64;
        let moderate_imm = scale_clamp(30, wbm_scale, 8, 120) as u64;
        let drain_pending = scale_clamp(2 * GB, wbm_scale, 512 * MB, 8 * GB) as u64;

        // Bulk sync pending thresholds: scale from wbm_bulk size (larger budget → higher tolerance).
        // Normal thresholds use wbm_normal scale; bulk thresholds use wbm_bulk scale.
        let wbm_bulk_scale = wbm_bulk as f64 / (8.0 * GB as f64);
        let severe_pending_bulk = scale_clamp(8 * GB, wbm_bulk_scale, 2 * GB, 48 * GB) as u64;
        let moderate_pending_bulk = scale_clamp(4 * GB, wbm_bulk_scale, GB, 24 * GB) as u64;

        Self {
            system_ram_bytes,
            cpu_count,
            is_secondary,
            rocksdb_budget_bytes: budget,
            wbm_normal_bytes: wbm_normal,
            block_cache_normal_bytes: cache_normal,
            wbm_bulk_sync_bytes: wbm_bulk,
            block_cache_bulk_sync_bytes: cache_bulk,
            write_buffer_mega_bytes: write_buffer_mega,
            write_buffer_high_bytes: write_buffer_high,
            write_buffer_low_bytes: write_buffer_low,
            write_buffer_hot_cf_bytes: write_buffer_hot,
            max_background_jobs,
            max_subcompactions,
            bulk_max_bytes_for_level_base: bulk_level_base,
            bulk_target_file_size_base: bulk_file_base,
            normal_max_bytes_for_level_base: normal_level_base,
            normal_target_file_size_base: normal_file_base,
            severe_compaction_pending_bytes: severe_pending,
            moderate_compaction_pending_bytes: moderate_pending,
            severe_immutable_memtables: severe_imm,
            moderate_immutable_memtables: moderate_imm,
            severe_compaction_pending_bytes_bulk: severe_pending_bulk,
            moderate_compaction_pending_bytes_bulk: moderate_pending_bulk,
            drain_pending_bytes_threshold: drain_pending,
        }
    }
}

fn read_proc_meminfo() -> Option<u64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let rest = rest.trim();
            let kb = rest.strip_suffix("kB").unwrap_or(rest).trim();
            return kb.parse::<u64>().ok().map(|value| value * 1024);
        }
    }
    None
}

fn read_cgroup_memory_limit() -> Option<u64> {
    if let Ok(content) = std::fs::read_to_string("/sys/fs/cgroup/memory.max") {
        let trimmed = content.trim();
        if trimmed != "max" {
            if let Ok(bytes) = trimmed.parse::<u64>() {
                return Some(bytes);
            }
        }
    }

    if let Ok(content) = std::fs::read_to_string("/sys/fs/cgroup/memory/memory.limit_in_bytes") {
        let trimmed = content.trim();
        if let Ok(bytes) = trimmed.parse::<u64>() {
            if bytes < 1024 * 1024 * 1024 * 1024 {
                return Some(bytes);
            }
        }
    }

    None
}

fn detect_system_resources(runtime_config: StoreRuntimeConfig) -> (u64, usize) {
    let ram = if let Some(gb) = runtime_config.memory_budget_gb {
        if gb > 0 {
            info!(gb, "Using explicit RocksDB memory_budget_gb override");
            gb * GB
        } else {
            warn!("Ignoring zero memory_budget_gb override, falling back to detection");
            read_proc_meminfo()
                .or_else(read_cgroup_memory_limit)
                .unwrap_or(32 * GB)
        }
    } else {
        read_proc_meminfo()
            .or_else(read_cgroup_memory_limit)
            .unwrap_or_else(|| {
                warn!("Could not detect system RAM, defaulting to 32 GB");
                32 * GB
            })
    };

    let cpus = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(8);

    (ram, cpus)
}

// Column family name constants
pub const CF_CELLS: &str = "cells";
pub const CF_LIVE_CELLS: &str = "live_cells";
pub const CF_CONSUMED_CELLS: &str = "consumed_cells";
pub const CF_REORG_UNDO_LOG_BY_BLOCK: &str = "reorg_undo_log_by_block";
pub const CF_BLOCK_HEADERS: &str = "block_headers";
pub const CF_BLOCK_HASH_INDEX: &str = "block_hash_index";
pub const CF_CELL_BY_LOCK: &str = "cell_by_lock";
pub const CF_CELL_BY_TYPE: &str = "cell_by_type";
pub const CF_CELL_BY_DATA_HASH: &str = "cell_by_data_hash";
pub const CF_TX_INDEX: &str = "tx_index";
pub const CF_TX_HASH_MAP: &str = "tx_hash_map";
pub const CF_ADDR_BALANCE: &str = "addr_balance";
pub const CF_ADDR_TXS: &str = "addr_txs";
pub const CF_DAO_DEPOSITS: &str = "dao_deposits";
pub const CF_DAO_BY_WITHDRAW_TX: &str = "dao_by_withdraw_tx";
pub const CF_DAO_BY_BLOCK: &str = "dao_by_block";
pub const CF_DAO_BY_LOCK_BLOCK: &str = "dao_by_lock_block";
pub const CF_DAO_BY_STATUS_BLOCK: &str = "dao_by_status_block";
pub const CF_TOKENS: &str = "tokens";
pub const CF_TOKEN_HOLDERS: &str = "token_holders";
pub const CF_TOKEN_HOLDERS_BY_BALANCE: &str = "token_holders_by_balance";
pub const CF_ADDR_TOKENS_BY_BALANCE: &str = "addr_tokens_by_balance";
pub const CF_SPORE_DATA: &str = "spore_data";
pub const CF_OBJECT_DATA: &str = "object_data";
pub const CF_OBJECT_BY_COLLECTION: &str = "object_by_collection";
pub const CF_IDENTITY_DATA: &str = "identity_data";
pub const CF_IDENTITY_BY_COLLECTION: &str = "identity_by_collection";
pub const CF_STATS_IDENTITY: &str = "stats_identity";
pub const CF_STATS_CHAIN: &str = "stats_chain";
pub const CF_STATS_DAO: &str = "stats_dao";
pub const CF_STATS_HODL: &str = "stats_hodl";
pub const CF_STATS_SCRIPT: &str = "stats_script";
pub const CF_STATS_TOKEN: &str = "stats_token";
pub const CF_STATS_SPORE: &str = "stats_spore";
pub const CF_STATS_OBJECT: &str = "stats_object";
pub const CF_SCRIPT_INFO: &str = "script_info";
pub const CF_SCRIPT_VERSIONS: &str = "script_versions";
pub const CF_SCRIPT_VERSIONS_BY_LABEL: &str = "script_versions_by_label";
pub const CF_SYNC_META: &str = "sync_meta";
pub const CF_SPORE_BY_CLUSTER: &str = "spore_by_cluster";
pub const CF_CELL_BY_LOCK_CODE: &str = "cell_by_lock_code";
pub const CF_CELL_BY_TYPE_CODE: &str = "cell_by_type_code";
pub const CF_TOKEN_TRANSFERS: &str = "token_transfers";
pub const CF_ACTIVITIES: &str = "activities";
pub const CF_CLUSTER_AGG: &str = "cluster_agg";
pub const CF_OBJECT_COLLECTION_AGG: &str = "object_collection_agg";
pub const CF_OBJECT_COLLECTION_ACTIVITIES: &str = "object_collection_activities";
pub const CF_IDENTITY_AGG: &str = "identity_agg";
pub const CF_IDENTITY_COLLECTION_ACTIVITIES: &str = "identity_collection_activities";
pub const CF_PENDING_PROPOSALS: &str = "pending_proposals";
pub const CF_FIBER_CHANNELS: &str = "fiber_channels";
pub const CF_FIBER_CHANNEL_BY_COMMITMENT: &str = "fiber_channel_by_commitment";
pub const CF_FIBER_CHANNEL_BY_FUNDING_ARGS: &str = "fiber_channel_by_funding_args";
pub const CF_ADDR_FIBER_CHANNELS: &str = "addr_fiber_channels";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfWritePolicy {
    AppendOnly,
    FinalSnapshot,
    SealedAggregate,
    BulkDisabled,
}
const CF_WRITE_POLICY_APPEND_ONLY: &[&str] = &[
    CF_CELLS,
    CF_BLOCK_HEADERS,
    CF_BLOCK_HASH_INDEX,
    CF_TX_INDEX,
    CF_TX_HASH_MAP,
    CF_CONSUMED_CELLS,
    CF_ADDR_TXS,
    CF_TOKEN_TRANSFERS,
    CF_ACTIVITIES,
    CF_OBJECT_COLLECTION_ACTIVITIES,
    CF_IDENTITY_COLLECTION_ACTIVITIES,
];

const CF_WRITE_POLICY_SEALED_AGGREGATE: &[&str] = &[
    CF_STATS_CHAIN,
    CF_STATS_DAO,
    CF_STATS_HODL,
    CF_STATS_SCRIPT,
    CF_STATS_TOKEN,
    CF_STATS_SPORE,
    CF_STATS_OBJECT,
];

const CF_WRITE_POLICY_BULK_DISABLED: &[&str] = &[CF_REORG_UNDO_LOG_BY_BLOCK, CF_PENDING_PROPOSALS];

const CF_WRITE_POLICY_FINAL_SNAPSHOT: &[&str] = &[
    CF_LIVE_CELLS,
    CF_CELL_BY_LOCK,
    CF_CELL_BY_TYPE,
    CF_CELL_BY_DATA_HASH,
    CF_ADDR_BALANCE,
    CF_DAO_DEPOSITS,
    CF_DAO_BY_WITHDRAW_TX,
    CF_DAO_BY_BLOCK,
    CF_DAO_BY_LOCK_BLOCK,
    CF_DAO_BY_STATUS_BLOCK,
    CF_TOKENS,
    CF_TOKEN_HOLDERS,
    CF_TOKEN_HOLDERS_BY_BALANCE,
    CF_ADDR_TOKENS_BY_BALANCE,
    CF_SPORE_DATA,
    CF_OBJECT_DATA,
    CF_OBJECT_BY_COLLECTION,
    CF_IDENTITY_DATA,
    CF_IDENTITY_BY_COLLECTION,
    CF_STATS_IDENTITY,
    CF_SCRIPT_INFO,
    CF_SCRIPT_VERSIONS,
    CF_SCRIPT_VERSIONS_BY_LABEL,
    CF_SYNC_META,
    CF_SPORE_BY_CLUSTER,
    CF_CELL_BY_LOCK_CODE,
    CF_CELL_BY_TYPE_CODE,
    CF_CLUSTER_AGG,
    CF_OBJECT_COLLECTION_AGG,
    CF_IDENTITY_AGG,
    CF_FIBER_CHANNELS,
    CF_FIBER_CHANNEL_BY_COMMITMENT,
    CF_FIBER_CHANNEL_BY_FUNDING_ARGS,
    CF_ADDR_FIBER_CHANNELS,
];

pub fn cf_write_policy(cf_name: &str) -> CfWritePolicy {
    if CF_WRITE_POLICY_APPEND_ONLY.contains(&cf_name) {
        CfWritePolicy::AppendOnly
    } else if CF_WRITE_POLICY_SEALED_AGGREGATE.contains(&cf_name) {
        CfWritePolicy::SealedAggregate
    } else if CF_WRITE_POLICY_BULK_DISABLED.contains(&cf_name) {
        CfWritePolicy::BulkDisabled
    } else if CF_WRITE_POLICY_FINAL_SNAPSHOT.contains(&cf_name) {
        CfWritePolicy::FinalSnapshot
    } else {
        panic!("unknown column family write policy: {}", cf_name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreClass {
    Domain,
    AppendOnly,
    TestUnified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoreWriteIntent {
    /// Default writer path: must respect append-only invariants.
    Normal,
    /// StoreBatch already validated per-op append-only invariants.
    AppendValidated,
    /// Bulk sync mode for append-only StoreBatch commits.
    BulkSyncAppendValidated,
}

/// All column family names, used during DB open.
pub const ALL_CFS: &[&str] = &[
    CF_CELLS,
    CF_LIVE_CELLS,
    CF_CONSUMED_CELLS,
    CF_REORG_UNDO_LOG_BY_BLOCK,
    CF_BLOCK_HEADERS,
    CF_BLOCK_HASH_INDEX,
    CF_CELL_BY_LOCK,
    CF_CELL_BY_TYPE,
    CF_CELL_BY_LOCK_CODE,
    CF_CELL_BY_TYPE_CODE,
    CF_CELL_BY_DATA_HASH,
    CF_TX_INDEX,
    CF_TX_HASH_MAP,
    CF_ADDR_BALANCE,
    CF_ADDR_TXS,
    CF_DAO_DEPOSITS,
    CF_DAO_BY_WITHDRAW_TX,
    CF_DAO_BY_BLOCK,
    CF_DAO_BY_LOCK_BLOCK,
    CF_DAO_BY_STATUS_BLOCK,
    CF_TOKENS,
    CF_TOKEN_HOLDERS,
    CF_TOKEN_HOLDERS_BY_BALANCE,
    CF_ADDR_TOKENS_BY_BALANCE,
    CF_SPORE_DATA,
    CF_OBJECT_DATA,
    CF_OBJECT_BY_COLLECTION,
    CF_IDENTITY_DATA,
    CF_IDENTITY_BY_COLLECTION,
    CF_STATS_CHAIN,
    CF_STATS_DAO,
    CF_STATS_HODL,
    CF_STATS_SCRIPT,
    CF_STATS_TOKEN,
    CF_STATS_SPORE,
    CF_STATS_OBJECT,
    CF_STATS_IDENTITY,
    CF_SCRIPT_INFO,
    CF_SCRIPT_VERSIONS,
    CF_SCRIPT_VERSIONS_BY_LABEL,
    CF_SYNC_META,
    CF_SPORE_BY_CLUSTER,
    CF_TOKEN_TRANSFERS,
    CF_ACTIVITIES,
    CF_CLUSTER_AGG,
    CF_OBJECT_COLLECTION_AGG,
    CF_OBJECT_COLLECTION_ACTIVITIES,
    CF_IDENTITY_AGG,
    CF_IDENTITY_COLLECTION_ACTIVITIES,
    CF_PENDING_PROPOSALS,
    CF_FIBER_CHANNELS,
    CF_FIBER_CHANNEL_BY_COMMITMENT,
    CF_FIBER_CHANNEL_BY_FUNDING_ARGS,
    CF_ADDR_FIBER_CHANNELS,
];

/// Column families intended for the domain mutable store.
pub const DOMAIN_CFS: &[&str] = &[
    // CF_CELLS removed — now in APPEND_CFS (content-addressed, hash-keyed)
    CF_LIVE_CELLS,
    CF_CONSUMED_CELLS,
    CF_REORG_UNDO_LOG_BY_BLOCK,
    CF_BLOCK_HEADERS,
    CF_BLOCK_HASH_INDEX,
    CF_CELL_BY_LOCK,
    CF_CELL_BY_TYPE,
    CF_CELL_BY_LOCK_CODE,
    CF_CELL_BY_TYPE_CODE,
    CF_CELL_BY_DATA_HASH,
    CF_TX_INDEX,
    CF_TX_HASH_MAP,
    CF_ADDR_BALANCE,
    CF_ADDR_TXS,
    CF_DAO_DEPOSITS,
    CF_DAO_BY_WITHDRAW_TX,
    CF_DAO_BY_BLOCK,
    CF_DAO_BY_LOCK_BLOCK,
    CF_DAO_BY_STATUS_BLOCK,
    CF_TOKENS,
    CF_TOKEN_HOLDERS,
    CF_TOKEN_HOLDERS_BY_BALANCE,
    CF_ADDR_TOKENS_BY_BALANCE,
    CF_SPORE_DATA,
    CF_OBJECT_DATA,
    CF_OBJECT_BY_COLLECTION,
    CF_IDENTITY_DATA,
    CF_IDENTITY_BY_COLLECTION,
    CF_STATS_CHAIN,
    CF_STATS_DAO,
    CF_STATS_HODL,
    CF_STATS_SCRIPT,
    CF_STATS_TOKEN,
    CF_STATS_SPORE,
    CF_STATS_OBJECT,
    CF_STATS_IDENTITY,
    CF_SCRIPT_INFO,
    CF_SCRIPT_VERSIONS,
    CF_SCRIPT_VERSIONS_BY_LABEL,
    CF_SYNC_META,
    CF_SPORE_BY_CLUSTER,
    CF_TOKEN_TRANSFERS,
    CF_ACTIVITIES,
    CF_CLUSTER_AGG,
    CF_OBJECT_COLLECTION_AGG,
    CF_OBJECT_COLLECTION_ACTIVITIES,
    CF_IDENTITY_AGG,
    CF_IDENTITY_COLLECTION_ACTIVITIES,
    CF_PENDING_PROPOSALS,
    CF_FIBER_CHANNELS,
    CF_FIBER_CHANNEL_BY_COMMITMENT,
    CF_FIBER_CHANNEL_BY_FUNDING_ARGS,
    CF_ADDR_FIBER_CHANNELS,
];

/// Column families for the append-only store (immutable, hash-keyed cell payloads).
pub const APPEND_CFS: &[&str] = &[CF_CELLS];

fn append_path_from_domain(domain_path: &Path) -> PathBuf {
    if domain_path.file_name().and_then(|name| name.to_str()) == Some("domain") {
        return domain_path.with_file_name("append-only");
    }
    PathBuf::from(format!("{}-append-only", domain_path.display()))
}

fn domain_path_from_append(append_path: &Path) -> PathBuf {
    if append_path.file_name().and_then(|name| name.to_str()) == Some("append-only") {
        return append_path.with_file_name("domain");
    }

    let mut stripped = append_path.to_path_buf();
    if let Some(name) = append_path.file_name().and_then(|name| name.to_str()) {
        if let Some(base_name) = name.strip_suffix("-append-only") {
            stripped.set_file_name(base_name);
            return stripped;
        }
    }

    append_path.to_path_buf()
}

fn consumed_cf_storage_bytes(
    live_data_bytes: Option<u64>,
    sst_files_bytes: Option<u64>,
    memtable_bytes: Option<u64>,
) -> (usize, &'static str) {
    let (base, source) = match live_data_bytes {
        Some(v) if v > 0 => (v, "live"),
        _ => match sst_files_bytes {
            Some(v) if v > 0 => (v, "sst"),
            _ => match memtable_bytes {
                Some(v) if v > 0 => (0, "mem"),
                _ => (0, "none"),
            },
        },
    };
    let total = base.saturating_add(memtable_bytes.unwrap_or(0));
    (usize::try_from(total).unwrap_or(usize::MAX), source)
}

fn short_hex(bytes: &[u8], max_len: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let shown = bytes.len().min(max_len);
    let mut out = String::with_capacity(shown * 2 + if shown < bytes.len() { 3 } else { 0 });
    for b in &bytes[..shown] {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    if shown < bytes.len() {
        out.push_str("...");
    }
    out
}

pub struct CkbadgerStore {
    db: DB,
    store_class: StoreClass,
    domain_path: PathBuf,
    append_path: PathBuf,
    /// Keep block cache alive for the lifetime of the store.
    /// Mutex because `set_capacity()` requires `&mut` (rare mode-transition calls only).
    block_cache: Mutex<rocksdb::Cache>,
    /// Global memtable memory budget — controls WHEN flushes happen across all CFs.
    /// With `atomic_flush=true` and many CFs, per-CF triggers cause unpredictable I/O storms.
    /// WBM replaces that with a single threshold: flush oldest CF when total memtable
    /// memory exceeds the budget, giving the indexer predictable flush behavior.
    write_buffer_manager: WriteBufferManager,
    bulk_sync_mode: AtomicBool,
    is_secondary: bool,
    memory_profile: MemoryProfile,
    runtime_config: StoreRuntimeConfig,
}

impl CkbadgerStore {
    fn cfs_for_class(store_class: StoreClass) -> &'static [&'static str] {
        match store_class {
            StoreClass::Domain => DOMAIN_CFS,
            StoreClass::AppendOnly => APPEND_CFS,
            StoreClass::TestUnified => ALL_CFS,
        }
    }

    fn cf_allowed(store_class: StoreClass, name: &str) -> bool {
        match store_class {
            StoreClass::Domain => DOMAIN_CFS.contains(&name),
            StoreClass::AppendOnly => APPEND_CFS.contains(&name),
            StoreClass::TestUnified => true,
        }
    }

    pub fn has_cf(&self, name: &str) -> bool {
        Self::cf_allowed(self.store_class, name)
    }

    pub(crate) fn is_append_only_store(&self) -> bool {
        self.store_class == StoreClass::AppendOnly
    }

    pub(crate) fn append_cf_name_for_handle(
        &self,
        cf: &ColumnFamily,
    ) -> anyhow::Result<&'static str> {
        if !self.is_append_only_store() {
            anyhow::bail!(
                "append_cf_name_for_handle called on non-append store: {:?}",
                self.store_class
            );
        }
        if std::ptr::eq(cf, self.cf_cells()) {
            return Ok(CF_CELLS);
        }
        anyhow::bail!(
            "unknown append-only column family handle in {:?} store",
            self.store_class
        );
    }

    pub(crate) fn validate_append_put_by_cf_name(
        &self,
        cf_name: &str,
        key: &[u8],
        value: &[u8],
        _intent: StoreWriteIntent,
    ) -> anyhow::Result<()> {
        if !self.is_append_only_store() {
            return Ok(());
        }
        let cf = self
            .db
            .cf_handle(cf_name)
            .ok_or_else(|| anyhow::anyhow!("CF '{}' not found", cf_name))?;
        if let Some(existing) = self.db.get_cf(cf, key)? {
            anyhow::bail!(
                "append-only overwrite blocked: cf={}, key=0x{}, existing_len={}, new_len={}",
                cf_name,
                short_hex(key, 24),
                existing.len(),
                value.len()
            );
        }
        Ok(())
    }

    pub(crate) fn validate_append_delete_by_cf_name(
        &self,
        cf_name: &str,
        key: &[u8],
        intent: StoreWriteIntent,
    ) -> anyhow::Result<()> {
        if !self.is_append_only_store() {
            return Ok(());
        }
        anyhow::bail!(
            "append-only delete blocked: cf={}, key=0x{}, intent={:?}",
            cf_name,
            short_hex(key, 24),
            intent
        );
    }

    fn open_with_class<P: AsRef<Path>>(
        path: P,
        store_class: StoreClass,
        runtime_config: StoreRuntimeConfig,
    ) -> anyhow::Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        let (domain_path, append_path) = match store_class {
            StoreClass::Domain => {
                let domain = db_path.clone();
                let append = append_path_from_domain(&domain);
                (domain, append)
            }
            StoreClass::AppendOnly => {
                let append = db_path.clone();
                let domain = domain_path_from_append(&append);
                (domain, append)
            }
            StoreClass::TestUnified => {
                let domain = db_path.clone();
                let append = append_path_from_domain(&domain);
                (domain, append)
            }
        };

        let memory_profile = MemoryProfile::for_primary_with_config(runtime_config);
        let (opts, block_cache, write_buffer_manager) =
            Self::configured_options(&memory_profile, runtime_config);

        let existing_cfs = match DB::list_cf(&opts, &path) {
            Ok(cfs) => cfs,
            Err(_err) if !db_path.join("CURRENT").exists() => Vec::new(),
            Err(err) => {
                anyhow::bail!(
                    "failed to list column families at '{}': {}",
                    db_path.display(),
                    err
                );
            }
        };
        let allowed = Self::cfs_for_class(store_class);
        let allowed_set = allowed
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        for cf in &existing_cfs {
            if cf == "default" {
                continue;
            }
            if !allowed_set.contains(cf.as_str()) {
                anyhow::bail!(
                    "found column family '{}' in {} store at '{}' but it is not allowed there; \
                     expected only {:?}. Please rebuild this RocksDB path.",
                    cf,
                    match store_class {
                        StoreClass::Domain => "domain",
                        StoreClass::AppendOnly => "append-only",
                        StoreClass::TestUnified => "test-unified",
                    },
                    db_path.display(),
                    allowed
                );
            }
        }

        let cf_descriptors: Vec<ColumnFamilyDescriptor> = allowed
            .iter()
            .map(|name| {
                ColumnFamilyDescriptor::new(
                    *name,
                    Self::cf_options(name, &block_cache, &memory_profile),
                )
            })
            .collect();

        let db = DB::open_cf_descriptors(&opts, path, cf_descriptors)?;

        Ok(Self {
            db,
            store_class,
            domain_path,
            append_path,
            block_cache: Mutex::new(block_cache),
            write_buffer_manager,
            bulk_sync_mode: AtomicBool::new(false),
            is_secondary: false,
            memory_profile,
            runtime_config,
        })
    }

    fn open_secondary_with_class<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
        store_class: StoreClass,
        runtime_config: StoreRuntimeConfig,
    ) -> anyhow::Result<Self> {
        let db_path = primary_path.as_ref().to_path_buf();
        let (domain_path, append_path) = match store_class {
            StoreClass::Domain => {
                let domain = db_path.clone();
                let append = append_path_from_domain(&domain);
                (domain, append)
            }
            StoreClass::AppendOnly => {
                let append = db_path.clone();
                let domain = domain_path_from_append(&append);
                (domain, append)
            }
            StoreClass::TestUnified => {
                let domain = db_path.clone();
                let append = append_path_from_domain(&domain);
                (domain, append)
            }
        };
        let memory_profile = MemoryProfile::for_secondary_with_config(runtime_config);
        let (opts, block_cache, write_buffer_manager) =
            Self::configured_options(&memory_profile, runtime_config);
        let existing_cfs = match DB::list_cf(&opts, &primary_path) {
            Ok(cfs) => cfs,
            Err(_err) if !db_path.join("CURRENT").exists() => Vec::new(),
            Err(err) => {
                return Err(anyhow::anyhow!(
                    "failed to list column families at secondary primary path '{}': {}",
                    db_path.display(),
                    err
                ));
            }
        };
        let allowed = Self::cfs_for_class(store_class);
        let allowed_set = allowed
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        for cf in &existing_cfs {
            if cf == "default" {
                continue;
            }
            if !allowed_set.contains(cf.as_str()) {
                anyhow::bail!(
                    "found column family '{}' in {} store at '{}' but it is not allowed there; \
                     expected only {:?}. Please rebuild this RocksDB path.",
                    cf,
                    match store_class {
                        StoreClass::Domain => "domain",
                        StoreClass::AppendOnly => "append-only",
                        StoreClass::TestUnified => "test-unified",
                    },
                    db_path.display(),
                    allowed
                );
            }
        }
        let cf_refs: Vec<&str> = allowed.to_vec();
        let db = DB::open_cf_as_secondary(&opts, primary_path, secondary_path, cf_refs)?;

        Ok(Self {
            db,
            store_class,
            domain_path,
            append_path,
            block_cache: Mutex::new(block_cache),
            write_buffer_manager,
            bulk_sync_mode: AtomicBool::new(false),
            is_secondary: true,
            memory_profile,
            runtime_config,
        })
    }

    pub fn open_domain<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        Self::open_domain_with_runtime(path, StoreRuntimeConfig::default())
    }

    pub fn open_domain_with_runtime<P: AsRef<Path>>(
        path: P,
        runtime_config: StoreRuntimeConfig,
    ) -> anyhow::Result<Self> {
        Self::open_with_class(path, StoreClass::Domain, runtime_config)
    }

    pub fn open_append_only<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        Self::open_append_only_with_runtime(path, StoreRuntimeConfig::default())
    }

    pub fn open_append_only_with_runtime<P: AsRef<Path>>(
        path: P,
        runtime_config: StoreRuntimeConfig,
    ) -> anyhow::Result<Self> {
        Self::open_with_class(path, StoreClass::AppendOnly, runtime_config)
    }

    pub fn open_test_unified<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        Self::open_with_class(path, StoreClass::TestUnified, StoreRuntimeConfig::default())
    }

    pub fn open_domain_secondary<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
    ) -> anyhow::Result<Self> {
        Self::open_domain_secondary_with_runtime(
            primary_path,
            secondary_path,
            StoreRuntimeConfig::default(),
        )
    }

    pub fn open_domain_secondary_with_runtime<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
        runtime_config: StoreRuntimeConfig,
    ) -> anyhow::Result<Self> {
        Self::open_secondary_with_class(
            primary_path,
            secondary_path,
            StoreClass::Domain,
            runtime_config,
        )
    }

    pub fn open_append_only_secondary<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
    ) -> anyhow::Result<Self> {
        Self::open_append_only_secondary_with_runtime(
            primary_path,
            secondary_path,
            StoreRuntimeConfig::default(),
        )
    }

    pub fn open_append_only_secondary_with_runtime<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
        runtime_config: StoreRuntimeConfig,
    ) -> anyhow::Result<Self> {
        Self::open_secondary_with_class(
            primary_path,
            secondary_path,
            StoreClass::AppendOnly,
            runtime_config,
        )
    }

    pub fn open_test_unified_secondary<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
    ) -> anyhow::Result<Self> {
        Self::open_secondary_with_class(
            primary_path,
            secondary_path,
            StoreClass::TestUnified,
            StoreRuntimeConfig::default(),
        )
    }

    /// Catch up with primary instance writes (secondary only).
    pub fn refresh(&self) -> anyhow::Result<()> {
        if self.is_secondary {
            self.db.try_catch_up_with_primary()?;
        }
        Ok(())
    }

    /// Mega-write CFs with the heaviest per-batch write volume.
    /// 256MB write buffers prevent memtable flush stalls during mega-blocks
    /// (block 12M has ~1.31M txs → ~162MB per CF).
    const MEGA_WRITE_CFS: &'static [&'static str] = &[
        CF_CELLS,
        CF_LIVE_CELLS,
        CF_CONSUMED_CELLS,
        CF_REORG_UNDO_LOG_BY_BLOCK,
        CF_CELL_BY_LOCK,
        CF_CELL_BY_TYPE,
        CF_CELL_BY_LOCK_CODE,
        CF_CELL_BY_TYPE_CODE,
        CF_TX_INDEX,
        CF_TX_HASH_MAP,
        CF_ADDR_BALANCE,
        CF_ADDR_TXS,
        CF_ACTIVITIES,
    ];

    /// High-write column families that benefit from large write buffers (128 MB).
    const HIGH_WRITE_CFS: &'static [&'static str] = &[
        CF_CELLS,
        CF_LIVE_CELLS,
        CF_CONSUMED_CELLS,
        CF_REORG_UNDO_LOG_BY_BLOCK,
        CF_BLOCK_HEADERS,
        CF_BLOCK_HASH_INDEX,
        CF_CELL_BY_LOCK,
        CF_CELL_BY_TYPE,
        CF_TX_INDEX,
        CF_TX_HASH_MAP,
        CF_ADDR_BALANCE,
        CF_ADDR_TXS,
        CF_DAO_DEPOSITS,
        CF_DAO_BY_BLOCK,
        CF_DAO_BY_LOCK_BLOCK,
        CF_DAO_BY_STATUS_BLOCK,
        CF_ACTIVITIES,
        CF_OBJECT_COLLECTION_ACTIVITIES,
        CF_IDENTITY_COLLECTION_ACTIVITIES,
        CF_STATS_CHAIN,
        CF_STATS_SCRIPT,
        CF_STATS_TOKEN,
        CF_STATS_SPORE,
        CF_STATS_OBJECT,
        CF_IDENTITY_DATA,
    ];

    /// Historical append-heavy CFs.
    ///
    /// These indexes are primarily append writes during sync and large range scans on reads.
    /// Universal compaction reduces cross-level rewrite amplification for this write pattern.
    const HISTORICAL_APPEND_CFS: &'static [&'static str] = &[CF_CELLS];

    fn is_mega_write_cf(name: &str) -> bool {
        Self::MEGA_WRITE_CFS.contains(&name)
    }

    fn is_high_write_cf(name: &str) -> bool {
        Self::HIGH_WRITE_CFS.contains(&name)
    }

    fn is_historical_append_cf(name: &str) -> bool {
        Self::HISTORICAL_APPEND_CFS.contains(&name)
    }

    fn default_block_options(block_cache: &rocksdb::Cache) -> rocksdb::BlockBasedOptions {
        let mut block_opts = rocksdb::BlockBasedOptions::default();
        block_opts.set_block_size(16 * 1024);
        block_opts.set_block_cache(block_cache);
        block_opts.set_cache_index_and_filter_blocks(true);
        block_opts.set_bloom_filter(10.0, false);
        block_opts
    }

    fn configured_options(
        profile: &MemoryProfile,
        runtime_config: StoreRuntimeConfig,
    ) -> (Options, rocksdb::Cache, WriteBufferManager) {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        // Bypass OS page cache for reads: block cache already handles hot data,
        // and bulk sync reads (live cell lookups) are not reused — caching them
        // in the page cache wastes RAM and adds syscall overhead.
        opts.set_use_direct_reads(runtime_config.direct_io_reads);

        // Favor throughput during bulk sync while still smoothing fsync pressure.
        opts.set_bytes_per_sync(4 * 1024 * 1024);

        opts.set_write_buffer_size(profile.write_buffer_high_bytes);
        opts.set_max_write_buffer_number(4);

        // Compaction triggers: give L0 more headroom to avoid write stalls
        // With 5 parallel commit_no_wal() per batch, L0 files accumulate fast.
        // Wider thresholds let compaction catch up without stalling writers.
        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_level_zero_slowdown_writes_trigger(20);
        opts.set_level_zero_stop_writes_trigger(48);
        opts.set_max_bytes_for_level_base(profile.normal_max_bytes_for_level_base);
        opts.set_compression_type(DBCompressionType::Lz4);

        opts.set_max_background_jobs(profile.max_background_jobs);
        opts.set_max_subcompactions(profile.max_subcompactions);

        // Bypass OS page cache for flush/compaction to avoid cache pollution
        opts.set_use_direct_io_for_flush_and_compaction(true);

        // Atomic flush: when any CF's memtable triggers a flush, ALL CFs flush
        // together. This prevents cross-CF data inconsistency when using
        // commit_no_wal() during bulk sync — without it, a crash can leave
        // live_cells deletes flushed to SST while consumed_cells puts are lost
        // in memtable, creating unrecoverable "cell black holes".
        // Note: atomic_flush is incompatible with enable_pipelined_write, but
        // pipelined_write only helps pipeline WAL+memtable inserts — irrelevant
        // during bulk sync where WAL is disabled (commit_no_wal).
        opts.set_atomic_flush(true);

        // Unordered write: skip write-group leader serialization overhead.
        // Safe because the indexer is the sole writer process and the API
        // uses secondary (read-only) mode. Relaxes snapshot ordering but
        // maintains read-your-own-write consistency.
        opts.set_unordered_write(true);

        let block_cache = rocksdb::Cache::new_lru_cache(profile.block_cache_normal_bytes);
        let block_opts = Self::default_block_options(&block_cache);
        opts.set_block_based_table_factory(&block_opts);

        // Global WriteBufferManager: controls total memtable memory across all CFs.
        // With atomic_flush and many CFs, per-CF memtable limits cause unpredictable
        // I/O storms when a random hot CF triggers a flush of ALL CFs. WBM replaces
        // that with a global budget — flush only happens when total memtable usage
        // crosses the threshold, giving predictable, batched flush behavior.
        // allow_stall=true: stall writes when memtable memory exceeds budget rather
        // than OOM; the adaptive batch controller will detect stalls and reduce batch size.
        let write_buffer_manager =
            WriteBufferManager::new_write_buffer_manager(profile.wbm_normal_bytes, true);
        opts.set_write_buffer_manager(&write_buffer_manager);

        (opts, block_cache, write_buffer_manager)
    }

    /// Per-CF options with 3 tiers:
    /// - Mega-write CFs: 256MB × 4 buffers = 1GB per CF
    /// - High-write (remaining CFs): 128MB × 4 buffers = 512MB per CF
    /// - Everything else: 32MB × 2 buffers = 64MB per CF
    fn cf_options(name: &str, block_cache: &rocksdb::Cache, profile: &MemoryProfile) -> Options {
        let mut opts = Options::default();

        if Self::is_mega_write_cf(name) {
            opts.set_write_buffer_size(profile.write_buffer_mega_bytes);
            opts.set_max_write_buffer_number(4);
        } else if Self::is_high_write_cf(name) {
            opts.set_write_buffer_size(profile.write_buffer_high_bytes);
            opts.set_max_write_buffer_number(4);
        } else {
            opts.set_write_buffer_size(profile.write_buffer_low_bytes);
            opts.set_max_write_buffer_number(2);
        }

        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_level_zero_slowdown_writes_trigger(12);
        opts.set_level_zero_stop_writes_trigger(24);
        opts.set_max_bytes_for_level_base(profile.normal_max_bytes_for_level_base);
        opts.set_compression_type(DBCompressionType::Lz4);
        if Self::is_historical_append_cf(name) {
            // Prioritize write throughput for append-heavy history CFs.
            opts.set_compression_type(DBCompressionType::None);
            opts.set_compaction_style(DBCompactionStyle::Universal);
            let mut uco = UniversalCompactOptions::default();
            uco.set_size_ratio(10);
            uco.set_max_size_amplification_percent(100);
            opts.set_universal_compaction_options(&uco);
        } else {
            // Dynamic level sizing for Leveled compaction CFs: RocksDB sizes each
            // level relative to the last (largest) level rather than using fixed
            // size ratios. This halves write amplification (~10x → ~5x) because
            // the upper levels stay proportionally smaller, triggering far fewer
            // cross-level rewrites during bulk sync.
            opts.set_level_compaction_dynamic_level_bytes(true);
        }

        let block_opts = Self::default_block_options(block_cache);
        opts.set_block_based_table_factory(&block_opts);

        opts
    }

    // ---- Column family accessors ----

    pub fn cf(&self, name: &str) -> &ColumnFamily {
        if !Self::cf_allowed(self.store_class, name) {
            panic!(
                "CF '{}' is not allowed in {:?} store",
                name, self.store_class
            );
        }
        self.db
            .cf_handle(name)
            .unwrap_or_else(|| panic!("CF '{}' not found", name))
    }

    pub fn cf_live_cells(&self) -> &ColumnFamily {
        self.cf(CF_LIVE_CELLS)
    }
    pub fn cf_cells(&self) -> &ColumnFamily {
        self.cf(CF_CELLS)
    }
    pub fn cf_consumed_cells(&self) -> &ColumnFamily {
        self.cf(CF_CONSUMED_CELLS)
    }
    pub fn cf_reorg_undo_log_by_block(&self) -> &ColumnFamily {
        self.cf(CF_REORG_UNDO_LOG_BY_BLOCK)
    }
    pub fn cf_block_headers(&self) -> &ColumnFamily {
        self.cf(CF_BLOCK_HEADERS)
    }
    pub fn cf_block_hash_index(&self) -> &ColumnFamily {
        self.cf(CF_BLOCK_HASH_INDEX)
    }
    pub fn cf_cell_by_lock(&self) -> &ColumnFamily {
        self.cf(CF_CELL_BY_LOCK)
    }
    pub fn cf_cell_by_type(&self) -> &ColumnFamily {
        self.cf(CF_CELL_BY_TYPE)
    }
    pub fn cf_tx_index(&self) -> &ColumnFamily {
        self.cf(CF_TX_INDEX)
    }
    pub fn cf_tx_hash_map(&self) -> &ColumnFamily {
        self.cf(CF_TX_HASH_MAP)
    }
    pub fn cf_addr_balance(&self) -> &ColumnFamily {
        self.cf(CF_ADDR_BALANCE)
    }
    pub fn cf_addr_txs(&self) -> &ColumnFamily {
        self.cf(CF_ADDR_TXS)
    }
    pub fn cf_dao_deposits(&self) -> &ColumnFamily {
        self.cf(CF_DAO_DEPOSITS)
    }
    pub fn cf_dao_by_withdraw_tx(&self) -> &ColumnFamily {
        self.cf(CF_DAO_BY_WITHDRAW_TX)
    }
    pub fn cf_dao_by_block(&self) -> &ColumnFamily {
        self.cf(CF_DAO_BY_BLOCK)
    }
    pub fn cf_dao_by_lock_block(&self) -> &ColumnFamily {
        self.cf(CF_DAO_BY_LOCK_BLOCK)
    }
    pub fn cf_dao_by_status_block(&self) -> &ColumnFamily {
        self.cf(CF_DAO_BY_STATUS_BLOCK)
    }
    pub fn cf_tokens(&self) -> &ColumnFamily {
        self.cf(CF_TOKENS)
    }
    pub fn cf_token_holders(&self) -> &ColumnFamily {
        self.cf(CF_TOKEN_HOLDERS)
    }
    pub fn cf_token_holders_by_balance(&self) -> &ColumnFamily {
        self.cf(CF_TOKEN_HOLDERS_BY_BALANCE)
    }
    pub fn cf_addr_tokens_by_balance(&self) -> &ColumnFamily {
        self.cf(CF_ADDR_TOKENS_BY_BALANCE)
    }
    pub fn cf_spore_data(&self) -> &ColumnFamily {
        self.cf(CF_SPORE_DATA)
    }
    pub fn cf_object_data(&self) -> &ColumnFamily {
        self.cf(CF_OBJECT_DATA)
    }
    pub fn cf_object_by_collection(&self) -> &ColumnFamily {
        self.cf(CF_OBJECT_BY_COLLECTION)
    }
    pub fn cf_identity_data(&self) -> &ColumnFamily {
        self.cf(CF_IDENTITY_DATA)
    }
    pub fn cf_identity_by_collection(&self) -> &ColumnFamily {
        self.cf(CF_IDENTITY_BY_COLLECTION)
    }
    pub fn cf_stats_identity(&self) -> &ColumnFamily {
        self.cf(CF_STATS_IDENTITY)
    }
    pub fn cf_stats_chain(&self) -> &ColumnFamily {
        self.cf(CF_STATS_CHAIN)
    }
    pub fn cf_stats_dao(&self) -> &ColumnFamily {
        self.cf(CF_STATS_DAO)
    }
    pub fn cf_stats_hodl(&self) -> &ColumnFamily {
        self.cf(CF_STATS_HODL)
    }
    pub fn cf_stats_script(&self) -> &ColumnFamily {
        self.cf(CF_STATS_SCRIPT)
    }
    pub fn cf_stats_token(&self) -> &ColumnFamily {
        self.cf(CF_STATS_TOKEN)
    }
    pub fn cf_stats_spore(&self) -> &ColumnFamily {
        self.cf(CF_STATS_SPORE)
    }
    pub fn cf_stats_object(&self) -> &ColumnFamily {
        self.cf(CF_STATS_OBJECT)
    }
    pub fn cf_script_info(&self) -> &ColumnFamily {
        self.cf(CF_SCRIPT_INFO)
    }
    pub fn cf_script_versions(&self) -> &ColumnFamily {
        self.cf(CF_SCRIPT_VERSIONS)
    }
    pub fn cf_script_versions_by_label(&self) -> &ColumnFamily {
        self.cf(CF_SCRIPT_VERSIONS_BY_LABEL)
    }
    pub fn cf_sync_meta(&self) -> &ColumnFamily {
        self.cf(CF_SYNC_META)
    }
    pub fn cf_spore_by_cluster(&self) -> &ColumnFamily {
        self.cf(CF_SPORE_BY_CLUSTER)
    }
    pub fn cf_cell_by_lock_code(&self) -> &ColumnFamily {
        self.cf(CF_CELL_BY_LOCK_CODE)
    }
    pub fn cf_cell_by_type_code(&self) -> &ColumnFamily {
        self.cf(CF_CELL_BY_TYPE_CODE)
    }
    pub fn cf_cell_by_data_hash(&self) -> &ColumnFamily {
        self.cf(CF_CELL_BY_DATA_HASH)
    }
    pub fn cf_token_transfers(&self) -> &ColumnFamily {
        self.cf(CF_TOKEN_TRANSFERS)
    }
    pub fn cf_activities(&self) -> &ColumnFamily {
        self.cf(CF_ACTIVITIES)
    }
    pub fn cf_cluster_agg(&self) -> &ColumnFamily {
        self.cf(CF_CLUSTER_AGG)
    }
    pub fn cf_object_collection_agg(&self) -> &ColumnFamily {
        self.cf(CF_OBJECT_COLLECTION_AGG)
    }
    pub fn cf_object_collection_activities(&self) -> &ColumnFamily {
        self.cf(CF_OBJECT_COLLECTION_ACTIVITIES)
    }
    pub fn cf_identity_agg(&self) -> &ColumnFamily {
        self.cf(CF_IDENTITY_AGG)
    }
    pub fn cf_identity_collection_activities(&self) -> &ColumnFamily {
        self.cf(CF_IDENTITY_COLLECTION_ACTIVITIES)
    }
    pub fn cf_pending_proposals(&self) -> &ColumnFamily {
        self.cf(CF_PENDING_PROPOSALS)
    }
    pub fn cf_fiber_channels(&self) -> &ColumnFamily {
        self.cf(CF_FIBER_CHANNELS)
    }
    pub fn cf_fiber_channel_by_commitment(&self) -> &ColumnFamily {
        self.cf(CF_FIBER_CHANNEL_BY_COMMITMENT)
    }
    pub fn cf_fiber_channel_by_funding_args(&self) -> &ColumnFamily {
        self.cf(CF_FIBER_CHANNEL_BY_FUNDING_ARGS)
    }
    pub fn cf_addr_fiber_channels(&self) -> &ColumnFamily {
        self.cf(CF_ADDR_FIBER_CHANNELS)
    }

    // ---- Raw DB operations ----

    pub fn get_cf(&self, cf: &ColumnFamily, key: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.db.get_cf(cf, key)?)
    }

    pub fn put_cf(&self, cf: &ColumnFamily, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        if self.is_append_only_store() {
            let cf_name = self.append_cf_name_for_handle(cf)?;
            self.validate_append_put_by_cf_name(cf_name, key, value, StoreWriteIntent::Normal)?;
        }
        Ok(self.db.put_cf(cf, key, value)?)
    }

    pub fn delete_cf(&self, cf: &ColumnFamily, key: &[u8]) -> anyhow::Result<()> {
        if self.is_append_only_store() {
            let cf_name = self.append_cf_name_for_handle(cf)?;
            self.validate_append_delete_by_cf_name(cf_name, key, StoreWriteIntent::Normal)?;
        }
        Ok(self.db.delete_cf(cf, key)?)
    }

    pub fn multi_get_cf(
        &self,
        keys: Vec<(&ColumnFamily, &[u8])>,
    ) -> Vec<Result<Option<Vec<u8>>, rocksdb::Error>> {
        self.db.multi_get_cf(keys)
    }

    fn write_batch_unchecked(&self, batch: WriteBatch) -> anyhow::Result<()> {
        Ok(self.db.write(batch)?)
    }

    pub(crate) fn write_batch_with_intent(
        &self,
        batch: WriteBatch,
        intent: StoreWriteIntent,
    ) -> anyhow::Result<()> {
        if self.is_append_only_store()
            && !matches!(
                intent,
                StoreWriteIntent::AppendValidated | StoreWriteIntent::BulkSyncAppendValidated
            )
        {
            anyhow::bail!(
                "append-only raw write_batch blocked for intent={:?}; \
                 use StoreBatch commit path",
                intent
            );
        }
        self.write_batch_unchecked(batch)
    }

    pub fn write_batch(&self, batch: WriteBatch) -> anyhow::Result<()> {
        self.write_batch_with_intent(batch, StoreWriteIntent::Normal)
    }

    /// Write a batch with WAL disabled. Use during bulk sync where crash recovery
    /// re-syncs from the last committed block header.
    fn write_batch_no_wal_unchecked(&self, batch: WriteBatch) -> anyhow::Result<()> {
        let mut opts = rocksdb::WriteOptions::default();
        opts.disable_wal(true);
        Ok(self.db.write_opt(batch, &opts)?)
    }

    pub(crate) fn write_batch_no_wal_with_intent(
        &self,
        batch: WriteBatch,
        intent: StoreWriteIntent,
    ) -> anyhow::Result<()> {
        if self.is_append_only_store()
            && !matches!(
                intent,
                StoreWriteIntent::AppendValidated | StoreWriteIntent::BulkSyncAppendValidated
            )
        {
            anyhow::bail!(
                "append-only raw write_batch_no_wal blocked for intent={:?}; \
                 use StoreBatch commit_no_wal path",
                intent
            );
        }
        self.write_batch_no_wal_unchecked(batch)
    }

    pub fn write_batch_no_wal(&self, batch: WriteBatch) -> anyhow::Result<()> {
        self.write_batch_no_wal_with_intent(batch, StoreWriteIntent::Normal)
    }

    pub(crate) fn apply_batch_op_by_cf_name(
        &self,
        batch: &mut WriteBatch,
        cf_name: &str,
        key: &[u8],
        value: Option<&[u8]>,
    ) -> anyhow::Result<()> {
        self.apply_batch_op_by_cf_name_with_intent(
            batch,
            cf_name,
            key,
            value,
            StoreWriteIntent::Normal,
        )
    }

    pub(crate) fn apply_batch_op_by_cf_name_with_intent(
        &self,
        batch: &mut WriteBatch,
        cf_name: &str,
        key: &[u8],
        value: Option<&[u8]>,
        intent: StoreWriteIntent,
    ) -> anyhow::Result<()> {
        if !Self::cf_allowed(self.store_class, cf_name) {
            anyhow::bail!(
                "CF '{}' is not allowed in {:?} store",
                cf_name,
                self.store_class
            );
        }

        let cf = self
            .db
            .cf_handle(cf_name)
            .ok_or_else(|| anyhow::anyhow!("CF '{}' not found", cf_name))?;
        if let Some(v) = value {
            self.validate_append_put_by_cf_name(cf_name, key, v, intent)?;
            batch.put_cf(cf, key, v);
        } else {
            self.validate_append_delete_by_cf_name(cf_name, key, intent)?;
            batch.delete_cf(cf, key);
        }
        Ok(())
    }

    /// Resolve the target stats CF by stats key prefix.
    pub fn stats_cf_by_prefix(&self, prefix: u8) -> anyhow::Result<&ColumnFamily> {
        match prefix {
            keys::STATS_PREFIX_DAILY
            | keys::STATS_PREFIX_HOURLY
            | keys::STATS_PREFIX_EPOCH
            | keys::STATS_PREFIX_MINER
            | keys::STATS_PREFIX_BLOCK_TIME_DIST
            | keys::STATS_PREFIX_EPOCH_TIME_DIST
            | keys::STATS_PREFIX_DAILY_BLOCK
            | keys::STATS_PREFIX_ACTIVITY_DAILY
            | keys::STATS_PREFIX_ACTIVITY_HOURLY => Ok(self.cf_stats_chain()),
            keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT
            | keys::STATS_PREFIX_DAO_LATEST_STATS
            | keys::STATS_PREFIX_DAO_TOP_DEPOSITORS => Ok(self.cf_stats_dao()),
            keys::STATS_PREFIX_HODL_WAVE
            | keys::STATS_PREFIX_CELL_DISTRIBUTION
            | keys::STATS_PREFIX_ADDR_COHORT => Ok(self.cf_stats_hodl()),
            keys::STATS_PREFIX_SCRIPT_DAILY => Ok(self.cf_stats_script()),
            keys::STATS_PREFIX_TOKEN_TRANSFERS
            | keys::STATS_PREFIX_TOKEN_HOURLY
            | keys::STATS_PREFIX_TOKEN_DAILY => Ok(self.cf_stats_token()),
            keys::STATS_PREFIX_CLUSTER_OWNER
            | keys::STATS_PREFIX_SPORE_HOURLY
            | keys::STATS_PREFIX_CLUSTER_DAILY
            | keys::STATS_PREFIX_SPORE_DAILY
            | keys::STATS_PREFIX_SPORE_OUTPOINT
            | keys::STATS_PREFIX_SPORE_TYPE_INDEX
            | keys::STATS_PREFIX_SPORE_OUTPOINT_BY_ID => Ok(self.cf_stats_spore()),
            keys::STATS_PREFIX_NFT_HOURLY
            | keys::STATS_PREFIX_NFT_DAILY
            | keys::STATS_PREFIX_NFT_TYPE_INDEX
            | keys::STATS_PREFIX_MNFT_CLASS_OUTPOINT
            | keys::STATS_PREFIX_MNFT_TOKEN_OUTPOINT
            | keys::STATS_PREFIX_DOTBIT_ACCOUNT_OUTPOINT
            | keys::STATS_PREFIX_DOTBIT_OUTPOINT_BY_ACCOUNT_ID
            | keys::STATS_PREFIX_NFT_COLLECTION_OWNER => Ok(self.cf_stats_object()),
            _ => anyhow::bail!("unsupported stats prefix: 0x{:02x}", prefix),
        }
    }

    /// Resolve target stats CF for a full key.
    pub fn cf_for_stats_key(&self, key: &[u8]) -> anyhow::Result<&ColumnFamily> {
        let Some(prefix) = key.first().copied() else {
            anyhow::bail!("empty stats key");
        };
        self.stats_cf_by_prefix(prefix)
    }

    pub fn get_stats_key(&self, key: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        let cf = self.cf_for_stats_key(key)?;
        self.get_cf(cf, key)
    }

    pub fn put_stats_key(&self, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        let cf = self.cf_for_stats_key(key)?;
        self.put_cf(cf, key, value)
    }

    /// Returns true if the CF_CELLS column family contains at least one entry.
    /// Used by bulk sync guard to verify append-only store is empty.
    pub fn has_any_data_in_cells_cf(&self) -> anyhow::Result<bool> {
        let cf = self.cf_cells();
        let mut iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        Ok(iter.next().is_some())
    }

    /// Iterate over a CF starting from a specific key.
    pub fn iterator_cf(
        &self,
        cf: &ColumnFamily,
        mode: IteratorMode,
    ) -> impl Iterator<Item = KvResult> + '_ {
        self.db.iterator_cf(cf, mode)
    }

    pub(crate) fn snapshot(&self) -> rocksdb::Snapshot<'_> {
        self.db.snapshot()
    }

    /// Iterate over a CF with a prefix.
    ///
    /// Uses `total_order_seek` instead of RocksDB's built-in `prefix_iterator_cf`
    /// which requires a configured prefix extractor to work correctly.  Without
    /// one, `set_prefix_same_as_start(true)` can silently skip SST files and
    /// return incomplete results.
    pub fn prefix_iterator_cf(
        &self,
        cf: &ColumnFamily,
        prefix: &[u8],
    ) -> impl Iterator<Item = KvResult> + '_ {
        let mut opts = rocksdb::ReadOptions::default();
        opts.set_total_order_seek(true);
        let mode = IteratorMode::From(prefix, rocksdb::Direction::Forward);
        let prefix_vec = prefix.to_vec();
        self.db
            .iterator_cf_opt(cf, opts, mode)
            .take_while(move |item| match item {
                Ok((key, _)) => key.starts_with(&prefix_vec),
                Err(_) => true,
            })
    }

    // ---- Bulk sync mode ----

    pub fn set_bulk_sync_mode(&self, enabled: bool) {
        self.bulk_sync_mode.store(enabled, Ordering::Relaxed);
    }

    pub fn is_bulk_sync_mode(&self) -> bool {
        self.bulk_sync_mode.load(Ordering::Relaxed)
    }

    pub fn memory_profile(&self) -> &MemoryProfile {
        &self.memory_profile
    }

    /// Log the key RocksDB tuning parameters at startup.
    pub fn log_config(&self) {
        let p = &self.memory_profile;
        info!(
            system_ram_gb = p.system_ram_bytes / (1024 * 1024 * 1024),
            cpu_count = p.cpu_count,
            rocksdb_budget_gb = p.rocksdb_budget_bytes / (1024 * 1024 * 1024),
            write_buffer_mega_mb = p.write_buffer_mega_bytes / (1024 * 1024),
            write_buffer_high_mb = p.write_buffer_high_bytes / (1024 * 1024),
            write_buffer_low_mb = p.write_buffer_low_bytes / (1024 * 1024),
            write_buffer_hot_cf_mb = p.write_buffer_hot_cf_bytes / (1024 * 1024),
            max_write_buffers_high = 4,
            max_write_buffers_low = 2,
            l0_slowdown = 20,
            l0_stop = 48,
            l0_slowdown_bulk = 64,
            l0_stop_bulk = 128,
            max_background_jobs = p.max_background_jobs,
            max_subcompactions = p.max_subcompactions,
            block_cache_normal_mb = p.block_cache_normal_bytes / (1024 * 1024),
            block_cache_bulk_mb = p.block_cache_bulk_sync_bytes / (1024 * 1024),
            wbm_normal_mb = p.wbm_normal_bytes / (1024 * 1024),
            wbm_bulk_mb = p.wbm_bulk_sync_bytes / (1024 * 1024),
            unordered_write = true,
            direct_io_reads = self.runtime_config.direct_io_reads,
            direct_io_compaction = true,
            bytes_per_sync_mb = 4,
            dynamic_level_bytes = true,
            target_file_size_base_normal_mb = p.normal_target_file_size_base / (1024 * 1024),
            target_file_size_base_bulk_mb = p.bulk_target_file_size_base / (1024 * 1024),
            severe_pending_gb = p.severe_compaction_pending_bytes / (1024 * 1024 * 1024),
            moderate_pending_gb = p.moderate_compaction_pending_bytes / (1024 * 1024 * 1024),
            severe_pending_bulk_gb = p.severe_compaction_pending_bytes_bulk / (1024 * 1024 * 1024),
            moderate_pending_bulk_gb =
                p.moderate_compaction_pending_bytes_bulk / (1024 * 1024 * 1024),
            severe_immutable_memtables = p.severe_immutable_memtables,
            moderate_immutable_memtables = p.moderate_immutable_memtables,
            drain_pending_mb = p.drain_pending_bytes_threshold / (1024 * 1024),
            mega_write_cfs = Self::MEGA_WRITE_CFS.len(),
            high_write_cfs = Self::HIGH_WRITE_CFS.len(),
            historical_append_cfs = Self::HISTORICAL_APPEND_CFS.len(),
            column_families = ALL_CFS.len(),
            "RocksDB configuration"
        );
    }

    pub fn is_secondary(&self) -> bool {
        self.is_secondary
    }

    pub fn domain_path(&self) -> &Path {
        &self.domain_path
    }

    pub fn append_path(&self) -> &Path {
        &self.append_path
    }

    pub fn runtime_config(&self) -> StoreRuntimeConfig {
        self.runtime_config
    }

    /// Set relaxed L0 thresholds and larger write buffers for bulk sync.
    ///
    /// During bulk sync, parallel writer threads each commit large WriteBatches.
    /// The default per-CF `max_write_buffer_number=4` can cause flush stalls when
    /// memtables fill faster than background flush can drain. Increasing to
    /// 12 (mega) / 8 (high) / 6 (low) gives more headroom before stalling.
    ///
    /// Also expands the global WriteBufferManager budget and shrinks block cache
    /// to shift memory from reads to writes during the write-heavy bulk phase.
    pub fn set_bulk_sync_compaction_options(&self) {
        // Idempotent: skip if already in bulk mode
        if self.bulk_sync_mode.load(Ordering::Relaxed) {
            return;
        }
        self.bulk_sync_mode.store(true, Ordering::Relaxed);

        let p = &self.memory_profile;
        let level_base_str = p.bulk_max_bytes_for_level_base.to_string();
        let file_base_str = p.bulk_target_file_size_base.to_string();
        let hot_cf_buffer_str = p.write_buffer_hot_cf_bytes.to_string();

        self.write_buffer_manager
            .set_buffer_size(p.wbm_bulk_sync_bytes);

        self.block_cache
            .lock()
            .expect("block_cache lock poisoned")
            .set_capacity(p.block_cache_bulk_sync_bytes);

        let mut ok = 0u32;
        let mut fail = 0u32;
        for &cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                // More write buffers = more memtable headroom before flush stall.
                // With WBM controlling global budget, per-CF limits are a safety net.
                let max_wb = if Self::is_mega_write_cf(cf_name) {
                    "12"
                } else if Self::is_high_write_cf(cf_name) {
                    "8"
                } else {
                    "6"
                };
                let result = self.db.set_options_cf(
                    cf,
                    &[
                        ("level0_slowdown_writes_trigger", "96"),
                        ("level0_stop_writes_trigger", "192"),
                        ("max_write_buffer_number", max_wb),
                        ("max_bytes_for_level_base", &level_base_str),
                        ("target_file_size_base", &file_base_str),
                    ],
                );
                if result.is_ok() {
                    ok += 1;
                } else {
                    warn!(
                        "Failed to set bulk sync options for CF '{}': {:?}",
                        cf_name,
                        result.err()
                    );
                    fail += 1;
                }
            }
        }

        for &hot_cf_name in &[
            CF_TX_INDEX,
            CF_CELLS,
            CF_LIVE_CELLS,
            CF_CONSUMED_CELLS,
            CF_CELL_BY_LOCK,
            CF_ACTIVITIES,
        ] {
            if let Some(cf) = self.db.cf_handle(hot_cf_name) {
                let result = self
                    .db
                    .set_options_cf(cf, &[("write_buffer_size", &hot_cf_buffer_str)]);
                if result.is_ok() {
                    ok += 1;
                } else {
                    warn!(
                        cf = hot_cf_name,
                        "Failed to set hot CF write_buffer_size: {:?}",
                        result.err()
                    );
                    fail += 1;
                }
            }
        }

        info!(
            ok,
            fail,
            wbm_budget_mb = p.wbm_bulk_sync_bytes / (1024 * 1024),
            block_cache_mb = p.block_cache_bulk_sync_bytes / (1024 * 1024),
            "Bulk sync compaction options set: l0_slowdown=96, l0_stop=192, \
             write_buffers mega=12/high=8/low=6"
        );
    }

    /// Restore normal L0 thresholds and write buffer counts after bulk sync.
    ///
    /// Reverts L0 slowdown/stop triggers to 12/24, write buffers to 4 (mega/high)
    /// or 2 (low), `max_bytes_for_level_base` to 512 MB, and restores WBM budget
    /// and block cache to normal sizes.
    ///
    /// **Critical:** flushes all memtables BEFORE reducing WBM budget. Without this,
    /// data written with `commit_no_wal()` during bulk sync can sit in the active
    /// memtable indefinitely — the reduced WBM budget has massive headroom for the
    /// low-write-rate live sync phase, so no automatic flush triggers. A crash then
    /// loses all unflushed data, creating block header gaps.
    pub fn restore_normal_compaction_options(&self) {
        // Idempotent: skip if already in normal mode
        if !self.bulk_sync_mode.load(Ordering::Relaxed) {
            return;
        }
        self.bulk_sync_mode.store(false, Ordering::Relaxed);

        // Flush all memtables to SST BEFORE reducing WBM budget.
        // This ensures all commit_no_wal() data from bulk sync is durable.
        if let Err(e) = self.flush_all_memtables() {
            error!(
                error = %e,
                "Failed to flush memtables during bulk-to-normal transition; \
                 unflushed commit_no_wal data is at risk on crash"
            );
        }

        let p = &self.memory_profile;
        let level_base_str = p.normal_max_bytes_for_level_base.to_string();
        let file_base_str = p.normal_target_file_size_base.to_string();

        self.write_buffer_manager
            .set_buffer_size(p.wbm_normal_bytes);

        self.block_cache
            .lock()
            .expect("block_cache lock poisoned")
            .set_capacity(p.block_cache_normal_bytes);

        for &cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                let max_wb = if Self::is_mega_write_cf(cf_name) || Self::is_high_write_cf(cf_name) {
                    "4"
                } else {
                    "2"
                };
                let _ = self.db.set_options_cf(
                    cf,
                    &[
                        ("level0_slowdown_writes_trigger", "12"),
                        ("level0_stop_writes_trigger", "24"),
                        ("max_write_buffer_number", max_wb),
                        ("max_bytes_for_level_base", &level_base_str),
                        ("target_file_size_base", &file_base_str),
                    ],
                );
            }
        }
        info!(
            wbm_budget_mb = p.wbm_normal_bytes / (1024 * 1024),
            block_cache_mb = p.block_cache_normal_bytes / (1024 * 1024),
            "Normal compaction options restored: l0_slowdown=12, l0_stop=24"
        );
    }

    /// Flush all column family memtables to SST files.
    ///
    /// Required after bulk sync (commit_no_wal) to make memtable data durable.
    /// With atomic_flush=true, flushing any single CF triggers all CFs to flush
    /// together, but we iterate all CFs to ensure every memtable is captured.
    pub fn flush_all_memtables(&self) -> anyhow::Result<()> {
        let started = std::time::Instant::now();

        let cfs = Self::cfs_for_class(self.store_class);
        for &cf_name in cfs {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                self.db.flush_cf(cf).map_err(|e| {
                    anyhow::anyhow!("flush_all_memtables failed on CF '{}': {}", cf_name, e)
                })?;
            }
        }

        let elapsed_ms = started.elapsed().as_millis();
        info!(elapsed_ms, cfs = cfs.len(), "Flushed all memtables to SST");
        Ok(())
    }

    /// Compact the highest-write column families to reclaim space and reduce
    /// L0 file count.  Intended for mid-bulk-sync pressure relief, not routine use.
    pub fn compact_hot_cfs(&self) {
        let hot_cfs: &[&str] = &[
            CF_ACTIVITIES,
            CF_LIVE_CELLS,
            CF_CONSUMED_CELLS,
            CF_CELL_BY_LOCK,
            CF_ADDR_TXS,
            CF_TX_INDEX,
        ];
        let started = std::time::Instant::now();
        for &cf_name in hot_cfs {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                self.db.compact_range_cf(cf, None::<&[u8]>, None::<&[u8]>);
            }
        }
        let elapsed_ms = started.elapsed().as_millis();
        info!(elapsed_ms, cfs = hot_cfs.len(), "Compacted hot CFs");
    }

    /// Disable auto-compactions on all column families.
    /// Call during bulk sync to avoid compaction competing with writes.
    pub fn disable_auto_compactions(&self) {
        for cf_name in ALL_CFS.iter().copied() {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                if let Err(e) = self
                    .db
                    .set_options_cf(cf, &[("disable_auto_compactions", "true")])
                {
                    tracing::warn!(cf = cf_name, error = %e, "Failed to disable auto compactions");
                }
            }
        }
        info!("Auto-compactions disabled for bulk sync");
    }

    // ---- Memory stats ----

    /// Lightweight compaction pressure snapshot for the adaptive batch controller.
    /// Only collects the 3 RocksDB properties needed for backpressure decisions,
    /// avoiding the full CF iteration of `memory_stats()`.
    pub fn compaction_pressure(&self) -> CompactionPressureSnapshot {
        let mut compaction_pending_bytes = 0u64;
        let mut l0_files_total = 0u64;
        let mut l0_files_max: u64 = 0;
        let mut immutable_memtables = 0u64;
        for &cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.estimate-pending-compaction-bytes")
                {
                    compaction_pending_bytes += v;
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.num-files-at-level0")
                {
                    l0_files_total += v;
                    l0_files_max = l0_files_max.max(v);
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.num-immutable-mem-table")
                {
                    immutable_memtables += v;
                }
            }
        }
        CompactionPressureSnapshot {
            l0_files_total,
            l0_files_max,
            compaction_pending_bytes,
            immutable_memtables,
        }
    }

    pub fn memory_stats(&self) -> MemoryStats {
        let mut memtable_bytes = 0usize;
        let mut table_readers_bytes = 0usize;
        let mut cells_count = 0usize;
        let mut live_cells_count = 0usize;
        let mut consumed_cells_count = 0usize;
        let mut consumed_cf_live_data_bytes: Option<u64> = None;
        let mut consumed_cf_sst_files_bytes: Option<u64> = None;
        let mut consumed_cf_memtable_bytes: Option<u64> = None;
        let mut block_headers_count = 0usize;
        let mut addr_balance_count = 0usize;
        let mut compaction_pending_bytes = 0u64;
        let mut num_running_compactions_fallback = 0u64;
        let mut sst_files_size = 0u64;
        let mut l0_files_total = 0u64;
        let mut l0_files_max: u64 = 0;
        let mut l0_worst_cf = String::new();
        let mut immutable_memtables = 0u64;
        let mut cf_sizes: Vec<(String, u64)> = Vec::new();

        for &cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.cur-size-all-mem-tables")
                {
                    memtable_bytes += v as usize;
                    if cf_name == CF_CONSUMED_CELLS {
                        consumed_cf_memtable_bytes = Some(v);
                    }
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.estimate-table-readers-mem")
                {
                    table_readers_bytes += v as usize;
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.estimate-pending-compaction-bytes")
                {
                    compaction_pending_bytes += v;
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.estimate-num-keys")
                {
                    match cf_name {
                        CF_CELLS => cells_count = v as usize,
                        CF_LIVE_CELLS => live_cells_count = v as usize,
                        CF_CONSUMED_CELLS => consumed_cells_count = v as usize,
                        CF_BLOCK_HEADERS => block_headers_count = v as usize,
                        CF_ADDR_BALANCE => addr_balance_count = v as usize,
                        _ => {}
                    }
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.num-running-compactions")
                {
                    num_running_compactions_fallback = num_running_compactions_fallback.max(v);
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.total-sst-files-size")
                {
                    sst_files_size += v;
                    if cf_name == CF_CONSUMED_CELLS {
                        consumed_cf_sst_files_bytes = Some(v);
                    }
                }
                // L0 file count — track both total and per-CF max
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.num-files-at-level0")
                {
                    l0_files_total += v;
                    if v > l0_files_max {
                        l0_files_max = v;
                        l0_worst_cf = cf_name.to_string();
                    }
                }
                // Immutable memtables waiting for flush — high values indicate
                // flush can't keep up and writes will stall when all buffers fill.
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.num-immutable-mem-table")
                {
                    immutable_memtables += v;
                }
                // Per-CF live data size for top-N display
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.estimate-live-data-size")
                {
                    if cf_name == CF_CONSUMED_CELLS {
                        consumed_cf_live_data_bytes = Some(v);
                    }
                    if v > 0 {
                        cf_sizes.push((cf_name.to_string(), v));
                    }
                }
            }
        }

        // Sort by size descending and keep top 5
        cf_sizes.sort_by(|a, b| b.1.cmp(&a.1));
        cf_sizes.truncate(5);
        let (consumed_cells_bytes, consumed_cells_bytes_source) = consumed_cf_storage_bytes(
            consumed_cf_live_data_bytes,
            consumed_cf_sst_files_bytes,
            consumed_cf_memtable_bytes,
        );

        let block_cache_bytes = self
            .block_cache
            .lock()
            .expect("block_cache lock poisoned")
            .get_usage();
        let memory_bytes = memtable_bytes + block_cache_bytes + table_readers_bytes;
        let num_running_compactions = self
            .db
            .property_int_value("rocksdb.num-running-compactions")
            .ok()
            .flatten()
            .unwrap_or(num_running_compactions_fallback);

        MemoryStats {
            live_cells_count,
            consumed_cells_count,
            consumed_cells_bytes,
            consumed_cells_bytes_source,
            block_headers_count,
            addr_balance_count,
            cells_count,
            memory_bytes,
            memtable_bytes,
            block_cache_bytes,
            table_readers_bytes,
            compaction_pending_bytes,
            num_running_compactions,
            sst_files_size,
            l0_files_count: l0_files_total,
            l0_files_max,
            l0_worst_cf,
            immutable_memtables,
            top_cf_sizes: cf_sizes,
            wbm_usage_bytes: self.write_buffer_manager.get_usage(),
            wbm_budget_bytes: self.write_buffer_manager.get_buffer_size(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_open_and_close() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        assert!(!store.is_secondary());
        drop(store);
    }

    #[test]
    fn test_all_cfs_accessible() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        for cf_name in ALL_CFS {
            let _ = store.cf(cf_name);
        }
    }

    #[test]
    fn test_open_domain_restricts_append_only_cfs() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let panicked = std::panic::catch_unwind(|| {
            let _ = store.cf_cells();
        })
        .is_err();
        assert!(panicked, "domain store should reject append-only CF access");
    }

    #[test]
    fn test_open_domain_allows_activities_cf() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        // Activities CF is now in domain store, access should succeed
        let _ = store.cf_activities();
    }

    #[test]
    fn test_open_append_only_restricts_domain_cfs() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let panicked = std::panic::catch_unwind(|| {
            let _ = store.cf_sync_meta();
        })
        .is_err();
        assert!(panicked, "append-only store should reject domain CF access");
    }

    #[test]
    fn test_open_append_only_allows_cells_cf() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let _ = store.cf_cells();
    }

    #[test]
    fn test_open_append_only_rejects_path_with_domain_cfs() {
        let dir = TempDir::new().unwrap();
        let _ = CkbadgerStore::open_domain(dir.path()).unwrap();
        let err = match CkbadgerStore::open_append_only(dir.path()) {
            Ok(_) => panic!("expected open_append_only to fail on domain CF path"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("not allowed there"));
    }

    #[test]
    fn test_open_domain_rejects_path_with_append_cfs() {
        let dir = TempDir::new().unwrap();
        let _ = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let err = match CkbadgerStore::open_domain(dir.path()) {
            Ok(_) => panic!("expected open_domain to fail on append-only CF path"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("not allowed there"));
    }

    #[test]
    fn test_append_only_put_rejects_duplicate_same_value() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let cf = store.cf_cells();

        store.put_cf(cf, b"k1", b"v1").unwrap();
        let err = store.put_cf(cf, b"k1", b"v1").unwrap_err();
        assert!(err.to_string().contains("append-only overwrite blocked"));
    }

    #[test]
    fn test_append_only_put_rejects_overwrite_with_different_value() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let cf = store.cf_cells();

        store.put_cf(cf, b"k1", b"v1").unwrap();
        let err = store.put_cf(cf, b"k1", b"v2").unwrap_err();
        assert!(err.to_string().contains("append-only overwrite blocked"));
    }

    #[test]
    fn test_append_only_delete_rejected_in_normal_path() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let cf = store.cf_cells();

        store.put_cf(cf, b"k1", b"v1").unwrap();
        let err = store.delete_cf(cf, b"k1").unwrap_err();
        assert!(err.to_string().contains("append-only delete blocked"));
    }

    #[test]
    fn test_append_only_raw_write_batch_rejected_in_normal_path() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();

        let mut batch = WriteBatch::default();
        batch.put_cf(store.cf_cells(), b"k1", b"v1");
        let err = store.write_batch(batch).unwrap_err();
        assert!(err
            .to_string()
            .contains("append-only raw write_batch blocked"));
    }

    #[test]
    fn test_append_only_delete_rejected_in_append_validated_intent() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let cf = store.cf_cells();
        store.put_cf(cf, b"k1", b"v1").unwrap();

        let mut batch = WriteBatch::default();
        let err = store
            .apply_batch_op_by_cf_name_with_intent(
                &mut batch,
                CF_CELLS,
                b"k1",
                None,
                StoreWriteIntent::AppendValidated,
            )
            .unwrap_err();
        assert!(err.to_string().contains("append-only delete blocked"));
    }

    #[test]
    fn test_put_get_delete() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let cf = store.cf_sync_meta();
        store.put_cf(cf, b"test_key", b"test_value").unwrap();

        let val = store.get_cf(cf, b"test_key").unwrap();
        assert_eq!(val.as_deref(), Some(b"test_value".as_slice()));

        store.delete_cf(cf, b"test_key").unwrap();
        let val = store.get_cf(cf, b"test_key").unwrap();
        assert!(val.is_none());
    }

    #[test]
    fn test_stats_key_routing_writes_to_split_cfs() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let cases: Vec<(Vec<u8>, &[u8])> = vec![
            (
                crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_DAILY, b"20240201"),
                b"chain",
            ),
            (
                crate::keys::encode_stats_key(
                    crate::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
                    b"20240201",
                ),
                b"dao",
            ),
            (
                crate::keys::encode_stats_key(
                    crate::keys::STATS_PREFIX_DAO_LATEST_STATS,
                    b"latest",
                ),
                b"dao_latest",
            ),
            (
                crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_HODL_WAVE, b"20240201"),
                b"hodl",
            ),
            (
                crate::keys::encode_script_daily_key(&[0xAB; 32], false, 20240201).to_vec(),
                b"script",
            ),
            (
                crate::keys::encode_token_daily_key(&[0xBC; 32], 20240201).to_vec(),
                b"token",
            ),
            (
                crate::keys::encode_spore_daily_key(&[0xCD; 32], 20240201).to_vec(),
                b"spore",
            ),
            (
                crate::keys::encode_nft_daily_key(&[0xDE; 24], 20240201).to_vec(),
                b"nft",
            ),
        ];

        for (key, value) in &cases {
            store.put_stats_key(key, value).unwrap();
            let loaded = store.get_stats_key(key).unwrap().unwrap();
            assert_eq!(loaded, *value);
        }

        // Verify representative keys landed in expected split CFs.
        assert!(store
            .get_cf(store.cf_stats_chain(), &cases[0].0)
            .unwrap()
            .is_some());
        assert!(store
            .get_cf(store.cf_stats_dao(), &cases[1].0)
            .unwrap()
            .is_some());
        assert!(store
            .get_cf(store.cf_stats_dao(), &cases[2].0)
            .unwrap()
            .is_some());
        assert!(store
            .get_cf(store.cf_stats_hodl(), &cases[3].0)
            .unwrap()
            .is_some());
        assert!(store
            .get_cf(store.cf_stats_script(), &cases[4].0)
            .unwrap()
            .is_some());
        assert!(store
            .get_cf(store.cf_stats_token(), &cases[5].0)
            .unwrap()
            .is_some());
        assert!(store
            .get_cf(store.cf_stats_spore(), &cases[6].0)
            .unwrap()
            .is_some());
        assert!(store
            .get_cf(store.cf_stats_object(), &cases[7].0)
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_stats_key_routing_rejects_unknown_prefix() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let err = store.put_stats_key(&[0xFE, 0x00], b"v").unwrap_err();
        assert!(err.to_string().contains("unsupported stats prefix"));
    }

    #[test]
    fn test_write_batch() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let cf = store.cf_sync_meta();
        let mut batch = WriteBatch::default();
        batch.put_cf(cf, b"k1", b"v1");
        batch.put_cf(cf, b"k2", b"v2");
        store.write_batch(batch).unwrap();

        assert_eq!(
            store.get_cf(cf, b"k1").unwrap().as_deref(),
            Some(b"v1".as_slice())
        );
        assert_eq!(
            store.get_cf(cf, b"k2").unwrap().as_deref(),
            Some(b"v2".as_slice())
        );
    }

    #[test]
    fn test_secondary_instance() {
        let primary_dir = TempDir::new().unwrap();
        let secondary_dir = TempDir::new().unwrap();

        let primary = CkbadgerStore::open_domain(primary_dir.path()).unwrap();
        let cf = primary.cf_sync_meta();
        primary.put_cf(cf, b"key", b"value").unwrap();

        let secondary =
            CkbadgerStore::open_domain_secondary(primary_dir.path(), secondary_dir.path()).unwrap();
        assert!(secondary.is_secondary());
        secondary.refresh().unwrap();

        let cf = secondary.cf_sync_meta();
        let val = secondary.get_cf(cf, b"key").unwrap();
        assert_eq!(val.as_deref(), Some(b"value".as_slice()));
    }

    #[test]
    fn test_append_path_from_domain_uses_workdir_layout() {
        assert_eq!(
            append_path_from_domain(Path::new("/tmp/work/data/domain")),
            PathBuf::from("/tmp/work/data/append-only")
        );
        assert_eq!(
            append_path_from_domain(Path::new("/tmp/custom-domain")),
            PathBuf::from("/tmp/custom-domain-append-only")
        );
    }

    #[test]
    fn test_domain_path_from_append_uses_workdir_layout() {
        assert_eq!(
            domain_path_from_append(Path::new("/tmp/work/data/append-only")),
            PathBuf::from("/tmp/work/data/domain")
        );
        assert_eq!(
            domain_path_from_append(Path::new("/tmp/custom-domain-append-only")),
            PathBuf::from("/tmp/custom-domain")
        );
    }

    #[test]
    fn test_open_domain_with_runtime_uses_explicit_store_runtime_config() {
        let dir = TempDir::new().unwrap();
        let runtime_config = StoreRuntimeConfig {
            memory_budget_gb: Some(12),
            direct_io_reads: false,
        };

        let store = CkbadgerStore::open_domain_with_runtime(dir.path(), runtime_config).unwrap();

        assert_eq!(store.runtime_config(), runtime_config);
        assert_eq!(store.memory_profile().system_ram_bytes, 12 * GB);
    }

    #[test]
    fn test_bulk_sync_mode() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        assert!(!store.is_bulk_sync_mode());
        store.set_bulk_sync_mode(true);
        assert!(store.is_bulk_sync_mode());
        store.set_bulk_sync_mode(false);
        assert!(!store.is_bulk_sync_mode());
    }

    #[test]
    fn test_memory_stats() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        store
            .put_cf(store.cf_cells(), b"cell-k1", b"cell-v1")
            .unwrap();
        store
            .put_cf(store.cf_live_cells(), b"live-k1", b"live-v1")
            .unwrap();
        store
            .put_cf(store.cf_consumed_cells(), b"consumed-k1", b"consumed-v1")
            .unwrap();
        store
            .put_cf(store.cf_block_headers(), b"hdr-k1", b"hdr-v1")
            .unwrap();
        store
            .put_addr_balance_direct(
                b"addr-k1",
                &crate::types::AddressBalance {
                    balance: 1,
                    ..Default::default()
                },
            )
            .unwrap();

        // Flush written keys so RocksDB estimate properties have observable values.
        store.db.flush_cf(store.cf_cells()).unwrap();
        store.db.flush_cf(store.cf_live_cells()).unwrap();
        store.db.flush_cf(store.cf_consumed_cells()).unwrap();
        store.db.flush_cf(store.cf_block_headers()).unwrap();
        store.db.flush_cf(store.cf_addr_balance()).unwrap();

        let stats = store.memory_stats();
        assert!(stats.live_cells_count >= 1);
        assert!(stats.consumed_cells_count >= 1);
        assert!(stats.block_headers_count >= 1);
        assert!(stats.addr_balance_count >= 1);
        assert!(stats.cells_count >= stats.live_cells_count);
        assert!(
            matches!(
                stats.consumed_cells_bytes_source,
                "live" | "sst" | "mem" | "none"
            ),
            "unexpected source: {}",
            stats.consumed_cells_bytes_source
        );
    }

    #[test]
    fn test_memory_stats_compaction_count_matches_global_property_when_available() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let stats = store.memory_stats();
        let global = store
            .db
            .property_int_value("rocksdb.num-running-compactions")
            .unwrap();
        if let Some(expected) = global {
            assert_eq!(stats.num_running_compactions, expected);
        }
    }

    #[test]
    fn test_compaction_pressure_snapshot_reports_l0_total_and_l0_max() {
        let snapshot = CompactionPressureSnapshot {
            l0_files_total: 82,
            l0_files_max: 3,
            compaction_pending_bytes: 0,
            immutable_memtables: 0,
        };

        assert_eq!(snapshot.l0_files_total, 82);
        assert_eq!(snapshot.l0_files_max, 3);
    }

    #[test]
    fn test_consumed_cf_storage_bytes_prefers_live_data_estimate() {
        assert_eq!(
            consumed_cf_storage_bytes(Some(100), Some(500), Some(20)),
            (120, "live")
        );
    }

    #[test]
    fn test_consumed_cf_storage_bytes_falls_back_to_sst_when_live_missing() {
        assert_eq!(
            consumed_cf_storage_bytes(None, Some(500), Some(20)),
            (520, "sst")
        );
        assert_eq!(
            consumed_cf_storage_bytes(Some(0), Some(500), Some(20)),
            (520, "sst")
        );
    }

    #[test]
    fn test_consumed_cf_storage_bytes_returns_none_when_all_missing() {
        assert_eq!(consumed_cf_storage_bytes(None, None, None), (0, "none"));
    }

    #[test]
    fn test_consumed_cf_storage_bytes_memtable_only_source() {
        assert_eq!(consumed_cf_storage_bytes(None, None, Some(20)), (20, "mem"));
    }

    #[test]
    fn test_mega_write_cfs_contains_activities() {
        assert!(
            CkbadgerStore::is_mega_write_cf(CF_ACTIVITIES),
            "CF_ACTIVITIES should be in MEGA_WRITE_CFS"
        );
    }

    #[test]
    fn test_mega_write_cfs_excludes_script_info() {
        assert!(
            !CkbadgerStore::is_mega_write_cf(CF_SCRIPT_INFO),
            "CF_SCRIPT_INFO should NOT be in MEGA_WRITE_CFS"
        );
    }

    #[test]
    fn test_mega_write_cfs_excludes_script_cfs() {
        for cf in [CF_SCRIPT_VERSIONS, CF_SCRIPT_VERSIONS_BY_LABEL] {
            assert!(
                !CkbadgerStore::is_mega_write_cf(cf),
                "{cf} should NOT be in MEGA_WRITE_CFS"
            );
        }
    }

    #[test]
    fn test_mega_write_cfs_expected_members() {
        let expected = &[
            CF_CELLS,
            CF_LIVE_CELLS,
            CF_CONSUMED_CELLS,
            CF_REORG_UNDO_LOG_BY_BLOCK,
            CF_CELL_BY_LOCK,
            CF_CELL_BY_TYPE,
            CF_CELL_BY_LOCK_CODE,
            CF_CELL_BY_TYPE_CODE,
            CF_TX_INDEX,
            CF_TX_HASH_MAP,
            CF_ADDR_BALANCE,
            CF_ADDR_TXS,
            CF_ACTIVITIES,
        ];
        for cf in expected {
            assert!(
                CkbadgerStore::is_mega_write_cf(cf),
                "{cf} should be in MEGA_WRITE_CFS"
            );
        }
        assert_eq!(
            CkbadgerStore::MEGA_WRITE_CFS.len(),
            expected.len(),
            "MEGA_WRITE_CFS length mismatch"
        );
    }

    #[test]
    fn test_historical_append_cfs_expected_members() {
        let expected = &[CF_CELLS];
        for cf in expected {
            assert!(
                CkbadgerStore::is_historical_append_cf(cf),
                "{cf} should be in HISTORICAL_APPEND_CFS"
            );
        }
        assert_eq!(
            CkbadgerStore::HISTORICAL_APPEND_CFS.len(),
            expected.len(),
            "HISTORICAL_APPEND_CFS length mismatch"
        );
    }

    #[test]
    fn test_set_bulk_sync_compaction_options_does_not_panic() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        // Should not panic on a freshly opened store
        store.set_bulk_sync_compaction_options();
    }

    #[test]
    fn test_restore_normal_compaction_options_does_not_panic() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        // Should not panic on a freshly opened store
        store.restore_normal_compaction_options();
    }

    #[test]
    fn test_bulk_sync_then_restore_compaction_round_trip() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        // Set bulk sync options, then restore — should work without panics
        store.set_bulk_sync_compaction_options();
        store.restore_normal_compaction_options();
        // Verify DB is still operational after option changes
        let cf = store.cf_sync_meta();
        store.put_cf(cf, b"test", b"value").unwrap();
        let val = store.get_cf(cf, b"test").unwrap();
        assert_eq!(val.as_deref(), Some(b"value".as_slice()));
    }

    #[test]
    fn test_set_bulk_compaction_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        assert!(!store.is_bulk_sync_mode());

        // First call sets mode
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());

        // Second call is a no-op (idempotent)
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());
    }

    #[test]
    fn test_restore_normal_compaction_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        // Restore on fresh store (already normal) is a no-op
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());

        // Enter bulk, then restore
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());

        // Second restore is a no-op
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());
    }

    #[test]
    fn test_compaction_mode_tracks_bulk_sync_mode_flag() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        // Initial state
        assert!(!store.is_bulk_sync_mode());

        // Enter bulk → flag true
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());

        // Restore normal → flag false
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());

        // Re-enter bulk → flag true again
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());
    }

    #[test]
    fn test_log_config_does_not_panic() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store.log_config();
    }

    #[test]
    fn test_memory_profile_32gb_primary_reproduces_original_constants() {
        let profile = MemoryProfile::compute(32 * GB, 24, false);
        assert_eq!(profile.rocksdb_budget_bytes, 16 * GB as usize);
        assert_eq!(profile.wbm_normal_bytes, 8 * GB as usize);
        assert_eq!(profile.block_cache_normal_bytes, 8 * GB as usize);
        assert_eq!(profile.write_buffer_mega_bytes, 256 * MB as usize);
        assert_eq!(profile.write_buffer_high_bytes, 128 * MB as usize);
        assert_eq!(profile.write_buffer_low_bytes, 32 * MB as usize);
        assert_eq!(profile.max_background_jobs, 24);
        assert_eq!(profile.severe_compaction_pending_bytes, 8 * GB);
        assert_eq!(profile.moderate_compaction_pending_bytes, 4 * GB);
        assert_eq!(profile.severe_immutable_memtables, 60);
        assert_eq!(profile.moderate_immutable_memtables, 30);
        assert_eq!(profile.drain_pending_bytes_threshold, 2 * GB);
    }

    #[test]
    fn test_memory_profile_bulk_pending_thresholds_are_higher() {
        let profile = MemoryProfile::compute(96 * GB, 24, false);
        assert!(
            profile.severe_compaction_pending_bytes_bulk > profile.severe_compaction_pending_bytes,
            "bulk severe pending threshold ({}) should exceed normal ({})",
            profile.severe_compaction_pending_bytes_bulk,
            profile.severe_compaction_pending_bytes,
        );
        assert!(
            profile.moderate_compaction_pending_bytes_bulk
                > profile.moderate_compaction_pending_bytes,
            "bulk moderate pending threshold ({}) should exceed normal ({})",
            profile.moderate_compaction_pending_bytes_bulk,
            profile.moderate_compaction_pending_bytes,
        );
    }

    #[test]
    fn test_memory_profile_secondary_cap() {
        let profile = MemoryProfile::compute(128 * GB, 24, true);
        assert_eq!(profile.rocksdb_budget_bytes, 16 * GB as usize);
    }

    #[test]
    fn test_secondary_store_path_uses_owner_suffix() {
        assert_eq!(
            secondary_store_path("/tmp/domain", SecondaryStoreOwner::Api),
            PathBuf::from("/tmp/domain-api-secondary")
        );
        assert_eq!(
            secondary_store_path("/tmp/domain", SecondaryStoreOwner::Tui),
            PathBuf::from("/tmp/domain-tui-secondary")
        );
        assert_eq!(
            secondary_store_path("/tmp/domain", SecondaryStoreOwner::Cli),
            PathBuf::from("/tmp/domain-cli-secondary")
        );
    }

    #[test]
    fn test_memory_profile_accessor() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let profile = store.memory_profile();
        assert!(!profile.is_secondary);
        assert!(profile.system_ram_bytes > 0);
        assert!(profile.cpu_count > 0);
    }

    #[test]
    fn test_flush_all_memtables_makes_no_wal_data_durable() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        // Write data without WAL (simulating bulk sync commit_no_wal)
        let cf = store.cf_sync_meta();
        let mut batch = WriteBatch::default();
        batch.put_cf(cf, b"nowal-key", b"nowal-value");
        store.write_batch_no_wal(batch).unwrap();

        // Flush to make the no-WAL data durable (persisted to SST)
        store.flush_all_memtables().unwrap();

        // Verify data is readable after flush
        let val = store.get_cf(cf, b"nowal-key").unwrap();
        assert_eq!(val.as_deref(), Some(b"nowal-value".as_slice()));
    }

    #[test]
    fn test_flush_all_memtables_on_empty_store() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        // Should succeed even with no data to flush
        store.flush_all_memtables().unwrap();
    }

    #[test]
    fn test_restore_normal_compaction_flushes_before_wbm_reduction() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        // Enter bulk sync mode
        store.set_bulk_sync_compaction_options();

        // Write data without WAL (as bulk sync does)
        let cf = store.cf_sync_meta();
        let mut batch = WriteBatch::default();
        batch.put_cf(cf, b"bulk-key", b"bulk-value");
        store.write_batch_no_wal(batch).unwrap();

        // Transition to normal mode — this should flush memtables first
        store.restore_normal_compaction_options();

        // Data written during bulk sync must survive the transition
        let val = store.get_cf(cf, b"bulk-key").unwrap();
        assert_eq!(val.as_deref(), Some(b"bulk-value".as_slice()));
    }

    #[test]
    fn test_cf_write_policy_marks_cells_as_append_only() {
        assert_eq!(cf_write_policy(CF_CELLS), CfWritePolicy::AppendOnly);
    }

    #[test]
    fn test_cf_write_policy_marks_live_cells_as_final_snapshot() {
        assert_eq!(cf_write_policy(CF_LIVE_CELLS), CfWritePolicy::FinalSnapshot);
    }

    #[test]
    fn test_cf_write_policy_marks_stats_chain_as_sealed_aggregate() {
        assert_eq!(
            cf_write_policy(CF_STATS_CHAIN),
            CfWritePolicy::SealedAggregate
        );
    }

    #[test]
    fn test_cf_write_policy_never_marks_domain_cell_markers_as_append_only() {
        assert_ne!(cf_write_policy(CF_LIVE_CELLS), CfWritePolicy::AppendOnly);
        assert_ne!(
            cf_write_policy(CF_CONSUMED_CELLS),
            CfWritePolicy::FinalSnapshot
        );
    }

    #[test]
    fn test_cf_write_policy_handles_all_known_column_families() {
        for &cf_name in ALL_CFS {
            cf_write_policy(cf_name);
        }
    }

    #[test]
    #[should_panic(expected = "unknown column family write policy")]
    fn test_cf_write_policy_panics_on_unknown_column_family() {
        cf_write_policy("missing_cf");
    }
}
