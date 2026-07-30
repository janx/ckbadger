use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
    pub message: String,
}

impl ApiError {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (StatusCode::NOT_FOUND, Json(Self::new("not_found", message)))
    }

    pub fn bad_request(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::BAD_REQUEST,
            Json(Self::new("bad_request", message)),
        )
    }

    pub fn internal(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Self::new("internal_error", message)),
        )
    }

    /// A state that only exists while the indexer is still starting up, and that
    /// resolves on its own as sync progresses (e.g. the genesis baseline or the
    /// first daily snapshot has not been written yet).
    ///
    /// Must stay 503 + `initializing`: that pair is the contract the pre-sync
    /// router serves and the one the SPA's `isNetworkInitializingError` keys its
    /// retry-with-banner UX on. A 500 here reads as a server fault and gets the
    /// error screen instead.
    pub fn initializing(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Self::new("initializing", message)),
        )
    }

    pub fn warmup_pending(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Self::new("warmup_pending", message)),
        )
    }

    pub fn unauthorized(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::UNAUTHORIZED,
            Json(Self::new("unauthorized", message)),
        )
    }
}

/// Cursor-based pagination response for efficient large dataset traversal.
/// Uses keyset pagination instead of OFFSET for O(1) page access.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorPaginatedResponse<T> {
    pub data: Vec<T>,
    /// Total count of items. Null when count is expensive (large table scans).
    /// Use pre-aggregated counts from sync:status or address_balances when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
    pub limit: i64,
    pub has_more: bool,
    /// Cursor for the next page (null if no more data)
    pub next_cursor: Option<String>,
}

impl<T: Serialize + Clone> CursorPaginatedResponse<T> {
    pub fn new(data: Vec<T>, total: i64, limit: i64, next_cursor: Option<String>) -> Self {
        let has_more = next_cursor.is_some();
        Self {
            data,
            total: Some(total),
            limit,
            has_more,
            next_cursor,
        }
    }

    /// Create a response without a total count (avoids expensive COUNT queries).
    /// has_more is determined by fetching limit+1 rows.
    pub fn without_total(data: Vec<T>, limit: i64, next_cursor: Option<String>) -> Self {
        let has_more = next_cursor.is_some();
        Self {
            data,
            total: None,
            limit,
            has_more,
            next_cursor,
        }
    }
}

/// Encode cursor from block_number and index (for transactions, cells, etc.)
pub fn encode_cursor(block_number: i64, index: i32) -> String {
    format!("{}:{}", block_number, index)
}

/// Encode cursor from single i64 value (for tables with simple id ordering)
pub fn encode_cursor_single(id: i64) -> String {
    id.to_string()
}

/// Decode cursor to single i64 value
pub fn decode_cursor_single(cursor: &str) -> Option<i64> {
    cursor.parse().ok()
}

pub type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

/// Common error type used by route handlers.
pub type ApiRouteError = (StatusCode, Json<ApiError>);

/// Sync status for WebSocket broadcasts and the statistics endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusResponse {
    pub is_syncing: bool,
    pub synced_block: i64,
    pub tip_block: i64,
    pub progress: f64,
    pub estimated_time: Option<String>,
    pub chart_data_may_be_incomplete: bool,
    pub blocks_per_second: Option<f64>,
    pub ema_blocks_per_second: Option<f64>,
    pub txs_per_second: Option<f64>,
    pub ema_txs_per_second: Option<f64>,
    pub sync_mode: String,
    pub started_at: Option<i64>,
    pub elapsed_time: Option<String>,
    pub total_time: Option<String>,
}

pub fn ok<T: Serialize>(data: T) -> ApiResult<T> {
    Ok(Json(data))
}

/// Default pagination limit shared across all route modules.
pub fn default_limit() -> i64 {
    50
}

/// Map CKB script hash_type integer to its string representation.
/// Returns `None` for unknown hash_type values so callers must handle them explicitly.
pub fn hash_type_to_str(hash_type: i16) -> Option<&'static str> {
    match hash_type {
        0 => Some("data"),
        1 => Some("type"),
        2 => Some("data1"),
        4 => Some("data2"),
        _ => None,
    }
}

/// Shared script response type used by transaction, cell, and other route modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResponse {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFamilyListItemResponse {
    pub family_id: String,
    pub name: String,
    pub description: Option<String>,
    pub script_kind: Option<String>,
    pub deprecated: bool,
    pub website: Option<String>,
    pub live_cells_count: i64,
    pub cells_count: i64,
    pub owned_capacity_sum: String,
    pub owned_knowledge_sum: String,
    pub versions_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptObservedReferenceResponse {
    pub reference_hash: String,
    pub hash_type: String,
    pub live_cells_count: i64,
    pub cells_count: i64,
    pub owned_capacity_sum: String,
    pub owned_knowledge_sum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptVersionDeploymentResponse {
    pub hash_type: String,
    pub type_reference_hash: Option<String>,
    pub data_reference_hash: String,
    pub code_cell_tx_hash: String,
    pub code_cell_output_index: i32,
    pub deployed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptVersionDetailResponse {
    pub version_hash: String,
    pub name: String,
    pub description: Option<String>,
    pub script_kind: Option<String>,
    pub website: Option<String>,
    pub deprecated: bool,
    pub canonical_reference_hash: Option<String>,
    pub canonical_hash_type: Option<String>,
    pub deployed_at: Option<i64>,
    pub live_cells_count: i64,
    pub cells_count: i64,
    pub owned_capacity_sum: String,
    pub owned_knowledge_sum: String,
    pub code_cells_live_count: i64,
    pub code_cells_total: i64,
    pub deployments: Vec<ScriptVersionDeploymentResponse>,
    pub references: Vec<ScriptObservedReferenceResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFamilyDetailResponse {
    pub family_id: String,
    pub name: String,
    pub description: Option<String>,
    pub script_kind: Option<String>,
    pub website: Option<String>,
    pub live_cells_count: i64,
    pub cells_count: i64,
    pub owned_capacity_sum: String,
    pub owned_knowledge_sum: String,
    pub versions_count: i64,
    pub versions: Vec<ScriptVersionDetailResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartDataPoint {
    pub date: String,
    pub value: String,
    pub value2: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartResponse {
    pub data: Vec<ChartDataPoint>,
    pub title: String,
    pub y_axis_label: String,
    pub y2_axis_label: Option<String>,
}

pub fn chart_response_has_data(response: &ChartResponse) -> bool {
    !response.data.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_paginated_response_with_total() {
        let resp = CursorPaginatedResponse::new(vec![1, 2, 3], 100, 10, Some("5:0".to_string()));
        assert_eq!(resp.total, Some(100));
        assert!(resp.has_more);
        assert_eq!(resp.limit, 10);
        assert_eq!(resp.data, vec![1, 2, 3]);
        assert_eq!(resp.next_cursor, Some("5:0".to_string()));
    }

    #[test]
    fn test_cursor_paginated_response_without_total() {
        let resp =
            CursorPaginatedResponse::without_total(vec![1, 2, 3], 10, Some("5:0".to_string()));
        assert_eq!(resp.total, None);
        assert!(resp.has_more);
    }

    #[test]
    fn test_cursor_paginated_response_without_total_no_more() {
        let resp = CursorPaginatedResponse::<i32>::without_total(vec![1, 2], 10, None);
        assert_eq!(resp.total, None);
        assert!(!resp.has_more);
        assert_eq!(resp.next_cursor, None);
    }

    #[test]
    fn test_without_total_serialization_omits_total() {
        let resp = CursorPaginatedResponse::without_total(vec!["a"], 10, None);
        let json = serde_json::to_value(&resp).unwrap();
        assert!(
            json.get("total").is_none(),
            "total should be omitted from JSON when None"
        );
        assert_eq!(json["hasMore"], false);
        assert_eq!(json["limit"], 10);
    }

    #[test]
    fn test_with_total_serialization_includes_total() {
        let resp = CursorPaginatedResponse::new(vec!["a"], 42, 10, None);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["total"], 42);
    }

    /// `encode_cursor` writes the shape that `utils::params::parse_block_tx_cursor`
    /// reads. Rejection cases live with that parser; what matters here is that
    /// the two halves still agree.
    #[test]
    fn test_encode_cursor_roundtrips_through_the_shared_parser() {
        let cursor = encode_cursor(12345, 7);
        let (block, idx) = crate::utils::parse_block_tx_cursor(&cursor, "cursor").unwrap();
        assert_eq!(block, 12345);
        assert_eq!(idx, 7);
    }

    #[test]
    fn test_encode_decode_cursor_single_roundtrip() {
        let cursor = encode_cursor_single(99999);
        let id = decode_cursor_single(&cursor).unwrap();
        assert_eq!(id, 99999);
    }

    #[test]
    fn test_decode_cursor_single_invalid() {
        assert_eq!(decode_cursor_single("not_a_number"), None);
    }

    #[test]
    fn test_api_error_serialization() {
        let err = ApiError::new("test_error", "something went wrong");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["error"], "test_error");
        assert_eq!(json["message"], "something went wrong");
    }

    #[test]
    fn test_default_limit() {
        assert_eq!(default_limit(), 50);
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
    fn test_script_response_serialization() {
        let script = ScriptResponse {
            code_hash: "0xabc".to_string(),
            hash_type: "type".to_string(),
            args: "0x1234".to_string(),
        };
        let json = serde_json::to_value(&script).unwrap();
        assert_eq!(json["codeHash"], "0xabc");
        assert_eq!(json["hashType"], "type");
        assert_eq!(json["args"], "0x1234");
    }
}
