use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::clickhouse::{hex_hash, unhex_hash};
use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/search", get(search))
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    q: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub result_type: String,
    pub id: String,
    pub label: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub query: String,
}

#[derive(Debug, Row, Deserialize)]
struct BlockRowClickHouse {
    number: u64,
    #[allow(dead_code)]
    hash: String,
}

#[derive(Debug, Row, Deserialize)]
struct TransactionRowClickHouse {
    #[allow(dead_code)]
    hash: String,
    block_number: u64,
}

#[derive(Debug, Row, Deserialize)]
struct CellRowClickHouse {
    capacity: u64,
    status: u8,
}

#[derive(Debug, Row, Deserialize)]
struct CellCountRowClickHouse {
    count: u64,
}

async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> ApiResult<SearchResponse> {
    if let Some(ch_client) = &state.clickhouse_client {
        search_clickhouse(ch_client, params).await
    } else {
        search_postgres(&state, params).await
    }
}

async fn search_clickhouse(
    ch_client: &crate::clickhouse::ClickHouseClient,
    params: SearchParams,
) -> ApiResult<SearchResponse> {
    let query = params.q.trim();
    let mut results = Vec::new();

    if query.is_empty() {
        return ok(SearchResponse {
            results,
            query: query.to_string(),
        });
    }

    // Search by block number
    if let Ok(block_num) = query.parse::<u64>() {
        let query_str = format!(
            "SELECT number, {} as hash FROM blocks WHERE number = {}",
            hex_hash("hash"),
            block_num
        );

        if let Ok(rows) = ch_client
            .client()
            .query(&query_str)
            .fetch_all::<BlockRowClickHouse>()
            .await
        {
            if !rows.is_empty() {
                results.push(SearchResult {
                    result_type: "block".to_string(),
                    id: block_num.to_string(),
                    label: format!("Block #{}", block_num),
                    url: format!("/blocks/{}", block_num),
                });
            }
        }
    }

    let query_lower = query.to_lowercase();
    let hash_query = if query_lower.starts_with("0x") {
        query_lower.clone()
    } else {
        format!("0x{}", query_lower)
    };

    if hash_query.len() == 66 {
        if let Ok(_hash_bytes) = unhex_hash(&hash_query) {
            // Search for transaction
            let query_str = format!(
                "SELECT {} as hash, block_number FROM transactions WHERE {} = unhex('{}')",
                hex_hash("hash"),
                "hash",
                hash_query.strip_prefix("0x").unwrap_or(&hash_query)
            );

            if let Ok(rows) = ch_client
                .client()
                .query(&query_str)
                .fetch_all::<TransactionRowClickHouse>()
                .await
            {
                if !rows.is_empty() {
                    let row = &rows[0];
                    results.push(SearchResult {
                        result_type: "transaction".to_string(),
                        id: hash_query.clone(),
                        label: format!("Transaction in Block #{}", row.block_number),
                        url: format!("/tx/{}", hash_query),
                    });
                }
            }

            // Search for block by hash
            let query_str = format!(
                "SELECT number, {} as hash FROM blocks WHERE {} = unhex('{}')",
                hex_hash("hash"),
                "hash",
                hash_query.strip_prefix("0x").unwrap_or(&hash_query)
            );

            if let Ok(rows) = ch_client
                .client()
                .query(&query_str)
                .fetch_all::<BlockRowClickHouse>()
                .await
            {
                if !rows.is_empty() {
                    let row = &rows[0];
                    results.push(SearchResult {
                        result_type: "block".to_string(),
                        id: row.number.to_string(),
                        label: format!("Block #{}", row.number),
                        url: format!("/blocks/{}", row.number),
                    });
                }
            }

            // Search for address (by lock_script_hash)
            let query_str = format!(
                "SELECT COUNT() as count FROM cells WHERE {} = unhex('{}')",
                "lock_script_hash",
                hash_query.strip_prefix("0x").unwrap_or(&hash_query)
            );

            if let Ok(rows) = ch_client
                .client()
                .query(&query_str)
                .fetch_all::<CellCountRowClickHouse>()
                .await
            {
                if !rows.is_empty() && rows[0].count > 0 {
                    results.push(SearchResult {
                        result_type: "address".to_string(),
                        id: hash_query.clone(),
                        label: format!("Address ({} cells)", rows[0].count),
                        url: format!("/address/{}", hash_query),
                    });
                }
            }
        }
    }

    // Search for cell by tx_hash-output_index
    if query.contains('-') && !query.starts_with("ckb") && !query.starts_with("ckt") {
        let parts: Vec<&str> = query.split('-').collect();
        if parts.len() == 2 {
            let tx_hash = parts[0];
            if let Ok(index) = parts[1].parse::<u16>() {
                let tx_hash_normalized = if tx_hash.starts_with("0x") {
                    tx_hash.to_string()
                } else {
                    format!("0x{}", tx_hash)
                };

                if tx_hash_normalized.len() == 66 {
                    if let Ok(_hash_bytes) = unhex_hash(&tx_hash_normalized) {
                        let query_str = format!(
                            "SELECT capacity, status FROM cells WHERE {} = unhex('{}') AND output_index = {}",
                            "tx_hash",
                            tx_hash_normalized.strip_prefix("0x").unwrap_or(&tx_hash_normalized),
                            index
                        );

                        if let Ok(rows) = ch_client
                            .client()
                            .query(&query_str)
                            .fetch_all::<CellRowClickHouse>()
                            .await
                        {
                            if !rows.is_empty() {
                                let row = &rows[0];
                                let status_str = if row.status == 0 { "Live" } else { "Dead" };
                                results.push(SearchResult {
                                    result_type: "cell".to_string(),
                                    id: format!("{}-{}", tx_hash_normalized, index),
                                    label: format!(
                                        "Cell ({}, {} CKB)",
                                        status_str,
                                        parse_capacity(row.capacity)
                                    ),
                                    url: format!("/cell/{}-{}", tx_hash_normalized, index),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    ok(SearchResponse {
        results,
        query: query.to_string(),
    })
}

async fn search_postgres(state: &Arc<AppState>, params: SearchParams) -> ApiResult<SearchResponse> {
    let query = params.q.trim();
    let mut results = Vec::new();

    if query.is_empty() {
        return ok(SearchResponse {
            results,
            query: query.to_string(),
        });
    }

    if let Ok(block_num) = query.parse::<i64>() {
        let block = sqlx::query_as::<_, (Vec<u8>,)>("SELECT hash FROM blocks WHERE number = $1")
            .bind(block_num)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        if let Some((_hash,)) = block {
            results.push(SearchResult {
                result_type: "block".to_string(),
                id: block_num.to_string(),
                label: format!("Block #{}", block_num),
                url: format!("/blocks/{}", block_num),
            });
        }
    }

    let query_lower = query.to_lowercase();
    let hash_query = if query_lower.starts_with("0x") {
        query_lower.clone()
    } else {
        format!("0x{}", query_lower)
    };

    if hash_query.len() == 66 {
        if let Ok(hash_bytes) = hex::decode(&hash_query[2..]) {
            let tx = sqlx::query_as::<_, (i64,)>(
                "SELECT block_number FROM transactions WHERE hash = $1",
            )
            .bind(&hash_bytes)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            if let Some((block_num,)) = tx {
                results.push(SearchResult {
                    result_type: "transaction".to_string(),
                    id: hash_query.clone(),
                    label: format!("Transaction in Block #{}", block_num),
                    url: format!("/tx/{}", hash_query),
                });
            }

            let block = sqlx::query_as::<_, (i64,)>("SELECT number FROM blocks WHERE hash = $1")
                .bind(&hash_bytes)
                .fetch_optional(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            if let Some((block_num,)) = block {
                results.push(SearchResult {
                    result_type: "block".to_string(),
                    id: block_num.to_string(),
                    label: format!("Block #{}", block_num),
                    url: format!("/blocks/{}", block_num),
                });
            }

            let cell_count = sqlx::query_as::<_, (i64,)>(
                "SELECT COUNT(*) FROM cells WHERE lock_script_hash = $1",
            )
            .bind(&hash_bytes)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            if cell_count.0 > 0 {
                results.push(SearchResult {
                    result_type: "address".to_string(),
                    id: hash_query.clone(),
                    label: format!("Address ({} cells)", cell_count.0),
                    url: format!("/address/{}", hash_query),
                });
            }
        }
    }

    if query.contains('-') && !query.starts_with("ckb") && !query.starts_with("ckt") {
        let parts: Vec<&str> = query.split('-').collect();
        if parts.len() == 2 {
            let tx_hash = parts[0];
            if let Ok(index) = parts[1].parse::<i32>() {
                let tx_hash_normalized = if tx_hash.starts_with("0x") {
                    tx_hash.to_string()
                } else {
                    format!("0x{}", tx_hash)
                };

                if tx_hash_normalized.len() == 66 {
                    if let Ok(hash_bytes) = hex::decode(&tx_hash_normalized[2..]) {
                        let cell = sqlx::query_as::<_, (String, i16)>(
                            "SELECT capacity::TEXT, status FROM cells WHERE tx_hash = $1 AND output_index = $2",
                        )
                        .bind(&hash_bytes)
                        .bind(index)
                        .fetch_optional(&state.pool)
                        .await
                        .map_err(|e| ApiError::internal(e.to_string()))?;

                        if let Some((capacity, status)) = cell {
                            let status_str = if status == 0 { "Live" } else { "Dead" };
                            results.push(SearchResult {
                                result_type: "cell".to_string(),
                                id: format!("{}-{}", tx_hash_normalized, index),
                                label: format!(
                                    "Cell ({}, {} CKB)",
                                    status_str,
                                    parse_capacity_str(&capacity)
                                ),
                                url: format!("/cell/{}-{}", tx_hash_normalized, index),
                            });
                        }
                    }
                }
            }
        }
    }

    ok(SearchResponse {
        results,
        query: query.to_string(),
    })
}

fn parse_capacity(capacity: u64) -> String {
    let ckb = capacity as f64 / 1e8;
    if ckb >= 1_000_000.0 {
        format!("{:.2}M", ckb / 1_000_000.0)
    } else if ckb >= 1_000.0 {
        format!("{:.2}K", ckb / 1_000.0)
    } else {
        format!("{:.2}", ckb)
    }
}

fn parse_capacity_str(capacity: &str) -> String {
    let ckb = capacity.parse::<u64>().unwrap_or(0) as f64 / 1e8;
    if ckb >= 1_000_000.0 {
        format!("{:.2}M", ckb / 1_000_000.0)
    } else if ckb >= 1_000.0 {
        format!("{:.2}K", ckb / 1_000.0)
    } else {
        format!("{:.2}", ckb)
    }
}
