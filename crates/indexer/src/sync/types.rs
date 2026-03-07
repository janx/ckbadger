use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::db::ReorgResult;
use crate::parser::{
    ParsedDotbitAccountOutput, ParsedMnftClass, ParsedMnftIssuer, ParsedMnftToken,
};

// ── UndoSeqScope constants & enum ──────────────────────────────────────

pub(crate) const UNDO_SEQ_SCOPE_SHIFT: u32 = 48;
pub(crate) const UNDO_SEQ_LOCAL_MAX: u64 = (1u64 << UNDO_SEQ_SCOPE_SHIFT) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum UndoSeqScope {
    TxContext = 0x0001,
    AppendAddrTx = 0x0002,
    AppendActivity = 0x0003,
    AppendNftCollectionActivity = 0x0004,
}

// ── Sync / Reorg action enums ──────────────────────────────────────────

pub(crate) enum SyncAction {
    CaughtUp,
    Continue,
    ReorgHandled,
    DeepForkPaused,
}

#[allow(dead_code)]
pub(crate) enum ReorgAction {
    Handled(ReorgResult),
    DeepForkPaused,
}

// ── Pre-parsed NFT / DotBit bridge types ───────────────────────────────

/// Pre-parsed mNFT/DotBit data computed in the parser stage.
/// Moves all CPU-intensive parsing out of the t6b writer thread.
pub(crate) struct PreParsedNftData {
    pub(crate) mnft_issuers: Vec<(usize, ParsedMnftIssuer)>,
    pub(crate) mnft_classes: Vec<(usize, usize, ParsedMnftClass)>,
    pub(crate) mnft_tokens: Vec<(usize, usize, ParsedMnftToken)>,
    pub(crate) dotbit_accounts: Vec<(usize, ParsedDotbitAccountOutput)>,
    pub(crate) consumed_dotbit: Vec<DotbitConsumptionEvent>,
    /// DAS action string per transaction (tx_global_index -> action).
    pub(crate) dotbit_tx_actions: HashMap<usize, String>,
}

pub(crate) struct DotbitConsumptionEvent {
    pub(crate) account_id: Vec<u8>,
    pub(crate) block_number: i64,
    pub(crate) consuming_tx_hash: [u8; 32],
    pub(crate) tx_idx: i32,
    pub(crate) ts_ms: i64,
}

/// Per-tx .bit activity data for direct collection activity writes.
pub(crate) struct DotbitTxActivityData {
    pub(crate) das_action: Option<String>,
    pub(crate) created_account_ids: HashSet<Vec<u8>>,
    pub(crate) consumed_account_ids: HashSet<Vec<u8>>,
    pub(crate) block_number: i64,
    pub(crate) tx_idx: i32,
    pub(crate) timestamp_ms: i64,
}

// ── XUDT extension ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct XudtExtensionScript {
    pub(crate) args: Vec<u8>,
}

// ── Cell caches ────────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct CachedCellInfo {
    pub(crate) capacity: i64,
    pub(crate) created_at_block: i64,
    pub(crate) lock_script_hash: Vec<u8>,
    pub(crate) lock_code_hash: Vec<u8>,
    pub(crate) lock_hash_type: i16,
    pub(crate) lock_args: Vec<u8>,
    pub(crate) type_script_hash: Option<Vec<u8>>,
    pub(crate) type_code_hash: Option<Vec<u8>>,
    pub(crate) type_args: Option<Vec<u8>>,
    pub(crate) data_size: i32,
    pub(crate) occupied_capacity: i64,
    pub(crate) udt_amount: Option<u128>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct CachedUdtCellInfo {
    pub(crate) type_script_hash: Vec<u8>,
    pub(crate) type_code_hash: Vec<u8>,
    pub(crate) type_hash_type: i16,
    pub(crate) type_args: Vec<u8>,
    pub(crate) lock_script_hash: Vec<u8>,
    pub(crate) amount: u128,
    pub(crate) standard: String,
}

// ── Transaction data ───────────────────────────────────────────────────

pub(crate) struct TxData {
    pub(crate) hash: [u8; 32],
    pub(crate) block_number: i64,
    pub(crate) tx_index: i32,
    pub(crate) inputs_count: i16,
    pub(crate) outputs_count: i16,
    pub(crate) is_cellbase: bool,
    pub(crate) inputs: Vec<crate::parser::transaction::ParsedInput>,
    pub(crate) cells: Vec<crate::parser::cell::ParsedCell>,
    pub(crate) witnesses: Vec<String>,
    pub(crate) outputs_data: Vec<String>,
    pub(crate) total_input_capacity: i64,
    pub(crate) total_output_capacity: i64,
    pub(crate) fee: i64,
    pub(crate) tx_size: i32,
    pub(crate) cycles: Option<i64>,
    pub(crate) timestamp: DateTime<Utc>,
}

// ── Batch write metrics ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BatchWriteMetrics {
    pub(crate) commit_ms: f64,
    pub(crate) write_ms: f64,
    pub(crate) txs: u64,
    pub(crate) cells: u64,
    pub(crate) inputs: u64,
    pub(crate) t1_ms: f64,
    pub(crate) t_act_ms: f64,
}

// ── Unresolved outpoint probe summaries ────────────────────────────────

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct UnresolvedLocalProbeSummary {
    pub(crate) sampled: usize,
    pub(crate) live_hits: usize,
    pub(crate) consumed_hits: usize,
    pub(crate) tx_location_hits: usize,
    pub(crate) missing_everywhere: usize,
    pub(crate) store_errors: usize,
    pub(crate) sample_details: Vec<String>,
}

impl UnresolvedLocalProbeSummary {
    pub(crate) fn format_for_log(&self) -> String {
        format!(
            "sampled={} live_hits={} consumed_hits={} tx_location_hits={} missing_everywhere={} store_errors={} sample=[{}]",
            self.sampled,
            self.live_hits,
            self.consumed_hits,
            self.tx_location_hits,
            self.missing_everywhere,
            self.store_errors,
            self.sample_details.join(", ")
        )
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct UnresolvedRpcProbeSummary {
    pub(crate) sampled_tx_hashes: usize,
    pub(crate) committed: usize,
    pub(crate) pending: usize,
    pub(crate) proposed: usize,
    pub(crate) rejected: usize,
    pub(crate) unknown_status: usize,
    pub(crate) rpc_null: usize,
    pub(crate) rpc_errors: usize,
    pub(crate) sample_details: Vec<String>,
}

impl UnresolvedRpcProbeSummary {
    pub(crate) fn format_for_log(&self) -> String {
        format!(
            "sampled_tx_hashes={} committed={} pending={} proposed={} rejected={} unknown_status={} rpc_null={} rpc_errors={} sample=[{}]",
            self.sampled_tx_hashes,
            self.committed,
            self.pending,
            self.proposed,
            self.rejected,
            self.unknown_status,
            self.rpc_null,
            self.rpc_errors,
            self.sample_details.join(", ")
        )
    }
}
