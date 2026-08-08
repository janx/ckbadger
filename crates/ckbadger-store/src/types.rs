//! Value types for all column families.
//!
//! All types use `bincode` serialization for compact binary storage.

use std::collections::HashMap;
use std::ops::Deref;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ============================================
// Group A: Core cell data (ported from LiveCellStorage)
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveCellInfo {
    pub capacity: i64,
    pub lock_script_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub type_script_hash: Option<Vec<u8>>,
    pub type_code_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub type_hash_type: Option<i16>,
    #[serde(default)]
    pub type_args: Option<Vec<u8>>,
    pub data_size: i32,
    #[serde(default)]
    pub occupied_capacity: i64,
    #[serde(default)]
    pub udt_amount: Option<u128>,
    #[serde(default)]
    pub data_hash: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionedCellInfo {
    pub cell: LiveCellInfo,
    pub created_at_block: i64,
}

impl PositionedCellInfo {
    pub fn new(cell: LiveCellInfo, created_at_block: i64) -> Self {
        Self {
            cell,
            created_at_block,
        }
    }
}

impl Deref for PositionedCellInfo {
    type Target = LiveCellInfo;

    fn deref(&self) -> &Self::Target {
        &self.cell
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumedCellInfo {
    pub cell: LiveCellInfo,
    pub consumed_at_block: i64,
    pub consumed_by_tx: Option<Vec<u8>>,
    pub created_at_block: i64,
}

/// Persistent lock_hash -> script components mapping.
/// Written once per unique lock_hash, never deleted.
/// Survives cell consumption, enabling address resolution for spent locks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockScriptEntry {
    pub code_hash: Vec<u8>,
    pub hash_type: i16,
    pub args: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsumedCellMeta {
    pub created_at_block: i64,
    pub consumed_at_block: i64,
    pub consumed_by_tx: Option<Vec<u8>>,
}

/// Pre-computed value stored in CF_ADDR_TXS.
/// Encodes capacity change, transaction type, and the participant tag bitmask
/// (mirrors `ParticipantDelta::tags` for the same `(lock_hash, block, tx_idx, tx_hash)`),
/// so filtered scans of the addr_tx index can reject non-matching entries without
/// reading `CF_TX_ACTIONS`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddrTxValue {
    pub capacity_change: i64,
    pub flags: u8,
    pub tags: u16,
}

impl AddrTxValue {
    pub const TX_TYPE_RECEIVED: u8 = 0;
    pub const TX_TYPE_SENT: u8 = 1;
    pub const TX_TYPE_INTERNAL: u8 = 2;
    pub const TX_TYPE_TRANSFER: u8 = 3;

    pub fn new(capacity_change: i64, has_inputs: bool, has_outputs: bool, tags: u16) -> Self {
        let tx_type = match (has_inputs, has_outputs) {
            (true, true) => {
                if capacity_change > 0 {
                    Self::TX_TYPE_RECEIVED
                } else if capacity_change < 0 {
                    Self::TX_TYPE_SENT
                } else {
                    Self::TX_TYPE_INTERNAL
                }
            }
            (false, true) => Self::TX_TYPE_RECEIVED,
            (true, false) => Self::TX_TYPE_SENT,
            (false, false) => Self::TX_TYPE_TRANSFER,
        };
        Self {
            capacity_change,
            flags: tx_type,
            tags,
        }
    }

    pub fn tx_type_str(&self) -> &'static str {
        match self.flags {
            Self::TX_TYPE_RECEIVED => "received",
            Self::TX_TYPE_SENT => "sent",
            Self::TX_TYPE_INTERNAL => "internal",
            _ => "transfer",
        }
    }
}

/// Semantic tag bitmap constants for tagging transactions by protocol type.
pub mod semantic_tags {
    pub const PLAIN: u16 = 0;
    pub const DAO: u16 = 1 << 0;
    pub const SUDT: u16 = 1 << 1;
    pub const XUDT: u16 = 1 << 2;
    pub const DOTBIT: u16 = 1 << 3;
    pub const MNFT: u16 = 1 << 4;
    pub const SPORE: u16 = 1 << 5;
    pub const CLUSTER: u16 = 1 << 6;
    pub const BIT_CELL: u16 = 1 << 7;
    pub const DID_CKB: u16 = 1 << 8;
}

/// Aggregated cell statistics for a token.
#[derive(Debug, Clone, Default)]
pub struct TokenCellStats {
    pub cells_count: i64,
    pub total_capacity: i128,
    pub total_used_capacity: i128,
}

impl ConsumedCellInfo {
    pub fn from_live_cell_info(
        info: &LiveCellInfo,
        consumed_at_block: i64,
        created_at_block: i64,
    ) -> Self {
        Self::from_live_cell_info_with_consumer(info, consumed_at_block, None, created_at_block)
    }

    pub fn from_live_cell_info_with_consumer(
        info: &LiveCellInfo,
        consumed_at_block: i64,
        consumed_by_tx: Option<&[u8]>,
        created_at_block: i64,
    ) -> Self {
        Self {
            cell: info.clone(),
            consumed_at_block,
            consumed_by_tx: consumed_by_tx.map(|tx| tx.to_vec()),
            created_at_block,
        }
    }

    pub fn to_positioned_cell_info(&self) -> PositionedCellInfo {
        PositionedCellInfo::new(self.cell.clone(), self.created_at_block)
    }
}

/// Decode consumed cell metadata from the canonical schema.
pub fn decode_consumed_cell_meta(value: &[u8]) -> Result<ConsumedCellMeta, bincode::Error> {
    bincode::deserialize::<ConsumedCellMeta>(value)
}

/// Decode created_at_block from the live cell marker value (8 bytes LE).
pub fn decode_live_cell_marker(value: &[u8]) -> Option<i64> {
    if value.len() == 8 {
        Some(i64::from_le_bytes(value.try_into().ok()?))
    } else {
        None
    }
}

/// Encode created_at_block for the live cell marker value.
pub fn encode_live_cell_marker(created_at_block: i64) -> [u8; 8] {
    created_at_block.to_le_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedBlockHeader {
    pub hash: Vec<u8>,
    pub parent_hash: Vec<u8>,
    pub timestamp: i64,
    pub epoch_number: i64,
    pub epoch_index: i32,
    pub epoch_length: i32,
    pub dao: Vec<u8>,
    pub transactions_count: i32,
    /// Number of uncle blocks included in this block.
    /// Needed by reorg rollback to correctly decrement
    /// `DailyBlockStats.total_uncles` for rolled-back blocks.
    #[serde(default)]
    pub uncles_count: i32,
    /// Number of proposal short ids in this block.
    #[serde(default)]
    pub proposals_count: i32,
    /// PoW compact target of this block's header. Source of truth for
    /// per-block difficulty (`compact_to_difficulty`).
    #[serde(default)]
    pub compact_target: u32,
    /// Script hash of the block's own miner, from the cellbase witness lock
    /// (RFC-0022 `CellbaseWitness.lock`). `None` only for the genesis block.
    /// Source of truth for miner attribution (daily miner stats + reorg
    /// rollback deltas). NOT the cellbase output lock (that pays the reward
    /// of the block 11 confirmations back).
    #[serde(default)]
    pub miner_lock_hash: Option<Vec<u8>>,
    /// Total cycles consumed by all transactions in this block.
    /// Written only by lazy cycles evaluation, not during bulk/live sync.
    #[serde(default)]
    pub cycles: Option<i64>,
}

// ============================================
// Group B: Transaction indexes
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxIndexEntry {
    pub is_cellbase: bool,
    pub timestamp: i64,
    pub inputs_count: i16,
    pub outputs_count: i16,
    pub fee: i64,
    pub tx_size: i32,
    pub cycles: Option<i64>,
    #[serde(default)]
    pub semantic_tags: u16,
}

// ============================================
// Group C: Address data
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AddressBalance {
    pub balance: i128,
    #[serde(default)]
    pub used_capacity: i128,
    pub live_cells_count: i32,
    pub total_cells_count: i64,
    pub txs_count: i64,
    pub first_seen_block: i64,
    pub first_seen_tx: Vec<u8>,
    pub last_activity_block: i64,
    pub last_activity_tx: Vec<u8>,
}

// ============================================
// Group D: DAO
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaoDepositCacheEntry {
    pub capacity: i64,
    /// Exact occupied capacity of the original deposit cell.
    ///
    /// DAO compensation accrues only on `capacity - occupied_capacity`; this
    /// varies with the lock script and must not be replaced by a fixed minimum.
    pub occupied_capacity: i64,
    pub deposit_block_number: i64,
    #[serde(default)]
    pub deposit_timestamp: i64,
    pub lock_script_hash: Vec<u8>,
    pub deposit_ar: i64,
    pub status: i16,
    pub withdraw_request_tx: Option<Vec<u8>>,
    #[serde(default)]
    pub withdraw_request_output_index: Option<i16>,
    pub withdraw_request_block: Option<i64>,
    pub withdraw_request_ar: Option<i64>,
    /// Exact occupied capacity of the phase-1 withdraw-request cell.
    ///
    /// RFC-0023 computes `counted_capacity` from the WITHDRAWING cell, not
    /// the original deposit cell. The DAO type script does not enforce lock
    /// preservation, so a withdraw request may carry a different lock than
    /// its deposit — and then the two occupied capacities differ. Set at
    /// phase-1; `None` while the deposit is still active (status 0).
    #[serde(default)]
    pub withdraw_request_occupied_capacity: Option<i64>,
    pub withdraw_block: Option<i64>,
    pub withdraw_tx: Option<Vec<u8>>,
    #[serde(default)]
    pub withdraw_to_output_index: Option<i16>,
    pub compensation: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DaoCompensationBreakdown {
    pub claimed: i128,
    pub unclaimed: i128,
    /// Compensation still accruing on status-0 deposits at the observation AR.
    ///
    /// Diagnostic only: this is the status-0 share of `unclaimed`, excluding
    /// interest already frozen on phase-1 withdraw-request cells. It must never
    /// be used to derive treasury — see [`dao_treasury_split`].
    pub active_unmade: i128,
}

impl DaoCompensationBreakdown {
    pub fn total(self) -> Option<i128> {
        self.claimed.checked_add(self.unclaimed)
    }

    /// Compensation frozen on phase-1 withdraw-request cells.
    ///
    /// This is the exact semantic overlap in CKB Explorer's legacy treasury
    /// partition: its treasury subtracts only [`Self::active_unmade`], while
    /// its DAO compensation includes all [`Self::unclaimed`].
    pub fn frozen_phase1(self) -> anyhow::Result<i128> {
        anyhow::ensure!(
            self.active_unmade >= 0,
            "negative active DAO compensation: active_unmade={}",
            self.active_unmade
        );
        anyhow::ensure!(
            self.unclaimed >= self.active_unmade,
            "active DAO compensation exceeds unclaimed: active_unmade={}, unclaimed={}",
            self.active_unmade,
            self.unclaimed
        );
        self.unclaimed
            .checked_sub(self.active_unmade)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "phase-1 DAO compensation subtraction overflow: unclaimed={}, active_unmade={}",
                    self.unclaimed,
                    self.active_unmade
                )
            })
    }

    /// Treasury portion of the protocol's secondary pool at this observation.
    pub fn treasury(self, secondary_pool: i128) -> anyhow::Result<i128> {
        dao_treasury_split(secondary_pool, self.unclaimed)
    }
}

/// Split the DAO secondary pool `S` into its treasury portion.
///
/// This is the single derivation of treasury; every write and read path routes
/// through it.
///
/// RFC-0023 keeps *all* unissued secondary issuance in `S`, and `S` shrinks only
/// when a phase-2 completion transaction subtracts `withdrawed_interests`. So at
/// any observation block `S` still holds the interest owed to both live
/// (status-0) deposits and phase-1 withdraw-request cells whose amount is
/// already frozen. Treasury is what remains after removing all of it:
/// `S - unclaimed`.
///
/// Subtracting only the status-0 share leaves phase-1 frozen interest inside
/// treasury while `cum_dao_compensation` (= claimed + unclaimed) already counts
/// it, double-counting those shannons across two buckets of one partition
/// (77,489,937.99 CKB on mainnet at the time this was found).
pub fn dao_treasury_split(secondary_pool: i128, unclaimed: i128) -> anyhow::Result<i128> {
    anyhow::ensure!(
        secondary_pool >= 0,
        "negative DAO secondary_pool: {secondary_pool}"
    );
    anyhow::ensure!(
        unclaimed >= 0,
        "negative unclaimed DAO compensation: {unclaimed}"
    );
    let treasury = secondary_pool.checked_sub(unclaimed).ok_or_else(|| {
        anyhow::anyhow!(
            "DAO treasury subtraction overflow: secondary_pool={secondary_pool}, unclaimed={unclaimed}"
        )
    })?;
    anyhow::ensure!(
        treasury >= 0,
        "unmade DAO interests exceed secondary_pool: secondary_pool={secondary_pool}, unclaimed={unclaimed}"
    );
    Ok(treasury)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoDailySnapshot {
    pub date: String,
    pub total_deposited: i128,
    pub depositors_count: i64,
    pub new_deposits: i64,
    pub withdrawals: i64,
    /// Cumulative DAO compensation claimed by completed phase-2 withdrawals.
    pub compensation: i128,
    /// Cumulative gross deposit amount (sum of all deposit capacities, never
    /// decreased by withdrawals). Used to compute daily gross deposits via
    /// deltas between consecutive snapshots.
    #[serde(default)]
    pub cumulative_deposit_amount: i128,
    /// C field from DAO header: total CKB issuance up to this date (shannons).
    #[serde(default)]
    pub total_issuance: i128,
    /// S field from DAO header: cumulative non-miner secondary issuance (shannons).
    #[serde(default)]
    pub secondary_pool: i128,
    /// U field from DAO header: total occupied capacity (shannons).
    #[serde(default)]
    pub occupied_capacity: i128,
    /// Cumulative secondary issuance to miners (shannons).
    #[serde(default)]
    pub cum_miner_secondary: i128,
    /// Cumulative secondary issuance to DAO depositors (shannons).
    #[serde(default)]
    pub cum_dao_compensation: i128,
    /// Cumulative secondary issuance to treasury (shannons).
    #[serde(default)]
    pub cum_treasury: i128,
    /// Unclaimed DAO compensation at end of day (shannons): interest accruing on
    /// live deposits plus interest already frozen on phase-1 withdraw-request
    /// cells. Treasury is derived from this via [`dao_treasury_split`].
    #[serde(default)]
    pub unclaimed_compensation: i128,
    /// Exact compensation frozen on phase-1 withdraw-request cells at end of
    /// day. Persisted so external legacy treasury partitions can be normalized
    /// without rescanning every DAO lifecycle entry at API read time.
    #[serde(default)]
    pub frozen_phase1_compensation: i128,
    /// Cumulative count of unique addresses that have ever deposited into DAO
    /// (only increments, never decremented on withdrawal).
    #[serde(default)]
    pub cumulative_depositors: i64,
    /// Number of unique addresses that made DAO deposits on this day
    /// (includes repeat depositors, not just first-timers).
    #[serde(default)]
    pub daily_depositor_addresses: i64,
    /// Protocol-level total deposited (includes status=1 cells still locked in DAO).
    #[serde(default)]
    pub protocol_deposited: Option<i128>,
}

/// Genesis economic baseline, derived once from block 0 and persisted.
/// All values are shannons. `virtual_occupied` = `burnt` × the network's
/// occupied ratio (CKB-Explorer accounting), NOT the genesis DAO U field.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenesisBaseline {
    pub total_issuance: i128,
    pub burnt: i128,
    pub virtual_occupied: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoLatestStatistics {
    pub tip_block_number: i64,
    pub total_deposited: i128,
    pub total_depositors: i32,
    pub active_deposits: i32,
    pub total_compensation_paid: i128,
    pub unclaimed_compensation: i128,
    pub average_deposit_days: String,
    pub estimated_apc: String,
    pub mining_reward: i128,
    pub deposit_compensation: i128,
    pub burnt: i128,
    /// Capacity of status=1 cells (withdraw-request pending completion).
    #[serde(default)]
    pub pending_withdrawal_capacity: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoTopDepositorEntry {
    pub lock_script_hash: Vec<u8>,
    pub total_capacity: i128,
    pub deposit_count: i32,
    #[serde(alias = "average_deposit_blocks")]
    pub average_deposit_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoTopDepositors {
    pub tip_block_number: i64,
    pub depositors: Vec<DaoTopDepositorEntry>,
}

// ============================================
// Group F: Tokens & Objects
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub type_code_hash: Vec<u8>,
    pub hash_type: u8,
    pub type_args: Vec<u8>,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<i32>,
    #[serde(default)]
    pub max_supply: Option<u128>,
    pub first_seen_block: i64,
    pub icon_url: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub transfers_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenTransferRecord {
    pub tx_hash: Vec<u8>,
    pub block_number: i64,
    pub from_lock_hash: Option<Vec<u8>>,
    pub to_lock_hash: Vec<u8>,
    pub amount: u128,
    pub is_mint: bool,
    pub is_burn: bool,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompositionTier {
    /// Content stored natively on CKB (inline cell data, ckbfs://, data: URIs).
    PureCkb,
    /// Content depends on both Bitcoin (btcfs://) and CKB storage.
    /// Objects are always CKB cells, so btcfs:// content is never "fully on Bitcoin" alone.
    BtcCkb,
    DecentralizedMixture,
    CentralizedMixture,
    #[default]
    Unknown,
}

impl CompositionTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            CompositionTier::PureCkb => "pure_ckb",
            CompositionTier::BtcCkb => "btc_ckb",
            CompositionTier::DecentralizedMixture => "decentralized_mixture",
            CompositionTier::CentralizedMixture => "centralized_mixture",
            CompositionTier::Unknown => "unknown",
        }
    }

    /// Returns true if the tier represents any form of on-chain storage.
    pub fn is_onchain(&self) -> bool {
        matches!(self, CompositionTier::PureCkb | CompositionTier::BtcCkb)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SporeMediaSource {
    pub uri: String,
    pub scheme: String,
    pub source_location: String,
    #[serde(default)]
    pub dependency_tier: CompositionTier,
}

/// One decoder's output in a DOB decode chain.
///
/// The raw output is stored as a content-addressed blob in [`MediaBlobStore`].
/// Parsed metadata (traits, media type) is recorded alongside for quick access
/// without reading the blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DobDecodedStep {
    /// 0-indexed position in the decoder chain.
    pub step: u32,
    /// MIME type of the raw output (sniffed from bytes).
    pub media_type: String,
    /// Byte size of the raw output.
    pub size: u64,
    /// Blake2b content hash (hex-encoded), also the blob filename.
    pub hash: String,
    /// If the raw output is valid `DobTraitGroup[]` JSON, the parsed traits.
    /// Empty if the output is not JSON trait data (e.g. SVG, PNG).
    #[serde(default)]
    pub traits: Vec<DobDecodedTrait>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SporeMediaProfile {
    #[serde(default)]
    pub tier: CompositionTier,
    #[serde(default)]
    pub sources: Vec<SporeMediaSource>,
    #[serde(default)]
    pub issues: Vec<String>,
}

/// Cached DOB decode result from CKB-VM execution.
///
/// Each decoder in the chain produces one [`DobDecodedStep`]. The raw output
/// of each step is stored as-is in [`MediaBlobStore`]; parsed metadata is
/// recorded here for quick access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobDecodedEntry {
    /// Per-decoder step outputs, in chain order.
    pub steps: Vec<DobDecodedStep>,
    /// Media sources (URIs) extracted from decoded trait values.
    pub media_sources: Vec<SporeMediaSource>,
    /// Epoch timestamp when this was decoded.
    pub decoded_at: i64,
}

/// Persisted outcome of a DOB decode attempt for one spore.
///
/// Stored in `CF_DOB_DECODED`. A `Failed` value is written only for
/// deterministic failures (bad/dangling on-chain data or a decoder that
/// rejected immutable DNA); transient RPC/node errors are never persisted so
/// they keep retrying on the next worker run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecodeOutcome {
    Decoded(DobDecodedEntry),
    Failed(DobDecodeFailure),
}

/// A recorded deterministic DOB decode failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobDecodeFailure {
    pub category: DobDecodeFailureCategory,
    /// Human-readable detail (surfaced to the API `issues` list).
    pub message: String,
    /// Epoch seconds when the failure was recorded.
    pub failed_at: i64,
}

/// Stable taxonomy of deterministic decode failures.
///
/// Bincode-serialized: only append new variants at the END.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DobDecodeFailureCategory {
    /// Clusterless "Sole Spores" — collection_id is SOLE_SPORES_SENTINEL_COLLECTION,
    /// so there is no DOB cluster/decoder to decode against.
    Clusterless,
    /// A real (non-sentinel) cluster_id that is not present in the index.
    ClusterNotFound,
    /// Cluster exists but its metadata is unusable (no description, not JSON,
    /// missing `dob` field, or an invalid decoder reference).
    ClusterMetadataInvalid,
    /// Referenced decoder cell (code_hash or type_id) has no live cell.
    DecoderNotFound,
    /// The decoder binary ran and rejected the spore (non-zero exit, etc.).
    DecoderExecutionFailed,
    /// The spore's on-chain content could not yield valid DNA.
    DnaInvalid,
    /// Any other deterministic failure.
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobDecodedTrait {
    pub name: String,
    pub value: String,
}

/// Object standard identifier.
///
/// Object is the unified asset type on CKB covering Spore/DOB and mNFT.
/// Each variant represents a specific standard or entity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ObjectStandard {
    /// A Spore item (individual DOB).
    #[default]
    Spore,
    /// A Spore cluster (collection of Spores).
    SporeCluster,
    /// mNFT issuer (top-level entity that creates classes).
    MnftIssuer,
    /// mNFT class (a collection of mNFT tokens).
    MnftClass,
    /// mNFT token (individual NFT item).
    MnftToken,
}

impl ObjectStandard {
    /// Wire-level name for logging/debugging.
    pub fn as_str(&self) -> &'static str {
        match self {
            ObjectStandard::Spore => "spore",
            ObjectStandard::SporeCluster => "spore_cluster",
            ObjectStandard::MnftIssuer => "mnft_issuer",
            ObjectStandard::MnftClass => "mnft_class",
            ObjectStandard::MnftToken => "mnft",
        }
    }

    /// Asset-level standard name for API grouping (collapses cluster → "spore").
    pub fn asset_standard(&self) -> &'static str {
        match self {
            ObjectStandard::Spore | ObjectStandard::SporeCluster => "spore",
            ObjectStandard::MnftIssuer | ObjectStandard::MnftClass | ObjectStandard::MnftToken => {
                "m-nft"
            }
        }
    }

    /// Returns `true` for collection-level entries (clusters/classes/issuers), `false` for items.
    pub fn is_cluster(&self) -> bool {
        matches!(
            self,
            ObjectStandard::SporeCluster | ObjectStandard::MnftIssuer | ObjectStandard::MnftClass
        )
    }
}

/// Standard-specific data for Object entries, stored inline via bincode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObjectExtra {
    /// Spore item: MIME content type and content byte length.
    Spore {
        content_type: String,
        content_length: i64,
        #[serde(default)]
        media_profile: SporeMediaProfile,
    },
    /// Spore cluster: no extra fields (name/description live on `ObjectEntry`).
    SporeCluster,
    /// mNFT issuer metadata.
    MnftIssuer {
        class_count: u32,
        set_count: u32,
        /// Raw issuer info bytes from on-chain data.
        info: Option<Vec<u8>>,
    },
    /// mNFT class (collection) metadata.
    MnftClass {
        description: Option<String>,
        renderer: Option<String>,
        total: u32,
        issued: u32,
        configure: u8,
        #[serde(default)]
        composition_tier: CompositionTier,
    },
    /// mNFT token (individual item) metadata.
    MnftToken {
        token_index: u32,
        characteristic: Vec<u8>,
        configure: u8,
        state: u8,
    },
}

/// An Object entry stored in the `spore_data` or `object_data` column family.
///
/// Covers all Object standards: Spore (item/cluster), mNFT (issuer/class/token).
/// Standard-specific data lives in `extra`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectEntry {
    pub standard: ObjectStandard,
    /// Parent collection. Spore → cluster_id, mNFT tokens → class_id, mNFT classes → issuer_id.
    /// `None` = default collection for this standard.
    pub collection_id: Option<Vec<u8>>,
    pub token_id: Option<Vec<u8>>,
    pub owner_lock_hash: Option<Vec<u8>>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
    pub created_at_tx: Vec<u8>,
    /// Standard-specific payload (bincode-serialized, no JSON).
    pub extra: ObjectExtra,
}

/// Identity standard identifier.
///
/// Identity is a CKB asset type covering on-chain identities and domain names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IdentityStandard {
    /// .bit (DotBit) domain name account.
    #[default]
    DotBit,
    /// `.bit Cell`, the DID companion asset issued by the .bit protocol.
    BitCell,
    /// did:ckb decentralized identity.
    DidCkb,
}

impl IdentityStandard {
    /// Wire-level name for logging/debugging.
    pub fn as_str(&self) -> &'static str {
        match self {
            IdentityStandard::DotBit => "dotbit",
            IdentityStandard::BitCell => "bit_cell",
            IdentityStandard::DidCkb => "did_ckb",
        }
    }

    /// Asset-level standard name for API grouping.
    pub fn asset_standard(&self) -> &'static str {
        match self {
            IdentityStandard::DotBit => "dotbit",
            IdentityStandard::BitCell => "bit_cell",
            IdentityStandard::DidCkb => "did_ckb",
        }
    }

    /// Sentinel collection key for this identity standard.
    pub fn sentinel_collection_id(&self) -> &'static [u8; 32] {
        match self {
            IdentityStandard::DotBit => &DOTBIT_SENTINEL_COLLECTION,
            IdentityStandard::BitCell => &BIT_CELL_SENTINEL_COLLECTION,
            IdentityStandard::DidCkb => &DID_CKB_SENTINEL_COLLECTION,
        }
    }
}

/// Sentinel collection key for the .bit identity collection (32 bytes).
pub const DOTBIT_SENTINEL_COLLECTION: [u8; 32] = *b"dotbit_collection_______________";
/// Sentinel collection key for the `.bit Cell` identity collection (32 bytes).
pub const BIT_CELL_SENTINEL_COLLECTION: [u8; 32] = *b"bit_cell_collection_____________";
/// Sentinel collection key for the did:ckb identity collection (32 bytes).
pub const DID_CKB_SENTINEL_COLLECTION: [u8; 32] = *b"did_ckb_collection______________";
/// Sentinel collection key for clusterless Spore objects (32 bytes).
pub const SOLE_SPORES_SENTINEL_COLLECTION: [u8; 32] = *b"sole_spores_collection__________";

/// Standard-specific data for Identity entries, stored inline via bincode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdentityExtra {
    /// .bit account metadata.
    DotBit {
        /// Account expiration timestamp (Unix epoch seconds).
        expired_at: Option<u64>,
        /// Account registration timestamp (Unix epoch seconds).
        #[serde(default)]
        registered_at: Option<u64>,
        /// Account status: 0=normal, 1=selling, 2=auction, 3=cross-chain, 4=approved-transfer.
        #[serde(default)]
        status: Option<u8>,
    },
    /// `.bit Cell` metadata. `account_id` links the companion DID asset to its
    /// canonical 20-byte .bit AccountCell identity without merging lifecycles.
    BitCell {
        account_id: Vec<u8>,
        expired_at: u64,
    },
    /// did:ckb identity: reserved for future fields.
    DidCkb,
}

/// An Identity entry stored in the `identity_data` column family.
///
/// Covers all identity standards: .bit AccountCell, `.bit Cell`, and did:ckb.
/// Standard-specific data lives in `extra`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityEntry {
    pub standard: IdentityStandard,
    pub owner_lock_hash: Option<Vec<u8>>,
    pub name: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
    pub created_at_tx: Vec<u8>,
    /// Standard-specific payload (bincode-serialized, no JSON).
    pub extra: IdentityExtra,
}

/// Pre-aggregated cluster (DOB collection) data, maintained inline by the indexer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClusterAggregate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub total_count: i64,
    pub live_count: i64,
    pub owner_count: i64,
    #[serde(default)]
    pub btc_ckb_count: i64,
    #[serde(default)]
    pub pure_ckb_count: i64,
    #[serde(default)]
    pub decentralized_mixture_count: i64,
    #[serde(default)]
    pub centralized_mixture_count: i64,
    #[serde(default)]
    pub unknown_count: i64,
    /// Cumulative owned capacity (shannon) of all live spores in this cluster.
    #[serde(default)]
    pub owned_capacity: i128,
    /// Cumulative occupied capacity (shannon) of all live spores in this cluster.
    #[serde(default)]
    pub owned_knowledge: i128,
}

/// Pre-aggregated mNFT collection data, maintained inline by the indexer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MnftCollectionAggregate {
    pub name: Option<String>,
    pub standard: ObjectStandard,
    pub total_count: i64,
    pub live_count: i64,
    #[serde(default)]
    pub holders_count: i64,
    #[serde(default)]
    pub activities_count: i64,
    #[serde(default)]
    pub btc_ckb_count: i64,
    #[serde(default)]
    pub pure_ckb_count: i64,
    #[serde(default)]
    pub decentralized_mixture_count: i64,
    #[serde(default)]
    pub centralized_mixture_count: i64,
    #[serde(default)]
    pub unknown_count: i64,
}

/// Pre-aggregated Identity collection data, maintained inline by the indexer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IdentityCollectionAggregate {
    pub name: Option<String>,
    pub standard: IdentityStandard,
    pub total_count: i64,
    pub live_count: i64,
    pub holders_count: i64,
    pub activities_count: i64,
}

// ============================================
// Group G: Statistics
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyStats {
    pub blocks_count: i32,
    pub transactions_count: i32,
    pub cells_created: i32,
    pub cells_consumed: i32,
    pub capacity_transferred: i128,
    #[serde(default)]
    pub used_capacity_created: i128,
    #[serde(default)]
    pub used_capacity_consumed: i128,
    pub total_live_cells: i64,
    pub total_dead_cells: i64,
    pub total_all_cells: i64,
    pub total_data_size: i64,
    pub knowledge_size: Option<i128>,
    /// Sum of inter-block time deltas (ms) for blocks landing on this day.
    /// Combined with `block_time_count` this is the single source of truth for
    /// the day's average block time; `avg_block_time_ms()` derives it.
    #[serde(default)]
    pub block_time_sum_ms: i64,
    /// Number of inter-block time deltas accumulated into `block_time_sum_ms`.
    #[serde(default)]
    pub block_time_count: i32,
}

impl DailyStats {
    /// Derive the day's average block time (ms) from the stored sum and count.
    /// Returns `None` only when no deltas have been accumulated yet (a fresh
    /// day with at most one block — typically only the genesis day).
    pub fn avg_block_time_ms(&self) -> Option<i64> {
        if self.block_time_count > 0 {
            Some(self.block_time_sum_ms / self.block_time_count as i64)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HourlyStats {
    pub hour: i64,
    pub blocks_count: i32,
    pub transactions_count: i32,
    pub cells_created: i32,
    pub cells_consumed: i32,
    pub capacity_transferred: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EpochStats {
    pub epoch_number: i64,
    pub start_block: i64,
    pub end_block: Option<i64>,
    pub blocks_count: i32,
    pub length: i32,
    pub start_timestamp: DateTime<Utc>,
    pub end_timestamp: Option<DateTime<Utc>>,
    pub transactions_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MinerStats {
    pub miner_lock_hash: Vec<u8>,
    pub blocks_count: i32,
    pub last_block_number: i64,
}

/// Per-day block-level aggregates: difficulty, block count, uncle count.
///
/// Inter-block time lives on [`DailyStats`] only. This row deliberately does
/// not duplicate it: /charts/average-block-time and the /charts/hash-rate
/// divisor both read `DailyStats.block_time_sum_ms`, which every write path
/// (live, bulk, and reorg rollback repair) maintains.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyBlockStats {
    pub avg_difficulty: f64,
    pub block_count: i32,
    pub total_uncles: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptInfo {
    pub code_hash: Vec<u8>,
    pub hash_type: u8,
    pub name: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
    pub category: Option<String>,
    pub website: Option<String>,
    pub description: Option<String>,
    pub cells_count: i64,
    pub capacity_used: i128,
    // Per-kind usage stats (lock vs type)
    pub lock_cells_count: i64,
    pub lock_live_cells_count: i64,
    pub lock_capacity_sum: i128,
    pub lock_owned_capacity_sum: i128,
    #[serde(default)]
    pub lock_used_capacity_sum: i128,
    #[serde(default)]
    pub lock_owned_knowledge_sum: i128,
    pub type_cells_count: i64,
    pub type_live_cells_count: i64,
    pub type_capacity_sum: i128,
    pub type_owned_capacity_sum: i128,
    #[serde(default)]
    pub type_used_capacity_sum: i128,
    #[serde(default)]
    pub type_owned_knowledge_sum: i128,
    /// type_script_hash of the deployment cell (from label data).
    /// Used to find the code cell for hash_type="data"/"data1"/"data2" scripts.
    #[serde(default)]
    pub dep_type_hash: Option<Vec<u8>>,
    /// data_hash of the deployment cell (from label data).
    /// Used as fallback when dep_type_hash is absent (e.g. genesis cells).
    #[serde(default)]
    pub dep_data_hash: Option<Vec<u8>>,
    /// Pre-resolved code cell outpoint (resolved during label import).
    /// Only populated for scripts where runtime lookup is expensive
    /// (data/data1/data2 without dep_type_hash).
    #[serde(default)]
    pub code_cell_tx_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub code_cell_output_index: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScriptVersionInfo {
    pub version_hash: Vec<u8>,
    pub name: Option<String>,
    #[serde(default)]
    pub family_id: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
    pub category: Option<String>,
    pub website: Option<String>,
    pub description: Option<String>,
    pub lock_cells_count: i64,
    pub lock_live_cells_count: i64,
    pub lock_capacity_sum: i128,
    pub lock_owned_capacity_sum: i128,
    #[serde(default)]
    pub lock_used_capacity_sum: i128,
    #[serde(default)]
    pub lock_owned_knowledge_sum: i128,
    pub type_cells_count: i64,
    pub type_live_cells_count: i64,
    pub type_capacity_sum: i128,
    pub type_owned_capacity_sum: i128,
    #[serde(default)]
    pub type_used_capacity_sum: i128,
    #[serde(default)]
    pub type_owned_knowledge_sum: i128,
    #[serde(default)]
    pub canonical_reference_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub canonical_hash_type: Option<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScriptFamilyInfo {
    pub family_id: String,
    pub name: String,
    pub description: Option<String>,
    pub website: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
    pub versions_count: i64,
    pub live_cells_count: i64,
    pub cells_count: i64,
    pub lock_cells_count: i64,
    pub type_cells_count: i64,
    pub owned_capacity_sum: i128,
    pub owned_knowledge_sum: i128,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScriptReferenceInfo {
    pub reference_hash: Vec<u8>,
    pub hash_type: u8,
    pub lock_cells_count: i64,
    pub lock_live_cells_count: i64,
    pub lock_capacity_sum: i128,
    pub lock_owned_capacity_sum: i128,
    #[serde(default)]
    pub lock_used_capacity_sum: i128,
    #[serde(default)]
    pub lock_owned_knowledge_sum: i128,
    pub type_cells_count: i64,
    pub type_live_cells_count: i64,
    pub type_capacity_sum: i128,
    pub type_owned_capacity_sum: i128,
    #[serde(default)]
    pub type_used_capacity_sum: i128,
    #[serde(default)]
    pub type_owned_knowledge_sum: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScriptDailyDelta {
    /// Net live capacity change in shannons for this script deployment + kind + day.
    pub owned_capacity_delta: i128,
    /// Net live used capacity change in shannons for this script deployment + kind + day.
    pub owned_knowledge_delta: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenDailyDelta {
    /// Net live capacity change in shannons for this token's cells on a day.
    pub owned_capacity_delta: i128,
    /// Net live used capacity change in shannons for this token's cells on a day.
    pub owned_knowledge_delta: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClusterDailyDelta {
    /// Net live capacity change in shannons for this cluster's spores on a day.
    pub owned_capacity_delta: i128,
    /// Net live used capacity change in shannons for this cluster's spores on a day.
    pub owned_knowledge_delta: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SporeDailyDelta {
    /// Net live capacity change in shannons for this spore on a day.
    pub owned_capacity_delta: i128,
    /// Net live used capacity change in shannons for this spore on a day.
    pub owned_knowledge_delta: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SporeTypeIndex {
    pub spore_id: Vec<u8>,
    pub cluster_id: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MnftDailyDelta {
    /// Net live capacity change in shannons for this mNFT collection on a day.
    pub owned_capacity_delta: i128,
    /// Net live used capacity change in shannons for this mNFT collection on a day.
    pub owned_knowledge_delta: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MnftTypeIndex {
    pub collection_id: Vec<u8>,
}

// ============================================
// Group G2: HODL Wave
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyHodlWave {
    pub band_24h: i128,
    pub band_1d_1w: i128,
    pub band_1w_1m: i128,
    pub band_1m_3m: i128,
    pub band_3m_6m: i128,
    pub band_6m_1y: i128,
    pub band_1y_3y: i128,
    pub band_gt_3y: i128,
    pub holder_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HodlTrackerState {
    pub capacity_by_date: Vec<(String, i128)>,
    pub date_transitions: Vec<(i64, String)>,
    pub holder_count: i64,
    pub last_snapshot_date: Option<String>,
    /// The last block number processed by this tracker.
    /// Distinct from `date_transitions.last()` which only records date boundary changes.
    #[serde(default)]
    pub last_processed_block: Option<i64>,
}

// ============================================
// Group G3: Cell Distribution & Address Cohort
// ============================================

/// Daily snapshot of live cell distribution by size bucket.
/// Materialized by the indexer at each day boundary.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyCellDistribution {
    /// Cell count and capacity by size bucket: <100, 100-1k, 1k-10k, 10k-100k, 100k-1m, >=1m CKB
    pub size_bucket_counts: [i64; 6],
    pub size_bucket_capacities: [i128; 6],
}

/// Daily snapshot of address cohort retention data.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyAddressCohort {
    pub cohorts: Vec<AddressCohortEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressCohortEntry {
    pub cohort_month: String, // "YYYY-MM"
    pub used_capacity: i128,
    pub total_balance: i128,
}

/// Serializable state for the cell distribution tracker.
/// Persisted to sync_meta for crash recovery.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CellDistributionTrackerState {
    pub count_by_bucket: [i64; 6],
    pub total_capacity_by_bucket: [i128; 6],
    pub date_transitions: Vec<(i64, String)>,
    pub last_snapshot_date: Option<String>,
    /// Incremental address cohort accumulator: (YYYY-MM, used_capacity, balance).
    #[serde(default)]
    pub cohort_accum: Vec<(String, i128, i128)>,
    /// The last block number processed by this tracker.
    /// Distinct from `date_transitions.last()` which only records date boundary changes.
    #[serde(default)]
    pub last_processed_block: Option<i64>,
}

// ============================================
// Group H: System
// ============================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncStatus {
    pub tip_block_number: i64,
    pub tip_block_hash: Vec<u8>,
    pub total_transactions: i64,
    pub total_cells_created: i64,
    pub total_cells_consumed: i64,
    pub last_synced_at: i64,
    #[serde(default)]
    pub sync_started_at: Option<i64>,
    #[serde(default)]
    pub sync_started_block: i64,
    #[serde(default)]
    pub sync_ema_rate: Option<f64>,
    #[serde(default)]
    pub bulk_sync_completed_at: Option<i64>,
    #[serde(default)]
    pub bulk_sync_completed_block: Option<i64>,
    pub deep_fork_detected: bool,
    pub deep_fork_info: Option<DeepForkInfo>,
}

impl SyncStatus {
    pub fn init_sync_start(&mut self, start_block: i64, is_bulk_sync: bool) {
        if is_bulk_sync {
            let should_start_new_bulk_session = self.sync_started_at.is_none()
                || self.bulk_sync_completed_at.is_some()
                || start_block < self.sync_started_block;

            if should_start_new_bulk_session {
                self.sync_started_at = Some(chrono::Utc::now().timestamp());
                self.sync_started_block = start_block;
                self.bulk_sync_completed_at = None;
                self.bulk_sync_completed_block = None;
            }
        } else {
            if self.sync_started_at.is_none() || start_block < self.sync_started_block {
                self.sync_started_at = Some(chrono::Utc::now().timestamp());
            }
            self.sync_started_block = start_block;
        }
    }

    pub fn mark_bulk_sync_completed(&mut self, chain_tip: i64) {
        if self.bulk_sync_completed_at.is_none() {
            self.bulk_sync_completed_at = Some(chrono::Utc::now().timestamp());
            self.bulk_sync_completed_block = Some(chain_tip);
        }
    }

    pub fn bulk_sync_total_seconds(&self) -> Option<i64> {
        let started = self.sync_started_at?;
        let completed = self.bulk_sync_completed_at?;
        Some(completed - started)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeStatus {
    #[serde(default)]
    pub active_run_id: Option<String>,
    #[serde(default)]
    pub last_run_id: Option<String>,
    #[serde(default)]
    pub run_started_at: i64,
    #[serde(default)]
    pub last_heartbeat_at: i64,
    #[serde(default)]
    pub last_heartbeat_block: i64,
    #[serde(default)]
    pub last_heartbeat_target_block: i64,
    #[serde(default)]
    pub last_heartbeat_stage: Option<String>,
    #[serde(default)]
    pub last_heartbeat_oom_events: Option<u64>,
    #[serde(default)]
    pub last_heartbeat_oom_kill_events: Option<u64>,
    #[serde(default)]
    pub last_shutdown_reason: Option<String>,
    #[serde(default)]
    pub last_exit_code: Option<i32>,
    #[serde(default)]
    pub last_incident_id: Option<String>,
    #[serde(default)]
    pub last_incident_at: i64,
    #[serde(default)]
    pub last_incident_summary: Option<String>,
    #[serde(default)]
    pub last_shutdown_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BulkBuildSessionMarker {
    pub run_id: String,
    pub started_at: i64,
    pub start_block: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepForkInfo {
    pub db_tip: i64,
    pub db_tip_hash: Vec<u8>,
    pub chain_tip: i64,
    pub chain_tip_hash: Vec<u8>,
    pub depth: i32,
    pub fork_point: i64,
}

/// How the indexer answered a detected fork.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReorgEventKind {
    /// Depth within the automatic limit: rolled back and re-synced in place.
    Automatic,
    /// Depth beyond the automatic limit: sync paused for operator intervention.
    Deep,
}

/// One persisted reorg observation.
///
/// This is a log record, so it stores the observation verbatim: every field is
/// what the writer was handed at detection time, and nothing is re-derived at
/// read time from chain state that may since have moved. `/forks` reports these
/// fields directly, so a field that is not recorded here cannot be reported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorgEvent {
    /// Unix seconds at detection.
    pub detected_at: i64,
    pub kind: ReorgEventKind,
    /// Last common ancestor: the block the DB was rolled back to.
    pub fork_point: i64,
    pub fork_point_hash: Vec<u8>,
    /// DB tip on the abandoned branch at detection time.
    pub old_tip: i64,
    pub old_tip_hash: Vec<u8>,
    /// Chain tip on the winning branch at detection time.
    pub new_tip: i64,
    pub new_tip_hash: Vec<u8>,
    pub depth: i32,
    /// Blocks actually removed by the rollback (0 for a paused deep fork,
    /// which rolls nothing back).
    pub orphaned_blocks: i64,
    /// Transactions actually removed by the rollback.
    pub orphaned_txs: i64,
}

/// One history row: the persisted event plus the identity of its history key.
#[derive(Debug, Clone)]
pub struct ReorgEventRecord {
    /// Full `reorg:<ms>:<uuid>` history key; also the pagination cursor.
    pub key: String,
    /// Detection time in milliseconds, parsed from the key. This is the public
    /// event id: the uuid segment only breaks same-millisecond ties.
    pub detected_at_ms: i64,
    pub event: ReorgEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UndoLogStoreTarget {
    Domain,
    AppendOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UndoInputOutPoint {
    pub tx_hash: Vec<u8>,
    pub output_index: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UndoTxContext {
    pub tx_hash: Vec<u8>,
    pub outputs_count: i16,
    pub inputs: Vec<UndoInputOutPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UndoLogEntry {
    /// Previous value before the forward write.
    /// `None` means the key did not exist and rollback should delete it.
    KeyMutation {
        target_store: UndoLogStoreTarget,
        cf_name: String,
        key: Vec<u8>,
        previous_value: Option<Vec<u8>>,
    },
    /// Transaction context for deriving cell/consumed rollback from canonical tx data.
    TxContext(UndoTxContext),
}

/// Memory/storage statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// Estimated number of live cells (from RocksDB estimate-num-keys on live_cells CF).
    pub live_cells_count: usize,
    /// Estimated number of consumed cells (from consumed_cells CF).
    pub consumed_cells_count: usize,
    /// Estimated bytes used by consumed_cells CF (live-data estimate with SST fallback).
    pub consumed_cells_bytes: usize,
    /// Source used to estimate consumed_cells_bytes: live/sst/mem/none.
    pub consumed_cells_bytes_source: &'static str,
    /// Estimated number of cached block headers.
    pub block_headers_count: usize,
    /// Estimated number of address entries in addr_balance column family.
    pub addr_balance_count: usize,
    /// Estimated number of canonical cells in `cells` column family.
    pub cells_count: usize,
    pub memory_bytes: usize,
    pub memtable_bytes: usize,
    pub block_cache_bytes: usize,
    pub table_readers_bytes: usize,
    /// Estimated bytes pending compaction across all CFs
    pub compaction_pending_bytes: u64,
    /// Number of currently running compactions
    pub num_running_compactions: u64,
    /// Total SST file size on disk (all CFs)
    pub sst_files_size: u64,
    /// Total L0 files across all CFs (sum)
    pub l0_files_count: u64,
    /// Max L0 files in any single CF (the actual write stall trigger)
    pub l0_files_max: u64,
    /// Name of the CF with the most L0 files
    pub l0_worst_cf: String,
    /// Total immutable memtables across all CFs (waiting for flush)
    pub immutable_memtables: u64,
    /// Top column families by estimated live data size: (name, bytes)
    pub top_cf_sizes: Vec<(String, u64)>,
    /// WriteBufferManager current usage in bytes
    pub wbm_usage_bytes: usize,
    /// WriteBufferManager budget (buffer_size) in bytes
    pub wbm_budget_bytes: usize,
}

// ============================================
// Group I: Activities
// ============================================

// Tag bit flags for TxActions / ParticipantDelta classification.
pub const TAG_TOKEN: u16 = 1 << 0;
pub const TAG_OBJECT: u16 = 1 << 1;
pub const TAG_IDENTITY: u16 = 1 << 2;
pub const TAG_DAO: u16 = 1 << 3;
pub const TAG_PROTOCOL: u16 = 1 << 4;
pub const TAG_CELLBASE: u16 = 1 << 5;
pub const TAG_TYPE_CALL: u16 = 1 << 6;
pub const TAG_LOCK_CALL: u16 = 1 << 7;

// ItemDelta kind discriminators.
pub const ITEM_KIND_TOKEN: u8 = 0;
pub const ITEM_KIND_OBJECT: u8 = 1;
pub const ITEM_KIND_IDENTITY: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDelta {
    pub item_id: Vec<u8>,
    pub kind: u8,
    /// Absolute value of the delta. For a token item this is a UDT amount (u128); for
    /// object/identity items it is a small count. Stored only for non-zero deltas, so
    /// `magnitude` is always >= 1 when persisted.
    pub magnitude: u128,
    /// Sign of the delta (true = negative).
    pub negative: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantDelta {
    pub lock_hash: Vec<u8>,
    pub ckb_delta: i128,
    pub used_delta: i64,
    pub item_deltas: Vec<ItemDelta>,
    pub tags: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxActions {
    pub tx_hash: Vec<u8>,
    pub block_hash: Vec<u8>,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: i64,
    pub is_cellbase: bool,
    pub protocol_actions: Vec<ProtocolAction>,
    pub type_calls: Vec<TypeCallEntry>,
    pub lock_calls: Vec<LockCallEntry>,
    pub participants: Vec<ParticipantDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeCallEntry {
    pub type_code_hash: Vec<u8>,
    pub type_hash_type: i16,
    pub type_args: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockCallEntry {
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
}

/// Stable storage form for protocol metadata.
///
/// `serde_json::Value` can be serialized by bincode but cannot be
/// deserialized back because its `Deserialize` impl requires
/// `deserialize_any`, which bincode does not support. Store canonical JSON
/// text instead, then parse it at the API/indexer boundary when needed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ProtocolMetadata(String);

impl ProtocolMetadata {
    pub fn from_value(value: serde_json::Value) -> Self {
        Self(
            serde_json::to_string(&value)
                .expect("protocol metadata JSON serialization should never fail"),
        )
    }

    pub fn to_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::from_str(&self.0)
    }

    pub fn raw(&self) -> &str {
        &self.0
    }
}

impl From<serde_json::Value> for ProtocolMetadata {
    fn from(value: serde_json::Value) -> Self {
        Self::from_value(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolAction {
    /// Protocol identifier: "rgbpp", "utxoswap", "fiber", etc.
    pub protocol: String,
    /// Action name: "leap_to_ckb", "leap_to_btc", "transfer", etc.
    pub action: String,
    /// Protocol-specific decoded metadata.
    pub metadata: ProtocolMetadata,
}

impl ProtocolAction {
    pub fn new(
        protocol: impl Into<String>,
        action: impl Into<String>,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            protocol: protocol.into(),
            action: action.into(),
            metadata: ProtocolMetadata::from_value(metadata),
        }
    }

    pub fn metadata_value(&self) -> serde_json::Result<serde_json::Value> {
        self.metadata.to_value()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetAction {
    Mint,
    Transfer,
    Burn,
    /// .bit: expired account removed from chain (capacity refunded).
    Recycle,
    /// .bit: account expiry extended (no ownership change).
    Renew,
    /// .bit: metadata changed (edit_records, edit_manager, marketplace state, etc.).
    Update,
}

// ============================================
// Group J2: Fiber Channel State
// ============================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FiberChannelState {
    Open,
    CooperativelyClosed,
    ForceClosed,
    Settled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiberChannel {
    pub funding_tx_hash: Vec<u8>,
    pub funding_output_index: u32,
    pub state: FiberChannelState,
    pub capacity: u64,
    pub udt_type_hash: Option<Vec<u8>>,
    pub udt_amount: Option<u128>,
    pub open_block: i64,
    pub open_timestamp: i64,
    pub close_tx_hash: Option<Vec<u8>>,
    pub close_block: Option<i64>,
    pub close_timestamp: Option<i64>,
    pub commitment_tx_hash: Option<Vec<u8>>,
    pub commitment_output_index: Option<u32>,
    pub delay_epoch: Option<u64>,
    pub settlement_tx_hash: Option<Vec<u8>>,
    pub settlement_block: Option<i64>,
    pub settlement_timestamp: Option<i64>,
    pub participants: Vec<Vec<u8>>,
    pub funding_lock_args: Vec<u8>,
}

// ============================================
// Group J: Daily Activity Stats
// ============================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyActivityStats {
    /// Plain CKB transfers (no asset changes, not coinbase)
    pub transfer_count: u32,
    /// DAO deposit activities
    pub dao_deposit_count: u32,
    /// DAO withdraw request activities
    pub dao_withdraw_request_count: u32,
    /// DAO withdraw completion activities
    pub dao_withdraw_complete_count: u32,
    /// Token (xUDT/sUDT) transfer activities
    pub token_count: u32,
    /// Object activities (Spore + M-NFT)
    pub object_count: u32,
    /// Identity activities (.bit + did:ckb)
    #[serde(default)]
    pub identity_count: u32,
    /// Activities involving unrecognized type scripts
    #[serde(default)]
    pub script_call_count: u32,
    /// Fallback — should always be 0; non-zero indicates a classification bug
    #[serde(default)]
    pub unknown_count: u32,
    /// Coinbase (miner reward) activities
    pub coinbase_count: u32,
    /// Number of unique addresses active this day
    pub unique_address_count: u32,
    /// Sum of absolute CKB deltas in shannons
    pub total_ckb_moved: u128,
    /// Per-script activity counts: hex code_hash -> count
    #[serde(default)]
    pub script_counts: HashMap<String, u32>,
    /// Per-protocol action counts: "rgbpp:leap_to_ckb" -> count
    #[serde(default)]
    pub protocol_action_counts: HashMap<String, u32>,
}

impl DailyActivityStats {
    /// Accumulate statistics from a single transaction's actions.
    ///
    /// This is the **single calculation path** for activity stats accumulation,
    /// shared by both live sync (indexer) and reorg rollback (store).
    pub fn accumulate_from_tx_actions(&mut self, tx_actions: &TxActions) {
        // Coinbase transactions are counted but excluded from all other metrics
        if tx_actions.is_cellbase {
            self.coinbase_count += 1;
            return;
        }

        // Total CKB moved (absolute value of all participants) — excludes coinbase
        for p in &tx_actions.participants {
            self.total_ckb_moved = self
                .total_ckb_moved
                .checked_add(p.ckb_delta.unsigned_abs())
                .expect("total_ckb_moved overflow in accumulate_from_tx_actions");
        }

        // Count each involved script from type_calls and lock_calls — excludes coinbase
        for tc in &tx_actions.type_calls {
            let hex = hex::encode(&tc.type_code_hash);
            *self.script_counts.entry(hex).or_insert(0) += 1;
        }
        for lc in &tx_actions.lock_calls {
            let hex = hex::encode(&lc.lock_code_hash);
            *self.script_counts.entry(hex).or_insert(0) += 1;
        }

        // Count each protocol action — excludes coinbase
        for pa in &tx_actions.protocol_actions {
            let key = format!("{}:{}", pa.protocol, pa.action);
            *self.protocol_action_counts.entry(key).or_insert(0) += 1;
        }

        // DAO counts from protocol_actions (TX-level)
        let mut has_dao = false;
        for pa in &tx_actions.protocol_actions {
            if pa.protocol == "dao" {
                has_dao = true;
                match pa.action.as_str() {
                    "deposit" => self.dao_deposit_count += 1,
                    "withdraw_request" => self.dao_withdraw_request_count += 1,
                    "withdraw_complete" => self.dao_withdraw_complete_count += 1,
                    _ => {}
                }
            }
        }

        // Token/Object/Identity/Script call flags from participant tags
        let mut has_token = false;
        let mut has_object = false;
        let mut has_identity = false;
        for p in &tx_actions.participants {
            if p.tags & TAG_TOKEN != 0 {
                has_token = true;
            }
            if p.tags & TAG_OBJECT != 0 {
                has_object = true;
            }
            if p.tags & TAG_IDENTITY != 0 {
                has_identity = true;
            }
        }

        let has_script_call = !tx_actions.type_calls.is_empty();

        if has_token {
            self.token_count += 1;
        }
        if has_object {
            self.object_count += 1;
        }
        if has_identity {
            self.identity_count += 1;
        }
        if has_script_call {
            self.script_call_count += 1;
        }

        // transfer_count = Layer 1 only (CKB delta with no Layer 2 or Layer 3 signals).
        // Any Layer 2 asset/script signal or Layer 3 protocol action excludes from transfer.
        let has_protocol_action = !tx_actions.protocol_actions.is_empty();
        let matched = has_dao
            || has_token
            || has_object
            || has_identity
            || has_script_call
            || has_protocol_action;
        if !matched {
            // Check if any participant has a type_call tag (meaning type script was involved)
            let has_type_call_tag = tx_actions
                .participants
                .iter()
                .any(|p| p.tags & TAG_TYPE_CALL != 0);
            if !has_type_call_tag {
                self.transfer_count += 1;
            } else {
                self.unknown_count += 1;
            }
        }
    }
}

// ============================================
// Group I-b: Object Collection Activities (pre-computed)
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectCollectionActivityEntry {
    pub tx_hash: Vec<u8>,
    #[serde(default)]
    pub block_hash: Vec<u8>,
    pub timestamp_ms: i64,
    pub actions: Vec<AssetAction>,
}

// ============================================
// Group I-c: Token Activities (derived at read time from token_transfers CF)
// ============================================

/// A single token activity: one transaction with aggregated actions and individual transfers.
/// Derived at read time by grouping token_transfers records by tx_hash — not persisted.
#[derive(Debug, Clone)]
pub struct TokenActivityEntry {
    pub tx_hash: Vec<u8>,
    pub block_number: i64,
    pub timestamp_ms: i64,
    pub actions: Vec<AssetAction>,
    pub transfers: Vec<TokenActivityTransfer>,
}

#[derive(Debug, Clone)]
pub struct TokenActivityTransfer {
    pub from_lock_hash: Option<Vec<u8>>,
    pub to_lock_hash: Vec<u8>,
    pub amount: u128,
    pub is_mint: bool,
    pub is_burn: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_live_cell_info() -> LiveCellInfo {
        LiveCellInfo {
            capacity: 1_000_000_000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: Some(vec![0x44; 32]),
            type_code_hash: Some(vec![0x55; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x66; 8]),
            data_size: 16,
            occupied_capacity: 6_100_000_000,
            udt_amount: Some(42),
            data_hash: None,
        }
    }

    #[test]
    fn test_addr_tx_value_stores_tags_from_new() {
        let value = AddrTxValue::new(1_000_000_000, true, true, TAG_TOKEN | TAG_DAO);
        assert_eq!(value.tags, TAG_TOKEN | TAG_DAO);
        assert_eq!(value.capacity_change, 1_000_000_000);
        assert_eq!(value.flags, AddrTxValue::TX_TYPE_RECEIVED);
    }

    #[test]
    fn test_addr_tx_value_roundtrip_preserves_tags() {
        let value = AddrTxValue::new(-500, true, false, TAG_TOKEN | TAG_PROTOCOL);
        let bytes = bincode::serialize(&value).unwrap();
        let decoded: AddrTxValue = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.tags, TAG_TOKEN | TAG_PROTOCOL);
        assert_eq!(decoded.capacity_change, -500);
        assert_eq!(decoded.flags, AddrTxValue::TX_TYPE_SENT);
    }

    #[test]
    fn test_decode_outcome_bincode_roundtrip() {
        let decoded = DecodeOutcome::Decoded(DobDecodedEntry {
            steps: vec![],
            media_sources: vec![],
            decoded_at: 1_700_000_000,
        });
        let bytes = bincode::serialize(&decoded).unwrap();
        match bincode::deserialize::<DecodeOutcome>(&bytes).unwrap() {
            DecodeOutcome::Decoded(e) => assert_eq!(e.decoded_at, 1_700_000_000),
            DecodeOutcome::Failed(_) => panic!("expected Decoded"),
        }

        let failed = DecodeOutcome::Failed(DobDecodeFailure {
            category: DobDecodeFailureCategory::DecoderExecutionFailed,
            message: "decoder exited with non-zero code: 7".to_string(),
            failed_at: 1_700_000_001,
        });
        let bytes = bincode::serialize(&failed).unwrap();
        match bincode::deserialize::<DecodeOutcome>(&bytes).unwrap() {
            DecodeOutcome::Failed(f) => {
                assert_eq!(f.category, DobDecodeFailureCategory::DecoderExecutionFailed);
                assert_eq!(f.message, "decoder exited with non-zero code: 7");
                assert_eq!(f.failed_at, 1_700_000_001);
            }
            DecodeOutcome::Decoded(_) => panic!("expected Failed"),
        }
    }

    #[test]
    fn test_decode_consumed_cell_meta_current_schema() {
        let info = sample_live_cell_info();
        let consumed =
            ConsumedCellInfo::from_live_cell_info_with_consumer(&info, 999, Some(&[0xAA; 32]), 123);
        let meta = ConsumedCellMeta {
            created_at_block: 123,
            consumed_at_block: consumed.consumed_at_block,
            consumed_by_tx: consumed.consumed_by_tx.clone(),
        };
        let bytes = bincode::serialize(&meta).unwrap();
        let decoded = decode_consumed_cell_meta(&bytes).unwrap();
        assert_eq!(decoded.created_at_block, 123);
        assert_eq!(decoded.consumed_at_block, 999);
        assert_eq!(decoded.consumed_by_tx, Some(vec![0xAA; 32]));
    }

    #[test]
    fn test_decode_live_cell_marker_current_schema() {
        let bytes = encode_live_cell_marker(123);
        let decoded = decode_live_cell_marker(&bytes).unwrap();
        assert_eq!(decoded, 123);
    }

    #[test]
    fn test_decode_consumed_cell_meta_rejects_legacy_info_schema() {
        let info = sample_live_cell_info();
        let legacy = ConsumedCellInfo::from_live_cell_info_with_consumer(&info, 888, None, 123);
        let bytes = bincode::serialize(&legacy).unwrap();
        assert!(decode_consumed_cell_meta(&bytes).is_err());
    }

    #[test]
    fn test_decode_consumed_cell_meta_rejects_live_cell_schema() {
        let info = sample_live_cell_info();
        let bytes = bincode::serialize(&info).unwrap();
        assert!(decode_consumed_cell_meta(&bytes).is_err());
    }

    #[test]
    fn test_undo_log_entry_roundtrip() {
        let entry = UndoLogEntry::KeyMutation {
            target_store: UndoLogStoreTarget::AppendOnly,
            cf_name: "addr_txs".to_string(),
            key: vec![0xAA; 12],
            previous_value: Some(vec![0xBB; 8]),
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: UndoLogEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded, entry);
    }

    #[test]
    fn test_undo_log_entry_none_previous_value_roundtrip() {
        let entry = UndoLogEntry::KeyMutation {
            target_store: UndoLogStoreTarget::Domain,
            cf_name: "sync_meta".to_string(),
            key: b"new_key".to_vec(),
            previous_value: None,
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: UndoLogEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded, entry);
    }

    #[test]
    fn test_undo_log_tx_context_roundtrip() {
        let entry = UndoLogEntry::TxContext(UndoTxContext {
            tx_hash: vec![0x11; 32],
            outputs_count: 2,
            inputs: vec![
                UndoInputOutPoint {
                    tx_hash: vec![0x22; 32],
                    output_index: 0,
                },
                UndoInputOutPoint {
                    tx_hash: vec![0x33; 32],
                    output_index: 1,
                },
            ],
        });
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: UndoLogEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded, entry);
    }

    // ---- ScriptDailyDelta ----

    #[test]
    fn test_script_daily_delta_roundtrip() {
        let delta = ScriptDailyDelta {
            owned_capacity_delta: 123_000_000_000,
            owned_knowledge_delta: -45_000_000_000,
        };
        let bytes = bincode::serialize(&delta).unwrap();
        let decoded: ScriptDailyDelta = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.owned_capacity_delta, 123_000_000_000);
        assert_eq!(decoded.owned_knowledge_delta, -45_000_000_000);
    }

    // ---- TokenDailyDelta ----

    #[test]
    fn test_token_daily_delta_roundtrip() {
        let delta = TokenDailyDelta {
            owned_capacity_delta: 890_000_000_000,
            owned_knowledge_delta: -120_000_000_000,
        };
        let bytes = bincode::serialize(&delta).unwrap();
        let decoded: TokenDailyDelta = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.owned_capacity_delta, 890_000_000_000);
        assert_eq!(decoded.owned_knowledge_delta, -120_000_000_000);
    }

    // ---- ClusterDailyDelta ----

    #[test]
    fn test_cluster_daily_delta_roundtrip() {
        let delta = ClusterDailyDelta {
            owned_capacity_delta: 321_000_000_000,
            owned_knowledge_delta: -90_000_000_000,
        };
        let bytes = bincode::serialize(&delta).unwrap();
        let decoded: ClusterDailyDelta = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.owned_capacity_delta, 321_000_000_000);
        assert_eq!(decoded.owned_knowledge_delta, -90_000_000_000);
    }

    // ---- SporeDailyDelta ----

    #[test]
    fn test_spore_daily_delta_roundtrip() {
        let delta = SporeDailyDelta {
            owned_capacity_delta: 111_000_000_000,
            owned_knowledge_delta: -22_000_000_000,
        };
        let bytes = bincode::serialize(&delta).unwrap();
        let decoded: SporeDailyDelta = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.owned_capacity_delta, 111_000_000_000);
        assert_eq!(decoded.owned_knowledge_delta, -22_000_000_000);
    }

    // ---- SporeTypeIndex ----

    #[test]
    fn test_spore_type_index_roundtrip() {
        let index = SporeTypeIndex {
            spore_id: vec![0xAB; 32],
            cluster_id: Some(vec![0xCD; 32]),
        };
        let bytes = bincode::serialize(&index).unwrap();
        let decoded: SporeTypeIndex = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.spore_id, vec![0xAB; 32]);
        assert_eq!(decoded.cluster_id, Some(vec![0xCD; 32]));
    }

    // ---- MnftDailyDelta ----

    #[test]
    fn test_object_daily_delta_roundtrip() {
        let delta = MnftDailyDelta {
            owned_capacity_delta: 222_000_000_000,
            owned_knowledge_delta: -33_000_000_000,
        };
        let bytes = bincode::serialize(&delta).unwrap();
        let decoded: MnftDailyDelta = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.owned_capacity_delta, 222_000_000_000);
        assert_eq!(decoded.owned_knowledge_delta, -33_000_000_000);
    }

    // ---- MnftTypeIndex ----

    #[test]
    fn test_object_type_index_roundtrip() {
        let index = MnftTypeIndex {
            collection_id: vec![0xEE; 24],
        };
        let bytes = bincode::serialize(&index).unwrap();
        let decoded: MnftTypeIndex = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.collection_id, vec![0xEE; 24]);
    }

    // ---- TxActions / ItemDelta / ParticipantDelta ----

    #[test]
    fn test_item_delta_roundtrip() {
        let item = ItemDelta {
            item_id: vec![0xAA; 32],
            kind: ITEM_KIND_TOKEN,
            magnitude: 999000000,
            negative: true,
        };
        let bytes = bincode::serialize(&item).unwrap();
        let decoded: ItemDelta = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.item_id, vec![0xAA; 32]);
        assert_eq!(decoded.kind, ITEM_KIND_TOKEN);
        assert_eq!(decoded.magnitude, 999_000_000);
        assert!(decoded.negative);
    }

    #[test]
    fn item_delta_preserves_full_u128_signed_magnitude() {
        let item = ItemDelta {
            item_id: vec![],
            kind: ITEM_KIND_TOKEN,
            magnitude: u128::MAX,
            negative: true,
        };
        let bytes = bincode::serialize(&item).unwrap();
        let decoded: ItemDelta = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.magnitude, u128::MAX);
        assert!(decoded.negative);
    }

    #[test]
    fn test_participant_delta_roundtrip() {
        let participant = ParticipantDelta {
            lock_hash: vec![0xBB; 32],
            ckb_delta: -500_00000000,
            used_delta: 610_000_000_000,
            item_deltas: vec![
                ItemDelta {
                    item_id: vec![0xAA; 32],
                    kind: ITEM_KIND_TOKEN,
                    magnitude: 1000000,
                    negative: false,
                },
                ItemDelta {
                    item_id: vec![0xCC; 32],
                    kind: ITEM_KIND_OBJECT,
                    magnitude: 1,
                    negative: false,
                },
            ],
            tags: TAG_TOKEN | TAG_OBJECT,
        };
        let bytes = bincode::serialize(&participant).unwrap();
        let decoded: ParticipantDelta = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.lock_hash, vec![0xBB; 32]);
        assert_eq!(decoded.ckb_delta, -500_00000000);
        assert_eq!(decoded.used_delta, 610_000_000_000);
        assert_eq!(decoded.item_deltas.len(), 2);
        assert_eq!(decoded.item_deltas[0].kind, ITEM_KIND_TOKEN);
        assert_eq!(decoded.item_deltas[1].kind, ITEM_KIND_OBJECT);
        assert_eq!(decoded.tags, TAG_TOKEN | TAG_OBJECT);
    }

    #[test]
    fn test_tx_actions_roundtrip() {
        let actions = TxActions {
            tx_hash: vec![0x01; 32],
            block_hash: vec![0xF1; 32],
            block_number: 12345,
            tx_index: 3,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            protocol_actions: vec![ProtocolAction::new(
                "stablepp",
                "deposit",
                serde_json::json!({"vaultCount": 2}),
            )],
            type_calls: vec![TypeCallEntry {
                type_code_hash: vec![0xDD; 32],
                type_hash_type: 1,
                type_args: vec![0xEE; 20],
            }],
            lock_calls: vec![LockCallEntry {
                lock_code_hash: vec![0xFF; 32],
                lock_hash_type: 0,
                lock_args: vec![0x11; 20],
            }],
            participants: vec![ParticipantDelta {
                lock_hash: vec![0xAA; 32],
                ckb_delta: -500_00000000,
                used_delta: 0,
                item_deltas: vec![ItemDelta {
                    item_id: vec![0xBB; 32],
                    kind: ITEM_KIND_TOKEN,
                    magnitude: 42,
                    negative: false,
                }],
                tags: TAG_TOKEN | TAG_PROTOCOL,
            }],
        };
        let bytes = bincode::serialize(&actions).unwrap();
        let decoded: TxActions = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.tx_hash, vec![0x01; 32]);
        assert_eq!(decoded.block_hash, vec![0xF1; 32]);
        assert_eq!(decoded.block_number, 12345);
        assert_eq!(decoded.tx_index, 3);
        assert!(!decoded.is_cellbase);
        assert_eq!(decoded.protocol_actions.len(), 1);
        assert_eq!(decoded.type_calls.len(), 1);
        assert_eq!(decoded.lock_calls.len(), 1);
        assert_eq!(decoded.participants.len(), 1);
        assert_eq!(decoded.participants[0].ckb_delta, -500_00000000);
        assert_eq!(decoded.participants[0].item_deltas.len(), 1);
        assert_eq!(decoded.participants[0].tags, TAG_TOKEN | TAG_PROTOCOL);
    }

    #[test]
    fn test_tx_actions_empty_roundtrip() {
        let actions = TxActions {
            tx_hash: vec![0x00; 32],
            block_hash: vec![],
            block_number: 0,
            tx_index: 0,
            timestamp: 0,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![],
        };
        let bytes = bincode::serialize(&actions).unwrap();
        let decoded: TxActions = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.block_number, 0);
        assert!(decoded.protocol_actions.is_empty());
        assert!(decoded.type_calls.is_empty());
        assert!(decoded.lock_calls.is_empty());
        assert!(decoded.participants.is_empty());
    }

    #[test]
    fn test_tx_actions_cellbase_roundtrip() {
        let actions = TxActions {
            tx_hash: vec![0x10; 32],
            block_hash: vec![0xF0; 32],
            block_number: 500,
            tx_index: 0,
            timestamp: 1_700_000_000,
            is_cellbase: true,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                lock_hash: vec![0xAA; 32],
                ckb_delta: 100_00000000,
                used_delta: 0,
                item_deltas: vec![],
                tags: TAG_CELLBASE,
            }],
        };
        let bytes = bincode::serialize(&actions).unwrap();
        let decoded: TxActions = bincode::deserialize(&bytes).unwrap();
        assert!(decoded.is_cellbase);
        assert_eq!(decoded.participants.len(), 1);
        assert_eq!(decoded.participants[0].tags, TAG_CELLBASE);
    }

    #[test]
    fn test_tx_actions_multiple_participants_roundtrip() {
        let actions = TxActions {
            tx_hash: vec![0x20; 32],
            block_hash: vec![0xF2; 32],
            block_number: 1000,
            tx_index: 5,
            timestamp: 1_700_000_100,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![
                ParticipantDelta {
                    lock_hash: vec![0xAA; 32],
                    ckb_delta: -200_00000000,
                    used_delta: -100,
                    item_deltas: vec![
                        ItemDelta {
                            item_id: vec![0x11; 32],
                            kind: ITEM_KIND_TOKEN,
                            magnitude: 50,
                            negative: true,
                        },
                        ItemDelta {
                            item_id: vec![0x22; 32],
                            kind: ITEM_KIND_IDENTITY,
                            magnitude: 1,
                            negative: true,
                        },
                    ],
                    tags: TAG_TOKEN | TAG_IDENTITY,
                },
                ParticipantDelta {
                    lock_hash: vec![0xBB; 32],
                    ckb_delta: 200_00000000,
                    used_delta: 100,
                    item_deltas: vec![
                        ItemDelta {
                            item_id: vec![0x11; 32],
                            kind: ITEM_KIND_TOKEN,
                            magnitude: 50,
                            negative: false,
                        },
                        ItemDelta {
                            item_id: vec![0x22; 32],
                            kind: ITEM_KIND_IDENTITY,
                            magnitude: 1,
                            negative: false,
                        },
                    ],
                    tags: TAG_TOKEN | TAG_IDENTITY,
                },
            ],
        };
        let bytes = bincode::serialize(&actions).unwrap();
        let decoded: TxActions = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.participants.len(), 2);
        assert_eq!(decoded.participants[0].ckb_delta, -200_00000000);
        assert_eq!(decoded.participants[1].ckb_delta, 200_00000000);
        assert_eq!(decoded.participants[0].item_deltas.len(), 2);
        assert_eq!(decoded.participants[1].item_deltas.len(), 2);
        // Verify conservation: deltas should sum to zero
        let total_ckb: i128 = decoded.participants.iter().map(|p| p.ckb_delta).sum();
        assert_eq!(total_ckb, 0);
    }

    #[test]
    fn test_tx_actions_with_protocol_metadata_roundtrip() {
        let actions = TxActions {
            tx_hash: vec![0x21; 32],
            block_hash: vec![0x31; 32],
            block_number: 101,
            tx_index: 4,
            timestamp: 1_700_000_001_000,
            is_cellbase: false,
            protocol_actions: vec![ProtocolAction::new(
                "stablepp",
                "deposit",
                serde_json::json!({
                    "hasIntent": true,
                    "vaultCount": 2,
                }),
            )],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                lock_hash: vec![0xAB; 32],
                ckb_delta: 7,
                used_delta: 0,
                item_deltas: vec![],
                tags: TAG_PROTOCOL,
            }],
        };

        let bytes = bincode::serialize(&actions).unwrap();
        let decoded: TxActions = bincode::deserialize(&bytes).unwrap();
        let metadata = decoded.protocol_actions[0].metadata_value().unwrap();
        assert_eq!(metadata["hasIntent"], true);
        assert_eq!(metadata["vaultCount"], 2);
    }

    #[test]
    fn test_tag_constants_are_distinct_bits() {
        let all_tags = [
            TAG_TOKEN,
            TAG_OBJECT,
            TAG_IDENTITY,
            TAG_DAO,
            TAG_PROTOCOL,
            TAG_CELLBASE,
            TAG_TYPE_CALL,
            TAG_LOCK_CALL,
        ];
        // Each tag should be a single bit (power of 2)
        for tag in &all_tags {
            assert!(tag.is_power_of_two(), "tag {tag} is not a power of 2");
        }
        // All tags combined should have no overlap
        let combined: u16 = all_tags.iter().sum();
        let ored: u16 = all_tags.iter().fold(0, |acc, t| acc | t);
        assert_eq!(combined, ored, "tags have overlapping bits");
    }

    #[test]
    fn test_item_kind_constants() {
        assert_eq!(ITEM_KIND_TOKEN, 0);
        assert_eq!(ITEM_KIND_OBJECT, 1);
        assert_eq!(ITEM_KIND_IDENTITY, 2);
        // All distinct
        assert_ne!(ITEM_KIND_TOKEN, ITEM_KIND_OBJECT);
        assert_ne!(ITEM_KIND_OBJECT, ITEM_KIND_IDENTITY);
        assert_ne!(ITEM_KIND_TOKEN, ITEM_KIND_IDENTITY);
    }

    // ---- ObjectStandard ----

    #[test]
    fn test_object_standard_as_str() {
        assert_eq!(ObjectStandard::Spore.as_str(), "spore");
        assert_eq!(ObjectStandard::SporeCluster.as_str(), "spore_cluster");
        assert_eq!(ObjectStandard::MnftIssuer.as_str(), "mnft_issuer");
        assert_eq!(ObjectStandard::MnftClass.as_str(), "mnft_class");
        assert_eq!(ObjectStandard::MnftToken.as_str(), "mnft");
    }

    #[test]
    fn test_object_standard_asset_standard() {
        assert_eq!(ObjectStandard::Spore.asset_standard(), "spore");
        assert_eq!(ObjectStandard::SporeCluster.asset_standard(), "spore");
        assert_eq!(ObjectStandard::MnftIssuer.asset_standard(), "m-nft");
        assert_eq!(ObjectStandard::MnftClass.asset_standard(), "m-nft");
        assert_eq!(ObjectStandard::MnftToken.asset_standard(), "m-nft");
    }

    #[test]
    fn test_object_standard_is_cluster() {
        assert!(!ObjectStandard::Spore.is_cluster());
        assert!(ObjectStandard::SporeCluster.is_cluster());
        assert!(ObjectStandard::MnftIssuer.is_cluster());
        assert!(ObjectStandard::MnftClass.is_cluster());
        assert!(!ObjectStandard::MnftToken.is_cluster());
    }

    // ---- IdentityStandard ----

    #[test]
    fn test_identity_standard_as_str() {
        assert_eq!(IdentityStandard::DotBit.as_str(), "dotbit");
        assert_eq!(IdentityStandard::BitCell.as_str(), "bit_cell");
        assert_eq!(IdentityStandard::DidCkb.as_str(), "did_ckb");
    }

    #[test]
    fn test_identity_standard_asset_standard() {
        assert_eq!(IdentityStandard::DotBit.asset_standard(), "dotbit");
        assert_eq!(IdentityStandard::BitCell.asset_standard(), "bit_cell");
        assert_eq!(IdentityStandard::DidCkb.asset_standard(), "did_ckb");
        assert_eq!(
            IdentityStandard::BitCell.sentinel_collection_id(),
            &BIT_CELL_SENTINEL_COLLECTION
        );
    }

    // ---- Bincode roundtrip: ObjectEntry variants ----

    #[test]
    fn test_object_entry_spore_roundtrip() {
        let entry = ObjectEntry {
            standard: ObjectStandard::Spore,
            collection_id: Some(vec![0xAA; 32]),
            token_id: None,
            owner_lock_hash: Some(vec![0xBB; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 500,
            created_at_tx: vec![0xCC; 32],
            extra: ObjectExtra::Spore {
                content_type: "image/png".to_string(),
                content_length: 4096,
                media_profile: SporeMediaProfile::default(),
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: ObjectEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, ObjectStandard::Spore);
        match decoded.extra {
            ObjectExtra::Spore {
                content_type,
                content_length,
                media_profile,
            } => {
                assert_eq!(content_type, "image/png");
                assert_eq!(content_length, 4096);
                assert_eq!(media_profile.tier, CompositionTier::Unknown);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_object_entry_cluster_roundtrip() {
        let entry = ObjectEntry {
            standard: ObjectStandard::SporeCluster,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0xDD; 32]),
            name: Some("My Cluster".to_string()),
            description: Some("A test cluster".to_string()),
            is_live: true,
            created_at_block: 600,
            created_at_tx: vec![0xEE; 32],
            extra: ObjectExtra::SporeCluster,
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: ObjectEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, ObjectStandard::SporeCluster);
        assert_eq!(decoded.name.as_deref(), Some("My Cluster"));
        assert_eq!(decoded.description.as_deref(), Some("A test cluster"));
        assert!(matches!(decoded.extra, ObjectExtra::SporeCluster));
    }

    #[test]
    fn test_object_entry_mnft_issuer_roundtrip() {
        let entry = ObjectEntry {
            standard: ObjectStandard::MnftIssuer,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0xAA; 32]),
            name: Some("Test Issuer".to_string()),
            description: None,
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![0xBB; 32],
            extra: ObjectExtra::MnftIssuer {
                class_count: 5,
                set_count: 2,
                info: Some(vec![0x01, 0x02]),
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: ObjectEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, ObjectStandard::MnftIssuer);
        assert_eq!(decoded.name.as_deref(), Some("Test Issuer"));
        match decoded.extra {
            ObjectExtra::MnftIssuer {
                class_count,
                set_count,
                info,
            } => {
                assert_eq!(class_count, 5);
                assert_eq!(set_count, 2);
                assert_eq!(info, Some(vec![0x01, 0x02]));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_object_entry_mnft_class_roundtrip() {
        let entry = ObjectEntry {
            standard: ObjectStandard::MnftClass,
            collection_id: Some(vec![0xBB; 32]),
            token_id: None,
            owner_lock_hash: Some(vec![0xCC; 32]),
            name: Some("Test Class".to_string()),
            description: None,
            is_live: true,
            created_at_block: 200,
            created_at_tx: vec![0xDD; 32],
            extra: ObjectExtra::MnftClass {
                description: Some("desc".to_string()),
                renderer: None,
                total: 100,
                issued: 42,
                configure: 0xFF,
                composition_tier: CompositionTier::PureCkb,
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: ObjectEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, ObjectStandard::MnftClass);
        match decoded.extra {
            ObjectExtra::MnftClass {
                total,
                issued,
                configure,
                ..
            } => {
                assert_eq!(total, 100);
                assert_eq!(issued, 42);
                assert_eq!(configure, 0xFF);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_object_entry_mnft_token_roundtrip() {
        let entry = ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: Some(vec![0x11; 32]),
            token_id: Some(vec![0x22; 32]),
            owner_lock_hash: Some(vec![0x33; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 300,
            created_at_tx: vec![0x44; 32],
            extra: ObjectExtra::MnftToken {
                token_index: 7,
                characteristic: vec![0xDE, 0xAD],
                configure: 0x01,
                state: 0x02,
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: ObjectEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, ObjectStandard::MnftToken);
        match decoded.extra {
            ObjectExtra::MnftToken {
                token_index,
                characteristic,
                configure,
                state,
            } => {
                assert_eq!(token_index, 7);
                assert_eq!(characteristic, vec![0xDE, 0xAD]);
                assert_eq!(configure, 0x01);
                assert_eq!(state, 0x02);
            }
            _ => panic!("wrong variant"),
        }
    }

    // ---- Bincode roundtrip: IdentityEntry variants ----

    #[test]
    fn test_identity_entry_dotbit_roundtrip() {
        let entry = IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(vec![0x55; 32]),
            name: Some("test.bit".to_string()),
            is_live: true,
            created_at_block: 400,
            created_at_tx: vec![0x66; 32],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1_700_000_000),
                registered_at: Some(1_600_000_000),
                status: Some(1),
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: IdentityEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, IdentityStandard::DotBit);
        assert_eq!(decoded.name.as_deref(), Some("test.bit"));
        match decoded.extra {
            IdentityExtra::DotBit {
                expired_at,
                registered_at,
                status,
            } => {
                assert_eq!(expired_at, Some(1_700_000_000));
                assert_eq!(registered_at, Some(1_600_000_000));
                assert_eq!(status, Some(1));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_identity_entry_did_ckb_roundtrip() {
        let entry = IdentityEntry {
            standard: IdentityStandard::DidCkb,
            owner_lock_hash: Some(vec![0xFF; 32]),
            name: Some("did:ckb:test".to_string()),
            is_live: true,
            created_at_block: 700,
            created_at_tx: vec![0x11; 32],
            extra: IdentityExtra::DidCkb,
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: IdentityEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, IdentityStandard::DidCkb);
        assert_eq!(decoded.name.as_deref(), Some("did:ckb:test"));
        assert!(matches!(decoded.extra, IdentityExtra::DidCkb));
    }

    #[test]
    fn test_identity_entry_bit_cell_roundtrip() {
        let entry = IdentityEntry {
            standard: IdentityStandard::BitCell,
            owner_lock_hash: Some(vec![0x77; 32]),
            name: Some("20240507.bit".to_string()),
            is_live: true,
            created_at_block: 13_184_726,
            created_at_tx: vec![0x88; 32],
            extra: IdentityExtra::BitCell {
                account_id: vec![0x99; 20],
                expired_at: 1_778_140_699,
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: IdentityEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, IdentityStandard::BitCell);
        assert_eq!(decoded.name.as_deref(), Some("20240507.bit"));
        assert!(matches!(
            decoded.extra,
            IdentityExtra::BitCell {
                account_id,
                expired_at: 1_778_140_699
            } if account_id == vec![0x99; 20]
        ));
    }

    // ---- AddressBalance ----

    #[test]
    fn test_address_balance_roundtrip() {
        let entry = AddressBalance {
            balance: 100_000_000_000,
            used_capacity: 610_000_000_000,
            live_cells_count: 3,
            total_cells_count: 10,
            txs_count: 7,
            first_seen_block: 100,
            first_seen_tx: vec![0x01; 32],
            last_activity_block: 500,
            last_activity_tx: vec![0x02; 32],
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: AddressBalance = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.balance, 100_000_000_000);
        assert_eq!(decoded.used_capacity, 610_000_000_000);
        assert_eq!(decoded.live_cells_count, 3);
        assert_eq!(decoded.total_cells_count, 10);
        assert_eq!(decoded.txs_count, 7);
        assert_eq!(decoded.first_seen_block, 100);
        assert_eq!(decoded.last_activity_block, 500);
    }

    #[test]
    fn test_sync_status_init_sync_start_sets_started_at_for_non_bulk() {
        let mut status = SyncStatus::default();
        status.init_sync_start(128, false);

        assert_eq!(status.sync_started_block, 128);
        assert!(status.sync_started_at.is_some());
    }
}
