use anyhow::{anyhow, Result};
use std::collections::HashMap;

use super::facts::{CellFacts, CellSemanticTag, FactsArena, OutPointKey, ResolvedInputFacts};
use super::sequencer::BulkSequencer;
use crate::sync::types::InternId;

#[derive(Debug, Default)]
pub(crate) struct LiveCellOwner {
    live: HashMap<OutPointKey, LiveCellSlot>,
}

impl LiveCellOwner {
    pub(crate) fn live_count(&self) -> usize {
        self.live.len()
    }

    pub(crate) fn insert_created(&mut self, slot: LiveCellSlot) -> Result<()> {
        let outpoint = slot.outpoint;
        if self.live.insert(outpoint, slot).is_some() {
            return Err(anyhow!(
                "duplicate live output insertion: outpoint={}",
                format_outpoint(&outpoint)
            ));
        }
        Ok(())
    }

    pub(crate) fn consume(
        &mut self,
        outpoint: &OutPointKey,
        ctx: &ConsumeContext,
    ) -> Result<ResolvedInputFacts> {
        let slot = self.live.remove(outpoint).ok_or_else(|| {
            anyhow!(
                "missing live input: block={} tx=0x{} tx_index={} input_index={} outpoint={}",
                ctx.block_number,
                hex::encode(ctx.tx_hash),
                ctx.tx_index,
                ctx.input_index,
                format_outpoint(outpoint),
            )
        })?;

        Ok(slot.into_resolved_input_facts())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConsumeContext {
    pub(crate) block_number: i64,
    pub(crate) tx_hash: [u8; 32],
    pub(crate) tx_index: i32,
    pub(crate) input_index: i32,
}

#[derive(Debug, Clone)]
pub(crate) struct LiveCellSlot {
    pub(crate) outpoint: OutPointKey,
    pub(crate) created_at_block: i64,
    pub(crate) capacity: i64,
    pub(crate) occupied_capacity: i64,
    pub(crate) data_size: i32,
    pub(crate) udt_amount: Option<u128>,
    pub(crate) lock_script_hash_id: InternId,
    pub(crate) lock_code_hash_id: InternId,
    pub(crate) lock_hash_type: i16,
    pub(crate) lock_args_id: InternId,
    pub(crate) type_script_hash_id: Option<InternId>,
    pub(crate) type_code_hash_id: Option<InternId>,
    pub(crate) type_hash_type: Option<i16>,
    pub(crate) type_args_id: Option<InternId>,
    pub(crate) semantic_tag: CellSemanticTag,
}

impl LiveCellSlot {
    pub(crate) fn from_cell_facts(cell: &CellFacts) -> Self {
        Self {
            outpoint: cell.outpoint,
            created_at_block: cell.created_at_block,
            capacity: cell.capacity,
            occupied_capacity: cell.occupied_capacity,
            data_size: cell.data_size,
            udt_amount: cell.udt_amount,
            lock_script_hash_id: cell.lock_script_hash_id,
            lock_code_hash_id: cell.lock_code_hash_id,
            lock_hash_type: cell.lock_hash_type,
            lock_args_id: cell.lock_args_id,
            type_script_hash_id: cell.type_script_hash_id,
            type_code_hash_id: cell.type_code_hash_id,
            type_hash_type: cell.type_hash_type,
            type_args_id: cell.type_args_id,
            semantic_tag: cell.semantic_tag,
        }
    }

    fn into_resolved_input_facts(self) -> ResolvedInputFacts {
        ResolvedInputFacts {
            outpoint: self.outpoint,
            created_at_block: self.created_at_block,
            capacity: self.capacity,
            occupied_capacity: self.occupied_capacity,
            data_size: self.data_size,
            udt_amount: self.udt_amount,
            lock_script_hash_id: self.lock_script_hash_id,
            lock_code_hash_id: self.lock_code_hash_id,
            lock_hash_type: self.lock_hash_type,
            lock_args_id: self.lock_args_id,
            type_script_hash_id: self.type_script_hash_id,
            type_code_hash_id: self.type_code_hash_id,
            type_hash_type: self.type_hash_type,
            type_args_id: self.type_args_id,
            semantic_tag: self.semantic_tag,
        }
    }
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveCellResolutionSnapshot {
    pub txs: Vec<ResolvedTxSnapshot>,
    pub remaining_live_cells: usize,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTxSnapshot {
    pub tx_index: i32,
    pub resolved_inputs: Vec<ResolvedInputSnapshot>,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInputSnapshot {
    pub capacity: i64,
    pub occupied_capacity: i64,
    pub semantic_tag: CellSemanticTag,
}

pub(crate) fn resolve_live_cell_snapshot_for_test(
    arena: &FactsArena,
) -> Result<LiveCellResolutionSnapshot> {
    let mut sequencer = BulkSequencer::default();
    let resolved_txs = sequencer.resolve(arena)?;

    Ok(LiveCellResolutionSnapshot {
        txs: resolved_txs
            .into_iter()
            .map(|tx| ResolvedTxSnapshot {
                tx_index: tx.tx_index,
                resolved_inputs: tx
                    .resolved_inputs
                    .into_iter()
                    .map(|input| ResolvedInputSnapshot {
                        capacity: input.capacity,
                        occupied_capacity: input.occupied_capacity,
                        semantic_tag: input.semantic_tag,
                    })
                    .collect(),
            })
            .collect(),
        remaining_live_cells: sequencer.live_count(),
    })
}

fn format_outpoint(outpoint: &OutPointKey) -> String {
    format!("0x{}:{}", hex::encode(outpoint.tx_hash), outpoint.index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::bulk_build::facts::{CellSemanticTag, OutPointKey};
    use crate::sync::types::InternId;

    fn sample_outpoint() -> OutPointKey {
        OutPointKey::new([0x11; 32], 0)
    }

    fn sample_created_slot() -> LiveCellSlot {
        LiveCellSlot {
            outpoint: sample_outpoint(),
            created_at_block: 14_000_000,
            capacity: 100_00000000,
            occupied_capacity: 61_00000000,
            data_size: 0,
            udt_amount: None,
            lock_script_hash_id: InternId::new(0),
            lock_code_hash_id: InternId::new(1),
            lock_hash_type: 1,
            lock_args_id: InternId::new(2),
            type_script_hash_id: None,
            type_code_hash_id: None,
            type_hash_type: None,
            type_args_id: None,
            semantic_tag: CellSemanticTag::Plain,
        }
    }

    fn sample_consume_ctx() -> ConsumeContext {
        ConsumeContext {
            block_number: 14_000_001,
            tx_hash: [0x22; 32],
            tx_index: 1,
            input_index: 0,
        }
    }

    #[test]
    fn live_cell_owner_starts_empty() {
        let owner = LiveCellOwner::default();
        assert_eq!(owner.live_count(), 0);
    }

    #[test]
    fn live_cell_owner_resolves_same_batch_create_then_consume() {
        let mut owner = LiveCellOwner::default();
        owner.insert_created(sample_created_slot()).expect("insert");

        let resolved = owner
            .consume(&sample_outpoint(), &sample_consume_ctx())
            .expect("consume");

        assert_eq!(resolved.capacity, 100_00000000);
        assert_eq!(resolved.occupied_capacity, 61_00000000);
        assert_eq!(resolved.semantic_tag, CellSemanticTag::Plain);
        assert_eq!(owner.live_count(), 0);
    }

    #[test]
    fn live_cell_owner_errors_on_missing_input() {
        let mut owner = LiveCellOwner::default();

        let err = owner
            .consume(&sample_outpoint(), &sample_consume_ctx())
            .expect_err("missing input must fail");
        let err_text = err.to_string();

        assert!(err_text.contains("missing live input"));
        assert!(err_text.contains("block=14000001"));
        assert!(err_text.contains("tx_index=1"));
        assert!(err_text.contains("input_index=0"));
        assert!(err_text.contains(&format!(
            "outpoint=0x{}:0",
            hex::encode(sample_outpoint().tx_hash)
        )));
    }

    #[test]
    fn live_cell_owner_errors_on_duplicate_created_outpoint() {
        let mut owner = LiveCellOwner::default();
        owner
            .insert_created(sample_created_slot())
            .expect("first insert");

        let err = owner
            .insert_created(sample_created_slot())
            .expect_err("duplicate outpoint must fail");

        assert!(err.to_string().contains(&format!(
            "outpoint=0x{}:0",
            hex::encode(sample_outpoint().tx_hash)
        )));
    }
}
