#![allow(clippy::type_complexity)]
#![allow(clippy::manual_is_multiple_of)]

use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use ckbadger_common::dao::{
    is_genesis_special_burn_cell, GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED,
};

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::cache::{CacheKeys, CacheTtl};
use crate::response::{
    decode_cursor, encode_cursor, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::utils::{
    address_to_lock_script_hash, is_ckb_address, script_to_address, shannon_to_ckb,
};
use crate::AppState;
use ckbadger_store::keys;

const SHANNONS_PER_CKB: i64 = 100_000_000;

struct DepGroupParseResult {
    is_dep_group: bool,
    items: Option<Vec<DepGroupItem>>,
}

fn parse_dep_group(data: &[u8], data_size: i32) -> DepGroupParseResult {
    let full_size = data_size as usize;

    // OutPointVec format: 4 bytes count + N * 36 bytes OutPoints
    if full_size < 40 || (full_size - 4) % 36 != 0 {
        return DepGroupParseResult {
            is_dep_group: false,
            items: None,
        };
    }

    if data.len() < 4 {
        return DepGroupParseResult {
            is_dep_group: false,
            items: None,
        };
    }

    let count = match data[0..4].try_into().ok().map(u32::from_le_bytes) {
        Some(c) => c as usize,
        None => {
            return DepGroupParseResult {
                is_dep_group: false,
                items: None,
            }
        }
    };

    let expected_size = 4 + count * 36;
    if count == 0 || count > 256 || expected_size != full_size {
        return DepGroupParseResult {
            is_dep_group: false,
            items: None,
        };
    }

    // At this point we know it's a valid dep group format
    let mut items = Vec::with_capacity(count);
    for i in 0..count {
        let offset = 4 + i * 36;
        if offset + 36 > data.len() {
            break;
        }
        let tx_hash = format!("0x{}", hex::encode(&data[offset..offset + 32]));
        if let Some(index) = data[offset + 32..offset + 36]
            .try_into()
            .ok()
            .map(u32::from_le_bytes)
        {
            items.push(DepGroupItem {
                tx_hash,
                output_index: index,
            });
        }
    }

    DepGroupParseResult {
        is_dep_group: true,
        items: if items.len() == count {
            Some(items)
        } else {
            None // Data truncated, can't return complete list
        },
    }
}

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
            "/addresses/{addr}/stats-history",
            get(get_address_stats_history),
        )
}

#[derive(Debug, Deserialize)]
pub struct ListCellsParams {
    #[serde(default = "default_limit")]
    limit: i64,
    lock_script_hash: Option<String>,
    type_script_hash: Option<String>,
    type_code_hash: Option<String>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListCellsByScriptParams {
    #[serde(default = "default_limit")]
    limit: i64,
    code_hash: String,
    hash_type: String,
    #[serde(default = "default_script_kind")]
    script_kind: String,
    #[allow(dead_code)]
    cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

fn default_script_kind() -> String {
    "both".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellResponse {
    pub tx_hash: String,
    pub output_index: i32,
    pub capacity: String,
    pub lock_script_hash: String,
    pub type_script_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_code_hash: Option<String>,
    pub data_size: i32,
    pub created_at_block: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_occupied_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udt_amount: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResponse {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepGroupItem {
    pub tx_hash: String,
    pub output_index: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellScript {
    pub name: String,
    pub code_hash: String,
    pub hash_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoInfo {
    pub is_dao_cell: bool,
    pub dao_status: String,
    pub deposit_block_number: i64,
    pub deposit_timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdraw_request_block: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdraw_request_timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdraw_block: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdraw_timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compensation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compensation_ckb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_apc: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDetailResponse {
    pub tx_hash: String,
    pub output_index: i32,
    pub capacity: String,
    pub occupied_capacity: i64,
    pub occupied_capacity_breakdown: OccupiedCapacityBreakdown,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_occupied_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_type: Option<String>,
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub type_script_hash: Option<String>,
    pub data_size: i32,
    pub created_at_block: i64,
    pub status: String,
    pub consumed_at_block: Option<i64>,
    pub consumed_by_tx: Option<String>,
    pub lock: ScriptResponse,
    #[serde(rename = "type")]
    pub type_script: Option<ScriptResponse>,
    pub data: Option<String>,
    pub is_dep_group: bool,
    pub dep_group_items: Option<Vec<DepGroupItem>>,
    pub code_cell_of: Option<Vec<CodeCellScript>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dao_info: Option<DaoInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OccupiedCapacityBreakdown {
    pub capacity_field_bytes: i64,
    pub lock_script_bytes: i64,
    pub type_script_bytes: i64,
    pub data_bytes: i64,
    pub total_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockScriptInfo {
    pub code_hash: String,
    pub name: String,
    pub script_kind: Option<String>,
    pub deprecated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressResponse {
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub balance: String,
    pub occupied_capacity: String,
    pub live_cells_count: i64,
    pub transactions_count: i64,
    pub recent_activities_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_script: Option<ScriptResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_script_info: Option<LockScriptInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopAddressResponse {
    pub lock_script_hash: String,
    pub balance: String,
    pub live_cells_count: i32,
    pub transactions_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct TopAddressesParams {
    #[serde(default = "default_top_limit")]
    limit: i64,
}

fn default_top_limit() -> i64 {
    100
}

#[derive(Debug, Deserialize)]
pub struct ActiveAddressesParams {
    #[serde(default = "default_top_limit")]
    limit: i64,
    #[serde(default = "default_days")]
    days: i64,
}

fn default_days() -> i64 {
    7
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAddressResponse {
    pub lock_script_hash: String,
    pub balance: String,
    pub live_cells_count: i32,
    pub transactions_count: i64,
    pub last_activity_block: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressTransactionResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_type: String,
    pub capacity_change: String,
    pub timestamp: String,
    pub inputs_count: i16,
    pub outputs_count: i16,
    pub fee: String,
    pub is_cellbase: bool,
    pub tx_size: Option<i32>,
    pub cycles: Option<i64>,
    pub script_labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddressTxParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddressTokensParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[allow(dead_code)]
    cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressTokenResponse {
    pub type_script_hash: String,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: i16,
    pub icon_url: Option<String>,
    pub balance: String,
}

/// Helper to convert a LiveCellInfo into a CellResponse.
fn cell_info_to_response(
    tx_hash: &[u8],
    output_index: i16,
    info: &ckbadger_store::LiveCellInfo,
) -> CellResponse {
    let is_special_burn = is_genesis_special_burn_cell(&info.lock_args, info.created_at_block);
    CellResponse {
        tx_hash: format!("0x{}", hex::encode(tx_hash)),
        output_index: output_index as i32,
        capacity: info.capacity.to_string(),
        lock_script_hash: format!("0x{}", hex::encode(&info.lock_script_hash)),
        type_script_hash: info
            .type_script_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        type_code_hash: info
            .type_code_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        data_size: info.data_size,
        created_at_block: info.created_at_block,
        cell_type: if is_special_burn {
            Some("genesis_special_burn".to_string())
        } else {
            None
        },
        virtual_occupied_capacity: if is_special_burn {
            Some(GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED.to_string())
        } else {
            None
        },
        udt_amount: None,
    }
}

fn estimated_occupied_capacity_breakdown(
    info: &ckbadger_store::LiveCellInfo,
) -> OccupiedCapacityBreakdown {
    let capacity_field_bytes = 8;
    let lock_script_bytes = 32 + 1 + info.lock_args.len() as i64;
    let type_script_bytes = if info.type_code_hash.is_some() {
        32 + 1 + info.type_args.as_ref().map_or(0, |args| args.len() as i64)
    } else {
        0
    };
    let data_bytes = info.data_size as i64;
    let total_bytes = capacity_field_bytes + lock_script_bytes + type_script_bytes + data_bytes;

    OccupiedCapacityBreakdown {
        capacity_field_bytes,
        lock_script_bytes,
        type_script_bytes,
        data_bytes,
        total_bytes,
    }
}

/// Decode a cell cursor (hex-encoded full cell index key).
fn decode_cell_cursor(cursor: &str) -> Option<Vec<u8>> {
    hex::decode(cursor.strip_prefix("0x").unwrap_or(cursor)).ok()
}

/// Encode a cell cursor from the last result's components.
fn encode_cell_cursor(
    script_hash: &[u8],
    block_num: i64,
    tx_hash: &[u8],
    output_index: i16,
) -> String {
    let key = keys::encode_cell_index_key(script_hash, block_num, tx_hash, output_index);
    hex::encode(key)
}

async fn list_live_cells(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListCellsParams>,
) -> ApiResult<CursorPaginatedResponse<CellResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    let after_key = params.cursor.as_deref().and_then(decode_cell_cursor);

    let lock_hash_bytes = if let Some(ref lock_hash) = params.lock_script_hash {
        Some(if is_ckb_address(lock_hash) {
            address_to_lock_script_hash(lock_hash)
                .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
        } else {
            hex::decode(lock_hash.strip_prefix("0x").unwrap_or(lock_hash))
                .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?
        })
    } else {
        None
    };

    let type_hash_bytes = if let Some(ref type_hash) = params.type_script_hash {
        Some(
            hex::decode(type_hash.strip_prefix("0x").unwrap_or(type_hash))
                .map_err(|_| ApiError::bad_request("Invalid type script hash"))?,
        )
    } else {
        None
    };

    let _type_code_hash_bytes = if let Some(ref code_hash) = params.type_code_hash {
        Some(
            hex::decode(code_hash.strip_prefix("0x").unwrap_or(code_hash))
                .map_err(|_| ApiError::bad_request("Invalid type code hash"))?,
        )
    } else {
        None
    };

    let after_key_ref = after_key.as_deref();

    // Fetch cells from the store based on available filters.
    // The store supports listing by lock hash or type hash via prefix scans.
    let raw_cells: Vec<(Vec<u8>, i16, ckbadger_store::LiveCellInfo)> =
        match (&lock_hash_bytes, &type_hash_bytes) {
            (Some(lock_bytes), Some(type_bytes)) => {
                // Filter by lock first (usually more selective), then post-filter by type
                let all = state
                    .store
                    .list_cells_by_lock(lock_bytes, limit * 10 + 1, after_key_ref)
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                all.into_iter()
                    .filter(|(_, _, info)| {
                        info.type_script_hash
                            .as_ref()
                            .map(|h| h == type_bytes)
                            .unwrap_or(false)
                    })
                    .take(limit + 1)
                    .collect()
            }
            (Some(lock_bytes), None) => {
                // For type_code_hash filtering, list by lock then post-filter
                if let Some(ref tch) = _type_code_hash_bytes {
                    let all = state
                        .store
                        .list_cells_by_lock(lock_bytes, limit * 10 + 1, after_key_ref)
                        .map_err(|e| ApiError::internal(e.to_string()))?;
                    all.into_iter()
                        .filter(|(_, _, info)| {
                            info.type_code_hash
                                .as_ref()
                                .map(|h| h == tch)
                                .unwrap_or(false)
                        })
                        .take(limit + 1)
                        .collect()
                } else {
                    state
                        .store
                        .list_cells_by_lock(lock_bytes, limit + 1, after_key_ref)
                        .map_err(|e| ApiError::internal(e.to_string()))?
                }
            }
            (None, Some(type_bytes)) => state
                .store
                .list_cells_by_type(type_bytes, limit + 1, after_key_ref)
                .map_err(|e| ApiError::internal(e.to_string()))?,
            (None, None) => {
                // No filter: not practical for RocksDB full scan, return empty.
                // The old PG query scanned the whole table; in RocksDB we can't
                // efficiently paginate the full live_cells CF without a secondary index.
                Vec::new()
            }
        };

    let has_more = raw_cells.len() > limit;
    let raw_cells: Vec<_> = raw_cells.into_iter().take(limit).collect();

    // Determine which script hash was used as the index prefix for cursor encoding
    let index_hash = lock_hash_bytes.as_deref().or(type_hash_bytes.as_deref());

    let next_cursor = if has_more {
        raw_cells.last().and_then(|(tx_hash, output_index, info)| {
            index_hash.map(|h| encode_cell_cursor(h, info.created_at_block, tx_hash, *output_index))
        })
    } else {
        None
    };

    let cells: Vec<CellResponse> = raw_cells
        .iter()
        .map(|(tx_hash, output_index, info)| cell_info_to_response(tx_hash, *output_index, info))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        cells,
        limit as i64,
        next_cursor,
    ))
}

fn parse_hash_type(hash_type: &str) -> Option<u8> {
    match hash_type {
        "data" => Some(0),
        "type" => Some(1),
        "data1" => Some(2),
        "data2" => Some(4),
        _ => None,
    }
}

async fn list_cells_by_script(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListCellsByScriptParams>,
) -> ApiResult<CursorPaginatedResponse<CellResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    let code_hash_bytes = hex::decode(
        params
            .code_hash
            .strip_prefix("0x")
            .unwrap_or(&params.code_hash),
    )
    .map_err(|_| ApiError::bad_request("Invalid code_hash hex"))?;

    let _hash_type_num = parse_hash_type(&params.hash_type).ok_or_else(|| {
        ApiError::bad_request("Invalid hash_type. Must be one of: data, type, data1, data2")
    })?;

    let script_kind = params.script_kind.as_str();

    // Look up script info from the store to get pre-aggregated count
    let script_info = state
        .store
        .get_script_info(&code_hash_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let total: i64 = match (script_kind, &script_info) {
        ("lock", Some(si)) => si.lock_live_cells_count,
        ("type", Some(si)) => si.type_live_cells_count,
        (_, Some(si)) => si.lock_live_cells_count + si.type_live_cells_count,
        (_, None) => 0,
    };

    // Parse cursor for pagination
    let after_key = params.cursor.as_deref().and_then(decode_cell_cursor);
    let after_key_ref = after_key.as_deref();

    // Fetch limit+1 to detect has_more
    let fetch_limit = limit + 1;

    // Use code_hash indexes for efficient prefix scans
    let results: Vec<(Vec<u8>, i16, ckbadger_store::LiveCellInfo)> = match script_kind {
        "lock" => state
            .store
            .list_cells_by_lock_code_hash(&code_hash_bytes, fetch_limit, after_key_ref)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        "type" => state
            .store
            .list_cells_by_type_code_hash(&code_hash_bytes, fetch_limit, after_key_ref)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        _ => {
            // "both": merge results from lock and type indexes
            let mut merged = state
                .store
                .list_cells_by_lock_code_hash(&code_hash_bytes, fetch_limit, after_key_ref)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let type_results = state
                .store
                .list_cells_by_type_code_hash(&code_hash_bytes, fetch_limit, after_key_ref)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            for r in type_results {
                if merged.len() >= fetch_limit {
                    break;
                }
                // Deduplicate: a cell could match both lock and type
                if !merged.iter().any(|(h, i, _)| h == &r.0 && *i == r.1) {
                    merged.push(r);
                }
            }
            merged
        }
    };

    let has_more = results.len() > limit;
    let results: Vec<_> = results.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        results.last().map(|(tx_hash, output_index, info)| {
            encode_cell_cursor(
                &code_hash_bytes,
                info.created_at_block,
                tx_hash,
                *output_index,
            )
        })
    } else {
        None
    };

    let cells: Vec<CellResponse> = results
        .iter()
        .map(|(tx_hash, output_index, info)| cell_info_to_response(tx_hash, *output_index, info))
        .collect();

    ok(CursorPaginatedResponse::new(
        cells,
        total,
        limit as i64,
        next_cursor,
    ))
}

async fn get_address(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
) -> ApiResult<AddressResponse> {
    // Check cache first
    let cache_key = CacheKeys::address_balance(&addr);
    if let Some(cached) = state.cache.get::<AddressResponse>(&cache_key).await {
        return ok(cached);
    }

    let (lock_hash, input_address) = if is_ckb_address(&addr) {
        let hash = address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?;
        (hash, Some(addr.clone()))
    } else {
        let hash = hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?;
        (hash, None)
    };

    // Get balance from the store
    let addr_balance = state
        .store
        .get_addr_balance(&lock_hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (balance, occupied_capacity, live_cells_count, transactions_count) = match &addr_balance {
        Some(ab) => (
            ab.balance.to_string(),
            ab.occupied_capacity.to_string(),
            ab.live_cells_count as i64,
            ab.txs_count,
        ),
        None => ("0".to_string(), "0".to_string(), 0, 0),
    };

    // Try to find a cell for this lock hash to get the lock script details
    let cells_for_script = state
        .store
        .list_cells_by_lock(&lock_hash, 1, None)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (lock_script, address) = if let Some((_, _, info)) = cells_for_script.first() {
        // LiveCellInfo doesn't store hash_type directly; derive from code_hash via script_info
        let hash_type_num = state
            .store
            .get_script_info(&info.lock_code_hash)
            .ok()
            .flatten()
            .map(|si| si.hash_type as i16)
            .unwrap_or(1); // Default to "type"

        let hash_type_str = match hash_type_num {
            0 => "data",
            1 => "type",
            2 => "data1",
            4 => "data2",
            _ => "data",
        };

        let script = ScriptResponse {
            code_hash: format!("0x{}", hex::encode(&info.lock_code_hash)),
            hash_type: hash_type_str.to_string(),
            args: format!("0x{}", hex::encode(&info.lock_args)),
        };

        let addr = input_address.or_else(|| {
            script_to_address(
                &info.lock_code_hash,
                hash_type_num,
                &info.lock_args,
                &state.ckb_network,
            )
            .ok()
        });

        (Some(script), addr)
    } else {
        // No live cells found, also check consumed cells for script info.
        // For now, just return what we have.
        (None, input_address)
    };

    // Look up lock script info from script_info CF
    let lock_script_info = if let Some((_, _, info)) = cells_for_script.first() {
        state
            .store
            .get_script_info(&info.lock_code_hash)
            .ok()
            .flatten()
            .map(|si| LockScriptInfo {
                code_hash: format!("0x{}", hex::encode(&si.code_hash)),
                name: si.name.unwrap_or_else(|| "Unknown".to_string()),
                script_kind: Some("lock".to_string()),
                deprecated: false,
            })
    } else {
        None
    };

    let recent_activities_count = transactions_count;

    let response = AddressResponse {
        lock_script_hash: format!("0x{}", hex::encode(&lock_hash)),
        address,
        balance,
        occupied_capacity,
        live_cells_count,
        transactions_count,
        recent_activities_count,
        lock_script,
        lock_script_info,
    };

    // Cache the response for 30 seconds
    state
        .cache
        .set(&cache_key, &response, CacheTtl::ADDRESS_BALANCE)
        .await;

    ok(response)
}

fn lookup_code_cell_scripts(
    store: &ckbadger_store::CkbadgerStore,
    data_hash: &[u8],
    type_script_hash: Option<&Vec<u8>>,
) -> Option<Vec<CodeCellScript>> {
    let mut scripts = Vec::new();

    // Look up by data hash (for data/data1/data2 hash types)
    if let Ok(Some(si)) = store.get_script_info(data_hash) {
        let hash_type_str = match si.hash_type {
            0 => "data",
            2 => "data1",
            4 => "data2",
            _ => "data",
        };
        scripts.push(CodeCellScript {
            name: si.name.unwrap_or_else(|| "Unknown".to_string()),
            code_hash: format!("0x{}", hex::encode(&si.code_hash)),
            hash_type: hash_type_str.to_string(),
        });
    }

    // Look up by type script hash (for "type" hash type)
    if let Some(type_hash) = type_script_hash {
        if let Ok(Some(si)) = store.get_script_info(type_hash) {
            if si.hash_type == 1 {
                scripts.push(CodeCellScript {
                    name: si.name.unwrap_or_else(|| "Unknown".to_string()),
                    code_hash: format!("0x{}", hex::encode(&si.code_hash)),
                    hash_type: "type".to_string(),
                });
            }
        }
    }

    if scripts.is_empty() {
        None
    } else {
        Some(scripts)
    }
}

fn lookup_dao_info(
    store: &ckbadger_store::CkbadgerStore,
    tx_hash: &[u8],
    output_index: i16,
) -> Option<DaoInfo> {
    let outpoint_key = ckbadger_store::keys::encode_outpoint(tx_hash, output_index);

    let entry = store.get_dao_deposit(&outpoint_key).ok()?;

    // If not found by outpoint, try by withdraw_tx
    let entry = if entry.is_none() {
        let outpoint_key_data = store.get_dao_deposit_by_withdraw_tx(tx_hash).ok()?;
        if let Some(key_data) = outpoint_key_data {
            store.get_dao_deposit(&key_data).ok()?
        } else {
            None
        }
    } else {
        entry
    }?;

    let dao_status = match entry.status {
        0 => "deposited",
        1 => "withdrawing",
        2 => "withdrawn",
        _ => "unknown",
    }
    .to_string();

    // Get block header for deposit timestamp
    let deposit_timestamp = store
        .get_block_header(entry.deposit_block_number)
        .ok()
        .flatten()
        .map(|h| {
            chrono::DateTime::from_timestamp(h.timestamp / 1000, 0)
                .unwrap_or_default()
                .to_rfc3339()
        })
        .unwrap_or_default();

    let withdraw_request_timestamp = entry.withdraw_request_block.and_then(|bn| {
        store.get_block_header(bn).ok().flatten().map(|h| {
            chrono::DateTime::from_timestamp(h.timestamp / 1000, 0)
                .unwrap_or_default()
                .to_rfc3339()
        })
    });

    let withdraw_timestamp = entry.withdraw_block.and_then(|bn| {
        store.get_block_header(bn).ok().flatten().map(|h| {
            chrono::DateTime::from_timestamp(h.timestamp / 1000, 0)
                .unwrap_or_default()
                .to_rfc3339()
        })
    });

    let compensation = entry.compensation.map(|c| c.to_string());
    let compensation_ckb = compensation.as_ref().map(|c| shannon_to_ckb(c));

    Some(DaoInfo {
        is_dao_cell: true,
        dao_status,
        deposit_block_number: entry.deposit_block_number,
        deposit_timestamp,
        withdraw_request_block: entry.withdraw_request_block,
        withdraw_request_timestamp,
        withdraw_block: entry.withdraw_block,
        withdraw_timestamp,
        compensation,
        compensation_ckb,
        estimated_apc: None,
    })
}

async fn get_cell(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((tx_hash, output_index)): axum::extract::Path<(String, i32)>,
) -> ApiResult<CellDetailResponse> {
    let hash_bytes = hex::decode(tx_hash.strip_prefix("0x").unwrap_or(&tx_hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let output_idx = output_index as i16;

    // Try live cells first
    let live_cell = state
        .store
        .get_cell(&hash_bytes, output_idx)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Try consumed cells if not found in live
    let consumed_cell = if live_cell.is_none() {
        state
            .store
            .get_consumed_cell_info(&hash_bytes, output_idx)
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        None
    };

    let (info, status_str, consumed_meta) = match (live_cell, consumed_cell) {
        (Some(cell), _) => (cell, "live", None),
        (None, Some(cell)) => (
            cell.cell,
            "dead",
            Some((cell.consumed_at_block, cell.consumed_by_tx)),
        ),
        (None, None) => return Err(ApiError::not_found("Cell not found")),
    };

    // Use the cell's own stored hash_type (not from script_info, which is a canonical
    // default and may differ from the actual per-cell hash_type).
    let lock_hash_type_num: i16 = info.lock_hash_type;

    let hash_type_str = |ht: i16| match ht {
        0 => "data",
        1 => "type",
        2 => "data1",
        4 => "data2",
        _ => "data",
    };

    let type_script = info.type_code_hash.as_ref().map(|code_hash| {
        let type_hash_type_num: i16 = state
            .store
            .get_script_info(code_hash)
            .ok()
            .flatten()
            .map(|si| si.hash_type as i16)
            .unwrap_or(1);
        ScriptResponse {
            code_hash: format!("0x{}", hex::encode(code_hash)),
            hash_type: hash_type_str(type_hash_type_num).to_string(),
            args: format!(
                "0x{}",
                info.type_args
                    .as_ref()
                    .map_or_else(String::new, hex::encode)
            ),
        }
    });

    let address = script_to_address(
        &info.lock_code_hash,
        lock_hash_type_num,
        &info.lock_args,
        &state.ckb_network,
    )
    .ok();

    // For cell data (e.g. dep groups), read from CKB direct store if available
    let cell_data = state.ckb_store.as_ref().and_then(|ckb| {
        if hash_bytes.len() == 32 {
            let mut tx_h = [0u8; 32];
            tx_h.copy_from_slice(&hash_bytes);
            ckb.get_cell_data(&tx_h, output_index as u32)
        } else {
            None
        }
    });

    let dep_group_result = cell_data
        .as_ref()
        .map(|d| parse_dep_group(d, info.data_size))
        .unwrap_or(DepGroupParseResult {
            is_dep_group: false,
            items: None,
        });

    // Compute data_hash from cell data for code_cell lookup
    let data_hash = cell_data.as_ref().map(|d| {
        use ckb_hash::new_blake2b;
        let mut hasher = new_blake2b();
        hasher.update(d);
        let mut hash = vec![0u8; 32];
        hasher.finalize(&mut hash);
        hash
    });

    let code_cell_of = data_hash
        .as_ref()
        .and_then(|dh| lookup_code_cell_scripts(&state.store, dh, info.type_script_hash.as_ref()));

    let occupied_capacity_breakdown = estimated_occupied_capacity_breakdown(&info);
    let occupied_capacity = if info.occupied_capacity > 0 {
        info.occupied_capacity
    } else {
        occupied_capacity_breakdown
            .total_bytes
            .saturating_mul(SHANNONS_PER_CKB)
    };

    let is_satoshi = is_genesis_special_burn_cell(&info.lock_args, info.created_at_block);
    let (cell_type, virtual_occupied_capacity) = if is_satoshi {
        (
            Some("genesis_special_burn".to_string()),
            Some(GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED.to_string()),
        )
    } else {
        (None, None)
    };

    let (consumed_at_block, consumed_by_tx) = if let Some((block, tx)) = consumed_meta {
        let tx_hash = tx.map(|raw| format!("0x{}", hex::encode(raw)));
        (if block > 0 { Some(block) } else { None }, tx_hash)
    } else {
        (None, None)
    };

    let dao_info = lookup_dao_info(&state.store, &hash_bytes, output_idx);

    ok(CellDetailResponse {
        tx_hash: format!("0x{}", hex::encode(&hash_bytes)),
        output_index: output_idx as i32,
        capacity: info.capacity.to_string(),
        occupied_capacity,
        occupied_capacity_breakdown,
        virtual_occupied_capacity,
        cell_type,
        lock_script_hash: format!("0x{}", hex::encode(&info.lock_script_hash)),
        address,
        type_script_hash: info
            .type_script_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        data_size: info.data_size,
        created_at_block: info.created_at_block,
        status: status_str.to_string(),
        consumed_at_block,
        consumed_by_tx,
        lock: ScriptResponse {
            code_hash: format!("0x{}", hex::encode(&info.lock_code_hash)),
            hash_type: hash_type_str(lock_hash_type_num).to_string(),
            args: format!("0x{}", hex::encode(&info.lock_args)),
        },
        type_script,
        data: cell_data.map(|d| format!("0x{}", hex::encode(d))),
        is_dep_group: dep_group_result.is_dep_group,
        dep_group_items: dep_group_result.items,
        code_cell_of,
        dao_info,
    })
}

async fn get_top_addresses(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TopAddressesParams>,
) -> ApiResult<Vec<TopAddressResponse>> {
    let sync_status = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if sync_status.address_balances_deferred {
        return ok(Vec::new());
    }

    let limit = params.limit.clamp(1, 500) as usize;

    let rows = state
        .store
        .top_addresses(limit)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let addresses: Vec<TopAddressResponse> = rows
        .into_iter()
        .filter(|(_, ab)| ab.balance > 0)
        .map(|(lock_hash, ab)| TopAddressResponse {
            lock_script_hash: format!("0x{}", hex::encode(&lock_hash)),
            balance: ab.balance.to_string(),
            live_cells_count: ab.live_cells_count,
            transactions_count: ab.txs_count,
        })
        .collect();

    ok(addresses)
}

async fn get_active_addresses(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActiveAddressesParams>,
) -> ApiResult<Vec<ActiveAddressResponse>> {
    let sync_status = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if sync_status.address_balances_deferred {
        return ok(Vec::new());
    }

    let limit = params.limit.clamp(1, 500) as usize;
    let days = params.days.clamp(1, 365);

    let tip_block = sync_status.tip_block_number;
    let blocks_per_day: i64 = 8640;
    let min_block = tip_block.saturating_sub(days * blocks_per_day);

    // Full scan of addr_balance CF, filter by last_activity_block
    let iter = state
        .store
        .iterator_cf(state.store.cf_addr_balance(), rocksdb::IteratorMode::Start);

    let mut all: Vec<(Vec<u8>, ckbadger_store::AddressBalance)> = Vec::new();
    for item in iter.flatten() {
        let (key, value) = item;
        if let Ok(ab) = bincode::deserialize::<ckbadger_store::AddressBalance>(&value) {
            if ab.last_activity_block >= min_block {
                all.push((key.to_vec(), ab));
            }
        }
    }

    // Sort by last_activity_block desc
    all.sort_by(|a, b| b.1.last_activity_block.cmp(&a.1.last_activity_block));
    all.truncate(limit);

    let addresses: Vec<ActiveAddressResponse> = all
        .into_iter()
        .map(|(lock_hash, ab)| ActiveAddressResponse {
            lock_script_hash: format!("0x{}", hex::encode(&lock_hash)),
            balance: ab.balance.to_string(),
            live_cells_count: ab.live_cells_count,
            transactions_count: ab.txs_count,
            last_activity_block: ab.last_activity_block,
        })
        .collect();

    ok(addresses)
}

async fn get_address_transactions(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
    Query(params): Query<AddressTxParams>,
) -> ApiResult<CursorPaginatedResponse<AddressTransactionResponse>> {
    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    let cursor = params.cursor.as_ref().and_then(|c| decode_cursor(c));

    // Fetch recent transactions for this address (newest first)
    let addr_txs = state
        .store
        .list_addr_txs_recent(&lock_hash, limit + 1, cursor)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = addr_txs.len() > limit;
    let addr_txs: Vec<_> = addr_txs.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        addr_txs
            .last()
            .map(|(block_num, tx_idx, _)| encode_cursor(*block_num, *tx_idx))
    } else {
        None
    };

    let txs: Vec<AddressTransactionResponse> = addr_txs
        .into_iter()
        .map(|(block_number, tx_idx, tx_hash)| {
            let timestamp = state
                .store
                .get_block_header(block_number)
                .ok()
                .flatten()
                .map(|h| {
                    chrono::DateTime::from_timestamp_millis(h.timestamp)
                        .unwrap_or_default()
                        .to_rfc3339()
                })
                .unwrap_or_default();

            let tx_entry = state
                .store
                .get_tx_index(block_number, tx_idx)
                .ok()
                .flatten();
            let is_cellbase = tx_entry.as_ref().map(|e| e.is_cellbase).unwrap_or(false);
            let outputs_count = tx_entry.as_ref().map(|e| e.outputs_count).unwrap_or(0);

            // Compute capacity change: sum outputs to this address minus sum inputs from this address
            let mut output_capacity: i128 = 0;
            let mut input_capacity: i128 = 0;
            let mut has_outputs = false;
            let mut has_inputs = false;
            let mut script_code_hashes: std::collections::HashSet<Vec<u8>> =
                std::collections::HashSet::new();

            // Check outputs belonging to this address
            for idx in 0..outputs_count {
                let cell = state
                    .store
                    .get_cell(&tx_hash, idx)
                    .ok()
                    .flatten()
                    .or_else(|| state.store.get_consumed_cell(&tx_hash, idx).ok().flatten());
                if let Some(cell) = cell {
                    if let Some(ref tch) = cell.type_code_hash {
                        script_code_hashes.insert(tch.clone());
                    }
                    script_code_hashes.insert(cell.lock_code_hash.clone());
                    if cell.lock_script_hash == lock_hash {
                        output_capacity += cell.capacity as i128;
                        has_outputs = true;
                    }
                }
            }

            // Check inputs belonging to this address (resolve previous outpoints)
            let mut dao_compensation: i128 = 0;
            if !is_cellbase {
                if let Some(ref ckb_store) = state.ckb_store {
                    if tx_hash.len() == 32 {
                        let mut tx_hash_arr = [0u8; 32];
                        tx_hash_arr.copy_from_slice(&tx_hash);
                        if let Some(tx_view) = ckb_store.get_transaction(&tx_hash_arr) {
                            use ckb_types::prelude::*;
                            for input in tx_view.inputs().into_iter() {
                                let prev_hash: [u8; 32] =
                                    input.previous_output().tx_hash().unpack();
                                let prev_index: u32 = input.previous_output().index().unpack();
                                // Check if this input is a DAO withdrawal request
                                if let Ok(Some(outpoint_key)) =
                                    state.store.get_dao_deposit_by_withdraw_tx(&prev_hash)
                                {
                                    if let Ok(Some(entry)) =
                                        state.store.get_dao_deposit(&outpoint_key)
                                    {
                                        if let Some(comp) = entry.compensation {
                                            dao_compensation += comp as i128;
                                        }
                                    }
                                }
                                let cell = state
                                    .store
                                    .get_consumed_cell(&prev_hash, prev_index as i16)
                                    .ok()
                                    .flatten()
                                    .or_else(|| {
                                        state
                                            .store
                                            .get_cell(&prev_hash, prev_index as i16)
                                            .ok()
                                            .flatten()
                                    });
                                if let Some(cell) = cell {
                                    if let Some(ref tch) = cell.type_code_hash {
                                        script_code_hashes.insert(tch.clone());
                                    }
                                    script_code_hashes.insert(cell.lock_code_hash.clone());
                                    if cell.lock_script_hash == lock_hash {
                                        input_capacity += cell.capacity as i128;
                                        has_inputs = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let capacity_change = output_capacity - input_capacity;
            let tx_type = if has_inputs && has_outputs {
                if capacity_change < 0 {
                    "sent"
                } else if capacity_change > 0 {
                    "received"
                } else {
                    "internal"
                }
            } else if has_outputs {
                "received"
            } else if has_inputs {
                "sent"
            } else {
                "transfer"
            };

            let inputs_count = tx_entry.as_ref().map(|e| e.inputs_count).unwrap_or(0);
            let stored_fee = tx_entry.as_ref().map(|e| e.fee as i128).unwrap_or(0);
            // For DAO withdrawals, stored fee = actual_fee - compensation (negative).
            // Correct by adding back the DAO compensation.
            let fee = (stored_fee + dao_compensation).max(0) as i64;
            let tx_size = tx_entry.as_ref().map(|e| e.tx_size);
            let cycles = tx_entry.as_ref().and_then(|e| e.cycles);

            // Resolve script labels from collected code hashes (type + lock scripts)
            let mut script_labels: Vec<String> = script_code_hashes
                .iter()
                .filter_map(|ch| {
                    state
                        .store
                        .get_script_info(ch)
                        .ok()
                        .flatten()
                        .and_then(|si| si.name)
                })
                .filter(|name| {
                    // Filter out common lock scripts that aren't interesting as labels
                    !matches!(
                        name.as_str(),
                        "Default Lock" | "Default Multisig" | "anyone_can_pay"
                    )
                })
                .collect();
            script_labels.sort();
            script_labels.dedup();

            AddressTransactionResponse {
                tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                block_number,
                tx_type: tx_type.to_string(),
                capacity_change: capacity_change.to_string(),
                timestamp,
                inputs_count,
                outputs_count,
                fee: fee.to_string(),
                is_cellbase,
                tx_size,
                cycles,
                script_labels,
            }
        })
        .collect();

    ok(CursorPaginatedResponse::without_total(
        txs,
        limit as i64,
        next_cursor,
    ))
}

async fn get_address_tokens(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
    Query(params): Query<AddressTokensParams>,
) -> ApiResult<CursorPaginatedResponse<AddressTokenResponse>> {
    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    // Get all tokens and check balances for this address
    let all_tokens = state
        .store
        .list_tokens()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut token_balances: Vec<(Vec<u8>, ckbadger_store::TokenInfo, i128)> = Vec::new();

    for (type_hash, token_info) in all_tokens {
        if let Ok(Some(balance)) = state.store.get_token_holder_balance(&type_hash, &lock_hash) {
            if balance > 0 {
                token_balances.push((type_hash, token_info, balance));
            }
        }
    }

    // Sort by balance descending
    token_balances.sort_by(|a, b| b.2.cmp(&a.2));

    let has_more = token_balances.len() > limit;
    let token_balances: Vec<_> = token_balances.into_iter().take(limit).collect();

    let next_cursor: Option<String> = if has_more {
        token_balances
            .last()
            .map(|(type_hash, _, balance)| format!("{}:{}", balance, hex::encode(type_hash)))
    } else {
        None
    };

    let tokens: Vec<AddressTokenResponse> = token_balances
        .into_iter()
        .map(|(type_hash, token_info, balance)| AddressTokenResponse {
            type_script_hash: format!("0x{}", hex::encode(&type_hash)),
            standard: token_info.standard,
            name: token_info.name,
            symbol: token_info.symbol,
            decimals: token_info.decimals.unwrap_or(0) as i16,
            icon_url: token_info.icon_url,
            balance: balance.to_string(),
        })
        .collect();

    ok(CursorPaginatedResponse::without_total(
        tokens,
        limit as i64,
        next_cursor,
    ))
}

async fn get_address_stats_history(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
) -> ApiResult<crate::routes::statistics::StackedAreaChartResponse> {
    use crate::routes::statistics::{
        StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries,
    };

    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    // Date range: today - 365 days to today
    let now = chrono::Utc::now();
    let today = now.format("%Y%m%d").to_string().parse::<u32>().unwrap_or(0);
    let one_year_ago = (now - chrono::Duration::days(365))
        .format("%Y%m%d")
        .to_string()
        .parse::<u32>()
        .unwrap_or(0);

    let daily_stats = state
        .store
        .list_addr_daily_stats(&lock_hash, one_year_ago, today)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Get current live_cells_count to compute baseline
    let addr_balance = state
        .store
        .get_addr_balance(&lock_hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let current_live_cells = addr_balance
        .map(|ab| ab.live_cells_count as i64)
        .unwrap_or(0);

    // Sum all cells_delta in range to find baseline
    let total_delta: i64 = daily_stats.iter().map(|(_, s)| s.cells_delta as i64).sum();
    let baseline_live_cells = current_live_cells - total_delta;

    // Build cumulative series
    let mut cum_activities: i64 = 0;
    let mut cum_txs: i64 = 0;
    let mut live_cells = baseline_live_cells;

    let data: Vec<StackedAreaDataPoint> = daily_stats
        .into_iter()
        .map(|(date, stats)| {
            cum_activities += stats.activities as i64;
            cum_txs += stats.txs as i64;
            live_cells += stats.cells_delta as i64;

            let date_str = format!("{}-{}-{}", date / 10000, (date / 100) % 100, date % 100);

            let mut values = std::collections::HashMap::new();
            values.insert(
                "cumulativeActivities".to_string(),
                cum_activities.to_string(),
            );
            values.insert("liveCells".to_string(), live_cells.to_string());
            values.insert("cumulativeTransactions".to_string(), cum_txs.to_string());

            StackedAreaDataPoint {
                date: date_str,
                values,
            }
        })
        .collect();

    let series = vec![
        StackedAreaSeries {
            key: "cumulativeActivities".to_string(),
            label: "Cumulative Activities".to_string(),
            color: "#22c55e".to_string(),
        },
        StackedAreaSeries {
            key: "liveCells".to_string(),
            label: "Live Cells".to_string(),
            color: "#f59e0b".to_string(),
        },
        StackedAreaSeries {
            key: "cumulativeTransactions".to_string(),
            label: "Cumulative Transactions".to_string(),
            color: "#8b5cf6".to_string(),
        },
    ];

    ok(StackedAreaChartResponse {
        data,
        series,
        title: "Address Stats History".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dep_group_valid() {
        // 2 outpoints: count(4) + 2 * 36 = 76 bytes
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_le_bytes());
        // OutPoint 1: 32 bytes tx_hash + 4 bytes index
        data.extend_from_slice(&[1u8; 32]);
        data.extend_from_slice(&0u32.to_le_bytes());
        // OutPoint 2
        data.extend_from_slice(&[2u8; 32]);
        data.extend_from_slice(&1u32.to_le_bytes());

        let result = parse_dep_group(&data, 76);
        assert!(result.is_dep_group);
        assert!(result.items.is_some());
        let items = result.items.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].output_index, 0);
        assert_eq!(items[1].output_index, 1);
    }

    #[test]
    fn test_parse_dep_group_invalid_size() {
        let data = vec![0u8; 10];
        let result = parse_dep_group(&data, 10);
        assert!(!result.is_dep_group);
    }

    #[test]
    fn test_parse_dep_group_zero_count() {
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes());
        let result = parse_dep_group(&data, 4);
        assert!(!result.is_dep_group);
    }

    #[test]
    fn test_parse_hash_type() {
        assert_eq!(parse_hash_type("data"), Some(0));
        assert_eq!(parse_hash_type("type"), Some(1));
        assert_eq!(parse_hash_type("data1"), Some(2));
        assert_eq!(parse_hash_type("data2"), Some(4));
        assert_eq!(parse_hash_type("invalid"), None);
    }

    #[test]
    fn test_cell_info_to_response_normal() {
        let info = ckbadger_store::LiveCellInfo {
            capacity: 10000000000,
            created_at_block: 100,
            lock_script_hash: vec![0u8; 32],
            lock_code_hash: vec![1u8; 32],
            lock_hash_type: 1,
            lock_args: vec![2u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 0,
        };
        let tx_hash = vec![3u8; 32];
        let resp = cell_info_to_response(&tx_hash, 0, &info);
        assert_eq!(resp.output_index, 0);
        assert_eq!(resp.capacity, "10000000000");
        assert!(resp.cell_type.is_none());
        assert!(resp.virtual_occupied_capacity.is_none());
    }

    #[test]
    fn test_estimated_occupied_capacity_breakdown_without_type_script() {
        let info = ckbadger_store::LiveCellInfo {
            capacity: 10000000000,
            created_at_block: 100,
            lock_script_hash: vec![0u8; 32],
            lock_code_hash: vec![1u8; 32],
            lock_hash_type: 1,
            lock_args: vec![2u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 16,
            occupied_capacity: 0,
        };

        let breakdown = estimated_occupied_capacity_breakdown(&info);
        assert_eq!(breakdown.capacity_field_bytes, 8);
        assert_eq!(breakdown.lock_script_bytes, 53);
        assert_eq!(breakdown.type_script_bytes, 0);
        assert_eq!(breakdown.data_bytes, 16);
        assert_eq!(breakdown.total_bytes, 77);
    }

    #[test]
    fn test_estimated_occupied_capacity_breakdown_with_type_script() {
        let info = ckbadger_store::LiveCellInfo {
            capacity: 10000000000,
            created_at_block: 100,
            lock_script_hash: vec![0u8; 32],
            lock_code_hash: vec![1u8; 32],
            lock_hash_type: 1,
            lock_args: vec![2u8; 20],
            type_script_hash: Some(vec![3u8; 32]),
            type_code_hash: Some(vec![4u8; 32]),
            type_args: Some(vec![5u8; 24]),
            data_size: 16,
            occupied_capacity: 0,
        };

        let breakdown = estimated_occupied_capacity_breakdown(&info);
        assert_eq!(breakdown.capacity_field_bytes, 8);
        assert_eq!(breakdown.lock_script_bytes, 53);
        assert_eq!(breakdown.type_script_bytes, 57);
        assert_eq!(breakdown.data_bytes, 16);
        assert_eq!(breakdown.total_bytes, 134);
    }
}
