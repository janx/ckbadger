use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Extension, Router,
};
use ckb_store_reader::RpcTransactionView;
use ckb_types::packed;
use ckbadger_common::cycles_task::{CyclesTaskResult, CyclesTaskStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::time::{sleep, Instant};

use crate::cache::InMemoryCache;
use crate::cycles::{CyclesStatus, CyclesStatusResponse};
use crate::response::{
    default_limit, encode_cursor, hash_type_to_str, ok, ApiError, ApiResult, ApiRouteError,
    CursorPaginatedResponse, ScriptResponse,
};
use crate::routes::identities::{dotcell_record_response, DotCellRecordResponse};
use crate::routes::tx_lookup::{fetch_transaction_lookup, pending_transaction_resource_error};
use crate::utils::{
    parse_hash32, parse_optional_block_tx_cursor, script_to_address, validate_block_number,
};
use crate::{AppState, RequestReadView};
use tracing::instrument;

/// (block_number, tx_hash, tx_index, tx_index_entry, block_hash)
type TxListEntry = (i64, Vec<u8>, i32, ckbadger_store::TxIndexEntry, Vec<u8>);
type TxIoBundle = (
    Vec<TransactionInputResponse>,
    Vec<TransactionOutputResponse>,
    u128,
    u128,
    u128,
    u128,
    Vec<String>,
    bool,
    Vec<DotCellNameWitnessResponse>,
);

#[derive(Debug, Clone)]
struct PendingTxIoBundle {
    inputs: Vec<TransactionInputResponse>,
    outputs: Vec<TransactionOutputResponse>,
    inputs_capacity: u128,
    outputs_capacity: u128,
    inputs_used_capacity: u128,
    outputs_used_capacity: u128,
    fee: u128,
    witnesses: Vec<String>,
    witnesses_available: bool,
    dotcell_names: Vec<DotCellNameWitnessResponse>,
}
const TX_BLOCK_HASHES_CACHE_TTL: StdDuration = StdDuration::from_secs(30);

/// A transaction's size as CKB counts it: the molecule size plus the 4-byte
/// offset slot the transaction occupies in the block's transactions table.
///
/// This is the size the node's tx-pool, wallets, and the official explorer all
/// report, and it is the fee-rate denominator. The API serves it as `txSize` /
/// `size` so that the size and the fee rate in one response are reproducible
/// from each other — reporting the bare molecule size while dividing by this
/// one made `fee / txSize` disagree with the served `feeRate`.
///
/// The store keeps molecule sizes; conversion happens once, here, at the
/// response boundary.
pub(crate) fn tx_serialized_size_in_block(molecule_size: i32) -> i32 {
    molecule_size
        .checked_add(4)
        .unwrap_or_else(|| panic!("transaction molecule size overflows i32: {molecule_size}"))
}

/// Fee rate in shannons per 1000 bytes, floored, over the same size the API
/// serves as `txSize`. Single definition for every fee rate the API returns
/// (transaction detail, pending detail, block fee-stats).
pub(crate) fn tx_fee_rate(fee: u128, size_in_block: i32) -> String {
    let denominator = u128::try_from(size_in_block).unwrap_or_else(|_| {
        panic!("fee-rate call sites guard size_in_block > 0, got {size_in_block}")
    });
    (fee * 1000 / denominator).to_string()
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycles_status: Option<String>,
    pub is_cellbase: bool,
    pub timestamp: String,
}

fn derive_cycles_status(cycles: Option<i64>, is_cellbase: bool) -> Option<String> {
    if is_cellbase {
        return None;
    }
    match cycles {
        Some(c) if c > 0 => None,
        Some(-1) => Some("failed".to_string()),
        _ => Some("pending".to_string()),
    }
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

    // Validate before any store access: `get_block_header` encodes the number
    // into a store key, and a negative one trips an assert that aborts the
    // process under `panic = "abort"`.
    let block_number = params
        .block_number
        .map(|bn| validate_block_number(bn, "block_number"))
        .transpose()?;

    // Get total count
    let total: i64 = if let Some(block_number) = block_number {
        let store = state.store.clone();
        tokio::task::spawn_blocking(move || {
            store
                .get_block_header(block_number)
                .map(|header| header.map(|h| h.transactions_count as i64).unwrap_or(0))
        })
        .await
        .map_err(|e| ApiError::internal(format!("block header lookup failed: {}", e)))?
        .map_err(|e| {
            ApiError::internal(format!(
                "block header unavailable for block_number={}: {}",
                block_number, e
            ))
        })?
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

    if let Some(block_number) = block_number {
        // List transactions for a specific block (ascending order)
        let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "cursor")?;
        // Cursor is the tx_idx of the last item on the previous page.
        // We want txs AFTER that index. Use -1 for first page (returns from idx 0).
        let after_tx_idx = cursor.map(|(_, idx)| idx).unwrap_or(-1);
        let fetch_limit = (limit + 1) as usize;

        let store_c = store.clone();
        let ckb_store_c = ckb_store.clone();
        let mem_cache_c = mem_cache.clone();
        let (page_txs, block_tx_hashes) = tokio::task::spawn_blocking(move || {
            let page_txs = store_c.list_block_txs_after(block_number, after_tx_idx, fetch_limit)?;
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
                    tx_size: Some(tx_serialized_size_in_block(entry.tx_size)),
                    cycles: entry.cycles,
                    cycles_status: derive_cycles_status(entry.cycles, entry.is_cellbase),
                    is_cellbase: entry.is_cellbase,
                    timestamp,
                }
            })
            .collect();

        ok(CursorPaginatedResponse::new(txs, total, limit, next_cursor))
    } else {
        // List latest transactions (DESC order across blocks)
        let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "cursor")?;
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
                    tx_size: Some(tx_serialized_size_in_block(entry.tx_size)),
                    cycles: entry.cycles,
                    cycles_status: derive_cycles_status(entry.cycles, entry.is_cellbase),
                    is_cellbase: entry.is_cellbase,
                    timestamp,
                }
            })
            .collect();

        ok(CursorPaginatedResponse::new(txs, total, limit, next_cursor))
    }
}

#[instrument(skip(state), level = "debug")]
async fn get_transaction(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;

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
                tx_size: Some(tx_serialized_size_in_block(entry.tx_size)),
                cycles: entry.cycles,
                cycles_status: derive_cycles_status(entry.cycles, entry.is_cellbase),
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
    #[serde(rename = "commonKnowledgeSize")]
    pub used_capacity: i64,
    #[serde(
        rename = "virtualCommonKnowledgeSize",
        skip_serializing_if = "Option::is_none"
    )]
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
    /// Where an uncommitted transaction stands: `pending`, `proposed`, or
    /// `committed_awaiting_index` (the node has it in a block, this process's
    /// store has not indexed it yet). Absent once the store has it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_status: Option<String>,
    /// How completely an uncommitted transaction could be interpreted, with the
    /// reasons when it could not be interpreted fully.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<crate::pool::InterpretationResponse>,
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
    pub cycles_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<i64>,
    pub is_cellbase: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs_capacity: Option<String>,
    #[serde(
        rename = "inputsCommonKnowledgeSize",
        skip_serializing_if = "Option::is_none"
    )]
    pub inputs_used_capacity: Option<String>,
    #[serde(
        rename = "outputsCommonKnowledgeSize",
        skip_serializing_if = "Option::is_none"
    )]
    pub outputs_used_capacity: Option<String>,
    pub inputs: Vec<TransactionInputResponse>,
    pub outputs: Vec<TransactionOutputResponse>,
    pub witnesses: Vec<String>,
    pub witnesses_available: bool,
    /// The `.cell` names this transaction creates, each with the records
    /// payload decoded from the witness at its own output index. Absent when
    /// the transaction creates none (or its witnesses are unavailable).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dotcell_names: Vec<DotCellNameWitnessResponse>,
}

/// A `.cell` name created by a transaction, with the records its creating
/// witness carries (spec §1.3: `WitnessArgs.output_type` at the name cell's
/// own output index, hash-verified against `data[1..33]`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DotCellNameWitnessResponse {
    pub output_index: usize,
    pub label: String,
    pub name: String,
    pub identity_id: String,
    pub records_hash: String,
    pub records: Vec<DotCellRecordResponse>,
}

/// The same string-encoded transaction, in the indexer's RPC type: the direct
/// reader and the node lookup mirror the indexer's `rpc::TransactionView`
/// field for field, and the `.cell` parser takes the indexer's.
fn indexer_tx_view(tx: &RpcTransactionView) -> ckbadger_indexer::rpc::TransactionView {
    use ckbadger_indexer::rpc::{CellDep, CellInput, CellOutput, OutPoint, Script};

    let out_point = |p: &ckb_store_reader::RpcOutPoint| OutPoint {
        tx_hash: p.tx_hash.clone(),
        index: p.index.clone(),
    };
    let script = |s: &ckb_store_reader::RpcScript| Script {
        code_hash: s.code_hash.clone(),
        hash_type: s.hash_type.clone(),
        args: s.args.clone(),
    };
    ckbadger_indexer::rpc::TransactionView {
        hash: tx.hash.clone(),
        version: tx.version.clone(),
        cell_deps: tx
            .cell_deps
            .iter()
            .map(|dep| CellDep {
                out_point: out_point(&dep.out_point),
                dep_type: dep.dep_type.clone(),
            })
            .collect(),
        header_deps: tx.header_deps.clone(),
        inputs: tx
            .inputs
            .iter()
            .map(|input| CellInput {
                since: input.since.clone(),
                previous_output: out_point(&input.previous_output),
            })
            .collect(),
        outputs: tx
            .outputs
            .iter()
            .map(|output| CellOutput {
                capacity: output.capacity.clone(),
                lock: script(&output.lock),
                type_: output.type_.as_ref().map(script),
            })
            .collect(),
        outputs_data: tx.outputs_data.clone(),
        witnesses: tx.witnesses.clone(),
    }
}

/// The `.cell` names a transaction creates, decoded by the exact function the
/// live indexer runs (`DotCellParser::parse_name_cells_with_output_indices`).
///
/// A name cell whose own-index witness is missing or does not hash to the
/// cell's records hash is an error, not an empty list: its type script was
/// verified on chain (or by the pool), so the mismatch is a broken invariant.
/// The ring root is verified the same way but not listed: it is protocol
/// infrastructure, not a name with an identity.
fn dotcell_names_for_tx(
    tx: &RpcTransactionView,
) -> Result<Vec<DotCellNameWitnessResponse>, ApiRouteError> {
    let names = ckbadger_indexer::parser::DotCellParser::parse_name_cells_with_output_indices(
        &indexer_tx_view(tx),
    )
    .map_err(|e| ApiError::internal(format!("{e:#}")))?;
    Ok(names
        .into_iter()
        .filter(|(_, name, _)| !name.is_root())
        .map(|(output_index, name, records)| DotCellNameWitnessResponse {
            output_index,
            name: format!("{}.cell", name.label),
            identity_id: format!("0x{}", hex::encode(name.id)),
            records_hash: format!("0x{}", hex::encode(name.records_hash)),
            records: records.iter().map(dotcell_record_response).collect(),
            label: name.label,
        })
        .collect())
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
    ckbadger_common::hash_type_from_label(hash_type)
        .map(i16::from)
        .ok_or_else(|| {
            ApiError::internal(format!(
                "unknown script hash_type label in CKB store: '{}'",
                hash_type
            ))
        })
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

/// Compute an UNCOMMITTED transaction's fee from its fully resolved cells.
///
/// Committed transactions serve the stored fee — the indexer write path is the
/// single calculation path there. For an uncommitted one this is the only
/// source: `(Σ inputs + Σ DAO compensation) − Σ outputs`, where the
/// compensation is what the spent withdraw-request cells pay out (exactly the
/// live-sync correction `correct_dao_withdrawal_fees` applies). A negative
/// result is a broken invariant, never a zero.
fn compute_tx_fee_from_io(
    inputs_capacity: u128,
    dao_compensation: u128,
    outputs_capacity: u128,
    is_cellbase: bool,
    block_number: Option<i64>,
    tx_hash: &[u8],
) -> Result<u128, ApiRouteError> {
    if is_cellbase {
        return Ok(0);
    }

    inputs_capacity
        .checked_add(dao_compensation)
        .and_then(|effective_inputs| effective_inputs.checked_sub(outputs_capacity))
        .ok_or_else(|| {
            ApiError::internal(format!(
                "transaction inputs/outputs invariant broken at block {}: tx_hash=0x{}, inputs_capacity={}, dao_compensation={}, outputs_capacity={}",
                block_number.map_or_else(|| "(in pool)".to_string(), |number| number.to_string()),
                hex::encode(tx_hash),
                inputs_capacity,
                dao_compensation,
                outputs_capacity
            ))
        })
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
            Ok(Some(token)) => {
                return hash_type_to_str(token.hash_type as i16)
                    .map(|s| s.to_string())
                    .ok_or_else(|| {
                        ApiError::internal(format!(
                            "unknown hash_type {} for token type_script_hash=0x{}",
                            token.hash_type,
                            hex::encode(type_hash)
                        ))
                    })
            }
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
        Ok(Some(script)) => hash_type_to_str(script.hash_type as i16)
            .map(|s| s.to_string())
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "unknown hash_type {} for script code_hash=0x{}",
                    script.hash_type,
                    hex::encode(type_code_hash)
                ))
            }),
        Ok(None) => Ok("unknown".to_string()),
        Err(e) => Err(ApiError::internal(format!(
            "failed to resolve script hash_type for type_code_hash=0x{}: {}",
            hex::encode(type_code_hash),
            e
        ))),
    }
}

#[instrument(skip(state, read_view), level = "debug")]
async fn get_transaction_detail(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
    // Optional so the handler still works in routers built without the
    // read-view middleware (unit tests mounting `routes()` directly).
    read_view: Option<Extension<RequestReadView>>,
) -> ApiResult<TransactionDetailResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;

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

        // A transaction the node has committed but this process's store has not
        // indexed yet is served provisionally too. That window is seconds long,
        // and a pool row that links here must not become a dead link for them.
        if !tx_lookup.is_pending_like() && !tx_lookup.is_committed() {
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
        // A committed response always names its committing block: the tx page
        // links it while the store catches up.
        if tx_lookup.is_committed()
            && (tx_lookup.block_number.is_none() || tx_lookup.block_hash.is_none())
        {
            return Err(ApiError::internal(format!(
                "node reported transaction {hash} committed without its block number/hash"
            )));
        }

        // The one store value this branch reads (immutable once derived), read
        // before letting go of the request's view: everything below talks to
        // the node, and holding the view across node round trips would stall
        // the secondary's catch-up for as long as the node takes to answer.
        let virtual_occupied = state.genesis_baseline()?.virtual_occupied;
        if let Some(Extension(view)) = read_view {
            view.release();
        }

        // One resolver, one order: a parent the mirror's snapshot holds, then
        // the parent transaction as the node returns it — so a chained
        // unconfirmed spend and a just-committed transaction's (spent) inputs
        // resolve here exactly as they do in the mirror.
        let pool_source = crate::pool::HttpPoolSource::new(state.ckb_rpc_url.clone());
        let pool_snapshot = state.pool_mirror.load();
        let previous_outputs =
            crate::pool::resolve_previous_outputs(&pool_source, rpc_tx, pool_snapshot.as_ref())
                .await
                .map_err(|e| {
                    ApiError::internal(format!(
                        "failed to resolve inputs of uncommitted transaction {hash}: {e}"
                    ))
                })?;
        let resolved = crate::pool::resolve_pool_tx(rpc_tx, &previous_outputs).map_err(|e| {
            ApiError::internal(format!(
                "failed to read uncommitted transaction {hash}: {e}"
            ))
        })?;

        // A parent the node does not know is a state that passes (it was
        // evicted, or has not propagated yet): say so and let the caller retry,
        // rather than serving inputs, a fee and an interpretation that are
        // missing a piece.
        let unresolved = resolved.unresolved_inputs();
        if !unresolved.is_empty() {
            let outpoints = unresolved
                .iter()
                .map(|(tx_hash, index)| format!("0x{}:{index}", hex::encode(tx_hash)))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ApiError::service_unavailable(format!(
                "inputs of uncommitted transaction {hash} are not resolvable yet: the node does \
                 not know the transaction that created {outpoints}; retry"
            )));
        }

        let interpretation = crate::pool::interpretation_of(&resolved);
        // The mirror is authoritative for the pool status when it tracks this
        // transaction (only it knows `committed_awaiting_index`); otherwise the
        // node's own status stands.
        let pool_status = pool_snapshot
            .records
            .get(&resolved.tx_hash)
            .map(|record| record.pool_status.as_str().to_string())
            .unwrap_or_else(|| {
                if tx_lookup.is_committed() {
                    crate::pool::PoolStatus::COMMITTED_AWAITING_INDEX.to_string()
                } else {
                    tx_lookup.status_str().to_string()
                }
            });

        let io = build_inputs_outputs_from_pool_tx(
            rpc_tx,
            &resolved,
            &state.ckb_network,
            tx_lookup.block_number,
            virtual_occupied,
        )?;

        let pending_since = tx_lookup.time_added_to_pool.and_then(|timestamp| {
            chrono::DateTime::from_timestamp_millis(timestamp as i64).map(|dt| dt.to_rfc3339())
        });

        // The fee has ONE source: the fully resolved inputs (plus the exact DAO
        // compensation they pay out) minus the outputs. The node's pool-entry
        // fee is absent for committed transactions and is not a second path.
        let fee_value = io.fee;
        let fee = fee_value.to_string();

        let pending_tx_size = tx_lookup
            .tx_size
            .filter(|size| *size > 0)
            .map(tx_serialized_size_in_block);
        let fee_rate = pending_tx_size.map(|size| tx_fee_rate(fee_value, size));

        let pool_cycles = tx_lookup.cycles.map(|value| value as i64);
        let pool_is_cellbase = resolved.is_cellbase;
        return ok(TransactionDetailResponse {
            hash: format!("0x{}", hex::encode(&hash_bytes)),
            status: tx_lookup.status_str().to_string(),
            pool_status: Some(pool_status),
            interpretation: Some(crate::pool::InterpretationResponse::from(&interpretation)),
            pending_since,
            // Present only while the node reports a committing block this
            // process's store has not indexed yet.
            block_number: tx_lookup.block_number,
            block_hash: tx_lookup
                .block_hash
                .map(|hash| format!("0x{}", hex::encode(hash))),
            index: None,
            inputs_count: rpc_tx.inputs.len() as i32,
            outputs_count: rpc_tx.outputs.len() as i32,
            fee,
            fee_rate,
            tx_size: pending_tx_size,
            cycles: pool_cycles,
            cycles_status: derive_cycles_status(pool_cycles, pool_is_cellbase),
            confirmations: None,
            is_cellbase: pool_is_cellbase,
            timestamp: None,
            inputs_capacity: Some(io.inputs_capacity.to_string()),
            outputs_capacity: Some(io.outputs_capacity.to_string()),
            inputs_used_capacity: Some(io.inputs_used_capacity.to_string()),
            outputs_used_capacity: Some(io.outputs_used_capacity.to_string()),
            inputs: io.inputs,
            outputs: io.outputs,
            witnesses: io.witnesses,
            witnesses_available: io.witnesses_available,
            dotcell_names: io.dotcell_names,
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

    let final_tx_size = if tx_size > 0 {
        Some(tx_serialized_size_in_block(tx_size))
    } else {
        None
    };

    // Read full transaction from CKB node's RocksDB for inputs/outputs
    let (
        inputs,
        outputs,
        inputs_capacity,
        outputs_capacity,
        inputs_occupied_capacity,
        outputs_occupied_capacity,
        witnesses,
        witnesses_available,
        dotcell_names,
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
                    state.genesis_baseline()?.virtual_occupied,
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

    // The fee persisted by the indexer is the single source of truth; the
    // write path already accounts for DAO phase-2 compensation, so there is
    // no read-time recomputation from inputs/outputs.
    let fee_value = u128::try_from(entry.fee).map_err(|_| {
        ApiError::internal(format!(
            "negative stored fee for committed tx 0x{} at block {}: {}",
            hex::encode(&hash_bytes),
            block_number,
            entry.fee
        ))
    })?;
    let fee = fee_value.to_string();

    let fee_rate = final_tx_size.map(|size| tx_fee_rate(fee_value, size));

    ok(TransactionDetailResponse {
        hash: tx_hash_hex,
        status: "committed".to_string(),
        // A transaction the local store has indexed is no longer provisional.
        pool_status: None,
        interpretation: None,
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
        cycles_status: derive_cycles_status(cycles, is_cellbase),
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
        dotcell_names,
    })
}

fn empty_inputs_outputs() -> TxIoBundle {
    (vec![], vec![], 0, 0, 0, 0, vec![], false, vec![])
}

/// Build the `/tx/{hash}` response's inputs and outputs from a transaction the
/// shared pool resolver has already resolved — every input of it: the caller
/// answers 503 while any is not.
///
/// The previous store-based lookup (live cell, else consumed cell) is gone: an
/// uncommitted transaction's inputs are resolved in exactly ONE place — the
/// pool resolver's fixed order of snapshot parent, then the parent transaction
/// from the node — so the pending view and the address-page pool rows can
/// never disagree about what a transaction spends.
///
/// `block_number` is the committing block of a committed-awaiting-index
/// transaction and `None` for one still in the pool: only block 0's cells can
/// be the genesis burn cell.
fn build_inputs_outputs_from_pool_tx(
    rpc_tx: &RpcTransactionView,
    resolved: &crate::pool::ResolvedPoolTx,
    network: &str,
    block_number: Option<i64>,
    virtual_occupied: i128,
) -> Result<PendingTxIoBundle, ApiRouteError> {
    let mut inputs_capacity: u128 = 0;
    let mut inputs_occupied_capacity: u128 = 0;
    let mut dao_compensation: u128 = 0;

    let inputs = resolved
        .inputs
        .iter()
        .map(|input| -> Result<TransactionInputResponse, ApiRouteError> {
            let index = if resolved.is_cellbase && input.cell.is_none() {
                // The cellbase pseudo-input's 0xffffffff, rendered exactly as
                // the committed path renders it (`as i32`, i.e. -1), so the
                // same transaction reads the same before and after indexing.
                input.previous_output_index as i32
            } else {
                i32::try_from(input.previous_output_index).map_err(|_| {
                    ApiError::internal(format!(
                        "previous output index {} exceeds i32 for tx 0x{}",
                        input.previous_output_index,
                        hex::encode(resolved.tx_hash)
                    ))
                })?
            };
            let previous_output = Some(PreviousOutput {
                tx_hash: format!("0x{}", hex::encode(input.previous_tx_hash)),
                index,
            });

            let Some(cell) = input.cell.as_ref() else {
                if !resolved.is_cellbase {
                    return Err(ApiError::internal(format!(
                        "unresolved input 0x{}:{} reached the response builder for tx 0x{}",
                        hex::encode(input.previous_tx_hash),
                        input.previous_output_index,
                        hex::encode(resolved.tx_hash)
                    )));
                }
                // The cellbase pseudo-input spends no cell.
                return Ok(TransactionInputResponse {
                    previous_output,
                    since: input.since.clone(),
                    capacity: None,
                    lock: None,
                    r#type: None,
                    address: None,
                });
            };

            inputs_capacity += cell.capacity as u128;
            if let Some(compensation) = cell.dao_compensation {
                dao_compensation += u128::try_from(compensation).map_err(|_| {
                    ApiError::internal(format!(
                        "negative DAO compensation {compensation} for input 0x{}:{} of tx 0x{}",
                        hex::encode(input.previous_tx_hash),
                        input.previous_output_index,
                        hex::encode(resolved.tx_hash)
                    ))
                })?;
            }
            inputs_occupied_capacity += occupied_capacity_bytes(
                cell.lock_args.len(),
                cell.type_code_hash
                    .as_ref()
                    .map(|_| cell.type_args.as_deref().unwrap_or(&[]).len()),
                cell.data.len(),
            ) as u128;

            let lock_hash_type_str = hash_type_to_str(cell.lock_hash_type).ok_or_else(|| {
                ApiError::internal(format!(
                    "unknown lock hash_type {} for resolved pool input",
                    cell.lock_hash_type
                ))
            })?;
            let lock = Some(ScriptResponse {
                code_hash: format!("0x{}", hex::encode(&cell.lock_code_hash)),
                hash_type: lock_hash_type_str.to_string(),
                args: format!("0x{}", hex::encode(&cell.lock_args)),
            });

            let type_script = cell
                .type_code_hash
                .as_ref()
                .map(|type_code_hash| -> Result<ScriptResponse, ApiRouteError> {
                    // The node returned the script itself, so the hash_type is
                    // known exactly — no store lookup, no "unknown" label.
                    let hash_type = cell.type_hash_type.ok_or_else(|| {
                        ApiError::internal(
                            "resolved pool input has a type code_hash without a hash_type"
                                .to_string(),
                        )
                    })?;
                    let hash_type_str = hash_type_to_str(hash_type).ok_or_else(|| {
                        ApiError::internal(format!(
                            "unknown type hash_type {hash_type} for resolved pool input"
                        ))
                    })?;
                    Ok(ScriptResponse {
                        code_hash: format!("0x{}", hex::encode(type_code_hash)),
                        hash_type: hash_type_str.to_string(),
                        args: format!(
                            "0x{}",
                            hex::encode(cell.type_args.as_deref().unwrap_or(&[]))
                        ),
                    })
                })
                .transpose()?;

            Ok(TransactionInputResponse {
                previous_output,
                since: input.since.clone(),
                capacity: Some(cell.capacity.to_string()),
                lock,
                r#type: type_script,
                address: script_to_address(
                    &cell.lock_code_hash,
                    cell.lock_hash_type,
                    &cell.lock_args,
                    network,
                )
                .ok(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut outputs_capacity: u128 = 0;
    let mut outputs_occupied_capacity: u128 = 0;
    let outputs = resolved
        .outputs
        .iter()
        .map(|cell| -> Result<TransactionOutputResponse, ApiRouteError> {
            outputs_capacity += cell.capacity as u128;
            let occupied = occupied_capacity_bytes(
                cell.lock_args.len(),
                cell.type_code_hash
                    .as_ref()
                    .map(|_| cell.type_args.as_deref().unwrap_or(&[]).len()),
                cell.data.len(),
            );
            outputs_occupied_capacity += occupied as u128;

            let lock_hash_type_str = hash_type_to_str(cell.lock_hash_type).ok_or_else(|| {
                ApiError::internal(format!(
                    "unknown lock hash_type {} for pool output",
                    cell.lock_hash_type
                ))
            })?;
            let type_script = cell
                .type_code_hash
                .as_ref()
                .map(|type_code_hash| -> Result<ScriptResponse, ApiRouteError> {
                    let hash_type = cell.type_hash_type.ok_or_else(|| {
                        ApiError::internal(
                            "pool output has a type code_hash without a hash_type".to_string(),
                        )
                    })?;
                    let hash_type_str = hash_type_to_str(hash_type).ok_or_else(|| {
                        ApiError::internal(format!(
                            "unknown type hash_type {hash_type} for pool output"
                        ))
                    })?;
                    Ok(ScriptResponse {
                        code_hash: format!("0x{}", hex::encode(type_code_hash)),
                        hash_type: hash_type_str.to_string(),
                        args: format!(
                            "0x{}",
                            hex::encode(cell.type_args.as_deref().unwrap_or(&[]))
                        ),
                    })
                })
                .transpose()?;

            let is_satoshi = block_number == Some(0)
                && ckbadger_common::burn_policy::burn_policy(network)
                    .is_some_and(|p| cell.lock_args.as_slice() == p.lock_args);
            let (cell_type, virtual_occupied_capacity) = if is_satoshi {
                (
                    Some("genesis_special_burn".to_string()),
                    Some(virtual_occupied.to_string()),
                )
            } else {
                (None, None)
            };

            Ok(TransactionOutputResponse {
                capacity: cell.capacity.to_string(),
                used_capacity: occupied as i64,
                virtual_used_capacity: virtual_occupied_capacity,
                cell_type,
                lock: Some(ScriptResponse {
                    code_hash: format!("0x{}", hex::encode(&cell.lock_code_hash)),
                    hash_type: lock_hash_type_str.to_string(),
                    args: format!("0x{}", hex::encode(&cell.lock_args)),
                }),
                r#type: type_script,
                address: script_to_address(
                    &cell.lock_code_hash,
                    cell.lock_hash_type,
                    &cell.lock_args,
                    network,
                )
                .ok(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let fee = compute_tx_fee_from_io(
        inputs_capacity,
        dao_compensation,
        outputs_capacity,
        resolved.is_cellbase,
        block_number,
        &resolved.tx_hash,
    )?;

    Ok(PendingTxIoBundle {
        inputs,
        outputs,
        inputs_capacity,
        outputs_capacity,
        inputs_used_capacity: inputs_occupied_capacity,
        outputs_used_capacity: outputs_occupied_capacity,
        fee,
        witnesses: resolved.witnesses.clone(),
        witnesses_available: true,
        dotcell_names: dotcell_names_for_tx(rpc_tx)?,
    })
}

/// Build inputs/outputs from CKB node's RocksDB transaction view.
#[allow(clippy::too_many_arguments)]
fn build_inputs_outputs_from_ckb(
    tx_view: &ckb_types::core::TransactionView,
    ckb_store: &ckb_store_reader::CkbChainReader,
    core_store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
    store: &ckbadger_store::CkbadgerStore,
    network: &str,
    block_number: i64,
    virtual_occupied: i128,
) -> Result<TxIoBundle, ApiRouteError> {
    let rpc_tx = ckb_store_reader::convert_transaction_view(tx_view);
    let witnesses = rpc_tx.witnesses.clone();
    let dotcell_names = dotcell_names_for_tx(&rpc_tx)?;

    let mut inputs_capacity: u128 = 0;
    let mut inputs_occupied_capacity: u128 = 0;

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

            // Cellbase input: prev_tx_hash is all zeros, index is 0xffffffff.
            // No previous cell exists to look up.
            let is_cellbase_input = prev_tx_hash_bytes.iter().all(|&b| b == 0);

            let (capacity, lock, type_script, address) = if is_cellbase_input {
                (None, None, None, None)
            } else {
                // Try live cells first, then consumed cells in our store
                let prev_index_i16 = i16::try_from(prev_index).map_err(|_| {
                    ApiError::internal(format!(
                        "output index {} exceeds i16 range while {}",
                        prev_index, input_context
                    ))
                })?;
                let cell_info = core_store
                    .get_cell(&prev_tx_hash_bytes, prev_index_i16, cells_store)
                    .ok()
                    .flatten()
                    .or_else(|| {
                        core_store
                            .get_consumed_cell(&prev_tx_hash_bytes, prev_index_i16, cells_store)
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

                        let lock_hash_type_str =
                            hash_type_to_str(info.lock_hash_type).ok_or_else(|| {
                                ApiError::internal(format!(
                                    "unknown lock hash_type {} for input cell",
                                    info.lock_hash_type
                                ))
                            })?;
                        let lock_resp = ScriptResponse {
                            code_hash: format!("0x{}", hex::encode(&info.lock_code_hash)),
                            hash_type: lock_hash_type_str.to_string(),
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

                let is_satoshi = block_number == 0
                    && ckbadger_common::burn_policy::burn_policy(network)
                        .is_some_and(|p| args_bytes.as_slice() == p.lock_args);
                let (cell_type, virtual_occupied_capacity) = if is_satoshi {
                    (
                        Some("genesis_special_burn".to_string()),
                        Some(virtual_occupied.to_string()),
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

    Ok((
        inputs,
        outputs,
        inputs_capacity,
        outputs_capacity,
        inputs_occupied_capacity,
        outputs_occupied_capacity,
        witnesses,
        true,
        dotcell_names,
    ))
}

/// Parse an out-point index as served by the CKB store reader (hex, usually
/// `0x`-prefixed). The value is node-derived, so a parse failure means
/// corrupted upstream data and must surface as an error, never as index 0.
fn parse_out_point_index(raw: &str) -> Result<i32, String> {
    let idx_str = raw.strip_prefix("0x").unwrap_or(raw);
    i32::from_str_radix(idx_str, 16).map_err(|e| e.to_string())
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
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;
    let mut tx_hash = [0u8; 32];
    tx_hash.copy_from_slice(&hash_bytes);

    // Cell deps are read from the CKB node's own RocksDB. Without that reader
    // the endpoint has no data source at all, so fail loudly instead of
    // shipping a silent `[]` that is indistinguishable from "no deps".
    let Some(store) = state.ckb_store.as_ref() else {
        return Err(ApiError::internal(format!(
            "cell deps for {} unavailable: CKB RocksDB reader is not open; check the configured CKB db path",
            hash
        )));
    };

    if let Some(tx_view) = store.get_transaction(&tx_hash) {
        let rpc_tx = ckb_store_reader::convert_transaction_view(&tx_view);
        let cell_deps: Vec<CellDepResponse> = rpc_tx
            .cell_deps
            .into_iter()
            .map(|dep| {
                let out_point_index = parse_out_point_index(&dep.out_point.index).map_err(|e| {
                    ApiError::internal(format!(
                        "malformed cell dep out_point index {:?} in tx {}: {}",
                        dep.out_point.index, hash, e
                    ))
                })?;
                Ok(CellDepResponse {
                    out_point_tx_hash: dep.out_point.tx_hash,
                    out_point_index,
                    dep_type: dep.dep_type,
                })
            })
            .collect::<Result<_, crate::response::ApiRouteError>>()?;
        return ok(cell_deps);
    }

    // Not in the node's chain data: either still in the mempool, or nonexistent.
    match fetch_transaction_lookup(&state.ckb_rpc_url, &hash)
        .await
        .map_err(ApiError::internal)?
    {
        Some(tx_lookup) if tx_lookup.is_pending_like() => Err(ApiError::bad_request(
            pending_transaction_resource_error(&hash, tx_lookup.status_str(), "Cell deps"),
        )),
        Some(tx_lookup) if tx_lookup.is_committed() => {
            // The node RPC sees the tx committed but the secondary reader does
            // not: catch-up lag or a stale reader path. A 404 would be a lie.
            Err(ApiError::internal(format!(
                "transaction {} is committed per node RPC but missing from the CKB RocksDB reader; reader catch-up lag or stale CKB db path",
                hash
            )))
        }
        _ => Err(ApiError::not_found("Transaction not found")),
    }
}

async fn get_cycles_status(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<CyclesStatusResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;

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
    // Optional so the handler still works in routers built without the
    // read-view middleware (unit tests mounting `routes()` directly).
    read_view: Option<Extension<RequestReadView>>,
) -> ApiResult<CyclesStatusResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;

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

            // This is the one handler whose contract is to observe the *next*
            // view: it waits for the indexer to write the cycles result, which
            // only becomes visible after a catch-up. Keeping the request's pin
            // would block that catch-up and guarantee a timeout, so release it
            // before waiting. Everything read above was pinned.
            if let Some(Extension(view)) = read_view {
                view.release();
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
    /// Main-chain block whose proposal zone carried this transaction's short id.
    /// For a proposal that arrived through an embedded uncle this is still the
    /// containing main-chain block — that is the block the commitment window is
    /// measured against — and `proposed_in_uncle` names the uncle itself.
    pub proposed_in: Option<LifecycleBlockInfo>,
    /// The embedded uncle that actually listed the short id, when the containing
    /// block's own proposal zone did not. `None` for directly proposed txs.
    pub proposed_in_uncle: Option<LifecycleUncleInfo>,
    pub committed_in: Option<LifecycleBlockInfo>,
    pub commitment_distance: Option<i64>,
    pub commitment_window: CommitmentWindow,
    pub is_cellbase: bool,
    pub confirmations: Option<i64>,
}

/// Identity of an uncle block that carried a transaction's proposal.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleUncleInfo {
    /// The uncle's own header block number (not the containing block's).
    pub block_number: i64,
    pub block_hash: String,
}

/// Where a committed transaction's proposal was found inside the
/// `[commit - 10, commit - 2]` window.
struct ProposalWindowHit {
    /// Main-chain block whose proposal zone (own or uncle-borne) carried the short id.
    block_number: i64,
    block_hash: Vec<u8>,
    timestamp: i64,
    /// `(uncle block number, uncle block hash)` when the short id came from an
    /// uncle embedded in `block_number`.
    uncle: Option<(i64, Vec<u8>)>,
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

#[instrument(skip(state), level = "debug")]
async fn get_transaction_lifecycle(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionLifecycleResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;

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
                proposed_in_uncle: None,
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
            proposed_in_uncle: None,
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
        tokio::task::spawn_blocking(move || -> Option<ProposalWindowHit> {
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
                let Ok(Some(header)) = store_c.get_block_header(bn) else {
                    continue;
                };
                if header.hash.len() != 32 {
                    continue;
                }
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&header.hash);
                let Some(block) = ckb_store_c.get_block(&hash) else {
                    continue;
                };
                let contains_short_id = |proposals: packed::ProposalShortIdVec| {
                    proposals
                        .into_iter()
                        .any(|id| id.raw_data().to_vec() == short_hash_c)
                };
                // The block's own proposal zone owns the attribution when both it and
                // one of its uncles carry the short id.
                if contains_short_id(block.data().proposals()) {
                    return Some(ProposalWindowHit {
                        block_number: bn,
                        block_hash: header.hash.clone(),
                        timestamp: header.timestamp,
                        uncle: None,
                    });
                }
                // CKB consensus counts the proposal zones of the uncles a block
                // embeds, so a tx can legitimately be proposed only inside an uncle.
                for uncle in block.uncles() {
                    if contains_short_id(uncle.data().proposals()) {
                        return Some(ProposalWindowHit {
                            block_number: bn,
                            block_hash: header.hash.clone(),
                            timestamp: header.timestamp,
                            uncle: Some((uncle.number() as i64, uncle.hash().raw_data().to_vec())),
                        });
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

    let (proposed_in_info, proposed_in_uncle, commitment_distance) = match proposed_in {
        Some(hit) => {
            let ts = chrono::DateTime::from_timestamp_millis(hit.timestamp)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();
            (
                Some(LifecycleBlockInfo {
                    block_number: hit.block_number,
                    block_hash: format!("0x{}", hex::encode(&hit.block_hash)),
                    timestamp: ts,
                }),
                hit.uncle
                    .map(|(uncle_number, uncle_hash)| LifecycleUncleInfo {
                        block_number: uncle_number,
                        block_hash: format!("0x{}", hex::encode(&uncle_hash)),
                    }),
                Some(commit_block_number - hit.block_number),
            )
        }
        None => (None, None, None),
    };

    ok(TransactionLifecycleResponse {
        hash: hash_hex,
        phase: LifecyclePhase::Committed,
        proposal_id: proposal_id_hex,
        proposed_in: proposed_in_info,
        proposed_in_uncle,
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
    fn test_parse_out_point_index_valid_and_malformed() {
        assert_eq!(parse_out_point_index("0x0"), Ok(0));
        assert_eq!(parse_out_point_index("0x2"), Ok(2));
        assert_eq!(parse_out_point_index("0xff"), Ok(255));
        assert_eq!(parse_out_point_index("5"), Ok(5));
        // Malformed node-derived data must error, never default to index 0.
        assert!(parse_out_point_index("0xzz").is_err());
        assert!(parse_out_point_index("").is_err());
        assert!(parse_out_point_index("0x").is_err());
    }

    /// Regression: the `txSize` a response serves and the `feeRate` it serves
    /// must be reproducible from each other. They were not — `feeRate` used the
    /// protocol's serialized-size-in-block while `txSize` reported the bare
    /// molecule size, so a client (and the frontend, in three places) recomputing
    /// `fee / txSize` got a different rate than the API's own field, and the
    /// size disagreed with the node and official explorer by 4 bytes.
    ///
    /// Vector: mainnet tx 0xdf615176… — molecule 981 bytes, fee 2074 shannons.
    /// The node/explorer size is 985 and the protocol fee rate is 2105.
    #[test]
    fn fee_rate_is_reproducible_from_the_served_tx_size() {
        let molecule_size = 981i32;
        let fee: u128 = 2074;

        let served_size = tx_serialized_size_in_block(molecule_size);
        assert_eq!(served_size, 985, "must match the node/explorer size");

        let served_rate = tx_fee_rate(fee, served_size);
        assert_eq!(served_rate, "2105");
        assert_eq!(
            served_rate,
            (fee * 1000 / u128::try_from(served_size).unwrap()).to_string(),
            "feeRate must equal fee * 1000 / txSize using the served txSize"
        );
    }

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
            cycles_status: None,
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
        assert_eq!(params.limit, 50);
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
        assert_eq!(hash_type_to_str(0), Some("data"));
        assert_eq!(hash_type_to_str(1), Some("type"));
        assert_eq!(hash_type_to_str(2), Some("data1"));
        assert_eq!(hash_type_to_str(4), Some("data2"));
        assert_eq!(hash_type_to_str(99), None);
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
            pool_status: None,
            interpretation: None,
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
            cycles_status: None,
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
            dotcell_names: vec![],
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
        let fee = compute_tx_fee_from_io(1_000, 0, 950, false, Some(10), &[0x11; 32]).unwrap();
        assert_eq!(fee, 50);
    }

    #[test]
    fn test_compute_tx_fee_from_io_adds_dao_compensation_to_inputs() {
        // Outputs exceed raw inputs because the withdrawal pays compensation.
        let fee = compute_tx_fee_from_io(1_000, 150, 1_100, false, None, &[0x22; 32]).unwrap();
        assert_eq!(fee, 50);
        // Compensation smaller than the fee: raw inputs still exceed outputs,
        // and the compensation must STILL be counted, not dropped.
        let fee = compute_tx_fee_from_io(1_000, 10, 950, false, None, &[0x22; 32]).unwrap();
        assert_eq!(fee, 60);
    }

    #[test]
    fn test_compute_tx_fee_from_io_errors_when_non_dao_outputs_exceed_inputs() {
        let err =
            compute_tx_fee_from_io(1_000, 0, 1_100, false, Some(10), &[0x33; 32]).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.1 .0.message.contains("inputs/outputs invariant broken"));
    }

    /// A script replay that failed is persisted as the `-1` marker and must be
    /// served as `failed` — never as a `done` cycle count. Guards the surface
    /// that reported an aborted Nervos DAO script group as an authoritative
    /// total.
    #[test]
    fn test_derive_cycles_status_never_reports_failed_replay_as_done() {
        assert_eq!(
            derive_cycles_status(Some(-1), false),
            Some("failed".to_string())
        );
        // Not yet calculated stays pending, so the worker can pick it up.
        assert_eq!(
            derive_cycles_status(None, false),
            Some("pending".to_string())
        );
        // A successful replay is the only case that reports a number.
        assert_eq!(derive_cycles_status(Some(3_380_228), false), None);
        // Cellbase transactions run no scripts; consensus counts them as 0.
        assert_eq!(derive_cycles_status(Some(0), true), None);
    }

    // ── `.cell` records on the tx page ───────────────────────────────────
    // Chain data copied from `crates/indexer/src/parser/dotcell_fixtures.rs`
    // (`T2_REGISTER_JOAOM`, testnet tx 0x89191ea4…386b at block 22471181;
    // `M1_RING_ROOT`, mainnet tx 0x219d1540…15ed), node-verified 2026-09-24.
    // That module is `#[cfg(test)]` in the indexer crate and not reachable here.

    const T2_TX_HASH: &str = "0x89191ea4bae150f82521140968fae918040e724f647eadec02e02077d748386b";
    /// `ACCOUNT_LOCK_CODE_HASH_TESTNET`.
    const T2_ACCOUNT_LOCK: &str =
        "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd";
    /// `ACCOUNT_TYPE_CODE_HASH_TESTNET`.
    const T2_ACCOUNT_TYPE: &str =
        "0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9";
    /// `NAMESPACE_ARGS_TESTNET`.
    const T2_NAMESPACE_ARGS: &str = "0x2510c78057479c9b023fe6e98ce43979e92a1353";
    /// `T2_OUT0_DATA`: `maria.cell`, whose records are in witness 0.
    const T2_OUT0_DATA: &str = "0x033b339494bd0e29b77c0dca959bc4d6d87e4ac232bd7df9c1335163fe85f5eb18241e3586a41eb75dd6d68bf555acea74d6649ed529da856c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b1416d61726961";
    /// `T2_WITNESS_0`: WitnessArgs whose output_type is maria's six records.
    const T2_WITNESS_0: &str = "0xc901000010000000100000001c000000080000007265676973746572a901000006000b616464726573732e333039006400636b7431717266727763646e76737373776477706e337339763866703837656d617433303663746a77736d336e6d6c6b6a673871797a61326371677171397837357a75346c37676c64363036723665796430306d346c7a79337a6b786b71346e79777a752c01000009616464726573732e30003e00746231703237767464306a776d3235746478336d36746763757168346c6578756c3076736c7572683079637a633766393272667030736b737264737936742c0100000a616464726573732e3630002a003078656646324634436132444536656444363364416665343833383536463134453639346437333144442c0100000d70726f66696c652e656d61696c0011006d61726961406578616d706c652e636f6d2c0100000d70726f66696c652e70686f6e650010002b3335312039313220333435203637382c0100000a647765622e636b626673004800636b6266733a2f2f346266306362646261633066386538656231616664646534333963336263306234333731623366336165636538633730343866656630366566366563376231302c010000";
    /// `M1_OUT0_DATA`: the mainnet ring root (empty label).
    const M1_RING_ROOT_DATA: &str = "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e20000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
    /// `M1_RING_ROOT.witnesses[0]`: a secp witness whose output_type is the
    /// empty records payload the root's records hash commits to.
    const M1_WITNESS_0: &str = "0x5b00000010000000550000005500000041000000fee51268d7edab7259e6f1fb460d2d523859de3e132f0910daa784ae288bc6295b666e226cca500c490e193bf5a412d68f3b5d1dbe9c7695c5934d2954420db500020000000000";

    fn rpc_script(code_hash: &str, args: &str) -> ckb_store_reader::RpcScript {
        ckb_store_reader::RpcScript {
            code_hash: code_hash.to_string(),
            hash_type: "type".to_string(),
            args: args.to_string(),
        }
    }

    fn rpc_tx(
        outputs: Vec<(ckb_store_reader::RpcCellOutput, &str)>,
        witnesses: &[&str],
    ) -> RpcTransactionView {
        RpcTransactionView {
            hash: T2_TX_HASH.to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs_data: outputs.iter().map(|(_, data)| data.to_string()).collect(),
            outputs: outputs.into_iter().map(|(output, _)| output).collect(),
            witnesses: witnesses.iter().map(|w| w.to_string()).collect(),
        }
    }

    fn name_output(
        lock: &str,
        type_code_hash: &str,
        args: &str,
    ) -> ckb_store_reader::RpcCellOutput {
        ckb_store_reader::RpcCellOutput {
            capacity: "0x59682f000".to_string(),
            lock: rpc_script(lock, "0x"),
            type_: Some(rpc_script(type_code_hash, args)),
        }
    }

    #[test]
    fn test_dotcell_names_from_rpc_tx() {
        let tx = rpc_tx(
            vec![(
                name_output(T2_ACCOUNT_LOCK, T2_ACCOUNT_TYPE, T2_NAMESPACE_ARGS),
                T2_OUT0_DATA,
            )],
            &[T2_WITNESS_0],
        );

        let names = dotcell_names_for_tx(&tx).expect("maria's records decode");
        assert_eq!(names.len(), 1);
        let maria = &names[0];
        assert_eq!(maria.output_index, 0);
        assert_eq!(maria.label, "maria");
        assert_eq!(maria.name, "maria.cell");
        // blake2b("maria")[..20] under CKB's default personalization.
        assert_eq!(
            maria.identity_id,
            "0x2224948f63975a7a0741139cd5d2a45b9fb02c03"
        );
        // The hash the cell carries in data[1..33].
        assert_eq!(maria.records_hash, format!("0x{}", &T2_OUT0_DATA[4..68]));
        assert_eq!(maria.records.len(), 6);
        assert_eq!(maria.records[0].key, "address.309");
        let decoded = maria.records[0]
            .decoded_address
            .as_ref()
            .expect("address.309 is a CKB address");
        assert!(decoded.address.starts_with("ckt1"), "{}", decoded.address);
        assert_eq!(maria.records[5].key, "dweb.ckbfs");
        assert!(maria.records[5]
            .value_utf8
            .as_deref()
            .is_some_and(|v| v.starts_with("ckbfs://")));
    }

    #[test]
    fn test_dotcell_names_hash_mismatch_is_an_error() {
        // One payload byte changed (the last record's ttl): the payload no
        // longer hashes to what the cell commits to.
        let tampered = format!("{}1", &T2_WITNESS_0[..T2_WITNESS_0.len() - 1]);
        let tx = rpc_tx(
            vec![(
                name_output(T2_ACCOUNT_LOCK, T2_ACCOUNT_TYPE, T2_NAMESPACE_ARGS),
                T2_OUT0_DATA,
            )],
            &[&tampered],
        );

        let err = dotcell_names_for_tx(&tx).unwrap_err();
        let message = err.1 .0.message;
        assert!(message.contains("records hash mismatch"), "{message}");
        assert!(message.contains("output_index=0"), "{message}");
    }

    #[test]
    fn test_dotcell_names_empty_for_plain_tx() {
        let plain = ckb_store_reader::RpcCellOutput {
            capacity: "0x2540be400".to_string(),
            lock: ckb_store_reader::RpcScript {
                code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                    .to_string(),
                hash_type: "type".to_string(),
                args: "0x23870b08ec5f6260c50a63646170d61e26d155c7".to_string(),
            },
            type_: None,
        };
        let tx = rpc_tx(vec![(plain, "0x")], &[]);

        assert!(dotcell_names_for_tx(&tx).unwrap().is_empty());
    }

    /// The ring root is protocol infrastructure, not a name: its (empty)
    /// records payload is still verified against its hash, but it is not
    /// listed as a name with an identity.
    #[test]
    fn test_dotcell_names_skip_the_ring_root_after_verifying_it() {
        let root = name_output(
            "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            "0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54",
            "0xb4f4302965b7d6421481a520ee7eb5971a5e808c",
        );
        let tx = rpc_tx(vec![(root.clone(), M1_RING_ROOT_DATA)], &[M1_WITNESS_0]);
        assert!(dotcell_names_for_tx(&tx).unwrap().is_empty());

        // A root with no witness at its index is still an error.
        let tx = rpc_tx(vec![(root, M1_RING_ROOT_DATA)], &[]);
        let err = dotcell_names_for_tx(&tx).unwrap_err();
        assert!(
            err.1 .0.message.contains("output_index=0"),
            "{}",
            err.1 .0.message
        );
    }
}
