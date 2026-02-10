use anyhow::Result;
use ckbadger_common::ActivitiesRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &ActivitiesRebuildConfig,
) -> Result<()> {
    info!(
        "activities rebuild is a no-op with RocksDB storage (activities CF maintained by indexer)"
    );
    db.complete_task(
        task_id,
        Some(
            serde_json::json!({"message": "No-op: activities CF maintained by indexer in RocksDB"}),
        ),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ckbadger_common::{ActivitiesRebuildConfig, ActivitiesRebuildResult};

    #[test]
    fn test_default_config() {
        let config = ActivitiesRebuildConfig::default();
        assert_eq!(config.batch_size, 10_000);
    }

    #[test]
    fn test_result_struct() {
        let result = ActivitiesRebuildResult {
            activities_created: 12345,
            blocks_processed: 100,
        };
        assert_eq!(result.activities_created, 12345);
        assert_eq!(result.blocks_processed, 100);
    }

    #[test]
    fn test_result_serialization() {
        let result = ActivitiesRebuildResult {
            activities_created: 999,
            blocks_processed: 50,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["activitiesCreated"], 999);
        assert_eq!(json["blocksProcessed"], 50);
    }
}
