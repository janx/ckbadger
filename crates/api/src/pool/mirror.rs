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
use std::sync::Arc;

use arc_swap::ArcSwap;
use ckbadger_indexer::db::build_tx_actions_with_production_detectors;
use ckbadger_store::types::{AddrTxValue, TxActions};
use ckbadger_store::{read_view, CkbadgerStore};

use super::resolve::{
    resolve_pool_tx, resolve_previous_outputs, PoolParentCells, ResolvedCell, ResolvedPoolTx,
};
use super::snapshot::{
    Interpretation, MirrorStatus, PartialReason, PoolEntryError, PoolParticipant, PoolSnapshot,
    PoolStatus, PoolTxRecord,
};
use super::source::{NodeTxStatus, PoolEntryMeta, PoolSource, TxPoolInfo};

/// How many `get_transaction` calls are in flight at once while picking up new
/// pool transactions.
const TX_FETCH_CONCURRENCY: usize = 16;

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
    pub max_tracked_txs: usize,
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
    /// Transactions whose interpretation failed. Kept so a systematically
    /// broken transaction is not re-fetched every poll while it sits in the
    /// pool; its error stays visible in the mirror status.
    failed: HashMap<[u8; 32], String>,
    last_pool_updated_at: Option<u64>,
    last_tip_hash: Option<[u8; 32]>,
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

impl PoolRefresher {
    pub fn new(
        source: Arc<dyn PoolSource>,
        store: Arc<CkbadgerStore>,
        mirror: Arc<PoolMirror>,
        config: PoolRefresherConfig,
    ) -> Self {
        Self {
            source,
            store,
            mirror,
            config,
            records: HashMap::new(),
            failed: HashMap::new(),
            last_pool_updated_at: None,
            last_tip_hash: None,
        }
    }

    /// One poll round.
    ///
    /// Never returns `Err`: a failed round publishes an unhealthy snapshot
    /// carrying the reason, because "the mirror is broken" is information the
    /// UI must show, not an error to swallow.
    pub async fn refresh_once(&mut self) -> RefreshOutcome {
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
        let needs_retry = self.records.values().any(|r| r.needs_input_retry());
        let awaiting_index = self
            .records
            .values()
            .any(|r| matches!(r.pool_status, PoolStatus::CommittedAwaitingIndex { .. }));

        if pool_unchanged && !needs_retry && !awaiting_index {
            self.publish(now_ms, &info, Vec::new(), false);
            return RefreshOutcome {
                skipped: true,
                tracked: self.records.len(),
                ..RefreshOutcome::default()
            };
        }

        let mut entry_errors: Vec<PoolEntryError> = Vec::new();

        // Step 2. The pool set. Skipped when the node says nothing moved: the
        // membership cannot have changed, only our own records' resolution.
        let current = if pool_unchanged {
            self.records
                .iter()
                .filter(|(_, record)| {
                    !matches!(
                        record.pool_status,
                        PoolStatus::CommittedAwaitingIndex { .. }
                    )
                })
                .map(|(hash, record)| (*hash, (record.pool_status, record.entry)))
                .collect::<HashMap<_, _>>()
        } else {
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
                    current
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
        };

        // Forget interpretation failures for transactions that have left the
        // pool, so the same hash re-entering gets a fresh attempt.
        self.failed.retain(|hash, _| current.contains_key(hash));

        // Steps 4 and 5: carry existing records forward, resolve the ones that
        // left the pool.
        let previous = std::mem::take(&mut self.records);
        let mut next: HashMap<[u8; 32], Arc<PoolTxRecord>> = HashMap::with_capacity(current.len());
        let mut gone: Vec<Arc<PoolTxRecord>> = Vec::new();
        for (hash, record) in previous {
            match current.get(&hash) {
                Some((status, entry)) => {
                    let mut updated = (*record).clone();
                    updated.pool_status = *status;
                    updated.entry = *entry;
                    updated.last_seen_ms = now_ms;
                    next.insert(hash, Arc::new(updated));
                }
                None => gone.push(record),
            }
        }
        let removed = self
            .resolve_gone_records(gone, now_ms, &mut next, &mut entry_errors)
            .await;

        // Step 3: new transactions, and retries of records whose inputs were
        // not resolvable last round.
        let mut to_build: Vec<[u8; 32]> = current
            .keys()
            .copied()
            .filter(|hash| !next.contains_key(hash) && !self.failed.contains_key(hash))
            .collect();
        let retries: Vec<[u8; 32]> = next
            .iter()
            .filter(|(_, record)| record.needs_input_retry())
            .map(|(hash, _)| *hash)
            .collect();
        to_build.extend(retries.iter().copied());

        let added = self
            .build_records(
                &to_build,
                &retries.iter().copied().collect::<HashSet<_>>(),
                &current,
                now_ms,
                &mut next,
                &mut entry_errors,
            )
            .await;

        self.records = next;

        // Step 6: the tracking cap. Newest by time added to pool are kept.
        let truncated = self.enforce_cap();

        // Step 7: publish.
        self.last_pool_updated_at = Some(info.last_txs_updated_at);
        self.last_tip_hash = Some(info.tip_hash);
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

    /// Records whose transaction is no longer in the pool.
    ///
    /// Node status is authoritative for the transition; the local store is the
    /// ONE condition for dropping a committed record, so a transaction never
    /// vanishes from an address page between node commit and local index.
    async fn resolve_gone_records(
        &mut self,
        gone: Vec<Arc<PoolTxRecord>>,
        now_ms: i64,
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
        for record in gone {
            if indexed.contains(&record.tx_hash) {
                removed += 1;
                continue;
            }

            match self.source.get_transaction(&record.tx_hash).await {
                Ok(Some(lookup)) => match lookup.status {
                    NodeTxStatus::Committed => match (lookup.block_number, lookup.block_hash) {
                        (Some(block_number), Some(block_hash)) => {
                            let mut updated = (*record).clone();
                            updated.pool_status = PoolStatus::CommittedAwaitingIndex {
                                block_number,
                                block_hash,
                            };
                            updated.last_seen_ms = now_ms;
                            next.insert(updated.tx_hash, Arc::new(updated));
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
                    NodeTxStatus::Pending | NodeTxStatus::Proposed => {
                        // A reorg returned it to the pool.
                        let mut updated = (*record).clone();
                        updated.pool_status = if lookup.status == NodeTxStatus::Pending {
                            PoolStatus::Pending
                        } else {
                            PoolStatus::Proposed
                        };
                        updated.last_seen_ms = now_ms;
                        next.insert(updated.tx_hash, Arc::new(updated));
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

    /// Fetch, resolve and interpret the given transactions.
    ///
    /// `retries` are hashes already present in `next` whose inputs were not
    /// resolvable last round; a rebuilt record replaces the old one.
    async fn build_records(
        &mut self,
        hashes: &[[u8; 32]],
        retries: &HashSet<[u8; 32]>,
        current: &HashMap<[u8; 32], (PoolStatus, PoolEntryMeta)>,
        now_ms: i64,
        next: &mut HashMap<[u8; 32], Arc<PoolTxRecord>>,
        entry_errors: &mut Vec<PoolEntryError>,
    ) -> usize {
        if hashes.is_empty() {
            return 0;
        }

        // Fetch bodies with bounded concurrency.
        let source = self.source.clone();
        let mut bodies: HashMap<[u8; 32], ckb_store_reader::RpcTransactionView> = HashMap::new();
        for chunk in hashes.chunks(TX_FETCH_CONCURRENCY) {
            let fetched = futures::future::join_all(chunk.iter().map(|hash| {
                let source = source.clone();
                async move { (*hash, source.get_transaction(hash).await) }
            }))
            .await;
            for (hash, result) in fetched {
                match result {
                    Ok(Some(lookup)) => match lookup.transaction {
                        Some(tx) => {
                            bodies.insert(hash, tx);
                        }
                        None => entry_errors.push(PoolEntryError {
                            tx_hash: hash,
                            message: "node returned a pool transaction without a body".to_string(),
                        }),
                    },
                    // Left the pool between `get_raw_tx_pool` and here: normal
                    // churn, picked up (or not) by the next round.
                    Ok(None) => {}
                    Err(error) => entry_errors.push(PoolEntryError {
                        tx_hash: hash,
                        message: error,
                    }),
                }
            }
        }

        // Build parents before children so a chained unconfirmed spend resolves
        // from its parent's outputs instead of asking the node for a cell that
        // does not exist on chain.
        let order = topological_order(&bodies);

        let mut fresh_outputs: HashMap<[u8; 32], Vec<ResolvedCell>> = HashMap::new();
        let mut added = 0usize;
        for hash in order {
            let Some(tx) = bodies.get(&hash) else {
                continue;
            };
            let Some((status, entry)) = current.get(&hash).copied() else {
                continue;
            };

            let parents = PoolParentsView {
                tracked: next,
                fresh: &fresh_outputs,
            };
            let previous_outputs =
                match resolve_previous_outputs(self.source.as_ref(), tx, &parents).await {
                    Ok(cells) => cells,
                    Err(error) => {
                        entry_errors.push(PoolEntryError {
                            tx_hash: hash,
                            message: error,
                        });
                        continue;
                    }
                };

            let resolved = match resolve_pool_tx(tx, &previous_outputs) {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.failed.insert(hash, error.clone());
                    entry_errors.push(PoolEntryError {
                        tx_hash: hash,
                        message: error,
                    });
                    continue;
                }
            };

            let first_seen_ms = next
                .get(&hash)
                .map(|record| record.first_seen_ms)
                .unwrap_or(now_ms);

            match build_record(
                &resolved,
                status,
                entry,
                first_seen_ms,
                now_ms,
                self.config.is_mainnet,
            ) {
                Ok(record) => {
                    fresh_outputs.insert(hash, record.outputs.clone());
                    if !retries.contains(&hash) {
                        added += 1;
                    }
                    next.insert(hash, Arc::new(record));
                }
                Err(error) => {
                    self.failed.insert(hash, error.clone());
                    entry_errors.push(PoolEntryError {
                        tx_hash: hash,
                        message: error,
                    });
                }
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

    /// Keep the newest `max_tracked_txs` by time added to pool. Returns whether
    /// anything was dropped, which every response then reports as `truncated`.
    fn enforce_cap(&mut self) -> bool {
        if self.records.len() <= self.config.max_tracked_txs {
            return false;
        }
        let mut ordered: Vec<([u8; 32], u64)> = self
            .records
            .iter()
            .map(|(hash, record)| (*hash, record.entry.time_added_to_pool_ms))
            .collect();
        // Newest first; tx hash breaks ties so the kept set is deterministic.
        ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let keep: HashSet<[u8; 32]> = ordered
            .into_iter()
            .take(self.config.max_tracked_txs)
            .map(|(hash, _)| hash)
            .collect();
        self.records.retain(|hash, _| keep.contains(hash));
        true
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
                    .unwrap_or("");
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
    now_ms: i64,
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

    let actions = match resolved.tx_view(&zero_block_hash, time_added_ms) {
        Some(view) => {
            let mut built = build_tx_actions_with_production_detectors(&[view], is_mainnet)
                .map_err(|e| format!("activity interpretation failed: {e}"))?;
            Some(built.pop().ok_or_else(|| {
                "activity interpretation returned no actions for one transaction".to_string()
            })?)
        }
        // An unresolved input means the spender's position is not derivable.
        // No interpretation is published rather than a partial-input one.
        None => None,
    };

    let participants = match actions.as_ref() {
        Some(actions) => participants_from(actions, resolved)?,
        None => Vec::new(),
    };

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
        actions,
        participants,
        inputs_count,
        outputs_count,
        semantic_tags: resolved.semantic_tags(),
        is_cellbase: resolved.is_cellbase,
        interpretation,
        first_seen_ms,
        last_seen_ms: now_ms,
    })
}

/// Per-participant `AddrTxValue`, built through the same constructor the
/// indexer uses for committed `addr_txs` rows.
fn participants_from(
    actions: &TxActions,
    resolved: &ResolvedPoolTx,
) -> Result<Vec<PoolParticipant>, String> {
    let has_input: HashSet<&[u8]> = resolved
        .inputs
        .iter()
        .filter_map(|input| input.cell.as_ref())
        .map(|cell| cell.lock_script_hash.as_slice())
        .collect();
    let has_output: HashSet<&[u8]> = resolved
        .outputs
        .iter()
        .map(|cell| cell.lock_script_hash.as_slice())
        .collect();

    actions
        .participants
        .iter()
        .map(|participant| {
            let lock_hash = <[u8; 32]>::try_from(participant.lock_hash.as_slice())
                .map_err(|_| "participant lock hash is not 32 bytes".to_string())?;
            let capacity_change = i64::try_from(participant.ckb_delta).map_err(|_| {
                format!(
                    "participant ckb_delta {} exceeds i64 for lock 0x{}",
                    participant.ckb_delta,
                    hex::encode(lock_hash)
                )
            })?;
            Ok(PoolParticipant {
                lock_hash,
                addr_tx: AddrTxValue::new(
                    capacity_change,
                    has_input.contains(participant.lock_hash.as_slice()),
                    has_output.contains(participant.lock_hash.as_slice()),
                    participant.tags,
                ),
            })
        })
        .collect()
}

/// How completely a resolved transaction could be interpreted.
///
/// The ONE place that judgement is made: the mirror records it on every pool
/// record, and `/tx/{hash}` reports the same verdict for the same transaction.
pub fn interpretation_of(resolved: &ResolvedPoolTx) -> Interpretation {
    let mut reasons: Vec<PartialReason> = resolved
        .unresolved_inputs()
        .into_iter()
        .map(|(tx_hash, index)| PartialReason::UnresolvedInput { tx_hash, index })
        .collect();
    if resolved.completes_dao_withdrawal() {
        reasons.push(PartialReason::DaoCompensationUnavailable);
    }
    if reasons.is_empty() {
        Interpretation::Complete
    } else {
        Interpretation::Partial { reasons }
    }
}
