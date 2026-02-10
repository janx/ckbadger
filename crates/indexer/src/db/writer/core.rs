use std::sync::Arc;

use ckbadger_store::CkbadgerStore;

use crate::cache::CacheInvalidator;

#[derive(Clone)]
pub struct BatchWriter {
    pub(super) store: Arc<CkbadgerStore>,
    pub(super) cache_invalidator: Option<CacheInvalidator>,
}

impl BatchWriter {
    pub fn new(store: Arc<CkbadgerStore>) -> Self {
        Self {
            store,
            cache_invalidator: None,
        }
    }

    pub fn with_fast_sync_mode(store: Arc<CkbadgerStore>, _fast_sync_mode: bool) -> Self {
        Self {
            store,
            cache_invalidator: None,
        }
    }

    pub fn with_cache(
        store: Arc<CkbadgerStore>,
        _fast_sync_mode: bool,
        cache_invalidator: CacheInvalidator,
    ) -> Self {
        Self {
            store,
            cache_invalidator: Some(cache_invalidator),
        }
    }

    pub fn cache_invalidator(&self) -> Option<&CacheInvalidator> {
        self.cache_invalidator.as_ref()
    }

    pub fn store(&self) -> &Arc<CkbadgerStore> {
        &self.store
    }
}
