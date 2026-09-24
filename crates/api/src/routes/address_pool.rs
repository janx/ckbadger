//! An address's tx-pool segment: the ONE place that decides which pool
//! records belong to a lock's page one, in what order, and what they add up to.
//!
//! Three responses show it — the activity feed and the transaction list (as
//! page-one rows plus their `pool` summary, after each list's own filter) and
//! the address detail (as `pendingSummary`, never filtered) — and all three
//! read it from [`page_one_segment`], so a pending transaction cannot count in
//! one place and not in another.

use ckbadger_store::CkbadgerStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::pool::{PoolParticipant, PoolSnapshot, PoolSummaryResponse, PoolTxRecord};

/// How many tx-pool rows one address's page one may carry.
///
/// A bound, not a sample: beyond it the segment reports `truncated`, so a
/// caller is never shown a silently partial pool segment.
pub(crate) const POOL_ROWS_PER_ADDRESS: usize = 200;

/// One pool transaction on a lock's page one, with the lock's part in it.
pub(crate) struct PoolSegmentRow {
    pub record: Arc<PoolTxRecord>,
    pub participant: PoolParticipant,
}

impl PoolSegmentRow {
    /// This lock's net CKB change in the transaction, in shannons — the
    /// participant's `AddrTxValue`, built by the same constructor the indexer
    /// uses for the committed row.
    pub fn capacity_change(&self) -> i64 {
        self.participant.addr_tx.capacity_change
    }
}

/// A lock's page-one pool segment.
pub(crate) struct PoolSegment {
    /// Newest `time_added_to_pool` first; already-indexed transactions removed.
    pub rows: Vec<PoolSegmentRow>,
    /// The lock has more pool records than one page carries, or the mirror
    /// itself is at its tracking cap.
    pub truncated: bool,
}

/// Build a lock's page-one pool segment.
///
/// One dedup rule, committed wins: a transaction this request's pinned store
/// view already has is served from the committed segment, not from here.
///
/// Blocking (reads the store).
pub(crate) fn page_one_segment(
    store: &CkbadgerStore,
    snapshot: &PoolSnapshot,
    lock_hash: &[u8; 32],
) -> anyhow::Result<PoolSegment> {
    let mut records = snapshot.records_for_lock(lock_hash);
    let over_cap = records.len() > POOL_ROWS_PER_ADDRESS;
    records.truncate(POOL_ROWS_PER_ADDRESS);

    let mut rows = Vec::with_capacity(records.len());
    for record in records {
        if store.get_tx_by_hash(&record.tx_hash)?.is_some() {
            continue;
        }
        // `by_lock` is built from the record's participants, so a record
        // reached through it must have an entry for this lock. A missing one
        // is a mirror invariant violation, not a row to skip quietly.
        let participant = record.participant(lock_hash).cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "pool record indexed by lock 0x{} has no participant for it: tx=0x{}",
                hex::encode(lock_hash),
                hex::encode(record.tx_hash)
            )
        })?;
        rows.push(PoolSegmentRow {
            record,
            participant,
        });
    }

    Ok(PoolSegment {
        rows,
        truncated: over_cap || snapshot.status.truncated,
    })
}

/// The `pool` object for a page-one list response, over the rows that list
/// actually serves (after its own filter).
pub(crate) fn pool_summary<'a>(
    snapshot: &PoolSnapshot,
    served: impl IntoIterator<Item = &'a PoolSegmentRow>,
    truncated: bool,
) -> anyhow::Result<PoolSummaryResponse> {
    let (count, pending_ckb_delta) = count_and_delta(served);
    let last_polled_at = snapshot
        .status
        .last_polled_at_ms
        .map(|ms| {
            crate::pool::pool_timestamp_rfc3339(ms)
                .map_err(|e| anyhow::anyhow!("pool mirror last_polled_at: {e}"))
        })
        .transpose()?;
    Ok(PoolSummaryResponse {
        enabled: snapshot.status.enabled,
        healthy: snapshot.status.healthy,
        last_polled_at,
        count,
        pending_ckb_delta: pending_ckb_delta.to_string(),
        truncated,
    })
}

/// What an address has pending in the tx pool, whatever a list filters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSummaryResponse {
    /// Pool transactions involving the address and not yet indexed.
    pub tx_count: usize,
    /// Signed net change of the address's capacity across them, in shannons.
    pub capacity_delta: String,
}

/// An address's pending summary: every row of its page-one pool segment, with
/// no activity filter applied. `None` when the mirror is disabled or unhealthy
/// — then there is no pool view to summarise, and "0 pending" would be a guess.
///
/// Blocking (reads the store).
pub(crate) fn pending_summary(
    store: &CkbadgerStore,
    snapshot: &PoolSnapshot,
    lock_hash: &[u8; 32],
) -> anyhow::Result<Option<PendingSummaryResponse>> {
    if !(snapshot.status.enabled && snapshot.status.healthy) {
        return Ok(None);
    }
    let segment = page_one_segment(store, snapshot, lock_hash)?;
    let (tx_count, capacity_delta) = count_and_delta(&segment.rows);
    Ok(Some(PendingSummaryResponse {
        tx_count,
        capacity_delta: capacity_delta.to_string(),
    }))
}

fn count_and_delta<'a>(rows: impl IntoIterator<Item = &'a PoolSegmentRow>) -> (usize, i128) {
    rows.into_iter().fold((0, 0i128), |(count, delta), row| {
        (count + 1, delta + i128::from(row.capacity_change()))
    })
}
