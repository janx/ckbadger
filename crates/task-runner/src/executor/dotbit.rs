use anyhow::Result;
use ckbadger_common::DotbitRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &DotbitRebuildConfig,
) -> Result<()> {
    info!("dotbit rebuild is a no-op with RocksDB storage (NFT CFs maintained by indexer)");
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "No-op: NFT CFs maintained by indexer in RocksDB"})),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DotbitRebuildConfig::default();
        assert_eq!(config.batch_size, 10_000);
    }
}
