use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{anyhow, bail, Result};
use chrono::NaiveDate;
use ckbadger_store::keys;
use ckbadger_store::types::{DaoDailySnapshot, DaoDepositCacheEntry};
use ckbadger_store::{
    CkbadgerStore, CF_DAO_BY_BLOCK, CF_DAO_BY_LOCK_BLOCK, CF_DAO_BY_STATUS_BLOCK,
    CF_DAO_BY_WITHDRAW_TX, CF_DAO_DEPOSITS, CF_STATS_DAO,
};
use rustc_hash::FxHashMap;

use super::{BulkReducer, ReducerContext};
use crate::db::writer::dao::calculate_dao_compensation_from_ar;
use crate::rpc::BlockResponseWithCycles;
use crate::sync::bulk_build::facts::{
    BlockFacts, CellFacts, CellSemanticTag, DaoCellState, OutPointKey, ResolvedInputFacts,
    ResolvedTxFacts,
};
use crate::sync::bulk_build::interner::IdentityInterner;
use crate::sync::bulk_build::materialize::{MaterializedRow, Materializer};
use crate::sync::bulk_build::sequencer::BulkSequencer;
use crate::sync::dao_helpers::{
    accumulate_miner_secondary_for_block, derive_running_depositors, extract_dao_csu, BatchStats,
};
use crate::sync::pipeline::build_bulk_facts_arena_from_blocks;

#[derive(Debug, Default)]
pub(crate) struct DaoOwner {
    deposits: FxHashMap<OutPointKey, DaoDepositCacheEntry>,
    request_outpoints: FxHashMap<OutPointKey, OutPointKey>,
    snapshot_dates: BTreeSet<NaiveDate>,
    daily_dao_fields: FxHashMap<NaiveDate, (i128, i128, i128)>,
    daily_active_delta: FxHashMap<NaiveDate, i128>,
    daily_protocol_delta: FxHashMap<NaiveDate, i128>,
    daily_gross_deposit_delta: FxHashMap<NaiveDate, i128>,
    daily_new_deposits_delta: FxHashMap<NaiveDate, i64>,
    daily_withdrawals_delta: FxHashMap<NaiveDate, i64>,
    daily_unique_depositors_delta: FxHashMap<NaiveDate, i64>,
    daily_cumulative_depositors_delta: FxHashMap<NaiveDate, i64>,
    daily_secondary_miner_delta: FxHashMap<NaiveDate, i128>,
    /// Running protocol-level deposited total, updated per-block for accurate
    /// protocol-locked DAO statistics.
    running_protocol_deposited: i128,
    /// Protocol delta accumulated within the current block (reset per block).
    current_block_protocol_delta: i128,
    active_deposit_counts_by_lock: FxHashMap<Vec<u8>, i64>,
    /// Tracks all lock_hashes that have ever created a DAO deposit (never removed).
    ever_deposited: HashSet<Vec<u8>>,
    /// Per-day unique addresses that made deposits (including repeat depositors).
    daily_depositing_addresses: FxHashMap<NaiveDate, HashSet<Vec<u8>>>,
    /// Parent block's DAO `C`/`U`, the base of the per-block miner secondary
    /// split `floor(s_i * U_{i-1} / C_{i-1})` (RFC-0023).
    prev_dao_cu: Option<(i128, i128)>,
    /// Consensus secondary issuance per epoch, from the node's `get_consensus`.
    /// `None` until configured; the miner split then fails loudly rather than
    /// silently issuing zero.
    secondary_epoch_reward: Option<u64>,
    /// Per-date end-of-day block number and AR, for exact unmade_dao_interests
    /// computation during materialization.
    daily_end_of_day: FxHashMap<NaiveDate, (i64, u64)>,
}

impl DaoOwner {
    /// Supply the consensus secondary issuance per epoch, required by the
    /// per-block miner secondary split. Must be called before any block is
    /// recorded; `record_block` fails loudly otherwise instead of silently
    /// attributing zero secondary issuance.
    pub(crate) fn set_secondary_epoch_reward(&mut self, shannons: u64) {
        self.secondary_epoch_reward = Some(shannons);
    }

    /// Seed the parent block's DAO `C`/`U` when bulk build resumes at a block
    /// above genesis. Without it the first recorded block has no state to
    /// split its secondary issuance against and `record_block` fails fast.
    pub(crate) fn seed_prev_dao_cu(&mut self, total_issuance: i128, occupied_capacity: i128) {
        self.prev_dao_cu = Some((total_issuance, occupied_capacity));
    }
}

struct DaoCompensationTimeline<'a> {
    deposits: &'a FxHashMap<OutPointKey, DaoDepositCacheEntry>,
    deposit_events: Vec<(i64, OutPointKey)>,
    request_events: Vec<(i64, OutPointKey)>,
    withdrawal_events: Vec<(i64, OutPointKey)>,
    next_deposit: usize,
    next_request: usize,
    next_withdrawal: usize,
    active: HashSet<OutPointKey>,
    frozen: FxHashMap<OutPointKey, i128>,
    frozen_total: i128,
    claimed: i128,
    last_observation_block: Option<i64>,
}

impl<'a> DaoCompensationTimeline<'a> {
    fn new(deposits: &'a FxHashMap<OutPointKey, DaoDepositCacheEntry>) -> Result<Self> {
        let mut deposit_events = Vec::with_capacity(deposits.len());
        let mut request_events = Vec::new();
        let mut withdrawal_events = Vec::new();

        for (outpoint, entry) in deposits {
            if entry.deposit_block_number < 0 {
                bail!(
                    "negative DAO deposit block in bulk compensation timeline: outpoint={}, deposit_block={}",
                    format_outpoint(outpoint),
                    entry.deposit_block_number
                );
            }
            deposit_events.push((entry.deposit_block_number, *outpoint));

            match (
                entry.status,
                entry.withdraw_request_block,
                entry.withdraw_block,
            ) {
                (0, None, None) => {}
                (1, Some(request_block), None) => {
                    Self::validate_request_block(*outpoint, entry, request_block)?;
                    request_events.push((request_block, *outpoint));
                }
                (2, Some(request_block), Some(withdraw_block)) => {
                    Self::validate_request_block(*outpoint, entry, request_block)?;
                    if withdraw_block <= request_block {
                        bail!(
                            "DAO completion block must follow request block in bulk compensation timeline: outpoint={}, request_block={}, withdraw_block={}",
                            format_outpoint(outpoint),
                            request_block,
                            withdraw_block
                        );
                    }
                    if entry.compensation.is_none() {
                        bail!(
                            "completed DAO deposit missing compensation in bulk compensation timeline: outpoint={}, withdraw_block={}",
                            format_outpoint(outpoint),
                            withdraw_block
                        );
                    }
                    request_events.push((request_block, *outpoint));
                    withdrawal_events.push((withdraw_block, *outpoint));
                }
                _ => {
                    bail!(
                        "inconsistent DAO lifecycle in bulk compensation timeline: outpoint={}, status={}, deposit_block={}, request_block={:?}, withdraw_block={:?}",
                        format_outpoint(outpoint),
                        entry.status,
                        entry.deposit_block_number,
                        entry.withdraw_request_block,
                        entry.withdraw_block
                    );
                }
            }
        }

        Self::sort_events(&mut deposit_events);
        Self::sort_events(&mut request_events);
        Self::sort_events(&mut withdrawal_events);

        Ok(Self {
            deposits,
            deposit_events,
            request_events,
            withdrawal_events,
            next_deposit: 0,
            next_request: 0,
            next_withdrawal: 0,
            active: HashSet::new(),
            frozen: FxHashMap::default(),
            frozen_total: 0,
            claimed: 0,
            last_observation_block: None,
        })
    }

    fn validate_request_block(
        outpoint: OutPointKey,
        entry: &DaoDepositCacheEntry,
        request_block: i64,
    ) -> Result<()> {
        if request_block < entry.deposit_block_number {
            bail!(
                "DAO request block precedes deposit in bulk compensation timeline: outpoint={}, deposit_block={}, request_block={}",
                format_outpoint(&outpoint),
                entry.deposit_block_number,
                request_block
            );
        }
        Ok(())
    }

    fn sort_events(events: &mut [(i64, OutPointKey)]) {
        events.sort_unstable_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.tx_hash.cmp(&right.1.tx_hash))
                .then_with(|| left.1.index.cmp(&right.1.index))
        });
    }

    fn entry(&self, outpoint: OutPointKey) -> Result<&DaoDepositCacheEntry> {
        self.deposits.get(&outpoint).ok_or_else(|| {
            anyhow!(
                "missing DAO entry in bulk compensation timeline: outpoint={}",
                format_outpoint(&outpoint)
            )
        })
    }

    fn frozen_compensation(&self, outpoint: OutPointKey) -> Result<i128> {
        let entry = self.entry(outpoint)?;
        let request_block = entry.withdraw_request_block.ok_or_else(|| {
            anyhow!(
                "DAO deposit missing request block in bulk compensation timeline: outpoint={}",
                format_outpoint(&outpoint)
            )
        })?;
        let request_ar = entry.withdraw_request_ar.ok_or_else(|| {
            anyhow!(
                "phase-1 DAO deposit missing request AR in bulk compensation timeline: outpoint={}, request_block={:?}",
                format_outpoint(&outpoint),
                entry.withdraw_request_block
            )
        })?;
        let request_ar = u64::try_from(request_ar).map_err(|_| {
            anyhow!(
                "negative DAO request AR in bulk compensation timeline: outpoint={}, request_ar={}",
                format_outpoint(&outpoint),
                request_ar
            )
        })?;
        let contribution =
            ckbadger_store::dao_compensation_for_entry_at(entry, request_block, request_ar)?;
        if contribution.claimed != 0 || contribution.active_unmade != 0 {
            bail!(
                "DAO request produced non-frozen compensation in bulk timeline: outpoint={}, request_block={}, contribution={:?}",
                format_outpoint(&outpoint),
                request_block,
                contribution
            );
        }
        Ok(contribution.unclaimed)
    }

    fn advance_to(
        &mut self,
        observation_block: i64,
        observation_ar: u64,
    ) -> Result<ckbadger_store::DaoCompensationBreakdown> {
        if self
            .last_observation_block
            .is_some_and(|previous| observation_block < previous)
        {
            bail!(
                "DAO compensation timeline observation moved backwards: previous={:?}, current={}",
                self.last_observation_block,
                observation_block
            );
        }

        while self
            .deposit_events
            .get(self.next_deposit)
            .is_some_and(|(block, _)| *block <= observation_block)
        {
            let (block, outpoint) = self.deposit_events[self.next_deposit];
            if !self.active.insert(outpoint) {
                bail!(
                    "duplicate active DAO deposit event in bulk compensation timeline: outpoint={}, block={}",
                    format_outpoint(&outpoint),
                    block
                );
            }
            self.next_deposit += 1;
        }

        while self
            .request_events
            .get(self.next_request)
            .is_some_and(|(block, _)| *block <= observation_block)
        {
            let (block, outpoint) = self.request_events[self.next_request];
            if !self.active.remove(&outpoint) {
                bail!(
                    "DAO request event has no active deposit in bulk compensation timeline: outpoint={}, block={}",
                    format_outpoint(&outpoint),
                    block
                );
            }
            let compensation = self.frozen_compensation(outpoint)?;
            if self.frozen.insert(outpoint, compensation).is_some() {
                bail!(
                    "duplicate frozen DAO request in bulk compensation timeline: outpoint={}, block={}",
                    format_outpoint(&outpoint),
                    block
                );
            }
            self.frozen_total = self
                .frozen_total
                .checked_add(compensation)
                .ok_or_else(|| anyhow!("frozen DAO compensation overflow at block {}", block))?;
            self.next_request += 1;
        }

        while self
            .withdrawal_events
            .get(self.next_withdrawal)
            .is_some_and(|(block, _)| *block <= observation_block)
        {
            let (block, outpoint) = self.withdrawal_events[self.next_withdrawal];
            let compensation = self.frozen.remove(&outpoint).ok_or_else(|| {
                anyhow!(
                    "DAO completion event has no frozen request in bulk compensation timeline: outpoint={}, block={}",
                    format_outpoint(&outpoint),
                    block
                )
            })?;
            self.frozen_total = self
                .frozen_total
                .checked_sub(compensation)
                .ok_or_else(|| anyhow!("frozen DAO compensation underflow at block {}", block))?;
            if self.frozen_total < 0 {
                bail!(
                    "negative frozen DAO compensation after completion: outpoint={}, block={}, frozen_total={}",
                    format_outpoint(&outpoint),
                    block,
                    self.frozen_total
                );
            }
            self.claimed = self
                .claimed
                .checked_add(compensation)
                .ok_or_else(|| anyhow!("claimed DAO compensation overflow at block {}", block))?;
            self.next_withdrawal += 1;
        }

        let mut active_unmade = 0i128;
        for outpoint in &self.active {
            let entry = self.entry(*outpoint)?;
            let contribution = ckbadger_store::dao_compensation_for_entry_at(
                entry,
                observation_block,
                observation_ar,
            )?;
            if contribution.claimed != 0 || contribution.unclaimed != contribution.active_unmade {
                bail!(
                    "active DAO entry produced non-active compensation in bulk timeline: outpoint={}, block={}, contribution={:?}",
                    format_outpoint(outpoint),
                    observation_block,
                    contribution
                );
            }
            active_unmade = active_unmade
                .checked_add(contribution.active_unmade)
                .ok_or_else(|| {
                    anyhow!(
                        "active DAO compensation overflow at observation block {}",
                        observation_block
                    )
                })?;
        }

        self.last_observation_block = Some(observation_block);
        Ok(ckbadger_store::DaoCompensationBreakdown {
            claimed: self.claimed,
            unclaimed: self
                .frozen_total
                .checked_add(active_unmade)
                .ok_or_else(|| {
                    anyhow!(
                        "unclaimed DAO compensation overflow at observation block {}",
                        observation_block
                    )
                })?,
            active_unmade,
        })
    }
}

impl BulkReducer for DaoOwner {
    fn apply_tx(&mut self, tx: &ResolvedTxFacts<'_>, ctx: &ReducerContext<'_>) -> Result<()> {
        let tx_date = ckbadger_common::block_date_from_ms(tx.timestamp_ms);

        let dao_outputs = tx
            .cells
            .iter()
            .map(|cell| DaoCellView::from_output(cell, ctx, tx))
            .collect::<Result<Vec<_>>>()?;
        let request_outputs = dao_outputs
            .iter()
            .enumerate()
            .filter_map(|(pos, output)| match output {
                Some(output) if matches!(output.state, DaoCellState::WithdrawRequest { .. }) => {
                    Some((pos, output))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let candidate_withdraw_to_outputs = tx
            .cells
            .iter()
            .filter(|cell| !matches!(cell.semantic_tag, CellSemanticTag::Dao))
            .map(|cell| {
                Ok((
                    checked_outpoint_index_i16(cell.outpoint, tx, "withdraw-to output index")?,
                    ctx.resolve_identity(cell.lock_script_hash_id).to_vec(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut consumed_request_output_positions = HashSet::new();

        for input in &tx.resolved_inputs {
            let Some(input_view) = DaoCellView::from_input(input, ctx, tx)? else {
                continue;
            };

            let origin_outpoint = if self.deposits.contains_key(&input_view.outpoint) {
                if matches!(
                    self.deposits
                        .get(&input_view.outpoint)
                        .map(|entry| entry.status),
                    Some(1 | 2)
                ) {
                    bail!(
                        "DAO status/input mismatch: status={} but consumed original deposit outpoint directly: block={}, tx=0x{}, tx_index={}, outpoint={}",
                        self.deposits
                            .get(&input_view.outpoint)
                            .map(|entry| entry.status)
                            .unwrap_or_default(),
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index,
                        format_outpoint(&input_view.outpoint)
                    );
                }
                input_view.outpoint
            } else {
                self.request_outpoints
                    .get(&input_view.outpoint)
                    .copied()
                    .ok_or_else(|| {
                        anyhow!(
                            "DAO input missing tracked deposit/request mapping: block={}, tx=0x{}, tx_index={}, outpoint={}",
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index,
                            format_outpoint(&input_view.outpoint)
                        )
                    })?
            };

            let entry = self.deposits.get_mut(&origin_outpoint).ok_or_else(|| {
                anyhow!(
                    "DAO tracked deposit missing while consuming input: block={}, tx=0x{}, tx_index={}, origin_outpoint={}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    format_outpoint(&origin_outpoint)
                )
            })?;

            match entry.status {
                0 => {
                    if !matches!(input_view.state, DaoCellState::Deposit) {
                        bail!(
                            "DAO deposit consumed with non-deposit state: block={}, tx=0x{}, tx_index={}, outpoint={}",
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index,
                            format_outpoint(&input_view.outpoint)
                        );
                    }

                    let (request_output_pos, request_output) = select_phase1_output_for_deposit(
                        &request_outputs,
                        &consumed_request_output_positions,
                        entry.capacity,
                        entry.deposit_block_number,
                        &entry.lock_script_hash,
                    )?
                    .ok_or_else(|| {
                        anyhow!(
                            "DAO phase-1 output not found in bulk reducer: block={}, tx=0x{}, tx_index={}, deposit_outpoint={}, capacity={}, deposit_block={}, lock_hash=0x{}",
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index,
                            format_outpoint(&origin_outpoint),
                            entry.capacity,
                            entry.deposit_block_number,
                            hex::encode(&entry.lock_script_hash),
                        )
                    })?;
                    consumed_request_output_positions.insert(request_output_pos);

                    entry.status = 1;
                    entry.withdraw_request_block = Some(tx.block_number);
                    entry.withdraw_request_tx = Some(tx.tx_hash.to_vec());
                    entry.withdraw_request_ar = Some(i64::try_from(tx.block_dao_ar).map_err(|_| {
                        anyhow!(
                            "DAO withdraw request AR exceeds i64 range in bulk reducer: block={}, tx=0x{}, tx_index={}, ar={}",
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index,
                            tx.block_dao_ar
                        )
                    })?);
                    entry.withdraw_request_output_index = Some(checked_outpoint_index_i16(
                        request_output.outpoint,
                        tx,
                        "DAO withdraw request output index",
                    )?);
                    // RFC-0023 computes compensation from the WITHDRAWING
                    // cell, so persist this request cell's exact occupied
                    // capacity — it differs from the deposit cell's whenever
                    // the withdraw request changes lock script.
                    entry.withdraw_request_occupied_capacity =
                        Some(request_output.occupied_capacity);
                    if let Some(existing) = self
                        .request_outpoints
                        .insert(request_output.outpoint, origin_outpoint)
                    {
                        bail!(
                            "duplicate DAO withdraw-request mapping: block={}, tx=0x{}, tx_index={}, request_outpoint={}, existing_origin={}, new_origin={}",
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index,
                            format_outpoint(&request_output.outpoint),
                            format_outpoint(&existing),
                            format_outpoint(&origin_outpoint)
                        );
                    }
                    // Phase-1: deposit leaves active status at withdraw
                    // request, matching CKB explorer convention.
                    Self::bump_daily_i128(
                        &mut self.daily_active_delta,
                        tx_date,
                        -(entry.capacity as i128),
                        "dao daily active delta (phase-1 withdraw request)",
                    )?;
                    // Protocol delta NOT subtracted at phase-1 — cell still
                    // locked in DAO contract until phase-2 completion.
                    Self::bump_active_depositor_count(
                        &mut self.active_deposit_counts_by_lock,
                        &mut self.daily_unique_depositors_delta,
                        tx_date,
                        &entry.lock_script_hash,
                        -1,
                        "dao depositor count (phase-1 withdraw request)",
                    )?;
                }
                1 => {
                    if !matches!(input_view.state, DaoCellState::WithdrawRequest { .. }) {
                        bail!(
                            "DAO withdraw request consumed with non-request state: block={}, tx=0x{}, tx_index={}, outpoint={}",
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index,
                            format_outpoint(&input_view.outpoint)
                        );
                    }

                    let request_block = entry.withdraw_request_block.ok_or_else(|| {
                        anyhow!(
                            "withdraw_request_block missing for DAO status=1 entry: block={}, tx=0x{}, tx_index={}, origin_outpoint={}",
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index,
                            format_outpoint(&origin_outpoint)
                        )
                    })?;
                    let request_tx_hash = entry.withdraw_request_tx.as_ref().ok_or_else(|| {
                        anyhow!(
                            "withdraw_request_tx missing for DAO status=1 entry: block={}, tx=0x{}, tx_index={}, origin_outpoint={}",
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index,
                            format_outpoint(&origin_outpoint)
                        )
                    })?;
                    let request_output_index = if let Some(output_index) =
                        entry.withdraw_request_output_index
                    {
                        output_index
                    } else {
                        infer_request_output_index_from_inputs(&tx.resolved_inputs, request_tx_hash)
                            .ok_or_else(|| {
                                anyhow!(
                                    "DAO withdraw request output index missing/ambiguous in bulk reducer: block={}, tx=0x{}, tx_index={}, origin_outpoint={}, request_tx=0x{}",
                                    tx.block_number,
                                    hex::encode(tx.tx_hash),
                                    tx.tx_index,
                                    format_outpoint(&origin_outpoint),
                                    hex::encode(request_tx_hash)
                                )
                            })?
                    };

                    let deposit_ar = u64::try_from(entry.deposit_ar).map_err(|_| {
                        anyhow!(
                            "negative DAO deposit AR in bulk reducer: deposit_block={}, block={}, tx=0x{}, tx_index={}, origin_outpoint={}, deposit_ar={}",
                            entry.deposit_block_number,
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index,
                            format_outpoint(&origin_outpoint),
                            entry.deposit_ar
                        )
                    })?;
                    let request_ar =
                        entry.withdraw_request_ar.ok_or_else(|| {
                            anyhow!(
                                "withdraw_request_ar missing for DAO status=1 entry: request_block={}, block={}, tx=0x{}, tx_index={}, origin_outpoint={}",
                                request_block,
                                tx.block_number,
                                hex::encode(tx.tx_hash),
                                tx.tx_index,
                                format_outpoint(&origin_outpoint)
                            )
                        })
                        .and_then(|ar| {
                            u64::try_from(ar).map_err(|_| {
                                anyhow!(
                                    "negative DAO withdraw request AR in bulk reducer: request_block={}, block={}, tx=0x{}, tx_index={}, origin_outpoint={}, withdraw_request_ar={}",
                                    request_block,
                                    tx.block_number,
                                    hex::encode(tx.tx_hash),
                                    tx.tx_index,
                                    format_outpoint(&origin_outpoint),
                                    ar
                                )
                            })
                        })?;
                    // RFC-0023: `counted_capacity` comes from the WITHDRAWING
                    // (phase-1 request) cell, not the original deposit cell.
                    let request_occupied_capacity = entry
                        .withdraw_request_occupied_capacity
                        .ok_or_else(|| {
                            anyhow!(
                                "withdraw_request_occupied_capacity missing for DAO status=1 entry: request_block={}, block={}, tx=0x{}, tx_index={}, origin_outpoint={}",
                                request_block,
                                tx.block_number,
                                hex::encode(tx.tx_hash),
                                tx.tx_index,
                                format_outpoint(&origin_outpoint)
                            )
                        })?;
                    let compensation = calculate_dao_compensation_from_ar(
                        entry.capacity,
                        request_occupied_capacity,
                        deposit_ar,
                        request_ar,
                    )?;
                    let withdraw_to_output_index = infer_withdraw_to_output_index_from_outputs(
                        &candidate_withdraw_to_outputs,
                        &entry.lock_script_hash,
                    );

                    entry.status = 2;
                    entry.withdraw_block = Some(tx.block_number);
                    entry.withdraw_tx = Some(tx.tx_hash.to_vec());
                    entry.withdraw_request_output_index = Some(request_output_index);
                    entry.withdraw_to_output_index = withdraw_to_output_index;
                    entry.compensation = Some(compensation);
                    self.request_outpoints.remove(&input_view.outpoint);
                    Self::bump_daily_i64(
                        &mut self.daily_withdrawals_delta,
                        tx_date,
                        1,
                        "dao daily withdrawals delta",
                    )?;
                    Self::bump_daily_i128(
                        &mut self.daily_protocol_delta,
                        tx_date,
                        -(entry.capacity as i128),
                        "dao daily protocol delta (phase-2 withdrawal)",
                    )?;
                    self.current_block_protocol_delta = self
                        .current_block_protocol_delta
                        .checked_sub(entry.capacity as i128)
                        .ok_or_else(|| {
                            anyhow!(
                                "DAO block protocol capacity delta underflow: block={}, tx=0x{}",
                                tx.block_number,
                                hex::encode(tx.tx_hash)
                            )
                        })?;
                    // Active delta and depositor count already subtracted at
                    // phase-1 withdraw request — no double-counting at
                    // phase-2 completion.  Only the compensations above,
                    // withdrawal count, and ever-deposited tracking below
                    // are handled here.
                    Self::bump_active_depositor_count(
                        &mut self.active_deposit_counts_by_lock,
                        &mut self.daily_unique_depositors_delta,
                        tx_date,
                        &entry.lock_script_hash,
                        0,
                        "dao depositor count (phase-2 no-op)",
                    )?;
                }
                status => {
                    bail!(
                        "unsupported DAO status while consuming input in bulk reducer: status={}, block={}, tx=0x{}, tx_index={}, origin_outpoint={}",
                        status,
                        tx.block_number,
                        hex::encode(tx.tx_hash),
                        tx.tx_index,
                        format_outpoint(&origin_outpoint)
                    );
                }
            }
        }

        for output in dao_outputs.into_iter().flatten() {
            if !matches!(output.state, DaoCellState::Deposit) {
                continue;
            }

            let existing = self.deposits.insert(
                output.outpoint,
                DaoDepositCacheEntry {
                    capacity: output.capacity,
                    occupied_capacity: output.occupied_capacity,
                    deposit_block_number: tx.block_number,
                    deposit_timestamp: tx.timestamp_ms,
                    lock_script_hash: output.lock_hash.clone(),
                    deposit_ar: i64::try_from(tx.block_dao_ar).map_err(|_| {
                        anyhow!(
                            "DAO deposit AR exceeds i64 range in bulk reducer: block={}, tx=0x{}, tx_index={}, ar={}",
                            tx.block_number,
                            hex::encode(tx.tx_hash),
                            tx.tx_index,
                            tx.block_dao_ar
                        )
                    })?,
                    status: 0,
                    withdraw_request_tx: None,
                    withdraw_request_output_index: None,
                    withdraw_request_block: None,
                    withdraw_request_ar: None,
                    withdraw_block: None,
                    withdraw_tx: None,
                    withdraw_request_occupied_capacity: None,
                    withdraw_to_output_index: None,
                    compensation: None,
                },
            );
            if existing.is_some() {
                bail!(
                    "duplicate DAO deposit outpoint in bulk reducer: block={}, tx=0x{}, tx_index={}, outpoint={}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    format_outpoint(&output.outpoint)
                );
            }

            Self::bump_daily_i128(
                &mut self.daily_active_delta,
                tx_date,
                output.capacity as i128,
                "dao daily active delta",
            )?;
            Self::bump_daily_i128(
                &mut self.daily_protocol_delta,
                tx_date,
                output.capacity as i128,
                "dao daily protocol delta",
            )?;
            self.current_block_protocol_delta = self
                .current_block_protocol_delta
                .checked_add(output.capacity as i128)
                .ok_or_else(|| {
                    anyhow!(
                        "DAO block protocol capacity delta overflow: block={}, tx=0x{}",
                        tx.block_number,
                        hex::encode(tx.tx_hash)
                    )
                })?;
            Self::bump_daily_i128(
                &mut self.daily_gross_deposit_delta,
                tx_date,
                output.capacity as i128,
                "dao daily gross deposit delta",
            )?;
            Self::bump_daily_i64(
                &mut self.daily_new_deposits_delta,
                tx_date,
                1,
                "dao daily new deposits delta",
            )?;
            Self::bump_active_depositor_count(
                &mut self.active_deposit_counts_by_lock,
                &mut self.daily_unique_depositors_delta,
                tx_date,
                &output.lock_hash,
                1,
                "dao unique active depositor count",
            )?;
            // Track all-time cumulative depositors (only increments, never decrements).
            if self.ever_deposited.insert(output.lock_hash.clone()) {
                *self
                    .daily_cumulative_depositors_delta
                    .entry(tx_date)
                    .or_default() += 1;
            }
            // Track per-day unique depositing addresses (including repeat depositors).
            self.daily_depositing_addresses
                .entry(tx_date)
                .or_default()
                .insert(output.lock_hash.clone());
        }

        Ok(())
    }

    fn flush_sealed(&mut self, materializer: &mut Materializer<'_>) -> Result<()> {
        let rows = self.build_sealed_rows()?;
        if rows.is_empty() {
            return Ok(());
        }
        materializer.stream_sealed_aggregate_rows(&rows)
    }

    fn materialize_final(&self, materializer: &mut Materializer<'_>) -> Result<()> {
        materializer.materialize_final_snapshot_bounded(|sink| {
            self.emit_snapshot_rows(|row| sink.push(row))
        })
    }
}

impl DaoOwner {
    pub(crate) fn estimated_bytes(&self) -> u64 {
        let deposits_bytes =
            crate::sync::bulk_build::accounting::hash_map_serialized_bytes(&self.deposits);
        let request_bytes =
            crate::sync::bulk_build::accounting::hash_map_serialized_bytes(&self.request_outpoints);
        let date_set_bytes = std::mem::size_of::<BTreeSet<NaiveDate>>() as u64
            + self.snapshot_dates.len() as u64 * std::mem::size_of::<NaiveDate>() as u64;
        let fixed_map_bytes = self.daily_dao_fields.len() as u64
            * std::mem::size_of::<(NaiveDate, (i128, i128, i128))>() as u64
            + self.daily_active_delta.len() as u64
                * std::mem::size_of::<(NaiveDate, i128)>() as u64
            + self.daily_protocol_delta.len() as u64
                * std::mem::size_of::<(NaiveDate, i128)>() as u64
            + self.daily_gross_deposit_delta.len() as u64
                * std::mem::size_of::<(NaiveDate, i128)>() as u64
            + self.daily_new_deposits_delta.len() as u64
                * std::mem::size_of::<(NaiveDate, i64)>() as u64
            + self.daily_withdrawals_delta.len() as u64
                * std::mem::size_of::<(NaiveDate, i64)>() as u64
            + self.daily_unique_depositors_delta.len() as u64
                * std::mem::size_of::<(NaiveDate, i64)>() as u64
            + self.daily_secondary_miner_delta.len() as u64
                * std::mem::size_of::<(NaiveDate, i128)>() as u64
            + self.active_deposit_counts_by_lock.len() as u64
                * std::mem::size_of::<(Vec<u8>, i64)>() as u64
            + self.daily_cumulative_depositors_delta.len() as u64
                * std::mem::size_of::<(NaiveDate, i64)>() as u64
            + self.ever_deposited.len() as u64
                * (std::mem::size_of::<Vec<u8>>() as u64 + 32) // 32-byte lock hashes on heap
            + self.daily_depositing_addresses.len() as u64
                * std::mem::size_of::<(NaiveDate, HashSet<Vec<u8>>)>() as u64
            + self.daily_depositing_addresses.values().map(|s|
                s.len() as u64 * (std::mem::size_of::<Vec<u8>>() as u64 + 32)
              ).sum::<u64>()
            + self.daily_end_of_day.len() as u64
                * std::mem::size_of::<(NaiveDate, (i64, u64))>() as u64;
        std::mem::size_of::<Self>() as u64
            + deposits_bytes
            + request_bytes
            + date_set_bytes
            + fixed_map_bytes
    }

    fn build_sealed_rows(&self) -> Result<Vec<MaterializedRow>> {
        let mut rows = Vec::new();
        let mut running_total_deposited = 0i128;
        let mut running_protocol_deposited = 0i128;
        let mut running_total_deposit_count = 0i64;
        let mut running_total_withdrawal_count = 0i64;
        let mut running_cumulative_depositors = 0i64;
        let mut running_total_depositors = 0i64;
        let mut running_cumulative_deposit_amount = 0i128;
        let mut running_cum_miner = 0i128;
        let mut compensation_timeline = DaoCompensationTimeline::new(&self.deposits)?;

        for date in &self.snapshot_dates {
            running_total_deposited = checked_next_i128_total(
                running_total_deposited,
                self.daily_active_delta.get(date).copied().unwrap_or(0),
                "dao running total_deposited",
                *date,
            )?;
            running_protocol_deposited = checked_next_i128_total(
                running_protocol_deposited,
                self.daily_protocol_delta.get(date).copied().unwrap_or(0),
                "dao running protocol_deposited",
                *date,
            )?;
            running_cumulative_deposit_amount = checked_next_i128_total(
                running_cumulative_deposit_amount,
                self.daily_gross_deposit_delta
                    .get(date)
                    .copied()
                    .unwrap_or(0),
                "dao running cumulative_deposit_amount",
                *date,
            )?;
            running_total_deposit_count = checked_next_i64_total(
                running_total_deposit_count,
                self.daily_new_deposits_delta
                    .get(date)
                    .copied()
                    .unwrap_or(0),
                "dao running total_deposit_count",
                *date,
            )?;
            running_total_withdrawal_count = checked_next_i64_total(
                running_total_withdrawal_count,
                self.daily_withdrawals_delta.get(date).copied().unwrap_or(0),
                "dao running total_withdrawal_count",
                *date,
            )?;

            let (total_issuance, secondary_pool, occupied_capacity) = self
                .daily_dao_fields
                .get(date)
                .copied()
                .ok_or_else(|| anyhow!("missing DAO field for bulk snapshot date {}", date))?;

            let daily_miner = self
                .daily_secondary_miner_delta
                .get(date)
                .copied()
                .unwrap_or(0);
            running_cum_miner = checked_next_i128_total(
                running_cum_miner,
                daily_miner,
                "dao running cum_miner_secondary",
                *date,
            )?;
            running_total_depositors = derive_running_depositors(
                running_total_depositors,
                self.daily_unique_depositors_delta
                    .get(date)
                    .copied()
                    .unwrap_or(0),
                *date,
            )?;
            running_cumulative_depositors = running_cumulative_depositors
                .checked_add(
                    self.daily_cumulative_depositors_delta
                        .get(date)
                        .copied()
                        .unwrap_or(0),
                )
                .ok_or_else(|| anyhow!("dao cumulative_depositors overflow on {}", date))?;

            let &(last_block, ar) = self.daily_end_of_day.get(date).ok_or_else(|| {
                anyhow!(
                    "missing end-of-day DAO block/AR for bulk snapshot date {}",
                    date
                )
            })?;
            let compensation = compensation_timeline.advance_to(last_block, ar)?;
            let total_compensation = compensation.total().ok_or_else(|| {
                anyhow!("DAO total compensation overflow on snapshot date {}", date)
            })?;
            let cumulative_treasury = secondary_pool
                .checked_sub(compensation.active_unmade)
                .ok_or_else(|| anyhow!("DAO treasury subtraction overflow on {}", date))?;
            if cumulative_treasury < 0 {
                bail!(
                    "active DAO interests exceed secondary pool on {}: secondary_pool={}, active_unmade={}",
                    date,
                    secondary_pool,
                    compensation.active_unmade
                );
            }

            let snapshot = DaoDailySnapshot {
                date: date.format("%Y-%m-%d").to_string(),
                total_deposited: running_total_deposited,
                depositors_count: running_total_depositors,
                new_deposits: running_total_deposit_count,
                withdrawals: running_total_withdrawal_count,
                compensation: compensation.claimed,
                cumulative_deposit_amount: running_cumulative_deposit_amount,
                total_issuance,
                secondary_pool,
                occupied_capacity,
                cum_miner_secondary: running_cum_miner,
                cum_dao_compensation: total_compensation,
                cum_treasury: cumulative_treasury,
                unmade_dao_interests: compensation.active_unmade,
                unclaimed_compensation: compensation.unclaimed,
                cumulative_depositors: running_cumulative_depositors,
                daily_depositor_addresses: self
                    .daily_depositing_addresses
                    .get(date)
                    .map(|s| s.len() as i64)
                    .unwrap_or(0),
                protocol_deposited: Some(running_protocol_deposited),
            };
            let key = keys::encode_stats_key(
                keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
                date.format("%Y%m%d").to_string().as_bytes(),
            );
            rows.push(MaterializedRow::new(
                CF_STATS_DAO,
                key,
                bincode::serialize(&snapshot)?,
            ));
        }

        Ok(rows)
    }

    pub(crate) fn emit_snapshot_rows<F>(&self, mut emit: F) -> Result<()>
    where
        F: FnMut(MaterializedRow) -> Result<()>,
    {
        for (outpoint, entry) in &self.deposits {
            let outpoint_key = encode_outpoint_key(*outpoint)?;
            emit(MaterializedRow::new(
                CF_DAO_DEPOSITS,
                outpoint_key.to_vec(),
                bincode::serialize(entry)?,
            ))?;
            emit(MaterializedRow::new(
                CF_DAO_BY_BLOCK,
                keys::encode_dao_by_block_key(entry.deposit_block_number, &outpoint_key).to_vec(),
                Vec::new(),
            ))?;
            emit(MaterializedRow::new(
                CF_DAO_BY_LOCK_BLOCK,
                keys::encode_dao_by_lock_block_key(
                    &entry.lock_script_hash,
                    entry.deposit_block_number,
                    &outpoint_key,
                )
                .to_vec(),
                Vec::new(),
            ))?;
            emit(MaterializedRow::new(
                CF_DAO_BY_STATUS_BLOCK,
                keys::encode_dao_by_status_block_key(
                    entry.status,
                    entry.deposit_block_number,
                    &outpoint_key,
                )
                .to_vec(),
                Vec::new(),
            ))?;

            if entry.status >= 1 {
                let request_tx_hash = entry.withdraw_request_tx.as_ref().ok_or_else(|| {
                    anyhow!(
                        "DAO status {} missing withdraw_request_tx during materialization: outpoint={}",
                        entry.status,
                        format_outpoint(outpoint)
                    )
                })?;
                let request_output_index =
                    entry.withdraw_request_output_index.ok_or_else(|| {
                        anyhow!(
                            "DAO status {} missing withdraw_request_output_index during materialization: outpoint={}",
                            entry.status,
                            format_outpoint(outpoint)
                        )
                    })?;
                emit(MaterializedRow::new(
                    CF_DAO_BY_WITHDRAW_TX,
                    keys::encode_outpoint(request_tx_hash, request_output_index).to_vec(),
                    outpoint_key.to_vec(),
                ))?;
            }
        }

        Ok(())
    }

    #[cfg(test)]
    fn build_snapshot_rows(&self) -> Result<Vec<MaterializedRow>> {
        let mut rows = Vec::new();
        self.emit_snapshot_rows(|row| {
            rows.push(row);
            Ok(())
        })?;
        Ok(rows)
    }

    #[cfg(test)]
    pub(crate) fn build_final_rows(&self) -> Result<super::super::materialize::OwnerFinalRows> {
        Ok(super::super::materialize::OwnerFinalRows {
            sealed_rows: self.build_sealed_rows()?,
            snapshot_rows: self.build_snapshot_rows()?,
        })
    }

    /// Compute exact unmade_dao_interests at a given block by iterating all
    /// deposits and summing per-deposit compensation for those that were
    /// status-0 at `block_number`.  Same formula as the live-sync path
    /// (`compute_unmade_dao_interests` in dao_ops.rs).
    #[cfg(test)]
    fn compute_unmade_at_block(&self, block_number: i64, ar: u64) -> Result<i128> {
        Ok(self
            .compute_compensation_at_block(block_number, ar)?
            .active_unmade)
    }

    #[cfg(test)]
    fn compute_compensation_at_block(
        &self,
        block_number: i64,
        ar: u64,
    ) -> Result<ckbadger_store::DaoCompensationBreakdown> {
        DaoCompensationTimeline::new(&self.deposits)?.advance_to(block_number, ar)
    }

    pub(crate) fn record_block(&mut self, block: &BlockFacts) -> Result<()> {
        let block_date = ckbadger_common::block_date_from_ms(block.timestamp_ms);
        self.snapshot_dates.insert(block_date);

        let (c, s, u) = extract_dao_csu(&block.dao).ok_or_else(|| {
            anyhow!(
                "invalid DAO field bytes in bulk reducer block header: block={} dao_len={}",
                block.number,
                block.dao.len()
            )
        })?;
        self.daily_dao_fields.insert(block_date, (c, s, u));

        // Track end-of-day block number and AR for exact unmade_dao_interests
        // computation during materialization (HashMap overwrites → last block
        // of each day persists).
        if let Some(ar) = extract_ar_from_dao_bytes(&block.dao) {
            self.daily_end_of_day.insert(block_date, (block.number, ar));
        }

        let secondary_epoch_reward = self.secondary_epoch_reward.ok_or_else(|| {
            anyhow!(
                "missing consensus secondary_epoch_reward in bulk DAO reducer: block={}",
                block.number
            )
        })?;
        let mut stats = BatchStats::default();
        accumulate_miner_secondary_for_block(
            &mut stats,
            block.number,
            block_date,
            i64::from(block.epoch_index),
            i64::from(block.epoch_length),
            c,
            u,
            secondary_epoch_reward,
            &mut self.prev_dao_cu,
        )?;
        if let Some(delta) = stats.daily_secondary_miner_delta.get(&block_date) {
            Self::bump_daily_i128(
                &mut self.daily_secondary_miner_delta,
                block_date,
                *delta,
                "dao daily secondary miner delta",
            )?;
        }

        // Commit this block's protocol delta to the running total.
        self.running_protocol_deposited = self
            .running_protocol_deposited
            .checked_add(self.current_block_protocol_delta)
            .ok_or_else(|| {
                anyhow!(
                    "running DAO protocol capacity overflow after block {}",
                    block.number
                )
            })?;
        self.current_block_protocol_delta = 0;
        if self.running_protocol_deposited < 0 {
            bail!(
                "negative running DAO protocol capacity after block {}: {}",
                block.number,
                self.running_protocol_deposited
            );
        }

        Ok(())
    }

    fn bump_daily_i128(
        target: &mut FxHashMap<NaiveDate, i128>,
        date: NaiveDate,
        delta: i128,
        metric: &str,
    ) -> Result<()> {
        if delta == 0 {
            return Ok(());
        }
        let current = target.get(&date).copied().unwrap_or(0);
        let next = current.checked_add(delta).ok_or_else(|| {
            anyhow!(
                "{} overflow: date={} current={} delta={}",
                metric,
                date,
                current,
                delta
            )
        })?;
        target.insert(date, next);
        Ok(())
    }

    fn bump_daily_i64(
        target: &mut FxHashMap<NaiveDate, i64>,
        date: NaiveDate,
        delta: i64,
        metric: &str,
    ) -> Result<()> {
        if delta == 0 {
            return Ok(());
        }
        let current = target.get(&date).copied().unwrap_or(0);
        let next = current.checked_add(delta).ok_or_else(|| {
            anyhow!(
                "{} overflow: date={} current={} delta={}",
                metric,
                date,
                current,
                delta
            )
        })?;
        if next < 0 {
            bail!(
                "{} underflow: date={} current={} delta={} next={}",
                metric,
                date,
                current,
                delta,
                next
            );
        }
        target.insert(date, next);
        Ok(())
    }

    fn bump_active_depositor_count(
        active_deposit_counts_by_lock: &mut FxHashMap<Vec<u8>, i64>,
        daily_unique_depositors_delta: &mut FxHashMap<NaiveDate, i64>,
        date: NaiveDate,
        lock_hash: &[u8],
        delta: i64,
        metric: &str,
    ) -> Result<()> {
        if delta == 0 {
            return Ok(());
        }
        let current = active_deposit_counts_by_lock
            .get(lock_hash)
            .copied()
            .unwrap_or(0);
        let next = current.checked_add(delta).ok_or_else(|| {
            anyhow!(
                "{} overflow: date={} lock_hash=0x{} current={} delta={}",
                metric,
                date,
                hex::encode(lock_hash),
                current,
                delta
            )
        })?;
        if next < 0 {
            bail!(
                "{} underflow: date={} lock_hash=0x{} current={} delta={} next={}",
                metric,
                date,
                hex::encode(lock_hash),
                current,
                delta,
                next
            );
        }
        if current == 0 && next > 0 {
            *daily_unique_depositors_delta.entry(date).or_default() += 1;
        } else if current > 0 && next == 0 {
            *daily_unique_depositors_delta.entry(date).or_default() -= 1;
        }
        active_deposit_counts_by_lock.insert(lock_hash.to_vec(), next);
        Ok(())
    }
}

fn checked_next_i128_total(
    current: i128,
    delta: i128,
    metric: &str,
    date: NaiveDate,
) -> Result<i128> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: date={} current={} delta={}",
            metric,
            date,
            current,
            delta
        )
    })?;
    if next < 0 {
        bail!(
            "{} underflow: date={} current={} delta={} next={}",
            metric,
            date,
            current,
            delta,
            next
        );
    }
    Ok(next)
}

fn checked_next_i64_total(current: i64, delta: i64, metric: &str, date: NaiveDate) -> Result<i64> {
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "{} overflow: date={} current={} delta={}",
            metric,
            date,
            current,
            delta
        )
    })?;
    if next < 0 {
        bail!(
            "{} underflow: date={} current={} delta={} next={}",
            metric,
            date,
            current,
            delta,
            next
        );
    }
    Ok(next)
}

#[derive(Debug, Clone)]
struct DaoCellView {
    outpoint: OutPointKey,
    lock_hash: Vec<u8>,
    capacity: i64,
    occupied_capacity: i64,
    state: DaoCellState,
}

impl DaoCellView {
    fn from_output(
        cell: &CellFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<Option<Self>> {
        Self::from_parts(
            cell.outpoint,
            cell.capacity,
            cell.occupied_capacity,
            cell.lock_script_hash_id,
            cell.semantic_tag,
            cell.dao_state,
            ctx,
            tx,
            format!("output outpoint={}", format_outpoint(&cell.outpoint)),
        )
    }

    fn from_input(
        input: &ResolvedInputFacts,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
    ) -> Result<Option<Self>> {
        Self::from_parts(
            input.outpoint,
            input.capacity,
            input.occupied_capacity,
            input.lock_script_hash_id,
            input.semantic_tag,
            input.dao_state,
            ctx,
            tx,
            format!("input outpoint={}", format_outpoint(&input.outpoint)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        outpoint: OutPointKey,
        capacity: i64,
        occupied_capacity: i64,
        lock_script_hash_id: crate::sync::types::InternId,
        semantic_tag: CellSemanticTag,
        dao_state: Option<DaoCellState>,
        ctx: &ReducerContext<'_>,
        tx: &ResolvedTxFacts<'_>,
        location: String,
    ) -> Result<Option<Self>> {
        if !matches!(semantic_tag, CellSemanticTag::Dao) {
            return Ok(None);
        }

        Ok(Some(Self {
            outpoint,
            lock_hash: ctx.resolve_identity(lock_script_hash_id).to_vec(),
            capacity,
            occupied_capacity,
            state: dao_state.ok_or_else(|| {
                anyhow!(
                    "missing DAO state for DAO cell in bulk reducer: block={}, tx=0x{}, tx_index={}, {}",
                    tx.block_number,
                    hex::encode(tx.tx_hash),
                    tx.tx_index,
                    location
                )
            })?,
        }))
    }
}

fn select_phase1_output_for_deposit<'a>(
    request_outputs: &'a [(usize, &'a DaoCellView)],
    consumed_output_positions: &HashSet<usize>,
    capacity: i64,
    deposit_block_number: i64,
    lock_script_hash: &[u8],
) -> Result<Option<(usize, &'a DaoCellView)>> {
    let deposit_block_u64 = u64::try_from(deposit_block_number).map_err(|_| {
        anyhow!(
            "invalid negative DAO deposit block number while matching phase-1 output in bulk reducer: {}",
            deposit_block_number
        )
    })?;

    let base_candidates = || {
        request_outputs
            .iter()
            .filter_map(move |(pos, output)| match output.state {
                DaoCellState::WithdrawRequest {
                    deposit_block_number: output_deposit_block,
                } => (output.capacity == capacity
                    && u64::try_from(output_deposit_block).ok() == Some(deposit_block_u64)
                    && !consumed_output_positions.contains(pos))
                .then_some((*pos, *output)),
                DaoCellState::Deposit => None,
            })
    };

    // Prefer lock_script_hash match for disambiguation (multiple deposits with
    // same capacity/deposit_block but different locks).  Fall back to
    // (capacity, deposit_block) only — the CKB DAO type script does not
    // enforce lock preservation, so a legitimate withdraw request may change
    // the lock script.
    //
    // Known limitation: when multiple request outputs share the same
    // (capacity, deposit_block) AND locks differ from the original deposits,
    // the fallback min_by_key pairing is deterministic but arbitrary — it may
    // mis-associate deposits with request outputs.  This requires a single tx
    // to withdraw multiple identical-capacity deposits from the same block
    // while also changing locks, which is rare in practice.
    let with_lock = base_candidates()
        .filter(|(_, output)| output.lock_hash.as_slice() == lock_script_hash)
        .min_by_key(|(pos, output)| (output.outpoint.index, *pos));

    if with_lock.is_some() {
        return Ok(with_lock);
    }

    Ok(base_candidates().min_by_key(|(pos, output)| (output.outpoint.index, *pos)))
}

fn infer_request_output_index_from_inputs(
    inputs: &[ResolvedInputFacts],
    request_tx_hash: &[u8],
) -> Option<i16> {
    let mut matches = inputs
        .iter()
        .filter_map(|input| {
            (input.outpoint.tx_hash.as_slice() == request_tx_hash)
                .then(|| i16::try_from(input.outpoint.index).ok())
                .flatten()
        })
        .take(2);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

fn infer_withdraw_to_output_index_from_outputs(
    candidate_outputs: &[(i16, Vec<u8>)],
    lock_script_hash: &[u8],
) -> Option<i16> {
    if candidate_outputs.is_empty() {
        return None;
    }

    let mut same_lock = candidate_outputs
        .iter()
        .filter_map(|(output_index, output_lock_hash)| {
            (output_lock_hash.as_slice() == lock_script_hash).then_some(*output_index)
        });
    if let Some(first) = same_lock.next() {
        if same_lock.next().is_none() {
            return Some(first);
        }
        return None;
    }

    if candidate_outputs.len() == 1 {
        return Some(candidate_outputs[0].0);
    }

    None
}

fn extract_ar_from_dao_bytes(dao: &[u8]) -> Option<u64> {
    if dao.len() < 16 {
        return None;
    }
    let bytes: [u8; 8] = dao[8..16].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn checked_outpoint_index_i16(
    outpoint: OutPointKey,
    tx: &ResolvedTxFacts<'_>,
    context: &str,
) -> Result<i16> {
    i16::try_from(outpoint.index).map_err(|_| {
        anyhow!(
            "{} exceeds i16 range in bulk reducer: block={}, tx=0x{}, tx_index={}, outpoint={}",
            context,
            tx.block_number,
            hex::encode(tx.tx_hash),
            tx.tx_index,
            format_outpoint(&outpoint)
        )
    })
}

fn encode_outpoint_key(outpoint: OutPointKey) -> Result<[u8; 34]> {
    Ok(keys::encode_outpoint(
        &outpoint.tx_hash,
        i16::try_from(outpoint.index).map_err(|_| {
            anyhow!(
                "DAO outpoint index exceeds i16 during materialization: outpoint={}",
                format_outpoint(&outpoint)
            )
        })?,
    ))
}

fn format_outpoint(outpoint: &OutPointKey) -> String {
    format!("0x{}:{}", hex::encode(outpoint.tx_hash), outpoint.index)
}

#[doc(hidden)]
#[derive(Debug, Default, Clone)]
pub struct DaoStateSnapshot {
    pub deposits: HashMap<Vec<u8>, DaoDepositCacheEntry>,
    pub withdraw_lookup: HashMap<Vec<u8>, HashMap<i16, Vec<u8>>>,
    pub by_status: HashMap<i16, Vec<Vec<u8>>>,
    pub by_lock: HashMap<Vec<u8>, Vec<Vec<u8>>>,
}

#[doc(hidden)]
pub(crate) fn materialize_dao_state_for_test(
    blocks: &[BlockResponseWithCycles],
) -> Result<DaoStateSnapshot> {
    let interner = IdentityInterner::default();
    let (arena, _) = build_bulk_facts_arena_from_blocks(blocks, &interner)?;
    let resolved = BulkSequencer::default().resolve(&arena)?;
    let frozen = interner.snapshot_for_reads();
    let ctx = ReducerContext::new(&frozen);
    let mut owner = DaoOwner::default();

    for tx in &resolved {
        owner.apply_tx(tx, &ctx)?;
    }

    let root = super::super::unique_temp_test_dir("bulk-build-dao-owner");
    std::fs::create_dir_all(&root)?;
    let domain_path = root.join("domain");
    let append_path = root.join("append-only");
    std::fs::create_dir_all(&domain_path)?;
    std::fs::create_dir_all(&append_path)?;

    let snapshot = {
        let domain_store = CkbadgerStore::open_domain(&domain_path)?;
        let append_store = CkbadgerStore::open_append_only(&append_path)?;
        let mut materializer = Materializer::new(&domain_store, &append_store);
        owner.materialize_final(&mut materializer)?;
        let _ = materializer.finish();

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
                            "dao_by_withdraw_tx missing in DAO snapshot helper: request_tx=0x{}, output_index={}",
                            hex::encode(request_tx),
                            request_output_index
                        )
                    })?;
                withdraw_lookup
                    .entry(request_tx.clone())
                    .or_default()
                    .insert(request_output_index, linked);
                if withdraw_lookup
                    .get(request_tx)
                    .and_then(|rows| rows.get(&request_output_index))
                    != Some(outpoint_key)
                {
                    bail!(
                        "dao_by_withdraw_tx mismatch in DAO snapshot helper: request_tx=0x{}, output_index={}",
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
                .map(|(outpoint, _)| outpoint)
                .collect::<Vec<_>>();
            by_status.insert(status, outpoints);
        }

        let mut by_lock: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
        for (outpoint_key, entry) in &deposits {
            let rows = domain_store
                .list_dao_deposits_by_lock_paginated(&entry.lock_script_hash, page_limit, None)?
                .into_iter()
                .map(|(outpoint, _)| outpoint)
                .collect::<Vec<_>>();
            by_lock.insert(entry.lock_script_hash.clone(), rows);
            if !by_lock
                .get(&entry.lock_script_hash)
                .is_some_and(|rows| rows.iter().any(|row| row == outpoint_key))
            {
                bail!(
                    "dao_by_lock_block missing outpoint in DAO snapshot helper: outpoint=0x{}",
                    hex::encode(outpoint_key)
                );
            }
        }

        DaoStateSnapshot {
            deposits,
            withdraw_lookup,
            by_status,
            by_lock,
        }
    };

    let _ = std::fs::remove_dir_all(&root);
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::dao::DAO_CODE_HASH;
    use crate::sync::types::InternId;

    /// Regression: previously the running-sum formula crashed at block 64 with
    /// "negative unmade_dao_interests -1" due to double integer truncation.
    /// The new approach computes exact per-deposit compensation, matching the
    /// live-sync path.
    #[test]
    fn compute_unmade_exact_near_genesis() {
        let mut owner = DaoOwner::default();

        // Simulate a deposit at genesis: capacity=193 CKB, ar_deposit=10^16.
        let ar_genesis: u64 = 10_000_000_000_000_000;
        let deposit = DaoDepositCacheEntry {
            capacity: 193_00000000,
            occupied_capacity: 102_00000000,
            deposit_block_number: 0,
            deposit_timestamp: 0,
            lock_script_hash: vec![0xaa; 32],
            deposit_ar: ar_genesis as i64,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_request_occupied_capacity: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        owner.deposits.insert(
            OutPointKey {
                tx_hash: [0x01; 32],
                index: 0,
            },
            deposit,
        );

        // Block 64 AR from actual mainnet data.
        let ar_block64: u64 = 10_000_006_706_531_899;

        // Exact per-deposit computation should succeed (no negative).
        let unmade = owner.compute_unmade_at_block(64, ar_block64).unwrap();

        // Interest = free_cap * ar / ar_deposit - free_cap
        // free_cap = (193 - 102) * 10^8 = 91 * 10^8 = 9_100_000_000
        // = 9_100_000_000 * 10_000_006_706_531_899 / 10_000_000_000_000_000 - 9_100_000_000
        // = 9_100_006_102 - 9_100_000_000 = 6102
        assert_eq!(
            unmade, 6102,
            "exact per-deposit compensation for genesis deposit at block 64"
        );
    }

    /// Verify that deposits not yet created or already withdrawn are excluded.
    #[test]
    fn compute_unmade_filters_by_block_number() {
        let mut owner = DaoOwner::default();

        // Deposit at block 100, withdrawn at block 200.
        let deposit_withdrawn = DaoDepositCacheEntry {
            capacity: 200_00000000,
            occupied_capacity: 102_00000000,
            deposit_block_number: 100,
            deposit_timestamp: 0,
            lock_script_hash: vec![0xbb; 32],
            deposit_ar: 10_000_000_000_000_000,
            status: 1,
            withdraw_request_tx: Some(vec![0x99; 32]),
            withdraw_request_output_index: Some(0),
            withdraw_request_block: Some(200),
            withdraw_request_ar: Some(10_000_100_000_000_000),
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_request_occupied_capacity: Some(102_00000000),
            withdraw_to_output_index: None,
            compensation: None,
        };
        owner.deposits.insert(
            OutPointKey {
                tx_hash: [0x02; 32],
                index: 0,
            },
            deposit_withdrawn,
        );

        // Deposit at block 300 (future).
        let deposit_future = DaoDepositCacheEntry {
            capacity: 300_00000000,
            occupied_capacity: 102_00000000,
            deposit_block_number: 300,
            deposit_timestamp: 0,
            lock_script_hash: vec![0xcc; 32],
            deposit_ar: 10_000_100_000_000_000,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_request_occupied_capacity: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        owner.deposits.insert(
            OutPointKey {
                tx_hash: [0x03; 32],
                index: 0,
            },
            deposit_future,
        );

        let ar = 10_000_050_000_000_000_u64;

        // At block 150: deposit_withdrawn is active (created 100, not yet withdrawn 200).
        // deposit_future doesn't exist yet (created 300).
        let unmade_150 = owner.compute_unmade_at_block(150, ar).unwrap();
        assert!(
            unmade_150 > 0,
            "withdrawn deposit should be active at block 150"
        );

        // At block 250: deposit_withdrawn is no longer active (withdrawn at 200).
        // deposit_future doesn't exist yet.
        let unmade_250 = owner.compute_unmade_at_block(250, ar).unwrap();
        assert_eq!(unmade_250, 0, "no active deposits at block 250");

        // At block 350: only deposit_future is active.
        // AR must be higher than deposit_future's ar_deposit for interest.
        let ar_later = 10_000_200_000_000_000_u64;
        let unmade_350 = owner.compute_unmade_at_block(350, ar_later).unwrap();
        assert!(
            unmade_350 > 0,
            "future deposit should be active at block 350"
        );
    }

    #[test]
    fn dao_owner_reduces_deposit_request_completion_lifecycle() {
        let interner = IdentityInterner::default();
        let lock_hash = interner.intern_bytes(vec![0xaa; 32]);
        let dao_code_hash_id =
            interner.intern_bytes(hex::decode(&DAO_CODE_HASH[2..]).expect("dao code hash"));
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let mut owner = DaoOwner::default();

        let tx0 = ResolvedTxFacts {
            tx_hash: [0x31; 32],
            block_number: 100,
            block_hash: [0x04; 32],
            timestamp_ms: 1_700_000_000_000,
            block_dao_ar: 10_000,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x31; 32], 0),
                created_at_block: 100,
                created_by_block_dao_ar: 10_000,
                capacity: 200_00000000,
                lock_script_hash_id: lock_hash,
                lock_code_hash_id: InternId::new(1),
                lock_hash_type: 1,
                lock_args_id: InternId::new(2),
                type_script_hash_id: Some(InternId::new(3)),
                type_code_hash_id: Some(dao_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(4)),
                occupied_capacity: 142_00000000,
                data_size: 8,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Dao,
                dao_state: Some(DaoCellState::Deposit),
                protocol_facts: None,
            }]
            .into(),
        };
        owner.apply_tx(&tx0, &ctx).expect("apply deposit");
        assert_eq!(owner.current_block_protocol_delta, 200_00000000);
        assert_eq!(
            owner
                .deposits
                .get(&OutPointKey::new([0x31; 32], 0))
                .unwrap()
                .occupied_capacity,
            142_00000000
        );

        let tx1 = ResolvedTxFacts {
            tx_hash: [0x32; 32],
            block_number: 101,
            block_hash: [0x04; 32],
            timestamp_ms: 1_700_000_000_001,
            block_dao_ar: 12_000,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x31; 32], 0),
                created_at_block: 100,
                created_by_block_dao_ar: 10_000,
                capacity: 200_00000000,
                occupied_capacity: 142_00000000,
                data_size: 0,
                data_hash: None,
                udt_amount: None,
                lock_script_hash_id: lock_hash,
                lock_code_hash_id: InternId::new(1),
                lock_hash_type: 1,
                lock_args_id: InternId::new(2),
                type_script_hash_id: Some(InternId::new(3)),
                type_code_hash_id: Some(dao_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(4)),
                semantic_tag: CellSemanticTag::Dao,
                dao_state: Some(DaoCellState::Deposit),
                dao_compensation_ars: None,
                protocol_facts: None,
            }],
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x32; 32], 0),
                created_at_block: 101,
                created_by_block_dao_ar: 12_000,
                capacity: 200_00000000,
                lock_script_hash_id: lock_hash,
                lock_code_hash_id: InternId::new(1),
                lock_hash_type: 1,
                lock_args_id: InternId::new(2),
                type_script_hash_id: Some(InternId::new(3)),
                type_code_hash_id: Some(dao_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(4)),
                occupied_capacity: 142_00000000,
                data_size: 8,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Dao,
                dao_state: Some(DaoCellState::WithdrawRequest {
                    deposit_block_number: 100,
                }),
                protocol_facts: None,
            }]
            .into(),
        };
        owner.apply_tx(&tx1, &ctx).expect("apply request");
        assert_eq!(
            owner
                .compute_compensation_at_block(101, 12_000)
                .unwrap()
                .unclaimed,
            11_60000000,
            "phase-1 compensation must freeze at request AR using the deposit cell's exact occupied capacity"
        );

        let tx2 = ResolvedTxFacts {
            tx_hash: [0x33; 32],
            block_number: 102,
            block_hash: [0x04; 32],
            timestamp_ms: 1_700_000_000_002,
            block_dao_ar: 13_000,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x32; 32], 0),
                created_at_block: 101,
                created_by_block_dao_ar: 12_000,
                capacity: 200_00000000,
                occupied_capacity: 142_00000000,
                data_size: 0,
                data_hash: None,
                udt_amount: None,
                lock_script_hash_id: lock_hash,
                lock_code_hash_id: InternId::new(1),
                lock_hash_type: 1,
                lock_args_id: InternId::new(2),
                type_script_hash_id: Some(InternId::new(3)),
                type_code_hash_id: Some(dao_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(4)),
                semantic_tag: CellSemanticTag::Dao,
                dao_state: Some(DaoCellState::WithdrawRequest {
                    deposit_block_number: 100,
                }),
                dao_compensation_ars: None,
                protocol_facts: None,
            }],
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x33; 32], 0),
                created_at_block: 102,
                created_by_block_dao_ar: 13_000,
                capacity: 211_60000000,
                lock_script_hash_id: lock_hash,
                lock_code_hash_id: InternId::new(5),
                lock_hash_type: 1,
                lock_args_id: InternId::new(6),
                type_script_hash_id: None,
                type_code_hash_id: None,
                type_hash_type: None,
                type_args_id: None,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            }]
            .into(),
        };
        owner.apply_tx(&tx2, &ctx).expect("apply completion");

        let entry = owner
            .deposits
            .get(&OutPointKey::new([0x31; 32], 0))
            .expect("dao entry");
        assert_eq!(entry.status, 2);
        assert_eq!(entry.withdraw_request_block, Some(101));
        assert_eq!(entry.withdraw_request_tx, Some(vec![0x32; 32]));
        assert_eq!(
            entry.withdraw_request_ar,
            Some(12_000),
            "completed entries must retain the request AR so a rollback to phase 1 remains computable"
        );
        assert_eq!(entry.withdraw_request_output_index, Some(0));
        assert_eq!(entry.withdraw_block, Some(102));
        assert_eq!(entry.withdraw_tx, Some(vec![0x33; 32]));
        assert_eq!(entry.withdraw_to_output_index, Some(0));
        assert_eq!(entry.compensation, Some(11_60000000));
        assert!(owner.request_outpoints.is_empty());
        assert_eq!(
            owner.compute_compensation_at_block(101, 12_000).unwrap(),
            ckbadger_store::DaoCompensationBreakdown {
                claimed: 0,
                unclaimed: 11_60000000,
                active_unmade: 0,
            },
            "a finalized entry must reconstruct its historical phase-1 frozen compensation"
        );
        assert_eq!(
            owner.compute_compensation_at_block(102, 13_000).unwrap(),
            ckbadger_store::DaoCompensationBreakdown {
                claimed: 11_60000000,
                unclaimed: 0,
                active_unmade: 0,
            }
        );
    }

    #[test]
    fn dao_owner_phase1_matches_when_lock_script_changes() {
        // Regression: CKB DAO type script does not enforce lock preservation.
        // A withdraw request may use a different lock than the original deposit.
        let interner = IdentityInterner::default();
        let deposit_lock = interner.intern_bytes(vec![0xaa; 32]);
        let request_lock = interner.intern_bytes(vec![0xbb; 32]); // different lock
        let dao_code_hash_id =
            interner.intern_bytes(hex::decode(&DAO_CODE_HASH[2..]).expect("dao code hash"));
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let mut owner = DaoOwner::default();

        // Create deposit with deposit_lock
        let tx0 = ResolvedTxFacts {
            tx_hash: [0x31; 32],
            block_number: 5668752,
            block_hash: [0x04; 32],
            timestamp_ms: 1_700_000_000_000,
            block_dao_ar: 10_000,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: Vec::new(),
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x31; 32], 0),
                created_at_block: 5668752,
                created_by_block_dao_ar: 10_000,
                capacity: 120_00000000,
                lock_script_hash_id: deposit_lock,
                lock_code_hash_id: InternId::new(1),
                lock_hash_type: 1,
                lock_args_id: InternId::new(2),
                type_script_hash_id: Some(InternId::new(3)),
                type_code_hash_id: Some(dao_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(4)),
                occupied_capacity: 102_00000000,
                data_size: 8,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Dao,
                dao_state: Some(DaoCellState::Deposit),
                protocol_facts: None,
            }]
            .into(),
        };
        owner.apply_tx(&tx0, &ctx).expect("apply deposit");

        // Withdraw request with request_lock (different from deposit_lock)
        let tx1 = ResolvedTxFacts {
            tx_hash: [0x32; 32],
            block_number: 5733774,
            block_hash: [0x05; 32],
            timestamp_ms: 1_700_000_000_001,
            block_dao_ar: 12_000,
            tx_index: 1,
            dotbit_action: None,
            resolved_inputs: vec![ResolvedInputFacts {
                outpoint: OutPointKey::new([0x31; 32], 0),
                created_at_block: 5668752,
                created_by_block_dao_ar: 10_000,
                capacity: 120_00000000,
                occupied_capacity: 102_00000000,
                data_size: 0,
                data_hash: None,
                udt_amount: None,
                lock_script_hash_id: deposit_lock,
                lock_code_hash_id: InternId::new(1),
                lock_hash_type: 1,
                lock_args_id: InternId::new(2),
                type_script_hash_id: Some(InternId::new(3)),
                type_code_hash_id: Some(dao_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(4)),
                semantic_tag: CellSemanticTag::Dao,
                dao_state: Some(DaoCellState::Deposit),
                dao_compensation_ars: None,
                protocol_facts: None,
            }],
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x32; 32], 0),
                created_at_block: 5733774,
                created_by_block_dao_ar: 12_000,
                capacity: 120_00000000,
                lock_script_hash_id: request_lock, // different lock
                lock_code_hash_id: InternId::new(5),
                lock_hash_type: 1,
                lock_args_id: InternId::new(6),
                type_script_hash_id: Some(InternId::new(3)),
                type_code_hash_id: Some(dao_code_hash_id),
                type_hash_type: Some(1),
                type_args_id: Some(InternId::new(4)),
                occupied_capacity: 102_00000000,
                data_size: 8,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Dao,
                dao_state: Some(DaoCellState::WithdrawRequest {
                    deposit_block_number: 5668752,
                }),
                protocol_facts: None,
            }]
            .into(),
        };
        owner
            .apply_tx(&tx1, &ctx)
            .expect("apply withdraw request with changed lock");

        let entry = owner
            .deposits
            .get(&OutPointKey::new([0x31; 32], 0))
            .expect("dao entry");
        assert_eq!(entry.status, 1);
        assert_eq!(entry.withdraw_request_block, Some(5733774));
        assert_eq!(entry.withdraw_request_tx, Some(vec![0x32; 32]));
        assert_eq!(entry.withdraw_request_output_index, Some(0));
    }

    #[test]
    fn dao_owner_returns_error_instead_of_panicking_when_request_output_index_is_ambiguous() {
        let interner = IdentityInterner::default();
        let lock_hash = interner.intern_bytes(vec![0xaa; 32]);
        let plain_lock_hash = interner.intern_bytes(vec![0xbb; 32]);
        let dao_code_hash_id =
            interner.intern_bytes(hex::decode(&DAO_CODE_HASH[2..]).expect("dao code hash"));
        let frozen = interner.snapshot_for_reads();
        let ctx = ReducerContext::new(&frozen);
        let mut owner = DaoOwner::default();

        let origin_outpoint = OutPointKey::new([0x31; 32], 0);
        let request_outpoint = OutPointKey::new([0x32; 32], 0);
        owner.deposits.insert(
            origin_outpoint,
            DaoDepositCacheEntry {
                capacity: 200_00000000,
                occupied_capacity: 102_00000000,
                deposit_block_number: 100,
                deposit_timestamp: 0,
                lock_script_hash: vec![0xaa; 32],
                deposit_ar: 10_000,
                status: 1,
                withdraw_request_tx: Some(vec![0x32; 32]),
                withdraw_request_output_index: None,
                withdraw_request_block: Some(101),
                withdraw_request_ar: Some(12_000),
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: Some(102_00000000),
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        owner
            .request_outpoints
            .insert(request_outpoint, origin_outpoint);

        let tx = ResolvedTxFacts {
            tx_hash: [0x33; 32],
            block_number: 102,
            block_hash: [0x44; 32],
            timestamp_ms: 1_700_000_000_002,
            block_dao_ar: 13_000,
            tx_index: 0,
            dotbit_action: None,
            resolved_inputs: vec![
                ResolvedInputFacts {
                    outpoint: request_outpoint,
                    created_at_block: 101,
                    created_by_block_dao_ar: 12_000,
                    capacity: 200_00000000,
                    occupied_capacity: 142_00000000,
                    data_size: 0,
                    data_hash: None,
                    udt_amount: None,
                    lock_script_hash_id: lock_hash,
                    lock_code_hash_id: InternId::new(1),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(2),
                    type_script_hash_id: Some(InternId::new(3)),
                    type_code_hash_id: Some(dao_code_hash_id),
                    type_hash_type: Some(1),
                    type_args_id: Some(InternId::new(4)),
                    semantic_tag: CellSemanticTag::Dao,
                    dao_state: Some(DaoCellState::WithdrawRequest {
                        deposit_block_number: 100,
                    }),
                    dao_compensation_ars: None,
                    protocol_facts: None,
                },
                ResolvedInputFacts {
                    outpoint: OutPointKey::new([0x32; 32], 1),
                    created_at_block: 101,
                    created_by_block_dao_ar: 12_000,
                    capacity: 61_00000000,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data_hash: None,
                    udt_amount: None,
                    lock_script_hash_id: plain_lock_hash,
                    lock_code_hash_id: InternId::new(5),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(6),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    semantic_tag: CellSemanticTag::Plain,
                    dao_state: None,
                    dao_compensation_ars: None,
                    protocol_facts: None,
                },
            ],
            cells: vec![CellFacts {
                outpoint: OutPointKey::new([0x33; 32], 0),
                created_at_block: 102,
                created_by_block_dao_ar: 13_000,
                capacity: 219_60000000,
                lock_script_hash_id: lock_hash,
                lock_code_hash_id: InternId::new(7),
                lock_hash_type: 1,
                lock_args_id: InternId::new(8),
                type_script_hash_id: None,
                type_code_hash_id: None,
                type_hash_type: None,
                type_args_id: None,
                occupied_capacity: 61_00000000,
                data_size: 0,
                data: Vec::new(),
                data_hash: None,
                udt_amount: None,
                semantic_tag: CellSemanticTag::Plain,
                dao_state: None,
                protocol_facts: None,
            }]
            .into(),
        };

        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.apply_tx(&tx, &ctx)));
        assert!(
            result.is_ok(),
            "DAO reducer should return an error instead of panicking"
        );

        let err = result
            .expect("no panic")
            .expect_err("ambiguous request output index should error");
        assert!(
            err.to_string()
                .contains("DAO withdraw request output index missing/ambiguous"),
            "unexpected error: {err}"
        );
    }
}
