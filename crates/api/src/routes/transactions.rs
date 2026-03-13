use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Router,
};
use ckbadger_common::cycles_task::{CyclesTaskResult, CyclesTaskStatus};
use ckbadger_common::dao::{
    is_genesis_special_burn_cell, GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::time::{sleep, Instant};

use crate::cache::InMemoryCache;
use crate::cycles::{CyclesStatus, CyclesStatusResponse};
use crate::response::{
    decode_cursor, default_limit, encode_cursor, hash_type_to_str, ok, ApiError, ApiResult,
    ApiRouteError, CursorPaginatedResponse, ScriptResponse,
};
use crate::routes::tx_lookup::{fetch_transaction_lookup, pending_transaction_resource_error};
use crate::utils::script_to_address;
use crate::AppState;

/// (block_number, tx_hash, tx_index, tx_index_entry, block_hash)
type TxListEntry = (i64, Vec<u8>, i32, ckbadger_store::TxIndexEntry, Vec<u8>);
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

#[derive(Debug, Clone)]
struct PendingTxIoBundle {
    inputs: Vec<TransactionInputResponse>,
    outputs: Vec<TransactionOutputResponse>,
    inputs_capacity: Option<u128>,
    outputs_capacity: u128,
    inputs_used_capacity: Option<u128>,
    outputs_used_capacity: u128,
    computed_fee: Option<u128>,
    witnesses: Vec<String>,
    witnesses_available: bool,
}
const DAO_TYPE_CODE_HASH_HEX: &str =
    "82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
const TX_BLOCK_HASHES_CACHE_TTL: StdDuration = StdDuration::from_secs(30);

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

/// Helper: get all tx hashes in a block from the CKB node's RocksDB.
fn get_block_tx_hashes_from_ckb_store(
    ckb_store: &Option<Arc<ckb_store_reader::CkbChainReader>>,
    block_num: i64,
) -> Option<Vec<Vec<u8>>> {
    let store = ckb_store.as_ref()?;
    let block_hash_bytes = store.get_block_hash(block_num as u64)?;
    let block = store.get_block(&block_hash_bytes)?;
    Some(
        block
            .transactions()
            .into_iter()
            .map(|tx| tx.hash().raw_data().to_vec())
            .collect(),
    )
}

fn tx_hash_from_prefetched_hashes(
    block_tx_hashes: &Option<Vec<Vec<u8>>>,
    tx_idx: i32,
) -> Option<Vec<u8>> {
    let idx = usize::try_from(tx_idx).ok()?;
    block_tx_hashes.as_ref()?.get(idx).cloned()
}

fn tx_block_hashes_cache_key(block_num: i64) -> String {
    format!("transactions:block_tx_hashes:{block_num}")
}

fn get_block_tx_hashes_cached_with_fetch<F>(
    mem_cache: &InMemoryCache,
    block_num: i64,
    fetch: F,
) -> Option<Vec<Vec<u8>>>
where
    F: FnOnce(i64) -> Option<Vec<Vec<u8>>>,
{
    let cache_key = tx_block_hashes_cache_key(block_num);
    if let Some(cached) = mem_cache.get::<Vec<Vec<u8>>>(&cache_key) {
        return Some(cached);
    }
    let fetched = fetch(block_num)?;
    mem_cache.set(&cache_key, &fetched, TX_BLOCK_HASHES_CACHE_TTL);
    Some(fetched)
}

fn get_block_tx_hashes_cached(
    mem_cache: &InMemoryCache,
    ckb_store: &Option<Arc<ckb_store_reader::CkbChainReader>>,
    block_num: i64,
) -> Option<Vec<Vec<u8>>> {
    get_block_tx_hashes_cached_with_fetch(mem_cache, block_num, |bn| {
        get_block_tx_hashes_from_ckb_store(ckb_store, bn)
    })
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
        state
            .store
            .get_sync_status()
            .map_err(|e| ApiError::internal(format!("sync status unavailable: {}", e)))?
            .total_transactions
    };

    let store = state.store.clone();
    let ckb_store = state.ckb_store.clone();
    let mem_cache = state.mem_cache.clone();

    if let Some(block_number) = params.block_number {
        // List transactions for a specific block
        let cursor = params.cursor.as_ref().and_then(|c| decode_cursor(c));
        let (_cursor_block, cursor_index) = cursor.unwrap_or((i64::MAX, i32::MAX));
        let fetch_limit = (limit + 1) as usize;

        let store_c = store.clone();
        let ckb_store_c = ckb_store.clone();
        let mem_cache_c = mem_cache.clone();
        let (page_txs, block_tx_hashes) = tokio::task::spawn_blocking(move || {
            // Use range-limited query: only fetch txs with tx_idx < cursor_index,
            // limited to the page size we need. Avoids loading all block transactions.
            let page_txs =
                store_c.list_block_txs_before(block_number, cursor_index, fetch_limit)?;
            let block_tx_hashes =
                get_block_tx_hashes_cached(&mem_cache_c, &ckb_store_c, block_number);
            Ok::<_, anyhow::Error>((page_txs, block_tx_hashes))
        })
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

        let has_more = page_txs.len() as i64 > limit;
        let page: Vec<_> = page_txs.into_iter().take(limit as usize).collect();

        let next_cursor = if has_more {
            page.last()
                .map(|(tx_idx, _)| encode_cursor(block_number, *tx_idx))
        } else {
            None
        };

        let txs: Vec<TransactionResponse> = page
            .into_iter()
            .map(|(tx_idx, entry)| {
                let tx_hash = tx_hash_from_prefetched_hashes(&block_tx_hashes, tx_idx)
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
        let mem_cache_c = mem_cache.clone();
        let fetch_limit = (limit + 1) as usize;

        let txs_result =
            tokio::task::spawn_blocking(move || -> Result<Vec<TxListEntry>, anyhow::Error> {
                let mut results = Vec::with_capacity(fetch_limit);
                // Fetch blocks in small batches to avoid loading many more blocks than needed.
                // Most blocks have 1-3 transactions, so fetch_limit blocks is usually enough.
                let block_batch_size = fetch_limit.max(4);
                let mut next_block_cursor = Some(cursor_block);

                while results.len() < fetch_limit {
                    let from_block = match next_block_cursor {
                        Some(b) => b,
                        None => break, // no more blocks
                    };
                    let blocks =
                        store_c.list_blocks_desc(Some(from_block), block_batch_size + 1)?;
                    if blocks.is_empty() {
                        break;
                    }

                    // Determine next cursor for the next batch (if we need more blocks)
                    next_block_cursor = if blocks.len() > block_batch_size {
                        blocks.last().map(|(bn, _)| *bn)
                    } else {
                        None
                    };

                    for (block_num, header) in &blocks {
                        // For the cursor block, only fetch txs before the cursor index
                        let block_txs = if *block_num == cursor_block {
                            store_c.list_block_txs_before(*block_num, cursor_index, fetch_limit)?
                        } else {
                            store_c.list_block_txs(*block_num)?
                        };
                        let block_tx_hashes =
                            get_block_tx_hashes_cached(&mem_cache_c, &ckb_store_c, *block_num);
                        for (tx_idx, entry) in block_txs.into_iter().rev() {
                            let tx_hash = tx_hash_from_prefetched_hashes(&block_tx_hashes, tx_idx)
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
        None => {
            let ckb_block_number =
                lookup_tx_block_number_in_ckb_store(state.ckb_store.as_ref(), &hash_bytes);
            Err(missing_tx_lookup_error(&hash_bytes, ckb_block_number))
        }
    }
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
    pub used_capacity: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_used_capacity: Option<String>,
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
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i32>,
    pub inputs_count: i32,
    pub outputs_count: i32,
    pub fee: String,
    pub fee_rate: Option<String>,
    pub tx_size: Option<i32>,
    pub cycles: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<i64>,
    pub is_cellbase: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs_used_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs_used_capacity: Option<String>,
    pub inputs: Vec<TransactionInputResponse>,
    pub outputs: Vec<TransactionOutputResponse>,
    pub witnesses: Vec<String>,
    pub witnesses_available: bool,
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

fn parse_hash_type_label_to_i16(hash_type: &str) -> Result<i16, ApiRouteError> {
    match hash_type {
        "data" => Ok(0),
        "type" => Ok(1),
        "data1" => Ok(2),
        "data2" => Ok(4),
        other => Err(ApiError::internal(format!(
            "unknown script hash_type label in CKB store: '{}'",
            other
        ))),
    }
}

fn decode_hex_bytes_with_context(
    raw: &str,
    field: &str,
    context: &str,
    expected_len: Option<usize>,
) -> Result<Vec<u8>, ApiRouteError> {
    let bytes = hex::decode(raw.strip_prefix("0x").unwrap_or(raw)).map_err(|e| {
        ApiError::internal(format!(
            "invalid hex for {} while {}: value='{}', error={}",
            field, context, raw, e
        ))
    })?;
    if let Some(expected) = expected_len {
        if bytes.len() != expected {
            return Err(ApiError::internal(format!(
                "invalid byte length for {} while {}: expected {}, got {}",
                field,
                context,
                expected,
                bytes.len()
            )));
        }
    }
    Ok(bytes)
}

fn parse_u64_hex_field_with_context(
    raw: &str,
    field: &str,
    context: &str,
) -> Result<u64, ApiRouteError> {
    u64::from_str_radix(raw.strip_prefix("0x").unwrap_or(raw), 16).map_err(|e| {
        ApiError::internal(format!(
            "invalid hex u64 for {} while {}: value='{}', error={}",
            field, context, raw, e
        ))
    })
}

fn is_dao_type_code_hash_hex(code_hash: &str) -> bool {
    code_hash
        .strip_prefix("0x")
        .unwrap_or(code_hash)
        .eq_ignore_ascii_case(DAO_TYPE_CODE_HASH_HEX)
}

fn compute_tx_fee_from_io(
    inputs_capacity: u128,
    outputs_capacity: u128,
    is_cellbase: bool,
    has_dao_type_input: bool,
    block_number: i64,
    tx_hash: &[u8],
) -> Result<u128, ApiRouteError> {
    if is_cellbase {
        return Ok(0);
    }

    if let Some(fee) = inputs_capacity.checked_sub(outputs_capacity) {
        return Ok(fee);
    }

    if has_dao_type_input {
        return Ok(0);
    }

    Err(ApiError::internal(format!(
        "transaction inputs/outputs invariant broken at block {}: tx_hash=0x{}, inputs_capacity={}, outputs_capacity={}",
        block_number,
        hex::encode(tx_hash),
        inputs_capacity,
        outputs_capacity
    )))
}

fn occupied_capacity_bytes(
    lock_args_len: usize,
    type_args_len: Option<usize>,
    data_size: usize,
) -> usize {
    let type_size = type_args_len.map_or(0, |len| 32 + 1 + len);
    8 + 32 + 1 + lock_args_len + type_size + data_size
}

fn resolve_stored_input_type_hash_type(
    core_store: &ckbadger_store::CkbadgerStore,
    store: &ckbadger_store::CkbadgerStore,
    type_script_hash: Option<&[u8]>,
    type_code_hash: &[u8],
) -> Result<String, ApiRouteError> {
    if let Some(type_hash) = type_script_hash {
        match core_store.get_token(type_hash) {
            Ok(Some(token)) => return Ok(hash_type_to_str(token.hash_type as i16).to_string()),
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

    match store.get_script_info(type_code_hash) {
        Ok(Some(script)) => Ok(hash_type_to_str(script.hash_type as i16).to_string()),
        Ok(None) => Ok("unknown".to_string()),
        Err(e) => Err(ApiError::internal(format!(
            "failed to resolve script hash_type for type_code_hash=0x{}: {}",
            hex::encode(type_code_hash),
            e
        ))),
    }
}

async fn get_transaction_detail(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionDetailResponse> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let store = state.store.clone();
    let hash_c = hash_bytes.clone();
    let tx_result = tokio::task::spawn_blocking(move || store.get_tx_by_hash(&hash_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let Some((block_number, tx_idx, entry)) = tx_result else {
        let tx_lookup = fetch_transaction_lookup(&state.ckb_rpc_url, &hash)
            .await
            .map_err(ApiError::internal)?;

        let Some(tx_lookup) = tx_lookup else {
            let ckb_block_number =
                lookup_tx_block_number_in_ckb_store(state.ckb_store.as_ref(), &hash_bytes);
            return Err(missing_tx_lookup_error(&hash_bytes, ckb_block_number));
        };

        if !tx_lookup.is_pending_like() {
            let ckb_block_number =
                lookup_tx_block_number_in_ckb_store(state.ckb_store.as_ref(), &hash_bytes);
            return Err(missing_tx_lookup_error(&hash_bytes, ckb_block_number));
        }

        let Some(rpc_tx) = tx_lookup.transaction.as_ref() else {
            return Err(ApiError::internal(format!(
                "pending transaction {} missing JSON transaction body from RPC",
                hash
            )));
        };

        let io = build_inputs_outputs_from_rpc_pending(
            rpc_tx,
            &state.store,
            &state.append_only_store,
            &state.store,
            &state.ckb_network,
            0,
        )?;

        let pending_since = tx_lookup.time_added_to_pool.and_then(|timestamp| {
            chrono::DateTime::from_timestamp_millis(timestamp as i64).map(|dt| dt.to_rfc3339())
        });

        let fee = tx_lookup
            .fee
            .map(|value| value.to_string())
            .or_else(|| io.computed_fee.map(|value| value.to_string()))
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "pending transaction {} missing fee from RPC and local computation",
                    hash
                ))
            })?;

        let fee_rate = tx_lookup.tx_size.and_then(|size| {
            if size <= 0 {
                return None;
            }
            let fee_value: u128 = fee.parse().ok()?;
            Some(((fee_value * 1000) / size as u128).to_string())
        });

        return ok(TransactionDetailResponse {
            hash: format!("0x{}", hex::encode(&hash_bytes)),
            status: tx_lookup.status_str().to_string(),
            pending_since,
            block_number: None,
            block_hash: None,
            index: None,
            inputs_count: rpc_tx.inputs.len() as i32,
            outputs_count: rpc_tx.outputs.len() as i32,
            fee,
            fee_rate,
            tx_size: tx_lookup.tx_size,
            cycles: tx_lookup.cycles.map(|value| value as i64),
            confirmations: None,
            is_cellbase: rpc_tx.inputs.first().is_some_and(|input| {
                input.previous_output.tx_hash
                    == "0x0000000000000000000000000000000000000000000000000000000000000000"
            }),
            timestamp: None,
            inputs_capacity: io.inputs_capacity.map(|value| value.to_string()),
            outputs_capacity: Some(io.outputs_capacity.to_string()),
            inputs_used_capacity: io.inputs_used_capacity.map(|value| value.to_string()),
            outputs_used_capacity: Some(io.outputs_used_capacity.to_string()),
            inputs: io.inputs,
            outputs: io.outputs,
            witnesses: io.witnesses,
            witnesses_available: io.witnesses_available,
        });
    };

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
    let tip_block = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(format!("sync status unavailable: {}", e)))?
        .tip_block_number;

    let confirmations = if tip_block >= block_number {
        tip_block - block_number + 1
    } else {
        tracing::warn!(
            tip_block,
            block_number,
            "tip_block < block_number for committed tx; secondary reader may be stale"
        );
        0
    };

    let final_tx_size = if tx_size > 0 { Some(tx_size) } else { None };

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
                    &state.append_only_store,
                    &state.store,
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

    ok(TransactionDetailResponse {
        hash: tx_hash_hex,
        status: "committed".to_string(),
        pending_since: None,
        block_number: Some(block_number),
        block_hash: Some(format!("0x{}", hex::encode(&block_hash))),
        index: Some(tx_idx),
        inputs_count,
        outputs_count,
        fee,
        fee_rate,
        tx_size: final_tx_size,
        cycles,
        confirmations: Some(confirmations),
        is_cellbase,
        timestamp: Some(timestamp),
        inputs_capacity: Some(inputs_capacity.to_string()),
        outputs_capacity: Some(outputs_capacity.to_string()),
        inputs_used_capacity: Some(inputs_occupied_capacity.to_string()),
        outputs_used_capacity: Some(outputs_occupied_capacity.to_string()),
        inputs,
        outputs,
        witnesses,
        witnesses_available,
    })
}

fn empty_inputs_outputs() -> TxIoBundle {
    (vec![], vec![], 0, 0, 0, 0, 0, vec![], false)
}

fn build_inputs_outputs_from_rpc_pending(
    rpc_tx: &ckb_store_reader::RpcTransactionView,
    core_store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
    store: &ckbadger_store::CkbadgerStore,
    network: &str,
    block_number: i64,
) -> Result<PendingTxIoBundle, ApiRouteError> {
    if rpc_tx.outputs.len() != rpc_tx.outputs_data.len() {
        return Err(ApiError::internal(format!(
            "pending transaction outputs mismatch: tx_hash={}, outputs={}, outputs_data={}",
            rpc_tx.hash,
            rpc_tx.outputs.len(),
            rpc_tx.outputs_data.len()
        )));
    }

    let tx_hash = decode_hex_bytes_with_context(
        &rpc_tx.hash,
        "transaction.hash",
        "building pending transaction detail",
        Some(32),
    )?;
    let tx_hash_hex = format!("0x{}", hex::encode(&tx_hash));

    let mut inputs_capacity: u128 = 0;
    let mut inputs_occupied_capacity: u128 = 0;
    let mut inputs_complete = true;
    let mut has_dao_type_input = false;

    let inputs = rpc_tx
        .inputs
        .iter()
        .map(|input| -> Result<TransactionInputResponse, ApiRouteError> {
            let prev_tx_hash_hex = &input.previous_output.tx_hash;
            let prev_index_hex = &input.previous_output.index;
            let input_context = format!(
                "building pending input for tx={} prev_outpoint=({}, {})",
                tx_hash_hex, prev_tx_hash_hex, prev_index_hex
            );
            let prev_index = u32::from_str_radix(
                prev_index_hex.strip_prefix("0x").unwrap_or(prev_index_hex),
                16,
            )
            .map_err(|e| {
                ApiError::internal(format!(
                    "invalid previous_output.index while {}: value='{}', error={}",
                    input_context, prev_index_hex, e
                ))
            })?;

            let prev_tx_hash_bytes = decode_hex_bytes_with_context(
                prev_tx_hash_hex,
                "input.previous_output.tx_hash",
                &input_context,
                Some(32),
            )?;

            let cell_info = core_store
                .get_cell(&prev_tx_hash_bytes, prev_index as i16, cells_store)
                .ok()
                .flatten()
                .or_else(|| {
                    core_store
                        .get_consumed_cell(&prev_tx_hash_bytes, prev_index as i16, cells_store)
                        .ok()
                        .flatten()
                });

            let (capacity, lock, type_script, address) = match cell_info {
                Some(info) => {
                    let cap = info.capacity as u128;
                    inputs_capacity += cap;

                    let occ = occupied_capacity_bytes(
                        info.lock_args.len(),
                        info.type_code_hash
                            .as_ref()
                            .map(|_| info.type_args.as_deref().unwrap_or(&[]).len()),
                        info.data_size as usize,
                    );
                    inputs_occupied_capacity += occ as u128;

                    let lock_resp = ScriptResponse {
                        code_hash: format!("0x{}", hex::encode(&info.lock_code_hash)),
                        hash_type: hash_type_to_str(info.lock_hash_type).to_string(),
                        args: format!("0x{}", hex::encode(&info.lock_args)),
                    };
                    let type_resp = info
                        .type_code_hash
                        .as_ref()
                        .map(|type_code_hash| -> Result<ScriptResponse, ApiRouteError> {
                            Ok(ScriptResponse {
                                code_hash: format!("0x{}", hex::encode(type_code_hash)),
                                hash_type: resolve_stored_input_type_hash_type(
                                    core_store,
                                    store,
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
                    inputs_complete = false;
                    (None, None, None, None)
                }
            };

            if type_script
                .as_ref()
                .is_some_and(|script| is_dao_type_code_hash_hex(&script.code_hash))
            {
                has_dao_type_input = true;
            }

            Ok(TransactionInputResponse {
                previous_output: Some(PreviousOutput {
                    tx_hash: prev_tx_hash_hex.clone(),
                    index: prev_index as i32,
                }),
                since: input.since.clone(),
                capacity,
                lock,
                r#type: type_script,
                address,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut outputs_capacity: u128 = 0;
    let mut outputs_occupied_capacity: u128 = 0;
    let outputs = rpc_tx
        .outputs
        .iter()
        .enumerate()
        .map(
            |(output_idx, output)| -> Result<TransactionOutputResponse, ApiRouteError> {
                let output_context = format!(
                    "building pending output for tx={} output_index={}",
                    tx_hash_hex, output_idx
                );
                let cap = parse_u64_hex_field_with_context(
                    &output.capacity,
                    "output.capacity",
                    &output_context,
                )?;
                outputs_capacity += cap as u128;

                let code_hash_bytes = decode_hex_bytes_with_context(
                    &output.lock.code_hash,
                    "output.lock.code_hash",
                    &output_context,
                    Some(32),
                )?;
                let hash_type = parse_hash_type_label_to_i16(&output.lock.hash_type)?;
                let args_bytes = decode_hex_bytes_with_context(
                    &output.lock.args,
                    "output.lock.args",
                    &output_context,
                    None,
                )?;

                let lock_resp = ScriptResponse {
                    code_hash: output.lock.code_hash.clone(),
                    hash_type: output.lock.hash_type.clone(),
                    args: output.lock.args.clone(),
                };

                let address =
                    script_to_address(&code_hash_bytes, hash_type, &args_bytes, network).ok();

                let type_resp = output.type_.as_ref().map(|script| ScriptResponse {
                    code_hash: script.code_hash.clone(),
                    hash_type: script.hash_type.clone(),
                    args: script.args.clone(),
                });

                let type_args_len = output
                    .type_
                    .as_ref()
                    .map(|script| {
                        decode_hex_bytes_with_context(
                            &script.args,
                            "output.type.args",
                            &output_context,
                            None,
                        )
                        .map(|bytes| bytes.len())
                    })
                    .transpose()?;

                let output_data = rpc_tx.outputs_data.get(output_idx).ok_or_else(|| {
                    ApiError::internal(format!(
                        "missing output data while {}: output_index={}",
                        output_context, output_idx
                    ))
                })?;
                let data_size = decode_hex_bytes_with_context(
                    output_data,
                    "output.data",
                    &output_context,
                    None,
                )?
                .len();

                let occ = occupied_capacity_bytes(args_bytes.len(), type_args_len, data_size);
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

                Ok(TransactionOutputResponse {
                    capacity: cap.to_string(),
                    used_capacity: occ as i64,
                    virtual_used_capacity: virtual_occupied_capacity,
                    cell_type,
                    lock: Some(lock_resp),
                    r#type: type_resp,
                    address,
                })
            },
        )
        .collect::<Result<Vec<_>, _>>()?;

    let is_cellbase = rpc_tx.inputs.first().is_some_and(|input| {
        input.previous_output.tx_hash
            == "0x0000000000000000000000000000000000000000000000000000000000000000"
    });

    let computed_fee = if is_cellbase {
        Some(0)
    } else if inputs_complete {
        Some(compute_tx_fee_from_io(
            inputs_capacity,
            outputs_capacity,
            false,
            has_dao_type_input,
            block_number,
            &tx_hash,
        )?)
    } else {
        None
    };

    Ok(PendingTxIoBundle {
        inputs,
        outputs,
        inputs_capacity: inputs_complete.then_some(inputs_capacity),
        outputs_capacity,
        inputs_used_capacity: inputs_complete.then_some(inputs_occupied_capacity),
        outputs_used_capacity: outputs_occupied_capacity,
        computed_fee,
        witnesses: rpc_tx.witnesses.clone(),
        witnesses_available: true,
    })
}

/// Build inputs/outputs from CKB node's RocksDB transaction view.
fn build_inputs_outputs_from_ckb(
    tx_view: &ckb_types::core::TransactionView,
    ckb_store: &ckb_store_reader::CkbChainReader,
    core_store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
    store: &ckbadger_store::CkbadgerStore,
    network: &str,
    block_number: i64,
) -> Result<TxIoBundle, ApiRouteError> {
    let rpc_tx = ckb_store_reader::convert_transaction_view(tx_view);
    let witnesses = rpc_tx.witnesses.clone();

    let mut inputs_capacity: u128 = 0;
    let mut inputs_occupied_capacity: u128 = 0;
    let mut has_dao_type_input = false;

    let inputs: Vec<TransactionInputResponse> = rpc_tx
        .inputs
        .iter()
        .map(|input| -> Result<TransactionInputResponse, ApiRouteError> {
            let prev_tx_hash_hex = &input.previous_output.tx_hash;
            let prev_index_hex = &input.previous_output.index;
            let input_context = format!(
                "building input for tx=0x{} prev_outpoint=({}, {})",
                hex::encode(tx_view.hash().raw_data()),
                prev_tx_hash_hex,
                prev_index_hex
            );
            let prev_index = u32::from_str_radix(
                prev_index_hex.strip_prefix("0x").unwrap_or(prev_index_hex),
                16,
            )
            .map_err(|e| {
                ApiError::internal(format!(
                    "invalid previous_output.index while {}: value='{}', error={}",
                    input_context, prev_index_hex, e
                ))
            })?;

            let since = &input.since;

            // Try to look up the previous output cell for capacity/lock info
            let prev_tx_hash_bytes = decode_hex_bytes_with_context(
                prev_tx_hash_hex,
                "input.previous_output.tx_hash",
                &input_context,
                Some(32),
            )?;

            let (capacity, lock, type_script, address) = {
                // Try live cells first, then consumed cells in our store
                let cell_info = core_store
                    .get_cell(&prev_tx_hash_bytes, prev_index as i16, cells_store)
                    .ok()
                    .flatten()
                    .or_else(|| {
                        core_store
                            .get_consumed_cell(&prev_tx_hash_bytes, prev_index as i16, cells_store)
                            .ok()
                            .flatten()
                    });

                match cell_info {
                    Some(info) => {
                        let cap = info.capacity as u128;
                        inputs_capacity += cap;

                        let occ = occupied_capacity_bytes(
                            info.lock_args.len(),
                            info.type_code_hash
                                .as_ref()
                                .map(|_| info.type_args.as_deref().unwrap_or(&[]).len()),
                            info.data_size as usize,
                        );
                        inputs_occupied_capacity += occ as u128;

                        let lock_resp = ScriptResponse {
                            code_hash: format!("0x{}", hex::encode(&info.lock_code_hash)),
                            hash_type: hash_type_to_str(info.lock_hash_type).to_string(),
                            args: format!("0x{}", hex::encode(&info.lock_args)),
                        };
                        let type_resp = info
                            .type_code_hash
                            .as_ref()
                            .map(|type_code_hash| -> Result<ScriptResponse, ApiRouteError> {
                                Ok(ScriptResponse {
                                    code_hash: format!("0x{}", hex::encode(type_code_hash)),
                                    hash_type: resolve_stored_input_type_hash_type(
                                        core_store,
                                        store,
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
                    None => (None, None, None, None),
                }
            };

            if type_script
                .as_ref()
                .is_some_and(|script| is_dao_type_code_hash_hex(&script.code_hash))
            {
                has_dao_type_input = true;
            }

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

    let tx_hash: Vec<u8> = tx_view.hash().raw_data().to_vec();
    let outputs: Vec<TransactionOutputResponse> = rpc_tx
        .outputs
        .iter()
        .enumerate()
        .map(
            |(output_idx, output)| -> Result<TransactionOutputResponse, ApiRouteError> {
                let output_context = format!(
                    "building output for tx=0x{} output_index={}",
                    hex::encode(&tx_hash),
                    output_idx
                );
                let cap_hex = &output.capacity;
                let cap =
                    parse_u64_hex_field_with_context(cap_hex, "output.capacity", &output_context)?;
                outputs_capacity += cap as u128;

                let lock = &output.lock;
                let code_hash_bytes = decode_hex_bytes_with_context(
                    &lock.code_hash,
                    "output.lock.code_hash",
                    &output_context,
                    Some(32),
                )?;
                let ht = parse_hash_type_label_to_i16(&lock.hash_type)?;
                let args_bytes = decode_hex_bytes_with_context(
                    &lock.args,
                    "output.lock.args",
                    &output_context,
                    None,
                )?;

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

                let type_args_len = output
                    .type_
                    .as_ref()
                    .map(|t| {
                        decode_hex_bytes_with_context(
                            &t.args,
                            "output.type.args",
                            &output_context,
                            None,
                        )
                        .map(|v| v.len())
                    })
                    .transpose()?;

                // Get data size from CKB store
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

                let occ = occupied_capacity_bytes(args_bytes.len(), type_args_len, data_size);
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

                Ok(TransactionOutputResponse {
                    capacity: cap.to_string(),
                    used_capacity: occ as i64,
                    virtual_used_capacity: virtual_occupied_capacity,
                    cell_type,
                    lock: Some(lock_resp),
                    r#type: type_resp,
                    address,
                })
            },
        )
        .collect::<Result<Vec<_>, _>>()?;

    let is_cellbase = rpc_tx.inputs.first().is_some_and(|input| {
        input.previous_output.tx_hash
            == "0x0000000000000000000000000000000000000000000000000000000000000000"
    });

    // DAO phase-2 withdrawals may legitimately satisfy outputs > inputs due compensation.
    let computed_fee = compute_tx_fee_from_io(
        inputs_capacity,
        outputs_capacity,
        is_cellbase,
        has_dao_type_input,
        block_number,
        tx_view.hash().raw_data().as_ref(),
    )?;

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

    if let Some(tx_lookup) = fetch_transaction_lookup(&state.ckb_rpc_url, &hash)
        .await
        .map_err(ApiError::internal)?
    {
        if tx_lookup.is_pending_like() {
            return Err(ApiError::bad_request(pending_transaction_resource_error(
                &hash,
                tx_lookup.status_str(),
                "Cell deps",
            )));
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
                        "Cycles task dispatch unavailable: worker not connected".to_string(),
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
) -> Result<Option<(Option<i64>, bool)>, ApiRouteError> {
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
) -> Result<CyclesStatusResponse, ApiRouteError> {
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

fn lookup_tx_block_number_in_ckb_store(
    ckb_store: Option<&Arc<ckb_store_reader::CkbChainReader>>,
    hash_bytes: &[u8],
) -> Option<u64> {
    if hash_bytes.len() != 32 {
        return None;
    }

    let store = ckb_store?;
    let mut tx_hash = [0u8; 32];
    tx_hash.copy_from_slice(hash_bytes);
    store
        .get_transaction_with_block_number(&tx_hash)
        .map(|(_, block_number)| block_number)
}

fn missing_tx_lookup_error(hash_bytes: &[u8], ckb_block_number: Option<u64>) -> ApiRouteError {
    let tx_hash_hex = format!("0x{}", hex::encode(hash_bytes));
    match ckb_block_number {
        Some(block_number) => ApiError::internal(format!(
            "transaction exists in CKB RocksDB but tx index mapping is missing: tx_hash={}, block_number={}; fix indexer write/read logic, then rebuild ckbadger RocksDB and re-sync from genesis",
            tx_hash_hex, block_number
        )),
        None => ApiError::not_found("Transaction not found"),
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
            if let Some(tx_lookup) = fetch_transaction_lookup(&state.ckb_rpc_url, &hash)
                .await
                .map_err(ApiError::internal)?
            {
                if tx_lookup.is_pending_like() {
                    return Err(ApiError::bad_request(pending_transaction_resource_error(
                        &hash,
                        tx_lookup.status_str(),
                        "Lifecycle data",
                    )));
                }
            }
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
    let tip = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(format!("sync status unavailable: {}", e)))?
        .tip_block_number;

    let compute_confirmations = |block_num: i64| -> i64 {
        if tip >= block_num {
            tip - block_num + 1
        } else {
            tracing::warn!(
                tip,
                block_num,
                "tip < block_num for committed tx; secondary reader may be stale"
            );
            0
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
            confirmations: Some(compute_confirmations(commit_block_number)),
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
        confirmations: Some(compute_confirmations(commit_block_number)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_common::cycles_task::{CyclesTaskResult, CyclesTaskStatus};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

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
    fn test_tx_hash_from_prefetched_hashes_returns_hash_by_index() {
        let hashes = Some(vec![vec![0x11; 32], vec![0x22; 32]]);
        assert_eq!(
            tx_hash_from_prefetched_hashes(&hashes, 1),
            Some(vec![0x22; 32])
        );
    }

    #[test]
    fn test_tx_hash_from_prefetched_hashes_rejects_invalid_index() {
        let hashes = Some(vec![vec![0x11; 32]]);
        assert_eq!(tx_hash_from_prefetched_hashes(&hashes, -1), None);
        assert_eq!(tx_hash_from_prefetched_hashes(&hashes, 3), None);
        assert_eq!(tx_hash_from_prefetched_hashes(&None, 0), None);
    }

    #[test]
    fn test_tx_block_hashes_cache_key_is_stable() {
        assert_eq!(
            tx_block_hashes_cache_key(42),
            "transactions:block_tx_hashes:42"
        );
    }

    #[test]
    fn test_get_block_tx_hashes_cached_with_fetch_uses_cache() {
        let cache = InMemoryCache::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let first_calls = calls.clone();
        let first = get_block_tx_hashes_cached_with_fetch(&cache, 99, move |_| {
            first_calls.fetch_add(1, Ordering::SeqCst);
            Some(vec![vec![0x11; 32]])
        })
        .unwrap();
        assert_eq!(first, vec![vec![0x11; 32]]);

        let second_calls = calls.clone();
        let second = get_block_tx_hashes_cached_with_fetch(&cache, 99, move |_| {
            second_calls.fetch_add(1, Ordering::SeqCst);
            Some(vec![vec![0x22; 32]])
        })
        .unwrap();
        assert_eq!(second, vec![vec![0x11; 32]]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_hash_type_to_str() {
        assert_eq!(hash_type_to_str(0), "data");
        assert_eq!(hash_type_to_str(1), "type");
        assert_eq!(hash_type_to_str(2), "data1");
        assert_eq!(hash_type_to_str(4), "data2");
        assert_eq!(hash_type_to_str(99), "unknown");
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
    fn test_parse_hash_type_label_to_i16() {
        assert_eq!(parse_hash_type_label_to_i16("data").unwrap(), 0);
        assert_eq!(parse_hash_type_label_to_i16("type").unwrap(), 1);
        assert_eq!(parse_hash_type_label_to_i16("data1").unwrap(), 2);
        assert_eq!(parse_hash_type_label_to_i16("data2").unwrap(), 4);
    }

    #[test]
    fn test_parse_hash_type_label_to_i16_rejects_unknown() {
        let err = parse_hash_type_label_to_i16("unknown").unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("unknown script hash_type label in CKB store"));
    }

    #[test]
    fn test_decode_hex_bytes_with_context_rejects_invalid_hex() {
        let err =
            decode_hex_bytes_with_context("0xzz", "lock.args", "unit-test", None).unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("invalid hex for lock.args while unit-test"));
    }

    #[test]
    fn test_decode_hex_bytes_with_context_rejects_len_mismatch() {
        let err = decode_hex_bytes_with_context("0x1234", "lock.code_hash", "unit-test", Some(32))
            .unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("invalid byte length for lock.code_hash while unit-test"));
    }

    #[test]
    fn test_parse_u64_hex_field_with_context_rejects_invalid_hex() {
        let err = parse_u64_hex_field_with_context("0x-not-hex", "output.capacity", "unit-test")
            .unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("invalid hex u64 for output.capacity while unit-test"));
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
            status: "committed".to_string(),
            pending_since: None,
            block_number: Some(100),
            block_hash: Some("0xdef".to_string()),
            index: Some(0),
            inputs_count: 1,
            outputs_count: 1,
            fee: "42".to_string(),
            fee_rate: Some("1000".to_string()),
            tx_size: Some(123),
            cycles: Some(456),
            confirmations: Some(7),
            is_cellbase: false,
            timestamp: Some("2024-01-01T00:00:00Z".to_string()),
            inputs_capacity: Some("100".to_string()),
            outputs_capacity: Some("58".to_string()),
            inputs_used_capacity: Some("10".to_string()),
            outputs_used_capacity: Some("9".to_string()),
            inputs: vec![],
            outputs: vec![],
            witnesses: vec!["0x".to_string(), "0x1234".to_string()],
            witnesses_available: true,
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["status"], "committed");
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

    #[test]
    fn test_missing_tx_lookup_error_returns_not_found_when_ckb_not_found() {
        let hash = vec![0xabu8; 32];
        let (status, body) = missing_tx_lookup_error(&hash, None);
        assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
        assert_eq!(body.0.error, "not_found");
        assert_eq!(body.0.message, "Transaction not found");
    }

    #[test]
    fn test_missing_tx_lookup_error_returns_internal_with_context_when_ckb_found() {
        let hash = vec![0xcdu8; 32];
        let (status, body) = missing_tx_lookup_error(&hash, Some(42));
        assert_eq!(status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.0.error, "internal_error");
        assert!(body.0.message.contains("tx index mapping is missing"));
        assert!(body.0.message.contains("tx_hash=0x"));
        assert!(body.0.message.contains("block_number=42"));
    }

    #[test]
    fn test_is_dao_type_code_hash_hex() {
        assert!(is_dao_type_code_hash_hex(
            "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e"
        ));
        assert!(is_dao_type_code_hash_hex(
            "82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e"
        ));
        assert!(!is_dao_type_code_hash_hex(
            "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
        ));
    }

    #[test]
    fn test_occupied_capacity_bytes_without_type_script() {
        let occ = occupied_capacity_bytes(20, None, 64);
        assert_eq!(occ, 8 + 32 + 1 + 20 + 64);
    }

    #[test]
    fn test_occupied_capacity_bytes_includes_type_script_size() {
        let occ = occupied_capacity_bytes(20, Some(16), 64);
        assert_eq!(occ, 8 + 32 + 1 + 20 + (32 + 1 + 16) + 64);
    }

    #[test]
    fn test_compute_tx_fee_from_io_for_regular_tx() {
        let fee = compute_tx_fee_from_io(1_000, 950, false, false, 10, &[0x11; 32]).unwrap();
        assert_eq!(fee, 50);
    }

    #[test]
    fn test_compute_tx_fee_from_io_allows_dao_compensation_case() {
        let fee = compute_tx_fee_from_io(1_000, 1_100, false, true, 10, &[0x22; 32]).unwrap();
        assert_eq!(fee, 0);
    }

    #[test]
    fn test_compute_tx_fee_from_io_errors_when_non_dao_outputs_exceed_inputs() {
        let err = compute_tx_fee_from_io(1_000, 1_100, false, false, 10, &[0x33; 32]).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.1 .0.message.contains("inputs/outputs invariant broken"));
    }
}
