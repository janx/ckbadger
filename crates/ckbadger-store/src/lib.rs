//! `ckbadger-store` — RocksDB-based embedded storage engine for ckbadger.
//!
//! Two physical RocksDB instances:
//! - **default store** (mutable): indices, aggregates, sync meta — rollback via range-delete
//! - **append store** (immutable): cells, tx_meta, block_meta, activities — never deleted
//!
//! # Usage
//!
//! ```no_run
//! use ckbadger_store::CkbadgerStore;
//!
//! // Primary (read-write) — used by indexer
//! let store = CkbadgerStore::open(
//!     "./data/ckbadger-store",
//!     "./data/ckbadger-store-append",
//! ).unwrap();
//!
//! // Secondary (read-only) — used by API/TUI
//! let reader = CkbadgerStore::open_secondary(
//!     "./data/ckbadger-store",
//!     "./data/ckbadger-store-secondary",
//!     "./data/ckbadger-store-append",
//!     "./data/ckbadger-store-append-secondary",
//! ).unwrap();
//! reader.refresh().unwrap();
//! ```

pub mod batch;
pub mod keys;
pub mod pagination;
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
mod mnft_ops;
mod nft_ops;
mod reorg_ops;
mod spore_ops;
mod stats_ops;
mod sync_ops;
mod token_ops;
mod tx_ops;

pub use batch::StoreBatch;
pub use cell_ops::TokenCellStats;
pub use pagination::PaginatedResult;
pub use reorg_ops::RollbackResult;
pub use store::{CkbadgerStore, ALL_CFS, APPEND_CFS, DEFAULT_CFS};

// New canonical CF constants
pub use store::{
    CF_ACTIVITIES, CF_ADDR_ACTIVITIES, CF_ADDR_STATS, CF_ADDR_TXS, CF_ASSET_META, CF_BLOCK_INDEX,
    CF_BLOCK_ISSUANCE, CF_BLOCK_META, CF_CELLS, CF_CONSUMED_CELLS, CF_DAO_DEPOSITS,
    CF_DAO_WITHDRAW_INDEX, CF_FT_ACTIVITIES, CF_FT_HOLDERS, CF_FT_INDEX, CF_FT_OUTPOINTS,
    CF_FT_STATS, CF_LIVE_CELLS, CF_LIVE_CELLS_BY_LOCK, CF_LIVE_CELLS_BY_LOCK_CODE,
    CF_LIVE_CELLS_BY_TYPE, CF_LIVE_CELLS_BY_TYPE_CODE, CF_NFT_COLLECTION_ACTIVITIES,
    CF_NFT_COLLECTION_STATS, CF_NFT_ITEM_BY_COLLECTION, CF_NFT_ITEM_INDEX, CF_NFT_ITEM_META,
    CF_NFT_OUTPOINTS, CF_STATS, CF_TX_INDEX, CF_TX_META,
};

// Legacy CF aliases (kept for backward compatibility during migration)
pub use store::{
    CF_ADDR_BALANCE, CF_ADDR_DAILY_STATS, CF_BLOCK_HASH_INDEX, CF_BLOCK_HEADERS, CF_CELL_BY_LOCK,
    CF_CELL_BY_LOCK_CODE, CF_CELL_BY_TYPE, CF_CELL_BY_TYPE_CODE, CF_CLUSTER_AGG,
    CF_DAO_BY_WITHDRAW_TX, CF_NFT_COLLECTION_AGG, CF_NFT_DATA, CF_SCRIPT_INFO, CF_SPORE_BY_CLUSTER,
    CF_SPORE_DATA, CF_SYNC_META, CF_TOKENS, CF_TOKEN_HOLDERS, CF_TOKEN_TRANSFERS, CF_TX_HASH_MAP,
};

pub use types::*;
