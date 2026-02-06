use anyhow::Result;
use ckbadger_common::{TxBlockMapRebuildConfig, TxBlockMapRebuildResult};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    _config: &TxBlockMapRebuildConfig,
) -> Result<()> {
    info!("Starting tx_block_map_rebuild task");

    let total_txs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
        .fetch_one(pool)
        .await?;

    db.update_progress(
        task_id,
        0,
        total_txs,
        Some("Creating new tx_block_map table"),
        None,
    )
    .await?;

    sqlx::query("DROP TABLE IF EXISTS tx_block_map_new")
        .execute(pool)
        .await?;

    db.update_progress(
        task_id,
        0,
        total_txs,
        Some("Copying data from transactions"),
        None,
    )
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE tx_block_map_new AS 
        SELECT hash AS tx_hash, block_number FROM transactions
        "#,
    )
    .execute(pool)
    .await?;

    db.update_progress(
        task_id,
        total_txs / 2,
        total_txs,
        Some("Building primary key"),
        None,
    )
    .await?;

    sqlx::query("ALTER TABLE tx_block_map_new ADD PRIMARY KEY (tx_hash)")
        .execute(pool)
        .await?;

    db.update_progress(
        task_id,
        total_txs * 3 / 4,
        total_txs,
        Some("Building block_number index"),
        None,
    )
    .await?;

    sqlx::query("CREATE INDEX idx_tx_block_map_new_block ON tx_block_map_new(block_number)")
        .execute(pool)
        .await?;

    db.update_progress(task_id, total_txs, total_txs, Some("Swapping tables"), None)
        .await?;

    let mut tx = pool.begin().await?;

    sqlx::query("DROP TABLE IF EXISTS tx_block_map")
        .execute(&mut *tx)
        .await?;

    sqlx::query("ALTER TABLE tx_block_map_new RENAME TO tx_block_map")
        .execute(&mut *tx)
        .await?;

    sqlx::query("ALTER INDEX idx_tx_block_map_new_block RENAME TO idx_tx_block_map_block")
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    sqlx::query(
        "UPDATE sync_status SET tx_block_map_deferred = FALSE, tx_block_map_rebuild_completed_at = NOW() WHERE id = 1",
    )
    .execute(pool)
    .await?;

    let result = TxBlockMapRebuildResult {
        rows_inserted: total_txs,
    };

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "tx_block_map_rebuild completed: {} rows inserted",
        total_txs
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
