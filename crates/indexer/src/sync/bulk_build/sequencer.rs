use anyhow::{anyhow, Result};

use super::facts::{FactsArena, ResolvedTxFacts};
use super::live_cells::{ConsumeContext, LiveCellOwner, LiveCellSlot};

#[derive(Debug, Default)]
pub(crate) struct BulkSequencer {
    live_cells: LiveCellOwner,
}

impl BulkSequencer {
    pub(crate) fn resolve(&mut self, arena: &FactsArena) -> Result<Vec<ResolvedTxFacts>> {
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

            for cell in outputs {
                self.live_cells
                    .insert_created(LiveCellSlot::from_cell_facts(cell))?;
            }

            resolved_txs.push(ResolvedTxFacts {
                tx_hash: tx.hash,
                block_number: tx.block_number,
                block_dao_ar: tx.block_dao_ar,
                tx_index: tx.tx_index,
                resolved_inputs,
                cells: outputs.to_vec(),
            });
        }

        Ok(resolved_txs)
    }

    pub(crate) fn live_count(&self) -> usize {
        self.live_cells.live_count()
    }
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
                tx_range: 0..2,
            }],
            txs: vec![
                TxFacts {
                    hash: [0xaa; 32],
                    block_number: 14_000_321,
                    block_dao_ar: 10_000_000_000,
                    tx_index: 0,
                    is_cellbase: true,
                    input_outpoints: Vec::new(),
                    output_range: 0..1,
                },
                TxFacts {
                    hash: [0xbb; 32],
                    block_number: 14_000_321,
                    block_dao_ar: 10_000_000_000,
                    tx_index: 1,
                    is_cellbase: false,
                    input_outpoints: vec![OutPointKey::new([0xaa; 32], 0)],
                    output_range: 1..2,
                },
            ],
            cells: vec![
                CellFacts {
                    outpoint: OutPointKey::new([0xaa; 32], 0),
                    created_at_block: 14_000_321,
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
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Plain,
                    dao_state: None,
                },
                CellFacts {
                    outpoint: OutPointKey::new([0xbb; 32], 0),
                    created_at_block: 14_000_321,
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
                    udt_amount: None,
                    semantic_tag: CellSemanticTag::Plain,
                    dao_state: None,
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
}
