use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
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

    // Get block_number from transactions first for partition pruning
    let tx_info =
        sqlx::query_as::<_, (i64,)>("SELECT block_number FROM transactions_index WHERE hash = $1")
            .bind(&hash_bytes)
            .fetch_optional(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let block_number = match tx_info {
        Some((bn,)) => bn,
        None => return Err(ApiError::not_found("Transaction not found")),
    };

    let cell = sqlx::query_as::<_, (Vec<u8>, i16, String, i16, i64, Option<i64>, Option<Vec<u8>>)>(
        r#"
        SELECT tx_hash, output_index, capacity::TEXT, status, created_at_block, consumed_at_block, consumed_by_tx
        FROM cells WHERE tx_hash = $1 AND output_index = $2 AND created_at_block = $3
        "#,
    )
    .bind(&hash_bytes)
    .bind(output_index)
    .bind(block_number)
    .fetch_optional(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    if let Some((_, _, capacity, status, created_at_block, consumed_at_block, consumed_by_tx)) =
        cell
    {
        nodes.push(GraphNode {
            id: cell_id.clone(),
            node_type: "cell".to_string(),
            label: format!("{} CKB", parse_capacity(&capacity)),
            data: serde_json::json!({
                "txHash": format!("0x{}", hex::encode(&hash_bytes)),
                "outputIndex": output_index,
                "capacity": capacity,
                "status": if status == 0 { "live" } else { "dead" },
                "createdAtBlock": created_at_block,
            }),
        });

        let created_tx_id = format!("tx-{}", tx_hash);
        nodes.push(GraphNode {
            id: created_tx_id.clone(),
            node_type: "transaction".to_string(),
            label: format!("TX ...{}", &tx_hash[tx_hash.len().saturating_sub(8)..]),
            data: serde_json::json!({
                "hash": tx_hash,
                "blockNumber": created_at_block,
            }),
        });

        links.push(GraphLink {
            source: created_tx_id.clone(),
            target: cell_id.clone(),
            link_type: "creates".to_string(),
        });

        if depth > 1 {
            let inputs = sqlx::query_as::<_, (Vec<u8>, i16, Vec<u8>, i16)>(
                r#"
                SELECT tx_hash, input_index, previous_tx_hash, previous_output_index
                FROM transaction_inputs WHERE tx_hash = $1 AND tx_block_number = $2
                "#,
            )
            .bind(&hash_bytes)
            .bind(block_number)
            .fetch_all(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            if !inputs.is_empty() {
                let prev_tx_hashes: Vec<&[u8]> =
                    inputs.iter().map(|(_, _, h, _)| h.as_slice()).collect();
                let prev_indices: Vec<i16> = inputs.iter().map(|(_, _, _, i)| *i).collect();

                // Join with transactions to get created_at_block for partition pruning
                let prev_cells = sqlx::query_as::<_, (Vec<u8>, i16, String)>(
                    r#"
                    SELECT c.tx_hash, c.output_index, c.capacity::TEXT
                    FROM cells c
                    JOIN transactions_index t ON t.hash = c.tx_hash AND t.block_number = c.created_at_block
                    JOIN UNNEST($1::bytea[], $2::smallint[]) AS u(tx_hash, output_index)
                      ON c.tx_hash = u.tx_hash AND c.output_index = u.output_index
                    "#,
                )
                .bind(&prev_tx_hashes)
                .bind(&prev_indices)
                .fetch_all(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

                let cell_map: HashMap<(Vec<u8>, i16), String> = prev_cells
                    .into_iter()
                    .map(|(tx_hash, idx, cap)| ((tx_hash, idx), cap))
                    .collect();

                for (_, _, prev_tx_hash, prev_idx) in inputs {
                    let prev_cell_id =
                        format!("cell-0x{}-{}", hex::encode(&prev_tx_hash), prev_idx);

                    if let Some(cap) = cell_map.get(&(prev_tx_hash.clone(), prev_idx)) {
                        nodes.push(GraphNode {
                            id: prev_cell_id.clone(),
                            node_type: "cell".to_string(),
                            label: format!("{} CKB", parse_capacity(cap)),
                            data: serde_json::json!({
                                "txHash": format!("0x{}", hex::encode(&prev_tx_hash)),
                                "outputIndex": prev_idx,
                                "capacity": cap,
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

        if let Some(consuming_tx) = consumed_by_tx {
            let consuming_tx_hex = format!("0x{}", hex::encode(&consuming_tx));
            let consuming_tx_id = format!("tx-{}", consuming_tx_hex);

            nodes.push(GraphNode {
                id: consuming_tx_id.clone(),
                node_type: "transaction".to_string(),
                label: format!(
                    "TX ...{}",
                    &consuming_tx_hex[consuming_tx_hex.len().saturating_sub(8)..]
                ),
                data: serde_json::json!({
                    "hash": consuming_tx_hex,
                    "blockNumber": consumed_at_block,
                }),
            });

            links.push(GraphLink {
                source: cell_id,
                target: consuming_tx_id,
                link_type: "consumed_by".to_string(),
            });
        }
    } else {
        return Err(ApiError::not_found("Cell not found"));
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

    let tx = sqlx::query_as::<_, (Vec<u8>, i64, String, bool)>(
        "SELECT hash, block_number, fee::TEXT, is_cellbase FROM transactions_index WHERE hash = $1",
    )
    .bind(&hash_bytes)
    .fetch_optional(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    if let Some((_, block_number, fee, is_cellbase)) = tx {
        let tx_id = format!("tx-{}", hash);

        nodes.push(GraphNode {
            id: tx_id.clone(),
            node_type: "transaction".to_string(),
            label: format!("TX ...{}", &hash[hash.len().saturating_sub(8)..]),
            data: serde_json::json!({
                "hash": hash,
                "blockNumber": block_number,
                "fee": fee,
                "isCellbase": is_cellbase,
            }),
        });

        if !is_cellbase {
            let inputs = sqlx::query_as::<_, (Vec<u8>, i16)>(
                "SELECT previous_tx_hash, previous_output_index FROM transaction_inputs WHERE tx_hash = $1 AND tx_block_number = $2",
            )
            .bind(&hash_bytes)
            .bind(block_number)
            .fetch_all(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            if !inputs.is_empty() {
                let prev_tx_hashes: Vec<&[u8]> = inputs.iter().map(|(h, _)| h.as_slice()).collect();
                let prev_indices: Vec<i16> = inputs.iter().map(|(_, i)| *i).collect();

                // Join with transactions to get created_at_block for partition pruning
                let prev_cells = sqlx::query_as::<_, (Vec<u8>, i16, String)>(
                    r#"
                    SELECT c.tx_hash, c.output_index, c.capacity::TEXT
                    FROM cells c
                    JOIN transactions_index t ON t.hash = c.tx_hash AND t.block_number = c.created_at_block
                    JOIN UNNEST($1::bytea[], $2::smallint[]) AS u(tx_hash, output_index)
                      ON c.tx_hash = u.tx_hash AND c.output_index = u.output_index
                    "#,
                )
                .bind(&prev_tx_hashes)
                .bind(&prev_indices)
                .fetch_all(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

                let cell_map: HashMap<(Vec<u8>, i16), String> = prev_cells
                    .into_iter()
                    .map(|(tx_hash, idx, cap)| ((tx_hash, idx), cap))
                    .collect();

                for (prev_tx_hash, prev_idx) in inputs {
                    let prev_tx_hex = format!("0x{}", hex::encode(&prev_tx_hash));
                    let input_cell_id = format!("cell-{}-{}", prev_tx_hex, prev_idx);

                    let label = cell_map
                        .get(&(prev_tx_hash.clone(), prev_idx))
                        .map(|c| format!("{} CKB", parse_capacity(c)))
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

        // Query outputs - use created_at_block for partition pruning
        let outputs = sqlx::query_as::<_, (i16, String, i16)>(
            "SELECT output_index, capacity::TEXT, status FROM cells WHERE tx_hash = $1 AND created_at_block = $2",
        )
        .bind(&hash_bytes)
        .bind(block_number)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

        for (output_index, capacity, status) in outputs {
            let output_cell_id = format!("cell-{}-{}", hash, output_index);

            nodes.push(GraphNode {
                id: output_cell_id.clone(),
                node_type: "cell".to_string(),
                label: format!("{} CKB", parse_capacity(&capacity)),
                data: serde_json::json!({
                    "txHash": hash,
                    "outputIndex": output_index,
                    "capacity": capacity,
                    "status": if status == 0 { "live" } else { "dead" },
                }),
            });

            links.push(GraphLink {
                source: tx_id.clone(),
                target: output_cell_id,
                link_type: "output".to_string(),
            });
        }
    } else {
        return Err(ApiError::not_found("Transaction not found"));
    }

    ok(GraphResponse { nodes, links })
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
    let block_row: Option<(Vec<u8>, i32)> =
        sqlx::query_as("SELECT hash, proposals_count FROM blocks_index WHERE number = $1")
            .bind(block_number)
            .fetch_optional(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let (block_hash, proposals_count) =
        block_row.ok_or_else(|| ApiError::not_found("Block not found"))?;

    // NC-Max: w_close=2, w_far=10 (proposals can commit 2-10 blocks later)
    const W_CLOSE: i64 = 2;
    const W_FAR: i64 = 10;
    let earliest_commit = block_number + W_CLOSE;
    let latest_commit = block_number + W_FAR;

    let rows: Vec<(Vec<u8>, Vec<u8>, i64)> = sqlx::query_as(
        r#"
        SELECT
            bp.proposal_id,
            t.hash as tx_hash,
            t.block_number as commit_block
        FROM block_proposals bp
        INNER JOIN transactions_index t ON substring(t.hash, 1, 10) = bp.proposal_id
            AND t.block_number BETWEEN $1 + 2 AND $1 + 10
        WHERE bp.block_number = $1
        ORDER BY t.block_number, bp.proposal_index
        "#,
    )
    .bind(block_number)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

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

    for (proposal_id, tx_hash, commit_block) in &rows {
        let proposal_id_hex = format!("0x{}", hex::encode(proposal_id));
        let tx_hash_hex = format!("0x{}", hex::encode(tx_hash));
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

        *commit_blocks_seen.entry(*commit_block).or_insert(0) += 1;

        let commit_block_id = format!("commit-block-{}", commit_block);
        links.push(GraphLink {
            source: proposal_node_id,
            target: commit_block_id,
            link_type: "commits".to_string(),
        });
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

    let committed_count = rows.len() as i32;

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
