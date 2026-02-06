use crate::cache::CacheInvalidator;
use crate::db::{DbPool, DynLiveCellStorage};

#[derive(Clone, Default)]
pub struct BatchWriter {
    pub(super) pool: DbPool,
    pub(super) fast_sync_mode: bool,
    pub(super) live_cell_store: Option<DynLiveCellStorage>,
    pub(super) cache_invalidator: Option<CacheInvalidator>,
}

impl BatchWriter {
    pub fn new(pool: DbPool) -> Self {
        Self {
            pool,
            fast_sync_mode: true,
            live_cell_store: None,
            cache_invalidator: None,
        }
    }

    pub fn with_fast_sync_mode(pool: DbPool, fast_sync_mode: bool) -> Self {
        Self {
            pool,
            fast_sync_mode,
            live_cell_store: None,
            cache_invalidator: None,
        }
    }

    pub fn with_live_cell_store(
        pool: DbPool,
        fast_sync_mode: bool,
        live_cell_store: DynLiveCellStorage,
        cache_invalidator: CacheInvalidator,
    ) -> Self {
        Self {
            pool,
            fast_sync_mode,
            live_cell_store: Some(live_cell_store),
            cache_invalidator: Some(cache_invalidator),
        }
    }

    pub fn cache_invalidator(&self) -> Option<&CacheInvalidator> {
        self.cache_invalidator.as_ref()
    }

    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    pub fn live_cell_store(&self) -> Option<&DynLiveCellStorage> {
        self.live_cell_store.as_ref()
    }
}
