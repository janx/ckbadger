use anyhow::Result;
use ckbadger_common::StatisticsRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &StatisticsRebuildConfig,
) -> Result<()> {
    info!(
        "statistics rebuild is a no-op with RocksDB storage (statistics CF maintained by indexer)"
    );
    db.complete_task(
        task_id,
        Some(
            serde_json::json!({"message": "No-op: statistics CF maintained by indexer in RocksDB"}),
        ),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ckbadger_common::{StatisticsRebuildConfig, StatisticsRebuildResult};

    #[test]
    fn test_default_config_none_returns_all() {
        let config = StatisticsRebuildConfig { tables: None };
        assert!(config.tables.is_none());
    }

    #[test]
    fn test_result_default() {
        let result = StatisticsRebuildResult::default();
        assert!(result.completed_tables.is_empty());
        assert!(result.failed.is_empty());
        assert!(result.current_table.is_none());
    }
}
