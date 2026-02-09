use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Router,
};
use ckbadger_common::dao::{
    is_genesis_special_burn_cell, GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED,
};
use ckbadger_common::parse_hex_to_bytes;
use ckbadger_common::sync::{SyncStatusData, SYNC_STATUS_REDIS_KEY};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::cycles::{CyclesStatus, CyclesStatusResponse};
use crate::response::{
    decode_cursor, encode_cursor, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::routes::activities::{fetch_transaction_activities, ActivityResponse};
use crate::tx_block_map::get_block_number_for_tx;
use crate::utils::script_to_address;
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
        .route("/transactions/{hash}/activities", get(get_tx_activities))
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

async fn list_transactions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<TransactionResponse>> {
    let limit = params.limit.clamp(1, 100);

    // Get total count - either for specific block or all transactions
    let total: i64 = if let Some(block_number) = params.block_number {
        let row: Option<(i32,)> =
            sqlx::query_as("SELECT tx_count FROM blocks_index WHERE number = $1")
                .bind(block_number)
                .fetch_optional(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
        row.map(|r| r.0 as i64).unwrap_or(0)
    } else {
        match state
            .cache
            .get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY)
            .await
        {
            Some(status) => status.total_transactions,
            None => sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM transactions_index")
                .fetch_one(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?,
        }
    };

    let rows = if let Some(block_number) = params.block_number {
        let cursor = params.cursor.as_ref().and_then(|c| decode_cursor(c));
        let (_cursor_block, cursor_index) = cursor.unwrap_or((i64::MAX, i32::MAX));

        sqlx::query_as::<_, (Vec<u8>, i64, Vec<u8>, i32, i32, i32, String, Option<i32>, Option<i64>, bool, chrono::DateTime<chrono::Utc>)>(
            r#"
            SELECT t.hash, t.block_number, t.block_hash, t.tx_index, t.inputs_count::int4, t.outputs_count::int4, t.fee::text, t.tx_size, t.cycles, t.is_cellbase, t.timestamp
            FROM transactions t
            WHERE t.block_number = $1 AND t.tx_index < $2
            ORDER BY t.tx_index ASC
            LIMIT $3
            "#,
        )
        .bind(block_number)
        .bind(cursor_index)
        .bind(limit + 1)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else if let Some(ref cursor_str) = params.cursor {
        let (cursor_block, cursor_index) = decode_cursor(cursor_str)
            .ok_or_else(|| ApiError::bad_request("Invalid cursor format"))?;

        sqlx::query_as::<_, (Vec<u8>, i64, Vec<u8>, i32, i32, i32, String, Option<i32>, Option<i64>, bool, chrono::DateTime<chrono::Utc>)>(
            r#"
            SELECT t.hash, t.block_number, t.block_hash, t.tx_index, t.inputs_count::int4, t.outputs_count::int4, t.fee::text, t.tx_size, t.cycles, t.is_cellbase, t.timestamp
            FROM transactions t
            WHERE (t.block_number, t.tx_index) < ($1, $2)
            ORDER BY t.block_number DESC, t.tx_index DESC
            LIMIT $3
            "#,
        )
        .bind(cursor_block)
        .bind(cursor_index)
        .bind(limit + 1)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        sqlx::query_as::<_, (Vec<u8>, i64, Vec<u8>, i32, i32, i32, String, Option<i32>, Option<i64>, bool, chrono::DateTime<chrono::Utc>)>(
            r#"
            SELECT t.hash, t.block_number, t.block_hash, t.tx_index, t.inputs_count::int4, t.outputs_count::int4, t.fee::text, t.tx_size, t.cycles, t.is_cellbase, t.timestamp
            FROM transactions t
            ORDER BY t.block_number DESC, t.tx_index DESC
            LIMIT $1
            "#,
        )
        .bind(limit + 1)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(_, block_number, _, tx_index, _, _, _, _, _, _, _)| {
                encode_cursor(*block_number, *tx_index)
            })
    } else {
        None
    };

    let txs: Vec<TransactionResponse> = rows
        .into_iter()
        .map(
            |(
                hash,
                block_number,
                block_hash,
                index,
                inputs_count,
                outputs_count,
                fee,
                tx_size,
                cycles,
                is_cellbase,
                timestamp,
            )| {
                TransactionResponse {
                    hash: format!("0x{}", hex::encode(&hash)),
                    block_number,
                    block_hash: format!("0x{}", hex::encode(&block_hash)),
                    index,
                    inputs_count,
                    outputs_count,
                    fee,
                    tx_size,
                    cycles,
                    is_cellbase,
                    timestamp: timestamp.to_rfc3339(),
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::new(txs, total, limit, next_cursor))
}

async fn get_transaction(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionResponse> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let row = sqlx::query_as::<_, (Vec<u8>, i64, Vec<u8>, i32, i32, i32, String, String, String, Option<i32>, Option<i64>, bool, chrono::DateTime<chrono::Utc>)>(
        r#"
        SELECT t.hash, t.block_number, t.block_hash, t.tx_index, t.inputs_count::int4, t.outputs_count::int4,
               t.fee::text, t.total_input_capacity::text, t.total_output_capacity::text,
               t.tx_size, t.cycles, t.is_cellbase, t.timestamp
        FROM transactions t
        WHERE t.hash = $1
        "#,
    )
    .bind(&hash_bytes)
    .fetch_optional(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    match row {
        Some((
            hash,
            block_number,
            block_hash,
            index,
            inputs_count,
            outputs_count,
            _stored_fee,
            input_cap,
            output_cap,
            tx_size,
            cycles,
            is_cellbase,
            timestamp,
        )) => {
            let input: u128 = input_cap.parse().unwrap_or(0);
            let output: u128 = output_cap.parse().unwrap_or(0);

            let fee = if output > input {
                let dao_compensation: u128 = sqlx::query_as::<_, (Option<String>,)>(
                    "SELECT SUM(compensation::numeric)::text FROM dao_deposits WHERE withdraw_tx = $1 AND status = 2",
                )
                .bind(&hash_bytes)
                .fetch_one(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
                .0
                .and_then(|s| s.parse::<u128>().ok())
                .unwrap_or(0);

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
                hash: format!("0x{}", hex::encode(&hash)),
                block_number,
                block_hash: format!("0x{}", hex::encode(&block_hash)),
                index,
                inputs_count,
                outputs_count,
                fee,
                tx_size,
                cycles,
                is_cellbase,
                timestamp: timestamp.to_rfc3339(),
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
    pub activities: Vec<ActivityResponse>,
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
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionDetailResponse> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let tx_row = sqlx::query_as::<_, (Vec<u8>, i64, Vec<u8>, i32, i32, i32, String, bool, chrono::DateTime<chrono::Utc>, Option<i32>, Option<i64>)>(
        r#"
        SELECT t.hash, t.block_number, t.block_hash, t.tx_index, t.inputs_count::int4, t.outputs_count::int4, t.fee::text, t.is_cellbase, t.timestamp, t.tx_size, t.cycles
        FROM transactions t
        WHERE t.hash = $1
        "#,
    )
    .bind(&hash_bytes)
    .fetch_optional(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (
        tx_hash,
        block_number,
        block_hash,
        index,
        inputs_count,
        outputs_count,
        _stored_fee,
        is_cellbase,
        timestamp,
        tx_size,
        cycles,
    ) = tx_row.ok_or_else(|| ApiError::not_found("Transaction not found"))?;

    let tx_hash_hex = format!("0x{}", hex::encode(&tx_hash));

    // Parallelize independent queries
    type TryJoinError = (axum::http::StatusCode, axum::Json<ApiError>);
    let (tip_block, final_tx_size, input_rows, output_rows, activities) = tokio::try_join!(
        async { Ok::<_, TryJoinError>(state.cache.get_sync_tip(&state.read_pool).await) },
        async {
            Ok::<_, TryJoinError>(match tx_size {
                Some(size) => Some(size),
                None => fetch_tx_size_from_rpc(&state.ckb_rpc_url, &tx_hash_hex).await,
            })
        },
        async {
            sqlx::query_as::<
                _,
                (
                    Vec<u8>,
                    i16,
                    String,
                    Option<String>,
                    Option<i32>,
                    Option<Vec<u8>>,
                    Option<i16>,
                    Option<Vec<u8>>,
                ),
            >(
                r#"
                SELECT ti.previous_tx_hash, ti.previous_output_index, ti.since::TEXT, c.capacity::TEXT,
                       CASE WHEN c.capacity IS NOT NULL THEN
                           8 + 32 + 1 + LENGTH(c.lock_args) +
                           CASE WHEN c.type_code_hash IS NOT NULL THEN 32 + 1 + COALESCE(LENGTH(c.type_args), 0) ELSE 0 END +
                           c.data_size
                       ELSE NULL END::INT as occupied_capacity,
                       c.lock_code_hash, c.lock_hash_type, c.lock_args
                FROM transaction_inputs ti
                LEFT JOIN cells c ON c.tx_hash = ti.previous_tx_hash AND c.output_index = ti.previous_output_index
                WHERE ti.tx_hash = $1 AND ti.tx_block_number = $2
                ORDER BY ti.input_index ASC
                "#,
            )
            .bind(&hash_bytes)
            .bind(block_number)
            .fetch_all(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))
        },
        async {
            sqlx::query_as::<
                _,
                (
                    String,
                    Vec<u8>,
                    i16,
                    Vec<u8>,
                    Option<Vec<u8>>,
                    Option<i16>,
                    Option<Vec<u8>>,
                    i32,
                    i64,
                ),
            >(
                r#"
                SELECT capacity::TEXT, lock_code_hash, lock_hash_type, lock_args,
                       type_code_hash, type_hash_type, type_args,
                       (8 + 32 + 1 + LENGTH(lock_args) +
                       CASE WHEN type_code_hash IS NOT NULL THEN 32 + 1 + COALESCE(LENGTH(type_args), 0) ELSE 0 END +
                       data_size)::INT as occupied_capacity,
                       created_at_block
                FROM cells
                WHERE tx_hash = $1 AND created_at_block = $2
                ORDER BY output_index ASC
                "#,
            )
            .bind(&hash_bytes)
            .bind(block_number)
            .fetch_all(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))
        },
        async {
            fetch_transaction_activities(&state.read_pool, &hash_bytes)
                .await
                .map_err(ApiError::internal)
        },
    )?;

    let confirmations = tip_block - block_number + 1;

    let mut inputs_capacity: u128 = 0;
    let mut inputs_occupied_capacity: u128 = 0;
    let inputs: Vec<TransactionInputResponse> = input_rows
        .into_iter()
        .map(
            |(
                prev_tx_hash,
                prev_index,
                since,
                capacity,
                occupied,
                lock_code_hash,
                lock_hash_type,
                lock_args,
            )| {
                if let Some(ref cap) = capacity {
                    inputs_capacity += cap.parse::<u128>().unwrap_or(0);
                }
                if let Some(occ) = occupied {
                    inputs_occupied_capacity += occ as u128;
                }

                let (lock, address) = match (lock_code_hash, lock_hash_type, lock_args) {
                    (Some(code_hash), Some(hash_type), Some(args)) => {
                        let lock = ScriptResponse {
                            code_hash: format!("0x{}", hex::encode(&code_hash)),
                            hash_type: hash_type_to_string(hash_type),
                            args: format!("0x{}", hex::encode(&args)),
                        };
                        let address =
                            script_to_address(&code_hash, hash_type, &args, &state.ckb_network)
                                .ok();
                        (Some(lock), address)
                    }
                    _ => (None, None),
                };

                TransactionInputResponse {
                    previous_output: Some(PreviousOutput {
                        tx_hash: format!("0x{}", hex::encode(&prev_tx_hash)),
                        index: prev_index as i32,
                    }),
                    since,
                    capacity,
                    lock,
                    address,
                }
            },
        )
        .collect();

    let mut outputs_capacity: u128 = 0;
    let mut outputs_occupied_capacity: u128 = 0;
    let outputs: Vec<TransactionOutputResponse> = output_rows
        .into_iter()
        .map(
            |(
                capacity,
                lock_code_hash,
                lock_hash_type,
                lock_args,
                type_code_hash,
                type_hash_type,
                type_args,
                occupied,
                created_at_block,
            )| {
                outputs_capacity += capacity.parse::<u128>().unwrap_or(0);
                outputs_occupied_capacity += occupied as u128;

                let lock = Some(ScriptResponse {
                    code_hash: format!("0x{}", hex::encode(&lock_code_hash)),
                    hash_type: hash_type_to_string(lock_hash_type),
                    args: format!("0x{}", hex::encode(&lock_args)),
                });

                let address = script_to_address(
                    &lock_code_hash,
                    lock_hash_type,
                    &lock_args,
                    &state.ckb_network,
                )
                .ok();

                let type_script = match (&type_code_hash, type_hash_type, &type_args) {
                    (Some(code_hash), Some(hash_type), Some(args)) => Some(ScriptResponse {
                        code_hash: format!("0x{}", hex::encode(code_hash)),
                        hash_type: hash_type_to_string(hash_type),
                        args: format!("0x{}", hex::encode(args)),
                    }),
                    _ => None,
                };

                let is_satoshi = is_genesis_special_burn_cell(&lock_args, created_at_block);
                let (cell_type, virtual_occupied_capacity) = if is_satoshi {
                    (
                        Some("genesis_special_burn".to_string()),
                        Some(GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED.to_string()),
                    )
                } else {
                    (None, None)
                };

                TransactionOutputResponse {
                    capacity,
                    occupied_capacity: occupied as i64,
                    virtual_occupied_capacity,
                    cell_type,
                    lock,
                    r#type: type_script,
                    address,
                }
            },
        )
        .collect();

    // Fee calculation: only query DAO compensation when outputs > inputs (~1% of transactions)
    let fee = if outputs_capacity > inputs_capacity {
        // Outputs exceed inputs - could be DAO withdrawal or special protocol (DAS, etc.)
        let dao_compensation: u128 = sqlx::query_as::<_, (Option<String>,)>(
            "SELECT SUM(compensation::numeric)::text FROM dao_deposits WHERE withdraw_tx = $1 AND status = 2",
        )
        .bind(&hash_bytes)
        .fetch_one(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .0
        .and_then(|s| s.parse::<u128>().ok())
        .unwrap_or(0);

        let effective_input = inputs_capacity + dao_compensation;
        if effective_input >= outputs_capacity {
            (effective_input - outputs_capacity).to_string()
        } else {
            "0".to_string()
        }
    } else {
        (inputs_capacity - outputs_capacity).to_string()
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
        block_number,
        block_hash: format!("0x{}", hex::encode(&block_hash)),
        index,
        inputs_count,
        outputs_count,
        fee,
        fee_rate,
        tx_size: final_tx_size,
        cycles,
        confirmations,
        is_cellbase,
        timestamp: timestamp.to_rfc3339(),
        inputs_capacity: inputs_capacity.to_string(),
        outputs_capacity: outputs_capacity.to_string(),
        inputs_occupied_capacity: inputs_occupied_capacity.to_string(),
        outputs_occupied_capacity: outputs_occupied_capacity.to_string(),
        inputs,
        outputs,
        activities,
    })
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

    // 2-phase lookup: get block_number from tx_block_map for partition pruning
    let block_number = get_block_number_for_tx(&state.read_pool, &hash_bytes)
        .await
        .ok()
        .flatten();

    let rows = if let Some(bn) = block_number {
        // Fast path: partition-pruned query
        sqlx::query_as::<_, (Vec<u8>, i16, i16)>(
            r#"
            SELECT out_point_tx_hash, out_point_index, dep_type
            FROM transaction_cell_deps
            WHERE tx_hash = $1 AND tx_block_number = $2
            ORDER BY dep_index ASC
            "#,
        )
        .bind(&hash_bytes)
        .bind(bn)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        // Fallback: full scan (tx_block_map not populated)
        sqlx::query_as::<_, (Vec<u8>, i16, i16)>(
            r#"
            SELECT out_point_tx_hash, out_point_index, dep_type
            FROM transaction_cell_deps
            WHERE tx_hash = $1
            ORDER BY dep_index ASC
            "#,
        )
        .bind(&hash_bytes)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let cell_deps: Vec<CellDepResponse> = rows
        .into_iter()
        .map(
            |(out_point_tx_hash, out_point_index, dep_type)| CellDepResponse {
                out_point_tx_hash: format!("0x{}", hex::encode(&out_point_tx_hash)),
                out_point_index: out_point_index as i32,
                dep_type: match dep_type {
                    1 => "dep_group".to_string(),
                    _ => "code".to_string(),
                },
            },
        )
        .collect();

    ok(cell_deps)
}

async fn get_cycles_status(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<CyclesStatusResponse> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let row: Option<(Option<i64>, bool)> =
        sqlx::query_as("SELECT cycles, is_cellbase FROM transactions_index WHERE hash = $1")
            .bind(&hash_bytes)
            .fetch_optional(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let (db_cycles, is_cellbase) = match row {
        Some((cycles, cellbase)) => (cycles, cellbase),
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
        Some(-1) => {
            let error = state.cycles_calculator.get_error(&hash).await;
            ok(CyclesStatusResponse {
                status: CyclesStatus::Failed,
                cycles: None,
                error: error.or_else(|| Some("Calculation failed".to_string())),
            })
        }
        _ => {
            let queue_status = state.cycles_calculator.get_status(&hash).await;

            match queue_status {
                CyclesStatus::Calculating => ok(CyclesStatusResponse {
                    status: CyclesStatus::Calculating,
                    cycles: None,
                    error: None,
                }),
                CyclesStatus::Queued => ok(CyclesStatusResponse {
                    status: CyclesStatus::Queued,
                    cycles: None,
                    error: None,
                }),
                _ => ok(CyclesStatusResponse {
                    status: CyclesStatus::Done,
                    cycles: None,
                    error: None,
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

    let row: Option<(Option<i64>, bool)> =
        sqlx::query_as("SELECT cycles, is_cellbase FROM transactions_index WHERE hash = $1")
            .bind(&hash_bytes)
            .fetch_optional(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let (db_cycles, is_cellbase) = match row {
        Some((cycles, cellbase)) => (cycles, cellbase),
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
            let status = state.cycles_calculator.request_calculation(&hash).await;
            ok(CyclesStatusResponse {
                status,
                cycles: None,
                error: None,
            })
        }
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

    // Get the short_hash (proposal_id) - first 10 bytes
    let short_hash = if hash_bytes.len() >= 10 {
        hash_bytes[..10].to_vec()
    } else {
        return Err(ApiError::bad_request("Transaction hash too short"));
    };

    // Query transaction info
    let tx_row: Option<(i64, Vec<u8>, bool, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        r#"
        SELECT t.block_number, t.block_hash, t.is_cellbase, t.timestamp
        FROM transactions t
        WHERE t.hash = $1
        "#,
    )
    .bind(&hash_bytes)
    .fetch_optional(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (commit_block_number, commit_block_hash, is_cellbase, commit_timestamp) = match tx_row {
        Some(row) => row,
        None => {
            // Transaction not found - might be pending in mempool
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

    let hash_hex = format!("0x{}", hex::encode(&hash_bytes));
    let proposal_id_hex = format!("0x{}", hex::encode(&short_hash));

    // Cellbase transactions don't go through proposal phase
    if is_cellbase {
        let tip = state.cache.get_sync_tip(&state.read_pool).await;

        return ok(TransactionLifecycleResponse {
            hash: hash_hex,
            phase: LifecyclePhase::Committed,
            proposal_id: proposal_id_hex,
            proposed_in: None,
            committed_in: Some(LifecycleBlockInfo {
                block_number: commit_block_number,
                block_hash: format!("0x{}", hex::encode(&commit_block_hash)),
                timestamp: commit_timestamp.to_rfc3339(),
            }),
            commitment_distance: None,
            commitment_window: CommitmentWindow::default(),
            is_cellbase: true,
            confirmations: Some(tip - commit_block_number + 1),
        });
    }

    // Find proposal block - look in the valid proposal window before commit
    // A transaction committed in block C must be proposed in block P where: C - 10 <= P <= C - 2
    let proposal_row: Option<(i64, Vec<u8>, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        r#"
        SELECT bp.block_number, b.hash, b.timestamp
        FROM block_proposals bp
        JOIN blocks_index b ON b.number = bp.block_number
        WHERE bp.proposal_id = $1
          AND bp.block_number BETWEEN $2 - 10 AND $2 - 2
        ORDER BY bp.block_number ASC
        LIMIT 1
        "#,
    )
    .bind(&short_hash)
    .bind(commit_block_number)
    .fetch_optional(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let tip = state.cache.get_sync_tip(&state.read_pool).await;

    let (proposed_in, commitment_distance) = match proposal_row {
        Some((proposal_block, proposal_hash, proposal_timestamp)) => (
            Some(LifecycleBlockInfo {
                block_number: proposal_block,
                block_hash: format!("0x{}", hex::encode(&proposal_hash)),
                timestamp: proposal_timestamp.to_rfc3339(),
            }),
            Some(commit_block_number - proposal_block),
        ),
        None => (None, None),
    };

    ok(TransactionLifecycleResponse {
        hash: hash_hex,
        phase: LifecyclePhase::Committed,
        proposal_id: proposal_id_hex,
        proposed_in,
        committed_in: Some(LifecycleBlockInfo {
            block_number: commit_block_number,
            block_hash: format!("0x{}", hex::encode(&commit_block_hash)),
            timestamp: commit_timestamp.to_rfc3339(),
        }),
        commitment_distance,
        commitment_window: CommitmentWindow::default(),
        is_cellbase: false,
        confirmations: Some(tip - commit_block_number + 1),
    })
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
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Vec<TxAssetTransferResponse>> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

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

    let block_number = get_block_number_for_tx(&state.read_pool, &hash_bytes)
        .await
        .ok()
        .flatten();

    let rows: Vec<ActivityRow> = if let Some(bn) = block_number {
        sqlx::query_as(
            r#"
            SELECT tx_hash, block_number, tx_index, activity_index,
                   activity_category, activity_type, asset_id,
                   from_lock_hash, to_lock_hash, amount::TEXT, metadata, timestamp
            FROM activities
            WHERE tx_hash = $1 AND block_number = $2 AND activity_category IN ('token', 'dob', 'nft', 'dao')
            ORDER BY activity_index ASC
            "#,
        )
        .bind(&hash_bytes)
        .bind(bn)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        sqlx::query_as(
            r#"
            SELECT tx_hash, block_number, tx_index, activity_index,
                   activity_category, activity_type, asset_id,
                   from_lock_hash, to_lock_hash, amount::TEXT, metadata, timestamp
            FROM activities
            WHERE tx_hash = $1 AND activity_category IN ('token', 'dob', 'nft', 'dao')
            ORDER BY activity_index ASC
            "#,
        )
        .bind(&hash_bytes)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let token_ids: Vec<Vec<u8>> = rows
        .iter()
        .filter(|(_, _, _, _, cat, _, asset_id, _, _, _, _, _)| {
            cat == "token" && asset_id.is_some()
        })
        .filter_map(|(_, _, _, _, _, _, asset_id, _, _, _, _, _)| asset_id.clone())
        .collect();

    type TokenMeta = (Vec<u8>, Option<String>, Option<String>, i16);
    let token_metadata: std::collections::HashMap<Vec<u8>, (Option<String>, Option<String>, i16)> =
        if !token_ids.is_empty() {
            let meta_rows: Vec<TokenMeta> = sqlx::query_as(
                r#"
                SELECT type_script_hash, name, symbol, decimals
                FROM tokens
                WHERE type_script_hash = ANY($1)
                "#,
            )
            .bind(&token_ids)
            .fetch_all(&state.read_pool)
            .await
            .unwrap_or_default();

            meta_rows
                .into_iter()
                .map(|(hash, name, symbol, decimals)| (hash, (name, symbol, decimals)))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    let transfers: Vec<TxAssetTransferResponse> = rows
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
                let direction_str = if from_lock_hash.is_some() && to_lock_hash.is_none() {
                    "out"
                } else if from_lock_hash.is_none() && to_lock_hash.is_some() {
                    "in"
                } else {
                    "transfer"
                };

                let peer_lock_hash = if from_lock_hash.is_some() {
                    to_lock_hash
                } else {
                    from_lock_hash
                };

                let event_type = match activity_type.as_str() {
                    "TOKEN_MINT" | "DOB_MINT" | "NFT_MINT" => "mint",
                    "TOKEN_BURN" | "DOB_BURN" => "burn",
                    "TOKEN_TRANSFER" | "DOB_TRANSFER" | "NFT_TRANSFER" => "transfer",
                    "DAO_DEPOSIT" => "deposit",
                    "DAO_WITHDRAW_REQUEST" => "withdraw_request",
                    "DAO_WITHDRAW_COMPLETE" => "withdraw_complete",
                    _ => &activity_type.to_lowercase(),
                };

                let (token_name, token_symbol, token_decimals) =
                    if activity_category == "token" && asset_id.is_some() {
                        asset_id
                            .as_ref()
                            .and_then(|id| token_metadata.get(id))
                            .map(|(n, s, d)| (n.clone(), s.clone(), Some(*d)))
                            .unwrap_or_else(|| extract_token_meta(&metadata))
                    } else {
                        extract_token_meta(&metadata)
                    };

                TxAssetTransferResponse {
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
                    event_type: Some(event_type.to_string()),
                    timestamp: timestamp.to_rfc3339(),
                    token_name,
                    token_symbol,
                    token_decimals,
                }
            },
        )
        .collect();

    ok(transfers)
}

fn extract_token_meta(
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

async fn get_tx_activities(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Vec<ActivityResponse>> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    if hash_bytes.len() != 32 {
        return Err(ApiError::bad_request(
            "Transaction hash must be 32 bytes (64 hex chars)",
        ));
    }

    let activities = fetch_transaction_activities(&state.read_pool, &hash_bytes)
        .await
        .map_err(ApiError::internal)?;

    ok(activities)
}
