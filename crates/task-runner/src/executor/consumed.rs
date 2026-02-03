use anyhow::Result;
use ckbadger_common::{ConsumedAtBackfillConfig, ConsumedAtBackfillResult};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &ConsumedAtBackfillConfig,
) -> Result<()> {
    info!("Starting consumed_at_backfill task");

    let total_blocks: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
        .fetch_one(pool)
        .await?;

    db.update_progress(task_id, 0, total_blocks, Some("Starting backfill"), None)
        .await?;

    let mut cells_updated: i64 = 0;
    let mut blocks_processed: i64 = 0;
    let mut current_block: i64 = 0;

    while current_block <= total_blocks {
        let end_block = (current_block + config.batch_size).min(total_blocks);

        let updated = sqlx::query_scalar::<_, i64>(
            r#"
            WITH input_consumption AS (
                SELECT 
                    ti.previous_tx_hash,
                    ti.previous_output_index,
                    ti.tx_block_number AS consumed_at_block,
                    ti.tx_hash AS consumed_by_tx,
                    ti.input_index AS consumed_at_index
                FROM transaction_inputs ti
                WHERE ti.tx_block_number >= $1 
                  AND ti.tx_block_number < $2
                  AND ti.previous_tx_hash IS NOT NULL
            )
            UPDATE cells c SET
                status = 1,
                consumed_at_block = ic.consumed_at_block,
                consumed_by_tx = ic.consumed_by_tx,
                consumed_at_index = ic.consumed_at_index
            FROM input_consumption ic
            WHERE c.tx_hash = ic.previous_tx_hash
              AND c.output_index = ic.previous_output_index
              AND c.consumed_at_block IS NULL
            RETURNING 1
            "#,
        )
        .bind(current_block)
        .bind(end_block)
        .fetch_all(pool)
        .await?
        .len() as i64;

        cells_updated += updated;
        blocks_processed = end_block;

        if updated > 0 {
            info!(
                "Backfilled blocks {}-{}: {} cells marked as consumed",
                current_block, end_block, updated
            );
        }

        db.update_progress(
            task_id,
            blocks_processed,
            total_blocks,
            Some(&format!(
                "Processed blocks {}-{}, {} cells updated",
                current_block, end_block, cells_updated
            )),
            None,
        )
        .await?;

        current_block = end_block;
    }

    let result = ConsumedAtBackfillResult {
        cells_updated,
        blocks_processed,
    };

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "consumed_at_backfill completed: {} cells updated across {} blocks",
        cells_updated, blocks_processed
    );

    Ok(())
}
