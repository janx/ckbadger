//! The immutable view of the node's tx pool that requests read.
//!
//! A snapshot is built by one refresh round and published whole. Nothing here
//! is persisted: the mirror is rebuilt by re-observing the node after a
//! restart, and no field ever reaches a store, a balance, a holder list or a
//! statistic.

use std::collections::HashMap;
use std::sync::Arc;

use ckbadger_store::types::{AddrTxValue, ParticipantId, TxActions};
use serde::{Deserialize, Serialize};

use super::resolve::ResolvedCell;
use super::source::PoolEntryMeta;

/// Where a mirrored transaction stands, as the node reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolStatus {
    Pending,
    Proposed,
    /// The node has committed it, but this process's store view has not indexed
    /// it yet. Keeping the record for that window is what stops a transaction
    /// from vanishing off the address page between commit and index.
    CommittedAwaitingIndex {
        block_number: i64,
        block_hash: [u8; 32],
    },
}

impl PoolStatus {
    /// The wire value for a transaction the node has committed and the local
    /// store has not indexed yet.
    pub const COMMITTED_AWAITING_INDEX: &'static str = "committed_awaiting_index";

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Proposed => "proposed",
            Self::CommittedAwaitingIndex { .. } => Self::COMMITTED_AWAITING_INDEX,
        }
    }
}

/// Why a record's interpretation is incomplete. Always reported; never
/// silently absorbed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartialReason {
    /// Neither the mirror nor the node knows the transaction that created this
    /// outpoint, so the spender's CKB position is not derivable.
    UnresolvedInput { tx_hash: [u8; 32], index: u32 },
}

impl PartialReason {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnresolvedInput { .. } => "unresolved_input",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Self::UnresolvedInput { tx_hash, index } => {
                Some(format!("0x{}:{index}", hex::encode(tx_hash)))
            }
        }
    }
}

/// How completely this transaction could be interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Interpretation {
    Complete,
    Partial { reasons: Vec<PartialReason> },
}

impl Interpretation {
    pub fn is_partial(&self) -> bool {
        matches!(self, Self::Partial { .. })
    }

    pub fn reasons(&self) -> &[PartialReason] {
        match self {
            Self::Complete => &[],
            Self::Partial { reasons } => reasons,
        }
    }
}

/// One participant's position in a pool transaction, pre-computed through the
/// same `AddrTxValue::new` constructor the indexer uses for committed rows, so
/// a pool row's `txType` and `capacityChange` cannot drift from what the same
/// transaction will show once indexed.
#[derive(Debug, Clone)]
pub struct PoolParticipant {
    pub id: ParticipantId,
    pub addr_tx: AddrTxValue,
}

/// A lock script exactly as the node reported it: enough to encode its
/// owner's address without a store lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolLockScript {
    pub lock_hash: [u8; 32],
    pub code_hash: Vec<u8>,
    pub hash_type: i16,
    pub args: Vec<u8>,
}

/// One mirrored transaction.
#[derive(Debug, Clone)]
pub struct PoolTxRecord {
    pub tx_hash: [u8; 32],
    pub pool_status: PoolStatus,
    /// The node's own exact pool-entry values (fee, size, cycles, ...).
    pub entry: PoolEntryMeta,
    /// This transaction's outputs, kept so a chained child in the same pool can
    /// resolve its inputs without asking the node for a cell that does not
    /// exist on chain yet.
    pub outputs: Vec<ResolvedCell>,
    /// The lock scripts of the cells this transaction spends — one per
    /// distinct lock, in input order, for every input that resolved. With
    /// `outputs` this covers every party that holds a cell in the
    /// transaction, including a sender whose lock exists so far only as the
    /// output of another pending transaction (no store row has it yet).
    pub input_locks: Vec<PoolLockScript>,
    /// The interpretation, built by the indexer's activity builder. `None` when
    /// an input could not be resolved: a position derived from a partial input
    /// set would be wrong, and a wrong number is worse than a missing one.
    pub actions: Option<TxActions>,
    pub participants: Vec<PoolParticipant>,
    pub inputs_count: i16,
    pub outputs_count: i16,
    pub semantic_tags: u16,
    pub is_cellbase: bool,
    pub interpretation: Interpretation,
    pub first_seen_ms: i64,
}

impl PoolTxRecord {
    /// Whether the next refresh should try to interpret this record again.
    ///
    /// Only unresolved inputs are retried: they are transient (the node's live
    /// set moves). A DAO-compensation gap is structural in v1 and retrying it
    /// would spend RPC on an answer that cannot change.
    pub fn needs_input_retry(&self) -> bool {
        self.actions.is_none()
    }

    /// The full lock script behind `lock_hash`, from this transaction's own
    /// cells (outputs first, then spent inputs).
    pub fn lock_script(&self, lock_hash: &[u8; 32]) -> Option<PoolLockScript> {
        self.outputs
            .iter()
            .find(|cell| cell.lock_script_hash.as_slice() == lock_hash.as_slice())
            .map(|cell| PoolLockScript {
                lock_hash: *lock_hash,
                code_hash: cell.lock_code_hash.clone(),
                hash_type: cell.lock_hash_type,
                args: cell.lock_args.clone(),
            })
            .or_else(|| {
                self.input_locks
                    .iter()
                    .find(|lock| lock.lock_hash == *lock_hash)
                    .cloned()
            })
    }

    /// This lock's participation, through the one participant matcher: a
    /// `Lock` compares whole, a protocol-named prefix compares its 20 bytes.
    pub fn participant(&self, lock_hash: &[u8; 32]) -> Option<&PoolParticipant> {
        self.participants.iter().find(|p| p.id.matches(lock_hash))
    }
}

/// One node value the mirror could not make sense of, kept per entry so a
/// single malformed transaction is visible without taking the rest down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolEntryError {
    pub tx_hash: [u8; 32],
    pub message: String,
}

/// What the mirror knows about itself. Every response that shows pool data
/// carries enough of this for the UI to say "pool view unavailable" instead of
/// "0 pending".
#[derive(Debug, Clone, Default)]
pub struct MirrorStatus {
    pub enabled: bool,
    pub healthy: bool,
    pub last_polled_at_ms: Option<i64>,
    pub last_pool_updated_at: Option<u64>,
    pub tip_number: Option<u64>,
    pub pending: usize,
    pub proposed: usize,
    pub awaiting_index: usize,
    pub partial: usize,
    pub truncated: bool,
    pub last_error: Option<String>,
    pub entry_errors: Vec<PoolEntryError>,
}

/// The published view: records by hash, an index by participant lock, and the
/// mirror's own health.
#[derive(Debug, Clone, Default)]
pub struct PoolSnapshot {
    pub records: HashMap<[u8; 32], Arc<PoolTxRecord>>,
    /// Participant lock hash → transaction hashes, newest `time_added_to_pool`
    /// first. A pool transaction is by definition later than every committed
    /// one, so this order is the order the API serves pool rows in.
    pub by_lock: HashMap<[u8; 32], Vec<[u8; 32]>>,
    /// Same index for parties a protocol named by a 20-byte lock-hash prefix.
    /// A prefix party has no full hash, so it cannot live in `by_lock`; every
    /// per-address lookup merges the two, exactly as the committed
    /// `list_addr_txs_recent` merges its two column families.
    pub by_lock_prefix: HashMap<[u8; 20], Vec<[u8; 32]>>,
    pub status: MirrorStatus,
}

impl PoolSnapshot {
    /// The snapshot a disabled mirror publishes: no records, and `enabled`
    /// false so responses can say so rather than implying an empty pool.
    pub fn disabled() -> Self {
        Self::default()
    }

    /// The snapshot an enabled mirror starts from, before its first poll:
    /// enabled but not yet healthy, so the UI shows "pool view unavailable"
    /// rather than "0 pending".
    pub fn initial() -> Self {
        Self {
            status: MirrorStatus {
                enabled: true,
                healthy: false,
                ..MirrorStatus::default()
            },
            ..Self::default()
        }
    }

    /// Build a snapshot from records: the participant index, its ordering, and
    /// the status counts are derived here and nowhere else, so the refresh loop
    /// and any test fixture produce the same shape.
    ///
    /// `by_lock` is newest `time_added_to_pool` first. A pool transaction can
    /// only land in a future block, so it sorts above every committed row
    /// regardless of block timestamps; the tx hash breaks ties so the order is
    /// deterministic.
    pub fn from_records(records: Vec<Arc<PoolTxRecord>>, status: MirrorStatus) -> Self {
        let mut status = status;
        status.pending = 0;
        status.proposed = 0;
        status.awaiting_index = 0;
        status.partial = 0;

        // Each entry carries its own sort key, so ordering never depends on a
        // lookup that could miss and quietly sort by a default.
        let mut by_lock: HashMap<[u8; 32], Vec<(u64, [u8; 32])>> = HashMap::new();
        let mut by_lock_prefix: HashMap<[u8; 20], Vec<(u64, [u8; 32])>> = HashMap::new();
        let mut by_hash: HashMap<[u8; 32], Arc<PoolTxRecord>> =
            HashMap::with_capacity(records.len());

        for record in records {
            match record.pool_status {
                PoolStatus::Pending => status.pending += 1,
                PoolStatus::Proposed => status.proposed += 1,
                PoolStatus::CommittedAwaitingIndex { .. } => status.awaiting_index += 1,
            }
            if record.interpretation.is_partial() {
                status.partial += 1;
            }
            for participant in &record.participants {
                match participant.id {
                    ParticipantId::Lock(lock_hash) => by_lock
                        .entry(lock_hash)
                        .or_default()
                        .push((record.entry.time_added_to_pool_ms, record.tx_hash)),
                    ParticipantId::LockPrefix(prefix) => by_lock_prefix
                        .entry(prefix)
                        .or_default()
                        .push((record.entry.time_added_to_pool_ms, record.tx_hash)),
                }
            }
            by_hash.insert(record.tx_hash, record);
        }

        fn sort_newest_first<K: std::hash::Hash + Eq>(
            index: HashMap<K, Vec<(u64, [u8; 32])>>,
        ) -> HashMap<K, Vec<[u8; 32]>> {
            index
                .into_iter()
                .map(|(key, mut entries)| {
                    entries.sort_by(|(a_time, a_hash), (b_time, b_hash)| {
                        b_time.cmp(a_time).then_with(|| a_hash.cmp(b_hash))
                    });
                    (key, entries.into_iter().map(|(_, hash)| hash).collect())
                })
                .collect()
        }

        Self {
            records: by_hash,
            by_lock: sort_newest_first(by_lock),
            by_lock_prefix: sort_newest_first(by_lock_prefix),
            status,
        }
    }

    /// Both identity forms of one address, newest `time_added_to_pool` first.
    ///
    /// A (party, tx) pair reaches exactly one of the two indexes — the builder's
    /// merge pass already unified same-tx appearances — so this is a merge, not
    /// a dedup.
    fn tx_hashes_for_lock(&self, lock_hash: &[u8; 32]) -> Vec<[u8; 32]> {
        let empty: &[[u8; 32]] = &[];
        let by_lock = self.by_lock.get(lock_hash).map_or(empty, Vec::as_slice);
        let prefix: [u8; 20] = lock_hash[..20]
            .try_into()
            .expect("a 32-byte lock hash always has a 20-byte prefix");
        let by_prefix = self
            .by_lock_prefix
            .get(&prefix)
            .map_or(empty, Vec::as_slice);
        if by_prefix.is_empty() {
            return by_lock.to_vec();
        }
        let mut merged: Vec<[u8; 32]> = Vec::with_capacity(by_lock.len() + by_prefix.len());
        merged.extend_from_slice(by_lock);
        merged.extend_from_slice(by_prefix);
        merged.sort_by(|a, b| {
            let time_of = |hash: &[u8; 32]| {
                self.records
                    .get(hash)
                    .map(|record| record.entry.time_added_to_pool_ms)
            };
            time_of(b).cmp(&time_of(a)).then_with(|| a.cmp(b))
        });
        merged
    }

    pub fn records_for_lock(&self, lock_hash: &[u8]) -> Vec<Arc<PoolTxRecord>> {
        let Ok(lock_hash) = <[u8; 32]>::try_from(lock_hash) else {
            return Vec::new();
        };
        self.tx_hashes_for_lock(&lock_hash)
            .iter()
            .filter_map(|hash| self.records.get(hash).cloned())
            .collect()
    }

    pub fn pending_count_for_lock(&self, lock_hash: &[u8]) -> usize {
        let Ok(lock_hash) = <[u8; 32]>::try_from(lock_hash) else {
            return 0;
        };
        self.tx_hashes_for_lock(&lock_hash).len()
    }
}

/// A published snapshot answers step 1 of the input-resolution order: the
/// outputs of transactions that are themselves still in the pool.
impl super::resolve::PoolParentCells for PoolSnapshot {
    fn output_cell(&self, tx_hash: &[u8; 32], index: u32) -> Option<ResolvedCell> {
        self.records
            .get(tx_hash)
            .and_then(|record| record.outputs.get(index as usize).cloned())
    }
}

/// The `pool` object attached to page-one address list responses.
///
/// Separate from `total`, which stays the committed count: chain truth and
/// provisional state are reported side by side, never summed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PoolSummaryResponse {
    pub enabled: bool,
    pub healthy: bool,
    pub last_polled_at: Option<String>,
    /// Pool rows served for this address on this page.
    pub count: usize,
    /// Net CKB this address would gain or lose if every one of those pool
    /// transactions were committed. Shown apart from Balance, never added to it.
    pub pending_ckb_delta: String,
    /// The pool segment may be incomplete — either this address has more pool
    /// rows than one page carries, or the mirror itself is at its tracking cap.
    pub truncated: bool,
}

/// The interpretation attached to a pool row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InterpretationResponse {
    /// `complete` or `partial`.
    pub status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<InterpretationReasonResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InterpretationReasonResponse {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl From<&Interpretation> for InterpretationResponse {
    fn from(interpretation: &Interpretation) -> Self {
        Self {
            status: match interpretation {
                Interpretation::Complete => "complete".to_string(),
                Interpretation::Partial { .. } => "partial".to_string(),
            },
            reasons: interpretation
                .reasons()
                .iter()
                .map(|reason| InterpretationReasonResponse {
                    code: reason.code().to_string(),
                    detail: reason.detail(),
                })
                .collect(),
        }
    }
}

/// Format a pool timestamp (milliseconds) as RFC 3339, or report it as the
/// out-of-range value it is rather than substituting the epoch.
pub fn pool_timestamp_rfc3339(timestamp_ms: i64) -> Result<String, String> {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.to_rfc3339())
        .ok_or_else(|| format!("pool timestamp {timestamp_ms} ms is out of range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled_snapshot_reports_disabled_not_empty() {
        let snapshot = PoolSnapshot::disabled();
        assert!(!snapshot.status.enabled);
        assert!(!snapshot.status.healthy);
    }

    #[test]
    fn test_initial_snapshot_is_enabled_but_not_yet_healthy() {
        let snapshot = PoolSnapshot::initial();
        assert!(snapshot.status.enabled);
        assert!(
            !snapshot.status.healthy,
            "a mirror that has not polled yet must not claim an empty pool"
        );
    }

    #[test]
    fn test_interpretation_response_carries_reason_detail() {
        let interpretation = Interpretation::Partial {
            reasons: vec![PartialReason::UnresolvedInput {
                tx_hash: [0xAB; 32],
                index: 3,
            }],
        };
        let response = InterpretationResponse::from(&interpretation);
        assert_eq!(response.status, "partial");
        assert_eq!(response.reasons.len(), 1);
        assert_eq!(response.reasons[0].code, "unresolved_input");
        assert_eq!(
            response.reasons[0].detail.as_deref(),
            Some(format!("0x{}:3", "ab".repeat(32)).as_str())
        );
    }

    #[test]
    fn test_pool_timestamp_out_of_range_is_an_error_not_the_epoch() {
        assert_eq!(
            pool_timestamp_rfc3339(0).unwrap(),
            "1970-01-01T00:00:00+00:00"
        );
        assert!(pool_timestamp_rfc3339(i64::MAX).is_err());
    }
}
