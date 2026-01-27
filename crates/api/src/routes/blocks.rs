#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::cache::{CacheKeys, CacheTtl};
use crate::clickhouse::{hex_hash, unhex_hash};
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

#[derive(Debug, Row, Deserialize)]
struct BlockRowClickHouse {
    number: u64,
    hash: String,
    parent_hash: String,
    timestamp: u32,
    transactions_count: u32,
    proposals_count: u32,
    uncles_count: u32,
    epoch_number: u64,
    epoch_index: u32,
    epoch_length: u32,
    nonce: String,
    transactions_root: String,
    #[allow(dead_code)]
    miner_lock_hash: Option<String>,
    miner_message: Option<String>,
    compact_target: u64,
    version: u32,
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

    let cursor_number = params.cursor.unwrap_or(i64::MAX);

    // Get total count from ClickHouse sync_status
    let total_query = "SELECT tip_block_number + 1 FROM sync_status WHERE id = 1";
    let total_rows = state
        .clickhouse
        .client()
        .query(total_query)
        .fetch_all::<u64>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let total = total_rows
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::internal("Failed to get total block count"))?
        as i64;

    // Query ClickHouse for blocks
    let query = format!(
        "SELECT 
            number,
            {} as hash,
            {} as parent_hash,
            toUnixTimestamp(timestamp) as timestamp,
            transactions_count,
            proposals_count,
            uncles_count,
            epoch_number,
            epoch_index,
            epoch_length,
            {} as nonce,
            {} as transactions_root,
            {} as miner_lock_hash,
            miner_message,
            compact_target,
            version
        FROM blocks
        WHERE number < {}
        ORDER BY number DESC
        LIMIT {}",
        hex_hash("hash"),
        hex_hash("parent_hash"),
        hex_hash("nonce"),
        hex_hash("transactions_root"),
        hex_hash("miner_lock_hash"),
        cursor_number,
        limit + 1
    );

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<BlockRowClickHouse>()
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
        .map(clickhouse_row_to_block_response)
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

fn clickhouse_row_to_block_response(row: BlockRowClickHouse) -> BlockResponse {
    let difficulty = compact_target_to_difficulty(row.compact_target as u32);
    let timestamp = chrono::DateTime::from_timestamp(row.timestamp as i64, 0)
        .unwrap_or_default()
        .to_rfc3339();

    BlockResponse {
        number: row.number as i64,
        hash: format!("0x{}", row.hash),
        parent_hash: format!("0x{}", row.parent_hash),
        timestamp,
        transactions_count: row.transactions_count as i32,
        proposals_count: row.proposals_count as i32,
        uncles_count: row.uncles_count as i32,
        epoch: format!("{}/{}", row.epoch_index, row.epoch_length),
        epoch_number: row.epoch_number as i64,
        epoch_index: row.epoch_index as i32,
        epoch_length: row.epoch_length as i32,
        difficulty,
        nonce: format!("0x{}", row.nonce),
        transactions_root: format!("0x{}", row.transactions_root),
        miner_address: None,
        miner_message: row.miner_message.map(|m| format!("0x{}", m)),
        mining_reward: None,
        mining_reward_tx_hash: None,
        compact_target: format!("0x{:x}", row.compact_target),
        version: row.version as i32,
    }
}

async fn get_block(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<BlockResponse> {
    let query = if id.starts_with("0x") {
        let _hash_bytes = unhex_hash(&id)?;
        format!(
            "SELECT 
                number,
                {} as hash,
                {} as parent_hash,
                toUnixTimestamp(timestamp) as timestamp,
                transactions_count,
                proposals_count,
                uncles_count,
                epoch_number,
                epoch_index,
                epoch_length,
                {} as nonce,
                {} as transactions_root,
                {} as miner_lock_hash,
                miner_message,
                compact_target,
                version
            FROM blocks
            WHERE hash = unhex('{}')
            LIMIT 1",
            hex_hash("hash"),
            hex_hash("parent_hash"),
            hex_hash("nonce"),
            hex_hash("transactions_root"),
            hex_hash("miner_lock_hash"),
            id.strip_prefix("0x").unwrap_or(&id)
        )
    } else {
        let number: i64 = id
            .parse()
            .map_err(|_| ApiError::bad_request("Invalid block number"))?;
        format!(
            "SELECT 
                number,
                {} as hash,
                {} as parent_hash,
                toUnixTimestamp(timestamp) as timestamp,
                transactions_count,
                proposals_count,
                uncles_count,
                epoch_number,
                epoch_index,
                epoch_length,
                {} as nonce,
                {} as transactions_root,
                {} as miner_lock_hash,
                miner_message,
                compact_target,
                version
            FROM blocks
            WHERE number = {}
            LIMIT 1",
            hex_hash("hash"),
            hex_hash("parent_hash"),
            hex_hash("nonce"),
            hex_hash("transactions_root"),
            hex_hash("miner_lock_hash"),
            number
        )
    };

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<BlockRowClickHouse>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match rows.into_iter().next() {
        Some(r) => {
            let block_hash = format!("0x{}", r.hash);
            let block_number = r.number as i64;
            let miner_address =
                get_miner_address(&state.clickhouse, block_number, &state.ckb_network).await;
            let reward_info =
                get_mining_reward(&state.ckb_rpc_url, &block_hash, &state.clickhouse).await;
            let (mining_reward, mining_reward_tx_hash) = match reward_info {
                Some(info) => (Some(info.reward), info.cellbase_tx_hash),
                None => (None, None),
            };

            let mut response = clickhouse_row_to_block_response(r);
            response.miner_address = miner_address;
            response.mining_reward = mining_reward;
            response.mining_reward_tx_hash = mining_reward_tx_hash;

            ok(response)
        }
        None => Err(ApiError::not_found("Block not found")),
    }
}

async fn get_miner_address(
    ch_client: &crate::clickhouse::ClickHouseClient,
    block_number: i64,
    network: &str,
) -> Option<String> {
    let query = format!(
        "SELECT 
            {} as lock_code_hash,
            lock_hash_type,
            {} as lock_args
        FROM cells
        WHERE tx_hash IN (
            SELECT {} as tx_hash FROM transactions WHERE block_number = {} AND tx_index = 0
        ) AND output_index = 0
        LIMIT 1",
        hex_hash("lock_code_hash"),
        hex_hash("lock_args"),
        hex_hash("hash"),
        block_number
    );

    let rows = ch_client
        .client()
        .query(&query)
        .fetch_all::<(String, i16, String)>()
        .await
        .ok()?;

    rows.into_iter()
        .next()
        .and_then(|(code_hash, hash_type, args)| {
            let code_hash_bytes = hex::decode(&code_hash).ok()?;
            let args_bytes = hex::decode(&args).ok()?;
            script_to_address(&code_hash_bytes, hash_type, &args_bytes, network).ok()
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
    ch_client: &crate::clickhouse::ClickHouseClient,
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

    let cellbase_tx_hash = get_cellbase_tx_hash(ch_client, &economic_state.finalized_at).await;

    Some(MiningRewardInfo {
        reward: total.to_string(),
        cellbase_tx_hash,
    })
}

async fn get_cellbase_tx_hash(
    ch_client: &crate::clickhouse::ClickHouseClient,
    finalized_at_hash: &str,
) -> Option<String> {
    let hash_hex = finalized_at_hash
        .strip_prefix("0x")
        .unwrap_or(finalized_at_hash);

    let query = format!(
        "SELECT {} as tx_hash FROM transactions WHERE block_number IN (
            SELECT number FROM blocks WHERE hash = unhex('{}')
        ) AND tx_index = 0 LIMIT 1",
        hex_hash("hash"),
        hash_hex
    );

    let rows = ch_client
        .client()
        .query(&query)
        .fetch_all::<String>()
        .await
        .ok()?;

    rows.into_iter().next().map(|hash| format!("0x{}", hash))
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
        let _hash_bytes = unhex_hash(&id)?;
        let query = format!(
            "SELECT number FROM blocks WHERE hash = unhex('{}') LIMIT 1",
            id.strip_prefix("0x").unwrap_or(&id)
        );

        let rows = state
            .clickhouse
            .client()
            .query(&query)
            .fetch_all::<u64>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        rows.into_iter()
            .next()
            .ok_or_else(|| ApiError::not_found("Block not found"))? as i64
    } else {
        id.parse()
            .map_err(|_| ApiError::bad_request("Invalid block number"))?
    };

    let query = format!(
        "SELECT
            sum(tx_size) as total_size,
            sum(cycles) as total_cycles,
            avg(if(is_cellbase = 0 AND tx_size > 0, fee / tx_size, NULL)) as avg_fee_rate,
            min(if(is_cellbase = 0 AND tx_size > 0, fee / tx_size, NULL)) as min_fee_rate,
            max(if(is_cellbase = 0 AND tx_size > 0, fee / tx_size, NULL)) as max_fee_rate,
            countIf(is_cellbase = 0) as tx_count
        FROM transactions
        WHERE block_number = {}",
        block_number
    );

    #[derive(Row, Deserialize)]
    struct FeeStatsRow {
        total_size: u64,
        total_cycles: u64,
        avg_fee_rate: f64,
        min_fee_rate: f64,
        max_fee_rate: f64,
        tx_count: u64,
    }

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<FeeStatsRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match rows.into_iter().next() {
        Some(stats) => ok(BlockFeeStatsResponse {
            block_number,
            total_size: stats.total_size as i64,
            total_cycles: stats.total_cycles as i64,
            avg_fee_rate: stats.avg_fee_rate,
            min_fee_rate: stats.min_fee_rate,
            max_fee_rate: stats.max_fee_rate,
            transaction_count: stats.tx_count as i32,
        }),
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
        let _hash_bytes = unhex_hash(&id)?;
        let query = format!(
            "SELECT number FROM blocks WHERE hash = unhex('{}') LIMIT 1",
            id.strip_prefix("0x").unwrap_or(&id)
        );

        let rows = state
            .clickhouse
            .client()
            .query(&query)
            .fetch_all::<u64>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        rows.into_iter()
            .next()
            .ok_or_else(|| ApiError::not_found("Block not found"))? as i64
    } else {
        id.parse()
            .map_err(|_| ApiError::bad_request("Invalid block number"))?
    };

    let query = format!(
        "SELECT 
            bp.proposal_index, 
            {} as proposal_id,
            {} as committed_tx_hash,
            t.block_number as committed_block_number
        FROM block_proposals bp
        LEFT JOIN transactions t ON t.short_hash = bp.proposal_id
            AND t.block_number BETWEEN {} AND {}
        WHERE bp.block_number = {}
        ORDER BY bp.proposal_index",
        hex_hash("bp.proposal_id"),
        hex_hash("t.hash"),
        block_number + 2,
        block_number + 10,
        block_number
    );

    #[derive(Row, Deserialize)]
    struct ProposalRow {
        proposal_index: i16,
        proposal_id: String,
        committed_tx_hash: Option<String>,
        committed_block_number: Option<u64>,
    }

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<ProposalRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let proposals: Vec<BlockProposal> = rows
        .into_iter()
        .map(|row| BlockProposal {
            proposal_index: row.proposal_index,
            proposal_id: format!("0x{}", row.proposal_id),
            committed_tx_hash: row.committed_tx_hash.map(|h| format!("0x{}", h)),
            committed_block_number: row.committed_block_number.map(|n| n as i64),
        })
        .collect();

    ok(proposals)
}
