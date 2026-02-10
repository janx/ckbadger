use anyhow::Result;
use ckbadger_common::TxBlockMapRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &TxBlockMapRebuildConfig,
) -> Result<()> {
    info!("tx_block_map rebuild is a no-op with RocksDB storage (tx_hash_map CF maintained by indexer)");
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "No-op: tx_hash_map CF maintained by indexer in RocksDB"})),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ckbadger_common::{TxBlockMapRebuildConfig, TxBlockMapRebuildResult};

    #[test]
    fn test_default_config() {
        let config = TxBlockMapRebuildConfig::default();
        assert!(config._reserved.is_none());
    }

    #[test]
    fn test_result_struct() {
        let result = TxBlockMapRebuildResult {
            rows_inserted: 12345,
        };
        assert_eq!(result.rows_inserted, 12345);
    }

    #[test]
    fn test_result_serialization() {
        let result = TxBlockMapRebuildResult { rows_inserted: 999 };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["rowsInserted"], 999);
    }
}
