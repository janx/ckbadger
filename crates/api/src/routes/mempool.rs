#![allow(clippy::manual_is_multiple_of)]

use axum::{extract::State, routing::get, Router};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::cache::CacheTtl;
use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/mempool/info", get(get_mempool_info))
        .route("/mempool/transactions", get(get_mempool_transactions))
        .route("/mempool/blocks", get(get_mempool_blocks))
        .route("/mempool/fees", get(get_recommended_fees))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MempoolInfo {
    pub pending_count: u64,
    pub proposed_count: u64,
    pub orphan_count: u64,
    pub total_size: u64,
    pub total_cycles: u64,
    pub min_fee_rate: u64,
    pub tip_number: u64,
    pub tip_hash: String,
    pub last_updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MempoolTransaction {
    pub tx_hash: String,
    pub fee: u64,
    pub size: u64,
    pub cycles: u64,
    pub fee_rate: f64,
    pub ancestors_count: u64,
    pub timestamp: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MempoolBlock {
    pub index: u32,
    pub transaction_count: u32,
    pub total_size: u64,
    pub total_fee: u64,
    pub total_cycles: u64,
    pub fee_rate_range: FeeRateRange,
    pub median_fee_rate: f64,
    pub estimated_time_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeRateRange {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MempoolBlocksResponse {
    pub pending_blocks: Vec<MempoolBlock>,
    pub total_pending_count: u64,
    pub total_proposed_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendedFees {
    pub fastest_fee: f64,
    pub half_hour_fee: f64,
    pub hour_fee: f64,
    pub economy_fee: f64,
    pub minimum_fee: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcTxPoolInfo {
    tip_hash: String,
    tip_number: String,
    pending: String,
    proposed: String,
    orphan: String,
    total_tx_cycles: String,
    total_tx_size: String,
    min_fee_rate: String,
    last_txs_updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcTxPoolVerbose {
    pending: HashMap<String, RpcTxPoolEntry>,
    proposed: HashMap<String, RpcTxPoolEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcTxPoolEntry {
    cycles: String,
    size: String,
    fee: String,
    ancestors_count: String,
    timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
struct RpcRequest<T> {
    jsonrpc: &'static str,
    method: &'static str,
    params: T,
    id: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

fn parse_hex(hex: &str) -> u64 {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    u64::from_str_radix(hex, 16).unwrap_or(0)
}

async fn fetch_tx_pool_info(url: &str) -> Result<RpcTxPoolInfo, String> {
    let client = Client::new();
    let request = RpcRequest {
        jsonrpc: "2.0",
        method: "tx_pool_info",
        params: (),
        id: 1,
    };

    let response = client
        .post(url)
        .json(&request)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<RpcResponse<RpcTxPoolInfo>>()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(error) = response.error {
        return Err(format!("RPC error {}: {}", error.code, error.message));
    }

    response.result.ok_or_else(|| "Empty response".to_string())
}

async fn fetch_raw_tx_pool_verbose(url: &str) -> Result<RpcTxPoolVerbose, String> {
    let client = Client::new();
    let request = RpcRequest {
        jsonrpc: "2.0",
        method: "get_raw_tx_pool",
        params: (Some(true),),
        id: 1,
    };

    let response = client
        .post(url)
        .json(&request)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<RpcResponse<RpcTxPoolVerbose>>()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(error) = response.error {
        return Err(format!("RPC error {}: {}", error.code, error.message));
    }

    response.result.ok_or_else(|| "Empty response".to_string())
}

async fn get_mempool_info(State(state): State<Arc<AppState>>) -> ApiResult<MempoolInfo> {
    let cache_key = "mempool:info";
    if let Some(cached) = state.cache.get::<MempoolInfo>(cache_key).await {
        return ok(cached);
    }

    let info = fetch_tx_pool_info(&state.ckb_rpc_url)
        .await
        .map_err(ApiError::internal)?;

    let result = MempoolInfo {
        pending_count: parse_hex(&info.pending),
        proposed_count: parse_hex(&info.proposed),
        orphan_count: parse_hex(&info.orphan),
        total_size: parse_hex(&info.total_tx_size),
        total_cycles: parse_hex(&info.total_tx_cycles),
        min_fee_rate: parse_hex(&info.min_fee_rate),
        tip_number: parse_hex(&info.tip_number),
        tip_hash: info.tip_hash,
        last_updated_at: parse_hex(&info.last_txs_updated_at),
    };

    state
        .cache
        .set(cache_key, &result, CacheTtl::MEMPOOL_INFO)
        .await;

    ok(result)
}

async fn get_mempool_transactions(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Vec<MempoolTransaction>> {
    let cache_key = "mempool:transactions";
    if let Some(cached) = state.cache.get::<Vec<MempoolTransaction>>(cache_key).await {
        return ok(cached);
    }

    let pool = fetch_raw_tx_pool_verbose(&state.ckb_rpc_url)
        .await
        .map_err(ApiError::internal)?;

    let mut transactions: Vec<MempoolTransaction> = Vec::new();

    for (hash, entry) in pool.pending.iter() {
        let size = parse_hex(&entry.size);
        let fee = parse_hex(&entry.fee);
        let fee_rate = if size > 0 {
            fee as f64 / size as f64
        } else {
            0.0
        };

        transactions.push(MempoolTransaction {
            tx_hash: hash.clone(),
            fee,
            size,
            cycles: parse_hex(&entry.cycles),
            fee_rate,
            ancestors_count: parse_hex(&entry.ancestors_count),
            timestamp: parse_hex(&entry.timestamp),
            status: "pending".to_string(),
        });
    }

    for (hash, entry) in pool.proposed.iter() {
        let size = parse_hex(&entry.size);
        let fee = parse_hex(&entry.fee);
        let fee_rate = if size > 0 {
            fee as f64 / size as f64
        } else {
            0.0
        };

        transactions.push(MempoolTransaction {
            tx_hash: hash.clone(),
            fee,
            size,
            cycles: parse_hex(&entry.cycles),
            fee_rate,
            ancestors_count: parse_hex(&entry.ancestors_count),
            timestamp: parse_hex(&entry.timestamp),
            status: "proposed".to_string(),
        });
    }

    transactions.sort_by(|a, b| b.fee_rate.total_cmp(&a.fee_rate));

    state
        .cache
        .set(cache_key, &transactions, CacheTtl::MEMPOOL_INFO)
        .await;

    ok(transactions)
}

const CKB_BLOCK_SIZE_LIMIT: u64 = 597_000;
const CKB_BLOCK_CYCLES_LIMIT: u64 = 3_500_000_000;
const CKB_AVG_BLOCK_TIME_SECONDS: u32 = 10;
const MIN_PENDING_BLOCKS: usize = 3;

async fn get_mempool_blocks(
    State(state): State<Arc<AppState>>,
) -> ApiResult<MempoolBlocksResponse> {
    let cache_key = "mempool:blocks";
    if let Some(cached) = state.cache.get::<MempoolBlocksResponse>(cache_key).await {
        return ok(cached);
    }

    let pool = fetch_raw_tx_pool_verbose(&state.ckb_rpc_url)
        .await
        .map_err(ApiError::internal)?;

    let mut all_txs: Vec<(String, u64, u64, u64, f64)> = Vec::new();

    for (hash, entry) in pool.pending.iter().chain(pool.proposed.iter()) {
        let size = parse_hex(&entry.size);
        let fee = parse_hex(&entry.fee);
        let cycles = parse_hex(&entry.cycles);
        let fee_rate = if size > 0 {
            fee as f64 / size as f64
        } else {
            0.0
        };
        all_txs.push((hash.clone(), size, fee, cycles, fee_rate));
    }

    all_txs.sort_by(|a, b| b.4.total_cmp(&a.4));

    let mut pending_blocks: Vec<MempoolBlock> = Vec::new();
    let mut current_block_txs: Vec<(u64, u64, u64, f64)> = Vec::new();
    let mut current_size: u64 = 0;
    let mut current_cycles: u64 = 0;

    for (_hash, size, fee, cycles, fee_rate) in all_txs {
        let would_exceed_size = current_size + size > CKB_BLOCK_SIZE_LIMIT;
        let would_exceed_cycles = current_cycles + cycles > CKB_BLOCK_CYCLES_LIMIT;

        if would_exceed_size || would_exceed_cycles {
            if !current_block_txs.is_empty() {
                let block = create_mempool_block(
                    pending_blocks.len() as u32,
                    &current_block_txs,
                    current_size,
                    current_cycles,
                );
                pending_blocks.push(block);
            }
            current_block_txs.clear();
            current_size = 0;
            current_cycles = 0;
        }

        current_block_txs.push((size, fee, cycles, fee_rate));
        current_size += size;
        current_cycles += cycles;

        if pending_blocks.len() >= 8 {
            break;
        }
    }

    if !current_block_txs.is_empty() && pending_blocks.len() < 8 {
        let block = create_mempool_block(
            pending_blocks.len() as u32,
            &current_block_txs,
            current_size,
            current_cycles,
        );
        pending_blocks.push(block);
    }

    while pending_blocks.len() < MIN_PENDING_BLOCKS {
        pending_blocks.push(create_empty_block(pending_blocks.len() as u32));
    }

    let result = MempoolBlocksResponse {
        pending_blocks,
        total_pending_count: pool.pending.len() as u64,
        total_proposed_count: pool.proposed.len() as u64,
    };

    state
        .cache
        .set(cache_key, &result, CacheTtl::MEMPOOL_INFO)
        .await;

    ok(result)
}

fn create_mempool_block(
    index: u32,
    txs: &[(u64, u64, u64, f64)],
    total_size: u64,
    total_cycles: u64,
) -> MempoolBlock {
    let total_fee: u64 = txs.iter().map(|(_, fee, _, _)| fee).sum();
    let fee_rates: Vec<f64> = txs.iter().map(|(_, _, _, rate)| *rate).collect();

    let min_rate = fee_rates.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_rate = fee_rates.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    let median_fee_rate = if !fee_rates.is_empty() {
        let mut sorted = fee_rates.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let mid = sorted.len() / 2;
        if sorted.len() % 2 == 0 {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        }
    } else {
        0.0
    };

    let estimated_time_minutes = ((index + 1) * CKB_AVG_BLOCK_TIME_SECONDS) / 60;

    MempoolBlock {
        index,
        transaction_count: txs.len() as u32,
        total_size,
        total_fee,
        total_cycles,
        fee_rate_range: FeeRateRange {
            min: if min_rate.is_finite() { min_rate } else { 0.0 },
            max: if max_rate.is_finite() { max_rate } else { 0.0 },
        },
        median_fee_rate,
        estimated_time_minutes,
    }
}

fn create_empty_block(index: u32) -> MempoolBlock {
    let estimated_time_minutes = ((index + 1) * CKB_AVG_BLOCK_TIME_SECONDS) / 60;
    MempoolBlock {
        index,
        transaction_count: 0,
        total_size: 0,
        total_fee: 0,
        total_cycles: 0,
        fee_rate_range: FeeRateRange { min: 0.0, max: 0.0 },
        median_fee_rate: 0.0,
        estimated_time_minutes,
    }
}

async fn get_recommended_fees(State(state): State<Arc<AppState>>) -> ApiResult<RecommendedFees> {
    let cache_key = "mempool:fees";
    if let Some(cached) = state.cache.get::<RecommendedFees>(cache_key).await {
        return ok(cached);
    }

    let info = fetch_tx_pool_info(&state.ckb_rpc_url)
        .await
        .map_err(ApiError::internal)?;

    let min_fee_rate = parse_hex(&info.min_fee_rate) as f64;

    let blocks_response = get_mempool_blocks_internal(&state).await;

    let (fastest, half_hour, hour, economy) = match blocks_response {
        Ok(blocks) if !blocks.pending_blocks.is_empty() => {
            let fastest = blocks
                .pending_blocks
                .first()
                .map(|b| b.fee_rate_range.max)
                .unwrap_or(min_fee_rate)
                .max(min_fee_rate);

            let half_hour = blocks
                .pending_blocks
                .get(2)
                .map(|b| b.median_fee_rate)
                .unwrap_or(min_fee_rate)
                .max(min_fee_rate);

            let hour = blocks
                .pending_blocks
                .get(5)
                .map(|b| b.median_fee_rate)
                .unwrap_or(min_fee_rate)
                .max(min_fee_rate);

            let economy = blocks
                .pending_blocks
                .last()
                .map(|b| b.fee_rate_range.min)
                .unwrap_or(min_fee_rate)
                .max(min_fee_rate);

            (fastest, half_hour, hour, economy)
        }
        _ => (min_fee_rate, min_fee_rate, min_fee_rate, min_fee_rate),
    };

    let result = RecommendedFees {
        fastest_fee: fastest,
        half_hour_fee: half_hour,
        hour_fee: hour,
        economy_fee: economy,
        minimum_fee: min_fee_rate,
    };

    state
        .cache
        .set(cache_key, &result, CacheTtl::MEMPOOL_INFO)
        .await;

    ok(result)
}

async fn get_mempool_blocks_internal(state: &AppState) -> Result<MempoolBlocksResponse, String> {
    let pool = fetch_raw_tx_pool_verbose(&state.ckb_rpc_url).await?;

    let mut all_txs: Vec<(String, u64, u64, u64, f64)> = Vec::new();

    for (hash, entry) in pool.pending.iter().chain(pool.proposed.iter()) {
        let size = parse_hex(&entry.size);
        let fee = parse_hex(&entry.fee);
        let cycles = parse_hex(&entry.cycles);
        let fee_rate = if size > 0 {
            fee as f64 / size as f64
        } else {
            0.0
        };
        all_txs.push((hash.clone(), size, fee, cycles, fee_rate));
    }

    all_txs.sort_by(|a, b| b.4.total_cmp(&a.4));

    let mut pending_blocks: Vec<MempoolBlock> = Vec::new();
    let mut current_block_txs: Vec<(u64, u64, u64, f64)> = Vec::new();
    let mut current_size: u64 = 0;
    let mut current_cycles: u64 = 0;

    for (_hash, size, fee, cycles, fee_rate) in all_txs {
        let would_exceed_size = current_size + size > CKB_BLOCK_SIZE_LIMIT;
        let would_exceed_cycles = current_cycles + cycles > CKB_BLOCK_CYCLES_LIMIT;

        if would_exceed_size || would_exceed_cycles {
            if !current_block_txs.is_empty() {
                let block = create_mempool_block(
                    pending_blocks.len() as u32,
                    &current_block_txs,
                    current_size,
                    current_cycles,
                );
                pending_blocks.push(block);
            }
            current_block_txs.clear();
            current_size = 0;
            current_cycles = 0;
        }

        current_block_txs.push((size, fee, cycles, fee_rate));
        current_size += size;
        current_cycles += cycles;

        if pending_blocks.len() >= 8 {
            break;
        }
    }

    if !current_block_txs.is_empty() && pending_blocks.len() < 8 {
        let block = create_mempool_block(
            pending_blocks.len() as u32,
            &current_block_txs,
            current_size,
            current_cycles,
        );
        pending_blocks.push(block);
    }

    while pending_blocks.len() < MIN_PENDING_BLOCKS {
        pending_blocks.push(create_empty_block(pending_blocks.len() as u32));
    }

    Ok(MempoolBlocksResponse {
        pending_blocks,
        total_pending_count: pool.pending.len() as u64,
        total_proposed_count: pool.proposed.len() as u64,
    })
}
