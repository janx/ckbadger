use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Extension, Router,
};
use ckb_store_reader::RpcTransactionView;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::Instant;

use crate::consensus::ProposalWindow;
use crate::cycles::{cycles_replay_paused_by_deep_fork, CyclesStatus, CyclesStatusResponse};
use crate::response::{
    default_limit, encode_cursor, hash_type_to_str, ok, ApiError, ApiResult, ApiRouteError,
    CursorPaginatedResponse, ScriptResponse,
};
use crate::routes::identities::{dotcell_record_response, DotCellRecordResponse};
use crate::routes::proposal_window::{
    block_proposal_zone, cellbase_reward_target, earliest_proposal_in_window, ZoneProposal,
};
use crate::routes::tx_lookup::{fetch_transaction_lookup, pending_transaction_resource_error};
use crate::utils::{
    parse_hash32, parse_optional_block_tx_cursor, script_to_address, validate_block_number,
};
use crate::{AppState, RequestReadView, RequestWait};
use tracing::instrument;

/// (block_number, tx_hash, tx_index, tx_index_entry, block_hash)
type TxListEntry = (i64, Vec<u8>, i32, ckbadger_store::TxIndexEntry);
type TxIoBundle = (
    Vec<TransactionInputResponse>,
    Vec<TransactionOutputResponse>,
    u128,
    u128,
    u128,
    u128,
    Vec<String>,
    bool,
    Vec<DotCellNameWitnessResponse>,
);

#[derive(Debug, Clone)]
struct PendingTxIoBundle {
    inputs: Vec<TransactionInputResponse>,
    outputs: Vec<TransactionOutputResponse>,
    inputs_capacity: u128,
    outputs_capacity: u128,
    inputs_used_capacity: u128,
    outputs_used_capacity: u128,
    fee: u128,
    witnesses: Vec<String>,
    witnesses_available: bool,
    dotcell_names: Vec<DotCellNameWitnessResponse>,
}

/// A transaction's size as CKB counts it: the molecule size plus the 4-byte
/// offset slot the transaction occupies in the block's transactions table.
///
/// This is the size the node's tx-pool, wallets, and the official explorer all
/// report, and it is the fee-rate denominator. The API serves it as `txSize` /
/// `size` so that the size and the fee rate in one response are reproducible
/// from each other — reporting the bare molecule size while dividing by this
/// one made `fee / txSize` disagree with the served `feeRate`.
///
/// The store keeps molecule sizes; conversion happens once, here, at the
/// response boundary.
pub(crate) fn tx_serialized_size_in_block(molecule_size: i32) -> i32 {
    molecule_size
        .checked_add(4)
        .unwrap_or_else(|| panic!("transaction molecule size overflows i32: {molecule_size}"))
}

/// Fee rate in shannons per 1000 bytes, floored, over the same size the API
/// serves as `txSize`. Single definition for every fee rate the API returns
/// (transaction detail, pending detail, block fee-stats).
pub(crate) fn tx_fee_rate(fee: u128, size_in_block: i32) -> String {
    tx_fee_rate_value(fee, size_in_block).to_string()
}

/// [`tx_fee_rate`] as a number, for callers that order by it.
pub(crate) fn tx_fee_rate_value(fee: u128, size_in_block: i32) -> u128 {
    let denominator = u128::try_from(size_in_block).unwrap_or_else(|_| {
        panic!("fee-rate call sites guard size_in_block > 0, got {size_in_block}")
    });
    fee * 1000 / denominator
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/transactions", get(list_transactions))
        .route(
            "/transactions/pool",
            get(super::pool_transactions::list_pool_transactions),
        )
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
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    block_number: Option<i64>,
    cursor: Option<String>,
    activity_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionResponse {
    /// Sum of positive participant CKB deltas; cellbase excluded.
    pub value: String,
    pub output_capacity: String,
    pub activity: Option<super::activities::GlobalActivityResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal_origin: Option<ProposalOrigin>,
    pub hash: String,
    pub block_number: i64,
    pub block_hash: String,
    pub index: i32,
    pub inputs_count: i32,
    pub outputs_count: i32,
    pub fee: String,
    pub tx_size: Option<i32>,
    pub cycles: Option<i64>,
    pub cycles_status: String,
    pub is_cellbase: bool,
    pub timestamp: String,
    /// For a cellbase, the block whose reward it pays (TX-23 / TX-25, the same
    /// rule as the block page's `cellbasePays`); `null` for every other
    /// transaction and for cellbases too close to genesis to pay any reward.
    pub pays: Option<CellbasePaysResponse>,
}

/// The block a cellbase pays the reward of: its own block number minus the
/// finalization delay ([`cellbase_reward_target`]) — read-time arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellbasePaysResponse {
    pub number: i64,
}

/// `pays` of a transaction at `block`: `None` unless it is a cellbase that
/// pays a reward.
fn cellbase_pays_response(
    is_cellbase: bool,
    window: ProposalWindow,
    block: i64,
) -> Option<CellbasePaysResponse> {
    if !is_cellbase {
        return None;
    }
    cellbase_reward_target(window, block).map(|number| CellbasePaysResponse { number })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalOrigin {
    pub block_number: i64,
    pub distance: i64,
    pub uncle: Option<ProposalOriginUncle>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalOriginUncle {
    pub number: i64,
    pub hash: String,
}

/// Where a committed transaction's cycles stand, from the store's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TxCyclesStatus {
    Done,
    Pending,
    /// The replay failed (`-1`); it will not be retried on its own.
    Failed,
}

/// The single reading of a stored `cycles` value.
pub(crate) fn tx_cycles_status(cycles: Option<i64>) -> TxCyclesStatus {
    match cycles {
        Some(c) if c > 0 => TxCyclesStatus::Done,
        Some(-1) => TxCyclesStatus::Failed,
        _ => TxCyclesStatus::Pending,
    }
}

pub(crate) fn display_cycles(
    cycles: Option<i64>,
    execution_exempt: bool,
    tx_hash: &[u8],
) -> Result<(Option<i64>, String), ApiRouteError> {
    if cycles.is_some_and(|cycles| cycles < -1) {
        return Err(ApiError::internal(format!(
            "invalid stored cycles {cycles:?} for transaction 0x{}",
            hex::encode(tx_hash)
        )));
    }
    if execution_exempt {
        if cycles.is_some_and(|value| value != 0) {
            return Err(ApiError::internal(format!(
                "execution-exempt transaction 0x{} has nonzero stored cycles {cycles:?}",
                hex::encode(tx_hash)
            )));
        }
        return Ok((Some(0), "complete".into()));
    }
    Ok(match tx_cycles_status(cycles) {
        TxCyclesStatus::Done => (cycles, "complete".into()),
        TxCyclesStatus::Failed => (None, "failed".into()),
        TxCyclesStatus::Pending => (None, "pending".into()),
    })
}

fn required_tx_block_hash(state: &AppState, block_number: i64) -> Result<Vec<u8>, ApiRouteError> {
    state
        .store
        .get_block_header(block_number)
        .map_err(|error| {
            ApiError::internal(format!(
                "transaction block {block_number} header read: {error}"
            ))
        })?
        .map(|header| header.hash)
        .ok_or_else(|| {
            ApiError::internal(format!(
                "transaction block {block_number} is missing its indexed header"
            ))
        })
}

fn required_tx_timestamp(
    timestamp_ms: i64,
    block_number: i64,
    tx_index: i32,
) -> Result<String, ApiRouteError> {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|timestamp| timestamp.to_rfc3339())
        .ok_or_else(|| {
            ApiError::internal(format!(
            "transaction block={block_number} index={tx_index} has invalid timestamp {timestamp_ms}"
        ))
        })
}

/// `reward_window` is the consensus proposal window, which a cellbase row needs
/// for its `pays`; a cellbase row without it is an error, never a `null`.
pub(crate) fn transaction_response(
    state: &AppState,
    block: i64,
    index: i32,
    entry: &ckbadger_store::TxIndexEntry,
    block_hash: &[u8],
    window: Option<ProposalWindow>,
    reward_window: Option<ProposalWindow>,
) -> Result<TransactionResponse, ApiRouteError> {
    let (value, activity) = if entry.is_cellbase {
        ("0".into(), None)
    } else {
        let actions = state.store.get_tx_actions(block, index, &entry.tx_hash)
            .map_err(|error| ApiError::internal(format!("tx actions block={block} index={index}: {error}")))?
            .ok_or_else(|| ApiError::internal(format!("transaction requires missing canonical actions: block={block} index={index} tx=0x{}", hex::encode(entry.tx_hash))))?;
        let activity = super::activities::build_global_activity_response(
            &state.store,
            &state.append_only_store,
            &state.ckb_network,
            &actions,
            &mut std::collections::HashMap::new(),
            &mut std::collections::HashMap::new(),
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
        (actions.ckb_moved().to_string(), Some(activity))
    };
    // One batched read of every output payload, not one point read each.
    let payloads = state
        .append_only_store
        .get_tx_output_payloads(&entry.tx_hash, entry.outputs_count)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let mut output_capacity = 0u128;
    for (output, cell) in payloads.into_iter().enumerate() {
        let cell = cell.ok_or_else(|| {
            ApiError::internal(format!(
                "missing canonical output payload 0x{}:{output}",
                hex::encode(entry.tx_hash)
            ))
        })?;
        let capacity = u128::try_from(cell.capacity)
            .map_err(|_| ApiError::internal("negative output capacity"))?;
        output_capacity = output_capacity
            .checked_add(capacity)
            .ok_or_else(|| ApiError::internal("output capacity overflow"))?;
    }
    let proposal_origin = if entry.is_cellbase || block == 0 {
        None
    } else if let Some(window) = window {
        let reader = state.ckb_store.as_ref().ok_or_else(|| {
            ApiError::internal("block transaction proposal origin requires CKB RocksDB reader")
        })?;
        let hit = find_proposal_window_hit(&state.store, reader, window, block, &entry.tx_hash)?
            .ok_or_else(|| ApiError::internal(format!("committed transaction 0x{} at block {block} has no proposal in consensus window", hex::encode(entry.tx_hash))))?;
        Some(ProposalOrigin {
            block_number: hit.block_number,
            distance: block - hit.block_number,
            uncle: hit.uncle.map(|(number, hash)| ProposalOriginUncle {
                number,
                hash: format!("0x{}", hex::encode(hash)),
            }),
        })
    } else {
        None
    };
    let (cycles, cycles_status) = display_cycles(
        entry.cycles,
        entry.is_cellbase || block == 0,
        &entry.tx_hash,
    )?;
    let pays = if entry.is_cellbase {
        let window = reward_window.ok_or_else(|| {
            ApiError::internal(format!(
                "cellbase row 0x{} at block {block} was built without the consensus window \
                 its `pays` needs",
                hex::encode(entry.tx_hash)
            ))
        })?;
        cellbase_pays_response(true, window, block)
    } else {
        None
    };
    Ok(TransactionResponse {
        pays,
        value,
        activity,
        output_capacity: output_capacity.to_string(),
        proposal_origin,
        hash: format!("0x{}", hex::encode(entry.tx_hash)),
        block_number: block,
        block_hash: format!("0x{}", hex::encode(block_hash)),
        index,
        inputs_count: i32::from(entry.inputs_count),
        outputs_count: i32::from(entry.outputs_count),
        fee: entry.fee.to_string(),
        tx_size: Some(tx_serialized_size_in_block(entry.tx_size)),
        cycles,
        cycles_status,
        is_cellbase: entry.is_cellbase,
        timestamp: required_tx_timestamp(entry.timestamp, block, index)?,
    })
}

pub(crate) fn transaction_activity_filter(
    filter: Option<&str>,
) -> Result<Option<&str>, ApiRouteError> {
    match filter {
        None | Some("") | Some("all") => Ok(None),
        Some(value @ ("ckb" | "script" | "cellbase" | "dao" | "token" | "object" | "identity")) => Ok(Some(value)),
        Some(value) => Err(ApiError::bad_request(format!("invalid activity_type '{value}'; expected ckb, script, cellbase, dao, token, object, or identity"))),
    }
}

/// Cellbase has no persisted TxActions row: scan the existing canonical index.
fn list_canonical_cellbase_page(
    store: &ckbadger_store::CkbadgerStore,
    limit: usize,
    cursor: Option<(i64, i32)>,
) -> anyhow::Result<Vec<TxListEntry>> {
    let mut from = Some(cursor.map(|(block, _)| block).unwrap_or(i64::MAX));
    let mut rows = Vec::with_capacity(limit);
    while let Some(block_cursor) = from {
        let blocks = store.list_blocks_desc(Some(block_cursor), 128)?;
        if blocks.is_empty() {
            break;
        }
        for (block, header) in &blocks {
            if cursor
                .is_some_and(|(last_block, last_index)| *block == last_block && last_index == 0)
            {
                continue;
            }
            // Consensus fixes cellbase at position zero; do not scan every
            // transaction in a block to discover the same position again.
            let entry = store.get_tx_index(*block, 0)?.ok_or_else(|| {
                anyhow::anyhow!("canonical block {block} lacks its cellbase transaction index")
            })?;
            anyhow::ensure!(
                entry.is_cellbase,
                "canonical block {block} transaction zero is not cellbase"
            );
            anyhow::ensure!(
                store.get_tx_location(&entry.tx_hash)? == Some((*block, 0)),
                "cellbase canonical location mismatch at {block}:0"
            );
            rows.push((*block, header.hash.clone(), 0, entry));
            if rows.len() == limit {
                return Ok(rows);
            }
        }
        let last_block = blocks.last().expect("nonempty block chunk").0;
        from = if last_block == 0 {
            None
        } else {
            Some(last_block - 1)
        };
    }
    Ok(rows)
}

async fn list_transactions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<TransactionResponse>> {
    let limit = params.limit.clamp(1, 100);
    let activity_filter =
        transaction_activity_filter(params.activity_type.as_deref())?.map(str::to_owned);
    // Cellbase rows name the block whose reward they pay (`pays`).
    let reward_window = state.consensus().await?.tx_proposal_window;

    // Validate before any store access: `get_block_header` encodes the number
    // into a store key, and a negative one trips an assert that aborts the
    // process under `panic = "abort"`.
    let block_number = params
        .block_number
        .map(|bn| validate_block_number(bn, "block_number"))
        .transpose()?;

    if block_number.is_none() {
        if let Some(filter) = activity_filter.clone() {
            let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "cursor")?;
            let store = state.store.clone();
            let rows = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<TxListEntry>> {
                if filter == "cellbase" {
                    return list_canonical_cellbase_page(&store, limit as usize + 1, cursor);
                }
                super::activities::list_canonical_global_actions_page(
                    &store,
                    limit as usize + 1,
                    cursor,
                    Some(&filter),
                )?
                .into_iter()
                .map(|actions| {
                    let entry = store
                        .get_tx_index(actions.block_number, actions.tx_index)?
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "canonical filtered transaction missing index at {}:{}",
                                actions.block_number,
                                actions.tx_index
                            )
                        })?;
                    let header =
                        store
                            .get_block_header(actions.block_number)?
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "canonical filtered transaction missing block {}",
                                    actions.block_number
                                )
                            })?;
                    Ok((actions.block_number, header.hash, actions.tx_index, entry))
                })
                .collect()
            })
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;
            let has_more = rows.len() > limit as usize;
            let page: Vec<_> = rows.into_iter().take(limit as usize).collect();
            let next_cursor = if has_more {
                page.last()
                    .map(|(block, _, index, _)| encode_cursor(*block, *index))
            } else {
                None
            };
            let rows = page
                .into_iter()
                .map(|(block, hash, index, entry)| {
                    transaction_response(
                        &state,
                        block,
                        index,
                        &entry,
                        &hash,
                        None,
                        Some(reward_window),
                    )
                })
                .collect::<Result<_, _>>()?;
            return ok(CursorPaginatedResponse::without_total(
                rows,
                limit,
                next_cursor,
            ));
        }
    }

    let count_store = state.store.clone();
    let (total, selected_block_hash) =
        tokio::task::spawn_blocking(move || -> Result<(i64, Option<Vec<u8>>), ApiRouteError> {
            Ok(if let Some(block_number) = block_number {
                match count_store
                    .get_block_header(block_number)
                    .map_err(|error| {
                        ApiError::internal(format!("block {block_number} header read: {error}"))
                    })? {
                    Some(header) => (i64::from(header.transactions_count), Some(header.hash)),
                    None => {
                        let has_index = !count_store
                            .list_block_txs_after(block_number, -1, 1)
                            .map_err(|error| {
                                ApiError::internal(format!(
                                    "block {block_number} transaction index read: {error}"
                                ))
                            })?
                            .is_empty();
                        if has_index {
                            return Err(ApiError::internal(format!(
                                "block {block_number} transaction index exists without header"
                            )));
                        }
                        return Err(ApiError::not_found(format!(
                            "block {block_number} not found"
                        )));
                    }
                }
            } else {
                (
                    count_store
                        .get_sync_status()
                        .map_err(|e| ApiError::internal(format!("sync status unavailable: {}", e)))?
                        .total_transactions,
                    None,
                )
            })
        })
        .await
        .map_err(|error| ApiError::internal(format!("transaction count task: {error}")))??;

    let store = state.store.clone();

    if let Some(block_number) = block_number {
        // List transactions for a specific block (ascending order)
        let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "cursor")?;
        // Cursor is the tx_idx of the last item on the previous page.
        // We want txs AFTER that index. Use -1 for first page (returns from idx 0).
        let after_tx_idx = cursor.map(|(_, idx)| idx).unwrap_or(-1);
        let fetch_limit = (limit + 1) as usize;

        let store_c = store.clone();
        let filter = activity_filter.clone();
        let page_txs = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            if let Some(filter) = filter {
                let mut rows = Vec::with_capacity(fetch_limit);
                for (index, entry) in store_c.list_block_txs(block_number)? {
                    if index <= after_tx_idx {
                        continue;
                    }
                    let matches = if entry.is_cellbase {
                        filter == "cellbase"
                    } else {
                        let actions = store_c
                            .get_tx_actions(block_number, index, &entry.tx_hash)?
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "missing actions for block {block_number} transaction {index}"
                                )
                            })?;
                        super::activities::matches_global_activity_filter(&actions, Some(&filter))
                    };
                    if matches {
                        rows.push((index, entry));
                    }

                    if rows.len() == fetch_limit {
                        break;
                    }
                }
                Ok(rows)
            } else {
                store_c.list_block_txs_after(block_number, after_tx_idx, fetch_limit)
            }
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

        // Get block hash for responses
        let block_hash = selected_block_hash.expect("block filter established header");

        let has_more = page_txs.len() as i64 > limit;
        let page: Vec<_> = page_txs.into_iter().take(limit as usize).collect();

        let next_cursor = if has_more {
            page.last()
                .map(|(tx_idx, _)| encode_cursor(block_number, *tx_idx))
        } else {
            None
        };

        if state.ckb_store.is_none() {
            return Err(ApiError::internal(
                "block transaction evidence requires CKB RocksDB reader",
            ));
        }
        let window = state.consensus().await?.tx_proposal_window;
        let txs: Vec<TransactionResponse> = page
            .into_iter()
            .map(|(tx_idx, entry)| -> Result<_, ApiRouteError> {
                transaction_response(
                    &state,
                    block_number,
                    tx_idx,
                    &entry,
                    &block_hash,
                    Some(window),
                    Some(window),
                )
            })
            .collect::<Result<_, _>>()?;

        ok(if activity_filter.is_some() {
            CursorPaginatedResponse::without_total(txs, limit, next_cursor)
        } else {
            CursorPaginatedResponse::new(txs, total, limit, next_cursor)
        })
    } else {
        // List latest transactions (DESC order across blocks)
        let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "cursor")?;
        let (cursor_block, cursor_index) = cursor.unwrap_or((i64::MAX, i32::MAX));

        let store_c = store.clone();
        let fetch_limit = (limit + 1) as usize;

        let txs_result =
            tokio::task::spawn_blocking(move || -> Result<Vec<TxListEntry>, anyhow::Error> {
                let mut results = Vec::with_capacity(fetch_limit);
                // Fetch blocks in small batches to avoid loading many more blocks than needed.
                // Most blocks have 1-3 transactions, so fetch_limit blocks is usually enough.
                let block_batch_size = fetch_limit.max(4);
                let mut next_block_cursor = Some(cursor_block);

                while results.len() < fetch_limit {
                    let from_block = match next_block_cursor {
                        Some(b) => b,
                        None => break, // no more blocks
                    };
                    let blocks =
                        store_c.list_blocks_desc(Some(from_block), block_batch_size + 1)?;
                    if blocks.is_empty() {
                        break;
                    }

                    // Determine next cursor for the next batch (if we need more blocks)
                    next_block_cursor = if blocks.len() > block_batch_size {
                        blocks.last().map(|(bn, _)| *bn)
                    } else {
                        None
                    };

                    for (block_num, header) in &blocks {
                        // For the cursor block, only fetch txs before the cursor index
                        let block_txs = if *block_num == cursor_block {
                            store_c.list_block_txs_before(*block_num, cursor_index, fetch_limit)?
                        } else {
                            store_c.list_block_txs(*block_num)?
                        };
                        for (tx_idx, entry) in block_txs.into_iter().rev() {
                            results.push((*block_num, header.hash.clone(), tx_idx, entry));
                            if results.len() >= fetch_limit {
                                break;
                            }
                        }
                        if results.len() >= fetch_limit {
                            break;
                        }
                    }
                }
                Ok(results)
            })
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let has_more = txs_result.len() as i64 > limit;
        let page: Vec<_> = txs_result.into_iter().take(limit as usize).collect();

        let next_cursor = if has_more {
            page.last()
                .map(|(block_num, _, tx_idx, _)| encode_cursor(*block_num, *tx_idx))
        } else {
            None
        };

        let txs: Vec<TransactionResponse> = page
            .into_iter()
            .map(
                |(block_num, block_hash, tx_idx, entry)| -> Result<_, ApiRouteError> {
                    transaction_response(
                        &state,
                        block_num,
                        tx_idx,
                        &entry,
                        &block_hash,
                        None,
                        Some(reward_window),
                    )
                },
            )
            .collect::<Result<_, _>>()?;

        ok(CursorPaginatedResponse::new(txs, total, limit, next_cursor))
    }
}

#[instrument(skip(state), level = "debug")]
async fn get_transaction(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;

    let store = state.store.clone();
    let hash_c = hash_bytes.clone();
    let tx_result = tokio::task::spawn_blocking(move || store.get_tx_by_hash(&hash_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match tx_result {
        Some((block_num, tx_idx, entry)) => {
            let block_hash = required_tx_block_hash(&state, block_num)?;
            let reward_window = state.consensus().await?.tx_proposal_window;
            ok(transaction_response(
                &state,
                block_num,
                tx_idx,
                &entry,
                &block_hash,
                None,
                Some(reward_window),
            )?)
        }
        None => {
            let ckb_block_number =
                lookup_tx_block_number_in_ckb_store(state.ckb_store.as_ref(), &hash_bytes);
            Err(missing_tx_lookup_error(&hash_bytes, ckb_block_number))
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionInputResponse {
    pub previous_output: Option<PreviousOutput>,
    pub since: String,
    pub capacity: Option<String>,
    /// Exact occupied bytes, matching output commonKnowledgeSize.
    #[serde(rename = "commonKnowledgeSize")]
    pub used_capacity: Option<String>,
    pub lock: Option<ScriptResponse>,
    pub r#type: Option<ScriptResponse>,
    pub address: Option<String>,
    pub cell_type: Option<String>,
    pub display: Option<super::transaction_cells::CellDisplay>,
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
    pub display: Option<super::transaction_cells::CellDisplay>,
    pub capacity: String,
    #[serde(rename = "commonKnowledgeSize")]
    pub used_capacity: String,
    #[serde(
        rename = "virtualCommonKnowledgeSize",
        skip_serializing_if = "Option::is_none"
    )]
    pub virtual_used_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_type: Option<String>,
    pub lock: Option<ScriptResponse>,
    pub r#type: Option<ScriptResponse>,
    pub address: Option<String>,
    pub status: Option<String>,
    pub consumed_by_tx: Option<String>,
    /// Change (TX-22): this output's lock is a lock of the transaction's own
    /// inputs and it carries no type script. Decided once, in
    /// [`settle_fee_payers_and_change`].
    pub is_change: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionDetailResponse {
    pub hash: String,
    pub status: String,
    /// Where an uncommitted transaction stands: `pending`, `proposed`, or
    /// `committed_awaiting_index` (the node has it in a block, this process's
    /// store has not indexed it yet). Absent once the store has it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_status: Option<String>,
    /// How completely an uncommitted transaction could be interpreted, with the
    /// reasons when it could not be interpreted fully.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<crate::pool::InterpretationResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i32>,
    pub inputs_count: i32,
    pub outputs_count: i32,
    pub fee: String,
    pub fee_rate: Option<String>,
    pub tx_size: Option<i32>,
    pub cycles: Option<i64>,
    pub cycles_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<i64>,
    pub is_cellbase: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs_capacity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs_capacity: Option<String>,
    #[serde(
        rename = "inputsCommonKnowledgeSize",
        skip_serializing_if = "Option::is_none"
    )]
    pub inputs_used_capacity: Option<String>,
    #[serde(
        rename = "outputsCommonKnowledgeSize",
        skip_serializing_if = "Option::is_none"
    )]
    pub outputs_used_capacity: Option<String>,
    pub inputs: Vec<TransactionInputResponse>,
    pub outputs: Vec<TransactionOutputResponse>,
    pub script_groups: Vec<super::transaction_structure::ScriptGroupResponse>,
    pub witness_details: Vec<super::transaction_structure::WitnessResponse>,
    pub witnesses: Vec<String>,
    pub witnesses_available: bool,
    /// The `.cell` names this transaction creates, each with the records
    /// payload decoded from the witness at its own output index. Absent when
    /// the transaction creates none (or its witnesses are unavailable).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dotcell_names: Vec<DotCellNameWitnessResponse>,
    /// Headers the transaction reads ("read, not consumed"): a DAO withdrawal
    /// reads its deposit and request blocks. In `header_deps` order.
    pub header_deps: Vec<HeaderDepResponse>,
    /// A cellbase's decoded witness 0 (miner lock and client message); `null`
    /// for every other transaction.
    pub cellbase_witness: Option<super::transaction_structure::CellbaseWitnessResponse>,
    /// For a cellbase, the block whose reward it pays (TX-23); `null` otherwise.
    pub pays: Option<CellbasePaysResponse>,
    /// Who paid the fee (TX-18): every input lock whose inputs hold more than
    /// the transaction returns to it, largest outflow first. Empty when no
    /// lock's balance fell — a cellbase, or a DAO withdrawal whose fee comes
    /// out of its compensation.
    pub fee_paid_by: Vec<FeePayerResponse>,
}

/// One lock that paid (part of) a transaction's fee.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeePayerResponse {
    pub lock_hash: String,
    pub address: Option<String>,
    /// Its inputs' capacity minus its outputs' capacity in this transaction,
    /// shannons (> 0).
    pub outflow: String,
}

/// The one rule for TX-18's fee payer and TX-22's change, over a detail's own
/// resolved inputs and outputs. A lock's net flow is its outputs' capacity
/// minus its inputs'; the payers are the locks whose net flow is negative.
/// An output is change when its lock is an input lock and it has no type
/// script. The cellbase's null input spends no cell (no lock, no capacity);
/// any other input without both is an error.
fn settle_fee_payers_and_change(
    inputs: &[TransactionInputResponse],
    outputs: &mut [TransactionOutputResponse],
    tx: &str,
) -> Result<Vec<FeePayerResponse>, ApiRouteError> {
    let error = |detail: String| ApiError::internal(format!("transaction {tx}: {detail}"));
    let capacity = |raw: &str, what: &str| -> Result<u128, ApiRouteError> {
        raw.parse::<u128>()
            .map_err(|_| error(format!("{what} capacity {raw:?} is not a shannon amount")))
    };
    // lock hash -> (inputs, outputs, address)
    let mut flows: std::collections::BTreeMap<String, (u128, u128, Option<String>)> =
        std::collections::BTreeMap::new();
    for (index, input) in inputs.iter().enumerate() {
        match (&input.lock, &input.capacity) {
            (Some(lock), Some(raw)) => {
                let hash = super::transaction_structure::script_hash(lock)
                    .map_err(|e| error(format!("input {index} lock hash: {e}")))?;
                let flow = flows.entry(hash).or_insert((0, 0, input.address.clone()));
                flow.0 = flow
                    .0
                    .checked_add(capacity(raw, &format!("input {index}"))?)
                    .ok_or_else(|| error("input capacity overflow".into()))?;
            }
            (None, None) => {}
            _ => {
                return Err(error(format!(
                    "input {index} is resolved without both its lock and its capacity"
                )))
            }
        }
    }
    for (index, output) in outputs.iter_mut().enumerate() {
        let lock = output
            .lock
            .as_ref()
            .ok_or_else(|| error(format!("output {index} has no lock")))?;
        let hash = super::transaction_structure::script_hash(lock)
            .map_err(|e| error(format!("output {index} lock hash: {e}")))?;
        let value = capacity(&output.capacity, &format!("output {index}"))?;
        output.is_change = false;
        if let Some(flow) = flows.get_mut(&hash) {
            flow.1 = flow
                .1
                .checked_add(value)
                .ok_or_else(|| error("output capacity overflow".into()))?;
            output.is_change = output.r#type.is_none();
        }
    }
    let mut payers: Vec<(u128, String, Option<String>)> = flows
        .into_iter()
        .filter(|(_, (spent, returned, _))| spent > returned)
        .map(|(hash, (spent, returned, address))| (spent - returned, hash, address))
        .collect();
    payers.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(payers
        .into_iter()
        .map(|(outflow, lock_hash, address)| FeePayerResponse {
            lock_hash,
            address,
            outflow: outflow.to_string(),
        })
        .collect())
}

/// One header dependency of a transaction.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderDepResponse {
    pub block_hash: String,
    /// The block's number in this store. Always set for a committed
    /// transaction (its header deps are canonical); `null` only for an
    /// uncommitted one whose header dep block is not indexed yet.
    pub block_number: Option<i64>,
}

/// Resolve header deps (`0x` block hashes) to their block numbers. A
/// committed transaction's header dep that is not a canonical block is an
/// error; an uncommitted one's may not be indexed yet.
fn header_deps_response(
    store: &ckbadger_store::CkbadgerStore,
    header_deps: &[String],
    committed: bool,
    tx: &str,
) -> Result<Vec<HeaderDepResponse>, ApiRouteError> {
    header_deps
        .iter()
        .map(|raw| {
            let hash = decode_hex_bytes_with_context(raw, "header dep", tx, Some(32))?;
            let block_number = store
                .get_block_number_by_hash(&hash)
                .map_err(|e| ApiError::internal(format!("{tx} header dep {raw}: {e}")))?;
            if committed && block_number.is_none() {
                return Err(ApiError::internal(format!(
                    "committed transaction {tx} reads header {raw}, which is not an indexed canonical block"
                )));
            }
            Ok(HeaderDepResponse {
                block_hash: format!("0x{}", hex::encode(&hash)),
                block_number,
            })
        })
        .collect()
}

/// Witness 0 of a cellbase, decoded; `None` for any other transaction.
fn cellbase_witness_response(
    is_cellbase: bool,
    witnesses: &[String],
    tx: &str,
) -> Result<Option<super::transaction_structure::CellbaseWitnessResponse>, ApiRouteError> {
    if !is_cellbase {
        return Ok(None);
    }
    let Some(raw) = witnesses.first() else {
        return Ok(None);
    };
    let bytes = decode_hex_bytes_with_context(raw, "cellbase witness", tx, None)?;
    Ok(super::transaction_structure::cellbase_witness(&bytes))
}

/// A `.cell` name created by a transaction, with the records its creating
/// witness carries (spec §1.3: `WitnessArgs.output_type` at the name cell's
/// own output index, hash-verified against `data[1..33]`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DotCellNameWitnessResponse {
    pub output_index: usize,
    pub label: String,
    pub name: String,
    pub identity_id: String,
    pub records_hash: String,
    pub records: Vec<DotCellRecordResponse>,
}

/// The same string-encoded transaction, in the indexer's RPC type: the direct
/// reader and the node lookup mirror the indexer's `rpc::TransactionView`
/// field for field, and the `.cell` parser takes the indexer's.
pub(crate) fn indexer_tx_view(tx: &RpcTransactionView) -> ckbadger_indexer::rpc::TransactionView {
    use ckbadger_indexer::rpc::{CellDep, CellInput, CellOutput, OutPoint, Script};

    let out_point = |p: &ckb_store_reader::RpcOutPoint| OutPoint {
        tx_hash: p.tx_hash.clone(),
        index: p.index.clone(),
    };
    let script = |s: &ckb_store_reader::RpcScript| Script {
        code_hash: s.code_hash.clone(),
        hash_type: s.hash_type.clone(),
        args: s.args.clone(),
    };
    ckbadger_indexer::rpc::TransactionView {
        hash: tx.hash.clone(),
        version: tx.version.clone(),
        cell_deps: tx
            .cell_deps
            .iter()
            .map(|dep| CellDep {
                out_point: out_point(&dep.out_point),
                dep_type: dep.dep_type.clone(),
            })
            .collect(),
        header_deps: tx.header_deps.clone(),
        inputs: tx
            .inputs
            .iter()
            .map(|input| CellInput {
                since: input.since.clone(),
                previous_output: out_point(&input.previous_output),
            })
            .collect(),
        outputs: tx
            .outputs
            .iter()
            .map(|output| CellOutput {
                capacity: output.capacity.clone(),
                lock: script(&output.lock),
                type_: output.type_.as_ref().map(script),
            })
            .collect(),
        outputs_data: tx.outputs_data.clone(),
        witnesses: tx.witnesses.clone(),
    }
}

/// The `.cell` names a transaction creates, decoded by the exact function the
/// live indexer runs (`DotCellParser::parse_name_cells_with_output_indices`).
///
/// A name cell whose own-index witness is missing or does not hash to the
/// cell's records hash is an error, not an empty list: its type script was
/// verified on chain (or by the pool), so the mismatch is a broken invariant.
/// The ring root is verified the same way but not listed: it is protocol
/// infrastructure, not a name with an identity.
fn dotcell_names_for_tx(
    tx: &RpcTransactionView,
) -> Result<Vec<DotCellNameWitnessResponse>, ApiRouteError> {
    let names = ckbadger_indexer::parser::DotCellParser::parse_name_cells_with_output_indices(
        &indexer_tx_view(tx),
    )
    .map_err(|e| ApiError::internal(format!("{e:#}")))?;
    Ok(names
        .into_iter()
        .filter(|(_, name, _)| !name.is_root())
        .map(|(output_index, name, records)| DotCellNameWitnessResponse {
            output_index,
            name: format!("{}.cell", name.label),
            identity_id: format!("0x{}", hex::encode(name.id)),
            records_hash: format!("0x{}", hex::encode(name.records_hash)),
            records: records.iter().map(dotcell_record_response).collect(),
            label: name.label,
        })
        .collect())
}

#[cfg(test)]
fn hash_type_byte_to_i16(byte: u8) -> i16 {
    match byte {
        0 => 0,
        1 => 1,
        2 => 2,
        4 => 4,
        _ => 0,
    }
}

fn parse_hash_type_label_to_i16(hash_type: &str) -> Result<i16, ApiRouteError> {
    ckbadger_common::hash_type_from_label(hash_type)
        .map(i16::from)
        .ok_or_else(|| {
            ApiError::internal(format!(
                "unknown script hash_type label in CKB store: '{}'",
                hash_type
            ))
        })
}

fn decode_hex_bytes_with_context(
    raw: &str,
    field: &str,
    context: &str,
    expected_len: Option<usize>,
) -> Result<Vec<u8>, ApiRouteError> {
    let bytes = hex::decode(raw.strip_prefix("0x").unwrap_or(raw)).map_err(|e| {
        ApiError::internal(format!(
            "invalid hex for {} while {}: value='{}', error={}",
            field, context, raw, e
        ))
    })?;
    if let Some(expected) = expected_len {
        if bytes.len() != expected {
            return Err(ApiError::internal(format!(
                "invalid byte length for {} while {}: expected {}, got {}",
                field,
                context,
                expected,
                bytes.len()
            )));
        }
    }
    Ok(bytes)
}

fn parse_u64_hex_field_with_context(
    raw: &str,
    field: &str,
    context: &str,
) -> Result<u64, ApiRouteError> {
    u64::from_str_radix(raw.strip_prefix("0x").unwrap_or(raw), 16).map_err(|e| {
        ApiError::internal(format!(
            "invalid hex u64 for {} while {}: value='{}', error={}",
            field, context, raw, e
        ))
    })
}

/// Compute an UNCOMMITTED transaction's fee from its fully resolved cells.
///
/// Committed transactions serve the stored fee — the indexer write path is the
/// single calculation path there. For an uncommitted one this is the only
/// source: `(Σ inputs + Σ DAO compensation) − Σ outputs`, where the
/// compensation is what the spent withdraw-request cells pay out (exactly the
/// live-sync correction `correct_dao_withdrawal_fees` applies). A negative
/// result is a broken invariant, never a zero.
fn compute_tx_fee_from_io(
    inputs_capacity: u128,
    dao_compensation: u128,
    outputs_capacity: u128,
    is_cellbase: bool,
    block_number: Option<i64>,
    tx_hash: &[u8],
) -> Result<u128, ApiRouteError> {
    if is_cellbase {
        return Ok(0);
    }

    inputs_capacity
        .checked_add(dao_compensation)
        .and_then(|effective_inputs| effective_inputs.checked_sub(outputs_capacity))
        .ok_or_else(|| {
            ApiError::internal(format!(
                "transaction inputs/outputs invariant broken at block {}: tx_hash=0x{}, inputs_capacity={}, dao_compensation={}, outputs_capacity={}",
                block_number.map_or_else(|| "(in pool)".to_string(), |number| number.to_string()),
                hex::encode(tx_hash),
                inputs_capacity,
                dao_compensation,
                outputs_capacity
            ))
        })
}

fn occupied_capacity_bytes(
    lock_args_len: usize,
    type_args_len: Option<usize>,
    data_size: usize,
) -> usize {
    let type_size = type_args_len.map_or(0, |len| 32 + 1 + len);
    8 + 32 + 1 + lock_args_len + type_size + data_size
}

#[instrument(skip(state, read_view), level = "debug")]
async fn get_transaction_detail(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
    // Optional so the handler still works in routers built without the
    // read-view middleware (unit tests mounting `routes()` directly).
    read_view: Option<Extension<RequestReadView>>,
) -> ApiResult<TransactionDetailResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;

    let store = state.store.clone();
    let hash_c = hash_bytes.clone();
    let tx_result = tokio::task::spawn_blocking(move || store.get_tx_by_hash(&hash_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let Some((block_number, tx_idx, entry)) = tx_result else {
        let tx_lookup = fetch_transaction_lookup(&state.ckb_rpc_url, &hash)
            .await
            .map_err(ApiError::internal)?;

        let Some(tx_lookup) = tx_lookup else {
            let ckb_block_number =
                lookup_tx_block_number_in_ckb_store(state.ckb_store.as_ref(), &hash_bytes);
            return Err(missing_tx_lookup_error(&hash_bytes, ckb_block_number));
        };

        // A transaction the node has committed but this process's store has not
        // indexed yet is served provisionally too. That window is seconds long,
        // and a pool row that links here must not become a dead link for them.
        if !tx_lookup.is_pending_like() && !tx_lookup.is_committed() {
            let ckb_block_number =
                lookup_tx_block_number_in_ckb_store(state.ckb_store.as_ref(), &hash_bytes);
            return Err(missing_tx_lookup_error(&hash_bytes, ckb_block_number));
        }

        let Some(rpc_tx) = tx_lookup.transaction.as_ref() else {
            return Err(ApiError::internal(format!(
                "pending transaction {} missing JSON transaction body from RPC",
                hash
            )));
        };
        // A committed response always names its committing block: the tx page
        // links it while the store catches up.
        if tx_lookup.is_committed()
            && (tx_lookup.block_number.is_none() || tx_lookup.block_hash.is_none())
        {
            return Err(ApiError::internal(format!(
                "node reported transaction {hash} committed without its block number/hash"
            )));
        }

        // The one store value this branch reads (immutable once derived), read
        // before letting go of the request's view: everything below talks to
        // the node, and holding the view across node round trips would stall
        // the secondary's catch-up for as long as the node takes to answer.
        let virtual_occupied = state.genesis_baseline()?.virtual_occupied;
        if let Some(Extension(view)) = read_view {
            view.release();
        }

        // One resolver, one order: a parent the mirror's snapshot holds, then
        // the parent transaction as the node returns it — so a chained
        // unconfirmed spend and a just-committed transaction's (spent) inputs
        // resolve here exactly as they do in the mirror.
        let pool_source = crate::pool::HttpPoolSource::new(state.ckb_rpc_url.clone());
        let pool_snapshot = state.pool_mirror.load();
        let previous_outputs =
            crate::pool::resolve_previous_outputs(&pool_source, rpc_tx, pool_snapshot.as_ref())
                .await
                .map_err(|e| {
                    ApiError::internal(format!(
                        "failed to resolve inputs of uncommitted transaction {hash}: {e}"
                    ))
                })?;
        let resolved = crate::pool::resolve_pool_tx(rpc_tx, &previous_outputs).map_err(|e| {
            ApiError::internal(format!(
                "failed to read uncommitted transaction {hash}: {e}"
            ))
        })?;

        // A parent the node does not know is a state that passes (it was
        // evicted, or has not propagated yet): say so and let the caller retry,
        // rather than serving inputs, a fee and an interpretation that are
        // missing a piece.
        let unresolved = resolved.unresolved_inputs();
        if !unresolved.is_empty() {
            let outpoints = unresolved
                .iter()
                .map(|(tx_hash, index)| format!("0x{}:{index}", hex::encode(tx_hash)))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ApiError::service_unavailable(format!(
                "inputs of uncommitted transaction {hash} are not resolvable yet: the node does \
                 not know the transaction that created {outpoints}; retry"
            )));
        }

        let interpretation = crate::pool::interpretation_of(&resolved);
        // The mirror is authoritative for the pool status when it tracks this
        // transaction (only it knows `committed_awaiting_index`); otherwise the
        // node's own status stands.
        let pool_status = pool_snapshot
            .records
            .get(&resolved.tx_hash)
            .map(|record| record.pool_status.as_str().to_string())
            .unwrap_or_else(|| {
                if tx_lookup.is_committed() {
                    crate::pool::PoolStatus::COMMITTED_AWAITING_INDEX.to_string()
                } else {
                    tx_lookup.status_str().to_string()
                }
            });

        let mut io = build_inputs_outputs_from_pool_tx(
            rpc_tx,
            &resolved,
            &state.ckb_network,
            tx_lookup.block_number,
            virtual_occupied,
        )?;

        for (index, (input, resolved_input)) in
            io.inputs.iter_mut().zip(&resolved.inputs).enumerate()
        {
            if let Some(cell) = &resolved_input.cell {
                input.display = super::transaction_cells::from_parts(
                    &state.store,
                    &cell.capacity.to_string(),
                    input.lock.as_ref().expect("resolved input lock"),
                    input.r#type.as_ref(),
                    &cell.data,
                    false,
                )
                .map_err(|e| {
                    ApiError::internal(format!(
                        "transaction {hash} pool input {index} display: {e:#}"
                    ))
                })?;
            }
        }
        for (index, (output, cell)) in io.outputs.iter_mut().zip(&resolved.outputs).enumerate() {
            output.display = super::transaction_cells::from_parts(
                &state.store,
                &cell.capacity.to_string(),
                output.lock.as_ref().expect("resolved output lock"),
                output.r#type.as_ref(),
                &cell.data,
                false,
            )
            .map_err(|e| {
                ApiError::internal(format!(
                    "transaction {hash} pool output {index} display: {e:#}"
                ))
            })?;
        }

        let pending_since = tx_lookup.time_added_to_pool.and_then(|timestamp| {
            chrono::DateTime::from_timestamp_millis(timestamp as i64).map(|dt| dt.to_rfc3339())
        });

        // The fee has ONE source: the fully resolved inputs (plus the exact DAO
        // compensation they pay out) minus the outputs. The node's pool-entry
        // fee is absent for committed transactions and is not a second path.
        let fee_value = io.fee;
        let fee = fee_value.to_string();

        let pending_tx_size = tx_lookup
            .tx_size
            .filter(|size| *size > 0)
            .map(tx_serialized_size_in_block);
        let fee_rate = pending_tx_size.map(|size| tx_fee_rate(fee_value, size));

        let pool_cycles = tx_lookup
            .cycles
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                ApiError::internal(format!("pool transaction {hash} cycles exceed i64"))
            })?;
        let pool_is_cellbase = resolved.is_cellbase;
        let (cycles, cycles_status) = display_cycles(
            pool_cycles,
            pool_is_cellbase || tx_lookup.block_number == Some(0),
            &hash_bytes,
        )?;
        let mut script_groups =
            super::transaction_structure::script_groups(&io.inputs, &io.outputs)
                .map_err(|e| ApiError::internal(e.to_string()))?;
        super::transaction_structure::apply_group_cycles(
            &mut script_groups,
            pool_is_cellbase || tx_lookup.block_number == Some(0),
            None,
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
        let outputs_data: Vec<&[u8]> = resolved
            .outputs
            .iter()
            .map(|cell| cell.data.as_slice())
            .collect();
        super::transaction_structure::attach_content_witnesses(
            &mut script_groups,
            &io.witnesses,
            &outputs_data,
        )
        .map_err(|e| {
            ApiError::internal(format!(
                "uncommitted transaction {hash} content witnesses: {e:#}"
            ))
        })?;
        let fee_paid_by = settle_fee_payers_and_change(&io.inputs, &mut io.outputs, &hash)?;
        // A cellbase is never pending; one the node has committed and this
        // store has not indexed yet still names the block it pays.
        let pays = match (pool_is_cellbase, tx_lookup.block_number) {
            (true, Some(number)) => {
                let window = state.consensus().await?.tx_proposal_window;
                cellbase_pays_response(true, window, number)
            }
            _ => None,
        };
        return ok(TransactionDetailResponse {
            pays,
            fee_paid_by,
            hash: format!("0x{}", hex::encode(&hash_bytes)),
            status: tx_lookup.status_str().to_string(),
            pool_status: Some(pool_status),
            interpretation: Some(crate::pool::InterpretationResponse::from(&interpretation)),
            pending_since,
            // Present only while the node reports a committing block this
            // process's store has not indexed yet.
            block_number: tx_lookup.block_number,
            block_hash: tx_lookup
                .block_hash
                .map(|hash| format!("0x{}", hex::encode(hash))),
            index: None,
            inputs_count: rpc_tx.inputs.len() as i32,
            outputs_count: rpc_tx.outputs.len() as i32,
            fee,
            fee_rate,
            tx_size: pending_tx_size,
            cycles,
            cycles_status,
            confirmations: None,
            is_cellbase: pool_is_cellbase,
            timestamp: None,
            inputs_capacity: Some(io.inputs_capacity.to_string()),
            outputs_capacity: Some(io.outputs_capacity.to_string()),
            inputs_used_capacity: Some(io.inputs_used_capacity.to_string()),
            outputs_used_capacity: Some(io.outputs_used_capacity.to_string()),
            script_groups,
            witness_details: super::transaction_structure::witness_details(
                &io.witnesses,
                pool_is_cellbase,
            )
            .map_err(|e| ApiError::internal(e.to_string()))?,
            header_deps: header_deps_response(&state.store, &rpc_tx.header_deps, false, &hash)?,
            cellbase_witness: cellbase_witness_response(pool_is_cellbase, &io.witnesses, &hash)?,
            inputs: io.inputs,
            outputs: io.outputs,
            witnesses: io.witnesses,
            witnesses_available: io.witnesses_available,
            dotcell_names: io.dotcell_names,
        });
    };

    let tx_hash_hex = format!("0x{}", hex::encode(&hash_bytes));
    let is_cellbase = entry.is_cellbase;
    let tx_size = entry.tx_size;
    let (cycles, cycles_status) =
        display_cycles(entry.cycles, is_cellbase || block_number == 0, &hash_bytes)?;
    let inputs_count = entry.inputs_count as i32;
    let outputs_count = entry.outputs_count as i32;

    let block_hash = required_tx_block_hash(&state, block_number)?;
    let timestamp = required_tx_timestamp(entry.timestamp, block_number, tx_idx)?;

    // Get tip block for confirmations
    let tip_block = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(format!("sync status unavailable: {}", e)))?
        .tip_block_number;

    let confirmations = if tip_block >= block_number {
        tip_block - block_number + 1
    } else {
        tracing::warn!(
            tip_block,
            block_number,
            "tip_block < block_number for committed tx; secondary reader may be stale"
        );
        0
    };

    let final_tx_size = if tx_size > 0 {
        Some(tx_serialized_size_in_block(tx_size))
    } else {
        None
    };

    // Read full transaction from CKB node's RocksDB for inputs/outputs
    let (
        inputs,
        mut outputs,
        inputs_capacity,
        outputs_capacity,
        inputs_occupied_capacity,
        outputs_occupied_capacity,
        witnesses,
        witnesses_available,
        dotcell_names,
        outputs_data,
        header_deps,
    ) = {
        let ckb_store = state.ckb_store.as_ref().ok_or_else(|| {
            ApiError::internal(format!(
                "transaction {tx_hash_hex} details unavailable: CKB RocksDB reader is not open"
            ))
        })?;
        let tx_hash_arr: [u8; 32] = hash_bytes
            .as_slice()
            .try_into()
            .expect("validated transaction hash");
        let tx_view = ckb_store.get_transaction(&tx_hash_arr).ok_or_else(|| ApiError::internal(format!(
            "canonical transaction {tx_hash_hex} at block {block_number}:{tx_idx} is missing from CKB RocksDB"
        )))?;
        let (
            inputs,
            outputs,
            inputs_capacity,
            outputs_capacity,
            inputs_occupied_capacity,
            outputs_occupied_capacity,
            witnesses,
            witnesses_available,
            dotcell_names,
        ) = build_inputs_outputs_from_ckb(
            &tx_view,
            ckb_store,
            &state.store,
            &state.append_only_store,
            &state.ckb_network,
            block_number,
            state.genesis_baseline()?.virtual_occupied,
        )?;
        let outputs_data: Vec<ckb_types::bytes::Bytes> = tx_view
            .outputs_data()
            .into_iter()
            .map(|data| data.raw_data())
            .collect();
        let header_deps: Vec<String> = tx_view
            .header_deps_iter()
            .map(|hash| format!("0x{}", hex::encode(hash.raw_data())))
            .collect();
        (
            inputs,
            outputs,
            inputs_capacity,
            outputs_capacity,
            inputs_occupied_capacity,
            outputs_occupied_capacity,
            witnesses,
            witnesses_available,
            dotcell_names,
            outputs_data,
            header_deps,
        )
    };

    for (index, output) in outputs.iter_mut().enumerate() {
        let index =
            i16::try_from(index).map_err(|_| ApiError::internal("output index exceeds i16"))?;
        let outpoint = ckbadger_store::keys::encode_outpoint(&hash_bytes, index);
        if state
            .store
            .get_cf(state.store.cf_live_cells(), &outpoint)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some()
        {
            output.status = Some("live".into());
        } else if let Some(consumed) = state
            .store
            .get_consumed_cell_info(&hash_bytes, index, &state.append_only_store)
            .map_err(|e| ApiError::internal(e.to_string()))?
        {
            output.status = Some("consumed".into());
            output.consumed_by_tx =
                Some(format!(
                    "0x{}",
                    hex::encode(consumed.consumed_by_tx.ok_or_else(|| ApiError::internal(
                        format!("missing consuming transaction for {tx_hash_hex}:{index}")
                    ))?)
                ));
        } else {
            return Err(ApiError::internal(format!(
                "missing canonical output state for {tx_hash_hex}:{index}"
            )));
        }
    }

    // The fee persisted by the indexer is the single source of truth; the
    // write path already accounts for DAO phase-2 compensation, so there is
    // no read-time recomputation from inputs/outputs.
    let fee_value = u128::try_from(entry.fee).map_err(|_| {
        ApiError::internal(format!(
            "negative stored fee for committed tx 0x{} at block {}: {}",
            hex::encode(&hash_bytes),
            block_number,
            entry.fee
        ))
    })?;
    let fee = fee_value.to_string();

    let fee_rate = final_tx_size.map(|size| tx_fee_rate(fee_value, size));

    let mut script_groups = super::transaction_structure::script_groups(&inputs, &outputs)
        .map_err(|e| ApiError::internal(format!("transaction {tx_hash_hex} groups: {e}")))?;
    let group_result = if is_cellbase || block_number == 0 {
        None
    } else {
        state
            .store
            .get_tx_cycles_result(&hash_bytes)
            .map_err(|e| ApiError::internal(format!("transaction {tx_hash_hex} cycles: {e}")))?
    };
    super::transaction_structure::apply_group_cycles(
        &mut script_groups,
        is_cellbase || block_number == 0,
        group_result.as_ref(),
    )
    .map_err(|e| ApiError::internal(format!("transaction {tx_hash_hex} group cycles: {e}")))?;
    super::transaction_structure::attach_content_witnesses(
        &mut script_groups,
        &witnesses,
        &outputs_data,
    )
    .map_err(|e| {
        ApiError::internal(format!(
            "transaction {tx_hash_hex} content witnesses: {e:#}"
        ))
    })?;

    let header_deps = header_deps_response(&state.store, &header_deps, true, &tx_hash_hex)?;
    let cellbase_witness = cellbase_witness_response(is_cellbase, &witnesses, &tx_hash_hex)?;
    let fee_paid_by = settle_fee_payers_and_change(&inputs, &mut outputs, &tx_hash_hex)?;
    let pays = if is_cellbase {
        let window = state.consensus().await?.tx_proposal_window;
        cellbase_pays_response(true, window, block_number)
    } else {
        None
    };
    ok(TransactionDetailResponse {
        pays,
        fee_paid_by,
        hash: tx_hash_hex,
        status: "committed".to_string(),
        // A transaction the local store has indexed is no longer provisional.
        pool_status: None,
        interpretation: None,
        pending_since: None,
        block_number: Some(block_number),
        block_hash: Some(format!("0x{}", hex::encode(&block_hash))),
        index: Some(tx_idx),
        inputs_count,
        outputs_count,
        fee,
        fee_rate,
        tx_size: final_tx_size,
        cycles,
        cycles_status,
        confirmations: Some(confirmations),
        is_cellbase,
        timestamp: Some(timestamp),
        inputs_capacity: Some(inputs_capacity.to_string()),
        outputs_capacity: Some(outputs_capacity.to_string()),
        inputs_used_capacity: Some(inputs_occupied_capacity.to_string()),
        outputs_used_capacity: Some(outputs_occupied_capacity.to_string()),
        script_groups,
        witness_details: super::transaction_structure::witness_details(&witnesses, is_cellbase)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        header_deps,
        cellbase_witness,
        inputs,
        outputs,
        witnesses,
        witnesses_available,
        dotcell_names,
    })
}

/// Build the `/tx/{hash}` response's inputs and outputs from a transaction the
/// shared pool resolver has already resolved — every input of it: the caller
/// answers 503 while any is not.
///
/// The previous store-based lookup (live cell, else consumed cell) is gone: an
/// uncommitted transaction's inputs are resolved in exactly ONE place — the
/// pool resolver's fixed order of snapshot parent, then the parent transaction
/// from the node — so the pending view and the address-page pool rows can
/// never disagree about what a transaction spends.
///
/// `block_number` is the committing block of a committed-awaiting-index
/// transaction and `None` for one still in the pool: only block 0's cells can
/// be the genesis burn cell.
fn build_inputs_outputs_from_pool_tx(
    rpc_tx: &RpcTransactionView,
    resolved: &crate::pool::ResolvedPoolTx,
    network: &str,
    block_number: Option<i64>,
    virtual_occupied: i128,
) -> Result<PendingTxIoBundle, ApiRouteError> {
    let mut inputs_capacity: u128 = 0;
    let mut inputs_occupied_capacity: u128 = 0;
    let mut dao_compensation: u128 = 0;

    let inputs = resolved
        .inputs
        .iter()
        .map(|input| -> Result<TransactionInputResponse, ApiRouteError> {
            let index = if resolved.is_cellbase && input.cell.is_none() {
                // The cellbase pseudo-input's 0xffffffff, rendered exactly as
                // the committed path renders it (`as i32`, i.e. -1), so the
                // same transaction reads the same before and after indexing.
                input.previous_output_index as i32
            } else {
                i32::try_from(input.previous_output_index).map_err(|_| {
                    ApiError::internal(format!(
                        "previous output index {} exceeds i32 for tx 0x{}",
                        input.previous_output_index,
                        hex::encode(resolved.tx_hash)
                    ))
                })?
            };
            let previous_output = Some(PreviousOutput {
                tx_hash: format!("0x{}", hex::encode(input.previous_tx_hash)),
                index,
            });

            let Some(cell) = input.cell.as_ref() else {
                if !resolved.is_cellbase {
                    return Err(ApiError::internal(format!(
                        "unresolved input 0x{}:{} reached the response builder for tx 0x{}",
                        hex::encode(input.previous_tx_hash),
                        input.previous_output_index,
                        hex::encode(resolved.tx_hash)
                    )));
                }
                // The cellbase pseudo-input spends no cell.
                return Ok(TransactionInputResponse {
                    display: None,
                    previous_output,
                    since: input.since.clone(),
                    capacity: None,
                    used_capacity: None,
                    lock: None,
                    r#type: None,
                    address: None,
                    cell_type: None,
                });
            };

            inputs_capacity += cell.capacity as u128;
            if let Some(compensation) = cell.dao_compensation {
                dao_compensation += u128::try_from(compensation).map_err(|_| {
                    ApiError::internal(format!(
                        "negative DAO compensation {compensation} for input 0x{}:{} of tx 0x{}",
                        hex::encode(input.previous_tx_hash),
                        input.previous_output_index,
                        hex::encode(resolved.tx_hash)
                    ))
                })?;
            }
            let occupied_bytes = occupied_capacity_bytes(
                cell.lock_args.len(),
                cell.type_code_hash
                    .as_ref()
                    .map(|_| cell.type_args.as_deref().unwrap_or(&[]).len()),
                cell.data.len(),
            );
            inputs_occupied_capacity += occupied_bytes as u128;

            let lock_hash_type_str = hash_type_to_str(cell.lock_hash_type).ok_or_else(|| {
                ApiError::internal(format!(
                    "unknown lock hash_type {} for resolved pool input",
                    cell.lock_hash_type
                ))
            })?;
            let lock = Some(ScriptResponse {
                code_hash: format!("0x{}", hex::encode(&cell.lock_code_hash)),
                hash_type: lock_hash_type_str.to_string(),
                args: format!("0x{}", hex::encode(&cell.lock_args)),
            });

            let type_script = cell
                .type_code_hash
                .as_ref()
                .map(|type_code_hash| -> Result<ScriptResponse, ApiRouteError> {
                    // The node returned the script itself, so the hash_type is
                    // known exactly — no store lookup, no "unknown" label.
                    let hash_type = cell.type_hash_type.ok_or_else(|| {
                        ApiError::internal(
                            "resolved pool input has a type code_hash without a hash_type"
                                .to_string(),
                        )
                    })?;
                    let hash_type_str = hash_type_to_str(hash_type).ok_or_else(|| {
                        ApiError::internal(format!(
                            "unknown type hash_type {hash_type} for resolved pool input"
                        ))
                    })?;
                    Ok(ScriptResponse {
                        code_hash: format!("0x{}", hex::encode(type_code_hash)),
                        hash_type: hash_type_str.to_string(),
                        args: format!(
                            "0x{}",
                            hex::encode(cell.type_args.as_deref().unwrap_or(&[]))
                        ),
                    })
                })
                .transpose()?;

            Ok(TransactionInputResponse {
                display: None,
                previous_output,
                since: input.since.clone(),
                capacity: Some(cell.capacity.to_string()),
                used_capacity: Some(occupied_bytes.to_string()),
                lock,
                cell_type: Some(super::transaction_structure::cell_type(
                    type_script.as_ref(),
                )?),
                r#type: type_script,
                address: script_to_address(
                    &cell.lock_code_hash,
                    cell.lock_hash_type,
                    &cell.lock_args,
                    network,
                )
                .ok(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut outputs_capacity: u128 = 0;
    let mut outputs_occupied_capacity: u128 = 0;
    let outputs = resolved
        .outputs
        .iter()
        .map(|cell| -> Result<TransactionOutputResponse, ApiRouteError> {
            outputs_capacity += cell.capacity as u128;
            let occupied = occupied_capacity_bytes(
                cell.lock_args.len(),
                cell.type_code_hash
                    .as_ref()
                    .map(|_| cell.type_args.as_deref().unwrap_or(&[]).len()),
                cell.data.len(),
            );
            outputs_occupied_capacity += occupied as u128;

            let lock_hash_type_str = hash_type_to_str(cell.lock_hash_type).ok_or_else(|| {
                ApiError::internal(format!(
                    "unknown lock hash_type {} for pool output",
                    cell.lock_hash_type
                ))
            })?;
            let type_script = cell
                .type_code_hash
                .as_ref()
                .map(|type_code_hash| -> Result<ScriptResponse, ApiRouteError> {
                    let hash_type = cell.type_hash_type.ok_or_else(|| {
                        ApiError::internal(
                            "pool output has a type code_hash without a hash_type".to_string(),
                        )
                    })?;
                    let hash_type_str = hash_type_to_str(hash_type).ok_or_else(|| {
                        ApiError::internal(format!(
                            "unknown type hash_type {hash_type} for pool output"
                        ))
                    })?;
                    Ok(ScriptResponse {
                        code_hash: format!("0x{}", hex::encode(type_code_hash)),
                        hash_type: hash_type_str.to_string(),
                        args: format!(
                            "0x{}",
                            hex::encode(cell.type_args.as_deref().unwrap_or(&[]))
                        ),
                    })
                })
                .transpose()?;

            let is_satoshi = block_number == Some(0)
                && ckbadger_common::burn_policy::burn_policy(network)
                    .is_some_and(|p| cell.lock_args.as_slice() == p.lock_args);
            let (cell_type, virtual_occupied_capacity) = if is_satoshi {
                (
                    Some("genesis_special_burn".to_string()),
                    Some(virtual_occupied.to_string()),
                )
            } else {
                (
                    Some(super::transaction_structure::cell_type(
                        type_script.as_ref(),
                    )?),
                    None,
                )
            };

            Ok(TransactionOutputResponse {
                display: None,
                status: Some("pending".into()),
                consumed_by_tx: None,
                is_change: false,
                capacity: cell.capacity.to_string(),
                used_capacity: occupied.to_string(),
                virtual_used_capacity: virtual_occupied_capacity,
                cell_type,
                lock: Some(ScriptResponse {
                    code_hash: format!("0x{}", hex::encode(&cell.lock_code_hash)),
                    hash_type: lock_hash_type_str.to_string(),
                    args: format!("0x{}", hex::encode(&cell.lock_args)),
                }),
                r#type: type_script,
                address: script_to_address(
                    &cell.lock_code_hash,
                    cell.lock_hash_type,
                    &cell.lock_args,
                    network,
                )
                .ok(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let fee = compute_tx_fee_from_io(
        inputs_capacity,
        dao_compensation,
        outputs_capacity,
        resolved.is_cellbase,
        block_number,
        &resolved.tx_hash,
    )?;

    Ok(PendingTxIoBundle {
        inputs,
        outputs,
        inputs_capacity,
        outputs_capacity,
        inputs_used_capacity: inputs_occupied_capacity,
        outputs_used_capacity: outputs_occupied_capacity,
        fee,
        witnesses: resolved.witnesses.clone(),
        witnesses_available: true,
        dotcell_names: dotcell_names_for_tx(rpc_tx)?,
    })
}

fn required_committed_input_cell(
    domain_store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
    tx_hash: &[u8],
    output_index: i16,
    context: &str,
) -> Result<ckbadger_store::types::PositionedCellInfo, ApiRouteError> {
    if let Some(cell) = domain_store
        .get_cell(tx_hash, output_index, cells_store)
        .map_err(|error| {
            ApiError::internal(format!(
                "live input cell read failed while {context}: {error}"
            ))
        })?
    {
        return Ok(cell);
    }
    domain_store
        .get_consumed_cell(tx_hash, output_index, cells_store)
        .map_err(|error| {
            ApiError::internal(format!(
                "consumed input cell read failed while {context}: {error}"
            ))
        })?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "missing indexed input cell while {context}: outpoint=0x{}:{}",
                hex::encode(tx_hash),
                output_index
            ))
        })
}

fn required_output_data_size(
    outputs_data: &[String],
    output_index: usize,
    context: &str,
) -> Result<usize, ApiRouteError> {
    let data = outputs_data.get(output_index).ok_or_else(|| {
        ApiError::internal(format!(
            "missing output data for {context}, output_index={output_index}"
        ))
    })?;
    decode_hex_bytes_with_context(data, "output.data", context, None).map(|bytes| bytes.len())
}

/// Build inputs/outputs from CKB node's RocksDB transaction view.
#[allow(clippy::too_many_arguments)]
fn build_inputs_outputs_from_ckb(
    tx_view: &ckb_types::core::TransactionView,
    reader: &ckb_store_reader::CkbChainReader,
    core_store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
    network: &str,
    block_number: i64,
    virtual_occupied: i128,
) -> Result<TxIoBundle, ApiRouteError> {
    let rpc_tx = ckb_store_reader::convert_transaction_view(tx_view);
    let witnesses = rpc_tx.witnesses.clone();
    let dotcell_names = dotcell_names_for_tx(&rpc_tx)?;

    let mut inputs_capacity: u128 = 0;
    let mut inputs_occupied_capacity: u128 = 0;

    let inputs: Vec<TransactionInputResponse> = rpc_tx
        .inputs
        .iter()
        .map(|input| -> Result<TransactionInputResponse, ApiRouteError> {
            let prev_tx_hash_hex = &input.previous_output.tx_hash;
            let prev_index_hex = &input.previous_output.index;
            let input_context = format!(
                "building input for tx=0x{} prev_outpoint=({}, {})",
                hex::encode(tx_view.hash().raw_data()),
                prev_tx_hash_hex,
                prev_index_hex
            );
            let prev_index = u32::from_str_radix(
                prev_index_hex.strip_prefix("0x").unwrap_or(prev_index_hex),
                16,
            )
            .map_err(|e| {
                ApiError::internal(format!(
                    "invalid previous_output.index while {}: value='{}', error={}",
                    input_context, prev_index_hex, e
                ))
            })?;

            let since = &input.since;

            // Try to look up the previous output cell for capacity/lock info
            let prev_tx_hash_bytes = decode_hex_bytes_with_context(
                prev_tx_hash_hex,
                "input.previous_output.tx_hash",
                &input_context,
                Some(32),
            )?;

            // Cellbase input: prev_tx_hash is all zeros, index is 0xffffffff.
            // No previous cell exists to look up.
            let is_cellbase_input = prev_tx_hash_bytes.iter().all(|&b| b == 0);

            let (capacity, used_capacity, lock, type_script, address) = if is_cellbase_input {
                (None, None, None, None, None)
            } else {
                // Try live cells first, then consumed cells in our store
                let prev_index_i16 = i16::try_from(prev_index).map_err(|_| {
                    ApiError::internal(format!(
                        "output index {} exceeds i16 range while {}",
                        prev_index, input_context
                    ))
                })?;
                let cell_info = required_committed_input_cell(
                    core_store,
                    cells_store,
                    &prev_tx_hash_bytes,
                    prev_index_i16,
                    &input_context,
                )?;

                let info = cell_info;
                let cap = info.capacity as u128;
                inputs_capacity += cap;

                let occ = occupied_capacity_bytes(
                    info.lock_args.len(),
                    info.type_code_hash
                        .as_ref()
                        .map(|_| info.type_args.as_deref().unwrap_or(&[]).len()),
                    info.data_size as usize,
                );
                inputs_occupied_capacity += occ as u128;

                let lock_hash_type_str =
                    hash_type_to_str(info.lock_hash_type).ok_or_else(|| {
                        ApiError::internal(format!(
                            "unknown lock hash_type {} for input cell",
                            info.lock_hash_type
                        ))
                    })?;
                let lock_resp = ScriptResponse {
                    code_hash: format!("0x{}", hex::encode(&info.lock_code_hash)),
                    hash_type: lock_hash_type_str.to_string(),
                    args: format!("0x{}", hex::encode(&info.lock_args)),
                };
                let type_resp = info
                    .type_code_hash
                    .as_ref()
                    .map(|type_code_hash| -> Result<ScriptResponse, ApiRouteError> {
                        Ok(ScriptResponse {
                            code_hash: format!("0x{}", hex::encode(type_code_hash)),
                            hash_type: info
                                .type_hash_type
                                .and_then(hash_type_to_str)
                                .ok_or_else(|| {
                                    ApiError::internal(format!(
                                        "missing or invalid stored type hash_type {:?} while {}",
                                        info.type_hash_type, input_context
                                    ))
                                })?
                                .to_string(),
                            args: format!(
                                "0x{}",
                                hex::encode(info.type_args.as_deref().unwrap_or(&[]))
                            ),
                        })
                    })
                    .transpose()?;

                let addr = script_to_address(
                    &info.lock_code_hash,
                    info.lock_hash_type,
                    &info.lock_args,
                    network,
                )
                .ok();

                (
                    Some(cap.to_string()),
                    Some(occ.to_string()),
                    Some(lock_resp),
                    type_resp,
                    addr,
                )
            };

            let display = if super::transaction_cells::has_display(type_script.as_ref())
                .map_err(|e| ApiError::internal(format!("{input_context}: {e}")))?
            {
                let hash: [u8; 32] = prev_tx_hash_bytes
                    .as_slice()
                    .try_into()
                    .expect("validated input hash");
                let previous = reader.get_transaction(&hash).ok_or_else(|| {
                    ApiError::internal(format!(
                        "missing CKB RocksDB input transaction while {input_context}"
                    ))
                })?;
                let data = previous
                    .outputs_data()
                    .get(prev_index as usize)
                    .ok_or_else(|| {
                        ApiError::internal(format!(
                            "missing CKB RocksDB input data while {input_context}"
                        ))
                    })?
                    .raw_data();
                super::transaction_cells::from_parts(
                    core_store,
                    capacity.as_deref().expect("real input capacity"),
                    lock.as_ref().expect("real input lock"),
                    type_script.as_ref(),
                    &data,
                    true,
                )
                .map_err(|e| ApiError::internal(format!("{input_context} display: {e:#}")))?
            } else {
                None
            };

            Ok(TransactionInputResponse {
                display,
                previous_output: Some(PreviousOutput {
                    tx_hash: prev_tx_hash_hex.clone(),
                    index: prev_index as i32,
                }),
                since: since.clone(),
                cell_type: if capacity.is_some() {
                    Some(super::transaction_structure::cell_type(
                        type_script.as_ref(),
                    )?)
                } else {
                    None
                },
                capacity,
                used_capacity,
                lock,
                r#type: type_script,
                address,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut outputs_capacity: u128 = 0;
    let mut outputs_occupied_capacity: u128 = 0;

    let tx_hash: Vec<u8> = tx_view.hash().raw_data().to_vec();
    let outputs: Vec<TransactionOutputResponse> = rpc_tx
        .outputs
        .iter()
        .enumerate()
        .map(
            |(output_idx, output)| -> Result<TransactionOutputResponse, ApiRouteError> {
                let output_context = format!(
                    "building output for tx=0x{} output_index={}",
                    hex::encode(&tx_hash),
                    output_idx
                );
                let cap_hex = &output.capacity;
                let cap =
                    parse_u64_hex_field_with_context(cap_hex, "output.capacity", &output_context)?;
                outputs_capacity += cap as u128;

                let lock = &output.lock;
                let code_hash_bytes = decode_hex_bytes_with_context(
                    &lock.code_hash,
                    "output.lock.code_hash",
                    &output_context,
                    Some(32),
                )?;
                let ht = parse_hash_type_label_to_i16(&lock.hash_type)?;
                let args_bytes = decode_hex_bytes_with_context(
                    &lock.args,
                    "output.lock.args",
                    &output_context,
                    None,
                )?;

                let lock_resp = ScriptResponse {
                    code_hash: lock.code_hash.clone(),
                    hash_type: lock.hash_type.clone(),
                    args: lock.args.clone(),
                };

                let address = script_to_address(&code_hash_bytes, ht, &args_bytes, network).ok();

                let type_resp = output.type_.as_ref().map(|t| ScriptResponse {
                    code_hash: t.code_hash.clone(),
                    hash_type: t.hash_type.clone(),
                    args: t.args.clone(),
                });

                let type_args_len = output
                    .type_
                    .as_ref()
                    .map(|t| {
                        decode_hex_bytes_with_context(
                            &t.args,
                            "output.type.args",
                            &output_context,
                            None,
                        )
                        .map(|v| v.len())
                    })
                    .transpose()?;

                let data_size =
                    required_output_data_size(&rpc_tx.outputs_data, output_idx, &output_context)?;

                let occ = occupied_capacity_bytes(args_bytes.len(), type_args_len, data_size);
                outputs_occupied_capacity += occ as u128;

                let is_satoshi = block_number == 0
                    && ckbadger_common::burn_policy::burn_policy(network)
                        .is_some_and(|p| args_bytes.as_slice() == p.lock_args);
                let (cell_type, virtual_occupied_capacity) = if is_satoshi {
                    (
                        Some("genesis_special_burn".to_string()),
                        Some(virtual_occupied.to_string()),
                    )
                } else {
                    (
                        Some(super::transaction_structure::cell_type(type_resp.as_ref())?),
                        None,
                    )
                };

                let display = super::transaction_cells::from_parts(
                    core_store,
                    &cap.to_string(),
                    &lock_resp,
                    type_resp.as_ref(),
                    &decode_hex_bytes_with_context(
                        &rpc_tx.outputs_data[output_idx],
                        "output.data",
                        &output_context,
                        None,
                    )?,
                    true,
                )
                .map_err(|e| ApiError::internal(format!("{output_context} display: {e:#}")))?;

                Ok(TransactionOutputResponse {
                    display,
                    status: None,
                    consumed_by_tx: None,
                    is_change: false,
                    capacity: cap.to_string(),
                    used_capacity: occ.to_string(),
                    virtual_used_capacity: virtual_occupied_capacity,
                    cell_type,
                    lock: Some(lock_resp),
                    r#type: type_resp,
                    address,
                })
            },
        )
        .collect::<Result<Vec<_>, _>>()?;

    Ok((
        inputs,
        outputs,
        inputs_capacity,
        outputs_capacity,
        inputs_occupied_capacity,
        outputs_occupied_capacity,
        witnesses,
        true,
        dotcell_names,
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellDepResponse {
    pub out_point_tx_hash: String,
    pub out_point_index: i32,
    pub dep_type: String,
    pub resolved_cells: Vec<super::transaction_structure::ResolvedDependencyCell>,
    pub script_hashes: Vec<String>,
}

async fn get_cell_deps(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Vec<CellDepResponse>> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;
    let mut tx_hash = [0u8; 32];
    tx_hash.copy_from_slice(&hash_bytes);

    if state
        .store
        .get_tx_location(&hash_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_none()
        && state.pool_mirror.load().records.contains_key(&tx_hash)
    {
        return Err(ApiError::bad_request(format!(
            "Cell deps for pending transaction {hash} are not committed"
        )));
    }

    // Cell deps are read from the CKB node's own RocksDB. Without that reader
    // the endpoint has no data source at all, so fail loudly instead of
    // shipping a silent `[]` that is indistinguishable from "no deps".
    let Some(store) = state.ckb_store.as_ref() else {
        return Err(ApiError::internal(format!(
            "cell deps for {} unavailable: CKB RocksDB reader is not open; check the configured CKB db path",
            hash
        )));
    };

    if let Some(tx_view) = store.get_transaction(&tx_hash) {
        return ok(
            super::transaction_structure::resolved_dependencies(&tx_view, store)
                .map_err(|e| ApiError::internal(format!("cell deps for {hash}: {e}")))?,
        );
    }

    if state
        .store
        .get_tx_location(&hash_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_some()
    {
        Err(ApiError::internal(format!(
            "canonical transaction {hash} missing from CKB RocksDB"
        )))
    } else if state.pool_mirror.load().records.contains_key(&tx_hash) {
        Err(ApiError::bad_request(format!(
            "Cell deps for pending transaction {hash} are not committed"
        )))
    } else {
        Err(ApiError::not_found("Transaction not found"))
    }
}

async fn get_cycles_status(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<CyclesStatusResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;
    if let Some(response) = stored_cycles_response(load_tx_cycles_state(&state, &hash_bytes).await?)
    {
        return ok(response);
    }
    if !state.cycles_client.is_enabled() {
        return ok(CyclesStatusResponse {
            status: CyclesStatus::Failed,
            cycles: None,
            error: Some("Cycles task dispatch unavailable: worker not connected".into()),
        });
    }
    ok(cycles_enqueue_response(
        state.cycles_client.enqueue_task(&hash).await,
    ))
}

async fn trigger_cycles_calculation(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
    read_view: Option<Extension<RequestReadView>>,
    wait: RequestWait,
) -> ApiResult<CyclesStatusResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;
    if let Some(response) = stored_cycles_response(load_tx_cycles_state(&state, &hash_bytes).await?)
    {
        return ok(response);
    }
    if let Err(error) = state.cycles_client.enqueue_task(&hash).await {
        return ok(CyclesStatusResponse {
            status: CyclesStatus::Failed,
            cycles: None,
            error: Some(error),
        });
    }
    // Completion is the canonical indexer-written group record, not an IPC
    // notification whose RocksDB secondary view may not yet be visible. Drop
    // this request's pin before waiting for catch-up to reveal that record.
    if let Some(Extension(view)) = read_view {
        view.release();
    }
    ok(wait_cycles_result(&state, &hash_bytes, &wait).await?)
}

#[derive(Debug, Clone)]
enum StoredCycles {
    Complete(i64),
    Failed { total: Option<i64>, error: String },
    Paused { total: Option<i64>, fork_point: u64 },
    Pending,
}

fn stored_cycles_response(state: Option<StoredCycles>) -> Option<CyclesStatusResponse> {
    Some(match state {
        Some(StoredCycles::Complete(cycles)) => CyclesStatusResponse {
            status: CyclesStatus::Done,
            cycles: Some(cycles),
            error: None,
        },
        Some(StoredCycles::Failed { total, error }) => CyclesStatusResponse {
            status: CyclesStatus::Failed,
            cycles: total,
            error: Some(error),
        },
        Some(StoredCycles::Paused { total, fork_point }) => CyclesStatusResponse {
            status: CyclesStatus::Paused,
            cycles: total,
            error: Some(format!(
                "Indexer paused on a deep fork above block {fork_point}"
            )),
        },
        Some(StoredCycles::Pending) => return None,
        None => CyclesStatusResponse {
            status: CyclesStatus::NotFound,
            cycles: None,
            error: Some("Transaction not found".into()),
        },
    })
}

async fn load_tx_cycles_state(
    state: &Arc<AppState>,
    hash_bytes: &[u8],
) -> Result<Option<StoredCycles>, ApiRouteError> {
    let state = state.clone();
    let hash = hash_bytes.to_vec();
    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        // Every poll pins only its bounded multi-CF read, never the sleep.
        let _view = ckbadger_store::read_view::acquire_read();
        let store = &state.store;
        let Some((block, _, entry)) = store.get_tx_by_hash(&hash)? else {
            return Ok(None);
        };
        anyhow::ensure!(
            !entry.cycles.is_some_and(|cycles| cycles < -1),
            "invalid stored cycles {:?} for transaction 0x{}",
            entry.cycles,
            hex::encode(&hash)
        );
        if entry.is_cellbase || block == 0 {
            anyhow::ensure!(
                entry.cycles.is_none_or(|cycles| cycles == 0),
                "execution-exempt transaction 0x{} at block {block} has nonzero stored cycles {:?}",
                hex::encode(&hash),
                entry.cycles
            );
            return Ok(Some(StoredCycles::Complete(0)));
        }
        Ok(Some(match store.get_tx_group_cycles_record(&hash)? {
            Some(ckbadger_common::cycles_task::TxGroupCyclesRecord::Complete(result)) => {
                StoredCycles::Complete(result.cycles)
            }
            Some(ckbadger_common::cycles_task::TxGroupCyclesRecord::Failed { failure, .. }) => {
                StoredCycles::Failed {
                    total: entry.cycles.filter(|cycles| *cycles > 0),
                    error: failure.to_string(),
                }
            }
            None if entry.cycles == Some(-1) => anyhow::bail!(
                "failed cycles sentinel lacks group failure record: tx=0x{}, block={block}",
                hex::encode(&hash)
            ),
            None => match cycles_replay_paused_by_deep_fork(&state, block)? {
                Some(fork_point) => StoredCycles::Paused {
                    total: entry.cycles.filter(|cycles| *cycles > 0),
                    fork_point,
                },
                None => StoredCycles::Pending,
            },
        }))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))
}

async fn wait_cycles_result(
    state: &Arc<AppState>,
    hash_bytes: &[u8],
    wait: &RequestWait,
) -> Result<CyclesStatusResponse, ApiRouteError> {
    let deadline = Instant::now() + state.cycles_client.wait_timeout();
    loop {
        if let Some(response) =
            stored_cycles_response(load_tx_cycles_state(state, hash_bytes).await?)
        {
            return Ok(response);
        }
        if Instant::now() >= deadline {
            return Ok(CyclesStatusResponse {
                status: CyclesStatus::Calculating,
                cycles: None,
                error: None,
            });
        }
        wait.sleep(state.cycles_client.poll_interval()).await;
    }
}

fn cycles_enqueue_response(enqueue_result: Result<(), String>) -> CyclesStatusResponse {
    match enqueue_result {
        Ok(()) => CyclesStatusResponse {
            status: CyclesStatus::Queued,
            cycles: None,
            error: None,
        },
        Err(e) => CyclesStatusResponse {
            status: CyclesStatus::Failed,
            cycles: None,
            error: Some(e),
        },
    }
}

fn lookup_tx_block_number_in_ckb_store(
    ckb_store: Option<&Arc<ckb_store_reader::CkbChainReader>>,
    hash_bytes: &[u8],
) -> Option<u64> {
    if hash_bytes.len() != 32 {
        return None;
    }

    let store = ckb_store?;
    let mut tx_hash = [0u8; 32];
    tx_hash.copy_from_slice(hash_bytes);
    store
        .get_transaction_with_block_number(&tx_hash)
        .map(|(_, block_number)| block_number)
}

fn missing_tx_lookup_error(hash_bytes: &[u8], ckb_block_number: Option<u64>) -> ApiRouteError {
    let tx_hash_hex = format!("0x{}", hex::encode(hash_bytes));
    match ckb_block_number {
        Some(block_number) => ApiError::internal(format!(
            "transaction exists in CKB RocksDB but tx index mapping is missing: tx_hash={}, block_number={}; fix indexer write/read logic, then rebuild ckbadger RocksDB and re-sync from genesis",
            tx_hash_hex, block_number
        )),
        None => ApiError::not_found("Transaction not found"),
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
    /// Main-chain block whose proposal zone carried this transaction's short id.
    /// For a proposal that arrived through an embedded uncle this is still the
    /// containing main-chain block — that is the block the commitment window is
    /// measured against — and `proposed_in_uncle` names the uncle itself.
    pub proposed_in: Option<LifecycleBlockInfo>,
    /// The embedded uncle that actually listed the short id, when the containing
    /// block's own proposal zone did not. `None` for directly proposed txs.
    pub proposed_in_uncle: Option<LifecycleUncleInfo>,
    pub committed_in: Option<LifecycleBlockInfo>,
    pub commitment_distance: Option<i64>,
    pub commitment_window: CommitmentWindow,
    pub is_cellbase: bool,
    pub confirmations: Option<i64>,
}

/// Identity of an uncle block that carried a transaction's proposal.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleUncleInfo {
    /// The uncle's own header block number (not the containing block's).
    pub block_number: i64,
    pub block_hash: String,
}

/// Where a committed transaction's proposal was found inside the
/// `[commit - 10, commit - 2]` window.
struct ProposalWindowHit {
    /// Main-chain block whose proposal zone (own or uncle-borne) carried the short id.
    block_number: i64,
    block_hash: Vec<u8>,
    timestamp: i64,
    /// `(uncle block number, uncle block hash)` when the short id came from an
    /// uncle embedded in `block_number`.
    uncle: Option<(i64, Vec<u8>)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentWindow {
    pub close: i64,
    pub far: i64,
}

impl From<ProposalWindow> for CommitmentWindow {
    fn from(window: ProposalWindow) -> Self {
        Self {
            close: window.closest(),
            far: window.farthest(),
        }
    }
}

fn find_proposal_window_hit(
    store: &ckbadger_store::CkbadgerStore,
    ckb_store: &ckb_store_reader::CkbChainReader,
    window: ProposalWindow,
    commit_block_number: i64,
    hash_bytes: &[u8],
) -> Result<Option<ProposalWindowHit>, ApiRouteError> {
    let short_hash = &hash_bytes[..10];
    let mut headers = std::collections::HashMap::new();
    let zone_of = |bn: i64| -> Result<Option<Vec<ZoneProposal>>, ApiRouteError> {
        let context = format!(
            "transaction 0x{} proposal block {bn}",
            hex::encode(hash_bytes)
        );
        let header = store
            .get_block_header(bn)
            .map_err(|error| ApiError::internal(format!("{context} header read: {error}")))?
            .ok_or_else(|| ApiError::internal(format!("{context}: missing header")))?;
        let hash: [u8; 32] = header.hash.as_slice().try_into().map_err(|_| {
            ApiError::internal(format!(
                "{context}: invalid header hash length {}",
                header.hash.len()
            ))
        })?;
        let block = ckb_store.get_block(&hash).ok_or_else(|| {
            ApiError::internal(format!(
                "{context}: block 0x{} missing from node DB",
                hex::encode(hash)
            ))
        })?;
        headers.insert(bn, (header.hash, header.timestamp));
        Ok(Some(block_proposal_zone(&block)))
    };
    let Some(hit) = earliest_proposal_in_window(window, commit_block_number, short_hash, zone_of)?
    else {
        return Ok(None);
    };
    let (block_hash, timestamp) = headers.remove(&hit.block_number).ok_or_else(|| {
        ApiError::internal(format!(
            "proposal hit missing its header: block={}",
            hit.block_number
        ))
    })?;
    Ok(Some(ProposalWindowHit {
        block_number: hit.block_number,
        block_hash,
        timestamp,
        uncle: hit.uncle.map(|uncle| (uncle.number, uncle.hash.to_vec())),
    }))
}

#[instrument(skip(state), level = "debug")]
async fn get_transaction_lifecycle(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<TransactionLifecycleResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;
    let window = state.consensus().await?.tx_proposal_window;

    let short_hash = if hash_bytes.len() >= 10 {
        hash_bytes[..10].to_vec()
    } else {
        return Err(ApiError::bad_request("Transaction hash too short"));
    };

    // Query transaction info from store
    let store = state.store.clone();
    let hash_c = hash_bytes.clone();
    let tx_result = tokio::task::spawn_blocking(move || store.get_tx_by_hash(&hash_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (commit_block_number, is_cellbase, commit_timestamp) = match tx_result {
        Some((block_num, _, entry)) => (block_num, entry.is_cellbase, entry.timestamp),
        None => {
            if let Some(tx_lookup) = fetch_transaction_lookup(&state.ckb_rpc_url, &hash)
                .await
                .map_err(ApiError::internal)?
            {
                if tx_lookup.is_pending_like() {
                    return Err(ApiError::bad_request(pending_transaction_resource_error(
                        &hash,
                        tx_lookup.status_str(),
                        "Lifecycle data",
                    )));
                }
            }
            return ok(TransactionLifecycleResponse {
                hash: format!("0x{}", hex::encode(&hash_bytes)),
                phase: LifecyclePhase::Pending,
                proposal_id: format!("0x{}", hex::encode(&short_hash)),
                proposed_in: None,
                proposed_in_uncle: None,
                committed_in: None,
                commitment_distance: None,
                commitment_window: CommitmentWindow::from(window),
                is_cellbase: false,
                confirmations: None,
            });
        }
    };

    let commit_block_hash = required_tx_block_hash(&state, commit_block_number)?;

    let hash_hex = format!("0x{}", hex::encode(&hash_bytes));
    let proposal_id_hex = format!("0x{}", hex::encode(&short_hash));

    let commit_ts_str = required_tx_timestamp(commit_timestamp, commit_block_number, 0)?;

    // Get sync tip
    let tip = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(format!("sync status unavailable: {}", e)))?
        .tip_block_number;

    let compute_confirmations = |block_num: i64| -> i64 {
        if tip >= block_num {
            tip - block_num + 1
        } else {
            tracing::warn!(
                tip,
                block_num,
                "tip < block_num for committed tx; secondary reader may be stale"
            );
            0
        }
    };

    if is_cellbase {
        return ok(TransactionLifecycleResponse {
            hash: hash_hex,
            phase: LifecyclePhase::Committed,
            proposal_id: proposal_id_hex,
            proposed_in: None,
            proposed_in_uncle: None,
            committed_in: Some(LifecycleBlockInfo {
                block_number: commit_block_number,
                block_hash: format!("0x{}", hex::encode(&commit_block_hash)),
                timestamp: commit_ts_str,
            }),
            commitment_distance: None,
            commitment_window: CommitmentWindow::from(window),
            is_cellbase: true,
            confirmations: Some(compute_confirmations(commit_block_number)),
        });
    }

    // Look for proposal block in CKB node's RocksDB
    // A tx committed in block C must be proposed in block P where:
    // C - farthest <= P <= C - closest (blocks below genesis do not exist).
    let ckb_store = state.ckb_store.clone().ok_or_else(|| {
        ApiError::internal(format!(
            "transaction {hash} proposal lookup requires the CKB node DB reader"
        ))
    })?;
    let store = state.store.clone();
    let proposed_in = tokio::task::spawn_blocking(
        move || -> Result<Option<ProposalWindowHit>, ApiRouteError> {
            find_proposal_window_hit(&store, &ckb_store, window, commit_block_number, &hash_bytes)
        },
    )
    .await
    .map_err(|error| {
        ApiError::internal(format!("transaction {hash} proposal lookup task: {error}"))
    })??;

    let (proposed_in_info, proposed_in_uncle, commitment_distance) = match proposed_in {
        Some(hit) => {
            let ts = required_tx_timestamp(hit.timestamp, hit.block_number, 0)?;
            (
                Some(LifecycleBlockInfo {
                    block_number: hit.block_number,
                    block_hash: format!("0x{}", hex::encode(&hit.block_hash)),
                    timestamp: ts,
                }),
                hit.uncle
                    .map(|(uncle_number, uncle_hash)| LifecycleUncleInfo {
                        block_number: uncle_number,
                        block_hash: format!("0x{}", hex::encode(&uncle_hash)),
                    }),
                Some(commit_block_number - hit.block_number),
            )
        }
        None => (None, None, None),
    };

    ok(TransactionLifecycleResponse {
        hash: hash_hex,
        phase: LifecyclePhase::Committed,
        proposal_id: proposal_id_hex,
        proposed_in: proposed_in_info,
        proposed_in_uncle,
        committed_in: Some(LifecycleBlockInfo {
            block_number: commit_block_number,
            block_hash: format!("0x{}", hex::encode(&commit_block_hash)),
            timestamp: commit_ts_str,
        }),
        commitment_distance,
        commitment_window: CommitmentWindow::from(window),
        is_cellbase: false,
        confirmations: Some(compute_confirmations(commit_block_number)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: the `txSize` a response serves and the `feeRate` it serves
    /// must be reproducible from each other. They were not — `feeRate` used the
    /// protocol's serialized-size-in-block while `txSize` reported the bare
    /// molecule size, so a client (and the frontend, in three places) recomputing
    /// `fee / txSize` got a different rate than the API's own field, and the
    /// size disagreed with the node and official explorer by 4 bytes.
    ///
    /// Vector: mainnet tx 0xdf615176… — molecule 981 bytes, fee 2074 shannons.
    /// The node/explorer size is 985 and the protocol fee rate is 2105.
    #[test]
    fn fee_rate_is_reproducible_from_the_served_tx_size() {
        let molecule_size = 981i32;
        let fee: u128 = 2074;

        let served_size = tx_serialized_size_in_block(molecule_size);
        assert_eq!(served_size, 985, "must match the node/explorer size");

        let served_rate = tx_fee_rate(fee, served_size);
        assert_eq!(served_rate, "2105");
        assert_eq!(
            served_rate,
            (fee * 1000 / u128::try_from(served_size).unwrap()).to_string(),
            "feeRate must equal fee * 1000 / txSize using the served txSize"
        );
    }

    #[test]
    fn test_transaction_response_serialization() {
        let resp = TransactionResponse {
            pays: None,
            output_capacity: "0".into(),
            activity: None,
            proposal_origin: None,
            value: "0".to_string(),
            hash: "0xabc".to_string(),
            block_number: 100,
            block_hash: "0xdef".to_string(),
            index: 0,
            inputs_count: 1,
            outputs_count: 2,
            fee: "1000".to_string(),
            tx_size: Some(200),
            cycles: Some(5000),
            cycles_status: "complete".into(),
            is_cellbase: false,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["blockNumber"], 100);
        assert_eq!(json["inputsCount"], 1);
        assert_eq!(json["outputsCount"], 2);
        assert_eq!(json["isCellbase"], false);
    }

    #[test]
    fn test_list_params_defaults() {
        let params: ListParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.limit, 50);
        assert!(params.block_number.is_none());
        assert!(params.cursor.is_none());
    }

    #[test]
    fn committed_input_requires_indexed_cell_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = ckbadger_store::CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let error =
            required_committed_input_cell(&store, &store, &[0x11; 32], 0, "tx=0x22 input=0")
                .unwrap_err();
        assert!(error.1 .0.message.contains("missing indexed input cell"));
    }

    #[test]
    fn committed_output_data_is_required_and_may_be_empty() {
        assert_eq!(
            required_output_data_size(&["0x".to_string()], 0, "tx 0x01").unwrap(),
            0
        );
        assert_eq!(
            required_output_data_size(&["0x1234".to_string()], 0, "tx 0x01").unwrap(),
            2
        );
        assert!(required_output_data_size(&[], 0, "tx 0x01")
            .unwrap_err()
            .1
             .0
            .message
            .contains("missing output data"));
    }

    #[test]
    fn test_hash_type_to_str() {
        assert_eq!(hash_type_to_str(0), Some("data"));
        assert_eq!(hash_type_to_str(1), Some("type"));
        assert_eq!(hash_type_to_str(2), Some("data1"));
        assert_eq!(hash_type_to_str(4), Some("data2"));
        assert_eq!(hash_type_to_str(99), None);
    }

    #[test]
    fn test_hash_type_byte_to_i16() {
        assert_eq!(hash_type_byte_to_i16(0), 0);
        assert_eq!(hash_type_byte_to_i16(1), 1);
        assert_eq!(hash_type_byte_to_i16(2), 2);
        assert_eq!(hash_type_byte_to_i16(4), 4);
        assert_eq!(hash_type_byte_to_i16(255), 0);
    }

    #[test]
    fn test_parse_hash_type_label_to_i16() {
        assert_eq!(parse_hash_type_label_to_i16("data").unwrap(), 0);
        assert_eq!(parse_hash_type_label_to_i16("type").unwrap(), 1);
        assert_eq!(parse_hash_type_label_to_i16("data1").unwrap(), 2);
        assert_eq!(parse_hash_type_label_to_i16("data2").unwrap(), 4);
    }

    #[test]
    fn test_parse_hash_type_label_to_i16_rejects_unknown() {
        let err = parse_hash_type_label_to_i16("unknown").unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("unknown script hash_type label in CKB store"));
    }

    #[test]
    fn test_decode_hex_bytes_with_context_rejects_invalid_hex() {
        let err =
            decode_hex_bytes_with_context("0xzz", "lock.args", "unit-test", None).unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("invalid hex for lock.args while unit-test"));
    }

    #[test]
    fn test_decode_hex_bytes_with_context_rejects_len_mismatch() {
        let err = decode_hex_bytes_with_context("0x1234", "lock.code_hash", "unit-test", Some(32))
            .unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("invalid byte length for lock.code_hash while unit-test"));
    }

    #[test]
    fn test_parse_u64_hex_field_with_context_rejects_invalid_hex() {
        let err = parse_u64_hex_field_with_context("0x-not-hex", "output.capacity", "unit-test")
            .unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("invalid hex u64 for output.capacity while unit-test"));
    }

    #[test]
    fn test_transaction_input_response_serializes_type_script() {
        let input = TransactionInputResponse {
            display: None,
            used_capacity: Some("61".into()),
            cell_type: Some("plain".into()),
            previous_output: Some(PreviousOutput {
                tx_hash: "0x01".to_string(),
                index: 0,
            }),
            since: "0x0".to_string(),
            capacity: Some("100".to_string()),
            lock: Some(ScriptResponse {
                code_hash: "0x02".to_string(),
                hash_type: "type".to_string(),
                args: "0x".to_string(),
            }),
            r#type: Some(ScriptResponse {
                code_hash: "0x03".to_string(),
                hash_type: "data1".to_string(),
                args: "0x11".to_string(),
            }),
            address: Some("ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgpl6m0j".to_string()),
        };

        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["type"]["codeHash"], "0x03");
        assert_eq!(json["type"]["hashType"], "data1");
    }

    #[test]
    fn test_transaction_detail_response_serializes_witness_fields() {
        let detail = TransactionDetailResponse {
            pays: None,
            fee_paid_by: vec![],
            hash: "0xabc".to_string(),
            status: "committed".to_string(),
            pool_status: None,
            interpretation: None,
            pending_since: None,
            block_number: Some(100),
            block_hash: Some("0xdef".to_string()),
            index: Some(0),
            inputs_count: 1,
            outputs_count: 1,
            fee: "42".to_string(),
            fee_rate: Some("1000".to_string()),
            tx_size: Some(123),
            cycles: Some(456),
            cycles_status: "complete".into(),
            confirmations: Some(7),
            is_cellbase: false,
            timestamp: Some("2024-01-01T00:00:00Z".to_string()),
            inputs_capacity: Some("100".to_string()),
            outputs_capacity: Some("58".to_string()),
            inputs_used_capacity: Some("10".to_string()),
            outputs_used_capacity: Some("9".to_string()),
            inputs: vec![],
            outputs: vec![],
            script_groups: vec![],
            witness_details: vec![],
            witnesses: vec!["0x".to_string(), "0x1234".to_string()],
            witnesses_available: true,
            dotcell_names: vec![],
            header_deps: vec![],
            cellbase_witness: None,
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["status"], "committed");
        assert_eq!(json["witnessesAvailable"], true);
        assert_eq!(json["witnesses"][0], "0x");
        assert_eq!(json["witnesses"][1], "0x1234");
    }

    #[test]
    fn test_commitment_window_from_consensus_window() {
        let window = CommitmentWindow::from(ProposalWindow::new(3, 12).unwrap());
        assert_eq!(window.close, 3);
        assert_eq!(window.far, 12);
    }

    #[test]
    fn test_cell_dep_response_serialization() {
        let resp = CellDepResponse {
            resolved_cells: vec![],
            script_hashes: vec![],
            out_point_tx_hash: "0xabc".to_string(),
            out_point_index: 0,
            dep_type: "code".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["outPointTxHash"], "0xabc");
        assert_eq!(json["depType"], "code");
    }

    #[test]
    fn test_lifecycle_phase_serialization() {
        let pending = serde_json::to_value(&LifecyclePhase::Pending).unwrap();
        assert_eq!(pending, "pending");
        let committed = serde_json::to_value(&LifecyclePhase::Committed).unwrap();
        assert_eq!(committed, "committed");
    }

    #[test]
    fn stored_complete_cycles_keep_exact_value_including_zero() {
        for cycles in [0, 42] {
            let response = stored_cycles_response(Some(StoredCycles::Complete(cycles))).unwrap();
            assert_eq!(response.status, CyclesStatus::Done);
            assert_eq!(response.cycles, Some(cycles));
            assert!(response.error.is_none());
        }
        assert!(stored_cycles_response(Some(StoredCycles::Pending)).is_none());
    }

    #[test]
    fn stored_failed_cycles_and_missing_transactions_are_explicit() {
        for total in [Some(333), None] {
            let failed = stored_cycles_response(Some(StoredCycles::Failed {
                total,
                error: "script verification failed".into(),
            }))
            .unwrap();
            assert_eq!(failed.status, CyclesStatus::Failed);
            assert_eq!(failed.cycles, total);
            assert_eq!(failed.error.as_deref(), Some("script verification failed"));
        }
        let missing = stored_cycles_response(None).unwrap();
        assert_eq!(missing.status, CyclesStatus::NotFound);
        assert_eq!(missing.cycles, None);
    }

    #[test]
    fn test_cycles_enqueue_response_queued() {
        let response = cycles_enqueue_response(Ok(()));
        assert_eq!(response.status, CyclesStatus::Queued);
        assert_eq!(response.cycles, None);
        assert!(response.error.is_none());
    }

    #[test]
    fn test_cycles_enqueue_response_failed() {
        let response = cycles_enqueue_response(Err("enqueue failed".to_string()));
        assert_eq!(response.status, CyclesStatus::Failed);
        assert_eq!(response.cycles, None);
        assert_eq!(response.error.unwrap_or_default(), "enqueue failed");
    }

    #[test]
    fn test_missing_tx_lookup_error_returns_not_found_when_ckb_not_found() {
        let hash = vec![0xabu8; 32];
        let (status, body) = missing_tx_lookup_error(&hash, None);
        assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
        assert_eq!(body.0.error, "not_found");
        assert_eq!(body.0.message, "Transaction not found");
    }

    #[test]
    fn test_missing_tx_lookup_error_returns_internal_with_context_when_ckb_found() {
        let hash = vec![0xcdu8; 32];
        let (status, body) = missing_tx_lookup_error(&hash, Some(42));
        assert_eq!(status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.0.error, "internal_error");
        assert!(body.0.message.contains("tx index mapping is missing"));
        assert!(body.0.message.contains("tx_hash=0x"));
        assert!(body.0.message.contains("block_number=42"));
    }

    #[test]
    fn test_occupied_capacity_bytes_without_type_script() {
        let occ = occupied_capacity_bytes(20, None, 64);
        assert_eq!(occ, 8 + 32 + 1 + 20 + 64);
    }

    #[test]
    fn test_occupied_capacity_bytes_includes_type_script_size() {
        let occ = occupied_capacity_bytes(20, Some(16), 64);
        assert_eq!(occ, 8 + 32 + 1 + 20 + (32 + 1 + 16) + 64);
    }

    #[test]
    fn test_compute_tx_fee_from_io_for_regular_tx() {
        let fee = compute_tx_fee_from_io(1_000, 0, 950, false, Some(10), &[0x11; 32]).unwrap();
        assert_eq!(fee, 50);
    }

    #[test]
    fn test_compute_tx_fee_from_io_adds_dao_compensation_to_inputs() {
        // Outputs exceed raw inputs because the withdrawal pays compensation.
        let fee = compute_tx_fee_from_io(1_000, 150, 1_100, false, None, &[0x22; 32]).unwrap();
        assert_eq!(fee, 50);
        // Compensation smaller than the fee: raw inputs still exceed outputs,
        // and the compensation must STILL be counted, not dropped.
        let fee = compute_tx_fee_from_io(1_000, 10, 950, false, None, &[0x22; 32]).unwrap();
        assert_eq!(fee, 60);
    }

    #[test]
    fn test_compute_tx_fee_from_io_errors_when_non_dao_outputs_exceed_inputs() {
        let err =
            compute_tx_fee_from_io(1_000, 0, 1_100, false, Some(10), &[0x33; 32]).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.1 .0.message.contains("inputs/outputs invariant broken"));
    }

    /// A script replay that failed is persisted as the `-1` marker and must be
    /// served as `failed` — never as a `done` cycle count. Guards the surface
    /// that reported an aborted Nervos DAO script group as an authoritative
    /// total.
    #[test]
    fn test_derive_cycles_status_never_reports_failed_replay_as_done() {
        for (stored, cellbase, value, status) in [
            (Some(-1), false, None, "failed"),
            (None, false, None, "pending"),
            (Some(0), false, None, "pending"),
            (Some(3_380_228), false, Some(3_380_228), "complete"),
            (None, true, Some(0), "complete"),
        ] {
            assert_eq!(
                display_cycles(stored, cellbase, &[0x11; 32]).unwrap(),
                (value, status.into())
            );
        }
        let error = display_cycles(Some(-2), false, &[0x11; 32]).unwrap_err();
        assert!(error.1 .0.message.contains("invalid stored cycles"));
        assert!(error.1 .0.message.contains(&hex::encode([0x11; 32])));
    }

    #[test]
    fn execution_exempt_cycles_reject_nonzero_state() {
        for cycles in [-1, 1, 42] {
            let error = display_cycles(Some(cycles), true, &[0x71; 32]).unwrap_err();
            assert!(error.1 .0.message.contains("execution-exempt"));
            assert!(error.1 .0.message.contains(&hex::encode([0x71; 32])));
        }
        assert_eq!(
            display_cycles(Some(0), true, &[0x71; 32]).unwrap(),
            (Some(0), "complete".into())
        );
    }

    // ── `.cell` records on the tx page ───────────────────────────────────
    // Chain data copied from `crates/indexer/src/parser/dotcell_fixtures.rs`
    // (`T2_REGISTER_JOAOM`, testnet tx 0x89191ea4…386b at block 22471181;
    // `M1_RING_ROOT`, mainnet tx 0x219d1540…15ed), node-verified 2026-09-24.
    // That module is `#[cfg(test)]` in the indexer crate and not reachable here.

    const T2_TX_HASH: &str = "0x89191ea4bae150f82521140968fae918040e724f647eadec02e02077d748386b";
    /// `ACCOUNT_LOCK_CODE_HASH_TESTNET`.
    const T2_ACCOUNT_LOCK: &str =
        "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd";
    /// `ACCOUNT_TYPE_CODE_HASH_TESTNET`.
    const T2_ACCOUNT_TYPE: &str =
        "0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9";
    /// `NAMESPACE_ARGS_TESTNET`.
    const T2_NAMESPACE_ARGS: &str = "0x2510c78057479c9b023fe6e98ce43979e92a1353";
    /// `T2_OUT0_DATA`: `maria.cell`, whose records are in witness 0.
    const T2_OUT0_DATA: &str = "0x033b339494bd0e29b77c0dca959bc4d6d87e4ac232bd7df9c1335163fe85f5eb18241e3586a41eb75dd6d68bf555acea74d6649ed529da856c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b1416d61726961";
    /// `T2_WITNESS_0`: WitnessArgs whose output_type is maria's six records.
    const T2_WITNESS_0: &str = "0xc901000010000000100000001c000000080000007265676973746572a901000006000b616464726573732e333039006400636b7431717266727763646e76737373776477706e337339763866703837656d617433303663746a77736d336e6d6c6b6a673871797a61326371677171397837357a75346c37676c64363036723665796430306d346c7a79337a6b786b71346e79777a752c01000009616464726573732e30003e00746231703237767464306a776d3235746478336d36746763757168346c6578756c3076736c7572683079637a633766393272667030736b737264737936742c0100000a616464726573732e3630002a003078656646324634436132444536656444363364416665343833383536463134453639346437333144442c0100000d70726f66696c652e656d61696c0011006d61726961406578616d706c652e636f6d2c0100000d70726f66696c652e70686f6e650010002b3335312039313220333435203637382c0100000a647765622e636b626673004800636b6266733a2f2f346266306362646261633066386538656231616664646534333963336263306234333731623366336165636538633730343866656630366566366563376231302c010000";
    /// `M1_OUT0_DATA`: the mainnet ring root (empty label).
    const M1_RING_ROOT_DATA: &str = "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e20000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
    /// `M1_RING_ROOT.witnesses[0]`: a secp witness whose output_type is the
    /// empty records payload the root's records hash commits to.
    const M1_WITNESS_0: &str = "0x5b00000010000000550000005500000041000000fee51268d7edab7259e6f1fb460d2d523859de3e132f0910daa784ae288bc6295b666e226cca500c490e193bf5a412d68f3b5d1dbe9c7695c5934d2954420db500020000000000";

    fn rpc_script(code_hash: &str, args: &str) -> ckb_store_reader::RpcScript {
        ckb_store_reader::RpcScript {
            code_hash: code_hash.to_string(),
            hash_type: "type".to_string(),
            args: args.to_string(),
        }
    }

    fn rpc_tx(
        outputs: Vec<(ckb_store_reader::RpcCellOutput, &str)>,
        witnesses: &[&str],
    ) -> RpcTransactionView {
        RpcTransactionView {
            hash: T2_TX_HASH.to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs_data: outputs.iter().map(|(_, data)| data.to_string()).collect(),
            outputs: outputs.into_iter().map(|(output, _)| output).collect(),
            witnesses: witnesses.iter().map(|w| w.to_string()).collect(),
        }
    }

    fn name_output(
        lock: &str,
        type_code_hash: &str,
        args: &str,
    ) -> ckb_store_reader::RpcCellOutput {
        ckb_store_reader::RpcCellOutput {
            capacity: "0x59682f000".to_string(),
            lock: rpc_script(lock, "0x"),
            type_: Some(rpc_script(type_code_hash, args)),
        }
    }

    #[test]
    fn test_dotcell_names_from_rpc_tx() {
        let tx = rpc_tx(
            vec![(
                name_output(T2_ACCOUNT_LOCK, T2_ACCOUNT_TYPE, T2_NAMESPACE_ARGS),
                T2_OUT0_DATA,
            )],
            &[T2_WITNESS_0],
        );

        let names = dotcell_names_for_tx(&tx).expect("maria's records decode");
        assert_eq!(names.len(), 1);
        let maria = &names[0];
        assert_eq!(maria.output_index, 0);
        assert_eq!(maria.label, "maria");
        assert_eq!(maria.name, "maria.cell");
        // blake2b("maria")[..20] under CKB's default personalization.
        assert_eq!(
            maria.identity_id,
            "0x2224948f63975a7a0741139cd5d2a45b9fb02c03"
        );
        // The hash the cell carries in data[1..33].
        assert_eq!(maria.records_hash, format!("0x{}", &T2_OUT0_DATA[4..68]));
        assert_eq!(maria.records.len(), 6);
        assert_eq!(maria.records[0].key, "address.309");
        let decoded = maria.records[0]
            .decoded_address
            .as_ref()
            .expect("address.309 is a CKB address");
        assert!(decoded.address.starts_with("ckt1"), "{}", decoded.address);
        assert_eq!(maria.records[5].key, "dweb.ckbfs");
        assert!(maria.records[5]
            .value_utf8
            .as_deref()
            .is_some_and(|v| v.starts_with("ckbfs://")));
    }

    #[test]
    fn test_dotcell_names_hash_mismatch_is_an_error() {
        // One payload byte changed (the last record's ttl): the payload no
        // longer hashes to what the cell commits to.
        let tampered = format!("{}1", &T2_WITNESS_0[..T2_WITNESS_0.len() - 1]);
        let tx = rpc_tx(
            vec![(
                name_output(T2_ACCOUNT_LOCK, T2_ACCOUNT_TYPE, T2_NAMESPACE_ARGS),
                T2_OUT0_DATA,
            )],
            &[&tampered],
        );

        let err = dotcell_names_for_tx(&tx).unwrap_err();
        let message = err.1 .0.message;
        assert!(message.contains("records hash mismatch"), "{message}");
        assert!(message.contains("output_index=0"), "{message}");
    }

    #[test]
    fn test_dotcell_names_empty_for_plain_tx() {
        let plain = ckb_store_reader::RpcCellOutput {
            capacity: "0x2540be400".to_string(),
            lock: ckb_store_reader::RpcScript {
                code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                    .to_string(),
                hash_type: "type".to_string(),
                args: "0x23870b08ec5f6260c50a63646170d61e26d155c7".to_string(),
            },
            type_: None,
        };
        let tx = rpc_tx(vec![(plain, "0x")], &[]);

        assert!(dotcell_names_for_tx(&tx).unwrap().is_empty());
    }

    /// The ring root is protocol infrastructure, not a name: its (empty)
    /// records payload is still verified against its hash, but it is not
    /// listed as a name with an identity.
    #[test]
    fn test_dotcell_names_skip_the_ring_root_after_verifying_it() {
        let root = name_output(
            "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            "0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54",
            "0xb4f4302965b7d6421481a520ee7eb5971a5e808c",
        );
        let tx = rpc_tx(vec![(root.clone(), M1_RING_ROOT_DATA)], &[M1_WITNESS_0]);
        assert!(dotcell_names_for_tx(&tx).unwrap().is_empty());

        // A root with no witness at its index is still an error.
        let tx = rpc_tx(vec![(root, M1_RING_ROOT_DATA)], &[]);
        let err = dotcell_names_for_tx(&tx).unwrap_err();
        assert!(
            err.1 .0.message.contains("output_index=0"),
            "{}",
            err.1 .0.message
        );
    }
}
