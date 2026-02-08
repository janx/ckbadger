#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_common::sync::{SyncStatusData, SYNC_STATUS_REDIS_KEY};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;

use crate::cache::{CacheKeys, CacheTtl};
use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::script_to_address;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/blocks", get(list_blocks))
        .route("/blocks/{id}", get(get_block))
        .route("/blocks/{id}/fee-stats", get(get_block_fee_stats))
        .route("/blocks/{id}/proposals", get(get_block_proposals))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<i64>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockResponse {
    pub number: i64,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: String,
    pub transactions_count: i32,
    pub proposals_count: i32,
    pub uncles_count: i32,
    pub epoch: String,
    pub epoch_number: i64,
    pub epoch_index: i32,
    pub epoch_length: i32,
    pub difficulty: String,
    pub nonce: String,
    pub transactions_root: String,
    pub miner_address: Option<String>,
    pub miner_message: Option<String>,
    pub mining_reward: Option<String>,
    pub mining_reward_tx_hash: Option<String>,
    pub compact_target: String,
    pub version: i32,
}

#[derive(Debug, FromRow)]
struct BlockRow {
    number: i64,
    hash: Vec<u8>,
    parent_hash: Vec<u8>,
    timestamp: chrono::DateTime<chrono::Utc>,
    transactions_count: i32,
    proposals_count: i32,
    uncles_count: i32,
    epoch_number: i64,
    epoch_index: i32,
    epoch_length: i32,
    nonce: Vec<u8>,
    transactions_root: Vec<u8>,
    #[allow(dead_code)]
    miner_lock_hash: Option<Vec<u8>>,
    miner_message: Option<Vec<u8>>,
    reward: Option<Decimal>,
    compact_target: i64,
    version: i32,
}

async fn list_blocks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<BlockResponse>> {
    let limit = params.limit.clamp(1, 100);

    let cache_key = format!("{}:{}", CacheKeys::LATEST_BLOCKS, limit);
    let is_first_page = params.cursor.is_none();
    if is_first_page {
        if let Some(cached) = state
            .cache
            .get::<CursorPaginatedResponse<BlockResponse>>(&cache_key)
            .await
        {
            return ok(cached);
        }
    }

    let total = match state
        .cache
        .get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY)
        .await
    {
        Some(status) => status.tip_block_number + 1,
        None => sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(number), 0) + 1 FROM blocks")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?,
    };

    let cursor_number = params.cursor.unwrap_or(i64::MAX);

    let rows: Vec<BlockRow> = sqlx::query_as(
        r#"
        SELECT number, hash, parent_hash, timestamp, transactions_count, proposals_count, uncles_count, 
               epoch_number, epoch_index, epoch_length, nonce, transactions_root, miner_lock_hash, 
               miner_message, reward, compact_target, version
        FROM blocks
        WHERE number < $1
        ORDER BY number DESC
        LIMIT $2
        "#,
    )
    .bind(cursor_number)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|r| r.number.to_string())
    } else {
        None
    };

    let blocks: Vec<BlockResponse> = rows
        .into_iter()
        .map(|r| {
            row_to_block_response(
                r,
                BlockExtra {
                    miner_address: None,
                    mining_reward: None,
                    mining_reward_tx_hash: None,
                },
            )
        })
        .collect();

    let response = CursorPaginatedResponse::new(blocks, total, limit, next_cursor);

    if is_first_page {
        state
            .cache
            .set(&cache_key, &response, CacheTtl::LATEST_BLOCKS)
            .await;
    }

    ok(response)
}

/// Convert compact target to difficulty string (in human-readable format like "1.49 EH")
fn compact_target_to_difficulty(compact: u32) -> String {
    let exponent = compact >> 24;
    let mantissa = compact & 0x00ff_ffff;

    if exponent <= 3 {
        let target = mantissa >> (8 * (3 - exponent));
        if target == 0 {
            return "0".to_string();
        }
        // Difficulty = 2^256 / target, but for very small targets we simplify
        format!("{}", u64::MAX / target as u64)
    } else {
        // Calculate target: mantissa * 256^(exponent-3)
        // Difficulty = 2^256 / target
        // For large targets, we use approximation
        let shift = 8 * (exponent - 3);
        if shift >= 256 {
            return "0".to_string();
        }

        // Use logarithmic calculation for large numbers
        // difficulty ≈ 2^256 / (mantissa * 2^shift) = 2^(256-shift) / mantissa
        let effective_bits = 256 - shift;
        if effective_bits > 64 {
            // Very high difficulty
            let excess_bits = effective_bits - 64;
            let base_difficulty = (1u128 << 64) / mantissa as u128;
            let difficulty = base_difficulty << excess_bits.min(64);

            // Format as human readable
            if difficulty >= 1_000_000_000_000_000_000 {
                // EH (10^18)
                format!("{:.2} EH", difficulty as f64 / 1e18)
            } else if difficulty >= 1_000_000_000_000_000 {
                // PH (10^15)
                format!("{:.2} PH", difficulty as f64 / 1e15)
            } else if difficulty >= 1_000_000_000_000 {
                // TH (10^12)
                format!("{:.2} TH", difficulty as f64 / 1e12)
            } else {
                format!("{}", difficulty)
            }
        } else {
            let difficulty = (1u64 << effective_bits) / mantissa as u64;
            format!("{}", difficulty)
        }
    }
}

struct BlockExtra {
    miner_address: Option<String>,
    mining_reward: Option<String>,
    mining_reward_tx_hash: Option<String>,
}

fn row_to_block_response(row: BlockRow, extra: BlockExtra) -> BlockResponse {
    let difficulty = compact_target_to_difficulty(row.compact_target as u32);

    BlockResponse {
        number: row.number,
        hash: format!("0x{}", hex::encode(&row.hash)),
        parent_hash: format!("0x{}", hex::encode(&row.parent_hash)),
        timestamp: row.timestamp.to_rfc3339(),
        transactions_count: row.transactions_count,
        proposals_count: row.proposals_count,
        uncles_count: row.uncles_count,
        epoch: format!("{}/{}", row.epoch_index, row.epoch_length),
        epoch_number: row.epoch_number,
        epoch_index: row.epoch_index,
        epoch_length: row.epoch_length,
        difficulty,
        nonce: format!("0x{}", hex::encode(&row.nonce)),
        transactions_root: format!("0x{}", hex::encode(&row.transactions_root)),
        miner_address: extra.miner_address,
        miner_message: row.miner_message.map(|m| format!("0x{}", hex::encode(&m))),
        mining_reward: extra
            .mining_reward
            .or_else(|| row.reward.map(|r| r.to_string())),
        mining_reward_tx_hash: extra.mining_reward_tx_hash,
        compact_target: format!("0x{:x}", row.compact_target),
        version: row.version,
    }
}

async fn get_block(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<BlockResponse> {
    let row: Option<BlockRow> = if id.starts_with("0x") {
        let hash = hex::decode(id.strip_prefix("0x").unwrap_or(&id))
            .map_err(|_| ApiError::bad_request("Invalid block hash"))?;

        sqlx::query_as(
            r#"
            SELECT number, hash, parent_hash, timestamp, transactions_count, proposals_count, uncles_count,
                   epoch_number, epoch_index, epoch_length, nonce, transactions_root, miner_lock_hash,
                   miner_message, reward, compact_target, version
            FROM blocks WHERE hash = $1
            "#,
        )
        .bind(&hash)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let number: i64 = id
            .parse()
            .map_err(|_| ApiError::bad_request("Invalid block number"))?;

        sqlx::query_as(
            r#"
            SELECT number, hash, parent_hash, timestamp, transactions_count, proposals_count, uncles_count,
                   epoch_number, epoch_index, epoch_length, nonce, transactions_root, miner_lock_hash,
                   miner_message, reward, compact_target, version
            FROM blocks WHERE number = $1
            "#,
        )
        .bind(number)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    match row {
        Some(r) => {
            let block_hash = format!("0x{}", hex::encode(&r.hash));
            let miner_address = get_miner_address(&state.pool, r.number, &state.ckb_network).await;
            let reward_info = get_mining_reward(&state.ckb_rpc_url, &block_hash, &state.pool).await;
            let (mining_reward, mining_reward_tx_hash) = match reward_info {
                Some(info) => (Some(info.reward), info.cellbase_tx_hash),
                None => (None, None),
            };
            ok(row_to_block_response(
                r,
                BlockExtra {
                    miner_address,
                    mining_reward,
                    mining_reward_tx_hash,
                },
            ))
        }
        None => Err(ApiError::not_found("Block not found")),
    }
}

async fn get_miner_address(
    pool: &sqlx::PgPool,
    block_number: i64,
    network: &str,
) -> Option<String> {
    let result: Option<(Vec<u8>, i16, Vec<u8>)> = sqlx::query_as(
        r#"
        SELECT c.lock_code_hash, c.lock_hash_type, c.lock_args
        FROM cells c
        JOIN transactions t ON c.tx_hash = t.hash AND c.created_at_block = t.block_number
        WHERE t.block_number = $1 AND t.tx_index = 0 AND c.output_index = 0
        LIMIT 1
        "#,
    )
    .bind(block_number)
    .fetch_optional(pool)
    .await
    .ok()?;

    result.and_then(|(code_hash, hash_type, args)| {
        script_to_address(&code_hash, hash_type, &args, network).ok()
    })
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct BlockEconomicState {
    miner_reward: MinerReward,
    finalized_at: String,
}

#[derive(Debug, Deserialize)]
struct MinerReward {
    primary: String,
    secondary: String,
    committed: String,
    proposal: String,
}

struct MiningRewardInfo {
    reward: String,
    cellbase_tx_hash: Option<String>,
}

fn parse_hex_u128(hex: &str) -> u128 {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    u128::from_str_radix(hex, 16).unwrap_or(0)
}

async fn get_mining_reward(
    rpc_url: &str,
    block_hash: &str,
    pool: &sqlx::PgPool,
) -> Option<MiningRewardInfo> {
    let client = reqwest::Client::new();
    let response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "id": 1,
            "jsonrpc": "2.0",
            "method": "get_block_economic_state",
            "params": [block_hash]
        }))
        .send()
        .await
        .ok()?;

    let rpc_result: RpcResponse<BlockEconomicState> = response.json().await.ok()?;
    let economic_state = rpc_result.result?;

    let primary = parse_hex_u128(&economic_state.miner_reward.primary);
    let secondary = parse_hex_u128(&economic_state.miner_reward.secondary);
    let committed = parse_hex_u128(&economic_state.miner_reward.committed);
    let proposal = parse_hex_u128(&economic_state.miner_reward.proposal);

    let total = primary + secondary + committed + proposal;

    let cellbase_tx_hash = get_cellbase_tx_hash(pool, &economic_state.finalized_at).await;

    Some(MiningRewardInfo {
        reward: total.to_string(),
        cellbase_tx_hash,
    })
}

async fn get_cellbase_tx_hash(pool: &sqlx::PgPool, finalized_at_hash: &str) -> Option<String> {
    let hash_bytes = hex::decode(
        finalized_at_hash
            .strip_prefix("0x")
            .unwrap_or(finalized_at_hash),
    )
    .ok()?;

    let result: Option<(Vec<u8>,)> = sqlx::query_as(
        r#"
        SELECT t.hash
        FROM transactions t
        WHERE t.block_hash = $1 AND t.tx_index = 0
        LIMIT 1
        "#,
    )
    .bind(&hash_bytes)
    .fetch_optional(pool)
    .await
    .ok()?;

    result.map(|(hash,)| format!("0x{}", hex::encode(hash)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockFeeStatsResponse {
    pub block_number: i64,
    pub total_size: i64,
    pub total_cycles: i64,
    pub avg_fee_rate: f64,
    pub min_fee_rate: f64,
    pub max_fee_rate: f64,
    pub transaction_count: i32,
}

async fn get_block_fee_stats(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<BlockFeeStatsResponse> {
    let block_number: i64 = if id.starts_with("0x") {
        let hash = hex::decode(id.strip_prefix("0x").unwrap_or(&id))
            .map_err(|_| ApiError::bad_request("Invalid block hash"))?;

        let row: Option<(i64,)> = sqlx::query_as("SELECT number FROM blocks WHERE hash = $1")
            .bind(&hash)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        row.ok_or_else(|| ApiError::not_found("Block not found"))?.0
    } else {
        id.parse()
            .map_err(|_| ApiError::bad_request("Invalid block number"))?
    };

    // total_size includes all txs; fee_rate only from non-cellbase txs
    let stats: Option<(i64, i64, Option<f64>, Option<f64>, Option<f64>, i64)> = sqlx::query_as(
        r#"
        SELECT
            COALESCE(SUM(tx_size), 0)::bigint as total_size,
            COALESCE(SUM(cycles), 0)::bigint as total_cycles,
            AVG(CASE WHEN NOT is_cellbase AND tx_size > 0 THEN (fee::float8 / tx_size::float8) END) as avg_fee_rate,
            MIN(CASE WHEN NOT is_cellbase AND tx_size > 0 THEN (fee::float8 / tx_size::float8) END) as min_fee_rate,
            MAX(CASE WHEN NOT is_cellbase AND tx_size > 0 THEN (fee::float8 / tx_size::float8) END) as max_fee_rate,
            COUNT(*) FILTER (WHERE NOT is_cellbase)::bigint as tx_count
        FROM transactions
        WHERE block_number = $1
        "#,
    )
    .bind(block_number)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    match stats {
        Some((total_size, total_cycles, avg_fee_rate, min_fee_rate, max_fee_rate, tx_count)) => {
            ok(BlockFeeStatsResponse {
                block_number,
                total_size,
                total_cycles,
                avg_fee_rate: avg_fee_rate.unwrap_or(0.0),
                min_fee_rate: min_fee_rate.unwrap_or(0.0),
                max_fee_rate: max_fee_rate.unwrap_or(0.0),
                transaction_count: tx_count as i32,
            })
        }
        None => Err(ApiError::not_found("Block not found")),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockProposal {
    pub proposal_index: i16,
    pub proposal_id: String,
    pub committed_tx_hash: Option<String>,
    pub committed_block_number: Option<i64>,
}

async fn get_block_proposals(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Vec<BlockProposal>> {
    let block_number: i64 = if id.starts_with("0x") {
        let hash = hex::decode(id.strip_prefix("0x").unwrap_or(&id))
            .map_err(|_| ApiError::bad_request("Invalid block hash"))?;

        let row: Option<(i64,)> = sqlx::query_as("SELECT number FROM blocks WHERE hash = $1")
            .bind(&hash)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        row.ok_or_else(|| ApiError::not_found("Block not found"))?.0
    } else {
        id.parse()
            .map_err(|_| ApiError::bad_request("Invalid block number"))?
    };

    let rows: Vec<(i16, Vec<u8>, Option<Vec<u8>>, Option<i64>)> = sqlx::query_as(
        r#"
        SELECT 
            bp.proposal_index, 
            bp.proposal_id,
            t.hash as committed_tx_hash,
            t.block_number as committed_block_number
        FROM block_proposals bp
        LEFT JOIN transactions t ON t.short_hash = bp.proposal_id
            AND t.block_number BETWEEN $1 + 2 AND $1 + 10
        WHERE bp.block_number = $1
        ORDER BY bp.proposal_index
        "#,
    )
    .bind(block_number)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let proposals: Vec<BlockProposal> = rows
        .into_iter()
        .map(
            |(proposal_index, proposal_id, committed_tx_hash, committed_block_number)| {
                BlockProposal {
                    proposal_index,
                    proposal_id: format!("0x{}", hex::encode(proposal_id)),
                    committed_tx_hash: committed_tx_hash.map(|h| format!("0x{}", hex::encode(h))),
                    committed_block_number,
                }
            },
        )
        .collect();

    ok(proposals)
}
