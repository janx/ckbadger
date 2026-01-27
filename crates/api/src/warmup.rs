use crate::AppState;
use ckbadger_common::dao::GENESIS_BURNT;
use clickhouse::Row;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

const CHART_CACHE_TTL: Duration = Duration::from_secs(3600);

pub async fn warmup_chart_caches(state: Arc<AppState>) {
    info!("Starting cache warmup for charts...");

    macro_rules! warmup {
        ($key:expr, $fn:ident) => {
            if state.cache.get::<serde_json::Value>($key).await.is_none() {
                match $fn(&state).await {
                    Ok(_) => info!("Warmed up cache: {}", $key),
                    Err(e) => tracing::warn!("Failed to warmup {}: {}", $key, e),
                }
            }
        };
    }

    warmup!("chart:average-block-time", warmup_average_block_time);
    warmup!("chart:hash-rate", warmup_hash_rate);
    warmup!("chart:difficulty", warmup_difficulty);
    warmup!("chart:uncle-rate", warmup_uncle_rate);
    warmup!(
        "chart:block-time-distribution",
        warmup_block_time_distribution
    );
    warmup!(
        "chart:epoch-time-distribution",
        warmup_epoch_time_distribution
    );
    warmup!("chart:epoch-time-length", warmup_epoch_time_length);
    warmup!(
        "chart:miner-address-distribution",
        warmup_miner_distribution
    );
    warmup!("chart:total-supply", warmup_total_supply);
    warmup!("chart:secondary-issuance", warmup_secondary_issuance);

    info!("Cache warmup completed");
}

async fn warmup_average_block_time(state: &AppState) -> Result<(), String> {
    #[derive(Row, Deserialize)]
    struct DailyStatsRow {
        date: String,
        avg_block_time_ms: i64,
    }

    let query = "SELECT toString(date) as date, avg_block_time_ms FROM daily_statistics WHERE avg_block_time_ms IS NOT NULL ORDER BY date ASC";

    let rows = state
        .clickhouse
        .client()
        .query(query)
        .fetch_all::<DailyStatsRow>()
        .await
        .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let avg_time_ms = row.avg_block_time_ms as f64;
            serde_json::json!({
                "date": row.date,
                "value": format!("{:.2}", avg_time_ms / 1000.0)
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Average Block Time",
        "yAxisLabel": "Seconds"
    });

    state
        .cache
        .set("chart:average-block-time", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_hash_rate(state: &AppState) -> Result<(), String> {
    #[derive(Row, Deserialize)]
    struct DailyBlockStatsRow {
        date: String,
        avg_compact_target: i64,
        block_count: i64,
    }

    let query = "SELECT toString(date) as date, avg_compact_target, block_count FROM daily_block_stats WHERE date < (SELECT MAX(date) FROM daily_block_stats) ORDER BY date ASC";

    let rows = state
        .clickhouse
        .client()
        .query(query)
        .fetch_all::<DailyBlockStatsRow>()
        .await
        .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let difficulty = compact_to_difficulty(row.avg_compact_target);
            let block_count = row.block_count as f64;
            let avg_block_time = 86400.0 / block_count;
            let hash_rate = difficulty as f64 / avg_block_time;
            serde_json::json!({
                "date": row.date,
                "value": format!("{:.0}", hash_rate)
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Hash Rate",
        "yAxisLabel": "Hash Rate (H/s)"
    });

    state
        .cache
        .set("chart:hash-rate", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_difficulty(state: &AppState) -> Result<(), String> {
    #[derive(Row, Deserialize)]
    struct DifficultyRow {
        date: String,
        avg_compact_target: i64,
    }

    let query = "SELECT toString(date) as date, avg_compact_target FROM daily_block_stats WHERE date < (SELECT MAX(date) FROM daily_block_stats) ORDER BY date ASC";

    let rows = state
        .clickhouse
        .client()
        .query(query)
        .fetch_all::<DifficultyRow>()
        .await
        .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let difficulty = compact_to_difficulty(row.avg_compact_target);
            serde_json::json!({
                "date": row.date,
                "value": difficulty.to_string()
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Difficulty",
        "yAxisLabel": "Difficulty"
    });

    state
        .cache
        .set("chart:difficulty", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_uncle_rate(state: &AppState) -> Result<(), String> {
    #[derive(Row, Deserialize)]
    struct UncleRateRow {
        date: String,
        avg_uncle_rate: f64,
    }

    let query = "SELECT toString(date) as date, avg_uncle_rate FROM daily_block_stats WHERE date < (SELECT MAX(date) FROM daily_block_stats) ORDER BY date ASC";

    let rows = state
        .clickhouse
        .client()
        .query(query)
        .fetch_all::<UncleRateRow>()
        .await
        .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "date": row.date,
                "value": format!("{:.6}", row.avg_uncle_rate)
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Uncle Rate",
        "yAxisLabel": "Uncle Rate"
    });

    state
        .cache
        .set("chart:uncle-rate", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_block_time_distribution(state: &AppState) -> Result<(), String> {
    #[derive(Row, Deserialize)]
    struct BlockTimeDistRow {
        bucket_seconds: i64,
        block_count: i64,
    }

    let query =
        "SELECT bucket_seconds, block_count FROM block_time_distribution ORDER BY bucket_seconds";

    let rows = state
        .clickhouse
        .client()
        .query(query)
        .fetch_all::<BlockTimeDistRow>()
        .await
        .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "date": format!("{}s", row.bucket_seconds),
                "value": row.block_count.to_string()
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Block Time Distribution",
        "yAxisLabel": "Blocks"
    });

    state
        .cache
        .set("chart:block-time-distribution", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_epoch_time_distribution(state: &AppState) -> Result<(), String> {
    #[derive(Row, Deserialize)]
    struct EpochTimeDistRow {
        bucket_minutes: i32,
        epoch_count: i64,
    }

    let query =
        "SELECT bucket_minutes, epoch_count FROM epoch_time_distribution ORDER BY bucket_minutes";

    let rows = state
        .clickhouse
        .client()
        .query(query)
        .fetch_all::<EpochTimeDistRow>()
        .await
        .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let hours = row.bucket_minutes / 60;
            let mins = row.bucket_minutes % 60;
            serde_json::json!({
                "date": format!("{}:{:02}", hours, mins),
                "value": row.epoch_count.to_string()
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Epoch Time Distribution",
        "yAxisLabel": "Epochs"
    });

    state
        .cache
        .set("chart:epoch-time-distribution", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_epoch_time_length(state: &AppState) -> Result<(), String> {
    #[derive(Row, Deserialize)]
    struct EpochTimeLengthRow {
        epoch_number: i64,
        duration_hours: f64,
        blocks_count: i64,
    }

    let query = r#"
        SELECT 
            epoch_number,
            (dateDiff('second', start_timestamp, end_timestamp) / 3600.0) as duration_hours,
            blocks_count
        FROM epoch_statistics
        WHERE end_timestamp IS NOT NULL
        ORDER BY epoch_number ASC
    "#;

    let rows = state
        .clickhouse
        .client()
        .query(query)
        .fetch_all::<EpochTimeLengthRow>()
        .await
        .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "date": row.epoch_number.to_string(),
                "value": format!("{:.2}", row.duration_hours),
                "value2": row.blocks_count.to_string()
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Epoch Time Length",
        "yAxisLabel": "Hours",
        "y2AxisLabel": "Blocks"
    });

    state
        .cache
        .set("chart:epoch-time-length", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_miner_distribution(state: &AppState) -> Result<(), String> {
    #[derive(Row, Deserialize)]
    struct TotalRow {
        total: i64,
    }

    #[derive(Row, Deserialize)]
    struct MinerRow {
        lock_script_hash: String,
        blocks_mined: i64,
    }

    let total_query = "SELECT COALESCE(SUM(blocks_mined), 0) as total FROM miner_statistics";

    let total_rows = state
        .clickhouse
        .client()
        .query(total_query)
        .fetch_all::<TotalRow>()
        .await
        .map_err(|e| e.to_string())?;

    let total_blocks = total_rows.first().map(|r| r.total).unwrap_or(0);

    let query = "SELECT lock_script_hash, blocks_mined FROM miner_statistics ORDER BY blocks_mined DESC LIMIT 100";

    let rows = state
        .clickhouse
        .client()
        .query(query)
        .fetch_all::<MinerRow>()
        .await
        .map_err(|e| e.to_string())?;

    let total = total_blocks as f64;
    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let percentage = if total > 0.0 {
                (row.blocks_mined as f64 / total) * 100.0
            } else {
                0.0
            };
            serde_json::json!({
                "address": row.lock_script_hash,
                "minerName": serde_json::Value::Null,
                "blocksMined": row.blocks_mined,
                "percentage": format!("{:.4}", percentage)
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Miner Address Distribution",
        "totalBlocks": total_blocks
    });

    state
        .cache
        .set(
            "chart:miner-address-distribution",
            &response,
            CHART_CACHE_TTL,
        )
        .await;

    Ok(())
}

async fn warmup_total_supply(state: &AppState) -> Result<(), String> {
    #[derive(Row, Deserialize)]
    struct TotalSupplyRow {
        date: String,
        total_issuance: String,
        total_deposit: String,
        cumulative_burnt: String,
    }

    let query = r#"
        SELECT toString(date) as date, toString(total_issuance) as total_issuance, toString(total_deposit) as total_deposit, COALESCE(toString(cumulative_burnt), '0') as cumulative_burnt
        FROM dao_daily_snapshots
        WHERE total_issuance != 0
        ORDER BY date ASC
    "#;

    let rows = state
        .clickhouse
        .client()
        .query(query)
        .fetch_all::<TotalSupplyRow>()
        .await
        .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .filter_map(|row| {
            let total_issuance: u128 = row.total_issuance.parse().unwrap_or(0);
            let locked: u128 = row.total_deposit.parse().unwrap_or(0);
            let secondary_burnt: u128 = row.cumulative_burnt.parse().unwrap_or(0);
            let total_burnt = GENESIS_BURNT + secondary_burnt;
            let circulating = total_issuance.saturating_sub(total_burnt);
            let liquid = circulating.saturating_sub(locked);

            Some(serde_json::json!({
                "date": row.date,
                "values": {
                    "circulating": shannon_to_ckb(liquid),
                    "locked": shannon_to_ckb(locked),
                    "burnt": shannon_to_ckb(total_burnt)
                }
            }))
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "series": [
            {"key": "circulating", "label": "Circulating", "color": "#00c389"},
            {"key": "locked", "label": "Locked in DAO", "color": "#8b5cf6"},
            {"key": "burnt", "label": "Burnt", "color": "#6b7280"}
        ],
        "title": "Total Supply"
    });

    state
        .cache
        .set("chart:total-supply", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_secondary_issuance(state: &AppState) -> Result<(), String> {
    #[derive(Row, Deserialize)]
    struct SecondaryIssuanceRow {
        date: String,
        cumulative_mining_reward: f64,
        cumulative_deposit_compensation: f64,
        cumulative_burnt: f64,
    }

    let query = r#"
        SELECT 
            toString(date) as date,
            cumulative_mining_reward,
            cumulative_deposit_compensation,
            cumulative_burnt
        FROM dao_daily_snapshots
        WHERE cumulative_burnt IS NOT NULL 
          AND cumulative_mining_reward IS NOT NULL
          AND cumulative_deposit_compensation IS NOT NULL
        ORDER BY date ASC
    "#;

    let rows = state
        .clickhouse
        .client()
        .query(query)
        .fetch_all::<SecondaryIssuanceRow>()
        .await
        .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .filter_map(|row| {
            let mining = row.cumulative_mining_reward;
            let compensation = row.cumulative_deposit_compensation;
            let burnt = row.cumulative_burnt;

            let total = mining + compensation + burnt;
            if total <= 0.0 {
                return None;
            }

            let mining_pct = mining / total * 100.0;
            let compensation_pct = compensation / total * 100.0;
            let burnt_pct = burnt / total * 100.0;

            Some(serde_json::json!({
                "date": row.date,
                "values": {
                    "burnt": format!("{:.2}", burnt_pct),
                    "mining": format!("{:.2}", mining_pct),
                    "compensation": format!("{:.2}", compensation_pct)
                }
            }))
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "series": [
            {"key": "compensation", "label": "Deposit Compensation", "color": "#00c389"},
            {"key": "mining", "label": "Mining Reward", "color": "#8b5cf6"},
            {"key": "burnt", "label": "Burnt", "color": "#6b7280"}
        ],
        "title": "Secondary Issuance"
    });

    state
        .cache
        .set("chart:secondary-issuance", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

fn compact_to_difficulty(compact: i64) -> u64 {
    let compact = compact as u32;
    let exponent = ((compact >> 24) & 0xFF) as i32;
    let mantissa = (compact & 0x00FFFFFF) as f64;

    if mantissa == 0.0 {
        return 0;
    }

    const GENESIS_EXP: i32 = 0x20;
    const GENESIS_MAN: f64 = 0x01_0000 as f64;

    let exp_diff = GENESIS_EXP - exponent;
    let difficulty = if exp_diff >= 0 {
        (GENESIS_MAN / mantissa) * (256.0_f64).powi(exp_diff)
    } else {
        (GENESIS_MAN / mantissa) / (256.0_f64).powi(-exp_diff)
    };

    difficulty as u64
}

fn shannon_to_ckb(shannon: u128) -> String {
    let ckb = shannon / 100_000_000;
    let remainder = shannon % 100_000_000;
    if remainder == 0 {
        format!("{}", ckb)
    } else {
        format!("{}.{:08}", ckb, remainder)
            .trim_end_matches('0')
            .to_string()
    }
}
