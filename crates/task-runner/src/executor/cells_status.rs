use anyhow::Result;
use ckbadger_common::{CellsStatusRebuildConfig, CellsStatusRebuildResult, RateCalculator};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

/// Cell partition boundaries (5M blocks each, matching migrations/postgres/001_init.sql)
const CELL_PARTITIONS: &[(&str, i64, i64)] = &[
    ("cells_p00", 0, 5_000_000),
    ("cells_p01", 5_000_000, 10_000_000),
    ("cells_p02", 10_000_000, 15_000_000),
    ("cells_p03", 15_000_000, 20_000_000),
    ("cells_p04", 20_000_000, 25_000_000),
    ("cells_p05", 25_000_000, 30_000_000),
    ("cells_p06", 30_000_000, 35_000_000),
    ("cells_p07", 35_000_000, 40_000_000),
    ("cells_p08", 40_000_000, 45_000_000),
    ("cells_p09", 45_000_000, 50_000_000),
];

/// Rebuilds cells.status and consumed_at_* fields from transaction_inputs.
///
/// This task consolidates the former `cells_status_rebuild` and `consumed_at_backfill`
/// tasks into a single pass. It handles:
/// - Cells with status=0 that need to be marked as consumed (status=1)
/// - Cells with NULL consumed_at_block that need backfilling
///
/// Optimization: targets individual cell partitions explicitly to avoid cross-partition
/// hash joins. Each UPDATE only touches one cell partition, enabling efficient index
/// scans instead of sequential scans across all partitions.
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

    while current_block < total_blocks {
        let end_block = (current_block + config.batch_size).min(total_blocks);

        // Acquire a single connection for the entire batch so the temp table
        // is visible to all subsequent queries (temp tables are session-scoped).
        let mut conn = pool.acquire().await?;

        // Materialize transaction_inputs for this batch into a temp table ONCE,
        // then each partition UPDATE joins from it. Without this, the CTE rescans
        // the same transaction_inputs range N times (once per partition).
        sqlx::query(
            r#"
            CREATE TEMP TABLE _batch_inputs ON COMMIT DROP AS
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
            "#,
        )
        .bind(current_block)
        .bind(end_block)
        .execute(&mut *conn)
        .await?;

        // Analyze the temp table so the planner knows its size for join decisions
        sqlx::query("ANALYZE _batch_inputs")
            .execute(&mut *conn)
            .await?;

        // Process each cell partition that could contain consumed cells for this batch.
        // Targeting partitions explicitly avoids cross-partition hash joins.
        let mut batch_updated: i64 = 0;
        for &(partition, part_start, part_end) in CELL_PARTITIONS {
            // Skip partitions that can't contain cells created before end_block
            if part_start >= end_block {
                break;
            }
            // Skip partitions with no overlap
            if part_end <= 0 {
                continue;
            }

            let sql = format!(
                r#"
                UPDATE {} c SET
                    status = 1,
                    consumed_at_block = ic.consumed_at_block,
                    consumed_by_tx = ic.consumed_by_tx,
                    consumed_at_index = ic.consumed_at_index
                FROM _batch_inputs ic
                WHERE c.tx_hash = ic.previous_tx_hash
                  AND c.output_index = ic.previous_output_index
                  AND (c.status = 0 OR c.consumed_at_block IS NULL)
                "#,
                partition
            );

            let updated = sqlx::query(&sql).execute(&mut *conn).await?.rows_affected() as i64;

            batch_updated += updated;
        }

        // Drop the temp table explicitly (also dropped on commit, but be explicit)
        sqlx::query("DROP TABLE IF EXISTS _batch_inputs")
            .execute(&mut *conn)
            .await?;

        cells_updated += batch_updated;
        blocks_processed = end_block;

        if batch_updated > 0 {
            info!(
                "Rebuilt blocks {}-{}: {} cells marked as consumed",
                current_block, end_block, batch_updated
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

    #[test]
    fn test_cell_partitions_count() {
        assert_eq!(CELL_PARTITIONS.len(), 10);
    }

    #[test]
    fn test_cell_partitions_contiguous() {
        // Each partition's end should equal the next partition's start
        for i in 0..CELL_PARTITIONS.len() - 1 {
            let (_, _, end) = CELL_PARTITIONS[i];
            let (_, next_start, _) = CELL_PARTITIONS[i + 1];
            assert_eq!(
                end,
                next_start,
                "partition {} end ({}) != partition {} start ({})",
                i,
                end,
                i + 1,
                next_start
            );
        }
    }

    #[test]
    fn test_cell_partitions_start_at_zero() {
        let (_, start, _) = CELL_PARTITIONS[0];
        assert_eq!(start, 0);
    }

    #[test]
    fn test_cell_partitions_cover_50m_blocks() {
        let (_, _, end) = CELL_PARTITIONS[CELL_PARTITIONS.len() - 1];
        assert_eq!(end, 50_000_000);
    }

    #[test]
    fn test_cell_partitions_5m_each() {
        for &(_, start, end) in CELL_PARTITIONS {
            assert_eq!(end - start, 5_000_000, "partition should span 5M blocks");
        }
    }

    #[test]
    fn test_cell_partitions_naming() {
        for (i, &(name, _, _)) in CELL_PARTITIONS.iter().enumerate() {
            let expected = format!("cells_p{:02}", i);
            assert_eq!(name, expected.as_str());
        }
    }
}
