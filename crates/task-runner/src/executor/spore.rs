use anyhow::Result;
use ckbadger_common::{SporeRebuildConfig, SporeRebuildResult};
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
    info!("Starting spore rebuild task");

    let _batch_size = config.batch_size;
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

    db.update_progress(
        task_id,
        0,
        total_spores,
        Some("Rebuilding spore is_live status..."),
        None,
    )
    .await?;

    let marked_consumed = sqlx::query(
        r#"
        UPDATE spore_cells sc
        SET 
            is_live = FALSE,
            consumed_at_block = c.consumed_at_block,
            consumed_by_tx = c.consumed_by_tx,
            updated_at = NOW()
        FROM cells c
        WHERE sc.tx_hash = c.tx_hash
          AND sc.output_index = c.output_index
          AND c.consumed_at_block IS NOT NULL
          AND sc.is_live = TRUE
        "#,
    )
    .execute(pool)
    .await?;
    result.spores_marked_consumed = marked_consumed.rows_affected() as i64;

    info!(
        "Marked {} spores as consumed based on cell status",
        result.spores_marked_consumed
    );

    db.update_progress(
        task_id,
        total_spores / 2,
        total_spores,
        Some(&format!(
            "Marked {} consumed, updating cluster counts...",
            result.spores_marked_consumed
        )),
        None,
    )
    .await?;

    if db.check_cancelled(task_id).await? {
        info!("Task cancelled");
        return Ok(());
    }

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
        total_spores,
        total_spores,
        Some(&format!(
            "Completed: {} consumed, {} clusters updated",
            result.spores_marked_consumed, result.clusters_updated
        )),
        None,
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
}
