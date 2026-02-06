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
use ckbadger_common::sync::{SyncStatusData, SYNC_STATUS_REDIS_KEY};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;

use crate::response::{
    decode_cursor, encode_cursor, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::tx_block_map::get_block_number_for_tx;
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

    type LiveCellRow = (
        Vec<u8>,         // tx_hash
        i16,             // output_index
        String,          // capacity
        Vec<u8>,         // lock_script_hash
        Option<Vec<u8>>, // type_script_hash
        Option<Vec<u8>>, // type_code_hash
        i32,             // data_size
        i64,             // created_at_block
        Vec<u8>,         // lock_args
        Option<String>,  // udt_amount (from udt_cells.amount)
    );

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

    let type_code_hash_bytes = if let Some(ref code_hash) = params.type_code_hash {
        Some(
            hex::decode(code_hash.strip_prefix("0x").unwrap_or(code_hash))
                .map_err(|_| ApiError::bad_request("Invalid type code hash"))?,
        )
    } else {
        None
    };

    if let Some(type_code_bytes) = &type_code_hash_bytes {
        if let Some(lock_bytes) = &lock_hash_bytes {
            let total: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM live_cells WHERE lock_script_hash = $1 AND type_code_hash = $2",
            )
            .bind(lock_bytes)
            .bind(type_code_bytes)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            let rows = sqlx::query_as::<_, LiveCellRow>(
                r#"
                SELECT lc.tx_hash, lc.output_index, lc.capacity::TEXT, lc.lock_script_hash, 
                       lc.type_script_hash, lc.type_code_hash, lc.data_size, lc.created_at_block, lc.lock_args,
                       uc.amount::TEXT
                FROM live_cells lc
                LEFT JOIN udt_cells uc ON lc.tx_hash = uc.tx_hash AND lc.output_index = uc.output_index
                WHERE lc.lock_script_hash = $1 AND lc.type_code_hash = $2
                  AND (lc.created_at_block, lc.output_index) < ($3, $4)
                ORDER BY lc.created_at_block DESC, lc.output_index DESC
                LIMIT $5
                "#,
            )
            .bind(lock_bytes)
            .bind(type_code_bytes)
            .bind(cursor_block)
            .bind(cursor_idx)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            let has_more = rows.len() as i64 > limit;
            let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

            let next_cursor = if has_more {
                rows.last()
                    .map(|(_, output_index, _, _, _, _, _, created_at_block, _, _)| {
                        encode_cursor(*created_at_block, *output_index as i32)
                    })
            } else {
                None
            };

            let cells: Vec<CellResponse> = rows
                .into_iter()
                .map(
                    |(
                        tx_hash,
                        output_index,
                        capacity,
                        lock_script_hash,
                        type_script_hash,
                        type_code_hash,
                        data_size,
                        created_at_block,
                        lock_args,
                        udt_amount,
                    )| {
                        let is_special_burn =
                            is_genesis_special_burn_cell(&lock_args, created_at_block);
                        CellResponse {
                            tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                            output_index: output_index as i32,
                            capacity,
                            lock_script_hash: format!("0x{}", hex::encode(&lock_script_hash)),
                            type_script_hash: type_script_hash
                                .map(|h| format!("0x{}", hex::encode(&h))),
                            type_code_hash: type_code_hash
                                .map(|h| format!("0x{}", hex::encode(&h))),
                            data_size,
                            created_at_block,
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
                            udt_amount,
                        }
                    },
                )
                .collect();

            return ok(CursorPaginatedResponse::new(
                cells,
                total.0,
                limit,
                next_cursor,
            ));
        }
    }

    let (total, rows): (i64, Vec<LiveCellRow>) = match (&lock_hash_bytes, &type_hash_bytes) {
        (Some(lock_bytes), Some(type_bytes)) => {
            let total: (i64,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM live_cells WHERE lock_script_hash = $1 AND type_script_hash = $2",
                )
                .bind(lock_bytes)
                .bind(type_bytes)
                .fetch_one(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            let rows = sqlx::query_as::<_, LiveCellRow>(
                    r#"
                    SELECT lc.tx_hash, lc.output_index, lc.capacity::TEXT, lc.lock_script_hash, 
                           lc.type_script_hash, lc.type_code_hash, lc.data_size, lc.created_at_block, lc.lock_args,
                           uc.amount::TEXT
                    FROM live_cells lc
                    LEFT JOIN udt_cells uc ON lc.tx_hash = uc.tx_hash AND lc.output_index = uc.output_index
                    WHERE lc.lock_script_hash = $1 AND lc.type_script_hash = $2
                      AND (lc.created_at_block, lc.output_index) < ($3, $4)
                    ORDER BY lc.created_at_block DESC, lc.output_index DESC
                    LIMIT $5
                    "#,
                )
                .bind(lock_bytes)
                .bind(type_bytes)
                .bind(cursor_block)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (Some(lock_bytes), None) => {
            let total: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM live_cells WHERE lock_script_hash = $1")
                    .bind(lock_bytes)
                    .fetch_one(&state.pool)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;

            let rows = sqlx::query_as::<_, LiveCellRow>(
                    r#"
                    SELECT lc.tx_hash, lc.output_index, lc.capacity::TEXT, lc.lock_script_hash, 
                           lc.type_script_hash, lc.type_code_hash, lc.data_size, lc.created_at_block, lc.lock_args,
                           uc.amount::TEXT
                    FROM live_cells lc
                    LEFT JOIN udt_cells uc ON lc.tx_hash = uc.tx_hash AND lc.output_index = uc.output_index
                    WHERE lc.lock_script_hash = $1
                      AND (lc.created_at_block, lc.output_index) < ($2, $3)
                    ORDER BY lc.created_at_block DESC, lc.output_index DESC
                    LIMIT $4
                    "#,
                )
                .bind(lock_bytes)
                .bind(cursor_block)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (None, Some(type_bytes)) => {
            let total: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM live_cells WHERE type_script_hash = $1")
                    .bind(type_bytes)
                    .fetch_one(&state.pool)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;

            let rows = sqlx::query_as::<_, LiveCellRow>(
                    r#"
                    SELECT lc.tx_hash, lc.output_index, lc.capacity::TEXT, lc.lock_script_hash, 
                           lc.type_script_hash, lc.type_code_hash, lc.data_size, lc.created_at_block, lc.lock_args,
                           uc.amount::TEXT
                    FROM live_cells lc
                    LEFT JOIN udt_cells uc ON lc.tx_hash = uc.tx_hash AND lc.output_index = uc.output_index
                    WHERE lc.type_script_hash = $1
                      AND (lc.created_at_block, lc.output_index) < ($2, $3)
                    ORDER BY lc.created_at_block DESC, lc.output_index DESC
                    LIMIT $4
                    "#,
                )
                .bind(type_bytes)
                .bind(cursor_block)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (None, None) => {
            let total = match state
                .cache
                .get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY)
                .await
            {
                Some(status) => status.total_live_cells,
                None => sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM live_cells")
                    .fetch_one(&state.pool)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?,
            };

            let rows = sqlx::query_as::<_, LiveCellRow>(
                    r#"
                    SELECT lc.tx_hash, lc.output_index, lc.capacity::TEXT, lc.lock_script_hash, 
                           lc.type_script_hash, lc.type_code_hash, lc.data_size, lc.created_at_block, lc.lock_args,
                           uc.amount::TEXT
                    FROM live_cells lc
                    LEFT JOIN udt_cells uc ON lc.tx_hash = uc.tx_hash AND lc.output_index = uc.output_index
                    WHERE (lc.created_at_block, lc.output_index) < ($1, $2)
                    ORDER BY lc.created_at_block DESC, lc.output_index DESC
                    LIMIT $3
                    "#,
                )
                .bind(cursor_block)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            (total, rows)
        }
    };

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(_, output_index, _, _, _, _, _, created_at_block, _, _)| {
                encode_cursor(*created_at_block, *output_index as i32)
            })
    } else {
        None
    };

    let cells: Vec<CellResponse> = rows
        .into_iter()
        .map(
            |(
                tx_hash,
                output_index,
                capacity,
                lock_script_hash,
                type_script_hash,
                type_code_hash,
                data_size,
                created_at_block,
                lock_args,
                udt_amount,
            )| {
                let is_special_burn = is_genesis_special_burn_cell(&lock_args, created_at_block);
                CellResponse {
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    output_index: output_index as i32,
                    capacity,
                    lock_script_hash: format!("0x{}", hex::encode(&lock_script_hash)),
                    type_script_hash: type_script_hash.map(|h| format!("0x{}", hex::encode(&h))),
                    type_code_hash: type_code_hash.map(|h| format!("0x{}", hex::encode(&h))),
                    data_size,
                    created_at_block,
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
                    udt_amount,
                }
            },
        )
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

    let code_hash_bytes = hex::decode(
        params
            .code_hash
            .strip_prefix("0x")
            .unwrap_or(&params.code_hash),
    )
    .map_err(|_| ApiError::bad_request("Invalid code_hash hex"))?;

    let hash_type_num = parse_hash_type(&params.hash_type).ok_or_else(|| {
        ApiError::bad_request("Invalid hash_type. Must be one of: data, type, data1, data2")
    })?;

    let script_kind = params.script_kind.as_str();

    let total: i64 = match script_kind {
        "lock" | "type" => {
            let row: Option<(i64,)> = sqlx::query_as(
                "SELECT live_cells_count FROM script_usage_stats WHERE code_hash = $1 AND script_kind = $2",
            )
            .bind(&code_hash_bytes)
            .bind(script_kind)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
            row.map(|(c,)| c).unwrap_or(0)
        }
        _ => {
            let row: (i64,) = sqlx::query_as(
                "SELECT COALESCE(SUM(live_cells_count), 0) FROM script_usage_stats WHERE code_hash = $1",
            )
            .bind(&code_hash_bytes)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
            row.0
        }
    };

    let rows: Vec<(Vec<u8>, i16, String, Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>, i32, i64, Vec<u8>)> =
        match script_kind {
            "lock" => {
                sqlx::query_as(
                    r#"
                SELECT tx_hash, output_index, capacity::TEXT, lock_script_hash, type_script_hash, type_code_hash, data_size, created_at_block, lock_args
                FROM cells
                WHERE lock_code_hash = $1 AND lock_hash_type = $2 AND status = 0
                  AND (created_at_block, output_index) < ($3, $4)
                ORDER BY created_at_block DESC, output_index DESC
                LIMIT $5
                "#,
                )
                .bind(&code_hash_bytes)
                .bind(hash_type_num)
                .bind(cursor_block)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
            }
            "type" => {
                sqlx::query_as(
                    r#"
                SELECT tx_hash, output_index, capacity::TEXT, lock_script_hash, type_script_hash, type_code_hash, data_size, created_at_block, lock_args
                FROM cells
                WHERE type_code_hash = $1 AND type_hash_type = $2 AND status = 0
                  AND (created_at_block, output_index) < ($3, $4)
                ORDER BY created_at_block DESC, output_index DESC
                LIMIT $5
                "#,
                )
                .bind(&code_hash_bytes)
                .bind(hash_type_num)
                .bind(cursor_block)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
            }
            _ => {
                sqlx::query_as(
                    r#"
                SELECT tx_hash, output_index, capacity::TEXT, lock_script_hash, type_script_hash, type_code_hash, data_size, created_at_block, lock_args
                FROM cells
                WHERE ((lock_code_hash = $1 AND lock_hash_type = $2) OR (type_code_hash = $1 AND type_hash_type = $2))
                  AND status = 0
                  AND (created_at_block, output_index) < ($3, $4)
                ORDER BY created_at_block DESC, output_index DESC
                LIMIT $5
                "#,
                )
                .bind(&code_hash_bytes)
                .bind(hash_type_num)
                .bind(cursor_block)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
            }
        };

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(_, output_index, _, _, _, _, _, created_at_block, _)| {
                encode_cursor(*created_at_block, *output_index as i32)
            })
    } else {
        None
    };

    let cells: Vec<CellResponse> = rows
        .into_iter()
        .map(
            |(
                tx_hash,
                output_index,
                capacity,
                lock_script_hash,
                type_script_hash,
                type_code_hash,
                data_size,
                created_at_block,
                lock_args,
            )| {
                let is_special_burn = is_genesis_special_burn_cell(&lock_args, created_at_block);
                CellResponse {
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    output_index: output_index as i32,
                    capacity,
                    lock_script_hash: format!("0x{}", hex::encode(&lock_script_hash)),
                    type_script_hash: type_script_hash.map(|h| format!("0x{}", hex::encode(&h))),
                    type_code_hash: type_code_hash.map(|h| format!("0x{}", hex::encode(&h))),
                    data_size,
                    created_at_block,
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
            },
        )
        .collect();

    ok(CursorPaginatedResponse::new(
        cells,
        total,
        limit,
        next_cursor,
    ))
}

async fn get_address(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
) -> ApiResult<AddressResponse> {
    let (lock_hash, input_address) = if is_ckb_address(&addr) {
        let hash = address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?;
        (hash, Some(addr.clone()))
    } else {
        let hash = hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?;
        (hash, None)
    };

    // Check if address_balances is deferred
    let sync_status = state.cache.get_sync_status(&state.pool).await;

    let row = if sync_status.address_balances_deferred {
        // Fallback: query cells table directly
        sqlx::query_as::<_, (String, i32, i64)>(
            r#"
            SELECT 
                COALESCE(SUM(capacity)::TEXT, '0'),
                COALESCE(COUNT(*)::INT, 0),
                0
            FROM cells
            WHERE lock_script_hash = $1 AND status = 0
            "#,
        )
        .bind(&lock_hash)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        // Normal: query address_balances table
        sqlx::query_as::<_, (String, i32, i64)>(
            r#"
            SELECT 
                COALESCE(balance::TEXT, '0'),
                COALESCE(live_cells_count, 0),
                COALESCE(transactions_count, 0)
            FROM address_balances
            WHERE lock_script_hash = $1
            "#,
        )
        .bind(&lock_hash)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let script_row = sqlx::query_as::<_, (Vec<u8>, i16, Vec<u8>)>(
        r#"
        SELECT lock_code_hash, lock_hash_type, lock_args
        FROM cells
        WHERE lock_script_hash = $1
        LIMIT 1
        "#,
    )
    .bind(&lock_hash)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (lock_script, address) = match &script_row {
        Some((code_hash, hash_type, args)) => {
            let hash_type_str = match hash_type {
                0 => "data",
                1 => "type",
                2 => "data1",
                4 => "data2",
                _ => "data",
            };
            let script = ScriptResponse {
                code_hash: format!("0x{}", hex::encode(code_hash)),
                hash_type: hash_type_str.to_string(),
                args: format!("0x{}", hex::encode(args)),
            };
            let addr = input_address.or_else(|| {
                script_to_address(code_hash, *hash_type, args, &state.ckb_network).ok()
            });
            (Some(script), addr)
        }
        None => (None, input_address),
    };

    let lock_script_info = if let Some((code_hash, _, _)) = &script_row {
        sqlx::query_as::<_, (Vec<u8>, String, Option<String>, bool)>(
            r#"
            SELECT DISTINCT ON (ks.code_hash)
                ks.code_hash,
                ks.name,
                sus.script_kind,
                ks.deprecated
            FROM known_scripts ks
            LEFT JOIN script_usage_stats sus ON sus.code_hash = ks.code_hash AND sus.script_kind = 'lock'
            WHERE ks.code_hash = $1 AND ks.network = $2
            ORDER BY ks.code_hash, ks.deprecated ASC, ks.is_system DESC
            "#,
        )
        .bind(code_hash)
        .bind(&state.ckb_network)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
        .map(|(ch, name, script_kind, deprecated)| LockScriptInfo {
            code_hash: format!("0x{}", hex::encode(&ch)),
            name,
            script_kind,
            deprecated,
        })
    } else {
        None
    };

    let (balance, live_cells_count, transactions_count) = row
        .map(|(b, l, t)| (b, l as i64, t))
        .unwrap_or(("0".to_string(), 0, 0));

    let recent_activities_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM activities WHERE from_lock_hash = $1 OR to_lock_hash = $1",
    )
    .bind(&lock_hash)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    ok(AddressResponse {
        lock_script_hash: format!("0x{}", hex::encode(&lock_hash)),
        address,
        balance,
        live_cells_count,
        transactions_count,
        recent_activities_count,
        lock_script,
        lock_script_info,
    })
}

async fn lookup_code_cell_scripts(
    pool: &sqlx::PgPool,
    network: &str,
    data_hash: &[u8],
    type_script_hash: Option<&Vec<u8>>,
) -> Option<Vec<CodeCellScript>> {
    let mut scripts = Vec::new();

    let data_scripts: Vec<(String, Vec<u8>, String)> = sqlx::query_as(
        r#"
        SELECT DISTINCT name, code_hash, hash_type
        FROM known_scripts
        WHERE code_hash = $1 AND network = $2 AND hash_type IN ('data', 'data1', 'data2')
        ORDER BY name
        "#,
    )
    .bind(data_hash)
    .bind(network)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for (name, code_hash, hash_type) in data_scripts {
        scripts.push(CodeCellScript {
            name,
            code_hash: format!("0x{}", hex::encode(&code_hash)),
            hash_type,
        });
    }

    if let Some(type_hash) = type_script_hash {
        let type_scripts: Vec<(String, Vec<u8>, String)> = sqlx::query_as(
            r#"
            SELECT DISTINCT name, code_hash, hash_type
            FROM known_scripts
            WHERE code_hash = $1 AND network = $2 AND hash_type = 'type'
            ORDER BY name
            "#,
        )
        .bind(type_hash)
        .bind(network)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for (name, code_hash, hash_type) in type_scripts {
            scripts.push(CodeCellScript {
                name,
                code_hash: format!("0x{}", hex::encode(&code_hash)),
                hash_type,
            });
        }
    }

    if scripts.is_empty() {
        None
    } else {
        Some(scripts)
    }
}

async fn lookup_dao_info(
    pool: &sqlx::PgPool,
    tx_hash: &[u8],
    output_index: i16,
) -> Option<DaoInfo> {
    type DaoRow = (
        i64,
        chrono::DateTime<chrono::Utc>,
        i16,
        Option<i64>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<i64>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
    );

    let row = sqlx::query_as::<_, DaoRow>(
        r#"
        SELECT 
            deposit_block_number,
            deposit_timestamp,
            status,
            withdraw_request_block,
            withdraw_request_timestamp,
            withdraw_block,
            withdraw_timestamp,
            CAST(compensation AS TEXT)
        FROM dao_deposits
        WHERE tx_hash = $1 AND output_index = $2
        "#,
    )
    .bind(tx_hash)
    .bind(output_index)
    .fetch_optional(pool)
    .await
    .ok()?;

    let row = if row.is_none() {
        sqlx::query_as::<_, DaoRow>(
            r#"
            SELECT 
                deposit_block_number,
                deposit_timestamp,
                status,
                withdraw_request_block,
                withdraw_request_timestamp,
                withdraw_block,
                withdraw_timestamp,
                CAST(compensation AS TEXT)
            FROM dao_deposits
            WHERE withdraw_request_tx = $1
            "#,
        )
        .bind(tx_hash)
        .fetch_optional(pool)
        .await
        .ok()?
    } else {
        row
    }?;

    let (
        deposit_block_number,
        deposit_timestamp,
        status,
        withdraw_request_block,
        withdraw_request_timestamp,
        withdraw_block,
        withdraw_timestamp,
        compensation,
    ) = row;

    let dao_status = match status {
        0 => "deposited",
        1 => "withdrawing",
        2 => "withdrawn",
        _ => "unknown",
    }
    .to_string();

    let compensation_ckb = compensation.as_ref().map(|c| shannon_to_ckb(c));

    Some(DaoInfo {
        is_dao_cell: true,
        dao_status,
        deposit_block_number,
        deposit_timestamp: deposit_timestamp.to_rfc3339(),
        withdraw_request_block,
        withdraw_request_timestamp: withdraw_request_timestamp.map(|t| t.to_rfc3339()),
        withdraw_block,
        withdraw_timestamp: withdraw_timestamp.map(|t| t.to_rfc3339()),
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

    #[derive(FromRow)]
    struct CellRow {
        tx_hash: Vec<u8>,
        output_index: i16,
        capacity: String,
        lock_code_hash: Vec<u8>,
        lock_hash_type: i16,
        lock_args: Vec<u8>,
        lock_script_hash: Vec<u8>,
        type_code_hash: Option<Vec<u8>>,
        type_hash_type: Option<i16>,
        type_args: Option<Vec<u8>>,
        type_script_hash: Option<Vec<u8>>,
        data_hash: Vec<u8>,
        data_size: i32,
        status: i16,
        created_at_block: i64,
        consumed_at_block: Option<i64>,
        consumed_by_tx: Option<Vec<u8>>,
        data: Option<Vec<u8>>,
        full_data: Option<Vec<u8>>,
    }

    let block_number = get_block_number_for_tx(&state.pool, &hash_bytes)
        .await
        .ok()
        .flatten();

    let row = if let Some(bn) = block_number {
        sqlx::query_as::<_, CellRow>(
            r#"
            SELECT 
                c.tx_hash, c.output_index, c.capacity::TEXT,
                c.lock_code_hash, c.lock_hash_type, c.lock_args, c.lock_script_hash,
                c.type_code_hash, c.type_hash_type, c.type_args, c.type_script_hash,
                c.data_hash, c.data_size, c.status, c.created_at_block, c.consumed_at_block, c.consumed_by_tx, 
                c.data,
                cd.data AS full_data
            FROM cells c
            LEFT JOIN cell_data cd ON cd.tx_hash = c.tx_hash AND cd.output_index = c.output_index
            WHERE c.tx_hash = $1 AND c.output_index = $2 AND c.created_at_block = $3
            "#,
        )
        .bind(&hash_bytes)
        .bind(output_index)
        .bind(bn)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        sqlx::query_as::<_, CellRow>(
            r#"
            SELECT 
                c.tx_hash, c.output_index, c.capacity::TEXT,
                c.lock_code_hash, c.lock_hash_type, c.lock_args, c.lock_script_hash,
                c.type_code_hash, c.type_hash_type, c.type_args, c.type_script_hash,
                c.data_hash, c.data_size, c.status, c.created_at_block, c.consumed_at_block, c.consumed_by_tx, 
                c.data,
                cd.data AS full_data
            FROM cells c
            LEFT JOIN cell_data cd ON cd.tx_hash = c.tx_hash AND cd.output_index = c.output_index
            WHERE c.tx_hash = $1 AND c.output_index = $2
            "#,
        )
        .bind(&hash_bytes)
        .bind(output_index)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    match row {
        Some(r) => {
            let hash_type_str = |ht: i16| match ht {
                0 => "data",
                1 => "type",
                2 => "data1",
                4 => "data2",
                _ => "data",
            };

            let type_script = r.type_code_hash.as_ref().map(|code_hash| ScriptResponse {
                code_hash: format!("0x{}", hex::encode(code_hash)),
                hash_type: hash_type_str(r.type_hash_type.unwrap_or(0)).to_string(),
                args: format!("0x{}", hex::encode(r.type_args.as_ref().unwrap_or(&vec![]))),
            });

            let address = script_to_address(
                &r.lock_code_hash,
                r.lock_hash_type,
                &r.lock_args,
                &state.ckb_network,
            )
            .ok();

            let cell_data = r.full_data.as_ref().or(r.data.as_ref());
            let dep_group_result = cell_data
                .map(|d| parse_dep_group(d, r.data_size))
                .unwrap_or(DepGroupParseResult {
                    is_dep_group: false,
                    items: None,
                });

            let code_cell_of = lookup_code_cell_scripts(
                &state.pool,
                &state.ckb_network,
                &r.data_hash,
                r.type_script_hash.as_ref(),
            )
            .await;

            // Calculate occupied capacity: 8 (capacity) + 32 (code_hash) + 1 (hash_type) + lock_args + type_script + data
            let type_script_size: i64 = if r.type_code_hash.is_some() {
                32 + 1 + r.type_args.as_ref().map(|a| a.len()).unwrap_or(0) as i64
            } else {
                0
            };
            let occupied_capacity =
                8 + 32 + 1 + r.lock_args.len() as i64 + type_script_size + r.data_size as i64;

            let is_satoshi = is_genesis_special_burn_cell(&r.lock_args, r.created_at_block);
            let (cell_type, virtual_occupied_capacity) = if is_satoshi {
                (
                    Some("genesis_special_burn".to_string()),
                    Some(GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED.to_string()),
                )
            } else {
                (None, None)
            };

            let dao_info = lookup_dao_info(&state.pool, &hash_bytes, output_index as i16).await;

            ok(CellDetailResponse {
                tx_hash: format!("0x{}", hex::encode(&r.tx_hash)),
                output_index: r.output_index as i32,
                capacity: r.capacity,
                occupied_capacity,
                virtual_occupied_capacity,
                cell_type,
                lock_script_hash: format!("0x{}", hex::encode(&r.lock_script_hash)),
                address,
                type_script_hash: r
                    .type_script_hash
                    .as_ref()
                    .map(|h| format!("0x{}", hex::encode(h))),
                data_size: r.data_size,
                created_at_block: r.created_at_block,
                status: if r.status == 0 {
                    "live".to_string()
                } else {
                    "dead".to_string()
                },
                consumed_at_block: r.consumed_at_block,
                consumed_by_tx: r
                    .consumed_by_tx
                    .as_ref()
                    .map(|h| format!("0x{}", hex::encode(h))),
                lock: ScriptResponse {
                    code_hash: format!("0x{}", hex::encode(&r.lock_code_hash)),
                    hash_type: hash_type_str(r.lock_hash_type).to_string(),
                    args: format!("0x{}", hex::encode(&r.lock_args)),
                },
                type_script,
                data: cell_data.map(|d| format!("0x{}", hex::encode(d))),
                is_dep_group: dep_group_result.is_dep_group,
                dep_group_items: dep_group_result.items,
                code_cell_of,
                dao_info,
            })
        }
        None => Err(ApiError::not_found("Cell not found")),
    }
}

async fn get_top_addresses(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TopAddressesParams>,
) -> ApiResult<Vec<TopAddressResponse>> {
    let sync_status = state.cache.get_sync_status(&state.pool).await;

    if sync_status.address_balances_deferred {
        return ok(Vec::new());
    }

    let limit = params.limit.clamp(1, 500);

    let rows = sqlx::query_as::<_, (Vec<u8>, String, i32, i64)>(
        r#"
        SELECT lock_script_hash, balance::TEXT, live_cells_count, transactions_count
        FROM address_balances
        WHERE balance > 0
        ORDER BY balance DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let addresses: Vec<TopAddressResponse> = rows
        .into_iter()
        .map(
            |(lock_script_hash, balance, live_cells_count, transactions_count)| {
                TopAddressResponse {
                    lock_script_hash: format!("0x{}", hex::encode(&lock_script_hash)),
                    balance,
                    live_cells_count,
                    transactions_count,
                }
            },
        )
        .collect();

    ok(addresses)
}

async fn get_active_addresses(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActiveAddressesParams>,
) -> ApiResult<Vec<ActiveAddressResponse>> {
    let sync_status = state.cache.get_sync_status(&state.pool).await;

    if sync_status.address_balances_deferred {
        return ok(Vec::new());
    }

    let limit = params.limit.clamp(1, 500);
    let days = params.days.clamp(1, 365);

    let tip_block = state.cache.get_sync_tip(&state.pool).await;

    let blocks_per_day: i64 = 8640;
    let min_block = tip_block.saturating_sub(days * blocks_per_day);

    let rows = sqlx::query_as::<_, (Vec<u8>, String, i32, i64, i64)>(
        r#"
        SELECT lock_script_hash, balance::TEXT, live_cells_count, transactions_count, last_activity_block
        FROM address_balances
        WHERE last_activity_block >= $1
        ORDER BY last_activity_block DESC
        LIMIT $2
        "#,
    )
    .bind(min_block)
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let addresses: Vec<ActiveAddressResponse> = rows
        .into_iter()
        .map(
            |(
                lock_script_hash,
                balance,
                live_cells_count,
                transactions_count,
                last_activity_block,
            )| {
                ActiveAddressResponse {
                    lock_script_hash: format!("0x{}", hex::encode(&lock_script_hash)),
                    balance,
                    live_cells_count,
                    transactions_count,
                    last_activity_block,
                }
            },
        )
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

    let limit = params.limit.clamp(1, 100);
    let cursor = params.cursor.as_ref().and_then(|c| decode_cursor(c));
    let (cursor_block, cursor_idx) = cursor.unwrap_or((i64::MAX, i32::MAX));

    let sync_status = state.cache.get_sync_status(&state.pool).await;

    let total: (i64,) = if sync_status.address_balances_deferred {
        sqlx::query_as(
            "SELECT COUNT(*) FROM activities WHERE activity_category = 'ckb' AND (from_lock_hash = $1 OR to_lock_hash = $1)",
        )
        .bind(&lock_hash)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        sqlx::query_as(
            "SELECT COALESCE(transactions_count, 0) FROM address_balances WHERE lock_script_hash = $1",
        )
        .bind(&lock_hash)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .unwrap_or((0,))
    };

    // Query activities table for CKB transfers involving this address
    // Determine direction based on from/to lock hash
    type ActivityRow = (
        Vec<u8>,
        i64,
        i32,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        String,
        chrono::DateTime<chrono::Utc>,
    );
    let rows = sqlx::query_as::<_, ActivityRow>(
        r#"
        SELECT tx_hash, block_number, tx_index, from_lock_hash, to_lock_hash, amount::TEXT, timestamp
        FROM activities
        WHERE activity_category = 'ckb' 
          AND (from_lock_hash = $1 OR to_lock_hash = $1)
          AND (block_number, tx_index) < ($2, $3)
        ORDER BY block_number DESC, tx_index DESC
        LIMIT $4
        "#,
    )
    .bind(&lock_hash)
    .bind(cursor_block)
    .bind(cursor_idx)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(_, block_number, tx_index, _, _, _, _)| encode_cursor(*block_number, *tx_index))
    } else {
        None
    };

    let txs: Vec<AddressTransactionResponse> = rows
        .into_iter()
        .map(
            |(
                tx_hash,
                block_number,
                _tx_index,
                from_lock_hash,
                to_lock_hash,
                amount,
                timestamp,
            )| {
                // Determine tx_type based on from/to relationship with lock_hash
                let is_sender = from_lock_hash
                    .as_ref()
                    .map(|h| h == &lock_hash)
                    .unwrap_or(false);
                let is_receiver = to_lock_hash
                    .as_ref()
                    .map(|h| h == &lock_hash)
                    .unwrap_or(false);
                let tx_type_str = match (is_sender, is_receiver) {
                    (true, true) => "internal", // Self-transfer
                    (true, false) => "sent",
                    (false, true) => "received",
                    (false, false) => "unknown",
                };
                // Calculate capacity_change: positive for received, negative for sent
                let capacity_change = if is_sender && !is_receiver {
                    format!("-{}", amount)
                } else {
                    amount
                };
                AddressTransactionResponse {
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    block_number,
                    tx_type: tx_type_str.to_string(),
                    capacity_change,
                    timestamp: timestamp.to_rfc3339(),
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::new(
        txs,
        total.0,
        limit,
        next_cursor,
    ))
}

async fn get_address_tokens(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(addr): axum::extract::Path<String>,
    Query(params): Query<AddressTokensParams>,
) -> ApiResult<CursorPaginatedResponse<AddressTokenResponse>> {
    let sync_status = state.cache.get_sync_status(&state.pool).await;

    if sync_status.token_deferred {
        return ok(CursorPaginatedResponse::new(
            Vec::new(),
            0,
            params.limit.clamp(1, 100),
            None,
        ));
    }

    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100);
    let (cursor_balance, cursor_token_id) = params
        .cursor
        .as_ref()
        .and_then(|c| {
            let parts: Vec<&str> = c.split(':').collect();
            if parts.len() == 2 {
                Some((parts[0].to_string(), parts[1].parse::<i64>().ok()?))
            } else {
                None
            }
        })
        .unwrap_or_else(|| (String::new(), i64::MAX));

    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM token_balances WHERE lock_script_hash = $1 AND balance > 0",
    )
    .bind(&lock_hash)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    type TokenBalanceRow = (
        i64,
        String,
        Vec<u8>,
        String,
        Option<String>,
        Option<String>,
        i16,
        Option<String>,
    );

    let rows = if cursor_balance.is_empty() {
        sqlx::query_as::<_, TokenBalanceRow>(
            r#"
            SELECT tb.token_id, tb.balance::text, t.type_script_hash, t.standard, 
                   t.name, t.symbol, t.decimals, t.icon_url
            FROM token_balances tb
            JOIN tokens t ON tb.token_id = t.id
            WHERE tb.lock_script_hash = $1 AND tb.balance > 0
            ORDER BY tb.balance DESC, tb.token_id DESC
            LIMIT $2
            "#,
        )
        .bind(&lock_hash)
        .bind(limit + 1)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        sqlx::query_as::<_, TokenBalanceRow>(
            r#"
            SELECT tb.token_id, tb.balance::text, t.type_script_hash, t.standard,
                   t.name, t.symbol, t.decimals, t.icon_url
            FROM token_balances tb
            JOIN tokens t ON tb.token_id = t.id
            WHERE tb.lock_script_hash = $1 AND tb.balance > 0
              AND (tb.balance, tb.token_id) < ($2::numeric, $3)
            ORDER BY tb.balance DESC, tb.token_id DESC
            LIMIT $4
            "#,
        )
        .bind(&lock_hash)
        .bind(&cursor_balance)
        .bind(cursor_token_id)
        .bind(limit + 1)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(token_id, balance, _, _, _, _, _, _)| format!("{}:{}", balance, token_id))
    } else {
        None
    };

    let tokens: Vec<AddressTokenResponse> = rows
        .into_iter()
        .map(
            |(_, balance, type_script_hash, standard, name, symbol, decimals, icon_url)| {
                AddressTokenResponse {
                    type_script_hash: format!("0x{}", hex::encode(&type_script_hash)),
                    standard,
                    name,
                    symbol,
                    decimals,
                    icon_url,
                    balance,
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::new(
        tokens,
        total.0,
        limit,
        next_cursor,
    ))
}

#[derive(Debug, Deserialize)]
pub struct AssetTransfersParams {
    #[serde(default = "default_limit")]
    limit: i64,
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

    let limit = params.limit.clamp(1, 100);
    let cursor = params
        .cursor
        .as_ref()
        .and_then(|c| decode_timeline_cursor(c));
    let (cursor_block, cursor_tx_idx, cursor_evt_idx) =
        cursor.unwrap_or((i64::MAX, i32::MAX, i16::MAX));

    let valid_categories = ["token", "dob", "nft", "dao"];
    if let Some(ref cat) = params.category {
        if !valid_categories.contains(&cat.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Invalid category '{}'. Must be one of: token, dob, nft, dao",
                cat
            )));
        }
    }

    #[rustfmt::skip]
    type ActivityRow = (
        Vec<u8>,                       // tx_hash
        i64,                           // block_number
        i32,                           // tx_index
        i16,                           // activity_index
        String,                        // activity_category
        String,                        // activity_type
        Option<Vec<u8>>,               // asset_id
        Option<Vec<u8>>,               // from_lock_hash
        Option<Vec<u8>>,               // to_lock_hash
        String,                        // amount
        serde_json::Value,             // metadata
        chrono::DateTime<chrono::Utc>, // timestamp
    );

    let base_condition = "(from_lock_hash = $1 OR to_lock_hash = $1) AND activity_category IN ('token', 'dob', 'nft', 'dao')";

    let total: i64 = if let Some(ref cat) = params.category {
        let query = format!(
            "SELECT COUNT(*) FROM activities WHERE {} AND activity_category = $2",
            base_condition
        );
        sqlx::query_scalar(&query)
            .bind(&lock_hash)
            .bind(cat)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let query = format!("SELECT COUNT(*) FROM activities WHERE {}", base_condition);
        sqlx::query_scalar(&query)
            .bind(&lock_hash)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let rows: Vec<ActivityRow> = if let Some(ref cat) = params.category {
        let query = format!(
            r#"
            SELECT tx_hash, block_number, tx_index, activity_index,
                   activity_category, activity_type, asset_id,
                   from_lock_hash, to_lock_hash, amount::TEXT, metadata, timestamp
            FROM activities
            WHERE {} AND activity_category = $2
              AND (block_number, tx_index, activity_index) < ($3, $4, $5)
            ORDER BY block_number DESC, tx_index DESC, activity_index DESC
            LIMIT $6
            "#,
            base_condition
        );
        sqlx::query_as(&query)
            .bind(&lock_hash)
            .bind(cat)
            .bind(cursor_block)
            .bind(cursor_tx_idx)
            .bind(cursor_evt_idx)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let query = format!(
            r#"
            SELECT tx_hash, block_number, tx_index, activity_index,
                   activity_category, activity_type, asset_id,
                   from_lock_hash, to_lock_hash, amount::TEXT, metadata, timestamp
            FROM activities
            WHERE {}
              AND (block_number, tx_index, activity_index) < ($2, $3, $4)
            ORDER BY block_number DESC, tx_index DESC, activity_index DESC
            LIMIT $5
            "#,
            base_condition
        );
        sqlx::query_as(&query)
            .bind(&lock_hash)
            .bind(cursor_block)
            .bind(cursor_tx_idx)
            .bind(cursor_evt_idx)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(
            |(_, block_number, tx_index, activity_index, _, _, _, _, _, _, _, _)| {
                encode_timeline_cursor(*block_number, *tx_index, *activity_index)
            },
        )
    } else {
        None
    };

    let token_ids: Vec<Vec<u8>> = rows
        .iter()
        .filter(|(_, _, _, _, cat, _, asset_id, _, _, _, _, _)| {
            cat == "token" && asset_id.is_some()
        })
        .filter_map(|(_, _, _, _, _, _, asset_id, _, _, _, _, _)| asset_id.clone())
        .collect();

    let token_metadata: std::collections::HashMap<Vec<u8>, (Option<String>, Option<String>, i16)> =
        if !token_ids.is_empty() {
            let meta_rows: Vec<(Vec<u8>, Option<String>, Option<String>, i16)> = sqlx::query_as(
                r#"
                SELECT type_script_hash, name, symbol, decimals
                FROM tokens
                WHERE type_script_hash = ANY($1)
                "#,
            )
            .bind(&token_ids)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

            meta_rows
                .into_iter()
                .map(|(hash, name, symbol, decimals)| (hash, (name, symbol, decimals)))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    let transfers: Vec<AssetTransferResponse> = rows
        .into_iter()
        .map(
            |(
                tx_hash,
                block_number,
                tx_index,
                activity_index,
                activity_category,
                activity_type,
                asset_id,
                from_lock_hash,
                to_lock_hash,
                amount,
                metadata,
                timestamp,
            )| {
                let is_sender = from_lock_hash
                    .as_ref()
                    .map(|h| h == &lock_hash)
                    .unwrap_or(false);
                let is_receiver = to_lock_hash
                    .as_ref()
                    .map(|h| h == &lock_hash)
                    .unwrap_or(false);

                let direction_str = match (is_sender, is_receiver) {
                    (true, false) => "out",
                    (false, true) => "in",
                    _ => "unknown",
                };

                let peer_lock_hash = if is_sender {
                    to_lock_hash
                } else {
                    from_lock_hash
                };

                let event_type = activity_type_to_event_type(&activity_type);

                let (token_name, token_symbol, token_decimals) =
                    if activity_category == "token" && asset_id.is_some() {
                        asset_id
                            .as_ref()
                            .and_then(|id| token_metadata.get(id))
                            .map(|(n, s, d)| (n.clone(), s.clone(), Some(*d)))
                            .unwrap_or((None, None, None))
                    } else {
                        extract_token_meta_from_metadata(&metadata)
                    };

                AssetTransferResponse {
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    block_number,
                    tx_index,
                    event_index: activity_index,
                    asset_category: activity_category,
                    asset_type: activity_type,
                    asset_id: asset_id.map(|id| format!("0x{}", hex::encode(&id))),
                    direction: direction_str.to_string(),
                    peer_address: peer_lock_hash.map(|h| format!("0x{}", hex::encode(&h))),
                    amount: Some(amount),
                    event_type: Some(event_type),
                    timestamp: timestamp.to_rfc3339(),
                    token_name,
                    token_symbol,
                    token_decimals,
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::new(
        transfers,
        total,
        limit,
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
