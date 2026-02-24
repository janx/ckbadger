#![allow(clippy::type_complexity)]

use axum::{extract::State, routing::get, Router};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use ckb_types::utilities::compact_to_difficulty as ckb_compact_to_difficulty;
use ckbadger_common::dao::GENESIS_BURNT;
use ckbadger_common::sync::{
    format_duration_smart, SyncProgressData, SyncStatusData, SYNC_PROGRESS_REDIS_KEY,
    SYNC_STATUS_REDIS_KEY,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::cache::{CacheKeys, CacheTtl};
use crate::response::{ok, ApiError, ApiResult};
use crate::utils::{
    apply_live_capacity_delta, format_duration, resolve_dob_collection_name,
    resolve_nft_collection_name,
};
use crate::AppState;

type ApiRouteError = (axum::http::StatusCode, axum::Json<ApiError>);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/statistics/network", get(get_network_stats))
        .route("/statistics/tx-stats", get(get_tx_stats))
        .route("/statistics/recent-blocks", get(get_recent_blocks))
        .route(
            "/charts/transaction-count",
            get(get_transaction_count_chart),
        )
        .route("/charts/cell-count", get(get_cell_count_chart))
        .route("/charts/knowledge-size", get(get_knowledge_size_chart))
        .route(
            "/charts/common-knowledge-composition",
            get(get_common_knowledge_composition_chart),
        )
        .route(
            "/charts/cell-age-vs-occupied-capacity",
            get(get_cell_age_vs_occupied_capacity_chart),
        )
        .route(
            "/charts/capacity-turnover-ratio",
            get(get_capacity_turnover_ratio_chart),
        )
        .route(
            "/charts/cell-size-distribution",
            get(get_cell_size_distribution_chart),
        )
        .route(
            "/charts/address-cohort-retention",
            get(get_address_cohort_retention_chart),
        )
        .route(
            "/charts/most-utilized-scripts",
            get(get_most_utilized_scripts_chart),
        )
        .route(
            "/charts/most-utilized-assets",
            get(get_most_utilized_assets_chart),
        )
        .route(
            "/charts/block-time-distribution",
            get(get_block_time_distribution_chart),
        )
        .route(
            "/charts/epoch-time-distribution",
            get(get_epoch_time_distribution_chart),
        )
        .route(
            "/charts/epoch-time-length",
            get(get_epoch_time_length_chart),
        )
        .route(
            "/charts/average-block-time",
            get(get_average_block_time_chart),
        )
        .route("/charts/hash-rate", get(get_hash_rate_chart))
        .route("/charts/difficulty", get(get_difficulty_chart))
        .route("/charts/uncle-rate", get(get_uncle_rate_chart))
        .route(
            "/charts/miner-address-distribution",
            get(get_miner_address_distribution_chart),
        )
        // Economics charts
        .route("/charts/total-supply", get(get_total_supply_chart))
        .route("/charts/nominal-apc", get(get_nominal_apc_chart))
        .route(
            "/charts/secondary-issuance",
            get(get_secondary_issuance_chart),
        )
        .route("/charts/inflation-rate", get(get_inflation_rate_chart))
        .route("/charts/hodl-wave", get(get_hodl_wave_chart))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub is_syncing: bool,
    pub synced_block: i64,
    pub tip_block: i64,
    pub progress: f64,
    pub estimated_time: Option<String>,
    pub chart_data_may_be_incomplete: bool,
    pub blocks_per_second: Option<f64>,
    pub ema_blocks_per_second: Option<f64>,
    pub sync_mode: String,
    pub started_at: Option<i64>,
    pub elapsed_time: Option<String>,
    pub total_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepForkStatus {
    pub detected: bool,
    pub detected_at: Option<DateTime<Utc>>,
    pub depth: Option<i32>,
    pub db_tip: Option<i64>,
    pub chain_tip: Option<i64>,
    pub fork_point: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStats {
    pub latest_block: i64,
    pub avg_block_time: String,
    pub hash_rate: String,
    pub difficulty: String,
    pub epoch: String,
    pub tps: String,
    pub estimated_epoch_time: String,
    pub transactions_per_minute: String,
    pub transactions_per_day: String,
    pub sync_status: SyncStatus,
    pub deep_fork_status: DeepForkStatus,
}

async fn get_network_stats(State(state): State<Arc<AppState>>) -> ApiResult<NetworkStats> {
    if let Some(cached) = state
        .cache
        .get::<NetworkStats>(CacheKeys::NETWORK_STATS)
        .await
    {
        return ok(cached);
    }

    let stats = fetch_network_stats_from_db(&state).await?;

    state
        .cache
        .set(CacheKeys::NETWORK_STATS, &stats, CacheTtl::NETWORK_STATS)
        .await;

    ok(stats)
}

async fn get_tx_stats(State(state): State<Arc<AppState>>) -> ApiResult<TxStatsResponse> {
    let cache_key = "statistics:tx-stats";
    if let Some(cached) = state.cache.get::<TxStatsResponse>(cache_key).await {
        return ok(cached);
    }

    // Get the latest block header to determine reference time
    let store = state.store.clone();
    let latest_header = store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_time = latest_header
        .as_ref()
        .and_then(|(_, h)| DateTime::from_timestamp_millis(h.timestamp))
        .unwrap_or_else(Utc::now);
    let reference_ts = reference_time.timestamp() * 1000; // ms

    // Get hourly stats (last 24 hours)
    let hourly_stats = store
        .list_hourly_stats_with_keys()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let cutoff_24h = reference_ts - 24 * 3600 * 1000;

    // Filter hourly stats to last 24 hours
    let mut recent_hourly: Vec<(String, ckbadger_store::HourlyStats)> = hourly_stats
        .into_iter()
        .filter(|(_, h)| h.hour * 1000 > cutoff_24h && h.hour * 1000 <= reference_ts)
        .collect();
    recent_hourly.sort_by(|a, b| b.1.hour.cmp(&a.1.hour)); // desc
    recent_hourly.truncate(24);

    // Get daily stats (last 14 days)
    let daily_stats = store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_date = ckbadger_common::block_date(reference_time);
    let cutoff_date = reference_date - chrono::Duration::days(14);

    let mut recent_daily: Vec<(String, ckbadger_store::DailyStats)> = daily_stats
        .into_iter()
        .filter(|(date_str, _)| {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y%m%d") {
                date > cutoff_date && date <= reference_date
            } else {
                false
            }
        })
        .collect();
    recent_daily.sort_by(|a, b| b.0.cmp(&a.0)); // desc
    recent_daily.truncate(14);

    let txs_this_hour: i64 = recent_hourly
        .first()
        .map(|(_, h)| h.transactions_count as i64)
        .unwrap_or(0);
    let txs_in_24_hours: i64 = recent_hourly
        .iter()
        .map(|(_, h)| h.transactions_count as i64)
        .sum();

    let hourly_data: Vec<TxStatsDataPoint> = recent_hourly
        .into_iter()
        .rev()
        .map(|(_, h)| {
            let dt = DateTime::from_timestamp(h.hour, 0).unwrap_or_default();
            TxStatsDataPoint {
                label: dt.format("%H:00").to_string(),
                value: h.transactions_count as i64,
            }
        })
        .collect();

    let daily_data: Vec<TxStatsDataPoint> = recent_daily
        .into_iter()
        .rev()
        .map(|(date_str, stats)| {
            let label = if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y%m%d") {
                date.format("%m/%d").to_string()
            } else {
                date_str
            };
            TxStatsDataPoint {
                label,
                value: stats.transactions_count as i64,
            }
        })
        .collect();

    let response = TxStatsResponse {
        current_hour: txs_this_hour,
        current_day: txs_in_24_hours,
        hourly_data,
        daily_data,
    };

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(10))
        .await;

    ok(response)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentBlockItem {
    pub timestamp: i64,
    pub transactions_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentBlocksResponse {
    pub blocks: Vec<RecentBlockItem>,
}

async fn get_recent_blocks(State(state): State<Arc<AppState>>) -> ApiResult<RecentBlocksResponse> {
    let cache_key = "statistics:recent-blocks";
    if let Some(cached) = state.cache.get::<RecentBlocksResponse>(cache_key).await {
        return ok(cached);
    }

    let store = state.store.clone();

    // Get the latest block to find the reference time
    let latest = store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_ts = latest
        .as_ref()
        .map(|(_, h)| h.timestamp)
        .unwrap_or_else(|| Utc::now().timestamp_millis());

    let cutoff_ts = reference_ts - 24 * 3600 * 1000; // 24 hours ago in ms

    // Get blocks for last 24 hours
    // Estimate: ~8640 blocks in 24h at ~10s/block
    let blocks_desc = store
        .list_blocks_desc(None, 10000)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut blocks: Vec<RecentBlockItem> = blocks_desc
        .into_iter()
        .filter(|(_, h)| h.timestamp > cutoff_ts)
        .map(|(_, h)| RecentBlockItem {
            timestamp: h.timestamp,
            transactions_count: h.transactions_count,
        })
        .collect();

    // Reverse to ascending order
    blocks.reverse();

    let response = RecentBlocksResponse { blocks };

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(10))
        .await;

    ok(response)
}

fn compact_to_difficulty(compact: i64) -> u64 {
    let difficulty = ckb_compact_to_difficulty(compact as u32);
    difficulty.to_string().parse::<u64>().unwrap_or(u64::MAX)
}

fn format_hash_rate(hash_rate: f64) -> String {
    const KILO: f64 = 1_000.0;
    const MEGA: f64 = 1_000_000.0;
    const GIGA: f64 = 1_000_000_000.0;
    const TERA: f64 = 1_000_000_000_000.0;
    const PETA: f64 = 1_000_000_000_000_000.0;
    const EXA: f64 = 1_000_000_000_000_000_000.0;

    if hash_rate >= EXA {
        format!("{:.2} EH/s", hash_rate / EXA)
    } else if hash_rate >= PETA {
        format!("{:.2} PH/s", hash_rate / PETA)
    } else if hash_rate >= TERA {
        format!("{:.2} TH/s", hash_rate / TERA)
    } else if hash_rate >= GIGA {
        format!("{:.2} GH/s", hash_rate / GIGA)
    } else if hash_rate >= MEGA {
        format!("{:.2} MH/s", hash_rate / MEGA)
    } else if hash_rate >= KILO {
        format!("{:.2} KH/s", hash_rate / KILO)
    } else {
        format!("{:.2} H/s", hash_rate)
    }
}

fn format_difficulty(difficulty: u64) -> String {
    const KILO: u64 = 1_000;
    const MEGA: u64 = 1_000_000;
    const GIGA: u64 = 1_000_000_000;
    const TERA: u64 = 1_000_000_000_000;
    const PETA: u64 = 1_000_000_000_000_000;
    const EXA: u64 = 1_000_000_000_000_000_000;

    if difficulty >= EXA {
        format!("{:.2} E", difficulty as f64 / EXA as f64)
    } else if difficulty >= PETA {
        format!("{:.2} P", difficulty as f64 / PETA as f64)
    } else if difficulty >= TERA {
        format!("{:.2} T", difficulty as f64 / TERA as f64)
    } else if difficulty >= GIGA {
        format!("{:.2} G", difficulty as f64 / GIGA as f64)
    } else if difficulty >= MEGA {
        format!("{:.2} M", difficulty as f64 / MEGA as f64)
    } else if difficulty >= KILO {
        format!("{:.2} K", difficulty as f64 / KILO as f64)
    } else {
        format!("{}", difficulty)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxStatsDataPoint {
    pub label: String,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxStatsResponse {
    pub current_hour: i64,
    pub current_day: i64,
    pub hourly_data: Vec<TxStatsDataPoint>,
    pub daily_data: Vec<TxStatsDataPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartDataPoint {
    pub date: String,
    pub value: String,
    pub value2: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartResponse {
    pub data: Vec<ChartDataPoint>,
    pub title: String,
    pub y_axis_label: String,
    pub y2_axis_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackedAreaDataPoint {
    pub date: String,
    pub values: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackedAreaSeries {
    pub key: String,
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackedAreaChartResponse {
    pub data: Vec<StackedAreaDataPoint>,
    pub series: Vec<StackedAreaSeries>,
    pub title: String,
}

const MOST_UTILIZED_LIMIT: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MostUtilizedScriptsChartResponse {
    pub title: String,
    pub occupied_share: StackedAreaChartResponse,
    pub capacity_share: StackedAreaChartResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MostUtilizedAssetsChartResponse {
    pub title: String,
    pub occupied_share: StackedAreaChartResponse,
    pub capacity_share: StackedAreaChartResponse,
}

#[derive(Debug, Clone, Copy)]
enum UtilizationMetric {
    Occupied,
    Capacity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EntityState {
    total_cells_capacity: i128,
    occupied_capacity: i128,
}

#[derive(Debug, Clone)]
struct ScriptEntity {
    key: String,
    final_total_cells_capacity: i128,
    final_occupied_capacity: i128,
}

#[derive(Debug, Clone)]
struct AssetEntity {
    key: String,
    final_total_cells_capacity: i128,
    final_occupied_capacity: i128,
}

fn is_known_script_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("unknown")
}

fn format_asset_label(name: &str, asset_type: &str) -> String {
    format!("{name} ({asset_type})")
}

fn metric_value(state: &EntityState, metric: UtilizationMetric) -> i128 {
    match metric {
        UtilizationMetric::Occupied => state.occupied_capacity,
        UtilizationMetric::Capacity => state.total_cells_capacity,
    }
}

fn utilization_palette(index: usize) -> &'static str {
    const PALETTE: [&str; 20] = [
        "#00c389", "#f59e0b", "#3b82f6", "#ef4444", "#8b5cf6", "#14b8a6", "#f97316", "#84cc16",
        "#ec4899", "#06b6d4", "#eab308", "#22c55e", "#6366f1", "#fb7185", "#10b981", "#f43f5e",
        "#0ea5e9", "#a855f7", "#65a30d", "#f97316",
    ];
    PALETTE[index % PALETTE.len()]
}

fn top_keys_by_metric<T>(
    entities: &[T],
    metric: UtilizationMetric,
    key_of: impl Fn(&T) -> &str,
    occupied_of: impl Fn(&T) -> i128,
    capacity_of: impl Fn(&T) -> i128,
) -> Vec<String> {
    let mut keys: Vec<&T> = entities.iter().collect();
    keys.sort_by(|a, b| {
        let a_metric = match metric {
            UtilizationMetric::Occupied => occupied_of(a),
            UtilizationMetric::Capacity => capacity_of(a),
        };
        let b_metric = match metric {
            UtilizationMetric::Occupied => occupied_of(b),
            UtilizationMetric::Capacity => capacity_of(b),
        };
        let a_secondary = match metric {
            UtilizationMetric::Occupied => capacity_of(a),
            UtilizationMetric::Capacity => occupied_of(a),
        };
        let b_secondary = match metric {
            UtilizationMetric::Occupied => capacity_of(b),
            UtilizationMetric::Capacity => occupied_of(b),
        };
        b_metric
            .cmp(&a_metric)
            .then_with(|| b_secondary.cmp(&a_secondary))
            .then_with(|| key_of(a).cmp(key_of(b)))
    });

    keys.into_iter()
        .take(MOST_UTILIZED_LIMIT)
        .map(|entry| key_of(entry).to_string())
        .collect()
}

fn apply_capacity_delta_i128(
    total_cells_capacity: i128,
    occupied_capacity: i128,
    capacity_delta: i128,
    occupied_delta: i128,
    context: &str,
) -> Result<(i128, i128), ApiRouteError> {
    apply_live_capacity_delta(
        total_cells_capacity,
        occupied_capacity,
        capacity_delta,
        occupied_delta,
        context,
    )
    .map_err(|e| ApiError::internal(e.to_string()))
}

fn accumulate_capacity_deltas<I>(deltas: I) -> Result<(i128, i128), ApiRouteError>
where
    I: IntoIterator<Item = (i128, i128)>,
{
    let mut total_cells_capacity: i128 = 0;
    let mut occupied_capacity: i128 = 0;

    for (idx, (capacity_delta, occupied_delta)) in deltas.into_iter().enumerate() {
        (total_cells_capacity, occupied_capacity) = apply_live_capacity_delta(
            total_cells_capacity,
            occupied_capacity,
            capacity_delta,
            occupied_delta,
            &format!("accumulating capacity deltas at step {}", idx + 1),
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
    }

    Ok((total_cells_capacity, occupied_capacity))
}

fn build_most_utilized_share_chart(
    chart_title: String,
    metric: UtilizationMetric,
    top_keys: &[String],
    labels_by_key: &HashMap<String, String>,
    dates: &[u32],
    deltas_by_date: &BTreeMap<u32, Vec<(String, i128, i128)>>,
) -> Result<StackedAreaChartResponse, ApiRouteError> {
    let mut states: HashMap<String, EntityState> = HashMap::new();
    let mut total_cells_capacity: i128 = 0;
    let mut total_occupied_capacity: i128 = 0;

    let mut series: Vec<StackedAreaSeries> = top_keys
        .iter()
        .enumerate()
        .map(|(index, key)| StackedAreaSeries {
            key: format!("top{index}"),
            label: labels_by_key
                .get(key)
                .cloned()
                .unwrap_or_else(|| key.clone()),
            color: utilization_palette(index).to_string(),
        })
        .collect();
    series.push(StackedAreaSeries {
        key: "others".to_string(),
        label: "Others".to_string(),
        color: "#64748b".to_string(),
    });

    let mut data: Vec<StackedAreaDataPoint> = Vec::with_capacity(dates.len());
    for date in dates {
        if let Some(deltas) = deltas_by_date.get(date) {
            for (entity_key, capacity_delta, occupied_delta) in deltas {
                let state = states.entry(entity_key.clone()).or_insert(EntityState {
                    total_cells_capacity: 0,
                    occupied_capacity: 0,
                });

                let old_capacity = state.total_cells_capacity;
                let old_occupied = state.occupied_capacity;

                let (new_capacity, new_occupied) = apply_live_capacity_delta(
                    old_capacity,
                    old_occupied,
                    *capacity_delta,
                    *occupied_delta,
                    &format!(
                        "building most-utilized share chart for {} on date {}",
                        entity_key, date
                    ),
                )
                .map_err(|e| ApiError::internal(e.to_string()))?;

                state.total_cells_capacity = new_capacity;
                state.occupied_capacity = new_occupied;

                total_cells_capacity += new_capacity - old_capacity;
                total_occupied_capacity += new_occupied - old_occupied;
            }
        }

        let mut values: HashMap<String, String> = HashMap::new();
        let mut selected_sum: i128 = 0;
        for (index, key) in top_keys.iter().enumerate() {
            let value = states
                .get(key)
                .map(|state| metric_value(state, metric))
                .unwrap_or(0);
            if value < 0 {
                return Err(ApiError::internal(format!(
                    "negative metric value for top key '{}' on date {}: {}",
                    key, date, value
                )));
            }
            selected_sum += value;
            values.insert(format!("top{index}"), value.to_string());
        }

        let total = match metric {
            UtilizationMetric::Occupied => total_occupied_capacity,
            UtilizationMetric::Capacity => total_cells_capacity,
        };
        if total < 0 {
            return Err(ApiError::internal(format!(
                "negative total metric on date {}: {}",
                date, total
            )));
        }
        if selected_sum > total {
            return Err(ApiError::internal(format!(
                "selected sum exceeds total on date {}: selected={}, total={}",
                date, selected_sum, total
            )));
        }
        let others = total - selected_sum;
        values.insert("others".to_string(), others.to_string());

        data.push(StackedAreaDataPoint {
            date: format_date_key(&format!("{date:08}")),
            values,
        });
    }

    Ok(StackedAreaChartResponse {
        data,
        series,
        title: chart_title,
    })
}

async fn get_most_utilized_scripts_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<MostUtilizedScriptsChartResponse> {
    let cache_key = "chart:most-utilized-scripts:v2";
    if let Some(cached) = state
        .cache
        .get::<MostUtilizedScriptsChartResponse>(cache_key)
        .await
    {
        return ok(cached);
    }

    let all_scripts = state
        .store
        .list_script_infos()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut labels_by_key: HashMap<String, String> = HashMap::new();
    let mut final_by_key: HashMap<String, (i128, i128)> = HashMap::new();
    let mut deltas_by_date: BTreeMap<u32, Vec<(String, i128, i128)>> = BTreeMap::new();

    for (code_hash, info) in all_scripts {
        let code_hash_hex = format!("0x{}", hex::encode(&code_hash));
        let raw_name = info.name.as_deref().unwrap_or("Unknown").trim();
        let is_known_script = is_known_script_name(raw_name);
        let key = if is_known_script {
            format!("known:{raw_name}")
        } else {
            format!("unknown:{code_hash_hex}")
        };

        let label = if is_known_script {
            raw_name.to_string()
        } else {
            code_hash_hex.clone()
        };
        labels_by_key.insert(key.clone(), label);

        let final_total_cells_capacity = info.lock_live_capacity_sum + info.type_live_capacity_sum;
        let final_occupied_capacity =
            info.lock_live_occupied_capacity_sum + info.type_live_occupied_capacity_sum;
        if final_total_cells_capacity < 0 {
            return Err(ApiError::internal(format!(
                "negative script total capacity for key {}: {}",
                key, final_total_cells_capacity
            )));
        }
        if final_occupied_capacity < 0 {
            return Err(ApiError::internal(format!(
                "negative script occupied capacity for key {}: {}",
                key, final_occupied_capacity
            )));
        }
        if final_occupied_capacity > final_total_cells_capacity {
            return Err(ApiError::internal(format!(
                "script occupied capacity exceeds total for key {}: occupied={}, total={}",
                key, final_occupied_capacity, final_total_cells_capacity
            )));
        }
        let entry = final_by_key.entry(key.clone()).or_insert((0, 0));
        entry.0 += final_total_cells_capacity;
        entry.1 += final_occupied_capacity;

        for is_type in [false, true] {
            let deltas = state
                .store
                .list_script_daily_deltas(&code_hash, is_type)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            for (date, delta) in deltas {
                deltas_by_date.entry(date).or_default().push((
                    key.clone(),
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                ));
            }
        }
    }

    let entities_unfiltered: Vec<ScriptEntity> = final_by_key
        .iter()
        .map(|(key, (capacity, occupied))| -> Result<ScriptEntity, ApiRouteError> {
            if *capacity < 0 {
                return Err(ApiError::internal(format!(
                    "negative aggregated script total capacity for key {}: {}",
                    key, capacity
                )));
            }
            if *occupied < 0 {
                return Err(ApiError::internal(format!(
                    "negative aggregated script occupied capacity for key {}: {}",
                    key, occupied
                )));
            }
            if *occupied > *capacity {
                return Err(ApiError::internal(format!(
                    "aggregated script occupied capacity exceeds total for key {}: occupied={}, total={}",
                    key, occupied, capacity
                )));
            }
            Ok(ScriptEntity {
                key: key.clone(),
                final_total_cells_capacity: *capacity,
                final_occupied_capacity: *occupied,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let entities: Vec<ScriptEntity> = entities_unfiltered
        .into_iter()
        .filter(|entity| {
            entity.final_total_cells_capacity > 0 || entity.final_occupied_capacity > 0
        })
        .collect();

    let top_occupied_keys = top_keys_by_metric(
        &entities,
        UtilizationMetric::Occupied,
        |entity| &entity.key,
        |entity| entity.final_occupied_capacity,
        |entity| entity.final_total_cells_capacity,
    );
    let top_capacity_keys = top_keys_by_metric(
        &entities,
        UtilizationMetric::Capacity,
        |entity| &entity.key,
        |entity| entity.final_occupied_capacity,
        |entity| entity.final_total_cells_capacity,
    );

    let dates: Vec<u32> = deltas_by_date.keys().copied().collect();
    let occupied_share = build_most_utilized_share_chart(
        "Top Scripts Occupied Share".to_string(),
        UtilizationMetric::Occupied,
        &top_occupied_keys,
        &labels_by_key,
        &dates,
        &deltas_by_date,
    )?;
    let capacity_share = build_most_utilized_share_chart(
        "Top Scripts Capacity Share".to_string(),
        UtilizationMetric::Capacity,
        &top_capacity_keys,
        &labels_by_key,
        &dates,
        &deltas_by_date,
    )?;

    let response = MostUtilizedScriptsChartResponse {
        title: "Scripts Occupied & Total CKBytes".to_string(),
        occupied_share,
        capacity_share,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    ok(response)
}

async fn get_most_utilized_assets_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<MostUtilizedAssetsChartResponse> {
    let cache_key = "chart:most-utilized-assets:v2";
    if let Some(cached) = state
        .cache
        .get::<MostUtilizedAssetsChartResponse>(cache_key)
        .await
    {
        return ok(cached);
    }

    let mut labels_by_key: HashMap<String, String> = HashMap::new();
    let mut entities: Vec<AssetEntity> = Vec::new();
    let mut deltas_by_date: BTreeMap<u32, Vec<(String, i128, i128)>> = BTreeMap::new();

    let tokens = state
        .store
        .list_tokens()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    for (type_hash, info) in tokens {
        if info.name.is_none() && info.symbol.is_none() && info.holders_count == 0 {
            continue;
        }
        let deltas = state
            .store
            .list_token_daily_deltas(&type_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (total_cells_capacity, occupied_capacity) =
            accumulate_capacity_deltas(deltas.iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))?;
        if total_cells_capacity <= 0 && occupied_capacity <= 0 {
            continue;
        }
        let id = format!("0x{}", hex::encode(&type_hash));
        let name = info
            .symbol
            .clone()
            .or_else(|| info.name.clone())
            .unwrap_or_else(|| id.clone());
        let entity_key = format!("token:{id}");
        labels_by_key.insert(entity_key.clone(), format_asset_label(&name, "token"));
        if occupied_capacity > total_cells_capacity {
            return Err(ApiError::internal(format!(
                "token occupied capacity exceeds total for {}: occupied={}, total={}",
                entity_key, occupied_capacity, total_cells_capacity
            )));
        }
        entities.push(AssetEntity {
            key: entity_key.clone(),
            final_total_cells_capacity: total_cells_capacity,
            final_occupied_capacity: occupied_capacity,
        });
        for (date, delta) in deltas {
            deltas_by_date.entry(date).or_default().push((
                entity_key.clone(),
                delta.live_capacity_delta,
                delta.live_occupied_capacity_delta,
            ));
        }
    }

    let clusters = state
        .store
        .list_cluster_aggregates()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    for (cluster_id, agg) in clusters {
        if agg.total_count == 0 {
            continue;
        }
        let deltas = state
            .store
            .list_cluster_daily_deltas(&cluster_id)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (total_cells_capacity, occupied_capacity) =
            accumulate_capacity_deltas(deltas.iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))?;
        if total_cells_capacity <= 0 && occupied_capacity <= 0 {
            continue;
        }

        let id = format!("0x{}", hex::encode(&cluster_id));
        let name =
            resolve_dob_collection_name(state.store.as_ref(), &cluster_id, agg.name.as_deref())
                .unwrap_or_else(|| id.clone());
        let entity_key = format!("dob:{id}");
        labels_by_key.insert(entity_key.clone(), format_asset_label(&name, "nft"));
        if occupied_capacity > total_cells_capacity {
            return Err(ApiError::internal(format!(
                "DOB occupied capacity exceeds total for {}: occupied={}, total={}",
                entity_key, occupied_capacity, total_cells_capacity
            )));
        }
        entities.push(AssetEntity {
            key: entity_key.clone(),
            final_total_cells_capacity: total_cells_capacity,
            final_occupied_capacity: occupied_capacity,
        });
        for (date, delta) in deltas {
            deltas_by_date.entry(date).or_default().push((
                entity_key.clone(),
                delta.live_capacity_delta,
                delta.live_occupied_capacity_delta,
            ));
        }
    }

    let nft_collections = state
        .store
        .list_nft_collection_aggregates()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    for (collection_id, agg) in nft_collections {
        if agg.total_count == 0 {
            continue;
        }
        let deltas = state
            .store
            .list_nft_daily_deltas(&collection_id)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (total_cells_capacity, occupied_capacity) =
            accumulate_capacity_deltas(deltas.iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))?;
        if total_cells_capacity <= 0 && occupied_capacity <= 0 {
            continue;
        }

        let id = format!("0x{}", hex::encode(&collection_id));
        let standard = agg.standard.asset_standard().to_string();
        let name = resolve_nft_collection_name(&standard, agg.name.as_deref())
            .unwrap_or_else(|| id.clone());
        let entity_key = format!("nft:{id}");
        labels_by_key.insert(entity_key.clone(), format_asset_label(&name, "nft"));
        if occupied_capacity > total_cells_capacity {
            return Err(ApiError::internal(format!(
                "NFT occupied capacity exceeds total for {}: occupied={}, total={}",
                entity_key, occupied_capacity, total_cells_capacity
            )));
        }
        entities.push(AssetEntity {
            key: entity_key.clone(),
            final_total_cells_capacity: total_cells_capacity,
            final_occupied_capacity: occupied_capacity,
        });
        for (date, delta) in deltas {
            deltas_by_date.entry(date).or_default().push((
                entity_key.clone(),
                delta.live_capacity_delta,
                delta.live_occupied_capacity_delta,
            ));
        }
    }

    let top_occupied_keys = top_keys_by_metric(
        &entities,
        UtilizationMetric::Occupied,
        |entity| &entity.key,
        |entity| entity.final_occupied_capacity,
        |entity| entity.final_total_cells_capacity,
    );
    let top_capacity_keys = top_keys_by_metric(
        &entities,
        UtilizationMetric::Capacity,
        |entity| &entity.key,
        |entity| entity.final_occupied_capacity,
        |entity| entity.final_total_cells_capacity,
    );

    let dates: Vec<u32> = deltas_by_date.keys().copied().collect();
    let occupied_share = build_most_utilized_share_chart(
        "Top Assets Occupied Share".to_string(),
        UtilizationMetric::Occupied,
        &top_occupied_keys,
        &labels_by_key,
        &dates,
        &deltas_by_date,
    )?;
    let capacity_share = build_most_utilized_share_chart(
        "Top Assets Capacity Share".to_string(),
        UtilizationMetric::Capacity,
        &top_capacity_keys,
        &labels_by_key,
        &dates,
        &deltas_by_date,
    )?;

    let response = MostUtilizedAssetsChartResponse {
        title: "Assets Occupied & Total CKBytes".to_string(),
        occupied_share,
        capacity_share,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    ok(response)
}

async fn get_transaction_count_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let daily_stats = state
        .store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = daily_stats
        .into_iter()
        .map(|(date_str, stats)| {
            let formatted_date = format_date_for_chart(&date_str);
            ChartDataPoint {
                date: formatted_date,
                value: stats.transactions_count.to_string(),
                value2: None,
            }
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Transaction Count".to_string(),
        y_axis_label: "Transactions".to_string(),
        y2_axis_label: None,
    })
}

async fn get_cell_count_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    let daily_stats = state
        .store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // The stored values are per-day deltas (cells created/consumed that day).
    // Compute cumulative running totals to match the official explorer.
    let mut cum_live: i64 = 0;
    let mut cum_dead: i64 = 0;
    let mut cum_all: i64 = 0;

    let data: Vec<StackedAreaDataPoint> = daily_stats
        .into_iter()
        .filter_map(|(date_str, stats)| {
            cum_live += stats.total_live_cells;
            cum_dead += stats.total_dead_cells;
            cum_all += stats.total_all_cells;

            if cum_all <= 0 {
                return None;
            }

            let mut values = std::collections::HashMap::new();
            values.insert("allCells".to_string(), cum_all.to_string());
            values.insert("liveCells".to_string(), cum_live.to_string());
            values.insert("deadCells".to_string(), cum_dead.to_string());
            Some(StackedAreaDataPoint {
                date: format_date_for_chart(&date_str),
                values,
            })
        })
        .collect();

    let series = vec![
        StackedAreaSeries {
            key: "allCells".to_string(),
            label: "All Cells".to_string(),
            color: "#6b7280".to_string(),
        },
        StackedAreaSeries {
            key: "liveCells".to_string(),
            label: "Live Cells".to_string(),
            color: "#00c389".to_string(),
        },
        StackedAreaSeries {
            key: "deadCells".to_string(),
            label: "Dead Cells".to_string(),
            color: "#ef4444".to_string(),
        },
    ];

    ok(StackedAreaChartResponse {
        data,
        series,
        title: "Cell Count".to_string(),
    })
}

async fn get_knowledge_size_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:knowledge-size:v2";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_stats = state
        .store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let snapshots = state
        .store
        .list_dao_daily_snapshots()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let circulating_by_date = build_circulating_supply_by_date_map(&snapshots)?;

    let data: Vec<ChartDataPoint> = daily_stats
        .into_iter()
        .filter_map(|(date_str, stats)| {
            let snapshot_date = format_date_key(&date_str);
            stats.knowledge_size.map(|ks| ChartDataPoint {
                date: snapshot_date.clone(),
                value: shannon_to_ckb_string(ks),
                value2: Some(
                    circulating_by_date
                        .get(&snapshot_date)
                        .map(|circulating| {
                            if *circulating > 0 {
                                format!("{:.4}", ks as f64 * 100.0 / *circulating as f64)
                            } else {
                                "0.0000".to_string()
                            }
                        })
                        .unwrap_or_else(|| "0.0000".to_string()),
                ),
            })
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Common Knowledge Size".to_string(),
        y_axis_label: "CKB".to_string(),
        y2_axis_label: Some("Utilization (%)".to_string()),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

const SHANNONS_PER_CKB: i128 = 100_000_000;

const DAO_CODE_HASHES: &[&str] =
    &["0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e"];

const UDT_CODE_HASHES: &[&str] = &[
    "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5",
    "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95",
    "0x25c29dc317811a6f6f3985a7a9ebc4838bd388d19d0feeecf0bcd60f6c0975bb",
];

const NFT_SPORE_CODE_HASHES: &[&str] = &[
    "0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5",
    "0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33",
    "0x685a60219309029d01310311dba953d67029170ca4848a4ff638e57002130a0d",
    "0xbbad126377d45f90a8ee120da988a2d7332c78ba8fd679aab478a19d6c133494",
    "0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075",
    "0x0bbe768b519d8ea7b96d58f1182eb7e6ef96c541fbd9526975077ee09f049058",
    "0x598d793defef36e2eeba54a9b45130e4ca92822e1d193671f490950c3b856080",
];

fn parse_code_hash_set(hexes: &[&str]) -> HashSet<Vec<u8>> {
    hexes
        .iter()
        .filter_map(|h| {
            let raw = h.strip_prefix("0x").unwrap_or(h);
            let bytes = hex::decode(raw).ok()?;
            (bytes.len() == 32).then_some(bytes)
        })
        .collect()
}

fn shannon_to_ckb_string(value: i128) -> String {
    let negative = value < 0;
    let abs = value.abs();
    let whole = abs / SHANNONS_PER_CKB;
    let frac = abs % SHANNONS_PER_CKB;

    if frac == 0 {
        return if negative {
            format!("-{whole}")
        } else {
            whole.to_string()
        };
    }

    let mut frac_str = format!("{frac:08}");
    while frac_str.ends_with('0') {
        frac_str.pop();
    }

    if negative {
        format!("-{whole}.{frac_str}")
    } else {
        format!("{whole}.{frac_str}")
    }
}

fn build_circulating_supply_by_date_map(
    snapshots: &[ckbadger_store::DaoDailySnapshot],
) -> Result<HashMap<String, i128>, ApiRouteError> {
    let mut by_date = HashMap::with_capacity(snapshots.len());

    for snapshot in snapshots {
        let Some(total_supply) = snapshot_total_issuance(snapshot) else {
            continue;
        };
        let (_, _, cum_treasury) = snapshot_secondary_cumulative(snapshot)?;
        if snapshot.total_deposited < 0 {
            return Err(ApiError::internal(format!(
                "negative total_deposited in dao_daily_snapshots for {}: {}",
                snapshot.date, snapshot.total_deposited
            )));
        }
        let burnt = GENESIS_BURNT as i128 + cum_treasury;
        let circulating = total_supply - burnt - snapshot.total_deposited;
        if circulating < 0 {
            return Err(ApiError::internal(format!(
                "negative circulating supply at {}: total={}, burnt={}, dao_locked={}",
                snapshot.date, total_supply, burnt, snapshot.total_deposited
            )));
        }
        by_date.insert(snapshot.date.clone(), circulating);
    }

    Ok(by_date)
}

async fn get_common_knowledge_composition_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    let cache_key = "chart:common-knowledge-composition:v1";
    if let Some(cached) = state.cache.get::<StackedAreaChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_stats = state
        .store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut knowledge_by_date: BTreeMap<u32, i128> = BTreeMap::new();
    for (date_key, stats) in daily_stats {
        let Some(knowledge) = stats.knowledge_size else {
            continue;
        };
        let Ok(date) = date_key.parse::<u32>() else {
            continue;
        };
        if knowledge < 0 {
            return Err(ApiError::internal(format!(
                "negative knowledge_size in daily_stats for {}: {}",
                date_key, knowledge
            )));
        }
        knowledge_by_date.insert(date, knowledge);
    }

    if knowledge_by_date.is_empty() {
        return ok(StackedAreaChartResponse {
            data: vec![],
            series: vec![],
            title: "Common Knowledge Bytes Composition".to_string(),
        });
    }

    let script_infos = state
        .store
        .list_script_infos()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let type_code_hashes: HashSet<Vec<u8>> = script_infos
        .into_iter()
        .map(|(code_hash, _)| code_hash)
        .collect();

    let dao_hashes = parse_code_hash_set(DAO_CODE_HASHES);
    let udt_hashes = parse_code_hash_set(UDT_CODE_HASHES);
    let nft_spore_hashes = parse_code_hash_set(NFT_SPORE_CODE_HASHES);

    let mut type_daily_delta: HashMap<u32, i128> = HashMap::new();
    let mut dao_daily_delta: HashMap<u32, i128> = HashMap::new();
    let mut udt_daily_delta: HashMap<u32, i128> = HashMap::new();
    let mut nft_spore_daily_delta: HashMap<u32, i128> = HashMap::new();

    for code_hash in type_code_hashes {
        let deltas = state
            .store
            .list_script_daily_deltas_in_range(&code_hash, true, None, None)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (date, delta) in deltas {
            let occupied_delta = delta.live_occupied_capacity_delta;
            *type_daily_delta.entry(date).or_insert(0) += occupied_delta;

            if dao_hashes.contains(&code_hash) {
                *dao_daily_delta.entry(date).or_insert(0) += occupied_delta;
            } else if udt_hashes.contains(&code_hash) {
                *udt_daily_delta.entry(date).or_insert(0) += occupied_delta;
            } else if nft_spore_hashes.contains(&code_hash) {
                *nft_spore_daily_delta.entry(date).or_insert(0) += occupied_delta;
            }
        }
    }

    let mut cumulative_type: i128 = 0;
    let mut cumulative_dao: i128 = 0;
    let mut cumulative_udt: i128 = 0;
    let mut cumulative_nft_spore: i128 = 0;
    let mut data = Vec::with_capacity(knowledge_by_date.len());

    for (date, knowledge) in knowledge_by_date {
        (cumulative_type, _) = apply_capacity_delta_i128(
            cumulative_type,
            0,
            type_daily_delta.remove(&date).unwrap_or(0),
            0,
            &format!("accumulating type composition for date {}", date),
        )?;
        (cumulative_dao, _) = apply_capacity_delta_i128(
            cumulative_dao,
            0,
            dao_daily_delta.remove(&date).unwrap_or(0),
            0,
            &format!("accumulating DAO composition for date {}", date),
        )?;
        (cumulative_udt, _) = apply_capacity_delta_i128(
            cumulative_udt,
            0,
            udt_daily_delta.remove(&date).unwrap_or(0),
            0,
            &format!("accumulating UDT composition for date {}", date),
        )?;
        (cumulative_nft_spore, _) = apply_capacity_delta_i128(
            cumulative_nft_spore,
            0,
            nft_spore_daily_delta.remove(&date).unwrap_or(0),
            0,
            &format!("accumulating NFT/spore composition for date {}", date),
        )?;

        if cumulative_dao + cumulative_udt + cumulative_nft_spore > cumulative_type {
            return Err(ApiError::internal(format!(
                "typed category sum exceeds total typed capacity on date {}: dao={}, udt={}, nft_spore={}, total_typed={}",
                date, cumulative_dao, cumulative_udt, cumulative_nft_spore, cumulative_type
            )));
        }
        if cumulative_type > knowledge {
            return Err(ApiError::internal(format!(
                "typed capacity exceeds knowledge size on date {}: typed={}, knowledge={}",
                date, cumulative_type, knowledge
            )));
        }

        let typed_effective = cumulative_type;
        let mut remaining_typed = typed_effective;
        let dao = cumulative_dao.min(remaining_typed);
        remaining_typed -= dao;
        let udt = cumulative_udt.min(remaining_typed);
        remaining_typed -= udt;
        let nft_spore = cumulative_nft_spore.min(remaining_typed);
        remaining_typed -= nft_spore;
        let other_contracts = remaining_typed;
        let transfer = knowledge - typed_effective;

        data.push(StackedAreaDataPoint {
            date: format_date_key(&format!("{date:08}")),
            values: HashMap::from([
                ("transfer".to_string(), shannon_to_ckb_string(transfer)),
                ("dao".to_string(), shannon_to_ckb_string(dao)),
                ("udt".to_string(), shannon_to_ckb_string(udt)),
                ("nftSpore".to_string(), shannon_to_ckb_string(nft_spore)),
                (
                    "otherContracts".to_string(),
                    shannon_to_ckb_string(other_contracts),
                ),
            ]),
        });
    }

    let response = StackedAreaChartResponse {
        data,
        series: vec![
            StackedAreaSeries {
                key: "transfer".to_string(),
                label: "CKB Transfer".to_string(),
                color: "#22c55e".to_string(),
            },
            StackedAreaSeries {
                key: "dao".to_string(),
                label: "DAO".to_string(),
                color: "#f59e0b".to_string(),
            },
            StackedAreaSeries {
                key: "udt".to_string(),
                label: "UDT".to_string(),
                color: "#3b82f6".to_string(),
            },
            StackedAreaSeries {
                key: "nftSpore".to_string(),
                label: "NFT (Spore)".to_string(),
                color: "#ec4899".to_string(),
            },
            StackedAreaSeries {
                key: "otherContracts".to_string(),
                label: "Other Contracts".to_string(),
                color: "#8b5cf6".to_string(),
            },
        ],
        title: "Common Knowledge Bytes Composition".to_string(),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

fn load_block_date_transitions(
    store: &ckbadger_store::CkbadgerStore,
) -> Result<Vec<(i64, NaiveDate)>, String> {
    if let Some(state) = store.get_hodl_tracker_state().map_err(|e| e.to_string())? {
        let mut transitions: Vec<(i64, NaiveDate)> = state
            .date_transitions
            .into_iter()
            .filter_map(|(block, date_str)| {
                NaiveDate::parse_from_str(&date_str, "%Y%m%d")
                    .ok()
                    .map(|date| (block, date))
            })
            .collect();
        transitions.sort_by_key(|(block, _)| *block);
        transitions.dedup_by_key(|(block, _)| *block);
        if !transitions.is_empty() {
            return Ok(transitions);
        }
    }

    let mut transitions: Vec<(i64, NaiveDate)> = Vec::new();
    let mut last_date: Option<NaiveDate> = None;
    let iter = store.iterator_cf(store.cf_block_headers(), rocksdb::IteratorMode::Start);
    for item in iter.flatten() {
        let (key, value) = item;
        if key.len() != 8 {
            continue;
        }
        let block_number = i64::from_be_bytes(key[..8].try_into().unwrap_or([0; 8]));
        if let Ok(header) = bincode::deserialize::<ckbadger_store::CachedBlockHeader>(&value) {
            if let Some(dt) = DateTime::from_timestamp_millis(header.timestamp) {
                let date = ckbadger_common::block_date(dt);
                if last_date != Some(date) {
                    transitions.push((block_number, date));
                    last_date = Some(date);
                }
            }
        }
    }
    Ok(transitions)
}

fn block_number_to_date(transitions: &[(i64, NaiveDate)], block_number: i64) -> Option<NaiveDate> {
    if transitions.is_empty() {
        return None;
    }
    let idx =
        transitions.partition_point(|(transition_block, _)| *transition_block <= block_number);
    if idx == 0 {
        Some(transitions[0].1)
    } else {
        Some(transitions[idx - 1].1)
    }
}

fn current_snapshot_date(store: &ckbadger_store::CkbadgerStore) -> Result<NaiveDate, String> {
    let tip = store.get_sync_tip_block().map_err(|e| e.to_string())?;
    let date = tip
        .and_then(|(_, header)| DateTime::from_timestamp_millis(header.timestamp))
        .map(ckbadger_common::block_date)
        .unwrap_or_else(|| ckbadger_common::block_date(Utc::now()));
    Ok(date)
}

fn occupied_capacity_bucket_index(occupied_shannons: i128) -> usize {
    let ckb_100 = 100_i128 * SHANNONS_PER_CKB;
    let ckb_1k = 1_000_i128 * SHANNONS_PER_CKB;
    let ckb_10k = 10_000_i128 * SHANNONS_PER_CKB;
    let ckb_100k = 100_000_i128 * SHANNONS_PER_CKB;
    let ckb_1m = 1_000_000_i128 * SHANNONS_PER_CKB;

    if occupied_shannons < ckb_100 {
        0
    } else if occupied_shannons < ckb_1k {
        1
    } else if occupied_shannons < ckb_10k {
        2
    } else if occupied_shannons < ckb_100k {
        3
    } else if occupied_shannons < ckb_1m {
        4
    } else {
        5
    }
}

async fn get_cell_age_vs_occupied_capacity_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    let cache_key = "chart:cell-age-vs-occupied-capacity:v1";
    if let Some(cached) = state.cache.get::<StackedAreaChartResponse>(cache_key).await {
        return ok(cached);
    }

    let snapshot_date = current_snapshot_date(state.store.as_ref()).map_err(ApiError::internal)?;
    let transitions =
        load_block_date_transitions(state.store.as_ref()).map_err(ApiError::internal)?;

    let mut lt_1d: i128 = 0;
    let mut d1_7d: i128 = 0;
    let mut d7_30d: i128 = 0;
    let mut d30_180d: i128 = 0;
    let mut gt_180d: i128 = 0;

    let iter = state
        .store
        .iterator_cf(state.store.cf_live_cells(), rocksdb::IteratorMode::Start);
    for item in iter.flatten() {
        let (_, value) = item;
        let Ok(cell) = bincode::deserialize::<ckbadger_store::LiveCellInfo>(&value) else {
            continue;
        };
        let Some(created_date) = block_number_to_date(&transitions, cell.created_at_block) else {
            continue;
        };
        let age_days_raw = (snapshot_date - created_date).num_days();
        if age_days_raw < 0 {
            return Err(ApiError::internal(format!(
                "negative cell age detected: snapshot_date={}, created_date={}, created_at_block={}",
                snapshot_date, created_date, cell.created_at_block
            )));
        }
        let age_days = age_days_raw;
        let occupied = cell.occupied_capacity as i128;
        if occupied < 0 {
            return Err(ApiError::internal(format!(
                "negative occupied_capacity in live cell: created_at_block={}, occupied_capacity={}",
                cell.created_at_block, occupied
            )));
        }
        match age_days {
            0 => lt_1d += occupied,
            1..=6 => d1_7d += occupied,
            7..=29 => d7_30d += occupied,
            30..=179 => d30_180d += occupied,
            _ => gt_180d += occupied,
        }
    }

    let snapshot_values = HashMap::from([
        ("lt1d".to_string(), shannon_to_ckb_string(lt_1d)),
        ("d1to7d".to_string(), shannon_to_ckb_string(d1_7d)),
        ("d7to30d".to_string(), shannon_to_ckb_string(d7_30d)),
        ("d30to180d".to_string(), shannon_to_ckb_string(d30_180d)),
        ("gt180d".to_string(), shannon_to_ckb_string(gt_180d)),
    ]);

    let snapshot_label = snapshot_date.format("%Y-%m-%d").to_string();
    let previous_label = (snapshot_date - Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    let response = StackedAreaChartResponse {
        data: vec![
            StackedAreaDataPoint {
                date: previous_label,
                values: snapshot_values.clone(),
            },
            StackedAreaDataPoint {
                date: snapshot_label,
                values: snapshot_values,
            },
        ],
        series: vec![
            StackedAreaSeries {
                key: "lt1d".to_string(),
                label: "< 1d".to_string(),
                color: "#22c55e".to_string(),
            },
            StackedAreaSeries {
                key: "d1to7d".to_string(),
                label: "1-7d".to_string(),
                color: "#84cc16".to_string(),
            },
            StackedAreaSeries {
                key: "d7to30d".to_string(),
                label: "7-30d".to_string(),
                color: "#f59e0b".to_string(),
            },
            StackedAreaSeries {
                key: "d30to180d".to_string(),
                label: "30-180d".to_string(),
                color: "#f97316".to_string(),
            },
            StackedAreaSeries {
                key: "gt180d".to_string(),
                label: "> 180d".to_string(),
                color: "#ef4444".to_string(),
            },
        ],
        title: "Cell Age vs Occupied Capacity".to_string(),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    ok(response)
}

async fn get_capacity_turnover_ratio_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:capacity-turnover-ratio:v1";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let mut daily_stats = state
        .store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    daily_stats.sort_by(|a, b| a.0.cmp(&b.0));

    let mut rolling: VecDeque<(i128, i128)> = VecDeque::new();
    let mut rolling_consumed_sum: i128 = 0;
    let mut rolling_live_sum: i128 = 0;
    let mut data = Vec::with_capacity(daily_stats.len());

    for (date, stats) in daily_stats {
        let live = stats.knowledge_size.unwrap_or(0);
        if live < 0 {
            return Err(ApiError::internal(format!(
                "negative knowledge_size in daily_stats for {}: {}",
                date, live
            )));
        }
        let consumed = stats.occupied_capacity_consumed;
        if consumed < 0 {
            return Err(ApiError::internal(format!(
                "negative occupied_capacity_consumed in daily_stats for {}: {}",
                date, consumed
            )));
        }

        rolling.push_back((consumed, live));
        rolling_consumed_sum += consumed;
        rolling_live_sum += live;
        if rolling.len() > 7 {
            if let Some((old_consumed, old_live)) = rolling.pop_front() {
                rolling_consumed_sum -= old_consumed;
                rolling_live_sum -= old_live;
            }
        }

        let daily_turnover = if live > 0 {
            consumed as f64 * 100.0 / live as f64
        } else {
            0.0
        };
        let weekly_turnover = if rolling_live_sum > 0 {
            rolling_consumed_sum as f64 * 100.0 / rolling_live_sum as f64
        } else {
            0.0
        };

        data.push(ChartDataPoint {
            date: format_date_key(&date),
            value: format!("{daily_turnover:.6}"),
            value2: Some(format!("{weekly_turnover:.6}")),
        });
    }

    let response = ChartResponse {
        data,
        title: "Capacity Turnover Ratio".to_string(),
        y_axis_label: "Daily Turnover (%)".to_string(),
        y2_axis_label: Some("Weekly Turnover (%)".to_string()),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    ok(response)
}

async fn get_cell_size_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:cell-size-distribution:v1";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let bucket_labels = [
        "<100 CKB",
        "100-1k CKB",
        "1k-10k CKB",
        "10k-100k CKB",
        "100k-1m CKB",
        ">=1m CKB",
    ];
    let mut bucket_counts = vec![0_i128; bucket_labels.len()];
    let mut bucket_occupied = vec![0_i128; bucket_labels.len()];

    let iter = state
        .store
        .iterator_cf(state.store.cf_live_cells(), rocksdb::IteratorMode::Start);
    for item in iter.flatten() {
        let (_, value) = item;
        let Ok(cell) = bincode::deserialize::<ckbadger_store::LiveCellInfo>(&value) else {
            continue;
        };
        let occupied = cell.occupied_capacity as i128;
        if occupied < 0 {
            return Err(ApiError::internal(format!(
                "negative occupied_capacity in live cell: created_at_block={}, occupied_capacity={}",
                cell.created_at_block, occupied
            )));
        }
        let idx = occupied_capacity_bucket_index(occupied);
        bucket_counts[idx] += 1;
        bucket_occupied[idx] += occupied;
    }

    let data = bucket_labels
        .iter()
        .enumerate()
        .map(|(idx, label)| ChartDataPoint {
            date: (*label).to_string(),
            value: bucket_counts[idx].to_string(),
            value2: Some(shannon_to_ckb_string(bucket_occupied[idx])),
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Cell Size Distribution".to_string(),
        y_axis_label: "Live Cells".to_string(),
        y2_axis_label: Some("Occupied Capacity (CKB)".to_string()),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    ok(response)
}

async fn get_address_cohort_retention_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:address-cohort-retention:v1";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let transitions =
        load_block_date_transitions(state.store.as_ref()).map_err(ApiError::internal)?;
    let mut cohorts: BTreeMap<String, (i128, i128)> = BTreeMap::new();

    let iter = state
        .store
        .iterator_cf(state.store.cf_addr_balance(), rocksdb::IteratorMode::Start);
    for item in iter.flatten() {
        let (_, value) = item;
        let Ok(balance) = bincode::deserialize::<ckbadger_store::AddressBalance>(&value) else {
            continue;
        };
        let Some(first_seen_date) = block_number_to_date(&transitions, balance.first_seen_block)
        else {
            continue;
        };

        let cohort = first_seen_date.format("%Y-%m").to_string();
        let occupied = balance.occupied_capacity;
        let total_balance = balance.balance;
        if occupied < 0 {
            return Err(ApiError::internal(format!(
                "negative address occupied_capacity: first_seen_block={}, occupied_capacity={}",
                balance.first_seen_block, occupied
            )));
        }
        if total_balance < 0 {
            return Err(ApiError::internal(format!(
                "negative address balance: first_seen_block={}, balance={}",
                balance.first_seen_block, total_balance
            )));
        }
        if occupied > total_balance {
            return Err(ApiError::internal(format!(
                "address occupied capacity exceeds balance: first_seen_block={}, occupied_capacity={}, balance={}",
                balance.first_seen_block, occupied, total_balance
            )));
        }
        let entry = cohorts.entry(cohort).or_insert((0, 0));
        entry.0 += occupied;
        entry.1 += total_balance;
    }

    let data = cohorts
        .into_iter()
        .map(|(cohort, (occupied, total_balance))| {
            let retention = if total_balance > 0 {
                occupied as f64 * 100.0 / total_balance as f64
            } else {
                0.0
            };
            ChartDataPoint {
                date: cohort,
                value: format!("{retention:.6}"),
                value2: Some(shannon_to_ckb_string(occupied)),
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Address Cohort Retention".to_string(),
        y_axis_label: "Occupied / Balance (%)".to_string(),
        y2_axis_label: Some("Occupied Capacity (CKB)".to_string()),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    ok(response)
}

async fn get_block_time_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:block-time-distribution:v2";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let response =
        build_block_time_distribution_response(state.store.as_ref()).map_err(ApiError::internal)?;

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

const BLOCK_TIME_DIST_RECENT_BLOCKS: usize = 50_000;
const BLOCK_TIME_DIST_BUCKET_MS: i64 = 100;
const BLOCK_TIME_DIST_MAX_MS: i64 = 50_000;
const BLOCK_TIME_DIST_BUCKET_COUNT: usize =
    (BLOCK_TIME_DIST_MAX_MS / BLOCK_TIME_DIST_BUCKET_MS + 1) as usize;

fn block_time_ms_to_bucket_index(diff_ms: i64) -> Option<usize> {
    if !(0..=BLOCK_TIME_DIST_MAX_MS).contains(&diff_ms) {
        return None;
    }
    Some((diff_ms / BLOCK_TIME_DIST_BUCKET_MS) as usize)
}

fn build_block_time_distribution_data(
    bucket_counts: &[u64],
    total_blocks: u64,
) -> Vec<ChartDataPoint> {
    (0..BLOCK_TIME_DIST_BUCKET_COUNT)
        .map(|idx| {
            let ratio = if total_blocks > 0 {
                (bucket_counts[idx] as f64 / total_blocks as f64) * 100.0
            } else {
                0.0
            };
            ChartDataPoint {
                date: format!("{:.1}", idx as f64 / 10.0),
                value: format!("{:.3}", ratio),
                value2: None,
            }
        })
        .collect()
}

pub(crate) fn build_block_time_distribution_response(
    store: &ckbadger_store::CkbadgerStore,
) -> Result<ChartResponse, String> {
    let mut bucket_counts = vec![0u64; BLOCK_TIME_DIST_BUCKET_COUNT];
    let mut total_blocks = 0u64;

    let mut headers = store
        .list_blocks_desc(None, BLOCK_TIME_DIST_RECENT_BLOCKS + 1)
        .map_err(|e| e.to_string())?;

    if headers.len() >= 2 {
        headers.reverse();
        for window in headers.windows(2) {
            let (prev_number, prev_header) = &window[0];
            let (curr_number, curr_header) = &window[1];
            if *curr_number != *prev_number + 1 {
                continue;
            }

            let diff_ms = curr_header.timestamp - prev_header.timestamp;
            if let Some(bucket_index) = block_time_ms_to_bucket_index(diff_ms) {
                bucket_counts[bucket_index] += 1;
                total_blocks += 1;
            }
        }
    }

    Ok(ChartResponse {
        data: build_block_time_distribution_data(&bucket_counts, total_blocks),
        title: "Block Time Distribution (Recent 50000 blocks)".to_string(),
        y_axis_label: "Block Ratio (%)".to_string(),
        y2_axis_label: None,
    })
}

async fn get_epoch_time_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:epoch-time-distribution";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let dist = state
        .store
        .list_epoch_time_dist()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = dist
        .into_iter()
        .map(|(bucket_minutes, count)| {
            let hours_decimal = bucket_minutes as f64 / 60.0;
            ChartDataPoint {
                date: format!("{:.2}", hours_decimal),
                value: count.to_string(),
                value2: None,
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Epoch Time Distribution".to_string(),
        y_axis_label: "Epochs".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_epoch_time_length_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:epoch-time-length";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let epochs = state
        .store
        .list_epoch_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = epochs
        .into_iter()
        .filter_map(|e| {
            let end = e.end_timestamp?;
            let duration_secs = end.signed_duration_since(e.start_timestamp).num_seconds() as f64;
            let duration_hours = duration_secs / 3600.0;
            Some(ChartDataPoint {
                date: e.epoch_number.to_string(),
                value: format!("{:.2}", duration_hours),
                value2: Some(e.blocks_count.to_string()),
            })
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Epoch Time Length".to_string(),
        y_axis_label: "Hours".to_string(),
        y2_axis_label: Some("Blocks".to_string()),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_average_block_time_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:average-block-time";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_stats = state
        .store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = daily_stats
        .into_iter()
        .filter_map(|(date_str, stats)| {
            stats.avg_block_time_ms.map(|avg_time_ms| ChartDataPoint {
                date: format_date_for_chart(&date_str),
                value: format!("{:.2}", avg_time_ms as f64 / 1000.0),
                value2: None,
            })
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Average Block Time".to_string(),
        y_axis_label: "Seconds".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn fetch_tip_block_from_ckb(ckb_rpc_url: &str) -> Result<u64, String> {
    #[derive(Serialize)]
    struct RpcRequest {
        jsonrpc: &'static str,
        method: &'static str,
        params: Vec<()>,
        id: u64,
    }

    #[derive(Deserialize)]
    struct RpcResponse {
        result: Option<String>,
    }

    let client = reqwest::Client::new();
    let request = RpcRequest {
        jsonrpc: "2.0",
        method: "get_tip_block_number",
        params: vec![],
        id: 1,
    };

    let response = client
        .post(ckb_rpc_url)
        .json(&request)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<RpcResponse>()
        .await
        .map_err(|e| e.to_string())?;

    let hex = response.result.ok_or("Empty RPC response")?;
    let hex = hex.strip_prefix("0x").unwrap_or(&hex);
    u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
}

async fn fetch_network_stats_from_db(
    state: &AppState,
) -> Result<
    NetworkStats,
    (
        axum::http::StatusCode,
        axum::Json<crate::response::ApiError>,
    ),
> {
    let store = &state.store;

    // Get latest block header from store
    let latest = store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (latest_block, epoch_number, epoch_index, epoch_length, latest_timestamp) = match latest {
        Some((block_num, header)) => {
            let ts = DateTime::from_timestamp_millis(header.timestamp).unwrap_or_else(Utc::now);
            (
                block_num,
                header.epoch_number,
                header.epoch_index,
                header.epoch_length,
                ts,
            )
        }
        None => (0i64, 0i64, 0i32, 1800i32, Utc::now()),
    };

    // Get compact_target from the latest block header's DAO or from block header directly
    // The CachedBlockHeader doesn't store compact_target, so we compute difficulty
    // from the latest DailyBlockStats instead. For now use the latest daily block stats.
    let today = ckbadger_common::block_date(latest_timestamp);
    let today_str = today.format("%Y%m%d").to_string();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y%m%d").to_string();

    // Fetch epoch stats for avg block time
    let epoch_stats = store
        .get_epoch_stats(epoch_number)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Get recent block for avg block time
    let recent_blocks = store
        .list_blocks_desc(Some(latest_block), 2)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Get 24h tx count from daily stats
    let today_stats = store
        .get_daily_stats(&today_str)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let yesterday_stats = store
        .get_daily_stats(&yesterday_str)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let tx_count_24h: i64 = today_stats
        .as_ref()
        .map(|s| s.transactions_count as i64)
        .unwrap_or(0)
        + yesterday_stats
            .as_ref()
            .map(|s| s.transactions_count as i64)
            .unwrap_or(0);

    // Fetch tip block from CKB node
    let tip_block_result = fetch_tip_block_from_ckb(&state.ckb_rpc_url).await;
    let tip_block_u64 = match tip_block_result {
        Ok(tip) => tip,
        Err(_) => u64::try_from(latest_block).map_err(|_| {
            ApiError::internal(format!(
                "latest block below zero, cannot convert to u64 for tip fallback: {}",
                latest_block
            ))
        })?,
    };
    let tip_block = i64::try_from(tip_block_u64).map_err(|_| {
        ApiError::internal(format!(
            "tip block exceeds i64 range: {} (max={})",
            tip_block_u64,
            i64::MAX
        ))
    })?;

    // Calculate epoch avg block time
    let epoch_avg_time = epoch_stats
        .and_then(|es| {
            if es.blocks_count > 1 {
                if let Some(end) = es.end_timestamp {
                    let duration =
                        end.signed_duration_since(es.start_timestamp).num_seconds() as f64;
                    Some(duration / (es.blocks_count - 1) as f64)
                } else {
                    // Epoch in progress
                    let duration = latest_timestamp
                        .signed_duration_since(es.start_timestamp)
                        .num_seconds() as f64;
                    Some(duration / epoch_index.max(1) as f64)
                }
            } else {
                None
            }
        })
        .unwrap_or(10.0);

    // Calculate recent avg block time from last 2 blocks
    let avg_time = if recent_blocks.len() == 2 {
        let ts0 = DateTime::from_timestamp_millis(recent_blocks[1].1.timestamp).unwrap_or_default();
        let ts1 = DateTime::from_timestamp_millis(recent_blocks[0].1.timestamp).unwrap_or_default();
        let duration = ts0.signed_duration_since(ts1).num_seconds() as f64;
        duration.abs().max(1.0)
    } else {
        10.0
    };

    // Get compact_target from daily block stats for difficulty/hash rate
    let daily_block_stats = store
        .get_daily_block_stats(&today_str)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .or_else(|| store.get_daily_block_stats(&yesterday_str).ok().flatten());

    let compact_target = daily_block_stats
        .as_ref()
        .map(|s| s.avg_compact_target as i64)
        .unwrap_or(0);

    let remaining_blocks = epoch_length - epoch_index;
    let estimated_epoch_seconds = (remaining_blocks as f64 * epoch_avg_time) as i64;

    let tps = tx_count_24h as f64 / 86400.0;
    let tx_per_minute = tps * 60.0;

    // Get sync status from Redis cache or from store
    let sync_status_from_redis: Option<SyncStatusData> =
        state.cache.get(SYNC_STATUS_REDIS_KEY).await;
    let (synced_block, db_ema_rate, sync_started_at, bulk_sync_completed_at) =
        sync_status_from_redis
            .as_ref()
            .map(|s| {
                (
                    s.tip_block_number,
                    s.sync_ema_rate,
                    s.sync_started_at,
                    s.bulk_sync_completed_at,
                )
            })
            .unwrap_or((latest_block, None, None, None));

    // Get deep fork status from store
    let store_sync = store
        .get_sync_status()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (
        deep_fork_detected,
        deep_fork_at,
        deep_fork_db_tip,
        deep_fork_chain_tip,
        deep_fork_depth,
        deep_fork_fork_point,
    ) = if store_sync.deep_fork_detected {
        if let Some(ref info) = store_sync.deep_fork_info {
            (
                true,
                None::<DateTime<Utc>>, // deep fork timestamp not stored in store
                Some(info.db_tip),
                Some(info.chain_tip),
                Some(info.depth),
                Some(info.fork_point),
            )
        } else {
            (true, None, None, None, None, None)
        }
    } else {
        (false, None, None, None, None, None)
    };

    let blocks_behind = tip_block - synced_block;
    let is_syncing = blocks_behind > 100;
    let is_bulk_syncing = blocks_behind > 1000;

    let sync_mode = if bulk_sync_completed_at.is_some() && !is_syncing {
        "synced".to_string()
    } else if is_bulk_syncing {
        "bulk".to_string()
    } else if is_syncing {
        "normal".to_string()
    } else {
        "synced".to_string()
    };

    let now = Utc::now().timestamp();
    let elapsed_time = sync_started_at.map(|started| {
        let end = bulk_sync_completed_at.unwrap_or(now);
        format_duration_smart((end - started) as f64)
    });

    let total_time =
        if let (Some(started), Some(completed)) = (sync_started_at, bulk_sync_completed_at) {
            Some(format_duration_smart((completed - started) as f64))
        } else {
            None
        };

    let sync_progress_from_redis: Option<SyncProgressData> =
        state.cache.get(SYNC_PROGRESS_REDIS_KEY).await;

    let (progress, estimated_time, blocks_per_second, ema_blocks_per_second) =
        if let Some(ref sp) = sync_progress_from_redis {
            let stale = Utc::now().timestamp() - sp.updated_at > 60;
            if !stale && is_syncing {
                (
                    sp.progress_percentage,
                    Some(sp.eta_formatted.clone()),
                    Some(sp.blocks_per_second),
                    Some(sp.ema_blocks_per_second),
                )
            } else {
                let p = if tip_block > 0 {
                    (synced_block as f64 / tip_block as f64 * 100.0).min(100.0)
                } else {
                    0.0
                };
                let (ema, eta) = if is_syncing {
                    if let Some(rate) = db_ema_rate {
                        if rate > 0.0 {
                            let remaining = blocks_behind as f64;
                            let eta_secs = remaining / rate;
                            let eta_str = format_duration_smart(eta_secs);
                            (Some(rate), Some(eta_str))
                        } else {
                            (Some(rate), None)
                        }
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };
                (p, eta, ema, ema)
            }
        } else {
            let p = if tip_block > 0 {
                (synced_block as f64 / tip_block as f64 * 100.0).min(100.0)
            } else {
                0.0
            };
            let (ema, eta) = if is_syncing {
                if let Some(rate) = db_ema_rate {
                    if rate > 0.0 {
                        let remaining = blocks_behind as f64;
                        let eta_secs = remaining / rate;
                        let eta_str = format_duration_smart(eta_secs);
                        (Some(rate), Some(eta_str))
                    } else {
                        (Some(rate), None)
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            };
            (p, eta, ema, ema)
        };

    let sync_status = SyncStatus {
        is_syncing,
        synced_block,
        tip_block,
        progress,
        estimated_time,
        chart_data_may_be_incomplete: blocks_behind > 1000,
        blocks_per_second,
        ema_blocks_per_second,
        sync_mode,
        started_at: sync_started_at,
        elapsed_time,
        total_time,
    };

    let deep_fork_status = DeepForkStatus {
        detected: deep_fork_detected,
        detected_at: deep_fork_at,
        depth: deep_fork_depth,
        db_tip: deep_fork_db_tip,
        chain_tip: deep_fork_chain_tip,
        fork_point: deep_fork_fork_point,
    };

    let difficulty = compact_to_difficulty(compact_target);
    let hash_rate = if avg_time > 0.0 {
        difficulty as f64 / avg_time
    } else {
        0.0
    };

    Ok(NetworkStats {
        latest_block,
        avg_block_time: format!("{:.2}s", avg_time),
        hash_rate: format_hash_rate(hash_rate),
        difficulty: format_difficulty(difficulty),
        epoch: format!("{}({}/{})", epoch_number, epoch_index, epoch_length),
        tps: format!("{:.2}", tps),
        estimated_epoch_time: format_duration(estimated_epoch_seconds as u64),
        transactions_per_minute: format!("{:.1}", tx_per_minute),
        transactions_per_day: tx_count_24h.to_string(),
        sync_status,
        deep_fork_status,
    })
}

async fn get_hash_rate_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:hash-rate";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_block_stats = state
        .store
        .list_daily_block_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Exclude the last day (incomplete) like the SQL version did
    let max_date = daily_block_stats
        .iter()
        .map(|(d, _)| d.as_str())
        .max()
        .map(|s| s.to_string());

    let data: Vec<ChartDataPoint> = daily_block_stats
        .into_iter()
        .filter(|(date, stats)| {
            stats.avg_compact_target > 0.0
                && max_date.as_ref().is_none_or(|m| date.as_str() < m.as_str())
        })
        .map(|(date_str, stats)| {
            let difficulty = compact_to_difficulty(stats.avg_compact_target as i64);
            let hash_rate = calculate_daily_hash_rate(difficulty, stats.block_count);
            ChartDataPoint {
                date: format_date_for_chart(&date_str),
                value: format!("{:.0}", hash_rate),
                value2: None,
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Hash Rate".to_string(),
        y_axis_label: "Hash Rate (H/s)".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_difficulty_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:difficulty";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_block_stats = state
        .store
        .list_daily_block_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let max_date = daily_block_stats
        .iter()
        .map(|(d, _)| d.as_str())
        .max()
        .map(|s| s.to_string());

    let data: Vec<ChartDataPoint> = daily_block_stats
        .into_iter()
        .filter(|(date, stats)| {
            stats.avg_compact_target > 0.0
                && max_date.as_ref().is_none_or(|m| date.as_str() < m.as_str())
        })
        .map(|(date_str, stats)| {
            let difficulty = compact_to_difficulty(stats.avg_compact_target as i64);
            ChartDataPoint {
                date: format_date_for_chart(&date_str),
                value: difficulty.to_string(),
                value2: None,
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Difficulty".to_string(),
        y_axis_label: "Difficulty".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_uncle_rate_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:uncle-rate";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_block_stats = state
        .store
        .list_daily_block_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let max_date = daily_block_stats
        .iter()
        .map(|(d, _)| d.as_str())
        .max()
        .map(|s| s.to_string());

    let data: Vec<ChartDataPoint> = daily_block_stats
        .into_iter()
        .filter(|(date, _)| max_date.as_ref().is_none_or(|m| date.as_str() < m.as_str()))
        .map(|(date_str, stats)| {
            let uncle_rate = if stats.block_count > 0 {
                stats.total_uncles as f64 / stats.block_count as f64
            } else {
                0.0
            };
            ChartDataPoint {
                date: format_date_for_chart(&date_str),
                value: format!("{:.6}", uncle_rate),
                value2: None,
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Uncle Rate".to_string(),
        y_axis_label: "Uncle Rate".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinerDistributionDataPoint {
    pub address: String,
    pub miner_name: Option<String>,
    pub blocks_mined: i64,
    pub percentage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinerDistributionResponse {
    pub data: Vec<MinerDistributionDataPoint>,
    pub title: String,
    pub total_blocks: i64,
}

async fn get_miner_address_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<MinerDistributionResponse> {
    let cache_key = "chart:miner-address-distribution";
    if let Some(cached) = state
        .cache
        .get::<MinerDistributionResponse>(cache_key)
        .await
    {
        return ok(cached);
    }

    let miner_stats = state
        .store
        .list_miner_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Aggregate by miner_lock_hash across all dates
    let mut aggregated: std::collections::HashMap<Vec<u8>, i64> = std::collections::HashMap::new();
    for ms in &miner_stats {
        *aggregated.entry(ms.miner_lock_hash.clone()).or_insert(0) += ms.blocks_count as i64;
    }

    let total_blocks: i64 = aggregated.values().sum();
    let total = total_blocks as f64;

    // Sort by blocks descending and take top 100
    let mut sorted: Vec<(Vec<u8>, i64)> = aggregated.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted.truncate(100);

    let data: Vec<MinerDistributionDataPoint> = sorted
        .into_iter()
        .map(|(hash, blocks_mined)| {
            let percentage = if total > 0.0 {
                (blocks_mined as f64 / total) * 100.0
            } else {
                0.0
            };
            let address = format!("0x{}", hex::encode(&hash));
            MinerDistributionDataPoint {
                address,
                miner_name: None,
                blocks_mined,
                percentage: format!("{:.4}", percentage),
            }
        })
        .collect();

    let response = MinerDistributionResponse {
        data,
        title: "Miner Address Distribution".to_string(),
        total_blocks,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

fn calculate_daily_hash_rate(difficulty: u64, block_count: i32) -> f64 {
    if block_count <= 0 {
        return 0.0;
    }

    // Explorer computes hash rate using average block time in milliseconds.
    let avg_block_time_ms = 86_400_000.0 / block_count as f64;
    if avg_block_time_ms <= 0.0 {
        0.0
    } else {
        difficulty as f64 / avg_block_time_ms
    }
}

fn snapshot_total_issuance(snapshot: &ckbadger_store::DaoDailySnapshot) -> Option<i128> {
    (snapshot.total_issuance > 0).then_some(snapshot.total_issuance)
}

fn snapshot_secondary_cumulative(
    snapshot: &ckbadger_store::DaoDailySnapshot,
) -> Result<(i128, i128, i128), ApiRouteError> {
    if snapshot.cum_miner_secondary < 0 {
        return Err(ApiError::internal(format!(
            "negative cum_miner_secondary in dao_daily_snapshots for {}: {}",
            snapshot.date, snapshot.cum_miner_secondary
        )));
    }
    if snapshot.cum_dao_compensation < 0 {
        return Err(ApiError::internal(format!(
            "negative cum_dao_compensation in dao_daily_snapshots for {}: {}",
            snapshot.date, snapshot.cum_dao_compensation
        )));
    }
    if snapshot.cum_treasury < 0 {
        return Err(ApiError::internal(format!(
            "negative cum_treasury in dao_daily_snapshots for {}: {}",
            snapshot.date, snapshot.cum_treasury
        )));
    }
    Ok((
        snapshot.cum_miner_secondary,
        snapshot.cum_dao_compensation,
        snapshot.cum_treasury,
    ))
}

async fn get_total_supply_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    let cache_key = "chart:total-supply";
    if let Some(cached) = state.cache.get::<StackedAreaChartResponse>(cache_key).await {
        return ok(cached);
    }

    let snapshots = state
        .store
        .list_dao_daily_snapshots()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    const SHANNON: f64 = 100_000_000.0;
    let mut data = Vec::with_capacity(snapshots.len());
    for snapshot in &snapshots {
        let Some(total_supply) = snapshot_total_issuance(snapshot) else {
            return Err(ApiError::internal(format!(
                "missing total_issuance in dao_daily_snapshots for {}. delete RocksDB and re-sync from genesis",
                snapshot.date
            )));
        };
        let total_supply = total_supply as f64;
        let (_, _, cum_treasury) = snapshot_secondary_cumulative(snapshot)?;
        let burnt = (GENESIS_BURNT as i128 + cum_treasury) as f64;

        // Nervos DAO locked = active deposits (can be unlocked, but currently locked)
        if snapshot.total_deposited < 0 {
            return Err(ApiError::internal(format!(
                "negative total_deposited in dao_daily_snapshots for {}: {}",
                snapshot.date, snapshot.total_deposited
            )));
        }
        let nervos_dao = snapshot.total_deposited as f64;
        // Circulating = total_supply - burnt - nervos_dao_locked
        let circulating = total_supply - burnt - nervos_dao;
        if circulating < 0.0 {
            return Err(ApiError::internal(format!(
                "negative circulating supply in total-supply chart for {}: total={}, burnt={}, dao_locked={}",
                snapshot.date, total_supply, burnt, nervos_dao
            )));
        }

        let mut values = std::collections::HashMap::new();
        values.insert(
            "circulating".to_string(),
            format!("{:.0}", circulating / SHANNON),
        );
        values.insert(
            "nervosdao".to_string(),
            format!("{:.0}", nervos_dao / SHANNON),
        );
        values.insert("burnt".to_string(), format!("{:.0}", burnt / SHANNON));
        data.push(StackedAreaDataPoint {
            date: snapshot.date.clone(),
            values,
        });
    }

    let series = vec![
        StackedAreaSeries {
            key: "circulating".to_string(),
            label: "Circulating".to_string(),
            color: "#00c389".to_string(),
        },
        StackedAreaSeries {
            key: "nervosdao".to_string(),
            label: "Nervos DAO Locked".to_string(),
            color: "#3b82f6".to_string(),
        },
        StackedAreaSeries {
            key: "burnt".to_string(),
            label: "Burnt".to_string(),
            color: "#6b7280".to_string(),
        },
    ];

    let response = StackedAreaChartResponse {
        data,
        series,
        title: "Total Supply".to_string(),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_nominal_apc_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let data: Vec<ChartDataPoint> = (0..=80)
        .map(|i| {
            let year = i as f64 * 0.25;
            let apc = calculate_nominal_apc(year);
            ChartDataPoint {
                date: format!("{}", year),
                value: format!("{:.4}", apc),
                value2: None,
            }
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Nominal DAO Compensation Rate".to_string(),
        y_axis_label: "APC".to_string(),
        y2_axis_label: None,
    })
}

fn calculate_nominal_apc(year: f64) -> f64 {
    // Genesis actual supply is 25.2B (33.6B - 8.4B burnt at genesis)
    const GENESIS_SUPPLY: f64 = 25_200_000_000.0;
    const SECONDARY_ISSUANCE_PER_YEAR: f64 = 1_344_000_000.0;

    let halving_count = (year / 4.0).floor() as u32;

    let mut total_primary_issued = 0.0;
    for h in 0..halving_count {
        let rate = 4_200_000_000.0 / 2.0_f64.powi(h as i32);
        total_primary_issued += rate * 4.0;
    }

    let years_in_current_era = year - (halving_count as f64 * 4.0);
    let current_era_rate = 4_200_000_000.0 / 2.0_f64.powi(halving_count as i32);
    total_primary_issued += current_era_rate * years_in_current_era;

    let total_secondary_issued = SECONDARY_ISSUANCE_PER_YEAR * year;
    let total_supply = GENESIS_SUPPLY + total_primary_issued + total_secondary_issued;

    (SECONDARY_ISSUANCE_PER_YEAR / total_supply) * 100.0
}

async fn get_secondary_issuance_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    let cache_key = "chart:secondary-issuance";
    if let Some(cached) = state.cache.get::<StackedAreaChartResponse>(cache_key).await {
        return ok(cached);
    }

    let snapshots = state
        .store
        .list_dao_daily_snapshots()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    const SHANNON: f64 = 100_000_000.0;
    let mut data = Vec::new();
    for snapshot in &snapshots {
        let (cum_miner, cum_dao, cum_treasury) = snapshot_secondary_cumulative(snapshot)?;
        if cum_miner <= 0 && cum_dao <= 0 && cum_treasury <= 0 {
            continue;
        }

        let mut values = std::collections::HashMap::new();
        values.insert(
            "compensation".to_string(),
            format!("{:.0}", cum_dao as f64 / SHANNON),
        );
        values.insert(
            "mining".to_string(),
            format!("{:.0}", cum_miner as f64 / SHANNON),
        );
        values.insert(
            "burnt".to_string(),
            format!("{:.0}", cum_treasury as f64 / SHANNON),
        );
        data.push(StackedAreaDataPoint {
            date: snapshot.date.clone(),
            values,
        });
    }

    let series = vec![
        StackedAreaSeries {
            key: "compensation".to_string(),
            label: "Deposit Compensation".to_string(),
            color: "#00c389".to_string(),
        },
        StackedAreaSeries {
            key: "mining".to_string(),
            label: "Mining Reward".to_string(),
            color: "#8b5cf6".to_string(),
        },
        StackedAreaSeries {
            key: "burnt".to_string(),
            label: "Burnt".to_string(),
            color: "#6b7280".to_string(),
        },
    ];

    let response = StackedAreaChartResponse {
        data,
        series,
        title: "Secondary Issuance".to_string(),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_inflation_rate_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let data: Vec<ChartDataPoint> = (0..=100)
        .map(|i| {
            let year = i as f64 * 0.5;
            let (nominal, real) = calculate_inflation_rates(year);
            ChartDataPoint {
                date: format!("{:.1}", year),
                value: format!("{:.4}", nominal),
                value2: Some(format!("{:.4}", real)),
            }
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Inflation Rate".to_string(),
        y_axis_label: "Nominal Inflation (%)".to_string(),
        y2_axis_label: Some("Real Inflation (%)".to_string()),
    })
}

fn calculate_inflation_rates(year: f64) -> (f64, f64) {
    const INITIAL_PRIMARY_RATE: f64 = 0.125;
    const SECONDARY_RATE: f64 = 0.0134;

    let halving_era = (year / 4.0).floor() as u32;
    let primary_rate = INITIAL_PRIMARY_RATE / 2.0_f64.powi(halving_era as i32);

    let nominal = (primary_rate + SECONDARY_RATE) * 100.0;

    let effective_locked_ratio = 0.5;
    let real = (primary_rate + SECONDARY_RATE * (1.0 - effective_locked_ratio)) * 100.0;

    (nominal, real)
}

/// Convert a date string from "YYYY-MM-DD" to "YYYY/MM/DD" for chart display.
fn format_date_for_chart(date_str: &str) -> String {
    date_str.replace('-', "/")
}

/// Format YYYYMMDD date key to YYYY-MM-DD for chart display.
fn format_date_key(date_key: &str) -> String {
    if date_key.len() == 8 {
        format!(
            "{}-{}-{}",
            &date_key[0..4],
            &date_key[4..6],
            &date_key[6..8]
        )
    } else {
        date_key.to_string()
    }
}

fn hodl_wave_cache_has_holder_count(response: &StackedAreaChartResponse) -> bool {
    response.data.iter().all(|point| {
        point
            .values
            .get("holderCount")
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|v| *v >= 0)
            .is_some()
    })
}

async fn get_hodl_wave_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    let cache_key = "chart:hodl-wave";
    if let Some(cached) = state.cache.get::<StackedAreaChartResponse>(cache_key).await {
        if hodl_wave_cache_has_holder_count(&cached) {
            return ok(cached);
        }
        state.cache.delete(cache_key).await;
    }

    let waves = state
        .store
        .list_hodl_waves()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<StackedAreaDataPoint> = waves
        .iter()
        .map(|(date, w)| {
            let total = (w.band_24h
                + w.band_1d_1w
                + w.band_1w_1m
                + w.band_1m_3m
                + w.band_3m_6m
                + w.band_6m_1y
                + w.band_1y_3y
                + w.band_gt_3y) as f64;
            let pct = |v: i128| -> String {
                if total > 0.0 {
                    format!("{:.2}", v as f64 / total * 100.0)
                } else {
                    "0".to_string()
                }
            };

            let mut values = std::collections::HashMap::new();
            values.insert("24h".to_string(), pct(w.band_24h));
            values.insert("1d1w".to_string(), pct(w.band_1d_1w));
            values.insert("1w1m".to_string(), pct(w.band_1w_1m));
            values.insert("1m3m".to_string(), pct(w.band_1m_3m));
            values.insert("3m6m".to_string(), pct(w.band_3m_6m));
            values.insert("6m1y".to_string(), pct(w.band_6m_1y));
            values.insert("1y3y".to_string(), pct(w.band_1y_3y));
            values.insert("gt3y".to_string(), pct(w.band_gt_3y));
            values.insert("holderCount".to_string(), w.holder_count.to_string());

            StackedAreaDataPoint {
                date: format_date_key(date),
                values,
            }
        })
        .collect();

    let series = vec![
        StackedAreaSeries {
            key: "gt3y".to_string(),
            label: "> 3y".to_string(),
            color: "#a78bfa".to_string(),
        },
        StackedAreaSeries {
            key: "1y3y".to_string(),
            label: "1y-3y".to_string(),
            color: "#67e8f9".to_string(),
        },
        StackedAreaSeries {
            key: "6m1y".to_string(),
            label: "6m-1y".to_string(),
            color: "#22c55e".to_string(),
        },
        StackedAreaSeries {
            key: "3m6m".to_string(),
            label: "3m-6m".to_string(),
            color: "#d4e157".to_string(),
        },
        StackedAreaSeries {
            key: "1m3m".to_string(),
            label: "1m-3m".to_string(),
            color: "#f59e0b".to_string(),
        },
        StackedAreaSeries {
            key: "1w1m".to_string(),
            label: "1w-1m".to_string(),
            color: "#f87171".to_string(),
        },
        StackedAreaSeries {
            key: "1d1w".to_string(),
            label: "1d-1w".to_string(),
            color: "#4ade80".to_string(),
        },
        StackedAreaSeries {
            key: "24h".to_string(),
            label: "24h".to_string(),
            color: "#6366f1".to_string(),
        },
    ];

    let response = StackedAreaChartResponse {
        data,
        series,
        title: "CKB HODL Wave".to_string(),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::CachedBlockHeader;
    use ckbadger_store::CkbadgerStore;

    fn snapshot(
        date: &str,
        total_deposited: i128,
        total_issuance: i128,
        secondary_pool: i128,
        occupied_capacity: i128,
    ) -> ckbadger_store::DaoDailySnapshot {
        ckbadger_store::DaoDailySnapshot {
            date: date.to_string(),
            total_deposited,
            depositors_count: 0,
            new_deposits: 0,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 0,
            total_issuance,
            secondary_pool,
            occupied_capacity,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury: 0,
        }
    }

    #[test]
    fn test_snapshot_total_issuance_uses_indexer_value() {
        let s = snapshot("2026-02-17", 100, 999, 0, 0);
        assert_eq!(snapshot_total_issuance(&s), Some(999));
    }

    #[test]
    fn test_snapshot_total_issuance_rejects_missing_value() {
        let s = snapshot("2026-02-17", 100, 0, 0, 0);
        assert_eq!(snapshot_total_issuance(&s), None);
    }

    #[test]
    fn test_snapshot_secondary_cumulative_returns_values() {
        let mut s = snapshot("2026-02-17", 100, 999, 0, 0);
        s.cum_miner_secondary = 7;
        s.cum_dao_compensation = 8;
        s.cum_treasury = 3;

        let (miner, dao, treasury) = snapshot_secondary_cumulative(&s).unwrap();
        assert_eq!(miner, 7);
        assert_eq!(dao, 8);
        assert_eq!(treasury, 3);
    }

    #[test]
    fn test_snapshot_secondary_cumulative_errors_on_negative_values() {
        let mut s = snapshot("2026-02-17", 100, 999, 0, 0);
        s.cum_miner_secondary = -1;
        s.cum_dao_compensation = 8;
        s.cum_treasury = -3;

        let err = snapshot_secondary_cumulative(&s).unwrap_err();
        assert!(err.1 .0.message.contains("negative cum_miner_secondary"));
    }

    #[test]
    fn test_shannon_to_ckb_string_formats_integer_and_fractional() {
        assert_eq!(shannon_to_ckb_string(100_000_000), "1");
        assert_eq!(shannon_to_ckb_string(123_456_789), "1.23456789");
        assert_eq!(shannon_to_ckb_string(-123_400_000), "-1.234");
    }

    #[test]
    fn test_parse_code_hash_set_keeps_only_valid_32_byte_hashes() {
        let hashes = parse_code_hash_set(&[
            "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e",
            "0x1234",
            "0xzzzz",
        ]);
        assert_eq!(hashes.len(), 1);
    }

    #[test]
    fn test_build_circulating_supply_by_date_map_uses_total_minus_burnt_and_dao() {
        let total = GENESIS_BURNT as i128 + 1_000_000;
        let mut s = snapshot("2026-02-17", 100, total, 0, 0);
        s.cum_treasury = 30;
        let map = build_circulating_supply_by_date_map(&[s]).unwrap();
        assert_eq!(map.get("2026-02-17"), Some(&(1_000_000 - 30 - 100)));
    }

    #[test]
    fn test_build_circulating_supply_by_date_map_errors_on_negative_dao_locked() {
        let total = GENESIS_BURNT as i128 + 1_000_000;
        let s = snapshot("2026-02-17", -1, total, 0, 0);
        let err = build_circulating_supply_by_date_map(&[s]).unwrap_err();
        assert!(err.1 .0.message.contains("negative total_deposited"));
    }

    #[test]
    fn test_accumulate_capacity_deltas_errors_on_underflow() {
        let err = accumulate_capacity_deltas([(100, 50), (-200, 0)]).unwrap_err();
        assert!(err.1 .0.message.contains("underflow"));
    }

    #[test]
    fn test_calculate_daily_hash_rate_uses_millisecond_block_time() {
        // 8,640 blocks/day => 10,000ms avg block time
        let hash_rate = calculate_daily_hash_rate(1_000_000, 8_640);
        assert_eq!(hash_rate, 100.0);
    }

    #[test]
    fn test_hodl_wave_cache_has_holder_count_true_when_all_present() {
        let mut values = std::collections::HashMap::new();
        values.insert("24h".to_string(), "1.00".to_string());
        values.insert("holderCount".to_string(), "123".to_string());
        let response = StackedAreaChartResponse {
            data: vec![StackedAreaDataPoint {
                date: "2026-02-19".to_string(),
                values,
            }],
            series: vec![],
            title: "CKB HODL Wave".to_string(),
        };
        assert!(hodl_wave_cache_has_holder_count(&response));
    }

    #[test]
    fn test_hodl_wave_cache_has_holder_count_false_when_missing() {
        let mut values = std::collections::HashMap::new();
        values.insert("24h".to_string(), "1.00".to_string());
        let response = StackedAreaChartResponse {
            data: vec![StackedAreaDataPoint {
                date: "2026-02-19".to_string(),
                values,
            }],
            series: vec![],
            title: "CKB HODL Wave".to_string(),
        };
        assert!(!hodl_wave_cache_has_holder_count(&response));
    }

    #[test]
    fn test_hodl_wave_cache_has_holder_count_false_when_invalid() {
        let mut values = std::collections::HashMap::new();
        values.insert("24h".to_string(), "1.00".to_string());
        values.insert("holderCount".to_string(), "not-a-number".to_string());
        let response = StackedAreaChartResponse {
            data: vec![StackedAreaDataPoint {
                date: "2026-02-19".to_string(),
                values,
            }],
            series: vec![],
            title: "CKB HODL Wave".to_string(),
        };
        assert!(!hodl_wave_cache_has_holder_count(&response));
    }

    #[test]
    fn test_hodl_wave_cache_has_holder_count_false_when_negative() {
        let mut values = std::collections::HashMap::new();
        values.insert("24h".to_string(), "1.00".to_string());
        values.insert("holderCount".to_string(), "-1".to_string());
        let response = StackedAreaChartResponse {
            data: vec![StackedAreaDataPoint {
                date: "2026-02-19".to_string(),
                values,
            }],
            series: vec![],
            title: "CKB HODL Wave".to_string(),
        };
        assert!(!hodl_wave_cache_has_holder_count(&response));
    }

    #[test]
    fn test_block_time_ms_to_bucket_index_handles_bounds() {
        assert_eq!(block_time_ms_to_bucket_index(-100), None);
        assert_eq!(block_time_ms_to_bucket_index(0), Some(0));
        assert_eq!(block_time_ms_to_bucket_index(99), Some(0));
        assert_eq!(block_time_ms_to_bucket_index(100), Some(1));
        assert_eq!(
            block_time_ms_to_bucket_index(BLOCK_TIME_DIST_MAX_MS),
            Some(BLOCK_TIME_DIST_BUCKET_COUNT - 1)
        );
        assert_eq!(
            block_time_ms_to_bucket_index(BLOCK_TIME_DIST_MAX_MS + 1),
            None
        );
    }

    #[test]
    fn test_build_block_time_distribution_data_formats_decisecond_buckets() {
        let mut counts = vec![0u64; BLOCK_TIME_DIST_BUCKET_COUNT];
        counts[10] = 1;
        counts[20] = 3;
        let data = build_block_time_distribution_data(&counts, 4);
        assert_eq!(data.len(), BLOCK_TIME_DIST_BUCKET_COUNT);
        assert_eq!(data[0].date, "0.0");
        assert_eq!(data[10].date, "1.0");
        assert_eq!(data[20].date, "2.0");
        assert_eq!(data[10].value, "25.000");
        assert_eq!(data[20].value, "75.000");
    }

    #[test]
    fn test_build_block_time_distribution_response_uses_recent_headers() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        for (number, ts_ms) in [(0i64, 0i64), (1, 1_000), (2, 3_000)] {
            batch.put_block_header(
                number,
                &CachedBlockHeader {
                    hash: vec![number as u8; 32],
                    timestamp: ts_ms,
                    epoch_number: 0,
                    epoch_index: 0,
                    epoch_length: 1,
                    dao: vec![0; 32],
                    transactions_count: 1,
                },
            );
        }
        batch.commit().unwrap();

        let response = build_block_time_distribution_response(&store).unwrap();
        assert_eq!(
            response.title,
            "Block Time Distribution (Recent 50000 blocks)"
        );
        assert_eq!(response.data.len(), BLOCK_TIME_DIST_BUCKET_COUNT);

        let at_1s = response
            .data
            .iter()
            .find(|point| point.date == "1.0")
            .unwrap();
        let at_2s = response
            .data
            .iter()
            .find(|point| point.date == "2.0")
            .unwrap();
        assert_eq!(at_1s.value, "50.000");
        assert_eq!(at_2s.value, "50.000");
    }

    #[test]
    fn test_build_block_time_distribution_response_excludes_overflow_from_50s_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        // 0->1 is 60s (overflow), 1->2 is 1s (in-range)
        for (number, ts_ms) in [(0i64, 0i64), (1, 60_000), (2, 61_000)] {
            batch.put_block_header(
                number,
                &CachedBlockHeader {
                    hash: vec![number as u8; 32],
                    timestamp: ts_ms,
                    epoch_number: 0,
                    epoch_index: 0,
                    epoch_length: 1,
                    dao: vec![0; 32],
                    transactions_count: 1,
                },
            );
        }
        batch.commit().unwrap();

        let response = build_block_time_distribution_response(&store).unwrap();
        let at_1s = response
            .data
            .iter()
            .find(|point| point.date == "1.0")
            .unwrap();
        let at_50s = response
            .data
            .iter()
            .find(|point| point.date == "50.0")
            .unwrap();
        assert_eq!(at_1s.value, "100.000");
        assert_eq!(at_50s.value, "0.000");
    }

    #[test]
    fn test_block_number_to_date_resolves_transition_ranges() {
        let transitions = vec![
            (0, NaiveDate::from_ymd_opt(2026, 2, 18).unwrap()),
            (100, NaiveDate::from_ymd_opt(2026, 2, 19).unwrap()),
            (220, NaiveDate::from_ymd_opt(2026, 2, 20).unwrap()),
        ];

        assert_eq!(
            block_number_to_date(&transitions, 0),
            NaiveDate::from_ymd_opt(2026, 2, 18)
        );
        assert_eq!(
            block_number_to_date(&transitions, 150),
            NaiveDate::from_ymd_opt(2026, 2, 19)
        );
        assert_eq!(
            block_number_to_date(&transitions, 999),
            NaiveDate::from_ymd_opt(2026, 2, 20)
        );
        assert_eq!(block_number_to_date(&[], 0), None);
    }

    #[test]
    fn test_occupied_capacity_bucket_index_boundaries() {
        assert_eq!(occupied_capacity_bucket_index(99 * SHANNONS_PER_CKB), 0);
        assert_eq!(occupied_capacity_bucket_index(100 * SHANNONS_PER_CKB), 1);
        assert_eq!(occupied_capacity_bucket_index(1_000 * SHANNONS_PER_CKB), 2);
        assert_eq!(occupied_capacity_bucket_index(10_000 * SHANNONS_PER_CKB), 3);
        assert_eq!(
            occupied_capacity_bucket_index(100_000 * SHANNONS_PER_CKB),
            4
        );
        assert_eq!(
            occupied_capacity_bucket_index(1_000_000 * SHANNONS_PER_CKB),
            5
        );
    }
}
