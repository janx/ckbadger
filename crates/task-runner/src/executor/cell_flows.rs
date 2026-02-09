use anyhow::Result;
use ckbadger_common::{CellFlowsRebuildConfig, CellFlowsRebuildResult};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &CellFlowsRebuildConfig,
) -> Result<()> {
    info!("Starting cell_flows_rebuild task");

    let total_blocks: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks_index")
        .fetch_one(pool)
        .await?;

    db.update_progress(
        task_id,
        0,
        total_blocks,
        Some("Truncating cell_flows table"),
        None,
    )
    .await?;

    sqlx::query("TRUNCATE cell_flows").execute(pool).await?;

    info!("Truncated cell_flows table, starting rebuild...");

    let mut flows_created: i64 = 0;
    let mut blocks_processed: i64 = 0;
    let mut current_block: i64 = 0;

    while current_block <= total_blocks {
        let end_block = (current_block + config.batch_size).min(total_blocks + 1);

        let batch_flows = rebuild_cell_flows_batch(pool, current_block, end_block).await?;
        flows_created += batch_flows;
        blocks_processed = end_block.min(total_blocks);

        if batch_flows > 0 {
            info!(
                "Rebuilt blocks {}-{}: {} flows created",
                current_block,
                end_block - 1,
                batch_flows
            );
        }

        db.update_progress(
            task_id,
            blocks_processed,
            total_blocks,
            Some(&format!(
                "Processed blocks {}-{}, {} total flows",
                current_block,
                end_block - 1,
                flows_created
            )),
            None,
        )
        .await?;

        current_block = end_block;
    }

    let result = CellFlowsRebuildResult {
        flows_created,
        blocks_processed,
    };

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "cell_flows_rebuild completed: {} flows created across {} blocks",
        flows_created, blocks_processed
    );

    Ok(())
}

async fn rebuild_cell_flows_batch(pool: &PgPool, start_block: i64, end_block: i64) -> Result<i64> {
    // Insert created flows (flow_type=0) from cells
    let created = sqlx::query(
        r#"
        INSERT INTO cell_flows (block_number, tx_hash, output_index, flow_type, lock_script_hash, capacity, data_size, consumed_by_tx)
        SELECT c.created_at_block, c.tx_hash, c.output_index, 0,
               c.lock_script_hash, c.capacity, c.data_size, NULL AS consumed_by_tx
        FROM cells c
        WHERE c.created_at_block >= $1 AND c.created_at_block < $2
        "#,
    )
    .bind(start_block)
    .bind(end_block)
    .execute(pool)
    .await?;

    // Insert consumed flows (flow_type=1) from transaction_inputs
    let consumed = sqlx::query(
        r#"
        INSERT INTO cell_flows (block_number, tx_hash, output_index, flow_type, lock_script_hash, capacity, data_size, consumed_by_tx)
        SELECT ti.tx_block_number, c.tx_hash, c.output_index, 1,
               c.lock_script_hash, c.capacity, c.data_size, ti.tx_hash AS consumed_by_tx
        FROM transaction_inputs ti
        JOIN tx_block_map tbm ON tbm.tx_hash = ti.previous_tx_hash
        JOIN cells c ON c.tx_hash = ti.previous_tx_hash
                    AND c.output_index = ti.previous_output_index
                    AND c.created_at_block = tbm.block_number
        WHERE ti.tx_block_number >= $1 AND ti.tx_block_number < $2
          AND ti.previous_tx_hash IS NOT NULL
        "#,
    )
    .bind(start_block)
    .bind(end_block)
    .execute(pool)
    .await?;

    Ok(created.rows_affected() as i64 + consumed.rows_affected() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

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
