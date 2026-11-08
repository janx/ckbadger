//! Core RocksDB store.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use rocksdb::{
    ColumnFamily, ColumnFamilyDescriptor, DBCompactionStyle, DBCompressionType, IteratorMode,
    Options, UniversalCompactOptions, WriteBatch, WriteBufferManager, DB,
};
use serde::{Deserialize, Serialize};

use crate::keys;
use crate::read_view;
use crate::types::MemoryStats;

/// Type alias for RocksDB iterator items to avoid complex type lint.
pub type KvResult = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>;

const GB: u64 = 1024 * 1024 * 1024;
const MB: u64 = 1024 * 1024;

/// Process-wide RocksDB `Cache` + `WriteBufferManager`, shared by every store this
/// process opens.
///
/// RocksDB budgets are per-PROCESS, not per-DB: both objects are designed to be
/// handed to every DB a process opens. Minting a fresh pair per open gave each
/// store its own full budget, so an indexer (domain + append-only) provisioned
/// `2 x budget` and N co-resident networks provisioned N x the host — the
/// over-commit that got bulk sync OOM-killed. Sharing one pair makes `budget` mean
/// what it says, so `MemoryProfile`'s arithmetic holds however many stores a
/// process opens.
///
/// Sized ONCE, from the first open's profile; later opens reuse that pair. This is
/// correct because a process's opens are homogeneous in budget by construction: the
/// indexer opens only primaries, and each single-network stack (indexer or API)
/// opens its domain + append-only stores with the same runtime config. The one case
/// where opens differ is a read-only multi-network monitor (`ckbadger tui` /
/// `ckbadger status`), which opens several networks' domain secondaries in one
/// process — possibly with different per-network `[store].memory_budget_gb`. They
/// share the first network's cap, which is harmless: a secondary block cache is a
/// lazily-filled LRU capacity, so one shared cap across N read-only monitors uses
/// less RAM, not more. So the invariant is "homogeneous per single-network stack",
/// which every writer process satisfies exactly.
static SHARED_BUDGET: OnceLock<(rocksdb::Cache, WriteBufferManager)> = OnceLock::new();

/// Highest LSM level counted by `sst_files_total`. RocksDB's default
/// `num_levels` is 7 (L0..L6); a level that does not exist simply reports no
/// value.
const MAX_LEVEL_FOR_FILE_COUNT: usize = 6;

/// How long a computed SST file count stays fresh. Bulk sync samples
/// `memory_stats()` every 3 s; the file count changes on the timescale of
/// compactions, so it is recomputed at most this often.
const SST_FILE_COUNT_MAX_AGE: Duration = Duration::from_secs(30);

/// Serde default for `StoreRuntimeConfig::network_count`, applied when the field
/// is absent. `NonZeroUsize` has no `Default`, so this must be named explicitly.
fn default_network_count() -> NonZeroUsize {
    NonZeroUsize::MIN
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreRuntimeConfig {
    pub memory_budget_gb: Option<u64>,
    pub direct_io_reads: bool,
    /// When true, use VectorRep memtable (O(1) insert) instead of skiplist
    /// (O(log n) insert). Safe only when there is a single writer thread
    /// and no concurrent memtable readers. Set at DB open time and persists
    /// for the lifetime of the process (cannot be changed at runtime).
    pub vector_memtable: bool,
    /// Number of network stacks co-resident on this host (see
    /// `ckbadger_config::co_resident_network_count`). The detected host RAM is
    /// divided by this so N networks' budgets sum to one network's share instead
    /// of N times it, which otherwise over-commits the host and gets bulk sync
    /// OOM-killed. 1 yields the full detected RAM.
    ///
    /// At very large N the divided budget can hit `compute`'s 2 GB floor, so the
    /// sum would exceed the intended share; realistic N (2-4) is unaffected.
    #[serde(default = "default_network_count")]
    pub network_count: NonZeroUsize,
}

impl Default for StoreRuntimeConfig {
    fn default() -> Self {
        Self {
            memory_budget_gb: None,
            direct_io_reads: true,
            vector_memtable: false,
            network_count: NonZeroUsize::MIN,
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
    Supervisor,
}

impl SecondaryStoreOwner {
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Api => "api-secondary",
            Self::Tui => "tui-secondary",
            Self::Cli => "cli-secondary",
            Self::Supervisor => "supervisor-secondary",
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

pub fn known_domain_secondary_store_paths<P: AsRef<Path>>(primary_path: P) -> [PathBuf; 4] {
    let primary_path = primary_path.as_ref();
    [
        secondary_store_path(primary_path, SecondaryStoreOwner::Api),
        secondary_store_path(primary_path, SecondaryStoreOwner::Tui),
        secondary_store_path(primary_path, SecondaryStoreOwner::Cli),
        secondary_store_path(primary_path, SecondaryStoreOwner::Supervisor),
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

/// Thread counts of the process-wide RocksDB `Env` pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundThreads {
    /// HIGH-priority pool: memtable flushes of every store in the process.
    pub flush: i32,
    /// LOW-priority pool: compactions.
    pub compaction: i32,
}

/// Splits a background-job count into the two `Env` pools. The bulk
/// controller moves `jobs`; the profile's `max_background_jobs` gives the
/// open-time split.
///
/// - Flush: `max(2, jobs / 4)`. Base used `max(1, jobs / 4)`, which at the
///   controller's floor (6-7 jobs on a 24-thread host) left ONE flush thread
///   shared by the domain store's atomic multi-CF flushes and the `cells`
///   flushes. The floor of 2 keeps the two stores from sharing one thread.
/// - Compaction: `max(1, jobs - max(1, jobs / 4))`, base's count.
///
/// From 8 jobs up the split is base's; below, the only difference is 2 flush
/// threads instead of 1. A fixed 6-thread flush pool (0-d) was dropped in
/// 0-e: extra concurrent background threads compete with the bulk build.
fn background_thread_split(jobs: i32) -> BackgroundThreads {
    BackgroundThreads {
        flush: (jobs / 4).max(2),
        compaction: (jobs - (jobs / 4).max(1)).max(1),
    }
}

/// Auto-detected system memory profile that drives RocksDB memory parameters.
#[derive(Debug, Clone)]
pub struct MemoryProfile {
    pub system_ram_bytes: u64,
    /// Physical cores (for I/O-bound work: fetch threads).
    pub cpu_count: usize,
    /// Logical cores including SMT/HT (for RocksDB bg_jobs, subcompactions).
    pub logical_cpu_count: usize,
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
    /// Open-time size of the process-wide RocksDB HIGH-priority (flush) pool,
    /// shared by every store the process opens:
    /// `background_thread_split(max_background_jobs).flush`, i.e.
    /// `max(2, max_background_jobs / 4)`. The bulk controller then moves it
    /// with `set_max_background_jobs`, never below 2.
    pub flush_threads: i32,
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
        let (ram, physical, logical) = detect_system_resources(runtime_config);
        Self::compute(ram, physical, logical, false)
    }

    pub fn for_secondary() -> Self {
        Self::for_secondary_with_config(StoreRuntimeConfig::default())
    }

    pub fn for_secondary_with_config(runtime_config: StoreRuntimeConfig) -> Self {
        let (ram, physical, logical) = detect_system_resources(runtime_config);
        Self::compute(ram, physical, logical, true)
    }

    /// Pure computation from fixed inputs for deterministic unit testing.
    pub fn compute(
        system_ram_bytes: u64,
        cpu_count: usize,
        logical_cpu_count: usize,
        is_secondary: bool,
    ) -> Self {
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

        // RocksDB compaction benefits from SMT — use logical cores.
        let logical = logical_cpu_count.max(1);
        let max_background_jobs = logical.clamp(4, 32) as i32;
        let flush_threads = background_thread_split(max_background_jobs).flush;
        let max_subcompactions = (logical / 4).clamp(2, 8) as u32;

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
            logical_cpu_count,
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
            flush_threads,
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

/// Detect total physical memory in bytes using POSIX sysconf (cross-platform).
fn read_system_memory() -> Option<u64> {
    unsafe {
        let pages = libc::sysconf(libc::_SC_PHYS_PAGES);
        let page_size = libc::sysconf(libc::_SC_PAGESIZE);
        if pages > 0 && page_size > 0 {
            Some(pages as u64 * page_size as u64)
        } else {
            None
        }
    }
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

/// RAM budget this store may size itself against.
///
/// Precedence: an explicit `memory_budget_gb` override wins and is used as-is —
/// never divided, because it is already the operator's per-network value.
/// Otherwise the detected host RAM is split across the co-resident networks.
/// `network_count == 1` yields the full detected RAM, so the single-network path
/// is this same expression rather than a separate branch.
///
/// `detect` is lazy so an explicit override skips detection entirely.
fn effective_ram_bytes(
    memory_budget_gb: Option<u64>,
    network_count: NonZeroUsize,
    detect: impl FnOnce() -> u64,
) -> u64 {
    match memory_budget_gb {
        Some(gb) if gb > 0 => gb * GB,
        _ => detect() / network_count.get() as u64,
    }
}

/// Returns (ram_bytes, physical_cores, logical_cores).
fn detect_system_resources(runtime_config: StoreRuntimeConfig) -> (u64, usize, usize) {
    match runtime_config.memory_budget_gb {
        Some(gb) if gb > 0 => info!(gb, "Using explicit RocksDB memory_budget_gb override"),
        Some(_) => warn!("Ignoring zero memory_budget_gb override, falling back to detection"),
        None => {}
    }

    let ram = effective_ram_bytes(
        runtime_config.memory_budget_gb,
        runtime_config.network_count,
        || {
            // Logged from inside the detection closure on purpose: it runs only on
            // the non-override path, which is exactly the path that divides. An
            // explicit memory_budget_gb short-circuits before this and is never
            // divided, so it must not claim division happened. Gating on the override
            // here instead would restate that precedence where it could drift from
            // `effective_ram_bytes`.
            if runtime_config.network_count > NonZeroUsize::MIN {
                info!(
                    network_count = runtime_config.network_count.get(),
                    "Dividing detected RAM across co-resident network stacks"
                );
            }
            read_system_memory()
                .or_else(read_cgroup_memory_limit)
                .unwrap_or_else(|| {
                    warn!("Could not detect system RAM, defaulting to 32 GB");
                    32 * GB
                })
        },
    );

    let physical = num_cpus::get_physical().max(1);
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(physical);

    (ram, physical, logical)
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
pub const CF_MNFT_DATA: &str = "mnft_data";
pub const CF_MNFT_BY_COLLECTION: &str = "mnft_by_collection";
pub const CF_IDENTITY_DATA: &str = "identity_data";
pub const CF_IDENTITY_BY_COLLECTION: &str = "identity_by_collection";
pub const CF_STATS_IDENTITY: &str = "stats_identity";
pub const CF_STATS_CHAIN: &str = "stats_chain";
pub const CF_STATS_DAO: &str = "stats_dao";
pub const CF_STATS_HODL: &str = "stats_hodl";
/// The HODL wave tracker's live capacity per cell creation day (B-4): key the
/// UTC+8 day `YYYYMMDD` (8 ASCII bytes), value the live capacity in shannons
/// (i128, 16 bytes little-endian, always > 0: a day whose cells are all
/// consumed has no row). The tracker's scalars are the `sync_meta` row
/// `hodl_cursor`.
pub const CF_HODL_CAPACITY_BY_DATE: &str = "hodl_capacity_by_date";
/// The cell-distribution tracker's address cohort accumulator (B-4): key the
/// cohort month `YYYY-MM` (7 ASCII bytes) of the addresses' first-seen block,
/// value `(used_capacity, balance)` in shannons (two i128, 32 bytes
/// little-endian, both >= 0). The tracker's scalars are the `sync_meta` row
/// `cell_dist_cursor`.
pub const CF_CELL_DIST_COHORT_BY_MONTH: &str = "cell_dist_cohort_by_month";
pub const CF_STATS_SCRIPT: &str = "stats_script";
pub const CF_STATS_TOKEN: &str = "stats_token";
pub const CF_STATS_SPORE: &str = "stats_spore";
pub const CF_STATS_MNFT: &str = "stats_mnft";
pub const CF_SCRIPT_INFO: &str = "script_info";
pub const CF_SCRIPT_VERSIONS: &str = "script_versions";
pub const CF_SCRIPT_VERSIONS_BY_LABEL: &str = "script_versions_by_label";
pub const CF_SCRIPT_FAMILIES: &str = "script_families";
pub const CF_SCRIPT_VERSIONS_BY_FAMILY: &str = "script_versions_by_family";
pub const CF_SCRIPT_REFERENCE_INFO: &str = "script_reference_info";
pub const CF_SCRIPT_REFERENCE_TO_VERSION: &str = "script_reference_to_version";
pub const CF_SCRIPT_FAMILY_BY_NAME: &str = "script_family_by_name";
pub const CF_SYNC_META: &str = "sync_meta";
pub const CF_SPORE_BY_CLUSTER: &str = "spore_by_cluster";
pub const CF_CELL_BY_LOCK_CODE: &str = "cell_by_lock_code";
pub const CF_CELL_BY_TYPE_CODE: &str = "cell_by_type_code";
pub const CF_TOKEN_TRANSFERS: &str = "token_transfers";
pub const CF_TX_ACTIONS: &str = "activities";
/// Global activity bucket index: bucket(1) + block_num_desc(8) + tx_idx_desc(4)
/// -> empty. One row per non-cellbase `CF_TX_ACTIONS` row.
pub const CF_TX_ACTIONS_BY_BUCKET: &str = "tx_actions_by_bucket";
/// Collection activity per UTC+8 day: collection_id(32, padded) + day
/// YYYYMMDD (u32 BE) -> `CollectionActivityDailyCounts`. One row per
/// (collection, day) with a mint, transfer or burn row.
pub const CF_COLLECTION_ACTIVITY_DAILY: &str = "collection_activity_daily";
/// Derived aggregate (indexer background derivation, never bulk-written):
/// rows over the live NervosDAO cells — one global row, one per phase-0
/// depositor lock, one per phase-0 deposit epoch and one per phase-1
/// (deposit epoch, request epoch). See `derive_ops.rs`.
pub const CF_DAO_LIVE_AGG: &str = "dao_live_agg";
/// Derived aggregate: `object_collection_activities` row count per padded
/// collection id. See `derive_ops.rs`.
pub const CF_OBJECT_COLLECTION_ACTIVITY_COUNT: &str = "object_collection_activity_count";
/// Derived aggregate: live `.bit Cell` items per `.bit` account id:
/// account_id(20) + bit cell identity id(32). See `derive_ops.rs`.
pub const CF_BIT_CELL_BY_ACCOUNT: &str = "bit_cell_by_account";
pub const CF_CLUSTER_AGG: &str = "cluster_agg";
pub const CF_MNFT_COLLECTION_AGG: &str = "mnft_collection_agg";
pub const CF_OBJECT_COLLECTION_ACTIVITIES: &str = "object_collection_activities";
pub const CF_IDENTITY_AGG: &str = "identity_agg";
pub const CF_IDENTITY_COLLECTION_ACTIVITIES: &str = "identity_collection_activities";
pub const CF_PENDING_PROPOSALS: &str = "pending_proposals";
pub const CF_FIBER_CHANNELS: &str = "fiber_channels";
pub const CF_FIBER_CHANNEL_BY_COMMITMENT: &str = "fiber_channel_by_commitment";
pub const CF_ADDR_FIBER_CHANNELS: &str = "addr_fiber_channels";
pub const CF_DOB_DECODED: &str = "dob_decoded";
/// A transaction's lazy script-group cycles (`TxGroupCyclesRecord`), keyed by
/// its canonical slot (`block ‖ tx_idx`): written by the on-demand cycles
/// worker after the block committed, removed with the block on rollback.
pub const CF_TX_GROUP_CYCLES: &str = "tx_group_cycles";
/// Object outpoint index: `kind(1) + outpoint(34)` → the object id
/// (`keys::object_outpoint_kind`; C-4b, from `stats_spore` / `stats_mnft`).
pub const CF_OBJECT_OUTPOINTS: &str = "object_outpoints";
/// Object outpoints by id: `kind(1) + id + outpoint(34)` → empty, or the
/// identity's `AssetAction` byte (C-4b, from `stats_spore` / `stats_mnft`).
pub const CF_OBJECT_OUTPOINT_BY_ID: &str = "object_outpoint_by_id";
/// The reorg-event history, `reorg:<ms>:<uuid>` → `ReorgEvent` (C-4b, from
/// `sync_meta`): the record of each fork switch and deep-fork detection.
pub const CF_REORG_EVENTS: &str = "reorg_events";
pub const CF_LOCK_SCRIPTS: &str = "lock_scripts";
/// Address-tx index for parties a protocol named by a 20-byte lock-hash prefix.
pub const CF_ADDR_TXS_BY_PREFIX: &str = "addr_txs_by_prefix";
/// Per-prefix participation counters for protocol-named parties holding no cell.
pub const CF_ADDR_PREFIX_STATS: &str = "addr_prefix_stats";
/// `.cell` names held by a 20-byte owner prefix: owner_hash20 + name_id -> empty.
pub const CF_DOTCELL_NAME_BY_OWNER: &str = "dotcell_name_by_owner";
/// One row per `.cell` namespace: the uniqueness ring's root cell.
pub const CF_DOTCELL_RING: &str = "dotcell_ring";

// Network crawler store column families (mutable / domain-like — NOT append-only).
// These live in the standalone "network" store class, not in domain or append-only.
pub const CF_NET_NODES: &str = "net_nodes";
pub const CF_NET_STATS: &str = "net_stats";
pub const CF_NET_CRAWL: &str = "net_crawl";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfWritePolicy {
    AppendOnly,
    FinalSnapshot,
    SealedAggregate,
    BulkDisabled,
}
/// Where a column family lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfStore {
    Domain,
    AppendOnly,
    Network,
}

/// Live write-buffer tier: `MemoryProfile::write_buffer_{mega,high,low}_bytes`
/// with 4 / 4 / 2 buffers live and 12 / 8 / 6 during bulk sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferTier {
    Mega,
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfCompression {
    Lz4,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfCompaction {
    /// Leveled with dynamic level bytes.
    Leveled,
    /// Universal (size ratio 10, size amplification 100 %): append-heavy history.
    Universal,
}

/// How a rollback restores a column family. Every CF has exactly one
/// (`docs/prompts/REORG_HANDLING.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackClass {
    /// Every live write records the key's value at the end of the previous
    /// block in the block's journal; rollback writes those values back.
    Journal,
    /// The key starts with the block number: rollback deletes the block's key
    /// range, nothing is journaled.
    RangeByBlock,
    /// Never rolled back: the append-only `cells` payloads (orphaned payloads
    /// are unreachable once their markers are gone), the network store,
    /// `reorg_events` (the history of reorgs, written by reorg handling: a
    /// rollback is itself recorded there, never undone), and
    /// `pending_proposals` (the proposal cache writes it out of band from
    /// mempool snapshots, with no block context; a committed proposal is
    /// removed after its batch commits and an expired one by the cache's
    /// expiry sweep, so a proposal of an orphaned block stays until it is
    /// committed or expires).
    Never,
}

/// One row of the column-family table. Every CF list and every per-CF option
/// in this module is a projection of [`CF_SPECS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CfSpec {
    pub name: &'static str,
    pub store: CfStore,
    pub write_policy: CfWritePolicy,
    pub buffer_tier: BufferTier,
    /// Bulk sync gives the CF `MemoryProfile::write_buffer_hot_cf_bytes`.
    pub bulk_hot_memtable: bool,
    pub compression: CfCompression,
    pub compaction: CfCompaction,
    pub rollback_class: RollbackClass,
}

const HOT: bool = true;
const COLD: bool = false;

#[allow(clippy::too_many_arguments)]
const fn cf(
    name: &'static str,
    store: CfStore,
    write_policy: CfWritePolicy,
    buffer_tier: BufferTier,
    bulk_hot_memtable: bool,
    compression: CfCompression,
    compaction: CfCompaction,
    rollback_class: RollbackClass,
) -> CfSpec {
    CfSpec {
        name,
        store,
        write_policy,
        buffer_tier,
        bulk_hot_memtable,
        compression,
        compaction,
        rollback_class,
    }
}

/// Every column family of the three store classes: chain stores first (`cells`,
/// then the domain store in open order), then the network store.
///
/// `RangeByBlock` is only for CFs whose key starts with the block number;
/// `cells`, `pending_proposals` and `reorg_events` are `Never`, every other domain CF
/// (`dob_decoded` included) `Journal`. The network store is not chain data and
/// is never rolled back.
pub const CF_SPECS: &[CfSpec] = cf_table::CF_SPECS;

mod cf_table {
    use super::BufferTier::{High, Low, Mega};
    use super::CfCompaction::{Leveled, Universal};
    use super::CfCompression::Lz4;
    use super::CfStore::{AppendOnly as AppendStore, Domain, Network};
    use super::CfWritePolicy::{AppendOnly, BulkDisabled, FinalSnapshot, SealedAggregate};
    use super::RollbackClass::{Journal, Never, RangeByBlock};
    use super::*;

    #[rustfmt::skip]
    pub(super) const CF_SPECS: &[CfSpec] = &[
        //  name                                    store        write policy     tier  bulk  comp. compaction rollback
        cf(CF_CELLS,                                  AppendStore, AppendOnly,      Mega, HOT,  Lz4,  Universal, Never),
        cf(CF_LIVE_CELLS,                             Domain,      FinalSnapshot,   Mega, HOT,  Lz4,  Leveled,   Journal),
        cf(CF_CONSUMED_CELLS,                         Domain,      AppendOnly,      Mega, HOT,  Lz4,  Leveled,   Journal),
        cf(CF_REORG_UNDO_LOG_BY_BLOCK,                Domain,      BulkDisabled,    Mega, COLD, Lz4,  Leveled,   RangeByBlock),
        cf(CF_BLOCK_HEADERS,                          Domain,      AppendOnly,      High, COLD, Lz4,  Leveled,   RangeByBlock),
        cf(CF_BLOCK_HASH_INDEX,                       Domain,      AppendOnly,      High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_CELL_BY_LOCK,                           Domain,      FinalSnapshot,   Mega, HOT,  Lz4,  Leveled,   Journal),
        cf(CF_CELL_BY_TYPE,                           Domain,      FinalSnapshot,   Mega, COLD, Lz4,  Leveled,   Journal),
        cf(CF_CELL_BY_LOCK_CODE,                      Domain,      FinalSnapshot,   Mega, COLD, Lz4,  Leveled,   Journal),
        cf(CF_CELL_BY_TYPE_CODE,                      Domain,      FinalSnapshot,   Mega, COLD, Lz4,  Leveled,   Journal),
        cf(CF_CELL_BY_DATA_HASH,                      Domain,      AppendOnly,      Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_TX_INDEX,                               Domain,      AppendOnly,      Mega, HOT,  Lz4,  Leveled,   RangeByBlock),
        cf(CF_TX_HASH_MAP,                            Domain,      AppendOnly,      Mega, COLD, Lz4,  Leveled,   Journal),
        cf(CF_ADDR_BALANCE,                           Domain,      FinalSnapshot,   Mega, COLD, Lz4,  Leveled,   Journal),
        cf(CF_ADDR_TXS,                               Domain,      AppendOnly,      Mega, COLD, Lz4,  Leveled,   Journal),
        cf(CF_DAO_DEPOSITS,                           Domain,      FinalSnapshot,   High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_DAO_BY_WITHDRAW_TX,                     Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_DAO_BY_BLOCK,                           Domain,      FinalSnapshot,   High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_DAO_BY_LOCK_BLOCK,                      Domain,      FinalSnapshot,   High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_DAO_BY_STATUS_BLOCK,                    Domain,      FinalSnapshot,   High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_TOKENS,                                 Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_TOKEN_HOLDERS,                          Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_TOKEN_HOLDERS_BY_BALANCE,               Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_ADDR_TOKENS_BY_BALANCE,                 Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SPORE_DATA,                             Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_MNFT_DATA,                              Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_MNFT_BY_COLLECTION,                     Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_IDENTITY_DATA,                          Domain,      FinalSnapshot,   High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_IDENTITY_BY_COLLECTION,                 Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_STATS_CHAIN,                            Domain,      SealedAggregate, High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_STATS_DAO,                              Domain,      SealedAggregate, Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_STATS_HODL,                             Domain,      SealedAggregate, Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_HODL_CAPACITY_BY_DATE,                  Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_CELL_DIST_COHORT_BY_MONTH,              Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_STATS_SCRIPT,                           Domain,      SealedAggregate, High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_STATS_TOKEN,                            Domain,      SealedAggregate, High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_STATS_SPORE,                            Domain,      SealedAggregate, High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_STATS_MNFT,                             Domain,      SealedAggregate, High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_STATS_IDENTITY,                         Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SCRIPT_INFO,                            Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SCRIPT_VERSIONS,                        Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SCRIPT_VERSIONS_BY_LABEL,               Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SCRIPT_FAMILIES,                        Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SCRIPT_VERSIONS_BY_FAMILY,              Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SCRIPT_REFERENCE_INFO,                  Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SCRIPT_REFERENCE_TO_VERSION,            Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SCRIPT_FAMILY_BY_NAME,                  Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SYNC_META,                              Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_SPORE_BY_CLUSTER,                       Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_TOKEN_TRANSFERS,                        Domain,      AppendOnly,      Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_TX_ACTIONS,                             Domain,      AppendOnly,      Mega, HOT,  Lz4,  Leveled,   RangeByBlock),
        cf(CF_CLUSTER_AGG,                            Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_MNFT_COLLECTION_AGG,                    Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_OBJECT_COLLECTION_ACTIVITIES,           Domain,      AppendOnly,      High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_IDENTITY_AGG,                           Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_IDENTITY_COLLECTION_ACTIVITIES,         Domain,      AppendOnly,      High, COLD, Lz4,  Leveled,   Journal),
        cf(CF_PENDING_PROPOSALS,                      Domain,      BulkDisabled,    Low,  COLD, Lz4,  Leveled,   Never),
        cf(CF_FIBER_CHANNELS,                         Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_FIBER_CHANNEL_BY_COMMITMENT,            Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_ADDR_FIBER_CHANNELS,                    Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_DOB_DECODED,                            Domain,      BulkDisabled,    Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_LOCK_SCRIPTS,                           Domain,      AppendOnly,      Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_ADDR_TXS_BY_PREFIX,                     Domain,      AppendOnly,      Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_ADDR_PREFIX_STATS,                      Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_DOTCELL_NAME_BY_OWNER,                  Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_DOTCELL_RING,                           Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_TX_ACTIONS_BY_BUCKET,                   Domain,      AppendOnly,      Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_COLLECTION_ACTIVITY_DAILY,              Domain,      SealedAggregate, Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_DAO_LIVE_AGG,                           Domain,      BulkDisabled,    Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_OBJECT_COLLECTION_ACTIVITY_COUNT,       Domain,      BulkDisabled,    Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_BIT_CELL_BY_ACCOUNT,                    Domain,      BulkDisabled,    Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_TX_GROUP_CYCLES,                        Domain,      BulkDisabled,    Low,  COLD, Lz4,  Leveled,   RangeByBlock),
        cf(CF_OBJECT_OUTPOINTS,                       Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_OBJECT_OUTPOINT_BY_ID,                  Domain,      FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Journal),
        cf(CF_REORG_EVENTS,                           Domain,      BulkDisabled,    Low,  COLD, Lz4,  Leveled,   Never),
        cf(CF_NET_NODES,                              Network,     FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Never),
        cf(CF_NET_STATS,                              Network,     FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Never),
        cf(CF_NET_CRAWL,                              Network,     FinalSnapshot,   Low,  COLD, Lz4,  Leveled,   Never),
    ];
}

/// The spec of `name`, if it names a column family.
pub fn cf_spec(name: &str) -> Option<&'static CfSpec> {
    CF_SPECS.iter().find(|spec| spec.name == name)
}

/// A writer an [`OUTSIDE_JOURNAL`] entry can name: the only one allowed to
/// write that entry's keys outside the journal. An unjournaled write carries
/// its writer — a [`crate::StoreBatch::outside_journal`] batch, or a direct
/// write inside [`CkbadgerStore::outside_journal_scope`] on the scope's
/// thread; any other unjournaled write carries none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutsideWriter {
    /// `label_import::import_all` (indexer startup, `ckbadger label-import`).
    LabelImport,
    /// The derive scheduler and the rollback reset of derived aggregates.
    Derive,
    /// `BatchWriter::record_deep_fork`, which withdraws
    /// `live_cell_summary:current`.
    DeepForkRecord,
    /// `BatchWriter::refresh_latest_dao_statistics`, the DAO tip summary.
    DaoTipSummary,
}

/// One range of `Journal`-class keys that is legitimately written outside
/// the block journal (an unjournaled commit), with its owner and why a
/// rollback need not restore it. [`OUTSIDE_JOURNAL`] lists every one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutsideJournal {
    pub cf: &'static str,
    /// Every key starting with this prefix (empty: the whole column family).
    pub key_prefix: &'static [u8],
    /// Who writes it outside the journal.
    pub owner: &'static str,
    /// Why a rollback need not restore it, and what that leaves.
    pub reason: &'static str,
    /// Never journaled, on any batch: a journaled batch stages it straight
    /// into its commit and the block overlay refuses it. Otherwise live
    /// batches journal it in their block's scope and only the owner writes it
    /// unjournaled.
    pub never_journaled: bool,
    /// The one writer admitted: every whole-column-family entry (Lane B
    /// carry-over 4c) and the two also-journaled `(cf, prefix)` entries
    /// (ruling 5) name it; `None` for a never-journaled `(cf, prefix)` entry,
    /// which names one narrow key range and admits any unjournaled writer.
    pub writer: Option<OutsideWriter>,
}

const fn outside(
    cf: &'static str,
    key_prefix: &'static [u8],
    owner: &'static str,
    reason: &'static str,
    never_journaled: bool,
) -> OutsideJournal {
    OutsideJournal {
        cf,
        key_prefix,
        owner,
        reason,
        never_journaled,
        writer: None,
    }
}

/// A `(cf, key prefix)` entry with one unjournaled writer: only `writer`
/// writes its keys outside the journal.
const fn outside_by(
    cf: &'static str,
    key_prefix: &'static [u8],
    writer: OutsideWriter,
    owner: &'static str,
    reason: &'static str,
    never_journaled: bool,
) -> OutsideJournal {
    OutsideJournal {
        cf,
        key_prefix,
        owner,
        reason,
        never_journaled,
        writer: Some(writer),
    }
}

/// A whole-column-family entry: only `writer` writes the CF outside the
/// journal.
const fn whole_cf(
    cf: &'static str,
    writer: OutsideWriter,
    owner: &'static str,
    reason: &'static str,
    never_journaled: bool,
) -> OutsideJournal {
    OutsideJournal {
        cf,
        key_prefix: b"",
        owner,
        reason,
        never_journaled,
        writer: Some(writer),
    }
}

const NEVER: bool = true;
const ALSO_JOURNALED: bool = false;
const LABEL_IMPORT: &str = "label import (indexer startup, `ckbadger label-import`)";
const LABELS_REASON: &str =
    "label metadata, not chain state; a rollback through a block journaled \
    before a label change restores that block's pre-change rows until the next import";
const DERIVE: &str = "background derivation (derive scheduler, rollback reset)";
const DERIVE_REASON: &str =
    "a derived aggregate is rebuilt from canonical rows; a rollback resets \
    an aggregate whose rows the journal does not own (the reset rule)";
const TELEMETRY: &str = "process telemetry, not chain state";

/// Every `Journal`-class key range written outside the block journal. With
/// journal discipline on (the indexer), an unjournaled commit outside bulk
/// sync that writes a `Journal`-class key not covered here is an error.
pub const OUTSIDE_JOURNAL: &[OutsideJournal] = &[
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::ENTITY_STATS_UNDO_CONTRACT,
        "live batch commit (prune), bulk completion",
        "describes which journal rows exist; the prune that advances it deletes rows no rollback \
         restores, so a rollback must never write an older floor back",
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::SYNC_RUNTIME,
        "indexer runtime: live batch, startup, deep-fork record, reorg, bulk completion",
        "run telemetry, bulk completion and deep-fork state belong to the running process",
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::RUNTIME_STATUS,
        "indexer heartbeat and shutdown record",
        TELEMETRY,
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::SYNC_PROGRESS,
        "indexer progress loop",
        TELEMETRY,
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::MEMORY_STATS,
        "indexer memory sampler",
        TELEMETRY,
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::BACKGROUND_TASKS,
        "background task progress mirror",
        TELEMETRY,
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::BULK_BUILD_SESSION_IN_PROGRESS,
        "bulk build engine",
        "marks an unfinished bulk session of a fresh store, which is rebuilt, never rolled back",
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::NETWORK_IDENTITY,
        "network guard (first start)",
        "the network the store is built for; constant",
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::GENESIS_BASELINE,
        "indexer at block 0",
        "derived from the genesis block; constant per network",
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::SECONDARY_EPOCH_REWARD,
        "indexer startup",
        "a consensus constant",
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::DERIVE_META_PREFIX,
        DERIVE,
        DERIVE_REASON,
        NEVER,
    ),
    outside(
        CF_SYNC_META,
        keys::sync_meta_keys::REORG_RECONCILE_PENDING,
        "reorg handling",
        "names the reorg event awaiting canonical replay",
        NEVER,
    ),
    outside_by(
        CF_SYNC_META,
        keys::sync_meta_keys::LIVE_CELL_SUMMARY_CURRENT,
        OutsideWriter::DeepForkRecord,
        "deep-fork record (withdraws it)",
        "a deep fork withdraws the summary until an operator resolves the fork; live batches \
         journal it per block",
        ALSO_JOURNALED,
    ),
    outside(
        CF_STATS_DAO,
        &[keys::STATS_PREFIX_DAO_LATEST_STATS],
        "DAO tip summary (refresh_latest_dao_statistics)",
        "a function of the sync tip, recomputed after every tip change",
        NEVER,
    ),
    outside(
        CF_STATS_DAO,
        &[keys::STATS_PREFIX_DAO_TOP_DEPOSITORS],
        "DAO tip summary (refresh_latest_dao_statistics)",
        "a function of the sync tip, recomputed after every tip change",
        NEVER,
    ),
    outside_by(
        CF_STATS_DAO,
        &[keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT],
        OutsideWriter::DaoTipSummary,
        "DAO tip summary (refresh_latest_dao_statistics)",
        "the open day's row is a function of the tip, recomputed after every tip change; a \
         sealed day's row is journaled at its sealer block",
        ALSO_JOURNALED,
    ),
    whole_cf(
        CF_DAO_LIVE_AGG,
        OutsideWriter::Derive,
        DERIVE,
        DERIVE_REASON,
        ALSO_JOURNALED,
    ),
    whole_cf(
        CF_OBJECT_COLLECTION_ACTIVITY_COUNT,
        OutsideWriter::Derive,
        DERIVE,
        DERIVE_REASON,
        ALSO_JOURNALED,
    ),
    whole_cf(
        CF_BIT_CELL_BY_ACCOUNT,
        OutsideWriter::Derive,
        DERIVE,
        DERIVE_REASON,
        ALSO_JOURNALED,
    ),
    whole_cf(
        CF_SCRIPT_VERSIONS,
        OutsideWriter::LabelImport,
        LABEL_IMPORT,
        LABELS_REASON,
        ALSO_JOURNALED,
    ),
    whole_cf(
        CF_SCRIPT_VERSIONS_BY_LABEL,
        OutsideWriter::LabelImport,
        LABEL_IMPORT,
        LABELS_REASON,
        ALSO_JOURNALED,
    ),
    whole_cf(
        CF_SCRIPT_VERSIONS_BY_FAMILY,
        OutsideWriter::LabelImport,
        LABEL_IMPORT,
        LABELS_REASON,
        ALSO_JOURNALED,
    ),
    whole_cf(
        CF_SCRIPT_FAMILIES,
        OutsideWriter::LabelImport,
        LABEL_IMPORT,
        LABELS_REASON,
        ALSO_JOURNALED,
    ),
    whole_cf(
        CF_SCRIPT_FAMILY_BY_NAME,
        OutsideWriter::LabelImport,
        LABEL_IMPORT,
        LABELS_REASON,
        ALSO_JOURNALED,
    ),
    whole_cf(
        CF_SCRIPT_INFO,
        OutsideWriter::LabelImport,
        LABEL_IMPORT,
        LABELS_REASON,
        ALSO_JOURNALED,
    ),
    whole_cf(
        CF_TOKENS,
        OutsideWriter::LabelImport,
        LABEL_IMPORT,
        LABELS_REASON,
        ALSO_JOURNALED,
    ),
];

/// The [`OUTSIDE_JOURNAL`] entry covering `(cf, key)`, if any, whoever the
/// writer (the block overlay's never-journaled test).
pub fn outside_journal_entry(cf: &str, key: &[u8]) -> Option<&'static OutsideJournal> {
    OUTSIDE_JOURNAL
        .iter()
        .find(|entry| entry.cf == cf && key.starts_with(entry.key_prefix))
}

/// The [`OUTSIDE_JOURNAL`] entry admitting an unjournaled write of `(cf, key)`
/// by `writer`, if any: an entry that names no writer admits any, one that
/// names a writer admits only that one.
pub fn outside_journal_entry_for(
    cf: &str,
    key: &[u8],
    writer: Option<OutsideWriter>,
) -> Option<&'static OutsideJournal> {
    OUTSIDE_JOURNAL.iter().find(|entry| {
        entry.cf == cf
            && key.starts_with(entry.key_prefix)
            && entry.writer.is_none_or(|named| writer == Some(named))
    })
}

/// An open [`CkbadgerStore::outside_journal_scope`]; closing it (drop) ends
/// the scope.
pub struct OutsideJournalScope<'a> {
    store: &'a CkbadgerStore,
}

impl Drop for OutsideJournalScope<'_> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.store.outside_writer_scope.lock() {
            *slot = None;
        }
    }
}

pub fn cf_write_policy(cf_name: &str) -> CfWritePolicy {
    match cf_spec(cf_name) {
        Some(spec) => spec.write_policy,
        None => panic!("unknown column family write policy: {}", cf_name),
    }
}

/// A projection of [`CF_SPECS`].
#[derive(Clone, Copy)]
enum CfSelect {
    /// The two chain stores (domain + append-only).
    Chain,
    Store(CfStore),
    Tier(BufferTier),
    BulkHot,
    Compaction(CfCompaction),
    /// Domain CFs bulk finalize phases 2-11 may write.
    FinalizeWritten,
    /// Domain CFs only the bulk batch loop writes.
    BatchLoopOnly,
}

impl CfSpec {
    const fn selected(&self, select: CfSelect) -> bool {
        match select {
            CfSelect::Chain => !matches!(self.store, CfStore::Network),
            CfSelect::Store(store) => self.store as u8 == store as u8,
            CfSelect::Tier(tier) => self.buffer_tier as u8 == tier as u8,
            CfSelect::BulkHot => self.bulk_hot_memtable,
            CfSelect::Compaction(compaction) => self.compaction as u8 == compaction as u8,
            CfSelect::FinalizeWritten => {
                matches!(self.store, CfStore::Domain) && self.finalize_writes()
            }
            CfSelect::BatchLoopOnly => {
                matches!(self.store, CfStore::Domain) && !self.finalize_writes()
            }
        }
    }

    /// The write policies the bounded bulk finalize materializer accepts.
    const fn finalize_writes(&self) -> bool {
        matches!(
            self.write_policy,
            CfWritePolicy::SealedAggregate | CfWritePolicy::FinalSnapshot
        )
    }
}

const fn cf_count(select: CfSelect) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < CF_SPECS.len() {
        if CF_SPECS[i].selected(select) {
            count += 1;
        }
        i += 1;
    }
    count
}

const fn cf_names<const N: usize>(select: CfSelect) -> [&'static str; N] {
    let mut names = [""; N];
    let mut count = 0;
    let mut i = 0;
    while i < CF_SPECS.len() {
        if CF_SPECS[i].selected(select) {
            names[count] = CF_SPECS[i].name;
            count += 1;
        }
        i += 1;
    }
    assert!(count == N);
    names
}

const ALL_CF_NAMES: [&str; cf_count(CfSelect::Chain)] = cf_names(CfSelect::Chain);
const DOMAIN_CF_NAMES: [&str; cf_count(CfSelect::Store(CfStore::Domain))] =
    cf_names(CfSelect::Store(CfStore::Domain));
const APPEND_CF_NAMES: [&str; cf_count(CfSelect::Store(CfStore::AppendOnly))] =
    cf_names(CfSelect::Store(CfStore::AppendOnly));
const NETWORK_CF_NAMES: [&str; cf_count(CfSelect::Store(CfStore::Network))] =
    cf_names(CfSelect::Store(CfStore::Network));
const MEGA_CF_NAMES: [&str; cf_count(CfSelect::Tier(BufferTier::Mega))] =
    cf_names(CfSelect::Tier(BufferTier::Mega));
const HIGH_CF_NAMES: [&str; cf_count(CfSelect::Tier(BufferTier::High))] =
    cf_names(CfSelect::Tier(BufferTier::High));
const BULK_HOT_CF_NAMES: [&str; cf_count(CfSelect::BulkHot)] = cf_names(CfSelect::BulkHot);
const UNIVERSAL_CF_NAMES: [&str; cf_count(CfSelect::Compaction(CfCompaction::Universal))] =
    cf_names(CfSelect::Compaction(CfCompaction::Universal));
const FINALIZE_WRITTEN_CF_NAMES: [&str; cf_count(CfSelect::FinalizeWritten)] =
    cf_names(CfSelect::FinalizeWritten);
const BATCH_LOOP_ONLY_CF_NAMES: [&str; cf_count(CfSelect::BatchLoopOnly)] =
    cf_names(CfSelect::BatchLoopOnly);

pub fn is_append_only_cf_name(cf_name: &str) -> bool {
    APPEND_CFS.contains(&cf_name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreClass {
    Domain,
    AppendOnly,
    TestUnified,
    /// Standalone whole-network crawler store (mutable, domain-like CFs).
    Network,
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

/// All chain-store column family names (domain + append-only), used during DB
/// open of a test-unified store.
pub const ALL_CFS: &[&str] = &ALL_CF_NAMES;

/// Column families of the domain mutable store.
pub const DOMAIN_CFS: &[&str] = &DOMAIN_CF_NAMES;

/// Domain CFs bulk finalize phases 2-11 may write, in [`DOMAIN_CFS`] order:
/// the `SealedAggregate` and `FinalSnapshot` write policies, the only two the
/// bounded finalize materializer accepts. Finalize phase 12 flushes exactly
/// these.
pub const FINALIZE_WRITTEN_CFS: &[&str] = &FINALIZE_WRITTEN_CF_NAMES;

/// Every other domain CF ([`DOMAIN_CFS`] minus [`FINALIZE_WRITTEN_CFS`]):
/// Class A history and bulk-disabled CFs, which only the bulk batch loop
/// writes. They hold their final bulk state once the flush channel has
/// committed its last batch, so finalize flushes them from then on, beside
/// phases 2-11.
pub const BATCH_LOOP_ONLY_CFS: &[&str] = &BATCH_LOOP_ONLY_CF_NAMES;

/// Column families of the append-only store (immutable, hash-keyed cell payloads).
pub const APPEND_CFS: &[&str] = &APPEND_CF_NAMES;

/// Column families of the standalone network crawler store (mutable, domain-like).
pub const NETWORK_CFS: &[&str] = &NETWORK_CF_NAMES;

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
    memtable_bytes: Option<u64>,
) -> (Option<u64>, Option<&'static str>) {
    let bytes =
        live_data_bytes.and_then(|live| memtable_bytes.and_then(|mem| live.checked_add(mem)));
    (bytes, bytes.map(|_| "live+mem"))
}

fn complete_observed_sum(prior: Option<u64>, value: Option<u64>) -> Option<u64> {
    prior.and_then(|prior| value.and_then(|value| prior.checked_add(value)))
}

fn manifest_bytes_at(db_path: &Path) -> Option<u64> {
    let current = match std::fs::read_to_string(db_path.join("CURRENT")) {
        Ok(current) => current,
        Err(error) => {
            warn!(path = %db_path.display(), %error, "failed to read RocksDB CURRENT");
            return None;
        }
    };
    let manifest_name = current.trim();
    if manifest_name.is_empty() {
        warn!(path = %db_path.display(), "RocksDB CURRENT has empty MANIFEST name");
        return None;
    }
    match std::fs::metadata(db_path.join(manifest_name)) {
        Ok(metadata) => Some(metadata.len()),
        Err(error) => {
            warn!(path = %db_path.display(), manifest = manifest_name, %error,
                "failed to stat RocksDB MANIFEST");
            None
        }
    }
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
    /// The path this handle opened. For a secondary this is the PRIMARY path,
    /// whose MANIFEST the secondary replays on open and on every catch-up.
    db_path: PathBuf,
    domain_path: PathBuf,
    append_path: PathBuf,
    /// Keep block cache alive for the lifetime of the store.
    /// Mutex because `set_capacity()` requires `&mut` (rare mode-transition calls only).
    block_cache: Mutex<rocksdb::Cache>,
    /// Serializes conditional cycles updates with canonical domain batch commits.
    domain_write_gate: Mutex<()>,
    /// Global memtable memory budget — controls WHEN flushes happen across all CFs.
    /// With many CFs, per-CF triggers cause unpredictable I/O storms. WBM replaces
    /// that with a single threshold: flush oldest CF when total memtable memory
    /// exceeds the budget, giving the indexer predictable flush behavior.
    write_buffer_manager: WriteBufferManager,
    bulk_sync_mode: AtomicBool,
    /// Set by the indexer: outside bulk sync, an unjournaled commit may write
    /// a `Journal`-class key only where [`OUTSIDE_JOURNAL`] covers it.
    journal_discipline: AtomicBool,
    /// The writer of direct (non-batch) unjournaled writes made on one thread
    /// inside [`Self::outside_journal_scope`].
    outside_writer_scope: Mutex<Option<(std::thread::ThreadId, OutsideWriter)>>,
    is_secondary: bool,
    memory_profile: MemoryProfile,
    runtime_config: StoreRuntimeConfig,
    /// `(measured_at, files)` for [`Self::sst_files_total_cached`].
    sst_file_count_cache: Mutex<Option<(Instant, u64)>>,
    /// Column family name by handle address, built on first use. Handles
    /// live in the DB's own map for the store's lifetime.
    cf_names_by_handle: std::sync::OnceLock<std::collections::HashMap<usize, &'static str>>,
    /// Test-only read-call counters, so a test can prove a hot path issues one
    /// batched read instead of N point reads. Never compiled into the binary.
    #[cfg(any(test, feature = "read-call-counters"))]
    read_calls: ReadCallCounters,
    /// Test-only: the temporary directory an `open_*_temp` store lives in,
    /// removed when the store drops. The LAST field: fields drop in
    /// declaration order and `db` is the first, so the database closes before
    /// its directory is removed, however many `Arc`s shared the store.
    #[cfg(any(test, feature = "test-store"))]
    test_dir: Option<tempfile::TempDir>,
}

#[cfg(any(test, feature = "read-call-counters"))]
#[derive(Default)]
struct ReadCallCounters {
    get_cf: std::sync::atomic::AtomicU64,
    multi_get_cf: std::sync::atomic::AtomicU64,
    put_cf: std::sync::atomic::AtomicU64,
    write_batch: std::sync::atomic::AtomicU64,
    /// Rows each column family's iterators yielded, by CF name, so a test can
    /// prove a read path never walks a CF (an activity walk, a cell walk).
    iterated_rows: Mutex<std::collections::HashMap<&'static str, u64>>,
    /// Point reads (`get_cf` and each `multi_get_cf` key), by CF name, so a
    /// test can prove a read path never looks a row up in a CF.
    point_reads: Mutex<std::collections::HashMap<&'static str, u64>>,
    /// `multi_get_cf` / `multi_get_cf_sorted` calls that read a CF, by CF
    /// name, so a test can prove a page resolves its pointers with one call.
    multi_gets: Mutex<std::collections::HashMap<&'static str, u64>>,
}

#[cfg(any(test, feature = "read-call-counters"))]
impl ReadCallCounters {
    fn count_point_read(&self, cf_name: &'static str) {
        *self
            .point_reads
            .lock()
            .expect("point-read counter poisoned")
            .entry(cf_name)
            .or_insert(0) += 1;
    }

    /// One multi-get call over `cf_names` (each CF counted once).
    fn count_multi_get(&self, cf_names: impl IntoIterator<Item = &'static str>) {
        let mut counts = self.multi_gets.lock().expect("multi-get counter poisoned");
        let mut seen = std::collections::HashSet::new();
        for cf_name in cf_names {
            if seen.insert(cf_name) {
                *counts.entry(cf_name).or_insert(0) += 1;
            }
        }
    }

    fn count_iterated_row(&self, cf_name: &'static str) {
        *self
            .iterated_rows
            .lock()
            .expect("iterated-row counter poisoned")
            .entry(cf_name)
            .or_insert(0) += 1;
    }
}

impl CkbadgerStore {
    fn cfs_for_class(store_class: StoreClass) -> &'static [&'static str] {
        match store_class {
            StoreClass::Domain => DOMAIN_CFS,
            StoreClass::AppendOnly => APPEND_CFS,
            StoreClass::TestUnified => ALL_CFS,
            StoreClass::Network => NETWORK_CFS,
        }
    }

    fn cf_allowed(store_class: StoreClass, name: &str) -> bool {
        match store_class {
            StoreClass::Domain => DOMAIN_CFS.contains(&name),
            StoreClass::AppendOnly => APPEND_CFS.contains(&name),
            StoreClass::TestUnified => true,
            StoreClass::Network => NETWORK_CFS.contains(&name),
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
            StoreClass::Network => {
                // Standalone store: no domain/append split. Both paths point here.
                (db_path.clone(), db_path.clone())
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
                        StoreClass::Network => "network",
                    },
                    db_path.display(),
                    allowed
                );
            }
        }

        let cf_descriptors =
            Self::cf_descriptors(allowed, &block_cache, &memory_profile, runtime_config);

        Self::size_flush_pool(&memory_profile)?;
        let db = DB::open_cf_descriptors(&opts, path, cf_descriptors)?;

        Ok(Self {
            db,
            store_class,
            db_path,
            domain_path,
            append_path,
            block_cache: Mutex::new(block_cache),
            domain_write_gate: Mutex::new(()),
            write_buffer_manager,
            bulk_sync_mode: AtomicBool::new(false),
            journal_discipline: AtomicBool::new(false),
            outside_writer_scope: Mutex::new(None),
            is_secondary: false,
            memory_profile,
            runtime_config,
            sst_file_count_cache: Mutex::new(None),
            cf_names_by_handle: std::sync::OnceLock::new(),
            #[cfg(any(test, feature = "read-call-counters"))]
            read_calls: ReadCallCounters::default(),
            #[cfg(any(test, feature = "test-store"))]
            test_dir: None,
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
            StoreClass::Network => {
                // Standalone store: no domain/append split. Both paths point here.
                (db_path.clone(), db_path.clone())
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
                        StoreClass::Network => "network",
                    },
                    db_path.display(),
                    allowed
                );
            }
        }
        // The same per-CF options as the primary: opened by name, every CF
        // got RocksDB's defaults instead (a private 32 MB block cache and no
        // bloom filter), so a point get read a data block in every sorted run.
        let cf_descriptors =
            Self::cf_descriptors(allowed, &block_cache, &memory_profile, runtime_config);
        let db = DB::open_cf_descriptors_as_secondary(
            &opts,
            primary_path,
            secondary_path,
            cf_descriptors,
        )?;

        Ok(Self {
            db,
            store_class,
            db_path,
            domain_path,
            append_path,
            block_cache: Mutex::new(block_cache),
            domain_write_gate: Mutex::new(()),
            write_buffer_manager,
            bulk_sync_mode: AtomicBool::new(false),
            journal_discipline: AtomicBool::new(false),
            outside_writer_scope: Mutex::new(None),
            is_secondary: true,
            memory_profile,
            runtime_config,
            sst_file_count_cache: Mutex::new(None),
            cf_names_by_handle: std::sync::OnceLock::new(),
            #[cfg(any(test, feature = "read-call-counters"))]
            read_calls: ReadCallCounters::default(),
            #[cfg(any(test, feature = "test-store"))]
            test_dir: None,
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

    /// Open the standalone network crawler store (read-write, primary) with the
    /// default (single-network, undivided) runtime config.
    ///
    /// Prefer [`Self::open_network_with_runtime`] anywhere a per-network config
    /// exists: RocksDB budgets are per-PROCESS and the shared cache /
    /// WriteBufferManager are pinned by whichever open lands first, so a
    /// `default()` open in a real deployment provisions from UNDIVIDED host RAM.
    pub fn open_network<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        Self::open_network_with_runtime(path, StoreRuntimeConfig::default())
    }

    /// Open the standalone network crawler store (read-write, primary) with an
    /// explicit runtime config — the crawler's only store open, so this is what
    /// keeps N co-resident crawlers from budgeting N x the host's RAM.
    pub fn open_network_with_runtime<P: AsRef<Path>>(
        path: P,
        runtime_config: StoreRuntimeConfig,
    ) -> anyhow::Result<Self> {
        Self::open_with_class(path, StoreClass::Network, runtime_config)
    }

    /// Open the network crawler store as a read-only secondary with the default
    /// (single-network, undivided) runtime config. See [`Self::open_network`] for
    /// why [`Self::open_network_secondary_with_runtime`] is preferred.
    pub fn open_network_secondary<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
    ) -> anyhow::Result<Self> {
        Self::open_network_secondary_with_runtime(
            primary_path,
            secondary_path,
            StoreRuntimeConfig::default(),
        )
    }

    /// Open the network crawler store as a read-only secondary with an explicit
    /// runtime config.
    pub fn open_network_secondary_with_runtime<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
        runtime_config: StoreRuntimeConfig,
    ) -> anyhow::Result<Self> {
        Self::open_secondary_with_class(
            primary_path,
            secondary_path,
            StoreClass::Network,
            runtime_config,
        )
    }

    /// Test helper: open a fresh network store (mirrors [`Self::open_test_unified`]).
    pub fn open_test_network<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        Self::open_network(path)
    }

    /// Test helper: a fresh unified store (every CF of every store class) in
    /// a new temporary directory that goes with the store: removed when the
    /// last handle drops, after the database closed.
    #[cfg(any(test, feature = "test-store"))]
    pub fn open_test_unified_temp() -> anyhow::Result<Self> {
        Self::open_in_test_dir(|path| Self::open_test_unified(path))
    }

    /// Test helper: [`Self::open_test_network`] in a temporary directory that
    /// goes with the store.
    #[cfg(any(test, feature = "test-store"))]
    pub fn open_test_network_temp() -> anyhow::Result<Self> {
        Self::open_in_test_dir(|path| Self::open_test_network(path))
    }

    /// Test helper: [`Self::open_domain`] in a temporary directory that goes
    /// with the store.
    #[cfg(any(test, feature = "test-store"))]
    pub fn open_domain_temp() -> anyhow::Result<Self> {
        Self::open_in_test_dir(|path| Self::open_domain(path))
    }

    /// Test helper: [`Self::open_append_only`] in a temporary directory that
    /// goes with the store.
    #[cfg(any(test, feature = "test-store"))]
    pub fn open_append_only_temp() -> anyhow::Result<Self> {
        Self::open_in_test_dir(|path| Self::open_append_only(path))
    }

    /// The temporary directory an `open_*_temp` store lives in; `None` for a
    /// store opened at a caller's path.
    #[cfg(any(test, feature = "test-store"))]
    pub fn test_dir(&self) -> Option<&Path> {
        self.test_dir.as_ref().map(tempfile::TempDir::path)
    }

    #[cfg(any(test, feature = "test-store"))]
    fn open_in_test_dir(open: impl FnOnce(&Path) -> anyhow::Result<Self>) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let mut store = open(dir.path())?;
        store.test_dir = Some(dir);
        Ok(store)
    }

    /// Catch up with primary instance writes (secondary only).
    ///
    /// Advancing a secondary's view is the only thing that can make two reads
    /// in one logical scope disagree, so it happens inside an exclusive
    /// [`read_view::CatchUpWindow`]: it waits for in-flight pinned read scopes
    /// and blocks new ones for its duration. Never call this from a thread
    /// holding a [`read_view::ReadViewGuard`] — that self-deadlocks.
    ///
    /// Use [`Self::catch_up_in_window`] instead when several stores must land
    /// in the same window.
    pub fn refresh(&self) -> anyhow::Result<()> {
        if !self.is_secondary {
            return Ok(());
        }
        let window = read_view::catch_up_window();
        self.catch_up_in_window(&window)
    }

    /// Advance this store's view inside an already-open catch-up window.
    ///
    /// Taking the window by reference is what makes "all secondaries advance
    /// together" checkable at compile time: cross-store reads (domain marker +
    /// append-only payload) cannot straddle two windows.
    pub fn catch_up_in_window(&self, _window: &read_view::CatchUpWindow) -> anyhow::Result<()> {
        if self.is_secondary {
            self.db.try_catch_up_with_primary()?;
        }
        Ok(())
    }

    /// Mega-tier CFs: the heaviest per-batch write volume (block 12M has
    /// ~1.31M txs → ~162MB per CF), so their memtables are the largest.
    const MEGA_WRITE_CFS: &'static [&'static str] = &MEGA_CF_NAMES;

    /// High-tier CFs: large write buffers.
    const HIGH_WRITE_CFS: &'static [&'static str] = &HIGH_CF_NAMES;

    /// Historical append-heavy CFs: append writes during sync and large range
    /// scans on reads. Universal compaction reduces cross-level rewrite
    /// amplification for this write pattern.
    const HISTORICAL_APPEND_CFS: &'static [&'static str] = &UNIVERSAL_CF_NAMES;

    fn spec(name: &str) -> &'static CfSpec {
        cf_spec(name).unwrap_or_else(|| panic!("unknown column family: {name}"))
    }

    fn is_mega_write_cf(name: &str) -> bool {
        Self::spec(name).buffer_tier == BufferTier::Mega
    }

    fn is_high_write_cf(name: &str) -> bool {
        Self::spec(name).buffer_tier == BufferTier::High
    }

    #[cfg(test)]
    fn is_historical_append_cf(name: &str) -> bool {
        Self::spec(name).compaction == CfCompaction::Universal
    }

    /// Per-CF write buffer for the live profile: `(max_write_buffer_number,
    /// write_buffer_size)`.
    ///
    /// The single source for both the open-time CF options and the live
    /// profile restored by [`Self::apply_normal_compaction_options`], so the
    /// two can never disagree about a CF's tier.
    ///
    /// `atomic_flush = 1` means ANY CF hitting its buffer switches memtables
    /// for the whole DB, so the smallest tier here sets the flush frequency of
    /// every CF the store opens — which is why the tier comes from the memory
    /// profile and is never hardcoded below it.
    fn live_cf_write_buffer(name: &str, profile: &MemoryProfile) -> (i32, usize) {
        if Self::is_mega_write_cf(name) {
            (4, profile.write_buffer_mega_bytes)
        } else if Self::is_high_write_cf(name) {
            (4, profile.write_buffer_high_bytes)
        } else {
            (2, profile.write_buffer_low_bytes)
        }
    }

    /// CFs whose bulk-sync memtable is `MemoryProfile::write_buffer_hot_cf_bytes`.
    const BULK_HOT_WRITE_CFS: &'static [&'static str] = &BULK_HOT_CF_NAMES;

    /// Per-CF option overrides applied by
    /// [`Self::set_bulk_sync_compaction_options`], as `(option, value)` pairs.
    ///
    /// The single source of a CF's bulk profile: the setter applies exactly
    /// this list, so the bulk tier of every CF can be pinned by a unit test
    /// from a `MemoryProfile` alone.
    fn bulk_cf_options(cf_name: &str, profile: &MemoryProfile) -> Vec<(&'static str, String)> {
        // More write buffers = more memtable headroom before flush stall.
        // With WBM controlling global budget, per-CF limits are a safety net.
        let max_write_buffer_number = if Self::is_mega_write_cf(cf_name) {
            12
        } else if Self::is_high_write_cf(cf_name) {
            8
        } else {
            6
        };
        let mut options = vec![
            ("level0_slowdown_writes_trigger", "96".to_string()),
            ("level0_stop_writes_trigger", "192".to_string()),
            (
                "max_write_buffer_number",
                max_write_buffer_number.to_string(),
            ),
            (
                "max_bytes_for_level_base",
                profile.bulk_max_bytes_for_level_base.to_string(),
            ),
            (
                "target_file_size_base",
                profile.bulk_target_file_size_base.to_string(),
            ),
        ];
        if Self::BULK_HOT_WRITE_CFS.contains(&cf_name) {
            options.push((
                "write_buffer_size",
                profile.write_buffer_hot_cf_bytes.to_string(),
            ));
        }
        options
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

        // Bound the MANIFEST. It is an append-only log of every version edit
        // (every flush and compaction writes one), and RocksDB only rolls it
        // when it exceeds this size — the 1 GB default never rolled, so the
        // 2026-09-22 flush storm grew it to 51 MB and a secondary spent 596 s
        // replaying it before serving a single read. 64 MB is the roll point;
        // the live file stays far below it once flush frequency is sane.
        opts.set_max_manifest_file_size(64 * MB as usize);

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
        // together.  Required during live sync (WAL enabled) to prevent
        // cross-CF data inconsistency on crash — without it, a crash can
        // leave live_cells deletes flushed to SST while consumed_cells puts
        // are lost in memtable, creating unrecoverable "cell black holes".
        //
        // Stays ON for the entire process lifetime (the rocksdb crate does not
        // expose SetDBOptions, so atomic_flush cannot be toggled at runtime).
        // This means background flushes during bulk sync coordinate every CF
        // the store opens, which is slower than independent per-CF flushes but ensures cross-CF
        // consistency on crash during live sync (WAL + atomic flush).
        opts.set_atomic_flush(true);

        // Unordered write: skip write-group leader serialization overhead.
        // Safe because the indexer is the sole writer process and the API
        // uses secondary (read-only) mode. Relaxes snapshot ordering but
        // maintains read-your-own-write consistency.
        //
        // Disabled when vector_memtable is set: unordered_write implies
        // allow_concurrent_memtable_write which is incompatible with VectorRep.
        if runtime_config.vector_memtable {
            opts.set_unordered_write(false);
            opts.set_allow_concurrent_memtable_write(false);
        } else {
            opts.set_unordered_write(true);
        }

        // Global WriteBufferManager: controls total memtable memory across all CFs.
        // With many CFs, per-CF memtable limits cause unpredictable flush storms.
        // WBM replaces that with a global budget — flush only happens when total
        // memtable usage crosses the threshold, giving predictable flush behavior.
        //
        // Shared per process (see SHARED_BUDGET): cache_normal + wbm_normal == budget,
        // so one shared pair means a process provisions exactly `budget` no matter how
        // many stores it opens.
        //
        // allow_stall=true: stall writes when memtable memory exceeds budget rather
        // than OOM; the adaptive batch controller detects stalls and reduces batch size.
        let (block_cache, write_buffer_manager) = SHARED_BUDGET
            .get_or_init(|| {
                (
                    rocksdb::Cache::new_lru_cache(profile.block_cache_normal_bytes),
                    WriteBufferManager::new_write_buffer_manager(profile.wbm_normal_bytes, true),
                )
            })
            .clone();
        let block_opts = Self::default_block_options(&block_cache);
        opts.set_block_based_table_factory(&block_opts);
        opts.set_write_buffer_manager(&write_buffer_manager);

        (opts, block_cache, write_buffer_manager)
    }

    /// The descriptors every open (primary or secondary) passes to RocksDB:
    /// each allowed CF with its [`Self::cf_options`], so both read through the
    /// process's shared block cache and the CF's bloom filter.
    fn cf_descriptors(
        allowed: &[&str],
        block_cache: &rocksdb::Cache,
        profile: &MemoryProfile,
        runtime_config: StoreRuntimeConfig,
    ) -> Vec<ColumnFamilyDescriptor> {
        allowed
            .iter()
            .map(|name| {
                ColumnFamilyDescriptor::new(
                    *name,
                    Self::cf_options(name, block_cache, profile, runtime_config),
                )
            })
            .collect()
    }

    /// Per-CF options with 3 tiers:
    /// - Mega-write CFs: 256MB × 4 buffers = 1GB per CF
    /// - High-write (remaining CFs): 128MB × 4 buffers = 512MB per CF
    /// - Everything else: 32MB × 2 buffers = 64MB per CF
    fn cf_options(
        name: &str,
        block_cache: &rocksdb::Cache,
        profile: &MemoryProfile,
        runtime_config: StoreRuntimeConfig,
    ) -> Options {
        let mut opts = Options::default();

        // VectorRep memtable: O(1) append instead of O(log n) skiplist insert.
        // Sort deferred to memtable→SST flush (background thread).
        // Only safe with single-writer, no concurrent memtable reads.
        if runtime_config.vector_memtable {
            opts.set_memtable_factory(rocksdb::MemtableFactory::Vector);
        }

        let (max_write_buffer_number, write_buffer_size) =
            Self::live_cf_write_buffer(name, profile);
        opts.set_write_buffer_size(write_buffer_size);
        opts.set_max_write_buffer_number(max_write_buffer_number);

        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_level_zero_slowdown_writes_trigger(12);
        opts.set_level_zero_stop_writes_trigger(24);
        opts.set_max_bytes_for_level_base(profile.normal_max_bytes_for_level_base);
        let spec = Self::spec(name);
        opts.set_compression_type(match spec.compression {
            CfCompression::Lz4 => DBCompressionType::Lz4,
            CfCompression::None => DBCompressionType::None,
        });
        if spec.compaction == CfCompaction::Universal {
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

    /// Returns true when the DB currently holds a live handle for `name`.
    ///
    /// Unlike [`Self::has_cf`] (which checks the store-class allow-list), this
    /// inspects the actually-opened column family handle.
    pub fn cf_handle_exists(&self, name: &str) -> bool {
        self.db.cf_handle(name).is_some()
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
    pub fn cf_mnft_data(&self) -> &ColumnFamily {
        self.cf(CF_MNFT_DATA)
    }
    pub fn cf_mnft_by_collection(&self) -> &ColumnFamily {
        self.cf(CF_MNFT_BY_COLLECTION)
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
    pub fn cf_hodl_capacity_by_date(&self) -> &ColumnFamily {
        self.cf(CF_HODL_CAPACITY_BY_DATE)
    }
    pub fn cf_cell_dist_cohort_by_month(&self) -> &ColumnFamily {
        self.cf(CF_CELL_DIST_COHORT_BY_MONTH)
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
    pub fn cf_stats_mnft(&self) -> &ColumnFamily {
        self.cf(CF_STATS_MNFT)
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
    pub fn cf_script_families(&self) -> &ColumnFamily {
        self.cf(CF_SCRIPT_FAMILIES)
    }
    pub fn cf_script_versions_by_family(&self) -> &ColumnFamily {
        self.cf(CF_SCRIPT_VERSIONS_BY_FAMILY)
    }
    pub fn cf_script_reference_info(&self) -> &ColumnFamily {
        self.cf(CF_SCRIPT_REFERENCE_INFO)
    }
    pub fn cf_script_reference_to_version(&self) -> &ColumnFamily {
        self.cf(CF_SCRIPT_REFERENCE_TO_VERSION)
    }
    pub fn cf_script_family_by_name(&self) -> &ColumnFamily {
        self.cf(CF_SCRIPT_FAMILY_BY_NAME)
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
    pub fn cf_tx_actions(&self) -> &ColumnFamily {
        self.cf(CF_TX_ACTIONS)
    }
    pub fn cf_cluster_agg(&self) -> &ColumnFamily {
        self.cf(CF_CLUSTER_AGG)
    }
    pub fn cf_mnft_collection_agg(&self) -> &ColumnFamily {
        self.cf(CF_MNFT_COLLECTION_AGG)
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
    pub fn cf_addr_fiber_channels(&self) -> &ColumnFamily {
        self.cf(CF_ADDR_FIBER_CHANNELS)
    }
    pub fn cf_dob_decoded(&self) -> &ColumnFamily {
        self.cf(CF_DOB_DECODED)
    }
    pub fn cf_tx_group_cycles(&self) -> &ColumnFamily {
        self.cf(CF_TX_GROUP_CYCLES)
    }
    pub fn cf_object_outpoints(&self) -> &ColumnFamily {
        self.cf(CF_OBJECT_OUTPOINTS)
    }
    pub fn cf_object_outpoint_by_id(&self) -> &ColumnFamily {
        self.cf(CF_OBJECT_OUTPOINT_BY_ID)
    }
    pub fn cf_reorg_events(&self) -> &ColumnFamily {
        self.cf(CF_REORG_EVENTS)
    }
    pub fn cf_lock_scripts(&self) -> &ColumnFamily {
        self.cf(CF_LOCK_SCRIPTS)
    }
    pub fn cf_addr_txs_by_prefix(&self) -> &ColumnFamily {
        self.cf(CF_ADDR_TXS_BY_PREFIX)
    }
    pub fn cf_addr_prefix_stats(&self) -> &ColumnFamily {
        self.cf(CF_ADDR_PREFIX_STATS)
    }
    pub fn cf_dotcell_name_by_owner(&self) -> &ColumnFamily {
        self.cf(CF_DOTCELL_NAME_BY_OWNER)
    }
    pub fn cf_dotcell_ring(&self) -> &ColumnFamily {
        self.cf(CF_DOTCELL_RING)
    }
    pub fn cf_tx_actions_by_bucket(&self) -> &ColumnFamily {
        self.cf(CF_TX_ACTIONS_BY_BUCKET)
    }
    pub fn cf_collection_activity_daily(&self) -> &ColumnFamily {
        self.cf(CF_COLLECTION_ACTIVITY_DAILY)
    }
    pub fn cf_dao_live_agg(&self) -> &ColumnFamily {
        self.cf(CF_DAO_LIVE_AGG)
    }
    pub fn cf_object_collection_activity_count(&self) -> &ColumnFamily {
        self.cf(CF_OBJECT_COLLECTION_ACTIVITY_COUNT)
    }
    pub fn cf_bit_cell_by_account(&self) -> &ColumnFamily {
        self.cf(CF_BIT_CELL_BY_ACCOUNT)
    }

    // ---- Raw DB operations ----

    pub fn get_cf(&self, cf: &ColumnFamily, key: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        #[cfg(any(test, feature = "read-call-counters"))]
        {
            self.read_calls.get_cf.fetch_add(1, Ordering::Relaxed);
            self.read_calls.count_point_read(self.cf_name_of(cf));
        }
        Ok(self.db.get_cf(cf, key)?)
    }

    #[cfg(any(test, feature = "read-call-counters"))]
    pub fn reset_read_call_counters(&self) {
        self.read_calls.get_cf.store(0, Ordering::Relaxed);
        self.read_calls.multi_get_cf.store(0, Ordering::Relaxed);
        self.read_calls
            .iterated_rows
            .lock()
            .expect("iterated-row counter poisoned")
            .clear();
        self.read_calls
            .point_reads
            .lock()
            .expect("point-read counter poisoned")
            .clear();
        self.read_calls
            .multi_gets
            .lock()
            .expect("multi-get counter poisoned")
            .clear();
    }

    /// `multi_get` calls that read column family `cf_name` since the last
    /// reset (`0` when none did).
    #[cfg(any(test, feature = "read-call-counters"))]
    pub fn multi_gets(&self, cf_name: &str) -> u64 {
        self.read_calls
            .multi_gets
            .lock()
            .expect("multi-get counter poisoned")
            .get(cf_name)
            .copied()
            .unwrap_or(0)
    }

    /// Point reads of one column family since the last reset (`0` when it
    /// was never read by key).
    #[cfg(any(test, feature = "read-call-counters"))]
    pub fn point_reads(&self, cf_name: &str) -> u64 {
        self.read_calls
            .point_reads
            .lock()
            .expect("point-read counter poisoned")
            .get(cf_name)
            .copied()
            .unwrap_or(0)
    }

    /// Rows yielded by `iterator_cf` / `prefix_iterator_cf` since the last
    /// reset, for one column family (`0` when it was never iterated).
    #[cfg(any(test, feature = "read-call-counters"))]
    pub fn iterated_rows(&self, cf_name: &str) -> u64 {
        self.read_calls
            .iterated_rows
            .lock()
            .expect("iterated-row counter poisoned")
            .get(cf_name)
            .copied()
            .unwrap_or(0)
    }

    /// The name of the column family `cf` is a handle of.
    pub(crate) fn cf_name_for_handle(&self, cf: &ColumnFamily) -> anyhow::Result<&'static str> {
        let names = self.cf_names_by_handle.get_or_init(|| {
            Self::cfs_for_class(self.store_class)
                .iter()
                .filter_map(|name| {
                    self.db
                        .cf_handle(name)
                        .map(|handle| (handle as *const ColumnFamily as usize, *name))
                })
                .collect()
        });
        names
            .get(&(cf as *const ColumnFamily as usize))
            .copied()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "column family handle does not belong to this {:?} store",
                    self.store_class
                )
            })
    }

    /// The name of the column family `cf` is a handle of (test counters only).
    #[cfg(any(test, feature = "read-call-counters"))]
    fn cf_name_of(&self, cf: &ColumnFamily) -> &'static str {
        Self::cfs_for_class(self.store_class)
            .iter()
            .copied()
            .find(|name| {
                self.db
                    .cf_handle(name)
                    .is_some_and(|handle| std::ptr::eq(handle, cf))
            })
            .expect("iterated column family handle belongs to this store")
    }

    /// `(get_cf calls, multi_get_cf calls)` since the last reset.
    #[cfg(any(test, feature = "read-call-counters"))]
    pub fn read_call_counts(&self) -> (u64, u64) {
        (
            self.read_calls.get_cf.load(Ordering::Relaxed),
            self.read_calls.multi_get_cf.load(Ordering::Relaxed),
        )
    }

    #[cfg(any(test, feature = "read-call-counters"))]
    pub fn reset_write_call_counters(&self) {
        self.read_calls.put_cf.store(0, Ordering::Relaxed);
        self.read_calls.write_batch.store(0, Ordering::Relaxed);
    }

    /// `(put_cf calls, write_batch calls)` since the last reset.
    #[cfg(any(test, feature = "read-call-counters"))]
    pub fn write_call_counts(&self) -> (u64, u64) {
        (
            self.read_calls.put_cf.load(Ordering::Relaxed),
            self.read_calls.write_batch.load(Ordering::Relaxed),
        )
    }

    pub fn put_cf(&self, cf: &ColumnFamily, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        #[cfg(any(test, feature = "read-call-counters"))]
        self.read_calls.put_cf.fetch_add(1, Ordering::Relaxed);
        if self.is_append_only_store() {
            let cf_name = self.append_cf_name_for_handle(cf)?;
            self.validate_append_put_by_cf_name(cf_name, key, value, StoreWriteIntent::Normal)?;
        } else if self.journal_discipline_applies() {
            self.check_unjournaled_write(
                self.cf_name_for_handle(cf)?,
                key,
                self.direct_outside_writer()?,
            )?;
        }
        let _guard = self
            .domain_write_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("store write gate poisoned"))?;
        Ok(self.db.put_cf(cf, key, value)?)
    }

    pub fn delete_cf(&self, cf: &ColumnFamily, key: &[u8]) -> anyhow::Result<()> {
        if self.is_append_only_store() {
            let cf_name = self.append_cf_name_for_handle(cf)?;
            self.validate_append_delete_by_cf_name(cf_name, key, StoreWriteIntent::Normal)?;
        } else if self.journal_discipline_applies() {
            self.check_unjournaled_write(
                self.cf_name_for_handle(cf)?,
                key,
                self.direct_outside_writer()?,
            )?;
        }
        let _guard = self
            .domain_write_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("store write gate poisoned"))?;
        Ok(self.db.delete_cf(cf, key)?)
    }

    /// Perform a read-check-write against canonical domain state without an
    /// intervening indexer batch commit. The closure must use the supplied DB
    /// for its final write rather than calling `put_cf` (which takes this gate).
    pub(crate) fn with_domain_write_gate<T>(
        &self,
        operation: impl FnOnce(&DB) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        anyhow::ensure!(
            matches!(
                self.store_class,
                StoreClass::Domain | StoreClass::TestUnified
            ),
            "conditional cycles write requires a domain store"
        );
        let _guard = self
            .domain_write_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("store write gate poisoned"))?;
        operation(&self.db)
    }

    pub fn multi_get_cf(
        &self,
        keys: Vec<(&ColumnFamily, &[u8])>,
    ) -> Vec<Result<Option<Vec<u8>>, rocksdb::Error>> {
        #[cfg(any(test, feature = "read-call-counters"))]
        {
            self.read_calls.multi_get_cf.fetch_add(1, Ordering::Relaxed);
            for (cf, _) in &keys {
                self.read_calls.count_point_read(self.cf_name_of(cf));
            }
            self.read_calls
                .count_multi_get(keys.iter().map(|(cf, _)| self.cf_name_of(cf)));
        }
        self.db.multi_get_cf(keys)
    }

    /// Same as `multi_get_cf` but sorts the input keys before issuing the
    /// underlying RocksDB call and remaps results back to the caller's order.
    ///
    /// RocksDB's C++ `MultiGet` implementation is faster on sorted keys
    /// (cursor reuse + I/O coalescing). The rust-rocksdb binding does not
    /// sort internally, so we do it here for batch read paths that issue
    /// thousands of point lookups (e.g. parser cell info batch).
    ///
    /// The returned `Vec` has the same length and ordering as the input.
    pub fn multi_get_cf_sorted(
        &self,
        keys: Vec<(&ColumnFamily, &[u8])>,
    ) -> Vec<Result<Option<Vec<u8>>, rocksdb::Error>> {
        #[cfg(any(test, feature = "read-call-counters"))]
        {
            self.read_calls.multi_get_cf.fetch_add(1, Ordering::Relaxed);
            self.read_calls
                .count_multi_get(keys.iter().map(|(cf, _)| self.cf_name_of(cf)));
        }
        let n = keys.len();
        if n <= 1 {
            return self.db.multi_get_cf(keys);
        }
        let mut indexed: Vec<(usize, &ColumnFamily, &[u8])> = keys
            .into_iter()
            .enumerate()
            .map(|(i, (cf, k))| (i, cf, k))
            .collect();
        indexed.sort_by(|a, b| {
            let ap = a.1 as *const ColumnFamily as usize;
            let bp = b.1 as *const ColumnFamily as usize;
            (ap, a.2).cmp(&(bp, b.2))
        });
        let sorted: Vec<(&ColumnFamily, &[u8])> =
            indexed.iter().map(|(_, cf, k)| (*cf, *k)).collect();
        let res = self.db.multi_get_cf(sorted);
        let mut out: Vec<Result<Option<Vec<u8>>, rocksdb::Error>> =
            (0..n).map(|_| Ok(None)).collect();
        for ((orig, _, _), r) in indexed.into_iter().zip(res) {
            out[orig] = r;
        }
        out
    }

    fn write_batch_unchecked(&self, batch: WriteBatch) -> anyhow::Result<()> {
        let _guard = self
            .domain_write_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("store write gate poisoned"))?;
        Ok(self.db.write(batch)?)
    }

    /// Write a batch and fsync the WAL before returning (`WriteOptions::sync`).
    /// Durable across an unclean/host-level shutdown, unlike the default
    /// (page-cache-buffered) WAL write. Used by [`StoreBatch::commit_synced`].
    fn write_batch_synced_unchecked(&self, batch: WriteBatch) -> anyhow::Result<()> {
        let _guard = self
            .domain_write_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("store write gate poisoned"))?;
        let mut opts = rocksdb::WriteOptions::default();
        opts.set_sync(true);
        Ok(self.db.write_opt(batch, &opts)?)
    }

    pub(crate) fn write_batch_with_intent(
        &self,
        batch: WriteBatch,
        intent: StoreWriteIntent,
    ) -> anyhow::Result<()> {
        #[cfg(any(test, feature = "read-call-counters"))]
        self.read_calls.write_batch.fetch_add(1, Ordering::Relaxed);
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

    pub(crate) fn write_batch_with_intent_synced(
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
                "append-only raw write_batch_synced blocked for intent={:?}; \
                 use StoreBatch commit_synced path",
                intent
            );
        }
        self.write_batch_synced_unchecked(batch)
    }

    pub fn write_batch(&self, batch: WriteBatch) -> anyhow::Result<()> {
        self.check_raw_write_batch()?;
        self.write_batch_with_intent(batch, StoreWriteIntent::Normal)
    }

    pub fn write_batch_synced(&self, batch: WriteBatch) -> anyhow::Result<()> {
        self.check_raw_write_batch()?;
        self.write_batch_with_intent_synced(batch, StoreWriteIntent::Normal)
    }

    /// The rollback core's one atomic write: the journal's previous values
    /// written back and the rolled-back blocks' ranges deleted. It is the
    /// rollback itself, so the journal discipline does not apply to it.
    pub(crate) fn write_rollback_batch(&self, batch: WriteBatch) -> anyhow::Result<()> {
        self.write_batch_with_intent(batch, StoreWriteIntent::Normal)
    }

    /// Write a batch with WAL disabled. Use during bulk sync where crash recovery
    /// re-syncs from the last committed block header.
    fn write_batch_no_wal_unchecked(&self, batch: WriteBatch) -> anyhow::Result<()> {
        let _guard = self
            .domain_write_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("store write gate poisoned"))?;
        let mut opts = rocksdb::WriteOptions::default();
        opts.disable_wal(true);
        Ok(self.db.write_opt(batch, &opts)?)
    }

    /// Commit a pre-built WriteBatch without WAL.
    ///
    /// Intended for the bulk-sync flush pipeline where the caller has already
    /// constructed the WriteBatch directly (bypassing StoreBatch).  The caller
    /// is responsible for ensuring all keys target CFs owned by this store.
    ///
    /// # Safety contract (logical, not `unsafe`)
    /// - Keys must target CFs that belong to this store instance.
    /// - Caller must be in bulk-sync mode (no WAL, crash = rebuild).
    pub fn write_batch_no_wal_bulk(&self, batch: rocksdb::WriteBatch) -> anyhow::Result<()> {
        self.check_raw_write_batch()?;
        self.write_batch_no_wal_unchecked(batch)
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
        self.check_raw_write_batch()?;
        self.write_batch_no_wal_with_intent(batch, StoreWriteIntent::Normal)
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
            | keys::STATS_PREFIX_ACTIVITY_HOURLY
            | keys::STATS_PREFIX_ACTIVITY_DAILY_ADDR_ROW
            | keys::STATS_PREFIX_ACTIVITY_HOURLY_ADDR_ROW => Ok(self.cf_stats_chain()),
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
            | keys::STATS_PREFIX_SPORE_TYPE_INDEX => Ok(self.cf_stats_spore()),
            keys::STATS_PREFIX_OBJECT_HOURLY
            | keys::STATS_PREFIX_OBJECT_DAILY
            | keys::STATS_PREFIX_OBJECT_TYPE_INDEX
            | keys::STATS_PREFIX_OBJECT_COLLECTION_OWNER => Ok(self.cf_stats_mnft()),
            _ => anyhow::bail!("unsupported stats prefix: 0x{:02x}", prefix),
        }
    }

    /// Resolve a column family handle by its name.
    pub fn cf_handle_by_name(&self, cf_name: &str) -> Option<&ColumnFamily> {
        self.db.cf_handle(cf_name)
    }

    /// Resolve the stats CF **name** for a stats key prefix.
    ///
    /// `stats_cf_by_prefix` hands back a live `&ColumnFamily` handle, which a
    /// block journal row cannot carry: journal rows are CF-name addressed so
    /// the rollback can re-resolve the handle in whichever process replays
    /// them. Both must stay in step — a prefix that resolves to a handle here
    /// but not to a name would silently lose its journal entries.
    pub fn stats_cf_name_by_prefix(prefix: u8) -> anyhow::Result<&'static str> {
        match prefix {
            keys::STATS_PREFIX_DAILY
            | keys::STATS_PREFIX_HOURLY
            | keys::STATS_PREFIX_EPOCH
            | keys::STATS_PREFIX_MINER
            | keys::STATS_PREFIX_BLOCK_TIME_DIST
            | keys::STATS_PREFIX_EPOCH_TIME_DIST
            | keys::STATS_PREFIX_DAILY_BLOCK
            | keys::STATS_PREFIX_ACTIVITY_DAILY
            | keys::STATS_PREFIX_ACTIVITY_HOURLY
            | keys::STATS_PREFIX_ACTIVITY_DAILY_ADDR_ROW
            | keys::STATS_PREFIX_ACTIVITY_HOURLY_ADDR_ROW => Ok(CF_STATS_CHAIN),
            keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT
            | keys::STATS_PREFIX_DAO_LATEST_STATS
            | keys::STATS_PREFIX_DAO_TOP_DEPOSITORS => Ok(CF_STATS_DAO),
            keys::STATS_PREFIX_HODL_WAVE
            | keys::STATS_PREFIX_CELL_DISTRIBUTION
            | keys::STATS_PREFIX_ADDR_COHORT => Ok(CF_STATS_HODL),
            keys::STATS_PREFIX_SCRIPT_DAILY => Ok(CF_STATS_SCRIPT),
            keys::STATS_PREFIX_TOKEN_TRANSFERS
            | keys::STATS_PREFIX_TOKEN_HOURLY
            | keys::STATS_PREFIX_TOKEN_DAILY => Ok(CF_STATS_TOKEN),
            keys::STATS_PREFIX_CLUSTER_OWNER
            | keys::STATS_PREFIX_SPORE_HOURLY
            | keys::STATS_PREFIX_CLUSTER_DAILY
            | keys::STATS_PREFIX_SPORE_DAILY
            | keys::STATS_PREFIX_SPORE_TYPE_INDEX => Ok(CF_STATS_SPORE),
            keys::STATS_PREFIX_OBJECT_HOURLY
            | keys::STATS_PREFIX_OBJECT_DAILY
            | keys::STATS_PREFIX_OBJECT_TYPE_INDEX
            | keys::STATS_PREFIX_OBJECT_COLLECTION_OWNER => Ok(CF_STATS_MNFT),
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
        #[cfg(any(test, feature = "read-call-counters"))]
        let cf_name = self.cf_name_of(cf);
        self.db.iterator_cf(cf, mode).inspect(move |_row| {
            #[cfg(any(test, feature = "read-call-counters"))]
            self.read_calls.count_iterated_row(cf_name);
        })
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
        #[cfg(any(test, feature = "read-call-counters"))]
        let cf_name = self.cf_name_of(cf);
        self.db
            .iterator_cf_opt(cf, opts, mode)
            .take_while(move |item| match item {
                Ok((key, _)) => key.starts_with(&prefix_vec),
                Err(_) => true,
            })
            .inspect(move |_row| {
                #[cfg(any(test, feature = "read-call-counters"))]
                self.read_calls.count_iterated_row(cf_name);
            })
    }

    // ---- Bulk sync mode ----

    pub fn set_bulk_sync_mode(&self, enabled: bool) {
        self.bulk_sync_mode.store(enabled, Ordering::Relaxed);
    }

    pub fn is_bulk_sync_mode(&self) -> bool {
        self.bulk_sync_mode.load(Ordering::Relaxed)
    }

    // ---- Journal discipline ----

    /// Turn the journal discipline on for this store handle (the indexer does,
    /// before its first write): from now on, outside bulk sync, an unjournaled
    /// commit that writes a `Journal`-class key not covered by
    /// [`OUTSIDE_JOURNAL`] fails, and so does a raw `WriteBatch` commit.
    pub fn enforce_journal_discipline(&self) {
        self.journal_discipline.store(true, Ordering::Relaxed);
    }

    pub(crate) fn journal_discipline_applies(&self) -> bool {
        self.journal_discipline.load(Ordering::Relaxed) && !self.is_bulk_sync_mode()
    }

    /// Run this thread's direct (non-batch) writes as `writer` until the
    /// returned guard drops: inside it, a direct unjournaled write is admitted
    /// where an [`OUTSIDE_JOURNAL`] entry names `writer`. The scope belongs to
    /// the thread that opened it (a sync writer such as label import; never
    /// held across an `.await`), and scopes do not nest.
    pub fn outside_journal_scope(
        &self,
        writer: OutsideWriter,
    ) -> anyhow::Result<OutsideJournalScope<'_>> {
        let mut slot = self
            .outside_writer_scope
            .lock()
            .map_err(|_| anyhow::anyhow!("outside-journal scope lock poisoned"))?;
        if let Some((thread, open)) = *slot {
            anyhow::bail!(
                "outside-journal scope {writer:?} requested while {open:?} is open on thread \
                 {thread:?}: scopes do not nest"
            );
        }
        *slot = Some((std::thread::current().id(), writer));
        Ok(OutsideJournalScope { store: self })
    }

    /// The writer of a direct write on this thread: the open
    /// [`Self::outside_journal_scope`]'s, if this thread opened it.
    fn direct_outside_writer(&self) -> anyhow::Result<Option<OutsideWriter>> {
        let slot = self
            .outside_writer_scope
            .lock()
            .map_err(|_| anyhow::anyhow!("outside-journal scope lock poisoned"))?;
        Ok(slot
            .filter(|(thread, _)| *thread == std::thread::current().id())
            .map(|(_, writer)| writer))
    }

    /// Check one unjournaled write (a direct put / delete, or a write of an
    /// unjournaled `StoreBatch`) by `writer` against the journal discipline.
    pub(crate) fn check_unjournaled_write(
        &self,
        cf: &str,
        key: &[u8],
        writer: Option<OutsideWriter>,
    ) -> anyhow::Result<()> {
        if !self.journal_discipline_applies() {
            return Ok(());
        }
        let journaled =
            cf_spec(cf).is_some_and(|spec| spec.rollback_class == RollbackClass::Journal);
        if !journaled || outside_journal_entry_for(cf, key, writer).is_some() {
            return Ok(());
        }
        anyhow::bail!(
            "unjournaled write to a Journal-class key outside the OUTSIDE_JOURNAL table: cf={cf} \
             key=0x{} writer={writer:?}; a live write goes through a journaled batch in its \
             block's scope, and a write that belongs outside the journal is listed in \
             OUTSIDE_JOURNAL with its owner, reason and (for a whole column family) the one \
             writer it admits",
            crate::bytes_to_hex(key)
        )
    }

    /// [`Self::check_unjournaled_write`] for a range delete `[start, end)`:
    /// covered only when one entry's prefix holds both bounds (or the entry
    /// is the whole column family).
    pub(crate) fn check_unjournaled_range_delete(
        &self,
        cf: &str,
        start: &[u8],
        end: &[u8],
        writer: Option<OutsideWriter>,
    ) -> anyhow::Result<()> {
        if !self.journal_discipline_applies() {
            return Ok(());
        }
        let journaled =
            cf_spec(cf).is_some_and(|spec| spec.rollback_class == RollbackClass::Journal);
        let covered = OUTSIDE_JOURNAL.iter().any(|entry| {
            entry.cf == cf
                && entry.writer.is_none_or(|named| writer == Some(named))
                && (entry.key_prefix.is_empty()
                    || (start.starts_with(entry.key_prefix) && end.starts_with(entry.key_prefix)))
        });
        if !journaled || covered {
            return Ok(());
        }
        anyhow::bail!(
            "unjournaled range delete on a Journal-class CF outside the OUTSIDE_JOURNAL table: \
             cf={cf} start=0x{} end=0x{} writer={writer:?}",
            crate::bytes_to_hex(start),
            crate::bytes_to_hex(end)
        )
    }

    fn check_raw_write_batch(&self) -> anyhow::Result<()> {
        if self.journal_discipline_applies() {
            anyhow::bail!(
                "raw WriteBatch commit on a store under journal discipline outside bulk sync: \
                 its writes cannot be checked against OUTSIDE_JOURNAL; commit through a \
                 StoreBatch"
            );
        }
        Ok(())
    }

    pub fn memory_profile(&self) -> &MemoryProfile {
        &self.memory_profile
    }

    /// Log the key RocksDB tuning parameters at startup.
    pub fn log_config(&self) {
        let p = &self.memory_profile;
        info!(
            system_ram_gb = p.system_ram_bytes / (1024 * 1024 * 1024),
            physical_cores = p.cpu_count,
            logical_cores = p.logical_cpu_count,
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
            flush_threads = p.flush_threads,
            max_subcompactions = p.max_subcompactions,
            block_cache_normal_mb = p.block_cache_normal_bytes / (1024 * 1024),
            block_cache_bulk_mb = p.block_cache_bulk_sync_bytes / (1024 * 1024),
            wbm_normal_mb = p.wbm_normal_bytes / (1024 * 1024),
            wbm_bulk_mb = p.wbm_bulk_sync_bytes / (1024 * 1024),
            unordered_write = !self.runtime_config.vector_memtable,
            vector_memtable = self.runtime_config.vector_memtable,
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

    /// Dynamically adjust the number of RocksDB background flush and
    /// compaction threads.  Used by the bottleneck controller to shift CPU
    /// between compaction and build/fetch work.
    ///
    /// RocksDB's `SetDBOptions` is not exposed through the C API, so we
    /// adjust the default `Env` thread pools directly, split by
    /// [`background_thread_split`]:
    ///   - HIGH priority pool → flush threads: `max(2, jobs / 4)`, never one
    ///     thread shared by the domain and append-only stores
    ///   - LOW  priority pool → compaction threads: `max(1, jobs - max(1, jobs / 4))`
    ///
    /// Returns the split it applied.
    pub fn set_max_background_jobs(&self, jobs: i32) -> anyhow::Result<BackgroundThreads> {
        let mut env = rocksdb::Env::new()
            .map_err(|e| anyhow::anyhow!("failed to get default RocksDB env: {}", e))?;
        let split = background_thread_split(jobs);
        env.set_high_priority_background_threads(split.flush);
        env.set_low_priority_background_threads(split.compaction);
        debug!(
            jobs,
            flush_threads = split.flush,
            compaction_threads = split.compaction,
            "RocksDB background pools resized"
        );
        Ok(split)
    }

    /// Size the process-wide HIGH-priority (flush) pool from the profile at
    /// primary open (idempotent: same profile, same size). RocksDB itself only
    /// raises the pool at open to `max_background_jobs / 4`, which can be 1;
    /// the profile's `flush_threads` is never below 2. The bulk controller
    /// moves the pool afterwards through `set_max_background_jobs`.
    fn size_flush_pool(profile: &MemoryProfile) -> anyhow::Result<()> {
        let mut env = rocksdb::Env::new()
            .map_err(|e| anyhow::anyhow!("failed to get default RocksDB env: {}", e))?;
        env.set_high_priority_background_threads(profile.flush_threads);
        Ok(())
    }

    /// Set relaxed L0 thresholds and larger write buffers for bulk sync.
    ///
    /// During bulk sync, parallel writer threads each commit large WriteBatches.
    /// The default per-CF `max_write_buffer_number=4` can cause flush stalls when
    /// memtables fill faster than background flush can drain. Increasing to
    /// 12 (mega) / 8 (high) / 6 (low) gives more headroom before stalling.
    /// Per-CF values come from [`Self::bulk_cf_options`].
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
                let options = Self::bulk_cf_options(cf_name, p);
                let options: Vec<(&str, &str)> = options
                    .iter()
                    .map(|(name, value)| (*name, value.as_str()))
                    .collect();
                let result = self.db.set_options_cf(cf, &options);
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
        // Idempotent: skip if already in normal mode. Reaching this branch
        // means we are coming out of bulk sync, which is the only context
        // that needs the pre-flush + WBM step combined.
        if !self.bulk_sync_mode.load(Ordering::Relaxed) {
            return;
        }
        self.bulk_sync_mode.store(false, Ordering::Relaxed);
        self.apply_normal_compaction_options(true);
    }

    /// Apply the live-sync compaction profile (WBM cap, per-CF write
    /// buffers, L0 triggers, block cache size) to a freshly-opened DB.
    ///
    /// Used both by `restore_normal_compaction_options` after bulk sync
    /// completes (with `flush_first = true` so any in-memory bulk data is
    /// durable before WBM contracts) **and** by the supervisor at
    /// startup when the indexer comes up directly in live mode (with
    /// `flush_first = false`, since memtables are empty and there is
    /// nothing to flush).
    ///
    /// Without the startup-time call, the WBM cap was previously only
    /// applied on the bulk→live boundary; an indexer that started near
    /// the chain tip kept the open-time `wbm_normal_bytes` (~24 GB) and
    /// never benefitted from the live-sync tuning intended for it.
    pub fn apply_normal_compaction_options(&self, flush_first: bool) {
        if flush_first {
            // Flush all memtables to SST BEFORE reducing WBM budget. This
            // ensures all `commit_no_wal()` data from bulk sync is durable.
            if let Err(e) = self.flush_all_memtables() {
                error!(
                    error = %e,
                    "Failed to flush memtables during bulk-to-normal transition; \
                     unflushed commit_no_wal data is at risk on crash"
                );
            }
        }

        let p = &self.memory_profile;
        let level_base_str = p.normal_max_bytes_for_level_base.to_string();
        let file_base_str = p.normal_target_file_size_base.to_string();

        // Cap WBM budget for live sync. The open-time wbm_normal_bytes is
        // scaled for bulk-sync headroom (often 24 GB+). Live sync writes
        // ~3-5 MB/block; the previous 64 MB cap triggered an atomic flush
        // every ~13-21 blocks. 384 MB stretches the interval to ~80-130
        // blocks. Skiplist memtables (live sync uses skiplist, not
        // VectorRep) handle 384 MB comfortably; memory cost is bounded
        // by at most one full WBM worth of dirty memtable.
        const LIVE_WBM_CAP_BYTES: usize = 384 * MB as usize;
        let live_wbm = p.wbm_normal_bytes.min(LIVE_WBM_CAP_BYTES);
        self.write_buffer_manager.set_buffer_size(live_wbm);

        self.block_cache
            .lock()
            .expect("block_cache lock poisoned")
            .set_capacity(p.block_cache_normal_bytes);

        // Per-CF write buffers come from the memory profile, the same tiering
        // used at open time. They are NOT shrunk to fixed small values here:
        // with `atomic_flush = 1` the smallest CF buffer decides how often the
        // WHOLE database switches memtables, so an 8/4/2 MB live profile made
        // `script_versions` (testnet ~2.0 MB/block) and `sync_meta` (mainnet
        // ~1.97 MB/block) trigger a 26-CF flush every 5-9 blocks: 15,548 /
        // 7,395 flushes on 2026-09-22, 7,148 / 5,343 SSTs and a 51 MB MANIFEST
        // that cost the API secondary 596 s to open. Total memtable memory
        // stays governed by the capped WBM above, not by per-CF minimums.
        for &cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                let (max_write_buffer_number, write_buffer_size) =
                    Self::live_cf_write_buffer(cf_name, p);
                let max_wb = max_write_buffer_number.to_string();
                let wb_size = write_buffer_size.to_string();
                if let Err(e) = self.db.set_options_cf(
                    cf,
                    &[
                        ("level0_slowdown_writes_trigger", "12"),
                        ("level0_stop_writes_trigger", "24"),
                        ("max_write_buffer_number", &max_wb),
                        ("write_buffer_size", &wb_size),
                        ("max_bytes_for_level_base", &level_base_str),
                        ("target_file_size_base", &file_base_str),
                    ],
                ) {
                    warn!(cf = cf_name, error = %e, "failed to set normal compaction options for CF");
                }
            }
        }
        info!(
            wbm_budget_mb = live_wbm / (1024 * 1024),
            block_cache_mb = p.block_cache_normal_bytes / (1024 * 1024),
            write_buffer_mega_mb = p.write_buffer_mega_bytes / (1024 * 1024),
            write_buffer_high_mb = p.write_buffer_high_bytes / (1024 * 1024),
            write_buffer_low_mb = p.write_buffer_low_bytes / (1024 * 1024),
            flush_first,
            "Live compaction options applied: l0_slowdown=12, l0_stop=24"
        );
    }

    /// Flush all column family memtables to SST files.
    ///
    /// Required after bulk sync (commit_no_wal) to make memtable data durable.
    /// Each CF must be flushed individually: `flush_cf` rotates that CF's
    /// active memtable to immutable and triggers a flush. With VectorRep
    /// memtables, unflushed active memtables require O(n) linear scan per
    /// read — flushing converts them to sorted SSTs with bloom filters.
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

    /// One waiting flush of exactly `cfs`. With `atomic_flush` RocksDB
    /// switches every listed CF's memtable in one write-thread pass and
    /// flushes them as one job (`DBImpl::Flush(options, column_families)`):
    /// serially on one flush thread, installed together. Each name must be a
    /// CF this store opens, and the list must not be empty (RocksDB reads an
    /// empty list as "every CF"); otherwise nothing is flushed.
    ///
    /// Bulk finalize flushes its two domain sets with it:
    /// [`BATCH_LOOP_ONLY_CFS`] once the drain has committed its last batch,
    /// [`FINALIZE_WRITTEN_CFS`] in phase 12.
    pub fn flush_column_families(&self, cfs: &[&str]) -> anyhow::Result<()> {
        if cfs.is_empty() {
            anyhow::bail!("flush_column_families needs at least one column family");
        }
        let started = std::time::Instant::now();
        let handles = cfs
            .iter()
            .map(|&name| self.opened_cf(name))
            .collect::<anyhow::Result<Vec<_>>>()?;
        self.db
            .flush_cfs_opt(&handles, &rocksdb::FlushOptions::default())
            .map_err(|e| {
                anyhow::anyhow!(
                    "flush_column_families failed on {} CFs [{}]: {}",
                    cfs.len(),
                    cfs.join(","),
                    e
                )
            })?;
        let elapsed_ms = started.elapsed().as_millis();
        info!(
            elapsed_ms,
            cfs = cfs.len(),
            "Flushed column family memtables to SST"
        );
        Ok(())
    }

    /// Memtable and SST state of one CF this store opens, from RocksDB's
    /// per-CF properties (read only).
    pub fn cf_flush_state(&self, cf_name: &str) -> anyhow::Result<CfFlushState> {
        let cf = self.opened_cf(cf_name)?;
        let read = |property: &str| {
            required_flush_property(
                self.db
                    .property_int_value_cf(cf, property)
                    .map_err(Into::into),
                &format!("CF {cf_name} {property}"),
            )
        };
        Ok(CfFlushState {
            active_entries: read("rocksdb.num-entries-active-mem-table")?,
            unflushed_immutables: read("rocksdb.num-immutable-mem-table")?,
            live_sst_bytes: read("rocksdb.live-sst-files-size")?,
        })
    }

    /// The handle of `name`, or an error naming it when this store does not
    /// open that CF.
    fn opened_cf(&self, name: &str) -> anyhow::Result<&ColumnFamily> {
        if !Self::cf_allowed(self.store_class, name) {
            anyhow::bail!(
                "column family '{}' is not a {:?} store column family",
                name,
                self.store_class
            );
        }
        self.db.cf_handle(name).ok_or_else(|| {
            anyhow::anyhow!(
                "column family '{}' is not open in this {:?} store",
                name,
                self.store_class
            )
        })
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
    pub fn compaction_pressure(&self) -> anyhow::Result<CompactionPressureSnapshot> {
        let mut compaction_pending_bytes = 0u64;
        let mut l0_files_total = 0u64;
        let mut l0_files_max: u64 = 0;
        let mut immutable_memtables = 0u64;
        for &cf_name in Self::cfs_for_class(self.store_class) {
            let cf = self.db.cf_handle(cf_name).ok_or_else(|| {
                anyhow::anyhow!(
                    "compaction pressure CF missing: path={} cf={cf_name}",
                    self.db_path.display()
                )
            })?;
            let read = |property| {
                required_compaction_property(
                    self.db
                        .property_int_value_cf(cf, property)
                        .map_err(Into::into),
                    &self.db_path,
                    cf_name,
                    property,
                )
            };
            let pending = read("rocksdb.estimate-pending-compaction-bytes")?;
            compaction_pending_bytes =
                compaction_pending_bytes
                    .checked_add(pending)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "compaction pending bytes overflow: path={} cf={cf_name}",
                            self.db_path.display()
                        )
                    })?;
            let l0 = read("rocksdb.num-files-at-level0")?;
            l0_files_total = l0_files_total.checked_add(l0).ok_or_else(|| {
                anyhow::anyhow!(
                    "compaction L0 file count overflow: path={} cf={cf_name}",
                    self.db_path.display()
                )
            })?;
            l0_files_max = l0_files_max.max(l0);
            let immutable = read("rocksdb.num-immutable-mem-table")?;
            immutable_memtables = immutable_memtables.checked_add(immutable).ok_or_else(|| {
                anyhow::anyhow!(
                    "compaction immutable memtable count overflow: path={} cf={cf_name}",
                    self.db_path.display()
                )
            })?;
        }
        Ok(CompactionPressureSnapshot {
            l0_files_total,
            l0_files_max,
            compaction_pending_bytes,
            immutable_memtables,
        })
    }

    pub fn memory_stats(&self) -> MemoryStats {
        let mut memtable_bytes = Some(0u64);
        let mut table_readers_bytes = Some(0u64);
        let mut cells_count = None;
        let mut live_cells_count = None;
        let mut consumed_cells_count = None;
        let mut consumed_cf_live_data_bytes: Option<u64> = None;
        let mut consumed_cf_memtable_bytes: Option<u64> = None;
        let mut block_headers_count = None;
        let mut addr_balance_count = None;
        let mut compaction_pending_bytes = Some(0u64);
        let mut sst_files_size = Some(0u64);
        let mut l0_files_total = Some(0u64);
        let mut l0_files_max = Some(0u64);
        let mut l0_worst_cf = Some(String::new());
        let mut immutable_memtables = Some(0u64);
        let mut cf_sizes: Option<Vec<(String, u64)>> = Some(Vec::new());

        for &cf_name in ALL_CFS {
            let Some(cf) = self.db.cf_handle(cf_name) else {
                continue;
            };
            let read = |property: &str| match self.db.property_int_value_cf(cf, property) {
                Ok(Some(value)) => Some(value),
                Ok(None) => {
                    warn!(path = %self.db_path.display(), cf = cf_name, property,
                        "RocksDB telemetry property unavailable");
                    None
                }
                Err(error) => {
                    warn!(path = %self.db_path.display(), cf = cf_name, property, %error,
                        "failed to read RocksDB telemetry property");
                    None
                }
            };
            let add = |metric: &str, prior: Option<u64>, value: Option<u64>| {
                let total = complete_observed_sum(prior, value);
                if let (Some(prior), Some(value), None) = (prior, value, total) {
                    warn!(path = %self.db_path.display(), metric, prior, value,
                        "RocksDB telemetry sum overflow");
                }
                total
            };

            let memtable = read("rocksdb.cur-size-all-mem-tables");
            memtable_bytes = add("memtable_bytes", memtable_bytes, memtable);
            if cf_name == CF_CONSUMED_CELLS {
                consumed_cf_memtable_bytes = memtable;
            }
            table_readers_bytes = add(
                "table_readers_bytes",
                table_readers_bytes,
                read("rocksdb.estimate-table-readers-mem"),
            );
            compaction_pending_bytes = add(
                "compaction_pending_bytes",
                compaction_pending_bytes,
                read("rocksdb.estimate-pending-compaction-bytes"),
            );
            if matches!(
                cf_name,
                CF_CELLS | CF_LIVE_CELLS | CF_CONSUMED_CELLS | CF_BLOCK_HEADERS | CF_ADDR_BALANCE
            ) {
                let count = read("rocksdb.estimate-num-keys");
                match cf_name {
                    CF_CELLS => cells_count = count,
                    CF_LIVE_CELLS => live_cells_count = count,
                    CF_CONSUMED_CELLS => consumed_cells_count = count,
                    CF_BLOCK_HEADERS => block_headers_count = count,
                    CF_ADDR_BALANCE => addr_balance_count = count,
                    _ => unreachable!(),
                }
            }
            let sst = read("rocksdb.total-sst-files-size");
            sst_files_size = add("sst_files_size", sst_files_size, sst);

            let l0 = read("rocksdb.num-files-at-level0");
            l0_files_total = add("l0_files_total", l0_files_total, l0);
            match (l0_files_max, l0) {
                (Some(max), Some(value)) if value > max => {
                    l0_files_max = Some(value);
                    l0_worst_cf = Some(cf_name.to_string());
                }
                (Some(_), Some(_)) => {}
                _ => {
                    l0_files_max = None;
                    l0_worst_cf = None;
                }
            }
            immutable_memtables = add(
                "immutable_memtables",
                immutable_memtables,
                read("rocksdb.num-immutable-mem-table"),
            );

            let live = read("rocksdb.estimate-live-data-size");
            if cf_name == CF_CONSUMED_CELLS {
                consumed_cf_live_data_bytes = live;
            }
            match (cf_sizes.as_mut(), live) {
                (Some(sizes), Some(value)) if value > 0 => sizes.push((cf_name.to_string(), value)),
                (Some(_), Some(_)) => {}
                _ => cf_sizes = None,
            }
        }

        // Sort by size descending and keep top 5
        if let Some(sizes) = cf_sizes.as_mut() {
            sizes.sort_by_key(|item| std::cmp::Reverse(item.1));
            sizes.truncate(5);
        }
        let (consumed_cells_bytes, consumed_cells_bytes_source) =
            consumed_cf_storage_bytes(consumed_cf_live_data_bytes, consumed_cf_memtable_bytes);
        if consumed_cf_live_data_bytes.is_some()
            && consumed_cf_memtable_bytes.is_some()
            && consumed_cells_bytes.is_none()
        {
            warn!(path = %self.db_path.display(), ?consumed_cf_live_data_bytes,
                ?consumed_cf_memtable_bytes, "consumed-cell storage observation overflow");
        }

        let block_cache_bytes = self
            .block_cache
            .lock()
            .expect("block_cache lock poisoned")
            .get_usage();
        let memory_bytes = memtable_bytes
            .and_then(|value| value.checked_add(block_cache_bytes as u64))
            .and_then(|value| table_readers_bytes.and_then(|readers| value.checked_add(readers)));
        if memtable_bytes.is_some() && table_readers_bytes.is_some() && memory_bytes.is_none() {
            warn!(path = %self.db_path.display(), ?memtable_bytes, block_cache_bytes,
                ?table_readers_bytes, "RocksDB total memory observation overflow");
        }
        let num_running_compactions = match self
            .db
            .property_int_value("rocksdb.num-running-compactions")
        {
            Ok(value) => {
                if value.is_none() {
                    warn!(path = %self.db_path.display(), "RocksDB global running-compactions property unavailable");
                }
                value
            }
            Err(error) => {
                warn!(path = %self.db_path.display(), %error, "failed to read RocksDB global running-compactions property");
                None
            }
        };

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
            sst_files_total: self.sst_files_total_cached(SST_FILE_COUNT_MAX_AGE),
            manifest_bytes: self.manifest_bytes(),
            top_cf_sizes: cf_sizes,
            wbm_usage_bytes: self.write_buffer_manager.get_usage(),
            wbm_budget_bytes: self.write_buffer_manager.get_buffer_size(),
        }
    }

    /// Total SST files across every level of every CF, recomputed at most once
    /// per `max_age`.
    ///
    /// The sweep costs one property read per level per CF (7 × 60 ≈ 420 calls
    /// per store, each under the DB mutex), while `memory_stats()` is sampled
    /// every 3 s during bulk sync. The number moves on the timescale of
    /// compactions, so a bounded-age cache gives the same signal without
    /// putting that sweep on every sample.
    fn sst_files_total_cached(&self, max_age: Duration) -> Option<u64> {
        let mut cache = self
            .sst_file_count_cache
            .lock()
            .expect("sst_file_count_cache lock poisoned");
        if let Some((measured_at, value)) = *cache {
            if measured_at.elapsed() < max_age {
                return Some(value);
            }
        }
        let value = self.count_sst_files();
        if let Some(value) = value {
            *cache = Some((Instant::now(), value));
        }
        value
    }

    fn count_sst_files(&self) -> Option<u64> {
        let mut total = 0u64;
        for &cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                for level in 0..=MAX_LEVEL_FOR_FILE_COUNT {
                    let property = format!("rocksdb.num-files-at-level{level}");
                    let value = match self.db.property_int_value_cf(cf, &property) {
                        Ok(Some(value)) => value,
                        Ok(None) => {
                            warn!(path = %self.db_path.display(), cf = cf_name, property,
                                "RocksDB SST file count unavailable");
                            return None;
                        }
                        Err(error) => {
                            warn!(path = %self.db_path.display(), cf = cf_name, property, %error,
                                "failed to read RocksDB SST file count");
                            return None;
                        }
                    };
                    total = match total.checked_add(value) {
                        Some(total) => total,
                        None => {
                            warn!(path = %self.db_path.display(), cf = cf_name, property,
                                prior = total, value, "RocksDB SST file count overflow");
                            return None;
                        }
                    };
                }
            }
        }
        Some(total)
    }

    /// Size of the MANIFEST named by this DB's `CURRENT` file, in bytes.
    ///
    /// None when `CURRENT` or the MANIFEST it names cannot be read — a
    /// just-created DB directory, or a secondary whose primary rotated the
    /// MANIFEST between the two reads. This is a diagnostic gauge, never an
    /// input to chain state.
    pub fn manifest_bytes(&self) -> Option<u64> {
        manifest_bytes_at(&self.db_path)
    }

    /// Cheap snapshot of write-side flush activity. Used by the live-sync
    /// health monitor to verify that WBM tuning actually reduces flush
    /// frequency (the suspected dominant cost in per-block commit time).
    ///
    /// `num_running_flushes` is DB-wide; the per-CF counters are summed
    /// over all CFs in this store. With atomic_flush enabled, flushes
    /// fan out across every CF of the store simultaneously, so summing is
    /// representative of total flush pressure.
    pub fn flush_stats(&self) -> anyhow::Result<FlushStats> {
        let num_running_flushes = required_flush_property(
            self.db
                .property_int_value("rocksdb.num-running-flushes")
                .map_err(Into::into),
            "db rocksdb.num-running-flushes",
        )?;
        let mut mem_table_flush_pending: u64 = 0;
        let mut active_memtable_bytes: u64 = 0;
        for &cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                let pending = required_flush_property(
                    self.db
                        .property_int_value_cf(cf, "rocksdb.mem-table-flush-pending")
                        .map_err(Into::into),
                    &format!("CF {cf_name} rocksdb.mem-table-flush-pending"),
                )?;
                mem_table_flush_pending =
                    mem_table_flush_pending
                        .checked_add(pending)
                        .ok_or_else(|| {
                            anyhow::anyhow!("flush-pending sum overflows at CF {cf_name}")
                        })?;
                let active = required_flush_property(
                    self.db
                        .property_int_value_cf(cf, "rocksdb.cur-size-active-mem-table")
                        .map_err(Into::into),
                    &format!("CF {cf_name} rocksdb.cur-size-active-mem-table"),
                )?;
                active_memtable_bytes =
                    active_memtable_bytes.checked_add(active).ok_or_else(|| {
                        anyhow::anyhow!("active memtable byte sum overflows at CF {cf_name}")
                    })?;
            }
        }
        Ok(FlushStats {
            num_running_flushes,
            mem_table_flush_pending,
            active_memtable_bytes,
            wbm_usage_bytes: self.write_buffer_manager.get_usage() as u64,
            wbm_budget_bytes: self.write_buffer_manager.get_buffer_size() as u64,
        })
    }
}

fn required_flush_property(value: anyhow::Result<Option<u64>>, name: &str) -> anyhow::Result<u64> {
    match value {
        Ok(Some(value)) => Ok(value),
        Ok(None) => anyhow::bail!("RocksDB flush property unavailable: {name}"),
        Err(error) => anyhow::bail!("RocksDB flush property {name}: {error}"),
    }
}

fn required_compaction_property(
    value: anyhow::Result<Option<u64>>,
    path: &Path,
    cf: &str,
    property: &str,
) -> anyhow::Result<u64> {
    match value {
        Ok(Some(value)) => Ok(value),
        Ok(None) => anyhow::bail!(
            "compaction pressure property unavailable: path={} cf={cf} property={property}",
            path.display()
        ),
        Err(error) => anyhow::bail!(
            "compaction pressure property read failed: path={} cf={cf} property={property}: {error}",
            path.display()
        ),
    }
}

/// One column family's memtable and SST state ([`CkbadgerStore::cf_flush_state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CfFlushState {
    /// Entries (puts and deletes) in the active memtable.
    pub active_entries: u64,
    /// Immutable memtables not flushed yet.
    pub unflushed_immutables: u64,
    /// Bytes of the CF's live SST files.
    pub live_sst_bytes: u64,
}

/// Lightweight snapshot of write-side flush activity, sampled by the
/// live-sync health monitor.
#[derive(Debug, Clone, Copy, Default)]
pub struct FlushStats {
    pub num_running_flushes: u64,
    pub mem_table_flush_pending: u64,
    pub active_memtable_bytes: u64,
    pub wbm_usage_bytes: u64,
    pub wbm_budget_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp test store owns its directory: shared through `Arc`s it stays,
    /// and the last handle's drop closes the database (the first field) and
    /// then removes the directory (the last field). The helpers used to
    /// `mem::forget` the `TempDir`, leaking one RocksDB directory a call.
    /// The directory must be gone afterwards: `TempDir`'s drop swallows its
    /// error, so a failed removal would leave it, and a database still open
    /// would write its LOG / MANIFEST on close and recreate it.
    #[test]
    fn a_temp_test_store_removes_its_directory_with_the_last_handle() {
        type Open = fn() -> anyhow::Result<CkbadgerStore>;
        let opens: [(&str, Open); 4] = [
            ("unified", CkbadgerStore::open_test_unified_temp),
            ("network", CkbadgerStore::open_test_network_temp),
            ("domain", CkbadgerStore::open_domain_temp),
            ("append-only", CkbadgerStore::open_append_only_temp),
        ];
        for (kind, open) in opens {
            let store = std::sync::Arc::new(open().unwrap());
            let dir = store
                .test_dir()
                .unwrap_or_else(|| panic!("{kind}: a temp store knows its directory"))
                .to_path_buf();
            assert!(
                dir.join("CURRENT").exists(),
                "{kind}: a RocksDB lives there"
            );
            let cf = store
                .db
                .cf_handle(CF_SYNC_META)
                .or_else(|| store.db.cf_handle(CF_CELLS));
            let cf = cf.or_else(|| store.db.cf_handle(CF_NET_NODES)).unwrap();
            store.put_cf(cf, b"k", b"v").unwrap();
            let shared = std::sync::Arc::clone(&store);
            drop(store);
            assert!(dir.exists(), "{kind}: a shared store keeps its directory");
            drop(shared);
            assert!(
                !dir.exists(),
                "{kind}: the last handle removes the directory, after the database closed"
            );
        }
        // A store opened at a caller's path owns no directory.
        let caller = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(caller.path()).unwrap();
        assert!(store.test_dir().is_none());
        drop(store);
        assert!(
            caller.path().exists(),
            "the caller's directory is the caller's"
        );
    }

    #[test]
    fn compaction_control_rejects_missing_or_failed_property() {
        let path = Path::new("/tmp/domain");
        assert_eq!(
            required_compaction_property(
                Ok(Some(0)),
                path,
                "activities",
                "rocksdb.num-files-at-level0"
            )
            .unwrap(),
            0
        );
        let missing = required_compaction_property(
            Ok(None),
            path,
            "activities",
            "rocksdb.num-files-at-level0",
        )
        .unwrap_err()
        .to_string();
        assert!(missing.contains("/tmp/domain"));
        assert!(missing.contains("activities"));
        assert!(missing.contains("rocksdb.num-files-at-level0"));
        let failed = required_compaction_property(
            Err(anyhow::anyhow!("I/O")),
            path,
            "activities",
            "rocksdb.num-files-at-level0",
        )
        .unwrap_err()
        .to_string();
        assert!(failed.contains("I/O"));
    }
    use tempfile::TempDir;

    #[test]
    fn unavailable_flush_property_is_not_reported_as_zero() {
        assert_eq!(
            required_flush_property(Ok(Some(0)), "db flushes").unwrap(),
            0
        );
        assert!(required_flush_property(Ok(None), "db flushes")
            .unwrap_err()
            .to_string()
            .contains("db flushes"));
        assert!(
            required_flush_property(Err(anyhow::anyhow!("IO failed")), "db flushes")
                .unwrap_err()
                .to_string()
                .contains("IO failed")
        );
    }

    #[test]
    fn empty_domain_store_exposes_real_flush_properties() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let stats = store.flush_stats().unwrap();
        assert_eq!(stats.num_running_flushes, 0);
        assert!(stats.wbm_budget_bytes > 0);
    }

    // ── P3.3: live write buffers, flush storm, MANIFEST ────────────────

    fn small_primary_profile() -> MemoryProfile {
        // 2 GB budget: mega 64 MB, high 32 MB, low 8 MB (all above the 8/4/2 MB
        // the live profile used to force).
        MemoryProfile::compute(2 * GB, 8, 16, false)
    }

    /// The live profile must never shrink a CF's write buffer below the
    /// profile's low tier. With `atomic_flush = 1` the SMALLEST per-CF buffer
    /// decides how often the WHOLE DB switches memtables: a 2 MB low tier made
    /// `script_versions` (testnet, ~2.0 MB rewritten per block) and `sync_meta`
    /// (mainnet, ~1.97 MB per block) trigger a 26-CF flush every 5-9 blocks —
    /// 15,548 / 7,395 flushes on 2026-09-22, 7,148 / 5,343 SSTs, 51 MB MANIFEST.
    #[test]
    fn live_options_never_set_cf_write_buffer_below_profile_low() {
        let profile = small_primary_profile();
        assert!(profile.write_buffer_low_bytes >= 8 * MB as usize);

        for &cf_name in ALL_CFS {
            let (max_write_buffer_number, write_buffer_size) =
                CkbadgerStore::live_cf_write_buffer(cf_name, &profile);
            assert!(
                write_buffer_size >= profile.write_buffer_low_bytes,
                "{cf_name}: live write_buffer_size {write_buffer_size} is below the profile low tier {}",
                profile.write_buffer_low_bytes
            );
            let expected = if CkbadgerStore::is_mega_write_cf(cf_name) {
                (4, profile.write_buffer_mega_bytes)
            } else if CkbadgerStore::is_high_write_cf(cf_name) {
                (4, profile.write_buffer_high_bytes)
            } else {
                (2, profile.write_buffer_low_bytes)
            };
            assert_eq!(
                (max_write_buffer_number, write_buffer_size),
                expected,
                "{cf_name}: live tiering must match the open-time tiering"
            );
        }
    }

    /// Small live batches must not cost one flush round each.
    ///
    /// Writes 24 MB in ~0.5 MB batches into a low-tier CF after applying the
    /// live profile, with auto-compaction off so L0 files count flush rounds
    /// 1:1. The old hardcoded 2 MB low tier switched the memtable every ~4
    /// batches (~12 rounds here) and, under `atomic_flush`, took every other
    /// CF with it; the profile's 8 MB low tier must stay in single digits.
    #[test]
    fn live_write_buffers_do_not_flush_once_per_small_batch() {
        let dir = TempDir::new().unwrap();
        let runtime_config = StoreRuntimeConfig {
            memory_budget_gb: Some(2),
            direct_io_reads: false,
            vector_memtable: false,
            network_count: NonZeroUsize::MIN,
        };
        let store = CkbadgerStore::open_domain_with_runtime(dir.path(), runtime_config).unwrap();
        store.apply_normal_compaction_options(false);
        let profile = &store.memory_profile;
        assert_eq!(profile.write_buffer_low_bytes, 8 * MB as usize);

        // L0 files then count flush rounds directly instead of being merged
        // away by compaction mid-test.
        let cf = store.cf(CF_SYNC_META);
        store
            .db
            .set_options_cf(cf, &[("disable_auto_compactions", "true")])
            .unwrap();

        const BATCHES: u32 = 48;
        const KEYS_PER_BATCH: u32 = 8;
        let payload = vec![0xAB_u8; 64 * 1024];
        for batch_index in 0..BATCHES {
            let mut batch = crate::batch::StoreBatch::new(&store);
            for key_index in 0..KEYS_PER_BATCH {
                let key = format!("p3.3-flush-probe-{batch_index:04}-{key_index:04}");
                batch
                    .put_raw_cf_by_name(CF_SYNC_META, key.as_bytes(), &payload)
                    .unwrap();
            }
            batch.commit().unwrap();
        }
        let written_bytes = u64::from(BATCHES) * u64::from(KEYS_PER_BATCH) * (64 * 1024 + 32);

        // Flushes are asynchronous: wait for quiescence before counting, and
        // FAIL if it is not reached — counting mid-flush would silently under
        // report rounds and make this test pass for the wrong reason.
        let mut quiesced = false;
        for _ in 0..200 {
            let running = store
                .db
                .property_int_value("rocksdb.num-running-flushes")
                .unwrap()
                .unwrap_or(0);
            let pending = store
                .db
                .property_int_value_cf(cf, "rocksdb.mem-table-flush-pending")
                .unwrap()
                .unwrap_or(0);
            if running == 0 && pending == 0 {
                quiesced = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            quiesced,
            "flushes never quiesced; the L0 count below would be meaningless"
        );
        let l0_files = store
            .db
            .property_int_value_cf(cf, "rocksdb.num-files-at-level0")
            .unwrap()
            .unwrap_or(0);

        // One round per full write buffer, plus one for the tail.
        let max_rounds = written_bytes / profile.write_buffer_low_bytes as u64 + 1;
        assert!(
            l0_files <= max_rounds,
            "flush rounds must follow the profile's write buffer, not one per batch: \
             l0_files={l0_files}, max_rounds={max_rounds}, written_bytes={written_bytes}, \
             write_buffer_low={}",
            profile.write_buffer_low_bytes
        );
    }

    /// The per-level file sweep costs ~420 property reads per store, each
    /// under the DB mutex, while bulk sync samples `memory_stats()` every 3 s.
    /// The count is cached for a bounded age instead.
    #[test]
    fn sst_file_count_is_recomputed_at_most_once_per_max_age() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let write_and_flush = |payload: &[u8], key: &str| {
            let mut batch = crate::batch::StoreBatch::new(&store);
            batch
                .put_raw_cf_by_name(CF_SYNC_META, key.as_bytes(), payload)
                .unwrap();
            batch.commit().unwrap();
            store.flush_all_memtables().unwrap();
        };

        write_and_flush(b"one", "p3.4-sst-cache-1");
        let first = store.sst_files_total_cached(Duration::from_secs(30));
        assert!(
            first.expect("SST count") >= 1,
            "a flushed store must have at least one SST"
        );

        // A second flush inside the window is not resampled...
        write_and_flush(b"two", "p3.4-sst-cache-2");
        assert_eq!(
            store.sst_files_total_cached(Duration::from_secs(30)),
            first,
            "the count must be served from cache inside its max age"
        );
        // ...and is picked up as soon as the cached value is too old.
        assert!(
            store
                .sst_files_total_cached(Duration::ZERO)
                .expect("SST count")
                > first.expect("SST count"),
            "an expired cache must recompute"
        );
    }

    /// The flush-storm outcome signals must be visible in-process, not only in
    /// the RocksDB LOG: SST file count and MANIFEST size are what made the API
    /// domain secondary take 596 s to open.
    #[test]
    fn memory_stats_report_sst_count_and_manifest_size() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = crate::batch::StoreBatch::new(&store);
        batch
            .put_raw_cf_by_name(CF_SYNC_META, b"p3.3-manifest-probe", b"v")
            .unwrap();
        batch.commit().unwrap();
        store.flush_all_memtables().unwrap();

        let stats = store.memory_stats();
        assert!(
            stats.sst_files_total.expect("SST count") >= 1,
            "a flushed store must report at least one SST file: {stats:?}"
        );
        assert!(
            stats.manifest_bytes.expect("MANIFEST size") > 0,
            "an open store must report its MANIFEST size: {stats:?}"
        );
    }

    /// The per-CF point-read counter behind the API's "never reads a row
    /// by key" assertions counts both read shapes, per column family.
    /// Each multi-get call counts once for every column family it reads.
    #[test]
    fn multi_gets_are_counted_per_column_family() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        store.reset_read_call_counters();
        let (index, headers) = (store.cf_tx_index(), store.cf_block_headers());
        store.multi_get_cf_sorted(vec![(index, b"a".as_slice()), (index, b"b".as_slice())]);
        store.multi_get_cf(vec![(index, b"a".as_slice()), (headers, b"h".as_slice())]);
        assert_eq!(store.multi_gets(CF_TX_INDEX), 2);
        assert_eq!(store.multi_gets(CF_BLOCK_HEADERS), 1);
        assert_eq!(store.multi_gets(CF_TX_ACTIONS), 0);
        store.reset_read_call_counters();
        assert_eq!(store.multi_gets(CF_TX_INDEX), 0);
    }

    #[test]
    fn point_reads_are_counted_per_column_family() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        store.reset_read_call_counters();
        store.get_cf(store.cf_dao_deposits(), b"k").unwrap();
        let _ = store.multi_get_cf(vec![
            (store.cf_dao_deposits(), b"a".as_slice()),
            (store.cf_sync_meta(), b"b".as_slice()),
        ]);
        assert_eq!(store.point_reads(CF_DAO_DEPOSITS), 2);
        assert_eq!(store.point_reads(CF_SYNC_META), 1);
        assert_eq!(store.point_reads(CF_DAO_BY_BLOCK), 0);
        store.reset_read_call_counters();
        assert_eq!(store.point_reads(CF_DAO_DEPOSITS), 0);
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
    fn test_domain_schema_has_no_ambiguous_fiber_funding_args_index() {
        assert_eq!(DOMAIN_CFS.len(), 74);
        assert!(!DOMAIN_CFS.contains(&"fiber_channel_by_funding_args"));
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
    fn test_is_append_only_cf_name_matches_cells_only_store() {
        assert!(is_append_only_cf_name(CF_CELLS));
        assert!(!is_append_only_cf_name(CF_LIVE_CELLS));
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

        let err = store
            .validate_append_delete_by_cf_name(CF_CELLS, b"k1", StoreWriteIntent::AppendValidated)
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
                crate::keys::encode_script_daily_key(&[0xAB; 32], 1, false, 20240201).to_vec(),
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
                crate::keys::encode_object_daily_key(&[0xDE; 24], 20240201).to_vec(),
                b"object",
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
            .get_cf(store.cf_stats_mnft(), &cases[7].0)
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

    /// A secondary reads every CF through the primary's own per-CF options
    /// (the process's shared block cache, the bloom filter), not RocksDB's
    /// defaults. Opened by name, each CF of a secondary got a private 32 MB
    /// LRU cache and no filter, so an API point get read a data block in
    /// every sorted run of `cells`, and a list page cycled through more
    /// blocks than 32 MB holds (A-5).
    #[test]
    fn secondary_reads_every_cf_through_the_primary_table_options() {
        let block_cache_capacity = |store: &CkbadgerStore, cf: &ColumnFamily| {
            store
                .db
                .property_int_value_cf(cf, "rocksdb.block-cache-capacity")
                .unwrap()
                .expect("a block-based CF reports its block cache capacity")
        };
        let rocksdb_default_capacity = 32 * MB;

        let append_dir = TempDir::new().unwrap();
        let append_secondary_dir = TempDir::new().unwrap();
        let append = CkbadgerStore::open_append_only(append_dir.path()).unwrap();
        let append_secondary = CkbadgerStore::open_append_only_secondary(
            append_dir.path(),
            append_secondary_dir.path(),
        )
        .unwrap();
        let primary_cells = block_cache_capacity(&append, append.cf_cells());
        assert_ne!(primary_cells, rocksdb_default_capacity);
        assert_eq!(
            block_cache_capacity(&append_secondary, append_secondary.cf_cells()),
            primary_cells,
            "the secondary's cells CF reads through the shared block cache"
        );

        let domain_dir = TempDir::new().unwrap();
        let domain_secondary_dir = TempDir::new().unwrap();
        let domain = CkbadgerStore::open_domain(domain_dir.path()).unwrap();
        let domain_secondary =
            CkbadgerStore::open_domain_secondary(domain_dir.path(), domain_secondary_dir.path())
                .unwrap();
        assert_eq!(
            block_cache_capacity(&domain_secondary, domain_secondary.cf_cell_by_lock()),
            block_cache_capacity(&domain, domain.cf_cell_by_lock()),
            "the secondary's index CFs read through the shared block cache"
        );
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
            vector_memtable: false,
            network_count: NonZeroUsize::MIN,
        };

        let store = CkbadgerStore::open_domain_with_runtime(dir.path(), runtime_config).unwrap();

        assert_eq!(store.runtime_config(), runtime_config);
        assert_eq!(store.memory_profile().system_ram_bytes, 12 * GB);
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
        assert!(stats.live_cells_count.expect("live cell count") >= 1);
        assert!(stats.consumed_cells_count.expect("consumed cell count") >= 1);
        assert!(stats.block_headers_count.expect("header count") >= 1);
        assert!(stats.addr_balance_count.expect("address count") >= 1);
        assert!(
            stats.cells_count.expect("cell count")
                >= stats.live_cells_count.expect("live cell count")
        );
        assert!(
            stats.consumed_cells_bytes_source == Some("live+mem"),
            "unexpected source: {:?}",
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
        assert_eq!(stats.num_running_compactions, global);
    }

    #[test]
    fn test_consumed_cf_storage_bytes_requires_both_observations() {
        assert_eq!(
            consumed_cf_storage_bytes(Some(100), Some(20)),
            (Some(120), Some("live+mem"))
        );
        assert_eq!(consumed_cf_storage_bytes(None, Some(20)), (None, None));
        assert_eq!(
            consumed_cf_storage_bytes(Some(0), Some(20)),
            (Some(20), Some("live+mem"))
        );
    }

    #[test]
    fn test_consumed_cf_storage_bytes_returns_none_when_all_missing() {
        assert_eq!(consumed_cf_storage_bytes(None, None), (None, None));
    }

    #[test]
    fn test_consumed_cf_storage_bytes_missing_memtable_is_unknown() {
        assert_eq!(consumed_cf_storage_bytes(Some(10), None), (None, None));
    }

    #[test]
    fn telemetry_sum_does_not_publish_partial_or_overflowed_values() {
        assert_eq!(complete_observed_sum(Some(7), Some(3)), Some(10));
        assert_eq!(complete_observed_sum(Some(7), None), None);
        assert_eq!(complete_observed_sum(None, Some(3)), None);
        assert_eq!(complete_observed_sum(Some(u64::MAX), Some(1)), None);
    }

    #[test]
    fn missing_manifest_observation_is_not_zero_bytes() {
        let dir = TempDir::new().unwrap();
        assert_eq!(manifest_bytes_at(dir.path()), None);
        std::fs::write(dir.path().join("CURRENT"), "MANIFEST-missing\n").unwrap();
        assert_eq!(manifest_bytes_at(dir.path()), None);
        std::fs::write(dir.path().join("MANIFEST-missing"), []).unwrap();
        assert_eq!(manifest_bytes_at(dir.path()), Some(0));
    }

    #[test]
    fn test_mega_write_cfs_excludes_script_cfs() {
        for cf in [
            CF_SCRIPT_INFO,
            CF_SCRIPT_VERSIONS,
            CF_SCRIPT_VERSIONS_BY_LABEL,
            CF_SCRIPT_FAMILIES,
            CF_SCRIPT_VERSIONS_BY_FAMILY,
            CF_SCRIPT_REFERENCE_INFO,
            CF_SCRIPT_REFERENCE_TO_VERSION,
            CF_SCRIPT_FAMILY_BY_NAME,
        ] {
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
            CF_TX_ACTIONS,
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

    /// `cells` keeps base's bulk profile: the budget-scaled hot-CF buffer and
    /// the mega tier's 12 buffers. A fixed 512 MiB x 16 buffer (task 0-b,
    /// reverted in 0-e) cut each flush short, but the 512 MiB L0 files made
    /// universal compaction rewrite `cells` almost twice: 73.3 GiB of
    /// compaction output against 42.0 / 40.1 GiB in base (cand2-1 vs
    /// base46-1 / base46-2), +555 s of background CPU, and a build 12-19 %
    /// slower per cell from block 5M on.
    #[test]
    fn bulk_cells_keeps_base_buffer_because_512_mib_l0_files_doubled_universal_compaction_output() {
        for system_ram in [48 * GB, 96 * GB] {
            let profile = MemoryProfile::compute(system_ram, 12, 24, false);
            assert_eq!(
                CkbadgerStore::bulk_cf_options(CF_CELLS, &profile),
                vec![
                    ("level0_slowdown_writes_trigger", "96".to_string()),
                    ("level0_stop_writes_trigger", "192".to_string()),
                    ("max_write_buffer_number", "12".to_string()),
                    (
                        "max_bytes_for_level_base",
                        profile.bulk_max_bytes_for_level_base.to_string(),
                    ),
                    (
                        "target_file_size_base",
                        profile.bulk_target_file_size_base.to_string(),
                    ),
                    (
                        "write_buffer_size",
                        profile.write_buffer_hot_cf_bytes.to_string(),
                    ),
                ],
                "cells bulk options at system_ram={system_ram}"
            );
        }
    }

    /// Every CF other than `cells` (pinned by its own test above) keeps its
    /// base bulk profile.
    #[test]
    fn bulk_options_of_every_other_cf_are_unchanged() {
        let domain_hot_cfs = [
            CF_TX_INDEX,
            CF_LIVE_CELLS,
            CF_CONSUMED_CELLS,
            CF_CELL_BY_LOCK,
            CF_TX_ACTIONS,
        ];
        let profile = MemoryProfile::compute(96 * GB, 12, 24, false);
        for &cf_name in ALL_CFS {
            if cf_name == CF_CELLS {
                continue;
            }
            let max_write_buffer_number = if CkbadgerStore::is_mega_write_cf(cf_name) {
                "12"
            } else if CkbadgerStore::is_high_write_cf(cf_name) {
                "8"
            } else {
                "6"
            };
            let mut expected = vec![
                ("level0_slowdown_writes_trigger", "96".to_string()),
                ("level0_stop_writes_trigger", "192".to_string()),
                (
                    "max_write_buffer_number",
                    max_write_buffer_number.to_string(),
                ),
                (
                    "max_bytes_for_level_base",
                    profile.bulk_max_bytes_for_level_base.to_string(),
                ),
                (
                    "target_file_size_base",
                    profile.bulk_target_file_size_base.to_string(),
                ),
            ];
            if domain_hot_cfs.contains(&cf_name) {
                expected.push((
                    "write_buffer_size",
                    profile.write_buffer_hot_cf_bytes.to_string(),
                ));
            }
            assert_eq!(
                CkbadgerStore::bulk_cf_options(cf_name, &profile),
                expected,
                "{cf_name}: bulk options"
            );
        }
    }

    /// Reads one CF's options from the newest `OPTIONS-*` file RocksDB wrote,
    /// which `SetOptions` rewrites on every call.
    fn persisted_cf_options(
        db_path: &Path,
        cf_name: &str,
    ) -> std::collections::HashMap<String, String> {
        let newest = std::fs::read_dir(db_path)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.unwrap().file_name().into_string().unwrap();
                let number = name.strip_prefix("OPTIONS-")?.parse::<u64>().ok()?;
                Some((number, name))
            })
            .max()
            .expect("RocksDB OPTIONS file")
            .1;
        let text = std::fs::read_to_string(db_path.join(newest)).unwrap();
        let header = format!("[CFOptions \"{cf_name}\"]");
        let mut options = std::collections::HashMap::new();
        let mut in_section = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_section = line == header;
                continue;
            }
            if in_section {
                if let Some((name, value)) = line.split_once('=') {
                    options.insert(name.to_string(), value.to_string());
                }
            }
        }
        assert!(!options.is_empty(), "no {header} section in OPTIONS file");
        options
    }

    /// The setter applies `bulk_cf_options` to a real append-only store, and
    /// the live restore puts `cells` back on its live tier afterwards.
    #[test]
    fn append_only_store_applies_bulk_cells_options_and_restores_live_tier() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();

        store.set_bulk_sync_compaction_options();
        let bulk = persisted_cf_options(dir.path(), CF_CELLS);
        assert_eq!(
            bulk["write_buffer_size"],
            store.memory_profile.write_buffer_hot_cf_bytes.to_string()
        );
        assert_eq!(bulk["max_write_buffer_number"], "12");

        store.restore_normal_compaction_options();
        let live = persisted_cf_options(dir.path(), CF_CELLS);
        let (live_max_write_buffer_number, live_write_buffer_size) =
            CkbadgerStore::live_cf_write_buffer(CF_CELLS, &store.memory_profile);
        assert_eq!(
            live["write_buffer_size"],
            live_write_buffer_size.to_string()
        );
        assert_eq!(
            live["max_write_buffer_number"],
            live_max_write_buffer_number.to_string()
        );
    }

    /// `cells` is lz4 on every option path the primary takes: open-time
    /// `cf_options`, the bulk overrides and the live restore (neither setter
    /// touches compression, so open-time lz4 must survive both), and a real
    /// flush in bulk mode writes compressed SSTs. A 16 KB-block lz4 trial on
    /// mainnet `cells` SSTs gave ratio 0.38; zstd-1 0.345 was not worth its
    /// CPU on the flush thread (plan J3).
    #[test]
    fn cells_is_lz4_compressed_on_open_bulk_and_live_restore_paths() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let assert_lz4 = |step: &str| {
            let options = persisted_cf_options(dir.path(), CF_CELLS);
            assert_eq!(
                options["compression"], "kLZ4Compression",
                "cells compression after {step}"
            );
            // kDisableCompressionOption = the bottommost level uses `compression`.
            assert_eq!(
                options["bottommost_compression"], "kDisableCompressionOption",
                "cells bottommost compression after {step}"
            );
            assert_eq!(
                options["compaction_style"], "kCompactionStyleUniversal",
                "cells compaction style after {step}"
            );
        };
        assert_lz4("open");

        store.set_bulk_sync_compaction_options();
        assert_lz4("bulk options");

        // Real bytes: 4 MiB of payload-shaped, compressible rows flushed in
        // bulk mode must land in SSTs well under the raw size.
        let cf = store.cf_cells();
        let mut written = 0usize;
        for i in 0u32..16_384 {
            let mut tx_hash = [0u8; 32];
            tx_hash[..4].copy_from_slice(&i.to_be_bytes());
            let key = crate::keys::encode_outpoint(&tx_hash, 0);
            let mut value = vec![0u8; 256];
            value[..4].copy_from_slice(&i.to_le_bytes());
            value[64..96].copy_from_slice(&[0x9b; 32]);
            store.db.put_cf(cf, key, &value).unwrap();
            written += key.len() + value.len();
        }
        store.flush_all_memtables().unwrap();
        let sst_bytes = store
            .db
            .property_int_value_cf(cf, "rocksdb.live-sst-files-size")
            .unwrap()
            .expect("live-sst-files-size");
        assert!(
            (sst_bytes as usize) * 100 < written * 60,
            "cells SST bytes {sst_bytes} not < 60 % of {written} bytes written"
        );

        store.restore_normal_compaction_options();
        assert_lz4("live restore");
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
    fn test_memory_profile_32gb_primary_reproduces_original_constants() {
        let profile = MemoryProfile::compute(32 * GB, 12, 24, false);
        assert_eq!(profile.rocksdb_budget_bytes, 16 * GB as usize);
        assert_eq!(profile.wbm_normal_bytes, 8 * GB as usize);
        assert_eq!(profile.max_background_jobs, 24);
        assert_eq!(profile.flush_threads, 6);
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
        let profile = MemoryProfile::compute(96 * GB, 12, 24, false);
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
        let profile = MemoryProfile::compute(128 * GB, 12, 24, true);
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
    fn supervisor_secondary_owner_is_distinct() {
        use super::{secondary_store_path, SecondaryStoreOwner};
        assert_eq!(
            SecondaryStoreOwner::Supervisor.suffix(),
            "supervisor-secondary"
        );
        let owners = [
            SecondaryStoreOwner::Api,
            SecondaryStoreOwner::Tui,
            SecondaryStoreOwner::Cli,
            SecondaryStoreOwner::Supervisor,
        ];
        for i in 0..owners.len() {
            for j in (i + 1)..owners.len() {
                assert_ne!(owners[i].suffix(), owners[j].suffix());
                assert_ne!(
                    secondary_store_path("/x/domain", owners[i]),
                    secondary_store_path("/x/domain", owners[j]),
                );
            }
        }
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

    /// `flush_column_families` is one waiting flush of exactly the listed
    /// CFs: their no-WAL rows reach SSTs, any other CF keeps its memtable,
    /// and `cf_flush_state` reports both.
    #[test]
    fn flush_column_families_flushes_only_the_listed_cfs() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = WriteBatch::default();
        batch.put_cf(store.cf(CF_TX_HASH_MAP), b"tx", b"a");
        batch.put_cf(store.cf(CF_BLOCK_HASH_INDEX), b"block", b"b");
        batch.put_cf(store.cf(CF_ADDR_BALANCE), b"addr", b"c");
        store.write_batch_no_wal(batch).unwrap();
        let unflushed = CfFlushState {
            active_entries: 1,
            unflushed_immutables: 0,
            live_sst_bytes: 0,
        };
        assert_eq!(store.cf_flush_state(CF_TX_HASH_MAP).unwrap(), unflushed);

        store
            .flush_column_families(&[CF_TX_HASH_MAP, CF_BLOCK_HASH_INDEX])
            .unwrap();

        for flushed in [CF_TX_HASH_MAP, CF_BLOCK_HASH_INDEX] {
            let state = store.cf_flush_state(flushed).unwrap();
            assert_eq!(
                (state.active_entries, state.unflushed_immutables),
                (0, 0),
                "{flushed}"
            );
            assert!(state.live_sst_bytes > 0, "{flushed}: {state:?}");
        }
        assert_eq!(store.cf_flush_state(CF_ADDR_BALANCE).unwrap(), unflushed);
        assert_eq!(
            store
                .get_cf(store.cf(CF_TX_HASH_MAP), b"tx")
                .unwrap()
                .as_deref(),
            Some(b"a".as_slice())
        );
    }

    /// A CF name this store does not open is an error naming it, for both
    /// helpers, and so is an empty list; nothing is flushed.
    #[test]
    fn flush_column_families_rejects_a_cf_outside_the_store() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store.put_cf(store.cf(CF_TX_HASH_MAP), b"tx", b"a").unwrap();
        for foreign in [CF_CELLS, CF_NET_NODES, "no_such_cf"] {
            let error = store
                .flush_column_families(&[CF_TX_HASH_MAP, foreign])
                .unwrap_err()
                .to_string();
            assert!(error.contains(foreign), "{error}");
            let error = store.cf_flush_state(foreign).unwrap_err().to_string();
            assert!(error.contains(foreign), "{error}");
        }
        // RocksDB reads an empty list as "every CF".
        let error = store.flush_column_families(&[]).unwrap_err().to_string();
        assert!(error.contains("at least one column family"), "{error}");
        assert_eq!(
            store.cf_flush_state(CF_TX_HASH_MAP).unwrap().active_entries,
            1,
            "a refused flush must not switch any CF"
        );
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
    fn test_cf_write_policy_marks_cell_by_data_hash_as_append_only_history() {
        assert_eq!(
            cf_write_policy(CF_CELL_BY_DATA_HASH),
            CfWritePolicy::AppendOnly
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

    #[test]
    fn test_set_max_background_jobs_succeeds() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        // This previously failed with "Extra option not recognized: max_background_jobs"
        // because set_options() targets CF options, not DB options.
        // The controller's job count moves both pools; the flush pool never
        // drops below 2.
        for (jobs, flush, compaction) in [(4, 2, 3), (6, 2, 5), (7, 2, 6), (8, 2, 6), (24, 6, 18)] {
            let applied = store
                .set_max_background_jobs(jobs)
                .expect("dynamic background jobs adjustment should succeed");
            assert_eq!(
                applied,
                BackgroundThreads { flush, compaction },
                "jobs={jobs}"
            );
        }
    }

    /// The flush pool follows the bulk controller as in base, `jobs / 4`,
    /// but never below 2. At the controller's floor (6-7 jobs on a 24-thread
    /// host) base's `max(1, jobs / 4)` left ONE flush thread shared by the
    /// domain store's atomic multi-CF flushes and the `cells` flushes: in
    /// base46-1, 66 s of the 89 s finalize `cells` flush was queueing. The
    /// compaction pool keeps base's count, so from 8 jobs up the split is
    /// base's, and below it the only difference is 2 flush threads instead of 1.
    #[test]
    fn background_thread_split_follows_the_controller_with_at_least_two_flush_threads() {
        for (jobs, flush, compaction) in [
            (4, 2, 3),
            (6, 2, 5),
            (7, 2, 6),
            (8, 2, 6),
            (12, 3, 9),
            (24, 6, 18),
        ] {
            assert_eq!(
                background_thread_split(jobs),
                BackgroundThreads { flush, compaction },
                "jobs={jobs}"
            );
        }
    }

    /// The open-time flush pool comes from the profile's
    /// `max_background_jobs` through the same split, never below 2, so the
    /// domain and append-only stores never share one flush thread.
    #[test]
    fn memory_profile_flush_threads_are_a_quarter_of_background_jobs_floor_two() {
        for (logical_cpus, max_background_jobs, flush_threads) in [
            (1, 4, 2),
            (4, 4, 2),
            (8, 8, 2),
            (12, 12, 3),
            (24, 24, 6),
            (64, 32, 8),
        ] {
            let profile = MemoryProfile::compute(32 * GB, 12, logical_cpus, false);
            assert_eq!(
                profile.max_background_jobs, max_background_jobs,
                "logical_cpus={logical_cpus}"
            );
            assert_eq!(
                profile.flush_threads, flush_threads,
                "logical_cpus={logical_cpus}"
            );
        }
    }

    #[test]
    fn test_multi_get_cf_sorted_preserves_input_order() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let cf = store.cf_live_cells();

        // Insert 200 keys with shuffled, non-monotonic values.
        let mut entries: Vec<(Vec<u8>, Vec<u8>)> = (0u32..200)
            .map(|i| {
                let mut k = vec![0u8; 36];
                k[0..4].copy_from_slice(&i.to_be_bytes());
                let v = format!("v{}", i).into_bytes();
                (k, v)
            })
            .collect();
        for (k, v) in &entries {
            store.put_cf(cf, k, v).unwrap();
        }

        // Reverse + interleave so input is far from sorted.
        entries.reverse();
        let queries: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            entries.iter().map(|(k, _)| (cf, k.as_slice())).collect();
        let unsorted = store.multi_get_cf(queries.clone());
        let sorted = store.multi_get_cf_sorted(queries);

        assert_eq!(unsorted.len(), sorted.len());
        for (i, (got_unsorted, got_sorted)) in unsorted.into_iter().zip(sorted).enumerate() {
            let expected = &entries[i].1;
            assert_eq!(
                got_unsorted.unwrap().as_deref(),
                Some(expected.as_slice()),
                "unsorted[{}] mismatch",
                i
            );
            assert_eq!(
                got_sorted.unwrap().as_deref(),
                Some(expected.as_slice()),
                "sorted[{}] mismatch — output order must match input order",
                i
            );
        }
    }

    #[test]
    fn test_multi_get_cf_sorted_handles_duplicates_and_missing() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let cf = store.cf_live_cells();

        let k_a = b"keyA__padding_to_outpoint_size__abcd".to_vec();
        let k_b = b"keyB__padding_to_outpoint_size__abcd".to_vec();
        let k_missing = b"missing_padding_to_outpoint_size_xyz".to_vec();
        store.put_cf(cf, &k_a, b"VA").unwrap();
        store.put_cf(cf, &k_b, b"VB").unwrap();

        let queries: Vec<(&rocksdb::ColumnFamily, &[u8])> = vec![
            (cf, k_b.as_slice()),
            (cf, k_a.as_slice()),
            (cf, k_b.as_slice()),
            (cf, k_missing.as_slice()),
            (cf, k_a.as_slice()),
        ];
        let res = store.multi_get_cf_sorted(queries);

        assert_eq!(res[0].as_ref().unwrap().as_deref(), Some(b"VB" as &[u8]));
        assert_eq!(res[1].as_ref().unwrap().as_deref(), Some(b"VA" as &[u8]));
        assert_eq!(res[2].as_ref().unwrap().as_deref(), Some(b"VB" as &[u8]));
        assert!(res[3].as_ref().unwrap().is_none());
        assert_eq!(res[4].as_ref().unwrap().as_deref(), Some(b"VA" as &[u8]));
    }

    #[test]
    fn test_multi_get_cf_sorted_mixed_cfs() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let live = store.cf_live_cells();
        let consumed = store.cf_consumed_cells();

        let mut k1 = vec![0u8; 36];
        k1[0] = 1;
        let mut k2 = vec![0u8; 36];
        k2[0] = 2;
        store.put_cf(live, &k1, b"L1").unwrap();
        store.put_cf(consumed, &k1, b"C1").unwrap();
        store.put_cf(live, &k2, b"L2").unwrap();

        let queries: Vec<(&rocksdb::ColumnFamily, &[u8])> = vec![
            (live, k2.as_slice()),
            (consumed, k1.as_slice()),
            (live, k1.as_slice()),
        ];
        let res = store.multi_get_cf_sorted(queries);
        assert_eq!(res[0].as_ref().unwrap().as_deref(), Some(b"L2" as &[u8]));
        assert_eq!(res[1].as_ref().unwrap().as_deref(), Some(b"C1" as &[u8]));
        assert_eq!(res[2].as_ref().unwrap().as_deref(), Some(b"L1" as &[u8]));
    }

    #[test]
    fn test_multi_get_cf_sorted_empty_and_single_inputs() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let cf = store.cf_live_cells();

        let empty: Vec<(&rocksdb::ColumnFamily, &[u8])> = vec![];
        assert!(store.multi_get_cf_sorted(empty).is_empty());

        let mut k = vec![0u8; 36];
        k[0] = 9;
        store.put_cf(cf, &k, b"only").unwrap();
        let one = store.multi_get_cf_sorted(vec![(cf, k.as_slice())]);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].as_ref().unwrap().as_deref(), Some(b"only" as &[u8]));
    }

    #[test]
    fn network_store_opens_with_three_cfs_rw() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_network(dir.path()).unwrap();
        // All network CFs must be resolvable as live handles in the opened store.
        assert!(store.cf_handle_exists(CF_NET_NODES));
        assert!(store.cf_handle_exists(CF_NET_STATS));
        assert!(store.cf_handle_exists(CF_NET_CRAWL));
        assert!(!store.is_secondary());
    }

    #[test]
    fn network_store_open_divides_ram_by_the_forwarded_network_count() {
        // Guards the wiring, not the arithmetic (the `effective_ram_*` and
        // `detect_system_resources` tests own that). `open_network` used to
        // hardcode `StoreRuntimeConfig::default()`, and the crawler's ONLY store
        // open goes through it — so N co-resident crawlers each provisioned
        // cache/WBM caps from UNDIVIDED host RAM, an N x over-commit. That single
        // forwarded argument is all that stands between this fix and being inert.
        //
        // Host-independent: both opens detect the same RAM R in this process, and
        // (R / 1) / 2 == R / 2 exactly for u64. The per-store MemoryProfile is
        // computed from the runtime config (unlike the process-wide SHARED_BUDGET,
        // which the first open in the process pins), so it is deterministic here.
        let one_dir = tempfile::tempdir().unwrap();
        let two_dir = tempfile::tempdir().unwrap();

        let one = CkbadgerStore::open_network_with_runtime(
            one_dir.path(),
            StoreRuntimeConfig {
                network_count: NonZeroUsize::MIN,
                ..Default::default()
            },
        )
        .unwrap();
        let two = CkbadgerStore::open_network_with_runtime(
            two_dir.path(),
            StoreRuntimeConfig {
                network_count: NonZeroUsize::new(2).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(two.runtime_config().network_count.get(), 2);
        assert_eq!(
            two.memory_profile().wbm_normal_bytes,
            one.memory_profile().wbm_normal_bytes / 2,
            "the network store's budget must be the network's RAM share, not the host's"
        );
    }

    #[test]
    fn open_network_keeps_its_default_runtime_config_for_test_callers() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_network(dir.path()).unwrap();
        assert_eq!(
            store.runtime_config().network_count,
            NonZeroUsize::MIN,
            "the thin wrapper must stay the single-network default"
        );
    }

    #[test]
    fn network_cfs_are_not_append_only() {
        // Network store is mutable/domain-like; it must NOT be classified append-only.
        assert!(!is_append_only_cf_name(CF_NET_NODES));
        assert!(!is_append_only_cf_name(CF_NET_STATS));
        assert!(!is_append_only_cf_name(CF_NET_CRAWL));
    }

    #[test]
    fn effective_ram_explicit_override_wins_and_is_never_divided() {
        // memory_budget_gb is already the operator's per-network value: dividing it
        // by N would silently halve what they asked for. The closure must not run.
        let ram = effective_ram_bytes(Some(40), NonZeroUsize::new(2).unwrap(), || {
            panic!("must not detect when overridden")
        });
        assert_eq!(ram, 40 * GB);
    }

    #[test]
    fn effective_ram_single_network_is_the_full_detected_ram() {
        // N=1 is the degenerate case of the same expression, not a special branch.
        let ram = effective_ram_bytes(None, NonZeroUsize::MIN, || 93 * GB);
        assert_eq!(ram, 93 * GB);
    }

    #[test]
    fn effective_ram_divides_detected_ram_across_co_resident_networks() {
        let ram = effective_ram_bytes(None, NonZeroUsize::new(2).unwrap(), || 93 * GB);
        assert_eq!(ram, 93 * GB / 2);
    }

    #[test]
    fn effective_ram_zero_override_falls_back_to_divided_detection() {
        let ram = effective_ram_bytes(Some(0), NonZeroUsize::new(2).unwrap(), || 64 * GB);
        assert_eq!(ram, 32 * GB);
    }

    #[test]
    fn detect_system_resources_divides_detected_ram_by_the_configured_network_count() {
        // Guards the wiring, not the arithmetic: the `effective_ram_*` tests call the
        // helper directly, so they stay green even if the call site stops forwarding
        // `runtime_config.network_count` and passes a literal 1. That single argument
        // is all that stands between this fix and being silently inert, so pin it
        // through the real (private) entry point that the store actually opens with.
        //
        // Host-independent: both calls detect the same RAM R in this process, and
        // (R / 1) / 2 == R / 2 exactly for u64 — same floor, no truncation drift.
        let one = detect_system_resources(StoreRuntimeConfig {
            network_count: NonZeroUsize::MIN,
            ..Default::default()
        })
        .0;
        let two = detect_system_resources(StoreRuntimeConfig {
            network_count: NonZeroUsize::new(2).unwrap(),
            ..Default::default()
        })
        .0;
        assert_eq!(two, one / 2);
    }

    #[test]
    fn store_runtime_config_defaults_to_one_network() {
        // A default-constructed config is the standalone single-network opener, so it
        // must resolve to the full detected RAM rather than a fraction of it.
        assert_eq!(
            StoreRuntimeConfig::default().network_count,
            NonZeroUsize::MIN
        );
    }

    #[test]
    fn network_count_deserializes_to_one_when_absent() {
        // Guards the serde attribute itself, which the Default-impl test above cannot:
        // without it the field is REQUIRED and this payload — the shape every opener
        // predating the field still sends — fails to parse outright.
        let cfg: StoreRuntimeConfig = serde_json::from_str(
            r#"{"memory_budget_gb":null,"direct_io_reads":true,"vector_memtable":false}"#,
        )
        .unwrap();
        assert_eq!(cfg.network_count, NonZeroUsize::MIN);
    }

    #[test]
    fn store_runtime_config_rejects_a_zero_network_count() {
        // 0 must be unrepresentable, not clamped: a clamped 0 resolves to the FULL
        // detected RAM per network, which is the over-commit this division prevents.
        // NonZeroUsize makes serde reject it at parse time, with no runtime guard.
        let err = serde_json::from_str::<StoreRuntimeConfig>(
            r#"{"memory_budget_gb":null,"direct_io_reads":true,"vector_memtable":false,"network_count":0}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("zero"), "got: {err}");
    }

    #[test]
    fn configured_options_shares_one_budget_across_opens_in_a_process() {
        // RocksDB budgets are per-process. Minting a fresh Cache/WBM per open gave a
        // process that opens two stores (domain + append-only) twice its intended
        // budget, and N co-resident networks N x the host -- the over-commit that got
        // bulk sync OOM-killed. Two opens must hand back the same shared objects.
        //
        // The two profiles deliberately differ: if the pair were re-minted per open,
        // the handles would report different buffer sizes. This asserts only that the
        // two handles AGREE, never an absolute size -- another test in this process may
        // have initialized SHARED_BUDGET first, which is exactly the sharing intended.
        // (The Cache is built in the same get_or_init closure, so it shares by
        // construction; rocksdb exposes no Cache capacity getter to assert on.)
        let profile_a = MemoryProfile::compute(64 * GB, 4, 8, false);
        let profile_b = MemoryProfile::compute(8 * GB, 4, 8, false);
        assert_ne!(
            profile_a.wbm_normal_bytes, profile_b.wbm_normal_bytes,
            "profiles must differ or this test proves nothing"
        );

        let (_, _, wbm_a) =
            CkbadgerStore::configured_options(&profile_a, StoreRuntimeConfig::default());
        let (_, _, wbm_b) =
            CkbadgerStore::configured_options(&profile_b, StoreRuntimeConfig::default());

        // Prove wbm_a and wbm_b are handles to the SAME shared WriteBufferManager.
        //
        // The soundest proof is pointer identity, but rocksdb 0.24's
        // `WriteBufferManager(pub(crate) Arc<WriteBufferManagerWrapper>)` keeps both
        // the Arc and the wrapper's `NonNull` pointer behind `pub(crate)` fields —
        // unreachable from this crate without forking rocksdb — so we prove identity
        // by observed buffer size instead.
        //
        // Robustness to concurrent mutation: sibling tests in this binary call
        // `set_buffer_size` on the shared WBM (set_bulk_sync_compaction_options /
        // apply_normal_compaction_options), so a single a-vs-b read can disagree
        // transiently when a mutation lands between the two reads (the flake this
        // guards against). But profile_a and profile_b differ (assert_ne above), so in
        // the BROKEN mint-per-open path wbm_a and wbm_b are DISTINCT objects with
        // distinct, stable sizes that can never be equal; in the shared path they
        // alias one object and read equal whenever no mutation straddles the pair.
        // Hence observing equality even once proves sharing: retry a bounded number of
        // times to skip transient mutation windows, and fail if they never agree (the
        // broken path never agrees, so this still fails fast on a regression).
        let shares_one_wbm = (0..256).any(|_| wbm_a.get_buffer_size() == wbm_b.get_buffer_size());
        assert!(
            shares_one_wbm,
            "configured_options returned two different WriteBufferManager objects; \
             the per-process SHARED_BUDGET is not being shared across opens"
        );
    }

    /// Every CF's effective RocksDB options and write policy, as one line each:
    /// `name policy live(tier,n) compression compaction bulk(n,buffer)`. Tier
    /// and buffer are symbols of the store's own `MemoryProfile`, so the table
    /// does not depend on the host's RAM.
    fn cf_option_snapshot() -> Vec<String> {
        fn symbol(bytes: &str, profile: &MemoryProfile) -> &'static str {
            let bytes: usize = bytes.parse().unwrap();
            if bytes == profile.write_buffer_mega_bytes {
                "mega"
            } else if bytes == profile.write_buffer_high_bytes {
                "high"
            } else if bytes == profile.write_buffer_low_bytes {
                "low"
            } else if bytes == profile.write_buffer_hot_cf_bytes {
                "hot"
            } else {
                panic!("write buffer {bytes} is none of the profile's tiers")
            }
        }
        let mut lines = Vec::new();
        for (network, cfs) in [(false, ALL_CFS), (true, NETWORK_CFS)] {
            let dir = TempDir::new().unwrap();
            let store = if network {
                CkbadgerStore::open_network(dir.path()).unwrap()
            } else {
                CkbadgerStore::open_test_unified(dir.path()).unwrap()
            };
            let profile = store.memory_profile.clone();
            let tiers = [
                profile.write_buffer_mega_bytes,
                profile.write_buffer_high_bytes,
                profile.write_buffer_low_bytes,
                profile.write_buffer_hot_cf_bytes,
            ];
            for (i, a) in tiers.iter().enumerate() {
                for b in &tiers[i + 1..] {
                    assert_ne!(a, b, "profile tiers must be distinct for the snapshot");
                }
            }
            let live: Vec<_> = cfs
                .iter()
                .map(|cf| persisted_cf_options(dir.path(), cf))
                .collect();
            store.set_bulk_sync_compaction_options();
            for (cf, live) in cfs.iter().zip(live) {
                let bulk = persisted_cf_options(dir.path(), cf);
                let bulk_options = CkbadgerStore::bulk_cf_options(cf, &profile)
                    .into_iter()
                    .map(|(name, value)| {
                        let value = if value == profile.bulk_max_bytes_for_level_base.to_string() {
                            "bulk_level_base".to_string()
                        } else if value == profile.bulk_target_file_size_base.to_string() {
                            "bulk_file_size".to_string()
                        } else if value == profile.write_buffer_hot_cf_bytes.to_string() {
                            "hot".to_string()
                        } else {
                            value
                        };
                        format!("{name}={value}")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                lines.push(format!(
                    "{cf} {:?} live({},{}) {} {} bulk({},{}) [{bulk_options}]",
                    cf_write_policy(cf),
                    symbol(&live["write_buffer_size"], &profile),
                    live["max_write_buffer_number"],
                    live["compression"],
                    live["compaction_style"],
                    bulk["max_write_buffer_number"],
                    symbol(&bulk["write_buffer_size"], &profile),
                ));
            }
        }
        lines
    }

    /// `CF_SPECS` is the one CF table: every name once, and every list and
    /// lookup in this module agrees with it.
    #[test]
    fn cf_specs_name_every_cf_exactly_once_and_projections_agree() {
        let mut names: Vec<&str> = CF_SPECS.iter().map(|spec| spec.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "a CF name appears twice in CF_SPECS");
        assert_eq!(CF_SPECS.len(), 78);
        assert_eq!(DOMAIN_CFS.len(), 74);
        assert_eq!(APPEND_CFS, &[CF_CELLS]);
        assert_eq!(NETWORK_CFS, &[CF_NET_NODES, CF_NET_STATS, CF_NET_CRAWL]);
        assert_eq!(ALL_CFS.len(), DOMAIN_CFS.len() + APPEND_CFS.len());
        for spec in CF_SPECS {
            let in_domain = DOMAIN_CFS.contains(&spec.name);
            let in_append = APPEND_CFS.contains(&spec.name);
            let in_network = NETWORK_CFS.contains(&spec.name);
            assert_eq!(
                (in_domain, in_append, in_network),
                (
                    spec.store == CfStore::Domain,
                    spec.store == CfStore::AppendOnly,
                    spec.store == CfStore::Network
                ),
                "{}: store lists disagree with its spec",
                spec.name
            );
            assert_eq!(ALL_CFS.contains(&spec.name), !in_network, "{}", spec.name);
            assert_eq!(
                cf_write_policy(spec.name),
                spec.write_policy,
                "{}",
                spec.name
            );
            assert_eq!(cf_spec(spec.name), Some(spec));
        }
        assert_eq!(cf_spec("no_such_cf"), None);
    }

    /// Bulk finalize's two flush sets split the domain store (D-3).
    /// `FINALIZE_WRITTEN_CFS` is every domain CF finalize phases 2-11 may
    /// write: the two write policies the bounded finalize materializer
    /// accepts. `BATCH_LOOP_ONLY_CFS` is every other domain CF; only the
    /// batch loop writes those, so finalize flushes them as soon as the
    /// flush channel has committed its last batch.
    #[test]
    fn batch_loop_only_cfs_are_the_domain_cfs_outside_finalize_written_cfs() {
        let finalize_policies: Vec<&str> = DOMAIN_CFS
            .iter()
            .copied()
            .filter(|name| {
                matches!(
                    cf_write_policy(name),
                    CfWritePolicy::SealedAggregate | CfWritePolicy::FinalSnapshot
                )
            })
            .collect();
        assert_eq!(FINALIZE_WRITTEN_CFS, finalize_policies.as_slice());

        let domain_minus_finalize: Vec<&str> = DOMAIN_CFS
            .iter()
            .copied()
            .filter(|name| !FINALIZE_WRITTEN_CFS.contains(name))
            .collect();
        assert_eq!(BATCH_LOOP_ONLY_CFS, domain_minus_finalize.as_slice());
        assert_eq!(
            FINALIZE_WRITTEN_CFS.len() + BATCH_LOOP_ONLY_CFS.len(),
            DOMAIN_CFS.len()
        );
        assert_eq!(
            (FINALIZE_WRITTEN_CFS.len(), BATCH_LOOP_ONLY_CFS.len()),
            (52, 22)
        );
        for name in BATCH_LOOP_ONLY_CFS {
            assert!(
                matches!(
                    cf_write_policy(name),
                    CfWritePolicy::AppendOnly | CfWritePolicy::BulkDisabled
                ),
                "{name}"
            );
        }
        for name in APPEND_CFS.iter().chain(NETWORK_CFS) {
            assert!(!FINALIZE_WRITTEN_CFS.contains(name), "{name}");
            assert!(!BATCH_LOOP_ONLY_CFS.contains(name), "{name}");
        }
        // Lane 0 §7.1 classed this history feed with the finalize-written
        // CFs; its policy (Class A) keeps it out of finalize.
        assert!(BATCH_LOOP_ONLY_CFS.contains(&CF_OBJECT_COLLECTION_ACTIVITIES));
        // Phase 11's tracker rows and cursors, written outside the
        // materializer, and the progress records in `sync_meta`.
        for name in [
            CF_HODL_CAPACITY_BY_DATE,
            CF_CELL_DIST_COHORT_BY_MONTH,
            CF_SYNC_META,
        ] {
            assert!(FINALIZE_WRITTEN_CFS.contains(&name), "{name}");
        }
    }

    /// C-5: `docs/STORE_SCHEMA.md`'s "Column Families" table is the reference
    /// for every column family. It lists exactly the CFs of `CF_SPECS`, each
    /// once, each marked with its spec's store (`**(append-only store)**`,
    /// `**(network store)**`, unmarked = domain); its title states the
    /// per-store counts of `CF_SPECS`; and every fixed-width key states the
    /// total width the encoder writes (`= NB` after a key of several
    /// components, `(NB` on a one-component key).
    #[test]
    fn store_schema_cf_table_matches_cf_specs() {
        const DOC: &str = include_str!("../../../docs/STORE_SCHEMA.md");
        let rows = store_schema_cf_table(DOC);

        let wrong_store: Vec<String> = rows
            .iter()
            .filter_map(|(name, store, _)| {
                let spec = cf_spec(name)?;
                (spec.store != *store)
                    .then(|| format!("{name}: marked {store:?}, CF_SPECS says {:?}", spec.store))
            })
            .collect();
        assert!(
            wrong_store.is_empty(),
            "STORE_SCHEMA.md store markers differ from CF_SPECS:\n{}",
            wrong_store.join("\n")
        );

        let mut doc_names: Vec<&str> = rows.iter().map(|(name, _, _)| *name).collect();
        doc_names.sort_unstable();
        let listed = doc_names.len();
        doc_names.dedup();
        assert_eq!(
            doc_names.len(),
            listed,
            "a CF is listed twice in STORE_SCHEMA.md"
        );
        let mut spec_names: Vec<&str> = CF_SPECS.iter().map(|spec| spec.name).collect();
        spec_names.sort_unstable();
        let missing: Vec<&str> = spec_names
            .iter()
            .filter(|name| !doc_names.contains(name))
            .copied()
            .collect();
        let unknown: Vec<&str> = doc_names
            .iter()
            .filter(|name| !spec_names.contains(name))
            .copied()
            .collect();
        assert!(
            missing.is_empty() && unknown.is_empty(),
            "STORE_SCHEMA.md CF table vs CF_SPECS: missing {missing:?}, not a CF {unknown:?}"
        );

        let count = |store: CfStore| CF_SPECS.iter().filter(|spec| spec.store == store).count();
        let title = format!(
            "# ckbadger-store Column Families ({} total: {} domain + {} append-only + {} network)",
            CF_SPECS.len(),
            count(CfStore::Domain),
            count(CfStore::AppendOnly),
            count(CfStore::Network)
        );
        assert_eq!(DOC.lines().next(), Some(title.as_str()));

        let fixed_widths: &[(&str, usize)] = &[
            (CF_CELLS, keys::OUTPOINT_KEY_SIZE),
            (CF_LIVE_CELLS, keys::OUTPOINT_KEY_SIZE),
            (CF_CONSUMED_CELLS, keys::OUTPOINT_KEY_SIZE),
            (CF_REORG_UNDO_LOG_BY_BLOCK, keys::REORG_UNDO_LOG_KEY_SIZE),
            (CF_BLOCK_HEADERS, keys::BLOCK_NUM_KEY_SIZE),
            (CF_CELL_BY_LOCK, keys::CELL_INDEX_KEY_SIZE),
            (CF_CELL_BY_TYPE, keys::CELL_INDEX_KEY_SIZE),
            (CF_CELL_BY_DATA_HASH, keys::CELL_INDEX_KEY_SIZE),
            (CF_CELL_BY_LOCK_CODE, keys::CELL_CODE_INDEX_KEY_SIZE),
            (CF_CELL_BY_TYPE_CODE, keys::CELL_CODE_INDEX_KEY_SIZE),
            (CF_ADDR_TXS, keys::ADDR_TX_KEY_SIZE),
            (CF_ADDR_TXS_BY_PREFIX, keys::ADDR_TX_BY_PREFIX_KEY_SIZE),
            (CF_DAO_DEPOSITS, keys::OUTPOINT_KEY_SIZE),
            (CF_DAO_BY_WITHDRAW_TX, keys::OUTPOINT_KEY_SIZE),
            (CF_DAO_BY_BLOCK, keys::DAO_BY_BLOCK_KEY_SIZE),
            (CF_DAO_BY_LOCK_BLOCK, keys::DAO_BY_LOCK_BLOCK_KEY_SIZE),
            (CF_DAO_BY_STATUS_BLOCK, keys::DAO_BY_STATUS_BLOCK_KEY_SIZE),
            (
                CF_TOKEN_HOLDERS_BY_BALANCE,
                keys::TOKEN_HOLDER_BALANCE_KEY_SIZE,
            ),
            (CF_ADDR_TOKENS_BY_BALANCE, keys::ADDR_TOKEN_BALANCE_KEY_SIZE),
            (CF_TOKEN_TRANSFERS, keys::TOKEN_TRANSFER_KEY_SIZE),
            (
                CF_OBJECT_COLLECTION_ACTIVITIES,
                keys::OBJECT_COLLECTION_ACTIVITY_KEY_SIZE,
            ),
            (
                CF_IDENTITY_COLLECTION_ACTIVITIES,
                keys::OBJECT_COLLECTION_ACTIVITY_KEY_SIZE,
            ),
            (
                CF_COLLECTION_ACTIVITY_DAILY,
                keys::COLLECTION_ACTIVITY_DAILY_KEY_SIZE,
            ),
            (
                CF_DOTCELL_NAME_BY_OWNER,
                keys::DOTCELL_NAME_BY_OWNER_KEY_SIZE,
            ),
            (CF_TX_ACTIONS, keys::TX_ACTIONS_KEY_SIZE),
            (CF_TX_ACTIONS_BY_BUCKET, keys::TX_ACTIONS_BY_BUCKET_KEY_SIZE),
            (CF_FIBER_CHANNELS, keys::FIBER_CHANNEL_KEY_SIZE),
            (CF_ADDR_FIBER_CHANNELS, keys::ADDR_FIBER_CHANNEL_KEY_SIZE),
            (CF_REORG_EVENTS, keys::REORG_EVENT_KEY_SIZE),
            (CF_TX_INDEX, keys::encode_tx_slot_key(0, 0).len()),
            (CF_TX_GROUP_CYCLES, keys::encode_tx_slot_key(0, 0).len()),
            (CF_OBJECT_OUTPOINTS, keys::SPORE_OUTPOINT_KEY_SIZE),
        ];
        let wrong: Vec<String> = fixed_widths
            .iter()
            .filter_map(|(cf, width)| {
                let (_, _, key) = rows
                    .iter()
                    .find(|(name, _, _)| name == cf)
                    .unwrap_or_else(|| panic!("{cf} is not in the table"));
                (!states_total(key, *width)).then(|| format!("{cf}: {width}B, table says `{key}`"))
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "STORE_SCHEMA.md key widths differ from the encoders:\n{}",
            wrong.join("\n")
        );
    }

    /// `(cf name, store marker, key cell)` of every row of STORE_SCHEMA.md's
    /// "Column Families" table.
    fn store_schema_cf_table(doc: &str) -> Vec<(&str, CfStore, &str)> {
        let mut lines = doc
            .lines()
            .skip_while(|line| *line != "## Column Families")
            .skip(1)
            .skip_while(|line| !line.starts_with('|'));
        let header = lines.next().expect("the Column Families table");
        assert!(header.starts_with("| Column Family"), "{header}");
        lines
            .skip(1)
            .take_while(|line| line.starts_with('|'))
            .map(|line| {
                let cells: Vec<&str> = line.split(" | ").collect();
                let name_cell = cells[0].trim_start_matches('|').trim();
                let name = name_cell
                    .strip_prefix('`')
                    .and_then(|rest| rest.split('`').next())
                    .unwrap_or_else(|| panic!("a CF table row without a `name`: {line}"));
                let store = if name_cell.contains("**(append-only store)**") {
                    CfStore::AppendOnly
                } else if name_cell.contains("**(network store)**") {
                    CfStore::Network
                } else {
                    CfStore::Domain
                };
                (name, store, cells.get(1).map_or("", |key| key.trim()))
            })
            .collect()
    }

    /// C-5: the rollback-class table of `docs/prompts/REORG_HANDLING.md`
    /// names exactly `CF_SPECS`'s `RangeByBlock` and `Never` CFs (`Journal`
    /// is "every domain CF not listed below").
    #[test]
    fn reorg_handling_class_table_matches_cf_specs() {
        const DOC: &str = include_str!("../../../docs/prompts/REORG_HANDLING.md");
        for (class, label) in [
            (RollbackClass::RangeByBlock, "`RangeByBlock`"),
            (RollbackClass::Never, "`Never`"),
        ] {
            let rows: Vec<&str> = DOC
                .lines()
                .filter(|line| {
                    line.strip_prefix('|')
                        .is_some_and(|rest| rest.trim_start().starts_with(label))
                })
                .collect();
            assert_eq!(rows.len(), 1, "one {label} row in the class table");
            let cfs_cell = rows[0].split(" | ").nth(1).expect("the CF cell");
            let mut listed: Vec<&str> = cfs_cell.split('`').skip(1).step_by(2).collect();
            listed.sort_unstable();
            let mut specs: Vec<&str> = CF_SPECS
                .iter()
                .filter(|spec| spec.rollback_class == class)
                .map(|spec| spec.name)
                .collect();
            specs.sort_unstable();
            assert_eq!(listed, specs, "REORG_HANDLING.md {label} row vs CF_SPECS");
        }
    }

    /// C-5: every tracked doc that states how many column families there are
    /// states `CF_SPECS`'s counts: "N domain" is the domain store's count and
    /// "N CFs total" / "N CFs overall" the count of all three stores.
    #[test]
    fn tracked_docs_state_the_cf_specs_counts() {
        const DOCS: &[(&str, &str)] = &[
            ("CLAUDE.md", include_str!("../../../CLAUDE.md")),
            ("README.md", include_str!("../../../README.md")),
            (
                "docs/STORE_SCHEMA.md",
                include_str!("../../../docs/STORE_SCHEMA.md"),
            ),
            (
                "docs/INDEXER_PIPELINE.md",
                include_str!("../../../docs/INDEXER_PIPELINE.md"),
            ),
            (
                "docs/ARCHITECTURE_MAP.md",
                include_str!("../../../docs/ARCHITECTURE_MAP.md"),
            ),
        ];
        let domain = CF_SPECS
            .iter()
            .filter(|spec| spec.store == CfStore::Domain)
            .count();
        let total = CF_SPECS.len();
        let mut wrong = Vec::new();
        let mut stated = 0;
        for (path, doc) in DOCS {
            for (phrase, expected) in [
                (" domain", domain),
                (" CFs total", total),
                (" CFs overall", total),
            ] {
                for count in counts_stated_before(doc, phrase) {
                    stated += 1;
                    if count != expected {
                        wrong.push(format!(
                            "{path}: \"{count}{phrase}\", CF_SPECS has {expected}"
                        ));
                    }
                }
            }
        }
        assert!(stated > 0, "no doc states a CF count any more");
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// The numbers written directly before `phrase` in `doc` ("74 domain").
    fn counts_stated_before(doc: &str, phrase: &str) -> Vec<usize> {
        doc.match_indices(phrase)
            .filter_map(|(at, _)| {
                let digits: String = doc[..at]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                (!digits.is_empty()).then(|| digits.chars().rev().collect::<String>())
            })
            .map(|digits| digits.parse().expect("a count"))
            .collect()
    }

    #[test]
    fn store_schema_table_helpers_read_names_key_cells_and_widths() {
        let doc = "# t\n\n## Column Families\n\n| Column Family | Key | Value |\n| --- | --- | --- |\n\
                   | `cells` **(append-only store)** | outpoint (34B) | payload |\n\
                   | `net_nodes` **(network store)** | peer_id | node |\n\
                   | `addr_txs` | lock(32) + desc(12) = 44B | v |\n\nafter\n| `not_a_row` | x | y |\n";
        assert_eq!(
            store_schema_cf_table(doc),
            vec![
                ("cells", CfStore::AppendOnly, "outpoint (34B)"),
                ("net_nodes", CfStore::Network, "peer_id"),
                ("addr_txs", CfStore::Domain, "lock(32) + desc(12) = 44B")
            ]
        );
        assert!(states_total("outpoint (34B)", 34));
        assert!(!states_total("key (134B)", 34));
        assert!(!states_total("block_number + tx_index", 12));
        assert!(states_total("lock(32) + desc(12) = 44B", 44));
        // A component's width that equals the total is not the total.
        assert!(!states_total(
            "hash (32B) + block (8B BE) + outpoint (74B)",
            74
        ));
        assert!(states_total(
            "hash (32B) + block (8B BE) + outpoint (34B) = 74B",
            74
        ));
        // Delta review N8: the components must add up to the stated total,
        // and every one must state its width.
        assert!(!states_total(
            "hash (32B) + block (8B BE) + outpoint (35B) = 74B",
            74
        ));
        assert!(!states_total(
            "hash (32B) + block + outpoint (34B) = 74B",
            74
        ));
        assert!(states_total(
            "block_number (8B BE) + tx_index (4B BE) = 12B, the canonical slot",
            12
        ));
        assert!(states_total(
            "collection_id (32B padded) + day YYYYMMDD (4B BE) = 36B",
            36
        ));
        assert_eq!(component_width("blake2b(cell_data) (32B)"), Some(32));
        assert_eq!(component_width("tx_idx_desc(4)"), Some(4));
        assert_eq!(component_width("block"), None);
        assert_eq!(
            counts_stated_before("74 domain + 1 append-only; the 68 domain CFs", " domain"),
            vec![74, 68]
        );
        assert_eq!(
            counts_stated_before("a domain", " domain"),
            Vec::<usize>::new()
        );
    }

    /// Whether the key cell `cell` states the key's total width `width`: a
    /// key of several components (`a + b`) as `= NB`, with every component's
    /// width stated (`name (NB…)` or `name(N)`) and summing to it; a
    /// one-component key as `(NB` on its one component — never a component's
    /// width that happens to equal the total.
    fn states_total(cell: &str, width: usize) -> bool {
        if !cell.contains(" + ") {
            return cell.contains(&format!("({width}B"));
        }
        let Some((components, _)) = cell.split_once(&format!(" = {width}B")) else {
            return false;
        };
        let widths: Option<Vec<usize>> = components.split(" + ").map(component_width).collect();
        widths.is_some_and(|widths| widths.iter().sum::<usize>() == width)
    }

    /// The width a key component states in its last parenthesis: `(32B)`,
    /// `(8B BE)`, `(32B padded)` or `(32)`.
    fn component_width(component: &str) -> Option<usize> {
        let (_, inside) = component.trim_end().strip_suffix(')')?.rsplit_once('(')?;
        let digits: String = inside.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest = &inside[digits.len()..];
        (rest.is_empty() || rest.starts_with('B'))
            .then(|| digits.parse().ok())
            .flatten()
    }

    /// Every OUTSIDE_JOURNAL entry names a `Journal`-class domain CF (the
    /// table is about keys the journal would otherwise own), and no two
    /// entries cover the same key.
    #[test]
    fn outside_journal_entries_name_journal_class_cfs_once() {
        for entry in OUTSIDE_JOURNAL {
            assert_eq!(
                cf_spec(entry.cf).map(|spec| spec.rollback_class),
                Some(RollbackClass::Journal),
                "{} {:?}",
                entry.cf,
                entry.key_prefix
            );
            assert!(!entry.owner.is_empty() && !entry.reason.is_empty());
            let overlapping = OUTSIDE_JOURNAL
                .iter()
                .filter(|other| {
                    other.cf == entry.cf
                        && (other.key_prefix.starts_with(entry.key_prefix)
                            || entry.key_prefix.starts_with(other.key_prefix))
                })
                .count();
            assert_eq!(
                overlapping, 1,
                "{} {:?} overlaps",
                entry.cf, entry.key_prefix
            );
        }
    }

    /// Under the journal discipline an unjournaled commit may write a
    /// `Journal`-class key only where OUTSIDE_JOURNAL covers it; the refusal
    /// writes nothing. Non-Journal CFs, bulk mode and a store without the
    /// discipline are not checked; raw `WriteBatch` commits are refused.
    #[test]
    fn journal_discipline_refuses_unlisted_unjournaled_writes() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        // Without the discipline: a fixture store, written freely.
        store
            .put_cf(store.cf_addr_balance(), b"fixture", b"1")
            .unwrap();
        store.enforce_journal_discipline();

        let mut batch = crate::StoreBatch::new(&store);
        batch.put_sync_meta(keys::sync_meta_keys::RUNTIME_STATUS, b"listed");
        batch
            .put_raw_cf_by_name(CF_ADDR_BALANCE, b"unlisted", b"2")
            .unwrap();
        let error = batch.commit().unwrap_err().to_string();
        assert!(error.contains("cf=addr_balance"), "{error}");
        assert!(
            !error.contains("  "),
            "the message reads as one line: {error}"
        );
        assert_eq!(
            store
                .get_cf(store.cf_sync_meta(), keys::sync_meta_keys::RUNTIME_STATUS)
                .unwrap(),
            None,
            "a refused batch writes nothing"
        );
        let error = store
            .put_cf(store.cf_addr_balance(), b"unlisted", b"2")
            .unwrap_err()
            .to_string();
        assert!(error.contains("OUTSIDE_JOURNAL"), "{error}");
        assert!(store
            .delete_cf(store.cf_addr_balance(), b"fixture")
            .is_err());

        // Listed keys, and a RangeByBlock CF, pass.
        let mut batch = crate::StoreBatch::new(&store);
        batch.put_sync_meta(keys::sync_meta_keys::RUNTIME_STATUS, b"listed");
        batch
            .put_raw_cf_by_name(CF_TX_INDEX, b"tx", b"row")
            .unwrap();
        batch.commit().unwrap();
        store
            .put_cf(
                store.cf_sync_meta(),
                keys::sync_meta_keys::SYNC_PROGRESS,
                b"p",
            )
            .unwrap();

        // A raw WriteBatch cannot be checked, so it is refused.
        let error = store
            .write_batch(WriteBatch::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains("raw WriteBatch"), "{error}");
        assert!(
            !error.contains("  "),
            "the message reads as one line: {error}"
        );

        // Bulk sync is not checked.
        store.set_bulk_sync_mode(true);
        store
            .put_cf(store.cf_addr_balance(), b"bulk", b"3")
            .unwrap();
        store
            .write_batch_no_wal_bulk(WriteBatch::default())
            .unwrap();
        store.set_bulk_sync_mode(false);
        assert!(store
            .write_batch_no_wal_bulk(WriteBatch::default())
            .is_err());
    }

    /// Lane B carry-over 4(c): a whole-CF `OUTSIDE_JOURNAL` entry admits only
    /// the writer it names. `tokens` and `dao_live_agg` are journaled by live
    /// batches and written outside the journal only by label import and the
    /// derive scheduler respectively; any other unjournaled write to them —
    /// an untagged batch, another writer's batch, a direct write outside the
    /// writer's scope or from another thread — is refused.
    #[test]
    fn whole_cf_outside_journal_entries_admit_only_their_writer() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store.enforce_journal_discipline();
        let refused = |result: anyhow::Result<()>, what: &str| {
            let error = result.expect_err(what).to_string();
            assert!(error.contains("OUTSIDE_JOURNAL"), "{what}: {error}");
        };

        // Untagged: refused for both whole-CF entries.
        let mut batch = crate::StoreBatch::new(&store);
        batch.put_raw_cf_by_name(CF_TOKENS, b"t", b"1").unwrap();
        refused(batch.commit(), "an untagged batch writing tokens");
        let mut batch = crate::StoreBatch::new(&store);
        batch
            .put_raw_cf_by_name(CF_DAO_LIVE_AGG, b"d", b"1")
            .unwrap();
        refused(batch.commit(), "an untagged batch writing dao_live_agg");
        refused(
            store.put_cf(store.cf_tokens(), b"t", b"1"),
            "a direct write to tokens outside the label import scope",
        );

        // The named writer passes; the other writer does not.
        let mut batch = crate::StoreBatch::outside_journal(&store, OutsideWriter::Derive);
        batch
            .put_raw_cf_by_name(CF_DAO_LIVE_AGG, b"d", b"1")
            .unwrap();
        batch
            .delete_range_raw_cf_by_name(CF_OBJECT_COLLECTION_ACTIVITY_COUNT, b"", &[0xff])
            .unwrap();
        batch.commit().unwrap();
        let mut batch = crate::StoreBatch::outside_journal(&store, OutsideWriter::Derive);
        batch.put_raw_cf_by_name(CF_TOKENS, b"t", b"1").unwrap();
        refused(batch.commit(), "the derive scheduler writing tokens");
        let mut batch = crate::StoreBatch::outside_journal(&store, OutsideWriter::LabelImport);
        batch
            .put_raw_cf_by_name(CF_DAO_LIVE_AGG, b"d", b"2")
            .unwrap();
        refused(batch.commit(), "label import writing dao_live_agg");

        // Direct writes inside the label import scope, on its thread.
        {
            let _scope = store
                .outside_journal_scope(OutsideWriter::LabelImport)
                .unwrap();
            store.put_cf(store.cf_tokens(), b"t", b"1").unwrap();
            store.put_cf(store.cf_script_info(), b"s", b"1").unwrap();
            refused(
                store.put_cf(store.cf_dao_live_agg(), b"d", b"3"),
                "label import's scope writing dao_live_agg",
            );
            std::thread::scope(|threads| {
                threads.spawn(|| {
                    refused(
                        store.put_cf(store.cf_tokens(), b"other", b"1"),
                        "another thread during the label import scope",
                    );
                });
            });
            assert!(
                store.outside_journal_scope(OutsideWriter::Derive).is_err(),
                "scopes do not nest"
            );
        }
        refused(
            store.put_cf(store.cf_tokens(), b"t", b"2"),
            "a direct write after the scope ended",
        );

        // The (cf, prefix) entries keep admitting any unjournaled writer.
        store
            .put_cf(
                store.cf_sync_meta(),
                keys::sync_meta_keys::SYNC_PROGRESS,
                b"p",
            )
            .unwrap();
    }

    /// Ruling 5: the two also-journaled `(cf, prefix)` entries admit only
    /// their one unjournaled writer — `live_cell_summary:current` the
    /// deep-fork record, `stats_dao` 0x08 the DAO tip summary.
    #[test]
    fn also_journaled_prefix_entries_admit_only_their_writer() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store.enforce_journal_discipline();
        let summary = keys::sync_meta_keys::LIVE_CELL_SUMMARY_CURRENT;
        let open_day = keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20261107");
        let write = |writer: Option<OutsideWriter>, cf: &str, key: &[u8]| {
            let mut batch = match writer {
                Some(writer) => crate::StoreBatch::outside_journal(&store, writer),
                None => crate::StoreBatch::new(&store),
            };
            batch.put_raw_cf_by_name(cf, key, b"v").unwrap();
            batch.commit()
        };
        for (writer, cf, key, admitted) in [
            (None, CF_SYNC_META, summary, false),
            (
                Some(OutsideWriter::DaoTipSummary),
                CF_SYNC_META,
                summary,
                false,
            ),
            (
                Some(OutsideWriter::DeepForkRecord),
                CF_SYNC_META,
                summary,
                true,
            ),
            (None, CF_STATS_DAO, open_day.as_slice(), false),
            (
                Some(OutsideWriter::DeepForkRecord),
                CF_STATS_DAO,
                open_day.as_slice(),
                false,
            ),
            (
                Some(OutsideWriter::DaoTipSummary),
                CF_STATS_DAO,
                open_day.as_slice(),
                true,
            ),
        ] {
            let result = write(writer, cf, key);
            assert_eq!(
                result.is_ok(),
                admitted,
                "{writer:?} writing {cf}:{}: {result:?}",
                String::from_utf8_lossy(key)
            );
        }
        // The DAO tip summary's never-journaled rows still admit any writer.
        let latest = keys::encode_stats_key(keys::STATS_PREFIX_DAO_LATEST_STATS, b"latest");
        write(None, CF_STATS_DAO, &latest).unwrap();
    }

    /// A never-journaled key written through a journaled batch goes straight
    /// into the commit, with no journal entry; an also-journaled one is
    /// journaled in its block.
    #[test]
    fn a_journaled_batch_writes_never_journaled_keys_outside_the_journal() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store.enforce_journal_discipline();
        crate::commit_journaled_block(&store, 3, |batch| {
            batch.put_sync_meta(keys::sync_meta_keys::RUNTIME_STATUS, b"telemetry");
            batch.put_sync_meta(keys::sync_meta_keys::LIVE_CELL_SUMMARY_CURRENT, b"summary");
        })
        .unwrap();
        let journaled: Vec<Vec<u8>> = store
            .journal_entries(3)
            .unwrap()
            .into_iter()
            .map(|entry| entry.key)
            .collect();
        assert_eq!(
            journaled,
            vec![keys::sync_meta_keys::LIVE_CELL_SUMMARY_CURRENT.to_vec()]
        );
        assert_eq!(
            store
                .get_cf(store.cf_sync_meta(), keys::sync_meta_keys::RUNTIME_STATUS)
                .unwrap(),
            Some(b"telemetry".to_vec())
        );
    }

    /// `RangeByBlock` only where the key starts with the block number;
    /// `cells`, `pending_proposals` (written out of band by the proposal
    /// cache) and the network store never roll back; every other domain CF,
    /// `dob_decoded` included (B-5h: the live writer applies a decode in a
    /// block's scope), is journaled.
    #[test]
    fn rollback_classes_follow_the_key_layout_rule() {
        for spec in CF_SPECS {
            let expected = match spec.name {
                CF_BLOCK_HEADERS
                | CF_TX_INDEX
                | CF_TX_ACTIONS
                | CF_REORG_UNDO_LOG_BY_BLOCK
                | CF_TX_GROUP_CYCLES => RollbackClass::RangeByBlock,
                CF_CELLS | CF_PENDING_PROPOSALS | CF_REORG_EVENTS | CF_NET_NODES | CF_NET_STATS
                | CF_NET_CRAWL => RollbackClass::Never,
                _ => RollbackClass::Journal,
            };
            assert_eq!(spec.rollback_class, expected, "{}", spec.name);
        }
    }

    /// B-1 pin: the `CfSpec` table must reproduce, for every CF, exactly the
    /// options and write policy the hand-maintained tables produced before it
    /// (main @8f8404da), plus Lane A's one measured change merged from main
    /// @b2409d4c: `cells` is lz4 (store refactor plan J3). Changing any value
    /// here is an option change, which needs its own paired bulk run. B-4
    /// adds two rows (`hodl_capacity_by_date`, `cell_dist_cohort_by_month`,
    /// the trackers' rows) and B-5c one (`tx_group_cycles`, the lazy cycles
    /// record moved out of `sync_meta`); C-4b three (`object_outpoints` and
    /// `object_outpoint_by_id`, the outpoint indexes moved out of
    /// `stats_spore` / `stats_mnft`, and `reorg_events`, the event history
    /// moved out of `sync_meta`); every other row is unchanged.
    #[test]
    fn cf_options_and_policies_match_the_pre_cfspec_snapshot() {
        let actual = cf_option_snapshot();
        let expected: Vec<String> = CF_OPTION_SNAPSHOT.iter().map(|s| s.to_string()).collect();
        if actual != expected {
            panic!(
                "CF option snapshot changed:\n{}",
                actual
                    .iter()
                    .map(|line| format!("    \"{line}\","))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    #[rustfmt::skip]
    const CF_OPTION_SNAPSHOT: &[&str] = &[
    "cells AppendOnly live(mega,4) kLZ4Compression kCompactionStyleUniversal bulk(12,hot) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size,write_buffer_size=hot]",
    "live_cells FinalSnapshot live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,hot) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size,write_buffer_size=hot]",
    "consumed_cells AppendOnly live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,hot) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size,write_buffer_size=hot]",
    "reorg_undo_log_by_block BulkDisabled live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,mega) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "block_headers AppendOnly live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "block_hash_index AppendOnly live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "cell_by_lock FinalSnapshot live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,hot) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size,write_buffer_size=hot]",
    "cell_by_type FinalSnapshot live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,mega) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "cell_by_lock_code FinalSnapshot live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,mega) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "cell_by_type_code FinalSnapshot live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,mega) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "cell_by_data_hash AppendOnly live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "tx_index AppendOnly live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,hot) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size,write_buffer_size=hot]",
    "tx_hash_map AppendOnly live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,mega) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "addr_balance FinalSnapshot live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,mega) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "addr_txs AppendOnly live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,mega) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "dao_deposits FinalSnapshot live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "dao_by_withdraw_tx FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "dao_by_block FinalSnapshot live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "dao_by_lock_block FinalSnapshot live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "dao_by_status_block FinalSnapshot live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "tokens FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "token_holders FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "token_holders_by_balance FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "addr_tokens_by_balance FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "spore_data FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "mnft_data FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "mnft_by_collection FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "identity_data FinalSnapshot live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "identity_by_collection FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "stats_chain SealedAggregate live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "stats_dao SealedAggregate live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "stats_hodl SealedAggregate live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "hodl_capacity_by_date FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "cell_dist_cohort_by_month FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "stats_script SealedAggregate live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "stats_token SealedAggregate live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "stats_spore SealedAggregate live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "stats_mnft SealedAggregate live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "stats_identity FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "script_info FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "script_versions FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "script_versions_by_label FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "script_families FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "script_versions_by_family FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "script_reference_info FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "script_reference_to_version FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "script_family_by_name FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "sync_meta FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "spore_by_cluster FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "token_transfers AppendOnly live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "activities AppendOnly live(mega,4) kLZ4Compression kCompactionStyleLevel bulk(12,hot) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=12,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size,write_buffer_size=hot]",
    "cluster_agg FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "mnft_collection_agg FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "object_collection_activities AppendOnly live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "identity_agg FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "identity_collection_activities AppendOnly live(high,4) kLZ4Compression kCompactionStyleLevel bulk(8,high) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=8,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "pending_proposals BulkDisabled live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "fiber_channels FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "fiber_channel_by_commitment FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "addr_fiber_channels FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "dob_decoded BulkDisabled live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "lock_scripts AppendOnly live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "addr_txs_by_prefix AppendOnly live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "addr_prefix_stats FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "dotcell_name_by_owner FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "dotcell_ring FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "tx_actions_by_bucket AppendOnly live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "collection_activity_daily SealedAggregate live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "dao_live_agg BulkDisabled live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "object_collection_activity_count BulkDisabled live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "bit_cell_by_account BulkDisabled live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "tx_group_cycles BulkDisabled live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "object_outpoints FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "object_outpoint_by_id FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "reorg_events BulkDisabled live(low,2) kLZ4Compression kCompactionStyleLevel bulk(6,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "net_nodes FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(2,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "net_stats FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(2,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    "net_crawl FinalSnapshot live(low,2) kLZ4Compression kCompactionStyleLevel bulk(2,low) [level0_slowdown_writes_trigger=96,level0_stop_writes_trigger=192,max_write_buffer_number=6,max_bytes_for_level_base=bulk_level_base,target_file_size_base=bulk_file_size]",
    ];
}
