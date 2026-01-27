use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::clickhouse::unhex_hash;
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

#[derive(Debug, Row, Deserialize)]
struct TokenRowClickHouse {
    id: u64,
    type_script_hash: String,
    type_code_hash: String,
    type_hash_type: i16,
    type_args: String,
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

    let search_hash = params
        .search
        .as_ref()
        .map(|s| s.strip_prefix("0x").unwrap_or(s).to_lowercase());
    let search_pattern = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", s.to_lowercase()));

    let query = match (&params.standard, &search_hash, &search_pattern) {
        (Some(standard), Some(hash), _) => {
            format!(
                "SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard, \
                 name, symbol, decimals, description, icon_url, published, famous, tags, udt_type, \
                 manager, email, operator_website, total_supply, holders_count, transfers_count, transfers_24h \
                 FROM tokens \
                 WHERE standard = '{}' AND type_script_hash = unhex('{}') AND (transfers_24h, holders_count, id) < ({}, {}, {}) \
                 ORDER BY transfers_24h DESC, holders_count DESC, id DESC \
                 LIMIT {}",
                standard, hash, cursor_24h, cursor_holders, cursor_id, limit + 1
            )
        }
        (Some(standard), None, Some(pattern)) => {
            format!(
                "SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard, \
                 name, symbol, decimals, description, icon_url, published, famous, tags, udt_type, \
                 manager, email, operator_website, total_supply, holders_count, transfers_count, transfers_24h \
                 FROM tokens \
                 WHERE standard = '{}' AND (LOWER(name) LIKE '{}' OR LOWER(symbol) LIKE '{}') \
                 AND (transfers_24h, holders_count, id) < ({}, {}, {}) \
                 ORDER BY transfers_24h DESC, holders_count DESC, id DESC \
                 LIMIT {}",
                standard, pattern, pattern, cursor_24h, cursor_holders, cursor_id, limit + 1
            )
        }
        (Some(standard), None, None) => {
            format!(
                "SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard, \
                 name, symbol, decimals, description, icon_url, published, famous, tags, udt_type, \
                 manager, email, operator_website, total_supply, holders_count, transfers_count, transfers_24h \
                 FROM tokens \
                 WHERE standard = '{}' AND (transfers_24h, holders_count, id) < ({}, {}, {}) \
                 ORDER BY transfers_24h DESC, holders_count DESC, id DESC \
                 LIMIT {}",
                standard, cursor_24h, cursor_holders, cursor_id, limit + 1
            )
        }
        (None, Some(hash), _) => {
            format!(
                "SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard, \
                 name, symbol, decimals, description, icon_url, published, famous, tags, udt_type, \
                 manager, email, operator_website, total_supply, holders_count, transfers_count, transfers_24h \
                 FROM tokens \
                 WHERE type_script_hash = unhex('{}') AND (transfers_24h, holders_count, id) < ({}, {}, {}) \
                 ORDER BY transfers_24h DESC, holders_count DESC, id DESC \
                 LIMIT {}",
                hash, cursor_24h, cursor_holders, cursor_id, limit + 1
            )
        }
        (None, None, Some(pattern)) => {
            format!(
                "SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard, \
                 name, symbol, decimals, description, icon_url, published, famous, tags, udt_type, \
                 manager, email, operator_website, total_supply, holders_count, transfers_count, transfers_24h \
                 FROM tokens \
                 WHERE (LOWER(name) LIKE '{}' OR LOWER(symbol) LIKE '{}') \
                 AND (transfers_24h, holders_count, id) < ({}, {}, {}) \
                 ORDER BY transfers_24h DESC, holders_count DESC, id DESC \
                 LIMIT {}",
                pattern, pattern, cursor_24h, cursor_holders, cursor_id, limit + 1
            )
        }
        (None, None, None) => {
            format!(
                "SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard, \
                 name, symbol, decimals, description, icon_url, published, famous, tags, udt_type, \
                 manager, email, operator_website, total_supply, holders_count, transfers_count, transfers_24h \
                 FROM tokens \
                 WHERE (transfers_24h, holders_count, id) < ({}, {}, {}) \
                 ORDER BY transfers_24h DESC, holders_count DESC, id DESC \
                 LIMIT {}",
                cursor_24h, cursor_holders, cursor_id, limit + 1
            )
        }
    };

    let rows: Vec<TokenRowClickHouse> = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let total_query = match (&params.standard, &search_hash, &search_pattern) {
        (Some(standard), Some(hash), _) => {
            format!(
                "SELECT COUNT(*) as count FROM tokens WHERE standard = '{}' AND type_script_hash = unhex('{}')",
                standard, hash
            )
        }
        (Some(standard), None, Some(pattern)) => {
            format!(
                "SELECT COUNT(*) as count FROM tokens WHERE standard = '{}' AND (LOWER(name) LIKE '{}' OR LOWER(symbol) LIKE '{}')",
                standard, pattern, pattern
            )
        }
        (Some(standard), None, None) => {
            format!(
                "SELECT COUNT(*) as count FROM tokens WHERE standard = '{}'",
                standard
            )
        }
        (None, Some(hash), _) => {
            format!(
                "SELECT COUNT(*) as count FROM tokens WHERE type_script_hash = unhex('{}')",
                hash
            )
        }
        (None, None, Some(pattern)) => {
            format!(
                "SELECT COUNT(*) as count FROM tokens WHERE (LOWER(name) LIKE '{}' OR LOWER(symbol) LIKE '{}')",
                pattern, pattern
            )
        }
        (None, None, None) => "SELECT COUNT(*) as count FROM tokens".to_string(),
    };

    #[derive(Row, Deserialize)]
    struct CountRow {
        count: u64,
    }

    let count_result: Vec<CountRow> = state
        .clickhouse
        .client()
        .query(&total_query)
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let total = count_result.first().map(|r| r.count as i64).unwrap_or(0);

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
            type_script_hash: format!("0x{}", r.type_script_hash),
            type_code_hash: format!("0x{}", r.type_code_hash),
            type_hash_type: hash_type_to_string(r.type_hash_type),
            type_args: format!("0x{}", r.type_args),
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
    let hash = type_hash
        .strip_prefix("0x")
        .unwrap_or(&type_hash)
        .to_lowercase();
    unhex_hash(&hash)?;

    let query = format!(
        "SELECT id, type_script_hash, type_code_hash, type_hash_type, type_args, standard, \
         name, symbol, decimals, description, icon_url, published, famous, tags, udt_type, \
         manager, email, operator_website, total_supply, holders_count, transfers_count, transfers_24h \
         FROM tokens \
         WHERE type_script_hash = unhex('{}')",
        hash
    );

    let rows: Vec<TokenRowClickHouse> = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match rows.first() {
        Some(r) => ok(TokenResponse {
            type_script_hash: format!("0x{}", r.type_script_hash),
            type_code_hash: format!("0x{}", r.type_code_hash),
            type_hash_type: hash_type_to_string(r.type_hash_type),
            type_args: format!("0x{}", r.type_args),
            standard: r.standard.clone(),
            name: r.name.clone(),
            symbol: r.symbol.clone(),
            decimals: r.decimals,
            description: r.description.clone(),
            icon_url: r.icon_url.clone(),
            published: r.published,
            famous: r.famous,
            tags: r.tags.clone(),
            udt_type: r.udt_type.clone(),
            manager: r.manager.clone(),
            email: r.email.clone(),
            operator_website: r.operator_website.clone(),
            total_supply: r.total_supply.clone(),
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
    let hash = type_hash
        .strip_prefix("0x")
        .unwrap_or(&type_hash)
        .to_lowercase();
    unhex_hash(&hash)?;

    #[derive(Row, Deserialize)]
    struct TokenRow {
        id: u64,
        holders_count: i32,
    }

    let token_query = format!(
        "SELECT id, holders_count FROM tokens WHERE type_script_hash = unhex('{}')",
        hash
    );

    let token_rows: Vec<TokenRow> = state
        .clickhouse
        .client()
        .query(&token_query)
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (token_id, holders_count) = match token_rows.first() {
        Some(r) => (r.id, r.holders_count as i64),
        None => return Err(ApiError::not_found("Token not found")),
    };

    let limit = params.limit.clamp(1, 100);
    let (cursor_balance, cursor_lock) = params
        .cursor
        .as_ref()
        .and_then(|c| {
            let parts: Vec<&str> = c.split(':').collect();
            if parts.len() == 2 {
                let lock_hex = parts[1];
                Some((parts[0].to_string(), lock_hex.to_string()))
            } else {
                None
            }
        })
        .unwrap_or_else(|| (String::new(), String::new()));

    #[derive(Row, Deserialize)]
    struct HolderRow {
        lock_script_hash: String,
        balance: String,
    }

    let holders_query = if cursor_balance.is_empty() {
        format!(
            "SELECT lock_script_hash, balance FROM token_balances \
             WHERE token_id = {} AND balance > 0 \
             ORDER BY balance DESC, lock_script_hash DESC \
             LIMIT {}",
            token_id,
            limit + 1
        )
    } else {
        format!(
            "SELECT lock_script_hash, balance FROM token_balances \
             WHERE token_id = {} AND balance > 0 \
             AND (balance, lock_script_hash) < ('{}', '{}') \
             ORDER BY balance DESC, lock_script_hash DESC \
             LIMIT {}",
            token_id,
            cursor_balance,
            cursor_lock,
            limit + 1
        )
    };

    let rows: Vec<HolderRow> = state
        .clickhouse
        .client()
        .query(&holders_query)
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|r| format!("{}:{}", r.balance, r.lock_script_hash))
    } else {
        None
    };

    let lock_hashes: Vec<String> = rows.iter().map(|r| r.lock_script_hash.clone()).collect();

    let mut address_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    if !lock_hashes.is_empty() {
        #[derive(Row, Deserialize)]
        struct LockRow {
            lock_script_hash: String,
            lock_code_hash: String,
            lock_hash_type: i16,
            lock_args: String,
        }

        let lock_hashes_str = lock_hashes
            .iter()
            .map(|h| format!("'{}'", h))
            .collect::<Vec<_>>()
            .join(",");

        let lock_query = format!(
            "SELECT DISTINCT lock_script_hash, lock_code_hash, lock_hash_type, lock_args \
             FROM cells \
             WHERE lock_script_hash IN ({})",
            lock_hashes_str
        );

        let lock_rows: Vec<LockRow> = state
            .clickhouse
            .client()
            .query(&lock_query)
            .fetch_all()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let network = &state.ckb_network;
        for lock_row in lock_rows {
            let code_hash_bytes = unhex_hash(&lock_row.lock_code_hash).unwrap_or_default();
            let args_bytes = unhex_hash(&lock_row.lock_args).unwrap_or_default();
            if let Ok(addr) = script_to_address(
                &code_hash_bytes,
                lock_row.lock_hash_type,
                &args_bytes,
                network,
            ) {
                address_map.insert(lock_row.lock_script_hash, addr);
            }
        }
    }

    let holders: Vec<TokenHolderResponse> = rows
        .into_iter()
        .map(|r| {
            let address = address_map.get(&r.lock_script_hash).cloned();
            TokenHolderResponse {
                lock_script_hash: format!("0x{}", r.lock_script_hash),
                address,
                balance: r.balance,
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
    let hash = type_hash
        .strip_prefix("0x")
        .unwrap_or(&type_hash)
        .to_lowercase();
    unhex_hash(&hash)?;

    #[derive(Row, Deserialize)]
    struct TokenRow {
        id: u64,
        transfers_count: i64,
    }

    let token_query = format!(
        "SELECT id, transfers_count FROM tokens WHERE type_script_hash = unhex('{}')",
        hash
    );

    let token_rows: Vec<TokenRow> = state
        .clickhouse
        .client()
        .query(&token_query)
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (token_id, transfers_count) = match token_rows.first() {
        Some(r) => (r.id, r.transfers_count),
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

    #[derive(Row, Deserialize)]
    struct TransferRow {
        id: u64,
        tx_hash: String,
        block_number: i64,
        from_lock_hash: Option<String>,
        to_lock_hash: String,
        amount: String,
        is_mint: bool,
        is_burn: bool,
        timestamp: String,
    }

    let transfers_query = format!(
        "SELECT id, tx_hash, block_number, from_lock_hash, to_lock_hash, amount, is_mint, is_burn, timestamp \
         FROM token_transfers \
         WHERE token_id = {} AND (block_number, id) < ({}, {}) \
         ORDER BY block_number DESC, id DESC \
         LIMIT {}",
        token_id, cursor_block, cursor_id, limit + 1
    );

    let rows: Vec<TransferRow> = state
        .clickhouse
        .client()
        .query(&transfers_query)
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|r| format!("{}:{}", r.block_number, r.id))
    } else {
        None
    };

    let mut lock_hashes: Vec<String> = Vec::new();
    for row in &rows {
        if let Some(from) = &row.from_lock_hash {
            if !lock_hashes.iter().any(|h| h == from) {
                lock_hashes.push(from.clone());
            }
        }
        if !lock_hashes.iter().any(|h| h == &row.to_lock_hash) {
            lock_hashes.push(row.to_lock_hash.clone());
        }
    }

    let mut address_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    if !lock_hashes.is_empty() {
        #[derive(Row, Deserialize)]
        struct LockRow {
            lock_script_hash: String,
            lock_code_hash: String,
            lock_hash_type: i16,
            lock_args: String,
        }

        let lock_hashes_str = lock_hashes
            .iter()
            .map(|h| format!("'{}'", h))
            .collect::<Vec<_>>()
            .join(",");

        let lock_query = format!(
            "SELECT DISTINCT lock_script_hash, lock_code_hash, lock_hash_type, lock_args \
             FROM cells \
             WHERE lock_script_hash IN ({})",
            lock_hashes_str
        );

        let lock_rows: Vec<LockRow> = state
            .clickhouse
            .client()
            .query(&lock_query)
            .fetch_all()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let network = &state.ckb_network;
        for lock_row in lock_rows {
            let code_hash_bytes = unhex_hash(&lock_row.lock_code_hash).unwrap_or_default();
            let args_bytes = unhex_hash(&lock_row.lock_args).unwrap_or_default();
            if let Ok(addr) = script_to_address(
                &code_hash_bytes,
                lock_row.lock_hash_type,
                &args_bytes,
                network,
            ) {
                address_map.insert(lock_row.lock_script_hash, addr);
            }
        }
    }

    let transfers: Vec<TokenTransferResponse> = rows
        .into_iter()
        .map(|r| {
            let from_address = r
                .from_lock_hash
                .as_ref()
                .and_then(|h| address_map.get(h).cloned());
            let to_address = address_map.get(&r.to_lock_hash).cloned();

            TokenTransferResponse {
                tx_hash: format!("0x{}", r.tx_hash),
                block_number: r.block_number,
                from_lock_hash: r.from_lock_hash.map(|h| format!("0x{}", h)),
                from_address,
                to_lock_hash: format!("0x{}", r.to_lock_hash),
                to_address,
                amount: r.amount,
                is_mint: r.is_mint,
                is_burn: r.is_burn,
                timestamp: r.timestamp,
            }
        })
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
