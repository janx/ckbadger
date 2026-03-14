#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::manual_is_multiple_of)]

use std::sync::Arc;

use ckbadger_store::CkbadgerStore;

use crate::cache::CacheInvalidator;

#[derive(Clone)]
pub struct BatchWriter {
    pub(super) store: Arc<CkbadgerStore>,
    pub(super) append_only_store: Arc<CkbadgerStore>,
    pub(super) cache_invalidator: Option<CacheInvalidator>,
}

impl BatchWriter {
    pub fn new(store: Arc<CkbadgerStore>, append_only_store: Arc<CkbadgerStore>) -> Self {
        Self {
            store,
            append_only_store,
            cache_invalidator: None,
        }
    }

    pub fn with_cache(
        store: Arc<CkbadgerStore>,
        append_only_store: Arc<CkbadgerStore>,
        cache_invalidator: CacheInvalidator,
    ) -> Self {
        Self {
            store,
            append_only_store,
            cache_invalidator: Some(cache_invalidator),
        }
    }

    pub fn cache_invalidator(&self) -> Option<&CacheInvalidator> {
        self.cache_invalidator.as_ref()
    }

    pub fn store(&self) -> &Arc<CkbadgerStore> {
        &self.store
    }

    pub fn append_only_store(&self) -> &CkbadgerStore {
        &self.append_only_store
    }
}

pub mod activities;
mod addresses;
pub(crate) mod cell_distribution;
mod cells;
mod chain;
mod dao;
pub(crate) mod dotbit;
pub(crate) mod fiber;
pub(crate) mod fiber_detector;
pub mod hodl_wave;
mod mnft;
pub(crate) mod nft_activity_acc;
mod reorg;
pub(crate) mod rgbpp_detector;
mod spore;
pub(crate) mod stablepp_detector;
mod statistics;
mod sync;
mod udt;
pub(crate) mod utxoswap_detector;

pub use dao::{DaoWithdrawalContext, DaoWithdrawalContextTrait};
pub use reorg::ReorgResult;
pub use statistics::DaoSnapshotInput;
