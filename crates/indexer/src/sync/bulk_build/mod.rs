#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use anyhow::{anyhow, bail};
use ckbadger_store::keys;
use ckbadger_store::store::CF_TOKEN_TRANSFERS;
use ckbadger_store::types::{
    decode_live_cell_marker, BulkBuildSessionMarker, CachedBlockHeader,
    CellDistributionTrackerState, ConsumedCellMeta, DailyActivityStats, DailyAddressCohort,
    DailyCellDistribution, HodlTrackerState, LiveCellInfo, ObjectStandard, SporeTypeIndex,
    SyncStatus, TokenTransferRecord, TxActivityBundle, TxIndexEntry,
    DID_CKB_SENTINEL_COLLECTION, DOTBIT_SENTINEL_COLLECTION, SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::{
    AddressBalance, CkbadgerStore, ScriptInfo, CF_ACTIVITIES, CF_ADDR_TXS, CF_BLOCK_HASH_INDEX,
    CF_BLOCK_HEADERS, CF_CELLS, CF_CELL_BY_DATA_HASH, CF_CELL_BY_LOCK, CF_CELL_BY_LOCK_CODE,
    CF_CELL_BY_TYPE, CF_CELL_BY_TYPE_CODE, CF_CONSUMED_CELLS, CF_IDENTITY_COLLECTION_ACTIVITIES,
    CF_LIVE_CELLS, CF_OBJECT_COLLECTION_ACTIVITIES, CF_STATS_CHAIN, CF_STATS_HODL,
    CF_TX_HASH_MAP, CF_TX_INDEX,
};
use rocksdb::IteratorMode;
use tracing::info;

use super::indexer::Indexer;
use crate::parser::cell::ParsedCell;
use crate::parser::{ParsedUdtCell, ScriptParser, UdtParser, UdtStandard};
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::owners::BulkReducer;

pub(crate) mod facts;
pub(crate) mod interner;
pub(crate) mod live_cells;
pub(crate) mod materialize;
pub(crate) mod owners;
pub(crate) mod sequencer;

#[derive(Default)]
pub(crate) struct BulkBuildEngine;

impl BulkBuildEngine {
    pub(crate) async fn run(indexer: &Indexer) -> Result<()> {
        let start_block = i64::try_from(indexer.progress.current()).map_err(|_| {
            anyhow!(
                "bulk build start block exceeds i64 range: current_block={}",
                indexer.progress.current()
            )
        })?;
        start_bulk_build_session_marker(
            indexer.writer.store().as_ref(),
            &indexer.run_id,
            start_block,
        )?;

        // Temporary routing seam: startup bulk sync now has an explicit build-engine
        // entrypoint, while the underlying execution still delegates to the existing
        // pipeline until reducers/materialization land in later tasks.
        info!(
            run_id = %indexer.run_id,
            "Bulk build engine route selected; delegating to pipeline until build engine materialization is implemented"
        );
        let result = indexer.run_pipeline().await;
        if result.is_ok() {
            indexer.writer.store().clear_bulk_build_session_marker()?;
        }
        result
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct BatchExecutionStats {
    last_block_number: Option<i64>,
    last_block_hash: Option<Vec<u8>>,
    block_count: u64,
    tx_count: u64,
    cells_created: i64,
    cells_consumed: i64,
}

impl BatchExecutionStats {
    fn is_empty(&self) -> bool {
        self.block_count == 0
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct BulkBuildSyncTotals {
    last_block_number: Option<i64>,
    last_block_hash: Option<Vec<u8>>,
    total_transactions: i64,
    total_cells_created: i64,
    total_cells_consumed: i64,
}

impl BulkBuildSyncTotals {
    fn record_batch(&mut self, stats: &BatchExecutionStats) -> Result<()> {
        if stats.is_empty() {
            return Ok(());
        }

        self.last_block_number = stats.last_block_number;
        self.last_block_hash = stats.last_block_hash.clone();
        self.total_transactions = checked_add_sync_total(
            "total_transactions",
            self.total_transactions,
            i64::try_from(stats.tx_count).map_err(|_| {
                anyhow!(
                    "bulk build tx_count exceeds i64 range while recording batch sync totals: tx_count={}",
                    stats.tx_count
                )
            })?,
            self.last_block_number.unwrap_or_default(),
        )?;
        self.total_cells_created = checked_add_sync_total(
            "total_cells_created",
            self.total_cells_created,
            stats.cells_created,
            self.last_block_number.unwrap_or_default(),
        )?;
        self.total_cells_consumed = checked_add_sync_total(
            "total_cells_consumed",
            self.total_cells_consumed,
            stats.cells_consumed,
            self.last_block_number.unwrap_or_default(),
        )?;
        Ok(())
    }

    fn finalize_success(self, store: &CkbadgerStore) -> Result<SyncStatus> {
        let mut status = store.get_sync_status()?;

        if let Some(last_block_number) = self.last_block_number {
            status.tip_block_number = last_block_number;
        }
        if let Some(last_block_hash) = self.last_block_hash {
            status.tip_block_hash = last_block_hash;
        }

        status.total_transactions = checked_add_sync_total(
            "total_transactions",
            status.total_transactions,
            self.total_transactions,
            status.tip_block_number,
        )?;
        status.total_cells_created = checked_add_sync_total(
            "total_cells_created",
            status.total_cells_created,
            self.total_cells_created,
            status.tip_block_number,
        )?;
        status.total_cells_consumed = checked_add_sync_total(
            "total_cells_consumed",
            status.total_cells_consumed,
            self.total_cells_consumed,
            status.tip_block_number,
        )?;
        status.last_synced_at = chrono::Utc::now().timestamp();
        status.mark_bulk_sync_completed(status.tip_block_number);
        store.set_sync_status(&status)?;
        Ok(status)
    }
}

fn checked_add_sync_total(
    label: &str,
    current: i64,
    delta: i64,
    block_number: i64,
) -> Result<i64> {
    current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "bulk build sync total overflow: field={} current={} delta={} block={}",
            label,
            current,
            delta,
            block_number
        )
    })
}

fn start_bulk_build_session_marker(
    store: &CkbadgerStore,
    run_id: &str,
    start_block: i64,
) -> Result<BulkBuildSessionMarker> {
    let marker = BulkBuildSessionMarker {
        run_id: run_id.to_string(),
        started_at: chrono::Utc::now().timestamp(),
        start_block,
    };
    store.set_bulk_build_session_marker(Some(&marker))?;
    Ok(marker)
}

#[derive(Default)]
struct CoreOwners {
    address: owners::address::AddressOwner,
    script: owners::script::ScriptOwner,
    token: owners::token::TokenOwner,
    dao: owners::dao::DaoOwner,
    object: owners::object::ObjectOwner,
}

impl CoreOwners {
    fn apply_tx(
        &mut self,
        tx: &facts::ResolvedTxFacts,
        ctx: &owners::ReducerContext<'_>,
    ) -> Result<()> {
        self.apply_tx_and_return_address_deltas(tx, ctx).map(|_| ())
    }

    fn apply_tx_and_return_address_deltas(
        &mut self,
        tx: &facts::ResolvedTxFacts,
        ctx: &owners::ReducerContext<'_>,
    ) -> Result<HashMap<Vec<u8>, owners::address::AddressTxDelta>> {
        let address_deltas = self.address.apply_tx_with_deltas(tx, ctx)?;
        self.script.apply_tx(tx, ctx)?;
        self.token.apply_tx(tx, ctx)?;
        self.dao.apply_tx(tx, ctx)?;
        self.object.apply_tx(tx, ctx)?;
        Ok(address_deltas)
    }

    fn materialize_all(&mut self, materializer: &mut materialize::Materializer<'_>) -> Result<()> {
        self.address.flush_sealed(materializer)?;
        self.script.flush_sealed(materializer)?;
        self.token.flush_sealed(materializer)?;
        self.dao.flush_sealed(materializer)?;
        self.object.flush_sealed(materializer)?;

        self.address.materialize_final(materializer)?;
        self.script.materialize_final(materializer)?;
        self.token.materialize_final(materializer)?;
        self.dao.materialize_final(materializer)?;
        self.object.materialize_final(materializer)?;
        Ok(())
    }
}

#[derive(Default)]
struct ActivityStatsAccumulator {
    daily_stats: HashMap<String, DailyActivityStats>,
    daily_addrs: HashMap<String, HashSet<[u8; 32]>>,
    hourly_stats: HashMap<String, DailyActivityStats>,
    hourly_addrs: HashMap<String, HashSet<[u8; 32]>>,
}

impl ActivityStatsAccumulator {
    fn apply_history_rows(&mut self, history_rows: &[materialize::MaterializedRow]) -> Result<()> {
        for row in history_rows
            .iter()
            .filter(|row| row.cf_name == CF_ACTIVITIES)
        {
            let bundle: TxActivityBundle = bincode::deserialize(&row.value).map_err(|e| {
                anyhow!(
                    "failed to deserialize TxActivityBundle while building sealed bulk activity stats: key=0x{} error={}",
                    hex::encode(&row.key),
                    e
                )
            })?;
            let date = ckbadger_common::block_date_from_ms(bundle.timestamp)
                .format("%Y%m%d")
                .to_string();
            let hour = ckbadger_common::block_datetime_from_ms(bundle.timestamp)
                .format("%Y%m%d%H")
                .to_string();

            for owner in &bundle.owners {
                crate::db::BatchWriter::accumulate_owner_activity_stats(
                    bundle.is_cellbase,
                    owner,
                    self.daily_stats.entry(date.clone()).or_default(),
                );
                crate::db::BatchWriter::accumulate_owner_activity_stats(
                    bundle.is_cellbase,
                    owner,
                    self.hourly_stats.entry(hour.clone()).or_default(),
                );

                if !bundle.is_cellbase && owner.lock_hash.len() == 32 {
                    let mut lock_hash = [0u8; 32];
                    lock_hash.copy_from_slice(&owner.lock_hash);
                    self.daily_addrs
                        .entry(date.clone())
                        .or_default()
                        .insert(lock_hash);
                    self.hourly_addrs
                        .entry(hour.clone())
                        .or_default()
                        .insert(lock_hash);
                }
            }
        }

        Ok(())
    }

    fn build_rows(&self) -> Result<Vec<materialize::MaterializedRow>> {
        let mut rows = Vec::with_capacity(self.daily_stats.len() + self.hourly_stats.len());

        let mut daily_entries = self
            .daily_stats
            .iter()
            .map(|(date, stats)| (date.clone(), stats.clone()))
            .collect::<Vec<_>>();
        daily_entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (date, mut stats) in daily_entries {
            stats.unique_address_count = self
                .daily_addrs
                .get(&date)
                .map_or(0, |set| set.len() as u32);
            rows.push(materialize::MaterializedRow::new(
                CF_STATS_CHAIN,
                keys::encode_stats_key(keys::stats_prefix::ACTIVITY_DAILY, date.as_bytes()),
                bincode::serialize(&stats)?,
            ));
        }

        let mut hourly_entries = self
            .hourly_stats
            .iter()
            .map(|(hour, stats)| (hour.clone(), stats.clone()))
            .collect::<Vec<_>>();
        hourly_entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (hour, mut stats) in hourly_entries {
            stats.unique_address_count = self
                .hourly_addrs
                .get(&hour)
                .map_or(0, |set| set.len() as u32);
            rows.push(materialize::MaterializedRow::new(
                CF_STATS_CHAIN,
                keys::encode_stats_key(keys::stats_prefix::ACTIVITY_HOURLY, hour.as_bytes()),
                bincode::serialize(&stats)?,
            ));
        }

        Ok(rows)
    }
}

struct BulkBuildRuntimeState {
    interner: interner::IdentityInterner,
    sequencer: sequencer::BulkSequencer,
    owners: CoreOwners,
    activity_stats: ActivityStatsAccumulator,
    hodl_tracker: crate::db::writer::hodl_wave::HodlWaveTracker,
    cell_dist_tracker: crate::db::writer::cell_distribution::CellDistributionTracker,
    hodl_live_cells_by_lock: HashMap<crate::sync::types::InternId, i32>,
}

impl Default for BulkBuildRuntimeState {
    fn default() -> Self {
        Self {
            interner: interner::IdentityInterner::default(),
            sequencer: sequencer::BulkSequencer::default(),
            owners: CoreOwners::default(),
            activity_stats: ActivityStatsAccumulator::default(),
            hodl_tracker: crate::db::writer::hodl_wave::HodlWaveTracker::new(),
            cell_dist_tracker: crate::db::writer::cell_distribution::CellDistributionTracker::new(),
            hodl_live_cells_by_lock: HashMap::new(),
        }
    }
}

impl BulkBuildRuntimeState {
    fn apply_blocks(
        &mut self,
        blocks: &[BlockResponseWithCycles],
        domain_store: &CkbadgerStore,
        materializer: &mut materialize::Materializer<'_>,
        is_mainnet: bool,
    ) -> Result<BatchExecutionStats> {
        if blocks.is_empty() {
            return Ok(BatchExecutionStats::default());
        }

        let arena =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(blocks, &mut self.interner)?;
        let resolved = self.sequencer.resolve(&arena)?;
        let tx_count = u64::try_from(arena.txs.len()).map_err(|_| {
            anyhow!(
                "bulk build tx count exceeds u64 range while applying block batch: txs={}",
                arena.txs.len()
            )
        })?;
        let cells_created = i64::try_from(arena.cells.len()).map_err(|_| {
            anyhow!(
                "bulk build created cell count exceeds i64 range while applying block batch: cells={}",
                arena.cells.len()
            )
        })?;
        let consumed_cells = i64::try_from(
            resolved
                .iter()
                .map(|tx| tx.resolved_inputs.len())
                .sum::<usize>(),
        )
        .map_err(|_| {
            anyhow!(
                "bulk build consumed cell count exceeds i64 range while applying block batch"
            )
        })?;
        let last_block = arena
            .blocks
            .last()
            .ok_or_else(|| anyhow!("bulk build arena missing blocks for non-empty batch"))?;
        let history = build_history_rows(&arena, &resolved, &self.interner, is_mainnet)?;
        self.apply_hodl_tracker_batch(&arena, &resolved)?;

        let BulkBuildRuntimeState {
            interner,
            owners,
            cell_dist_tracker,
            ..
        } = self;
        let ctx = owners::ReducerContext::new(interner);
        let mut cell_dist_sealed_rows = Vec::new();
        for block in &arena.blocks {
            let block_date = ckbadger_common::block_date_from_ms(block.timestamp_ms);
            cell_dist_tracker.record_block_date(block.number, block_date);

            for tx in &resolved[block.tx_range.clone()] {
                for input in &tx.resolved_inputs {
                    cell_dist_tracker.cell_consumed(input.occupied_capacity)?;
                }
                for cell in &tx.cells {
                    cell_dist_tracker.cell_created(cell.occupied_capacity);
                }

                let address_deltas = owners.apply_tx_and_return_address_deltas(tx, &ctx)?;
                apply_cell_dist_cohort_deltas(
                    cell_dist_tracker,
                    owners.address.balances(),
                    &address_deltas,
                    tx,
                )?;
            }

            if let Some((snapshot_date, snapshot)) = cell_dist_tracker.maybe_snapshot(block_date) {
                let date_str = snapshot_date.format("%Y%m%d").to_string();
                let cohort = cell_dist_tracker.cohort_snapshot();
                cell_dist_sealed_rows.push(materialize::MaterializedRow::new(
                    CF_STATS_HODL,
                    keys::encode_stats_key(
                        keys::stats_prefix::CELL_DISTRIBUTION,
                        date_str.as_bytes(),
                    ),
                    bincode::serialize(&snapshot)?,
                ));
                cell_dist_sealed_rows.push(materialize::MaterializedRow::new(
                    CF_STATS_HODL,
                    keys::encode_stats_key(
                        keys::stats_prefix::ADDR_COHORT,
                        date_str.as_bytes(),
                    ),
                    bincode::serialize(&cohort)?,
                ));
            }
        }
        owners
            .object
            .apply_identity_activity_count_deltas(&history.identity_activity_count_deltas)?;

        materializer.stream_sealed_aggregate_rows(&cell_dist_sealed_rows)?;
        self.activity_stats.apply_history_rows(&history.rows)?;
        materializer.stream_history_rows(&history.rows)?;
        materialize_activity_secondary_state(domain_store, &history.rows)?;

        Ok(BatchExecutionStats {
            last_block_number: Some(last_block.number),
            last_block_hash: Some(last_block.hash.to_vec()),
            block_count: u64::try_from(arena.blocks.len()).map_err(|_| {
                anyhow!(
                    "bulk build block count exceeds u64 range while applying block batch: blocks={}",
                    arena.blocks.len()
                )
            })?,
            tx_count,
            cells_created,
            cells_consumed: consumed_cells,
        })
    }

    fn finalize(
        self,
        domain_store: &CkbadgerStore,
        materializer: &mut materialize::Materializer<'_>,
    ) -> Result<()> {
        let BulkBuildRuntimeState {
            interner,
            sequencer,
            owners,
            activity_stats,
            hodl_tracker,
            cell_dist_tracker,
            ..
        } = self;

        let sealed_rows = activity_stats.build_rows()?;
        materializer.stream_sealed_aggregate_rows(&sealed_rows)?;

        let final_snapshot_rows = build_final_snapshot_rows(&sequencer, &interner)?;
        materializer.materialize_final_snapshot(&final_snapshot_rows)?;

        let mut owners = owners;
        owners.materialize_all(materializer)?;

        let mut meta_batch = ckbadger_store::batch::StoreBatch::new(domain_store);
        meta_batch.put_hodl_tracker_state(&hodl_tracker.to_state());
        meta_batch.put_cell_dist_tracker_state(&cell_dist_tracker.to_state());
        if !meta_batch.is_empty() {
            meta_batch.commit()?;
        }
        Ok(())
    }

    fn apply_hodl_tracker_batch(
        &mut self,
        arena: &facts::FactsArena,
        resolved: &[facts::ResolvedTxFacts],
    ) -> Result<()> {
        if arena.txs.len() != resolved.len() {
            bail!(
                "bulk build hodl tracker tx count mismatch: facts_txs={} resolved_txs={}",
                arena.txs.len(),
                resolved.len()
            );
        }

        for block in &arena.blocks {
            let block_date = ckbadger_common::block_date_from_ms(block.timestamp_ms);
            self.hodl_tracker.record_block_date(block.number, block_date);

            for tx in &resolved[block.tx_range.clone()] {
                for input in &tx.resolved_inputs {
                    self.update_hodl_holder_count(input.lock_script_hash_id, -1, tx)?;
                    self.hodl_tracker
                        .cell_consumed(input.created_at_block, input.capacity)?;
                }
                for cell in &tx.cells {
                    self.update_hodl_holder_count(cell.lock_script_hash_id, 1, tx)?;
                    self.hodl_tracker.cell_created(block_date, cell.capacity);
                }
            }
        }

        Ok(())
    }

    fn update_hodl_holder_count(
        &mut self,
        lock_hash_id: crate::sync::types::InternId,
        delta: i32,
        tx: &facts::ResolvedTxFacts,
    ) -> Result<()> {
        let old_live = self
            .hodl_live_cells_by_lock
            .get(&lock_hash_id)
            .copied()
            .unwrap_or(0);
        let new_live = old_live.checked_add(delta).ok_or_else(|| {
            anyhow!(
                "hodl live cell count overflow: tx=0x{} block={} tx_index={} lock_hash_id={:?} old_live={} delta={}",
                hex::encode(tx.tx_hash),
                tx.block_number,
                tx.tx_index,
                lock_hash_id,
                old_live,
                delta
            )
        })?;
        if new_live < 0 {
            bail!(
                "hodl live cell count underflow: tx=0x{} block={} tx_index={} lock_hash_id={:?} old_live={} delta={}",
                hex::encode(tx.tx_hash),
                tx.block_number,
                tx.tx_index,
                lock_hash_id,
                old_live,
                delta
            );
        }

        self.hodl_tracker.update_holder_count(old_live, new_live)?;
        if new_live == 0 {
            self.hodl_live_cells_by_lock.remove(&lock_hash_id);
        } else {
            self.hodl_live_cells_by_lock.insert(lock_hash_id, new_live);
        }
        Ok(())
    }
}

fn apply_cell_dist_cohort_deltas(
    tracker: &mut crate::db::writer::cell_distribution::CellDistributionTracker,
    balances: &HashMap<Vec<u8>, AddressBalance>,
    deltas: &HashMap<Vec<u8>, owners::address::AddressTxDelta>,
    tx: &facts::ResolvedTxFacts,
) -> Result<()> {
    for (lock_hash, delta) in deltas {
        let balance = balances.get(lock_hash).ok_or_else(|| {
            anyhow!(
                "missing address balance after applying tx deltas for cell distribution tracker: lock_hash=0x{}, block={}, tx=0x{}, tx_index={}",
                hex::encode(lock_hash),
                tx.block_number,
                hex::encode(tx.tx_hash),
                tx.tx_index
            )
        })?;
        tracker
            .apply_cohort_delta(
                balance.first_seen_block,
                delta.used_capacity_delta,
                delta.balance_delta,
            )
            .map_err(|e| {
                anyhow!(
                    "failed to apply cell distribution cohort delta: lock_hash=0x{}, first_seen_block={}, block={}, tx=0x{}, tx_index={}, error={}",
                    hex::encode(lock_hash),
                    balance.first_seen_block,
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    e
                )
            })?;
    }

    Ok(())
}

struct HistoryBuildResult {
    rows: Vec<materialize::MaterializedRow>,
    identity_activity_count_deltas: HashMap<Vec<u8>, i64>,
}

#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct CoreOwnerStateSnapshot {
    pub address_balances: HashMap<Vec<u8>, AddressBalance>,
    pub script_infos: HashMap<Vec<u8>, ScriptInfo>,
    pub token_state: owners::token::TokenStateSnapshot,
    pub dao_state: owners::dao::DaoStateSnapshot,
    pub object_state: owners::object::ObjectStateSnapshot,
}

#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct BulkArtifactSnapshot {
    pub report: materialize::MaterializationReport,
    pub sync_status: SyncStatus,
    pub bulk_build_session_marker: Option<BulkBuildSessionMarker>,
    pub hodl_tracker_state: Option<HodlTrackerState>,
    pub cell_dist_tracker_state: Option<CellDistributionTrackerState>,
    pub cell_distribution_snapshots: HashMap<String, DailyCellDistribution>,
    pub address_cohort_snapshots: HashMap<String, DailyAddressCohort>,
    pub block_headers: HashMap<i64, CachedBlockHeader>,
    pub block_numbers_by_hash: HashMap<Vec<u8>, i64>,
    pub txs_by_hash: HashMap<Vec<u8>, (i64, i32, TxIndexEntry)>,
    pub activity_bundles: HashMap<Vec<u8>, TxActivityBundle>,
    pub daily_activity_stats: HashMap<String, DailyActivityStats>,
    pub hourly_activity_stats: HashMap<String, DailyActivityStats>,
    pub cell_payloads: HashMap<Vec<u8>, LiveCellInfo>,
    pub live_cells: HashMap<Vec<u8>, i64>,
    pub consumed_cells: HashMap<Vec<u8>, ConsumedCellMeta>,
    pub cell_by_lock: HashSet<Vec<u8>>,
    pub cell_by_type: HashSet<Vec<u8>>,
    pub cell_by_lock_code: HashSet<Vec<u8>>,
    pub cell_by_type_code: HashSet<Vec<u8>>,
    pub cell_by_data_hash: HashSet<Vec<u8>>,
    pub core: CoreOwnerStateSnapshot,
}

#[doc(hidden)]
pub(crate) fn materialize_core_owner_state_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<CoreOwnerStateSnapshot> {
    Ok(materialize_bulk_artifacts_for_test(blocks)?.core)
}

#[doc(hidden)]
pub(crate) fn materialize_bulk_artifacts_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<BulkArtifactSnapshot> {
    materialize_bulk_artifacts_from_block_batches_for_test_impl(&[blocks])
}

#[doc(hidden)]
pub(crate) fn materialize_bulk_artifacts_from_batches_for_test(
    batches: &[Vec<BlockResponseWithCycles>],
) -> Result<BulkArtifactSnapshot> {
    let batch_slices = batches.iter().map(Vec::as_slice).collect::<Vec<_>>();
    materialize_bulk_artifacts_from_block_batches_for_test_impl(&batch_slices)
}

fn materialize_bulk_artifacts_from_block_batches_for_test_impl(
    block_batches: &[&[BlockResponseWithCycles]],
) -> Result<BulkArtifactSnapshot> {
    let mut runtime = BulkBuildRuntimeState::default();
    let mut sync_totals = BulkBuildSyncTotals::default();

    let root = unique_temp_test_dir("bulk-build-core-owners");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let snapshot = {
        let domain_store = CkbadgerStore::open_domain(&domain_path)?;
        let append_store = CkbadgerStore::open_append_only(&append_path)?;
        domain_store.update_sync_status(|status| status.init_sync_start(0, true))?;
        start_bulk_build_session_marker(&domain_store, "bulk-build-test-session", 0)?;
        let mut materializer = materialize::Materializer::new(&domain_store, &append_store);
        for batch in block_batches {
            let batch_stats = runtime.apply_blocks(batch, &domain_store, &mut materializer, true)?;
            sync_totals.record_batch(&batch_stats)?;
        }
        runtime.finalize(&domain_store, &mut materializer)?;
        let sync_status = sync_totals.finalize_success(&domain_store)?;
        domain_store.clear_bulk_build_session_marker()?;
        let report = materializer.finish();

        let core = collect_core_owner_state_snapshot(&domain_store)?;
        let (block_headers, block_numbers_by_hash, txs_by_hash, activity_bundles) =
            collect_history_snapshot(&domain_store)?;
        let (daily_activity_stats, hourly_activity_stats) =
            collect_activity_stats_snapshot(&domain_store)?;
        let (cell_distribution_snapshots, address_cohort_snapshots) =
            collect_hodl_stats_snapshot(&domain_store)?;
        let (
            cell_payloads,
            live_cells,
            consumed_cells,
            cell_by_lock,
            cell_by_type,
            cell_by_lock_code,
            cell_by_type_code,
            cell_by_data_hash,
        ) = collect_cell_snapshot(&domain_store, &append_store)?;
        let bulk_build_session_marker = domain_store.get_bulk_build_session_marker()?;
        let hodl_tracker_state = domain_store.get_hodl_tracker_state()?;
        let cell_dist_tracker_state = domain_store.get_cell_dist_tracker_state()?;

        BulkArtifactSnapshot {
            report,
            sync_status,
            bulk_build_session_marker,
            hodl_tracker_state,
            cell_dist_tracker_state,
            cell_distribution_snapshots,
            address_cohort_snapshots,
            block_headers,
            block_numbers_by_hash,
            txs_by_hash,
            activity_bundles,
            daily_activity_stats,
            hourly_activity_stats,
            cell_payloads,
            live_cells,
            consumed_cells,
            cell_by_lock,
            cell_by_type,
            cell_by_lock_code,
            cell_by_type_code,
            cell_by_data_hash,
            core,
        }
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(snapshot)
}

fn build_history_rows(
    arena: &facts::FactsArena,
    resolved: &[facts::ResolvedTxFacts],
    interner: &interner::IdentityInterner,
    is_mainnet: bool,
) -> Result<HistoryBuildResult> {
    let mut rows = Vec::with_capacity(
        arena.blocks.len() * 2 + arena.txs.len() * 2 + arena.cells.len() * 2 + arena.txs.len(),
    );
    let mut identity_activity_count_deltas = HashMap::new();

    for block in &arena.blocks {
        let header = CachedBlockHeader {
            hash: block.hash.to_vec(),
            timestamp: block.timestamp_ms,
            epoch_number: block.epoch_number,
            epoch_index: block.epoch_index,
            epoch_length: block.epoch_length,
            dao: block.dao.clone(),
            transactions_count: block.transactions_count,
        };
        rows.push(materialize::MaterializedRow::new(
            CF_BLOCK_HEADERS,
            keys::encode_block_num(block.number).to_vec(),
            bincode::serialize(&header)?,
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_BLOCK_HASH_INDEX,
            block.hash.to_vec(),
            block.number.to_le_bytes().to_vec(),
        ));
    }

    if arena.txs.len() != resolved.len() {
        bail!(
            "bulk build history tx count mismatch: facts_txs={} resolved_txs={}",
            arena.txs.len(),
            resolved.len()
        );
    }

    for (tx, resolved_tx) in arena.txs.iter().zip(resolved) {
        if tx.hash != resolved_tx.tx_hash
            || tx.block_number != resolved_tx.block_number
            || tx.tx_index != resolved_tx.tx_index
        {
            bail!(
                "bulk build history tx alignment mismatch: facts_tx=0x{} facts_block={} facts_tx_index={} resolved_tx=0x{} resolved_block={} resolved_tx_index={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index,
                hex::encode(resolved_tx.tx_hash),
                resolved_tx.block_number,
                resolved_tx.tx_index
            );
        }

        let entry = TxIndexEntry {
            is_cellbase: tx.is_cellbase,
            timestamp: tx.timestamp_ms,
            inputs_count: tx.inputs_count,
            outputs_count: tx.outputs_count,
            fee: resolved_tx_fee(tx, resolved_tx)?,
            tx_size: tx.tx_size,
            cycles: tx.cycles,
        };
        let tx_location = keys::encode_composite(&[
            &keys::encode_block_num(tx.block_number),
            &keys::encode_tx_idx(tx.tx_index),
        ]);
        rows.push(materialize::MaterializedRow::new(
            CF_TX_INDEX,
            tx_location.to_vec(),
            bincode::serialize(&entry)?,
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_TX_HASH_MAP,
            tx.hash.to_vec(),
            tx_location.to_vec(),
        ));

        let mut touched_lock_hash_ids = HashSet::new();
        for output in &resolved_tx.cells {
            touched_lock_hash_ids.insert(output.lock_script_hash_id);
        }
        for input in &resolved_tx.resolved_inputs {
            touched_lock_hash_ids.insert(input.lock_script_hash_id);
        }
        for lock_hash_id in touched_lock_hash_ids {
            rows.push(materialize::MaterializedRow::new(
                CF_ADDR_TXS,
                keys::encode_addr_tx_key(
                    interner.resolve_bytes(lock_hash_id),
                    tx.block_number,
                    tx.tx_index,
                    &tx.hash,
                ),
                Vec::new(),
            ));
        }

        if tx.is_cellbase {
            continue;
        }

        for input in &resolved_tx.resolved_inputs {
            rows.push(materialize::MaterializedRow::new(
                CF_CONSUMED_CELLS,
                keys::encode_outpoint(
                    &input.outpoint.tx_hash,
                    resolved_input_outpoint_index_i16(input)?,
                )
                .to_vec(),
                bincode::serialize(&ConsumedCellMeta {
                    created_at_block: input.created_at_block,
                    consumed_at_block: tx.block_number,
                    consumed_by_tx: Some(tx.hash.to_vec()),
                })?,
            ));
        }
    }

    rows.extend(build_token_transfer_rows(resolved, interner)?);
    rows.extend(build_activity_rows(arena, resolved, interner, is_mainnet)?);
    let object_activity_rows =
        build_object_collection_activity_rows(resolved, &mut identity_activity_count_deltas)?;
    rows.extend(object_activity_rows);

    for cell in &arena.cells {
        let outpoint_key =
            keys::encode_outpoint(&cell.outpoint.tx_hash, cell_outpoint_index_i16(cell)?).to_vec();
        rows.push(materialize::MaterializedRow::new(
            CF_CELLS,
            outpoint_key,
            bincode::serialize(&cell_facts_to_live_cell_info(cell, interner))?,
        ));

        if let Some(data_hash) = &cell.data_hash {
            rows.push(materialize::MaterializedRow::new(
                CF_CELL_BY_DATA_HASH,
                keys::encode_cell_index_key(
                    data_hash,
                    cell.created_at_block,
                    &cell.outpoint.tx_hash,
                    cell_outpoint_index_i16(cell)?,
                ),
                Vec::new(),
            ));
        }
    }

    Ok(HistoryBuildResult {
        rows,
        identity_activity_count_deltas,
    })
}

fn build_object_collection_activity_rows(
    resolved: &[facts::ResolvedTxFacts],
    identity_activity_count_deltas: &mut HashMap<Vec<u8>, i64>,
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut object_activity_acc =
        crate::db::writer::nft_activity_acc::ObjectCollectionActivityAccumulator::new();
    let mut identity_activity_acc =
        crate::db::writer::nft_activity_acc::ObjectCollectionActivityAccumulator::new();
    let mut rows = Vec::new();

    for tx in resolved {
        let mut dotbit_created_account_ids = HashSet::new();
        let mut dotbit_consumed_account_ids = HashSet::new();

        for input in &tx.resolved_inputs {
            let Some(protocol) = input.protocol_facts.as_ref() else {
                continue;
            };
            match protocol {
                facts::CellProtocolFacts::Spore(spore) if !spore.is_did => {
                    let collection_id = spore
                        .cluster_id
                        .map(|id| id.to_vec())
                        .unwrap_or_else(|| SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
                    object_activity_acc.record(
                        &collection_id,
                        &tx.tx_hash,
                        &spore.spore_id,
                        &tx.block_hash,
                        tx.block_number,
                        tx.tx_index,
                        tx.timestamp_ms,
                        false,
                    );
                }
                facts::CellProtocolFacts::Dotbit(dotbit) => {
                    dotbit_consumed_account_ids.insert(dotbit.account_id.to_vec());
                }
                _ => {}
            }
        }

        for cell in &tx.cells {
            let Some(protocol) = cell.protocol_facts.as_ref() else {
                continue;
            };
            match protocol {
                facts::CellProtocolFacts::Spore(spore) if spore.is_did => {
                    identity_activity_acc.record(
                        &DID_CKB_SENTINEL_COLLECTION,
                        &tx.tx_hash,
                        &spore.spore_id,
                        &tx.block_hash,
                        tx.block_number,
                        tx.tx_index,
                        tx.timestamp_ms,
                        true,
                    );
                }
                facts::CellProtocolFacts::Spore(spore) => {
                    let collection_id = spore
                        .cluster_id
                        .map(|id| id.to_vec())
                        .unwrap_or_else(|| SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
                    object_activity_acc.record(
                        &collection_id,
                        &tx.tx_hash,
                        &spore.spore_id,
                        &tx.block_hash,
                        tx.block_number,
                        tx.tx_index,
                        tx.timestamp_ms,
                        true,
                    );
                }
                facts::CellProtocolFacts::Dotbit(dotbit) => {
                    dotbit_created_account_ids.insert(dotbit.account_id.to_vec());
                }
                _ => {}
            }
        }

        if let Some(entry) = crate::db::writer::dotbit::build_dotbit_tx_activity_entry(
            tx.dotbit_action.as_deref(),
            &dotbit_created_account_ids,
            &dotbit_consumed_account_ids,
            &tx.tx_hash,
            &tx.block_hash,
            tx.timestamp_ms,
        ) {
            rows.push(materialize::MaterializedRow::new(
                CF_IDENTITY_COLLECTION_ACTIVITIES,
                keys::encode_nft_collection_activity_key(
                    &DOTBIT_SENTINEL_COLLECTION,
                    tx.block_number,
                    tx.tx_index,
                    &tx.block_hash,
                    &tx.tx_hash,
                )
                .to_vec(),
                bincode::serialize(&entry)?,
            ));
        }
    }

    for resolved_entry in object_activity_acc.into_resolved_entries() {
        rows.push(materialize::MaterializedRow::new(
            CF_OBJECT_COLLECTION_ACTIVITIES,
            keys::encode_nft_collection_activity_key(
                &resolved_entry.collection_id,
                resolved_entry.block_number,
                resolved_entry.tx_idx,
                &resolved_entry.entry.block_hash,
                &resolved_entry.entry.tx_hash,
            )
            .to_vec(),
            bincode::serialize(&resolved_entry.entry)?,
        ));
    }

    for resolved_entry in identity_activity_acc.into_resolved_entries() {
        let delta = identity_activity_count_deltas
            .entry(resolved_entry.collection_id.clone())
            .or_insert(0);
        *delta = delta.checked_add(1).ok_or_else(|| {
            anyhow!(
                "identity collection activity delta overflow in bulk build history rows: collection_id=0x{}",
                hex::encode(&resolved_entry.collection_id)
            )
        })?;
        rows.push(materialize::MaterializedRow::new(
            CF_IDENTITY_COLLECTION_ACTIVITIES,
            keys::encode_nft_collection_activity_key(
                &resolved_entry.collection_id,
                resolved_entry.block_number,
                resolved_entry.tx_idx,
                &resolved_entry.entry.block_hash,
                &resolved_entry.entry.tx_hash,
            )
            .to_vec(),
            bincode::serialize(&resolved_entry.entry)?,
        ));
    }

    Ok(rows)
}

fn build_sealed_aggregate_rows(
    history_rows: &[materialize::MaterializedRow],
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut accumulator = ActivityStatsAccumulator::default();
    accumulator.apply_history_rows(history_rows)?;
    accumulator.build_rows()
}

fn materialize_activity_secondary_state(
    domain_store: &CkbadgerStore,
    history_rows: &[materialize::MaterializedRow],
) -> Result<()> {
    for row in history_rows
        .iter()
        .filter(|row| row.cf_name == CF_ACTIVITIES)
    {
        let bundle: TxActivityBundle = bincode::deserialize(&row.value).map_err(|e| {
            anyhow!(
                "failed to deserialize TxActivityBundle while materializing bulk activity secondary state: key=0x{} error={}",
                hex::encode(&row.key),
                e
            )
        })?;
        let mut batch = ckbadger_store::batch::StoreBatch::new(domain_store);
        crate::db::writer::fiber::process_fiber_channel_events(&mut batch, domain_store, &bundle)?;
        if !batch.is_empty() {
            batch.commit()?;
        }
    }

    Ok(())
}

fn build_activity_rows(
    arena: &facts::FactsArena,
    resolved: &[facts::ResolvedTxFacts],
    interner: &interner::IdentityInterner,
    is_mainnet: bool,
) -> Result<Vec<materialize::MaterializedRow>> {
    let block_ar_by_number: HashMap<i64, u64> = arena
        .blocks
        .iter()
        .map(|block| {
            let ar = crate::db::writer::dao::extract_ar_from_dao(&block.dao).ok_or_else(|| {
                anyhow!(
                    "missing AR in block DAO field while building bulk activities: block={}",
                    block.number
                )
            })?;
            Ok((block.number, ar))
        })
        .collect::<Result<_>>()?;
    let detectors = build_activity_protocol_detectors(resolved, interner, is_mainnet)?;
    let token_info_cache = HashMap::new();
    let mut rows = Vec::new();

    for block in &arena.blocks {
        let txs = &arena.txs[block.tx_range.clone()];
        let resolved_txs = &resolved[block.tx_range.clone()];
        if txs.len() != resolved_txs.len() {
            bail!(
                "bulk build activity tx count mismatch within block: block={} facts_txs={} resolved_txs={}",
                block.number,
                txs.len(),
                resolved_txs.len()
            );
        }

        let mut block_inputs = Vec::with_capacity(txs.len());
        let mut block_outputs = Vec::with_capacity(txs.len());
        for (tx, resolved_tx) in txs.iter().zip(resolved_txs) {
            if tx.hash != resolved_tx.tx_hash
                || tx.block_number != resolved_tx.block_number
                || tx.tx_index != resolved_tx.tx_index
            {
                bail!(
                    "bulk build activity tx alignment mismatch: facts_tx=0x{} facts_block={} facts_tx_index={} resolved_tx=0x{} resolved_block={} resolved_tx_index={}",
                    hex::encode(tx.hash),
                    tx.block_number,
                    tx.tx_index,
                    hex::encode(resolved_tx.tx_hash),
                    resolved_tx.block_number,
                    resolved_tx.tx_index
                );
            }

            block_outputs.push(
                resolved_tx
                    .cells
                    .iter()
                    .map(|cell| parsed_cell_from_facts(cell, interner))
                    .collect::<Result<Vec<_>>>()?,
            );
            block_inputs.push(
                resolved_tx
                    .resolved_inputs
                    .iter()
                    .map(|input| {
                        activity_input_view_from_resolved_input(
                            input,
                            interner,
                            &block_ar_by_number,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
        }

        let tx_views = txs
            .iter()
            .zip(block_inputs.into_iter())
            .zip(block_outputs.iter())
            .map(
                |((tx, inputs), outputs)| crate::db::writer::activities::TxView {
                    tx_hash: &tx.hash,
                    block_hash: &tx.block_hash,
                    tx_index: tx.tx_index,
                    block_number: tx.block_number,
                    timestamp: tx.timestamp_ms,
                    is_cellbase: tx.is_cellbase,
                    inputs,
                    outputs,
                    witnesses: &[],
                },
            )
            .collect::<Vec<_>>();

        let bundles =
            crate::db::writer::activities::build_activity_bundles_for_block_with_detectors(
                &tx_views,
                &token_info_cache,
                &detectors,
            )?;
        for bundle in bundles {
            rows.push(materialize::MaterializedRow::new(
                CF_ACTIVITIES,
                keys::encode_tx_activity_bundle_key(
                    bundle.block_number,
                    bundle.tx_index,
                    &bundle.tx_hash,
                ),
                bincode::serialize(&bundle)?,
            ));
        }
    }

    Ok(rows)
}

fn build_token_transfer_rows(
    resolved: &[facts::ResolvedTxFacts],
    interner: &interner::IdentityInterner,
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut rows = Vec::new();
    let mut transfer_idx: HashMap<(Vec<u8>, i64), i32> = HashMap::new();

    for tx in resolved {
        let input_udts = tx
            .resolved_inputs
            .iter()
            .filter_map(|input| parsed_udt_cell_from_input(input, interner, tx).transpose())
            .collect::<Result<Vec<_>>>()?;
        let output_udts = tx
            .cells
            .iter()
            .filter_map(|cell| parsed_udt_cell_from_output(cell, interner, tx).transpose())
            .collect::<Result<Vec<_>>>()?;

        for transfer in UdtParser::build_transfers_from_cells(&input_udts, &output_udts) {
            let idx = transfer_idx
                .entry((transfer.type_script_hash.clone(), tx.block_number))
                .or_insert(0);
            let record = TokenTransferRecord {
                tx_hash: tx.tx_hash.to_vec(),
                block_number: tx.block_number,
                from_lock_hash: transfer.from_lock_hash.clone(),
                to_lock_hash: transfer.to_lock_hash.clone(),
                amount: transfer.amount,
                is_mint: transfer.is_mint,
                is_burn: transfer.is_burn,
                timestamp: tx.timestamp_ms,
            };
            rows.push(materialize::MaterializedRow::new(
                CF_TOKEN_TRANSFERS,
                keys::encode_token_transfer_key(&transfer.type_script_hash, tx.block_number, *idx),
                bincode::serialize(&record)?,
            ));
            *idx = idx.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "token transfer index overflow in bulk build history rows: type_hash=0x{} block={}",
                    hex::encode(&transfer.type_script_hash),
                    tx.block_number
                )
            })?;
        }
    }

    Ok(rows)
}

fn build_activity_protocol_detectors(
    resolved: &[facts::ResolvedTxFacts],
    interner: &interner::IdentityInterner,
    is_mainnet: bool,
) -> Result<Vec<Box<dyn crate::db::writer::activities::ProtocolDetector>>> {
    let mut lock_code_hashes = HashSet::new();
    let mut type_code_hashes = HashSet::new();

    for tx in resolved {
        for input in &tx.resolved_inputs {
            lock_code_hashes.insert(activity_code_hash(
                interner,
                input.lock_code_hash_id,
                "input lock_code_hash",
                tx,
            )?);
            if let Some(type_code_hash_id) = input.type_code_hash_id {
                type_code_hashes.insert(activity_code_hash(
                    interner,
                    type_code_hash_id,
                    "input type_code_hash",
                    tx,
                )?);
            }
        }

        for cell in &tx.cells {
            lock_code_hashes.insert(activity_code_hash(
                interner,
                cell.lock_code_hash_id,
                "output lock_code_hash",
                tx,
            )?);
            if let Some(type_code_hash_id) = cell.type_code_hash_id {
                type_code_hashes.insert(activity_code_hash(
                    interner,
                    type_code_hash_id,
                    "output type_code_hash",
                    tx,
                )?);
            }
        }
    }

    Ok(vec![
        Box::new(crate::db::writer::rgbpp_detector::RgbppDetector::new(
            is_mainnet,
        )) as Box<dyn crate::db::writer::activities::ProtocolDetector>,
        Box::new(crate::db::writer::fiber_detector::FiberDetector::new(
            is_mainnet,
        )),
        Box::new(crate::db::writer::stablepp_detector::StableppDetector::new(
            is_mainnet,
        )),
        Box::new(crate::db::writer::utxoswap_detector::UtxoSwapDetector::new(
            is_mainnet,
        )),
    ]
    .into_iter()
    .filter(|detector| detector.might_apply_batch(&lock_code_hashes, &type_code_hashes))
    .collect())
}

fn activity_code_hash(
    interner: &interner::IdentityInterner,
    id: crate::sync::types::InternId,
    label: &str,
    tx: &facts::ResolvedTxFacts,
) -> Result<[u8; 32]> {
    interner.resolve_bytes(id).try_into().map_err(|_| {
        anyhow!(
            "invalid {} length while building bulk activities: tx=0x{} block={} tx_index={} len={}",
            label,
            hex::encode(tx.tx_hash),
            tx.block_number,
            tx.tx_index,
            interner.resolve_bytes(id).len()
        )
    })
}

fn activity_input_view_from_resolved_input(
    input: &facts::ResolvedInputFacts,
    interner: &interner::IdentityInterner,
    block_ar_by_number: &HashMap<i64, u64>,
) -> Result<crate::db::writer::activities::InputCellView> {
    let (is_dao_withdraw_request, dao_compensation) = match input.dao_state {
        Some(facts::DaoCellState::WithdrawRequest {
            deposit_block_number,
        }) => {
            let deposit_ar = *block_ar_by_number.get(&deposit_block_number).ok_or_else(|| {
                anyhow!(
                    "missing deposit block AR while building bulk DAO activity input: deposit_block={} outpoint=0x{}:{}",
                    deposit_block_number,
                    hex::encode(input.outpoint.tx_hash),
                    input.outpoint.index
                )
            })?;
            let request_ar = *block_ar_by_number.get(&input.created_at_block).ok_or_else(|| {
                anyhow!(
                    "missing withdraw-request block AR while building bulk DAO activity input: request_block={} outpoint=0x{}:{}",
                    input.created_at_block,
                    hex::encode(input.outpoint.tx_hash),
                    input.outpoint.index
                )
            })?;
            let compensation = crate::db::writer::dao::calculate_dao_compensation_from_ar(
                input.capacity,
                deposit_ar,
                request_ar,
            )?;
            (true, Some(compensation))
        }
        _ => (false, None),
    };

    Ok(crate::db::writer::activities::InputCellView {
        lock_script_hash: interner.resolve_bytes(input.lock_script_hash_id).to_vec(),
        lock_code_hash: interner.resolve_bytes(input.lock_code_hash_id).to_vec(),
        lock_hash_type: input.lock_hash_type,
        lock_args: interner.resolve_bytes(input.lock_args_id).to_vec(),
        capacity: input.capacity,
        occupied_capacity: input.occupied_capacity,
        type_code_hash: input
            .type_code_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_hash_type: input.type_hash_type,
        type_script_hash: input
            .type_script_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_args: input
            .type_args_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        udt_amount: input.udt_amount,
        data: Vec::new(),
        is_dao_withdraw_request,
        dao_compensation,
    })
}

fn parsed_cell_from_facts(
    cell: &facts::CellFacts,
    interner: &interner::IdentityInterner,
) -> Result<ParsedCell> {
    if usize::try_from(cell.data_size).ok() != Some(cell.data.len()) {
        bail!(
            "bulk build cell data size mismatch while building activities: outpoint=0x{}:{} data_size={} actual_len={}",
            hex::encode(cell.outpoint.tx_hash),
            cell.outpoint.index,
            cell.data_size,
            cell.data.len()
        );
    }

    Ok(ParsedCell {
        capacity: cell.capacity,
        lock_code_hash: interner.resolve_bytes(cell.lock_code_hash_id).to_vec(),
        lock_hash_type: cell.lock_hash_type,
        lock_args: interner.resolve_bytes(cell.lock_args_id).to_vec(),
        lock_script_hash: interner.resolve_bytes(cell.lock_script_hash_id).to_vec(),
        type_code_hash: cell
            .type_code_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_hash_type: cell.type_hash_type,
        type_args: cell
            .type_args_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_script_hash: cell
            .type_script_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        data_hash: cell
            .data_hash
            .clone()
            .unwrap_or_else(|| ScriptParser::compute_data_hash(&cell.data)),
        data_size: cell.data_size,
        data: cell.data.clone(),
    })
}

fn parsed_udt_cell_from_output(
    cell: &facts::CellFacts,
    interner: &interner::IdentityInterner,
    tx: &facts::ResolvedTxFacts,
) -> Result<Option<ParsedUdtCell>> {
    parsed_udt_cell_from_parts(
        cell.semantic_tag,
        cell.type_script_hash_id,
        cell.type_code_hash_id,
        cell.type_hash_type,
        cell.type_args_id,
        cell.lock_script_hash_id,
        cell.udt_amount,
        interner,
        &format!(
            "output outpoint=0x{}:{} block={} tx=0x{} tx_index={}",
            hex::encode(cell.outpoint.tx_hash),
            cell.outpoint.index,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        ),
    )
}

fn parsed_udt_cell_from_input(
    input: &facts::ResolvedInputFacts,
    interner: &interner::IdentityInterner,
    tx: &facts::ResolvedTxFacts,
) -> Result<Option<ParsedUdtCell>> {
    parsed_udt_cell_from_parts(
        input.semantic_tag,
        input.type_script_hash_id,
        input.type_code_hash_id,
        input.type_hash_type,
        input.type_args_id,
        input.lock_script_hash_id,
        input.udt_amount,
        interner,
        &format!(
            "input outpoint=0x{}:{} block={} tx=0x{} tx_index={}",
            hex::encode(input.outpoint.tx_hash),
            input.outpoint.index,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index
        ),
    )
}

fn parsed_udt_cell_from_parts(
    semantic_tag: facts::CellSemanticTag,
    type_script_hash_id: Option<crate::sync::types::InternId>,
    type_code_hash_id: Option<crate::sync::types::InternId>,
    type_hash_type: Option<i16>,
    type_args_id: Option<crate::sync::types::InternId>,
    lock_script_hash_id: crate::sync::types::InternId,
    udt_amount: Option<u128>,
    interner: &interner::IdentityInterner,
    context: &str,
) -> Result<Option<ParsedUdtCell>> {
    let Some(standard) = udt_standard_for_semantic_tag(semantic_tag) else {
        return Ok(None);
    };

    let type_script_hash_id = type_script_hash_id.ok_or_else(|| {
        anyhow!(
            "missing type_script_hash_id for bulk build token transfer cell: {}",
            context
        )
    })?;
    let type_code_hash_id = type_code_hash_id.ok_or_else(|| {
        anyhow!(
            "missing type_code_hash_id for bulk build token transfer cell: {}",
            context
        )
    })?;
    let type_hash_type = type_hash_type.ok_or_else(|| {
        anyhow!(
            "missing type_hash_type for bulk build token transfer cell: {}",
            context
        )
    })?;
    let type_args_id = type_args_id.ok_or_else(|| {
        anyhow!(
            "missing type_args_id for bulk build token transfer cell: {}",
            context
        )
    })?;
    let amount = udt_amount.ok_or_else(|| {
        anyhow!(
            "missing udt_amount for bulk build token transfer cell: {}",
            context
        )
    })?;

    Ok(Some(ParsedUdtCell {
        type_script_hash: interner.resolve_bytes(type_script_hash_id).to_vec(),
        type_code_hash: interner.resolve_bytes(type_code_hash_id).to_vec(),
        type_hash_type,
        type_args: interner.resolve_bytes(type_args_id).to_vec(),
        lock_script_hash: interner.resolve_bytes(lock_script_hash_id).to_vec(),
        amount,
        standard,
    }))
}

fn udt_standard_for_semantic_tag(semantic_tag: facts::CellSemanticTag) -> Option<UdtStandard> {
    match semantic_tag {
        facts::CellSemanticTag::Sudt => Some(UdtStandard::Sudt),
        facts::CellSemanticTag::Xudt => Some(UdtStandard::Xudt),
        _ => None,
    }
}

fn build_final_snapshot_rows(
    sequencer: &sequencer::BulkSequencer,
    interner: &interner::IdentityInterner,
) -> Result<Vec<materialize::MaterializedRow>> {
    let mut rows = Vec::with_capacity(sequencer.live_count() * 5);

    for slot in sequencer.live_slots() {
        let outpoint_index = live_slot_outpoint_index_i16(slot)?;
        rows.push(materialize::MaterializedRow::new(
            CF_LIVE_CELLS,
            keys::encode_outpoint(&slot.outpoint.tx_hash, outpoint_index).to_vec(),
            slot.created_at_block.to_le_bytes().to_vec(),
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_CELL_BY_LOCK,
            keys::encode_cell_index_key(
                interner.resolve_bytes(slot.lock_script_hash_id),
                slot.created_at_block,
                &slot.outpoint.tx_hash,
                outpoint_index,
            ),
            Vec::new(),
        ));
        rows.push(materialize::MaterializedRow::new(
            CF_CELL_BY_LOCK_CODE,
            keys::encode_cell_index_key(
                interner.resolve_bytes(slot.lock_code_hash_id),
                slot.created_at_block,
                &slot.outpoint.tx_hash,
                outpoint_index,
            ),
            Vec::new(),
        ));
        if let Some(type_script_hash_id) = slot.type_script_hash_id {
            rows.push(materialize::MaterializedRow::new(
                CF_CELL_BY_TYPE,
                keys::encode_cell_index_key(
                    interner.resolve_bytes(type_script_hash_id),
                    slot.created_at_block,
                    &slot.outpoint.tx_hash,
                    outpoint_index,
                ),
                Vec::new(),
            ));
        }
        if let Some(type_code_hash_id) = slot.type_code_hash_id {
            rows.push(materialize::MaterializedRow::new(
                CF_CELL_BY_TYPE_CODE,
                keys::encode_cell_index_key(
                    interner.resolve_bytes(type_code_hash_id),
                    slot.created_at_block,
                    &slot.outpoint.tx_hash,
                    outpoint_index,
                ),
                Vec::new(),
            ));
        }
    }

    Ok(rows)
}

fn resolved_tx_fee(tx: &facts::TxFacts, resolved_tx: &facts::ResolvedTxFacts) -> Result<i64> {
    if tx.is_cellbase {
        return Ok(0);
    }

    let total_input_capacity =
        resolved_tx
            .resolved_inputs
            .iter()
            .try_fold(0i64, |acc, input| {
                acc.checked_add(input.capacity).ok_or_else(|| {
                    anyhow!(
                        "bulk build input capacity overflow while materializing tx index: tx=0x{} block={} tx_index={}",
                        hex::encode(tx.hash),
                        tx.block_number,
                        tx.tx_index
                    )
                })
            })?;
    let total_output_capacity = resolved_tx.cells.iter().try_fold(0i64, |acc, cell| {
        acc.checked_add(cell.capacity).ok_or_else(|| {
            anyhow!(
                "bulk build output capacity overflow while materializing tx index: tx=0x{} block={} tx_index={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index
            )
        })
    })?;

    total_input_capacity
        .checked_sub(total_output_capacity)
        .ok_or_else(|| {
            anyhow!(
                "bulk build negative fee while materializing tx index: tx=0x{} block={} tx_index={} inputs={} outputs={}",
                hex::encode(tx.hash),
                tx.block_number,
                tx.tx_index,
                total_input_capacity,
                total_output_capacity
            )
        })
}

fn cell_facts_to_live_cell_info(
    cell: &facts::CellFacts,
    interner: &interner::IdentityInterner,
) -> LiveCellInfo {
    LiveCellInfo {
        capacity: cell.capacity,
        lock_script_hash: interner.resolve_bytes(cell.lock_script_hash_id).to_vec(),
        lock_code_hash: interner.resolve_bytes(cell.lock_code_hash_id).to_vec(),
        lock_hash_type: cell.lock_hash_type,
        lock_args: interner.resolve_bytes(cell.lock_args_id).to_vec(),
        type_script_hash: cell
            .type_script_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_code_hash: cell
            .type_code_hash_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        type_hash_type: cell.type_hash_type,
        type_args: cell
            .type_args_id
            .map(|id| interner.resolve_bytes(id).to_vec()),
        data_size: cell.data_size,
        occupied_capacity: cell.occupied_capacity,
        udt_amount: cell.udt_amount,
        data_hash: cell.data_hash.clone(),
    }
}

fn cell_outpoint_index_i16(cell: &facts::CellFacts) -> Result<i16> {
    i16::try_from(cell.outpoint.index).map_err(|_| {
        anyhow!(
            "bulk build cell outpoint index exceeds i16 while materializing cells: tx=0x{} output_index={}",
            hex::encode(cell.outpoint.tx_hash),
            cell.outpoint.index
        )
    })
}

fn live_slot_outpoint_index_i16(slot: &live_cells::LiveCellSlot) -> Result<i16> {
    i16::try_from(slot.outpoint.index).map_err(|_| {
        anyhow!(
            "bulk build live outpoint index exceeds i16 while materializing live cells: tx=0x{} output_index={}",
            hex::encode(slot.outpoint.tx_hash),
            slot.outpoint.index
        )
    })
}

fn resolved_input_outpoint_index_i16(input: &facts::ResolvedInputFacts) -> Result<i16> {
    i16::try_from(input.outpoint.index).map_err(|_| {
        anyhow!(
            "bulk build consumed outpoint index exceeds i16 while materializing consumed cells: tx=0x{} output_index={}",
            hex::encode(input.outpoint.tx_hash),
            input.outpoint.index
        )
    })
}

fn collect_history_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<(
    HashMap<i64, CachedBlockHeader>,
    HashMap<Vec<u8>, i64>,
    HashMap<Vec<u8>, (i64, i32, TxIndexEntry)>,
    HashMap<Vec<u8>, TxActivityBundle>,
)> {
    let mut block_headers = HashMap::new();
    let mut block_numbers_by_hash = HashMap::new();
    let block_iter = domain_store.iterator_cf(domain_store.cf_block_headers(), IteratorMode::Start);
    for item in block_iter {
        let (key, value) = item?;
        if key.len() != 8 {
            bail!(
                "invalid block_headers key length in bulk artifact snapshot helper: key_len={}",
                key.len()
            );
        }
        let block_number = keys::decode_block_num(&key);
        let header: CachedBlockHeader = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize CachedBlockHeader in bulk artifact snapshot helper: block_number={} error={}",
                block_number,
                e
            )
        })?;
        let indexed_block_number = domain_store
            .get_block_number_by_hash(&header.hash)?
            .ok_or_else(|| {
                anyhow!(
                    "block_hash_index missing in bulk artifact snapshot helper: block_number={} hash=0x{}",
                    block_number,
                    hex::encode(&header.hash)
                )
            })?;
        if indexed_block_number != block_number {
            bail!(
                "block_hash_index mismatch in bulk artifact snapshot helper: block_number={} indexed_block_number={} hash=0x{}",
                block_number,
                indexed_block_number,
                hex::encode(&header.hash)
            );
        }
        block_numbers_by_hash.insert(header.hash.clone(), indexed_block_number);
        block_headers.insert(block_number, header);
    }

    let mut txs_by_hash = HashMap::new();
    let tx_iter = domain_store.iterator_cf(domain_store.cf_tx_hash_map(), IteratorMode::Start);
    for item in tx_iter {
        let (tx_hash, _value) = item?;
        let tx_entry = domain_store.get_tx_by_hash(&tx_hash)?.ok_or_else(|| {
            anyhow!(
                "tx index missing in bulk artifact snapshot helper: tx_hash=0x{}",
                hex::encode(&tx_hash)
            )
        })?;
        txs_by_hash.insert(tx_hash.to_vec(), tx_entry);
    }

    let mut activity_bundles = HashMap::new();
    let activity_iter = domain_store.iterator_cf(domain_store.cf_activities(), IteratorMode::Start);
    for item in activity_iter {
        let (key, value) = item?;
        let bundle: TxActivityBundle = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize TxActivityBundle in bulk artifact snapshot helper: key=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        activity_bundles.insert(key.to_vec(), bundle);
    }

    Ok((
        block_headers,
        block_numbers_by_hash,
        txs_by_hash,
        activity_bundles,
    ))
}

fn collect_activity_stats_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<(
    HashMap<String, DailyActivityStats>,
    HashMap<String, DailyActivityStats>,
)> {
    let daily_activity_stats = domain_store
        .list_daily_activity_stats()?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let hourly_activity_stats = domain_store
        .list_hourly_activity_stats_since("0000000000")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    Ok((daily_activity_stats, hourly_activity_stats))
}

fn collect_hodl_stats_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<(
    HashMap<String, DailyCellDistribution>,
    HashMap<String, DailyAddressCohort>,
)> {
    let mut cell_distribution_snapshots = HashMap::new();
    let cell_dist_iter = domain_store.prefix_iterator_cf(
        domain_store.cf_stats_hodl(),
        &[keys::stats_prefix::CELL_DISTRIBUTION],
    );
    for item in cell_dist_iter {
        let (key, value) = item?;
        if !key.starts_with(&[keys::stats_prefix::CELL_DISTRIBUTION]) {
            break;
        }
        let date = String::from_utf8_lossy(&key[1..]).to_string();
        let snapshot: DailyCellDistribution = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize DailyCellDistribution in bulk artifact snapshot helper: key=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        cell_distribution_snapshots.insert(date, snapshot);
    }

    let mut address_cohort_snapshots = HashMap::new();
    let cohort_iter = domain_store.prefix_iterator_cf(
        domain_store.cf_stats_hodl(),
        &[keys::stats_prefix::ADDR_COHORT],
    );
    for item in cohort_iter {
        let (key, value) = item?;
        if !key.starts_with(&[keys::stats_prefix::ADDR_COHORT]) {
            break;
        }
        let date = String::from_utf8_lossy(&key[1..]).to_string();
        let snapshot: DailyAddressCohort = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize DailyAddressCohort in bulk artifact snapshot helper: key=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        address_cohort_snapshots.insert(date, snapshot);
    }

    Ok((cell_distribution_snapshots, address_cohort_snapshots))
}

fn collect_cell_snapshot(
    domain_store: &CkbadgerStore,
    append_store: &CkbadgerStore,
) -> Result<(
    HashMap<Vec<u8>, LiveCellInfo>,
    HashMap<Vec<u8>, i64>,
    HashMap<Vec<u8>, ConsumedCellMeta>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
    HashSet<Vec<u8>>,
)> {
    let mut cell_payloads = HashMap::new();
    let cell_iter = append_store.iterator_cf(append_store.cf_cells(), IteratorMode::Start);
    for item in cell_iter {
        let (key, value) = item?;
        let cell: LiveCellInfo = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize LiveCellInfo in bulk artifact snapshot helper: outpoint=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        cell_payloads.insert(key.to_vec(), cell);
    }

    let mut live_cells = HashMap::new();
    let live_iter = domain_store.iterator_cf(domain_store.cf_live_cells(), IteratorMode::Start);
    for item in live_iter {
        let (key, value) = item?;
        let created_at_block = decode_live_cell_marker(&value).ok_or_else(|| {
            anyhow!(
                "invalid live cell marker value in bulk artifact snapshot helper: outpoint=0x{} value_len={}",
                hex::encode(&key),
                value.len()
            )
        })?;
        live_cells.insert(key.to_vec(), created_at_block);
    }

    let mut consumed_cells = HashMap::new();
    let consumed_iter =
        domain_store.iterator_cf(domain_store.cf_consumed_cells(), IteratorMode::Start);
    for item in consumed_iter {
        let (key, value) = item?;
        let consumed: ConsumedCellMeta = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize ConsumedCellMeta in bulk artifact snapshot helper: outpoint=0x{} error={}",
                hex::encode(&key),
                e
            )
        })?;
        consumed_cells.insert(key.to_vec(), consumed);
    }

    let cell_by_lock = collect_index_keys(
        domain_store.iterator_cf(domain_store.cf_cell_by_lock(), IteratorMode::Start),
    )?;
    let cell_by_type = collect_index_keys(
        domain_store.iterator_cf(domain_store.cf_cell_by_type(), IteratorMode::Start),
    )?;
    let cell_by_lock_code = collect_index_keys(
        domain_store.iterator_cf(domain_store.cf_cell_by_lock_code(), IteratorMode::Start),
    )?;
    let cell_by_type_code = collect_index_keys(
        domain_store.iterator_cf(domain_store.cf_cell_by_type_code(), IteratorMode::Start),
    )?;
    let cell_by_data_hash = collect_index_keys(
        domain_store.iterator_cf(domain_store.cf_cell_by_data_hash(), IteratorMode::Start),
    )?;

    Ok((
        cell_payloads,
        live_cells,
        consumed_cells,
        cell_by_lock,
        cell_by_type,
        cell_by_lock_code,
        cell_by_type_code,
        cell_by_data_hash,
    ))
}

fn collect_index_keys<I>(iter: I) -> Result<HashSet<Vec<u8>>>
where
    I: IntoIterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>>,
{
    let mut keys = HashSet::new();
    for item in iter {
        let (key, value) = item?;
        if !value.is_empty() {
            bail!(
                "cell index value must be empty in bulk artifact snapshot helper: key=0x{} value_len={}",
                hex::encode(&key),
                value.len()
            );
        }
        keys.insert(key.to_vec());
    }
    Ok(keys)
}

fn collect_core_owner_state_snapshot(
    domain_store: &CkbadgerStore,
) -> Result<CoreOwnerStateSnapshot> {
    let mut address_balances = HashMap::new();
    let addr_iter = domain_store.iterator_cf(domain_store.cf_addr_balance(), IteratorMode::Start);
    for item in addr_iter {
        let (key, value) = item?;
        let balance: AddressBalance = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize AddressBalance in core owner snapshot helper: lock_hash=0x{}, error={}",
                hex::encode(&key),
                e
            )
        })?;
        address_balances.insert(key.to_vec(), balance);
    }

    let script_infos = domain_store
        .list_script_infos()?
        .into_iter()
        .collect::<HashMap<_, _>>();

    let tokens = domain_store
        .list_tokens()?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let mut token_holders: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>> = HashMap::new();
    for type_hash in tokens.keys() {
        let holders = domain_store
            .list_token_holders(type_hash, usize::MAX)?
            .into_iter()
            .collect::<HashMap<_, _>>();
        token_holders.insert(type_hash.clone(), holders);
    }
    let mut addr_tokens: HashMap<Vec<u8>, HashMap<Vec<u8>, i128>> = HashMap::new();
    let addr_tokens_iter = domain_store.iterator_cf(
        domain_store.cf_addr_tokens_by_balance(),
        IteratorMode::Start,
    );
    for item in addr_tokens_iter {
        let (key, value) = item?;
        if !value.is_empty() {
            bail!(
                "addr_tokens_by_balance value must be empty in core owner snapshot helper: value_len={}",
                value.len()
            );
        }
        let (lock_hash, balance, type_hash) = keys::decode_addr_token_balance_key(&key);
        addr_tokens
            .entry(lock_hash)
            .or_default()
            .insert(type_hash, balance);
    }
    let token_state = owners::token::TokenStateSnapshot {
        tokens,
        token_holders,
        addr_tokens,
    };

    let deposits = domain_store
        .list_dao_deposits()?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let page_limit = deposits.len().max(1);
    let mut withdraw_lookup: HashMap<Vec<u8>, HashMap<i16, Vec<u8>>> = HashMap::new();
    for (outpoint_key, entry) in &deposits {
        if let (Some(request_tx), Some(request_output_index)) = (
            entry.withdraw_request_tx.as_ref(),
            entry.withdraw_request_output_index,
        ) {
            let linked = domain_store
                .get_dao_deposit_by_withdraw_tx(request_tx, request_output_index)?
                .ok_or_else(|| {
                    anyhow!(
                        "dao_by_withdraw_tx missing in core owner snapshot helper: request_tx=0x{}, output_index={}",
                        hex::encode(request_tx),
                        request_output_index
                    )
                })?;
            withdraw_lookup
                .entry(request_tx.clone())
                .or_default()
                .insert(request_output_index, linked.clone());
            if linked != *outpoint_key {
                bail!(
                    "dao_by_withdraw_tx mismatch in core owner snapshot helper: request_tx=0x{}, output_index={}",
                    hex::encode(request_tx),
                    request_output_index
                );
            }
        }
    }
    let mut by_status = HashMap::new();
    for status in [0i16, 1, 2] {
        let outpoints = domain_store
            .list_dao_deposits_by_status_paginated(status, page_limit, None)?
            .into_iter()
            .map(|(outpoint, _entry)| outpoint)
            .collect::<Vec<_>>();
        by_status.insert(status, outpoints);
    }
    let mut by_lock: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
    for (outpoint_key, entry) in &deposits {
        let rows = domain_store
            .list_dao_deposits_by_lock_paginated(&entry.lock_script_hash, page_limit, None)?
            .into_iter()
            .map(|(outpoint, _entry)| outpoint)
            .collect::<Vec<_>>();
        if !rows.iter().any(|row| row == outpoint_key) {
            bail!(
                "dao_by_lock_block missing outpoint in core owner snapshot helper: outpoint=0x{}",
                hex::encode(outpoint_key)
            );
        }
        by_lock.insert(entry.lock_script_hash.clone(), rows);
    }
    let dao_state = owners::dao::DaoStateSnapshot {
        deposits,
        withdraw_lookup,
        by_status,
        by_lock,
    };

    let spores = domain_store
        .list_spores(usize::MAX)?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let identities = domain_store
        .list_identities(usize::MAX)?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let cluster_aggs = domain_store
        .list_cluster_aggregates()?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let did_agg = domain_store.get_identity_collection_aggregate(&DID_CKB_SENTINEL_COLLECTION)?;
    let mut identities_by_collection = HashMap::new();
    let mut did_ids = domain_store.list_identity_ids_by_collection(
        &DID_CKB_SENTINEL_COLLECTION,
        None,
        usize::MAX,
    )?;
    did_ids.sort();
    if !did_ids.is_empty() {
        identities_by_collection.insert(DID_CKB_SENTINEL_COLLECTION.to_vec(), did_ids);
    }
    let mut spores_by_cluster = HashMap::new();
    let mut cluster_owner_counts = HashMap::new();
    for cluster_id in cluster_aggs.keys() {
        let mut members = domain_store
            .list_spores_by_cluster(cluster_id, usize::MAX)?
            .into_iter()
            .map(|(spore_id, _entry)| spore_id)
            .collect::<Vec<_>>();
        members.sort();
        spores_by_cluster.insert(cluster_id.clone(), members);
        let owners = domain_store
            .list_cluster_owner_counts(cluster_id)?
            .into_iter()
            .collect::<HashMap<_, _>>();
        cluster_owner_counts.insert(cluster_id.clone(), owners);
    }
    let did_owner_counts = domain_store
        .list_identity_owner_counts(&DID_CKB_SENTINEL_COLLECTION)?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let mut spore_outpoints = HashMap::new();
    for (spore_id, entry) in &spores {
        if entry.standard != ObjectStandard::Spore {
            continue;
        }
        let mut outpoints = domain_store.list_spore_outpoints_by_spore_id(spore_id)?;
        outpoints.sort();
        spore_outpoints.insert(spore_id.clone(), outpoints);
    }
    let mut spore_type_indexes = HashMap::new();
    let stats_spore_iter =
        domain_store.iterator_cf(domain_store.cf_stats_spore(), IteratorMode::Start);
    for item in stats_spore_iter {
        let (key, value) = item?;
        if key.len() != keys::SPORE_TYPE_INDEX_KEY_SIZE
            || key[0] != keys::STATS_PREFIX_SPORE_TYPE_INDEX
        {
            continue;
        }
        let type_hash = key[1..33].to_vec();
        let index: SporeTypeIndex = bincode::deserialize(&value).map_err(|e| {
            anyhow!(
                "failed to deserialize SporeTypeIndex in core owner snapshot helper: type_hash=0x{}, error={}",
                hex::encode(&type_hash),
                e
            )
        })?;
        spore_type_indexes.insert(type_hash, index);
    }
    let object_state = owners::object::ObjectStateSnapshot {
        spores,
        identities,
        cluster_aggs,
        did_agg,
        identities_by_collection,
        spores_by_cluster,
        did_owner_counts,
        cluster_owner_counts,
        spore_outpoints,
        spore_type_indexes,
        ..owners::object::ObjectStateSnapshot::default()
    };

    Ok(CoreOwnerStateSnapshot {
        address_balances,
        script_infos,
        token_state,
        dao_state,
        object_state,
    })
}

fn unique_temp_test_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ckbadger-{}-{}-{}",
        prefix,
        std::process::id(),
        nanos
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET;
    use crate::parser::spore::{
        CLUSTER_CODE_HASH_MAINNET_V2, SPORE_CODE_HASH_MAINNET_DID, SPORE_CODE_HASH_MAINNET_V2,
    };
    use crate::parser::udt::SUDT_CODE_HASH;
    use crate::parser::ScriptParser;
    use crate::rpc::{
        BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
        TransactionView,
    };
    use crate::sync::bulk_build::facts::{
        CellFacts, CellProtocolFacts, CellSemanticTag, DotbitProtocolFacts, OutPointKey,
        ResolvedInputFacts, ResolvedTxFacts,
    };
    use crate::sync::types::InternId;
    use ckbadger_store::store::CF_TOKEN_TRANSFERS;
    use ckbadger_store::types::{
        AssetAction, FiberChannelState, ObjectCollectionActivityEntry, TokenTransferRecord,
        TxActivityBundle, DID_CKB_SENTINEL_COLLECTION, DOTBIT_SENTINEL_COLLECTION,
    };
    use ckbadger_store::{
        keys, CF_ACTIVITIES, CF_ADDR_TXS, CF_IDENTITY_COLLECTION_ACTIVITIES,
        CF_OBJECT_COLLECTION_ACTIVITIES,
    };

    fn fixture_lock_script(args_hex: &str) -> Script {
        Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: args_hex.to_string(),
        }
    }

    fn fixture_header(number: u64, hash_byte: u8) -> HeaderView {
        HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: "0x18c7b3b2b00".to_string(),
            number: format!("0x{number:x}"),
            epoch: "0x7080006000028".to_string(),
            parent_hash: format!("0x{}", "11".repeat(32)),
            transactions_root: format!("0x{}", "22".repeat(32)),
            proposals_hash: format!("0x{}", "33".repeat(32)),
            extra_hash: format!("0x{}", "44".repeat(32)),
            dao: format!("0x{}", "00".repeat(32)),
            nonce: "0x1".to_string(),
            hash: format!("0x{}", format!("{hash_byte:02x}").repeat(32)),
        }
    }

    fn fixture_header_with_timestamp(number: u64, hash_byte: u8, timestamp_ms: u64) -> HeaderView {
        HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: format!("0x{timestamp_ms:x}"),
            number: format!("0x{number:x}"),
            epoch: "0x7080006000028".to_string(),
            parent_hash: format!("0x{}", "11".repeat(32)),
            transactions_root: format!("0x{}", "22".repeat(32)),
            proposals_hash: format!("0x{}", "33".repeat(32)),
            extra_hash: format!("0x{}", "44".repeat(32)),
            dao: format!("0x{}", "00".repeat(32)),
            nonce: "0x1".to_string(),
            hash: format!("0x{}", format!("{hash_byte:02x}").repeat(32)),
        }
    }

    fn bulk_build_addr_tx_fixture() -> BlockResponseWithCycles {
        let lock_a_args = format!("0x{}", "01".repeat(20));
        let lock_b_args = format!("0x{}", "02".repeat(20));
        let create_tx = TransactionView {
            hash: format!("0x{}", "aa".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&lock_a_args),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        let split_tx = TransactionView {
            hash: format!("0x{}", "bb".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: create_tx.hash.clone(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_a_args),
                    type_: None,
                },
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_b_args),
                    type_: None,
                },
            ],
            outputs_data: vec!["0x".to_string(), "0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_000_888, 0x99),
                uncles: vec![],
                transactions: vec![create_tx, split_tx],
                proposals: vec![],
            },
            cycles: None,
        }
    }

    fn fixture_sudt_type_script() -> Script {
        Script {
            code_hash: SUDT_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", "11".repeat(20)),
        }
    }

    fn encode_molecule_bytes(data: &[u8]) -> Vec<u8> {
        let len = data.len() as u32;
        let mut result = len.to_le_bytes().to_vec();
        result.extend_from_slice(data);
        result
    }

    fn create_cluster_type_script(cluster_id: &[u8; 32]) -> Script {
        Script {
            code_hash: CLUSTER_CODE_HASH_MAINNET_V2.to_string(),
            hash_type: "data1".to_string(),
            args: format!("0x{}", hex::encode(cluster_id)),
        }
    }

    fn create_spore_type_script(spore_id: &[u8; 32]) -> Script {
        Script {
            code_hash: SPORE_CODE_HASH_MAINNET_V2.to_string(),
            hash_type: "data1".to_string(),
            args: format!("0x{}", hex::encode(spore_id)),
        }
    }

    fn create_did_type_script(did_id: &[u8; 32]) -> Script {
        Script {
            code_hash: SPORE_CODE_HASH_MAINNET_DID.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", hex::encode(did_id)),
        }
    }

    fn create_cluster_data(name: &str, description: &str) -> Vec<u8> {
        let name_bytes = encode_molecule_bytes(name.as_bytes());
        let description_bytes = encode_molecule_bytes(description.as_bytes());
        let offset_name = 16u32;
        let offset_description = offset_name + name_bytes.len() as u32;
        let offset_end = offset_description + description_bytes.len() as u32;

        let mut data = Vec::new();
        data.extend_from_slice(&offset_end.to_le_bytes());
        data.extend_from_slice(&offset_name.to_le_bytes());
        data.extend_from_slice(&offset_description.to_le_bytes());
        data.extend_from_slice(&offset_end.to_le_bytes());
        data.extend_from_slice(&name_bytes);
        data.extend_from_slice(&description_bytes);
        data
    }

    fn create_spore_data(
        content_type: &str,
        content: &[u8],
        cluster_id: Option<&[u8; 32]>,
    ) -> Vec<u8> {
        let content_type_bytes = encode_molecule_bytes(content_type.as_bytes());
        let content_bytes = encode_molecule_bytes(content);
        let cluster_id_bytes = cluster_id.map(|id| encode_molecule_bytes(id));

        let offset_content_type = 16u32;
        let offset_content = offset_content_type + content_type_bytes.len() as u32;
        let offset_cluster_id = offset_content + content_bytes.len() as u32;
        let total_size = offset_cluster_id
            + cluster_id_bytes
                .as_ref()
                .map(|bytes| bytes.len())
                .unwrap_or(0) as u32;

        let mut data = Vec::new();
        data.extend_from_slice(&total_size.to_le_bytes());
        data.extend_from_slice(&offset_content_type.to_le_bytes());
        data.extend_from_slice(&offset_content.to_le_bytes());
        data.extend_from_slice(&offset_cluster_id.to_le_bytes());
        data.extend_from_slice(&content_type_bytes);
        data.extend_from_slice(&content_bytes);
        if let Some(cluster_id_bytes) = cluster_id_bytes {
            data.extend_from_slice(&cluster_id_bytes);
        }
        data
    }

    fn fixture_fiber_funding_lock_script(args_hex: &str) -> Script {
        Script {
            code_hash: FUNDING_LOCK_CODE_HASH_MAINNET.to_string(),
            hash_type: "type".to_string(),
            args: args_hex.to_string(),
        }
    }

    fn u128_data_hex(amount: u128) -> String {
        format!("0x{}", hex::encode(amount.to_le_bytes()))
    }

    fn bulk_build_token_transfer_fixture() -> BlockResponseWithCycles {
        let lock_a_args = format!("0x{}", "01".repeat(20));
        let lock_b_args = format!("0x{}", "02".repeat(20));
        let sudt_type = fixture_sudt_type_script();
        let create_tx = TransactionView {
            hash: format!("0x{}", "c1".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&lock_a_args),
                type_: Some(sudt_type.clone()),
            }],
            outputs_data: vec![u128_data_hex(200)],
            witnesses: vec!["0x".to_string()],
        };

        let split_tx = TransactionView {
            hash: format!("0x{}", "d2".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: create_tx.hash.clone(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_a_args),
                    type_: Some(sudt_type.clone()),
                },
                CellOutput {
                    capacity: format!("0x{:x}", 100_00000000u64),
                    lock: fixture_lock_script(&lock_b_args),
                    type_: Some(sudt_type),
                },
            ],
            outputs_data: vec![u128_data_hex(100), u128_data_hex(100)],
            witnesses: vec!["0x".to_string()],
        };

        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_000_889, 0x9a),
                uncles: vec![],
                transactions: vec![create_tx, split_tx],
                proposals: vec![],
            },
            cycles: None,
        }
    }

    fn bulk_build_fiber_open_fixture() -> BlockResponseWithCycles {
        let participant_args = format!("0x{}", "03".repeat(20));
        let funding_args = format!("0x{}", "bb".repeat(20));
        let create_tx = TransactionView {
            hash: format!("0x{}", "f1".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&participant_args),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        let open_tx = TransactionView {
            hash: format!("0x{}", "f2".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: create_tx.hash.clone(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: format!("0x{:x}", 130_00000000u64),
                    lock: fixture_fiber_funding_lock_script(&funding_args),
                    type_: None,
                },
                CellOutput {
                    capacity: format!("0x{:x}", 70_00000000u64),
                    lock: fixture_lock_script(&participant_args),
                    type_: None,
                },
            ],
            outputs_data: vec!["0x".to_string(), "0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(14_000_990, 0xa7),
                uncles: vec![],
                transactions: vec![create_tx, open_tx],
                proposals: vec![],
            },
            cycles: None,
        }
    }

    fn bulk_build_object_activity_fixture() -> Vec<BlockResponseWithCycles> {
        let cluster_id = [0x11; 32];
        let spore_id = [0x22; 32];
        let did_id = [0x33; 32];

        let create_tx = TransactionView {
            hash: format!("0x{}", "a1".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: format!("0x{:x}", 200_00000000u64),
                    lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
                    type_: Some(create_cluster_type_script(&cluster_id)),
                },
                CellOutput {
                    capacity: format!("0x{:x}", 200_00000000u64),
                    lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
                    type_: Some(create_spore_type_script(&spore_id)),
                },
                CellOutput {
                    capacity: format!("0x{:x}", 150_00000000u64),
                    lock: fixture_lock_script(&format!("0x{}", "03".repeat(20))),
                    type_: Some(create_did_type_script(&did_id)),
                },
            ],
            outputs_data: vec![
                format!(
                    "0x{}",
                    hex::encode(create_cluster_data(
                        "Genesis Cluster",
                        "{\"dob\":{\"ver\":1}}"
                    ))
                ),
                format!(
                    "0x{}",
                    hex::encode(create_spore_data(
                        "image/png",
                        b"spore-content",
                        Some(&cluster_id)
                    ))
                ),
                "0x".to_string(),
            ],
            witnesses: vec!["0x".to_string()],
        };

        let dummy_cellbase = TransactionView {
            hash: format!("0x{}", "b0".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 500_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "09".repeat(20))),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };

        let transfer_and_burn_tx = TransactionView {
            hash: format!("0x{}", "b1".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![
                CellInput {
                    since: "0x0".to_string(),
                    previous_output: OutPoint {
                        tx_hash: create_tx.hash.clone(),
                        index: "0x1".to_string(),
                    },
                },
                CellInput {
                    since: "0x0".to_string(),
                    previous_output: OutPoint {
                        tx_hash: create_tx.hash.clone(),
                        index: "0x2".to_string(),
                    },
                },
            ],
            outputs: vec![CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "02".repeat(20))),
                type_: Some(create_spore_type_script(&spore_id)),
            }],
            outputs_data: vec![format!(
                "0x{}",
                hex::encode(create_spore_data(
                    "image/png",
                    b"spore-content",
                    Some(&cluster_id)
                ))
            )],
            witnesses: vec!["0x".to_string()],
        };

        vec![
            BlockResponseWithCycles {
                block: BlockView {
                    header: fixture_header_with_timestamp(14_001_000, 0x81, 1_700_000_000_000),
                    uncles: vec![],
                    transactions: vec![create_tx],
                    proposals: vec![],
                },
                cycles: None,
            },
            BlockResponseWithCycles {
                block: BlockView {
                    header: fixture_header_with_timestamp(14_001_001, 0x82, 1_700_000_010_000),
                    uncles: vec![],
                    transactions: vec![dummy_cellbase, transfer_and_burn_tx],
                    proposals: vec![],
                },
                cycles: None,
            },
        ]
    }

    #[test]
    fn build_history_rows_materializes_addr_txs_for_unique_touched_locks() {
        let block = bulk_build_addr_tx_fixture();
        let lock_a_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "01".repeat(20)
        )));
        let lock_b_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "02".repeat(20)
        )));
        let create_tx_hash =
            hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
        let split_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("split tx hash");

        let mut interner = interner::IdentityInterner::default();
        let arena =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &mut interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");

        let addr_rows: Vec<_> = build_history_rows(&arena, &resolved, &interner, true)
            .expect("history rows")
            .rows
            .into_iter()
            .filter(|row| row.cf_name == CF_ADDR_TXS)
            .collect();

        let expected = [
            keys::encode_addr_tx_key(&lock_a_hash, 14_000_888, 0, &create_tx_hash),
            keys::encode_addr_tx_key(&lock_a_hash, 14_000_888, 1, &split_tx_hash),
            keys::encode_addr_tx_key(&lock_b_hash, 14_000_888, 1, &split_tx_hash),
        ];

        assert_eq!(addr_rows.len(), expected.len());
        let actual_keys: HashSet<Vec<u8>> = addr_rows.iter().map(|row| row.key.clone()).collect();
        assert_eq!(actual_keys.len(), expected.len());
        for key in expected {
            assert!(actual_keys.contains(&key));
        }
    }

    #[test]
    fn build_history_rows_materializes_token_transfer_records_in_tx_order() {
        let block = bulk_build_token_transfer_fixture();
        let lock_a_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "01".repeat(20)
        )));
        let lock_b_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "02".repeat(20)
        )));
        let type_hash = ScriptParser::compute_script_hash(&fixture_sudt_type_script());
        let create_tx_hash =
            hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
        let split_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("split tx hash");

        let mut interner = interner::IdentityInterner::default();
        let arena =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &mut interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");

        let token_rows: Vec<_> = build_history_rows(&arena, &resolved, &interner, true)
            .expect("history rows")
            .rows
            .into_iter()
            .filter(|row| row.cf_name == CF_TOKEN_TRANSFERS)
            .collect();

        assert_eq!(token_rows.len(), 2);
        let token_records: HashMap<Vec<u8>, TokenTransferRecord> = token_rows
            .into_iter()
            .map(|row| {
                (
                    row.key,
                    bincode::deserialize(&row.value).expect("deserialize token transfer"),
                )
            })
            .collect();

        let mint_key = keys::encode_token_transfer_key(&type_hash, 14_000_889, 0);
        let transfer_key = keys::encode_token_transfer_key(&type_hash, 14_000_889, 1);
        let mint = token_records.get(&mint_key).expect("mint transfer");
        assert_eq!(mint.tx_hash, create_tx_hash);
        assert_eq!(mint.block_number, 14_000_889);
        assert_eq!(mint.from_lock_hash, None);
        assert_eq!(mint.to_lock_hash, lock_a_hash);
        assert_eq!(mint.amount, 200);
        assert!(mint.is_mint);
        assert!(!mint.is_burn);

        let transfer = token_records.get(&transfer_key).expect("split transfer");
        assert_eq!(transfer.tx_hash, split_tx_hash);
        assert_eq!(transfer.block_number, 14_000_889);
        assert_eq!(transfer.from_lock_hash, Some(lock_a_hash));
        assert_eq!(transfer.to_lock_hash, lock_b_hash);
        assert_eq!(transfer.amount, 100);
        assert!(!transfer.is_mint);
        assert!(!transfer.is_burn);
    }

    #[test]
    fn build_history_rows_materializes_ckb_activity_bundles_in_tx_order() {
        let block = bulk_build_addr_tx_fixture();
        let create_tx_hash =
            hex::decode(&block.block.transactions[0].hash[2..]).expect("create tx hash");
        let split_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("split tx hash");
        let lock_a_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "01".repeat(20)
        )));
        let lock_b_hash = ScriptParser::compute_script_hash(&fixture_lock_script(&format!(
            "0x{}",
            "02".repeat(20)
        )));

        let mut interner = interner::IdentityInterner::default();
        let arena =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &mut interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");

        let activity_rows: Vec<_> = build_history_rows(&arena, &resolved, &interner, true)
            .expect("history rows")
            .rows
            .into_iter()
            .filter(|row| row.cf_name == CF_ACTIVITIES)
            .collect();

        assert_eq!(activity_rows.len(), 2);
        let activity_bundles: HashMap<Vec<u8>, TxActivityBundle> = activity_rows
            .into_iter()
            .map(|row| {
                (
                    row.key,
                    bincode::deserialize(&row.value).expect("deserialize tx activity bundle"),
                )
            })
            .collect();

        let create_key = keys::encode_tx_activity_bundle_key(14_000_888, 0, &create_tx_hash);
        let split_key = keys::encode_tx_activity_bundle_key(14_000_888, 1, &split_tx_hash);
        let create_bundle = activity_bundles.get(&create_key).expect("cellbase bundle");
        assert_eq!(create_bundle.tx_hash, create_tx_hash);
        assert!(create_bundle.is_cellbase);
        assert_eq!(create_bundle.owners.len(), 1);

        let split_bundle = activity_bundles.get(&split_key).expect("split bundle");
        assert_eq!(split_bundle.tx_hash, split_tx_hash);
        assert!(!split_bundle.is_cellbase);
        assert_eq!(split_bundle.owners.len(), 2);

        let owner_a = split_bundle
            .owners
            .iter()
            .find(|owner| owner.lock_hash == lock_a_hash)
            .expect("owner a");
        assert_eq!(owner_a.ckb_delta, -100_00000000);
        assert!(owner_a.asset_changes.is_empty());
        assert_eq!(owner_a.peers, vec![lock_b_hash.clone()]);

        let owner_b = split_bundle
            .owners
            .iter()
            .find(|owner| owner.lock_hash == lock_b_hash)
            .expect("owner b");
        assert_eq!(owner_b.ckb_delta, 100_00000000);
        assert!(owner_b.asset_changes.is_empty());
        assert_eq!(owner_b.peers, vec![lock_a_hash]);
    }

    #[test]
    fn bulk_build_materialization_processes_fiber_channel_events_from_activity_bundles() {
        let block = bulk_build_fiber_open_fixture();
        let open_tx_hash =
            hex::decode(&block.block.transactions[1].hash[2..]).expect("open tx hash");
        let funding_args = hex::decode("bb".repeat(20)).expect("funding args");
        let expected_channel_id = keys::encode_fiber_channel_id(&open_tx_hash, 0);

        let mut interner = interner::IdentityInterner::default();
        let arena =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&[block], &mut interner)
                .expect("facts arena");
        let mut sequencer = sequencer::BulkSequencer::default();
        let resolved = sequencer.resolve(&arena).expect("resolved txs");
        let mut owners = CoreOwners::default();
        let ctx = owners::ReducerContext::new(&interner);
        let history = build_history_rows(&arena, &resolved, &interner, true).expect("history rows");
        let sealed_rows = build_sealed_aggregate_rows(&history.rows).expect("sealed rows");
        let final_snapshot_rows =
            build_final_snapshot_rows(&sequencer, &interner).expect("final snapshot rows");

        let open_bundle = history
            .rows
            .iter()
            .filter(|row| row.cf_name == CF_ACTIVITIES)
            .map(|row| {
                bincode::deserialize::<TxActivityBundle>(&row.value)
                    .expect("deserialize tx activity bundle")
            })
            .find(|bundle| !bundle.is_cellbase)
            .expect("non-cellbase activity bundle");
        let participant_owner = open_bundle
            .owners
            .iter()
            .find(|owner| !owner.protocol_actions.is_empty())
            .expect("fiber participant owner");
        assert_eq!(participant_owner.protocol_actions.len(), 1);
        assert_eq!(participant_owner.protocol_actions[0].protocol, "fiber");
        assert_eq!(participant_owner.protocol_actions[0].action, "channel_open");

        for tx in &resolved {
            owners.apply_tx(tx, &ctx).expect("apply core owners");
        }

        let root = unique_temp_test_dir("bulk-build-fiber-activity");
        std::fs::create_dir_all(&root).expect("create root dir");
        let domain_path = root.join("domain");
        let append_path = root.join("append-only");
        std::fs::create_dir_all(&domain_path).expect("create domain dir");
        std::fs::create_dir_all(&append_path).expect("create append-only dir");

        let domain_store = CkbadgerStore::open_domain(&domain_path).expect("open domain store");
        let append_store =
            CkbadgerStore::open_append_only(&append_path).expect("open append-only store");
        let mut materializer = materialize::Materializer::new(&domain_store, &append_store);
        materializer
            .stream_history_rows(&history.rows)
            .expect("stream history rows");
        materialize_activity_secondary_state(&domain_store, &history.rows)
            .expect("materialize activity secondary state");
        materializer
            .stream_sealed_aggregate_rows(&sealed_rows)
            .expect("stream sealed rows");
        materializer
            .materialize_final_snapshot(&final_snapshot_rows)
            .expect("materialize final snapshot rows");
        owners
            .materialize_all(&mut materializer)
            .expect("materialize core owners");
        let _ = materializer.finish();

        let channels = domain_store
            .list_fiber_channels(10, None, None)
            .expect("list fiber channels");
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].0, expected_channel_id);
        assert_eq!(channels[0].1.state, FiberChannelState::Open);
        assert_eq!(channels[0].1.capacity, 130_00000000);
        assert_eq!(channels[0].1.funding_lock_args, funding_args);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn build_history_rows_materializes_spore_and_did_collection_activities() {
        let blocks = bulk_build_object_activity_fixture();
        let cluster_id = [0x11; 32];
        let create_block_hash = vec![0x81; 32];
        let transfer_block_hash = vec![0x82; 32];
        let create_tx_hash = vec![0xa1; 32];
        let transfer_tx_hash = vec![0xb1; 32];

        let mut interner = interner::IdentityInterner::default();
        let arena =
            crate::sync::pipeline::build_bulk_facts_arena_from_blocks(&blocks, &mut interner)
                .expect("facts arena");
        let resolved = sequencer::BulkSequencer::default()
            .resolve(&arena)
            .expect("resolved txs");

        let history_rows: Vec<_> = build_history_rows(&arena, &resolved, &interner, true)
            .expect("history rows")
            .rows
            .into_iter()
            .filter(|row| {
                row.cf_name == CF_OBJECT_COLLECTION_ACTIVITIES
                    || row.cf_name == CF_IDENTITY_COLLECTION_ACTIVITIES
            })
            .collect();

        let object_rows: std::collections::HashMap<Vec<u8>, ObjectCollectionActivityEntry> =
            history_rows
                .iter()
                .filter(|row| row.cf_name == CF_OBJECT_COLLECTION_ACTIVITIES)
                .map(|row| {
                    (
                        row.key.clone(),
                        bincode::deserialize(&row.value)
                            .expect("deserialize object collection activity"),
                    )
                })
                .collect();
        let identity_rows: std::collections::HashMap<Vec<u8>, ObjectCollectionActivityEntry> =
            history_rows
                .iter()
                .filter(|row| row.cf_name == CF_IDENTITY_COLLECTION_ACTIVITIES)
                .map(|row| {
                    (
                        row.key.clone(),
                        bincode::deserialize(&row.value)
                            .expect("deserialize identity collection activity"),
                    )
                })
                .collect();

        let cluster_mint_key = keys::encode_nft_collection_activity_key(
            &cluster_id,
            14_001_000,
            0,
            &create_block_hash,
            &create_tx_hash,
        );
        let cluster_transfer_key = keys::encode_nft_collection_activity_key(
            &cluster_id,
            14_001_001,
            1,
            &transfer_block_hash,
            &transfer_tx_hash,
        );
        let did_mint_key = keys::encode_nft_collection_activity_key(
            &DID_CKB_SENTINEL_COLLECTION,
            14_001_000,
            0,
            &create_block_hash,
            &create_tx_hash,
        );

        assert_eq!(object_rows.len(), 2);
        assert_eq!(identity_rows.len(), 1);

        let cluster_mint = object_rows
            .get(cluster_mint_key.as_slice())
            .expect("cluster mint activity");
        assert_eq!(cluster_mint.tx_hash, create_tx_hash);
        assert_eq!(cluster_mint.block_hash, create_block_hash);
        assert_eq!(cluster_mint.actions.len(), 1);
        assert!(matches!(cluster_mint.actions[0], AssetAction::Mint));

        let cluster_transfer = object_rows
            .get(cluster_transfer_key.as_slice())
            .expect("cluster transfer activity");
        assert_eq!(cluster_transfer.tx_hash, transfer_tx_hash);
        assert_eq!(cluster_transfer.block_hash, transfer_block_hash);
        assert_eq!(cluster_transfer.actions.len(), 1);
        assert!(matches!(cluster_transfer.actions[0], AssetAction::Transfer));

        let did_mint = identity_rows
            .get(did_mint_key.as_slice())
            .expect("did mint activity");
        assert_eq!(did_mint.tx_hash, create_tx_hash);
        assert_eq!(did_mint.block_hash, create_block_hash);
        assert_eq!(did_mint.actions.len(), 1);
        assert!(matches!(did_mint.actions[0], AssetAction::Mint));
    }

    #[test]
    fn build_object_collection_activity_rows_materializes_dotbit_identity_activities() {
        fn dotbit_output(
            tx_hash_byte: u8,
            lock_hash_id: InternId,
            account_id: [u8; 20],
            account: &str,
        ) -> CellFacts {
            CellFacts {
                outpoint: OutPointKey::new([tx_hash_byte; 32], 0),
                created_at_block: 0,
                capacity: 200_00000000,
                lock_script_hash_id: lock_hash_id,
                lock_code_hash_id: InternId::new(401),
                lock_hash_type: 1,
                lock_args_id: InternId::new(402),
                type_script_hash_id: Some(InternId::new(403)),
                type_code_hash_id: Some(InternId::new(404)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(405)),
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Dotbit,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Dotbit(DotbitProtocolFacts {
                    account_id,
                    account: Some(account.to_string()),
                    next_account_id: None,
                    expired_at: Some(1_800_000_000),
                    registered_at: Some(1_700_000_000),
                    status: Some(0),
                })),
            }
        }

        fn dotbit_input(
            tx_hash_byte: u8,
            lock_hash_id: InternId,
            account_id: [u8; 20],
            account: &str,
        ) -> ResolvedInputFacts {
            ResolvedInputFacts {
                outpoint: OutPointKey::new([tx_hash_byte; 32], 0),
                created_at_block: 0,
                capacity: 200_00000000,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data_hash: None,
                udt_amount: None,
                lock_script_hash_id: lock_hash_id,
                lock_code_hash_id: InternId::new(401),
                lock_hash_type: 1,
                lock_args_id: InternId::new(402),
                type_script_hash_id: Some(InternId::new(403)),
                type_code_hash_id: Some(InternId::new(404)),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(405)),
                semantic_tag: CellSemanticTag::Dotbit,
                dao_state: None,
                protocol_facts: Some(CellProtocolFacts::Dotbit(DotbitProtocolFacts {
                    account_id,
                    account: Some(account.to_string()),
                    next_account_id: None,
                    expired_at: Some(1_800_000_000),
                    registered_at: Some(1_700_000_000),
                    status: Some(0),
                })),
            }
        }

        let account_a = [0x51; 20];
        let account_b = [0x61; 20];
        let owner_a = InternId::new(501);
        let owner_b = InternId::new(502);

        let resolved = vec![
            ResolvedTxFacts {
                tx_hash: [0x31; 32],
                block_number: 300,
                block_hash: [0xa0; 32],
                timestamp_ms: 1_700_100_000_000,
                block_dao_ar: 0,
                tx_index: 0,
                is_cellbase: false,
                dotbit_action: Some("confirm_proposal".to_string()),
                resolved_inputs: Vec::new(),
                cells: vec![dotbit_output(0x31, owner_a, account_a, "alice.bit")],
            },
            ResolvedTxFacts {
                tx_hash: [0x32; 32],
                block_number: 301,
                block_hash: [0xa1; 32],
                timestamp_ms: 1_700_100_360_000,
                block_dao_ar: 0,
                tx_index: 0,
                is_cellbase: false,
                dotbit_action: Some("transfer_account".to_string()),
                resolved_inputs: vec![dotbit_input(0x31, owner_a, account_a, "alice.bit")],
                cells: vec![dotbit_output(0x32, owner_b, account_a, "alice.bit")],
            },
            ResolvedTxFacts {
                tx_hash: [0x33; 32],
                block_number: 302,
                block_hash: [0xa2; 32],
                timestamp_ms: 1_700_100_720_000,
                block_dao_ar: 0,
                tx_index: 0,
                is_cellbase: false,
                dotbit_action: Some("recycle_expired_account".to_string()),
                resolved_inputs: vec![dotbit_input(0x32, owner_b, account_a, "alice.bit")],
                cells: Vec::new(),
            },
            ResolvedTxFacts {
                tx_hash: [0x34; 32],
                block_number: 303,
                block_hash: [0xa3; 32],
                timestamp_ms: 1_700_101_080_000,
                block_dao_ar: 0,
                tx_index: 0,
                is_cellbase: false,
                dotbit_action: Some("confirm_proposal".to_string()),
                resolved_inputs: vec![dotbit_input(0x40, owner_a, account_b, "bob.bit")],
                cells: vec![dotbit_output(0x34, owner_a, account_b, "bob.bit")],
            },
        ];

        let mut identity_activity_count_deltas = HashMap::new();
        let rows =
            build_object_collection_activity_rows(&resolved, &mut identity_activity_count_deltas)
                .expect("dotbit collection activity rows");

        let identity_rows: HashMap<Vec<u8>, ObjectCollectionActivityEntry> = rows
            .iter()
            .filter(|row| row.cf_name == CF_IDENTITY_COLLECTION_ACTIVITIES)
            .map(|row| {
                (
                    row.key.clone(),
                    bincode::deserialize(&row.value)
                        .expect("deserialize identity collection activity"),
                )
            })
            .collect();

        let mint_key = keys::encode_nft_collection_activity_key(
            &DOTBIT_SENTINEL_COLLECTION,
            300,
            0,
            &[0xa0; 32],
            &[0x31; 32],
        );
        let transfer_key = keys::encode_nft_collection_activity_key(
            &DOTBIT_SENTINEL_COLLECTION,
            301,
            0,
            &[0xa1; 32],
            &[0x32; 32],
        );
        let recycle_key = keys::encode_nft_collection_activity_key(
            &DOTBIT_SENTINEL_COLLECTION,
            302,
            0,
            &[0xa2; 32],
            &[0x33; 32],
        );

        assert_eq!(identity_rows.len(), 3);
        assert!(identity_activity_count_deltas.is_empty());

        let mint = identity_rows.get(mint_key.as_slice()).expect("dotbit mint");
        assert_eq!(mint.actions.len(), 1);
        assert!(matches!(mint.actions[0], AssetAction::Mint));

        let transfer = identity_rows
            .get(transfer_key.as_slice())
            .expect("dotbit transfer");
        assert_eq!(transfer.actions.len(), 1);
        assert!(matches!(transfer.actions[0], AssetAction::Transfer));

        let recycle = identity_rows
            .get(recycle_key.as_slice())
            .expect("dotbit recycle");
        assert_eq!(recycle.actions.len(), 1);
        assert!(matches!(recycle.actions[0], AssetAction::Recycle));
    }
}
