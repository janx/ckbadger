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

    let total_blocks: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks_index")
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
    // Uses cell_flows table for net-balance approach matching the Rust ActivityParser:
    // 1. Compute net CKB balance change per address per transaction from cell_flows
    // 2. Identify senders (net negative) and receivers (net positive)
    // 3. Pair each receiver with the primary (largest) sender
    //
    // This eliminates the 4-table JOIN (cells + transaction_inputs + tx_block_map + transactions)
    // and reads only from cell_flows (~90 bytes/row vs ~400 bytes/row in cells).
    let result = sqlx::query(
        r#"
        WITH address_net AS (
            SELECT cf.tx_hash, cf.lock_script_hash,
                   SUM(CASE WHEN cf.flow_type = 0 THEN cf.capacity ELSE -cf.capacity END) AS net_change
            FROM cell_flows cf
            WHERE cf.block_number >= $1 AND cf.block_number < $2
            GROUP BY cf.tx_hash, cf.lock_script_hash
        ),
        block_txs AS (
            SELECT t.hash AS tx_hash, t.block_number, t.tx_index, t.is_cellbase, t.timestamp
            FROM transactions_index t
            WHERE t.block_number >= $1 AND t.block_number < $2
        ),
        -- Primary (largest) sender per transaction
        top_senders AS (
            SELECT DISTINCT ON (tx_hash)
                tx_hash,
                lock_script_hash AS from_lock_hash
            FROM address_net
            WHERE net_change < 0
            ORDER BY tx_hash, net_change ASC
        ),
        -- One CKB_TRANSFER per net receiver (matches Rust parser's net-balance approach)
        ckb_transfers AS (
            SELECT
                bt.block_number,
                bt.tx_hash,
                bt.tx_index,
                bt.timestamp,
                ts.from_lock_hash,
                an.lock_script_hash AS to_lock_hash,
                an.net_change AS amount
            FROM address_net an
            JOIN block_txs bt ON bt.tx_hash = an.tx_hash AND NOT bt.is_cellbase
            LEFT JOIN top_senders ts ON ts.tx_hash = an.tx_hash
            WHERE an.net_change > 0
        ),
        cellbase_flows AS (
            SELECT cf.tx_hash, cf.lock_script_hash AS to_lock_hash,
                   SUM(cf.capacity) AS amount
            FROM cell_flows cf
            JOIN block_txs bt ON bt.tx_hash = cf.tx_hash AND bt.is_cellbase
            WHERE cf.block_number >= $1 AND cf.block_number < $2
              AND cf.flow_type = 0
            GROUP BY cf.tx_hash, cf.lock_script_hash
        ),
        cellbase_rewards AS (
            SELECT
                bt.block_number,
                bt.tx_hash,
                bt.tx_index,
                bt.timestamp,
                cf.to_lock_hash,
                cf.amount
            FROM cellbase_flows cf
            JOIN block_txs bt ON bt.tx_hash = cf.tx_hash AND bt.is_cellbase
        ),
        all_activities AS (
            SELECT
                encode(sha256(tx_hash || 'CKB_TRANSFER'::bytea || int2send((ROW_NUMBER() OVER (PARTITION BY tx_hash ORDER BY to_lock_hash))::int2)), 'hex')::bytea AS activity_id,
                'CKB_TRANSFER' AS activity_type,
                'ckb' AS activity_category,
                block_number,
                tx_hash,
                tx_index,
                (ROW_NUMBER() OVER (PARTITION BY tx_hash ORDER BY to_lock_hash) - 1)::int2 AS activity_index,
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
                encode(sha256(tx_hash || 'CELLBASE_REWARD'::bytea || int2send((ROW_NUMBER() OVER (PARTITION BY tx_hash ORDER BY to_lock_hash))::int2)), 'hex')::bytea AS activity_id,
                'CELLBASE_REWARD' AS activity_type,
                'cellbase' AS activity_category,
                block_number,
                tx_hash,
                tx_index,
                (ROW_NUMBER() OVER (PARTITION BY tx_hash ORDER BY to_lock_hash) - 1)::int2 AS activity_index,
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
        "#,
    )
    .bind(start_block)
    .bind(end_block)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() as i64)
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
