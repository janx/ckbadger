use anyhow::Result;
use ckbadger_common::{AddressBalancesRebuildConfig, AddressBalancesRebuildResult};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::db::TaskDb;

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    _config: &AddressBalancesRebuildConfig,
) -> Result<()> {
    info!("Starting address_balances_rebuild task");

    db.update_progress(
        task_id,
        0,
        3,
        Some("Truncating address_balances table"),
        None,
    )
    .await?;

    sqlx::query("TRUNCATE address_balances")
        .execute(pool)
        .await?;

    info!("Truncated address_balances table, starting rebuild...");

    db.update_progress(
        task_id,
        1,
        3,
        Some("Rebuilding address balances from cells"),
        None,
    )
    .await?;

    let addresses_updated = rebuild_address_balances(pool).await?;

    info!(
        "Rebuilt {} address balances, updating sync_status...",
        addresses_updated
    );

    db.update_progress(task_id, 2, 3, Some("Clearing deferred flag"), None)
        .await?;

    sqlx::query(
        "UPDATE sync_status SET 
            address_balances_deferred = FALSE, 
            address_balances_rebuild_completed_at = NOW() 
         WHERE id = 1",
    )
    .execute(pool)
    .await?;

    let result = AddressBalancesRebuildResult { addresses_updated };

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    info!(
        "address_balances_rebuild completed: {} addresses updated",
        addresses_updated
    );

    Ok(())
}

async fn rebuild_address_balances(pool: &PgPool) -> Result<i64> {
    let inserted = sqlx::query_scalar::<_, i64>(
        r#"
        WITH cell_stats AS (
            SELECT 
                lock_script_hash,
                SUM(CASE WHEN status = 0 THEN capacity ELSE 0 END) AS balance,
                COUNT(CASE WHEN status = 0 THEN 1 END)::INTEGER AS live_cells_count,
                COUNT(*)::BIGINT AS total_cells_count,
                MIN(created_at_block) AS first_seen_block,
                MAX(created_at_block) AS last_activity_block
            FROM cells
            GROUP BY lock_script_hash
        ),
        tx_counts AS (
            SELECT 
                lock_script_hash,
                COUNT(DISTINCT tx_hash)::BIGINT AS transactions_count
            FROM cells
            GROUP BY lock_script_hash
        ),
        first_last_tx AS (
            SELECT DISTINCT ON (lock_script_hash)
                lock_script_hash,
                tx_hash AS first_seen_tx
            FROM cells
            ORDER BY lock_script_hash, created_at_block, output_index
        ),
        last_tx AS (
            SELECT DISTINCT ON (lock_script_hash)
                lock_script_hash,
                tx_hash AS last_activity_tx
            FROM cells
            ORDER BY lock_script_hash, created_at_block DESC, output_index DESC
        )
        INSERT INTO address_balances (
            lock_script_hash,
            balance,
            live_cells_count,
            total_cells_count,
            transactions_count,
            first_seen_block,
            first_seen_tx,
            last_activity_block,
            last_activity_tx,
            updated_at
        )
        SELECT 
            cs.lock_script_hash,
            COALESCE(cs.balance, 0),
            COALESCE(cs.live_cells_count, 0),
            COALESCE(cs.total_cells_count, 0),
            COALESCE(tc.transactions_count, 0),
            cs.first_seen_block,
            ft.first_seen_tx,
            cs.last_activity_block,
            lt.last_activity_tx,
            NOW()
        FROM cell_stats cs
        LEFT JOIN tx_counts tc ON tc.lock_script_hash = cs.lock_script_hash
        LEFT JOIN first_last_tx ft ON ft.lock_script_hash = cs.lock_script_hash
        LEFT JOIN last_tx lt ON lt.lock_script_hash = cs.lock_script_hash
        RETURNING 1
        "#,
    )
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
        let config = AddressBalancesRebuildConfig::default();
        assert!(config._reserved.is_none());
    }

    #[test]
    fn test_result_struct() {
        let result = AddressBalancesRebuildResult {
            addresses_updated: 12345,
        };
        assert_eq!(result.addresses_updated, 12345);
    }

    #[test]
    fn test_result_serialization() {
        let result = AddressBalancesRebuildResult {
            addresses_updated: 999,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["addressesUpdated"], 999);
    }
}
