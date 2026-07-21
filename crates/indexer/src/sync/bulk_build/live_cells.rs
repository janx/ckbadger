use anyhow::{anyhow, Result};
use rustc_hash::FxHashMap;
use serde::Serialize;

use super::facts::{
    CellFacts, CellProtocolFacts, CellSemanticTag, DaoCellState, DaoCompensationArs, FactsArena,
    OutPointKey, ResolvedInputFacts,
};
use super::sequencer::BulkSequencer;
use crate::sync::types::InternId;

#[derive(Debug, Default)]
pub(crate) struct LiveCellOwner {
    live: FxHashMap<OutPointKey, LiveCellSlot>,
    extras: FxHashMap<OutPointKey, LiveCellExtras>,
    protocol_facts: FxHashMap<OutPointKey, CellProtocolFacts>,
}

impl LiveCellOwner {
    pub(crate) fn live_count(&self) -> usize {
        self.live.len()
    }

    pub(crate) fn live_slots(&self) -> impl Iterator<Item = (&OutPointKey, &LiveCellSlot)> {
        self.live.iter()
    }

    pub(crate) fn estimated_bytes(&self) -> u64 {
        super::accounting::hash_map_serialized_bytes(&self.live)
            + super::accounting::hash_map_serialized_bytes(&self.extras)
            + super::accounting::hash_map_serialized_bytes(&self.protocol_facts)
    }

    pub(crate) fn insert_created(
        &mut self,
        outpoint: OutPointKey,
        slot: LiveCellSlot,
        extras: Option<LiveCellExtras>,
        protocol_facts: Option<CellProtocolFacts>,
    ) -> Result<()> {
        if self.live.contains_key(&outpoint)
            || self.extras.contains_key(&outpoint)
            || self.protocol_facts.contains_key(&outpoint)
        {
            return Err(anyhow!(
                "duplicate live output insertion: outpoint={}",
                format_outpoint(&outpoint)
            ));
        }
        self.live.insert(outpoint, slot);
        if let Some(extras) = extras {
            self.extras.insert(outpoint, extras);
        }
        if let Some(facts) = protocol_facts {
            self.protocol_facts.insert(outpoint, facts);
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
        let extras = self.extras.remove(outpoint).unwrap_or_default();
        let facts = self.protocol_facts.remove(outpoint);

        Ok(slot.into_resolved_input_facts(*outpoint, extras, facts))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConsumeContext {
    pub(crate) block_number: i64,
    pub(crate) tx_hash: [u8; 32],
    pub(crate) tx_index: i32,
    pub(crate) input_index: i32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct LiveCellSlot {
    pub(crate) created_at_block: i64,
    pub(crate) created_by_block_dao_ar: u64,
    pub(crate) capacity: i64,
    pub(crate) occupied_capacity: i64,
    pub(crate) data_size: i32,
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

/// Fields that are absent on most live cells. Keeping them in a sparse side
/// map avoids paying their large enum/`Option<u128>` layout for every entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub(crate) struct LiveCellExtras {
    pub(crate) data_hash: Option<[u8; 32]>,
    pub(crate) udt_amount: Option<u128>,
    pub(crate) dao_state: Option<DaoCellState>,
    pub(crate) dao_compensation_ars: Option<DaoCompensationArs>,
}

impl LiveCellExtras {
    pub(crate) fn from_cell_facts(
        cell: &CellFacts,
        dao_compensation_ars: Option<DaoCompensationArs>,
    ) -> Option<Self> {
        let extras = Self {
            data_hash: cell.data_hash,
            udt_amount: cell.udt_amount,
            dao_state: cell.dao_state,
            dao_compensation_ars,
        };
        (extras.data_hash.is_some()
            || extras.udt_amount.is_some()
            || extras.dao_state.is_some()
            || extras.dao_compensation_ars.is_some())
        .then_some(extras)
    }
}

impl LiveCellSlot {
    pub(crate) fn from_cell_facts(cell: &CellFacts) -> Self {
        Self {
            created_at_block: cell.created_at_block,
            created_by_block_dao_ar: cell.created_by_block_dao_ar,
            capacity: cell.capacity,
            occupied_capacity: cell.occupied_capacity,
            data_size: cell.data_size,
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

    fn into_resolved_input_facts(
        self,
        outpoint: OutPointKey,
        extras: LiveCellExtras,
        protocol_facts: Option<CellProtocolFacts>,
    ) -> ResolvedInputFacts {
        ResolvedInputFacts {
            outpoint,
            created_at_block: self.created_at_block,
            created_by_block_dao_ar: self.created_by_block_dao_ar,
            capacity: self.capacity,
            occupied_capacity: self.occupied_capacity,
            data_size: self.data_size,
            data_hash: extras.data_hash,
            udt_amount: extras.udt_amount,
            lock_script_hash_id: self.lock_script_hash_id,
            lock_code_hash_id: self.lock_code_hash_id,
            lock_hash_type: self.lock_hash_type,
            lock_args_id: self.lock_args_id,
            type_script_hash_id: self.type_script_hash_id,
            type_code_hash_id: self.type_code_hash_id,
            type_hash_type: self.type_hash_type,
            type_args_id: self.type_args_id,
            semantic_tag: self.semantic_tag,
            dao_state: extras.dao_state,
            dao_compensation_ars: extras.dao_compensation_ars,
            protocol_facts,
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
            created_at_block: 14_000_000,
            created_by_block_dao_ar: 10_000_000_000,
            capacity: 100_00000000,
            occupied_capacity: 61_00000000,
            data_size: 0,
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
    fn live_cell_base_slot_stays_compact() {
        assert!(
            std::mem::size_of::<LiveCellSlot>() <= 88,
            "LiveCellSlot grew to {} bytes",
            std::mem::size_of::<LiveCellSlot>()
        );
    }

    #[test]
    fn sparse_live_cell_extras_survive_resolution() {
        let mut owner = LiveCellOwner::default();
        let extras = LiveCellExtras {
            data_hash: Some([0x44; 32]),
            udt_amount: Some(123),
            dao_state: Some(DaoCellState::Deposit),
            dao_compensation_ars: Some(DaoCompensationArs {
                deposit_ar: 10,
                withdraw_request_ar: 20,
            }),
        };
        owner
            .insert_created(sample_outpoint(), sample_created_slot(), Some(extras), None)
            .unwrap();

        let resolved = owner
            .consume(&sample_outpoint(), &sample_consume_ctx())
            .unwrap();
        assert_eq!(resolved.outpoint, sample_outpoint());
        assert_eq!(resolved.data_hash, Some([0x44; 32]));
        assert_eq!(resolved.udt_amount, Some(123));
        assert_eq!(resolved.dao_state, Some(DaoCellState::Deposit));
        assert_eq!(
            resolved.dao_compensation_ars,
            Some(DaoCompensationArs {
                deposit_ar: 10,
                withdraw_request_ar: 20,
            })
        );
    }

    #[test]
    fn live_cell_owner_resolves_same_batch_create_then_consume() {
        let mut owner = LiveCellOwner::default();
        owner
            .insert_created(sample_outpoint(), sample_created_slot(), None, None)
            .expect("insert");

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
            .insert_created(sample_outpoint(), sample_created_slot(), None, None)
            .expect("first insert");

        let err = owner
            .insert_created(sample_outpoint(), sample_created_slot(), None, None)
            .expect_err("duplicate outpoint must fail");

        assert!(err.to_string().contains(&format!(
            "outpoint=0x{}:0",
            hex::encode(sample_outpoint().tx_hash)
        )));
    }
}
