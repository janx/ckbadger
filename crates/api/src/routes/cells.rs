#![allow(clippy::type_complexity)]
#![allow(clippy::manual_is_multiple_of)]
#![allow(dead_code)]

use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use ckbadger_common::dao::{
    is_genesis_special_burn_cell, GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED,
};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::clickhouse::{hex_hash, unhex_hash};
use crate::response::{
    decode_cursor, encode_cursor, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::utils::{address_to_lock_script_hash, is_ckb_address, script_to_address};
use crate::AppState;

#[derive(Row, Deserialize)]
struct CountRow {
    count: u64,
}

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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockScriptInfo {
    pub code_hash: String,
    pub name: String,
    pub script_kind: Option<String>,
    pub deprecated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressResponse {
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub balance: String,
    pub live_cells_count: i64,
    pub transactions_count: i64,
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
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddressTokensParams {
    #[serde(default = "default_limit")]
    limit: i64,
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

async fn list_live_cells(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListCellsParams>,
) -> ApiResult<CursorPaginatedResponse<CellResponse>> {
    let limit = params.limit.clamp(1, 100);
    let cursor = params.cursor.as_ref().and_then(|c| decode_cursor(c));
    let (cursor_block, cursor_idx) = cursor.unwrap_or((i64::MAX, i32::MAX));

    // Parse filter parameters
    let lock_hash_hex = if let Some(ref lock_hash) = params.lock_script_hash {
        Some(if is_ckb_address(lock_hash) {
            let bytes = address_to_lock_script_hash(lock_hash)
                .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?;
            hex::encode(bytes)
        } else {
            lock_hash
                .strip_prefix("0x")
                .unwrap_or(lock_hash)
                .to_string()
        })
    } else {
        None
    };

    let type_hash_hex = params
        .type_script_hash
        .as_ref()
        .map(|h| h.strip_prefix("0x").unwrap_or(h).to_string());

    let type_code_hash_hex = params
        .type_code_hash
        .as_ref()
        .map(|h| h.strip_prefix("0x").unwrap_or(h).to_string());

    // Build WHERE clause for filters
    let mut where_clauses = vec![format!(
        "(c.created_at_block, c.output_index) < ({}, {})",
        cursor_block, cursor_idx
    )];

    if let Some(ref lock_hex) = lock_hash_hex {
        where_clauses.push(format!("c.lock_script_hash = unhex('{}')", lock_hex));
    }

    if let Some(ref type_hex) = type_hash_hex {
        where_clauses.push(format!("c.type_script_hash = unhex('{}')", type_hex));
    }

    if let Some(ref code_hex) = type_code_hash_hex {
        where_clauses.push(format!("c.type_code_hash = unhex('{}')", code_hex));
    }

    let where_clause = where_clauses.join(" AND ");

    // Get total count from PostgreSQL (for now - could optimize later)
    let total: i64 = if let Some(ref lock_hex) = lock_hash_hex {
        let lock_bytes =
            hex::decode(lock_hex).map_err(|_| ApiError::bad_request("Invalid lock script hash"))?;

        if let Some(ref code_hex) = type_code_hash_hex {
            let code_bytes = hex::decode(code_hex)
                .map_err(|_| ApiError::bad_request("Invalid type code hash"))?;
            let count_query = format!(
                "SELECT COUNT(*) FROM cells WHERE lock_script_hash = unhex('{}') AND type_code_hash = unhex('{}')",
                hex::encode(&lock_bytes),
                hex::encode(&code_bytes)
            );
            let result = state
                .clickhouse
                .client()
                .query(&count_query)
                .fetch_one::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            result.count as i64
        } else if let Some(ref type_hex) = type_hash_hex {
            let type_bytes = hex::decode(type_hex)
                .map_err(|_| ApiError::bad_request("Invalid type script hash"))?;
            let count_query = format!(
                "SELECT COUNT(*) FROM cells WHERE lock_script_hash = unhex('{}') AND type_script_hash = unhex('{}')",
                hex::encode(&lock_bytes),
                hex::encode(&type_bytes)
            );
            let result = state
                .clickhouse
                .client()
                .query(&count_query)
                .fetch_one::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            result.count as i64
        } else {
            let count_query = format!(
                "SELECT COUNT(*) FROM cells WHERE lock_script_hash = unhex('{}')",
                hex::encode(&lock_bytes)
            );
            let result = state
                .clickhouse
                .client()
                .query(&count_query)
                .fetch_one::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            result.count as i64
        }
    } else if let Some(ref type_hex) = type_hash_hex {
        let type_bytes =
            hex::decode(type_hex).map_err(|_| ApiError::bad_request("Invalid type script hash"))?;
        let count_query = format!(
            "SELECT COUNT(*) FROM cells WHERE type_script_hash = unhex('{}')",
            hex::encode(&type_bytes)
        );
        let result = state
            .clickhouse
            .client()
            .query(&count_query)
            .fetch_one::<CountRow>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        result.count as i64
    } else {
        let count_query = "SELECT COUNT(*) FROM cells";
        let result = state
            .clickhouse
            .client()
            .query(count_query)
            .fetch_one::<CountRow>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        result.count as i64
    };

    // Query ClickHouse for live cells using LEFT ANTI JOIN
    let query = format!(
        "SELECT 
            {} as tx_hash,
            c.output_index,
            c.capacity,
            {} as lock_script_hash,
            {} as type_script_hash,
            {} as type_code_hash,
            c.data_size,
            c.created_at_block,
            {} as lock_args
        FROM cells c
        LEFT ANTI JOIN cell_consumptions cc 
            ON c.tx_hash = cc.tx_hash AND c.output_index = cc.output_index
        WHERE {}
        ORDER BY c.created_at_block DESC, c.output_index DESC
        LIMIT {}",
        hex_hash("c.tx_hash"),
        hex_hash("c.lock_script_hash"),
        hex_hash("c.type_script_hash"),
        hex_hash("c.type_code_hash"),
        hex_hash("c.lock_args"),
        where_clause,
        limit + 1
    );

    #[derive(Row, Deserialize)]
    struct LiveCellRowClickHouse {
        tx_hash: String,
        output_index: u16,
        capacity: u64,
        lock_script_hash: String,
        type_script_hash: Option<String>,
        type_code_hash: Option<String>,
        data_size: u32,
        created_at_block: u64,
        lock_args: String,
    }

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<LiveCellRowClickHouse>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|r| encode_cursor(r.created_at_block as i64, r.output_index as i32))
    } else {
        None
    };

    let cells: Vec<CellResponse> = rows
        .into_iter()
        .map(|r| {
            let lock_args_bytes = hex::decode(&r.lock_args).unwrap_or_default();
            let is_special_burn =
                is_genesis_special_burn_cell(&lock_args_bytes, r.created_at_block as i64);

            CellResponse {
                tx_hash: format!("0x{}", r.tx_hash),
                output_index: r.output_index as i32,
                capacity: r.capacity.to_string(),
                lock_script_hash: format!("0x{}", r.lock_script_hash),
                type_script_hash: r.type_script_hash.map(|h| format!("0x{}", h)),
                type_code_hash: r.type_code_hash.map(|h| format!("0x{}", h)),
                data_size: r.data_size as i32,
                created_at_block: r.created_at_block as i64,
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
                udt_amount: None, // UDT amounts not in ClickHouse yet
            }
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        cells,
        total,
        limit,
        next_cursor,
    ))
}

fn parse_hash_type(hash_type: &str) -> Option<i16> {
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
    let limit = params.limit.clamp(1, 100);
    let cursor = params.cursor.as_ref().and_then(|c| decode_cursor(c));
    let (cursor_block, cursor_idx) = cursor.unwrap_or((i64::MAX, i32::MAX));

    let code_hash_hex = params
        .code_hash
        .strip_prefix("0x")
        .unwrap_or(&params.code_hash)
        .to_string();

    let hash_type_num = parse_hash_type(&params.hash_type).ok_or_else(|| {
        ApiError::bad_request("Invalid hash_type. Must be one of: data, type, data1, data2")
    })?;

    let script_kind = params.script_kind.as_str();

    let _code_hash_bytes =
        hex::decode(&code_hash_hex).map_err(|_| ApiError::bad_request("Invalid code_hash hex"))?;

    let total: i64 = match script_kind {
        "lock" | "type" => {
            let count_query = format!(
                "SELECT live_cells_count FROM script_usage_stats WHERE code_hash = unhex('{}') AND script_kind = '{}'",
                code_hash_hex, script_kind
            );
            let row: Option<CountRow> = state
                .clickhouse
                .client()
                .query(&count_query)
                .fetch_optional::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            row.map(|r| r.count as i64).unwrap_or(0)
        }
        _ => {
            let count_query = format!(
                "SELECT COALESCE(SUM(live_cells_count), 0) FROM script_usage_stats WHERE code_hash = unhex('{}')",
                code_hash_hex
            );
            let result = state
                .clickhouse
                .client()
                .query(&count_query)
                .fetch_one::<CountRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            result.count as i64
        }
    };

    let query = match script_kind {
        "lock" => format!(
            "SELECT 
                {} as tx_hash,
                c.output_index,
                c.capacity,
                {} as lock_script_hash,
                {} as type_script_hash,
                {} as type_code_hash,
                c.data_size,
                c.created_at_block,
                {} as lock_args
            FROM cells c
            LEFT ANTI JOIN cell_consumptions cc 
                ON c.tx_hash = cc.tx_hash AND c.output_index = cc.output_index
            WHERE c.lock_code_hash = unhex('{}') 
                AND c.lock_hash_type = {}
                AND (c.created_at_block, c.output_index) < ({}, {})
            ORDER BY c.created_at_block DESC, c.output_index DESC
            LIMIT {}",
            hex_hash("c.tx_hash"),
            hex_hash("c.lock_script_hash"),
            hex_hash("c.type_script_hash"),
            hex_hash("c.type_code_hash"),
            hex_hash("c.lock_args"),
            code_hash_hex,
            hash_type_num,
            cursor_block,
            cursor_idx,
            limit + 1
        ),
        "type" => format!(
            "SELECT 
                {} as tx_hash,
                c.output_index,
                c.capacity,
                {} as lock_script_hash,
                {} as type_script_hash,
                {} as type_code_hash,
                c.data_size,
                c.created_at_block,
                {} as lock_args
            FROM cells c
            LEFT ANTI JOIN cell_consumptions cc 
                ON c.tx_hash = cc.tx_hash AND c.output_index = cc.output_index
            WHERE c.type_code_hash = unhex('{}') 
                AND c.type_hash_type = {}
                AND (c.created_at_block, c.output_index) < ({}, {})
            ORDER BY c.created_at_block DESC, c.output_index DESC
            LIMIT {}",
            hex_hash("c.tx_hash"),
            hex_hash("c.lock_script_hash"),
            hex_hash("c.type_script_hash"),
            hex_hash("c.type_code_hash"),
            hex_hash("c.lock_args"),
            code_hash_hex,
            hash_type_num,
            cursor_block,
            cursor_idx,
            limit + 1
        ),
        _ => format!(
            "SELECT 
                {} as tx_hash,
                c.output_index,
                c.capacity,
                {} as lock_script_hash,
                {} as type_script_hash,
                {} as type_code_hash,
                c.data_size,
                c.created_at_block,
                {} as lock_args
            FROM cells c
            LEFT ANTI JOIN cell_consumptions cc 
                ON c.tx_hash = cc.tx_hash AND c.output_index = cc.output_index
            WHERE ((c.lock_code_hash = unhex('{}') AND c.lock_hash_type = {}) 
                   OR (c.type_code_hash = unhex('{}') AND c.type_hash_type = {}))
                AND (c.created_at_block, c.output_index) < ({}, {})
            ORDER BY c.created_at_block DESC, c.output_index DESC
            LIMIT {}",
            hex_hash("c.tx_hash"),
            hex_hash("c.lock_script_hash"),
            hex_hash("c.type_script_hash"),
            hex_hash("c.type_code_hash"),
            hex_hash("c.lock_args"),
            code_hash_hex,
            hash_type_num,
            code_hash_hex,
            hash_type_num,
            cursor_block,
            cursor_idx,
            limit + 1
        ),
    };

    #[derive(Row, Deserialize)]
    struct CellByScriptRowClickHouse {
        tx_hash: String,
        output_index: u16,
        capacity: u64,
        lock_script_hash: String,
        type_script_hash: Option<String>,
        type_code_hash: Option<String>,
        data_size: u32,
        created_at_block: u64,
        lock_args: String,
    }

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<CellByScriptRowClickHouse>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|r| encode_cursor(r.created_at_block as i64, r.output_index as i32))
    } else {
        None
    };

    let cells: Vec<CellResponse> = rows
        .into_iter()
        .map(|r| {
            let lock_args_bytes = hex::decode(&r.lock_args).unwrap_or_default();
            let is_special_burn =
                is_genesis_special_burn_cell(&lock_args_bytes, r.created_at_block as i64);

            CellResponse {
                tx_hash: format!("0x{}", r.tx_hash),
                output_index: r.output_index as i32,
                capacity: r.capacity.to_string(),
                lock_script_hash: format!("0x{}", r.lock_script_hash),
                type_script_hash: r.type_script_hash.map(|h| format!("0x{}", h)),
                type_code_hash: r.type_code_hash.map(|h| format!("0x{}", h)),
                data_size: r.data_size as i32,
                created_at_block: r.created_at_block as i64,
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
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        cells,
        total,
        limit,
        next_cursor,
    ))
}

async fn get_cell(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((tx_hash, output_index)): axum::extract::Path<(String, i32)>,
) -> ApiResult<CellDetailResponse> {
    let hash_hex = tx_hash.strip_prefix("0x").unwrap_or(&tx_hash);
    let _hash_bytes = unhex_hash(&tx_hash)?;

    let query = format!(
        "SELECT 
            {} as tx_hash,
            c.output_index,
            c.capacity,
            {} as lock_code_hash,
            c.lock_hash_type,
            {} as lock_args,
            {} as lock_script_hash,
            {} as type_code_hash,
            c.type_hash_type,
            {} as type_args,
            {} as type_script_hash,
            {} as data_hash,
            c.data_size,
            c.created_at_block,
            {} as data
        FROM cells c
        WHERE c.tx_hash = unhex('{}') AND c.output_index = {}
        LIMIT 1",
        hex_hash("c.tx_hash"),
        hex_hash("c.lock_code_hash"),
        hex_hash("c.lock_args"),
        hex_hash("c.lock_script_hash"),
        hex_hash("c.type_code_hash"),
        hex_hash("c.type_args"),
        hex_hash("c.type_script_hash"),
        hex_hash("c.data_hash"),
        hex_hash("c.data"),
        hash_hex,
        output_index
    );

    #[derive(Row, Deserialize)]
    struct CellDetailRowClickHouse {
        tx_hash: String,
        output_index: u16,
        capacity: u64,
        lock_code_hash: String,
        lock_hash_type: u8,
        lock_args: String,
        lock_script_hash: String,
        type_code_hash: Option<String>,
        type_hash_type: Option<u8>,
        type_args: Option<String>,
        type_script_hash: Option<String>,
        data_hash: String,
        data_size: u32,
        created_at_block: u64,
        data: Option<String>,
    }

    let row = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_optional::<CellDetailRowClickHouse>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let r = row.ok_or_else(|| ApiError::not_found("Cell not found"))?;

    let consumption_query = format!(
        "SELECT 
            cc.consumed_at_block,
            {} as consumed_by_tx
        FROM cell_consumptions cc
        WHERE cc.tx_hash = unhex('{}') AND cc.output_index = {}
        LIMIT 1",
        hex_hash("cc.consumed_by_tx"),
        hash_hex,
        output_index
    );

    #[derive(Row, Deserialize)]
    struct ConsumptionRowClickHouse {
        consumed_at_block: u64,
        consumed_by_tx: String,
    }

    let consumption = state
        .clickhouse
        .client()
        .query(&consumption_query)
        .fetch_optional::<ConsumptionRowClickHouse>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (status, consumed_at_block, consumed_by_tx) = if let Some(c) = consumption {
        (
            "dead".to_string(),
            Some(c.consumed_at_block as i64),
            Some(format!("0x{}", c.consumed_by_tx)),
        )
    } else {
        ("live".to_string(), None, None)
    };

    let hash_type_str = |ht: u8| match ht {
        0 => "data",
        1 => "type",
        2 => "data1",
        4 => "data2",
        _ => "data",
    };

    let type_script = r.type_code_hash.as_ref().map(|code_hash| ScriptResponse {
        code_hash: format!("0x{}", code_hash),
        hash_type: hash_type_str(r.type_hash_type.unwrap_or(0)).to_string(),
        args: format!("0x{}", r.type_args.as_ref().unwrap_or(&String::new())),
    });

    let lock_code_hash_bytes = hex::decode(&r.lock_code_hash).unwrap_or_default();
    let lock_args_bytes = hex::decode(&r.lock_args).unwrap_or_default();

    let address = script_to_address(
        &lock_code_hash_bytes,
        r.lock_hash_type as i16,
        &lock_args_bytes,
        &state.ckb_network,
    )
    .ok();

    let cell_data = r.data.as_ref().map(|d| hex::decode(d).unwrap_or_default());
    let dep_group_result = cell_data
        .as_ref()
        .map(|d| parse_dep_group(d, r.data_size as i32))
        .unwrap_or(DepGroupParseResult {
            is_dep_group: false,
            items: None,
        });

    let data_hash_bytes = hex::decode(&r.data_hash).unwrap_or_default();
    let type_script_hash_bytes = r
        .type_script_hash
        .as_ref()
        .map(|h| hex::decode(h).unwrap_or_default());

    let code_cell_of = lookup_code_cell_scripts(
        &state.ckb_network,
        &data_hash_bytes,
        type_script_hash_bytes.as_ref(),
    )
    .await;

    let type_script_size: i64 = if r.type_code_hash.is_some() {
        32 + 1 + r.type_args.as_ref().map(|a| a.len() / 2).unwrap_or(0) as i64
    } else {
        0
    };
    let occupied_capacity =
        8 + 32 + 1 + (r.lock_args.len() / 2) as i64 + type_script_size + r.data_size as i64;

    let is_satoshi = is_genesis_special_burn_cell(&lock_args_bytes, r.created_at_block as i64);
    let (cell_type, virtual_occupied_capacity) = if is_satoshi {
        (
            Some("genesis_special_burn".to_string()),
            Some(GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED.to_string()),
        )
    } else {
        (None, None)
    };

    let hash_bytes =
        hex::decode(hash_hex).map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;
    let dao_info = lookup_dao_info(&hash_bytes, output_index as i16).await;

    ok(CellDetailResponse {
        tx_hash: format!("0x{}", r.tx_hash),
        output_index: r.output_index as i32,
        capacity: r.capacity.to_string(),
        occupied_capacity,
        virtual_occupied_capacity,
        cell_type,
        lock_script_hash: format!("0x{}", r.lock_script_hash),
        address,
        type_script_hash: r.type_script_hash.map(|h| format!("0x{}", h)),
        data_size: r.data_size as i32,
        created_at_block: r.created_at_block as i64,
        status,
        consumed_at_block,
        consumed_by_tx,
        lock: ScriptResponse {
            code_hash: format!("0x{}", r.lock_code_hash),
            hash_type: hash_type_str(r.lock_hash_type).to_string(),
            args: format!("0x{}", r.lock_args),
        },
        type_script,
        data: r.data.map(|d| format!("0x{}", d)),
        is_dep_group: dep_group_result.is_dep_group,
        dep_group_items: dep_group_result.items,
        code_cell_of,
        dao_info,
    })
}

async fn lookup_code_cell_scripts(
    _network: &str,
    _data_hash: &[u8],
    _type_script_hash: Option<&Vec<u8>>,
) -> Option<Vec<CodeCellScript>> {
    // Known scripts data not yet available in ClickHouse backend
    // TODO: Implement when known_scripts table is added to ClickHouse schema
    None
}

fn shannon_to_ckb(shannon: &str) -> String {
    let num: u128 = shannon.parse().unwrap_or(0);
    let ckb = num / 100_000_000;
    let remainder = num % 100_000_000;
    if remainder == 0 {
        format!("{}", ckb)
    } else {
        format!("{}.{:08}", ckb, remainder)
            .trim_end_matches('0')
            .to_string()
    }
}

async fn lookup_dao_info(_tx_hash: &[u8], _output_index: i16) -> Option<DaoInfo> {
    // DAO data not yet available in ClickHouse backend
    // TODO: Implement when DAO tables are added to ClickHouse schema
    None
}
