use std::borrow::Cow;
use std::collections::VecDeque;

use anyhow::{anyhow, Context, Result};

use ckbadger_store::types::{LiveCellSummary, LIVE_CELL_SUMMARY_HISTORY_BLOCKS};

use super::facts::{
    DaoCellState, DaoCompensationArs, FactsArena, ResolvedInputFacts, ResolvedTxFacts,
};
use super::interner::IdentityLiveness;
use super::live_cells::{ConsumeContext, LiveCellExtras, LiveCellOwner, LiveCellSlot};

#[derive(Debug)]
pub(crate) struct BulkSequencer {
    live_cells: LiveCellOwner,
    live_cell_summaries: VecDeque<LiveCellSummary>,
}

impl Default for BulkSequencer {
    fn default() -> Self {
        Self {
            live_cells: LiveCellOwner::default(),
            live_cell_summaries: VecDeque::with_capacity(
                usize::try_from(LIVE_CELL_SUMMARY_HISTORY_BLOCKS)
                    .expect("live-cell summary history window must fit usize"),
            ),
        }
    }
}

impl BulkSequencer {
    pub(crate) fn resolve<'a>(
        &mut self,
        arena: &'a FactsArena,
    ) -> Result<Vec<ResolvedTxFacts<'a>>> {
        self.resolve_inner(arena, None)
    }

    pub(crate) fn resolve_with_liveness<'a>(
        &mut self,
        arena: &'a FactsArena,
        liveness: &mut IdentityLiveness,
    ) -> Result<Vec<ResolvedTxFacts<'a>>> {
        self.resolve_inner(arena, Some(liveness))
    }

    fn resolve_inner<'a>(
        &mut self,
        arena: &'a FactsArena,
        mut liveness: Option<&mut IdentityLiveness>,
    ) -> Result<Vec<ResolvedTxFacts<'a>>> {
        validate_block_partition(arena, self.live_cell_summaries.back())?;
        let mut resolved_txs = Vec::with_capacity(arena.txs.len());
        let mut next_block_index = 0usize;

        for (tx_position, tx) in arena.txs.iter().enumerate() {
            let block = arena.blocks.get(next_block_index).ok_or_else(|| {
                anyhow!(
                    "bulk sequencer transaction is not covered by a block: tx_position={} blocks={}",
                    tx_position,
                    arena.blocks.len(),
                )
            })?;
            if tx.block_number != block.number || tx.block_hash != block.hash {
                return Err(anyhow!(
                    "bulk live-cell summary tx/block mismatch: block={} block_hash=0x{} tx_block={} tx_block_hash=0x{} tx=0x{} tx_index={}",
                    block.number,
                    hex::encode(block.hash),
                    tx.block_number,
                    hex::encode(tx.block_hash),
                    hex::encode(tx.hash),
                    tx.tx_index,
                ));
            }
            let mut resolved_inputs = Vec::with_capacity(tx.input_outpoints.len());

            if !tx.is_cellbase {
                for (input_position, outpoint) in tx.input_outpoints.iter().enumerate() {
                    let input_index = i32::try_from(input_position).map_err(|_| {
                        anyhow!(
                            "input index exceeds i32: tx=0x{} tx_index={} input_index={}",
                            hex::encode(tx.hash),
                            tx.tx_index,
                            input_position
                        )
                    })?;
                    let resolved = self.live_cells.consume(
                        outpoint,
                        &ConsumeContext {
                            block_number: tx.block_number,
                            tx_hash: tx.hash,
                            tx_index: tx.tx_index,
                            input_index,
                        },
                    )?;
                    if let Some(liveness) = liveness.as_deref_mut() {
                        release_input_identities(liveness, &resolved).with_context(|| {
                            format!(
                                "failed to release consumed live-cell identities: block={} tx=0x{} tx_index={} input_index={} outpoint=0x{}:{}",
                                tx.block_number,
                                hex::encode(tx.hash),
                                tx.tx_index,
                                input_index,
                                hex::encode(outpoint.tx_hash),
                                outpoint.index,
                            )
                        })?;
                    }
                    resolved_inputs.push(resolved);
                }
            }

            let outputs = arena.cells.get(tx.output_range.clone()).ok_or_else(|| {
                anyhow!(
                    "tx output range out of bounds: block={} tx=0x{} tx_index={} output_start={} output_end={} arena_cells={}",
                    tx.block_number,
                    hex::encode(tx.hash),
                    tx.tx_index,
                    tx.output_range.start,
                    tx.output_range.end,
                    arena.cells.len()
                )
            })?;

            let request_output_ars = resolve_request_output_ars(
                outputs,
                &resolved_inputs,
                tx.block_number,
                tx.hash,
                tx.tx_index,
                tx.block_dao_ar,
            )?;
            for (output_pos, cell) in outputs.iter().enumerate() {
                if let Some(liveness) = liveness.as_deref_mut() {
                    retain_cell_identities(liveness, cell).with_context(|| {
                        format!(
                            "failed to retain created live-cell identities: block={} tx=0x{} tx_index={} outpoint=0x{}:{}",
                            tx.block_number,
                            hex::encode(tx.hash),
                            tx.tx_index,
                            hex::encode(cell.outpoint.tx_hash),
                            cell.outpoint.index,
                        )
                    })?;
                }
                self.live_cells.insert_created(
                    cell.outpoint,
                    LiveCellSlot::from_cell_facts(cell)?,
                    LiveCellExtras::from_cell_facts(cell, request_output_ars[output_pos]),
                    cell.protocol_facts.clone(),
                )?;
            }

            resolved_txs.push(ResolvedTxFacts {
                tx_hash: tx.hash,
                block_number: tx.block_number,
                block_hash: tx.block_hash,
                timestamp_ms: tx.timestamp_ms,
                block_dao_ar: tx.block_dao_ar,
                tx_index: tx.tx_index,
                dotbit_action: tx.dotbit_action.clone(),
                resolved_inputs,
                cells: Cow::Borrowed(outputs),
            });

            let processed_txs = tx_position.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "bulk sequencer processed transaction count overflow: tx_position={}",
                    tx_position
                )
            })?;
            if processed_txs == block.tx_range.end {
                let summary = self.live_cells.summary(block.number, &block.hash)?;
                let history_blocks =
                    usize::try_from(LIVE_CELL_SUMMARY_HISTORY_BLOCKS).map_err(|_| {
                        anyhow!(
                            "live-cell summary history window is not a usize: blocks={}",
                            LIVE_CELL_SUMMARY_HISTORY_BLOCKS
                        )
                    })?;
                if self.live_cell_summaries.len() > history_blocks {
                    return Err(anyhow!(
                        "bulk live-cell summary window exceeded its fixed bound: len={} bound={}",
                        self.live_cell_summaries.len(),
                        history_blocks,
                    ));
                }
                if self.live_cell_summaries.len() == history_blocks {
                    self.live_cell_summaries.pop_front();
                }
                self.live_cell_summaries.push_back(summary);
                next_block_index = next_block_index.checked_add(1).ok_or_else(|| {
                    anyhow!(
                        "bulk sequencer block index overflow: block_index={}",
                        next_block_index
                    )
                })?;
            }
        }

        if next_block_index != arena.blocks.len() {
            return Err(anyhow!(
                "bulk sequencer did not finalize every block summary: finalized_blocks={} arena_blocks={} txs={}",
                next_block_index,
                arena.blocks.len(),
                arena.txs.len(),
            ));
        }

        Ok(resolved_txs)
    }

    pub(crate) fn live_count(&self) -> usize {
        self.live_cells.live_count()
    }

    pub(crate) fn live_slots(
        &self,
    ) -> impl Iterator<Item = (&super::facts::OutPointKey, &super::live_cells::LiveCellSlot)> {
        self.live_cells.live_slots()
    }

    pub(crate) fn live_cells_bytes(&self) -> u64 {
        self.live_cells.estimated_bytes()
            + std::mem::size_of::<VecDeque<LiveCellSummary>>() as u64
            + self.live_cell_summaries.capacity() as u64
                * std::mem::size_of::<LiveCellSummary>() as u64
    }

    pub(crate) fn live_cell_summaries(&self) -> impl Iterator<Item = &LiveCellSummary> {
        self.live_cell_summaries.iter()
    }
}

fn validate_block_partition(
    arena: &FactsArena,
    previous_summary: Option<&LiveCellSummary>,
) -> Result<()> {
    if arena.blocks.is_empty() {
        if arena.txs.is_empty() {
            return Ok(());
        }
        return Err(anyhow!(
            "bulk sequencer has transactions without blocks: txs={}",
            arena.txs.len()
        ));
    }

    let mut expected_tx_start = 0usize;
    let mut previous_number = previous_summary.map(|summary| summary.tip_block_number);
    for block in &arena.blocks {
        if block.tx_range.start != expected_tx_start
            || block.tx_range.end <= block.tx_range.start
            || block.tx_range.end > arena.txs.len()
        {
            return Err(anyhow!(
                "invalid bulk block transaction partition: block={} range={}..{} expected_start={} arena_txs={}",
                block.number,
                block.tx_range.start,
                block.tx_range.end,
                expected_tx_start,
                arena.txs.len(),
            ));
        }
        let actual_tx_count = block.tx_range.end - block.tx_range.start;
        let declared_tx_count = usize::try_from(block.transactions_count).map_err(|_| {
            anyhow!(
                "negative bulk block transaction count: block={} transactions_count={}",
                block.number,
                block.transactions_count,
            )
        })?;
        if declared_tx_count != actual_tx_count {
            return Err(anyhow!(
                "bulk block transaction count/range mismatch: block={} transactions_count={} range_count={}",
                block.number,
                declared_tx_count,
                actual_tx_count,
            ));
        }
        if let Some(previous_number) = previous_number {
            let expected_number = previous_number.checked_add(1).ok_or_else(|| {
                anyhow!(
                    "bulk live-cell summary block number overflow after block {}",
                    previous_number
                )
            })?;
            if block.number != expected_number {
                return Err(anyhow!(
                    "non-contiguous bulk live-cell summary block sequence: previous_block={} expected_block={} actual_block={}",
                    previous_number,
                    expected_number,
                    block.number,
                ));
            }
        }
        expected_tx_start = block.tx_range.end;
        previous_number = Some(block.number);
    }

    if expected_tx_start != arena.txs.len() {
        return Err(anyhow!(
            "bulk sequencer block ranges do not cover all transactions: covered_txs={} arena_txs={}",
            expected_tx_start,
            arena.txs.len(),
        ));
    }
    Ok(())
}

fn retain_cell_identities(
    liveness: &mut IdentityLiveness,
    cell: &super::facts::CellFacts,
) -> Result<()> {
    liveness.retain(cell.lock_script_hash_id)?;
    liveness.retain(cell.lock_code_hash_id)?;
    liveness.retain(cell.lock_args_id)?;
    if let Some(id) = cell.type_script_hash_id {
        liveness.retain(id)?;
    }
    if let Some(id) = cell.type_code_hash_id {
        liveness.retain(id)?;
    }
    if let Some(id) = cell.type_args_id {
        liveness.retain(id)?;
    }
    Ok(())
}

fn release_input_identities(
    liveness: &mut IdentityLiveness,
    input: &ResolvedInputFacts,
) -> Result<()> {
    liveness.release(input.lock_script_hash_id)?;
    liveness.release(input.lock_code_hash_id)?;
    liveness.release(input.lock_args_id)?;
    if let Some(id) = input.type_script_hash_id {
        liveness.release(id)?;
    }
    if let Some(id) = input.type_code_hash_id {
        liveness.release(id)?;
    }
    if let Some(id) = input.type_args_id {
        liveness.release(id)?;
    }
    Ok(())
}

/// Resolve DAO compensation AR values for withdraw-request outputs.
///
/// Per RFC 0023: "For a deposit cell at input index `i`, a withdrawing cell
/// MUST be created at output index `i`."  Matching is therefore positional —
/// the lock script may change between deposit and withdraw-request.
fn resolve_request_output_ars(
    outputs: &[super::facts::CellFacts],
    resolved_inputs: &[ResolvedInputFacts],
    block_number: i64,
    tx_hash: [u8; 32],
    tx_index: i32,
    block_dao_ar: u64,
) -> Result<Vec<Option<DaoCompensationArs>>> {
    let has_any_request = outputs
        .iter()
        .any(|c| matches!(c.dao_state, Some(DaoCellState::WithdrawRequest { .. })));
    if !has_any_request {
        return Ok(vec![None; outputs.len()]);
    }

    let mut matched = vec![None; outputs.len()];

    for (input_index, input) in resolved_inputs.iter().enumerate() {
        if !matches!(input.dao_state, Some(DaoCellState::Deposit)) {
            continue;
        }

        // RFC 0023 positional rule: deposit at input[i] → withdraw-request at output[i]
        let output = outputs.get(input_index).ok_or_else(|| {
            anyhow!(
                "DAO deposit at input index {} has no corresponding output: block={} tx=0x{} tx_index={}",
                input_index,
                block_number,
                hex::encode(tx_hash),
                tx_index,
            )
        })?;

        match output.dao_state {
            Some(DaoCellState::WithdrawRequest {
                deposit_block_number,
            }) => {
                if deposit_block_number != input.created_at_block {
                    return Err(anyhow!(
                        "DAO withdraw-request at output {} deposit_block mismatch: expected={} actual={} block={} tx=0x{} tx_index={}",
                        input_index,
                        input.created_at_block,
                        deposit_block_number,
                        block_number,
                        hex::encode(tx_hash),
                        tx_index,
                    ));
                }
                matched[input_index] = Some(DaoCompensationArs {
                    deposit_ar: input.created_by_block_dao_ar,
                    withdraw_request_ar: block_dao_ar,
                });
            }
            _ => {
                return Err(anyhow!(
                    "DAO deposit at input {} expected withdraw-request at same output position, found {:?}: block={} tx=0x{} tx_index={}",
                    input_index,
                    output.dao_state,
                    block_number,
                    hex::encode(tx_hash),
                    tx_index,
                ));
            }
        }
    }

    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::{
        BlockFacts, CellFacts, CellSemanticTag, FactsArena, OutPointKey, TxFacts,
    };
    use crate::sync::bulk_build::interner::IdentityLiveness;
    use crate::sync::types::InternId;

    fn build_sample_facts_arena() -> FactsArena {
        FactsArena {
            blocks: vec![BlockFacts {
                number: 14_000_321,
                hash: [0x10; 32],
                parent_hash: [0u8; 32],
                timestamp_ms: 1_704_067_200_000,
                epoch_number: 42,
                epoch_index: 0,
                epoch_length: 1800,
                dao: [0x00; 32],
                compact_target: 0x1a08a97e,
                uncles_count: 0,
                proposals_count: 0,
                miner_lock_hash: None,
                transactions_count: 2,
                tx_range: 0..2,
            }],
            txs: vec![
                TxFacts {
                    hash: [0xaa; 32],
                    block_number: 14_000_321,
                    block_hash: [0x10; 32],
                    timestamp_ms: 1_704_067_200_000,
                    block_dao_ar: 10_000_000_000,
                    tx_index: 0,
                    is_cellbase: true,
                    inputs_count: 1,
                    outputs_count: 1,
                    tx_size: 120,
                    cycles: Some(0),
                    dotbit_action: None,
                    input_outpoints: Vec::new(),
                    output_range: 0..1,
                },
                TxFacts {
                    hash: [0xbb; 32],
                    block_number: 14_000_321,
                    block_hash: [0x10; 32],
                    timestamp_ms: 1_704_067_200_000,
                    block_dao_ar: 10_000_000_000,
                    tx_index: 1,
                    is_cellbase: false,
                    inputs_count: 1,
                    outputs_count: 1,
                    tx_size: 144,
                    cycles: Some(1_000),
                    dotbit_action: None,
                    input_outpoints: vec![OutPointKey::new([0xaa; 32], 0)],
                    output_range: 1..2,
                },
            ],
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0xaa; 32], 0),
                    created_at_block: 14_000_321,
                    created_by_block_dao_ar: 10_000_000_000,
                    capacity: 100_00000000,
                    lock_script_hash_id: InternId::new(0),
                    lock_code_hash_id: InternId::new(1),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(2),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: Some([0x31; 32]),
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0xbb; 32], 0),
                    created_at_block: 14_000_321,
                    created_by_block_dao_ar: 10_000_000_000,
                    capacity: 99_00000000,
                    lock_script_hash_id: InternId::new(2),
                    lock_code_hash_id: InternId::new(3),
                    lock_hash_type: 1,
                    lock_args_id: InternId::new(4),
                    type_script_hash_id: None,
                    type_code_hash_id: None,
                    type_hash_type: None,
                    type_args_id: None,
                    occupied_capacity: 61_00000000,
                    data_size: 0,
                    data: Vec::new(),
                    data_hash: Some([0x32; 32]),
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Plain,
                    dao_state: None,
                    protocol_facts: None,
                },
            ],
        }
    }

    #[test]
    fn sequencer_emits_resolved_tx_facts_in_tx_order() {
        let arena = build_sample_facts_arena();
        let resolved = BulkSequencer::default().resolve(&arena).expect("resolve");

        assert_eq!(resolved[0].tx_index, 0);
        assert_eq!(resolved[1].tx_index, 1);
        assert_eq!(resolved[0].block_dao_ar, 10_000_000_000);
        assert_eq!(resolved[0].block_hash, [0x10; 32]);
        assert_eq!(resolved[0].timestamp_ms, 1_704_067_200_000);
    }

    #[test]
    fn sequencer_exposes_consumed_inputs_to_reducers_without_db_reads() {
        let arena = build_sample_facts_arena();
        let resolved = BulkSequencer::default().resolve(&arena).expect("resolve");

        assert_eq!(resolved[1].resolved_inputs.len(), 1);
        assert_eq!(resolved[1].resolved_inputs[0].capacity, 100_00000000);
        assert_eq!(
            resolved[1].resolved_inputs[0].occupied_capacity,
            61_00000000
        );
    }

    #[test]
    fn sequencer_retains_only_fixed_reorg_window_of_block_end_summaries() {
        let mut sequencer = BulkSequencer::default();
        let base_block = 14_000_321i64;
        let mut previous_hash = [0u8; 32];

        for offset in 0..40u8 {
            let mut arena = build_sample_facts_arena();
            let block_number = base_block + i64::from(offset);
            let block_hash = [offset + 1; 32];
            let cellbase_hash = [0x40 + offset; 32];
            let spend_hash = [0x80 + offset; 32];
            arena.blocks[0].number = block_number;
            arena.blocks[0].hash = block_hash;
            arena.blocks[0].parent_hash = previous_hash;
            for tx in &mut arena.txs {
                tx.block_number = block_number;
                tx.block_hash = block_hash;
            }
            arena.txs[0].hash = cellbase_hash;
            arena.txs[1].hash = spend_hash;
            arena.txs[1].input_outpoints = vec![OutPointKey::new(cellbase_hash, 0)];
            arena.cells[0].outpoint = OutPointKey::new(cellbase_hash, 0);
            arena.cells[1].outpoint = OutPointKey::new(spend_hash, 0);
            for cell in &mut arena.cells {
                cell.created_at_block = block_number;
            }

            sequencer.resolve(&arena).unwrap();
            previous_hash = block_hash;
        }

        let summaries = sequencer.live_cell_summaries().copied().collect::<Vec<_>>();
        assert_eq!(
            summaries.len(),
            usize::try_from(LIVE_CELL_SUMMARY_HISTORY_BLOCKS).unwrap()
        );
        assert_eq!(
            sequencer.live_cell_summaries.capacity(),
            usize::try_from(LIVE_CELL_SUMMARY_HISTORY_BLOCKS).unwrap()
        );
        assert_eq!(summaries.first().unwrap().tip_block_number, base_block + 3);
        assert_eq!(summaries.last().unwrap().tip_block_number, base_block + 39);
        assert_eq!(summaries.last().unwrap().live_cells().unwrap(), 40);
        assert_eq!(summaries.last().unwrap().plain, 40);
    }

    #[test]
    fn sequencer_tracks_only_end_of_batch_live_identity_references() {
        let arena = build_sample_facts_arena();
        let mut sequencer = BulkSequencer::default();
        let mut liveness = IdentityLiveness::default();
        liveness.ensure_slots(5);

        let resolved = sequencer
            .resolve_with_liveness(&arena, &mut liveness)
            .expect("resolve with liveness");
        drop(resolved);

        assert_eq!(liveness.live_refs(InternId::new(0)), Some(0));
        assert_eq!(liveness.live_refs(InternId::new(1)), Some(0));
        assert_eq!(liveness.live_refs(InternId::new(2)), Some(1));
        assert_eq!(liveness.live_refs(InternId::new(3)), Some(1));
        assert_eq!(liveness.live_refs(InternId::new(4)), Some(1));

        let mut reclaimable = liveness
            .drain_zero_candidates()
            .into_iter()
            .map(InternId::as_usize)
            .collect::<Vec<_>>();
        reclaimable.sort_unstable();
        assert_eq!(reclaimable, vec![0, 1]);
    }

    #[test]
    fn sequencer_preserves_protocol_facts_on_resolved_inputs() {
        let mut arena = build_sample_facts_arena();
        arena.cells[0].protocol_facts = Some(super::super::facts::CellProtocolFacts::Spore(
            super::super::facts::SporeProtocolFacts {
                spore_id: [0x44; 32],
                content_type: "image/png".to_string(),
                content: b"payload".to_vec(),
                cluster_id: Some([0x55; 32]),
            },
        ));

        let resolved = BulkSequencer::default().resolve(&arena).expect("resolve");

        match resolved[1].resolved_inputs[0]
            .protocol_facts
            .as_ref()
            .expect("protocol facts")
        {
            super::super::facts::CellProtocolFacts::Spore(spore) => {
                assert_eq!(spore.spore_id, [0x44; 32]);
                assert_eq!(spore.cluster_id, Some([0x55; 32]));
                assert_eq!(spore.content_type, "image/png");
                assert_eq!(spore.content, b"payload");
            }
            other => panic!("expected spore protocol facts, got {other:?}"),
        }
    }

    #[test]
    fn sequencer_borrows_output_cells_from_arena() {
        let arena = build_sample_facts_arena();
        let resolved = BulkSequencer::default().resolve(&arena).expect("resolve");

        assert!(matches!(resolved[0].cells, Cow::Borrowed(_)));
        assert_eq!(resolved[0].cells.as_ptr(), arena.cells[0..1].as_ptr());
    }

    /// Regression: block 5733774 crashed because the DAO withdraw-request
    /// matching required lock_script_hash_id equality.  CKB DAO allows
    /// changing the lock script during Phase 1 (RFC 0023).  After the fix,
    /// positional matching (input[i] → output[i]) is used instead.
    #[test]
    fn resolve_request_output_ars_allows_lock_script_change() {
        let deposit_ar = 10_000_000_000u64;
        let block_dao_ar = 10_500_000_000u64;
        let deposit_block = 5_668_752i64;

        // Deposit cell uses lock_script_hash_id 100
        let deposit_input = ResolvedInputFacts {
            outpoint: OutPointKey::new([0x6e; 32], 0),
            created_at_block: deposit_block,
            created_by_block_dao_ar: deposit_ar,
            capacity: 200_00000000,
            occupied_capacity: 102_00000000,
            data_size: 0,
            data_hash: None,
            udt_amount: None,
            lock_script_hash_id: InternId::new(100),
            lock_code_hash_id: InternId::new(1),
            lock_hash_type: 1,
            lock_args_id: InternId::new(2),
            type_script_hash_id: Some(InternId::new(10)),
            type_code_hash_id: Some(InternId::new(11)),
            type_hash_type: Some(1),
            type_args_id: Some(InternId::new(12)),
            semantic_tag: CellSemanticTag::Dao,
            dao_state: Some(DaoCellState::Deposit),
            dao_compensation_ars: None,
            protocol_facts: None,
        };

        // Withdraw-request output uses DIFFERENT lock_script_hash_id 999
        let request_output = CellFacts {
            outpoint: OutPointKey::new([0x1d; 32], 0),
            created_at_block: 5_733_774,
            created_by_block_dao_ar: block_dao_ar,
            capacity: 200_00000000,
            occupied_capacity: 102_00000000,
            udt_amount: None,
            lock_script_hash_id: InternId::new(999), // different!
            lock_code_hash_id: InternId::new(50),
            lock_hash_type: 1,
            lock_args_id: InternId::new(51),
            type_script_hash_id: Some(InternId::new(10)),
            type_code_hash_id: Some(InternId::new(11)),
            type_hash_type: Some(1),
            type_args_id: Some(InternId::new(12)),
            data_size: 8,
            data: deposit_block.to_le_bytes().to_vec(),
            data_hash: None,
            semantic_tag: CellSemanticTag::Dao,
            dao_state: Some(DaoCellState::WithdrawRequest {
                deposit_block_number: deposit_block,
            }),
            protocol_facts: None,
        };

        let result = resolve_request_output_ars(
            &[request_output],
            &[deposit_input],
            5_733_774,
            [0x1d; 32],
            1,
            block_dao_ar,
        );

        let matched = result.expect("should match despite different lock scripts");
        let ars = matched[0].expect("output 0 should have compensation ARs");
        assert_eq!(ars.deposit_ar, deposit_ar);
        assert_eq!(ars.withdraw_request_ar, block_dao_ar);
    }
}
