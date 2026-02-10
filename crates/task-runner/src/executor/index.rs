use anyhow::Result;
use ckbadger_common::IndexRebuildConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &IndexRebuildConfig,
) -> Result<()> {
    info!("Index rebuild is a no-op with RocksDB storage (no deferred indexes)");
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "No-op: RocksDB has no deferred indexes"})),
    )
    .await?;
    Ok(())
}
