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
        Some("Rebuilding address balances from cell_flows"),
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
    // Scans cell_flows (~90 bytes/row) instead of cells (~400 bytes/row),
    // reducing I/O by ~4-5x. Uses flow_type to compute net balance.
    let result = sqlx::query(
        r#"
        WITH cell_agg AS (
            SELECT
                lock_script_hash,
                SUM(CASE WHEN flow_type = 0 THEN capacity ELSE 0 END)
                  - SUM(CASE WHEN flow_type = 1 THEN capacity ELSE 0 END) AS balance,
                COUNT(CASE WHEN flow_type = 0 THEN 1 END)::INTEGER
                  - COUNT(CASE WHEN flow_type = 1 THEN 1 END)::INTEGER AS live_cells_count,
                COUNT(CASE WHEN flow_type = 0 THEN 1 END)::BIGINT AS total_cells_count,
                COUNT(DISTINCT tx_hash)::BIGINT AS transactions_count,
                MIN(block_number) AS first_seen_block,
                MAX(block_number) AS last_activity_block
            FROM cell_flows
            GROUP BY lock_script_hash
        ),
        first_tx AS (
            SELECT DISTINCT ON (lock_script_hash)
                lock_script_hash,
                tx_hash AS first_seen_tx
            FROM cell_flows
            WHERE flow_type = 0
            ORDER BY lock_script_hash, block_number, output_index
        ),
        last_tx AS (
            SELECT DISTINCT ON (lock_script_hash)
                lock_script_hash,
                tx_hash AS last_activity_tx
            FROM cell_flows
            ORDER BY lock_script_hash, block_number DESC, output_index DESC
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
            ca.lock_script_hash,
            COALESCE(ca.balance, 0),
            COALESCE(ca.live_cells_count, 0),
            COALESCE(ca.total_cells_count, 0),
            COALESCE(ca.transactions_count, 0),
            ca.first_seen_block,
            ft.first_seen_tx,
            ca.last_activity_block,
            lt.last_activity_tx,
            NOW()
        FROM cell_agg ca
        LEFT JOIN first_tx ft ON ft.lock_script_hash = ca.lock_script_hash
        LEFT JOIN last_tx lt ON lt.lock_script_hash = ca.lock_script_hash
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() as i64)
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
