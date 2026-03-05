#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckb_store_reader::CkbChainReader;
use ckbadger_common::hardforks_for_network;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::cache::{CacheBackend, CacheKeys, CacheTtl};
use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::{ensure_derived_ready, script_to_address};
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
pub struct HardforkResourceResponse {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardforkActivationResponse {
    pub id: String,
    pub name: String,
    pub short_name: String,
    pub activation_epoch: i64,
    pub activation_date: String,
    pub resources: Vec<HardforkResourceResponse>,
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
    pub hardfork_activation: Option<HardforkActivationResponse>,
    pub compact_target: String,
    pub version: i32,
}

async fn list_blocks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<BlockResponse>> {
    ensure_derived_ready(state.as_ref())?;

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

    let total = state
        .store
        .get_sync_status()
        .map(|s| s.tip_block_number + 1)
        .unwrap_or(0);

    // Use from_block: for cursor pagination, we want blocks with number < cursor
    // list_blocks_desc takes from_block as the starting point (inclusive in scan, but we want exclusive)
    // Since list_blocks_desc starts from `from_block` and goes backwards, we pass cursor - 1
    // or if no cursor, None (which starts from the end)
    let from_block = params.cursor.map(|c| c - 1);
    let fetch_limit = (limit + 1) as usize;
    let network = state.ckb_network.clone();

    let store = state.store.clone();
    let stats_store = state.store.clone();
    let ckb_store = state.ckb_store.clone();
    let hardfork_activation_by_block = tokio::task::spawn_blocking({
        let stats_store = stats_store.clone();
        move || resolve_hardfork_activation_blocks(&network, &stats_store)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let rows = tokio::task::spawn_blocking(move || store.list_blocks_desc(from_block, fetch_limit))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|(num, _)| num.to_string())
    } else {
        None
    };

    let blocks: Vec<BlockResponse> = rows
        .into_iter()
        .map(|(block_num, header)| {
            let header_info = enrich_from_rocksdb(&ckb_store, &header.hash);
            cached_header_to_block_response(
                block_num,
                header,
                header_info,
                BlockExtra {
                    miner_address: None,
                    miner_message: None,
                    mining_reward: None,
                    mining_reward_tx_hash: None,
                    hardfork_activation: hardfork_activation_by_block.get(&block_num).cloned(),
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

fn resolve_hardfork_activation_blocks(
    network: &str,
    stats_store: &ckbadger_store::CkbadgerStore,
) -> anyhow::Result<HashMap<i64, HardforkActivationResponse>> {
    let Some(specs) = hardforks_for_network(network) else {
        return Ok(HashMap::new());
    };

    let mut activations = HashMap::new();
    for spec in specs {
        let activation_block = stats_store
            .get_epoch_stats(spec.activation_epoch)?
            .map(|stats| stats.start_block);
        if let Some(activation_block) = activation_block {
            activations.insert(
                activation_block,
                HardforkActivationResponse {
                    id: spec.id.to_string(),
                    name: spec.name.to_string(),
                    short_name: spec.short_name.to_string(),
                    activation_epoch: spec.activation_epoch,
                    activation_date: spec.activation_date.to_string(),
                    resources: spec
                        .resources
                        .iter()
                        .map(|resource| HardforkResourceResponse {
                            label: resource.label.to_string(),
                            url: resource.url.to_string(),
                        })
                        .collect(),
                },
            );
        }
    }

    Ok(activations)
}

struct BlockExtra {
    miner_address: Option<String>,
    miner_message: Option<Vec<u8>>,
    mining_reward: Option<String>,
    mining_reward_tx_hash: Option<String>,
    hardfork_activation: Option<HardforkActivationResponse>,
}

/// Try to read header info from CKB node's RocksDB for fields not in our store.
fn enrich_from_rocksdb(
    ckb_store: &Option<Arc<CkbChainReader>>,
    block_hash: &[u8],
) -> Option<ckb_store_reader::BlockHeaderInfo> {
    let store = ckb_store.as_ref()?;
    if block_hash.len() != 32 {
        return None;
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(block_hash);
    store.get_block_header_info(&hash)
}

fn cached_header_to_block_response(
    block_num: i64,
    header: ckbadger_store::CachedBlockHeader,
    header_info: Option<ckb_store_reader::BlockHeaderInfo>,
    extra: BlockExtra,
) -> BlockResponse {
    let (parent_hash, nonce, transactions_root, version, compact_target, difficulty) =
        match header_info {
            Some(ref info) => {
                // We don't have compact_target in BlockHeaderInfo, default to "0x0"
                (
                    format!("0x{}", hex::encode(info.parent_hash)),
                    format!("0x{}", hex::encode(info.nonce.to_le_bytes())),
                    format!("0x{}", hex::encode(info.transactions_root)),
                    info.version as i32,
                    "0x0".to_string(),
                    "0".to_string(),
                )
            }
            None => (
                "0x".to_string(),
                "0x0".to_string(),
                "0x".to_string(),
                0,
                "0x0".to_string(),
                "0".to_string(),
            ),
        };

    // Format timestamp from millis to RFC3339
    let timestamp = chrono::DateTime::from_timestamp_millis(header.timestamp)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();

    BlockResponse {
        number: block_num,
        hash: format!("0x{}", hex::encode(&header.hash)),
        parent_hash,
        timestamp,
        transactions_count: header.transactions_count,
        proposals_count: 0, // Not stored in CachedBlockHeader; unavailable without RPC
        uncles_count: 0,    // Not stored in CachedBlockHeader; unavailable without RPC
        epoch: format!("{}/{}", header.epoch_index, header.epoch_length),
        epoch_number: header.epoch_number,
        epoch_index: header.epoch_index,
        epoch_length: header.epoch_length,
        difficulty,
        nonce,
        transactions_root,
        miner_address: extra.miner_address,
        miner_message: extra
            .miner_message
            .map(|m| format!("0x{}", hex::encode(&m))),
        mining_reward: extra.mining_reward,
        mining_reward_tx_hash: extra.mining_reward_tx_hash,
        hardfork_activation: extra.hardfork_activation,
        compact_target,
        version,
    }
}

fn resolve_hardfork_activation(
    network: &str,
    stats_store: &ckbadger_store::CkbadgerStore,
    block_num: i64,
    epoch_number: i64,
    epoch_index: i32,
) -> anyhow::Result<Option<HardforkActivationResponse>> {
    let Some(specs) = hardforks_for_network(network) else {
        return Ok(None);
    };

    for spec in specs {
        if spec.activation_epoch != epoch_number {
            continue;
        }

        let activation_block = stats_store
            .get_epoch_stats(spec.activation_epoch)?
            .map(|stats| stats.start_block);

        let is_activation = match activation_block {
            Some(num) => num == block_num,
            None => epoch_index == 0,
        };

        if is_activation {
            return Ok(Some(HardforkActivationResponse {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                short_name: spec.short_name.to_string(),
                activation_epoch: spec.activation_epoch,
                activation_date: spec.activation_date.to_string(),
                resources: spec
                    .resources
                    .iter()
                    .map(|resource| HardforkResourceResponse {
                        label: resource.label.to_string(),
                        url: resource.url.to_string(),
                    })
                    .collect(),
            }));
        }
    }

    Ok(None)
}

async fn get_block(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<BlockResponse> {
    ensure_derived_ready(state.as_ref())?;

    let store = state.store.clone();
    let ckb_store = state.ckb_store.clone();

    let block_result: Option<(i64, ckbadger_store::CachedBlockHeader)> = if id.starts_with("0x") {
        let hash = hex::decode(id.strip_prefix("0x").unwrap_or(&id))
            .map_err(|_| ApiError::bad_request("Invalid block hash"))?;

        let store_c = store.clone();
        let hash_c = hash.clone();
        tokio::task::spawn_blocking(move || -> Result<_, anyhow::Error> {
            if let Some(block_num) = store_c.get_block_number_by_hash(&hash_c)? {
                if let Some(header) = store_c.get_block_header(block_num)? {
                    return Ok(Some((block_num, header)));
                }
            }
            Ok(None)
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        let number: i64 = id
            .parse()
            .map_err(|_| ApiError::bad_request("Invalid block number"))?;

        let store_c = store.clone();
        tokio::task::spawn_blocking(move || -> Result<_, anyhow::Error> {
            Ok(store_c
                .get_block_header(number)?
                .map(|header| (number, header)))
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    match block_result {
        Some((block_num, header)) => {
            let block_hash = format!("0x{}", hex::encode(&header.hash));
            let header_info = enrich_from_rocksdb(&ckb_store, &header.hash);

            // Get miner message from CKB's RocksDB
            let miner_message = ckb_store.as_ref().and_then(|s| {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&header.hash);
                s.get_miner_message(&hash)
            });

            // Get miner address from cellbase output (tx_index=0, output_index=0)
            let miner_address =
                get_miner_address_from_store(&ckb_store, &header.hash, &state.ckb_network);

            let reward_info =
                get_mining_reward(&state.ckb_rpc_url, &block_hash, &store, &state.cache).await;
            let (mining_reward, mining_reward_tx_hash) = match reward_info {
                Some(info) => (Some(info.reward), info.cellbase_tx_hash),
                None => (None, None),
            };
            let activation = {
                let stats_store_c = state.store.clone();
                let network = state.ckb_network.clone();
                let block_num_c = block_num;
                let epoch_number = header.epoch_number;
                let epoch_index = header.epoch_index;
                tokio::task::spawn_blocking(move || {
                    resolve_hardfork_activation(
                        &network,
                        &stats_store_c,
                        block_num_c,
                        epoch_number,
                        epoch_index,
                    )
                })
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
                .map_err(|e| ApiError::internal(e.to_string()))?
            };

            ok(cached_header_to_block_response(
                block_num,
                header,
                header_info,
                BlockExtra {
                    miner_address,
                    miner_message,
                    mining_reward,
                    mining_reward_tx_hash,
                    hardfork_activation: activation,
                },
            ))
        }
        None => Err(ApiError::not_found("Block not found")),
    }
}

/// Get miner address from the cellbase transaction output in CKB's RocksDB.
fn get_miner_address_from_store(
    ckb_store: &Option<Arc<CkbChainReader>>,
    block_hash: &[u8],
    network: &str,
) -> Option<String> {
    let store = ckb_store.as_ref()?;
    if block_hash.len() != 32 {
        return None;
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(block_hash);

    let block = store.get_block(&hash)?;
    let txs = block.transactions();
    let cellbase = txs.first()?;
    let output = cellbase.output(0)?;
    let lock = output.lock();

    let code_hash: Vec<u8> = lock.code_hash().raw_data().to_vec();
    let hash_type_byte = lock.hash_type().as_bytes()[0];
    let hash_type = match hash_type_byte {
        0 => 0i16,
        1 => 1i16,
        2 => 2i16,
        4 => 4i16,
        _ => 0i16,
    };
    let args: Vec<u8> = lock.args().raw_data().to_vec();

    script_to_address(&code_hash, hash_type, &args, network).ok()
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

#[derive(Debug, Serialize, Deserialize)]
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
    store: &Arc<ckbadger_store::CkbadgerStore>,
    cache: &CacheBackend,
) -> Option<MiningRewardInfo> {
    let cache_key = CacheKeys::mining_reward(block_hash);
    if let Some(cached) = cache.get::<MiningRewardInfo>(&cache_key).await {
        return Some(cached);
    }

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

    let cellbase_tx_hash = get_cellbase_tx_hash(store, &economic_state.finalized_at);

    let info = MiningRewardInfo {
        reward: total.to_string(),
        cellbase_tx_hash,
    };

    cache.set(&cache_key, &info, CacheTtl::MINING_REWARD).await;

    Some(info)
}

/// Get cellbase tx hash by looking up the block hash, then finding tx_index=0.
fn get_cellbase_tx_hash(
    store: &ckbadger_store::CkbadgerStore,
    finalized_at_hash: &str,
) -> Option<String> {
    let hash_bytes = hex::decode(
        finalized_at_hash
            .strip_prefix("0x")
            .unwrap_or(finalized_at_hash),
    )
    .ok()?;

    let block_num = store.get_block_number_by_hash(&hash_bytes).ok()??;
    // List txs for this block and find tx_index=0 (cellbase)
    let txs = store.list_block_txs(block_num).ok()?;
    // The tx hash is stored in tx_hash_map as value -> (block_num, tx_idx),
    // but we need the reverse: given block_num+tx_idx=0, get the tx_hash.
    // We don't have a direct reverse lookup, so we use the CKB store if available,
    // or iterate the tx_hash_map. For now, return None if we can't find it easily.
    // However, we can scan the tx_hash_map looking for the matching (block_num, 0).
    // That's expensive. Instead, let's return None for the cellbase hash.
    // The mining reward info still has the reward amount.
    if txs.is_empty() {
        return None;
    }
    // The tx hash is not stored in TxIndexEntry. We need to do a reverse lookup.
    // The tx_hash_map maps tx_hash -> (block_num, tx_idx).
    // Without a reverse index, we can't efficiently get tx_hash from (block_num, tx_idx).
    // Return None for now - the reward amount is still available.
    None
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
    let store = state.store.clone();

    let block_number: i64 = if id.starts_with("0x") {
        let hash = hex::decode(id.strip_prefix("0x").unwrap_or(&id))
            .map_err(|_| ApiError::bad_request("Invalid block hash"))?;

        let store_c = store.clone();
        tokio::task::spawn_blocking(move || store_c.get_block_number_by_hash(&hash))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| ApiError::not_found("Block not found"))?
    } else {
        id.parse()
            .map_err(|_| ApiError::bad_request("Invalid block number"))?
    };

    let store_c = store.clone();
    let txs = tokio::task::spawn_blocking(move || store_c.list_block_txs(block_number))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if txs.is_empty() {
        // Block might not exist or has no transactions
        // Check if block header exists to differentiate
        let store_c = store.clone();
        let header = tokio::task::spawn_blocking(move || store_c.get_block_header(block_number))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

        if header.is_none() {
            return Err(ApiError::not_found("Block not found"));
        }
    }

    let mut total_size: i64 = 0;
    let mut total_cycles: i64 = 0;
    let mut fee_rates: Vec<f64> = Vec::new();
    let mut non_cellbase_count: i32 = 0;

    for (_tx_idx, entry) in &txs {
        total_size += entry.tx_size as i64;
        total_cycles += entry.cycles.unwrap_or(0);

        if !entry.is_cellbase && entry.tx_size > 0 {
            non_cellbase_count += 1;
            let fee_rate = entry.fee as f64 / entry.tx_size as f64;
            fee_rates.push(fee_rate);
        }
    }

    let avg_fee_rate = if fee_rates.is_empty() {
        0.0
    } else {
        fee_rates.iter().sum::<f64>() / fee_rates.len() as f64
    };
    let min_fee_rate = fee_rates.iter().copied().fold(f64::INFINITY, f64::min);
    let max_fee_rate = fee_rates.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    ok(BlockFeeStatsResponse {
        block_number,
        total_size,
        total_cycles,
        avg_fee_rate,
        min_fee_rate: if fee_rates.is_empty() {
            0.0
        } else {
            min_fee_rate
        },
        max_fee_rate: if fee_rates.is_empty() {
            0.0
        } else {
            max_fee_rate
        },
        transaction_count: non_cellbase_count,
    })
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
    let store = state.store.clone();

    let block_number: i64 = if id.starts_with("0x") {
        let hash = hex::decode(id.strip_prefix("0x").unwrap_or(&id))
            .map_err(|_| ApiError::bad_request("Invalid block hash"))?;

        let store_c = store.clone();
        tokio::task::spawn_blocking(move || store_c.get_block_number_by_hash(&hash))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| ApiError::not_found("Block not found"))?
    } else {
        id.parse()
            .map_err(|_| ApiError::bad_request("Invalid block number"))?
    };

    // Read proposals from CKB node's RocksDB (raw block data)
    let proposals: Vec<BlockProposal> = if let Some(ref ckb_store) = state.ckb_store {
        // Get the block hash from our store
        let store_c = store.clone();
        let header = tokio::task::spawn_blocking(move || store_c.get_block_header(block_number))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

        match header {
            Some(h) if h.hash.len() == 32 => {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&h.hash);
                if let Some(block) = ckb_store.get_block(&hash) {
                    block
                        .data()
                        .proposals()
                        .into_iter()
                        .enumerate()
                        .map(|(i, proposal_id)| {
                            let proposal_bytes: Vec<u8> = proposal_id.raw_data().to_vec();
                            BlockProposal {
                                proposal_index: i as i16,
                                proposal_id: format!("0x{}", hex::encode(&proposal_bytes)),
                                // Committed tx lookup would require scanning tx_hash_map
                                // for matching prefix - omit for now
                                committed_tx_hash: None,
                                committed_block_number: None,
                            }
                        })
                        .collect()
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    } else {
        vec![]
    };

    ok(proposals)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Convert compact target to difficulty string (in human-readable format like "1.49 EH")
    fn compact_target_to_difficulty(compact: u32) -> String {
        let exponent = compact >> 24;
        let mantissa = compact & 0x00ff_ffff;

        if exponent <= 3 {
            let target = mantissa >> (8 * (3 - exponent));
            if target == 0 {
                return "0".to_string();
            }
            format!("{}", u64::MAX / target as u64)
        } else {
            let shift = 8 * (exponent - 3);
            if shift >= 256 {
                return "0".to_string();
            }

            let effective_bits = 256 - shift;
            if effective_bits > 64 {
                let excess_bits = effective_bits - 64;
                let base_difficulty = (1u128 << 64) / mantissa as u128;
                let difficulty = base_difficulty << excess_bits.min(64);

                if difficulty >= 1_000_000_000_000_000_000 {
                    format!("{:.2} EH", difficulty as f64 / 1e18)
                } else if difficulty >= 1_000_000_000_000_000 {
                    format!("{:.2} PH", difficulty as f64 / 1e15)
                } else if difficulty >= 1_000_000_000_000 {
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

    #[test]
    fn test_compact_target_to_difficulty_zero() {
        assert_eq!(compact_target_to_difficulty(0), "0");
    }

    #[test]
    fn test_compact_target_to_difficulty_small_exponent() {
        // exponent=3, mantissa=0x010000 -> target=0x010000 >> 0 = 65536
        let result = compact_target_to_difficulty(0x03010000);
        // difficulty = u64::MAX / 65536
        let expected = (u64::MAX / 65536).to_string();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_compact_target_to_difficulty_large_shift() {
        // Shift >= 256 should return "0"
        let result = compact_target_to_difficulty(0xFF010000);
        assert_eq!(result, "0");
    }

    #[test]
    fn test_block_response_serialization() {
        let response = BlockResponse {
            number: 100,
            hash: "0xabc".to_string(),
            parent_hash: "0xdef".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            transactions_count: 5,
            proposals_count: 2,
            uncles_count: 0,
            epoch: "10/1800".to_string(),
            epoch_number: 50,
            epoch_index: 10,
            epoch_length: 1800,
            difficulty: "1.23 EH".to_string(),
            nonce: "0x0".to_string(),
            transactions_root: "0x".to_string(),
            miner_address: None,
            miner_message: None,
            mining_reward: None,
            mining_reward_tx_hash: None,
            hardfork_activation: None,
            compact_target: "0x0".to_string(),
            version: 0,
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["number"], 100);
        assert_eq!(json["transactionsCount"], 5);
        assert_eq!(json["epochNumber"], 50);
    }

    #[test]
    fn test_list_params_defaults() {
        let params: ListParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.limit, 20);
        assert!(params.cursor.is_none());
    }

    #[test]
    fn test_block_fee_stats_response_serialization() {
        let resp = BlockFeeStatsResponse {
            block_number: 100,
            total_size: 5000,
            total_cycles: 1000000,
            avg_fee_rate: 1.5,
            min_fee_rate: 0.5,
            max_fee_rate: 3.0,
            transaction_count: 10,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["blockNumber"], 100);
        assert_eq!(json["transactionCount"], 10);
    }

    #[test]
    fn test_parse_hex_u128() {
        assert_eq!(parse_hex_u128("0x0"), 0);
        assert_eq!(parse_hex_u128("0xa"), 10);
        assert_eq!(parse_hex_u128("0xff"), 255);
        assert_eq!(parse_hex_u128("ff"), 255);
    }
}
