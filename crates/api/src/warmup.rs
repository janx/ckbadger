use std::sync::Arc;
use std::time::Duration;

use crate::routes::statistics::{
    compact_target_to_difficulty, difficulty_to_hash_rate, ChartDataPoint, ChartResponse,
    RecentBlockResponse, TxStatsDataPoint, TxStatsResponse,
};
use crate::AppState;

const CACHE_TTL_CHART_SECS: u64 = 300;
const CACHE_TTL_TX_STATS_SECS: u64 = 60;
const CACHE_TTL_RECENT_BLOCKS_SECS: u64 = 10;

const CACHE_KEY_TX_STATS: &str = "stats:tx_stats";
const CACHE_KEY_RECENT_BLOCKS: &str = "stats:recent_blocks";
const CACHE_KEY_CHART_TX_COUNT: &str = "chart:transaction_count";
const CACHE_KEY_CHART_CELL_COUNT: &str = "chart:cell_count";
const CACHE_KEY_CHART_AVG_BLOCK_TIME: &str = "chart:avg_block_time";
const CACHE_KEY_CHART_HASH_RATE: &str = "chart:hash_rate";

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct HourlyTxRow {
    hour_label: String,
    tx_count: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct DailyTxRow {
    day_label: String,
    tx_count: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct RecentBlockRow {
    number: u64,
    hash: String,
    timestamp: i64,
    transactions_count: u32,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct DailyCountRow {
    date: String,
    count: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct DailyAvgBlockTimeRow {
    date: String,
    avg_time: f64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct DailyCompactTargetRow {
    date: String,
    avg_compact_target: f64,
}

pub async fn warmup_chart_caches(state: Arc<AppState>) {
    tracing::info!("Starting cache warmup...");

    tokio::time::sleep(Duration::from_secs(5)).await;

    let results = tokio::join!(
        warmup_tx_stats(&state),
        warmup_recent_blocks(&state),
        warmup_tx_count_chart(&state),
        warmup_cell_count_chart(&state),
        warmup_avg_block_time_chart(&state),
        warmup_hash_rate_chart(&state),
    );

    let success_count = [
        results.0, results.1, results.2, results.3, results.4, results.5,
    ]
    .iter()
    .filter(|r| **r)
    .count();

    tracing::info!(
        "Cache warmup complete: {}/6 caches populated",
        success_count
    );
}

async fn warmup_tx_stats(state: &AppState) -> bool {
    let hourly_query = r#"
        SELECT 
            formatDateTime(fromUnixTimestamp64Milli(b.timestamp), '%H:%M') as hour_label,
            count() as tx_count
        FROM transactions_all t
        INNER JOIN canonical_blocks AS c FINAL ON t.block_number = c.number AND t.block_hash = c.block_hash
        INNER JOIN blocks_all b ON c.number = b.number AND c.block_hash = b.hash
        WHERE b.timestamp >= (toUnixTimestamp64Milli(now64(3)) - 3600000)
        GROUP BY toStartOfFiveMinutes(fromUnixTimestamp64Milli(b.timestamp)), hour_label
        ORDER BY toStartOfFiveMinutes(fromUnixTimestamp64Milli(b.timestamp))
    "#;

    let hourly_rows: Vec<HourlyTxRow> =
        state.pool.query_all(hourly_query).await.unwrap_or_default();

    let daily_query = r#"
        SELECT 
            formatDateTime(fromUnixTimestamp64Milli(b.timestamp), '%m/%d') as day_label,
            count() as tx_count
        FROM transactions_all t
        INNER JOIN canonical_blocks AS c FINAL ON t.block_number = c.number AND t.block_hash = c.block_hash
        INNER JOIN blocks_all b ON c.number = b.number AND c.block_hash = b.hash
        WHERE b.timestamp >= (toUnixTimestamp64Milli(now64(3)) - 86400000)
        GROUP BY toStartOfHour(fromUnixTimestamp64Milli(b.timestamp)), day_label
        ORDER BY toStartOfHour(fromUnixTimestamp64Milli(b.timestamp))
    "#;

    let daily_rows: Vec<DailyTxRow> = state.pool.query_all(daily_query).await.unwrap_or_default();

    let response = TxStatsResponse {
        current_hour: hourly_rows.iter().map(|r| r.tx_count).sum(),
        current_day: daily_rows.iter().map(|r| r.tx_count).sum(),
        hourly_data: hourly_rows
            .into_iter()
            .map(|r| TxStatsDataPoint {
                label: r.hour_label,
                value: r.tx_count,
            })
            .collect(),
        daily_data: daily_rows
            .into_iter()
            .map(|r| TxStatsDataPoint {
                label: r.day_label,
                value: r.tx_count,
            })
            .collect(),
    };

    state
        .cache
        .set(
            CACHE_KEY_TX_STATS,
            &response,
            Duration::from_secs(CACHE_TTL_TX_STATS_SECS),
        )
        .await;

    tracing::debug!("Warmed up tx_stats cache");
    true
}

async fn warmup_recent_blocks(state: &AppState) -> bool {
    let query = r#"
        SELECT 
            b.number as number,
            hex(b.hash) as hash,
            b.timestamp as timestamp,
            b.transactions_count as transactions_count
        FROM blocks_all b
        INNER JOIN canonical_blocks AS c FINAL ON b.number = c.number AND b.hash = c.block_hash
        ORDER BY b.number DESC
        LIMIT 10
    "#;

    let rows: Vec<RecentBlockRow> = match state.pool.query_all(query).await {
        Ok(r) => r,
        Err(_) => return false,
    };

    let blocks: Vec<RecentBlockResponse> = rows
        .into_iter()
        .map(|r| RecentBlockResponse {
            number: r.number,
            hash: format!("0x{}", r.hash.to_lowercase()),
            timestamp: r.timestamp,
            transactions_count: r.transactions_count,
        })
        .collect();

    state
        .cache
        .set(
            CACHE_KEY_RECENT_BLOCKS,
            &blocks,
            Duration::from_secs(CACHE_TTL_RECENT_BLOCKS_SECS),
        )
        .await;

    tracing::debug!("Warmed up recent_blocks cache");
    true
}

async fn warmup_tx_count_chart(state: &AppState) -> bool {
    let query = r#"
        SELECT 
            toString(toDate(fromUnixTimestamp64Milli(b.timestamp))) as date,
            count() as count
        FROM transactions_all t
        INNER JOIN blocks_all b ON t.block_number = b.number AND t.block_hash = b.hash
        WHERE t.block_number >= (SELECT max(number) - 324000 FROM canonical_blocks)
        GROUP BY date
        ORDER BY date DESC
        LIMIT 30
    "#;

    let rows: Vec<DailyCountRow> = match state.pool.query_all(query).await {
        Ok(r) => r,
        Err(_) => return false,
    };

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .rev()
        .map(|r| ChartDataPoint {
            date: r.date,
            value: r.count.to_string(),
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Transaction Count".to_string(),
        y_axis_label: "Transactions".to_string(),
    };

    state
        .cache
        .set(
            CACHE_KEY_CHART_TX_COUNT,
            &response,
            Duration::from_secs(CACHE_TTL_CHART_SECS),
        )
        .await;

    tracing::debug!("Warmed up tx_count chart cache");
    true
}

async fn warmup_cell_count_chart(state: &AppState) -> bool {
    let query = r#"
        SELECT 
            toString(toDate(fromUnixTimestamp64Milli(b.timestamp))) as date,
            count() as count
        FROM cell_outputs_all co
        INNER JOIN blocks_all b ON co.block_number = b.number AND co.block_hash = b.hash
        WHERE co.block_number >= (SELECT max(number) - 324000 FROM canonical_blocks)
        GROUP BY date
        ORDER BY date DESC
        LIMIT 30
    "#;

    let rows: Vec<DailyCountRow> = match state.pool.query_all(query).await {
        Ok(r) => r,
        Err(_) => return false,
    };

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .rev()
        .map(|r| ChartDataPoint {
            date: r.date,
            value: r.count.to_string(),
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Cell Count".to_string(),
        y_axis_label: "Cells".to_string(),
    };

    state
        .cache
        .set(
            CACHE_KEY_CHART_CELL_COUNT,
            &response,
            Duration::from_secs(CACHE_TTL_CHART_SECS),
        )
        .await;

    tracing::debug!("Warmed up cell_count chart cache");
    true
}

async fn warmup_avg_block_time_chart(state: &AppState) -> bool {
    let query = r#"
        SELECT 
            date,
            avg(block_time) / 1000.0 as avg_time
        FROM (
            SELECT 
                toString(toDate(fromUnixTimestamp64Milli(timestamp))) as date,
                neighbor(timestamp, 1) - timestamp as block_time
            FROM (
                SELECT b.timestamp as timestamp
                FROM blocks_all b
                WHERE b.number >= (SELECT max(number) - 324000 FROM canonical_blocks)
                ORDER BY b.number
            )
        )
        WHERE block_time > 0 AND block_time < 600000
        GROUP BY date
        ORDER BY date DESC
        LIMIT 30
    "#;

    let rows: Vec<DailyAvgBlockTimeRow> = match state.pool.query_all(query).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to warmup avg_block_time: {}", e);
            return false;
        }
    };

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .rev()
        .map(|r| ChartDataPoint {
            date: r.date,
            value: format!("{:.2}", r.avg_time),
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Average Block Time".to_string(),
        y_axis_label: "Seconds".to_string(),
    };

    state
        .cache
        .set(
            CACHE_KEY_CHART_AVG_BLOCK_TIME,
            &response,
            Duration::from_secs(CACHE_TTL_CHART_SECS),
        )
        .await;

    tracing::debug!("Warmed up avg_block_time chart cache");
    true
}

async fn warmup_hash_rate_chart(state: &AppState) -> bool {
    let query = r#"
        SELECT 
            toString(toDate(fromUnixTimestamp64Milli(b.timestamp))) as date,
            avg(b.compact_target) as avg_compact_target
        FROM blocks_all b
        WHERE b.number >= (SELECT max(number) - 324000 FROM canonical_blocks)
        GROUP BY date
        ORDER BY date DESC
        LIMIT 30
    "#;

    let rows: Vec<DailyCompactTargetRow> = match state.pool.query_all(query).await {
        Ok(r) => r,
        Err(_) => return false,
    };

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .rev()
        .map(|r| {
            let difficulty = compact_target_to_difficulty(r.avg_compact_target as u64);
            let hash_rate = difficulty_to_hash_rate(difficulty);
            ChartDataPoint {
                date: r.date,
                value: format!("{:.0}", hash_rate),
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Hash Rate".to_string(),
        y_axis_label: "H/s".to_string(),
    };

    state
        .cache
        .set(
            CACHE_KEY_CHART_HASH_RATE,
            &response,
            Duration::from_secs(CACHE_TTL_CHART_SECS),
        )
        .await;

    tracing::debug!("Warmed up hash_rate chart cache");
    true
}
