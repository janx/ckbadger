use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/assets", get(list_assets))
}

// ============================================
// Request/Response Types
// ============================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListAssetsParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(rename = "type")]
    asset_type: Option<String>,
    cursor: Option<String>,
    search: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetResponse {
    pub id: String,
    pub asset_type: String,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub icon_url: Option<String>,
    pub published: bool,
    pub famous: bool,
    pub tags: Option<Vec<String>>,
    pub holders_count: u64,
    pub transfers_count: u64,
    pub transfers_24h: u64,
    pub decimals: Option<u8>,
    pub total_supply: Option<String>,
    pub content_type: Option<String>,
    pub content_size: Option<u32>,
    pub cluster_id: Option<String>,
    pub cluster_name: Option<String>,
}

// ============================================
// Query Row Types
// ============================================

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct TokenQueryRow {
    type_script_hash: [u8; 32],
    standard: String,
    name: String,
    symbol: String,
    icon_url: String,
    published: u8,
    famous: u8,
    tags: Vec<String>,
    holders_count: u32,
    transfers_count: u64,
    transfers_24h: u64,
    decimals: u8,
    total_supply: String,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
#[allow(dead_code)]
struct SporeClusterQueryRow {
    cluster_id: [u8; 32],
    name: String,
    description: String,
    spores_count: u32,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
#[allow(dead_code)]
struct MnftClassQueryRow {
    class_id: String,
    name: String,
    description: String,
    total: u32,
    issued: u32,
    holders_count: u32,
    transfers_count: u64,
    transfers_24h: u32,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct CountRow {
    count: u64,
}

// ============================================
// Conversions
// ============================================

impl From<TokenQueryRow> for AssetResponse {
    fn from(row: TokenQueryRow) -> Self {
        Self {
            id: format!("0x{}", hex::encode(row.type_script_hash)),
            asset_type: "token".to_string(),
            standard: row.standard,
            name: if row.name.is_empty() {
                None
            } else {
                Some(row.name)
            },
            symbol: if row.symbol.is_empty() {
                None
            } else {
                Some(row.symbol)
            },
            icon_url: if row.icon_url.is_empty() {
                None
            } else {
                Some(row.icon_url)
            },
            published: row.published == 1,
            famous: row.famous == 1,
            tags: if row.tags.is_empty() {
                None
            } else {
                Some(row.tags)
            },
            holders_count: row.holders_count as u64,
            transfers_count: row.transfers_count,
            transfers_24h: row.transfers_24h,
            decimals: Some(row.decimals),
            total_supply: Some(row.total_supply),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
        }
    }
}

impl From<SporeClusterQueryRow> for AssetResponse {
    fn from(row: SporeClusterQueryRow) -> Self {
        Self {
            id: format!("0x{}", hex::encode(row.cluster_id)),
            asset_type: "dob".to_string(),
            standard: "spore".to_string(),
            name: if row.name.is_empty() {
                None
            } else {
                Some(row.name)
            },
            symbol: None,
            icon_url: None,
            published: false,
            famous: false,
            tags: None,
            holders_count: 0, // TODO: Calculate from spore_cells owners
            transfers_count: 0,
            transfers_24h: 0,
            decimals: None,
            total_supply: Some(row.spores_count.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
        }
    }
}

impl From<MnftClassQueryRow> for AssetResponse {
    fn from(row: MnftClassQueryRow) -> Self {
        Self {
            id: row.class_id,
            asset_type: "nft".to_string(),
            standard: "m-nft".to_string(),
            name: if row.name.is_empty() {
                None
            } else {
                Some(row.name)
            },
            symbol: None,
            icon_url: None,
            published: false,
            famous: false,
            tags: None,
            holders_count: row.holders_count as u64,
            transfers_count: row.transfers_count,
            transfers_24h: row.transfers_24h as u64,
            decimals: None,
            total_supply: Some(row.total.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
        }
    }
}

// ============================================
// Route Handlers
// ============================================

async fn list_assets(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListAssetsParams>,
) -> ApiResult<CursorPaginatedResponse<AssetResponse>> {
    let asset_type = params.asset_type.as_deref().unwrap_or("token");

    match asset_type {
        "token" => list_tokens(&state, &params).await,
        "dob" => list_dobs(&state, &params).await,
        "nft" => list_nfts(&state, &params).await,
        _ => Err(ApiError::bad_request(
            "Invalid asset type. Use 'token', 'dob', or 'nft'",
        )),
    }
}

async fn list_tokens(
    state: &Arc<AppState>,
    params: &ListAssetsParams,
) -> ApiResult<CursorPaginatedResponse<AssetResponse>> {
    let search_condition = if let Some(ref search) = params.search {
        let search_escaped = search.replace('\'', "''");
        format!(
            "AND (lower(name) LIKE lower('%{}%') OR lower(symbol) LIKE lower('%{}%'))",
            search_escaped, search_escaped
        )
    } else {
        String::new()
    };

    let cursor_condition = if let Some(ref cursor) = params.cursor {
        let cursor_bytes = hex::decode(cursor.trim_start_matches("0x"))
            .map_err(|_| ApiError::bad_request("Invalid cursor format"))?;
        if cursor_bytes.len() != 32 {
            return Err(ApiError::bad_request("Cursor must be a 32-byte hash"));
        }
        format!(
            "AND type_script_hash > unhex('{}')",
            hex::encode(&cursor_bytes)
        )
    } else {
        String::new()
    };

    // Query tokens
    let query = format!(
        "SELECT type_script_hash, standard, name, symbol, icon_url, \
         published, famous, tags, holders_count, transfers_count, \
         transfers_24h, decimals, toString(total_supply) as total_supply \
         FROM tokens FINAL \
         WHERE 1=1 {} {} \
         ORDER BY type_script_hash ASC \
         LIMIT {}",
        search_condition,
        cursor_condition,
        params.limit + 1
    );

    let mut rows: Vec<TokenQueryRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query tokens: {}", e)))?;

    let has_more = rows.len() as i64 > params.limit;
    if has_more {
        rows.pop();
    }

    // Get total count
    let count_query = format!(
        "SELECT count() as count FROM tokens FINAL WHERE 1=1 {}",
        search_condition
    );
    let count_row: Option<CountRow> = state
        .pool
        .query_one(&count_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to count tokens: {}", e)))?;
    let total = count_row.map(|r| r.count).unwrap_or(0);

    let next_cursor = if has_more {
        rows.last()
            .map(|r| format!("0x{}", hex::encode(r.type_script_hash)))
    } else {
        None
    };

    let assets: Vec<AssetResponse> = rows.into_iter().map(|r| r.into()).collect();
    ok(CursorPaginatedResponse::new(
        assets,
        total as i64,
        params.limit,
        next_cursor,
    ))
}

async fn list_dobs(
    state: &Arc<AppState>,
    params: &ListAssetsParams,
) -> ApiResult<CursorPaginatedResponse<AssetResponse>> {
    let search_condition = if let Some(ref search) = params.search {
        let search_escaped = search.replace('\'', "''");
        format!("AND lower(name) LIKE lower('%{}%')", search_escaped)
    } else {
        String::new()
    };

    let cursor_condition = if let Some(ref cursor) = params.cursor {
        let cursor_bytes = hex::decode(cursor.trim_start_matches("0x"))
            .map_err(|_| ApiError::bad_request("Invalid cursor format"))?;
        if cursor_bytes.len() != 32 {
            return Err(ApiError::bad_request("Cursor must be a 32-byte hash"));
        }
        format!("AND cluster_id > unhex('{}')", hex::encode(&cursor_bytes))
    } else {
        String::new()
    };

    // Query spore clusters
    let query = format!(
        "SELECT cluster_id, name, description, spores_count \
         FROM spore_clusters FINAL \
         WHERE 1=1 {} {} \
         ORDER BY cluster_id ASC \
         LIMIT {}",
        search_condition,
        cursor_condition,
        params.limit + 1
    );

    let mut rows: Vec<SporeClusterQueryRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query spore clusters: {}", e)))?;

    let has_more = rows.len() as i64 > params.limit;
    if has_more {
        rows.pop();
    }

    // Get total count
    let count_query = format!(
        "SELECT count() as count FROM spore_clusters FINAL WHERE 1=1 {}",
        search_condition
    );
    let count_row: Option<CountRow> = state
        .pool
        .query_one(&count_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to count spore clusters: {}", e)))?;
    let total = count_row.map(|r| r.count).unwrap_or(0);

    let next_cursor = if has_more {
        rows.last()
            .map(|r| format!("0x{}", hex::encode(r.cluster_id)))
    } else {
        None
    };

    let assets: Vec<AssetResponse> = rows.into_iter().map(|r| r.into()).collect();
    ok(CursorPaginatedResponse::new(
        assets,
        total as i64,
        params.limit,
        next_cursor,
    ))
}

async fn list_nfts(
    state: &Arc<AppState>,
    params: &ListAssetsParams,
) -> ApiResult<CursorPaginatedResponse<AssetResponse>> {
    let search_condition = if let Some(ref search) = params.search {
        let search_escaped = search.replace('\'', "''");
        format!("AND lower(name) LIKE lower('%{}%')", search_escaped)
    } else {
        String::new()
    };

    let cursor_condition = if let Some(ref cursor) = params.cursor {
        let cursor_escaped = cursor.replace('\'', "''");
        format!("AND class_id > '{}'", cursor_escaped)
    } else {
        String::new()
    };

    // Query M-NFT classes
    let query = format!(
        "SELECT class_id, name, description, total, issued, \
         holders_count, transfers_count, transfers_24h \
         FROM mnft_classes FINAL \
         WHERE is_live = 1 {} {} \
         ORDER BY class_id ASC \
         LIMIT {}",
        search_condition,
        cursor_condition,
        params.limit + 1
    );

    let mut rows: Vec<MnftClassQueryRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query M-NFT classes: {}", e)))?;

    let has_more = rows.len() as i64 > params.limit;
    if has_more {
        rows.pop();
    }

    // Get total count
    let count_query = format!(
        "SELECT count() as count FROM mnft_classes FINAL WHERE is_live = 1 {}",
        search_condition
    );
    let count_row: Option<CountRow> = state
        .pool
        .query_one(&count_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to count M-NFT classes: {}", e)))?;
    let total = count_row.map(|r| r.count).unwrap_or(0);

    let next_cursor = if has_more {
        rows.last().map(|r| r.class_id.clone())
    } else {
        None
    };

    let assets: Vec<AssetResponse> = rows.into_iter().map(|r| r.into()).collect();
    ok(CursorPaginatedResponse::new(
        assets,
        total as i64,
        params.limit,
        next_cursor,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_response_serialization() {
        let response = AssetResponse {
            id: "0x1234".to_string(),
            asset_type: "token".to_string(),
            standard: "sudt".to_string(),
            name: Some("Test Token".to_string()),
            symbol: Some("TST".to_string()),
            icon_url: None,
            published: true,
            famous: false,
            tags: Some(vec!["defi".to_string()]),
            holders_count: 100,
            transfers_count: 5000,
            transfers_24h: 50,
            decimals: Some(8),
            total_supply: Some("1000000".to_string()),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"assetType\":\"token\""));
        assert!(json.contains("\"holdersCount\":100"));
        assert!(json.contains("\"transfers24h\":50"));
    }

    #[test]
    fn test_token_to_asset_response() {
        let row = TokenQueryRow {
            type_script_hash: [0u8; 32],
            standard: "sudt".to_string(),
            name: "Test".to_string(),
            symbol: "TST".to_string(),
            icon_url: "".to_string(),
            published: 1,
            famous: 0,
            tags: vec![],
            holders_count: 10,
            transfers_count: 100,
            transfers_24h: 5,
            decimals: 8,
            total_supply: "1000000".to_string(),
        };
        let asset: AssetResponse = row.into();
        assert_eq!(asset.asset_type, "token");
        assert_eq!(asset.standard, "sudt");
        assert!(asset.published);
        assert!(!asset.famous);
    }

    #[test]
    fn test_spore_cluster_to_asset_response() {
        let row = SporeClusterQueryRow {
            cluster_id: [1u8; 32],
            name: "My Collection".to_string(),
            description: "A test collection".to_string(),
            spores_count: 100,
        };
        let asset: AssetResponse = row.into();
        assert_eq!(asset.asset_type, "dob");
        assert_eq!(asset.standard, "spore");
        assert_eq!(asset.total_supply, Some("100".to_string()));
    }

    #[test]
    fn test_mnft_class_to_asset_response() {
        let row = MnftClassQueryRow {
            class_id: "0x1234567890".to_string(),
            name: "My NFT".to_string(),
            description: "Test NFT".to_string(),
            total: 1000,
            issued: 500,
            holders_count: 200,
            transfers_count: 2000,
            transfers_24h: 20,
        };
        let asset: AssetResponse = row.into();
        assert_eq!(asset.asset_type, "nft");
        assert_eq!(asset.standard, "m-nft");
        assert_eq!(asset.holders_count, 200);
    }
}
