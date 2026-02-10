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

    // Search by block number
    if let Ok(block_num) = query.parse::<i64>() {
        let block = state
            .store
            .get_block_header(block_num)
            .map_err(|e| ApiError::internal(e.to_string()))?;

        if block.is_some() {
            results.push(SearchResult {
                result_type: "block".to_string(),
                id: block_num.to_string(),
                label: format!("Block #{}", block_num),
                url: format!("/blocks/{}", block_num),
            });
        }
    }

    // Search by hash (32 bytes = 64 hex chars + "0x" = 66)
    let query_lower = query.to_lowercase();
    let hash_query = if query_lower.starts_with("0x") {
        query_lower.clone()
    } else {
        format!("0x{}", query_lower)
    };

    if hash_query.len() == 66 {
        if let Ok(hash_bytes) = hex::decode(&hash_query[2..]) {
            // Search for transaction by hash
            let tx_result = state
                .store
                .get_tx_location(&hash_bytes)
                .map_err(|e| ApiError::internal(e.to_string()))?;

            // Search for block by hash
            let block_result = state
                .store
                .get_block_number_by_hash(&hash_bytes)
                .map_err(|e| ApiError::internal(e.to_string()))?;

            // Search for address by lock_script_hash
            let addr_balance = state
                .store
                .get_addr_balance(&hash_bytes)
                .map_err(|e| ApiError::internal(e.to_string()))?;

            if let Some((block_num, _)) = tx_result {
                results.push(SearchResult {
                    result_type: "transaction".to_string(),
                    id: hash_query.clone(),
                    label: format!("Transaction in Block #{}", block_num),
                    url: format!("/tx/{}", hash_query),
                });
            }

            if let Some(block_num) = block_result {
                results.push(SearchResult {
                    result_type: "block".to_string(),
                    id: block_num.to_string(),
                    label: format!("Block #{}", block_num),
                    url: format!("/blocks/{}", block_num),
                });
            }

            if let Some(ab) = addr_balance {
                if ab.total_cells_count > 0 {
                    results.push(SearchResult {
                        result_type: "address".to_string(),
                        id: hash_query.clone(),
                        label: format!("Address ({} cells)", ab.total_cells_count),
                        url: format!("/address/{}", hash_query),
                    });
                }
            }
        }
    }

    // Search for cell outpoint: <tx_hash>-<output_index>
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
                        let output_idx = index as i16;

                        // Try live cell first
                        let live = state
                            .store
                            .get_cell(&hash_bytes, output_idx)
                            .map_err(|e| ApiError::internal(e.to_string()))?;

                        // Try consumed cell
                        let consumed = if live.is_none() {
                            state
                                .store
                                .get_consumed_cell(&hash_bytes, output_idx)
                                .map_err(|e| ApiError::internal(e.to_string()))?
                        } else {
                            None
                        };

                        match (live, consumed) {
                            (Some(cell), _) => {
                                let status_str = "Live";
                                results.push(SearchResult {
                                    result_type: "cell".to_string(),
                                    id: format!("{}-{}", tx_hash_normalized, index),
                                    label: format!(
                                        "Cell ({}, {} CKB)",
                                        status_str,
                                        parse_capacity(&cell.capacity.to_string())
                                    ),
                                    url: format!("/cell/{}-{}", tx_hash_normalized, index),
                                });
                            }
                            (None, Some(cell)) => {
                                let status_str = "Dead";
                                results.push(SearchResult {
                                    result_type: "cell".to_string(),
                                    id: format!("{}-{}", tx_hash_normalized, index),
                                    label: format!(
                                        "Cell ({}, {} CKB)",
                                        status_str,
                                        parse_capacity(&cell.capacity.to_string())
                                    ),
                                    url: format!("/cell/{}-{}", tx_hash_normalized, index),
                                });
                            }
                            _ => {}
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
    fn test_parse_capacity_invalid() {
        assert_eq!(parse_capacity("not_a_number"), "0.00");
    }

    #[test]
    fn test_search_response_serialization() {
        let resp = SearchResponse {
            results: vec![SearchResult {
                result_type: "block".to_string(),
                id: "100".to_string(),
                label: "Block #100".to_string(),
                url: "/blocks/100".to_string(),
            }],
            query: "100".to_string(),
        };

        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["query"], "100");
        assert_eq!(json["results"][0]["resultType"], "block");
        assert_eq!(json["results"][0]["id"], "100");
    }

    #[test]
    fn test_search_response_empty() {
        let resp = SearchResponse {
            results: vec![],
            query: "".to_string(),
        };

        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["results"].as_array().unwrap().len(), 0);
    }
}
