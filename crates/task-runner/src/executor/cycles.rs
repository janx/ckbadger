use anyhow::Result;
use ckbadger_common::CyclesBackfillConfig;
use ckbadger_store::CkbadgerStore;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    _store: &CkbadgerStore,
    task_id: Uuid,
    _config: &CyclesBackfillConfig,
) -> Result<()> {
    info!("Cycles backfill not yet implemented for RocksDB storage");
    db.complete_task(
        task_id,
        Some(serde_json::json!({"message": "Not yet implemented for RocksDB"})),
    )
    .await?;
    Ok(())
}
