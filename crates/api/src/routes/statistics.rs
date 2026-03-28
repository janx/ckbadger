#![allow(clippy::type_complexity)]

use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use chrono::{DateTime, Utc};
use ckb_types::utilities::compact_to_difficulty as ckb_compact_to_difficulty;
use ckbadger_common::dao::GENESIS_BURNT;
use ckbadger_common::sync::{format_duration_smart, BackgroundTaskEntry, SyncProgressData};
use ckbadger_store::types::{DailyAddressCohort, DailyCellDistribution};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::cache::{CacheKeys, CacheTtl};
use crate::response::{
    chart_response_has_data, ok, ApiError, ApiResult, ApiRouteError, ChartDataPoint, ChartResponse,
    SyncStatusResponse as SyncStatus,
};
use crate::utils::{apply_owned_capacity_delta, format_duration};
use crate::warmup::{
    CachedAssetEntry, CACHE_KEY_ASSETS_NFT, CACHE_KEY_ASSETS_TOKEN, CACHE_KEY_SCRIPTS_ALL,
};
use crate::AppState;

fn load_script_infos_cached(
    state: &Arc<AppState>,
) -> Result<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>, ApiRouteError> {
    state
        .mem_cache
        .get::<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>>(CACHE_KEY_SCRIPTS_ALL)
        .ok_or_else(|| ApiError::warmup_pending("script cache unavailable; warmup in progress"))
}

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
        // Activity stats
        .route("/stats/daily-activities", get(get_daily_activity_stats))
        .route("/stats/activity-summary-24h", get(get_activity_summary_24h))
        // Asset ecosystem
        .route("/statistics/asset-ecosystem", get(get_asset_ecosystem))
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
    // Hero metrics from latest DAO daily snapshot
    pub knowledge_size: Option<String>,
    pub circulating_supply: Option<String>,
    pub dao_locked: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_background_tasks: Option<Vec<BackgroundTaskEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetEcosystemResponse {
    pub top_tokens: Vec<TopTokenEntry>,
    pub capacity_breakdown: Vec<CapacityCategory>,
    pub total_knowledge_size_ckb: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopTokenEntry {
    pub type_script_hash: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub holders_count: i64,
    pub total_capacity_ckb: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapacityCategory {
    pub category: String,
    pub capacity_ckb: String,
    pub percentage: String,
}

async fn get_network_stats(State(state): State<Arc<AppState>>) -> ApiResult<NetworkStats> {
    let mut stats = if let Some(cached) = state
        .cache
        .get::<NetworkStats>(CacheKeys::NETWORK_STATS)
        .await
    {
        cached
    } else {
        let fresh = fetch_network_stats_from_db(&state).await?;
        state
            .cache
            .set(CacheKeys::NETWORK_STATS, &fresh, CacheTtl::NETWORK_STATS)
            .await;
        fresh
    };

    // Inject live background-task status after cache so it is never stale.
    let api_bg_tasks = {
        let data = state
            .background_tasks
            .read()
            .expect("background tasks lock poisoned");
        if data.tasks.is_empty() {
            None
        } else {
            Some(data.tasks.clone())
        }
    };
    stats.api_background_tasks = api_bg_tasks;

    ok(stats)
}

async fn get_tx_stats(State(state): State<Arc<AppState>>) -> ApiResult<TxStatsResponse> {
    let cache_key = "statistics:tx-stats";
    if let Some(cached) = state.cache.get::<TxStatsResponse>(cache_key).await {
        return ok(cached);
    }

    // Get the latest block header to determine reference time
    let store = state.store.clone();
    let latest_header = tokio::task::spawn_blocking(move || store.get_sync_tip_block())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_time = latest_header
        .as_ref()
        .and_then(|(_, h)| DateTime::from_timestamp_millis(h.timestamp))
        .unwrap_or_else(Utc::now);
    let reference_ts = reference_time.timestamp() * 1000; // ms

    // Get hourly stats (last 24 hours)
    let store = state.store.clone();
    let hourly_stats = tokio::task::spawn_blocking(move || store.list_hourly_stats_with_keys())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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
    let store = state.store.clone();
    let daily_stats = tokio::task::spawn_blocking(move || store.list_daily_stats_with_dates())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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

fn stacked_chart_response_has_data(response: &StackedAreaChartResponse) -> bool {
    !response.data.is_empty()
}

const MOST_UTILIZED_LIMIT: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MostUtilizedScriptsChartResponse {
    pub title: String,
    pub used_share: StackedAreaChartResponse,
    pub capacity_share: StackedAreaChartResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MostUtilizedAssetsChartResponse {
    pub title: String,
    pub used_share: StackedAreaChartResponse,
    pub capacity_share: StackedAreaChartResponse,
}

#[derive(Debug, Clone, Copy)]
enum UtilizationMetric {
    Used,
    Capacity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EntityState {
    total_cells_capacity: i128,
    used_capacity: i128,
}

#[derive(Debug, Clone)]
struct ScriptEntity {
    key: String,
    final_total_cells_capacity: i128,
    final_used_capacity: i128,
}

#[derive(Debug, Clone)]
struct AssetEntity {
    key: String,
    final_total_cells_capacity: i128,
    final_used_capacity: i128,
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
        UtilizationMetric::Used => state.used_capacity,
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
    used_of: impl Fn(&T) -> i128,
    capacity_of: impl Fn(&T) -> i128,
) -> Vec<String> {
    let mut keys: Vec<&T> = entities.iter().collect();
    keys.sort_by(|a, b| {
        let a_metric = match metric {
            UtilizationMetric::Used => used_of(a),
            UtilizationMetric::Capacity => capacity_of(a),
        };
        let b_metric = match metric {
            UtilizationMetric::Used => used_of(b),
            UtilizationMetric::Capacity => capacity_of(b),
        };
        let a_secondary = match metric {
            UtilizationMetric::Used => capacity_of(a),
            UtilizationMetric::Capacity => used_of(a),
        };
        let b_secondary = match metric {
            UtilizationMetric::Used => capacity_of(b),
            UtilizationMetric::Capacity => used_of(b),
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
    used_capacity: i128,
    capacity_delta: i128,
    used_delta: i128,
    context: &str,
) -> Result<(i128, i128), ApiRouteError> {
    apply_owned_capacity_delta(
        total_cells_capacity,
        used_capacity,
        capacity_delta,
        used_delta,
        context,
    )
    .map_err(|e| ApiError::internal(e.to_string()))
}

fn accumulate_capacity_deltas<I>(deltas: I) -> Result<(i128, i128), ApiRouteError>
where
    I: IntoIterator<Item = (i128, i128)>,
{
    let mut total_cells_capacity: i128 = 0;
    let mut used_capacity: i128 = 0;

    for (idx, (capacity_delta, used_delta)) in deltas.into_iter().enumerate() {
        (total_cells_capacity, used_capacity) = apply_owned_capacity_delta(
            total_cells_capacity,
            used_capacity,
            capacity_delta,
            used_delta,
            &format!("accumulating capacity deltas at step {}", idx + 1),
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
    }

    Ok((total_cells_capacity, used_capacity))
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
    let mut total_used_capacity: i128 = 0;

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
            for (entity_key, capacity_delta, used_delta) in deltas {
                let state = states.entry(entity_key.clone()).or_insert(EntityState {
                    total_cells_capacity: 0,
                    used_capacity: 0,
                });

                let old_capacity = state.total_cells_capacity;
                let old_used = state.used_capacity;

                let (new_capacity, new_used) = apply_owned_capacity_delta(
                    old_capacity,
                    old_used,
                    *capacity_delta,
                    *used_delta,
                    &format!(
                        "building most-utilized share chart for {} on date {}",
                        entity_key, date
                    ),
                )
                .map_err(|e| ApiError::internal(e.to_string()))?;

                state.total_cells_capacity = new_capacity;
                state.used_capacity = new_used;

                total_cells_capacity += new_capacity - old_capacity;
                total_used_capacity += new_used - old_used;
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
            UtilizationMetric::Used => total_used_capacity,
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

    let all_scripts = load_script_infos_cached(&state)?;

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

        let final_total_cells_capacity =
            info.lock_owned_capacity_sum + info.type_owned_capacity_sum;
        let final_used_capacity = info.lock_owned_knowledge_sum + info.type_owned_knowledge_sum;
        if final_total_cells_capacity < 0 {
            return Err(ApiError::internal(format!(
                "negative script total capacity for key {}: {}",
                key, final_total_cells_capacity
            )));
        }
        if final_used_capacity < 0 {
            return Err(ApiError::internal(format!(
                "negative script common knowledge size for key {}: {}",
                key, final_used_capacity
            )));
        }
        if final_used_capacity > final_total_cells_capacity {
            return Err(ApiError::internal(format!(
                "script common knowledge size exceeds total for key {}: used={}, total={}",
                key, final_used_capacity, final_total_cells_capacity
            )));
        }
        let entry = final_by_key.entry(key.clone()).or_insert((0, 0));
        entry.0 += final_total_cells_capacity;
        entry.1 += final_used_capacity;

        for is_type in [false, true] {
            let deltas = state
                .store
                .list_script_daily_deltas(&code_hash, is_type)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            for (date, delta) in deltas {
                deltas_by_date.entry(date).or_default().push((
                    key.clone(),
                    delta.owned_capacity_delta,
                    delta.owned_knowledge_delta,
                ));
            }
        }
    }

    let entities_unfiltered: Vec<ScriptEntity> = final_by_key
        .iter()
        .map(
            |(key, (capacity, used))| -> Result<ScriptEntity, ApiRouteError> {
                if *capacity < 0 {
                    return Err(ApiError::internal(format!(
                        "negative aggregated script total capacity for key {}: {}",
                        key, capacity
                    )));
                }
                if *used < 0 {
                    return Err(ApiError::internal(format!(
                        "negative aggregated script common knowledge size for key {}: {}",
                        key, used
                    )));
                }
                if *used > *capacity {
                    return Err(ApiError::internal(format!(
                    "aggregated script common knowledge size exceeds total for key {}: used={}, total={}",
                    key, used, capacity
                )));
                }
                Ok(ScriptEntity {
                    key: key.clone(),
                    final_total_cells_capacity: *capacity,
                    final_used_capacity: *used,
                })
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    let entities: Vec<ScriptEntity> = entities_unfiltered
        .into_iter()
        .filter(|entity| entity.final_total_cells_capacity > 0 || entity.final_used_capacity > 0)
        .collect();

    let top_used_keys = top_keys_by_metric(
        &entities,
        UtilizationMetric::Used,
        |entity| &entity.key,
        |entity| entity.final_used_capacity,
        |entity| entity.final_total_cells_capacity,
    );
    let top_capacity_keys = top_keys_by_metric(
        &entities,
        UtilizationMetric::Capacity,
        |entity| &entity.key,
        |entity| entity.final_used_capacity,
        |entity| entity.final_total_cells_capacity,
    );

    let dates: Vec<u32> = deltas_by_date.keys().copied().collect();
    let used_share = build_most_utilized_share_chart(
        "Top Scripts Common Knowledge Share".to_string(),
        UtilizationMetric::Used,
        &top_used_keys,
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
        title: "Scripts Used & Total CKBytes".to_string(),
        used_share,
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

    let token_assets = state
        .mem_cache
        .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_TOKEN)
        .ok_or_else(|| {
            state.asset_cache_unavailable("token asset cache unavailable; warmup in progress")
        })?;
    for token in token_assets {
        let type_hash = hex::decode(token.id.strip_prefix("0x").unwrap_or(token.id.as_str()))
            .map_err(|_| {
                ApiError::internal(format!(
                    "invalid token hash in warmup cache while building chart: {}",
                    token.id
                ))
            })?;
        let store = state.store.clone();
        let type_hash_c = type_hash.clone();
        let deltas =
            tokio::task::spawn_blocking(move || store.list_token_daily_deltas(&type_hash_c))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
                .map_err(|e| ApiError::internal(e.to_string()))?;
        let (total_cells_capacity, used_cap) = accumulate_capacity_deltas(
            deltas
                .iter()
                .map(|(_, delta)| (delta.owned_capacity_delta, delta.owned_knowledge_delta)),
        )?;
        if total_cells_capacity <= 0 && used_cap <= 0 {
            continue;
        }
        let id = token.id;
        let name = token
            .symbol
            .clone()
            .or_else(|| token.name.clone())
            .unwrap_or_else(|| id.clone());
        let entity_key = format!("token:{id}");
        labels_by_key.insert(entity_key.clone(), format_asset_label(&name, "token"));
        if used_cap > total_cells_capacity {
            return Err(ApiError::internal(format!(
                "token common knowledge size exceeds total for {}: used={}, total={}",
                entity_key, used_cap, total_cells_capacity
            )));
        }
        entities.push(AssetEntity {
            key: entity_key.clone(),
            final_total_cells_capacity: total_cells_capacity,
            final_used_capacity: used_cap,
        });
        for (date, delta) in deltas {
            deltas_by_date.entry(date).or_default().push((
                entity_key.clone(),
                delta.owned_capacity_delta,
                delta.owned_knowledge_delta,
            ));
        }
    }

    let nft_assets = state
        .mem_cache
        .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_NFT)
        .ok_or_else(|| {
            state.asset_cache_unavailable("nft asset cache unavailable; warmup in progress")
        })?;
    for nft in nft_assets {
        if nft.standard == "spore" {
            let cluster_id = nft.cluster_id.clone().unwrap_or_else(|| nft.id.clone());
            let cluster_bytes = hex::decode(cluster_id.strip_prefix("0x").unwrap_or(&cluster_id))
                .map_err(|_| {
                ApiError::internal(format!(
                    "invalid cluster id in warmup cache while building chart: {}",
                    cluster_id
                ))
            })?;
            let deltas = state
                .store
                .list_cluster_daily_deltas(&cluster_bytes)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let (total_cells_capacity, used_cap) = accumulate_capacity_deltas(
                deltas
                    .iter()
                    .map(|(_, delta)| (delta.owned_capacity_delta, delta.owned_knowledge_delta)),
            )?;
            if total_cells_capacity <= 0 && used_cap <= 0 {
                continue;
            }
            let name = nft.name.clone().unwrap_or_else(|| cluster_id.clone());
            let entity_key = format!("dob:{cluster_id}");
            labels_by_key.insert(entity_key.clone(), format_asset_label(&name, "nft"));
            if used_cap > total_cells_capacity {
                return Err(ApiError::internal(format!(
                    "DOB common knowledge size exceeds total for {}: used={}, total={}",
                    entity_key, used_cap, total_cells_capacity
                )));
            }
            entities.push(AssetEntity {
                key: entity_key.clone(),
                final_total_cells_capacity: total_cells_capacity,
                final_used_capacity: used_cap,
            });
            for (date, delta) in deltas {
                deltas_by_date.entry(date).or_default().push((
                    entity_key.clone(),
                    delta.owned_capacity_delta,
                    delta.owned_knowledge_delta,
                ));
            }
            continue;
        }

        let collection_id = nft.id;
        let collection_bytes = hex::decode(
            collection_id.strip_prefix("0x").unwrap_or(&collection_id),
        )
        .map_err(|_| {
            ApiError::internal(format!(
                "invalid nft collection id in warmup cache while building chart: {}",
                collection_id
            ))
        })?;
        let store = state.store.clone();
        let collection_bytes_c = collection_bytes.clone();
        let deltas =
            tokio::task::spawn_blocking(move || store.list_mnft_daily_deltas(&collection_bytes_c))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
                .map_err(|e| ApiError::internal(e.to_string()))?;
        let (total_cells_capacity, used_cap) = accumulate_capacity_deltas(
            deltas
                .iter()
                .map(|(_, delta)| (delta.owned_capacity_delta, delta.owned_knowledge_delta)),
        )?;
        if total_cells_capacity <= 0 && used_cap <= 0 {
            continue;
        }
        let name = nft.name.clone().unwrap_or_else(|| collection_id.clone());
        let entity_key = format!("nft:{collection_id}");
        labels_by_key.insert(entity_key.clone(), format_asset_label(&name, "nft"));
        if used_cap > total_cells_capacity {
            return Err(ApiError::internal(format!(
                "NFT common knowledge size exceeds total for {}: used={}, total={}",
                entity_key, used_cap, total_cells_capacity
            )));
        }
        entities.push(AssetEntity {
            key: entity_key.clone(),
            final_total_cells_capacity: total_cells_capacity,
            final_used_capacity: used_cap,
        });
        for (date, delta) in deltas {
            deltas_by_date.entry(date).or_default().push((
                entity_key.clone(),
                delta.owned_capacity_delta,
                delta.owned_knowledge_delta,
            ));
        }
    }

    let top_used_keys = top_keys_by_metric(
        &entities,
        UtilizationMetric::Used,
        |entity| &entity.key,
        |entity| entity.final_used_capacity,
        |entity| entity.final_total_cells_capacity,
    );
    let top_capacity_keys = top_keys_by_metric(
        &entities,
        UtilizationMetric::Capacity,
        |entity| &entity.key,
        |entity| entity.final_used_capacity,
        |entity| entity.final_total_cells_capacity,
    );

    let dates: Vec<u32> = deltas_by_date.keys().copied().collect();
    let used_share = build_most_utilized_share_chart(
        "Top Assets Common Knowledge Share".to_string(),
        UtilizationMetric::Used,
        &top_used_keys,
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
        title: "Assets Used & Total CKBytes".to_string(),
        used_share,
        capacity_share,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    ok(response)
}

async fn get_transaction_count_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let store = state.store.clone();
    let daily_stats = tokio::task::spawn_blocking(move || store.list_daily_stats_with_dates())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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
    let store = state.store.clone();
    let daily_stats = tokio::task::spawn_blocking(move || store.list_daily_stats_with_dates())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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
        if chart_response_has_data(&cached) {
            return ok(cached);
        }
        state.cache.delete(cache_key).await;
    }

    let store = state.store.clone();
    let daily_stats = tokio::task::spawn_blocking(move || store.list_daily_stats_with_dates())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let store = state.store.clone();
    let snapshots = tokio::task::spawn_blocking(move || store.list_dao_daily_snapshots())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let circulating_by_date = build_circulating_supply_by_date_map(&snapshots)?;

    // Exclude the current incomplete day to prevent cache divergence with composition chart.
    let today_key = current_ckb_date_key();
    let data: Vec<ChartDataPoint> = daily_stats
        .into_iter()
        .filter(|(date_str, _)| date_str.as_str() != today_key)
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

    if chart_response_has_data(&response) {
        state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    }

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

pub(crate) fn shannon_to_ckb_string(value: i128) -> String {
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
        if stacked_chart_response_has_data(&cached) {
            return ok(cached);
        }
        state.cache.delete(cache_key).await;
    }

    let store = state.store.clone();
    let daily_stats = tokio::task::spawn_blocking(move || store.list_daily_stats_with_dates())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    // Exclude the current incomplete day to prevent cache divergence with knowledge-size chart.
    let today_key = current_ckb_date_key();
    let mut knowledge_by_date: BTreeMap<u32, i128> = BTreeMap::new();
    for (date_key, stats) in daily_stats {
        if date_key.as_str() == today_key {
            continue;
        }
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

    let script_infos = load_script_infos_cached(&state)?;
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
        let store = state.store.clone();
        let code_hash_c = code_hash.clone();
        let deltas = tokio::task::spawn_blocking(move || {
            store.list_script_daily_deltas_in_range(&code_hash_c, true, None, None)
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
        for (date, delta) in deltas {
            let used_delta = delta.owned_knowledge_delta;
            *type_daily_delta.entry(date).or_insert(0) += used_delta;

            if dao_hashes.contains(&code_hash) {
                *dao_daily_delta.entry(date).or_insert(0) += used_delta;
            } else if udt_hashes.contains(&code_hash) {
                *udt_daily_delta.entry(date).or_insert(0) += used_delta;
            } else if nft_spore_hashes.contains(&code_hash) {
                *nft_spore_daily_delta.entry(date).or_insert(0) += used_delta;
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

    if stacked_chart_response_has_data(&response) {
        state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    }

    ok(response)
}

/// Build the cell-size-distribution chart response from a materialized snapshot.
pub(crate) fn build_cell_size_response(snapshot: &DailyCellDistribution) -> ChartResponse {
    let bucket_labels = [
        "<100 CKB",
        "100-1k CKB",
        "1k-10k CKB",
        "10k-100k CKB",
        "100k-1m CKB",
        ">=1m CKB",
    ];

    let data = bucket_labels
        .iter()
        .enumerate()
        .map(|(idx, label)| ChartDataPoint {
            date: (*label).to_string(),
            value: snapshot.size_bucket_counts[idx].to_string(),
            value2: Some(shannon_to_ckb_string(snapshot.size_bucket_capacities[idx])),
        })
        .collect();

    ChartResponse {
        data,
        title: "Cell Size Distribution".to_string(),
        y_axis_label: "Live Cells".to_string(),
        y2_axis_label: Some("Common Knowledge Size (CKB)".to_string()),
    }
}

pub(crate) fn empty_cell_size_response() -> ChartResponse {
    ChartResponse {
        data: Vec::new(),
        title: "Cell Size Distribution".to_string(),
        y_axis_label: "Live Cells".to_string(),
        y2_axis_label: Some("Common Knowledge Size (CKB)".to_string()),
    }
}

/// Build the address-cohort-retention chart response from a materialized snapshot.
pub(crate) fn build_address_cohort_response(cohort: &DailyAddressCohort) -> ChartResponse {
    let mut sorted_cohorts: Vec<_> = cohort.cohorts.iter().collect();
    sorted_cohorts.sort_by(|a, b| a.cohort_month.cmp(&b.cohort_month));

    let data = sorted_cohorts
        .into_iter()
        .map(|entry| {
            let retention = if entry.total_balance > 0 {
                entry.used_capacity as f64 * 100.0 / entry.total_balance as f64
            } else {
                0.0
            };
            ChartDataPoint {
                date: entry.cohort_month.clone(),
                value: format!("{retention:.6}"),
                value2: Some(shannon_to_ckb_string(entry.used_capacity)),
            }
        })
        .collect();

    ChartResponse {
        data,
        title: "Address Cohort Retention".to_string(),
        y_axis_label: "Common Knowledge / Balance (%)".to_string(),
        y2_axis_label: Some("Common Knowledge Size (CKB)".to_string()),
    }
}

pub(crate) fn empty_address_cohort_response() -> ChartResponse {
    ChartResponse {
        data: Vec::new(),
        title: "Address Cohort Retention".to_string(),
        y_axis_label: "Common Knowledge / Balance (%)".to_string(),
        y2_axis_label: Some("Common Knowledge Size (CKB)".to_string()),
    }
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
        let consumed = stats.used_capacity_consumed;
        if consumed < 0 {
            return Err(ApiError::internal(format!(
                "negative used_capacity_consumed in daily_stats for {}: {}",
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

    let latest_snapshot = state
        .store
        .get_latest_cell_distribution()
        .map_err(|e| ApiError::internal(format!("failed to read cell distribution: {e}")))?;
    let Some((_, snapshot)) = latest_snapshot else {
        return ok(empty_cell_size_response());
    };

    let response = build_cell_size_response(&snapshot);
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

    let latest_snapshot = state
        .store
        .get_latest_address_cohort()
        .map_err(|e| ApiError::internal(format!("failed to read address cohort: {e}")))?;
    let Some((_, snapshot)) = latest_snapshot else {
        return ok(empty_address_cohort_response());
    };

    let response = build_address_cohort_response(&snapshot);
    state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    ok(response)
}

async fn get_block_time_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:block-time-distribution:v2";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        if block_time_dist_has_data(&cached) {
            return ok(cached);
        }
        state.cache.delete(cache_key).await;
    }

    let response =
        build_block_time_distribution_response(state.store.as_ref()).map_err(ApiError::internal)?;

    if block_time_dist_has_data(&response) {
        state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    }

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

fn block_time_dist_has_data(response: &ChartResponse) -> bool {
    response
        .data
        .iter()
        .any(|p| p.value.parse::<f64>().is_ok_and(|v| v > 0.0))
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

    let store = state.store.clone();
    let dist = tokio::task::spawn_blocking(move || store.list_epoch_time_dist())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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

    let store = state.store.clone();
    let epochs = tokio::task::spawn_blocking(move || store.list_epoch_stats())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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
        if chart_response_has_data(&cached) {
            return ok(cached);
        }
        state.cache.delete(cache_key).await;
    }

    let store = state.store.clone();
    let daily_stats = tokio::task::spawn_blocking(move || store.list_daily_stats_with_dates())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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

    if chart_response_has_data(&response) {
        state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    }

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
    let daily_block_stats = match store
        .get_daily_block_stats(&today_str)
        .map_err(|e| ApiError::internal(e.to_string()))?
    {
        Some(stats) => Some(stats),
        None => store
            .get_daily_block_stats(&yesterday_str)
            .map_err(|e| ApiError::internal(e.to_string()))?,
    };

    let compact_target = daily_block_stats
        .as_ref()
        .map(|s| s.avg_compact_target as i64)
        .unwrap_or(0);

    let remaining_blocks = epoch_length - epoch_index;
    let estimated_epoch_seconds = (remaining_blocks as f64 * epoch_avg_time) as i64;

    let tps = tx_count_24h as f64 / 86400.0;
    let tx_per_minute = tps * 60.0;

    // Get sync status from store (single source of truth)
    let store_sync = store
        .get_sync_status()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let synced_block = store_sync.tip_block_number;
    let db_ema_rate = store_sync.sync_ema_rate;
    let sync_started_at = store_sync.sync_started_at;
    let bulk_sync_completed_at = store_sync.bulk_sync_completed_at;

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

    let sync_progress_from_store: Option<SyncProgressData> = store
        .get_sync_progress()
        .ok()
        .flatten()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());

    let (
        progress,
        estimated_time,
        blocks_per_second,
        ema_blocks_per_second,
        txs_per_second,
        ema_txs_per_second,
    ) = if let Some(ref sp) = sync_progress_from_store {
        let stale = Utc::now().timestamp() - sp.updated_at > 60;
        if !stale && is_syncing {
            (
                sp.progress_percentage,
                Some(sp.eta_formatted.clone()),
                Some(sp.blocks_per_second),
                Some(sp.ema_blocks_per_second),
                sp.txs_per_second,
                sp.ema_txs_per_second,
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
            (p, eta, ema, ema, None, None)
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
        (p, eta, ema, ema, None, None)
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
        txs_per_second,
        ema_txs_per_second,
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
    // Use epoch average block time for stable hash rate estimate.
    // Individual block intervals are too noisy; the epoch window (~4h)
    // matches CKB's difficulty adjustment granularity.
    let hash_rate = if epoch_avg_time > 0.0 {
        difficulty as f64 / epoch_avg_time
    } else {
        0.0
    };

    // Hero metrics from latest DAO daily snapshot
    let dao_snapshot = store
        .get_latest_dao_daily_snapshot()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let knowledge_size = dao_snapshot
        .as_ref()
        .map(|s| s.occupied_capacity.to_string());
    let circulating_supply = match dao_snapshot.as_ref() {
        Some(s) => {
            let total_supply = s.total_issuance;
            let (_, _, cum_treasury) = snapshot_secondary_cumulative(s)?;
            let burnt = GENESIS_BURNT as i128 + cum_treasury;
            Some((total_supply - burnt - s.total_deposited).to_string())
        }
        None => None,
    };
    let dao_locked = dao_snapshot.as_ref().map(|s| s.total_deposited.to_string());

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
        knowledge_size,
        circulating_supply,
        dao_locked,
        api_background_tasks: None,
    })
}

async fn get_hash_rate_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:hash-rate";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        if chart_response_has_data(&cached) {
            return ok(cached);
        }
        state.cache.delete(cache_key).await;
    }

    let store = state.store.clone();
    let daily_block_stats = tokio::task::spawn_blocking(move || store.list_daily_block_stats())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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

    if chart_response_has_data(&response) {
        state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    }

    ok(response)
}

async fn get_difficulty_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:difficulty";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let store = state.store.clone();
    let daily_block_stats = tokio::task::spawn_blocking(move || store.list_daily_block_stats())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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

    let store = state.store.clone();
    let daily_block_stats = tokio::task::spawn_blocking(move || store.list_daily_block_stats())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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

    let store = state.store.clone();
    let miner_stats = tokio::task::spawn_blocking(move || store.list_miner_stats())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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

    // Hash rate = difficulty / avg_block_time_seconds
    // avg_block_time_seconds = 86400 / block_count
    let avg_block_time_s = 86_400.0 / block_count as f64;
    if avg_block_time_s <= 0.0 {
        0.0
    } else {
        difficulty as f64 / avg_block_time_s
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

    let store = state.store.clone();
    let snapshots = tokio::task::spawn_blocking(move || store.list_dao_daily_snapshots())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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

    let store = state.store.clone();
    let snapshots = tokio::task::spawn_blocking(move || store.list_dao_daily_snapshots())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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

/// Current CKB day (UTC+8) as "YYYYMMDD" key. Used to exclude incomplete current day from charts.
fn current_ckb_date_key() -> String {
    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET).unwrap();
    Utc::now()
        .with_timezone(&utc8)
        .date_naive()
        .format("%Y%m%d")
        .to_string()
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

    let store = state.store.clone();
    let waves = tokio::task::spawn_blocking(move || store.list_hodl_waves())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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

// ============================================
// Daily Activity Stats
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyActivityStatsResponse {
    pub date: String,
    pub transfer_count: u32,
    pub dao_deposit_count: u32,
    pub dao_withdraw_request_count: u32,
    pub dao_withdraw_complete_count: u32,
    pub token_count: u32,
    pub object_count: u32,
    pub identity_count: u32,
    pub script_call_count: u32,
    pub unknown_count: u32,
    pub coinbase_count: u32,
    pub unique_address_count: u32,
    pub total_ckb_moved: String,
    pub script_counts: Vec<ScriptCountEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCountEntry {
    pub code_hash: String,
    pub name: Option<String>,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySummary24hResponse {
    pub transfer_count: u32,
    pub dao_deposit_count: u32,
    pub dao_withdraw_request_count: u32,
    pub dao_withdraw_complete_count: u32,
    pub token_count: u32,
    pub object_count: u32,
    pub identity_count: u32,
    pub script_call_count: u32,
    pub unknown_count: u32,
    pub coinbase_count: u32,
    /// Sum of per-hour unique address counts (approximate: overcounts cross-hour addresses)
    pub unique_address_count: u32,
    pub total_ckb_moved: String,
    pub script_counts: Vec<ScriptCountEntry>,
    /// Number of hourly buckets aggregated (0-24)
    pub hours_covered: u32,
}

#[derive(Debug, Deserialize)]
pub struct DailyActivityStatsParams {
    #[serde(default = "default_activity_days")]
    days: u32,
}

fn default_activity_days() -> u32 {
    30
}

async fn get_daily_activity_stats(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DailyActivityStatsParams>,
) -> ApiResult<Vec<DailyActivityStatsResponse>> {
    // days=0 means return all data (used by charts)
    let days = params.days;
    let cache_key = format!("stats:daily-activity-stats:{}", days);

    if let Some(cached) = state
        .cache
        .get::<Vec<DailyActivityStatsResponse>>(&cache_key)
        .await
    {
        return ok(cached);
    }

    let store = state.store.clone();
    let all_stats = tokio::task::spawn_blocking(move || store.list_daily_activity_stats())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // days=0 returns all data; otherwise take the last N days
    let selected: Vec<_> = if days == 0 {
        all_stats
    } else {
        all_stats
            .into_iter()
            .rev()
            .take(days as usize)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    };

    // Collect all unique code_hashes across selected days and resolve names
    let mut unique_code_hashes: HashSet<String> = HashSet::new();
    for (_date, s) in &selected {
        for code_hash in s.script_counts.keys() {
            unique_code_hashes.insert(code_hash.clone());
        }
    }

    let mut name_cache: HashMap<String, Option<String>> = HashMap::new();
    for code_hash_hex in &unique_code_hashes {
        if let Ok(bytes) = hex::decode(code_hash_hex) {
            let name = state
                .store
                .get_script_info(&bytes)
                .ok()
                .flatten()
                .and_then(|info| info.name);
            name_cache.insert(code_hash_hex.clone(), name);
        }
    }

    let result: Vec<DailyActivityStatsResponse> = selected
        .into_iter()
        .map(|(date, s)| {
            let mut script_counts: Vec<ScriptCountEntry> = s
                .script_counts
                .iter()
                .map(|(code_hash_hex, &count)| ScriptCountEntry {
                    code_hash: format!("0x{}", code_hash_hex),
                    name: name_cache.get(code_hash_hex).cloned().flatten(),
                    count,
                })
                .collect();
            script_counts.sort_by(|a, b| b.count.cmp(&a.count));

            DailyActivityStatsResponse {
                date,
                transfer_count: s.transfer_count,
                dao_deposit_count: s.dao_deposit_count,
                dao_withdraw_request_count: s.dao_withdraw_request_count,
                dao_withdraw_complete_count: s.dao_withdraw_complete_count,
                token_count: s.token_count,
                object_count: s.object_count,
                identity_count: s.identity_count,
                script_call_count: s.script_call_count,
                unknown_count: s.unknown_count,
                coinbase_count: s.coinbase_count,
                unique_address_count: s.unique_address_count,
                total_ckb_moved: s.total_ckb_moved.to_string(),
                script_counts,
            }
        })
        .collect();

    // Don't cache empty results — the indexer may not have written data yet
    // and we don't want to serve stale empty responses for hours.
    if !result.is_empty() {
        // Small day counts (homepage widget) use a short TTL; full chart data uses CHART TTL.
        let ttl = if days > 0 && days <= 7 {
            CacheTtl::NETWORK_STATS
        } else {
            CacheTtl::CHART
        };
        state.cache.set(&cache_key, &result, ttl).await;
    }
    ok(result)
}

// ============================================
// Activity Summary (24h rolling window)
// ============================================

async fn get_activity_summary_24h(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ActivitySummary24hResponse> {
    let cache_key = "stats:activity-summary-24h";

    if let Some(cached) = state
        .cache
        .get::<ActivitySummary24hResponse>(cache_key)
        .await
    {
        return ok(cached);
    }

    // Compute the hour key for 24 hours ago
    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::hours(24);
    let since_hour = cutoff.format("%Y%m%d%H").to_string();

    let store = state.store.clone();
    let since_hour_c = since_hour.clone();
    let hourly_stats =
        tokio::task::spawn_blocking(move || store.list_hourly_activity_stats_since(&since_hour_c))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

    // Aggregate all hourly buckets
    let mut agg = ckbadger_store::DailyActivityStats::default();
    let mut agg_script_counts: HashMap<String, u32> = HashMap::new();
    let hours_covered = hourly_stats.len() as u32;

    for (_hour, s) in &hourly_stats {
        agg.transfer_count += s.transfer_count;
        agg.dao_deposit_count += s.dao_deposit_count;
        agg.dao_withdraw_request_count += s.dao_withdraw_request_count;
        agg.dao_withdraw_complete_count += s.dao_withdraw_complete_count;
        agg.token_count += s.token_count;
        agg.object_count += s.object_count;
        agg.identity_count += s.identity_count;
        agg.script_call_count += s.script_call_count;
        agg.unknown_count += s.unknown_count;
        agg.coinbase_count += s.coinbase_count;
        agg.unique_address_count += s.unique_address_count;
        agg.total_ckb_moved = agg
            .total_ckb_moved
            .checked_add(s.total_ckb_moved)
            .ok_or_else(|| {
                ApiError::internal(
                    "total_ckb_moved overflow aggregating 24h activity stats".to_string(),
                )
            })?;
        for (code_hash, count) in &s.script_counts {
            *agg_script_counts.entry(code_hash.clone()).or_insert(0) += count;
        }
    }

    // Resolve script names
    let mut name_cache: HashMap<String, Option<String>> = HashMap::new();
    for code_hash_hex in agg_script_counts.keys() {
        if let Ok(bytes) = hex::decode(code_hash_hex) {
            let name = state
                .store
                .get_script_info(&bytes)
                .ok()
                .flatten()
                .and_then(|info| info.name);
            name_cache.insert(code_hash_hex.clone(), name);
        }
    }

    let mut script_counts: Vec<ScriptCountEntry> = agg_script_counts
        .iter()
        .map(|(ch, &count)| ScriptCountEntry {
            code_hash: format!("0x{}", ch),
            name: name_cache.get(ch).cloned().flatten(),
            count,
        })
        .collect();
    script_counts.sort_by(|a, b| b.count.cmp(&a.count));

    let result = ActivitySummary24hResponse {
        transfer_count: agg.transfer_count,
        dao_deposit_count: agg.dao_deposit_count,
        dao_withdraw_request_count: agg.dao_withdraw_request_count,
        dao_withdraw_complete_count: agg.dao_withdraw_complete_count,
        token_count: agg.token_count,
        object_count: agg.object_count,
        identity_count: agg.identity_count,
        script_call_count: agg.script_call_count,
        unknown_count: agg.unknown_count,
        coinbase_count: agg.coinbase_count,
        unique_address_count: agg.unique_address_count,
        total_ckb_moved: agg.total_ckb_moved.to_string(),
        script_counts,
        hours_covered,
    };

    state
        .cache
        .set(cache_key, &result, CacheTtl::NETWORK_STATS)
        .await;
    ok(result)
}

async fn get_asset_ecosystem(
    State(state): State<Arc<AppState>>,
) -> ApiResult<AssetEcosystemResponse> {
    let cache_key = "statistics:asset-ecosystem";
    if let Some(cached) = state.cache.get::<AssetEcosystemResponse>(cache_key).await {
        return ok(cached);
    }

    // Get top 5 tokens from the warmup cache (already sorted by transfers_24h DESC, holders DESC)
    let token_assets = state
        .mem_cache
        .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_TOKEN)
        .ok_or_else(|| {
            state.asset_cache_unavailable("token asset cache unavailable; warmup in progress")
        })?;

    let top_tokens: Vec<TopTokenEntry> = token_assets
        .iter()
        .take(5)
        .map(|t| {
            let capacity_shannon: i128 = t
                .owned_capacity
                .as_deref()
                .and_then(|s| s.parse::<i128>().ok())
                .unwrap_or(0);
            TopTokenEntry {
                type_script_hash: t.id.clone(),
                name: t.name.clone(),
                symbol: t.symbol.clone(),
                holders_count: t.holders_count,
                total_capacity_ckb: shannon_to_ckb_string(capacity_shannon),
            }
        })
        .collect();

    // Sum total token capacity
    let total_token_capacity: i128 = token_assets
        .iter()
        .filter_map(|t| {
            t.owned_capacity
                .as_deref()
                .and_then(|s| s.parse::<i128>().ok())
        })
        .sum();

    // Get total NFT/object capacity
    let nft_assets = state
        .mem_cache
        .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_NFT)
        .ok_or_else(|| {
            state.asset_cache_unavailable("nft asset cache unavailable; warmup in progress")
        })?;

    let total_object_capacity: i128 = nft_assets
        .iter()
        .filter_map(|n| {
            n.owned_capacity
                .as_deref()
                .and_then(|s| s.parse::<i128>().ok())
        })
        .sum();

    // Get DAO locked and knowledge size from latest snapshot
    let store = state.store.clone();
    let dao_snapshot = tokio::task::spawn_blocking(move || store.get_latest_dao_daily_snapshot())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (dao_locked, knowledge_size) = match dao_snapshot.as_ref() {
        Some(s) => (s.total_deposited, s.occupied_capacity),
        None => (0, 0),
    };

    // Compute "other" = knowledge_size - (tokens + objects + dao)
    let categorized = total_token_capacity + total_object_capacity + dao_locked;
    let other_capacity = if knowledge_size > categorized {
        knowledge_size - categorized
    } else {
        0
    };

    // Build capacity breakdown with percentages
    let total = knowledge_size;
    let pct = |value: i128| -> String {
        if total <= 0 {
            return "0.00".to_string();
        }
        let scaled = value.saturating_mul(10_000).checked_div(total).unwrap_or(0);
        let whole = scaled / 100;
        let frac = (scaled % 100).abs();
        format!("{whole}.{frac:02}")
    };

    let capacity_breakdown = vec![
        CapacityCategory {
            category: "dao".to_string(),
            capacity_ckb: shannon_to_ckb_string(dao_locked),
            percentage: pct(dao_locked),
        },
        CapacityCategory {
            category: "tokens".to_string(),
            capacity_ckb: shannon_to_ckb_string(total_token_capacity),
            percentage: pct(total_token_capacity),
        },
        CapacityCategory {
            category: "objects".to_string(),
            capacity_ckb: shannon_to_ckb_string(total_object_capacity),
            percentage: pct(total_object_capacity),
        },
        CapacityCategory {
            category: "other".to_string(),
            capacity_ckb: shannon_to_ckb_string(other_capacity),
            percentage: pct(other_capacity),
        },
    ];

    let response = AssetEcosystemResponse {
        top_tokens,
        capacity_breakdown,
        total_knowledge_size_ckb: shannon_to_ckb_string(knowledge_size),
    };

    state
        .cache
        .set(cache_key, &response, CacheTtl::ASSET_ECOSYSTEM)
        .await;

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
            unclaimed_compensation: 0,
            cumulative_depositors: 0,
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
    fn test_calculate_daily_hash_rate_uses_seconds() {
        // 8,640 blocks/day => 10s avg block time
        // hash_rate = difficulty / avg_block_time_s = 1_000_000 / 10 = 100_000 H/s
        let hash_rate = calculate_daily_hash_rate(1_000_000, 8_640);
        assert_eq!(hash_rate, 100000.0);
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
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        for (number, ts_ms) in [(0i64, 0i64), (1, 1_000), (2, 3_000)] {
            batch.put_block_header(
                number,
                &CachedBlockHeader {
                    hash: vec![number as u8; 32],
                    parent_hash: vec![0u8; 32],
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
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        // 0->1 is 60s (overflow), 1->2 is 1s (in-range)
        for (number, ts_ms) in [(0i64, 0i64), (1, 60_000), (2, 61_000)] {
            batch.put_block_header(
                number,
                &CachedBlockHeader {
                    hash: vec![number as u8; 32],
                    parent_hash: vec![0u8; 32],
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
    fn test_asset_ecosystem_response_serializes_camel_case() {
        let response = AssetEcosystemResponse {
            top_tokens: vec![TopTokenEntry {
                type_script_hash: "0xabc".to_string(),
                name: Some("TestToken".to_string()),
                symbol: Some("TT".to_string()),
                holders_count: 42,
                total_capacity_ckb: "1000".to_string(),
            }],
            capacity_breakdown: vec![CapacityCategory {
                category: "dao".to_string(),
                capacity_ckb: "500".to_string(),
                percentage: "50.00".to_string(),
            }],
            total_knowledge_size_ckb: "1000".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"topTokens\""));
        assert!(json.contains("\"capacityBreakdown\""));
        assert!(json.contains("\"totalKnowledgeSizeCkb\""));
        assert!(json.contains("\"typeScriptHash\""));
        assert!(json.contains("\"holdersCount\""));
        assert!(json.contains("\"totalCapacityCkb\""));
        assert!(json.contains("\"capacityCkb\""));
    }

    #[test]
    fn test_asset_ecosystem_response_roundtrips_through_serde() {
        let response = AssetEcosystemResponse {
            top_tokens: vec![
                TopTokenEntry {
                    type_script_hash: "0xaa".to_string(),
                    name: None,
                    symbol: None,
                    holders_count: 0,
                    total_capacity_ckb: "0".to_string(),
                },
                TopTokenEntry {
                    type_script_hash: "0xbb".to_string(),
                    name: Some("CKB".to_string()),
                    symbol: Some("CKB".to_string()),
                    holders_count: 100,
                    total_capacity_ckb: "999.5".to_string(),
                },
            ],
            capacity_breakdown: vec![
                CapacityCategory {
                    category: "dao".to_string(),
                    capacity_ckb: "200".to_string(),
                    percentage: "40.00".to_string(),
                },
                CapacityCategory {
                    category: "other".to_string(),
                    capacity_ckb: "300".to_string(),
                    percentage: "60.00".to_string(),
                },
            ],
            total_knowledge_size_ckb: "500".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: AssetEcosystemResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.top_tokens.len(), 2);
        assert_eq!(deserialized.capacity_breakdown.len(), 2);
        assert_eq!(deserialized.total_knowledge_size_ckb, "500");
        assert_eq!(deserialized.top_tokens[0].type_script_hash, "0xaa");
        assert_eq!(deserialized.top_tokens[1].holders_count, 100);
        assert_eq!(deserialized.capacity_breakdown[0].category, "dao");
        assert_eq!(deserialized.capacity_breakdown[1].percentage, "60.00");
    }

    #[test]
    fn test_build_cell_size_response_from_snapshot() {
        let snapshot = DailyCellDistribution {
            size_bucket_counts: [10, 20, 30, 40, 50, 60],
            size_bucket_capacities: [
                100_50000000, // 100.5 CKB
                200_00000000,
                300_00000000,
                400_00000000,
                500_00000000,
                600_00000000,
            ],
        };
        let response = build_cell_size_response(&snapshot);
        assert_eq!(response.title, "Cell Size Distribution");
        assert_eq!(response.data.len(), 6);
        assert_eq!(response.data[0].date, "<100 CKB");
        assert_eq!(response.data[0].value, "10");
        assert_eq!(response.data[0].value2.as_deref(), Some("100.5"));
        assert_eq!(response.data[5].date, ">=1m CKB");
        assert_eq!(response.data[5].value, "60");
    }

    #[test]
    fn test_empty_cell_size_response_has_metadata() {
        let response = empty_cell_size_response();
        assert_eq!(response.title, "Cell Size Distribution");
        assert_eq!(response.y_axis_label, "Live Cells");
        assert_eq!(
            response.y2_axis_label.as_deref(),
            Some("Common Knowledge Size (CKB)")
        );
        assert!(response.data.is_empty());
    }

    #[test]
    fn test_build_address_cohort_response_from_snapshot() {
        let cohort = DailyAddressCohort {
            cohorts: vec![
                ckbadger_store::AddressCohortEntry {
                    cohort_month: "2024-02".to_string(),
                    used_capacity: 200_00000000,
                    total_balance: 800_00000000,
                },
                ckbadger_store::AddressCohortEntry {
                    cohort_month: "2024-01".to_string(),
                    used_capacity: 100_00000000,
                    total_balance: 500_00000000,
                },
            ],
        };
        let response = build_address_cohort_response(&cohort);
        assert_eq!(response.title, "Address Cohort Retention");
        assert_eq!(response.data.len(), 2);
        // Should be sorted by cohort_month
        assert_eq!(response.data[0].date, "2024-01");
        assert_eq!(response.data[1].date, "2024-02");
        // retention = used / balance * 100
        // 2024-01: 100/500 * 100 = 20.0
        assert_eq!(response.data[0].value, "20.000000");
        assert_eq!(response.data[0].value2.as_deref(), Some("100"));
    }

    #[test]
    fn test_empty_address_cohort_response_has_metadata() {
        let response = empty_address_cohort_response();
        assert_eq!(response.title, "Address Cohort Retention");
        assert_eq!(response.y_axis_label, "Common Knowledge / Balance (%)");
        assert_eq!(
            response.y2_axis_label.as_deref(),
            Some("Common Knowledge Size (CKB)")
        );
        assert!(response.data.is_empty());
    }
}
