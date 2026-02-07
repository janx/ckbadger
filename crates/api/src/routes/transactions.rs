use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::response::{
    decode_cursor, encode_cursor, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::AppState;

const CACHE_TTL_TX_LIST_SECS: u64 = 5;
const CACHE_KEY_TX_LIST: &str = "transactions:list";
const CACHE_TTL_TX_DETAIL_SECS: u64 = 60;
const CACHE_KEY_TX_DETAIL: &str = "tx:detail";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/transactions", get(list_transactions))
        .route("/transactions/{hash}", get(get_transaction))
        .route("/transactions/{hash}/detail", get(get_transaction_detail))
        .route("/transactions/{hash}/cell-deps", get(get_cell_deps))
        .route("/transactions/{hash}/cycles", get(get_cycles_status))
        .route(
            "/transactions/{hash}/lifecycle",
            get(get_transaction_lifecycle),
        )
        .route(
            "/transactions/{hash}/calculate-cycles",
            post(trigger_cycles_calculation),
        )
        .route(
            "/transactions/{hash}/asset-transfers",
            get(get_transaction_asset_transfers),
        )
        .route("/transactions/{hash}/activities", get(get_tx_activities))
}

// ============================================
// Request/Response Types
// ============================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListTransactionsParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionResponse {
    pub hash: String,
    pub block_number: u64,
    pub block_hash: String,
    pub tx_index: u32,
    pub version: u32,
    pub inputs_count: u16,
    pub outputs_count: u16,
    pub witnesses_count: u16,
    pub cell_deps_count: u16,
    pub header_deps_count: u16,
    pub total_input_capacity: u64,
    pub total_output_capacity: u64,
    pub fee: u64,
    pub tx_size: u32,
    pub cycles: u64,
    pub is_cellbase: bool,
    pub timestamp: i64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct TransactionQueryRow {
    hash: [u8; 32],
    block_number: u64,
    block_hash: [u8; 32],
    tx_index: u32,
    version: u32,
    inputs_count: u16,
    outputs_count: u16,
    witnesses_count: u16,
    cell_deps_count: u16,
    header_deps_count: u16,
    total_input_capacity: u64,
    total_output_capacity: u64,
    fee: u64,
    tx_size: u32,
    cycles: u64,
    is_cellbase: u8,
    timestamp: i64, // DateTime64(3) as Unix timestamp millis
}

impl From<TransactionQueryRow> for TransactionResponse {
    fn from(row: TransactionQueryRow) -> Self {
        Self {
            hash: format!("0x{}", hex::encode(row.hash)),
            block_number: row.block_number,
            block_hash: format!("0x{}", hex::encode(row.block_hash)),
            tx_index: row.tx_index,
            version: row.version,
            inputs_count: row.inputs_count,
            outputs_count: row.outputs_count,
            witnesses_count: row.witnesses_count,
            cell_deps_count: row.cell_deps_count,
            header_deps_count: row.header_deps_count,
            total_input_capacity: row.total_input_capacity,
            total_output_capacity: row.total_output_capacity,
            fee: row.fee,
            tx_size: row.tx_size,
            cycles: row.cycles,
            is_cellbase: row.is_cellbase != 0,
            timestamp: row.timestamp,
        }
    }
}

// ============================================
// Transaction Detail Types (inputs/outputs)
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionDetailResponse {
    #[serde(flatten)]
    pub transaction: TransactionResponse,
    pub inputs: Vec<TransactionInputResponse>,
    pub outputs: Vec<TransactionOutputResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionInputResponse {
    pub input_index: u16,
    pub previous_tx_hash: String,
    pub previous_output_index: u16,
    pub since: String,
    pub capacity: Option<u64>,
    pub lock_script_hash: Option<String>,
    pub type_script_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionOutputResponse {
    pub output_index: u16,
    pub capacity: u64,
    pub lock_script_hash: String,
    pub type_script_hash: Option<String>,
    pub data_size: u32,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct CellInputQueryRow {
    input_index: u16,
    previous_tx_hash: [u8; 32],
    previous_output_index: u16,
    since: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct CellOutputQueryRow {
    output_index: u16,
    capacity: u64,
    lock_script_hash: [u8; 32],
    type_script_hash: [u8; 32],
    data_size: u32,
}

impl From<CellInputQueryRow> for TransactionInputResponse {
    fn from(row: CellInputQueryRow) -> Self {
        Self {
            input_index: row.input_index,
            previous_tx_hash: format!("0x{}", hex::encode(row.previous_tx_hash)),
            previous_output_index: row.previous_output_index,
            since: format!("0x{:x}", row.since),
            capacity: None,
            lock_script_hash: None,
            type_script_hash: None,
        }
    }
}

impl From<CellOutputQueryRow> for TransactionOutputResponse {
    fn from(row: CellOutputQueryRow) -> Self {
        let type_script_hash = if row.type_script_hash == [0u8; 32] {
            None
        } else {
            Some(format!("0x{}", hex::encode(row.type_script_hash)))
        };
        Self {
            output_index: row.output_index,
            capacity: row.capacity,
            lock_script_hash: format!("0x{}", hex::encode(row.lock_script_hash)),
            type_script_hash,
            data_size: row.data_size,
        }
    }
}

// ============================================
// Cell Deps Types
// ============================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDepResponse {
    pub dep_index: u16,
    pub out_point: OutPointResponse,
    pub dep_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutPointResponse {
    pub tx_hash: String,
    pub index: u16,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct CellDepQueryRow {
    dep_index: u16,
    out_point_tx_hash: [u8; 32],
    out_point_index: u16,
    dep_type: u8,
}

impl From<CellDepQueryRow> for CellDepResponse {
    fn from(row: CellDepQueryRow) -> Self {
        Self {
            dep_index: row.dep_index,
            out_point: OutPointResponse {
                tx_hash: format!("0x{}", hex::encode(row.out_point_tx_hash)),
                index: row.out_point_index,
            },
            dep_type: match row.dep_type {
                0 => "code".to_string(),
                1 => "dep_group".to_string(),
                _ => "unknown".to_string(),
            },
        }
    }
}

// ============================================
// Transaction Activities Types
// ============================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionActivityResponse {
    pub activity_id: String,
    pub activity_type: String,
    pub activity_category: String,
    pub activity_index: u16,
    pub from_lock_hash: Option<String>,
    pub to_lock_hash: Option<String>,
    pub amount: String,
    pub asset_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct ActivityQueryRow {
    activity_id: [u8; 32],
    activity_type: String,
    activity_category: String,
    activity_index: u16,
    from_lock_hash: [u8; 32],
    to_lock_hash: [u8; 32],
    amount: [u8; 32], // UInt256 as 32 bytes
    asset_id: [u8; 32],
    metadata: String,
}

impl From<ActivityQueryRow> for TransactionActivityResponse {
    fn from(row: ActivityQueryRow) -> Self {
        let from_lock_hash = if row.from_lock_hash == [0u8; 32] {
            None
        } else {
            Some(format!("0x{}", hex::encode(row.from_lock_hash)))
        };
        let to_lock_hash = if row.to_lock_hash == [0u8; 32] {
            None
        } else {
            Some(format!("0x{}", hex::encode(row.to_lock_hash)))
        };
        let asset_id = if row.asset_id == [0u8; 32] {
            None
        } else {
            Some(format!("0x{}", hex::encode(row.asset_id)))
        };
        // UInt256 is stored as little-endian bytes in ClickHouse
        let amount = u256_from_le_bytes(&row.amount);
        let metadata = if row.metadata.is_empty() {
            None
        } else {
            serde_json::from_str(&row.metadata).ok()
        };

        Self {
            activity_id: format!("0x{}", hex::encode(row.activity_id)),
            activity_type: row.activity_type,
            activity_category: row.activity_category,
            activity_index: row.activity_index,
            from_lock_hash,
            to_lock_hash,
            amount,
            asset_id,
            metadata,
        }
    }
}

/// Convert 32-byte little-endian UInt256 to decimal string
fn u256_from_le_bytes(bytes: &[u8; 32]) -> String {
    // UInt256 is stored as little-endian in ClickHouse
    // Convert to big integer for display
    let mut value = [0u64; 4];
    for (i, chunk) in bytes.chunks(8).enumerate() {
        value[i] = u64::from_le_bytes(chunk.try_into().unwrap());
    }
    // Build the u256 value: value[0] + value[1]*2^64 + value[2]*2^128 + value[3]*2^192
    // For simplicity, use u128 if the value fits, otherwise show as hex
    if value[2] == 0 && value[3] == 0 {
        let low = value[0] as u128 | ((value[1] as u128) << 64);
        low.to_string()
    } else {
        // Large value - show as hex
        format!(
            "0x{}",
            hex::encode(bytes.iter().rev().cloned().collect::<Vec<_>>())
        )
    }
}

// ============================================
// Route Handlers
// ============================================

async fn list_transactions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListTransactionsParams>,
) -> ApiResult<CursorPaginatedResponse<TransactionResponse>> {
    let limit = params.limit.min(100);
    let is_first_page = params.cursor.is_none();
    let cache_key = format!("{}:{}", CACHE_KEY_TX_LIST, limit);

    if is_first_page {
        if let Some(cached) = state
            .cache
            .get::<CursorPaginatedResponse<TransactionResponse>>(&cache_key)
            .await
        {
            return ok(cached);
        }
    }

    let cursor_filter = match params.cursor.as_ref().and_then(|c| decode_cursor(c)) {
        Some((block_num, tx_idx)) => format!(
            "AND (t.block_number < {} OR (t.block_number = {} AND t.tx_index < {}))",
            block_num, block_num, tx_idx
        ),
        None => String::new(),
    };

    let sync_status = state.cache.get_sync_status(&state.pool).await;
    let total = sync_status.total_transactions;

    let query = format!(
        "SELECT t.hash, t.block_number, t.block_hash, t.tx_index, t.version, \
         t.inputs_count, t.outputs_count, t.witnesses_count, t.cell_deps_count, t.header_deps_count, \
         t.total_input_capacity, t.total_output_capacity, t.fee, t.tx_size, t.cycles, t.is_cellbase, \
         t.timestamp \
         FROM transactions_all t \
         INNER JOIN canonical_blocks FINAL c ON t.block_number = c.number AND t.block_hash = c.block_hash \
         WHERE 1=1 {} \
         ORDER BY t.block_number DESC, t.tx_index DESC \
         LIMIT {}",
        cursor_filter,
        limit + 1
    );

    let rows: Vec<TransactionQueryRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query transactions: {}", e)))?;

    let has_more = rows.len() as i64 > limit;
    let data: Vec<TransactionResponse> = rows
        .into_iter()
        .take(limit as usize)
        .map(|r| r.into())
        .collect();

    let next_cursor = if has_more {
        data.last()
            .map(|tx| encode_cursor(tx.block_number as i64, tx.tx_index as i32))
    } else {
        None
    };

    let response = CursorPaginatedResponse::new(data, total, limit, next_cursor);

    if is_first_page {
        state
            .cache
            .set(
                &cache_key,
                &response,
                Duration::from_secs(CACHE_TTL_TX_LIST_SECS),
            )
            .await;
    }

    ok(response)
}

async fn get_transaction(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionResponse> {
    // Validate hash format
    if !hash.starts_with("0x") {
        return Err(ApiError::bad_request("Transaction hash must start with 0x"));
    }

    let hash_bytes = hex::decode(hash.trim_start_matches("0x"))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash format"))?;
    if hash_bytes.len() != 32 {
        return Err(ApiError::bad_request("Transaction hash must be 32 bytes"));
    }

    // Check cache first
    let cache_key = format!("{}:{}", CACHE_KEY_TX_DETAIL, hash.to_lowercase());
    if let Some(cached) = state.cache.get::<TransactionResponse>(&cache_key).await {
        return ok(cached);
    }

    let query = format!(
        "SELECT t.hash, t.block_number, t.block_hash, t.tx_index, t.version, \
         t.inputs_count, t.outputs_count, t.witnesses_count, t.cell_deps_count, t.header_deps_count, \
         t.total_input_capacity, t.total_output_capacity, t.fee, t.tx_size, t.cycles, t.is_cellbase, \
         t.timestamp \
         FROM transactions_all t \
         INNER JOIN canonical_blocks FINAL c ON t.block_number = c.number AND t.block_hash = c.block_hash \
         WHERE t.hash = unhex('{}') \
         LIMIT 1",
        hex::encode(&hash_bytes)
    );

    let row: Option<TransactionQueryRow> = state
        .pool
        .query_one(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query transaction: {}", e)))?;

    match row {
        Some(r) => {
            let response: TransactionResponse = r.into();
            // Cache the result
            state
                .cache
                .set(
                    &cache_key,
                    &response,
                    Duration::from_secs(CACHE_TTL_TX_DETAIL_SECS),
                )
                .await;
            ok(response)
        }
        None => Err(ApiError::not_found("Transaction not found")),
    }
}

async fn get_transaction_detail(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionDetailResponse> {
    if !hash.starts_with("0x") {
        return Err(ApiError::bad_request("Transaction hash must start with 0x"));
    }

    let hash_bytes = hex::decode(hash.trim_start_matches("0x"))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash format"))?;
    if hash_bytes.len() != 32 {
        return Err(ApiError::bad_request("Transaction hash must be 32 bytes"));
    }

    let cache_key = format!("{}:detail:{}", CACHE_KEY_TX_DETAIL, hash.to_lowercase());
    if let Some(cached) = state
        .cache
        .get::<TransactionDetailResponse>(&cache_key)
        .await
    {
        return ok(cached);
    }

    let hash_hex = hex::encode(&hash_bytes);

    let tx_query = format!(
        "SELECT t.hash, t.block_number, t.block_hash, t.tx_index, t.version, \
         t.inputs_count, t.outputs_count, t.witnesses_count, t.cell_deps_count, t.header_deps_count, \
         t.total_input_capacity, t.total_output_capacity, t.fee, t.tx_size, t.cycles, t.is_cellbase, \
         t.timestamp \
         FROM transactions_all t \
         INNER JOIN canonical_blocks FINAL c ON t.block_number = c.number AND t.block_hash = c.block_hash \
         WHERE t.hash = unhex('{}') \
         LIMIT 1",
        hash_hex
    );

    let tx_row: Option<TransactionQueryRow> = state
        .pool
        .query_one(&tx_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query transaction: {}", e)))?;

    let tx_row = match tx_row {
        Some(r) => r,
        None => return Err(ApiError::not_found("Transaction not found")),
    };
    let transaction: TransactionResponse = tx_row.into();

    let inputs_query = format!(
        "SELECT i.input_index, i.previous_tx_hash, i.previous_output_index, i.since \
         FROM cell_inputs_all i \
         INNER JOIN canonical_blocks FINAL c ON i.tx_block_number = c.number \
         INNER JOIN transactions_all t ON i.tx_hash = t.hash AND i.tx_block_number = t.block_number AND t.block_hash = c.block_hash \
         WHERE i.tx_hash = unhex('{}') \
         ORDER BY i.input_index",
        hash_hex
    );

    let input_rows: Vec<CellInputQueryRow> = state
        .pool
        .query_all(&inputs_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query inputs: {}", e)))?;
    let inputs: Vec<TransactionInputResponse> = input_rows.into_iter().map(|r| r.into()).collect();

    let outputs_query = format!(
        "SELECT o.output_index, o.capacity, o.lock_script_hash, o.type_script_hash, o.data_size \
         FROM cell_outputs_all o \
         INNER JOIN canonical_blocks FINAL c ON o.block_number = c.number AND o.block_hash = c.block_hash \
         WHERE o.tx_hash = unhex('{}') \
         ORDER BY o.output_index",
        hash_hex
    );

    let output_rows: Vec<CellOutputQueryRow> = state
        .pool
        .query_all(&outputs_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query outputs: {}", e)))?;
    let outputs: Vec<TransactionOutputResponse> =
        output_rows.into_iter().map(|r| r.into()).collect();

    let response = TransactionDetailResponse {
        transaction,
        inputs,
        outputs,
    };

    state
        .cache
        .set(
            &cache_key,
            &response,
            Duration::from_secs(CACHE_TTL_TX_DETAIL_SECS),
        )
        .await;

    ok(response)
}

async fn get_cell_deps(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Vec<CellDepResponse>> {
    if !hash.starts_with("0x") {
        return Err(ApiError::bad_request("Transaction hash must start with 0x"));
    }

    let hash_bytes = hex::decode(hash.trim_start_matches("0x"))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash format"))?;
    if hash_bytes.len() != 32 {
        return Err(ApiError::bad_request("Transaction hash must be 32 bytes"));
    }
    let hash_hex = hex::encode(&hash_bytes);

    let query = format!(
        "SELECT d.dep_index, d.out_point_tx_hash, d.out_point_index, d.dep_type \
         FROM transaction_cell_deps d \
         INNER JOIN canonical_blocks FINAL c ON d.tx_block_number = c.number \
         INNER JOIN transactions_all t ON d.tx_hash = t.hash AND d.tx_block_number = t.block_number AND t.block_hash = c.block_hash \
         WHERE d.tx_hash = unhex('{}') \
         ORDER BY d.dep_index",
        hash_hex
    );

    let rows: Vec<CellDepQueryRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query cell deps: {}", e)))?;

    let cell_deps: Vec<CellDepResponse> = rows.into_iter().map(|r| r.into()).collect();
    ok(cell_deps)
}

async fn get_cycles_status(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_transaction_lifecycle(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn trigger_cycles_calculation(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_transaction_asset_transfers(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_tx_activities(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Vec<TransactionActivityResponse>> {
    if !hash.starts_with("0x") {
        return Err(ApiError::bad_request("Transaction hash must start with 0x"));
    }

    let hash_bytes = hex::decode(hash.trim_start_matches("0x"))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash format"))?;
    if hash_bytes.len() != 32 {
        return Err(ApiError::bad_request("Transaction hash must be 32 bytes"));
    }
    let hash_hex = hex::encode(&hash_bytes);

    let query = format!(
        "SELECT a.activity_id, a.activity_type, a.activity_category, a.activity_index, \
         a.from_lock_hash, a.to_lock_hash, a.amount, a.asset_id, a.metadata \
         FROM activities_all a \
         INNER JOIN canonical_blocks FINAL c ON a.block_number = c.number \
         INNER JOIN transactions_all t ON a.tx_hash = t.hash AND a.block_number = t.block_number AND t.block_hash = c.block_hash \
         WHERE a.tx_hash = unhex('{}') \
         ORDER BY a.activity_index",
        hash_hex
    );

    let rows: Vec<ActivityQueryRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query activities: {}", e)))?;

    let activities: Vec<TransactionActivityResponse> = rows.into_iter().map(|r| r.into()).collect();
    ok(activities)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_ttl_tx_list_is_5_seconds() {
        assert_eq!(CACHE_TTL_TX_LIST_SECS, 5);
    }

    #[test]
    fn test_cache_key_tx_list_has_correct_prefix() {
        assert!(CACHE_KEY_TX_LIST.starts_with("transactions:"));
    }

    #[test]
    fn test_cache_ttl_tx_detail_is_60_seconds() {
        assert_eq!(CACHE_TTL_TX_DETAIL_SECS, 60);
    }

    #[test]
    fn test_cache_key_tx_detail_has_correct_prefix() {
        assert!(CACHE_KEY_TX_DETAIL.starts_with("tx:"));
    }

    #[test]
    fn test_transaction_response_serialization() {
        let response = TransactionResponse {
            hash: "0xabc".to_string(),
            block_number: 12345,
            block_hash: "0xdef".to_string(),
            tx_index: 1,
            version: 0,
            inputs_count: 2,
            outputs_count: 3,
            witnesses_count: 1,
            cell_deps_count: 1,
            header_deps_count: 0,
            total_input_capacity: 100000000,
            total_output_capacity: 99000000,
            fee: 1000000,
            tx_size: 500,
            cycles: 10000,
            is_cellbase: false,
            timestamp: 1704067200000,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"blockNumber\":12345"));
        assert!(json.contains("\"inputsCount\":2"));
        assert!(json.contains("\"isCellbase\":false"));
    }

    #[test]
    fn test_transaction_response_deserialization() {
        let json = r#"{"hash":"0xabc","blockNumber":12345,"blockHash":"0xdef","txIndex":1,"version":0,"inputsCount":2,"outputsCount":3,"witnessesCount":1,"cellDepsCount":1,"headerDepsCount":0,"totalInputCapacity":100000000,"totalOutputCapacity":99000000,"fee":1000000,"txSize":500,"cycles":10000,"isCellbase":false,"timestamp":1704067200000}"#;
        let response: TransactionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.block_number, 12345);
        assert_eq!(response.inputs_count, 2);
        assert!(!response.is_cellbase);
    }
}
