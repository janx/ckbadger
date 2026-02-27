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
//! let store = CkbadgerStore::open("./data/ckbadger-store").unwrap();
//!
//! // Secondary (read-only) — used by API/TUI
//! let reader = CkbadgerStore::open_secondary(
//!     "./data/ckbadger-store",
//!     "./data/ckbadger-store-secondary",
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
pub use store::{CkbadgerStore, ALL_CFS};
pub use store::{
    CF_ACTIVITIES, CF_ADDR_BALANCE, CF_ADDR_TXS, CF_BLOCK_HASH_INDEX, CF_BLOCK_HEADERS,
    CF_BLOCK_ISSUANCE, CF_CELL_BY_LOCK, CF_CELL_BY_LOCK_CODE, CF_CELL_BY_TYPE,
    CF_CELL_BY_TYPE_CODE, CF_CLUSTER_AGG, CF_CONSUMED_CELLS, CF_DAO_BY_WITHDRAW_TX,
    CF_DAO_DEPOSITS, CF_LIVE_CELLS, CF_NFT_COLLECTION_ACTIVITIES, CF_NFT_COLLECTION_AGG,
    CF_NFT_DATA, CF_SCRIPT_INFO, CF_SPORE_BY_CLUSTER, CF_SPORE_DATA, CF_STATS, CF_SYNC_META,
    CF_TOKENS, CF_TOKEN_HOLDERS, CF_TX_HASH_MAP, CF_TX_INDEX,
};
pub use types::*;
