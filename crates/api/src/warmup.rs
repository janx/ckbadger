use crate::utils::shannon_to_ckb_u128;
use crate::AppState;
use ckbadger_common::dao::GENESIS_BURNT;
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
    let rows = sqlx::query_as::<_, (chrono::NaiveDate, i32)>(
        "SELECT date, avg_block_time_ms FROM daily_statistics WHERE avg_block_time_ms IS NOT NULL ORDER BY date ASC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(date, avg_time_ms)| {
            serde_json::json!({
                "date": date.format("%Y/%m/%d").to_string(),
                "value": format!("{:.2}", avg_time_ms as f64 / 1000.0)
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
    let rows = sqlx::query_as::<_, (chrono::NaiveDate, i64, i32)>(
        "SELECT date, avg_compact_target, block_count FROM daily_block_stats WHERE date < (SELECT MAX(date) FROM daily_block_stats) ORDER BY date ASC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(date, compact_target, block_count)| {
            let difficulty = compact_to_difficulty(compact_target);
            let avg_block_time = 86400.0 / block_count as f64;
            let hash_rate = difficulty as f64 / avg_block_time;
            serde_json::json!({
                "date": date.format("%Y/%m/%d").to_string(),
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
    let rows = sqlx::query_as::<_, (chrono::NaiveDate, i64)>(
        "SELECT date, avg_compact_target FROM daily_block_stats WHERE date < (SELECT MAX(date) FROM daily_block_stats) ORDER BY date ASC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(date, compact_target)| {
            let difficulty = compact_to_difficulty(compact_target);
            serde_json::json!({
                "date": date.format("%Y/%m/%d").to_string(),
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
    let rows = sqlx::query_as::<_, (chrono::NaiveDate, f64)>(
        "SELECT date, avg_uncle_rate FROM daily_block_stats WHERE date < (SELECT MAX(date) FROM daily_block_stats) ORDER BY date ASC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(date, uncle_rate)| {
            serde_json::json!({
                "date": date.format("%Y/%m/%d").to_string(),
                "value": format!("{:.6}", uncle_rate)
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
    let rows = sqlx::query_as::<_, (i32, i64)>(
        "SELECT bucket_ms, block_count FROM block_time_distribution ORDER BY bucket_ms",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let total_blocks: i64 = rows.iter().map(|(_, count)| count).sum();

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(bucket_ms, count)| {
            let time_seconds = bucket_ms as f64 / 1000.0;
            let ratio = if total_blocks > 0 {
                (count as f64 / total_blocks as f64 * 100.0 * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            serde_json::json!({
                "date": format!("{:.1}", time_seconds),
                "value": format!("{:.3}", ratio)
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Block Time Distribution (Recent 50000 blocks)",
        "yAxisLabel": "Block Ratio (%)"
    });

    state
        .cache
        .set("chart:block-time-distribution", &response, CHART_CACHE_TTL)
        .await;

    Ok(())
}

async fn warmup_epoch_time_distribution(state: &AppState) -> Result<(), String> {
    let rows = sqlx::query_as::<_, (i32, i64)>(
        "SELECT bucket_minutes, epoch_count FROM epoch_time_distribution ORDER BY bucket_minutes",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(bucket_minutes, count)| {
            let hours = bucket_minutes / 60;
            let mins = bucket_minutes % 60;
            serde_json::json!({
                "date": format!("{}:{:02}", hours, mins),
                "value": count.to_string()
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
    let rows = sqlx::query_as::<_, (i64, f64, i32)>(
        r#"
        SELECT 
            epoch_number,
            (EXTRACT(EPOCH FROM (end_timestamp - start_timestamp)) / 3600.0)::float8 as duration_hours,
            blocks_count
        FROM epoch_statistics
        WHERE end_timestamp IS NOT NULL
        ORDER BY epoch_number ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(epoch_number, duration_hours, block_count)| {
            serde_json::json!({
                "date": epoch_number.to_string(),
                "value": format!("{:.2}", duration_hours),
                "value2": block_count.to_string()
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
    let total_blocks: (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(blocks_mined), 0)::bigint FROM miner_statistics")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| e.to_string())?;

    let rows = sqlx::query_as::<_, (Vec<u8>, i64)>(
        "SELECT lock_script_hash, blocks_mined FROM miner_statistics ORDER BY blocks_mined DESC LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let total = total_blocks.0 as f64;
    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(hash, blocks_mined)| {
            let percentage = if total > 0.0 {
                (blocks_mined as f64 / total) * 100.0
            } else {
                0.0
            };
            serde_json::json!({
                "address": format!("0x{}", hex::encode(&hash)),
                "minerName": serde_json::Value::Null,
                "blocksMined": blocks_mined,
                "percentage": format!("{:.4}", percentage)
            })
        })
        .collect();

    let response = serde_json::json!({
        "data": data,
        "title": "Miner Address Distribution",
        "totalBlocks": total_blocks.0
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
    let rows = sqlx::query_as::<_, (chrono::NaiveDate, String, String, String)>(
        r#"
        SELECT date, CAST(total_issuance AS TEXT), CAST(total_deposit AS TEXT), COALESCE(cumulative_burnt, '0')
        FROM dao_daily_snapshots
        WHERE total_issuance != 0
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(
            |(date, total_issuance_str, locked_capacity, cumulative_burnt_str)| {
                let total_issuance: u128 = total_issuance_str.parse().unwrap_or(0);
                let locked: u128 = locked_capacity.parse().unwrap_or(0);
                let secondary_burnt: u128 = cumulative_burnt_str.parse().unwrap_or(0);
                let total_burnt = GENESIS_BURNT + secondary_burnt;
                let circulating = total_issuance.saturating_sub(total_burnt);
                let liquid = circulating.saturating_sub(locked);

                serde_json::json!({
                    "date": date.format("%Y/%m/%d").to_string(),
                    "values": {
                        "circulating": shannon_to_ckb_u128(liquid),
                        "locked": shannon_to_ckb_u128(locked),
                        "burnt": shannon_to_ckb_u128(total_burnt)
                    }
                })
            },
        )
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
    let rows = sqlx::query_as::<
        _,
        (
            chrono::NaiveDate,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(
        r#"
        SELECT 
            date,
            cumulative_mining_reward,
            cumulative_deposit_compensation,
            cumulative_burnt
        FROM dao_daily_snapshots
        WHERE cumulative_burnt IS NOT NULL 
          AND cumulative_mining_reward IS NOT NULL
          AND cumulative_deposit_compensation IS NOT NULL
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .filter_map(|(date, mining_str, compensation_str, burnt_str)| {
            let mining: f64 = mining_str?.parse().ok()?;
            let compensation: f64 = compensation_str?.parse().ok()?;
            let burnt: f64 = burnt_str?.parse().ok()?;

            let total = mining + compensation + burnt;
            if total <= 0.0 {
                return None;
            }

            let mining_pct = mining / total * 100.0;
            let compensation_pct = compensation / total * 100.0;
            let burnt_pct = burnt / total * 100.0;

            Some(serde_json::json!({
                "date": date.format("%Y/%m/%d").to_string(),
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
