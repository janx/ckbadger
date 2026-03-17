mod adaptive;
mod batch;
pub(crate) mod dao_helpers;
mod diagnostics;
mod helpers;
mod indexer;
pub(crate) mod bulk_build;
mod nft_helpers;
mod pipeline;
mod progress;
mod reorg;
mod sync_mode;
mod token_helpers;
pub(crate) mod types;
pub(crate) mod undo;

#[doc(hidden)]
pub use bulk_build::facts::{CellFactsSnapshot, CellSemanticTag, FactsArenaSnapshot};
#[doc(hidden)]
pub use bulk_build::live_cells::{
    LiveCellResolutionSnapshot, ResolvedInputSnapshot, ResolvedTxSnapshot,
};
pub use indexer::Indexer;
pub use progress::SyncProgress;

#[doc(hidden)]
pub fn build_facts_arena_snapshot_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<FactsArenaSnapshot> {
    let mut interner = bulk_build::interner::IdentityInterner::default();
    let arena = pipeline::build_bulk_facts_arena_from_blocks(blocks, &mut interner)?;
    Ok(FactsArenaSnapshot::from_facts_arena(&arena))
}

#[doc(hidden)]
pub fn resolve_live_cell_snapshot_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<LiveCellResolutionSnapshot> {
    let mut interner = bulk_build::interner::IdentityInterner::default();
    let arena = pipeline::build_bulk_facts_arena_from_blocks(blocks, &mut interner)?;
    bulk_build::live_cells::resolve_live_cell_snapshot_for_test(&arena)
}

/// Convert transactions_count (i32) to usize, failing if negative.
pub(crate) fn checked_tx_count(count: i32, block_number: i64) -> anyhow::Result<usize> {
    usize::try_from(count).map_err(|_| {
        anyhow::anyhow!(
            "negative transactions_count {} at block {}",
            count,
            block_number
        )
    })
}
