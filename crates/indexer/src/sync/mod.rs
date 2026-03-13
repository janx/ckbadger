mod adaptive;
mod batch;
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
mod undo;

pub use indexer::Indexer;
pub use progress::SyncProgress;

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
