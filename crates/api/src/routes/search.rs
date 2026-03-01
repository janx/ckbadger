use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::utils::{address_to_lock_script_hash, is_ckb_address};
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
    pub match_kind: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub query: String,
    pub normalized_query: String,
    pub ambiguous: bool,
}

const MAX_RESULTS: usize = 20;
const NAME_MATCH_LIMIT: usize = 6;
const SPORE_NAME_SCAN_LIMIT: usize = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchScope {
    All,
    Block,
    Transaction,
    Address,
    Cell,
    Script,
    Token,
    Spore,
    Cluster,
    Asset,
}

#[derive(Default)]
struct SearchAccumulator {
    results: Vec<SearchResult>,
    dedupe: HashSet<(String, String)>,
}

impl SearchAccumulator {
    fn push(&mut self, result: SearchResult) {
        if self.results.len() >= MAX_RESULTS {
            return;
        }

        let key = (result.result_type.clone(), result.id.clone());
        if self.dedupe.insert(key) {
            self.results.push(result);
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedOutpoint {
    tx_hash_bytes: Vec<u8>,
    output_index: i16,
    normalized: String,
}

fn parse_scope(query: &str) -> (SearchScope, &str) {
    let Some((raw_prefix, raw_body)) = query.split_once(':') else {
        return (SearchScope::All, query);
    };

    let scope = match raw_prefix.trim().to_ascii_lowercase().as_str() {
        "block" => SearchScope::Block,
        "tx" => SearchScope::Transaction,
        "addr" | "address" => SearchScope::Address,
        "cell" => SearchScope::Cell,
        "script" => SearchScope::Script,
        "token" => SearchScope::Token,
        "spore" => SearchScope::Spore,
        "cluster" => SearchScope::Cluster,
        "asset" => SearchScope::Asset,
        _ => SearchScope::All,
    };

    if scope == SearchScope::All {
        (SearchScope::All, query)
    } else {
        (scope, raw_body)
    }
}

fn scope_allows(scope: SearchScope, allowed: &[SearchScope]) -> bool {
    scope == SearchScope::All || allowed.contains(&scope)
}

fn normalize_hash32(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let no_prefix = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if no_prefix.len() != 64 || !no_prefix.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("0x{}", no_prefix.to_ascii_lowercase()))
}

fn parse_output_index(raw: &str) -> Option<i16> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed_u32 = if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        trimmed.parse::<u32>().ok()?
    };

    i16::try_from(parsed_u32).ok()
}

fn parse_outpoint(value: &str) -> Option<ParsedOutpoint> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let delimiter_index = [trimmed.rfind('-'), trimmed.rfind(':'), trimmed.rfind('#')]
        .into_iter()
        .flatten()
        .max()?;

    if delimiter_index < 1 || delimiter_index >= trimmed.len() - 1 {
        return None;
    }

    let tx_hash = normalize_hash32(&trimmed[..delimiter_index])?;
    let tx_hash_bytes = hex::decode(&tx_hash[2..]).ok()?;
    let output_index = parse_output_index(&trimmed[delimiter_index + 1..])?;
    let normalized = format!("{}-{}", tx_hash, output_index);

    Some(ParsedOutpoint {
        tx_hash_bytes,
        output_index,
        normalized,
    })
}

fn is_known_script_name(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    let trimmed = name.trim();
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("unknown")
}

fn normalized_query_for_response(query: &str) -> String {
    let trimmed = query.trim();
    if let Some(outpoint) = parse_outpoint(trimmed) {
        return outpoint.normalized;
    }
    if let Some(hash) = normalize_hash32(trimmed) {
        return hash;
    }
    trimmed.to_string()
}

async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> ApiResult<SearchResponse> {
    let query = params.q.trim();
    let (scope, scoped_query_raw) = parse_scope(query);
    let scoped_query = scoped_query_raw.trim();
    let normalized_query = normalized_query_for_response(scoped_query);
    let mut results = SearchAccumulator::default();

    if scoped_query.is_empty() {
        return ok(SearchResponse {
            results: vec![],
            query: query.to_string(),
            normalized_query,
            ambiguous: false,
        });
    }

    if scope_allows(scope, &[SearchScope::Block]) {
        if let Ok(block_num) = scoped_query.parse::<i64>() {
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
                    match_kind: "exact_number".to_string(),
                });
            }
        }
    }

    if scope_allows(
        scope,
        &[
            SearchScope::Block,
            SearchScope::Transaction,
            SearchScope::Address,
            SearchScope::Script,
            SearchScope::Token,
            SearchScope::Spore,
            SearchScope::Cluster,
            SearchScope::Asset,
        ],
    ) {
        if let Some(hash_query) = normalize_hash32(scoped_query) {
            let hash_bytes =
                hex::decode(&hash_query[2..]).map_err(|e| ApiError::bad_request(e.to_string()))?;

            if scope_allows(scope, &[SearchScope::Transaction]) {
                let tx_result = state
                    .store
                    .get_tx_location(&hash_bytes)
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                if let Some((block_num, _)) = tx_result {
                    results.push(SearchResult {
                        result_type: "transaction".to_string(),
                        id: hash_query.clone(),
                        label: format!("Transaction in Block #{}", block_num),
                        url: format!("/tx/{}", hash_query),
                        match_kind: "exact_hash".to_string(),
                    });
                }
            }

            if scope_allows(scope, &[SearchScope::Block]) {
                let block_result = state
                    .store
                    .get_block_number_by_hash(&hash_bytes)
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                if let Some(block_num) = block_result {
                    results.push(SearchResult {
                        result_type: "block".to_string(),
                        id: block_num.to_string(),
                        label: format!("Block #{}", block_num),
                        url: format!("/blocks/{}", hash_query),
                        match_kind: "exact_hash".to_string(),
                    });
                }
            }

            if scope_allows(scope, &[SearchScope::Address]) {
                let addr_balance = state
                    .store
                    .get_addr_balance(&hash_bytes)
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                if let Some(ab) = addr_balance {
                    if ab.total_cells_count > 0 || ab.txs_count > 0 || ab.balance > 0 {
                        results.push(SearchResult {
                            result_type: "address".to_string(),
                            id: hash_query.clone(),
                            label: format!("Address ({} cells)", ab.total_cells_count),
                            url: format!("/address/{}", hash_query),
                            match_kind: "exact_hash".to_string(),
                        });
                    }
                }
            }

            if scope_allows(scope, &[SearchScope::Script]) {
                let script = state
                    .store
                    .get_script_info(&hash_bytes)
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                if let Some(script_info) = script {
                    let label = script_info
                        .name
                        .as_deref()
                        .filter(|name| is_known_script_name(Some(name)))
                        .map(|name| format!("Script {}", name))
                        .unwrap_or_else(|| "Script".to_string());
                    results.push(SearchResult {
                        result_type: "script".to_string(),
                        id: hash_query.clone(),
                        label,
                        url: format!("/script/{}", hash_query),
                        match_kind: "exact_hash".to_string(),
                    });
                }
            }

            if scope_allows(scope, &[SearchScope::Token, SearchScope::Asset]) {
                let token = state
                    .store
                    .get_token(&hash_bytes)
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                if let Some(token_info) = token {
                    let label_name = token_info
                        .symbol
                        .as_deref()
                        .or(token_info.name.as_deref())
                        .unwrap_or("Unknown token");
                    results.push(SearchResult {
                        result_type: "token".to_string(),
                        id: hash_query.clone(),
                        label: format!("Token {}", label_name),
                        url: format!("/tokens/{}", hash_query),
                        match_kind: "exact_hash".to_string(),
                    });
                }
            }

            if scope_allows(
                scope,
                &[SearchScope::Spore, SearchScope::Cluster, SearchScope::Asset],
            ) {
                let spore_or_cluster = state
                    .store
                    .get_spore(&hash_bytes)
                    .map_err(|e| ApiError::internal(e.to_string()))?;

                if let Some(entry) = spore_or_cluster {
                    if entry.standard.is_cluster() {
                        results.push(SearchResult {
                            result_type: "cluster".to_string(),
                            id: hash_query.clone(),
                            label: format!(
                                "Cluster {}",
                                entry.name.as_deref().unwrap_or("Unnamed cluster")
                            ),
                            url: format!("/clusters/{}", hash_query),
                            match_kind: "exact_hash".to_string(),
                        });
                    } else {
                        results.push(SearchResult {
                            result_type: "spore".to_string(),
                            id: hash_query.clone(),
                            label: format!(
                                "Spore {}",
                                entry.name.as_deref().unwrap_or("Unnamed spore")
                            ),
                            url: format!("/nfts/{}", hash_query),
                            match_kind: "exact_hash".to_string(),
                        });
                    }
                }
            }
        }
    }

    if scope_allows(scope, &[SearchScope::Address]) && is_ckb_address(scoped_query) {
        let lock_hash = address_to_lock_script_hash(scoped_query)
            .map_err(|e| ApiError::bad_request(format!("invalid CKB address: {}", e)))?;
        let addr_balance = state
            .store
            .get_addr_balance(&lock_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        if let Some(ab) = addr_balance {
            if ab.total_cells_count > 0 || ab.txs_count > 0 || ab.balance > 0 {
                results.push(SearchResult {
                    result_type: "address".to_string(),
                    id: scoped_query.to_string(),
                    label: format!("Address ({} cells)", ab.total_cells_count),
                    url: format!("/address/{}", scoped_query),
                    match_kind: "exact_address".to_string(),
                });
            }
        }
    }

    if scope_allows(scope, &[SearchScope::Cell]) {
        if let Some(outpoint) = parse_outpoint(scoped_query) {
            let live = state
                .store
                .get_cell(&outpoint.tx_hash_bytes, outpoint.output_index)
                .map_err(|e| ApiError::internal(e.to_string()))?;

            let consumed = if live.is_none() {
                state
                    .store
                    .get_consumed_cell(&outpoint.tx_hash_bytes, outpoint.output_index)
                    .map_err(|e| ApiError::internal(e.to_string()))?
            } else {
                None
            };

            match (live, consumed) {
                (Some(cell), _) => {
                    results.push(SearchResult {
                        result_type: "cell".to_string(),
                        id: outpoint.normalized.clone(),
                        label: format!("Cell (Live, {} CKB)", format_capacity(cell.capacity)),
                        url: format!("/cell/{}", outpoint.normalized),
                        match_kind: "outpoint".to_string(),
                    });
                }
                (None, Some(cell)) => {
                    results.push(SearchResult {
                        result_type: "cell".to_string(),
                        id: outpoint.normalized.clone(),
                        label: format!("Cell (Dead, {} CKB)", format_capacity(cell.capacity)),
                        url: format!("/cell/{}", outpoint.normalized),
                        match_kind: "outpoint".to_string(),
                    });
                }
                _ => {}
            }
        }
    }

    if scoped_query.len() >= 2 {
        let pattern = scoped_query.to_ascii_lowercase();

        if scope_allows(scope, &[SearchScope::Script]) {
            let mut script_matches: Vec<_> = state
                .store
                .list_script_infos()
                .map_err(|e| ApiError::internal(e.to_string()))?
                .into_iter()
                .filter_map(|(code_hash, info)| {
                    let name = info.name?;
                    if !is_known_script_name(Some(&name)) {
                        return None;
                    }
                    if !name.to_ascii_lowercase().contains(&pattern) {
                        return None;
                    }
                    Some((code_hash, name))
                })
                .collect();

            script_matches.sort_by(|a, b| a.1.cmp(&b.1));
            for (code_hash, name) in script_matches.into_iter().take(NAME_MATCH_LIMIT) {
                let hash = format!("0x{}", hex::encode(code_hash));
                results.push(SearchResult {
                    result_type: "script".to_string(),
                    id: hash.clone(),
                    label: format!("Script {}", name),
                    url: format!("/script/{}", hash),
                    match_kind: "name_contains".to_string(),
                });
            }
        }

        if scope_allows(scope, &[SearchScope::Token, SearchScope::Asset]) {
            let mut token_matches: Vec<_> = state
                .store
                .list_tokens()
                .map_err(|e| ApiError::internal(e.to_string()))?
                .into_iter()
                .filter_map(|(type_hash, info)| {
                    let name_match = info
                        .name
                        .as_ref()
                        .map(|name| name.to_ascii_lowercase().contains(&pattern))
                        .unwrap_or(false);
                    let symbol_match = info
                        .symbol
                        .as_ref()
                        .map(|symbol| symbol.to_ascii_lowercase().contains(&pattern))
                        .unwrap_or(false);
                    if !name_match && !symbol_match {
                        return None;
                    }
                    Some((type_hash, info, name_match, symbol_match))
                })
                .collect();

            token_matches.sort_by(|a, b| b.1.holders_count.cmp(&a.1.holders_count));
            for (type_hash, info, name_match, symbol_match) in
                token_matches.into_iter().take(NAME_MATCH_LIMIT)
            {
                let type_hash_hex = format!("0x{}", hex::encode(type_hash));
                let display = info
                    .symbol
                    .as_deref()
                    .or(info.name.as_deref())
                    .unwrap_or("Unknown token");
                results.push(SearchResult {
                    result_type: "token".to_string(),
                    id: type_hash_hex.clone(),
                    label: format!("Token {}", display),
                    url: format!("/tokens/{}", type_hash_hex),
                    match_kind: if name_match {
                        "name_contains".to_string()
                    } else if symbol_match {
                        "symbol_contains".to_string()
                    } else {
                        "name_contains".to_string()
                    },
                });
            }
        }

        if scope_allows(scope, &[SearchScope::Cluster, SearchScope::Asset]) {
            let mut cluster_matches: Vec<_> = state
                .store
                .list_cluster_aggregates()
                .map_err(|e| ApiError::internal(e.to_string()))?
                .into_iter()
                .filter_map(|(cluster_id, agg)| {
                    let name = agg.name?;
                    if !name.to_ascii_lowercase().contains(&pattern) {
                        return None;
                    }
                    Some((cluster_id, name, agg.total_count))
                })
                .collect();

            cluster_matches.sort_by(|a, b| b.2.cmp(&a.2));
            for (cluster_id, name, _) in cluster_matches.into_iter().take(NAME_MATCH_LIMIT) {
                let cluster_hex = format!("0x{}", hex::encode(cluster_id));
                results.push(SearchResult {
                    result_type: "cluster".to_string(),
                    id: cluster_hex.clone(),
                    label: format!("Cluster {}", name),
                    url: format!("/clusters/{}", cluster_hex),
                    match_kind: "name_contains".to_string(),
                });
            }
        }

        if scope_allows(scope, &[SearchScope::Asset]) {
            let mut nft_collection_matches: Vec<_> = state
                .store
                .list_nft_collection_aggregates()
                .map_err(|e| ApiError::internal(e.to_string()))?
                .into_iter()
                .filter_map(|(collection_id, agg)| {
                    let name = agg.name?;
                    if !name.to_ascii_lowercase().contains(&pattern) {
                        return None;
                    }
                    Some((collection_id, name, agg.live_count))
                })
                .collect();

            nft_collection_matches.sort_by(|a, b| b.2.cmp(&a.2));
            for (collection_id, name, _) in
                nft_collection_matches.into_iter().take(NAME_MATCH_LIMIT)
            {
                let collection_hex = format!("0x{}", hex::encode(collection_id));
                results.push(SearchResult {
                    result_type: "nft".to_string(),
                    id: collection_hex.clone(),
                    label: format!("NFT Collection {}", name),
                    url: format!("/nfts/{}", collection_hex),
                    match_kind: "name_contains".to_string(),
                });
            }
        }

        if scope_allows(scope, &[SearchScope::Spore]) {
            let mut spore_matches: Vec<_> = state
                .store
                .list_spores(SPORE_NAME_SCAN_LIMIT)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .into_iter()
                .filter_map(|(spore_id, entry)| {
                    if entry.standard.is_cluster() {
                        return None;
                    }
                    let name = entry.name?;
                    if !name.to_ascii_lowercase().contains(&pattern) {
                        return None;
                    }
                    Some((spore_id, name, entry.created_at_block))
                })
                .collect();

            spore_matches.sort_by(|a, b| b.2.cmp(&a.2));
            for (spore_id, name, _) in spore_matches.into_iter().take(NAME_MATCH_LIMIT) {
                let spore_hex = format!("0x{}", hex::encode(spore_id));
                results.push(SearchResult {
                    result_type: "spore".to_string(),
                    id: spore_hex.clone(),
                    label: format!("Spore {}", name),
                    url: format!("/nfts/{}", spore_hex),
                    match_kind: "name_contains".to_string(),
                });
            }
        }
    }

    let ambiguous = results.results.len() > 1;
    ok(SearchResponse {
        results: results.results,
        query: query.to_string(),
        normalized_query,
        ambiguous,
    })
}

fn format_capacity(capacity: i64) -> String {
    let ckb = capacity as f64 / 1e8;
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
    fn test_parse_scope_prefixed() {
        let (scope, query) = parse_scope("tx:0xabc");
        assert_eq!(scope, SearchScope::Transaction);
        assert_eq!(query, "0xabc");
    }

    #[test]
    fn test_normalize_hash32_without_prefix() {
        let hash = "A".repeat(64);
        assert_eq!(
            normalize_hash32(&hash),
            Some(format!("0x{}", "a".repeat(64)))
        );
    }

    #[test]
    fn test_parse_outpoint_colon_hex_index() {
        let hash = format!("0x{}", "b".repeat(64));
        let parsed = parse_outpoint(&format!("{}:0x2", hash)).expect("valid outpoint");
        assert_eq!(parsed.output_index, 2);
        assert_eq!(parsed.normalized, format!("{}-2", hash));
    }

    #[test]
    fn test_normalized_query_prefers_outpoint_shape() {
        let hash = format!("0x{}", "c".repeat(64));
        assert_eq!(
            normalized_query_for_response(&format!("{}#3", hash)),
            format!("{}-3", hash)
        );
    }

    #[test]
    fn test_format_capacity_small() {
        assert_eq!(format_capacity(10_000_000_000), "100.00");
    }

    #[test]
    fn test_search_response_serialization() {
        let resp = SearchResponse {
            results: vec![SearchResult {
                result_type: "block".to_string(),
                id: "100".to_string(),
                label: "Block #100".to_string(),
                url: "/blocks/100".to_string(),
                match_kind: "exact_number".to_string(),
            }],
            query: "100".to_string(),
            normalized_query: "100".to_string(),
            ambiguous: false,
        };

        let json = serde_json::to_value(&resp).expect("serialize search response");
        assert_eq!(json["query"], "100");
        assert_eq!(json["normalizedQuery"], "100");
        assert_eq!(json["ambiguous"], false);
        assert_eq!(json["results"][0]["resultType"], "block");
        assert_eq!(json["results"][0]["matchKind"], "exact_number");
    }
}
