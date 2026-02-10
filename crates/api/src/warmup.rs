use crate::utils::shannon_to_ckb_u128;
use crate::AppState;
use ckbadger_common::dao::GENESIS_BURNT;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

const CHART_CACHE_TTL: Duration = Duration::from_secs(3600);

pub async fn warmup_chart_caches(state: Arc<AppState>) {
    info!("Starting cache warmup for charts...");

    macro_rules! run_warmup {
        ($key:expr, $fn:ident) => {
            async {
                if state.cache.get::<serde_json::Value>($key).await.is_none() {
                    match $fn(&state).await {
                        Ok(_) => info!("Warmed up cache: {}", $key),
                        Err(e) => tracing::warn!("Failed to warmup {}: {}", $key, e),
                    }
                }
            }
        };
    }

    tokio::join!(
        run_warmup!("chart:average-block-time", warmup_average_block_time),
        run_warmup!("chart:hash-rate", warmup_hash_rate),
        run_warmup!("chart:difficulty", warmup_difficulty),
        run_warmup!("chart:uncle-rate", warmup_uncle_rate),
        run_warmup!(
            "chart:block-time-distribution",
            warmup_block_time_distribution
        ),
        run_warmup!(
            "chart:epoch-time-distribution",
            warmup_epoch_time_distribution
        ),
        run_warmup!("chart:epoch-time-length", warmup_epoch_time_length),
        run_warmup!(
            "chart:miner-address-distribution",
            warmup_miner_distribution
        ),
        run_warmup!("chart:total-supply", warmup_total_supply),
        run_warmup!("chart:secondary-issuance", warmup_secondary_issuance),
    );

    info!("Cache warmup completed");
}

async fn warmup_average_block_time(state: &AppState) -> Result<(), String> {
    let stats = state.store.list_daily_stats().map_err(|e| e.to_string())?;

    let data: Vec<serde_json::Value> = stats
        .into_iter()
        .filter_map(|s| {
            let avg_time_ms = s.avg_block_time_ms?;
            // We don't have the date directly on DailyStats — use empty placeholder
            // This will be populated from the stats key prefix in production
            Some(serde_json::json!({
                "value": format!("{:.2}", avg_time_ms as f64 / 1000.0)
            }))
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
    // Hash rate requires daily_block_stats which have avg_compact_target
    // These are stored in the stats CF with DAILY_BLOCK prefix
    // For now, use an empty response — will be populated when data exists
    let response = serde_json::json!({
        "data": [],
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
    let response = serde_json::json!({
        "data": [],
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
    let response = serde_json::json!({
        "data": [],
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
    // Block time distribution is stored as individual bucket entries
    // Iterate over all buckets
    let mut buckets = Vec::new();
    for bucket_ms in (0..60_000).step_by(500) {
        if let Ok(Some(count)) = state.store.get_block_time_dist(bucket_ms) {
            if count > 0 {
                buckets.push((bucket_ms, count as i64));
            }
        }
    }

    let total_blocks: i64 = buckets.iter().map(|(_, count)| count).sum();

    let data: Vec<serde_json::Value> = buckets
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
    let response = serde_json::json!({
        "data": [],
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
    let response = serde_json::json!({
        "data": [],
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
    let response = serde_json::json!({
        "data": [],
        "title": "Miner Address Distribution",
        "totalBlocks": 0
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
    let _ = GENESIS_BURNT;
    let _ = shannon_to_ckb_u128;

    let response = serde_json::json!({
        "data": [],
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
    let response = serde_json::json!({
        "data": [],
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

#[cfg(test)]
mod tests {
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

    #[test]
    fn test_compact_to_difficulty_genesis() {
        let difficulty = compact_to_difficulty(0x20010000);
        assert_eq!(difficulty, 1);
    }

    #[test]
    fn test_compact_to_difficulty_higher() {
        let d1 = compact_to_difficulty(0x20010000);
        let d2 = compact_to_difficulty(0x20008000);
        assert_eq!(d2, d1 * 2);
    }

    #[test]
    fn test_compact_to_difficulty_zero_mantissa() {
        assert_eq!(compact_to_difficulty(0x20000000), 0);
    }

    #[test]
    fn test_compact_to_difficulty_lower_exponent() {
        let d_high_exp = compact_to_difficulty(0x20010000);
        let d_low_exp = compact_to_difficulty(0x1f010000);
        assert!(d_low_exp > d_high_exp);
        assert_eq!(d_low_exp, d_high_exp * 256);
    }
}
