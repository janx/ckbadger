use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::clickhouse::query::{build_where_hash, hex_hash};
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
    if let Some(ch_client) = &state.clickhouse_client {
        get_cell_graph_clickhouse(ch_client, &state, tx_hash, output_index, params).await
    } else {
        get_cell_graph_postgres(&state, tx_hash, output_index, params).await
    }
}

async fn get_cell_graph_clickhouse(
    ch_client: &crate::clickhouse::ClickHouseClient,
    _state: &Arc<AppState>,
    tx_hash: String,
    output_index: i32,
    params: GraphParams,
) -> ApiResult<GraphResponse> {
    let mut nodes = Vec::new();
    let mut links = Vec::new();
    let depth = params.depth.clamp(1, 5);

    let cell_id = format!("cell-{}-{}", tx_hash, output_index);
    let tx_hash_where = build_where_hash("c.tx_hash", &tx_hash)?;

    let query = format!(
        "SELECT 
            {} as tx_hash,
            c.output_index,
            c.capacity,
            c.created_at_block,
            (SELECT 1 FROM cell_consumptions cc 
             WHERE cc.tx_hash = c.tx_hash AND cc.output_index = c.output_index LIMIT 1) as is_consumed,
            (SELECT {} FROM cell_consumptions cc 
             WHERE cc.tx_hash = c.tx_hash AND cc.output_index = c.output_index LIMIT 1) as consumed_by_tx,
            (SELECT cc.consumed_at_block FROM cell_consumptions cc 
             WHERE cc.tx_hash = c.tx_hash AND cc.output_index = c.output_index LIMIT 1) as consumed_at_block
        FROM cells c
        WHERE {} AND c.output_index = {}",
        hex_hash("c.tx_hash"),
        hex_hash("cc.consumed_by_tx"),
        tx_hash_where,
        output_index
    );

    #[derive(clickhouse::Row, Deserialize)]
    struct CellRow {
        #[allow(dead_code)]
        tx_hash: String,
        #[allow(dead_code)]
        output_index: u16,
        capacity: u64,
        created_at_block: u64,
        is_consumed: u8,
        consumed_by_tx: Option<String>,
        consumed_at_block: Option<u64>,
    }

    let cell = ch_client
        .client()
        .query(&query)
        .fetch_optional::<CellRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if let Some(cell_row) = cell {
        let capacity = cell_row.capacity.to_string();
        let created_at_block = cell_row.created_at_block as i64;
        let status = if cell_row.is_consumed == 0 { 0 } else { 1 };
        let consumed_at_block = cell_row.consumed_at_block.map(|b| b as i64);
        let consumed_by_tx = cell_row.consumed_by_tx;
        nodes.push(GraphNode {
            id: cell_id.clone(),
            node_type: "cell".to_string(),
            label: format!("{} CKB", parse_capacity(&capacity)),
            data: serde_json::json!({
                "txHash": tx_hash,
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
            let inputs_query = format!(
                "SELECT 
                    {} as previous_tx_hash,
                    previous_output_index
                FROM transaction_inputs
                WHERE {}",
                hex_hash("previous_tx_hash"),
                tx_hash_where
            );

            #[derive(clickhouse::Row, Deserialize)]
            struct InputRow {
                previous_tx_hash: String,
                previous_output_index: u16,
            }

            let inputs = ch_client
                .client()
                .query(&inputs_query)
                .fetch_all::<InputRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            if !inputs.is_empty() {
                let prev_outpoints: Vec<String> = inputs
                    .iter()
                    .map(|i| {
                        format!(
                            "(unhex('{}'), {})",
                            i.previous_tx_hash
                                .strip_prefix("0x")
                                .unwrap_or(&i.previous_tx_hash),
                            i.previous_output_index
                        )
                    })
                    .collect();

                let prev_cells_query = format!(
                    "SELECT 
                        {} as tx_hash,
                        output_index,
                        capacity
                    FROM cells
                    WHERE (tx_hash, output_index) IN ({})",
                    hex_hash("tx_hash"),
                    prev_outpoints.join(", ")
                );

                #[derive(clickhouse::Row, Deserialize)]
                struct PrevCellRow {
                    tx_hash: String,
                    output_index: u16,
                    capacity: u64,
                }

                let prev_cells = ch_client
                    .client()
                    .query(&prev_cells_query)
                    .fetch_all::<PrevCellRow>()
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;

                let cell_map: HashMap<(String, u16), u64> = prev_cells
                    .into_iter()
                    .map(|c| ((c.tx_hash, c.output_index), c.capacity))
                    .collect();

                for input in inputs {
                    let prev_cell_id = format!(
                        "cell-{}-{}",
                        input.previous_tx_hash, input.previous_output_index
                    );

                    if let Some(cap) =
                        cell_map.get(&(input.previous_tx_hash.clone(), input.previous_output_index))
                    {
                        nodes.push(GraphNode {
                            id: prev_cell_id.clone(),
                            node_type: "cell".to_string(),
                            label: format!("{} CKB", parse_capacity(&cap.to_string())),
                            data: serde_json::json!({
                                "txHash": input.previous_tx_hash,
                                "outputIndex": input.previous_output_index,
                                "capacity": cap.to_string(),
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

        if let Some(consuming_tx_hash) = consumed_by_tx {
            let consuming_tx_id = format!("tx-{}", consuming_tx_hash);

            nodes.push(GraphNode {
                id: consuming_tx_id.clone(),
                node_type: "transaction".to_string(),
                label: format!(
                    "TX ...{}",
                    &consuming_tx_hash[consuming_tx_hash.len().saturating_sub(8)..]
                ),
                data: serde_json::json!({
                    "hash": consuming_tx_hash,
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

async fn get_cell_graph_postgres(
    state: &Arc<AppState>,
    tx_hash: String,
    output_index: i32,
    params: GraphParams,
) -> ApiResult<GraphResponse> {
    let hash_bytes = hex::decode(tx_hash.strip_prefix("0x").unwrap_or(&tx_hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let mut nodes = Vec::new();
    let mut links = Vec::new();
    let depth = params.depth.clamp(1, 5);

    let cell_id = format!("cell-{}-{}", tx_hash, output_index);

    let tx_info =
        sqlx::query_as::<_, (i64,)>("SELECT block_number FROM transactions WHERE hash = $1")
            .bind(&hash_bytes)
            .fetch_optional(&state.pool)
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
    .fetch_optional(&state.pool)
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
                FROM transaction_inputs WHERE tx_hash = $1
                "#,
            )
            .bind(&hash_bytes)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            if !inputs.is_empty() {
                let prev_tx_hashes: Vec<&[u8]> =
                    inputs.iter().map(|(_, _, h, _)| h.as_slice()).collect();
                let prev_indices: Vec<i16> = inputs.iter().map(|(_, _, _, i)| *i).collect();

                let prev_cells = sqlx::query_as::<_, (Vec<u8>, i16, String)>(
                    r#"
                    SELECT c.tx_hash, c.output_index, c.capacity::TEXT
                    FROM cells c
                    JOIN transactions t ON t.hash = c.tx_hash AND t.block_number = c.created_at_block
                    JOIN UNNEST($1::bytea[], $2::smallint[]) AS u(tx_hash, output_index)
                      ON c.tx_hash = u.tx_hash AND c.output_index = u.output_index
                    "#,
                )
                .bind(&prev_tx_hashes)
                .bind(&prev_indices)
                .fetch_all(&state.pool)
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
    if let Some(ch_client) = &state.clickhouse_client {
        get_tx_graph_clickhouse(ch_client, &state, hash).await
    } else {
        get_tx_graph_postgres(&state, hash).await
    }
}

async fn get_tx_graph_clickhouse(
    ch_client: &crate::clickhouse::ClickHouseClient,
    _state: &Arc<AppState>,
    hash: String,
) -> ApiResult<GraphResponse> {
    let mut nodes = Vec::new();
    let mut links = Vec::new();

    let tx_hash_where = build_where_hash("hash", &hash)?;

    let tx_query = format!(
        "SELECT 
            {} as hash,
            block_number,
            fee,
            is_cellbase
        FROM transactions
        WHERE {}",
        hex_hash("hash"),
        tx_hash_where
    );

    #[derive(clickhouse::Row, Deserialize)]
    struct TxRow {
        #[allow(dead_code)]
        hash: String,
        block_number: u64,
        fee: u64,
        is_cellbase: u8,
    }

    let tx = ch_client
        .client()
        .query(&tx_query)
        .fetch_optional::<TxRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if let Some(tx_row) = tx {
        let block_number = tx_row.block_number;
        let is_cellbase = tx_row.is_cellbase != 0;
        let tx_id = format!("tx-{}", hash);

        nodes.push(GraphNode {
            id: tx_id.clone(),
            node_type: "transaction".to_string(),
            label: format!("TX ...{}", &hash[hash.len().saturating_sub(8)..]),
            data: serde_json::json!({
                "hash": hash,
                "blockNumber": block_number,
                "fee": tx_row.fee.to_string(),
                "isCellbase": is_cellbase,
            }),
        });

        if !is_cellbase {
            let inputs_query = format!(
                "SELECT 
                    {} as previous_tx_hash,
                    previous_output_index
                FROM transaction_inputs
                WHERE {}",
                hex_hash("previous_tx_hash"),
                tx_hash_where
            );

            #[derive(clickhouse::Row, Deserialize)]
            struct InputRow {
                previous_tx_hash: String,
                previous_output_index: u16,
            }

            let inputs = ch_client
                .client()
                .query(&inputs_query)
                .fetch_all::<InputRow>()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            if !inputs.is_empty() {
                let prev_outpoints: Vec<String> = inputs
                    .iter()
                    .map(|i| {
                        format!(
                            "(unhex('{}'), {})",
                            i.previous_tx_hash
                                .strip_prefix("0x")
                                .unwrap_or(&i.previous_tx_hash),
                            i.previous_output_index
                        )
                    })
                    .collect();

                let prev_cells_query = format!(
                    "SELECT 
                        {} as tx_hash,
                        output_index,
                        capacity
                    FROM cells
                    WHERE (tx_hash, output_index) IN ({})",
                    hex_hash("tx_hash"),
                    prev_outpoints.join(", ")
                );

                #[derive(clickhouse::Row, Deserialize)]
                struct PrevCellRow {
                    tx_hash: String,
                    output_index: u16,
                    capacity: u64,
                }

                let prev_cells = ch_client
                    .client()
                    .query(&prev_cells_query)
                    .fetch_all::<PrevCellRow>()
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;

                let cell_map: HashMap<(String, u16), u64> = prev_cells
                    .into_iter()
                    .map(|c| ((c.tx_hash, c.output_index), c.capacity))
                    .collect();

                for input in inputs {
                    let input_cell_id = format!(
                        "cell-{}-{}",
                        input.previous_tx_hash, input.previous_output_index
                    );

                    let label = cell_map
                        .get(&(input.previous_tx_hash.clone(), input.previous_output_index))
                        .map(|c| format!("{} CKB", parse_capacity(&c.to_string())))
                        .unwrap_or_else(|| "?".to_string());

                    nodes.push(GraphNode {
                        id: input_cell_id.clone(),
                        node_type: "cell".to_string(),
                        label,
                        data: serde_json::json!({
                            "txHash": input.previous_tx_hash,
                            "outputIndex": input.previous_output_index,
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

        let outputs_query = format!(
            "SELECT 
                output_index,
                capacity,
                (SELECT 1 FROM cell_consumptions cc 
                 WHERE cc.tx_hash = c.tx_hash AND cc.output_index = c.output_index LIMIT 1) as is_consumed
            FROM cells c
            WHERE {}",
            tx_hash_where
        );

        #[derive(clickhouse::Row, Deserialize)]
        struct OutputRow {
            output_index: u16,
            capacity: u64,
            is_consumed: u8,
        }

        let outputs = ch_client
            .client()
            .query(&outputs_query)
            .fetch_all::<OutputRow>()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        for output in outputs {
            let output_cell_id = format!("cell-{}-{}", hash, output.output_index);
            let status = if output.is_consumed == 0 { 0 } else { 1 };

            nodes.push(GraphNode {
                id: output_cell_id.clone(),
                node_type: "cell".to_string(),
                label: format!("{} CKB", parse_capacity(&output.capacity.to_string())),
                data: serde_json::json!({
                    "txHash": hash,
                    "outputIndex": output.output_index,
                    "capacity": output.capacity.to_string(),
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

async fn get_tx_graph_postgres(state: &Arc<AppState>, hash: String) -> ApiResult<GraphResponse> {
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    let mut nodes = Vec::new();
    let mut links = Vec::new();

    let tx = sqlx::query_as::<_, (Vec<u8>, i64, String, bool)>(
        "SELECT hash, block_number, fee::TEXT, is_cellbase FROM transactions WHERE hash = $1",
    )
    .bind(&hash_bytes)
    .fetch_optional(&state.pool)
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
                "SELECT previous_tx_hash, previous_output_index FROM transaction_inputs WHERE tx_hash = $1",
            )
            .bind(&hash_bytes)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            if !inputs.is_empty() {
                let prev_tx_hashes: Vec<&[u8]> = inputs.iter().map(|(h, _)| h.as_slice()).collect();
                let prev_indices: Vec<i16> = inputs.iter().map(|(_, i)| *i).collect();

                let prev_cells = sqlx::query_as::<_, (Vec<u8>, i16, String)>(
                    r#"
                    SELECT c.tx_hash, c.output_index, c.capacity::TEXT
                    FROM cells c
                    JOIN transactions t ON t.hash = c.tx_hash AND t.block_number = c.created_at_block
                    JOIN UNNEST($1::bytea[], $2::smallint[]) AS u(tx_hash, output_index)
                      ON c.tx_hash = u.tx_hash AND c.output_index = u.output_index
                    "#,
                )
                .bind(&prev_tx_hashes)
                .bind(&prev_indices)
                .fetch_all(&state.pool)
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

        let outputs = sqlx::query_as::<_, (i16, String, i16)>(
            "SELECT output_index, capacity::TEXT, status FROM cells WHERE tx_hash = $1 AND created_at_block = $2",
        )
        .bind(&hash_bytes)
        .bind(block_number)
        .fetch_all(&state.pool)
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
    if let Some(ch_client) = &state.clickhouse_client {
        get_proposal_graph_clickhouse(ch_client, &state, block_number).await
    } else {
        get_proposal_graph_postgres(&state, block_number).await
    }
}

async fn get_proposal_graph_clickhouse(
    ch_client: &crate::clickhouse::ClickHouseClient,
    _state: &Arc<AppState>,
    block_number: i64,
) -> ApiResult<ProposalGraphResponse> {
    let block_query = format!(
        "SELECT 
            {} as hash,
            proposals_count
        FROM blocks
        WHERE number = {}",
        hex_hash("hash"),
        block_number
    );

    #[derive(clickhouse::Row, Deserialize)]
    struct BlockRow {
        hash: String,
        proposals_count: u32,
    }

    let block_row = ch_client
        .client()
        .query(&block_query)
        .fetch_optional::<BlockRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let block_row = block_row.ok_or_else(|| ApiError::not_found("Block not found"))?;

    const W_CLOSE: i64 = 2;
    const W_FAR: i64 = 10;
    let earliest_commit = block_number + W_CLOSE;
    let latest_commit = block_number + W_FAR;

    let proposals_query = format!(
        "SELECT 
            {} as proposal_id,
            {} as tx_hash,
            t.block_number as commit_block
        FROM block_proposals bp
        INNER JOIN transactions t ON t.short_hash = bp.proposal_id
            AND t.block_number BETWEEN {} AND {}
        WHERE bp.block_number = {}
        ORDER BY t.block_number, bp.proposal_index",
        hex_hash("bp.proposal_id"),
        hex_hash("t.hash"),
        block_number + 2,
        block_number + 10,
        block_number
    );

    #[derive(clickhouse::Row, Deserialize)]
    struct ProposalRow {
        proposal_id: String,
        tx_hash: String,
        commit_block: u64,
    }

    let rows = ch_client
        .client()
        .query(&proposals_query)
        .fetch_all::<ProposalRow>()
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
            "blockHash": block_row.hash,
            "proposalsCount": block_row.proposals_count,
            "role": "proposer"
        }),
    });

    let mut commit_blocks_seen: HashMap<i64, i32> = HashMap::new();

    for row in &rows {
        let distance = row.commit_block as i64 - block_number;

        let proposal_node_id = format!("proposal-{}", row.proposal_id);
        nodes.push(GraphNode {
            id: proposal_node_id.clone(),
            node_type: "proposal".to_string(),
            label: format!(
                "...{}",
                &row.proposal_id[row.proposal_id.len().saturating_sub(8)..]
            ),
            data: serde_json::json!({
                "proposalId": row.proposal_id,
                "txHash": row.tx_hash,
                "commitBlock": row.commit_block,
                "distance": distance
            }),
        });

        links.push(GraphLink {
            source: source_block_id.clone(),
            target: proposal_node_id.clone(),
            link_type: "proposes".to_string(),
        });

        *commit_blocks_seen
            .entry(row.commit_block as i64)
            .or_insert(0) += 1;

        let commit_block_id = format!("commit-block-{}", row.commit_block);
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
            total_proposals: block_row.proposals_count as i32,
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

async fn get_proposal_graph_postgres(
    state: &Arc<AppState>,
    block_number: i64,
) -> ApiResult<ProposalGraphResponse> {
    let block_row: Option<(Vec<u8>, i32)> =
        sqlx::query_as("SELECT hash, proposals_count FROM blocks WHERE number = $1")
            .bind(block_number)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let (block_hash, proposals_count) =
        block_row.ok_or_else(|| ApiError::not_found("Block not found"))?;

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
        INNER JOIN transactions t ON t.short_hash = bp.proposal_id
            AND t.block_number BETWEEN $1 + 2 AND $1 + 10
        WHERE bp.block_number = $1
        ORDER BY t.block_number, bp.proposal_index
        "#,
    )
    .bind(block_number)
    .fetch_all(&state.pool)
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
