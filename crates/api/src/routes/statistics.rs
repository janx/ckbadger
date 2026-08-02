#![allow(clippy::type_complexity)]

use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use chrono::{DateTime, TimeZone, Utc};
use ckb_types::utilities::compact_to_difficulty as ckb_compact_to_difficulty;
use ckbadger_common::sync::{format_duration_smart, BackgroundTaskEntry, SyncProgressData};
use ckbadger_indexer::parser::registry::{ProtocolScript, PROTOCOL_REGISTRY};
use ckbadger_store::types::{CachedBlockHeader, DailyAddressCohort, DailyCellDistribution};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::cache::{CacheKeys, CacheTtl};
use crate::response::{
    chart_response_has_data, ok, ApiError, ApiResult, ApiRouteError, ChartDataPoint, ChartResponse,
    SyncStatusResponse as SyncStatus,
};
use crate::utils::{
    apply_owned_capacity_delta, dao_supply, dao_treasury, format_duration, hash_type_to_string,
    script_to_address,
};
use crate::warmup::CACHE_KEY_SCRIPTS_ALL;
use crate::AppState;
use tracing::instrument;

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
    /// Shares of `total_live_capacity_ckb` (full cell capacities on both
    /// sides), NOT of the knowledge size.
    pub capacity_breakdown: Vec<CapacityCategory>,
    /// Breakdown denominator: total live capacity, `C − S` from the tip
    /// header's DAO field.
    pub total_live_capacity_ckb: String,
    /// Standalone stat: common knowledge size (occupied bytes) from the
    /// latest DAO daily snapshot.
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

#[instrument(skip(state), level = "debug")]
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

#[instrument(skip(state), level = "debug")]
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
    // currentDay reports the natural (UTC+8) day so far — the same daily
    // bucket series that dailyData charts. The rolling window-normalized
    // per-day RATE lives in /statistics/network (transactionsPerDay); this
    // endpoint deliberately does not introduce a third semantic. An absent
    // bucket means no transactions have landed today yet (legitimate empty
    // state right after the UTC+8 midnight, not a masked invariant).
    let reference_date_str = reference_date.format("%Y%m%d").to_string();
    let txs_today: i64 = recent_daily
        .iter()
        .find(|(date_str, _)| *date_str == reference_date_str)
        .map(|(_, s)| s.transactions_count as i64)
        .unwrap_or(0);

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
        current_day: txs_today,
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

/// Page size for the recent-blocks 24h window collection (~10s/block ⇒
/// ~8.6k blocks/day, so a normal day spans ~5 pages).
const RECENT_BLOCKS_PAGE_SIZE: usize = 2000;

/// Safety bound on pages per collection. At the production page size this is
/// 200k blocks — impossible inside 24h — so hitting it means the cutoff or
/// the stored timestamps broke an invariant; fail fast instead of silently
/// truncating the window.
const RECENT_BLOCKS_MAX_PAGES: usize = 100;

/// Collect every header newer than `cutoff_ts`, walking `list_blocks_desc`
/// pages down from the tip until the first at-or-before-cutoff header or
/// store exhaustion — never a fixed total cap. A busy day exceeds any such
/// cap (node-proven: 10,141 blocks inside 24h on 2026-07-30, silently
/// shortened by the old single 10,000-block fetch). Returns headers
/// newest-first.
fn collect_recent_window_blocks(
    store: &ckbadger_store::CkbadgerStore,
    cutoff_ts: i64,
    page_size: usize,
) -> Result<Vec<(i64, CachedBlockHeader)>, ApiRouteError> {
    let mut blocks: Vec<(i64, CachedBlockHeader)> = Vec::new();
    let mut from_block: Option<i64> = None;
    for _ in 0..RECENT_BLOCKS_MAX_PAGES {
        let page = store
            .list_blocks_desc(from_block, page_size)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let exhausted = page.len() < page_size;
        for (block_num, header) in page {
            if header.timestamp <= cutoff_ts {
                return Ok(blocks);
            }
            blocks.push((block_num, header));
        }
        if exhausted {
            // Every stored block is inside the window.
            return Ok(blocks);
        }
        // A full page whose headers were all at/before the cutoff returned
        // above at its first header, so a full page always pushed blocks.
        let lowest_num = blocks
            .last()
            .map(|(num, _)| *num)
            .expect("full page produced no in-window blocks");
        if lowest_num == 0 {
            // Genesis reached: nothing exists below block 0 (and the cursor
            // must not step negative — encode_block_num rejects it).
            return Ok(blocks);
        }
        from_block = Some(lowest_num - 1);
    }
    Err(ApiError::internal(format!(
        "recent-blocks window collection exceeded safety bound: {} blocks in {} pages (page_size={}) without reaching cutoff_ts={} — cutoff or stored timestamps are broken",
        blocks.len(),
        RECENT_BLOCKS_MAX_PAGES,
        page_size,
        cutoff_ts
    )))
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

    let mut blocks: Vec<RecentBlockItem> =
        collect_recent_window_blocks(&store, cutoff_ts, RECENT_BLOCKS_PAGE_SIZE)?
            .into_iter()
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

/// Number of block gaps in the network-stats average-block-time window
/// (~100 minutes of chain time at the 10s target).
const NETWORK_STATS_BLOCK_TIME_WINDOW: usize = 600;

/// Estimate the network hash rate (true H/s) from the actual PoW work in the
/// recent header window: Σ per-block difficulty over the window's time span.
///
/// For Eaglesong PoW a block's difficulty ≈ the expected hashes to mine it,
/// so summing each block's own difficulty measures the work actually done in
/// the span. Dividing the tip-epoch difficulty by the window's average block
/// time instead overstates the rate by the difficulty step (~+20% observed,
/// 87.98 vs exact 73.54 PH/s) for the first ~window blocks after every epoch
/// boundary, because the window still spans mostly previous-epoch blocks
/// mined at the old difficulty.
///
/// `headers_desc` is newest-first (`list_blocks_desc` order). The oldest
/// header's own difficulty is excluded from the numerator: the span covers
/// only the gaps from oldest to newest, and the oldest block's work predates
/// its first gap. Returns `Ok(None)` while the store holds fewer than 2
/// blocks (mirrors the avg-block-time "no data yet" case).
fn estimate_hash_rate_from_window(
    headers_desc: &[(i64, CachedBlockHeader)],
) -> Result<Option<f64>, ApiRouteError> {
    if headers_desc.len() < 2 {
        return Ok(None);
    }
    let (newest_num, newest) = headers_desc.first().expect("len >= 2");
    let (oldest_num, oldest) = headers_desc.last().expect("len >= 2");
    let span_ms = newest.timestamp - oldest.timestamp;
    if span_ms <= 0 {
        return Err(ApiError::internal(format!(
            "non-increasing block timestamps across hash-rate window: newest_block={}, newest_ts_ms={}, oldest_block={}, oldest_ts_ms={}",
            newest_num, newest.timestamp, oldest_num, oldest.timestamp
        )));
    }
    let mut work_sum: u128 = 0;
    for (block_num, header) in &headers_desc[..headers_desc.len() - 1] {
        let difficulty_u256 = ckb_compact_to_difficulty(header.compact_target);
        let difficulty: u128 = difficulty_u256.to_string().parse().map_err(|_| {
            ApiError::internal(format!(
                "block difficulty exceeds u128 range in hash-rate window: block={}, compact_target={:#x}, difficulty={}",
                block_num, header.compact_target, difficulty_u256
            ))
        })?;
        work_sum = work_sum.checked_add(difficulty).ok_or_else(|| {
            ApiError::internal(format!(
                "summed work overflows u128 in hash-rate window: block={}, work_sum_so_far={}",
                block_num, work_sum
            ))
        })?;
    }
    Ok(Some(work_sum as f64 / (span_ms as f64 / 1000.0)))
}

/// Sum committed transactions over the trailing 24 hours of hourly buckets:
/// buckets whose start falls in `(reference - 24h, reference]`, newest 24.
/// Returns `(count, window_seconds)` where the window spans from the oldest
/// included bucket's start to the reference timestamp — the exact span the
/// count covers, for rate normalization. `(0, 0.0)` when no buckets qualify.
fn rolling_24h_tx_window(
    hourly: &[(String, ckbadger_store::HourlyStats)],
    reference_ts_ms: i64,
) -> (i64, f64) {
    let cutoff_ms = reference_ts_ms - 24 * 3600 * 1000;
    let mut recent: Vec<&ckbadger_store::HourlyStats> = hourly
        .iter()
        .map(|(_, h)| h)
        .filter(|h| h.hour * 1000 > cutoff_ms && h.hour * 1000 <= reference_ts_ms)
        .collect();
    recent.sort_by(|a, b| b.hour.cmp(&a.hour));
    recent.truncate(24);
    let count = recent.iter().map(|h| h.transactions_count as i64).sum();
    let window_secs = recent
        .last()
        .map(|oldest| (reference_ts_ms as f64 / 1000.0) - oldest.hour as f64)
        .unwrap_or(0.0);
    (count, window_secs)
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
            date: format_chart_date(&format!("{date:08}"))?,
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
    let cache_key = "chart:most-utilized-scripts:v4";
    if let Some(cached) = state
        .cache
        .get::<MostUtilizedScriptsChartResponse>(cache_key)
        .await
    {
        return ok(cached);
    }

    // Entities follow the same family resolution the usage counters use: every
    // observed reference form resolving into a family version is grouped under
    // that family, so the chart totals equal /scripts/{name}/usage. Loose
    // reference forms without a family keep their own label.
    let script_infos_by_code_hash: HashMap<Vec<u8>, ckbadger_store::ScriptInfo> =
        load_script_infos_cached(&state)?.into_iter().collect();
    let version_families: HashMap<Vec<u8>, String> =
        super::scripts::load_script_versions_cached(&state)?
            .into_iter()
            .filter_map(|(version_hash, info)| Some((version_hash, info.family_id?)))
            .collect();
    let family_names: HashMap<String, String> =
        super::scripts::load_script_families_cached(&state)?
            .into_iter()
            .map(|(family_id, info)| (family_id, info.name))
            .collect();

    let mut labels_by_key: HashMap<String, String> = HashMap::new();
    let mut final_by_key: HashMap<String, (i128, i128)> = HashMap::new();
    let mut entity_key_by_form: HashMap<(Vec<u8>, u8), String> = HashMap::new();
    let mut code_hashes: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();

    for ((reference_hash, hash_type), reference_info) in
        state
            .store
            .list_script_reference_infos()
            .map_err(|e| ApiError::internal(e.to_string()))?
    {
        let member_version = crate::utils::reference_form_member_version(
            &state.store,
            &state.append_only_store,
            hash_type,
            &reference_hash,
            &|hash: &[u8]| version_families.contains_key(hash),
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
        let family_id = member_version.and_then(|version| version_families.get(&version).cloned());

        // Aggregation keys are identities, never display names: a family bucket
        // is keyed by its family_id, a loose reference form by (code_hash,
        // hash_type). Buckets that merely share a label stay separate -- the
        // junk secp data form inherits the type form's ScriptInfo label
        // (ScriptInfo is keyed by code_hash alone) and two unrelated
        // deployments may carry the same name, yet none of them are the same
        // script. Labels ride along for display only, and loose forms carry
        // their form so same-named identities stay distinguishable.
        let (key, label) = match family_id {
            Some(family_id) => {
                let name = family_names.get(&family_id).ok_or_else(|| {
                    ApiError::internal(format!(
                        "script version points to missing family in most-utilized chart: family_id={}",
                        family_id
                    ))
                })?;
                (format!("family:{family_id}"), name.clone())
            }
            None => {
                let code_hash_hex = format!("0x{}", hex::encode(&reference_hash));
                let hash_type_name = hash_type_to_string(hash_type).ok_or_else(|| {
                    ApiError::internal(format!(
                        "script reference form has an unknown hash_type in most-utilized chart: reference_hash={}, hash_type={}",
                        code_hash_hex, hash_type
                    ))
                })?;
                let script_name = script_infos_by_code_hash
                    .get(&reference_hash)
                    .and_then(|info| info.name.as_deref())
                    .unwrap_or("Unknown")
                    .trim()
                    .to_string();
                let display = if is_known_script_name(&script_name) {
                    script_name
                } else {
                    code_hash_hex.clone()
                };
                (
                    format!("ref:{code_hash_hex}:{hash_type_name}"),
                    format!("{display} ({hash_type_name})"),
                )
            }
        };

        let final_total_cells_capacity =
            reference_info.lock_owned_capacity_sum + reference_info.type_owned_capacity_sum;
        let final_used_capacity =
            reference_info.lock_owned_knowledge_sum + reference_info.type_owned_knowledge_sum;
        if final_total_cells_capacity < 0 {
            return Err(ApiError::internal(format!(
                "negative script total capacity for key {}: reference_hash=0x{}, hash_type={}, value={}",
                key,
                hex::encode(&reference_hash),
                hash_type,
                final_total_cells_capacity
            )));
        }
        if final_used_capacity < 0 {
            return Err(ApiError::internal(format!(
                "negative script common knowledge size for key {}: reference_hash=0x{}, hash_type={}, value={}",
                key,
                hex::encode(&reference_hash),
                hash_type,
                final_used_capacity
            )));
        }
        if final_used_capacity > final_total_cells_capacity {
            return Err(ApiError::internal(format!(
                "script common knowledge size exceeds total for key {}: reference_hash=0x{}, hash_type={}, used={}, total={}",
                key,
                hex::encode(&reference_hash),
                hash_type,
                final_used_capacity,
                final_total_cells_capacity
            )));
        }

        labels_by_key.insert(key.clone(), label);
        let entry = final_by_key.entry(key.clone()).or_insert((0, 0));
        entry.0 += final_total_cells_capacity;
        entry.1 += final_used_capacity;
        entity_key_by_form.insert((reference_hash.clone(), hash_type), key);
        code_hashes.insert(reference_hash);
    }

    let mut deltas_by_date: BTreeMap<u32, Vec<(String, i128, i128)>> = BTreeMap::new();
    for code_hash in &code_hashes {
        let rows = state
            .store
            .list_script_daily_deltas_by_code_hash(code_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for ((hash_type, _is_type, date), delta) in rows {
            let key = entity_key_by_form
                .get(&(code_hash.clone(), hash_type))
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "script daily row without a matching reference form: code_hash=0x{}, hash_type={}, date={}",
                        hex::encode(code_hash),
                        hash_type,
                        date
                    ))
                })?;
            deltas_by_date.entry(date).or_default().push((
                key.clone(),
                delta.owned_capacity_delta,
                delta.owned_knowledge_delta,
            ));
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

    let token_assets = state.load_token_cache().ok_or_else(|| {
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

    let object_assets = state.load_object_cache().ok_or_else(|| {
        state.asset_cache_unavailable("object cache unavailable; warmup in progress")
    })?;
    for obj in object_assets {
        if obj.standard == "spore" {
            let cluster_id = obj.cluster_id.clone().unwrap_or_else(|| obj.id.clone());
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
            let name = obj.name.clone().unwrap_or_else(|| cluster_id.clone());
            let entity_key = format!("dob:{cluster_id}");
            labels_by_key.insert(entity_key.clone(), format_asset_label(&name, "object"));
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

        let collection_id = obj.id;
        let collection_bytes = hex::decode(
            collection_id.strip_prefix("0x").unwrap_or(&collection_id),
        )
        .map_err(|_| {
            ApiError::internal(format!(
                "invalid object collection id in warmup cache while building chart: {}",
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
        let name = obj.name.clone().unwrap_or_else(|| collection_id.clone());
        let entity_key = format!("object:{collection_id}");
        labels_by_key.insert(entity_key.clone(), format_asset_label(&name, "object"));
        if used_cap > total_cells_capacity {
            return Err(ApiError::internal(format!(
                "Object common knowledge size exceeds total for {}: used={}, total={}",
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
            Ok(ChartDataPoint {
                date: format_chart_date(&date_str)?,
                value: stats.transactions_count.to_string(),
                value2: None,
            })
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?;

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

    // The stored values are already cumulative running totals (each day carries
    // forward the previous day's totals + today's deltas).
    let data: Vec<StackedAreaDataPoint> = daily_stats
        .into_iter()
        .filter(|(_, stats)| stats.total_all_cells > 0)
        .map(|(date_str, stats)| {
            let mut values = std::collections::HashMap::new();
            values.insert("allCells".to_string(), stats.total_all_cells.to_string());
            values.insert("liveCells".to_string(), stats.total_live_cells.to_string());
            values.insert("deadCells".to_string(), stats.total_dead_cells.to_string());
            Ok(StackedAreaDataPoint {
                date: format_chart_date(&date_str)?,
                values,
            })
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?;

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

/// Common Knowledge Size (shannons) of a materialized DAO daily snapshot.
///
/// THE read-path definition of the concept, shared by `/statistics/network`,
/// `/statistics/asset-ecosystem` and `/charts/knowledge-size`: DAO header `U`
/// minus the active network's genesis-derived `virtual_occupied`
/// (CLAUDE.md "Common Knowledge", `docs/DAO_CALCULATIONS.md` §8).
///
/// `DaoDailySnapshot.occupied_capacity` is raw `U` — it still contains the
/// genesis burn cell's virtual occupied capacity (5.04B CKB on mainnet), which
/// stores no common knowledge. Reporting raw `U` as "Knowledge Size" made the
/// hero stat 32.6× the chart it links to. The persisted chart series applies
/// exactly this subtraction at write time
/// (`ckbadger_indexer::db::writer::calculate_knowledge_size`), so every surface
/// now plots one quantity.
///
/// A negative result means the snapshot's `U` or the baseline is wrong; it is
/// reported with both operands instead of being clamped to zero.
fn common_knowledge_size(
    snapshot: &ckbadger_store::DaoDailySnapshot,
    virtual_occupied: i128,
) -> Result<i128, ApiRouteError> {
    let knowledge_size = snapshot.occupied_capacity - virtual_occupied;
    if knowledge_size < 0 {
        return Err(ApiError::internal(format!(
            "negative common knowledge size on {}: occupied_capacity(U)={}, genesis virtual_occupied={}",
            snapshot.date, snapshot.occupied_capacity, virtual_occupied
        )));
    }
    Ok(knowledge_size)
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
    let genesis_burnt = state.genesis_baseline()?.burnt;
    let liquid_by_date = build_liquid_supply_by_date_map(&snapshots, genesis_burnt)?;

    // Exclude the current incomplete day to prevent cache divergence with composition chart.
    let today_key = current_ckb_date_key();
    let mut data: Vec<ChartDataPoint> = Vec::with_capacity(daily_stats.len());
    for (date_str, stats) in daily_stats {
        if date_str.as_str() == today_key {
            continue;
        }
        let Some(ks) = stats.knowledge_size else {
            continue;
        };
        let snapshot_date = format_chart_date(&date_str)?;
        let utilization = liquid_by_date
            .get(&snapshot_date)
            .map(|circulating| {
                if *circulating > 0 {
                    format!("{:.4}", ks as f64 * 100.0 / *circulating as f64)
                } else {
                    "0.0000".to_string()
                }
            })
            .unwrap_or_else(|| "0.0000".to_string());
        data.push(ChartDataPoint {
            date: snapshot_date,
            value: shannon_to_ckb_string(ks),
            value2: Some(utilization),
        });
    }

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

/// Knowledge-size composition bucket for a typed cell's `code_hash`, derived from the
/// shared network-agnostic `PROTOCOL_REGISTRY` (mainnet + testnet union). This replaces
/// the former mainnet-only `UDT_CODE_HASHES` / `NFT_SPORE_CODE_HASHES` const sets, which
/// undercounted testnet assets because they enumerated only mainnet code_hashes.
///
/// Coverage is preserved exactly (plus the testnet hashes the registry adds):
///
/// - `Dao` = registry `Dao`.
/// - `Udt` = registry `Sudt | Xudt`. The old set was sUDT + 2 xUDT canonical hashes, all
///   of which map to `Sudt`/`Xudt`; it carried NO udt-compatible (Stable++/ccBTC/USDI)
///   hashes, so none are added here.
/// - `NftSpore` = registry `SporeNft | SporeDid | Cluster`. `.bit Cell` is an independent
///   identity protocol and therefore remains in `OtherTyped`, not the Spore bucket.
/// - `OtherTyped` = every other typed cell (unchanged residual bucket).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnowledgeBucket {
    Dao,
    Udt,
    NftSpore,
    OtherTyped,
}

fn classify_knowledge_bucket(code_hash: &[u8]) -> KnowledgeBucket {
    // Single registry lookup; precedence Dao -> Udt -> NftSpore -> OtherTyped is preserved
    // by match-arm order (registry variants are mutually exclusive, so no overlap anyway).
    match PROTOCOL_REGISTRY.get(code_hash) {
        Some(ProtocolScript::Dao) => KnowledgeBucket::Dao,
        Some(ProtocolScript::Sudt | ProtocolScript::Xudt) => KnowledgeBucket::Udt,
        Some(ProtocolScript::SporeNft | ProtocolScript::SporeDid | ProtocolScript::Cluster) => {
            KnowledgeBucket::NftSpore
        }
        _ => KnowledgeBucket::OtherTyped,
    }
}

#[cfg(test)]
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

fn build_liquid_supply_by_date_map(
    snapshots: &[ckbadger_store::DaoDailySnapshot],
    genesis_burnt: i128,
) -> Result<HashMap<String, i128>, ApiRouteError> {
    let mut by_date = HashMap::with_capacity(snapshots.len());

    for snapshot in snapshots {
        let Some(supply) =
            dao_supply(snapshot, genesis_burnt).map_err(|e| ApiError::internal(e.to_string()))?
        else {
            continue;
        };
        by_date.insert(snapshot.date.clone(), supply.liquid);
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

    let mut type_daily_delta: HashMap<u32, i128> = HashMap::new();
    let mut dao_daily_delta: HashMap<u32, i128> = HashMap::new();
    let mut udt_daily_delta: HashMap<u32, i128> = HashMap::new();
    let mut nft_spore_daily_delta: HashMap<u32, i128> = HashMap::new();

    for code_hash in type_code_hashes {
        let store = state.store.clone();
        let code_hash_c = code_hash.clone();
        let deltas = tokio::task::spawn_blocking(move || {
            store.list_script_daily_deltas_by_code_hash(&code_hash_c)
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
        for ((_hash_type, is_type, date), delta) in deltas {
            if !is_type {
                continue;
            }
            let used_delta = delta.owned_knowledge_delta;
            *type_daily_delta.entry(date).or_insert(0) += used_delta;

            match classify_knowledge_bucket(&code_hash) {
                KnowledgeBucket::Dao => *dao_daily_delta.entry(date).or_insert(0) += used_delta,
                KnowledgeBucket::Udt => *udt_daily_delta.entry(date).or_insert(0) += used_delta,
                KnowledgeBucket::NftSpore => {
                    *nft_spore_daily_delta.entry(date).or_insert(0) += used_delta
                }
                KnowledgeBucket::OtherTyped => {}
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
            &format!("accumulating object/spore composition for date {}", date),
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
            date: format_chart_date(&format!("{date:08}"))?,
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
                label: "Object (Spore)".to_string(),
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
        let live = match stats.knowledge_size {
            Some(v) => v,
            None => continue,
        };
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
            date: format_chart_date(&date)?,
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

/// Number of complete epochs to include in the block-time distribution.
/// ~1 week at ~4 h/epoch. CKB adjusts difficulty per epoch, so aligning
/// on epoch boundaries gives a clean view of network behaviour.
const BLOCK_TIME_DIST_EPOCHS: i64 = 42;
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

/// Try to resolve a precise block range from EpochStats.
/// Returns `Some((end_block, fetch_count))` on success.
fn epoch_range_to_block_range(
    store: &ckbadger_store::CkbadgerStore,
    first_epoch: i64,
    last_epoch: i64,
) -> Option<(i64, usize)> {
    let first = store.get_epoch_stats(first_epoch).ok()??;
    let last = store.get_epoch_stats(last_epoch).ok()??;
    let end_block = last.end_block?;
    // One extra predecessor for the first time delta
    let start_block = (first.start_block - 1).max(0);
    Some((end_block, (end_block - start_block + 1) as usize))
}

pub(crate) fn build_block_time_distribution_response(
    store: &ckbadger_store::CkbadgerStore,
) -> Result<ChartResponse, String> {
    let (_, tip_header) = store
        .get_sync_tip_block()
        .map_err(|e| e.to_string())?
        .ok_or("no synced blocks")?;

    if tip_header.epoch_number < 1 {
        return Ok(ChartResponse {
            data: build_block_time_distribution_data(&vec![0u64; BLOCK_TIME_DIST_BUCKET_COUNT], 0),
            title: "Block Time Distribution (Last 0 Epochs)".to_string(),
            y_axis_label: "Block Ratio (%)".to_string(),
            y2_axis_label: None,
        });
    }

    // Last N complete epochs — current epoch may be incomplete
    let last_epoch = tip_header.epoch_number - 1;
    let first_epoch = (last_epoch - BLOCK_TIME_DIST_EPOCHS + 1).max(0);
    let actual_epochs = last_epoch - first_epoch + 1;

    // Precise range via EpochStats (two point lookups); fall back to scan from tip
    let mut headers = match epoch_range_to_block_range(store, first_epoch, last_epoch) {
        Some((end_block, count)) => store
            .list_blocks_desc(Some(end_block), count)
            .map_err(|e| e.to_string())?,
        None => {
            let estimate = (actual_epochs as usize + 1) * 2000 + 100;
            store
                .list_blocks_desc(None, estimate)
                .map_err(|e| e.to_string())?
        }
    };
    headers.reverse();

    let mut bucket_counts = vec![0u64; BLOCK_TIME_DIST_BUCKET_COUNT];
    let mut total_blocks = 0u64;

    if headers.len() >= 2 {
        for window in headers.windows(2) {
            let (prev_number, prev_header) = &window[0];
            let (curr_number, curr_header) = &window[1];
            if *curr_number != *prev_number + 1 {
                continue;
            }
            // Only count blocks within the epoch range
            if curr_header.epoch_number < first_epoch || curr_header.epoch_number > last_epoch {
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
        title: format!("Block Time Distribution (Last {} Epochs)", actual_epochs),
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

    let mut data: Vec<ChartDataPoint> = Vec::with_capacity(daily_stats.len());
    for (date_str, stats) in daily_stats {
        let Some(avg_time_ms) = stats.avg_block_time_ms() else {
            continue;
        };
        data.push(ChartDataPoint {
            date: format_chart_date(&date_str)?,
            value: format!("{:.2}", avg_time_ms as f64 / 1000.0),
            value2: None,
        });
    }

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

    let (
        latest_block,
        epoch_number,
        epoch_index,
        epoch_length,
        latest_timestamp,
        tip_compact_target,
    ) = match latest {
        Some((block_num, header)) => {
            let ts = DateTime::from_timestamp_millis(header.timestamp).unwrap_or_else(Utc::now);
            (
                block_num,
                header.epoch_number,
                header.epoch_index,
                header.epoch_length,
                ts,
                header.compact_target,
            )
        }
        None => (0i64, 0i64, 0i32, 1800i32, Utc::now(), 0u32),
    };

    // Recent-window average block time with millisecond precision.
    // NETWORK_STATS_BLOCK_TIME_WINDOW gaps (~100 minutes of chain time) is
    // recent enough to track the live network yet wide enough to smooth
    // single-interval noise. `None` until the store holds at least 2 blocks.
    let window_headers = store
        .list_blocks_desc(Some(latest_block), NETWORK_STATS_BLOCK_TIME_WINDOW + 1)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let avg_block_time_secs: Option<f64> = if window_headers.len() >= 2 {
        let newest_ms = window_headers.first().expect("len >= 2").1.timestamp;
        let oldest_ms = window_headers.last().expect("len >= 2").1.timestamp;
        let gaps = (window_headers.len() - 1) as f64;
        let span_ms = newest_ms - oldest_ms;
        if span_ms <= 0 {
            return Err(ApiError::internal(format!(
                "non-increasing block timestamps across avg-block-time window: tip_block={}, newest_ts_ms={}, oldest_ts_ms={}",
                latest_block, newest_ms, oldest_ms
            )));
        }
        Some(span_ms as f64 / gaps / 1000.0)
    } else {
        None
    };

    // Rolling last-24-hours committed transaction count, from the same exact
    // hourly buckets the tx-stats endpoint reports (window edges quantized to
    // bucket starts). Replaces the old today+yesterday calendar-day sum that
    // labeled up to 48 hours of transactions as "per day".
    let hourly_stats = store
        .list_hourly_stats_with_keys()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let (tx_count_24h, tx_window_secs) =
        rolling_24h_tx_window(&hourly_stats, latest_timestamp.timestamp_millis());

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

    // Current-epoch PoW difficulty from the tip header's compact_target —
    // node/explorer semantics, NOT a daily average. Zero only while the store
    // is empty (no tip header yet).
    let difficulty: u64 = if tip_compact_target == 0 {
        0
    } else {
        let difficulty_u256 = ckb_compact_to_difficulty(tip_compact_target);
        difficulty_u256.to_string().parse().map_err(|_| {
            ApiError::internal(format!(
                "difficulty exceeds u64 range: tip_block={}, compact_target={:#x}, difficulty={}",
                latest_block, tip_compact_target, difficulty_u256
            ))
        })?
    };

    let remaining_blocks = epoch_length - epoch_index;
    let estimated_epoch_seconds =
        (remaining_blocks as f64 * avg_block_time_secs.unwrap_or(0.0)) as i64;

    let tps = if tx_window_secs > 0.0 {
        tx_count_24h as f64 / tx_window_secs
    } else {
        0.0
    };
    let tx_per_minute = tps * 60.0;
    // Normalized over the same rolling window as tps/perMinute — the three
    // fields are one rate at three scales and must agree. The raw bucket sum
    // previously served here covers a ~23h quantized window (and less while
    // the window is still filling after a rebuild), contradicting perMinute.
    let tx_per_day = tps * 86400.0;

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

    // Network hash rate from the actual work in the recent window — NOT
    // tip_difficulty / avg_block_time, which overstates by the difficulty
    // step ratio for the first ~window blocks after every epoch boundary.
    // The displayed `difficulty` field below stays tip-epoch difficulty
    // (that semantic is correct). Zero only while the store holds < 2 blocks.
    let hash_rate = estimate_hash_rate_from_window(&window_headers)?.unwrap_or(0.0);

    // Hero metrics from latest DAO daily snapshot
    let dao_snapshot = store
        .get_latest_dao_daily_snapshot()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let knowledge_size = match dao_snapshot.as_ref() {
        Some(s) => {
            let virtual_occupied = state.genesis_baseline()?.virtual_occupied;
            Some(common_knowledge_size(s, virtual_occupied)?.to_string())
        }
        None => None,
    };
    let circulating_supply = match dao_snapshot.as_ref() {
        Some(s) => {
            let genesis_burnt = state.genesis_baseline()?.burnt;
            dao_supply(s, genesis_burnt)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .map(|supply| supply.circulating.to_string())
        }
        None => None,
    };
    let dao_locked = dao_snapshot.as_ref().map(|s| s.total_deposited.to_string());

    Ok(NetworkStats {
        latest_block,
        // "0.00s" only while the store holds fewer than 2 blocks.
        avg_block_time: format!("{:.2}s", avg_block_time_secs.unwrap_or(0.0)),
        hash_rate: format_hash_rate(hash_rate),
        difficulty: format_difficulty(difficulty),
        epoch: format!("{}({}/{})", epoch_number, epoch_index, epoch_length),
        tps: format!("{:.2}", tps),
        estimated_epoch_time: format_duration(estimated_epoch_seconds as u64),
        transactions_per_minute: format!("{:.1}", tx_per_minute),
        transactions_per_day: format!("{:.0}", tx_per_day),
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
    // The day's mined span comes from `DailyStats.block_time_sum_ms`, the single
    // place inter-block time is stored; `DailyBlockStats` carries the difficulty
    // and block count that form the numerator.
    let (daily_block_stats, daily_stats) = tokio::task::spawn_blocking(move || {
        let block_stats = store.list_daily_block_stats()?;
        let stats = store.list_daily_stats_with_dates()?;
        anyhow::Ok((block_stats, stats))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let block_time_sum_by_date: HashMap<String, i64> = daily_stats
        .into_iter()
        .map(|(date, stats)| (date, stats.block_time_sum_ms))
        .collect();

    // Exclude the last day (incomplete) like the SQL version did
    let max_date = daily_block_stats
        .iter()
        .map(|(d, _)| d.as_str())
        .max()
        .map(|s| s.to_string());

    let mut data: Vec<ChartDataPoint> = Vec::with_capacity(daily_block_stats.len());
    for (date_str, stats) in daily_block_stats {
        if stats.avg_difficulty <= 0.0
            || max_date
                .as_ref()
                .is_some_and(|m| date_str.as_str() >= m.as_str())
        {
            continue;
        }
        let block_time_sum_ms = *block_time_sum_by_date.get(&date_str).ok_or_else(|| {
            ApiError::internal(format!(
                "daily block stats for {date_str} have no matching daily stats row; \
                 both are written in the same batch, so this is upstream corruption"
            ))
        })?;
        let hash_rate = calculate_daily_hash_rate(&date_str, &stats, block_time_sum_ms)?;
        data.push(ChartDataPoint {
            date: format_chart_date(&date_str)?,
            value: format!("{:.0}", hash_rate),
            value2: None,
        });
    }

    let response = ChartResponse {
        data,
        title: "Hash Rate".to_string(),
        y_axis_label: "Hash Rate".to_string(),
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
            stats.avg_difficulty > 0.0
                && max_date.as_ref().is_none_or(|m| date.as_str() < m.as_str())
        })
        .map(|(date_str, stats)| {
            Ok(ChartDataPoint {
                date: format_chart_date(&date_str)?,
                value: format!("{:.0}", stats.avg_difficulty),
                value2: None,
            })
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?;

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
            Ok(ChartDataPoint {
                date: format_chart_date(&date_str)?,
                value: format!("{:.6}", uncle_rate),
                value2: None,
            })
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?;

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
    pub miner_lock_hash: String,
    pub address: Option<String>,
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
    pub window_days: i64,
    pub from_date: String,
    pub to_date: String,
}

const MINER_DISTRIBUTION_WINDOW_DAYS: i64 = 7;

fn format_exact_percentage_4(
    numerator: i128,
    denominator: i128,
    context: &str,
) -> Result<String, ApiRouteError> {
    if numerator < 0 || denominator <= 0 {
        return Err(ApiError::internal(format!(
            "invalid percentage operands for {}: numerator={}, denominator={}",
            context, numerator, denominator
        )));
    }
    let scaled = numerator.checked_mul(1_000_000).ok_or_else(|| {
        ApiError::internal(format!(
            "percentage scaling overflow for {}: numerator={}",
            context, numerator
        ))
    })? / denominator;
    Ok(format!("{}.{:04}", scaled / 10_000, scaled % 10_000))
}

fn build_miner_distribution_response(
    miner_stats: Vec<ckbadger_store::MinerStats>,
    addresses: HashMap<Vec<u8>, String>,
    from_date: chrono::NaiveDate,
    to_date: chrono::NaiveDate,
) -> Result<MinerDistributionResponse, ApiRouteError> {
    let mut aggregated: HashMap<Vec<u8>, i64> = HashMap::new();
    for stats in miner_stats {
        if stats.miner_lock_hash.len() != 32 {
            return Err(ApiError::internal(format!(
                "invalid miner lock hash length in miner stats: hash=0x{}, len={}",
                hex::encode(&stats.miner_lock_hash),
                stats.miner_lock_hash.len()
            )));
        }
        if stats.blocks_count <= 0 {
            return Err(ApiError::internal(format!(
                "non-positive miner block count in miner stats: hash=0x{}, blocks_count={}",
                hex::encode(&stats.miner_lock_hash),
                stats.blocks_count
            )));
        }
        let current = aggregated.entry(stats.miner_lock_hash.clone()).or_default();
        *current = current
            .checked_add(i64::from(stats.blocks_count))
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "miner block count overflow: hash=0x{}, current={}, delta={}",
                    hex::encode(&stats.miner_lock_hash),
                    current,
                    stats.blocks_count
                ))
            })?;
    }

    let total_blocks = aggregated.values().try_fold(0_i64, |total, blocks| {
        total.checked_add(*blocks).ok_or_else(|| {
            ApiError::internal(format!(
                "total miner block count overflow: current={}, delta={}",
                total, blocks
            ))
        })
    })?;

    let mut sorted: Vec<(Vec<u8>, i64)> = aggregated.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    sorted.truncate(100);

    let data = if total_blocks == 0 {
        Vec::new()
    } else {
        sorted
            .into_iter()
            .map(|(hash, blocks_mined)| {
                Ok(MinerDistributionDataPoint {
                    miner_lock_hash: format!("0x{}", hex::encode(&hash)),
                    address: addresses.get(&hash).cloned(),
                    miner_name: None,
                    blocks_mined,
                    percentage: format_exact_percentage_4(
                        i128::from(blocks_mined),
                        i128::from(total_blocks),
                        &format!("miner 0x{}", hex::encode(&hash)),
                    )?,
                })
            })
            .collect::<Result<Vec<_>, ApiRouteError>>()?
    };

    Ok(MinerDistributionResponse {
        data,
        title: format!(
            "Miner Distribution (Last {} Complete Days, UTC+8)",
            MINER_DISTRIBUTION_WINDOW_DAYS
        ),
        total_blocks,
        window_days: MINER_DISTRIBUTION_WINDOW_DAYS,
        from_date: from_date.format("%Y-%m-%d").to_string(),
        to_date: to_date.format("%Y-%m-%d").to_string(),
    })
}

async fn get_miner_address_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<MinerDistributionResponse> {
    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET)
        .ok_or_else(|| ApiError::internal("invalid CKB UTC+8 offset"))?;
    let to_date = Utc::now().with_timezone(&utc8).date_naive() - chrono::Duration::days(1);
    let from_date = to_date - chrono::Duration::days(MINER_DISTRIBUTION_WINDOW_DAYS - 1);
    let from_key = from_date.format("%Y%m%d").to_string();
    let to_key = to_date.format("%Y%m%d").to_string();
    let cache_key = format!(
        "chart:miner-address-distribution:v2:{}:{}",
        MINER_DISTRIBUTION_WINDOW_DAYS, to_key
    );
    if let Some(cached) = state
        .cache
        .get::<MinerDistributionResponse>(&cache_key)
        .await
    {
        return ok(cached);
    }

    let store = state.store.clone();
    let network = state.ckb_network.clone();
    let (miner_stats, addresses) = tokio::task::spawn_blocking(move || {
        let miner_stats = store.list_miner_stats_in_date_range(&from_key, &to_key)?;
        let mut addresses = HashMap::new();
        for stats in &miner_stats {
            if addresses.contains_key(&stats.miner_lock_hash) {
                continue;
            }
            let Some(script) = store.get_lock_script(&stats.miner_lock_hash)? else {
                continue;
            };
            let address =
                script_to_address(&script.code_hash, script.hash_type, &script.args, &network)
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "failed to encode miner address: lock_hash=0x{}, network={}, error={}",
                            hex::encode(&stats.miner_lock_hash),
                            network,
                            error
                        )
                    })?;
            addresses.insert(stats.miner_lock_hash.clone(), address);
        }
        anyhow::Ok((miner_stats, addresses))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let response = build_miner_distribution_response(miner_stats, addresses, from_date, to_date)?;

    state
        .cache
        .set(&cache_key, &response, CacheTtl::CHART)
        .await;

    ok(response)
}

/// Exact daily network hash rate in hashes per millisecond: the day's total
/// mined work divided by the time actually spent mining it.
///
/// * numerator — `avg_difficulty × block_count` is the day's exact difficulty
///   sum (the indexer stores the sum divided by the count).
/// * denominator — `DailyStats.block_time_sum_ms` sums every inter-block gap
///   `ts(b) − ts(b−1)` for the blocks `b` dated to this day, including the gap
///   across midnight from the previous day's last block, so it telescopes to
///   `ts(last block of day) − ts(last block of previous day)`: the day's real
///   mined span. Only block 0 contributes no gap, which is why mainnet's
///   genesis day spans 67_712_964 ms (the chain started 05:09:50 UTC+8) and
///   not 86_400_000.
///
/// Assuming a full calendar day understated the genesis day by 21.6% and every
/// later day by the difference between its real span and 86_400_000 ms.
fn calculate_daily_hash_rate(
    date_key: &str,
    block_stats: &ckbadger_store::DailyBlockStats,
    block_time_sum_ms: i64,
) -> Result<f64, ApiRouteError> {
    if block_stats.block_count <= 0 {
        return Err(ApiError::internal(format!(
            "daily block stats for {date_key} have avg_difficulty={} but block_count={}",
            block_stats.avg_difficulty, block_stats.block_count
        )));
    }
    if block_time_sum_ms <= 0 {
        return Err(ApiError::internal(format!(
            "cannot derive hash rate for {date_key}: block_time_sum_ms={block_time_sum_ms} with block_count={}. \
             The day's mined span is the only valid divisor, and every block except the genesis block \
             contributes a gap to it",
            block_stats.block_count
        )));
    }
    let difficulty_sum = block_stats.avg_difficulty * block_stats.block_count as f64;
    Ok(difficulty_sum / block_time_sum_ms as f64)
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

    let genesis_burnt = state.genesis_baseline()?.burnt;
    let mut data = Vec::with_capacity(snapshots.len());
    for snapshot in &snapshots {
        let Some(supply) =
            dao_supply(snapshot, genesis_burnt).map_err(|e| ApiError::internal(e.to_string()))?
        else {
            return Err(ApiError::internal(format!(
                "missing total_issuance in dao_daily_snapshots for {}. delete RocksDB and re-sync from genesis",
                snapshot.date
            )));
        };

        let mut values = std::collections::HashMap::new();
        values.insert(
            "circulating".to_string(),
            (supply.liquid / SHANNONS_PER_CKB).to_string(),
        );
        values.insert(
            "nervosdao".to_string(),
            (supply.dao_locked / SHANNONS_PER_CKB).to_string(),
        );
        values.insert(
            "burnt".to_string(),
            (supply.burnt / SHANNONS_PER_CKB).to_string(),
        );
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

    // Short TTL: includes the current incomplete day whose cumulative values
    // change every block.  Must stay fresh enough to match dao/statistics
    // (verified by S16 with a 10K CKB tolerance).
    state
        .cache
        .set(cache_key, &response, CacheTtl::ADDRESS_BALANCE)
        .await;

    ok(response)
}

async fn get_nominal_apc_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    // Derived genesis circulating supply in CKB (25.2B on mainnet), from the
    // persisted per-network GenesisBaseline — not a hardcoded mainnet literal.
    let baseline = state.genesis_baseline()?;
    let genesis_supply_ckb =
        (baseline.total_issuance - baseline.burnt) as f64 / SHANNONS_PER_CKB as f64;
    let data: Vec<ChartDataPoint> = (0..=80)
        .map(|i| {
            let year = i as f64 * 0.25;
            let apc = calculate_nominal_apc(year, genesis_supply_ckb);
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

fn calculate_nominal_apc(year: f64, genesis_supply_ckb: f64) -> f64 {
    // Secondary issuance is a fixed protocol invariant (1.344B CKB/year on every
    // network), so it stays a literal; only the genesis circulating base differs
    // per network and is passed in as `genesis_supply_ckb`.
    const SECONDARY_ISSUANCE_PER_YEAR_CKB: f64 = 1_344_000_000.0;

    let halving_count = (year / 4.0).floor() as u32;

    let mut total_primary_issued = 0.0;
    for h in 0..halving_count {
        let rate = 4_200_000_000.0 / 2.0_f64.powi(h as i32);
        total_primary_issued += rate * 4.0;
    }

    let years_in_current_era = year - (halving_count as f64 * 4.0);
    let current_era_rate = 4_200_000_000.0 / 2.0_f64.powi(halving_count as i32);
    total_primary_issued += current_era_rate * years_in_current_era;

    let total_secondary_issued = SECONDARY_ISSUANCE_PER_YEAR_CKB * year;
    let total_supply = genesis_supply_ckb + total_primary_issued + total_secondary_issued;

    (SECONDARY_ISSUANCE_PER_YEAR_CKB / total_supply) * 100.0
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

    let mut data = Vec::new();
    for snapshot in &snapshots {
        let (cum_miner, cum_dao, _) = snapshot_secondary_cumulative(snapshot)?;
        let treasury = dao_treasury(snapshot).map_err(|e| ApiError::internal(e.to_string()))?;
        if cum_miner <= 0 && cum_dao <= 0 && treasury <= 0 {
            continue;
        }

        let mut values = std::collections::HashMap::new();
        values.insert(
            "compensation".to_string(),
            (cum_dao / SHANNONS_PER_CKB).to_string(),
        );
        values.insert(
            "mining".to_string(),
            (cum_miner / SHANNONS_PER_CKB).to_string(),
        );
        values.insert(
            "burnt".to_string(),
            (treasury / SHANNONS_PER_CKB).to_string(),
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

    // Short TTL: includes the current incomplete day whose cumulative values
    // change every block.  Must stay fresh enough to match dao/statistics
    // (verified by S16 with a 10K CKB tolerance).
    state
        .cache
        .set(cache_key, &response, CacheTtl::ADDRESS_BALANCE)
        .await;

    ok(response)
}

const INFLATION_TRAILING_DAYS: i64 = 365;

fn inflation_rate_response(data: Vec<ChartDataPoint>) -> ChartResponse {
    ChartResponse {
        data,
        title: format!(
            "Realized Inflation Rate (Trailing {} Complete Days)",
            INFLATION_TRAILING_DAYS
        ),
        y_axis_label: "Nominal Inflation (%)".to_string(),
        y2_axis_label: Some("Real Inflation (%)".to_string()),
    }
}

fn snapshot_total_secondary_issuance(
    snapshot: &ckbadger_store::DaoDailySnapshot,
) -> Result<i128, ApiRouteError> {
    if snapshot.cum_miner_secondary < 0 {
        return Err(ApiError::internal(format!(
            "negative cum_miner_secondary in dao_daily_snapshots for {}: {}",
            snapshot.date, snapshot.cum_miner_secondary
        )));
    }
    if snapshot.secondary_pool < 0 {
        return Err(ApiError::internal(format!(
            "negative secondary_pool in dao_daily_snapshots for {}: {}",
            snapshot.date, snapshot.secondary_pool
        )));
    }
    if snapshot.compensation < 0 {
        return Err(ApiError::internal(format!(
            "negative cumulative claimed compensation in dao_daily_snapshots for {}: {}",
            snapshot.date, snapshot.compensation
        )));
    }

    // RFC-0023's S field contains non-miner secondary issuance that has not
    // yet been claimed. Add cumulative claimed compensation back to S, then
    // add the independently materialized miner share. `cum_dao_compensation`
    // and `cum_treasury` cannot be summed here because both include frozen
    // phase-1 compensation.
    snapshot
        .cum_miner_secondary
        .checked_add(snapshot.secondary_pool)
        .and_then(|value| value.checked_add(snapshot.compensation))
        .ok_or_else(|| {
            ApiError::internal(format!(
                "cumulative secondary issuance overflow in dao_daily_snapshots for {}: miner={}, secondary_pool={}, claimed={}",
                snapshot.date,
                snapshot.cum_miner_secondary,
                snapshot.secondary_pool,
                snapshot.compensation
            ))
        })
}

fn certify_inflation_snapshot_gap_is_blockless(
    store: &ckbadger_store::CkbadgerStore,
    first_missing_date: chrono::NaiveDate,
    next_snapshot_date: chrono::NaiveDate,
) -> Result<(), ApiRouteError> {
    if first_missing_date >= next_snapshot_date {
        return Err(ApiError::internal(format!(
            "invalid DAO snapshot gap bounds: first_missing_date={}, next_snapshot_date={}",
            first_missing_date, next_snapshot_date
        )));
    }

    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET)
        .ok_or_else(|| ApiError::internal("invalid CKB UTC+8 offset"))?;
    let day_start = first_missing_date.and_hms_opt(0, 0, 0).ok_or_else(|| {
        ApiError::internal(format!(
            "invalid start of missing DAO snapshot date {}",
            first_missing_date
        ))
    })?;
    let day_start_ms = utc8
        .from_local_datetime(&day_start)
        .single()
        .ok_or_else(|| {
            ApiError::internal(format!(
                "ambiguous start of missing DAO snapshot date {}",
                first_missing_date
            ))
        })?
        .timestamp_millis();

    let first_block = store
        .find_first_block_at_or_after_ms(day_start_ms)
        .map_err(|error| {
            ApiError::internal(format!(
                "failed to locate canonical block coverage for DAO snapshot gap starting {}: {}",
                first_missing_date, error
            ))
        })?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "DAO snapshot exists for {} but no canonical block exists at or after preceding gap start {}",
                next_snapshot_date, first_missing_date
            ))
        })?;
    let first_header = store
        .get_block_header(first_block)
        .map_err(|error| {
            ApiError::internal(format!(
                "failed to read canonical block {} while validating DAO snapshot gap starting {}: {}",
                first_block, first_missing_date, error
            ))
        })?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "canonical block header disappeared while validating DAO snapshot gap: block={}, first_missing_date={}",
                first_block, first_missing_date
            ))
        })?;
    let first_block_date = ckbadger_common::block_date_from_ms(first_header.timestamp);

    if first_block_date < next_snapshot_date {
        return Err(ApiError::internal(format!(
            "missing complete-day DAO snapshot for block-bearing date: missing_date={}, first_block={}, next_snapshot_date={}",
            first_block_date, first_block, next_snapshot_date
        )));
    }
    if first_block_date > next_snapshot_date {
        return Err(ApiError::internal(format!(
            "DAO snapshot has no canonical block-date coverage: snapshot_date={}, first_block_at_or_after_gap={}, first_block_date={}",
            next_snapshot_date, first_block, first_block_date
        )));
    }

    Ok(())
}

fn build_inflation_rate_response<F>(
    snapshots: &[ckbadger_store::DaoDailySnapshot],
    incomplete_tip_date: Option<chrono::NaiveDate>,
    mut certify_blockless_gap: F,
) -> Result<ChartResponse, ApiRouteError>
where
    F: FnMut(chrono::NaiveDate, chrono::NaiveDate) -> Result<(), ApiRouteError>,
{
    if snapshots.is_empty() {
        return Ok(inflation_rate_response(Vec::new()));
    }
    let incomplete_tip_date = incomplete_tip_date.ok_or_else(|| {
        ApiError::internal(
            "DAO daily snapshots exist without a sync-tip block; cannot identify incomplete day",
        )
    })?;

    let mut observed_by_date: BTreeMap<chrono::NaiveDate, &ckbadger_store::DaoDailySnapshot> =
        BTreeMap::new();
    let mut saw_tip_date = false;
    for snapshot in snapshots {
        let date =
            chrono::NaiveDate::parse_from_str(&snapshot.date, "%Y-%m-%d").map_err(|error| {
                ApiError::internal(format!(
                    "invalid dao_daily_snapshots date '{}': {}",
                    snapshot.date, error
                ))
            })?;
        if date.format("%Y-%m-%d").to_string() != snapshot.date {
            return Err(ApiError::internal(format!(
                "non-canonical dao_daily_snapshots date '{}': expected YYYY-MM-DD",
                snapshot.date
            )));
        }
        if date > incomplete_tip_date {
            return Err(ApiError::internal(format!(
                "DAO daily snapshot is after the sync-tip date: snapshot_date={}, tip_date={}",
                snapshot.date, incomplete_tip_date
            )));
        }
        if date == incomplete_tip_date {
            saw_tip_date = true;
        }
        if observed_by_date.insert(date, snapshot).is_some() {
            return Err(ApiError::internal(format!(
                "duplicate dao_daily_snapshots date while building inflation chart: {}",
                snapshot.date
            )));
        }
    }
    if !saw_tip_date {
        return Err(ApiError::internal(format!(
            "missing incomplete tip-day DAO snapshot: tip_date={}",
            incomplete_tip_date
        )));
    }

    let mut by_date: BTreeMap<chrono::NaiveDate, ckbadger_store::DaoDailySnapshot> =
        BTreeMap::new();
    let mut previous_observed: Option<(chrono::NaiveDate, &ckbadger_store::DaoDailySnapshot)> =
        None;
    for (&date, &snapshot) in &observed_by_date {
        if let Some((previous_date, previous_snapshot)) = previous_observed {
            let first_missing_date = previous_date + chrono::Duration::days(1);
            if first_missing_date < date {
                certify_blockless_gap(first_missing_date, date)?;

                let mut missing_date = first_missing_date;
                while missing_date < date {
                    let mut carried = previous_snapshot.clone();
                    carried.date = missing_date.format("%Y-%m-%d").to_string();
                    if by_date.insert(missing_date, carried).is_some() {
                        return Err(ApiError::internal(format!(
                            "duplicate carried DAO snapshot date while building inflation chart: {}",
                            missing_date
                        )));
                    }
                    missing_date += chrono::Duration::days(1);
                }
            }
        }

        by_date.insert(date, snapshot.clone());
        previous_observed = Some((date, snapshot));
    }
    by_date.remove(&incomplete_tip_date);

    let Some(first_date) = by_date.keys().next().copied() else {
        return Ok(inflation_rate_response(Vec::new()));
    };

    let mut data = Vec::new();
    for (&date, current) in &by_date {
        if date.signed_duration_since(first_date).num_days() < INFLATION_TRAILING_DAYS {
            continue;
        }
        let previous_date = date - chrono::Duration::days(INFLATION_TRAILING_DAYS);
        let previous = by_date.get(&previous_date).ok_or_else(|| {
            ApiError::internal(format!(
                "missing trailing-year DAO snapshot for inflation chart: date={}, required_previous_date={}",
                date, previous_date
            ))
        })?;
        if previous.total_issuance <= 0 {
            return Err(ApiError::internal(format!(
                "non-positive total_issuance in dao_daily_snapshots for inflation base date {}: {}. delete RocksDB and re-sync from genesis",
                previous.date, previous.total_issuance
            )));
        }
        if current.total_issuance < previous.total_issuance {
            return Err(ApiError::internal(format!(
                "total_issuance decreased across inflation window: from_date={}, from={}, to_date={}, to={}",
                previous.date,
                previous.total_issuance,
                current.date,
                current.total_issuance
            )));
        }

        let nominal_issuance = current
            .total_issuance
            .checked_sub(previous.total_issuance)
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "total_issuance subtraction overflow across inflation window: from_date={}, from={}, to_date={}, to={}",
                    previous.date,
                    previous.total_issuance,
                    current.date,
                    current.total_issuance
                ))
        })?;
        let previous_secondary = snapshot_total_secondary_issuance(previous)?;
        let current_secondary = snapshot_total_secondary_issuance(current)?;
        if current_secondary < previous_secondary {
            return Err(ApiError::internal(format!(
                "cumulative secondary issuance decreased across inflation window: from_date={}, from={}, to_date={}, to={}",
                previous.date, previous_secondary, current.date, current_secondary
            )));
        }
        let secondary_issuance =
            current_secondary.checked_sub(previous_secondary).ok_or_else(|| {
                ApiError::internal(format!(
                    "cumulative secondary issuance subtraction overflow across inflation window: from_date={}, from={}, to_date={}, to={}",
                    previous.date, previous_secondary, current.date, current_secondary
                ))
            })?;
        if secondary_issuance > nominal_issuance {
            return Err(ApiError::internal(format!(
                "secondary issuance exceeds total issuance growth across inflation window: from_date={}, to_date={}, total_delta={}, secondary_delta={}",
                previous.date, current.date, nominal_issuance, secondary_issuance
            )));
        }
        let primary_issuance = nominal_issuance
            .checked_sub(secondary_issuance)
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "primary issuance subtraction overflow across inflation window: from_date={}, to_date={}, total_delta={}, secondary_delta={}",
                    previous.date, current.date, nominal_issuance, secondary_issuance
                ))
            })?;

        data.push(ChartDataPoint {
            date: current.date.clone(),
            value: format_exact_percentage_4(
                nominal_issuance,
                previous.total_issuance,
                &format!("nominal inflation ending {}", current.date),
            )?,
            value2: Some(format_exact_percentage_4(
                primary_issuance,
                previous.total_issuance,
                &format!("real inflation ending {}", current.date),
            )?),
        });
    }

    Ok(inflation_rate_response(data))
}

async fn get_inflation_rate_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET)
        .ok_or_else(|| ApiError::internal("invalid CKB UTC+8 offset"))?;

    let store = state.store.clone();
    let tip_timestamp = tokio::task::spawn_blocking(move || {
        Ok::<_, anyhow::Error>(
            store
                .get_sync_tip_block()?
                .map(|(_, header)| header.timestamp),
        )
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let incomplete_tip_date = tip_timestamp
        .map(|timestamp| {
            DateTime::from_timestamp_millis(timestamp)
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "invalid sync-tip block timestamp while building inflation chart: {}",
                        timestamp
                    ))
                })
                .map(|date_time| date_time.with_timezone(&utc8).date_naive())
        })
        .transpose()?;
    let cache_key = format!(
        "chart:inflation-rate:v2:{}",
        incomplete_tip_date
            .map(|date| date.format("%Y%m%d").to_string())
            .unwrap_or_else(|| "empty".to_string())
    );
    if let Some(cached) = state.cache.get::<ChartResponse>(&cache_key).await {
        return ok(cached);
    }

    let store = state.store.clone();
    let response = tokio::task::spawn_blocking(move || {
        let snapshots = store
            .list_dao_daily_snapshots()
            .map_err(|error| ApiError::internal(error.to_string()))?;
        build_inflation_rate_response(
            &snapshots,
            incomplete_tip_date,
            |first_missing_date, next_snapshot_date| {
                certify_inflation_snapshot_gap_is_blockless(
                    &store,
                    first_missing_date,
                    next_snapshot_date,
                )
            },
        )
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))??;

    state
        .cache
        .set(&cache_key, &response, CacheTtl::CHART)
        .await;
    ok(response)
}

/// THE conversion from a RocksDB day key (`YYYYMMDD`, the UTC+8 stats-key
/// convention) to the one date format every chart point carries: `YYYY-MM-DD`.
///
/// Charts used to have two formatters. The second one only replaced `-` with
/// `/`, which is a no-op on a dash-less day key, so five endpoints shipped raw
/// `20191116` keys while their siblings shipped `2019-11-16`. Parsing through
/// `NaiveDate` keeps this total: a key that is not a real calendar date fails
/// with the key itself rather than reaching the client as a plausible label.
fn format_chart_date(date_key: &str) -> Result<String, ApiRouteError> {
    chrono::NaiveDate::parse_from_str(date_key, "%Y%m%d")
        .map(|date| date.format("%Y-%m-%d").to_string())
        .map_err(|error| {
            ApiError::internal(format!(
                "malformed chart date key {date_key:?} (expected YYYYMMDD): {error}"
            ))
        })
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

            Ok(StackedAreaDataPoint {
                date: format_chart_date(date)?,
                values,
            })
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?;

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

    // Activity hourly buckets are keyed by UTC+8 hour strings (the
    // `block_datetime_from_ms` convention shared by the live and bulk write
    // paths), so the cutoff must come from the same UTC+8 clock — a UTC
    // cutoff sits 8 hours too early in key space and widens the window to
    // ~33 buckets. The rolling window is exactly the last 24 buckets — the
    // current partial hour plus the 23 full hours before it — so the
    // inclusive `since` key is now-23h; an inclusive now-24h cutoff would
    // add a 25th bucket and span up to 25 hours.
    let cutoff = ckbadger_common::now_datetime_utc8() - chrono::Duration::hours(23);
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

/// Total live capacity in shannons from a block header's 32-byte DAO field:
/// `C − S` (RFC-0023 little-endian u64s: C = cumulative total issuance at
/// bytes [0..8], S = complete unissued secondary pool at bytes [16..24]).
/// Every existing cell's capacity — DAO deposits included — is part of
/// `C − S`, which makes it the structural upper bound for any breakdown of
/// live capacity.
fn live_capacity_from_dao(dao: &[u8]) -> Result<i128, String> {
    if dao.len() != 32 {
        return Err(format!("DAO field must be 32 bytes, got {}", dao.len()));
    }
    let total_issuance = u64::from_le_bytes(dao[0..8].try_into().expect("8-byte slice"));
    let unissued_secondary = u64::from_le_bytes(dao[16..24].try_into().expect("8-byte slice"));
    if unissued_secondary > total_issuance {
        return Err(format!(
            "unissued secondary pool exceeds total issuance: C={total_issuance}, S={unissued_secondary}"
        ));
    }
    Ok(total_issuance as i128 - unissued_secondary as i128)
}

/// Split total live capacity into the four asset-ecosystem categories, each
/// as an absolute capacity plus its percentage share of live capacity.
///
/// All four numerators are full cell capacities, matching the denominator's
/// unit. (The old denominator — snapshot knowledge size — counts occupied
/// bytes only, while DAO deposits are mostly free capacity, so dao displayed
/// as 161% and a clamp on `other` silently masked the contradiction.)
/// `other` is the exact remainder; a negative remainder is structurally
/// impossible — every categorized shannon is a live cell's capacity — so it
/// fails fast naming all four inputs.
fn build_capacity_breakdown(
    live_capacity: i128,
    dao_capacity: i128,
    token_capacity: i128,
    object_capacity: i128,
) -> Result<Vec<CapacityCategory>, ApiRouteError> {
    let other_capacity = live_capacity - dao_capacity - token_capacity - object_capacity;
    if other_capacity < 0 {
        return Err(ApiError::internal(format!(
            "categorized capacity exceeds total live capacity: live_capacity={live_capacity}, dao={dao_capacity}, tokens={token_capacity}, objects={object_capacity}"
        )));
    }
    let pct = |value: i128| -> String {
        if live_capacity <= 0 {
            return "0.00".to_string();
        }
        // value ≤ live_capacity < 2^64 shannons, so value × 10_000 is far
        // from i128 overflow, and live_capacity > 0 here.
        let scaled = value * 10_000 / live_capacity;
        let whole = scaled / 100;
        let frac = (scaled % 100).abs();
        format!("{whole}.{frac:02}")
    };
    Ok(vec![
        CapacityCategory {
            category: "dao".to_string(),
            capacity_ckb: shannon_to_ckb_string(dao_capacity),
            percentage: pct(dao_capacity),
        },
        CapacityCategory {
            category: "tokens".to_string(),
            capacity_ckb: shannon_to_ckb_string(token_capacity),
            percentage: pct(token_capacity),
        },
        CapacityCategory {
            category: "objects".to_string(),
            capacity_ckb: shannon_to_ckb_string(object_capacity),
            percentage: pct(object_capacity),
        },
        CapacityCategory {
            category: "other".to_string(),
            capacity_ckb: shannon_to_ckb_string(other_capacity),
            percentage: pct(other_capacity),
        },
    ])
}

#[instrument(skip(state), level = "debug")]
async fn get_asset_ecosystem(
    State(state): State<Arc<AppState>>,
) -> ApiResult<AssetEcosystemResponse> {
    let cache_key = "statistics:asset-ecosystem";
    if let Some(cached) = state.cache.get::<AssetEcosystemResponse>(cache_key).await {
        return ok(cached);
    }

    // Get top 5 tokens from the warmup cache (already sorted by transfers_24h DESC, holders DESC)
    let token_assets = state.load_token_cache().ok_or_else(|| {
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

    // Get total object capacity
    let object_assets = state.load_object_cache().ok_or_else(|| {
        state.asset_cache_unavailable("object cache unavailable; warmup in progress")
    })?;

    let total_object_capacity: i128 = object_assets
        .iter()
        .filter_map(|n| {
            n.owned_capacity
                .as_deref()
                .and_then(|s| s.parse::<i128>().ok())
        })
        .sum();

    // Tip live capacity (the breakdown denominator) + latest DAO snapshot.
    let store = state.store.clone();
    let (tip, dao_snapshot) = tokio::task::spawn_blocking(move || {
        (
            store.get_sync_tip_block(),
            store.get_latest_dao_daily_snapshot(),
        )
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let tip = tip.map_err(|e| ApiError::internal(e.to_string()))?;
    let dao_snapshot = dao_snapshot.map_err(|e| ApiError::internal(e.to_string()))?;

    // Zero only while the store is empty (no tip header ⇒ no cells at all);
    // any nonzero categorized capacity then fails the breakdown invariant.
    let live_capacity: i128 = match tip {
        Some((tip_block, header)) => live_capacity_from_dao(&header.dao).map_err(|e| {
            ApiError::internal(format!(
                "invalid DAO field on tip header: tip_block={tip_block}: {e}"
            ))
        })?,
        None => 0,
    };

    let (dao_locked, knowledge_size) = match dao_snapshot.as_ref() {
        Some(s) => {
            let virtual_occupied = state.genesis_baseline()?.virtual_occupied;
            (
                s.total_deposited,
                common_knowledge_size(s, virtual_occupied)?,
            )
        }
        None => (0, 0),
    };

    let capacity_breakdown = build_capacity_breakdown(
        live_capacity,
        dao_locked,
        total_token_capacity,
        total_object_capacity,
    )?;

    let response = AssetEcosystemResponse {
        top_tokens,
        capacity_breakdown,
        total_live_capacity_ckb: shannon_to_ckb_string(live_capacity),
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

    fn hourly_bucket(hour: i64, txs: i32) -> (String, ckbadger_store::HourlyStats) {
        (
            format!("{hour}"),
            ckbadger_store::HourlyStats {
                hour,
                blocks_count: 1,
                transactions_count: txs,
                cells_created: 0,
                cells_consumed: 0,
                capacity_transferred: 0,
            },
        )
    }

    /// Regression (F6): the rolling 24h window counts only buckets whose start
    /// falls within the trailing 24 hours and normalizes tps over the actual
    /// covered span — never today+yesterday calendar days.
    #[test]
    fn test_rolling_24h_tx_window_bounds_and_span() {
        let reference_ms = 1_800_000_000_000i64; // arbitrary fixed instant
        let reference_s = reference_ms / 1000;
        let hour = 3600;
        let buckets = vec![
            hourly_bucket(reference_s - hour, 10),      // inside
            hourly_bucket(reference_s - 5 * hour, 20),  // inside
            hourly_bucket(reference_s - 23 * hour, 30), // inside (oldest kept)
            hourly_bucket(reference_s - 25 * hour, 40), // outside — excluded
        ];
        let (count, window_secs) = rolling_24h_tx_window(&buckets, reference_ms);
        assert_eq!(count, 60);
        assert_eq!(window_secs, (23 * hour) as f64);
    }

    #[test]
    fn test_rolling_24h_tx_window_empty() {
        let (count, window_secs) = rolling_24h_tx_window(&[], 1_800_000_000_000);
        assert_eq!(count, 0);
        assert_eq!(window_secs, 0.0);
    }

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
            daily_depositor_addresses: 0,
            protocol_deposited: None,
            unmade_dao_interests: 0,
        }
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
    fn test_total_secondary_issuance_does_not_double_count_frozen_compensation() {
        let mut s = snapshot("2026-02-17", 100, 1_000, 50, 0);
        s.compensation = 10;
        s.cum_miner_secondary = 40;
        s.cum_dao_compensation = 30;
        s.cum_treasury = 30;

        assert_eq!(snapshot_total_secondary_issuance(&s).unwrap(), 100);
    }

    #[test]
    fn test_inflation_chart_rejects_missing_complete_day_snapshot() {
        let first = snapshot("2026-02-15", 100, 1_000, 0, 0);
        let after_gap = snapshot("2026-02-17", 100, 1_001, 0, 0);
        let tip = snapshot("2026-02-18", 100, 1_002, 0, 0);

        let error = build_inflation_rate_response(
            &[first, after_gap, tip],
            Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap()),
            |first_missing_date, next_snapshot_date| {
                Err(ApiError::internal(format!(
                    "missing complete-day DAO snapshot for block-bearing date: missing_date={}, next_snapshot_date={}",
                    first_missing_date, next_snapshot_date
                )))
            },
        )
        .unwrap_err();

        assert!(error.1 .0.message.contains("missing_date=2026-02-16"));
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

    fn code_hash_bytes(hex_str: &str) -> Vec<u8> {
        hex::decode(hex_str.trim_start_matches("0x")).expect("valid 32-byte hex")
    }

    #[test]
    fn test_knowledge_bucket_classifies_testnet_udt_as_udt() {
        // Testnet sUDT (simple-udt.toml `[testnet]`). The mainnet-only const set could
        // not reach this hash, so testnet UDT knowledge was undercounted before.
        assert_eq!(
            classify_knowledge_bucket(&code_hash_bytes(
                "0xc5e5dcf215925f7ef4dfaf5f4b4f105bc321c02776d6e7d52a1db3fcd9d011a4"
            )),
            KnowledgeBucket::Udt
        );
    }

    #[test]
    fn test_knowledge_bucket_classifies_testnet_spore_as_nft_spore() {
        // Testnet Spore NFT (spore.toml `[testnet]`).
        assert_eq!(
            classify_knowledge_bucket(&code_hash_bytes(
                "0x685a60219309029d01310311dba953d67029170ca4848a4ff638e57002130a0d"
            )),
            KnowledgeBucket::NftSpore
        );
    }

    #[test]
    fn test_knowledge_bucket_classifies_testnet_bit_cell_as_other_typed() {
        // `.bit Cell` uses a SporeData envelope in its current layout, but its
        // protocol semantics are DotBit identity rather than a Spore NFT.
        assert_eq!(
            classify_knowledge_bucket(&code_hash_bytes(
                "0x0b1f412fbae26853ff7d082d422c2bdd9e2ff94ee8aaec11240a5b34cc6e890f"
            )),
            KnowledgeBucket::OtherTyped
        );
    }

    #[test]
    fn test_knowledge_bucket_preserves_old_mainnet_udt_and_nft_spore_coverage() {
        // Exact-coverage regression (Task 7 style): every code_hash the pre-migration
        // mainnet-only const sets bucketed must land in the SAME bucket via the registry,
        // proving the registry migration does not narrow coverage.

        // Old `UDT_CODE_HASHES`: sUDT + 2 xUDT canonical hashes.
        for udt in [
            "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5",
            "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95",
            "0x25c29dc317811a6f6f3985a7a9ebc4838bd388d19d0feeecf0bcd60f6c0975bb",
        ] {
            assert_eq!(
                classify_knowledge_bucket(&code_hash_bytes(udt)),
                KnowledgeBucket::Udt,
                "old UDT hash {udt} must still bucket as UDT"
            );
        }

        // Actual Spore/Cluster hashes retain their historical bucket. The old list's
        // `.bit Cell` entry was a semantic misclassification and is asserted separately.
        for nft in [
            "0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5",
            "0x685a60219309029d01310311dba953d67029170ca4848a4ff638e57002130a0d",
            "0xbbad126377d45f90a8ee120da988a2d7332c78ba8fd679aab478a19d6c133494",
            "0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075",
            "0x0bbe768b519d8ea7b96d58f1182eb7e6ef96c541fbd9526975077ee09f049058",
            "0x598d793defef36e2eeba54a9b45130e4ca92822e1d193671f490950c3b856080",
        ] {
            assert_eq!(
                classify_knowledge_bucket(&code_hash_bytes(nft)),
                KnowledgeBucket::NftSpore,
                "old NFT/Spore hash {nft} must still bucket as NftSpore"
            );
        }
        assert_eq!(
            classify_knowledge_bucket(&code_hash_bytes(
                "0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33"
            )),
            KnowledgeBucket::OtherTyped,
            ".bit Cell must not be counted as a Spore NFT"
        );

        // DAO hash still classifies as Dao (checked first in the chain).
        assert_eq!(
            classify_knowledge_bucket(&code_hash_bytes(
                "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e"
            )),
            KnowledgeBucket::Dao
        );
    }

    #[test]
    fn test_knowledge_bucket_udt_compatible_hash_is_not_udt() {
        // Faithful-preservation guard: the old `UDT_CODE_HASHES` held ONLY the 3 canonical
        // sUDT/xUDT hashes and no udt-compatible (Stable++/ccBTC/USDI) hashes, so those
        // never counted toward the UDT bucket. USDI's own type-script code_hash
        // (usdi-asset.toml `canonical_ref_hash`) is not a registry Sudt/Xudt, so it must
        // stay in the residual `OtherTyped` bucket — the registry migration must NOT
        // silently widen mainnet UDT coverage to compatibles.
        assert_eq!(
            classify_knowledge_bucket(&code_hash_bytes(
                "0xbfa35a9c38a676682b65ade8f02be164d48632281477e36f8dc2f41f79e56bfc"
            )),
            KnowledgeBucket::OtherTyped
        );
    }

    #[test]
    fn test_build_liquid_supply_by_date_map_subtracts_unissued_secondary_and_dao() {
        // Genesis burnt is now threaded in from the derived baseline (8.4B CKB).
        let genesis_burnt = 840_000_000_000_000_000i128;
        let total = genesis_burnt + 1_000_000;
        let mut s = snapshot("2026-02-17", 100, total, 130, 0);
        s.unmade_dao_interests = 30;
        s.cum_treasury = 100;
        let map = build_liquid_supply_by_date_map(&[s], genesis_burnt).unwrap();
        assert_eq!(map.get("2026-02-17"), Some(&(1_000_000 - 130 - 100)));
    }

    #[test]
    fn test_build_liquid_supply_by_date_map_errors_on_negative_dao_locked() {
        let genesis_burnt = 840_000_000_000_000_000i128;
        let total = genesis_burnt + 1_000_000;
        let s = snapshot("2026-02-17", -1, total, 0, 0);
        let err = build_liquid_supply_by_date_map(&[s], genesis_burnt).unwrap_err();
        assert!(err.1 .0.message.contains("negative total_deposited"));
    }

    #[test]
    fn test_accumulate_capacity_deltas_errors_on_underflow() {
        let err = accumulate_capacity_deltas([(100, 50), (-200, 0)]).unwrap_err();
        assert!(err.1 .0.message.contains("underflow"));
    }

    fn daily_block_stats(avg_difficulty: f64, block_count: i32) -> ckbadger_store::DailyBlockStats {
        ckbadger_store::DailyBlockStats {
            avg_difficulty,
            block_count,
            total_uncles: 0,
            // Deliberately zero: only the bulk builder fills this copy, so the
            // read path must take its divisor from `DailyStats`.
        }
    }

    #[test]
    fn test_calculate_daily_hash_rate_uses_milliseconds() {
        // 8,640 blocks whose gaps sum to a full day (10s average):
        // hash_rate = Σdifficulty / block_time_sum_ms
        //           = 1_000_000 × 8_640 / 86_400_000 = 100 H/ms
        let hash_rate = calculate_daily_hash_rate(
            "20240115",
            &daily_block_stats(1_000_000.0, 8_640),
            86_400_000,
        )
        .unwrap();
        assert_eq!(hash_rate, 100.0);
    }

    /// Regression: mainnet's genesis day is partial — 598 blocks mined in
    /// 67_712_964 ms because the chain started 05:09:50 UTC+8. The node and the
    /// official explorer both report 73_466_099_633.87 H/ms; the full-day
    /// divisor reported 57_576_474_071 (−21.6%).
    #[test]
    fn test_calculate_daily_hash_rate_matches_node_on_partial_genesis_day() {
        let hash_rate = calculate_daily_hash_rate(
            "20191116",
            &daily_block_stats(8_318_741_404_228_533.0, 598),
            67_712_964,
        )
        .unwrap();
        assert_eq!(format!("{hash_rate:.2}"), "73466099633.87");
    }

    #[test]
    fn test_calculate_daily_hash_rate_fails_without_a_mined_span() {
        let err = calculate_daily_hash_rate("20260101", &daily_block_stats(1_000_000.0, 100), 0)
            .unwrap_err();
        assert!(
            err.1 .0.message.contains("20260101")
                && err.1 .0.message.contains("block_time_sum_ms=0"),
            "unexpected message: {}",
            err.1 .0.message
        );
    }

    #[test]
    fn test_format_chart_date_converts_day_keys_and_rejects_junk() {
        assert_eq!(format_chart_date("20191116").unwrap(), "2019-11-16");
        assert_eq!(format_chart_date("20240101").unwrap(), "2024-01-01");
        for junk in ["2024-01-15", "202401", "00000000", "2024011x"] {
            let err = format_chart_date(junk).unwrap_err();
            assert!(
                err.1 .0.message.contains(junk),
                "error must name the offending key: {}",
                err.1 .0.message
            );
        }
    }

    #[test]
    fn test_common_knowledge_size_subtracts_virtual_occupied_and_fails_below_zero() {
        let mut snapshot = ckbadger_store::DaoDailySnapshot {
            date: "2026-07-30".to_string(),
            total_deposited: 0,
            depositors_count: 0,
            new_deposits: 0,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 0,
            total_issuance: 0,
            secondary_pool: 0,
            occupied_capacity: 519_967_746_700_000_000,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury: 0,
            unmade_dao_interests: 0,
            unclaimed_compensation: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: None,
        };
        assert_eq!(
            common_knowledge_size(&snapshot, 504_000_000_000_000_000).unwrap(),
            15_967_746_700_000_000
        );
        // No burn policy declared ⇒ knowledge size is the raw U field.
        assert_eq!(
            common_knowledge_size(&snapshot, 0).unwrap(),
            519_967_746_700_000_000
        );

        snapshot.occupied_capacity = 1;
        let err = common_knowledge_size(&snapshot, 504_000_000_000_000_000).unwrap_err();
        assert!(
            err.1 .0.message.contains("2026-07-30")
                && err.1 .0.message.contains("504000000000000000"),
            "unexpected message: {}",
            err.1 .0.message
        );
    }

    fn window_header(number: i64, ts_ms: i64, compact_target: u32) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![number as u8; 32],
            parent_hash: vec![0u8; 32],
            timestamp: ts_ms,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1800,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target,
            miner_lock_hash: None,
            cycles: None,
        }
    }

    /// Regression (C2): hashRate must be the window's summed per-block work
    /// over its time span, not tip-epoch difficulty over the average gap.
    /// Right after an epoch difficulty step the window still spans mostly
    /// previous-epoch blocks, so the old formula overstated by the step ratio
    /// (~+20%) for ~600 blocks after EVERY epoch boundary.
    #[test]
    fn test_estimate_hash_rate_sums_window_work_instead_of_tip_difficulty() {
        // Mainnet-shaped step: 581 old-epoch gaps at difficulty D and 19
        // new-epoch blocks at ≈1.2·D inside one 600-gap window (the live
        // observation behind this fix: 87.98 displayed vs 73.54 exact PH/s).
        const COMPACT_OLD: u32 = 0x190d_f964;
        const COMPACT_NEW: u32 = 0x190b_a529; // ≈1.2× the difficulty of COMPACT_OLD
        let d_old: u128 = ckb_compact_to_difficulty(COMPACT_OLD)
            .to_string()
            .parse()
            .unwrap();
        let d_new: u128 = ckb_compact_to_difficulty(COMPACT_NEW)
            .to_string()
            .parse()
            .unwrap();
        let step = d_new as f64 / d_old as f64;
        assert!(
            (1.19..=1.21).contains(&step),
            "compact pair must encode a ~1.2× difficulty step, got {step}"
        );

        // 601 headers newest-first, uniform 8s gaps: 19 new-epoch at the top,
        // 582 old-epoch below (the oldest of which is outside the numerator).
        let gap_ms = 8_000i64;
        let total = 601usize;
        let headers: Vec<(i64, CachedBlockHeader)> = (0..total)
            .map(|i| {
                let number = (total - 1 - i) as i64;
                let ts_ms = 1_800_000_000_000i64 - (i as i64) * gap_ms;
                let compact = if i < 19 { COMPACT_NEW } else { COMPACT_OLD };
                (number, window_header(number, ts_ms, compact))
            })
            .collect();

        let span_secs = 600.0 * 8.0;
        // The oldest header's own difficulty predates the span: the counted
        // work is 581 old-epoch + 19 new-epoch blocks.
        let expected = (581 * d_old + 19 * d_new) as f64 / span_secs;
        let got = estimate_hash_rate_from_window(&headers).unwrap().unwrap();
        assert!(
            ((got - expected) / expected).abs() < 1e-12,
            "hash rate must be window work over span: got {got}, expected {expected}"
        );

        // The replaced formula — tip-epoch difficulty over the average gap —
        // reports the new-epoch rate for a window that is still ~97%
        // old-epoch blocks, overstating by ~19%.
        let old_formula = d_new as f64 / 8.0;
        assert!(
            old_formula > got * 1.15,
            "old formula ({old_formula}) must overstate the window-work rate ({got}) by ~19%"
        );
    }

    #[test]
    fn test_estimate_hash_rate_returns_none_below_two_headers() {
        assert_eq!(estimate_hash_rate_from_window(&[]).unwrap(), None);
        let single = vec![(7i64, window_header(7, 1_000, 0x190d_f964))];
        assert_eq!(estimate_hash_rate_from_window(&single).unwrap(), None);
    }

    #[test]
    fn test_estimate_hash_rate_fails_fast_on_non_increasing_timestamps() {
        let headers = vec![
            (2i64, window_header(2, 5_000, 0x190d_f964)),
            (1i64, window_header(1, 5_000, 0x190d_f964)),
        ];
        let err = estimate_hash_rate_from_window(&headers).unwrap_err();
        assert!(err.1 .0.message.contains("non-increasing block timestamps"));
        assert!(err.1 .0.message.contains("newest_block=2"));
    }

    /// Regression (C4): the 24h recent-blocks window must page through the
    /// store until the cutoff — the old single-fetch cap silently truncated
    /// any window holding more blocks than the cap.
    #[test]
    fn test_collect_recent_window_blocks_paginates_beyond_one_page() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let cutoff_ms = 1_000_000i64;

        let mut batch = StoreBatch::new(&store);
        // Blocks 0..=2 sit at/before the cutoff and must be excluded.
        for number in 0..=2i64 {
            batch.put_block_header(
                number,
                &window_header(number, cutoff_ms - (3 - number) * 1_000, 0),
            );
        }
        // Blocks 3..=10 (8 blocks) are inside the window.
        for number in 3..=10i64 {
            batch.put_block_header(
                number,
                &window_header(number, cutoff_ms + (number - 2) * 1_000, 0),
            );
        }
        batch.commit().unwrap();

        // page_size 3 < 8 in-window blocks: the old one-shot logic returned
        // only the first page's worth and silently dropped the rest.
        let blocks = collect_recent_window_blocks(&store, cutoff_ms, 3).unwrap();
        let numbers: Vec<i64> = blocks.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            numbers,
            vec![10, 9, 8, 7, 6, 5, 4, 3],
            "every in-window block must be returned, newest first"
        );
        assert!(blocks.iter().all(|(_, h)| h.timestamp > cutoff_ms));
    }

    #[test]
    fn test_collect_recent_window_blocks_handles_genesis_page_boundary() {
        // page_size 1 with every block in-window: the final page ends exactly
        // at genesis and the cursor must stop instead of stepping below
        // block 0 (encode_block_num panics on negatives).
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        for number in 0..=2i64 {
            batch.put_block_header(number, &window_header(number, 10_000 + number * 1_000, 0));
        }
        batch.commit().unwrap();

        let blocks = collect_recent_window_blocks(&store, 0, 1).unwrap();
        let numbers: Vec<i64> = blocks.iter().map(|(n, _)| *n).collect();
        assert_eq!(numbers, vec![2, 1, 0]);
    }

    #[test]
    fn test_collect_recent_window_blocks_fails_fast_at_safety_bound() {
        // 301 in-window blocks at page_size 3 need 101 pages — beyond the
        // 100-page bound. A window that deep relative to page size means the
        // cutoff or timestamps are broken; the helper must fail with the
        // counts, never silently truncate.
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        for number in 1..=301i64 {
            batch.put_block_header(number, &window_header(number, number, 0));
        }
        batch.commit().unwrap();

        let err = collect_recent_window_blocks(&store, 0, 3).unwrap_err();
        assert!(err.1 .0.message.contains("safety bound"));
        assert!(
            err.1 .0.message.contains("300 blocks"),
            "error must name the collected block count: {}",
            err.1 .0.message
        );
    }

    #[test]
    fn test_live_capacity_from_dao_is_c_minus_s() {
        // C = 47.9B CKB, S = 0.3B CKB (little-endian u64s at [0..8]/[16..24]);
        // AR and U are populated but must not affect the result.
        let mut dao = [0u8; 32];
        dao[0..8].copy_from_slice(&4_790_000_000_000_000_000u64.to_le_bytes());
        dao[8..16].copy_from_slice(&10_000_000_000_000_000_000u64.to_le_bytes());
        dao[16..24].copy_from_slice(&30_000_000_000_000_000u64.to_le_bytes());
        dao[24..32].copy_from_slice(&200_000_000_000_000_000u64.to_le_bytes());
        assert_eq!(
            live_capacity_from_dao(&dao).unwrap(),
            4_760_000_000_000_000_000i128
        );
    }

    #[test]
    fn test_live_capacity_from_dao_fails_fast_on_bad_input() {
        assert!(live_capacity_from_dao(&[0u8; 31])
            .unwrap_err()
            .contains("32 bytes"));
        let mut dao = [0u8; 32];
        dao[16..24].copy_from_slice(&1u64.to_le_bytes()); // S > C
        assert!(live_capacity_from_dao(&dao)
            .unwrap_err()
            .contains("exceeds total issuance"));
    }

    /// Regression (C3): the breakdown percentages are shares of TOTAL LIVE
    /// CAPACITY. With the old knowledge-size denominator (occupied bytes
    /// only) against the full-capacity dao numerator, dao displayed 161%.
    #[test]
    fn test_build_capacity_breakdown_percentages_are_shares_of_live_capacity() {
        // Realistic mainnet magnitudes in shannons: 47.6B CKB live capacity,
        // 8.37B CKB in DAO deposits, 0.12B tokens, 0.03B objects.
        let live = 4_760_000_000_000_000_000i128;
        let dao = 837_000_000_000_000_000i128;
        let tokens = 12_000_000_000_000_000i128;
        let objects = 3_000_000_000_000_000i128;

        let breakdown = build_capacity_breakdown(live, dao, tokens, objects).unwrap();
        let categories: Vec<&str> = breakdown.iter().map(|c| c.category.as_str()).collect();
        assert_eq!(categories, vec!["dao", "tokens", "objects", "other"]);

        let dao_pct: f64 = breakdown[0].percentage.parse().unwrap();
        assert_eq!(breakdown[0].percentage, "17.58");
        assert!(
            (15.0..18.0).contains(&dao_pct),
            "dao share of live capacity must be ~17.58%, got {dao_pct}"
        );

        // `other` is the exact remainder and the four shares partition 100%.
        assert_eq!(
            breakdown[3].capacity_ckb,
            shannon_to_ckb_string(live - dao - tokens - objects)
        );
        let pct_sum: f64 = breakdown
            .iter()
            .map(|c| c.percentage.parse::<f64>().unwrap())
            .sum();
        assert!(
            (99.9..=100.01).contains(&pct_sum),
            "category shares must partition live capacity, got {pct_sum}"
        );
    }

    #[test]
    fn test_build_capacity_breakdown_fails_fast_when_categorized_exceeds_live() {
        // The old code clamped `other` to zero here, silently masking the
        // unit inconsistency instead of failing.
        let err = build_capacity_breakdown(520_000_000_000_000_000, 837_000_000_000_000_000, 1, 2)
            .unwrap_err();
        let message = &err.1 .0.message;
        assert!(message.contains("live_capacity=520000000000000000"));
        assert!(message.contains("dao=837000000000000000"));
        assert!(message.contains("tokens=1"));
        assert!(message.contains("objects=2"));
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
    fn test_build_block_time_distribution_response_epoch_aligned() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        // Blocks in epoch 0 (complete) + one block in epoch 1 (tip → epoch 0 is complete)
        let mut batch = StoreBatch::new(&store);
        for (number, ts_ms, epoch) in [
            (0i64, 0i64, 0i64),
            (1, 1_000, 0),
            (2, 3_000, 0),
            (3, 4_000, 1),
        ] {
            batch.put_block_header(
                number,
                &CachedBlockHeader {
                    hash: vec![number as u8; 32],
                    parent_hash: vec![0u8; 32],
                    timestamp: ts_ms,
                    epoch_number: epoch,
                    epoch_index: 0,
                    epoch_length: 3,
                    dao: vec![0; 32],
                    transactions_count: 1,
                    uncles_count: 0,
                    proposals_count: 0,
                    compact_target: 0,
                    miner_lock_hash: None,
                    cycles: None,
                },
            );
        }
        batch.commit().unwrap();

        let response = build_block_time_distribution_response(&store).unwrap();
        assert_eq!(response.title, "Block Time Distribution (Last 1 Epochs)");
        assert_eq!(response.data.len(), BLOCK_TIME_DIST_BUCKET_COUNT);

        // Epoch 0 block deltas: 0→1 = 1s, 1→2 = 2s (block 3 is epoch 1, excluded)
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
        // 0→1 is 60s (overflow), 1→2 is 1s (in-range), block 3 is tip in epoch 1
        for (number, ts_ms, epoch) in [
            (0i64, 0i64, 0i64),
            (1, 60_000, 0),
            (2, 61_000, 0),
            (3, 62_000, 1),
        ] {
            batch.put_block_header(
                number,
                &CachedBlockHeader {
                    hash: vec![number as u8; 32],
                    parent_hash: vec![0u8; 32],
                    timestamp: ts_ms,
                    epoch_number: epoch,
                    epoch_index: 0,
                    epoch_length: 3,
                    dao: vec![0; 32],
                    transactions_count: 1,
                    uncles_count: 0,
                    proposals_count: 0,
                    compact_target: 0,
                    miner_lock_hash: None,
                    cycles: None,
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
    fn test_build_block_time_distribution_response_empty_when_epoch_zero() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        for (number, ts_ms) in [(0i64, 0i64), (1, 1_000)] {
            batch.put_block_header(
                number,
                &CachedBlockHeader {
                    hash: vec![number as u8; 32],
                    parent_hash: vec![0u8; 32],
                    timestamp: ts_ms,
                    epoch_number: 0,
                    epoch_index: number as i32,
                    epoch_length: 100,
                    dao: vec![0; 32],
                    transactions_count: 1,
                    uncles_count: 0,
                    proposals_count: 0,
                    compact_target: 0,
                    miner_lock_hash: None,
                    cycles: None,
                },
            );
        }
        batch.commit().unwrap();

        let response = build_block_time_distribution_response(&store).unwrap();
        assert_eq!(response.title, "Block Time Distribution (Last 0 Epochs)");
        assert!(!block_time_dist_has_data(&response));
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
            total_live_capacity_ckb: "2000".to_string(),
            total_knowledge_size_ckb: "1000".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"topTokens\""));
        assert!(json.contains("\"capacityBreakdown\""));
        assert!(json.contains("\"totalLiveCapacityCkb\""));
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
            total_live_capacity_ckb: "500".to_string(),
            total_knowledge_size_ckb: "200".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: AssetEcosystemResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.top_tokens.len(), 2);
        assert_eq!(deserialized.capacity_breakdown.len(), 2);
        assert_eq!(deserialized.total_live_capacity_ckb, "500");
        assert_eq!(deserialized.total_knowledge_size_ckb, "200");
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
