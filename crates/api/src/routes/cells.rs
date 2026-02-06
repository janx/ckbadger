use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/cells/live", get(list_live_cells))
        .route("/cells/by-script", get(list_cells_by_script))
        .route("/cells/{tx_hash}/{output_index}", get(get_cell))
        .route("/addresses/top", get(get_top_addresses))
        .route("/addresses/active", get(get_active_addresses))
        .route("/addresses/{addr}", get(get_address))
        .route(
            "/addresses/{addr}/transactions",
            get(get_address_transactions),
        )
        .route("/addresses/{addr}/tokens", get(get_address_tokens))
        .route(
            "/addresses/{addr}/asset-transfers",
            get(get_address_asset_transfers),
        )
}

// ============================================
// Request/Response Types
// ============================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListLiveCellsParams {
    /// Lock script hash to filter by (required for efficient query)
    lock_script_hash: Option<String>,
    #[serde(default = "default_limit")]
    limit: u64,
    #[serde(default)]
    offset: u64,
}

fn default_limit() -> u64 {
    20
}

/// Response for a single cell
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellResponse {
    pub tx_hash: String,
    pub output_index: u16,
    pub capacity: u64,
    pub lock_script_hash: String,
    pub type_script_hash: Option<String>,
    pub lock_code_hash: String,
    pub type_code_hash: Option<String>,
    pub data_size: u32,
    pub created_at_block: u64,
    pub is_live: bool,
}

/// Full cell details response (includes scripts and data)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDetailResponse {
    pub tx_hash: String,
    pub output_index: u16,
    pub block_number: u64,
    pub capacity: u64,
    // Lock script
    pub lock_code_hash: String,
    pub lock_hash_type: u8,
    pub lock_args: String,
    pub lock_script_hash: String,
    // Type script (optional)
    pub type_code_hash: Option<String>,
    pub type_hash_type: Option<u8>,
    pub type_args: Option<String>,
    pub type_script_hash: Option<String>,
    // Data
    pub data_hash: String,
    pub data_size: u32,
    pub data: Option<String>,
}

// ============================================
// ClickHouse Row Types
// ============================================

/// Row type for live cell queries from cell_state table
#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct CellStateQueryRow {
    tx_hash: [u8; 32],
    output_index: u16,
    capacity: u64,
    lock_script_hash: [u8; 32],
    type_script_hash: [u8; 32],
    lock_code_hash: [u8; 32],
    type_code_hash: [u8; 32],
    data_size: u32,
    created_at_block: u64,
    is_live: u8,
}

/// Row type for full cell details from cell_outputs_all table
#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct CellOutputQueryRow {
    tx_hash: [u8; 32],
    output_index: u16,
    block_number: u64,
    capacity: u64,
    // Lock script
    lock_code_hash: [u8; 32],
    lock_hash_type: u8,
    lock_args: String,
    lock_script_hash: [u8; 32],
    // Type script
    type_code_hash: [u8; 32],
    type_hash_type: u8,
    type_args: String,
    type_script_hash: [u8; 32],
    // Data
    data_hash: [u8; 32],
    data_size: u32,
    data: String,
}

// ============================================
// Row -> Response Conversions
// ============================================

fn is_empty_hash(hash: &[u8; 32]) -> bool {
    hash.iter().all(|&b| b == 0)
}

impl From<CellStateQueryRow> for CellResponse {
    fn from(row: CellStateQueryRow) -> Self {
        Self {
            tx_hash: format!("0x{}", hex::encode(row.tx_hash)),
            output_index: row.output_index,
            capacity: row.capacity,
            lock_script_hash: format!("0x{}", hex::encode(row.lock_script_hash)),
            type_script_hash: if is_empty_hash(&row.type_script_hash) {
                None
            } else {
                Some(format!("0x{}", hex::encode(row.type_script_hash)))
            },
            lock_code_hash: format!("0x{}", hex::encode(row.lock_code_hash)),
            type_code_hash: if is_empty_hash(&row.type_code_hash) {
                None
            } else {
                Some(format!("0x{}", hex::encode(row.type_code_hash)))
            },
            data_size: row.data_size,
            created_at_block: row.created_at_block,
            is_live: row.is_live != 0,
        }
    }
}

impl From<CellOutputQueryRow> for CellDetailResponse {
    fn from(row: CellOutputQueryRow) -> Self {
        Self {
            tx_hash: format!("0x{}", hex::encode(row.tx_hash)),
            output_index: row.output_index,
            block_number: row.block_number,
            capacity: row.capacity,
            lock_code_hash: format!("0x{}", hex::encode(row.lock_code_hash)),
            lock_hash_type: row.lock_hash_type,
            lock_args: if row.lock_args.is_empty() {
                "0x".to_string()
            } else {
                format!("0x{}", row.lock_args)
            },
            lock_script_hash: format!("0x{}", hex::encode(row.lock_script_hash)),
            type_code_hash: if is_empty_hash(&row.type_code_hash) {
                None
            } else {
                Some(format!("0x{}", hex::encode(row.type_code_hash)))
            },
            type_hash_type: if is_empty_hash(&row.type_code_hash) {
                None
            } else {
                Some(row.type_hash_type)
            },
            type_args: if row.type_args.is_empty() || is_empty_hash(&row.type_code_hash) {
                None
            } else {
                Some(format!("0x{}", row.type_args))
            },
            type_script_hash: if is_empty_hash(&row.type_script_hash) {
                None
            } else {
                Some(format!("0x{}", hex::encode(row.type_script_hash)))
            },
            data_hash: format!("0x{}", hex::encode(row.data_hash)),
            data_size: row.data_size,
            data: if row.data.is_empty() {
                None
            } else {
                Some(format!("0x{}", row.data))
            },
        }
    }
}

// ============================================
// Route Handlers
// ============================================

async fn list_live_cells(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListLiveCellsParams>,
) -> ApiResult<Vec<CellResponse>> {
    // Require lock_script_hash for efficient queries
    let lock_hash = params.lock_script_hash.ok_or_else(|| {
        ApiError::bad_request("lock_script_hash parameter is required for live cells query")
    })?;

    let hash_bytes = hex::decode(lock_hash.trim_start_matches("0x"))
        .map_err(|_| ApiError::bad_request("Invalid lock_script_hash format"))?;
    if hash_bytes.len() != 32 {
        return Err(ApiError::bad_request("lock_script_hash must be 32 bytes"));
    }

    // Query cell_state with LIMIT 1 BY pattern to get latest state per cell
    let query = format!(
        "SELECT tx_hash, output_index, capacity, lock_script_hash, type_script_hash, \
         lock_code_hash, type_code_hash, data_size, created_at_block, is_live \
         FROM cell_state \
         WHERE lock_script_hash = unhex('{}') \
         ORDER BY canon_version DESC \
         LIMIT 1 BY (tx_hash, output_index) \
         HAVING is_live = 1 AND is_present = 1 \
         LIMIT {} OFFSET {}",
        hex::encode(&hash_bytes),
        params.limit,
        params.offset
    );

    let rows: Vec<CellStateQueryRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query live cells: {}", e)))?;

    let cells: Vec<CellResponse> = rows.into_iter().map(|r| r.into()).collect();
    ok(cells)
}

async fn list_cells_by_script(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_cell(
    State(state): State<Arc<AppState>>,
    Path((tx_hash, output_index)): Path<(String, u16)>,
) -> ApiResult<CellDetailResponse> {
    let hash_bytes = hex::decode(tx_hash.trim_start_matches("0x"))
        .map_err(|_| ApiError::bad_request("Invalid tx_hash format"))?;
    if hash_bytes.len() != 32 {
        return Err(ApiError::bad_request("tx_hash must be 32 bytes"));
    }

    let query = format!(
        "SELECT tx_hash, output_index, block_number, capacity, \
         lock_code_hash, lock_hash_type, lock_args, lock_script_hash, \
         type_code_hash, type_hash_type, type_args, type_script_hash, \
         data_hash, data_size, data \
         FROM cell_outputs_all \
         WHERE tx_hash = unhex('{}') AND output_index = {} \
         LIMIT 1",
        hex::encode(&hash_bytes),
        output_index
    );

    let row: Option<CellOutputQueryRow> = state
        .pool
        .query_one(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query cell: {}", e)))?;

    match row {
        Some(r) => ok(r.into()),
        None => Err(ApiError::not_found("Cell not found")),
    }
}

async fn get_top_addresses(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_active_addresses(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_address(
    State(_state): State<Arc<AppState>>,
    Path(_addr): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_address_transactions(
    State(_state): State<Arc<AppState>>,
    Path(_addr): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_address_tokens(
    State(_state): State<Arc<AppState>>,
    Path(_addr): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_address_asset_transfers(
    State(_state): State<Arc<AppState>>,
    Path(_addr): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
