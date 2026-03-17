use std::ops::Range;

use crate::sync::types::InternId;

#[derive(Debug, Default)]
pub(crate) struct FactsArena {
    pub(crate) blocks: Vec<BlockFacts>,
    pub(crate) txs: Vec<TxFacts>,
    pub(crate) cells: Vec<CellFacts>,
}

#[derive(Debug, Default)]
pub(crate) struct BlockFacts {
    pub(crate) number: i64,
    pub(crate) tx_range: Range<usize>,
}

#[derive(Debug, Default)]
pub(crate) struct TxFacts {
    pub(crate) hash: [u8; 32],
    pub(crate) block_number: i64,
    pub(crate) tx_index: i32,
    pub(crate) is_cellbase: bool,
    pub(crate) input_outpoints: Vec<OutPointKey>,
    pub(crate) output_range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) struct OutPointKey {
    pub(crate) tx_hash: [u8; 32],
    pub(crate) index: u32,
}

impl OutPointKey {
    pub(crate) const fn new(tx_hash: [u8; 32], index: u32) -> Self {
        Self { tx_hash, index }
    }
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellSemanticTag {
    Plain,
    Dao,
    Sudt,
    Xudt,
    Dotbit,
    Mnft,
    Spore,
    Cluster,
}

#[derive(Debug)]
pub(crate) struct CellFacts {
    pub(crate) outpoint: OutPointKey,
    pub(crate) created_at_block: i64,
    pub(crate) capacity: i64,
    pub(crate) lock_script_hash_id: InternId,
    pub(crate) lock_code_hash_id: InternId,
    pub(crate) type_script_hash_id: Option<InternId>,
    pub(crate) type_code_hash_id: Option<InternId>,
    pub(crate) occupied_capacity: i64,
    pub(crate) data_size: i32,
    pub(crate) udt_amount: Option<u128>,
    pub(crate) semantic_tag: CellSemanticTag,
}

#[derive(Debug)]
pub(crate) struct ResolvedInputFacts {
    pub(crate) outpoint: OutPointKey,
    pub(crate) created_at_block: i64,
    pub(crate) capacity: i64,
    pub(crate) occupied_capacity: i64,
    pub(crate) data_size: i32,
    pub(crate) udt_amount: Option<u128>,
    pub(crate) lock_script_hash_id: InternId,
    pub(crate) lock_code_hash_id: InternId,
    pub(crate) type_script_hash_id: Option<InternId>,
    pub(crate) type_code_hash_id: Option<InternId>,
    pub(crate) semantic_tag: CellSemanticTag,
}

#[derive(Debug)]
pub(crate) struct ResolvedTxFacts {
    pub(crate) tx_hash: [u8; 32],
    pub(crate) block_number: i64,
    pub(crate) tx_index: i32,
    pub(crate) resolved_inputs: Vec<ResolvedInputFacts>,
    pub(crate) cells: Vec<CellFacts>,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactsArenaSnapshot {
    pub tx_count: usize,
    pub cells: Vec<CellFactsSnapshot>,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellFactsSnapshot {
    pub occupied_capacity: i64,
    pub udt_amount: Option<u128>,
    pub semantic_tag: CellSemanticTag,
}

impl FactsArenaSnapshot {
    pub(crate) fn from_facts_arena(arena: &FactsArena) -> Self {
        Self {
            tx_count: arena.txs.len(),
            cells: arena
                .cells
                .iter()
                .map(|cell| CellFactsSnapshot {
                    occupied_capacity: cell.occupied_capacity,
                    udt_amount: cell.udt_amount,
                    semantic_tag: cell.semantic_tag,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_arena_defaults_to_empty_indexes() {
        let arena = FactsArena::default();
        assert!(arena.blocks.is_empty());
        assert!(arena.txs.is_empty());
        assert!(arena.cells.is_empty());
    }
}
