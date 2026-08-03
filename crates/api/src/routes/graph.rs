use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckb_types::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::routes::proposal_window::{resolve_committed_txs, PROPOSAL_W_CLOSE, PROPOSAL_W_FAR};
use crate::routes::tx_lookup::{fetch_transaction_lookup, pending_transaction_resource_error};
use crate::utils::{parse_hash32, parse_output_index, validate_block_number};
use crate::AppState;
use tracing::instrument;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/graph/cell/{tx_hash}/{output_index}", get(get_cell_graph))
        .route("/graph/transaction/{hash}", get(get_tx_graph))
        .route("/graph/proposals/{block_number}", get(get_proposal_graph))
}

#[derive(Debug, Deserialize)]
pub struct GraphParams {
    #[serde(default = "default_depth")]
    depth: i32,
}

fn default_depth() -> i32 {
    2
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    pub node_type: String,
    pub label: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphLink {
    pub source: String,
    pub target: String,
    pub link_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphResponse {
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
}

/// Get the inputs for a transaction by reading the raw CKB block data.
/// Returns Vec<(prev_tx_hash, prev_output_index)> for non-cellbase transactions.
fn get_tx_inputs_from_ckb_store(
    ckb_store: &ckb_store_reader::CkbChainReader,
    tx_hash: &[u8],
) -> Vec<(Vec<u8>, i16)> {
    if tx_hash.len() != 32 {
        return Vec::new();
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(tx_hash);

    let tx_view = match ckb_store.get_transaction(&hash) {
        Some(tv) => tv,
        None => return Vec::new(),
    };

    let inputs = tx_view.inputs();
    let mut result = Vec::with_capacity(inputs.len());
    for i in 0..inputs.len() {
        let input = inputs.get(i).unwrap();
        let prev_outpoint = input.previous_output();
        let prev_tx_hash: [u8; 32] = prev_outpoint.tx_hash().unpack();
        let prev_index: u32 = prev_outpoint.index().unpack();
        // Skip null outpoints (cellbase inputs)
        if prev_tx_hash == [0u8; 32] {
            continue;
        }
        result.push((prev_tx_hash.to_vec(), prev_index as i16));
    }
    result
}

/// Get the outputs for a transaction by reading the raw CKB block data.
/// Returns Vec<(output_index, capacity)>.
fn get_tx_outputs_from_ckb_store(
    ckb_store: &ckb_store_reader::CkbChainReader,
    tx_hash: &[u8],
) -> Vec<(i16, i64)> {
    if tx_hash.len() != 32 {
        return Vec::new();
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(tx_hash);

    let tx_view = match ckb_store.get_transaction(&hash) {
        Some(tv) => tv,
        None => return Vec::new(),
    };

    let outputs = tx_view.outputs();
    let mut result = Vec::with_capacity(outputs.len());
    for i in 0..outputs.len() {
        let output = outputs.get(i).unwrap();
        let capacity: u64 = output.capacity().unpack();
        result.push((i as i16, capacity as i64));
    }
    result
}

fn append_consumed_by_relation(
    nodes: &mut Vec<GraphNode>,
    links: &mut Vec<GraphLink>,
    cell_id: &str,
    consumed_by_tx: Option<&[u8]>,
    consumed_at_block: i64,
) {
    let Some(consumed_by_tx) = consumed_by_tx else {
        return;
    };

    let consumed_hash = format!("0x{}", hex::encode(consumed_by_tx));
    let consumed_tx_id = format!("tx-{}", consumed_hash);
    let block_number = if consumed_at_block > 0 {
        Some(consumed_at_block)
    } else {
        None
    };

    if !nodes.iter().any(|n| n.id == consumed_tx_id) {
        nodes.push(GraphNode {
            id: consumed_tx_id.clone(),
            node_type: "transaction".to_string(),
            label: format!(
                "TX ...{}",
                &consumed_hash[consumed_hash.len().saturating_sub(8)..]
            ),
            data: serde_json::json!({
                "hash": consumed_hash,
                "blockNumber": block_number,
            }),
        });
    }

    if !links
        .iter()
        .any(|l| l.source == cell_id && l.target == consumed_tx_id && l.link_type == "consumed_by")
    {
        links.push(GraphLink {
            source: cell_id.to_string(),
            target: consumed_tx_id,
            link_type: "consumed_by".to_string(),
        });
    }
}

#[instrument(skip(state), level = "debug")]
async fn get_cell_graph(
    State(state): State<Arc<AppState>>,
    Path((tx_hash, output_index)): Path<(String, i32)>,
    Query(params): Query<GraphParams>,
) -> ApiResult<GraphResponse> {
    let hash_bytes = parse_hash32(&tx_hash, "tx_hash")?;

    let mut nodes = Vec::new();
    let mut links = Vec::new();
    let depth = params.depth.clamp(1, 5);

    // Narrowed before it is echoed back: the old `as i16` wrapped, so a request
    // for output 65536 was answered with output 0's capacity and block while
    // still reporting `"outputIndex": 65536`.
    let output_idx = parse_output_index(output_index, "output index")?;

    let cell_id = format!("cell-{}-{}", tx_hash, output_index);

    // Look up the transaction to verify it exists
    let store = state.store.clone();
    let ao_store = state.append_only_store.clone();
    let hash_c = hash_bytes.clone();
    let (tx_info, live_cell, consumed_cell) =
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let tx_info = store.get_tx_location(&hash_c)?;
            let live_cell = store.get_cell(&hash_c, output_idx, &ao_store)?;
            let consumed_cell = if live_cell.is_none() {
                store.get_consumed_cell_info(&hash_c, output_idx, &ao_store)?
            } else {
                None
            };
            Ok((tx_info, live_cell, consumed_cell))
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let block_number = match tx_info {
        Some((bn, _)) => bn,
        None => return Err(ApiError::not_found("Transaction not found")),
    };

    let (cell_info, status_str, consumed_by_tx, consumed_at_block): (
        ckbadger_store::PositionedCellInfo,
        &str,
        Option<Vec<u8>>,
        i64,
    ) = match (live_cell, consumed_cell) {
        (Some(info), _) => (info, "live", None, 0),
        (None, Some(info)) => (
            info.to_positioned_cell_info(),
            "dead",
            info.consumed_by_tx,
            info.consumed_at_block,
        ),
        (None, None) => return Err(ApiError::not_found("Cell not found")),
    };

    let capacity_str = cell_info.capacity.to_string();

    nodes.push(GraphNode {
        id: cell_id.clone(),
        node_type: "cell".to_string(),
        label: format!("{} CKB", parse_capacity(&capacity_str)),
        data: serde_json::json!({
            "txHash": format!("0x{}", hex::encode(&hash_bytes)),
            "outputIndex": output_index,
            "capacity": capacity_str,
            "status": status_str,
            "createdAtBlock": cell_info.created_at_block,
            "consumedAtBlock": if consumed_at_block > 0 { Some(consumed_at_block) } else { None },
        }),
    });

    let created_tx_id = format!("tx-{}", tx_hash);
    nodes.push(GraphNode {
        id: created_tx_id.clone(),
        node_type: "transaction".to_string(),
        label: format!("TX ...{}", &tx_hash[tx_hash.len().saturating_sub(8)..]),
        data: serde_json::json!({
            "hash": tx_hash,
            "blockNumber": block_number,
        }),
    });

    links.push(GraphLink {
        source: created_tx_id.clone(),
        target: cell_id.clone(),
        link_type: "creates".to_string(),
    });

    // Get inputs of the creating transaction (depth > 1)
    if depth > 1 {
        if let Some(ref ckb_store) = state.ckb_store {
            let inputs = get_tx_inputs_from_ckb_store(ckb_store, &hash_bytes);

            if !inputs.is_empty() {
                let store = state.store.clone();
                let ao_store = state.append_only_store.clone();
                let inputs_c = inputs.clone();
                let (cell_map, consumed_map) =
                    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                        let outpoints: Vec<(&[u8], i16)> =
                            inputs_c.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                        let cell_map = store.get_cells_batch(&outpoints, &ao_store)?;
                        let consumed_map = store.get_consumed_cells_batch(&outpoints, &ao_store)?;
                        Ok((cell_map, consumed_map))
                    })
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?
                    .map_err(|e| ApiError::internal(e.to_string()))?;

                for (prev_tx_hash, prev_idx) in &inputs {
                    let prev_cell_id = format!("cell-0x{}-{}", hex::encode(prev_tx_hash), prev_idx);

                    let label = cell_map
                        .get(&(prev_tx_hash.clone(), *prev_idx))
                        .or_else(|| consumed_map.get(&(prev_tx_hash.clone(), *prev_idx)))
                        .map(|c| format!("{} CKB", parse_capacity(&c.capacity.to_string())))
                        .unwrap_or_else(|| "?".to_string());

                    nodes.push(GraphNode {
                        id: prev_cell_id.clone(),
                        node_type: "cell".to_string(),
                        label,
                        data: serde_json::json!({
                            "txHash": format!("0x{}", hex::encode(prev_tx_hash)),
                            "outputIndex": prev_idx,
                            "status": "dead",
                        }),
                    });

                    links.push(GraphLink {
                        source: prev_cell_id,
                        target: created_tx_id.clone(),
                        link_type: "consumed_by".to_string(),
                    });
                }
            }
        }
    }

    if status_str == "dead" {
        append_consumed_by_relation(
            &mut nodes,
            &mut links,
            &cell_id,
            consumed_by_tx.as_deref(),
            consumed_at_block,
        );
    }

    ok(GraphResponse { nodes, links })
}

#[instrument(skip(state), level = "debug")]
async fn get_tx_graph(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
    Query(_params): Query<GraphParams>,
) -> ApiResult<GraphResponse> {
    let hash_bytes = parse_hash32(&hash, "transaction hash")?;

    let mut nodes = Vec::new();
    let mut links = Vec::new();

    // Look up transaction
    let store = state.store.clone();
    let hash_c = hash_bytes.clone();
    let tx_result = tokio::task::spawn_blocking(move || store.get_tx_by_hash(&hash_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (block_number, _tx_idx, tx_entry) = match tx_result {
        Some(info) => info,
        None => {
            if let Some(tx_lookup) = fetch_transaction_lookup(&state.ckb_rpc_url, &hash)
                .await
                .map_err(ApiError::internal)?
            {
                if tx_lookup.is_pending_like() {
                    return Err(ApiError::bad_request(pending_transaction_resource_error(
                        &hash,
                        tx_lookup.status_str(),
                        "Graph data",
                    )));
                }
            }
            return Err(ApiError::not_found("Transaction not found"));
        }
    };

    let tx_id = format!("tx-{}", hash);

    nodes.push(GraphNode {
        id: tx_id.clone(),
        node_type: "transaction".to_string(),
        label: format!("TX ...{}", &hash[hash.len().saturating_sub(8)..]),
        data: serde_json::json!({
            "hash": hash,
            "blockNumber": block_number,
            "fee": tx_entry.fee.to_string(),
            "isCellbase": tx_entry.is_cellbase,
        }),
    });

    // Get inputs (unless cellbase)
    if !tx_entry.is_cellbase {
        if let Some(ref ckb_store) = state.ckb_store {
            let inputs = get_tx_inputs_from_ckb_store(ckb_store, &hash_bytes);

            if !inputs.is_empty() {
                // Batch lookup cells
                let store = state.store.clone();
                let ao_store = state.append_only_store.clone();
                let inputs_c = inputs.clone();
                let (live_map, consumed_map) =
                    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                        let outpoints: Vec<(&[u8], i16)> =
                            inputs_c.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                        let live_map = store.get_cells_batch(&outpoints, &ao_store)?;
                        let consumed_map = store.get_consumed_cells_batch(&outpoints, &ao_store)?;
                        Ok((live_map, consumed_map))
                    })
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?
                    .map_err(|e| ApiError::internal(e.to_string()))?;

                for (prev_tx_hash, prev_idx) in inputs {
                    let prev_tx_hex = format!("0x{}", hex::encode(&prev_tx_hash));
                    let input_cell_id = format!("cell-{}-{}", prev_tx_hex, prev_idx);

                    let label = live_map
                        .get(&(prev_tx_hash.clone(), prev_idx))
                        .or_else(|| consumed_map.get(&(prev_tx_hash.clone(), prev_idx)))
                        .map(|c| format!("{} CKB", parse_capacity(&c.capacity.to_string())))
                        .unwrap_or_else(|| "?".to_string());

                    nodes.push(GraphNode {
                        id: input_cell_id.clone(),
                        node_type: "cell".to_string(),
                        label,
                        data: serde_json::json!({
                            "txHash": prev_tx_hex,
                            "outputIndex": prev_idx,
                            "status": "dead",
                        }),
                    });

                    links.push(GraphLink {
                        source: input_cell_id,
                        target: tx_id.clone(),
                        link_type: "input".to_string(),
                    });
                }
            }
        }
    }

    // Get outputs from CKB store (most accurate) or from ckbadger-store cells
    if let Some(ref ckb_store) = state.ckb_store {
        let outputs = get_tx_outputs_from_ckb_store(ckb_store, &hash_bytes);

        if !outputs.is_empty() {
            // Batch check which outputs are live
            let store = state.store.clone();
            let ao_store = state.append_only_store.clone();
            let hash_c = hash_bytes.clone();
            let output_indices: Vec<i16> = outputs.iter().map(|(idx, _)| *idx).collect();
            let live_set = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                let mut live = std::collections::HashSet::new();
                for idx in &output_indices {
                    if store.get_cell(&hash_c, *idx, &ao_store)?.is_some() {
                        live.insert(*idx);
                    }
                }
                Ok(live)
            })
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

            for (output_index, capacity) in outputs {
                let output_cell_id = format!("cell-{}-{}", hash, output_index);
                let capacity_str = capacity.to_string();
                let is_live = live_set.contains(&output_index);

                nodes.push(GraphNode {
                    id: output_cell_id.clone(),
                    node_type: "cell".to_string(),
                    label: format!("{} CKB", parse_capacity(&capacity_str)),
                    data: serde_json::json!({
                        "txHash": hash,
                        "outputIndex": output_index,
                        "capacity": capacity_str,
                        "status": if is_live { "live" } else { "dead" },
                    }),
                });

                links.push(GraphLink {
                    source: tx_id.clone(),
                    target: output_cell_id,
                    link_type: "output".to_string(),
                });
            }
        }
    } else {
        // Fallback: look up live cells created by this tx from the store.
        // This won't show consumed outputs.
        let store = state.store.clone();
        let ao_store = state.append_only_store.clone();
        let hash_c = hash_bytes.clone();
        let live_cells = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let iter = store.iterator_cf(
                store.cf_live_cells(),
                rocksdb::IteratorMode::From(&hash_c, rocksdb::Direction::Forward),
            );
            let mut results = Vec::new();
            for item in iter.flatten() {
                let (key, _) = item;
                if !live_cell_key_matches_tx_hash(&key, &hash_c) {
                    break;
                }
                let output_index = i16::from_be_bytes([key[32], key[33]]);
                if let Some(info) = store.get_live_cell_by_outpoint_key(&key, &ao_store)? {
                    results.push((output_index, info));
                }
            }
            Ok(results)
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

        for (output_index, info) in live_cells {
            let output_cell_id = format!("cell-{}-{}", hash, output_index);
            let capacity_str = info.capacity.to_string();

            nodes.push(GraphNode {
                id: output_cell_id.clone(),
                node_type: "cell".to_string(),
                label: format!("{} CKB", parse_capacity(&capacity_str)),
                data: serde_json::json!({
                    "txHash": hash,
                    "outputIndex": output_index,
                    "capacity": capacity_str,
                    "status": "live",
                }),
            });

            links.push(GraphLink {
                source: tx_id.clone(),
                target: output_cell_id,
                link_type: "output".to_string(),
            });
        }
    }

    ok(GraphResponse { nodes, links })
}

fn live_cell_key_matches_tx_hash(key: &[u8], tx_hash: &[u8]) -> bool {
    key.len() >= 34 && tx_hash.len() == 32 && &key[..32] == tx_hash
}

fn parse_capacity(capacity: &str) -> String {
    let ckb = capacity.parse::<u64>().unwrap_or(0) as f64 / 1e8;
    if ckb >= 1_000_000.0 {
        format!("{:.2}M", ckb / 1_000_000.0)
    } else if ckb >= 1_000.0 {
        format!("{:.2}K", ckb / 1_000.0)
    } else {
        format!("{:.2}", ckb)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalGraphResponse {
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
    pub metadata: ProposalGraphMetadata,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalGraphMetadata {
    pub source_block: i64,
    pub total_proposals: i32,
    pub committed_count: i32,
    pub commitment_window: ProposalCommitmentWindow,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalCommitmentWindow {
    pub close: i64,
    pub far: i64,
    pub earliest_commit_block: i64,
    pub latest_commit_block: i64,
}

/// One entry of a block's proposal zone: the short id, plus the uncle that
/// carried it when it did not come from the block's own `proposals()`.
struct ZoneProposal {
    short_id: Vec<u8>,
    /// `(uncle block number, uncle block hash)`; `None` for a direct proposal.
    uncle: Option<(i64, Vec<u8>)>,
}

/// The whole proposal zone CKB consensus attributes to `block`: its own
/// `proposals()` plus the proposal zones of the uncles it embeds.
///
/// A proposal borne by an uncle belongs to the main-chain block that embeds
/// the uncle — that is the block whose commitment window the proposal opens —
/// which is the same rule `/transactions/{hash}/lifecycle` applies. Ignoring
/// uncle zones dropped those proposals from the graph entirely and undercounted
/// `totalProposals`.
///
/// The block's own zone wins over its uncles' when both carry an id, and an id
/// listed by two embedded uncles is one proposal, attributed to the first uncle
/// in embedding order. The uncle data already travels inside the block, so this
/// costs no extra I/O.
fn block_proposal_zone(block: &ckb_types::core::BlockView) -> Vec<ZoneProposal> {
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut zone = Vec::new();

    for proposal_id in block.data().proposals().into_iter() {
        let short_id = proposal_id.raw_data().to_vec();
        if seen.insert(short_id.clone()) {
            zone.push(ZoneProposal {
                short_id,
                uncle: None,
            });
        }
    }

    for uncle in block.uncles() {
        let uncle_number = uncle.number() as i64;
        let uncle_hash = uncle.hash().raw_data().to_vec();
        for proposal_id in uncle.data().proposals().into_iter() {
            let short_id = proposal_id.raw_data().to_vec();
            if seen.insert(short_id.clone()) {
                zone.push(ZoneProposal {
                    short_id,
                    uncle: Some((uncle_number, uncle_hash.clone())),
                });
            }
        }
    }

    zone
}

#[instrument(skip(state), level = "debug")]
async fn get_proposal_graph(
    State(state): State<Arc<AppState>>,
    Path(block_number): Path<i64>,
) -> ApiResult<ProposalGraphResponse> {
    let block_number = validate_block_number(block_number, "block number")?;
    let store = state.store.clone();
    let block_header = tokio::task::spawn_blocking(move || store.get_block_header(block_number))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Block not found"))?;

    let block_hash = block_header.hash;

    // NC-Max: w_close=2, w_far=10 (proposals can commit 2-10 blocks later)
    let earliest_commit = block_number + PROPOSAL_W_CLOSE;
    let latest_commit = block_number + PROPOSAL_W_FAR;

    // Get the block's full proposal zone (own + embedded uncles') from CKB store
    let proposals: Vec<ZoneProposal> = if let Some(ref ckb_store) = state.ckb_store {
        if let Some(block_view) = ckb_store.get_block_by_number(block_number as u64) {
            block_proposal_zone(&block_view)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let proposals_count = proposals.len() as i32;

    let mut nodes = Vec::new();
    let mut links = Vec::new();

    let source_block_id = format!("block-{}", block_number);
    nodes.push(GraphNode {
        id: source_block_id.clone(),
        node_type: "source_block".to_string(),
        label: format!("Block #{}", block_number),
        data: serde_json::json!({
            "blockNumber": block_number,
            "blockHash": format!("0x{}", hex::encode(&block_hash)),
            "proposalsCount": proposals_count,
            "role": "proposer"
        }),
    });

    let mut commit_blocks_seen: HashMap<i64, i32> = HashMap::new();
    let mut committed_count = 0;

    // Resolve every proposal's committing transaction through the shared
    // commit-window helper (same path as /blocks/{id}/proposals). A proposal
    // short id is the first 10 bytes of the tx hash.
    let short_ids: Vec<Vec<u8>> = proposals.iter().map(|p| p.short_id.clone()).collect();
    let commitments: Vec<Option<(Vec<u8>, i64)>> = match state.ckb_store {
        Some(ref ckb_store) => resolve_committed_txs(ckb_store, block_number, &short_ids),
        // Without the reader `proposals` is already empty; keep the shape.
        None => vec![None; short_ids.len()],
    };

    for (proposal, found_tx) in proposals.iter().zip(commitments) {
        let proposal_id = &proposal.short_id;
        let proposal_id_hex = format!("0x{}", hex::encode(proposal_id));

        if let Some((tx_hash, commit_block)) = found_tx {
            let tx_hash_hex = format!("0x{}", hex::encode(&tx_hash));
            let distance = commit_block - block_number;

            let proposal_node_id = format!("proposal-{}", proposal_id_hex);
            nodes.push(GraphNode {
                id: proposal_node_id.clone(),
                node_type: "proposal".to_string(),
                label: format!(
                    "...{}",
                    &proposal_id_hex[proposal_id_hex.len().saturating_sub(8)..]
                ),
                data: serde_json::json!({
                    "proposalId": proposal_id_hex,
                    "txHash": tx_hash_hex,
                    "commitBlock": commit_block,
                    "distance": distance,
                    // Names the embedded uncle when the id came from its
                    // proposal zone rather than the source block's own.
                    // `null` for a directly proposed id.
                    "proposedInUncle": proposal.uncle.as_ref().map(|(number, hash)| {
                        serde_json::json!({
                            "blockNumber": number,
                            "blockHash": format!("0x{}", hex::encode(hash)),
                        })
                    }),
                }),
            });

            links.push(GraphLink {
                source: source_block_id.clone(),
                target: proposal_node_id.clone(),
                link_type: "proposes".to_string(),
            });

            *commit_blocks_seen.entry(commit_block).or_insert(0) += 1;

            let commit_block_id = format!("commit-block-{}", commit_block);
            links.push(GraphLink {
                source: proposal_node_id,
                target: commit_block_id,
                link_type: "commits".to_string(),
            });

            committed_count += 1;
        }
    }

    for (commit_block, commit_count) in commit_blocks_seen {
        let distance = commit_block - block_number;
        let commit_block_id = format!("commit-block-{}", commit_block);

        let speed_category = if distance <= 4 {
            "fast"
        } else if distance <= 7 {
            "medium"
        } else {
            "slow"
        };

        nodes.push(GraphNode {
            id: commit_block_id,
            node_type: "commit_block".to_string(),
            label: format!("Block #{}", commit_block),
            data: serde_json::json!({
                "blockNumber": commit_block,
                "distance": distance,
                "committedCount": commit_count,
                "speedCategory": speed_category,
                "role": "committer"
            }),
        });
    }

    ok(ProposalGraphResponse {
        nodes,
        links,
        metadata: ProposalGraphMetadata {
            source_block: block_number,
            total_proposals: proposals_count,
            committed_count,
            commitment_window: ProposalCommitmentWindow {
                close: PROPOSAL_W_CLOSE,
                far: PROPOSAL_W_FAR,
                earliest_commit_block: earliest_commit,
                latest_commit_block: latest_commit,
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_capacity_small() {
        assert_eq!(parse_capacity("10000000000"), "100.00");
    }

    #[test]
    fn test_parse_capacity_thousands() {
        // 100_000_000_000_000 shannon = 1_000_000 CKB = 1.00M
        assert_eq!(parse_capacity("100000000000000"), "1.00M");
    }

    #[test]
    fn test_parse_capacity_millions() {
        assert_eq!(parse_capacity("100000000000000000"), "1000.00M");
    }

    #[test]
    fn test_parse_capacity_zero() {
        assert_eq!(parse_capacity("0"), "0.00");
    }

    #[test]
    fn test_live_cell_key_matches_tx_hash() {
        let tx_hash = vec![0x11; 32];
        let mut key = tx_hash.clone();
        key.extend_from_slice(&0_i16.to_be_bytes());
        assert!(live_cell_key_matches_tx_hash(&key, &tx_hash));
        assert!(!live_cell_key_matches_tx_hash(&key[..33], &tx_hash));
        assert!(!live_cell_key_matches_tx_hash(&key, &[0x22; 32]));
    }

    #[test]
    fn test_graph_response_serialization() {
        let resp = GraphResponse {
            nodes: vec![GraphNode {
                id: "tx-0xabc".to_string(),
                node_type: "transaction".to_string(),
                label: "TX ...abc".to_string(),
                data: serde_json::json!({"hash": "0xabc"}),
            }],
            links: vec![GraphLink {
                source: "tx-0xabc".to_string(),
                target: "cell-0xabc-0".to_string(),
                link_type: "output".to_string(),
            }],
        };

        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["nodes"][0]["nodeType"], "transaction");
        assert_eq!(json["links"][0]["linkType"], "output");
    }

    #[test]
    fn test_proposal_graph_metadata_serialization() {
        let metadata = ProposalGraphMetadata {
            source_block: 100,
            total_proposals: 10,
            committed_count: 8,
            commitment_window: ProposalCommitmentWindow {
                close: 2,
                far: 10,
                earliest_commit_block: 102,
                latest_commit_block: 110,
            },
        };

        let json = serde_json::to_value(&metadata).unwrap();
        assert_eq!(json["sourceBlock"], 100);
        assert_eq!(json["totalProposals"], 10);
        assert_eq!(json["committedCount"], 8);
        assert_eq!(json["commitmentWindow"]["close"], 2);
        assert_eq!(json["commitmentWindow"]["far"], 10);
    }

    #[test]
    fn test_append_consumed_by_relation_adds_tx_node_and_link() {
        let mut nodes = vec![GraphNode {
            id: "cell-0xabc-0".to_string(),
            node_type: "cell".to_string(),
            label: "100 CKB".to_string(),
            data: serde_json::json!({}),
        }];
        let mut links = Vec::new();
        let consumed_by_tx = [0x11u8; 32];

        append_consumed_by_relation(
            &mut nodes,
            &mut links,
            "cell-0xabc-0",
            Some(&consumed_by_tx),
            123,
        );

        assert_eq!(nodes.len(), 2);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].source, "cell-0xabc-0");
        assert_eq!(links[0].link_type, "consumed_by");
        assert_eq!(nodes[1].node_type, "transaction");
        assert!(nodes[1].data["hash"].as_str().unwrap().starts_with("0x"));
        assert_eq!(nodes[1].data["blockNumber"].as_i64(), Some(123));
    }

    #[test]
    fn test_append_consumed_by_relation_no_consumer_is_noop() {
        let mut nodes = vec![GraphNode {
            id: "cell-0xabc-0".to_string(),
            node_type: "cell".to_string(),
            label: "100 CKB".to_string(),
            data: serde_json::json!({}),
        }];
        let mut links = Vec::new();

        append_consumed_by_relation(&mut nodes, &mut links, "cell-0xabc-0", None, 0);

        assert_eq!(nodes.len(), 1);
        assert!(links.is_empty());
    }
}
