use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Router,
};
use ckbadger_common::cycles_task::{CyclesTaskResult, CyclesTaskStatus};
use ckbadger_common::dao::{
    is_genesis_special_burn_cell, GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED,
};
use ckbadger_common::parse_hex_to_bytes;
use ckbadger_common::sync::{SyncStatusData, SYNC_STATUS_REDIS_KEY};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{sleep, Instant};

use crate::cycles::{CyclesStatus, CyclesStatusResponse};
use crate::response::{
    decode_cursor, encode_cursor, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::utils::{ensure_derived_ready, script_to_address};
use crate::AppState;

/// (block_number, tx_hash, tx_index, tx_index_entry, block_hash)
type TxListEntry = (i64, Vec<u8>, i32, ckbadger_store::TxIndexEntry, Vec<u8>);
type RouteError = (axum::http::StatusCode, axum::Json<ApiError>);
type TxIoBundle = (
    Vec<TransactionInputResponse>,
    Vec<TransactionOutputResponse>,
    u128,
    u128,
    u128,
    u128,
    u128,
    Vec<String>,
    bool,
);

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

/// Helper: get the tx hash for a (block_num, tx_idx) from the CKB node's RocksDB.
fn get_tx_hash_from_ckb_store(
    ckb_store: &Option<Arc<ckb_store_reader::CkbChainReader>>,
    block_num: i64,
    tx_idx: i32,
) -> Option<Vec<u8>> {
    let store = ckb_store.as_ref()?;
    let block_hash_bytes = store.get_block_hash(block_num as u64)?;
    let block = store.get_block(&block_hash_bytes)?;
    let txs = block.transactions();
    let tx = txs.get(tx_idx as usize)?;
    Some(tx.hash().raw_data().to_vec())
}

async fn list_transactions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<TransactionResponse>> {
    let limit = params.limit.clamp(1, 100);

    // Get total count
    let total: i64 = if let Some(block_number) = params.block_number {
        let store = state.store.clone();
        tokio::task::spawn_blocking(move || -> i64 {
            store
                .get_block_header(block_number)
                .ok()
                .flatten()
                .map(|h| h.transactions_count as i64)
                .unwrap_or(0)
        })
        .await
        .unwrap_or(0)
    } else {
        match state
            .cache
            .get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY)
            .await
        {
            Some(status) => status.total_transactions,
            None => {
                let store = state.store.clone();
                tokio::task::spawn_blocking(move || {
                    store
                        .get_sync_status()
                        .map(|s| s.total_transactions)
                        .unwrap_or(0)
                })
                .await
                .unwrap_or(0)
            }
        }
    };

    let store = state.store.clone();
    let ckb_store = state.ckb_store.clone();

    if let Some(block_number) = params.block_number {
        // List transactions for a specific block
        let cursor = params.cursor.as_ref().and_then(|c| decode_cursor(c));
        let (_cursor_block, cursor_index) = cursor.unwrap_or((i64::MAX, i32::MAX));

        let store_c = store.clone();
        let ckb_store_c = ckb_store.clone();
        let all_txs = tokio::task::spawn_blocking(move || store_c.list_block_txs(block_number))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

        // Get block hash for responses
        let store_c = store.clone();
        let block_hash = tokio::task::spawn_blocking(move || {
            store_c
                .get_block_header(block_number)
                .ok()
                .flatten()
                .map(|h| h.hash)
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

        // Filter by cursor (tx_index < cursor_index), take limit + 1 for has_more check
        let filtered: Vec<_> = all_txs
            .into_iter()
            .filter(|(tx_idx, _)| *tx_idx < cursor_index)
            .take((limit + 1) as usize)
            .collect();

        let has_more = filtered.len() as i64 > limit;
        let page: Vec<_> = filtered.into_iter().take(limit as usize).collect();

        let next_cursor = if has_more {
            page.last()
                .map(|(tx_idx, _)| encode_cursor(block_number, *tx_idx))
        } else {
            None
        };

        let txs: Vec<TransactionResponse> = page
            .into_iter()
            .map(|(tx_idx, entry)| {
                let tx_hash = get_tx_hash_from_ckb_store(&ckb_store_c, block_number, tx_idx)
                    .map(|h| format!("0x{}", hex::encode(&h)))
                    .unwrap_or_else(|| "0x".to_string());

                let timestamp = chrono::DateTime::from_timestamp_millis(entry.timestamp)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default();

                TransactionResponse {
                    hash: tx_hash,
                    block_number,
                    block_hash: format!("0x{}", hex::encode(&block_hash)),
                    index: tx_idx,
                    inputs_count: entry.inputs_count as i32,
                    outputs_count: entry.outputs_count as i32,
                    fee: entry.fee.to_string(),
                    tx_size: Some(entry.tx_size),
                    cycles: entry.cycles,
                    is_cellbase: entry.is_cellbase,
                    timestamp,
                }
            })
            .collect();

        ok(CursorPaginatedResponse::new(txs, total, limit, next_cursor))
    } else {
        // List latest transactions (DESC order across blocks)
        let cursor = params.cursor.as_ref().and_then(|c| decode_cursor(c));
        let (cursor_block, cursor_index) = cursor.unwrap_or((i64::MAX, i32::MAX));

        let store_c = store.clone();
        let ckb_store_c = ckb_store.clone();
        let fetch_limit = (limit + 1) as usize;

        let txs_result =
            tokio::task::spawn_blocking(move || -> Result<Vec<TxListEntry>, anyhow::Error> {
                let mut results = Vec::with_capacity(fetch_limit);
                // Start from cursor_block and go backwards
                let blocks = store_c.list_blocks_desc(Some(cursor_block), fetch_limit * 2)?;

                for (block_num, header) in &blocks {
                    let block_txs = store_c.list_block_txs(*block_num)?;
                    // For the first block (cursor_block), filter by cursor_index
                    for (tx_idx, entry) in block_txs.into_iter().rev() {
                        if *block_num == cursor_block && tx_idx >= cursor_index {
                            continue;
                        }
                        // Look up tx hash from CKB store
                        let tx_hash = get_tx_hash_from_ckb_store(&ckb_store_c, *block_num, tx_idx)
                            .unwrap_or_default();
                        results.push((*block_num, header.hash.clone(), tx_idx, entry, tx_hash));
                        if results.len() >= fetch_limit {
                            break;
                        }
                    }
                    if results.len() >= fetch_limit {
                        break;
                    }
                }
                Ok(results)
            })
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let has_more = txs_result.len() as i64 > limit;
        let page: Vec<_> = txs_result.into_iter().take(limit as usize).collect();

        let next_cursor = if has_more {
            page.last()
                .map(|(block_num, _, tx_idx, _, _)| encode_cursor(*block_num, *tx_idx))
        } else {
            None
        };

        let txs: Vec<TransactionResponse> = page
            .into_iter()
            .map(|(block_num, block_hash, tx_idx, entry, tx_hash)| {
                let timestamp = chrono::DateTime::from_timestamp_millis(entry.timestamp)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default();

                TransactionResponse {
                    hash: if tx_hash.is_empty() {
                        "0x".to_string()
                    } else {
                        format!("0x{}", hex::encode(&tx_hash))
                    },
                    block_number: block_num,
                    block_hash: format!("0x{}", hex::encode(&block_hash)),
                    index: tx_idx,
                    inputs_count: entry.inputs_count as i32,
                    outputs_count: entry.outputs_count as i32,
                    fee: entry.fee.to_string(),
                    tx_size: Some(entry.tx_size),
                    cycles: entry.cycles,
                    is_cellbase: entry.is_cellbase,
                    timestamp,
                }
            })
            .collect();

        ok(CursorPaginatedResponse::new(txs, total, limit, next_cursor))
    }
}

async fn get_transaction(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionResponse> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let store = state.store.clone();
    let hash_c = hash_bytes.clone();
    let tx_result = tokio::task::spawn_blocking(move || store.get_tx_by_hash(&hash_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match tx_result {
        Some((block_num, tx_idx, entry)) => {
            // Get block hash
            let store = state.store.clone();
            let block_hash = tokio::task::spawn_blocking(move || {
                store
                    .get_block_header(block_num)
                    .ok()
                    .flatten()
                    .map(|h| h.hash)
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();

            let timestamp = chrono::DateTime::from_timestamp_millis(entry.timestamp)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();

            ok(TransactionResponse {
                hash: format!("0x{}", hex::encode(&hash_bytes)),
                block_number: block_num,
                block_hash: format!("0x{}", hex::encode(&block_hash)),
                index: tx_idx,
                inputs_count: entry.inputs_count as i32,
                outputs_count: entry.outputs_count as i32,
                fee: entry.fee.to_string(),
                tx_size: Some(entry.tx_size),
                cycles: entry.cycles,
                is_cellbase: entry.is_cellbase,
                timestamp,
            })
        }
        None => Err(ApiError::not_found("Transaction not found")),
    }
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
    pub r#type: Option<ScriptResponse>,
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
    pub witnesses: Vec<String>,
    pub witnesses_available: bool,
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

#[cfg(test)]
fn hash_type_byte_to_i16(byte: u8) -> i16 {
    match byte {
        0 => 0,
        1 => 1,
        2 => 2,
        4 => 4,
        _ => 0,
    }
}

fn parse_hash_type_label_to_i16(hash_type: &str) -> i16 {
    match hash_type {
        "data" => 0,
        "type" => 1,
        "data1" => 2,
        "data2" => 4,
        _ => 0,
    }
}

fn resolve_stored_input_type_hash_type(
    core_store: &ckbadger_store::CkbadgerStore,
    derived_store: &ckbadger_store::CkbadgerStore,
    type_script_hash: Option<&[u8]>,
    type_code_hash: &[u8],
) -> Result<String, RouteError> {
    if let Some(type_hash) = type_script_hash {
        match core_store.get_token(type_hash) {
            Ok(Some(token)) => return Ok(hash_type_to_string(token.hash_type as i16)),
            Ok(None) => {}
            Err(e) => {
                return Err(ApiError::internal(format!(
                    "failed to resolve token hash_type for type_script_hash=0x{}: {}",
                    hex::encode(type_hash),
                    e
                )));
            }
        }
    }

    match derived_store.get_script_info(type_code_hash) {
        Ok(Some(script)) => Ok(hash_type_to_string(script.hash_type as i16)),
        Ok(None) => Ok("unknown".to_string()),
        Err(e) => Err(ApiError::internal(format!(
            "failed to resolve script hash_type for type_code_hash=0x{}: {}",
            hex::encode(type_code_hash),
            e
        ))),
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

async fn fetch_witnesses_from_rpc(rpc_url: &str, tx_hash: &str) -> Option<Vec<String>> {
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
        witnesses: Vec<String>,
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

    Some(tx.witnesses)
}

async fn get_transaction_detail(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionDetailResponse> {
    ensure_derived_ready(state.as_ref())?;

    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let store = state.store.clone();
    let hash_c = hash_bytes.clone();
    let tx_result = tokio::task::spawn_blocking(move || store.get_tx_by_hash(&hash_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (block_number, tx_idx, entry) =
        tx_result.ok_or_else(|| ApiError::not_found("Transaction not found"))?;

    let tx_hash_hex = format!("0x{}", hex::encode(&hash_bytes));
    let is_cellbase = entry.is_cellbase;
    let tx_size = entry.tx_size;
    let cycles = entry.cycles;
    let inputs_count = entry.inputs_count as i32;
    let outputs_count = entry.outputs_count as i32;

    // Get block hash
    let store = state.store.clone();
    let block_hash = tokio::task::spawn_blocking(move || {
        store
            .get_block_header(block_number)
            .ok()
            .flatten()
            .map(|h| h.hash)
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    let timestamp = chrono::DateTime::from_timestamp_millis(entry.timestamp)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();

    // Get tip block for confirmations
    let tip_block = match state
        .cache
        .get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY)
        .await
    {
        Some(status) => status.tip_block_number,
        None => {
            let store = state.store.clone();
            tokio::task::spawn_blocking(move || {
                store
                    .get_sync_status()
                    .map(|s| s.tip_block_number)
                    .unwrap_or(0)
            })
            .await
            .unwrap_or(0)
        }
    };

    let confirmations = tip_block - block_number + 1;

    // Get tx_size: use stored value or fallback to RPC
    let final_tx_size = match tx_size {
        s if s > 0 => Some(s),
        _ => fetch_tx_size_from_rpc(&state.ckb_rpc_url, &tx_hash_hex).await,
    };

    // Read full transaction from CKB node's RocksDB for inputs/outputs
    let (
        inputs,
        outputs,
        inputs_capacity,
        outputs_capacity,
        inputs_occupied_capacity,
        outputs_occupied_capacity,
        computed_fee,
        witnesses,
        witnesses_available,
    ) = if let Some(ref ckb_store) = state.ckb_store {
        if hash_bytes.len() == 32 {
            let mut tx_hash_arr = [0u8; 32];
            tx_hash_arr.copy_from_slice(&hash_bytes);
            if let Some(tx_view) = ckb_store.get_transaction(&tx_hash_arr) {
                build_inputs_outputs_from_ckb(
                    &tx_view,
                    ckb_store,
                    &state.store,
                    &state.derived_store,
                    &state.ckb_network,
                    block_number,
                )?
            } else {
                empty_inputs_outputs()
            }
        } else {
            empty_inputs_outputs()
        }
    } else {
        empty_inputs_outputs()
    };

    // Use computed fee from inputs/outputs if available, otherwise use stored fee
    let fee = if computed_fee > 0 {
        computed_fee.to_string()
    } else {
        entry.fee.to_string()
    };

    let fee_rate = final_tx_size.map(|size| {
        if size > 0 {
            let fee_val: u128 = fee.parse().unwrap_or(0);
            let rate = (fee_val * 1000) / (size as u128);
            rate.to_string()
        } else {
            "0".to_string()
        }
    });

    let (witnesses, witnesses_available) = if witnesses_available {
        (witnesses, true)
    } else {
        match fetch_witnesses_from_rpc(&state.ckb_rpc_url, &tx_hash_hex).await {
            Some(fetched) => (fetched, true),
            None => (witnesses, false),
        }
    };

    ok(TransactionDetailResponse {
        hash: tx_hash_hex,
        block_number,
        block_hash: format!("0x{}", hex::encode(&block_hash)),
        index: tx_idx,
        inputs_count,
        outputs_count,
        fee,
        fee_rate,
        tx_size: final_tx_size,
        cycles,
        confirmations,
        is_cellbase,
        timestamp,
        inputs_capacity: inputs_capacity.to_string(),
        outputs_capacity: outputs_capacity.to_string(),
        inputs_occupied_capacity: inputs_occupied_capacity.to_string(),
        outputs_occupied_capacity: outputs_occupied_capacity.to_string(),
        inputs,
        outputs,
        witnesses,
        witnesses_available,
    })
}

fn empty_inputs_outputs() -> TxIoBundle {
    (vec![], vec![], 0, 0, 0, 0, 0, vec![], false)
}

/// Build inputs/outputs from CKB node's RocksDB transaction view.
fn build_inputs_outputs_from_ckb(
    tx_view: &ckb_types::core::TransactionView,
    ckb_store: &ckb_store_reader::CkbChainReader,
    core_store: &ckbadger_store::CkbadgerStore,
    derived_store: &ckbadger_store::CkbadgerStore,
    network: &str,
    block_number: i64,
) -> Result<TxIoBundle, RouteError> {
    let rpc_tx = ckb_store_reader::convert_transaction_view(tx_view);
    let witnesses = rpc_tx.witnesses.clone();

    let mut inputs_capacity: u128 = 0;
    let mut inputs_occupied_capacity: u128 = 0;

    let inputs: Vec<TransactionInputResponse> = rpc_tx
        .inputs
        .iter()
        .map(|input| -> Result<TransactionInputResponse, RouteError> {
            let prev_tx_hash_hex = &input.previous_output.tx_hash;
            let prev_index_hex = &input.previous_output.index;
            let prev_index = u32::from_str_radix(
                prev_index_hex.strip_prefix("0x").unwrap_or(prev_index_hex),
                16,
            )
            .unwrap_or(0);

            let since = &input.since;

            // Try to look up the previous output cell for capacity/lock info
            let prev_tx_hash_bytes = hex::decode(
                prev_tx_hash_hex
                    .strip_prefix("0x")
                    .unwrap_or(prev_tx_hash_hex),
            )
            .unwrap_or_default();

            let (capacity, lock, type_script, address) = if prev_tx_hash_bytes.len() == 32 {
                // Try live cells first, then consumed cells in our store
                let cell_info = core_store
                    .get_cell(&prev_tx_hash_bytes, prev_index as i16)
                    .ok()
                    .flatten()
                    .or_else(|| {
                        core_store
                            .get_consumed_cell(&prev_tx_hash_bytes, prev_index as i16)
                            .ok()
                            .flatten()
                    });

                match cell_info {
                    Some(info) => {
                        let cap = info.capacity as u128;
                        inputs_capacity += cap;

                        let lock_args_len = info.lock_args.len();
                        let lock_size = 8 + 32 + 1 + lock_args_len + info.data_size as usize;
                        inputs_occupied_capacity += lock_size as u128;

                        let lock_resp = ScriptResponse {
                            code_hash: format!("0x{}", hex::encode(&info.lock_code_hash)),
                            hash_type: hash_type_to_string(info.lock_hash_type),
                            args: format!("0x{}", hex::encode(&info.lock_args)),
                        };
                        let type_resp = info
                            .type_code_hash
                            .as_ref()
                            .map(|type_code_hash| -> Result<ScriptResponse, RouteError> {
                                Ok(ScriptResponse {
                                    code_hash: format!("0x{}", hex::encode(type_code_hash)),
                                    hash_type: resolve_stored_input_type_hash_type(
                                        core_store,
                                        derived_store,
                                        info.type_script_hash.as_deref(),
                                        type_code_hash,
                                    )?,
                                    args: format!(
                                        "0x{}",
                                        hex::encode(info.type_args.as_deref().unwrap_or(&[]))
                                    ),
                                })
                            })
                            .transpose()?;

                        let addr = script_to_address(
                            &info.lock_code_hash,
                            info.lock_hash_type,
                            &info.lock_args,
                            network,
                        )
                        .ok();

                        (Some(cap.to_string()), Some(lock_resp), type_resp, addr)
                    }
                    None => {
                        // Fallback: read from CKB node's RocksDB
                        let mut prev_hash = [0u8; 32];
                        prev_hash.copy_from_slice(&prev_tx_hash_bytes);
                        if let Some(prev_tx) = ckb_store.get_transaction(&prev_hash) {
                            let rpc_prev_tx = ckb_store_reader::convert_transaction_view(&prev_tx);
                            if let Some(output) = rpc_prev_tx.outputs.get(prev_index as usize) {
                                let cap = u64::from_str_radix(
                                    output
                                        .capacity
                                        .strip_prefix("0x")
                                        .unwrap_or(&output.capacity),
                                    16,
                                )
                                .unwrap_or(0);
                                inputs_capacity += cap as u128;

                                let code_hash = hex::decode(
                                    output
                                        .lock
                                        .code_hash
                                        .strip_prefix("0x")
                                        .unwrap_or(&output.lock.code_hash),
                                )
                                .unwrap_or_default();
                                let ht = parse_hash_type_label_to_i16(&output.lock.hash_type);
                                let args = hex::decode(
                                    output
                                        .lock
                                        .args
                                        .strip_prefix("0x")
                                        .unwrap_or(&output.lock.args),
                                )
                                .unwrap_or_default();

                                let lock_resp = ScriptResponse {
                                    code_hash: output.lock.code_hash.clone(),
                                    hash_type: output.lock.hash_type.clone(),
                                    args: output.lock.args.clone(),
                                };
                                let type_resp =
                                    output.type_.as_ref().map(|type_script| ScriptResponse {
                                        code_hash: type_script.code_hash.clone(),
                                        hash_type: type_script.hash_type.clone(),
                                        args: type_script.args.clone(),
                                    });

                                let addr = script_to_address(&code_hash, ht, &args, network).ok();

                                let data_len = ckb_store
                                    .get_cell_data(&prev_hash, prev_index)
                                    .map(|d| d.len())
                                    .unwrap_or(0);
                                let occ = 8 + 32 + 1 + args.len() + data_len;
                                inputs_occupied_capacity += occ as u128;

                                (Some(cap.to_string()), Some(lock_resp), type_resp, addr)
                            } else {
                                (None, None, None, None)
                            }
                        } else {
                            (None, None, None, None)
                        }
                    }
                }
            } else {
                (None, None, None, None)
            };

            Ok(TransactionInputResponse {
                previous_output: Some(PreviousOutput {
                    tx_hash: prev_tx_hash_hex.clone(),
                    index: prev_index as i32,
                }),
                since: since.clone(),
                capacity,
                lock,
                r#type: type_script,
                address,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut outputs_capacity: u128 = 0;
    let mut outputs_occupied_capacity: u128 = 0;

    let outputs: Vec<TransactionOutputResponse> = rpc_tx
        .outputs
        .iter()
        .enumerate()
        .map(|(output_idx, output)| {
            let cap_hex = &output.capacity;
            let cap =
                u64::from_str_radix(cap_hex.strip_prefix("0x").unwrap_or(cap_hex), 16).unwrap_or(0);
            outputs_capacity += cap as u128;

            let lock = &output.lock;
            let code_hash_bytes =
                hex::decode(lock.code_hash.strip_prefix("0x").unwrap_or(&lock.code_hash))
                    .unwrap_or_default();
            let ht = match lock.hash_type.as_str() {
                "data" => 0i16,
                "type" => 1i16,
                "data1" => 2i16,
                "data2" => 4i16,
                _ => 0i16,
            };
            let args_bytes =
                hex::decode(lock.args.strip_prefix("0x").unwrap_or(&lock.args)).unwrap_or_default();

            let lock_resp = ScriptResponse {
                code_hash: lock.code_hash.clone(),
                hash_type: lock.hash_type.clone(),
                args: lock.args.clone(),
            };

            let address = script_to_address(&code_hash_bytes, ht, &args_bytes, network).ok();

            let type_resp = output.type_.as_ref().map(|t| ScriptResponse {
                code_hash: t.code_hash.clone(),
                hash_type: t.hash_type.clone(),
                args: t.args.clone(),
            });

            // Calculate occupied capacity
            let type_size = output.type_.as_ref().map_or(0, |t| {
                let type_args =
                    hex::decode(t.args.strip_prefix("0x").unwrap_or(&t.args)).unwrap_or_default();
                32 + 1 + type_args.len()
            });

            // Get data size from CKB store
            let tx_hash: Vec<u8> = tx_view.hash().raw_data().to_vec();
            let data_size = if tx_hash.len() == 32 {
                let mut th = [0u8; 32];
                th.copy_from_slice(&tx_hash);
                ckb_store
                    .get_cell_data(&th, output_idx as u32)
                    .map(|d| d.len())
                    .unwrap_or(0)
            } else {
                0
            };

            let occ = 8 + 32 + 1 + args_bytes.len() + type_size + data_size;
            outputs_occupied_capacity += occ as u128;

            let is_satoshi = is_genesis_special_burn_cell(&args_bytes, block_number);
            let (cell_type, virtual_occupied_capacity) = if is_satoshi {
                (
                    Some("genesis_special_burn".to_string()),
                    Some(GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED.to_string()),
                )
            } else {
                (None, None)
            };

            TransactionOutputResponse {
                capacity: cap.to_string(),
                occupied_capacity: occ as i64,
                virtual_occupied_capacity,
                cell_type,
                lock: Some(lock_resp),
                r#type: type_resp,
                address,
            }
        })
        .collect();

    let is_cellbase = rpc_tx.inputs.first().is_some_and(|input| {
        input.previous_output.tx_hash
            == "0x0000000000000000000000000000000000000000000000000000000000000000"
    });

    // Compute fee strictly: non-cellbase tx must satisfy inputs >= outputs.
    let computed_fee = if is_cellbase {
        0
    } else {
        inputs_capacity.checked_sub(outputs_capacity).ok_or_else(|| {
            ApiError::internal(format!(
                "transaction inputs/outputs invariant broken at block {}: tx_hash=0x{}, inputs_capacity={}, outputs_capacity={}",
                block_number,
                hex::encode(tx_view.hash().raw_data()),
                inputs_capacity,
                outputs_capacity
            ))
        })?
    };

    Ok((
        inputs,
        outputs,
        inputs_capacity,
        outputs_capacity,
        inputs_occupied_capacity,
        outputs_occupied_capacity,
        computed_fee,
        witnesses,
        true,
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
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Vec<CellDepResponse>> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    // Read cell_deps directly from CKB's RocksDB
    if let Some(ref store) = state.ckb_store {
        if hash_bytes.len() == 32 {
            let mut tx_hash = [0u8; 32];
            tx_hash.copy_from_slice(&hash_bytes);
            if let Some(tx_view) = store.get_transaction(&tx_hash) {
                let rpc_tx = ckb_store_reader::convert_transaction_view(&tx_view);
                let cell_deps: Vec<CellDepResponse> = rpc_tx
                    .cell_deps
                    .into_iter()
                    .map(|dep| CellDepResponse {
                        out_point_tx_hash: dep.out_point.tx_hash,
                        out_point_index: {
                            let idx_str = dep
                                .out_point
                                .index
                                .strip_prefix("0x")
                                .unwrap_or(&dep.out_point.index);
                            i32::from_str_radix(idx_str, 16).unwrap_or(0)
                        },
                        dep_type: dep.dep_type,
                    })
                    .collect();
                return ok(cell_deps);
            }
        }
    }

    // Fallback: RocksDB not available or tx not found
    ok(vec![])
}

async fn get_cycles_status(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<CyclesStatusResponse> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let (db_cycles, is_cellbase) = match load_tx_cycles_state(&state, &hash_bytes).await? {
        Some(state) => state,
        None => {
            return ok(CyclesStatusResponse {
                status: CyclesStatus::NotFound,
                cycles: None,
                error: Some("Transaction not found".to_string()),
            });
        }
    };

    if is_cellbase {
        return ok(CyclesStatusResponse {
            status: CyclesStatus::Done,
            cycles: Some(0),
            error: None,
        });
    }

    match db_cycles {
        Some(cycles) if cycles > 0 => ok(CyclesStatusResponse {
            status: CyclesStatus::Done,
            cycles: Some(cycles),
            error: None,
        }),
        Some(-1) => ok(CyclesStatusResponse {
            status: CyclesStatus::Failed,
            cycles: None,
            error: Some("Calculation failed".to_string()),
        }),
        _ => {
            if !state.cycles_client.is_enabled() {
                return ok(CyclesStatusResponse {
                    status: CyclesStatus::Failed,
                    cycles: None,
                    error: Some(
                        "Cycles task dispatch unavailable: Redis is not configured".to_string(),
                    ),
                });
            }

            match state.cycles_client.get_task_result(&hash).await {
                Ok(Some(result)) => ok(cycles_response_from_task(result)),
                Ok(None) => ok(cycles_enqueue_response(
                    state.cycles_client.enqueue_task(&hash).await,
                )),
                Err(e) => ok(CyclesStatusResponse {
                    status: CyclesStatus::Failed,
                    cycles: None,
                    error: Some(e),
                }),
            }
        }
    }
}

async fn trigger_cycles_calculation(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<CyclesStatusResponse> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let (db_cycles, is_cellbase) = match load_tx_cycles_state(&state, &hash_bytes).await? {
        Some(state) => state,
        None => {
            return ok(CyclesStatusResponse {
                status: CyclesStatus::NotFound,
                cycles: None,
                error: Some("Transaction not found".to_string()),
            });
        }
    };

    if is_cellbase {
        return ok(CyclesStatusResponse {
            status: CyclesStatus::Done,
            cycles: Some(0),
            error: None,
        });
    }

    match db_cycles {
        Some(cycles) if cycles > 0 => ok(CyclesStatusResponse {
            status: CyclesStatus::Done,
            cycles: Some(cycles),
            error: None,
        }),
        Some(-1) => ok(CyclesStatusResponse {
            status: CyclesStatus::Failed,
            cycles: None,
            error: Some("Calculation previously failed".to_string()),
        }),
        _ => {
            if let Ok(Some(result)) = state.cycles_client.get_task_result(&hash).await {
                return ok(cycles_response_from_task(result));
            }

            if let Err(e) = state.cycles_client.enqueue_task(&hash).await {
                return ok(CyclesStatusResponse {
                    status: CyclesStatus::Failed,
                    cycles: None,
                    error: Some(e),
                });
            }

            ok(wait_cycles_result(&state, &hash, &hash_bytes).await?)
        }
    }
}

async fn load_tx_cycles_state(
    state: &Arc<AppState>,
    hash_bytes: &[u8],
) -> Result<Option<(Option<i64>, bool)>, RouteError> {
    let store = state.store.clone();
    let hash_c = hash_bytes.to_vec();
    let row = tokio::task::spawn_blocking(move || store.get_tx_by_hash(&hash_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(row.map(|(_, _, entry)| (entry.cycles, entry.is_cellbase)))
}

async fn wait_cycles_result(
    state: &Arc<AppState>,
    hash: &str,
    hash_bytes: &[u8],
) -> Result<CyclesStatusResponse, RouteError> {
    let deadline = Instant::now() + state.cycles_client.wait_timeout();

    loop {
        match load_tx_cycles_state(state, hash_bytes).await? {
            Some((Some(cycles), _)) if cycles > 0 => {
                return Ok(CyclesStatusResponse {
                    status: CyclesStatus::Done,
                    cycles: Some(cycles),
                    error: None,
                });
            }
            Some((Some(-1), _)) => {
                return Ok(CyclesStatusResponse {
                    status: CyclesStatus::Failed,
                    cycles: None,
                    error: Some("Calculation failed".to_string()),
                });
            }
            Some((_cycles, true)) => {
                return Ok(CyclesStatusResponse {
                    status: CyclesStatus::Done,
                    cycles: Some(0),
                    error: None,
                });
            }
            Some(_) => {}
            None => {
                return Ok(CyclesStatusResponse {
                    status: CyclesStatus::NotFound,
                    cycles: None,
                    error: Some("Transaction not found".to_string()),
                });
            }
        }

        match state.cycles_client.get_task_result(hash).await {
            Ok(Some(result)) => return Ok(cycles_response_from_task(result)),
            Ok(None) => {}
            Err(e) => {
                return Ok(CyclesStatusResponse {
                    status: CyclesStatus::Failed,
                    cycles: None,
                    error: Some(e),
                });
            }
        }

        if Instant::now() >= deadline {
            return Ok(CyclesStatusResponse {
                status: CyclesStatus::Calculating,
                cycles: None,
                error: None,
            });
        }

        sleep(state.cycles_client.poll_interval()).await;
    }
}

fn cycles_response_from_task(result: CyclesTaskResult) -> CyclesStatusResponse {
    match result.status {
        CyclesTaskStatus::Done => CyclesStatusResponse {
            status: CyclesStatus::Done,
            cycles: result.cycles,
            error: None,
        },
        CyclesTaskStatus::Failed => CyclesStatusResponse {
            status: CyclesStatus::Failed,
            cycles: None,
            error: result
                .error
                .or_else(|| Some("Calculation failed".to_string())),
        },
        CyclesTaskStatus::NotFound => CyclesStatusResponse {
            status: CyclesStatus::NotFound,
            cycles: None,
            error: result
                .error
                .or_else(|| Some("Transaction not found".to_string())),
        },
    }
}

fn cycles_enqueue_response(enqueue_result: Result<(), String>) -> CyclesStatusResponse {
    match enqueue_result {
        Ok(()) => CyclesStatusResponse {
            status: CyclesStatus::Queued,
            cycles: None,
            error: None,
        },
        Err(e) => CyclesStatusResponse {
            status: CyclesStatus::Failed,
            cycles: None,
            error: Some(e),
        },
    }
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
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionLifecycleResponse> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let short_hash = if hash_bytes.len() >= 10 {
        hash_bytes[..10].to_vec()
    } else {
        return Err(ApiError::bad_request("Transaction hash too short"));
    };

    // Query transaction info from store
    let store = state.store.clone();
    let hash_c = hash_bytes.clone();
    let tx_result = tokio::task::spawn_blocking(move || store.get_tx_by_hash(&hash_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (commit_block_number, is_cellbase, commit_timestamp) = match tx_result {
        Some((block_num, _, entry)) => (block_num, entry.is_cellbase, entry.timestamp),
        None => {
            return ok(TransactionLifecycleResponse {
                hash: format!("0x{}", hex::encode(&hash_bytes)),
                phase: LifecyclePhase::Pending,
                proposal_id: format!("0x{}", hex::encode(&short_hash)),
                proposed_in: None,
                committed_in: None,
                commitment_distance: None,
                commitment_window: CommitmentWindow::default(),
                is_cellbase: false,
                confirmations: None,
            });
        }
    };

    // Get block hash
    let store = state.store.clone();
    let commit_block_hash = tokio::task::spawn_blocking(move || {
        store
            .get_block_header(commit_block_number)
            .ok()
            .flatten()
            .map(|h| h.hash)
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    let hash_hex = format!("0x{}", hex::encode(&hash_bytes));
    let proposal_id_hex = format!("0x{}", hex::encode(&short_hash));

    let commit_ts_str = chrono::DateTime::from_timestamp_millis(commit_timestamp)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();

    // Get sync tip
    let tip = match state
        .cache
        .get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY)
        .await
    {
        Some(status) => status.tip_block_number,
        None => {
            let store = state.store.clone();
            tokio::task::spawn_blocking(move || {
                store
                    .get_sync_status()
                    .map(|s| s.tip_block_number)
                    .unwrap_or(0)
            })
            .await
            .unwrap_or(0)
        }
    };

    if is_cellbase {
        return ok(TransactionLifecycleResponse {
            hash: hash_hex,
            phase: LifecyclePhase::Committed,
            proposal_id: proposal_id_hex,
            proposed_in: None,
            committed_in: Some(LifecycleBlockInfo {
                block_number: commit_block_number,
                block_hash: format!("0x{}", hex::encode(&commit_block_hash)),
                timestamp: commit_ts_str,
            }),
            commitment_distance: None,
            commitment_window: CommitmentWindow::default(),
            is_cellbase: true,
            confirmations: Some(tip - commit_block_number + 1),
        });
    }

    // Look for proposal block in CKB node's RocksDB
    // A tx committed in block C must be proposed in block P where: C - 10 <= P <= C - 2
    let proposed_in = if let Some(ref ckb_store) = state.ckb_store {
        let store_c = state.store.clone();
        let ckb_store_c = ckb_store.clone();
        let short_hash_c = short_hash.clone();
        tokio::task::spawn_blocking(move || -> Option<(i64, Vec<u8>, i64)> {
            if commit_block_number < 2 {
                return None;
            }
            let start = if commit_block_number > 10 {
                commit_block_number - 10
            } else {
                0
            };
            let end = commit_block_number - 2;
            if start > end {
                return None;
            }
            for bn in start..=end {
                if let Ok(Some(header)) = store_c.get_block_header(bn) {
                    if header.hash.len() == 32 {
                        let mut hash = [0u8; 32];
                        hash.copy_from_slice(&header.hash);
                        if let Some(block) = ckb_store_c.get_block(&hash) {
                            for proposal_id in block.data().proposals().into_iter() {
                                let proposal_bytes: Vec<u8> = proposal_id.raw_data().to_vec();
                                if proposal_bytes == short_hash_c {
                                    return Some((bn, header.hash.clone(), header.timestamp));
                                }
                            }
                        }
                    }
                }
            }
            None
        })
        .await
        .unwrap_or(None)
    } else {
        None
    };

    let (proposed_in_info, commitment_distance) = match proposed_in {
        Some((proposal_block, proposal_hash, proposal_ts)) => {
            let ts = chrono::DateTime::from_timestamp_millis(proposal_ts)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();
            (
                Some(LifecycleBlockInfo {
                    block_number: proposal_block,
                    block_hash: format!("0x{}", hex::encode(&proposal_hash)),
                    timestamp: ts,
                }),
                Some(commit_block_number - proposal_block),
            )
        }
        None => (None, None),
    };

    ok(TransactionLifecycleResponse {
        hash: hash_hex,
        phase: LifecyclePhase::Committed,
        proposal_id: proposal_id_hex,
        proposed_in: proposed_in_info,
        committed_in: Some(LifecycleBlockInfo {
            block_number: commit_block_number,
            block_hash: format!("0x{}", hex::encode(&commit_block_hash)),
            timestamp: commit_ts_str,
        }),
        commitment_distance,
        commitment_window: CommitmentWindow::default(),
        is_cellbase: false,
        confirmations: Some(tip - commit_block_number + 1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_common::cycles_task::{CyclesTaskResult, CyclesTaskStatus};

    #[test]
    fn test_transaction_response_serialization() {
        let resp = TransactionResponse {
            hash: "0xabc".to_string(),
            block_number: 100,
            block_hash: "0xdef".to_string(),
            index: 0,
            inputs_count: 1,
            outputs_count: 2,
            fee: "1000".to_string(),
            tx_size: Some(200),
            cycles: Some(5000),
            is_cellbase: false,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["blockNumber"], 100);
        assert_eq!(json["inputsCount"], 1);
        assert_eq!(json["outputsCount"], 2);
        assert_eq!(json["isCellbase"], false);
    }

    #[test]
    fn test_list_params_defaults() {
        let params: ListParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.limit, 20);
        assert!(params.block_number.is_none());
        assert!(params.cursor.is_none());
    }

    #[test]
    fn test_hash_type_to_string() {
        assert_eq!(hash_type_to_string(0), "data");
        assert_eq!(hash_type_to_string(1), "type");
        assert_eq!(hash_type_to_string(2), "data1");
        assert_eq!(hash_type_to_string(4), "data2");
        assert_eq!(hash_type_to_string(99), "unknown");
    }

    #[test]
    fn test_hash_type_byte_to_i16() {
        assert_eq!(hash_type_byte_to_i16(0), 0);
        assert_eq!(hash_type_byte_to_i16(1), 1);
        assert_eq!(hash_type_byte_to_i16(2), 2);
        assert_eq!(hash_type_byte_to_i16(4), 4);
        assert_eq!(hash_type_byte_to_i16(255), 0);
    }

    #[test]
    fn test_transaction_input_response_serializes_type_script() {
        let input = TransactionInputResponse {
            previous_output: Some(PreviousOutput {
                tx_hash: "0x01".to_string(),
                index: 0,
            }),
            since: "0x0".to_string(),
            capacity: Some("100".to_string()),
            lock: Some(ScriptResponse {
                code_hash: "0x02".to_string(),
                hash_type: "type".to_string(),
                args: "0x".to_string(),
            }),
            r#type: Some(ScriptResponse {
                code_hash: "0x03".to_string(),
                hash_type: "data1".to_string(),
                args: "0x11".to_string(),
            }),
            address: Some("ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgpl6m0j".to_string()),
        };

        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["type"]["codeHash"], "0x03");
        assert_eq!(json["type"]["hashType"], "data1");
    }

    #[test]
    fn test_transaction_detail_response_serializes_witness_fields() {
        let detail = TransactionDetailResponse {
            hash: "0xabc".to_string(),
            block_number: 100,
            block_hash: "0xdef".to_string(),
            index: 0,
            inputs_count: 1,
            outputs_count: 1,
            fee: "42".to_string(),
            fee_rate: Some("1000".to_string()),
            tx_size: Some(123),
            cycles: Some(456),
            confirmations: 7,
            is_cellbase: false,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            inputs_capacity: "100".to_string(),
            outputs_capacity: "58".to_string(),
            inputs_occupied_capacity: "10".to_string(),
            outputs_occupied_capacity: "9".to_string(),
            inputs: vec![],
            outputs: vec![],
            witnesses: vec!["0x".to_string(), "0x1234".to_string()],
            witnesses_available: true,
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["witnessesAvailable"], true);
        assert_eq!(json["witnesses"][0], "0x");
        assert_eq!(json["witnesses"][1], "0x1234");
    }

    #[test]
    fn test_commitment_window_default() {
        let window = CommitmentWindow::default();
        assert_eq!(window.close, 2);
        assert_eq!(window.far, 10);
    }

    #[test]
    fn test_cell_dep_response_serialization() {
        let resp = CellDepResponse {
            out_point_tx_hash: "0xabc".to_string(),
            out_point_index: 0,
            dep_type: "code".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["outPointTxHash"], "0xabc");
        assert_eq!(json["depType"], "code");
    }

    #[test]
    fn test_lifecycle_phase_serialization() {
        let pending = serde_json::to_value(&LifecyclePhase::Pending).unwrap();
        assert_eq!(pending, "pending");
        let committed = serde_json::to_value(&LifecyclePhase::Committed).unwrap();
        assert_eq!(committed, "committed");
    }

    #[test]
    fn test_cycles_response_from_task_done() {
        let response = cycles_response_from_task(CyclesTaskResult {
            status: CyclesTaskStatus::Done,
            cycles: Some(42),
            error: None,
            updated_at: 1_700_000_000,
        });
        assert_eq!(response.status, CyclesStatus::Done);
        assert_eq!(response.cycles, Some(42));
        assert!(response.error.is_none());
    }

    #[test]
    fn test_cycles_response_from_task_failed_uses_default_message() {
        let response = cycles_response_from_task(CyclesTaskResult {
            status: CyclesTaskStatus::Failed,
            cycles: None,
            error: None,
            updated_at: 1_700_000_000,
        });
        assert_eq!(response.status, CyclesStatus::Failed);
        assert_eq!(response.cycles, None);
        assert!(response
            .error
            .unwrap_or_default()
            .contains("Calculation failed"));
    }

    #[test]
    fn test_cycles_enqueue_response_queued() {
        let response = cycles_enqueue_response(Ok(()));
        assert_eq!(response.status, CyclesStatus::Queued);
        assert_eq!(response.cycles, None);
        assert!(response.error.is_none());
    }

    #[test]
    fn test_cycles_enqueue_response_failed() {
        let response = cycles_enqueue_response(Err("enqueue failed".to_string()));
        assert_eq!(response.status, CyclesStatus::Failed);
        assert_eq!(response.cycles, None);
        assert_eq!(response.error.unwrap_or_default(), "enqueue failed");
    }
}
