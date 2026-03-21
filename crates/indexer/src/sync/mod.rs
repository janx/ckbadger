mod adaptive;
mod batch;
pub(crate) mod bulk_build;
pub(crate) mod dao_helpers;
mod diagnostics;
mod helpers;
mod indexer;
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
#[doc(hidden)]
pub use bulk_build::materialize::MaterializationReport;
pub use indexer::Indexer;
#[doc(hidden)]
pub use indexer::StartupSyncPathSnapshot;
pub use progress::SyncProgress;

#[doc(hidden)]
pub fn build_facts_arena_snapshot_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<FactsArenaSnapshot> {
    let interner = bulk_build::interner::IdentityInterner::default();
    let (arena, _) = pipeline::build_bulk_facts_arena_from_blocks(blocks, &interner)?;
    Ok(FactsArenaSnapshot::from_facts_arena(&arena))
}

#[doc(hidden)]
pub fn resolve_live_cell_snapshot_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<LiveCellResolutionSnapshot> {
    let interner = bulk_build::interner::IdentityInterner::default();
    let (arena, _) = pipeline::build_bulk_facts_arena_from_blocks(blocks, &interner)?;
    bulk_build::live_cells::resolve_live_cell_snapshot_for_test(&arena)
}

#[doc(hidden)]
pub fn run_sample_bulk_materialization_for_test() -> anyhow::Result<MaterializationReport> {
    bulk_build::materialize::run_sample_bulk_materialization_for_test()
}

#[doc(hidden)]
pub fn materialize_address_balances_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<std::collections::HashMap<Vec<u8>, ckbadger_store::AddressBalance>> {
    bulk_build::owners::address::materialize_address_balances_for_test(blocks)
}

#[doc(hidden)]
pub fn materialize_script_infos_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<std::collections::HashMap<Vec<u8>, ckbadger_store::ScriptInfo>> {
    bulk_build::owners::script::materialize_script_infos_for_test(blocks)
}

#[doc(hidden)]
pub use bulk_build::owners::dao::DaoStateSnapshot;
#[doc(hidden)]
pub use bulk_build::owners::token::TokenStateSnapshot;

#[doc(hidden)]
pub fn materialize_token_state_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<TokenStateSnapshot> {
    bulk_build::owners::token::materialize_token_state_for_test(blocks)
}

#[doc(hidden)]
pub fn materialize_dao_state_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<DaoStateSnapshot> {
    bulk_build::owners::dao::materialize_dao_state_for_test(blocks)
}

#[doc(hidden)]
pub use bulk_build::owners::object::ObjectStateSnapshot;
#[doc(hidden)]
pub use bulk_build::{BulkArtifactSnapshot, CoreOwnerStateSnapshot};

#[doc(hidden)]
pub fn materialize_object_state_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<ObjectStateSnapshot> {
    bulk_build::owners::object::materialize_object_state_for_test(blocks)
}

#[doc(hidden)]
pub fn materialize_core_owner_state_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<CoreOwnerStateSnapshot> {
    bulk_build::materialize_core_owner_state_for_test(blocks)
}

#[doc(hidden)]
pub fn materialize_bulk_artifacts_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
) -> anyhow::Result<BulkArtifactSnapshot> {
    bulk_build::materialize_bulk_artifacts_for_test(blocks)
}

#[doc(hidden)]
pub fn materialize_bulk_stage_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
    chain_tip: u64,
    bulk_sync_threshold: u64,
) -> anyhow::Result<BulkArtifactSnapshot> {
    bulk_build::materialize_bulk_stage_for_test(blocks, chain_tip, bulk_sync_threshold)
}

#[doc(hidden)]
pub fn materialize_bulk_stage_then_complete_sync_status_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
    chain_tip: u64,
    bulk_sync_threshold: u64,
) -> anyhow::Result<ckbadger_store::SyncStatus> {
    bulk_build::materialize_bulk_stage_then_complete_sync_status_for_test(
        blocks,
        chain_tip,
        bulk_sync_threshold,
    )
}

#[doc(hidden)]
pub fn simulate_startup_sync_path_for_test(
    blocks: &[crate::rpc::BlockResponseWithCycles],
    chain_tip: u64,
    bulk_sync_threshold: u64,
    sync_tip_block: i64,
    sync_tip_hash: Option<Vec<u8>>,
) -> anyhow::Result<StartupSyncPathSnapshot> {
    indexer::simulate_startup_sync_path_for_test(
        blocks,
        chain_tip,
        bulk_sync_threshold,
        sync_tip_block,
        sync_tip_hash,
    )
}

#[doc(hidden)]
pub fn materialize_bulk_artifacts_from_batches_for_test(
    batches: &[Vec<crate::rpc::BlockResponseWithCycles>],
) -> anyhow::Result<BulkArtifactSnapshot> {
    bulk_build::materialize_bulk_artifacts_from_batches_for_test(batches)
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
