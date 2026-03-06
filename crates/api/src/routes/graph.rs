use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckb_types::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

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

async fn get_cell_graph(
    State(state): State<Arc<AppState>>,
    Path((tx_hash, output_index)): Path<(String, i32)>,
    Query(params): Query<GraphParams>,
) -> ApiResult<GraphResponse> {
    let hash_bytes = hex::decode(tx_hash.strip_prefix("0x").unwrap_or(&tx_hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let mut nodes = Vec::new();
    let mut links = Vec::new();
    let depth = params.depth.clamp(1, 5);

    let cell_id = format!("cell-{}-{}", tx_hash, output_index);

    // Look up the transaction to verify it exists
    let tx_info = state
        .store
        .get_tx_location(&hash_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let block_number = match tx_info {
        Some((bn, _)) => bn,
        None => return Err(ApiError::not_found("Transaction not found")),
    };

    // Get the cell info (live or consumed)
    let output_idx = output_index as i16;
    let live_cell = state
        .store
        .get_cell_with_payload_store(state.append_only_store.as_ref(), &hash_bytes, output_idx)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let consumed_cell = if live_cell.is_none() {
        state
            .store
            .get_consumed_cell_info_with_payload_store(
                state.append_only_store.as_ref(),
                &hash_bytes,
                output_idx,
            )
            .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        None
    };

    let (cell_info, status_str, consumed_by_tx, consumed_at_block) =
        match (live_cell, consumed_cell) {
            (Some(info), _) => (info, "live", None, 0),
            (None, Some(info)) => (
                info.cell,
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
                let outpoints: Vec<(&[u8], i16)> =
                    inputs.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                let cell_map = state
                    .store
                    .get_cells_batch_with_payload_store(
                        state.append_only_store.as_ref(),
                        &outpoints,
                    )
                    .map_err(|e| ApiError::internal(e.to_string()))?;

                for (prev_tx_hash, prev_idx) in &inputs {
                    let prev_cell_id = format!("cell-0x{}-{}", hex::encode(prev_tx_hash), prev_idx);

                    let label = cell_map
                        .get(&(prev_tx_hash.clone(), *prev_idx))
                        .map(|c| format!("{} CKB", parse_capacity(&c.capacity.to_string())))
                        .unwrap_or_else(|| {
                            // Try consumed cells
                            state
                                .store
                                .get_consumed_cell_with_payload_store(
                                    state.append_only_store.as_ref(),
                                    prev_tx_hash,
                                    *prev_idx,
                                )
                                .ok()
                                .flatten()
                                .map(|c| format!("{} CKB", parse_capacity(&c.capacity.to_string())))
                                .unwrap_or_else(|| "?".to_string())
                        });

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

async fn get_tx_graph(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
    Query(_params): Query<GraphParams>,
) -> ApiResult<GraphResponse> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let mut nodes = Vec::new();
    let mut links = Vec::new();

    // Look up transaction
    let tx_result = state
        .store
        .get_tx_by_hash(&hash_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (block_number, _tx_idx, tx_entry) = match tx_result {
        Some(info) => info,
        None => return Err(ApiError::not_found("Transaction not found")),
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
                let outpoints: Vec<(&[u8], i16)> =
                    inputs.iter().map(|(h, i)| (h.as_slice(), *i)).collect();

                let live_map = state
                    .store
                    .get_cells_batch_with_payload_store(
                        state.append_only_store.as_ref(),
                        &outpoints,
                    )
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                let consumed_map = state
                    .store
                    .get_consumed_cells_batch_with_payload_store(
                        state.append_only_store.as_ref(),
                        &outpoints,
                    )
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

        for (output_index, capacity) in outputs {
            let output_cell_id = format!("cell-{}-{}", hash, output_index);
            let capacity_str = capacity.to_string();

            // Check if cell is live or dead
            let is_live = state
                .store
                .get_cell_with_payload_store(
                    state.append_only_store.as_ref(),
                    &hash_bytes,
                    output_index,
                )
                .ok()
                .flatten()
                .is_some();

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
    } else {
        // Fallback: look up live cells created by this tx from the store.
        // This won't show consumed outputs.
        for (output_index, info) in load_live_cells_created_by_tx(
            state.store.as_ref(),
            state.append_only_store.as_ref(),
            &hash_bytes,
        )
        .map_err(|e| ApiError::internal(e.to_string()))?
        {
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

fn load_live_cells_created_by_tx(
    store: &ckbadger_store::CkbadgerStore,
    payload_store: &ckbadger_store::CkbadgerStore,
    tx_hash: &[u8],
) -> anyhow::Result<Vec<(i16, ckbadger_store::LiveCellInfo)>> {
    let iter = store.iterator_cf(
        store.cf_cell_state(),
        rocksdb::IteratorMode::From(tx_hash, rocksdb::Direction::Forward),
    );

    let mut rows = Vec::new();
    for item in iter {
        let (key, _) = item.map_err(|e| anyhow::anyhow!("failed to iterate cell_state: {}", e))?;
        if !live_cell_key_matches_tx_hash(&key, tx_hash) {
            break;
        }
        let output_index = i16::from_be_bytes([key[32], key[33]]);
        let Some(info) =
            store.get_live_cell_by_outpoint_key_with_payload_store(payload_store, &key)?
        else {
            continue;
        };
        rows.push((output_index, info));
    }
    Ok(rows)
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

async fn get_proposal_graph(
    State(state): State<Arc<AppState>>,
    Path(block_number): Path<i64>,
) -> ApiResult<ProposalGraphResponse> {
    let block_header = state
        .store
        .get_block_header(block_number)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Block not found"))?;

    let block_hash = block_header.hash;

    // NC-Max: w_close=2, w_far=10 (proposals can commit 2-10 blocks later)
    const W_CLOSE: i64 = 2;
    const W_FAR: i64 = 10;
    let earliest_commit = block_number + W_CLOSE;
    let latest_commit = block_number + W_FAR;

    // Get proposal short IDs from CKB store
    let proposals: Vec<Vec<u8>> = if let Some(ref ckb_store) = state.ckb_store {
        if let Some(block_view) = ckb_store.get_block_by_number(block_number as u64) {
            let proposal_ids = block_view.data().proposals();
            (0..proposal_ids.len())
                .map(|i| proposal_ids.get(i).unwrap().raw_data().to_vec())
                .collect()
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

    // For each proposal, try to find the committed transaction
    // Proposal short ID is the first 10 bytes of the tx hash
    for proposal_id in &proposals {
        let proposal_id_hex = format!("0x{}", hex::encode(proposal_id));

        // Search for the transaction that matches this proposal ID
        // by checking block transactions in the commitment window
        let mut found_tx: Option<(Vec<u8>, i64)> = None;

        if let Some(ref ckb_store) = state.ckb_store {
            for commit_block_num in earliest_commit..=latest_commit {
                if let Some(commit_block) = ckb_store.get_block_by_number(commit_block_num as u64) {
                    let txs = commit_block.transactions();
                    for tx in txs {
                        let tx_hash_bytes: [u8; 32] = tx.hash().unpack();
                        if tx_hash_bytes[..proposal_id.len()] == proposal_id[..] {
                            found_tx = Some((tx_hash_bytes.to_vec(), commit_block_num));
                            break;
                        }
                    }
                }
                if found_tx.is_some() {
                    break;
                }
            }
        }

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
                    "distance": distance
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
                close: W_CLOSE,
                far: W_FAR,
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
    fn test_load_live_cells_created_by_tx_reads_split_domain_append_layout() {
        let root = tempfile::tempdir().unwrap();
        let domain =
            ckbadger_store::CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let append =
            ckbadger_store::CkbadgerStore::open_append_only(root.path().join("append")).unwrap();
        let tx_hash = [0x33; 32];

        let live_cell = ckbadger_store::LiveCellInfo {
            capacity: 111_00000000,
            created_at_block: 10,
            lock_script_hash: vec![0x01; 32],
            lock_code_hash: vec![0x02; 32],
            lock_hash_type: 1,
            lock_args: vec![0x03; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
        };
        let consumed_cell = ckbadger_store::LiveCellInfo {
            capacity: 222_00000000,
            created_at_block: 10,
            lock_script_hash: vec![0x04; 32],
            lock_code_hash: vec![0x05; 32],
            lock_hash_type: 1,
            lock_args: vec![0x06; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
        };

        let mut domain_batch = ckbadger_store::batch::StoreBatch::new(&domain);
        domain_batch.put_cell(&tx_hash, 0, &live_cell);
        domain_batch.put_consumed_cell(&tx_hash, 1, &consumed_cell, 99);
        domain_batch.commit().unwrap();

        let mut append_batch = ckbadger_store::batch::StoreBatch::new(&append);
        append_batch.put_cell(&tx_hash, 0, &live_cell);
        append_batch.put_cell(&tx_hash, 1, &consumed_cell);
        append_batch.commit().unwrap();

        let rows = load_live_cells_created_by_tx(&domain, &append, &tx_hash).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 0);
        assert_eq!(rows[0].1.capacity, live_cell.capacity);
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
