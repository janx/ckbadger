use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Router,
};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::clickhouse::{hex_hash, unhex_hash};
use crate::cycles::CyclesStatusResponse;
use crate::response::{
    decode_cursor, encode_cursor, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::AppState;

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
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    block_number: Option<i64>,
    cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionResponse {
    pub hash: String,
    pub block_number: i64,
    pub block_hash: String,
    pub index: i32,
    pub inputs_count: i32,
    pub outputs_count: i32,
    pub fee: String,
    pub tx_size: Option<i32>,
    pub cycles: Option<i64>,
    pub is_cellbase: bool,
    pub timestamp: String,
}

#[derive(Debug, Row, Deserialize)]
struct TransactionRowClickHouse {
    hash: String,
    block_number: u64,
    block_hash: String,
    tx_index: u32,
    inputs_count: u16,
    outputs_count: u16,
    fee: u64,
    tx_size: Option<u32>,
    cycles: Option<u64>,
    is_cellbase: u8,
    timestamp: u32,
}

async fn list_transactions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<TransactionResponse>> {
    let limit = params.limit.clamp(1, 100);

    let query = if let Some(block_number) = params.block_number {
        // Filter by block_number, order by tx_index ASC
        let cursor = params.cursor.as_ref().and_then(|c| decode_cursor(c));
        let (_cursor_block, cursor_index) = cursor.unwrap_or((i64::MAX, i32::MAX));

        format!(
            "SELECT 
                {} as hash,
                t.block_number,
                {} as block_hash,
                t.tx_index,
                t.inputs_count,
                t.outputs_count,
                t.fee,
                t.tx_size,
                t.cycles,
                t.is_cellbase,
                toUnixTimestamp(t.timestamp) as timestamp
            FROM transactions t
            JOIN blocks b ON t.block_number = b.number
            WHERE t.block_number = {} AND t.tx_index < {}
            ORDER BY t.tx_index ASC
            LIMIT {}",
            hex_hash("t.hash"),
            hex_hash("b.hash"),
            block_number,
            cursor_index,
            limit + 1
        )
    } else if let Some(ref cursor_str) = params.cursor {
        // Global list with cursor
        let (cursor_block, cursor_index) = decode_cursor(cursor_str)
            .ok_or_else(|| ApiError::bad_request("Invalid cursor format"))?;

        format!(
            "SELECT 
                {} as hash,
                t.block_number,
                {} as block_hash,
                t.tx_index,
                t.inputs_count,
                t.outputs_count,
                t.fee,
                t.tx_size,
                t.cycles,
                t.is_cellbase,
                toUnixTimestamp(t.timestamp) as timestamp
            FROM transactions t
            JOIN blocks b ON t.block_number = b.number
            WHERE (t.block_number, t.tx_index) < ({}, {})
            ORDER BY t.block_number DESC, t.tx_index DESC
            LIMIT {}",
            hex_hash("t.hash"),
            hex_hash("b.hash"),
            cursor_block,
            cursor_index,
            limit + 1
        )
    } else {
        // Global list without cursor
        format!(
            "SELECT 
                {} as hash,
                t.block_number,
                {} as block_hash,
                t.tx_index,
                t.inputs_count,
                t.outputs_count,
                t.fee,
                t.tx_size,
                t.cycles,
                t.is_cellbase,
                toUnixTimestamp(t.timestamp) as timestamp
            FROM transactions t
            JOIN blocks b ON t.block_number = b.number
            ORDER BY t.block_number DESC, t.tx_index DESC
            LIMIT {}",
            hex_hash("t.hash"),
            hex_hash("b.hash"),
            limit + 1
        )
    };

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<TransactionRowClickHouse>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|r| encode_cursor(r.block_number as i64, r.tx_index as i32))
    } else {
        None
    };

    // Get total count from ClickHouse sync_status
    let total: i64 = if let Some(block_number) = params.block_number {
        let total_query = format!(
            "SELECT transactions_count FROM blocks WHERE number = {}",
            block_number
        );
        let total_rows = state
            .clickhouse
            .client()
            .query(&total_query)
            .fetch_all::<u32>()
            .await
            .unwrap_or_default();
        total_rows.into_iter().next().map(|c| c as i64).unwrap_or(0)
    } else {
        let total_query = "SELECT total_transactions FROM sync_status WHERE id = 1";
        let total_rows = state
            .clickhouse
            .client()
            .query(total_query)
            .fetch_all::<u64>()
            .await
            .unwrap_or_default();
        total_rows.into_iter().next().map(|c| c as i64).unwrap_or(0)
    };

    let txs: Vec<TransactionResponse> = rows
        .into_iter()
        .map(|r| TransactionResponse {
            hash: format!("0x{}", r.hash),
            block_number: r.block_number as i64,
            block_hash: format!("0x{}", r.block_hash),
            index: r.tx_index as i32,
            inputs_count: r.inputs_count as i32,
            outputs_count: r.outputs_count as i32,
            fee: r.fee.to_string(),
            tx_size: r.tx_size.map(|s| s as i32),
            cycles: r.cycles.map(|c| c as i64),
            is_cellbase: r.is_cellbase != 0,
            timestamp: chrono::DateTime::from_timestamp(r.timestamp as i64, 0)
                .unwrap_or_default()
                .to_rfc3339(),
        })
        .collect();

    ok(CursorPaginatedResponse::new(txs, total, limit, next_cursor))
}

async fn get_transaction(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionResponse> {
    let _hash_bytes = unhex_hash(&hash)?;

    let query = format!(
        "SELECT 
            {} as hash,
            t.block_number,
            {} as block_hash,
            t.tx_index,
            t.inputs_count,
            t.outputs_count,
            t.total_input_capacity,
            t.total_output_capacity,
            t.tx_size,
            t.cycles,
            t.is_cellbase,
            toUnixTimestamp(t.timestamp) as timestamp
        FROM transactions t
        JOIN blocks b ON t.block_number = b.number
        WHERE t.hash = unhex('{}')
        LIMIT 1",
        hex_hash("t.hash"),
        hex_hash("b.hash"),
        hash.strip_prefix("0x").unwrap_or(&hash)
    );

    let row = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_optional::<TransactionDetailRowClickHouse>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match row {
        Some(r) => {
            let input: u128 = r.total_input_capacity as u128;
            let output: u128 = r.total_output_capacity as u128;

            let fee = if output > input {
                let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
                    .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

                let dao_query = format!(
                    "SELECT SUM(compensation) FROM dao_deposits WHERE withdraw_tx = unhex('{}') AND status = 2",
                    hex::encode(&hash_bytes)
                );
                let dao_rows = state
                    .clickhouse
                    .client()
                    .query(&dao_query)
                    .fetch_all::<u64>()
                    .await
                    .unwrap_or_default();

                let dao_compensation: u128 =
                    dao_rows.into_iter().next().map(|c| c as u128).unwrap_or(0);

                let effective_input = input + dao_compensation;
                if effective_input >= output {
                    (effective_input - output).to_string()
                } else {
                    "0".to_string()
                }
            } else {
                (input - output).to_string()
            };

            ok(TransactionResponse {
                hash: format!("0x{}", r.hash),
                block_number: r.block_number as i64,
                block_hash: format!("0x{}", r.block_hash),
                index: r.tx_index as i32,
                inputs_count: r.inputs_count as i32,
                outputs_count: r.outputs_count as i32,
                fee,
                tx_size: r.tx_size.map(|s| s as i32),
                cycles: r.cycles.map(|c| c as i64),
                is_cellbase: r.is_cellbase != 0,
                timestamp: chrono::DateTime::from_timestamp(r.timestamp as i64, 0)
                    .unwrap_or_default()
                    .to_rfc3339(),
            })
        }
        None => Err(ApiError::not_found("Transaction not found")),
    }
}

#[derive(Debug, Row, Deserialize)]
struct TransactionDetailRowClickHouse {
    hash: String,
    block_number: u64,
    block_hash: String,
    tx_index: u32,
    inputs_count: u16,
    outputs_count: u16,
    total_input_capacity: u64,
    total_output_capacity: u64,
    tx_size: Option<u32>,
    cycles: Option<u64>,
    is_cellbase: u8,
    timestamp: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResponse {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionInputResponse {
    pub previous_output: Option<PreviousOutput>,
    pub since: String,
    pub capacity: Option<String>,
    pub lock: Option<ScriptResponse>,
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousOutput {
    pub tx_hash: String,
    pub index: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionOutputResponse {
    pub capacity: String,
    pub occupied_capacity: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_occupied_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_type: Option<String>,
    pub lock: Option<ScriptResponse>,
    pub r#type: Option<ScriptResponse>,
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionDetailResponse {
    pub hash: String,
    pub block_number: i64,
    pub block_hash: String,
    pub index: i32,
    pub inputs_count: i32,
    pub outputs_count: i32,
    pub fee: String,
    pub fee_rate: Option<String>,
    pub tx_size: Option<i32>,
    pub cycles: Option<i64>,
    pub confirmations: i64,
    pub is_cellbase: bool,
    pub timestamp: String,
    pub inputs_capacity: String,
    pub outputs_capacity: String,
    pub inputs_occupied_capacity: String,
    pub outputs_occupied_capacity: String,
    pub inputs: Vec<TransactionInputResponse>,
    pub outputs: Vec<TransactionOutputResponse>,
}

fn hash_type_to_string(hash_type: i16) -> String {
    match hash_type {
        0 => "data".to_string(),
        1 => "type".to_string(),
        2 => "data1".to_string(),
        4 => "data2".to_string(),
        _ => "unknown".to_string(),
    }
}

async fn fetch_tx_size_from_rpc(rpc_url: &str, tx_hash: &str) -> Option<i32> {
    #[derive(serde::Serialize)]
    struct RpcRequest<'a> {
        jsonrpc: &'static str,
        method: &'static str,
        params: (&'a str,),
        id: u64,
    }

    #[derive(serde::Deserialize)]
    struct RpcResponse {
        result: Option<TxResult>,
    }

    #[derive(serde::Deserialize)]
    struct TxResult {
        transaction: Option<TxView>,
    }

    #[derive(serde::Deserialize)]
    struct TxView {
        cell_deps: Vec<CellDep>,
        header_deps: Vec<String>,
        inputs: Vec<CellInput>,
        outputs: Vec<CellOutput>,
        outputs_data: Vec<String>,
        witnesses: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct CellDep {
        out_point: OutPoint,
    }

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct OutPoint {
        tx_hash: String,
        index: String,
    }

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct CellInput {
        previous_output: OutPoint,
        since: String,
    }

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct CellOutput {
        capacity: String,
        lock: Script,
        #[serde(rename = "type")]
        type_: Option<Script>,
    }

    #[derive(serde::Deserialize)]
    struct Script {
        args: String,
    }

    fn parse_hex_to_bytes(hex: &str) -> Vec<u8> {
        let hex = hex.strip_prefix("0x").unwrap_or(hex);
        hex::decode(hex).unwrap_or_default()
    }

    let client = reqwest::Client::new();
    let request = RpcRequest {
        jsonrpc: "2.0",
        method: "get_transaction",
        params: (tx_hash,),
        id: 1,
    };

    let response = client.post(rpc_url).json(&request).send().await.ok()?;
    let rpc_response: RpcResponse = response.json().await.ok()?;
    let tx = rpc_response.result?.transaction?;

    const MOLECULE_NUMBER_SIZE: usize = 4;
    const OUTPOINT_SIZE: usize = 36;
    const CELLINPUT_SIZE: usize = 44;

    let mut size = MOLECULE_NUMBER_SIZE * 3;

    let raw_tx_size = {
        let mut raw_size = MOLECULE_NUMBER_SIZE * 7;

        raw_size += MOLECULE_NUMBER_SIZE;
        raw_size += tx.cell_deps.len() * (OUTPOINT_SIZE + 1);

        raw_size += MOLECULE_NUMBER_SIZE;
        raw_size += tx.header_deps.len() * 32;

        raw_size += MOLECULE_NUMBER_SIZE;
        raw_size += tx.inputs.len() * CELLINPUT_SIZE;

        raw_size += MOLECULE_NUMBER_SIZE;
        for output in &tx.outputs {
            let lock_args = parse_hex_to_bytes(&output.lock.args);
            let lock_size = MOLECULE_NUMBER_SIZE + 32 + 1 + MOLECULE_NUMBER_SIZE + lock_args.len();

            let type_size = output.type_.as_ref().map_or(0, |type_script| {
                let type_args = parse_hex_to_bytes(&type_script.args);
                MOLECULE_NUMBER_SIZE + 32 + 1 + MOLECULE_NUMBER_SIZE + type_args.len()
            });

            let output_size = MOLECULE_NUMBER_SIZE * 4 + 8 + lock_size + type_size;
            raw_size += MOLECULE_NUMBER_SIZE + output_size;
        }

        raw_size += MOLECULE_NUMBER_SIZE;
        for output_data in &tx.outputs_data {
            let data = parse_hex_to_bytes(output_data);
            raw_size += MOLECULE_NUMBER_SIZE + data.len();
        }

        raw_size
    };

    size += raw_tx_size;

    size += MOLECULE_NUMBER_SIZE;
    for witness in &tx.witnesses {
        let witness_data = parse_hex_to_bytes(witness);
        size += MOLECULE_NUMBER_SIZE + witness_data.len();
    }

    Some(size as i32)
}

async fn get_transaction_detail(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<TransactionDetailResponse> {
    Err(ApiError::internal(
        "Transaction detail endpoint requires PostgreSQL - not yet implemented for ClickHouse",
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDepResponse {
    pub out_point_tx_hash: String,
    pub out_point_index: i32,
    pub dep_type: String,
}

async fn get_cell_deps(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<Vec<CellDepResponse>> {
    Err(ApiError::internal(
        "Cell deps endpoint requires PostgreSQL - not yet implemented for ClickHouse",
    ))
}

async fn get_cycles_status(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<CyclesStatusResponse> {
    Err(ApiError::internal(
        "Cycles status endpoint requires PostgreSQL - not yet implemented for ClickHouse",
    ))
}

async fn trigger_cycles_calculation(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<CyclesStatusResponse> {
    Err(ApiError::internal(
        "Cycles calculation endpoint requires PostgreSQL - not yet implemented for ClickHouse",
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecyclePhase {
    Pending,
    Committed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleBlockInfo {
    pub block_number: i64,
    pub block_hash: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionLifecycleResponse {
    pub hash: String,
    pub phase: LifecyclePhase,
    pub proposal_id: String,
    pub proposed_in: Option<LifecycleBlockInfo>,
    pub committed_in: Option<LifecycleBlockInfo>,
    pub commitment_distance: Option<i64>,
    pub commitment_window: CommitmentWindow,
    pub is_cellbase: bool,
    pub confirmations: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentWindow {
    pub close: i64,
    pub far: i64,
}

impl Default for CommitmentWindow {
    fn default() -> Self {
        Self { close: 2, far: 10 }
    }
}

async fn get_transaction_lifecycle(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<TransactionLifecycleResponse> {
    Err(ApiError::internal(
        "Transaction lifecycle endpoint requires PostgreSQL - not yet implemented for ClickHouse",
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxAssetTransferResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub event_index: i16,
    pub asset_category: String,
    pub asset_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    pub direction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_decimals: Option<i16>,
}

async fn get_transaction_asset_transfers(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<Vec<TxAssetTransferResponse>> {
    Err(ApiError::internal(
        "Asset transfers endpoint requires PostgreSQL - not yet implemented for ClickHouse",
    ))
}
