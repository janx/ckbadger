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
            "/addresses/{addr}/asset-transfers",
            get(get_address_asset_transfers),
        )
}

#[derive(Debug, Deserialize)]
pub struct ListCellsParams {
    #[serde(default = "default_limit")]
    limit: i64,
    lock_script_hash: Option<String>,
    type_script_hash: Option<String>,
    type_code_hash: Option<String>,
    #[allow(dead_code)]
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
}

#[derive(Debug, Deserialize)]
pub struct AddressTxParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[allow(dead_code)]
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

async fn list_live_cells(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListCellsParams>,
) -> ApiResult<CursorPaginatedResponse<CellResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

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

    // Fetch cells from the store based on available filters.
    // The store supports listing by lock hash or type hash via prefix scans.
    let raw_cells: Vec<(Vec<u8>, i16, ckbadger_store::LiveCellInfo)> =
        match (&lock_hash_bytes, &type_hash_bytes) {
            (Some(lock_bytes), Some(type_bytes)) => {
                // Filter by lock first (usually more selective), then post-filter by type
                let all = state
                    .store
                    .list_cells_by_lock(lock_bytes, limit * 10 + 1)
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
                        .list_cells_by_lock(lock_bytes, limit * 10 + 1)
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
                        .list_cells_by_lock(lock_bytes, limit + 1)
                        .map_err(|e| ApiError::internal(e.to_string()))?
                }
            }
            (None, Some(type_bytes)) => state
                .store
                .list_cells_by_type(type_bytes, limit + 1)
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

    let next_cursor = if has_more {
        raw_cells.last().map(|(_, output_index, info)| {
            encode_cursor(info.created_at_block, *output_index as i32)
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
    let (cursor_block, cursor_idx) = params
        .cursor
        .as_deref()
        .and_then(decode_cursor)
        .map(|(b, i)| (Some(b), Some(i as i16)))
        .unwrap_or((None, None));

    // Fetch limit+1 to detect has_more
    let fetch_limit = limit + 1;

    // Use code_hash indexes for efficient prefix scans
    let results: Vec<(Vec<u8>, i16, ckbadger_store::LiveCellInfo)> = match script_kind {
        "lock" => state
            .store
            .list_cells_by_lock_code_hash(&code_hash_bytes, fetch_limit, cursor_block, cursor_idx)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        "type" => state
            .store
            .list_cells_by_type_code_hash(&code_hash_bytes, fetch_limit, cursor_block, cursor_idx)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        _ => {
            // "both": merge results from lock and type indexes
            let mut merged = state
                .store
                .list_cells_by_lock_code_hash(
                    &code_hash_bytes,
                    fetch_limit,
                    cursor_block,
                    cursor_idx,
                )
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let type_results = state
                .store
                .list_cells_by_type_code_hash(
                    &code_hash_bytes,
                    fetch_limit,
                    cursor_block,
                    cursor_idx,
                )
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
        results.last().map(|(_, output_index, info)| {
            encode_cursor(info.created_at_block, *output_index as i32)
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

    let (balance, live_cells_count, transactions_count) = match &addr_balance {
        Some(ab) => (
            ab.balance.to_string(),
            ab.live_cells_count as i64,
            ab.txs_count,
        ),
        None => ("0".to_string(), 0, 0),
    };

    // Try to find a cell for this lock hash to get the lock script details
    let cells_for_script = state
        .store
        .list_cells_by_lock(&lock_hash, 1)
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
            .get_consumed_cell(&hash_bytes, output_idx)
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        None
    };

    let (info, status_str, is_consumed) = match (live_cell, consumed_cell) {
        (Some(cell), _) => (cell, "live", false),
        (None, Some(cell)) => (cell, "dead", true),
        (None, None) => return Err(ApiError::not_found("Cell not found")),
    };

    // Look up script hash_type from script_info
    let lock_hash_type_num: i16 = state
        .store
        .get_script_info(&info.lock_code_hash)
        .ok()
        .flatten()
        .map(|si| si.hash_type as i16)
        .unwrap_or(1);

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
            args: String::from("0x"), // type_args not stored in LiveCellInfo
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

    // Calculate occupied capacity: 8 (capacity) + 32 (code_hash) + 1 (hash_type) + lock_args + type_script + data
    let type_script_size: i64 = if info.type_code_hash.is_some() {
        32 + 1 // code_hash + hash_type (no args available from LiveCellInfo)
    } else {
        0
    };
    let occupied_capacity =
        8 + 32 + 1 + info.lock_args.len() as i64 + type_script_size + info.data_size as i64;

    let is_satoshi = is_genesis_special_burn_cell(&info.lock_args, info.created_at_block);
    let (cell_type, virtual_occupied_capacity) = if is_satoshi {
        (
            Some("genesis_special_burn".to_string()),
            Some(GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED.to_string()),
        )
    } else {
        (None, None)
    };

    // Find consumed_by info if cell is dead
    let (consumed_at_block, consumed_by_tx) = if is_consumed {
        // We don't have a direct consumed_by lookup in the store.
        // The consumed_cells CF stores the cell info but not who consumed it.
        (None, None)
    } else {
        (None, None)
    };

    let dao_info = lookup_dao_info(&state.store, &hash_bytes, output_idx);

    ok(CellDetailResponse {
        tx_hash: format!("0x{}", hex::encode(&hash_bytes)),
        output_index: output_idx as i32,
        capacity: info.capacity.to_string(),
        occupied_capacity,
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

    // Fetch recent transactions for this address (newest first)
    let addr_txs = state
        .store
        .list_addr_txs_recent(&lock_hash, limit + 1)
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
        .map(|(block_number, _tx_idx, tx_hash)| {
            let timestamp = state
                .store
                .get_block_header(block_number)
                .ok()
                .flatten()
                .map(|h| {
                    chrono::DateTime::from_timestamp(h.timestamp, 0)
                        .unwrap_or_default()
                        .to_rfc3339()
                })
                .unwrap_or_default();

            AddressTransactionResponse {
                tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                block_number,
                tx_type: "transfer".to_string(),
                capacity_change: "0".to_string(),
                timestamp,
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

#[derive(Debug, Deserialize)]
pub struct AssetTransfersParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[allow(dead_code)]
    cursor: Option<String>,
    category: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetTransferResponse {
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

fn encode_timeline_cursor(block_number: i64, tx_index: i32, event_index: i16) -> String {
    format!("{}:{}:{}", block_number, tx_index, event_index)
}

async fn get_address_asset_transfers(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
    Query(params): Query<AssetTransfersParams>,
) -> ApiResult<CursorPaginatedResponse<AssetTransferResponse>> {
    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    let valid_categories = ["token", "dob", "nft", "dao"];
    if let Some(ref cat) = params.category {
        if !valid_categories.contains(&cat.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Invalid category '{}'. Must be one of: token, dob, nft, dao",
                cat
            )));
        }
    }

    // Fetch activities for this address, filter to asset categories
    let activities = state
        .store
        .list_activities_by_addr(&lock_hash, limit * 10 + 1)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let filtered: Vec<_> = activities
        .into_iter()
        .filter(|a| {
            let cat = a.category.as_str();
            let is_asset = cat == "token" || cat == "dob" || cat == "nft" || cat == "dao";
            if let Some(ref filter_cat) = params.category {
                is_asset && cat == filter_cat.as_str()
            } else {
                is_asset
            }
        })
        .take(limit + 1)
        .collect();

    let has_more = filtered.len() > limit;
    let filtered: Vec<_> = filtered.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        filtered.last().map(|a| {
            let block_number = state
                .store
                .get_tx_location(&a.tx_hash)
                .ok()
                .flatten()
                .map(|(bn, _)| bn)
                .unwrap_or(0);
            encode_timeline_cursor(block_number, a.tx_idx, 0)
        })
    } else {
        None
    };

    // Collect token type hashes for metadata lookup
    let token_ids: Vec<Vec<u8>> = filtered
        .iter()
        .filter(|a| a.category == "token" && a.asset_id.is_some())
        .filter_map(|a| a.asset_id.clone())
        .collect();

    let token_metadata: std::collections::HashMap<Vec<u8>, (Option<String>, Option<String>, i16)> =
        if !token_ids.is_empty() {
            let mut map = std::collections::HashMap::new();
            for type_hash in &token_ids {
                if let Ok(Some(info)) = state.store.get_token(type_hash) {
                    map.insert(
                        type_hash.clone(),
                        (info.name, info.symbol, info.decimals.unwrap_or(0) as i16),
                    );
                }
            }
            map
        } else {
            std::collections::HashMap::new()
        };

    let transfers: Vec<AssetTransferResponse> = filtered
        .into_iter()
        .map(|activity| {
            let is_sender = activity
                .from_lock
                .as_ref()
                .map(|h| h == &lock_hash)
                .unwrap_or(false);
            let is_receiver = activity
                .to_lock
                .as_ref()
                .map(|h| h == &lock_hash)
                .unwrap_or(false);

            let direction_str = match (is_sender, is_receiver) {
                (true, false) => "out",
                (false, true) => "in",
                _ => "unknown",
            };

            let peer_lock_hash = if is_sender {
                activity.to_lock.as_ref()
            } else {
                activity.from_lock.as_ref()
            };

            let event_type = activity_type_to_event_type(&activity.activity_type);

            let (token_name, token_symbol, token_decimals) =
                if activity.category == "token" && activity.asset_id.is_some() {
                    activity
                        .asset_id
                        .as_ref()
                        .and_then(|id| token_metadata.get(id))
                        .map(|(n, s, d)| (n.clone(), s.clone(), Some(*d)))
                        .unwrap_or((None, None, None))
                } else {
                    extract_token_meta_from_metadata(
                        activity
                            .metadata
                            .as_ref()
                            .unwrap_or(&serde_json::Value::Null),
                    )
                };

            let block_number = state
                .store
                .get_tx_location(&activity.tx_hash)
                .ok()
                .flatten()
                .map(|(bn, _)| bn)
                .unwrap_or(0);

            let timestamp = chrono::DateTime::from_timestamp(activity.timestamp, 0)
                .unwrap_or_default()
                .to_rfc3339();

            AssetTransferResponse {
                tx_hash: format!("0x{}", hex::encode(&activity.tx_hash)),
                block_number,
                tx_index: activity.tx_idx,
                event_index: 0,
                asset_category: activity.category,
                asset_type: activity.activity_type,
                asset_id: activity
                    .asset_id
                    .as_ref()
                    .map(|id| format!("0x{}", hex::encode(id))),
                direction: direction_str.to_string(),
                peer_address: peer_lock_hash.map(|h| format!("0x{}", hex::encode(h))),
                amount: activity.amount.map(|a| a.to_string()),
                event_type: Some(event_type),
                timestamp,
                token_name,
                token_symbol,
                token_decimals,
            }
        })
        .collect();

    ok(CursorPaginatedResponse::without_total(
        transfers,
        limit as i64,
        next_cursor,
    ))
}

fn activity_type_to_event_type(activity_type: &str) -> String {
    match activity_type {
        "TOKEN_MINT" | "DOB_MINT" | "NFT_MINT" => "mint".to_string(),
        "TOKEN_BURN" | "DOB_BURN" => "burn".to_string(),
        "TOKEN_TRANSFER" | "DOB_TRANSFER" | "NFT_TRANSFER" => "transfer".to_string(),
        "DAO_DEPOSIT" => "deposit".to_string(),
        "DAO_WITHDRAW_REQUEST" => "withdraw_request".to_string(),
        "DAO_WITHDRAW_COMPLETE" => "withdraw_complete".to_string(),
        _ => activity_type.to_lowercase(),
    }
}

fn extract_token_meta_from_metadata(
    metadata: &serde_json::Value,
) -> (Option<String>, Option<String>, Option<i16>) {
    let name = metadata
        .get("token_name")
        .or_else(|| metadata.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let symbol = metadata
        .get("token_symbol")
        .or_else(|| metadata.get("symbol"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let decimals = metadata
        .get("decimals")
        .and_then(|v| v.as_i64())
        .map(|d| d as i16);
    (name, symbol, decimals)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_timeline_cursor(cursor: &str) -> Option<(i64, i32, i16)> {
        let parts: Vec<&str> = cursor.split(':').collect();
        if parts.len() == 3 {
            Some((
                parts[0].parse().ok()?,
                parts[1].parse().ok()?,
                parts[2].parse().ok()?,
            ))
        } else {
            None
        }
    }

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
    fn test_activity_type_to_event_type() {
        assert_eq!(activity_type_to_event_type("TOKEN_MINT"), "mint");
        assert_eq!(activity_type_to_event_type("TOKEN_BURN"), "burn");
        assert_eq!(activity_type_to_event_type("TOKEN_TRANSFER"), "transfer");
        assert_eq!(activity_type_to_event_type("DAO_DEPOSIT"), "deposit");
        assert_eq!(
            activity_type_to_event_type("DAO_WITHDRAW_REQUEST"),
            "withdraw_request"
        );
        assert_eq!(
            activity_type_to_event_type("DAO_WITHDRAW_COMPLETE"),
            "withdraw_complete"
        );
        assert_eq!(activity_type_to_event_type("UNKNOWN_TYPE"), "unknown_type");
    }

    #[test]
    fn test_extract_token_meta_from_metadata() {
        let metadata = serde_json::json!({
            "token_name": "TestToken",
            "token_symbol": "TT",
            "decimals": 8
        });
        let (name, symbol, decimals) = extract_token_meta_from_metadata(&metadata);
        assert_eq!(name, Some("TestToken".to_string()));
        assert_eq!(symbol, Some("TT".to_string()));
        assert_eq!(decimals, Some(8));
    }

    #[test]
    fn test_extract_token_meta_from_metadata_empty() {
        let metadata = serde_json::json!({});
        let (name, symbol, decimals) = extract_token_meta_from_metadata(&metadata);
        assert!(name.is_none());
        assert!(symbol.is_none());
        assert!(decimals.is_none());
    }

    #[test]
    fn test_decode_timeline_cursor() {
        let result = decode_timeline_cursor("100:5:3");
        assert_eq!(result, Some((100, 5, 3)));

        let result = decode_timeline_cursor("invalid");
        assert!(result.is_none());
    }

    #[test]
    fn test_encode_timeline_cursor() {
        let cursor = encode_timeline_cursor(100, 5, 3);
        assert_eq!(cursor, "100:5:3");
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
            data_size: 0,
        };
        let tx_hash = vec![3u8; 32];
        let resp = cell_info_to_response(&tx_hash, 0, &info);
        assert_eq!(resp.output_index, 0);
        assert_eq!(resp.capacity, "10000000000");
        assert!(resp.cell_type.is_none());
        assert!(resp.virtual_occupied_capacity.is_none());
    }
}
