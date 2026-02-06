use axum::{extract::State, routing::get, Router};
use serde::Serialize;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

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
        .route("/charts/total-supply", get(get_total_supply_chart))
        .route("/charts/nominal-apc", get(get_nominal_apc_chart))
        .route(
            "/charts/secondary-issuance",
            get(get_secondary_issuance_chart),
        )
        .route("/charts/inflation-rate", get(get_inflation_rate_chart))
}

// ============================================
// Response Types
// ============================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatsResponse {
    pub tip_block_number: u64,
    pub total_transactions: u64,
    pub total_live_cells: u64,
}

// ============================================
// ClickHouse Row Types
// ============================================

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct TipBlockRow {
    tip_block: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct TotalTransactionsRow {
    total_txs: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct TotalLiveCellsRow {
    total_live: u64,
}

// ============================================
// Route Handlers
// ============================================

async fn get_network_stats(State(state): State<Arc<AppState>>) -> ApiResult<NetworkStatsResponse> {
    // Query 1: Get tip block number
    let tip_query = "SELECT max(number) as tip_block FROM canonical_blocks FINAL";
    let tip_row: Option<TipBlockRow> = state
        .pool
        .query_one(tip_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query tip block: {}", e)))?;

    let tip_block_number = tip_row.map(|r| r.tip_block).unwrap_or(0);

    // Query 2: Get total transactions (canonical only)
    let tx_query = "SELECT count() as total_txs FROM transactions_all t \
                    INNER JOIN canonical_blocks c ON t.block_number = c.number AND t.block_hash = c.block_hash";
    let tx_row: Option<TotalTransactionsRow> =
        state.pool.query_one(tx_query).await.map_err(|e| {
            ApiError::internal(format!("Failed to query total transactions: {}", e))
        })?;

    let total_transactions = tx_row.map(|r| r.total_txs).unwrap_or(0);

    // Query 3: Get total live cells (latest version per cell)
    // Use subquery to get latest version per cell, then filter for live cells
    let cells_query = "SELECT count() as total_live FROM ( \
                           SELECT tx_hash, output_index, is_live, is_present \
                           FROM cell_state \
                           ORDER BY canon_version DESC \
                           LIMIT 1 BY (tx_hash, output_index) \
                       ) WHERE is_live = 1 AND is_present = 1";
    let cells_row: Option<TotalLiveCellsRow> = state
        .pool
        .query_one(cells_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query total live cells: {}", e)))?;

    let total_live_cells = cells_row.map(|r| r.total_live).unwrap_or(0);

    ok(NetworkStatsResponse {
        tip_block_number,
        total_transactions,
        total_live_cells,
    })
}

async fn get_tx_stats(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_recent_blocks(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_transaction_count_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_cell_count_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_knowledge_size_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_block_time_distribution_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_epoch_time_distribution_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_epoch_time_length_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_average_block_time_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_hash_rate_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_difficulty_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_uncle_rate_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_miner_address_distribution_chart(
    State(_state): State<Arc<AppState>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_total_supply_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_nominal_apc_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_secondary_issuance_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_inflation_rate_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
