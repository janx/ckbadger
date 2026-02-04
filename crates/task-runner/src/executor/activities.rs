use anyhow::Result;
use ckbadger_common::{ActivitiesRebuildConfig, ActivitiesRebuildResult};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &ActivitiesRebuildConfig,
) -> Result<()> {
    info!("Starting activities_rebuild task");

    let total_blocks: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
        .fetch_one(pool)
        .await?;

    db.update_progress(
        task_id,
        0,
        total_blocks,
        Some("Truncating activities table"),
        None,
    )
    .await?;

    sqlx::query("TRUNCATE activities").execute(pool).await?;

    info!("Truncated activities table, starting rebuild...");

    db.update_progress(
        task_id,
        0,
        total_blocks,
        Some("Starting activity parsing"),
        None,
    )
    .await?;

    let mut activities_created: i64 = 0;
    let mut blocks_processed: i64 = 0;
    let mut current_block: i64 = 0;

    while current_block <= total_blocks {
        let end_block = (current_block + config.batch_size).min(total_blocks + 1);

        let batch_activities = rebuild_activities_batch(pool, current_block, end_block).await?;
        activities_created += batch_activities;
        blocks_processed = end_block.min(total_blocks);

        if batch_activities > 0 {
            info!(
                "Rebuilt blocks {}-{}: {} activities created",
                current_block,
                end_block - 1,
                batch_activities
            );
        }

        db.update_progress(
            task_id,
            blocks_processed,
            total_blocks,
            Some(&format!(
                "Processed blocks {}-{}, {} total activities",
                current_block,
                end_block - 1,
                activities_created
            )),
            None,
        )
        .await?;

        current_block = end_block;
    }

    sqlx::query("UPDATE sync_status SET activities_deferred = FALSE, activities_rebuild_completed_at = NOW() WHERE id = 1")
        .execute(pool)
        .await?;

    let result = ActivitiesRebuildResult {
        activities_created,
        blocks_processed,
    };

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "activities_rebuild completed: {} activities created across {} blocks",
        activities_created, blocks_processed
    );

    Ok(())
}

async fn rebuild_activities_batch(pool: &PgPool, start_block: i64, end_block: i64) -> Result<i64> {
    let inserted = sqlx::query_scalar::<_, i64>(
        r#"
        WITH block_txs AS (
            SELECT 
                t.hash AS tx_hash,
                t.block_number,
                t.tx_index,
                t.is_cellbase,
                t.total_input_capacity,
                t.total_output_capacity,
                t.fee,
                b.timestamp
            FROM transactions t
            JOIN blocks b ON b.number = t.block_number
            WHERE t.block_number >= $1 AND t.block_number < $2
        ),
        tx_outputs AS (
            SELECT 
                c.tx_hash,
                c.output_index,
                c.capacity,
                c.lock_script_hash,
                c.type_script_hash,
                c.created_at_block
            FROM cells c
            WHERE c.created_at_block >= $1 AND c.created_at_block < $2
        ),
        tx_inputs AS (
            SELECT 
                ti.tx_hash,
                ti.tx_block_number,
                ti.input_index,
                c.capacity AS input_capacity,
                c.lock_script_hash AS input_lock_hash,
                c.type_script_hash AS input_type_hash
            FROM transaction_inputs ti
            JOIN cells c ON c.tx_hash = ti.previous_tx_hash 
                        AND c.output_index = ti.previous_output_index
            WHERE ti.tx_block_number >= $1 AND ti.tx_block_number < $2
        ),
        ckb_transfers AS (
            SELECT 
                bt.block_number,
                bt.tx_hash,
                bt.tx_index,
                bt.timestamp,
                ti.input_lock_hash AS from_lock_hash,
                o.lock_script_hash AS to_lock_hash,
                o.capacity AS amount
            FROM block_txs bt
            JOIN tx_outputs o ON o.tx_hash = bt.tx_hash
            LEFT JOIN tx_inputs ti ON ti.tx_hash = bt.tx_hash
            WHERE NOT bt.is_cellbase
              AND o.type_script_hash IS NULL
              AND ti.input_lock_hash IS DISTINCT FROM o.lock_script_hash
        ),
        cellbase_rewards AS (
            SELECT 
                bt.block_number,
                bt.tx_hash,
                bt.tx_index,
                bt.timestamp,
                o.lock_script_hash AS to_lock_hash,
                SUM(o.capacity) AS amount
            FROM block_txs bt
            JOIN tx_outputs o ON o.tx_hash = bt.tx_hash
            WHERE bt.is_cellbase
            GROUP BY bt.block_number, bt.tx_hash, bt.tx_index, bt.timestamp, o.lock_script_hash
        ),
        all_activities AS (
            SELECT 
                encode(sha256(tx_hash || 'CKB_TRANSFER'::bytea || int2send(ROW_NUMBER() OVER (PARTITION BY tx_hash ORDER BY from_lock_hash, to_lock_hash)::int2)), 'hex')::bytea AS activity_id,
                'CKB_TRANSFER' AS activity_type,
                'ckb' AS activity_category,
                block_number,
                tx_hash,
                tx_index,
                (ROW_NUMBER() OVER (PARTITION BY tx_hash ORDER BY from_lock_hash, to_lock_hash) - 1)::int2 AS activity_index,
                from_lock_hash,
                to_lock_hash,
                amount::numeric(40,0),
                NULL::bytea AS asset_id,
                '{}'::jsonb AS metadata,
                timestamp
            FROM ckb_transfers
            WHERE from_lock_hash IS NOT NULL
            
            UNION ALL
            
            SELECT 
                encode(sha256(tx_hash || 'CELLBASE_REWARD'::bytea || '\x0000'::bytea), 'hex')::bytea AS activity_id,
                'CELLBASE_REWARD' AS activity_type,
                'cellbase' AS activity_category,
                block_number,
                tx_hash,
                tx_index,
                0::int2 AS activity_index,
                NULL::bytea AS from_lock_hash,
                to_lock_hash,
                amount::numeric(40,0),
                NULL::bytea AS asset_id,
                '{}'::jsonb AS metadata,
                timestamp
            FROM cellbase_rewards
        )
        INSERT INTO activities (
            activity_id, activity_type, activity_category, block_number,
            tx_hash, tx_index, activity_index, from_lock_hash, to_lock_hash,
            amount, asset_id, metadata, timestamp
        )
        SELECT * FROM all_activities
        RETURNING 1
        "#,
    )
    .bind(start_block)
    .bind(end_block)
    .fetch_all(pool)
    .await?
    .len() as i64;

    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

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
