use anyhow::Result;
use chrono::NaiveDate;
use ckbadger_common::{StatisticsFailureInfo, StatisticsRebuildConfig, StatisticsRebuildResult};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::TaskDb;

const STATISTICS_TABLES: &[&str] = &[
    "daily_statistics",
    "daily_block_stats",
    "hourly_statistics",
    "miner_statistics",
    "block_time_distribution",
    "epoch_time_distribution",
    "dao_daily_snapshots",
];

pub async fn execute(
    db: &TaskDb,
    pool: &PgPool,
    task_id: Uuid,
    config: &StatisticsRebuildConfig,
) -> Result<()> {
    let tables: Vec<&str> = match &config.tables {
        Some(list) => list
            .iter()
            .filter_map(|t| STATISTICS_TABLES.iter().find(|&&s| s == t).copied())
            .collect(),
        None => STATISTICS_TABLES.to_vec(),
    };

    info!("Starting statistics rebuild: {} tables", tables.len());

    let total = tables.len() as i64;
    let mut result = StatisticsRebuildResult::default();

    db.update_progress(
        task_id,
        0,
        total,
        Some("Starting statistics rebuild..."),
        None,
    )
    .await?;

    sqlx::query("UPDATE sync_status SET stats_rebuild_in_progress = true")
        .execute(pool)
        .await?;

    for (i, table) in tables.iter().enumerate() {
        if db.check_cancelled(task_id).await? {
            info!("Task cancelled, stopping");
            sqlx::query("UPDATE sync_status SET stats_rebuild_in_progress = false")
                .execute(pool)
                .await?;
            return Ok(());
        }

        result.current_table = Some(table.to_string());
        db.update_result(task_id, &serde_json::to_value(&result)?)
            .await?;

        let msg = format!("Rebuilding: {}", table);
        db.append_log(task_id, &msg).await?;

        let rebuild_result = rebuild_table(pool, table).await;

        match rebuild_result {
            Ok(_) => {
                result.completed_tables.push(table.to_string());
                info!("Rebuilt table: {}", table);
            }
            Err(e) => {
                result.failed.push(StatisticsFailureInfo {
                    table: table.to_string(),
                    error: e.to_string(),
                });
                warn!("Failed to rebuild table {}: {}", table, e);
            }
        }

        let progress_msg = format!(
            "Tables: {}/{} completed",
            result.completed_tables.len(),
            total
        );
        db.update_progress(task_id, (i + 1) as i64, total, Some(&progress_msg), None)
            .await?;
    }

    result.current_table = None;

    sqlx::query("UPDATE sync_status SET stats_rebuild_in_progress = false")
        .execute(pool)
        .await?;

    info!(
        "Statistics rebuild completed: {}/{} tables",
        result.completed_tables.len(),
        tables.len()
    );

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    Ok(())
}

async fn rebuild_table(pool: &PgPool, table: &str) -> Result<()> {
    match table {
        "daily_statistics" => rebuild_daily_statistics(pool).await,
        "daily_block_stats" => rebuild_daily_block_stats(pool).await,
        "hourly_statistics" => rebuild_hourly_statistics(pool).await,
        "miner_statistics" => rebuild_miner_statistics(pool).await,
        "block_time_distribution" => rebuild_block_time_distribution(pool).await,
        "epoch_time_distribution" => rebuild_epoch_time_distribution(pool).await,
        "dao_daily_snapshots" => rebuild_dao_daily_snapshots(pool).await,
        _ => Err(anyhow::anyhow!("Unknown statistics table: {}", table)),
    }
}

async fn rebuild_daily_statistics(pool: &PgPool) -> Result<()> {
    info!("Rebuilding daily_statistics...");
    sqlx::query("TRUNCATE TABLE daily_statistics")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        INSERT INTO daily_statistics (
            date, blocks_count, transactions_count, cells_created, cells_consumed,
            capacity_transferred, total_live_cells, total_data_size
        )
        WITH daily_blocks AS (
            SELECT 
                timestamp::date as date,
                COUNT(*) as blocks_count,
                SUM(transactions_count) as transactions_count
            FROM blocks
            GROUP BY timestamp::date
        ),
        daily_cells AS (
            SELECT 
                b.timestamp::date as date,
                SUM(CASE WHEN c.created_at_block = b.number THEN 1 ELSE 0 END) as cells_created,
                SUM(CASE WHEN c.consumed_at_block = b.number THEN 1 ELSE 0 END) as cells_consumed,
                SUM(CASE WHEN c.created_at_block = b.number THEN c.capacity ELSE 0 END) as capacity_transferred,
                SUM(CASE WHEN c.created_at_block = b.number THEN c.data_size ELSE 0 END) as data_size_added,
                SUM(CASE WHEN c.consumed_at_block = b.number THEN c.data_size ELSE 0 END) as data_size_consumed
            FROM blocks b
            LEFT JOIN cells c ON c.created_at_block = b.number OR c.consumed_at_block = b.number
            GROUP BY b.timestamp::date
        )
        SELECT 
            db.date,
            db.blocks_count::int,
            db.transactions_count::int,
            COALESCE(dc.cells_created, 0)::int,
            COALESCE(dc.cells_consumed, 0)::int,
            COALESCE(dc.capacity_transferred, 0),
            SUM(COALESCE(dc.cells_created, 0) - COALESCE(dc.cells_consumed, 0)) 
                OVER (ORDER BY db.date) as total_live_cells,
            SUM(COALESCE(dc.data_size_added, 0) - COALESCE(dc.data_size_consumed, 0)) 
                OVER (ORDER BY db.date) as total_data_size
        FROM daily_blocks db
        LEFT JOIN daily_cells dc ON db.date = dc.date
        ORDER BY db.date
        "#,
    )
    .execute(pool)
    .await?;
    info!("daily_statistics rebuild completed");
    Ok(())
}

async fn rebuild_daily_block_stats(pool: &PgPool) -> Result<()> {
    info!("Rebuilding daily_block_stats...");
    sqlx::query("TRUNCATE TABLE daily_block_stats")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        INSERT INTO daily_block_stats (
            date, avg_compact_target, block_count, total_uncles, avg_uncle_rate, avg_block_time_ms
        )
        WITH block_times AS (
            SELECT 
                number,
                timestamp,
                timestamp::date as date,
                compact_target,
                uncles_count,
                EXTRACT(EPOCH FROM (timestamp - LAG(timestamp) OVER (ORDER BY number))) * 1000 as block_time_ms
            FROM blocks
        )
        SELECT 
            date,
            AVG(compact_target)::bigint as avg_compact_target,
            COUNT(*)::int as block_count,
            SUM(uncles_count)::int as total_uncles,
            SUM(uncles_count)::float / NULLIF(COUNT(*), 0)::float as avg_uncle_rate,
            AVG(block_time_ms)::int as avg_block_time_ms
        FROM block_times
        GROUP BY date
        ORDER BY date
        "#,
    )
    .execute(pool)
    .await?;
    info!("daily_block_stats rebuild completed");
    Ok(())
}

async fn rebuild_hourly_statistics(pool: &PgPool) -> Result<()> {
    info!("Rebuilding hourly_statistics...");
    sqlx::query("TRUNCATE TABLE hourly_statistics")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        INSERT INTO hourly_statistics (
            hour, blocks_count, transactions_count, cells_created, cells_consumed, capacity_transferred
        )
        WITH hourly_blocks AS (
            SELECT 
                date_trunc('hour', timestamp) as hour,
                COUNT(*) as blocks_count,
                SUM(transactions_count) as transactions_count
            FROM blocks
            GROUP BY date_trunc('hour', timestamp)
        ),
        hourly_cells AS (
            SELECT 
                date_trunc('hour', b.timestamp) as hour,
                SUM(CASE WHEN c.created_at_block = b.number THEN 1 ELSE 0 END) as cells_created,
                SUM(CASE WHEN c.consumed_at_block = b.number THEN 1 ELSE 0 END) as cells_consumed,
                SUM(CASE WHEN c.created_at_block = b.number THEN c.capacity ELSE 0 END) as capacity_transferred
            FROM blocks b
            LEFT JOIN cells c ON c.created_at_block = b.number OR c.consumed_at_block = b.number
            GROUP BY date_trunc('hour', b.timestamp)
        )
        SELECT 
            hb.hour,
            hb.blocks_count::int,
            hb.transactions_count::int,
            COALESCE(hc.cells_created, 0)::int,
            COALESCE(hc.cells_consumed, 0)::int,
            COALESCE(hc.capacity_transferred, 0)
        FROM hourly_blocks hb
        LEFT JOIN hourly_cells hc ON hb.hour = hc.hour
        ORDER BY hb.hour
        "#,
    )
    .execute(pool)
    .await?;
    info!("hourly_statistics rebuild completed");
    Ok(())
}

async fn rebuild_miner_statistics(pool: &PgPool) -> Result<()> {
    info!("Rebuilding miner_statistics...");
    sqlx::query("TRUNCATE TABLE miner_statistics")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        INSERT INTO miner_statistics (date, miner_lock_hash, blocks_count, last_block_number)
        SELECT 
            b.timestamp::date as date,
            c.lock_script_hash as miner_lock_hash,
            COUNT(*)::int as blocks_count,
            MAX(b.number) as last_block_number
        FROM blocks b
        JOIN transactions t ON t.block_number = b.number AND t.tx_index = 0
        JOIN cells c ON c.tx_hash = t.hash AND c.output_index = 0
        GROUP BY b.timestamp::date, c.lock_script_hash
        ORDER BY date, blocks_count DESC
        "#,
    )
    .execute(pool)
    .await?;
    info!("miner_statistics rebuild completed");
    Ok(())
}

async fn rebuild_block_time_distribution(pool: &PgPool) -> Result<()> {
    info!("Rebuilding block_time_distribution...");
    sqlx::query("TRUNCATE TABLE block_time_distribution")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        INSERT INTO block_time_distribution (bucket_seconds, block_count)
        SELECT 
            CASE 
                WHEN block_time_sec < 1 THEN 0
                WHEN block_time_sec < 30 THEN FLOOR(block_time_sec)::int
                ELSE 30
            END as bucket_seconds,
            COUNT(*) as block_count
        FROM (
            SELECT 
                EXTRACT(EPOCH FROM (timestamp - LAG(timestamp) OVER (ORDER BY number))) as block_time_sec
            FROM blocks
            WHERE number > 0
        ) block_times
        WHERE block_time_sec IS NOT NULL AND block_time_sec >= 0
        GROUP BY bucket_seconds
        ORDER BY bucket_seconds
        "#,
    )
    .execute(pool)
    .await?;
    info!("block_time_distribution rebuild completed");
    Ok(())
}

async fn rebuild_epoch_time_distribution(pool: &PgPool) -> Result<()> {
    info!("Rebuilding epoch_time_distribution...");
    sqlx::query("TRUNCATE TABLE epoch_time_distribution")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        INSERT INTO epoch_time_distribution (bucket_minutes, epoch_count)
        SELECT 
            CASE 
                WHEN epoch_minutes < 180 THEN 180
                WHEN epoch_minutes > 300 THEN 300
                ELSE (FLOOR(epoch_minutes / 5) * 5)::int
            END as bucket_minutes,
            COUNT(*) as epoch_count
        FROM (
            SELECT 
                EXTRACT(EPOCH FROM (end_timestamp - start_timestamp)) / 60 as epoch_minutes
            FROM epoch_statistics
            WHERE end_timestamp IS NOT NULL
        ) epoch_times
        GROUP BY bucket_minutes
        ORDER BY bucket_minutes
        "#,
    )
    .execute(pool)
    .await?;
    info!("epoch_time_distribution rebuild completed");
    Ok(())
}

async fn rebuild_dao_daily_snapshots(pool: &PgPool) -> Result<()> {
    info!("Rebuilding dao_daily_snapshots...");
    sqlx::query("TRUNCATE TABLE dao_daily_snapshots")
        .execute(pool)
        .await?;

    let dates: Vec<(NaiveDate,)> =
        sqlx::query_as("SELECT DISTINCT timestamp::date as date FROM blocks ORDER BY date")
            .fetch_all(pool)
            .await?;

    info!("Rebuilding DAO snapshots for {} days...", dates.len());

    for (date,) in dates {
        update_dao_daily_snapshot(pool, date).await?;
    }

    info!("dao_daily_snapshots rebuild completed");
    Ok(())
}

async fn update_dao_daily_snapshot(pool: &PgPool, date: NaiveDate) -> Result<()> {
    let block_data: Option<(i64, Vec<u8>)> = sqlx::query_as(
        r#"
        SELECT number, dao
        FROM blocks
        WHERE timestamp::date = $1
        ORDER BY number DESC
        LIMIT 1
        "#,
    )
    .bind(date)
    .fetch_optional(pool)
    .await?;

    let Some((block_number, dao_bytes)) = block_data else {
        return Ok(());
    };

    if dao_bytes.len() < 32 {
        return Ok(());
    }

    let total_issuance = u64::from_le_bytes(dao_bytes[0..8].try_into()?);
    let accumulated_rate = u64::from_le_bytes(dao_bytes[8..16].try_into()?);
    let secondary_issuance = u64::from_le_bytes(dao_bytes[16..24].try_into()?);
    let occupied_capacity = u64::from_le_bytes(dao_bytes[24..32].try_into()?);

    let dao_stats: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT 
            COALESCE(SUM(CASE WHEN withdraw_request_tx IS NULL THEN deposit_capacity ELSE 0 END), 0) as total_deposited,
            COALESCE(COUNT(CASE WHEN withdraw_request_tx IS NULL THEN 1 END), 0) as active_deposits,
            COALESCE(COUNT(DISTINCT CASE WHEN withdraw_request_tx IS NULL THEN depositor_address END), 0) as unique_depositors
        FROM dao_deposits
        WHERE deposit_block_number <= $1
        "#,
    )
    .bind(block_number)
    .fetch_one(pool)
    .await?;

    let (total_deposited, active_deposits, unique_depositors) = dao_stats;

    sqlx::query(
        r#"
        INSERT INTO dao_daily_snapshots (
            date, block_number, total_issuance, accumulated_rate, secondary_issuance,
            occupied_capacity, total_deposited, active_deposits, unique_depositors
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (date) DO UPDATE SET
            block_number = EXCLUDED.block_number,
            total_issuance = EXCLUDED.total_issuance,
            accumulated_rate = EXCLUDED.accumulated_rate,
            secondary_issuance = EXCLUDED.secondary_issuance,
            occupied_capacity = EXCLUDED.occupied_capacity,
            total_deposited = EXCLUDED.total_deposited,
            active_deposits = EXCLUDED.active_deposits,
            unique_depositors = EXCLUDED.unique_depositors
        "#,
    )
    .bind(date)
    .bind(block_number)
    .bind(total_issuance as i64)
    .bind(accumulated_rate as i64)
    .bind(secondary_issuance as i64)
    .bind(occupied_capacity as i64)
    .bind(total_deposited)
    .bind(active_deposits as i32)
    .bind(unique_depositors as i32)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statistics_tables_constant() {
        assert_eq!(STATISTICS_TABLES.len(), 7);
        assert!(STATISTICS_TABLES.contains(&"daily_statistics"));
        assert!(STATISTICS_TABLES.contains(&"dao_daily_snapshots"));
    }

    #[test]
    fn test_table_filtering_with_valid_tables() {
        let config = StatisticsRebuildConfig {
            tables: Some(vec![
                "daily_statistics".to_string(),
                "miner_statistics".to_string(),
            ]),
        };

        let tables: Vec<&str> = match &config.tables {
            Some(list) => list
                .iter()
                .filter_map(|t| STATISTICS_TABLES.iter().find(|&&s| s == t).copied())
                .collect(),
            None => STATISTICS_TABLES.to_vec(),
        };

        assert_eq!(tables.len(), 2);
        assert!(tables.contains(&"daily_statistics"));
        assert!(tables.contains(&"miner_statistics"));
    }

    #[test]
    fn test_table_filtering_ignores_invalid_tables() {
        let config = StatisticsRebuildConfig {
            tables: Some(vec![
                "daily_statistics".to_string(),
                "invalid_table".to_string(),
                "another_fake".to_string(),
            ]),
        };

        let tables: Vec<&str> = match &config.tables {
            Some(list) => list
                .iter()
                .filter_map(|t| STATISTICS_TABLES.iter().find(|&&s| s == t).copied())
                .collect(),
            None => STATISTICS_TABLES.to_vec(),
        };

        assert_eq!(tables.len(), 1);
        assert!(tables.contains(&"daily_statistics"));
    }

    #[test]
    fn test_table_filtering_none_returns_all() {
        let config = StatisticsRebuildConfig { tables: None };

        let tables: Vec<&str> = match &config.tables {
            Some(list) => list
                .iter()
                .filter_map(|t| STATISTICS_TABLES.iter().find(|&&s| s == t).copied())
                .collect(),
            None => STATISTICS_TABLES.to_vec(),
        };

        assert_eq!(tables.len(), 7);
    }
}
