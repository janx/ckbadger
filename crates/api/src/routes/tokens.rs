use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::script_to_address;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tokens", get(list_tokens))
        .route("/tokens/{type_hash}", get(get_token))
        .route("/tokens/{type_hash}/holders", get(get_token_holders))
        .route("/tokens/{type_hash}/transfers", get(get_token_transfers))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    standard: Option<String>,
    cursor: Option<String>,
    search: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Deserialize)]
pub struct HolderParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransferParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

#[derive(Debug, FromRow)]
struct TokenRow {
    id: i64,
    type_script_hash: Vec<u8>,
    type_code_hash: Vec<u8>,
    type_hash_type: i16,
    type_args: Vec<u8>,
    standard: String,
    name: Option<String>,
    symbol: Option<String>,
    decimals: i16,
    description: Option<String>,
    icon_url: Option<String>,
    published: bool,
    famous: bool,
    tags: Option<Vec<String>>,
    udt_type: Option<String>,
    manager: Option<String>,
    email: Option<String>,
    operator_website: Option<String>,
    total_supply: String,
    holders_count: i32,
    transfers_count: i64,
    transfers_24h: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    pub type_script_hash: String,
    pub type_code_hash: String,
    pub type_hash_type: String,
    pub type_args: String,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: i16,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub published: bool,
    pub famous: bool,
    pub tags: Option<Vec<String>>,
    pub udt_type: Option<String>,
    pub manager: Option<String>,
    pub email: Option<String>,
    pub operator_website: Option<String>,
    pub total_supply: String,
    pub holders_count: i32,
    pub transfers_count: i64,
    pub transfers_24h: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHolderResponse {
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub balance: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTransferResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub from_lock_hash: Option<String>,
    pub from_address: Option<String>,
    pub to_lock_hash: String,
    pub to_address: Option<String>,
    pub amount: String,
    pub is_mint: bool,
    pub is_burn: bool,
    pub timestamp: String,
}

async fn list_tokens(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<TokenResponse>> {
    let limit = params.limit.clamp(1, 100);
    let (cursor_24h, cursor_holders, cursor_id) = params
        .cursor
        .as_ref()
        .and_then(|c| {
            let parts: Vec<&str> = c.split(':').collect();
            if parts.len() == 3 {
                Some((
                    parts[0].parse::<i64>().ok()?,
                    parts[1].parse::<i32>().ok()?,
                    parts[2].parse::<i64>().ok()?,
                ))
            } else {
                None
            }
        })
        .unwrap_or((i64::MAX, i32::MAX, i64::MAX));

    let search_hash = params.search.as_ref().and_then(|s| {
        let s = s.strip_prefix("0x").unwrap_or(s);
        hex::decode(s).ok()
    });
    let search_pattern = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", s.to_lowercase()));

    let (total, rows): (i64, Vec<TokenRow>) = match (
        &params.standard,
        &search_hash,
        &search_pattern,
    ) {
        (Some(standard), Some(hash), _) => {
            let total: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM tokens WHERE standard = $1 AND type_script_hash = $2",
            )
            .bind(standard)
            .bind(hash)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            let rows = sqlx::query_as::<_, TokenRow>(
                r#"
                SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard,
                       name, symbol, decimals, description, icon_url, 
                       COALESCE(published, false) AS published, COALESCE(famous, false) AS famous,
                       tags, udt_type, manager, email, operator_website,
                       total_supply::text AS total_supply, holders_count, transfers_count, transfers_24h
                FROM tokens
                WHERE standard = $1 AND type_script_hash = $2 AND (transfers_24h, holders_count, id) < ($3, $4, $5)
                ORDER BY transfers_24h DESC, holders_count DESC, id DESC
                LIMIT $6
                "#,
            )
            .bind(standard)
            .bind(hash)
            .bind(cursor_24h)
            .bind(cursor_holders)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (Some(standard), None, Some(pattern)) => {
            let total: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM tokens WHERE standard = $1 AND (LOWER(name) LIKE $2 OR LOWER(symbol) LIKE $2)",
            )
            .bind(standard)
            .bind(pattern)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            let rows = sqlx::query_as::<_, TokenRow>(
                r#"
                SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard,
                       name, symbol, decimals, description, icon_url,
                       COALESCE(published, false) AS published, COALESCE(famous, false) AS famous,
                       tags, udt_type, manager, email, operator_website,
                       total_supply::text AS total_supply, holders_count, transfers_count, transfers_24h
                FROM tokens
                WHERE standard = $1 AND (LOWER(name) LIKE $2 OR LOWER(symbol) LIKE $2) AND (transfers_24h, holders_count, id) < ($3, $4, $5)
                ORDER BY transfers_24h DESC, holders_count DESC, id DESC
                LIMIT $6
                "#,
            )
            .bind(standard)
            .bind(pattern)
            .bind(cursor_24h)
            .bind(cursor_holders)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (Some(standard), None, None) => {
            let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tokens WHERE standard = $1")
                .bind(standard)
                .fetch_one(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            let rows = sqlx::query_as::<_, TokenRow>(
                r#"
                SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard,
                       name, symbol, decimals, description, icon_url,
                       COALESCE(published, false) AS published, COALESCE(famous, false) AS famous,
                       tags, udt_type, manager, email, operator_website,
                       total_supply::text AS total_supply, holders_count, transfers_count, transfers_24h
                FROM tokens
                WHERE standard = $1 AND (transfers_24h, holders_count, id) < ($2, $3, $4)
                ORDER BY transfers_24h DESC, holders_count DESC, id DESC
                LIMIT $5
                "#,
            )
            .bind(standard)
            .bind(cursor_24h)
            .bind(cursor_holders)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (None, Some(hash), _) => {
            let total: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM tokens WHERE type_script_hash = $1")
                    .bind(hash)
                    .fetch_one(&state.pool)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;

            let rows = sqlx::query_as::<_, TokenRow>(
                r#"
                SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard,
                       name, symbol, decimals, description, icon_url,
                       COALESCE(published, false) AS published, COALESCE(famous, false) AS famous,
                       tags, udt_type, manager, email, operator_website,
                       total_supply::text AS total_supply, holders_count, transfers_count, transfers_24h
                FROM tokens
                WHERE type_script_hash = $1 AND (transfers_24h, holders_count, id) < ($2, $3, $4)
                ORDER BY transfers_24h DESC, holders_count DESC, id DESC
                LIMIT $5
                "#,
            )
            .bind(hash)
            .bind(cursor_24h)
            .bind(cursor_holders)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (None, None, Some(pattern)) => {
            let total: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM tokens WHERE LOWER(name) LIKE $1 OR LOWER(symbol) LIKE $1",
            )
            .bind(pattern)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            let rows = sqlx::query_as::<_, TokenRow>(
                r#"
                SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard,
                       name, symbol, decimals, description, icon_url,
                       COALESCE(published, false) AS published, COALESCE(famous, false) AS famous,
                       tags, udt_type, manager, email, operator_website,
                       total_supply::text AS total_supply, holders_count, transfers_count, transfers_24h
                FROM tokens
                WHERE (LOWER(name) LIKE $1 OR LOWER(symbol) LIKE $1) AND (transfers_24h, holders_count, id) < ($2, $3, $4)
                ORDER BY transfers_24h DESC, holders_count DESC, id DESC
                LIMIT $5
                "#,
            )
            .bind(pattern)
            .bind(cursor_24h)
            .bind(cursor_holders)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (None, None, None) => {
            let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tokens")
                .fetch_one(&state.pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            let rows = sqlx::query_as::<_, TokenRow>(
                r#"
                SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard,
                       name, symbol, decimals, description, icon_url,
                       COALESCE(published, false) AS published, COALESCE(famous, false) AS famous,
                       tags, udt_type, manager, email, operator_website,
                       total_supply::text AS total_supply, holders_count, transfers_count, transfers_24h
                FROM tokens
                WHERE (transfers_24h, holders_count, id) < ($1, $2, $3)
                ORDER BY transfers_24h DESC, holders_count DESC, id DESC
                LIMIT $4
                "#,
            )
            .bind(cursor_24h)
            .bind(cursor_holders)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
    };

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|r| format!("{}:{}:{}", r.transfers_24h, r.holders_count, r.id))
    } else {
        None
    };

    let tokens: Vec<TokenResponse> = rows
        .into_iter()
        .map(|r| TokenResponse {
            type_script_hash: format!("0x{}", hex::encode(&r.type_script_hash)),
            type_code_hash: format!("0x{}", hex::encode(&r.type_code_hash)),
            type_hash_type: hash_type_to_string(r.type_hash_type),
            type_args: format!("0x{}", hex::encode(&r.type_args)),
            standard: r.standard,
            name: r.name,
            symbol: r.symbol,
            decimals: r.decimals,
            description: r.description,
            icon_url: r.icon_url,
            published: r.published,
            famous: r.famous,
            tags: r.tags,
            udt_type: r.udt_type,
            manager: r.manager,
            email: r.email,
            operator_website: r.operator_website,
            total_supply: r.total_supply,
            holders_count: r.holders_count,
            transfers_count: r.transfers_count,
            transfers_24h: r.transfers_24h,
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        tokens,
        total,
        limit,
        next_cursor,
    ))
}

async fn get_token(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
) -> ApiResult<TokenResponse> {
    let hash = hex::decode(type_hash.strip_prefix("0x").unwrap_or(&type_hash))
        .map_err(|_| ApiError::bad_request("Invalid type script hash"))?;

    let row = sqlx::query_as::<_, TokenRow>(
        r#"
        SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard,
               name, symbol, decimals, description, icon_url,
               COALESCE(published, false) AS published, COALESCE(famous, false) AS famous,
               tags, udt_type, manager, email, operator_website,
               total_supply::text AS total_supply, holders_count, transfers_count, transfers_24h
        FROM tokens
        WHERE type_script_hash = $1
        "#,
    )
    .bind(&hash)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    match row {
        Some(r) => ok(TokenResponse {
            type_script_hash: format!("0x{}", hex::encode(&r.type_script_hash)),
            type_code_hash: format!("0x{}", hex::encode(&r.type_code_hash)),
            type_hash_type: hash_type_to_string(r.type_hash_type),
            type_args: format!("0x{}", hex::encode(&r.type_args)),
            standard: r.standard,
            name: r.name,
            symbol: r.symbol,
            decimals: r.decimals,
            description: r.description,
            icon_url: r.icon_url,
            published: r.published,
            famous: r.famous,
            tags: r.tags,
            udt_type: r.udt_type,
            manager: r.manager,
            email: r.email,
            operator_website: r.operator_website,
            total_supply: r.total_supply,
            holders_count: r.holders_count,
            transfers_count: r.transfers_count,
            transfers_24h: r.transfers_24h,
        }),
        None => Err(ApiError::not_found("Token not found")),
    }
}

async fn get_token_holders(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
    Query(params): Query<HolderParams>,
) -> ApiResult<CursorPaginatedResponse<TokenHolderResponse>> {
    let hash = hex::decode(type_hash.strip_prefix("0x").unwrap_or(&type_hash))
        .map_err(|_| ApiError::bad_request("Invalid type script hash"))?;

    let token_row: Option<(i64, i32)> =
        sqlx::query_as("SELECT id, holders_count FROM tokens WHERE type_script_hash = $1")
            .bind(&hash)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let (token_id, holders_count) = match token_row {
        Some((id, count)) => (id, count as i64),
        None => return Err(ApiError::not_found("Token not found")),
    };

    let limit = params.limit.clamp(1, 100);
    let (cursor_balance, cursor_lock) = params
        .cursor
        .as_ref()
        .and_then(|c| {
            let parts: Vec<&str> = c.split(':').collect();
            if parts.len() == 2 {
                let lock_bytes = hex::decode(parts[1]).ok()?;
                Some((parts[0].to_string(), lock_bytes))
            } else {
                None
            }
        })
        .unwrap_or_else(|| (String::new(), Vec::new()));

    type HolderRow = (Vec<u8>, String);

    let rows = if cursor_balance.is_empty() {
        sqlx::query_as::<_, HolderRow>(
            r#"
            SELECT lock_script_hash, balance::text
            FROM token_balances
            WHERE token_id = $1 AND balance > 0
            ORDER BY balance DESC, lock_script_hash DESC
            LIMIT $2
            "#,
        )
        .bind(token_id)
        .bind(limit + 1)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        sqlx::query_as::<_, HolderRow>(
            r#"
            SELECT lock_script_hash, balance::text
            FROM token_balances
            WHERE token_id = $1 AND balance > 0
              AND (balance, lock_script_hash) < ($2::numeric, $3)
            ORDER BY balance DESC, lock_script_hash DESC
            LIMIT $4
            "#,
        )
        .bind(token_id)
        .bind(&cursor_balance)
        .bind(&cursor_lock)
        .bind(limit + 1)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(lock, balance)| format!("{}:{}", balance, hex::encode(lock)))
    } else {
        None
    };

    let lock_hashes: Vec<Vec<u8>> = rows.iter().map(|(lock, _)| lock.clone()).collect();

    let mut address_map: std::collections::HashMap<Vec<u8>, String> =
        std::collections::HashMap::new();

    if !lock_hashes.is_empty() {
        type LockRow = (Vec<u8>, Vec<u8>, i16, Vec<u8>);
        let lock_rows = sqlx::query_as::<_, LockRow>(
            r#"
            SELECT DISTINCT ON (lock_script_hash) 
                   lock_script_hash, lock_code_hash, lock_hash_type, lock_args
            FROM cells
            WHERE lock_script_hash = ANY($1)
            "#,
        )
        .bind(&lock_hashes)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

        let network = &state.ckb_network;
        for (lock_hash, code_hash, hash_type, args) in lock_rows {
            if let Ok(addr) = script_to_address(&code_hash, hash_type, &args, network) {
                address_map.insert(lock_hash, addr);
            }
        }
    }

    let holders: Vec<TokenHolderResponse> = rows
        .into_iter()
        .map(|(lock_script_hash, balance)| {
            let address = address_map.get(&lock_script_hash).cloned();
            TokenHolderResponse {
                lock_script_hash: format!("0x{}", hex::encode(&lock_script_hash)),
                address,
                balance,
            }
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        holders,
        holders_count,
        limit,
        next_cursor,
    ))
}

async fn get_token_transfers(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
    Query(params): Query<TransferParams>,
) -> ApiResult<CursorPaginatedResponse<TokenTransferResponse>> {
    let hash = hex::decode(type_hash.strip_prefix("0x").unwrap_or(&type_hash))
        .map_err(|_| ApiError::bad_request("Invalid type script hash"))?;

    let token_row: Option<(i64, i64)> =
        sqlx::query_as("SELECT id, transfers_count FROM tokens WHERE type_script_hash = $1")
            .bind(&hash)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let (token_id, transfers_count) = match token_row {
        Some((id, count)) => (id, count),
        None => return Err(ApiError::not_found("Token not found")),
    };

    let limit = params.limit.clamp(1, 100);
    let (cursor_block, cursor_id) = params
        .cursor
        .as_ref()
        .and_then(|c| {
            let parts: Vec<&str> = c.split(':').collect();
            if parts.len() == 2 {
                Some((parts[0].parse::<i64>().ok()?, parts[1].parse::<i64>().ok()?))
            } else {
                None
            }
        })
        .unwrap_or((i64::MAX, i64::MAX));

    type TransferRow = (
        i64,
        Vec<u8>,
        i64,
        Option<Vec<u8>>,
        Vec<u8>,
        String,
        bool,
        bool,
        chrono::DateTime<chrono::Utc>,
    );

    let rows = sqlx::query_as::<_, TransferRow>(
        r#"
        SELECT tt.id, tt.tx_hash, tt.block_number, tt.from_lock_hash, tt.to_lock_hash,
               tt.amount::text, tt.is_mint, tt.is_burn, tt.timestamp
        FROM token_transfers tt
        WHERE tt.token_id = $1 AND (tt.block_number, tt.id) < ($2, $3)
        ORDER BY tt.block_number DESC, tt.id DESC
        LIMIT $4
        "#,
    )
    .bind(token_id)
    .bind(cursor_block)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(id, _, block, _, _, _, _, _, _)| format!("{}:{}", block, id))
    } else {
        None
    };

    let mut lock_hashes: Vec<Vec<u8>> = Vec::new();
    for (_, _, _, from_lock, to_lock, _, _, _, _) in &rows {
        if let Some(from) = from_lock {
            if !lock_hashes.iter().any(|h| h == from) {
                lock_hashes.push(from.clone());
            }
        }
        if !lock_hashes.iter().any(|h| h == to_lock) {
            lock_hashes.push(to_lock.clone());
        }
    }

    let mut address_map: std::collections::HashMap<Vec<u8>, String> =
        std::collections::HashMap::new();

    if !lock_hashes.is_empty() {
        type LockRow = (Vec<u8>, Vec<u8>, i16, Vec<u8>);
        let lock_rows = sqlx::query_as::<_, LockRow>(
            r#"
            SELECT DISTINCT ON (lock_script_hash) 
                   lock_script_hash, lock_code_hash, lock_hash_type, lock_args
            FROM cells
            WHERE lock_script_hash = ANY($1)
            "#,
        )
        .bind(&lock_hashes)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

        let network = &state.ckb_network;
        for (lock_hash, code_hash, hash_type, args) in lock_rows {
            if let Ok(addr) = script_to_address(&code_hash, hash_type, &args, network) {
                address_map.insert(lock_hash, addr);
            }
        }
    }

    let transfers: Vec<TokenTransferResponse> = rows
        .into_iter()
        .map(
            |(
                _,
                tx_hash,
                block_number,
                from_lock_hash,
                to_lock_hash,
                amount,
                is_mint,
                is_burn,
                timestamp,
            )| {
                let from_address = from_lock_hash
                    .as_ref()
                    .and_then(|h| address_map.get(h).cloned());
                let to_address = address_map.get(&to_lock_hash).cloned();

                TokenTransferResponse {
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    block_number,
                    from_lock_hash: from_lock_hash.map(|h| format!("0x{}", hex::encode(&h))),
                    from_address,
                    to_lock_hash: format!("0x{}", hex::encode(&to_lock_hash)),
                    to_address,
                    amount,
                    is_mint,
                    is_burn,
                    timestamp: timestamp.to_rfc3339(),
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::new(
        transfers,
        transfers_count,
        limit,
        next_cursor,
    ))
}

fn hash_type_to_string(hash_type: i16) -> String {
    match hash_type {
        0 => "data".to_string(),
        1 => "type".to_string(),
        2 => "data1".to_string(),
        4 => "data2".to_string(),
        _ => format!("unknown({})", hash_type),
    }
}
