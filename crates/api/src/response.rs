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

/// Decode cursor to block_number and index
pub fn decode_cursor(cursor: &str) -> Option<(i64, i32)> {
    let parts: Vec<&str> = cursor.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let block_number = parts[0].parse().ok()?;
    let index = parts[1].parse().ok()?;
    Some((block_number, index))
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

pub fn ok<T: Serialize>(data: T) -> ApiResult<T> {
    Ok(Json(data))
}

/// Default pagination limit shared across all route modules.
pub fn default_limit() -> i64 {
    20
}

/// Map CKB script hash_type integer to its string representation.
pub fn hash_type_to_str(hash_type: i16) -> &'static str {
    match hash_type {
        0 => "data",
        1 => "type",
        2 => "data1",
        4 => "data2",
        _ => "unknown",
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

    #[test]
    fn test_encode_decode_cursor_roundtrip() {
        let cursor = encode_cursor(12345, 7);
        let (block, idx) = decode_cursor(&cursor).unwrap();
        assert_eq!(block, 12345);
        assert_eq!(idx, 7);
    }

    #[test]
    fn test_decode_cursor_invalid() {
        assert_eq!(decode_cursor("invalid"), None);
        assert_eq!(decode_cursor(""), None);
        assert_eq!(decode_cursor("1:2:3"), None);
        assert_eq!(decode_cursor("abc:def"), None);
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
        assert_eq!(default_limit(), 20);
    }

    #[test]
    fn test_hash_type_to_str() {
        assert_eq!(hash_type_to_str(0), "data");
        assert_eq!(hash_type_to_str(1), "type");
        assert_eq!(hash_type_to_str(2), "data1");
        assert_eq!(hash_type_to_str(4), "data2");
        assert_eq!(hash_type_to_str(99), "unknown");
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
