//! `ckbadger-store` — RocksDB-based embedded storage engine for ckbadger.
//!
//! Replaces PostgreSQL with RocksDB column families for all indexed/derived data.
//! The CKB node's own RocksDB (via `ckb-store-reader`) remains unchanged
//! for raw block/transaction data.
//!
//! # Usage
//!
//! ```no_run
//! use ckbadger_store::CkbadgerStore;
//!
//! // Primary (read-write) — used by indexer
//! let store = CkbadgerStore::open_domain("./data/ckbadger-store").unwrap();
//!
//! // Secondary (read-only) — used by API/TUI
//! let reader = CkbadgerStore::open_domain_secondary(
//!     "./data/ckbadger-store",
//!     "./data/ckbadger-store-secondary",
//! ).unwrap();
//! reader.refresh().unwrap();
//! ```

pub mod batch;
pub mod keys;
pub mod store;
pub mod types;

// Domain operation modules (impl blocks on CkbadgerStore)
mod activity_ops;
mod address_ops;
mod block_ops;
mod cell_ops;
mod cluster_ops;
mod dao_ops;
mod dotbit_ops;
mod identity_ops;
mod mnft_ops;
mod object_ops;
mod reorg_ops;
mod spore_ops;
mod stats_ops;
mod sync_ops;
mod token_ops;
mod tx_ops;
mod undo_log_ops;

pub use batch::StoreBatch;
pub use cell_ops::TokenCellStats;
pub use reorg_ops::RollbackResult;
pub use store::{
    known_append_only_secondary_store_paths, known_domain_secondary_store_paths,
    secondary_store_path, CkbadgerStore, MemoryProfile, SecondaryStoreOwner, StoreRuntimeConfig,
    ALL_CFS,
};
pub use store::{
    APPEND_CFS, CF_ACTIVITIES, CF_ADDR_BALANCE, CF_ADDR_TXS, CF_BLOCK_HASH_INDEX, CF_BLOCK_HEADERS,
    CF_CELLS, CF_CELL_BY_LOCK, CF_CELL_BY_LOCK_CODE, CF_CELL_BY_TYPE, CF_CELL_BY_TYPE_CODE,
    CF_CLUSTER_AGG, CF_CONSUMED_CELLS, CF_DAO_BY_BLOCK, CF_DAO_BY_LOCK_BLOCK,
    CF_DAO_BY_STATUS_BLOCK, CF_DAO_BY_WITHDRAW_TX, CF_DAO_DEPOSITS, CF_IDENTITY_DATA,
    CF_LIVE_CELLS, CF_OBJECT_COLLECTION_ACTIVITIES, CF_OBJECT_COLLECTION_AGG, CF_OBJECT_DATA,
    CF_REORG_UNDO_LOG_BY_BLOCK, CF_SCRIPT_INFO, CF_SPORE_BY_CLUSTER, CF_SPORE_DATA, CF_STATS_CHAIN,
    CF_STATS_DAO, CF_STATS_HODL, CF_STATS_OBJECT, CF_STATS_SCRIPT, CF_STATS_SPORE, CF_STATS_TOKEN,
    CF_SYNC_META, CF_TOKENS, CF_TOKEN_HOLDERS, CF_TX_HASH_MAP, CF_TX_INDEX, DOMAIN_CFS,
};
pub use types::*;
pub use undo_log_ops::UndoRollbackResult;
