use crate::sync::types::InternId;

#[derive(Debug, Default)]
pub(crate) struct FactsArena {
    pub(crate) blocks: Vec<BlockFacts>,
    pub(crate) txs: Vec<TxFacts>,
    pub(crate) cells: Vec<CellFacts>,
}

#[derive(Debug, Default)]
pub(crate) struct BlockFacts;

#[derive(Debug, Default)]
pub(crate) struct TxFacts;

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
    pub(crate) lock_script_hash_id: InternId,
    pub(crate) lock_code_hash_id: InternId,
    pub(crate) type_script_hash_id: Option<InternId>,
    pub(crate) type_code_hash_id: Option<InternId>,
    pub(crate) occupied_capacity: i64,
    pub(crate) udt_amount: Option<u128>,
    pub(crate) semantic_tag: CellSemanticTag,
}

#[derive(Debug)]
pub(crate) struct ResolvedInputFacts {
    pub(crate) lock_script_hash_id: InternId,
    pub(crate) lock_code_hash_id: InternId,
    pub(crate) type_script_hash_id: Option<InternId>,
    pub(crate) type_code_hash_id: Option<InternId>,
}

#[derive(Debug)]
pub(crate) struct ResolvedTxFacts {
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
