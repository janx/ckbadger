#![allow(dead_code, unused_imports)]

use anyhow::Result;

// mod clickhouse_client;  // Wave 2: ClickHouse implementation
pub mod live_cell_storage;
mod repository;
mod writer;

// pub use clickhouse_client::{ClickHouseClient, ClickHouseConfig};  // Wave 2

#[derive(Clone, Default)]
pub struct DbPool;

#[derive(Clone, Default)]
pub struct CopyConfig {
    pub max_copy_connections: usize,
    pub copy_batch_size: usize,
    pub copy_enabled: bool,
}

#[derive(Clone, Default)]
pub struct CopyPoolManager;

impl CopyPoolManager {
    pub fn new(_database_url: &str, _config: CopyConfig) -> Result<Self> {
        Ok(Self)
    }
}

#[derive(Clone, Default)]
pub struct ParallelCopyRouter;

impl ParallelCopyRouter {
    pub fn with_live_cell_store(
        _pool_manager: CopyPoolManager,
        _live_cell_store: DynLiveCellStorage,
    ) -> Self {
        Self
    }

    pub async fn copy_activities_parallel<T>(&self, _data: &[T]) -> Result<()> {
        Ok(())
    }

    pub async fn copy_udt_cells_parallel<T>(&self, _data: &[T]) -> Result<()> {
        Ok(())
    }

    pub async fn copy_tx_block_map<T>(&self, _data: &[T]) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct CopyClient;

pub async fn copy_activities_batch<T>(_client: &CopyClient, _data: T) -> Result<()> {
    Ok(())
}

pub async fn copy_udt_cells<T>(_client: &CopyClient, _data: T) -> Result<()> {
    Ok(())
}

pub async fn copy_tx_block_map<T>(_client: &CopyClient, _data: T) -> Result<()> {
    Ok(())
}

pub use live_cell_storage::{
    CachedBlockHeader, ConsumedCellRecord, DaoDepositCacheEntry, DynLiveCellStorage,
    InMemoryLiveCellStore, LiveCellInfo, LiveCellStorage, LiveCellStorageAsync, MemoryStats,
};
pub use repository::{DeepForkInfo, Repository};
pub use writer::{BatchWriter, DaoWithdrawalContextTrait, ReorgResult, SecondaryIssuanceBreakdown};
