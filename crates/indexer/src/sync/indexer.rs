#![allow(dead_code)]

use std::sync::Arc;

use anyhow::Result;

use crate::cache::CacheInvalidator;
use crate::config::Config;
use crate::db::{DbPool, MemoryStats};

use super::SyncProgress;

pub struct Indexer {
    progress: Arc<SyncProgress>,
    cache_invalidator: CacheInvalidator,
    memory_stats: MemoryStats,
}

impl Indexer {
    pub async fn new(_config: Config, _pool: DbPool) -> Result<Self> {
        let cache_invalidator = CacheInvalidator::new(None).await;
        Ok(Self {
            progress: Arc::new(SyncProgress::new(0, 0)),
            cache_invalidator,
            memory_stats: MemoryStats::default(),
        })
    }

    pub fn progress(&self) -> Arc<SyncProgress> {
        Arc::clone(&self.progress)
    }

    pub fn cache_invalidator(&self) -> &CacheInvalidator {
        &self.cache_invalidator
    }

    pub fn get_memory_stats(&self) -> MemoryStats {
        self.memory_stats.clone()
    }

    pub fn is_bulk_sync_active(&self) -> bool {
        false
    }

    pub async fn run(&self) -> Result<()> {
        Ok(())
    }
}
