//! The immutable view of the node's tx pool that requests read.
//!
//! A snapshot is built by one refresh round and published whole. Nothing here
//! is persisted: the mirror is rebuilt by re-observing the node after a
//! restart, and no field ever reaches a store, a balance, a holder list or a
//! statistic.

use std::collections::HashMap;
use std::sync::Arc;

use ckbadger_store::types::{AddrTxValue, TxActions};
use serde::Serialize;

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
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Proposed => "proposed",
            Self::CommittedAwaitingIndex { .. } => "committed_awaiting_index",
        }
    }
}

/// Why a record's interpretation is incomplete. Always reported; never
/// silently absorbed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartialReason {
    /// The node does not report this outpoint as live and no pool parent
    /// creates it, so the spender's CKB position is not derivable.
    UnresolvedInput { tx_hash: [u8; 32], index: u32 },
    /// A Nervos DAO withdrawal completion: its compensation needs the deposit
    /// and withdrawing header accumulated rates, which the mirror does not read
    /// yet. Layers 1 and 2 are exact; the `dao:withdraw_complete` action is
    /// absent rather than carrying an invented figure.
    DaoCompensationUnavailable,
}

impl PartialReason {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnresolvedInput { .. } => "unresolved_input",
            Self::DaoCompensationUnavailable => "dao_compensation_unavailable",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Self::UnresolvedInput { tx_hash, index } => {
                Some(format!("0x{}:{index}", hex::encode(tx_hash)))
            }
            Self::DaoCompensationUnavailable => None,
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
    pub lock_hash: [u8; 32],
    pub addr_tx: AddrTxValue,
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
    pub last_seen_ms: i64,
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

    pub fn participant(&self, lock_hash: &[u8]) -> Option<&PoolParticipant> {
        self.participants
            .iter()
            .find(|p| p.lock_hash.as_slice() == lock_hash)
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

    pub fn records_for_lock(&self, lock_hash: &[u8]) -> Vec<Arc<PoolTxRecord>> {
        let Ok(lock_hash) = <[u8; 32]>::try_from(lock_hash) else {
            return Vec::new();
        };
        self.by_lock
            .get(&lock_hash)
            .map(|hashes| {
                hashes
                    .iter()
                    .filter_map(|hash| self.records.get(hash).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn pending_count_for_lock(&self, lock_hash: &[u8]) -> usize {
        let Ok(lock_hash) = <[u8; 32]>::try_from(lock_hash) else {
            return 0;
        };
        self.by_lock
            .get(&lock_hash)
            .map(|hashes| hashes.len())
            .unwrap_or(0)
    }
}

/// The `pool` object attached to page-one address list responses.
///
/// Separate from `total`, which stays the committed count: chain truth and
/// provisional state are reported side by side, never summed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InterpretationResponse {
    /// `complete` or `partial`.
    pub status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<InterpretationReasonResponse>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
            reasons: vec![
                PartialReason::UnresolvedInput {
                    tx_hash: [0xAB; 32],
                    index: 3,
                },
                PartialReason::DaoCompensationUnavailable,
            ],
        };
        let response = InterpretationResponse::from(&interpretation);
        assert_eq!(response.status, "partial");
        assert_eq!(response.reasons[0].code, "unresolved_input");
        assert_eq!(
            response.reasons[0].detail.as_deref(),
            Some(format!("0x{}:3", "ab".repeat(32)).as_str())
        );
        assert_eq!(response.reasons[1].code, "dao_compensation_unavailable");
        assert_eq!(response.reasons[1].detail, None);
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
