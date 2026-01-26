use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> ApiResult<SearchResponse> {
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
                                    parse_capacity(&capacity)
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
