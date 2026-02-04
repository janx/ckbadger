use anyhow::Result;
use ckbadger_common::{RateCalculator, SporeRebuildConfig, SporeRebuildResult};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &SporeRebuildConfig,
) -> Result<()> {
    info!(
        "Starting spore rebuild task (batch_size={})",
        config.batch_size
    );

    let mut result = SporeRebuildResult::default();

    db.update_progress(task_id, 0, 100, Some("Counting spore cells..."), None)
        .await?;

    let (total_spores,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM spore_cells")
        .fetch_one(pool)
        .await?;

    info!("Total spore cells to process: {}", total_spores);

    if total_spores == 0 {
        db.complete_task(task_id, Some(serde_json::to_value(&result)?))
            .await?;
        return Ok(());
    }

    let (min_block, max_block): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(MIN(created_at_block), 0), COALESCE(MAX(created_at_block), 0) FROM spore_cells",
    )
    .fetch_one(pool)
    .await?;

    let total_blocks = max_block - min_block + 1;
    let batch_size = config.batch_size as i64;
    let mut rate_calc = RateCalculator::default();

    db.update_progress(
        task_id,
        0,
        total_blocks,
        Some("Rebuilding spore is_live status in batches..."),
        None,
    )
    .await?;

    let mut current_block = min_block;
    let mut processed_blocks: i64;

    while current_block <= max_block {
        if db.check_cancelled(task_id).await? {
            info!("Task cancelled");
            return Ok(());
        }

        let end_block = (current_block + batch_size).min(max_block + 1);

        let batch_updated = sqlx::query_scalar::<_, i64>(
            r#"
            WITH batch_spores AS (
                SELECT sc.tx_hash, sc.output_index
                FROM spore_cells sc
                WHERE sc.created_at_block >= $1 AND sc.created_at_block < $2
                  AND sc.is_live = TRUE
            )
            UPDATE spore_cells sc
            SET 
                is_live = FALSE,
                consumed_at_block = c.consumed_at_block,
                consumed_by_tx = c.consumed_by_tx,
                updated_at = NOW()
            FROM cells c, batch_spores bs
            WHERE sc.tx_hash = bs.tx_hash
              AND sc.output_index = bs.output_index
              AND sc.tx_hash = c.tx_hash
              AND sc.output_index = c.output_index
              AND c.consumed_at_block IS NOT NULL
            RETURNING 1::BIGINT
            "#,
        )
        .bind(current_block)
        .bind(end_block)
        .fetch_all(pool)
        .await?
        .len() as i64;

        result.spores_marked_consumed += batch_updated;
        processed_blocks = end_block - min_block;

        if batch_updated > 0 {
            info!(
                "Blocks {}-{}: {} spores marked consumed",
                current_block,
                end_block - 1,
                batch_updated
            );
        }

        rate_calc.add_sample(processed_blocks);
        db.update_progress(
            task_id,
            processed_blocks,
            total_blocks,
            Some(&format!(
                "Processed blocks {}-{}, {} spores consumed so far",
                current_block,
                end_block - 1,
                result.spores_marked_consumed
            )),
            rate_calc.rate(),
        )
        .await?;

        current_block = end_block;
    }

    info!(
        "Marked {} spores as consumed, updating cluster counts...",
        result.spores_marked_consumed
    );

    let updated_clusters = sqlx::query(
        r#"
        WITH live_counts AS (
            SELECT cluster_id, COUNT(*) as live_count
            FROM spore_cells
            WHERE cluster_id IS NOT NULL AND is_live = TRUE
            GROUP BY cluster_id
        )
        UPDATE spore_clusters sc
        SET spores_count = COALESCE(lc.live_count, 0),
            updated_at = NOW()
        FROM (
            SELECT sc2.cluster_id, COALESCE(lc.live_count, 0) as live_count
            FROM spore_clusters sc2
            LEFT JOIN live_counts lc ON sc2.cluster_id = lc.cluster_id
        ) lc
        WHERE sc.cluster_id = lc.cluster_id
          AND sc.spores_count != lc.live_count
        "#,
    )
    .execute(pool)
    .await?;
    result.clusters_updated = updated_clusters.rows_affected() as i64;

    info!(
        "Updated spores_count for {} clusters",
        result.clusters_updated
    );

    result.spores_processed = total_spores;

    db.update_progress(
        task_id,
        total_blocks,
        total_blocks,
        Some(&format!(
            "Completed: {} consumed, {} clusters updated",
            result.spores_marked_consumed, result.clusters_updated
        )),
        rate_calc.rate(),
    )
    .await?;

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "Spore rebuild completed: {} spores processed, {} marked consumed, {} clusters updated",
        result.spores_processed, result.spores_marked_consumed, result.clusters_updated
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SporeRebuildConfig::default();
        assert_eq!(config.batch_size, 10_000);
    }

    #[test]
    fn test_custom_batch_size() {
        let config = SporeRebuildConfig { batch_size: 5_000 };
        assert_eq!(config.batch_size, 5_000);
    }

    #[test]
    fn test_result_struct() {
        let result = SporeRebuildResult {
            spores_processed: 1000,
            spores_marked_consumed: 500,
            clusters_updated: 10,
        };
        assert_eq!(result.spores_processed, 1000);
        assert_eq!(result.spores_marked_consumed, 500);
        assert_eq!(result.clusters_updated, 10);
    }

    #[test]
    fn test_result_serialization() {
        let result = SporeRebuildResult {
            spores_processed: 100,
            spores_marked_consumed: 50,
            clusters_updated: 5,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["sporesProcessed"], 100);
        assert_eq!(json["sporesMarkedConsumed"], 50);
        assert_eq!(json["clustersUpdated"], 5);
    }
}
