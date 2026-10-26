#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckb_store_reader::CkbChainReader;
use ckb_types::{prelude::*, utilities::compact_to_difficulty};
use ckbadger_common::hardforks_for_network;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::cache::{CacheBackend, CacheKeys, CacheTtl};
use crate::response::{
    default_limit, ok, ApiError, ApiResult, ApiRouteError, CursorPaginatedResponse,
};
use crate::routes::hardforks::HardforkResourceResponse;
use crate::routes::proposal_window::{block_proposal_zone, resolve_committed_txs};
use crate::routes::transactions::tx_serialized_size_in_block;
use crate::utils::exact_fee::{mean_rate, FeeRate};
use crate::utils::{
    parse_block_number, parse_hash32, parse_optional_block_cursor_start, script_to_address,
};
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
    /// Exclusive upper-bound block number
    /// (see [`parse_optional_block_cursor_start`]). A string, not an `i64`, so
    /// that every rejection comes from the one parser and answers in this
    /// API's error envelope instead of axum's raw deserialization message.
    cursor: Option<String>,
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
    #[serde(flatten)]
    pub metrics: BlockMetricsResponse,
    pub reward_breakdown: Option<RewardBreakdownResponse>,
    pub reward_paid_at_block: Option<i64>,
    pub reward_finalizes_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardBreakdownResponse {
    pub primary: String,
    pub secondary: String,
    pub committed: String,
    pub proposal: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockCyclesStatus {
    Complete,
    Pending,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockMetricsResponse {
    pub max_block_bytes: u64,
    pub max_block_cycles: String,
    pub miner_message_text: Option<String>,
    pub size: usize,
    pub user_transactions_size: usize,
    pub overhead_size: usize,
    pub total_fees: String,
    pub cycles: Option<String>,
    pub cycles_status: BlockCyclesStatus,
    pub transaction_sizes: Vec<usize>,
    pub confirmations: i64,
    pub parent_timestamp: Option<String>,
    pub raw_header: RawBlockHeaderResponse,
    pub uncles: Vec<UncleBlockResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawBlockHeaderResponse {
    pub epoch: String,
    pub dao: String,
    pub proposals_hash: String,
    pub extra_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UncleBlockResponse {
    pub number: u64,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: String,
    pub proposal_ids: Vec<String>,
}

async fn list_blocks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<BlockResponse>> {
    let limit = params.limit.clamp(1, 100);

    // `list_blocks_desc` scans backwards from `from_block` inclusive while the
    // cursor is the exclusive upper bound, so the scan starts one block below
    // it. The parser owns that subtraction because it is the check that keeps
    // the result at or above genesis — `encode_block_num` asserts it.
    let from_block = parse_optional_block_cursor_start(params.cursor.as_deref(), "block cursor")?;

    let cache_key = format!("{}:{}", CacheKeys::LATEST_BLOCKS, limit);
    let is_first_page = from_block.is_none();
    if is_first_page {
        if let Some(cached) = state
            .cache
            .get::<CursorPaginatedResponse<BlockResponse>>(&cache_key)
            .await
        {
            return ok(cached);
        }
    }

    let sync = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(format!("sync status unavailable: {}", e)))?;
    let total = sync.tip_block_number + 1;

    let fetch_limit = (limit + 1) as usize;
    let network = state.ckb_network.clone();

    let store = state.store.clone();
    let stats_store = state.store.clone();
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

    let mut blocks = Vec::with_capacity(rows.len());
    for (block_num, header) in rows {
        blocks.push(
            build_block_response(
                &state,
                block_num,
                header,
                hardfork_activation_by_block.get(&block_num).cloned(),
            )
            .await?,
        );
    }

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
    metrics: BlockMetricsResponse,
    reward_breakdown: Option<RewardBreakdownResponse>,
    reward_paid_at_block: Option<i64>,
    reward_finalizes_at: Option<i64>,
}

fn cached_header_to_block_response(
    block_num: i64,
    header: ckbadger_store::CachedBlockHeader,
    header_info: Option<ckb_store_reader::BlockHeaderInfo>,
    extra: BlockExtra,
) -> Result<BlockResponse, ApiRouteError> {
    let parent_hash = format!("0x{}", hex::encode(&header.parent_hash));
    let info = header_info.ok_or_else(|| {
        ApiError::internal(format!(
            "block {block_num} header evidence missing from node DB; configure ckb_db_path"
        ))
    })?;
    let nonce = format!("{:#x}", info.nonce);
    let transactions_root = format!("0x{}", hex::encode(info.transactions_root));
    let version = i32::try_from(info.version).map_err(|_| {
        ApiError::internal(format!(
            "block {block_num} version {} exceeds i32",
            info.version
        ))
    })?;
    // Difficulty is derived from the header's stored compact_target — the
    // single source of truth for per-block PoW difficulty.
    let compact_target = format!("{:#x}", header.compact_target);
    let difficulty = compact_to_difficulty(header.compact_target).to_string();

    // Format timestamp from millis to RFC3339
    let timestamp = chrono::DateTime::from_timestamp_millis(header.timestamp)
        .ok_or_else(|| {
            ApiError::internal(format!(
                "cached block {block_num} (0x{}): invalid timestamp {} ms",
                hex::encode(&header.hash),
                header.timestamp
            ))
        })?
        .to_rfc3339();

    Ok(BlockResponse {
        number: block_num,
        hash: format!("0x{}", hex::encode(&header.hash)),
        parent_hash,
        timestamp,
        transactions_count: header.transactions_count,
        proposals_count: header.proposals_count,
        uncles_count: header.uncles_count,
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
        metrics: extra.metrics,
        reward_breakdown: extra.reward_breakdown,
        reward_paid_at_block: extra.reward_paid_at_block,
        reward_finalizes_at: extra.reward_finalizes_at,
    })
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

/// Both list and detail read the same block evidence. Nothing is written by
/// this path; the node owns serialized bytes and the indexer owns metadata.
async fn build_block_response(
    state: &Arc<AppState>,
    block_num: i64,
    header: ckbadger_store::CachedBlockHeader,
    hardfork_activation: Option<HardforkActivationResponse>,
) -> Result<BlockResponse, ApiRouteError> {
    let node = state.ckb_store.clone().ok_or_else(|| {
        ApiError::internal(format!(
            "block {block_num} requires the node DB; configure ckb_db_path"
        ))
    })?;
    let hash: [u8; 32] = header.hash.as_slice().try_into().map_err(|_| {
        ApiError::internal(format!(
            "block {block_num}: stored hash has {} bytes, expected 32",
            header.hash.len()
        ))
    })?;
    let store = state.store.clone();
    let network = state.ckb_network.clone();
    let cached = header.clone();
    let consensus = state.consensus().await?;
    let limits = consensus.clone();
    let (info, metrics, miner_address, miner_message) = tokio::task::spawn_blocking(move || {
        read_block_evidence(&node, &store, block_num, &cached, &network, &limits)
    })
    .await
    .map_err(|e| ApiError::internal(format!("block {block_num} evidence task: {e}")))??;
    let reward_finalizes_at = if block_num == 0 {
        None
    } else {
        Some(
            block_num
                .checked_add(consensus.tx_proposal_window.farthest())
                .and_then(|n| n.checked_add(1))
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "block {block_num} reward finalization height overflow"
                    ))
                })?,
        )
    };
    let reward = if block_num == 0 {
        None
    } else {
        get_mining_reward(&state.ckb_rpc_url, &hash, state.store.clone(), &state.cache).await?
    };
    let (mining_reward, mining_reward_tx_hash, reward_breakdown, reward_paid_at_block) =
        match reward {
            Some(reward) => (
                Some(reward.reward),
                Some(reward.cellbase_tx_hash),
                Some(reward.breakdown),
                Some(reward.paid_at_block),
            ),
            None => (None, None, None, None),
        };
    if metrics.cycles_status == BlockCyclesStatus::Pending && state.cycles_client.is_enabled() {
        let client = state.cycles_client.clone();
        tokio::spawn(async move {
            if let Err(error) = client.enqueue_block(block_num).await {
                tracing::warn!(block_num, %error, "could not request pending block cycles");
            }
        });
    }
    cached_header_to_block_response(
        block_num,
        header,
        Some(info),
        BlockExtra {
            miner_address,
            miner_message,
            mining_reward,
            mining_reward_tx_hash,
            hardfork_activation,
            metrics,
            reward_breakdown,
            reward_paid_at_block,
            reward_finalizes_at,
        },
    )
}

fn read_block_evidence(
    node: &CkbChainReader,
    store: &ckbadger_store::CkbadgerStore,
    block_num: i64,
    header: &ckbadger_store::CachedBlockHeader,
    network: &str,
    consensus: &crate::consensus::ConsensusParams,
) -> Result<
    (
        ckb_store_reader::BlockHeaderInfo,
        BlockMetricsResponse,
        Option<String>,
        Option<Vec<u8>>,
    ),
    ApiRouteError,
> {
    let context = format!("block {block_num} 0x{}", hex::encode(&header.hash));
    let error = |detail: String| ApiError::internal(format!("{context}: {detail}"));
    let hash: [u8; 32] = header
        .hash
        .as_slice()
        .try_into()
        .map_err(|_| error("invalid stored hash".into()))?;
    let block = node
        .get_block(&hash)
        .ok_or_else(|| error("indexed block is unavailable or malformed in the node DB".into()))?;
    if block.number() != block_num as u64
        || block.transactions().len() != header.transactions_count as usize
    {
        return Err(error(format!(
            "node block metadata mismatch: number={}, transactions={}, indexed transactions={}",
            block.number(),
            block.transactions().len(),
            header.transactions_count
        )));
    }
    if block.data().proposals().len() != header.proposals_count as usize {
        return Err(error(format!(
            "proposal count mismatch: indexed={}, node={}",
            header.proposals_count,
            block.data().proposals().len()
        )));
    }
    if block.data().uncles().len() != header.uncles_count as usize {
        return Err(error(format!(
            "uncle count mismatch: indexed={}, node={}",
            header.uncles_count,
            block.data().uncles().len()
        )));
    }
    let info = ckb_store_reader::BlockHeaderInfo {
        parent_hash: block.parent_hash().unpack(),
        nonce: block.header().nonce(),
        transactions_root: block.header().transactions_root().unpack(),
        version: block.header().version(),
    };
    let (miner_address, miner_message) = if block_num == 0 {
        (None, None)
    } else {
        let (lock, message) = node
            .get_cellbase_witness(&hash)
            .ok_or_else(|| error("missing or malformed cellbase witness".into()))?;
        let address = script_to_address(
            lock.code_hash().as_slice(),
            i16::from(lock.hash_type().as_slice()[0]),
            &lock.args().raw_data(),
            network,
        )
        .map_err(|e| error(format!("miner witness lock: {e}")))?;
        (Some(address), Some(message))
    };
    let txs = store
        .list_block_txs(block_num)
        .map_err(|e| error(e.to_string()))?;
    if txs.len() != block.transactions().len() {
        return Err(error(format!(
            "transaction index has {} rows, node has {}",
            txs.len(),
            block.transactions().len()
        )));
    }
    let mut transaction_sizes = Vec::with_capacity(txs.len());
    let mut user_transactions_size = 0usize;
    let mut total_fees = 0u128;
    for (position, ((index, entry), tx)) in txs.iter().zip(block.transactions().iter()).enumerate()
    {
        if *index != position as i32 || entry.tx_hash.as_slice() != tx.hash().as_slice() {
            return Err(error(format!(
                "transaction index/hash mismatch at index {position}"
            )));
        }
        if entry.is_cellbase != (position == 0) {
            return Err(error(format!(
                "transaction {index} has inconsistent cellbase marker {}",
                entry.is_cellbase
            )));
        }
        let size = tx.data().serialized_size_in_block();
        if entry.tx_size <= 0 || tx_serialized_size_in_block(entry.tx_size) as usize != size {
            return Err(error(format!(
                "transaction {index} byte size mismatch: indexed={}, node={size}",
                entry.tx_size
            )));
        }
        transaction_sizes.push(size);
        if !entry.is_cellbase {
            user_transactions_size = user_transactions_size
                .checked_add(size)
                .ok_or_else(|| error("transaction size sum overflow".into()))?;
            let fee = u128::try_from(entry.fee).map_err(|_| {
                error(format!(
                    "transaction {index} has negative fee {}",
                    entry.fee
                ))
            })?;
            total_fees = total_fees
                .checked_add(fee)
                .ok_or_else(|| error("fee sum overflow".into()))?;
        }
    }
    let tip = store
        .get_sync_tip_block()
        .map_err(|e| error(e.to_string()))?
        .ok_or_else(|| error("indexed block exists without a canonical tip".into()))?
        .0;
    let confirmations = tip
        .checked_sub(block_num)
        .and_then(|n| n.checked_add(1))
        .filter(|n| *n > 0)
        .ok_or_else(|| error(format!("invalid canonical tip {tip}")))?;
    let timestamp = |ms: i64| {
        chrono::DateTime::from_timestamp_millis(ms)
            .map(|date| date.to_rfc3339())
            .ok_or_else(|| error(format!("invalid timestamp {ms}")))
    };
    let parent_timestamp = if block_num == 0 {
        None
    } else {
        let parent = store
            .get_block_header(block_num - 1)
            .map_err(|e| error(e.to_string()))?
            .ok_or_else(|| error(format!("missing canonical parent at {}", block_num - 1)))?;
        Some(timestamp(parent.timestamp)?)
    };
    let miner_message_text = miner_message
        .as_ref()
        .and_then(|message| std::str::from_utf8(message).ok())
        .map(str::to_owned);
    let raw_header = RawBlockHeaderResponse {
        epoch: format!("{:#x}", block.epoch().full_value()),
        dao: format!("0x{}", hex::encode(block.header().dao().as_slice())),
        proposals_hash: format!(
            "0x{}",
            hex::encode(block.header().proposals_hash().as_slice())
        ),
        extra_hash: format!("0x{}", hex::encode(block.header().extra_hash().as_slice())),
    };
    let uncles = block
        .uncles()
        .into_iter()
        .map(|uncle| {
            let ms = i64::try_from(uncle.header().timestamp())
                .map_err(|_| error("uncle timestamp exceeds i64".into()))?;
            Ok(UncleBlockResponse {
                number: uncle.number(),
                hash: format!("0x{}", hex::encode(uncle.hash().as_slice())),
                parent_hash: format!("0x{}", hex::encode(uncle.header().parent_hash().as_slice())),
                timestamp: timestamp(ms)?,
                proposal_ids: uncle
                    .data()
                    .proposals()
                    .into_iter()
                    .map(|id| format!("0x{}", hex::encode(id.as_slice())))
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?;
    let size = block.data().as_slice().len();
    let overhead_size = size
        .checked_sub(user_transactions_size)
        .ok_or_else(|| error("user transaction bytes exceed block size".into()))?;
    let (cycles, cycles_status) = resolve_block_cycles(block_num, header.cycles, &txs)?;
    Ok((
        info,
        BlockMetricsResponse {
            max_block_bytes: consensus.max_block_bytes,
            max_block_cycles: consensus.max_block_cycles.to_string(),
            miner_message_text,
            size,
            user_transactions_size,
            overhead_size,
            total_fees: total_fees.to_string(),
            cycles: cycles.map(|n| n.to_string()),
            cycles_status,
            transaction_sizes,
            confirmations,
            parent_timestamp,
            raw_header,
            uncles,
        },
        miner_address,
        miner_message,
    ))
}

/// The single cycle aggregation path for list, detail and fee statistics.
fn resolve_block_cycles(
    block_num: i64,
    header_cycles: Option<i64>,
    txs: &[(i32, ckbadger_store::TxIndexEntry)],
) -> Result<(Option<i64>, BlockCyclesStatus), ApiRouteError> {
    // Consensus accepts genesis without running its scripts, including the
    // system-script transaction after cellbase. A stored nonzero result is an
    // invariant violation, not evidence of a historical execution.
    if block_num == 0 {
        if header_cycles.is_some_and(|cycles| cycles != 0) {
            return Err(ApiError::internal(format!(
                "genesis block 0 has aggregate cycles {header_cycles:?}; expected no script execution"
            )));
        }
        for (index, entry) in txs {
            if entry.cycles.is_some_and(|cycles| cycles != 0) {
                return Err(ApiError::internal(format!(
                    "genesis block 0 tx {index} 0x{} has cycles {:?}; expected no script execution",
                    hex::encode(entry.tx_hash),
                    entry.cycles
                )));
            }
        }
        return Ok((Some(0), BlockCyclesStatus::Complete));
    }
    if let Some(cycles) = header_cycles {
        if cycles < 0 {
            return Err(ApiError::internal(format!(
                "block {block_num} has negative aggregate cycles {cycles}"
            )));
        }
        return Ok((Some(cycles), BlockCyclesStatus::Complete));
    }
    let mut sum = 0i64;
    let mut status = BlockCyclesStatus::Complete;
    for (index, entry) in txs.iter().filter(|(_, entry)| !entry.is_cellbase) {
        match entry.cycles {
            Some(-1) => status = BlockCyclesStatus::Failed,
            None | Some(0) => {
                if status != BlockCyclesStatus::Failed {
                    status = BlockCyclesStatus::Pending;
                }
            }
            Some(cycles) if cycles > 0 => {
                sum = sum.checked_add(cycles).ok_or_else(|| {
                    ApiError::internal(format!(
                        "block {block_num} cycles sum overflow at tx {index}"
                    ))
                })?
            }
            Some(cycles) => {
                return Err(ApiError::internal(format!(
                    "block {block_num} tx {index} invalid cycles marker {cycles}"
                )))
            }
        }
    }
    Ok((
        if status == BlockCyclesStatus::Complete {
            Some(sum)
        } else {
            None
        },
        status,
    ))
}

async fn get_block(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<BlockResponse> {
    let store = state.store.clone();
    let block_result: Option<(i64, ckbadger_store::CachedBlockHeader)> = if id.starts_with("0x") {
        let hash = parse_hash32(&id, "block hash")?;

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
        let number = parse_block_number(&id, "block number")?;

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

            ok(build_block_response(&state, block_num, header, activation).await?)
        }
        None => Err(ApiError::not_found("Block not found")),
    }
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcErrorBody>,
}

#[derive(Debug, Deserialize)]
struct RpcErrorBody {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct RawBlockEconomicState {
    miner_reward: RawMinerReward,
    finalized_at: String,
}

#[derive(Debug, Deserialize)]
struct RawMinerReward {
    primary: String,
    secondary: String,
    committed: String,
    proposal: String,
}

/// A finalized block's economics, as the node's `get_block_economic_state`
/// reports them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockEconomicState {
    /// The block that finalized this block's reward — its cellbase pays it.
    pub finalized_at: [u8; 32],
    /// The reward's four parts (`miner_reward`), shannons.
    pub miner_reward: MinerReward,
    /// `primary + secondary + committed + proposal`, shannons: the capacity of
    /// the reward cell the finalizing block's cellbase creates.
    pub miner_reward_total: u64,
}

/// The reward of a finalized block's miner, as ckb's `RewardCalculator`
/// splits it (`core::MinerReward`), shannons. Each part is a CKB `Capacity`
/// (u64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MinerReward {
    /// Primary issuance: the block's whole primary block reward.
    pub primary: u64,
    /// The miner's share of the block's secondary issuance.
    pub secondary: u64,
    /// What the block's committed transactions paid, less the proposers'
    /// share (`BlockReward::tx_fee`).
    pub committed: u64,
    /// The proposers' share of the fees of the transactions this block (or
    /// an uncle it embeds) proposed first, committed within their window.
    pub proposal: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct MiningRewardInfo {
    reward: String,
    cellbase_tx_hash: String,
    breakdown: RewardBreakdownResponse,
    paid_at_block: i64,
}

fn parse_hex_u128(hex: &str, field_name: &str) -> Result<u128, String> {
    let stripped = hex.strip_prefix("0x").unwrap_or(hex);
    u128::from_str_radix(stripped, 16).map_err(|e| {
        format!(
            "failed to parse hex field '{}': value='{}', error={}",
            field_name, hex, e
        )
    })
}

/// The node's `get_block_economic_state(block_hash)` — the one fetch both the
/// block detail's mining reward and the pipeline's cellbase targets use.
///
/// `Ok(None)` means exactly one thing: the node answered `null`, i.e. the block
/// is not finalized yet. A transport failure, an RPC error and a malformed
/// field are errors naming the block.
pub(crate) async fn fetch_block_economic_state(
    rpc_url: &str,
    block_hash: &[u8; 32],
) -> Result<Option<BlockEconomicState>, ApiRouteError> {
    let hash_hex = format!("0x{}", hex::encode(block_hash));
    let call = format!("get_block_economic_state({hash_hex})");
    let response: RpcResponse<RawBlockEconomicState> = crate::utils::shared_http_client()
        .post(rpc_url)
        .json(&serde_json::json!({
            "id": 1,
            "jsonrpc": "2.0",
            "method": "get_block_economic_state",
            "params": [hash_hex]
        }))
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("{call} request failed: {e}")))?
        .json()
        .await
        .map_err(|e| ApiError::internal(format!("{call} response undecodable: {e}")))?;

    if let Some(error) = response.error {
        return Err(ApiError::internal(format!(
            "{call} RPC error {}: {}",
            error.code, error.message
        )));
    }
    let Some(raw) = response.result else {
        return Ok(None);
    };

    let finalized_at = crate::pool::source::parse_hex_hash32(
        &raw.finalized_at,
        "get_block_economic_state.finalized_at",
    )
    .map_err(|e| ApiError::internal(format!("{call}: {e}")))?;
    let reward = |value: &str, field: &str| -> Result<u64, ApiRouteError> {
        let shannons =
            parse_hex_u128(value, field).map_err(|e| ApiError::internal(format!("{call}: {e}")))?;
        u64::try_from(shannons).map_err(|_| {
            ApiError::internal(format!(
                "{call}: {field} {value} exceeds a CKB capacity (u64 shannons)"
            ))
        })
    };
    let miner_reward = MinerReward {
        primary: reward(&raw.miner_reward.primary, "miner_reward.primary")?,
        secondary: reward(&raw.miner_reward.secondary, "miner_reward.secondary")?,
        committed: reward(&raw.miner_reward.committed, "miner_reward.committed")?,
        proposal: reward(&raw.miner_reward.proposal, "miner_reward.proposal")?,
    };
    let miner_reward_total = miner_reward
        .primary
        .checked_add(miner_reward.secondary)
        .and_then(|sum| sum.checked_add(miner_reward.committed))
        .and_then(|sum| sum.checked_add(miner_reward.proposal))
        .ok_or_else(|| {
            ApiError::internal(format!(
                "{call}: miner_reward {miner_reward:?} sums past a CKB capacity (u64 shannons)"
            ))
        })?;

    Ok(Some(BlockEconomicState {
        finalized_at,
        miner_reward,
        miner_reward_total,
    }))
}

/// The block's total miner reward and the cellbase that pays it. `Ok(None)`
/// until the node finalizes the block and its paying cellbase is indexed.
async fn get_mining_reward(
    rpc_url: &str,
    block_hash: &[u8; 32],
    store: Arc<ckbadger_store::CkbadgerStore>,
    cache: &CacheBackend,
) -> Result<Option<MiningRewardInfo>, ApiRouteError> {
    let cache_key = CacheKeys::mining_reward(&format!("0x{}", hex::encode(block_hash)));
    if let Some(cached) = cache.get::<MiningRewardInfo>(&cache_key).await {
        return Ok(Some(cached));
    }

    let Some(economic_state) = fetch_block_economic_state(rpc_url, block_hash).await? else {
        return Ok(None);
    };
    let finalized_at = economic_state.finalized_at;
    let paid_at_block = store
        .get_block_number_by_hash(&finalized_at)
        .map_err(|e| ApiError::internal(format!("reward paying block: {e}")))?;
    let cellbase_tx_hash =
        tokio::task::spawn_blocking(move || get_cellbase_tx_hash(&store, &finalized_at))
            .await
            .map_err(|error| {
                ApiError::internal(format!("reward cellbase lookup task: {error}"))
            })??;
    let Some(cellbase_tx_hash) = cellbase_tx_hash else {
        return Ok(None);
    };
    let info = MiningRewardInfo {
        reward: economic_state.miner_reward_total.to_string(),
        cellbase_tx_hash,
        breakdown: RewardBreakdownResponse {
            primary: economic_state.miner_reward.primary.to_string(),
            secondary: economic_state.miner_reward.secondary.to_string(),
            committed: economic_state.miner_reward.committed.to_string(),
            proposal: economic_state.miner_reward.proposal.to_string(),
        },
        paid_at_block: paid_at_block.ok_or_else(|| {
            ApiError::internal("reward cellbase resolved without its canonical block number")
        })?,
    };

    cache.set(&cache_key, &info, CacheTtl::MINING_REWARD).await;

    Ok(Some(info))
}

/// Resolve the paying transaction through the canonical domain index. The node
/// reader is optional and can lag independently; it does not own this lookup.
fn get_cellbase_tx_hash(
    store: &ckbadger_store::CkbadgerStore,
    finalized_at: &[u8; 32],
) -> Result<Option<String>, ApiRouteError> {
    let context = format!("reward finalizing block 0x{}", hex::encode(finalized_at));
    let read_error = |error| ApiError::internal(format!("{context}: {error}"));
    let Some(number) = store
        .get_block_number_by_hash(finalized_at)
        .map_err(read_error)?
    else {
        return Ok(None);
    };
    let header = store
        .get_block_header(number)
        .map_err(read_error)?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "{context}: hash index points to missing header {number}"
            ))
        })?;
    if header.hash != finalized_at {
        return Err(ApiError::internal(format!(
            "{context}: canonical header mismatch at {number}"
        )));
    }
    let rows = store
        .list_block_txs_after(number, -1, 1)
        .map_err(read_error)?;
    let (index, cellbase) = rows.first().ok_or_else(|| {
        ApiError::internal(format!("{context}: indexed block {number} has no cellbase"))
    })?;
    if *index != 0 || !cellbase.is_cellbase {
        return Err(ApiError::internal(format!(
            "{context}: transaction {index} in block {number} is not cellbase index 0"
        )));
    }
    Ok(Some(format!("0x{}", hex::encode(cellbase.tx_hash))))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockFeeStatsResponse {
    pub block_number: i64,
    pub total_size: i64,
    pub total_cycles: Option<i64>,
    /// Shannon per 1000 bytes, rounded half-up to two decimals; null without fee samples.
    pub avg_fee_rate: Option<String>,
    pub min_fee_rate: Option<String>,
    pub max_fee_rate: Option<String>,
    pub transaction_count: i32,
    #[serde(default)]
    pub cycles_pending: bool,
    pub cycles_status: BlockCyclesStatus,
}

fn indexed_block_fee_rate(
    entry: &ckbadger_store::TxIndexEntry,
    block_number: i64,
    tx_index: i32,
) -> Result<FeeRate, ApiRouteError> {
    let fee = u64::try_from(entry.fee).map_err(|_| {
        ApiError::internal(format!(
            "negative transaction fee in block={block_number} tx_index={tx_index}: {}",
            entry.fee
        ))
    })?;
    if entry.tx_size <= 0 {
        return Err(ApiError::internal(format!(
            "nonpositive transaction size in block={block_number} tx_index={tx_index}: {}",
            entry.tx_size
        )));
    }
    let size = u64::try_from(tx_serialized_size_in_block(entry.tx_size)).map_err(|error| {
        ApiError::internal(format!(
            "invalid transaction size in block={block_number} tx_index={tx_index}: {error}"
        ))
    })?;
    FeeRate::new(fee, size).map_err(|error| {
        ApiError::internal(format!("block={block_number} tx_index={tx_index}: {error}"))
    })
}

async fn get_block_fee_stats(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<BlockFeeStatsResponse> {
    let store = state.store.clone();

    let block_number: i64 = if id.starts_with("0x") {
        let hash = parse_hash32(&id, "block hash")?;

        let store_c = store.clone();
        tokio::task::spawn_blocking(move || store_c.get_block_number_by_hash(&hash))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| ApiError::not_found("Block not found"))?
    } else {
        parse_block_number(&id, "block number")?
    };

    // Try to read pre-computed cycles from block header first
    let store_c = store.clone();
    let bn = block_number;
    let header_cycles = tokio::task::spawn_blocking(move || {
        store_c
            .get_block_header(bn)
            .map(|h| h.and_then(|h| h.cycles))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

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
    let mut fee_rates: Vec<FeeRate> = Vec::new();
    let mut non_cellbase_count: i32 = 0;

    for (tx_idx, entry) in &txs {
        if entry.is_cellbase {
            continue;
        }
        let fee_rate = indexed_block_fee_rate(entry, block_number, *tx_idx)?;
        non_cellbase_count += 1;
        total_size = total_size
            .checked_add(fee_rate.size() as i64)
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "block {block_number} total transaction size overflows i64"
                ))
            })?;
        fee_rates.push(fee_rate);
    }
    let (total_cycles, cycles_status) = resolve_block_cycles(block_number, header_cycles, &txs)?;
    let cycles_pending = cycles_status == BlockCyclesStatus::Pending;

    // Fire-and-forget block cycles request if pending
    if cycles_pending && state.cycles_client.is_enabled() {
        let client = state.cycles_client.clone();
        let bn = block_number;
        tokio::spawn(async move {
            if let Err(e) = client.enqueue_block(bn).await {
                tracing::debug!("Failed to enqueue block cycles request: {}", e);
            }
        });
    }

    let avg_fee_rate = mean_rate(&fee_rates, 1000);
    let min_fee_rate = fee_rates.iter().min().map(|rate| rate.format(1000));
    let max_fee_rate = fee_rates.iter().max().map(|rate| rate.format(1000));

    ok(BlockFeeStatsResponse {
        block_number,
        total_size,
        total_cycles,
        avg_fee_rate,
        min_fee_rate,
        max_fee_rate,
        transaction_count: non_cellbase_count,
        cycles_pending,
        cycles_status,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockProposal {
    /// Position in the block's proposal zone (own proposals first, then the
    /// embedded uncles').
    pub proposal_index: i16,
    pub proposal_id: String,
    /// The embedded uncle that carried this proposal; `None` for one of the
    /// block's own proposals.
    pub uncle: Option<ProposalUncle>,
    pub committed_tx_hash: Option<String>,
    pub committed_block_number: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalUncle {
    /// The uncle's own header number (not the containing block's).
    pub number: i64,
    pub hash: String,
}

/// The block's whole proposal zone — its own proposals plus those of the
/// uncles it embeds ([`block_proposal_zone`]) — each resolved to the
/// transaction that committed it through the shared commitment-window helper
/// (the same path `/graph/proposals` uses). A proposal not committed within
/// the window blocks that exist stays null: honest data near the tip.
///
/// The zone lives only in the node DB. Without the reader, or when the node DB
/// does not hold the block the store indexed, the proposals are unknown, not
/// empty: both are errors naming the cause.
async fn get_block_proposals(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Vec<BlockProposal>> {
    let store = state.store.clone();

    let block_number: i64 = if id.starts_with("0x") {
        let hash = parse_hash32(&id, "block hash")?;

        let store_c = store.clone();
        tokio::task::spawn_blocking(move || store_c.get_block_number_by_hash(&hash))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| ApiError::not_found("Block not found"))?
    } else {
        parse_block_number(&id, "block number")?
    };

    let store_c = store.clone();
    let header = tokio::task::spawn_blocking(move || store_c.get_block_header(block_number))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Block not found"))?;

    let ckb_store = state.ckb_store.as_ref().ok_or_else(|| {
        ApiError::internal(
            "block proposals are read from the node DB; configure ckb_db_path so the API \
             can open it",
        )
    })?;
    let hash: [u8; 32] = header.hash.as_slice().try_into().map_err(|_| {
        ApiError::internal(format!(
            "block {block_number} hash in the store is {} bytes, not 32",
            header.hash.len()
        ))
    })?;
    let block = ckb_store.get_block(&hash).ok_or_else(|| {
        ApiError::internal(format!(
            "block {block_number} 0x{} is indexed but not in the node DB",
            hex::encode(hash)
        ))
    })?;

    let window = state.consensus().await?.tx_proposal_window;
    let zone = block_proposal_zone(&block);
    let short_ids: Vec<Vec<u8>> = zone.iter().map(|p| p.short_id.clone()).collect();
    let commitments = resolve_committed_txs(ckb_store, window, block_number, &short_ids);

    let proposals = zone
        .into_iter()
        .zip(commitments)
        .enumerate()
        .map(|(i, (proposal, commitment))| {
            let proposal_index = i16::try_from(i).map_err(|_| {
                ApiError::internal(format!(
                    "block {block_number} proposal zone index {i} exceeds i16"
                ))
            })?;
            Ok(BlockProposal {
                proposal_index,
                proposal_id: format!("0x{}", hex::encode(&proposal.short_id)),
                uncle: proposal.uncle.map(|uncle| ProposalUncle {
                    number: uncle.number,
                    hash: format!("0x{}", hex::encode(uncle.hash)),
                }),
                committed_tx_hash: commitment
                    .as_ref()
                    .map(|(tx_hash, _)| format!("0x{}", hex::encode(tx_hash))),
                committed_block_number: commitment.map(|(_, number)| number),
            })
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?;

    ok(proposals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_has_no_script_execution_even_for_non_cellbase_transactions() {
        let tx = ckbadger_store::TxIndexEntry {
            tx_hash: [0x11; 32],
            is_cellbase: false,
            timestamp: 0,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        };
        for cycles in [None, Some(0)] {
            let mut entry = tx.clone();
            entry.cycles = cycles;
            assert_eq!(
                resolve_block_cycles(0, None, &[(1, entry)]).unwrap(),
                (Some(0), BlockCyclesStatus::Complete)
            );
        }
        for cycles in [-1, 1] {
            let mut entry = tx.clone();
            entry.cycles = Some(cycles);
            let error = resolve_block_cycles(0, None, &[(1, entry)])
                .unwrap_err()
                .1
                 .0
                .message;
            assert!(error.contains("genesis"), "{error}");
            assert!(error.contains("tx 1"), "{error}");
        }
        let error = resolve_block_cycles(0, Some(1), &[(1, tx)])
            .unwrap_err()
            .1
             .0
            .message;
        assert!(error.contains("genesis"), "{error}");
    }

    fn test_metrics() -> BlockMetricsResponse {
        BlockMetricsResponse {
            max_block_bytes: 597_000,
            max_block_cycles: "3500000000".into(),
            miner_message_text: None,
            size: 640,
            user_transactions_size: 0,
            overhead_size: 640,
            total_fees: "0".into(),
            cycles: Some("0".into()),
            cycles_status: BlockCyclesStatus::Complete,
            transaction_sizes: vec![100],
            confirmations: 1,
            parent_timestamp: None,
            raw_header: RawBlockHeaderResponse {
                epoch: "0x0".into(),
                dao: format!("0x{}", "00".repeat(32)),
                proposals_hash: format!("0x{}", "00".repeat(32)),
                extra_hash: format!("0x{}", "00".repeat(32)),
            },
            uncles: vec![],
        }
    }
    fn test_header_info() -> ckb_store_reader::BlockHeaderInfo {
        ckb_store_reader::BlockHeaderInfo {
            parent_hash: [0x11; 32],
            nonce: 0,
            transactions_root: [0x33; 32],
            version: 0,
        }
    }

    /// Regression: block responses must expose the header's real compact_target,
    /// exact difficulty, and proposals/uncles counts (previously hardcoded to 0).
    /// Vector: mainnet block 12,000,000 — compact_target 0x190df964, explorer
    /// difficulty 1320058941807520729.
    #[test]
    fn test_block_response_uses_real_header_fields() {
        let header = ckbadger_store::CachedBlockHeader {
            hash: vec![0x22; 32],
            parent_hash: vec![0x11; 32],
            timestamp: 1_705_733_626_764,
            epoch_number: 9128,
            epoch_index: 909,
            epoch_length: 1691,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 2,
            proposals_count: 3,
            compact_target: 0x190d_f964,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_705_733_626_764,
        };
        let resp = cached_header_to_block_response(
            12_000_000,
            header,
            Some(test_header_info()),
            BlockExtra {
                miner_address: None,
                miner_message: None,
                mining_reward: None,
                mining_reward_tx_hash: None,
                hardfork_activation: None,
                metrics: test_metrics(),
                reward_breakdown: None,
                reward_paid_at_block: None,
                reward_finalizes_at: Some(11),
            },
        )
        .unwrap();
        assert_eq!(resp.compact_target, "0x190df964");
        assert_eq!(resp.difficulty, "1320058941807520729");
        assert_eq!(resp.proposals_count, 3);
        assert_eq!(resp.uncles_count, 2);
    }

    /// Regression: nonce is a numeric JSON-RPC quantity. Encoding the u128's
    /// little-endian memory bytes reverses the displayed value.
    #[test]
    fn test_block_response_formats_nonce_as_numeric_hex() {
        let header = ckbadger_store::CachedBlockHeader {
            hash: vec![0x22; 32],
            parent_hash: vec![0x11; 32],
            timestamp: 1_705_733_626_764,
            epoch_number: 9128,
            epoch_index: 909,
            epoch_length: 1691,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x190d_f964,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1_705_733_626_764,
        };
        let header_info = ckb_store_reader::BlockHeaderInfo {
            parent_hash: [0x11; 32],
            nonce: 0x8a4c_8eb5_1d72_599e_0000_0001_2028_0402_u128,
            transactions_root: [0x33; 32],
            version: 0,
        };

        let response = cached_header_to_block_response(
            20_008_000,
            header,
            Some(header_info),
            BlockExtra {
                miner_address: None,
                miner_message: None,
                mining_reward: None,
                mining_reward_tx_hash: None,
                hardfork_activation: None,
                metrics: test_metrics(),
                reward_breakdown: None,
                reward_paid_at_block: None,
                reward_finalizes_at: Some(11),
            },
        )
        .unwrap();

        assert_eq!(response.nonce, "0x8a4c8eb51d72599e0000000120280402");
    }

    #[test]
    fn invalid_cached_block_timestamp_is_an_error() {
        let header = ckbadger_store::CachedBlockHeader {
            hash: vec![0x22; 32],
            parent_hash: vec![0x11; 32],
            timestamp: i64::MAX,
            epoch_number: 1,
            epoch_index: 0,
            epoch_length: 100,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0x190d_f964,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: i64::MAX,
        };
        let error = cached_header_to_block_response(
            42,
            header,
            Some(test_header_info()),
            BlockExtra {
                miner_address: None,
                miner_message: None,
                mining_reward: None,
                mining_reward_tx_hash: None,
                hardfork_activation: None,
                metrics: test_metrics(),
                reward_breakdown: None,
                reward_paid_at_block: None,
                reward_finalizes_at: Some(11),
            },
        )
        .unwrap_err();
        assert!(error.1 .0.message.contains("cached block 42"));
        assert!(error.1 .0.message.contains("invalid timestamp"));
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
            metrics: test_metrics(),
            reward_breakdown: None,
            reward_paid_at_block: None,
            reward_finalizes_at: Some(111),
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["number"], 100);
        assert_eq!(json["transactionsCount"], 5);
        assert_eq!(json["epochNumber"], 50);
    }

    #[test]
    fn test_list_params_defaults() {
        let params: ListParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.limit, 50);
        assert!(params.cursor.is_none());
    }

    #[test]
    fn test_block_fee_stats_response_serialization() {
        let resp = BlockFeeStatsResponse {
            block_number: 100,
            total_size: 5000,
            total_cycles: Some(1000000),
            avg_fee_rate: Some("1.50".to_string()),
            min_fee_rate: Some("0.50".to_string()),
            max_fee_rate: Some("3.00".to_string()),
            transaction_count: 10,
            cycles_pending: false,
            cycles_status: BlockCyclesStatus::Complete,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["blockNumber"], 100);
        assert_eq!(json["transactionCount"], 10);
        assert_eq!(json["cyclesPending"], false);
    }

    #[test]
    fn test_block_fee_stats_cycles_pending_serialization() {
        let resp = BlockFeeStatsResponse {
            block_number: 200,
            total_size: 3000,
            total_cycles: None,
            avg_fee_rate: None,
            min_fee_rate: None,
            max_fee_rate: None,
            transaction_count: 5,
            cycles_pending: true,
            cycles_status: BlockCyclesStatus::Pending,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["cyclesPending"], true);
        assert!(json["avgFeeRate"].is_null());
        assert!(json["minFeeRate"].is_null());
        assert!(json["maxFeeRate"].is_null());

        // Deserialize without cyclesPending field should default to false
        let without_pending = r#"{"blockNumber":300,"totalSize":100,"totalCycles":50,"avgFeeRate":"1.00","minFeeRate":"0.50","maxFeeRate":"1.50","transactionCount":2,"cyclesStatus":"complete"}"#;
        let deserialized: BlockFeeStatsResponse = serde_json::from_str(without_pending).unwrap();
        assert!(!deserialized.cycles_pending);
    }

    #[test]
    fn test_parse_hex_u128() {
        assert_eq!(parse_hex_u128("0x0", "test").unwrap(), 0);
        assert_eq!(parse_hex_u128("0xa", "test").unwrap(), 10);
        assert_eq!(parse_hex_u128("0xff", "test").unwrap(), 255);
        assert_eq!(parse_hex_u128("ff", "test").unwrap(), 255);
    }

    #[test]
    fn test_parse_hex_u128_invalid() {
        let result = parse_hex_u128("0xZZZZ", "bad_field");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("bad_field"),
            "error should name the field: {}",
            err
        );
        assert!(
            err.contains("0xZZZZ"),
            "error should include the value: {}",
            err
        );
    }
}
