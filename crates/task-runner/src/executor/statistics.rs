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

    // BURN_QUOTA * 0.6 in shannons = 8,400,000,000 CKB * 0.6 * 10^8 = 504,000,000,000,000,000
    // This constant is embedded in the SQL query below since sqlx doesn't support i128 binding.

    sqlx::query(
        r#"
        INSERT INTO daily_statistics (
            date, blocks_count, transactions_count, cells_created, cells_consumed,
            capacity_transferred, total_live_cells, total_dead_cells, total_all_cells,
            total_data_size, avg_block_time_ms, knowledge_size
        )
        WITH block_times AS (
            SELECT 
                number,
                timestamp,
                timestamp::date as date,
                transactions_count,
                dao,
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
        daily_dao AS (
            SELECT DISTINCT ON (date)
                date,
                dao
            FROM block_times
            ORDER BY date, number DESC
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
            SUM(COALESCE(cd.cells_consumed, 0)) 
                OVER (ORDER BY db.date) as total_dead_cells,
            SUM(COALESCE(cc.cells_created, 0)) 
                OVER (ORDER BY db.date) as total_all_cells,
            SUM(COALESCE(cc.data_size_added, 0) - COALESCE(cd.data_size_consumed, 0)) 
                OVER (ORDER BY db.date) as total_data_size,
            db.avg_block_time_ms,
            (('x' || encode(reverse(substring(dd.dao from 25 for 8)), 'hex'))::bit(64)::bigint)::numeric - 504000000000000000
                as knowledge_size
        FROM daily_blocks db
        LEFT JOIN cells_created_agg cc ON db.date = cc.date
        LEFT JOIN cells_consumed_agg cd ON db.date = cd.date
        LEFT JOIN daily_dao dd ON db.date = dd.date
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
        cells_created_agg AS (
            SELECT 
                date_trunc('hour', b.timestamp) as hour,
                COUNT(*) as cells_created,
                SUM(c.capacity) as capacity_transferred
            FROM cells c
            JOIN blocks b ON b.number = c.created_at_block
            GROUP BY date_trunc('hour', b.timestamp)
        ),
        cells_consumed_agg AS (
            SELECT 
                date_trunc('hour', b.timestamp) as hour,
                COUNT(*) as cells_consumed
            FROM cells c
            JOIN blocks b ON b.number = c.consumed_at_block
            WHERE c.consumed_at_block IS NOT NULL
            GROUP BY date_trunc('hour', b.timestamp)
        )
        SELECT 
            hb.hour,
            hb.blocks_count::int,
            hb.transactions_count::int,
            COALESCE(cc.cells_created, 0)::int,
            COALESCE(cd.cells_consumed, 0)::int,
            COALESCE(cc.capacity_transferred, 0)
        FROM hourly_blocks hb
        LEFT JOIN cells_created_agg cc ON hb.hour = cc.hour
        LEFT JOIN cells_consumed_agg cd ON hb.hour = cd.hour
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

    // Join through cellbase transaction to get miner's lock_script_hash from first output
    // This works even when blocks.miner_lock_hash is NULL (bulk sync doesn't populate it)
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
    info!("Rebuilding block_time_distribution (recent 50K blocks, 100ms buckets)...");
    sqlx::query("TRUNCATE TABLE block_time_distribution")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        WITH recent_blocks AS (
            SELECT number, timestamp
            FROM blocks
            WHERE number > 0
            ORDER BY number DESC
            LIMIT 50000
        ),
        block_times AS (
            SELECT 
                EXTRACT(EPOCH FROM (timestamp - LAG(timestamp) OVER (ORDER BY number))) * 1000 as block_time_ms
            FROM recent_blocks
        )
        INSERT INTO block_time_distribution (bucket_ms, block_count)
        SELECT 
            LEAST(CEIL(block_time_ms / 100.0)::int * 100, 50000) as bucket_ms,
            COUNT(*) as block_count
        FROM block_times
        WHERE block_time_ms IS NOT NULL AND block_time_ms > 0
        GROUP BY bucket_ms
        ORDER BY bucket_ms
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

    // Use 1-minute buckets to match official CKB Explorer
    sqlx::query(
        r#"
        INSERT INTO epoch_time_distribution (bucket_minutes, epoch_count)
        SELECT 
            ROUND(epoch_minutes)::int as bucket_minutes,
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

pub async fn rebuild_dao_daily_snapshots(pool: &PgPool) -> Result<()> {
    info!("Rebuilding dao_daily_snapshots...");
    sqlx::query("TRUNCATE TABLE dao_daily_snapshots")
        .execute(pool)
        .await?;

    // Single efficient query using window functions for cumulative sums
    // Key optimization: O(n) instead of O(n²) for secondary_issuance cumulative
    //
    // Previous approach: loop through ~2000 days, each doing:
    //   SELECT SUM(...) FROM block_secondary_issuance WHERE date <= $1
    // This scans from genesis for EVERY day = O(n²) ≈ 18 billion row scans
    //
    // New approach: Single query with window functions = O(n) ≈ 18 million rows once
    sqlx::query(
        r#"
        INSERT INTO dao_daily_snapshots (
            date, total_deposit, depositors_count, daily_deposit, daily_deposit_count,
            total_issuance, cumulative_burnt, cumulative_mining_reward, cumulative_deposit_compensation,
            dao_data
        )
        WITH 
        -- All unique dates from blockchain
        dates AS (
            SELECT DISTINCT timestamp::date as date FROM blocks
        ),
        -- Secondary issuance: aggregate by day, then cumulative sum via window function
        secondary_daily AS (
            SELECT 
                block_timestamp::date as date,
                SUM(burnt)::numeric as daily_burnt,
                SUM(miner_secondary)::numeric as daily_miner,
                SUM(dao_compensation)::numeric as daily_dao
            FROM block_secondary_issuance
            GROUP BY block_timestamp::date
        ),
        secondary_cumulative AS (
            SELECT 
                date,
                SUM(daily_burnt) OVER w as cumulative_burnt,
                SUM(daily_miner) OVER w as cumulative_mining_reward,
                SUM(daily_dao) OVER w as cumulative_deposit_compensation
            FROM secondary_daily
            WINDOW w AS (ORDER BY date ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)
        ),
        -- Last block of each day (for DAO field extraction)
        last_blocks AS (
            SELECT DISTINCT ON (timestamp::date)
                timestamp::date as date,
                dao
            FROM blocks
            ORDER BY timestamp::date, number DESC
        ),
        -- DAO deposit events: model as +capacity on deposit, -capacity on withdraw
        deposit_events AS (
            SELECT deposit_timestamp::date as date, capacity, lock_script_hash, 1 as event_type
            FROM dao_deposits
            UNION ALL
            SELECT withdraw_timestamp::date as date, capacity, lock_script_hash, -1 as event_type
            FROM dao_deposits
            WHERE withdraw_timestamp IS NOT NULL
        ),
        -- Aggregate deposit events by date
        deposit_daily AS (
            SELECT 
                date,
                SUM(capacity * event_type)::numeric as delta_capacity,
                SUM(CASE WHEN event_type = 1 THEN capacity ELSE 0 END)::numeric as daily_deposit,
                COUNT(*) FILTER (WHERE event_type = 1)::int as daily_deposit_count
            FROM deposit_events
            GROUP BY date
        ),
        -- Cumulative deposit totals using window function
        deposit_cumulative AS (
            SELECT 
                date,
                daily_deposit,
                daily_deposit_count,
                SUM(delta_capacity) OVER w as total_deposit
            FROM deposit_daily
            WINDOW w AS (ORDER BY date ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)
        ),
        -- Depositors count: track active depositors at each point in time
        -- This requires knowing when each depositor's first deposit started and last withdraw ended
        depositor_spans AS (
            SELECT 
                lock_script_hash,
                MIN(deposit_timestamp::date) as first_deposit,
                -- If any deposit has no withdraw, depositor is still active (use far future date)
                CASE 
                    WHEN bool_or(withdraw_timestamp IS NULL) THEN '9999-12-31'::date
                    ELSE MAX(withdraw_timestamp::date)
                END as last_active
            FROM dao_deposits
            GROUP BY lock_script_hash
        ),
        depositor_events AS (
            SELECT first_deposit as date, 1 as delta FROM depositor_spans
            UNION ALL
            SELECT last_active as date, -1 as delta FROM depositor_spans WHERE last_active < '9999-12-31'::date
        ),
        depositor_daily AS (
            SELECT date, SUM(delta)::int as delta_depositors
            FROM depositor_events
            GROUP BY date
        ),
        depositor_cumulative AS (
            SELECT 
                date,
                SUM(delta_depositors) OVER w as depositors_count
            FROM depositor_daily
            WINDOW w AS (ORDER BY date ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)
        )
        SELECT 
            d.date,
            COALESCE(dc_latest.total_deposit, 0) as total_deposit,
            COALESCE(depc_latest.depositors_count, (
                SELECT COUNT(DISTINCT lock_script_hash) FROM depositor_spans 
                WHERE first_deposit <= d.date AND last_active >= d.date
            ))::int as depositors_count,
            COALESCE(dc.daily_deposit, 0) as daily_deposit,
            COALESCE(dc.daily_deposit_count, 0) as daily_deposit_count,
            -- Extract total_issuance from DAO field bytes 0-7 (little-endian u64)
            COALESCE((
                get_byte(lb.dao, 0)::bigint +
                get_byte(lb.dao, 1)::bigint * 256 +
                get_byte(lb.dao, 2)::bigint * 65536 +
                get_byte(lb.dao, 3)::bigint * 16777216 +
                get_byte(lb.dao, 4)::bigint * 4294967296::bigint +
                get_byte(lb.dao, 5)::bigint * 1099511627776::bigint +
                get_byte(lb.dao, 6)::bigint * 281474976710656::bigint +
                get_byte(lb.dao, 7)::bigint * 72057594037927936::bigint
            )::numeric, 0) as total_issuance,
            COALESCE(sc.cumulative_burnt::text, '0') as cumulative_burnt,
            COALESCE(sc.cumulative_mining_reward::text, '0') as cumulative_mining_reward,
            COALESCE(sc.cumulative_deposit_compensation::text, '0') as cumulative_deposit_compensation,
            lb.dao as dao_data
        FROM dates d
        LEFT JOIN deposit_cumulative dc ON d.date = dc.date
        LEFT JOIN LATERAL (
            SELECT total_deposit FROM deposit_cumulative WHERE date <= d.date ORDER BY date DESC LIMIT 1
        ) dc_latest ON true
        LEFT JOIN depositor_cumulative depc ON d.date = depc.date
        LEFT JOIN LATERAL (
            SELECT depositors_count FROM depositor_cumulative WHERE date <= d.date ORDER BY date DESC LIMIT 1
        ) depc_latest ON true
        LEFT JOIN last_blocks lb ON d.date = lb.date
        LEFT JOIN secondary_cumulative sc ON d.date = sc.date
        ORDER BY d.date
        "#,
    )
    .execute(pool)
    .await?;

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
