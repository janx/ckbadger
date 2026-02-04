use anyhow::Result;
use ckbadger_common::{CellsStatusRebuildConfig, CellsStatusRebuildResult, RateCalculator};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

/// Rebuilds cells.status and consumed_at_* fields from transaction_inputs.
///
/// This task consolidates the former `cells_status_rebuild` and `consumed_at_backfill`
/// tasks into a single pass. It handles:
/// - Cells with status=0 that need to be marked as consumed (status=1)
/// - Cells with NULL consumed_at_block that need backfilling
///
/// The unified WHERE clause `(c.status = 0 OR c.consumed_at_block IS NULL)` ensures
/// both cases are handled efficiently in a single scan per batch.
pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &CellsStatusRebuildConfig,
) -> Result<()> {
    info!("Starting cells_status_rebuild task (unified with consumed_at_backfill)");

    let total_blocks: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
        .fetch_one(pool)
        .await?;

    db.update_progress(task_id, 0, total_blocks, Some("Starting rebuild"), None)
        .await?;

    let mut cells_updated: i64 = 0;
    let mut blocks_processed: i64 = 0;
    let mut current_block: i64 = 0;
    let mut rate_calc = RateCalculator::default();

    while current_block <= total_blocks {
        let end_block = (current_block + config.batch_size).min(total_blocks);

        // Unified query: handles both status=0 cells AND cells with NULL consumed_at_block
        // This replaces the need for separate cells_status_rebuild and consumed_at_backfill tasks
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
              AND (c.status = 0 OR c.consumed_at_block IS NULL)
            RETURNING 1::BIGINT
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
                "Rebuilt blocks {}-{}: {} cells marked as consumed",
                current_block, end_block, updated
            );
        }

        rate_calc.add_sample(blocks_processed);
        db.update_progress(
            task_id,
            blocks_processed,
            total_blocks,
            Some(&format!(
                "Processed blocks {}-{}, {} cells updated",
                current_block, end_block, cells_updated
            )),
            rate_calc.rate(),
        )
        .await?;

        current_block = end_block;
    }

    let result = CellsStatusRebuildResult {
        cells_updated,
        blocks_processed,
    };

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "cells_status_rebuild completed: {} cells updated across {} blocks",
        cells_updated, blocks_processed
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
