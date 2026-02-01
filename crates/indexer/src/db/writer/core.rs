use anyhow::Result;
use sqlx::{PgPool, Postgres, Transaction};

use crate::cache::CacheInvalidator;
use crate::db::DynLiveCellStorage;

#[derive(Clone)]
pub struct BatchWriter {
    pub(super) pool: PgPool,
    pub(super) fast_sync_mode: bool,
    pub(super) live_cell_store: Option<DynLiveCellStorage>,
    pub(super) cache_invalidator: Option<CacheInvalidator>,
}

impl BatchWriter {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            fast_sync_mode: true,
            live_cell_store: None,
            cache_invalidator: None,
        }
    }

    pub fn with_fast_sync_mode(pool: PgPool, fast_sync_mode: bool) -> Self {
        Self {
            pool,
            fast_sync_mode,
            live_cell_store: None,
            cache_invalidator: None,
        }
    }

    pub fn with_live_cell_store(
        pool: PgPool,
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

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn live_cell_store(&self) -> Option<&DynLiveCellStorage> {
        self.live_cell_store.as_ref()
    }

    pub async fn begin_transaction(&self) -> Result<Transaction<'_, Postgres>> {
        let mut tx = self.pool.begin().await?;
        if self.fast_sync_mode {
            sqlx::query("SET LOCAL synchronous_commit = off")
                .execute(&mut *tx)
                .await?;
        }
        Ok(tx)
    }
}
