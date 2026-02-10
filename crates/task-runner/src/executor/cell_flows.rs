use anyhow::Result;
use ckbadger_common::CellFlowsRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &CellFlowsRebuildConfig,
) -> Result<()> {
    info!(
        "cell_flows rebuild is a no-op with RocksDB storage (cell flow data maintained by indexer)"
    );
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "No-op: cell flow data maintained by indexer in RocksDB"})),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ckbadger_common::{CellFlowsRebuildConfig, CellFlowsRebuildResult};

    #[test]
    fn test_default_config() {
        let config = CellFlowsRebuildConfig::default();
        assert_eq!(config.batch_size, 100_000);
    }

    #[test]
    fn test_result_struct() {
        let result = CellFlowsRebuildResult {
            flows_created: 12345,
            blocks_processed: 100,
        };
        assert_eq!(result.flows_created, 12345);
        assert_eq!(result.blocks_processed, 100);
    }

    #[test]
    fn test_result_serialization() {
        let result = CellFlowsRebuildResult {
            flows_created: 999,
            blocks_processed: 50,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["flowsCreated"], 999);
        assert_eq!(json["blocksProcessed"], 50);
    }
}
