//! In-memory mirror of the local node's transaction pool.
//!
//! Why it exists: a transaction sitting in the node's pool is invisible to
//! every store-backed endpoint, so a brand-new address receiving its first
//! payment cannot be found by search and shows nothing on its page. The mirror
//! makes that transaction visible — interpreted by the same activity builder
//! the indexer runs, and labelled as the provisional state it is.
//!
//! What it is not: a store. Per `docs/prompts/WORLD_VIEW.md`, common knowledge
//! is state verified by global consensus; pool state is not. Nothing in this
//! module writes to the domain, append-only or network stores, and no pool
//! value ever enters a balance, a holder list or a statistic. After a restart
//! the mirror is rebuilt by re-observing the node.

pub mod mirror;
pub mod refresh;
pub mod resolve;
pub mod snapshot;
pub mod source;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

pub use mirror::{
    interpretation_of, PoolMirror, PoolRefresher, PoolRefresherConfig, RefreshOutcome,
};
pub use refresh::{refresh_pool_mirror_loop, POOL_MIRROR_TASK};
pub use resolve::{
    resolve_pool_tx, resolve_previous_outputs, NoPoolParents, OutPointKey, PoolParentCells,
    ResolvedCell, ResolvedInput, ResolvedPoolTx, TX_FETCH_CONCURRENCY,
};
pub use snapshot::{
    pool_timestamp_rfc3339, Interpretation, InterpretationReasonResponse, InterpretationResponse,
    MirrorStatus, PartialReason, PoolEntryError, PoolLockScript, PoolParticipant, PoolSnapshot,
    PoolStatus, PoolSummaryResponse, PoolTxRecord,
};
pub use source::{
    HttpPoolSource, NodeHeader, NodeTxStatus, PoolEntryMeta, PoolSource, PoolTxLookup, RawTxPool,
    TxPoolInfo,
};
