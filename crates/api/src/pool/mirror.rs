//! The tx-pool mirror: one poll round's diff, and the handle requests read.
//!
//! A pool transaction is a proposed transition that proof-of-work has not
//! confirmed. It is therefore never mixed into balances, holders, statistics or
//! any store — it lives here, in process memory, labelled provisional, and is
//! rebuilt by re-observing the node after a restart.
//!
//! Its *meaning* is produced by the same interpreter the indexer runs
//! (`build_tx_actions_with_production_detectors`), so a transaction reads
//! identically before and after commit; only its truth status changes.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use ckbadger_indexer::db::{
    addr_tx_rows, build_tx_actions_with_production_detectors_with_io, ParticipantIo,
};
use ckbadger_store::types::TxActions;
use ckbadger_store::{read_view, CkbadgerStore};

use super::resolve::{
    fetch_bounded, resolve_pool_tx, resolve_previous_outputs, PoolParentCells, ResolvedCell,
    ResolvedPoolTx,
};
use super::snapshot::{
    Interpretation, MirrorStatus, PartialReason, PoolEntryError, PoolLockScript, PoolParticipant,
    PoolSnapshot, PoolStatus, PoolTxRecord,
};
use super::source::{
    NodeHeader, NodeTxStatus, PoolEntryMeta, PoolSource, PoolTxLookup, RawTxPool, TxPoolInfo,
};

/// Upper bound on any single node call the refresher makes.
///
/// Bounded per CALL, not per round: a round moves the working set out of the
/// refresher (`std::mem::take(&mut self.records)`) and back in at the end, so
/// cancelling a whole round mid-way would lose every record. A call that
/// times out is an `Err` like any other — the round fails over to
/// `publish_unhealthy`, or the one entry is reported — so health is decided by
/// the refresher alone, and the refresher always finishes.
pub const POOL_RPC_TIMEOUT: Duration = Duration::from_secs(15);

/// Run one node call under [`POOL_RPC_TIMEOUT`]; an elapsed deadline becomes
/// the same `Err` shape as a failed call.
async fn timed<T>(
    method: &str,
    call: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::time::timeout(POOL_RPC_TIMEOUT, call)
        .await
        .map_err(|_| format!("{method} timed out after {}s", POOL_RPC_TIMEOUT.as_secs()))?
}

/// The refresher's view of the node: every call through it — its own, and the
/// ones the shared resolver makes on its behalf — is [`timed`].
struct BoundedSource(Arc<dyn PoolSource>);

#[async_trait]
impl PoolSource for BoundedSource {
    async fn tx_pool_info(&self) -> Result<TxPoolInfo, String> {
        timed("tx_pool_info", self.0.tx_pool_info()).await
    }

    async fn raw_tx_pool_verbose(&self) -> Result<RawTxPool, String> {
        timed("get_raw_tx_pool", self.0.raw_tx_pool_verbose()).await
    }

    async fn get_transaction(&self, tx_hash: &[u8; 32]) -> Result<Option<PoolTxLookup>, String> {
        timed("get_transaction", self.0.get_transaction(tx_hash)).await
    }

    async fn get_header(&self, block_hash: &[u8; 32]) -> Result<Option<NodeHeader>, String> {
        timed("get_header", self.0.get_header(block_hash)).await
    }

    async fn get_header_by_number(&self, number: u64) -> Result<Option<NodeHeader>, String> {
        timed("get_header_by_number", self.0.get_header_by_number(number)).await
    }
}

/// The published mirror. Cloneable handle; readers take a snapshot with
/// [`PoolMirror::load`] and never block the refresh loop.
pub struct PoolMirror {
    enabled: bool,
    snapshot: ArcSwap<PoolSnapshot>,
}

impl PoolMirror {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            snapshot: ArcSwap::from_pointee(if enabled {
                PoolSnapshot::initial()
            } else {
                PoolSnapshot::disabled()
            }),
        }
    }

    pub fn disabled() -> Self {
        Self::new(false)
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn load(&self) -> Arc<PoolSnapshot> {
        self.snapshot.load_full()
    }

    /// Publish a whole snapshot atomically. Also the injection point API tests
    /// use to install a deterministic pool without a node.
    pub fn publish(&self, snapshot: PoolSnapshot) {
        self.snapshot.store(Arc::new(snapshot));
    }
}

/// What one refresh round did, for the background-task report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshOutcome {
    /// The pool was unchanged and nothing needed revisiting: the round cost one
    /// `tx_pool_info` call.
    pub skipped: bool,
    pub tracked: usize,
    pub added: usize,
    pub removed: usize,
    pub entry_errors: usize,
    /// The round failed as a whole (the node is unreachable, say). The snapshot
    /// is published with `healthy = false` and this message.
    pub error: Option<String>,
}

pub struct PoolRefresherConfig {
    pub max_tracked_txs: std::num::NonZeroUsize,
    pub is_mainnet: bool,
}

/// Drives one mirror. Holds the working set between rounds; publishes an
/// immutable snapshot at the end of each.
pub struct PoolRefresher {
    source: Arc<dyn PoolSource>,
    store: Arc<CkbadgerStore>,
    mirror: Arc<PoolMirror>,
    config: PoolRefresherConfig,
    records: HashMap<[u8; 32], Arc<PoolTxRecord>>,
    /// The pool membership the node reported last (`get_raw_tx_pool`),
    /// reused as-is by working rounds in which `tx_pool_info` says nothing
    /// moved — so a round that only retries still knows the whole pool.
    pool: HashMap<[u8; 32], (PoolStatus, PoolEntryMeta)>,
    /// Transactions whose interpretation failed deterministically. Kept so a
    /// systematically broken transaction is not re-fetched every poll while it
    /// sits in the pool; its error stays visible in the mirror status.
    failed: HashMap<[u8; 32], String>,
    /// New transactions whose build failed for a reason that passes (the node
    /// did not answer). Retried on a backoff, whether or not the pool moves.
    retry: HashMap<[u8; 32], Backoff>,
    /// Rounds run so far; the clock the backoff counts in.
    round: u64,
    last_pool_updated_at: Option<u64>,
    last_tip_hash: Option<[u8; 32]>,
    /// What the last working round found, republished by idle rounds.
    last_truncated: bool,
    last_entry_errors: Vec<PoolEntryError>,
}

/// The longest a transiently failed transaction waits for its next attempt.
/// Attempts back off 2, 4, 8, 8… rounds.
const RETRY_BACKOFF_MAX_EXPONENT: u32 = 3;

/// When a transiently failed transaction is next attempted.
#[derive(Debug, Clone, Copy)]
struct Backoff {
    /// Consecutive failures, counted up to the backoff cap.
    failures: u32,
    due_round: u64,
}

impl Backoff {
    fn is_due(&self, round: u64) -> bool {
        self.due_round <= round
    }
}

/// Step 1 of the input-resolution order, over the records this round knows:
/// those already tracked, plus those built earlier in this same round (a child
/// that spends its parent's output while both are still in the pool).
struct PoolParentsView<'a> {
    tracked: &'a HashMap<[u8; 32], Arc<PoolTxRecord>>,
    fresh: &'a HashMap<[u8; 32], Vec<ResolvedCell>>,
}

impl PoolParentCells for PoolParentsView<'_> {
    fn output_cell(&self, tx_hash: &[u8; 32], index: u32) -> Option<ResolvedCell> {
        if let Some(outputs) = self.fresh.get(tx_hash) {
            return outputs.get(index as usize).cloned();
        }
        self.tracked
            .get(tx_hash)
            .and_then(|record| record.outputs.get(index as usize).cloned())
    }
}

/// The record with `status`, sharing the allocation when nothing changed.
fn with_status(record: Arc<PoolTxRecord>, status: PoolStatus) -> Arc<PoolTxRecord> {
    if record.pool_status == status {
        return record;
    }
    let mut updated = (*record).clone();
    updated.pool_status = status;
    Arc::new(updated)
}

impl PoolRefresher {
    pub fn new(
        source: Arc<dyn PoolSource>,
        store: Arc<CkbadgerStore>,
        mirror: Arc<PoolMirror>,
        config: PoolRefresherConfig,
    ) -> Self {
        Self {
            source: Arc::new(BoundedSource(source)),
            store,
            mirror,
            config,
            records: HashMap::new(),
            pool: HashMap::new(),
            failed: HashMap::new(),
            retry: HashMap::new(),
            round: 0,
            last_pool_updated_at: None,
            last_tip_hash: None,
            last_truncated: false,
            last_entry_errors: Vec::new(),
        }
    }

    /// Whether this round rebuilds a record whose inputs were unresolvable:
    /// not while it backs off, and never once its build failed for good.
    fn wants_input_retry(&self, record: &PoolTxRecord, round: u64) -> bool {
        record.needs_input_retry()
            && !self.backing_off(&record.tx_hash, round)
            && !self.failed.contains_key(&record.tx_hash)
    }

    /// Whether a transaction is waiting out a backoff this round.
    fn backing_off(&self, hash: &[u8; 32], round: u64) -> bool {
        self.retry
            .get(hash)
            .is_some_and(|backoff| !backoff.is_due(round))
    }

    /// One poll round.
    ///
    /// Never returns `Err`: a failed round publishes an unhealthy snapshot
    /// carrying the reason, because "the mirror is broken" is information the
    /// UI must show, not an error to swallow.
    pub async fn refresh_once(&mut self) -> RefreshOutcome {
        self.round += 1;
        let round = self.round;
        let now_ms = chrono::Utc::now().timestamp_millis();

        let info = match self.source.tx_pool_info().await {
            Ok(info) => info,
            Err(error) => {
                self.publish_unhealthy(now_ms, &error);
                return RefreshOutcome {
                    tracked: self.records.len(),
                    error: Some(error),
                    ..RefreshOutcome::default()
                };
            }
        };

        let pool_unchanged = self.last_pool_updated_at == Some(info.last_txs_updated_at)
            && self.last_tip_hash == Some(info.tip_hash);
        let needs_retry = self
            .records
            .values()
            .any(|r| self.wants_input_retry(r, round));
        let awaiting_index = self
            .records
            .values()
            .any(|r| matches!(r.pool_status, PoolStatus::CommittedAwaitingIndex { .. }));
        let retry_due = self.retry.values().any(|backoff| backoff.is_due(round));

        if pool_unchanged && !needs_retry && !awaiting_index && !retry_due {
            self.publish(
                now_ms,
                &info,
                self.last_entry_errors.clone(),
                self.last_truncated,
            );
            return RefreshOutcome {
                skipped: true,
                tracked: self.records.len(),
                ..RefreshOutcome::default()
            };
        }

        let mut entry_errors: Vec<PoolEntryError> = Vec::new();

        // Step 2. The pool set. Reused when the node says nothing moved: the
        // membership cannot have changed, only our own records' resolution.
        if !pool_unchanged {
            match self.source.raw_tx_pool_verbose().await {
                Ok(raw) => {
                    let mut current =
                        HashMap::with_capacity(raw.pending.len() + raw.proposed.len());
                    for (hash, entry) in raw.pending {
                        current.insert(hash, (PoolStatus::Pending, entry));
                    }
                    for (hash, entry) in raw.proposed {
                        current.insert(hash, (PoolStatus::Proposed, entry));
                    }
                    self.pool = current;
                }
                Err(error) => {
                    self.publish_unhealthy(now_ms, &error);
                    return RefreshOutcome {
                        tracked: self.records.len(),
                        error: Some(error),
                        ..RefreshOutcome::default()
                    };
                }
            }
        }
        // Moved out for the round and put back at its end; a round is never
        // cancelled part-way (every node call is bounded instead).
        let current = std::mem::take(&mut self.pool);

        // Forget what we know about transactions that have left the pool, so
        // the same hash re-entering gets a fresh attempt.
        self.failed.retain(|hash, _| current.contains_key(hash));
        self.retry.retain(|hash, _| current.contains_key(hash));

        // Steps 4 and 5: carry existing records forward — the same allocation
        // when nothing about them changed — and resolve the ones that left.
        let previous = std::mem::take(&mut self.records);
        let mut next: HashMap<[u8; 32], Arc<PoolTxRecord>> = HashMap::with_capacity(previous.len());
        let mut gone: Vec<Arc<PoolTxRecord>> = Vec::new();
        for (hash, record) in previous {
            match current.get(&hash) {
                Some((status, entry)) if record.entry == *entry => {
                    next.insert(hash, with_status(record, *status));
                }
                Some((status, entry)) => {
                    let mut updated = (*record).clone();
                    updated.pool_status = *status;
                    updated.entry = *entry;
                    next.insert(hash, Arc::new(updated));
                }
                None => gone.push(record),
            }
        }
        let removed = self
            .resolve_gone_records(gone, &mut next, &mut entry_errors)
            .await;

        // Step 6, BEFORE any body is fetched: the tracking cap. A transaction
        // outside it is never asked for.
        let (in_scope, truncated) = self.tracked_scope(&current, &next);
        next.retain(|hash, _| in_scope.contains(hash));

        // Step 3: new transactions in scope — unless known-bad or backing off —
        // and records whose inputs were not resolvable last round.
        let mut targets: HashMap<[u8; 32], (PoolStatus, PoolEntryMeta)> = HashMap::new();
        for (hash, (status, entry)) in &current {
            if in_scope.contains(hash)
                && !next.contains_key(hash)
                && !self.failed.contains_key(hash)
                && !self.backing_off(hash, round)
            {
                targets.insert(*hash, (*status, *entry));
            }
        }
        for (hash, record) in &next {
            if self.wants_input_retry(record, round) {
                targets.insert(*hash, (record.pool_status, record.entry));
            }
        }

        let added = self
            .build_records(&targets, now_ms, round, &mut next, &mut entry_errors)
            .await;

        self.records = next;
        self.pool = current;

        // Step 7: publish.
        self.last_pool_updated_at = Some(info.last_txs_updated_at);
        self.last_tip_hash = Some(info.tip_hash);
        self.last_truncated = truncated;
        self.last_entry_errors = entry_errors.clone();
        let entry_error_count = entry_errors.len();
        let tracked = self.records.len();
        self.publish(now_ms, &info, entry_errors, truncated);

        RefreshOutcome {
            skipped: false,
            tracked,
            added,
            removed,
            entry_errors: entry_error_count,
            error: None,
        }
    }

    /// The newest `max_tracked_txs` by time added to pool among every pool
    /// member and every record still held that is not one (committed, awaiting
    /// index). Returns that set and whether anything was left out of it, which
    /// every response then reports as `truncated`.
    fn tracked_scope(
        &self,
        current: &HashMap<[u8; 32], (PoolStatus, PoolEntryMeta)>,
        next: &HashMap<[u8; 32], Arc<PoolTxRecord>>,
    ) -> (HashSet<[u8; 32]>, bool) {
        let mut candidates: Vec<(u64, [u8; 32])> = current
            .iter()
            .map(|(hash, (_, entry))| (entry.time_added_to_pool_ms, *hash))
            .collect();
        candidates.extend(
            next.iter()
                .filter(|(hash, _)| !current.contains_key(*hash))
                .map(|(hash, record)| (record.entry.time_added_to_pool_ms, *hash)),
        );
        let max = self.config.max_tracked_txs.get();
        let truncated = candidates.len() > max;
        if truncated {
            // Newest first; tx hash breaks ties so the kept set is deterministic.
            candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            candidates.truncate(max);
        }
        (
            candidates.into_iter().map(|(_, hash)| hash).collect(),
            truncated,
        )
    }

    /// Records whose transaction is no longer in the pool.
    ///
    /// Node status is authoritative for the transition; the local store is the
    /// ONE condition for dropping a committed record, so a transaction never
    /// vanishes from an address page between node commit and local index.
    async fn resolve_gone_records(
        &self,
        gone: Vec<Arc<PoolTxRecord>>,
        next: &mut HashMap<[u8; 32], Arc<PoolTxRecord>>,
        entry_errors: &mut Vec<PoolEntryError>,
    ) -> usize {
        if gone.is_empty() {
            return 0;
        }

        // A committed record that this process's store has already indexed is
        // dropped without asking the node anything.
        let indexed = match self
            .store_indexed(gone.iter().map(|record| record.tx_hash).collect())
            .await
        {
            Ok(indexed) => indexed,
            Err(error) => {
                // A store read failure must not silently drop records: keep
                // them and report the error.
                for record in &gone {
                    entry_errors.push(PoolEntryError {
                        tx_hash: record.tx_hash,
                        message: error.clone(),
                    });
                }
                for record in gone {
                    next.insert(record.tx_hash, record);
                }
                return 0;
            }
        };

        let mut removed = 0usize;
        let mut to_ask: Vec<Arc<PoolTxRecord>> = Vec::with_capacity(gone.len());
        for record in gone {
            if indexed.contains(&record.tx_hash) {
                removed += 1;
            } else {
                to_ask.push(record);
            }
        }

        let source = self.source.as_ref();
        let lookups = fetch_bounded(
            to_ask.iter().map(|record| record.tx_hash).collect(),
            |hash| async move { source.get_transaction(&hash).await },
        )
        .await;
        // `fetch_bounded` answers in key order, i.e. `to_ask` order.
        for (record, (_, lookup)) in to_ask.into_iter().zip(lookups) {
            match lookup {
                Ok(Some(lookup)) => match lookup.status {
                    NodeTxStatus::Committed => match (lookup.block_number, lookup.block_hash) {
                        (Some(block_number), Some(block_hash)) => {
                            let status = PoolStatus::CommittedAwaitingIndex {
                                block_number,
                                block_hash,
                            };
                            next.insert(record.tx_hash, with_status(record, status));
                        }
                        _ => {
                            entry_errors.push(PoolEntryError {
                                tx_hash: record.tx_hash,
                                message: "node reported committed without block number/hash"
                                    .to_string(),
                            });
                            next.insert(record.tx_hash, record);
                        }
                    },
                    // A reorg returned it to the pool.
                    NodeTxStatus::Pending => {
                        next.insert(record.tx_hash, with_status(record, PoolStatus::Pending));
                    }
                    NodeTxStatus::Proposed => {
                        next.insert(record.tx_hash, with_status(record, PoolStatus::Proposed));
                    }
                    NodeTxStatus::Unknown | NodeTxStatus::Rejected => removed += 1,
                },
                Ok(None) => removed += 1,
                Err(error) => {
                    // A transient RPC failure is not evidence the transaction
                    // is gone; keep the record and report the error.
                    entry_errors.push(PoolEntryError {
                        tx_hash: record.tx_hash,
                        message: error,
                    });
                    next.insert(record.tx_hash, record);
                }
            }
        }
        removed
    }

    /// A build that failed for a reason that passes: report it, and schedule
    /// the next attempt 2, 4, then 8 rounds out (8 thereafter).
    fn transient_failure(
        &mut self,
        hash: [u8; 32],
        round: u64,
        message: String,
        entry_errors: &mut Vec<PoolEntryError>,
    ) {
        let failures = self.retry.get(&hash).map_or(1, |backoff| {
            (backoff.failures + 1).min(RETRY_BACKOFF_MAX_EXPONENT)
        });
        self.retry.insert(
            hash,
            Backoff {
                failures,
                due_round: round + (1u64 << failures),
            },
        );
        entry_errors.push(PoolEntryError {
            tx_hash: hash,
            message,
        });
    }

    /// A build that failed on the transaction itself: it will fail the same
    /// way on every attempt, so it is not attempted again while it stays in
    /// the pool.
    fn permanent_failure(
        &mut self,
        hash: [u8; 32],
        message: String,
        entry_errors: &mut Vec<PoolEntryError>,
    ) {
        self.retry.remove(&hash);
        self.failed.insert(hash, message.clone());
        entry_errors.push(PoolEntryError {
            tx_hash: hash,
            message,
        });
    }

    /// Fetch, resolve and interpret the given transactions.
    ///
    /// A target already present in `next` is a record whose inputs were not
    /// resolvable last round; a rebuilt record replaces it.
    async fn build_records(
        &mut self,
        targets: &HashMap<[u8; 32], (PoolStatus, PoolEntryMeta)>,
        now_ms: i64,
        round: u64,
        next: &mut HashMap<[u8; 32], Arc<PoolTxRecord>>,
        entry_errors: &mut Vec<PoolEntryError>,
    ) -> usize {
        if targets.is_empty() {
            return 0;
        }

        // Fetch bodies with bounded concurrency.
        let mut hashes: Vec<[u8; 32]> = targets.keys().copied().collect();
        hashes.sort_unstable();
        let source = self.source.clone();
        let lookups = fetch_bounded(hashes, |hash| {
            let source = source.clone();
            async move { source.get_transaction(&hash).await }
        })
        .await;
        let mut bodies: HashMap<[u8; 32], ckb_store_reader::RpcTransactionView> = HashMap::new();
        for (hash, result) in lookups {
            match result {
                Ok(Some(lookup)) => match (lookup.transaction, lookup.status) {
                    (Some(tx), _) => {
                        bodies.insert(hash, tx);
                    }
                    // What a real node answers for a transaction that left the
                    // pool between `get_raw_tx_pool` and here: normal churn,
                    // picked up (or not) by the next round.
                    (None, NodeTxStatus::Unknown | NodeTxStatus::Rejected) => {
                        self.retry.remove(&hash);
                    }
                    (None, status) => self.transient_failure(
                        hash,
                        round,
                        format!("node reported {status:?} for a pool transaction without its body"),
                        entry_errors,
                    ),
                },
                // Unknown to the node: the same churn.
                Ok(None) => {
                    self.retry.remove(&hash);
                }
                Err(error) => self.transient_failure(hash, round, error, entry_errors),
            }
        }

        // Build parents before children so a chained unconfirmed spend resolves
        // from its parent's already-built outputs (step 1 of the resolution
        // order) instead of costing another `get_transaction`.
        let order = topological_order(&bodies);

        let mut fresh_outputs: HashMap<[u8; 32], Vec<ResolvedCell>> = HashMap::new();
        let mut added = 0usize;
        for hash in order {
            let Some(tx) = bodies.get(&hash) else {
                continue;
            };
            let Some((status, entry)) = targets.get(&hash).copied() else {
                continue;
            };

            let resolution = {
                let parents = PoolParentsView {
                    tracked: next,
                    fresh: &fresh_outputs,
                };
                resolve_previous_outputs(self.source.as_ref(), tx, &parents).await
            };
            let previous_outputs = match resolution {
                Ok(cells) => cells,
                Err(error) => {
                    self.transient_failure(hash, round, error, entry_errors);
                    continue;
                }
            };

            let resolved = match resolve_pool_tx(tx, &previous_outputs) {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.permanent_failure(hash, error, entry_errors);
                    continue;
                }
            };

            let existing = next.get(&hash);
            let is_retry = existing.is_some();
            let first_seen_ms = match existing {
                Some(record) => record.first_seen_ms,
                None => now_ms,
            };

            match build_record(
                &resolved,
                status,
                entry,
                first_seen_ms,
                self.config.is_mainnet,
            ) {
                Ok(record) => {
                    self.retry.remove(&hash);
                    fresh_outputs.insert(hash, record.outputs.clone());
                    if !is_retry {
                        added += 1;
                    }
                    next.insert(hash, Arc::new(record));
                }
                Err(error) => self.permanent_failure(hash, error, entry_errors),
            }
        }
        added
    }

    /// Which of these transactions this process's store view already has.
    ///
    /// Runs on a blocking thread under a pinned read view, so it can never
    /// collide with the secondary catch-up window.
    async fn store_indexed(&self, hashes: Vec<[u8; 32]>) -> Result<HashSet<[u8; 32]>, String> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let _view = read_view::acquire_read();
            let mut indexed = HashSet::new();
            for hash in hashes {
                match store.get_tx_by_hash(&hash) {
                    Ok(Some(_)) => {
                        indexed.insert(hash);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        return Err(format!(
                            "store lookup for pool tx 0x{} failed: {e}",
                            hex::encode(hash)
                        ))
                    }
                }
            }
            Ok(indexed)
        })
        .await
        .map_err(|e| format!("pool store lookup task failed: {e}"))?
    }

    fn publish(
        &self,
        now_ms: i64,
        info: &TxPoolInfo,
        entry_errors: Vec<PoolEntryError>,
        truncated: bool,
    ) {
        self.mirror.publish(PoolSnapshot::from_records(
            self.records.values().cloned().collect(),
            MirrorStatus {
                enabled: true,
                healthy: true,
                last_polled_at_ms: Some(now_ms),
                last_pool_updated_at: Some(info.last_txs_updated_at),
                tip_number: Some(info.tip_number),
                truncated,
                last_error: None,
                entry_errors,
                ..MirrorStatus::default()
            },
        ));
    }

    /// Publish the records we still hold, marked unhealthy with the reason.
    ///
    /// The rows stay (they were true when last observed) but every response
    /// says the pool view is unavailable, so nothing reads "0 pending" off a
    /// node we cannot reach.
    fn publish_unhealthy(&self, now_ms: i64, error: &str) {
        let previous = self.mirror.load();
        self.mirror.publish(PoolSnapshot {
            records: previous.records.clone(),
            by_lock: previous.by_lock.clone(),
            by_lock_prefix: previous.by_lock_prefix.clone(),
            status: MirrorStatus {
                enabled: true,
                healthy: false,
                last_polled_at_ms: Some(now_ms),
                last_error: Some(error.to_string()),
                ..previous.status.clone()
            },
        });
    }
}

/// Parents before children, so a chained unconfirmed spend can resolve from the
/// parent's outputs. Pool ancestry is acyclic; anything left over after the
/// sweep (which cannot happen for a well-formed pool) is appended so it is
/// still attempted.
fn topological_order(
    bodies: &HashMap<[u8; 32], ckb_store_reader::RpcTransactionView>,
) -> Vec<[u8; 32]> {
    let mut ordered: Vec<[u8; 32]> = Vec::with_capacity(bodies.len());
    let mut placed: HashSet<[u8; 32]> = HashSet::with_capacity(bodies.len());
    let mut remaining: Vec<[u8; 32]> = bodies.keys().copied().collect();
    remaining.sort();

    loop {
        let mut progressed = false;
        remaining.retain(|hash| {
            let Some(tx) = bodies.get(hash) else {
                return false;
            };
            let parents_ready = tx.inputs.iter().all(|input| {
                let parent = input
                    .previous_output
                    .tx_hash
                    .strip_prefix("0x")
                    .unwrap_or(&input.previous_output.tx_hash);
                match hex::decode(parent)
                    .ok()
                    .and_then(|bytes| <[u8; 32]>::try_from(bytes.as_slice()).ok())
                {
                    Some(parent) => !bodies.contains_key(&parent) || placed.contains(&parent),
                    // Unparseable here is not fatal: the resolver reports it
                    // with full context when it builds the record.
                    None => true,
                }
            });
            if parents_ready {
                ordered.push(*hash);
                placed.insert(*hash);
                progressed = true;
                false
            } else {
                true
            }
        });
        if remaining.is_empty() || !progressed {
            break;
        }
    }
    ordered.extend(remaining);
    ordered
}

/// Turn a resolved pool transaction into a record: interpretation, per-
/// participant position, semantic tags, and an explicit statement of what could
/// not be interpreted.
fn build_record(
    resolved: &ResolvedPoolTx,
    status: PoolStatus,
    entry: PoolEntryMeta,
    first_seen_ms: i64,
    is_mainnet: bool,
) -> Result<PoolTxRecord, String> {
    let interpretation = interpretation_of(resolved);

    let zero_block_hash = [0u8; 32];
    let time_added_ms = i64::try_from(entry.time_added_to_pool_ms).map_err(|_| {
        format!(
            "pool entry timestamp {} exceeds i64",
            entry.time_added_to_pool_ms
        )
    })?;

    let built = match resolved.tx_view(&zero_block_hash, time_added_ms) {
        Some(view) => {
            let mut built = build_tx_actions_with_production_detectors_with_io(&[view], is_mainnet)
                .map_err(|e| format!("activity interpretation failed: {e}"))?;
            Some(built.pop().ok_or_else(|| {
                "activity interpretation returned no actions for one transaction".to_string()
            })?)
        }
        // An unresolved input means the spender's position is not derivable.
        // No interpretation is published rather than a partial-input one.
        None => None,
    };

    let participants = match built.as_ref() {
        Some(built) => participants_from(&built.actions, &built.participant_io)?,
        None => Vec::new(),
    };
    let actions = built.map(|built| built.actions);

    let mut input_locks: Vec<PoolLockScript> = Vec::new();
    for cell in resolved
        .inputs
        .iter()
        .filter_map(|input| input.cell.as_ref())
    {
        let lock_hash = <[u8; 32]>::try_from(cell.lock_script_hash.as_slice()).map_err(|_| {
            format!(
                "resolved input lock hash is {} bytes, not 32",
                cell.lock_script_hash.len()
            )
        })?;
        if input_locks.iter().all(|lock| lock.lock_hash != lock_hash) {
            input_locks.push(PoolLockScript {
                lock_hash,
                code_hash: cell.lock_code_hash.clone(),
                hash_type: cell.lock_hash_type,
                args: cell.lock_args.clone(),
            });
        }
    }

    let inputs_count = i16::try_from(resolved.inputs.len()).map_err(|_| {
        format!(
            "pool tx has {} inputs, exceeding i16",
            resolved.inputs.len()
        )
    })?;
    let outputs_count = i16::try_from(resolved.outputs.len()).map_err(|_| {
        format!(
            "pool tx has {} outputs, exceeding i16",
            resolved.outputs.len()
        )
    })?;

    Ok(PoolTxRecord {
        tx_hash: resolved.tx_hash,
        pool_status: status,
        entry,
        outputs: resolved.outputs.clone(),
        input_locks,
        actions,
        participants,
        inputs_count,
        outputs_count,
        semantic_tags: resolved.semantic_tags(),
        is_cellbase: resolved.is_cellbase,
        interpretation,
        first_seen_ms,
    })
}

/// Per-participant `addr_txs` row, through the indexer's own derivation.
///
/// Not a second definition: [`addr_tx_rows`] is the single one, so a pool
/// transaction and the same transaction once committed carry byte-identical
/// `AddrTxValue`s — including for a party the protocol named that holds no cell.
pub(super) fn participants_from(
    actions: &TxActions,
    io: &[ParticipantIo],
) -> Result<Vec<PoolParticipant>, String> {
    Ok(addr_tx_rows(actions, io)
        .map_err(|e| format!("pool participant rows: {e}"))?
        .into_iter()
        .map(|(id, addr_tx)| PoolParticipant { id, addr_tx })
        .collect())
}

/// How completely a resolved transaction could be interpreted.
///
/// The ONE place that judgement is made: the mirror records it on every pool
/// record, and `/tx/{hash}` reports the same verdict for the same transaction.
pub fn interpretation_of(resolved: &ResolvedPoolTx) -> Interpretation {
    let reasons: Vec<PartialReason> = resolved
        .unresolved_inputs()
        .into_iter()
        .map(|(tx_hash, index)| PartialReason::UnresolvedInput { tx_hash, index })
        .collect();
    if reasons.is_empty() {
        Interpretation::Complete
    } else {
        Interpretation::Partial { reasons }
    }
}
