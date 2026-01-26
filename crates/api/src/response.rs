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
    pub total: i64,
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
            total,
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

pub fn ok<T: Serialize>(data: T) -> ApiResult<T> {
    Ok(Json(data))
}
