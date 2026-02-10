use anyhow::Result;
use ckbadger_common::CellsStatusRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &CellsStatusRebuildConfig,
) -> Result<()> {
    info!(
        "cells_status rebuild is a no-op with RocksDB storage (consumed cells tracked by indexer)"
    );
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "No-op: consumed cells tracked by indexer in RocksDB"})),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ckbadger_common::{CellsStatusRebuildConfig, CellsStatusRebuildResult};

    #[test]
    fn test_default_config() {
        let config = CellsStatusRebuildConfig::default();
        assert_eq!(config.batch_size, 100_000);
    }

    #[test]
    fn test_config_custom_batch_size() {
        let config = CellsStatusRebuildConfig { batch_size: 50_000 };
        assert_eq!(config.batch_size, 50_000);
    }

    #[test]
    fn test_result_struct() {
        let result = CellsStatusRebuildResult {
            cells_updated: 12345,
            blocks_processed: 100,
        };
        assert_eq!(result.cells_updated, 12345);
        assert_eq!(result.blocks_processed, 100);
    }

    #[test]
    fn test_result_serialization() {
        let result = CellsStatusRebuildResult {
            cells_updated: 999,
            blocks_processed: 50,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["cellsUpdated"], 999);
        assert_eq!(json["blocksProcessed"], 50);
    }
}
