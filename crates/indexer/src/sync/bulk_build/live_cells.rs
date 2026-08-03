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
    protocol_heap_bytes: u64,
}

impl LiveCellOwner {
    pub(crate) fn live_count(&self) -> usize {
        self.live.len()
    }

    pub(crate) fn live_slots(&self) -> impl Iterator<Item = (&OutPointKey, &LiveCellSlot)> {
        self.live.iter()
    }

    pub(crate) fn estimated_bytes(&self) -> u64 {
        std::mem::size_of::<Self>() as u64
            + self.live.capacity() as u64
                * std::mem::size_of::<(OutPointKey, LiveCellSlot)>() as u64
            + self.extras.capacity() as u64
                * std::mem::size_of::<(OutPointKey, LiveCellExtras)>() as u64
            + self.protocol_facts.capacity() as u64
                * std::mem::size_of::<(OutPointKey, CellProtocolFacts)>() as u64
            + self.protocol_heap_bytes
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
            let heap_bytes = protocol_facts_heap_bytes(&facts);
            self.protocol_heap_bytes = self
                .protocol_heap_bytes
                .checked_add(heap_bytes)
                .ok_or_else(|| {
                    anyhow!(
                        "live protocol-facts heap accounting overflow: outpoint={} current_bytes={} added_bytes={}",
                        format_outpoint(&outpoint),
                        self.protocol_heap_bytes,
                        heap_bytes,
                    )
                })?;
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
        if let Some(facts) = facts.as_ref() {
            let heap_bytes = protocol_facts_heap_bytes(facts);
            self.protocol_heap_bytes = self
                .protocol_heap_bytes
                .checked_sub(heap_bytes)
                .ok_or_else(|| {
                    anyhow!(
                        "live protocol-facts heap accounting underflow: outpoint={} current_bytes={} removed_bytes={}",
                        format_outpoint(outpoint),
                        self.protocol_heap_bytes,
                        heap_bytes,
                    )
                })?;
        }

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
    pub(crate) data_hash: [u8; 32],
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
            udt_amount: cell.udt_amount,
            dao_state: cell.dao_state,
            dao_compensation_ars,
        };
        (extras.udt_amount.is_some()
            || extras.dao_state.is_some()
            || extras.dao_compensation_ars.is_some())
        .then_some(extras)
    }
}

impl LiveCellSlot {
    pub(crate) fn from_cell_facts(cell: &CellFacts) -> Result<Self> {
        let data_hash = cell.data_hash.ok_or_else(|| {
            anyhow!(
                "live cell is missing its canonical data hash: outpoint={}",
                format_outpoint(&cell.outpoint)
            )
        })?;
        Ok(Self {
            created_at_block: cell.created_at_block,
            created_by_block_dao_ar: cell.created_by_block_dao_ar,
            capacity: cell.capacity,
            occupied_capacity: cell.occupied_capacity,
            data_size: cell.data_size,
            data_hash,
            lock_script_hash_id: cell.lock_script_hash_id,
            lock_code_hash_id: cell.lock_code_hash_id,
            lock_hash_type: cell.lock_hash_type,
            lock_args_id: cell.lock_args_id,
            type_script_hash_id: cell.type_script_hash_id,
            type_code_hash_id: cell.type_code_hash_id,
            type_hash_type: cell.type_hash_type,
            type_args_id: cell.type_args_id,
            semantic_tag: cell.semantic_tag,
        })
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
            data_hash: Some(self.data_hash),
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

fn protocol_facts_heap_bytes(facts: &CellProtocolFacts) -> u64 {
    fn string_bytes(value: &Option<String>) -> u64 {
        value.as_ref().map_or(0, |value| value.capacity() as u64)
    }

    fn vec_bytes(value: &Option<Vec<u8>>) -> u64 {
        value.as_ref().map_or(0, |value| value.capacity() as u64)
    }

    match facts {
        CellProtocolFacts::Spore(facts) => {
            facts.content_type.capacity() as u64 + facts.content.capacity() as u64
        }
        CellProtocolFacts::DidCkb(facts) => facts.did_id.capacity() as u64,
        CellProtocolFacts::Cluster(facts) => {
            string_bytes(&facts.name) + string_bytes(&facts.description)
        }
        CellProtocolFacts::MnftIssuer(facts) => string_bytes(&facts.name) + vec_bytes(&facts.info),
        CellProtocolFacts::MnftClass(facts) => {
            facts.class_id.capacity() as u64
                + string_bytes(&facts.name)
                + string_bytes(&facts.description)
                + string_bytes(&facts.renderer)
        }
        CellProtocolFacts::MnftToken(facts) => {
            facts.token_id.capacity() as u64
                + facts.class_id.capacity() as u64
                + facts.characteristic.capacity() as u64
        }
        CellProtocolFacts::Dotbit(facts) => string_bytes(&facts.account),
        CellProtocolFacts::BitCell(facts) => facts.account.capacity() as u64,
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
    use crate::sync::bulk_build::facts::{CellFacts, CellSemanticTag, OutPointKey};
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
            data_hash: [0x44; 32],
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

    fn sample_plain_cell_facts() -> CellFacts {
        CellFacts {
            outpoint: sample_outpoint(),
            created_at_block: 14_000_000,
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
            data_hash: Some([0x44; 32]),
            udt_amount: None,
            semantic_tag: CellSemanticTag::Plain,
            dao_state: None,
            protocol_facts: None,
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
            std::mem::size_of::<LiveCellSlot>() <= 120,
            "LiveCellSlot grew to {} bytes",
            std::mem::size_of::<LiveCellSlot>()
        );
    }

    #[test]
    fn ordinary_cell_data_hash_does_not_allocate_sparse_extras() {
        let cell = sample_plain_cell_facts();

        assert_eq!(
            LiveCellExtras::from_cell_facts(&cell, None),
            None,
            "every CKB cell has a data hash; it belongs in the authoritative live slot, not the sparse side map"
        );
    }

    #[test]
    fn live_cell_without_canonical_data_hash_fails_with_outpoint_context() {
        let mut cell = sample_plain_cell_facts();
        cell.data_hash = None;

        let error = LiveCellSlot::from_cell_facts(&cell).unwrap_err();
        assert!(error.to_string().contains("canonical data hash"));
        assert!(error
            .to_string()
            .contains(&hex::encode(cell.outpoint.tx_hash)));
    }

    #[test]
    fn sparse_live_cell_extras_survive_resolution() {
        let mut owner = LiveCellOwner::default();
        let extras = LiveCellExtras {
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
