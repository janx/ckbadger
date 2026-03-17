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

#[derive(Debug, Default)]
pub(crate) struct CellFacts;

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
