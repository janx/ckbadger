use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::AppState;

const CACHE_TTL_BLOCKS_LIST_SECS: u64 = 5;
const CACHE_KEY_BLOCKS_LIST: &str = "blocks:list";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/blocks", get(list_blocks))
        .route("/blocks/{id}", get(get_block))
        .route("/blocks/{id}/fee-stats", get(get_block_fee_stats))
        .route("/blocks/{id}/proposals", get(get_block_proposals))
}

// ============================================
// Request/Response Types
// ============================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListBlocksParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

/// Fee statistics response for a block
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockFeeStatsResponse {
    pub block_number: u64,
    pub total_fee: u64,
    pub transaction_count: u64,
    pub min_fee: u64,
    pub max_fee: u64,
    pub avg_fee: f64,
    pub median_fee: f64,
}

/// Single proposal entry
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockProposal {
    pub index: u16,
    pub proposal_id: String,
}

/// Proposals response for a block
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockProposalsResponse {
    pub block_number: u64,
    pub proposals_count: u32,
    pub proposals: Vec<BlockProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockResponse {
    pub number: u64,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: i64,
    pub version: u32,
    pub compact_target: u64,
    pub transactions_count: u32,
    pub proposals_count: u32,
    pub uncles_count: u8,
    pub epoch_number: u64,
    pub epoch_index: u32,
    pub epoch_length: u32,
    pub dao: String,
    pub nonce: String,
    pub extra_hash: String,
    pub extension: Option<String>,
    pub proposals_hash: String,
    pub transactions_root: String,
    pub uncles_hash: String,
    pub miner_lock_hash: Option<String>,
    pub miner_message: String,
    pub total_difficulty: String,
    pub reward: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct BlockQueryRow {
    number: u64,
    hash: [u8; 32],
    parent_hash: [u8; 32],
    timestamp: i64, // DateTime64(3) as Unix timestamp millis
    version: u32,
    compact_target: u64,
    transactions_count: u32,
    proposals_count: u32,
    uncles_count: u8,
    epoch_number: u64,
    epoch_index: u32,
    epoch_length: u32,
    dao: [u8; 32],
    nonce: [u8; 16],
    extra_hash: [u8; 32],
    extension: String, // Empty string if none
    proposals_hash: [u8; 32],
    transactions_root: [u8; 32],
    uncles_hash: [u8; 32],
    miner_lock_hash: [u8; 32], // Empty if unknown
    miner_message: String,
    total_difficulty: String, // UInt256 as string
    reward: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct FeeStatsRow {
    total_fee: u64,
    tx_count: u64,
    min_fee: u64,
    max_fee: u64,
    avg_fee: f64,
    median_fee: f64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct ProposalRow {
    proposal_index: u16,
    proposal_id: [u8; 10],
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct BlockNumberRow {
    number: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct BlockInfoRow {
    number: u64,
    proposals_count: u32,
}

impl From<BlockQueryRow> for BlockResponse {
    fn from(row: BlockQueryRow) -> Self {
        Self {
            number: row.number,
            hash: format!("0x{}", hex::encode(row.hash)),
            parent_hash: format!("0x{}", hex::encode(row.parent_hash)),
            timestamp: row.timestamp,
            version: row.version,
            compact_target: row.compact_target,
            transactions_count: row.transactions_count,
            proposals_count: row.proposals_count,
            uncles_count: row.uncles_count,
            epoch_number: row.epoch_number,
            epoch_index: row.epoch_index,
            epoch_length: row.epoch_length,
            dao: format!("0x{}", hex::encode(row.dao)),
            nonce: format!("0x{}", hex::encode(row.nonce)),
            extra_hash: format!("0x{}", hex::encode(row.extra_hash)),
            extension: if row.extension.is_empty() {
                None
            } else {
                Some(row.extension)
            },
            proposals_hash: format!("0x{}", hex::encode(row.proposals_hash)),
            transactions_root: format!("0x{}", hex::encode(row.transactions_root)),
            uncles_hash: format!("0x{}", hex::encode(row.uncles_hash)),
            miner_lock_hash: if row.miner_lock_hash.iter().all(|&b| b == 0) {
                None
            } else {
                Some(format!("0x{}", hex::encode(row.miner_lock_hash)))
            },
            miner_message: row.miner_message,
            total_difficulty: row.total_difficulty,
            reward: row.reward,
        }
    }
}

// ============================================
// Route Handlers
// ============================================

async fn list_blocks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListBlocksParams>,
) -> ApiResult<CursorPaginatedResponse<BlockResponse>> {
    let is_first_page = params.cursor.is_none();
    let cache_key = format!("{}:{}", CACHE_KEY_BLOCKS_LIST, params.limit);

    if is_first_page {
        if let Some(cached) = state
            .cache
            .get::<CursorPaginatedResponse<BlockResponse>>(&cache_key)
            .await
        {
            return ok(cached);
        }
    }

    let cursor_condition = if let Some(ref cursor) = params.cursor {
        let cursor_number: u64 = cursor
            .parse()
            .map_err(|_| ApiError::bad_request("Invalid cursor format"))?;
        format!("AND b.number < {}", cursor_number)
    } else {
        String::new()
    };

    let query = format!(
        "SELECT b.number, b.hash, b.parent_hash, b.timestamp, \
         b.version, b.compact_target, b.transactions_count, b.proposals_count, b.uncles_count, \
         b.epoch_number, b.epoch_index, b.epoch_length, b.dao, b.nonce, b.extra_hash, \
         b.extension, b.proposals_hash, b.transactions_root, b.uncles_hash, \
         b.miner_lock_hash, b.miner_message, toString(b.total_difficulty) as total_difficulty, b.reward \
         FROM blocks_all b \
         INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash \
         WHERE 1=1 {} \
         ORDER BY b.number DESC \
         LIMIT {}",
        cursor_condition,
        params.limit + 1
    );

    let mut rows: Vec<BlockQueryRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query blocks: {}", e)))?;

    let has_more = rows.len() as i64 > params.limit;
    if has_more {
        rows.pop();
    }

    let sync_status = state.cache.get_sync_status(&state.pool).await;
    let total = sync_status.tip_block_number + 1;

    let next_cursor = if has_more {
        rows.last().map(|r| r.number.to_string())
    } else {
        None
    };

    let blocks: Vec<BlockResponse> = rows.into_iter().map(|r| r.into()).collect();
    let response = CursorPaginatedResponse::new(blocks, total, params.limit, next_cursor);

    if is_first_page {
        state
            .cache
            .set(
                &cache_key,
                &response,
                Duration::from_secs(CACHE_TTL_BLOCKS_LIST_SECS),
            )
            .await;
    }

    ok(response)
}

async fn get_block(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<BlockResponse> {
    // Determine if id is a block number or hash
    let query = if id.starts_with("0x") {
        // Hash lookup
        let hash_bytes = hex::decode(id.trim_start_matches("0x"))
            .map_err(|_| ApiError::bad_request("Invalid block hash format"))?;
        if hash_bytes.len() != 32 {
            return Err(ApiError::bad_request("Block hash must be 32 bytes"));
        }

        format!(
            "SELECT b.number, b.hash, b.parent_hash, b.timestamp, \
             b.version, b.compact_target, b.transactions_count, b.proposals_count, b.uncles_count, \
             b.epoch_number, b.epoch_index, b.epoch_length, b.dao, b.nonce, b.extra_hash, \
             b.extension, b.proposals_hash, b.transactions_root, b.uncles_hash, \
             b.miner_lock_hash, b.miner_message, toString(b.total_difficulty) as total_difficulty, b.reward \
             FROM blocks_all b \
             INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash \
             WHERE b.hash = unhex('{}') \
             LIMIT 1",
            hex::encode(hash_bytes)
        )
    } else {
        // Number lookup
        let block_number: u64 = id
            .parse()
            .map_err(|_| ApiError::bad_request("Invalid block number format"))?;

        format!(
            "SELECT b.number, b.hash, b.parent_hash, b.timestamp, \
             b.version, b.compact_target, b.transactions_count, b.proposals_count, b.uncles_count, \
             b.epoch_number, b.epoch_index, b.epoch_length, b.dao, b.nonce, b.extra_hash, \
             b.extension, b.proposals_hash, b.transactions_root, b.uncles_hash, \
             b.miner_lock_hash, b.miner_message, toString(b.total_difficulty) as total_difficulty, b.reward \
             FROM blocks_all b \
             INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash \
             WHERE b.number = {} \
             LIMIT 1",
            block_number
        )
    };

    let row: Option<BlockQueryRow> = state
        .pool
        .query_one(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query block: {}", e)))?;

    match row {
        Some(r) => ok(r.into()),
        None => Err(ApiError::not_found("Block not found")),
    }
}

async fn get_block_fee_stats(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<BlockFeeStatsResponse> {
    let block_number = if id.starts_with("0x") {
        let hash_bytes = hex::decode(id.trim_start_matches("0x"))
            .map_err(|_| ApiError::bad_request("Invalid block hash format"))?;
        if hash_bytes.len() != 32 {
            return Err(ApiError::bad_request("Block hash must be 32 bytes"));
        }

        let query = format!(
            "SELECT b.number \
             FROM blocks_all b \
             INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash \
             WHERE b.hash = unhex('{}') \
             LIMIT 1",
            hex::encode(&hash_bytes)
        );

        let row: Option<BlockNumberRow> = state
            .pool
            .query_one(&query)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to resolve block hash: {}", e)))?;

        row.ok_or_else(|| ApiError::not_found("Block not found"))?
            .number
    } else {
        id.parse::<u64>()
            .map_err(|_| ApiError::bad_request("Invalid block number format"))?
    };

    let query = format!(
        "SELECT \
            sum(t.fee) as total_fee, \
            count() as tx_count, \
            min(t.fee) as min_fee, \
            max(t.fee) as max_fee, \
            avg(t.fee) as avg_fee, \
            median(t.fee) as median_fee \
         FROM transactions_all t \
         INNER JOIN canonical_blocks c ON t.block_number = c.number AND t.block_hash = c.block_hash \
         WHERE t.block_number = {} AND t.is_cellbase = 0",
        block_number
    );

    let row: Option<FeeStatsRow> = state
        .pool
        .query_one(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query fee stats: {}", e)))?;

    let stats = row.unwrap_or(FeeStatsRow {
        total_fee: 0,
        tx_count: 0,
        min_fee: 0,
        max_fee: 0,
        avg_fee: 0.0,
        median_fee: 0.0,
    });

    ok(BlockFeeStatsResponse {
        block_number,
        total_fee: stats.total_fee,
        transaction_count: stats.tx_count,
        min_fee: stats.min_fee,
        max_fee: stats.max_fee,
        avg_fee: stats.avg_fee,
        median_fee: stats.median_fee,
    })
}

async fn get_block_proposals(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<BlockProposalsResponse> {
    let (block_number, proposals_count) = if id.starts_with("0x") {
        let hash_bytes = hex::decode(id.trim_start_matches("0x"))
            .map_err(|_| ApiError::bad_request("Invalid block hash format"))?;
        if hash_bytes.len() != 32 {
            return Err(ApiError::bad_request("Block hash must be 32 bytes"));
        }

        let query = format!(
            "SELECT b.number, b.proposals_count \
             FROM blocks_all b \
             INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash \
             WHERE b.hash = unhex('{}') \
             LIMIT 1",
            hex::encode(&hash_bytes)
        );

        let row: Option<BlockInfoRow> = state
            .pool
            .query_one(&query)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to resolve block hash: {}", e)))?;

        let info = row.ok_or_else(|| ApiError::not_found("Block not found"))?;
        (info.number, info.proposals_count)
    } else {
        let block_number = id
            .parse::<u64>()
            .map_err(|_| ApiError::bad_request("Invalid block number format"))?;

        let query = format!(
            "SELECT b.number, b.proposals_count \
             FROM blocks_all b \
             INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash \
             WHERE b.number = {} \
             LIMIT 1",
            block_number
        );

        let row: Option<BlockInfoRow> = state
            .pool
            .query_one(&query)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to fetch block: {}", e)))?;

        let info = row.ok_or_else(|| ApiError::not_found("Block not found"))?;
        (info.number, info.proposals_count)
    };

    let query = format!(
        "SELECT proposal_index, proposal_id \
         FROM block_proposals \
         WHERE block_number = {} \
         ORDER BY proposal_index ASC",
        block_number
    );

    let rows: Vec<ProposalRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query proposals: {}", e)))?;

    let proposals: Vec<BlockProposal> = rows
        .into_iter()
        .map(|row| BlockProposal {
            index: row.proposal_index,
            proposal_id: format!("0x{}", hex::encode(row.proposal_id)),
        })
        .collect();

    ok(BlockProposalsResponse {
        block_number,
        proposals_count,
        proposals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_ttl_blocks_list_is_5_seconds() {
        assert_eq!(CACHE_TTL_BLOCKS_LIST_SECS, 5);
    }

    #[test]
    fn test_cache_key_blocks_list_has_correct_prefix() {
        assert!(CACHE_KEY_BLOCKS_LIST.starts_with("blocks:"));
    }

    #[test]
    fn test_block_response_serialization() {
        let response = BlockResponse {
            number: 12345,
            hash: "0xabc".to_string(),
            parent_hash: "0xdef".to_string(),
            timestamp: 1704067200000,
            version: 0,
            compact_target: 0x1a2d3e4f,
            transactions_count: 5,
            proposals_count: 2,
            uncles_count: 0,
            epoch_number: 100,
            epoch_index: 50,
            epoch_length: 1800,
            dao: "0x00".to_string(),
            nonce: "0x00".to_string(),
            extra_hash: "0x00".to_string(),
            extension: None,
            proposals_hash: "0x00".to_string(),
            transactions_root: "0x00".to_string(),
            uncles_hash: "0x00".to_string(),
            miner_lock_hash: None,
            miner_message: "".to_string(),
            total_difficulty: "12345".to_string(),
            reward: 100000000,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"number\":12345"));
        assert!(json.contains("\"transactionsCount\":5"));
        assert!(json.contains("\"epochNumber\":100"));
    }

    #[test]
    fn test_block_response_deserialization() {
        let json = r#"{"number":12345,"hash":"0xabc","parentHash":"0xdef","timestamp":1704067200000,"version":0,"compactTarget":439041615,"transactionsCount":5,"proposalsCount":2,"unclesCount":0,"epochNumber":100,"epochIndex":50,"epochLength":1800,"dao":"0x00","nonce":"0x00","extraHash":"0x00","extension":null,"proposalsHash":"0x00","transactionsRoot":"0x00","unclesHash":"0x00","minerLockHash":null,"minerMessage":"","totalDifficulty":"12345","reward":100000000}"#;
        let response: BlockResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.number, 12345);
        assert_eq!(response.transactions_count, 5);
    }
}
