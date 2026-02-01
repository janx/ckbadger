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
            "Tables: {}/{} completed{}",
            result.completed_tables.len(),
            total,
            if !result.failed.is_empty() {
                format!(", {} failed", result.failed.len())
            } else {
                String::new()
            }
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
            capacity_transferred, total_live_cells, total_data_size, avg_block_time_ms
        )
        WITH block_times AS (
            SELECT 
                number,
                timestamp,
                timestamp::date as date,
                transactions_count,
                EXTRACT(EPOCH FROM (timestamp - LAG(timestamp) OVER (ORDER BY number))) * 1000 as block_time_ms
            FROM blocks
        ),
        daily_blocks AS (
            SELECT 
                date,
                COUNT(*) as blocks_count,
                SUM(transactions_count) as transactions_count,
                AVG(block_time_ms)::int as avg_block_time_ms
            FROM block_times
            GROUP BY date
        ),
        cells_created_agg AS (
            SELECT 
                b.timestamp::date as date,
                COUNT(*) as cells_created,
                SUM(c.capacity) as capacity_transferred,
                SUM(c.data_size) as data_size_added
            FROM cells c
            JOIN blocks b ON b.number = c.created_at_block
            GROUP BY b.timestamp::date
        ),
        cells_consumed_agg AS (
            SELECT 
                b.timestamp::date as date,
                COUNT(*) as cells_consumed,
                SUM(c.data_size) as data_size_consumed
            FROM cells c
            JOIN blocks b ON b.number = c.consumed_at_block
            WHERE c.consumed_at_block IS NOT NULL
            GROUP BY b.timestamp::date
        )
        SELECT 
            db.date,
            db.blocks_count::int,
            db.transactions_count::int,
            COALESCE(cc.cells_created, 0)::int,
            COALESCE(cd.cells_consumed, 0)::int,
            COALESCE(cc.capacity_transferred, 0),
            SUM(COALESCE(cc.cells_created, 0) - COALESCE(cd.cells_consumed, 0)) 
                OVER (ORDER BY db.date) as total_live_cells,
            SUM(COALESCE(cc.data_size_added, 0) - COALESCE(cd.data_size_consumed, 0)) 
                OVER (ORDER BY db.date) as total_data_size,
            db.avg_block_time_ms
        FROM daily_blocks db
        LEFT JOIN cells_created_agg cc ON db.date = cc.date
        LEFT JOIN cells_consumed_agg cd ON db.date = cd.date
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

pub async fn update_dao_daily_snapshot(pool: &PgPool, date: NaiveDate) -> Result<()> {
    // Query DAO deposit statistics for this date
    // Uses deposit_timestamp and withdraw_timestamp to determine active deposits at end of day
    let stats = sqlx::query_as::<_, (String, i64, String, i64)>(
        r#"
        SELECT 
            COALESCE(SUM(capacity::numeric), 0)::text as total_deposit,
            COUNT(DISTINCT lock_script_hash) as depositors_count,
            COALESCE(SUM(CASE WHEN deposit_timestamp::date = $1 THEN capacity::numeric ELSE 0 END), 0)::text as daily_deposit,
            COUNT(CASE WHEN deposit_timestamp::date = $1 THEN 1 END) as daily_deposit_count
        FROM dao_deposits
        WHERE deposit_timestamp::date <= $1
          AND (withdraw_timestamp IS NULL OR withdraw_timestamp::date > $1)
        "#,
    )
    .bind(date)
    .fetch_one(pool)
    .await?;

    // Get DAO field from the last block of the day
    let dao_data = sqlx::query_as::<_, (Vec<u8>,)>(
        "SELECT dao FROM blocks WHERE timestamp::date = $1 ORDER BY number DESC LIMIT 1",
    )
    .bind(date)
    .fetch_optional(pool)
    .await?;

    // Extract total_issuance from DAO field (bytes 0-7, little-endian u64)
    let total_issuance = dao_data
        .as_ref()
        .and_then(|(dao,)| {
            if dao.len() >= 8 {
                let bytes: [u8; 8] = dao[0..8].try_into().ok()?;
                Some(u64::from_le_bytes(bytes).to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "0".to_string());

    // Query cumulative secondary issuance breakdown from block_secondary_issuance table
    // This is critical for Total Supply and Secondary Issuance charts
    let secondary_issuance = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT 
            COALESCE(SUM(burnt), 0)::text,
            COALESCE(SUM(miner_secondary), 0)::text,
            COALESCE(SUM(dao_compensation), 0)::text
        FROM block_secondary_issuance
        WHERE block_timestamp::date <= $1
        "#,
    )
    .bind(date)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|_| ("0".to_string(), "0".to_string(), "0".to_string()));

    sqlx::query(
        r#"
        INSERT INTO dao_daily_snapshots (
            date, total_deposit, depositors_count, daily_deposit, daily_deposit_count, 
            total_issuance, cumulative_burnt, cumulative_mining_reward, cumulative_deposit_compensation,
            dao_data
        )
        VALUES ($1, $2::numeric, $3, $4::numeric, $5, $6::numeric, $7, $8, $9, $10)
        ON CONFLICT (date) DO UPDATE SET
            total_deposit = EXCLUDED.total_deposit,
            depositors_count = EXCLUDED.depositors_count,
            daily_deposit = EXCLUDED.daily_deposit,
            daily_deposit_count = EXCLUDED.daily_deposit_count,
            total_issuance = EXCLUDED.total_issuance,
            cumulative_burnt = EXCLUDED.cumulative_burnt,
            cumulative_mining_reward = EXCLUDED.cumulative_mining_reward,
            cumulative_deposit_compensation = EXCLUDED.cumulative_deposit_compensation,
            dao_data = EXCLUDED.dao_data
        "#,
    )
    .bind(date)
    .bind(&stats.0) // total_deposit
    .bind(stats.1 as i32) // depositors_count
    .bind(&stats.2) // daily_deposit
    .bind(stats.3 as i32) // daily_deposit_count
    .bind(&total_issuance)
    .bind(&secondary_issuance.0) // cumulative_burnt
    .bind(&secondary_issuance.1) // cumulative_mining_reward
    .bind(&secondary_issuance.2) // cumulative_deposit_compensation
    .bind(dao_data.map(|(d,)| d)) // dao_data
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
