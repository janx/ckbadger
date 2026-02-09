use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::tx_block_map::get_block_number_for_tx;
use crate::utils::{address_to_lock_script_hash, is_ckb_address};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/activities", get(list_activities))
        .route("/activities/address/{addr}", get(get_address_activities))
        .route(
            "/activities/transaction/{hash}",
            get(get_transaction_activities),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListActivitiesParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    activity_type: Option<String>,
    activity_category: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityResponse {
    pub activity_id: String,
    pub activity_type: String,
    pub activity_category: String,
    pub block_number: i64,
    pub tx_hash: String,
    pub tx_index: i32,
    pub activity_index: i16,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub from_lock_hash: Option<String>,
    pub to_lock_hash: Option<String>,
    pub amount: String,
    pub asset_id: Option<String>,
    pub metadata: serde_json::Value,
    pub timestamp: String,
}

type ActivityRow = (
    Vec<u8>,                       // activity_id
    String,                        // activity_type
    String,                        // activity_category
    i64,                           // block_number
    Vec<u8>,                       // tx_hash
    i32,                           // tx_index
    i16,                           // activity_index
    Option<Vec<u8>>,               // from_lock_hash
    Option<Vec<u8>>,               // to_lock_hash
    String,                        // amount (as text from NUMERIC)
    Option<Vec<u8>>,               // asset_id
    serde_json::Value,             // metadata
    chrono::DateTime<chrono::Utc>, // timestamp
);

fn encode_activity_cursor(block_number: i64, tx_index: i32, activity_index: i16) -> String {
    format!("{}:{}:{}", block_number, tx_index, activity_index)
}

fn decode_activity_cursor(cursor: &str) -> Option<(i64, i32, i16)> {
    let parts: Vec<&str> = cursor.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let block_number = parts[0].parse().ok()?;
    let tx_index = parts[1].parse().ok()?;
    let activity_index = parts[2].parse().ok()?;
    Some((block_number, tx_index, activity_index))
}

fn row_to_response(row: ActivityRow) -> ActivityResponse {
    let (
        activity_id,
        activity_type,
        activity_category,
        block_number,
        tx_hash,
        tx_index,
        activity_index,
        from_lock_hash,
        to_lock_hash,
        amount,
        asset_id,
        metadata,
        timestamp,
    ) = row;

    ActivityResponse {
        activity_id: format!("0x{}", hex::encode(&activity_id)),
        activity_type,
        activity_category,
        block_number,
        tx_hash: format!("0x{}", hex::encode(&tx_hash)),
        tx_index,
        activity_index,
        from_address: None,
        to_address: None,
        from_lock_hash: from_lock_hash.map(|h| format!("0x{}", hex::encode(&h))),
        to_lock_hash: to_lock_hash.map(|h| format!("0x{}", hex::encode(&h))),
        amount,
        asset_id: asset_id.map(|h| format!("0x{}", hex::encode(&h))),
        metadata,
        timestamp: timestamp.to_rfc3339(),
    }
}

const VALID_CATEGORIES: [&str; 8] = [
    "ckb", "cellbase", "token", "dob", "nft", "dao", "script", "rgbpp",
];

const VALID_TYPES: [&str; 18] = [
    "CKB_TRANSFER",
    "CELLBASE_REWARD",
    "TOKEN_MINT",
    "TOKEN_TRANSFER",
    "TOKEN_BURN",
    "DOB_MINT",
    "DOB_TRANSFER",
    "DOB_BURN",
    "NFT_MINT",
    "NFT_TRANSFER",
    "DAO_DEPOSIT",
    "DAO_WITHDRAW_REQUEST",
    "DAO_WITHDRAW_COMPLETE",
    "SCRIPT_DEPLOY",
    "RGBPP_TRANSFER",
    "RGBPP_LEAP_IN",
    "RGBPP_LEAP_OUT",
    "RGBPP_ISSUANCE",
];

async fn list_activities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<ActivityResponse>> {
    let limit = params.limit.clamp(1, 100);

    if let Some(ref cat) = params.activity_category {
        if !VALID_CATEGORIES.contains(&cat.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Invalid activity_category '{}'. Valid: {}",
                cat,
                VALID_CATEGORIES.join(", ")
            )));
        }
    }

    if let Some(ref typ) = params.activity_type {
        if !VALID_TYPES.contains(&typ.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Invalid activity_type '{}'. Valid: {}",
                typ,
                VALID_TYPES.join(", ")
            )));
        }
    }

    let cursor = params
        .cursor
        .as_ref()
        .and_then(|c| decode_activity_cursor(c));
    let (cursor_block, cursor_tx, cursor_idx) = cursor.unwrap_or((i64::MAX, i32::MAX, i16::MAX));

    let rows: Vec<ActivityRow> = match (&params.activity_type, &params.activity_category) {
        (Some(typ), Some(cat)) => sqlx::query_as(
            r#"
                SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                       tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                       asset_id, metadata, timestamp
                FROM activities
                WHERE activity_type = $1 AND activity_category = $2
                  AND (block_number, tx_index, activity_index) < ($3, $4, $5)
                ORDER BY block_number DESC, tx_index DESC, activity_index DESC
                LIMIT $6
                "#,
        )
        .bind(typ)
        .bind(cat)
        .bind(cursor_block)
        .bind(cursor_tx)
        .bind(cursor_idx)
        .bind(limit + 1)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?,
        (Some(typ), None) => sqlx::query_as(
            r#"
                SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                       tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                       asset_id, metadata, timestamp
                FROM activities
                WHERE activity_type = $1
                  AND (block_number, tx_index, activity_index) < ($2, $3, $4)
                ORDER BY block_number DESC, tx_index DESC, activity_index DESC
                LIMIT $5
                "#,
        )
        .bind(typ)
        .bind(cursor_block)
        .bind(cursor_tx)
        .bind(cursor_idx)
        .bind(limit + 1)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?,
        (None, Some(cat)) => sqlx::query_as(
            r#"
                SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                       tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                       asset_id, metadata, timestamp
                FROM activities
                WHERE activity_category = $1
                  AND (block_number, tx_index, activity_index) < ($2, $3, $4)
                ORDER BY block_number DESC, tx_index DESC, activity_index DESC
                LIMIT $5
                "#,
        )
        .bind(cat)
        .bind(cursor_block)
        .bind(cursor_tx)
        .bind(cursor_idx)
        .bind(limit + 1)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?,
        (None, None) => sqlx::query_as(
            r#"
                SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                       tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                       asset_id, metadata, timestamp
                FROM activities
                WHERE (block_number, tx_index, activity_index) < ($1, $2, $3)
                ORDER BY block_number DESC, tx_index DESC, activity_index DESC
                LIMIT $4
                "#,
        )
        .bind(cursor_block)
        .bind(cursor_tx)
        .bind(cursor_idx)
        .bind(limit + 1)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?,
    };

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|r| encode_activity_cursor(r.3, r.5, r.6))
    } else {
        None
    };

    let data: Vec<ActivityResponse> = rows.into_iter().map(row_to_response).collect();

    ok(CursorPaginatedResponse::without_total(
        data,
        limit,
        next_cursor,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressActivitiesParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    activity_type: Option<String>,
    activity_category: Option<String>,
    direction: Option<String>,
}

async fn get_address_activities(
    State(state): State<Arc<AppState>>,
    Path(addr): Path<String>,
    Query(params): Query<AddressActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<ActivityResponse>> {
    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100);

    if let Some(ref cat) = params.activity_category {
        if !VALID_CATEGORIES.contains(&cat.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Invalid activity_category '{}'. Valid: {}",
                cat,
                VALID_CATEGORIES.join(", ")
            )));
        }
    }

    if let Some(ref typ) = params.activity_type {
        if !VALID_TYPES.contains(&typ.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Invalid activity_type '{}'. Valid: {}",
                typ,
                VALID_TYPES.join(", ")
            )));
        }
    }

    if let Some(ref dir) = params.direction {
        if dir != "in" && dir != "out" && dir != "all" {
            return Err(ApiError::bad_request(
                "Invalid direction. Must be 'in', 'out', or 'all'",
            ));
        }
    }

    let cursor = params
        .cursor
        .as_ref()
        .and_then(|c| decode_activity_cursor(c));
    let (cursor_block, cursor_tx, cursor_idx) = cursor.unwrap_or((i64::MAX, i32::MAX, i16::MAX));

    let direction = params.direction.as_deref().unwrap_or("all");

    let base_condition = match direction {
        "in" => "to_lock_hash = $1",
        "out" => "from_lock_hash = $1",
        _ => "(from_lock_hash = $1 OR to_lock_hash = $1)",
    };

    let rows: Vec<ActivityRow> = match (&params.activity_type, &params.activity_category) {
        (Some(typ), Some(cat)) => {
            let query = format!(
                r#"
                SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                       tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                       asset_id, metadata, timestamp
                FROM activities
                WHERE {} AND activity_type = $2 AND activity_category = $3
                  AND (block_number, tx_index, activity_index) < ($4, $5, $6)
                ORDER BY block_number DESC, tx_index DESC, activity_index DESC
                LIMIT $7
                "#,
                base_condition
            );
            sqlx::query_as(&query)
                .bind(&lock_hash)
                .bind(typ)
                .bind(cat)
                .bind(cursor_block)
                .bind(cursor_tx)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
        (Some(typ), None) => {
            let query = format!(
                r#"
                SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                       tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                       asset_id, metadata, timestamp
                FROM activities
                WHERE {} AND activity_type = $2
                  AND (block_number, tx_index, activity_index) < ($3, $4, $5)
                ORDER BY block_number DESC, tx_index DESC, activity_index DESC
                LIMIT $6
                "#,
                base_condition
            );
            sqlx::query_as(&query)
                .bind(&lock_hash)
                .bind(typ)
                .bind(cursor_block)
                .bind(cursor_tx)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
        (None, Some(cat)) => {
            let query = format!(
                r#"
                SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                       tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                       asset_id, metadata, timestamp
                FROM activities
                WHERE {} AND activity_category = $2
                  AND (block_number, tx_index, activity_index) < ($3, $4, $5)
                ORDER BY block_number DESC, tx_index DESC, activity_index DESC
                LIMIT $6
                "#,
                base_condition
            );
            sqlx::query_as(&query)
                .bind(&lock_hash)
                .bind(cat)
                .bind(cursor_block)
                .bind(cursor_tx)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
        (None, None) => {
            let query = format!(
                r#"
                SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                       tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                       asset_id, metadata, timestamp
                FROM activities
                WHERE {}
                  AND (block_number, tx_index, activity_index) < ($2, $3, $4)
                ORDER BY block_number DESC, tx_index DESC, activity_index DESC
                LIMIT $5
                "#,
                base_condition
            );
            sqlx::query_as(&query)
                .bind(&lock_hash)
                .bind(cursor_block)
                .bind(cursor_tx)
                .bind(cursor_idx)
                .bind(limit + 1)
                .fetch_all(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
    };

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|r| encode_activity_cursor(r.3, r.5, r.6))
    } else {
        None
    };

    let data: Vec<ActivityResponse> = rows.into_iter().map(row_to_response).collect();

    ok(CursorPaginatedResponse::without_total(
        data,
        limit,
        next_cursor,
    ))
}

pub async fn fetch_transaction_activities(
    pool: &sqlx::PgPool,
    tx_hash: &[u8],
) -> Result<Vec<ActivityResponse>, String> {
    let block_number = get_block_number_for_tx(pool, tx_hash).await.ok().flatten();

    let rows: Vec<ActivityRow> = if let Some(bn) = block_number {
        sqlx::query_as(
            r#"
            SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                   tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                   asset_id, metadata, timestamp
            FROM activities
            WHERE tx_hash = $1 AND block_number = $2
            ORDER BY activity_index ASC
            "#,
        )
        .bind(tx_hash)
        .bind(bn)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?
    } else {
        sqlx::query_as(
            r#"
            SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                   tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                   asset_id, metadata, timestamp
            FROM activities
            WHERE tx_hash = $1
            ORDER BY activity_index ASC
            "#,
        )
        .bind(tx_hash)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?
    };

    Ok(rows.into_iter().map(row_to_response).collect())
}

async fn get_transaction_activities(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Vec<ActivityResponse>> {
    let tx_hash = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    if tx_hash.len() != 32 {
        return Err(ApiError::bad_request(
            "Transaction hash must be 32 bytes (64 hex chars)",
        ));
    }

    let block_number = get_block_number_for_tx(&state.read_pool, &tx_hash)
        .await
        .ok()
        .flatten();

    let rows: Vec<ActivityRow> = if let Some(bn) = block_number {
        sqlx::query_as(
            r#"
            SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                   tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                   asset_id, metadata, timestamp
            FROM activities
            WHERE tx_hash = $1 AND block_number = $2
            ORDER BY activity_index ASC
            "#,
        )
        .bind(&tx_hash)
        .bind(bn)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        sqlx::query_as(
            r#"
            SELECT activity_id, activity_type, activity_category, block_number, tx_hash,
                   tx_index, activity_index, from_lock_hash, to_lock_hash, amount::TEXT,
                   asset_id, metadata, timestamp
            FROM activities
            WHERE tx_hash = $1
            ORDER BY activity_index ASC
            "#,
        )
        .bind(&tx_hash)
        .fetch_all(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let data: Vec<ActivityResponse> = rows.into_iter().map(row_to_response).collect();

    ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_activity_cursor() {
        let cursor = encode_activity_cursor(12345, 5, 3);
        assert_eq!(cursor, "12345:5:3");

        let decoded = decode_activity_cursor(&cursor);
        assert_eq!(decoded, Some((12345, 5, 3)));
    }

    #[test]
    fn test_decode_activity_cursor_invalid() {
        assert_eq!(decode_activity_cursor("invalid"), None);
        assert_eq!(decode_activity_cursor("12345:5"), None);
        assert_eq!(decode_activity_cursor("12345:5:3:extra"), None);
        assert_eq!(decode_activity_cursor("abc:def:ghi"), None);
    }

    #[test]
    fn test_valid_categories() {
        assert!(VALID_CATEGORIES.contains(&"ckb"));
        assert!(VALID_CATEGORIES.contains(&"token"));
        assert!(VALID_CATEGORIES.contains(&"rgbpp"));
        assert!(!VALID_CATEGORIES.contains(&"invalid"));
    }

    #[test]
    fn test_valid_types() {
        assert!(VALID_TYPES.contains(&"CKB_TRANSFER"));
        assert!(VALID_TYPES.contains(&"TOKEN_MINT"));
        assert!(VALID_TYPES.contains(&"RGBPP_LEAP_IN"));
        assert!(!VALID_TYPES.contains(&"INVALID_TYPE"));
    }
}
