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

#[derive(Debug)]
pub(crate) struct CellFacts {
    pub(crate) lock_script_hash_id: InternId,
    pub(crate) lock_code_hash_id: InternId,
    pub(crate) type_script_hash_id: Option<InternId>,
    pub(crate) type_code_hash_id: Option<InternId>,
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
