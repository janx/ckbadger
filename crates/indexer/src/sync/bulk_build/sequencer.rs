use std::borrow::Cow;

use anyhow::{anyhow, Result};

use super::facts::{
    DaoCellState, DaoCompensationArs, FactsArena, ResolvedInputFacts, ResolvedTxFacts,
};
use super::live_cells::{ConsumeContext, LiveCellOwner, LiveCellSlot};

#[derive(Debug, Default)]
pub(crate) struct BulkSequencer {
    live_cells: LiveCellOwner,
}

impl BulkSequencer {
    pub(crate) fn resolve<'a>(
        &mut self,
        arena: &'a FactsArena,
    ) -> Result<Vec<ResolvedTxFacts<'a>>> {
        let mut resolved_txs = Vec::with_capacity(arena.txs.len());

        for tx in &arena.txs {
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
                    resolved_inputs.push(self.live_cells.consume(
                        outpoint,
                        &ConsumeContext {
                            block_number: tx.block_number,
                            tx_hash: tx.hash,
                            tx_index: tx.tx_index,
                            input_index,
                        },
                    )?);
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
                self.live_cells.insert_created(
                    LiveCellSlot::from_cell_facts(cell)
                        .with_dao_compensation_ars(request_output_ars[output_pos]),
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
        }

        Ok(resolved_txs)
    }

    pub(crate) fn live_count(&self) -> usize {
        self.live_cells.live_count()
    }

    pub(crate) fn live_slots(&self) -> impl Iterator<Item = &super::live_cells::LiveCellSlot> {
        self.live_cells.live_slots()
    }

    pub(crate) fn live_cells_bytes(&self) -> u64 {
        self.live_cells.estimated_bytes()
    }
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
    use crate::sync::types::InternId;

    fn build_sample_facts_arena() -> FactsArena {
        FactsArena {
            blocks: vec![BlockFacts {
                number: 14_000_321,
                hash: [0x10; 32],
                timestamp_ms: 1_704_067_200_000,
                epoch_number: 42,
                epoch_index: 0,
                epoch_length: 1800,
                dao: [0x00; 32],
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
                    data_hash: None,
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
                    data_hash: None,
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
    fn sequencer_preserves_protocol_facts_on_resolved_inputs() {
        let mut arena = build_sample_facts_arena();
        arena.cells[0].protocol_facts = Some(super::super::facts::CellProtocolFacts::Spore(
            super::super::facts::SporeProtocolFacts {
                spore_id: [0x44; 32],
                is_did: false,
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
