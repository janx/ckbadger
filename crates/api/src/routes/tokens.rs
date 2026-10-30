use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use ckbadger_common::TokenBalance;
use ckbadger_store::CkbadgerStore;

use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::response::{
    default_limit, hash_type_to_str, ok, ApiError, ApiResult, ApiRouteError,
    CursorPaginatedResponse,
};
use crate::utils::{
    accumulate_owned_capacity, apply_owned_capacity_delta, date_keys_inclusive,
    parse_chart_date_range, parse_hash32, parse_optional_block_tx_cursor, script_to_address,
};
use crate::warmup::CachedAssetEntry;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tokens", get(list_tokens))
        .route("/tokens/{type_hash}", get(get_token))
        .route("/tokens/{type_hash}/holders", get(get_token_holders))
        .route("/tokens/{type_hash}/transfers", get(get_token_transfers))
        .route("/tokens/{type_hash}/activities", get(get_token_activities))
        .route(
            "/tokens/{type_hash}/activity-feed",
            get(super::token_history::activity_feed),
        )
        .route(
            "/tokens/{type_hash}/transactions",
            get(super::token_history::transactions),
        )
        .route(
            "/tokens/{type_hash}/history",
            get(super::token_history::history),
        )
        .route(
            "/tokens/{type_hash}/supply-events",
            get(super::token_history::supply_events),
        )
        .route(
            "/tokens/{type_hash}/distribution",
            get(super::token_history::distribution),
        )
        .route(
            "/tokens/{type_hash}/charts/capacity-history",
            get(get_token_capacity_chart),
        )
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    standard: Option<String>,
    cursor: Option<String>,
    search: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct ChartRangeParams {
    from: Option<String>,
    to: Option<String>,
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
    /// None when unknown (no TOML label and no on-chain info cell) — never a
    /// fabricated 0, which would be indistinguishable from true 0 decimals.
    pub decimals: Option<i16>,
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
    pub maximum_supply: Option<String>,
    pub maximum_supply_status: String,
    pub holders_count: i64,
    pub transfers_count: i64,
    pub transfers_24h: i64,
    pub cells_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_symbol_tokens: Option<Vec<SameSymbolToken>>,
    /// Token detail only (absent on list rows): where the stored name /
    /// symbol / decimals came from and what the token's own Unique Cell says
    /// (IV-26). `null` when neither a label nor a Unique Cell named the token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Option<TokenMetadataResponse>>,
    pub issuer: Option<TokenIssuer>,
    pub total_capacity: Option<String>,
    #[serde(rename = "totalCommonKnowledgeSize")]
    pub total_used_capacity: Option<String>,
}

/// A token's metadata provenance (IV-26), from the one store rule
/// (`TokenInfo::checked_metadata_source` / `chain_metadata_conflict`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenMetadataResponse {
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<i16>,
    /// `"label"` when a curated label shipped with CKBadger wrote at least one
    /// of the fields above, `"chain"` when all came from the Unique Cell.
    pub source: &'static str,
    /// The token's bound Unique Cell values; `null` when none is bound.
    pub chain: Option<ChainTokenMetadataResponse>,
    /// The stored fields disagree with `chain`.
    pub conflict: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainTokenMetadataResponse {
    /// `null` when the Unique Cell's name is empty.
    pub name: Option<String>,
    /// `null` when the Unique Cell's symbol is empty.
    pub symbol: Option<String>,
    pub decimals: u8,
}

/// The `metadata` of a token detail; `None` when nothing named the token. A
/// row whose fields and source disagree is an error naming the token.
fn token_metadata_response(
    type_hash: &[u8],
    info: &ckbadger_store::TokenInfo,
) -> Result<Option<TokenMetadataResponse>, ApiRouteError> {
    let source = info.checked_metadata_source().map_err(|error| {
        ApiError::internal(format!("token 0x{}: {error}", hex::encode(type_hash)))
    })?;
    Ok(source.map(|source| TokenMetadataResponse {
        name: info.name.clone(),
        symbol: info.symbol.clone(),
        decimals: info.decimals.map(|d| d as i16),
        source: source.as_str(),
        chain: info
            .chain_metadata
            .as_ref()
            .map(|chain| ChainTokenMetadataResponse {
                name: chain.name.clone(),
                symbol: chain.symbol.clone(),
                decimals: chain.decimals,
            }),
        conflict: info.chain_metadata_conflict(),
    }))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SameSymbolToken {
    pub type_script_hash: String,
    pub holders_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenIssuer {
    pub lock_hash: String,
    pub address: Option<String>,
    pub lock_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHolderResponse {
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub balance: String,
    pub lock_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHoldersResponse {
    #[serde(flatten)]
    pub page: CursorPaginatedResponse<TokenHolderResponse>,
    pub total_supply: String,
    pub anchor: Option<TokenAnchor>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenAnchor {
    pub block_number: i64,
    pub block_hash: String,
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

struct TokenDerivedStats {
    total_supply: TokenBalance,
    holders_count: i64,
    transfers_count: i64,
    transfers_24h: i64,
    owned_capacity: Option<i128>,
    owned_knowledge: Option<i128>,
}

/// Convert a store TokenInfo + key into an API TokenResponse.
fn token_info_to_response(
    type_hash: &[u8],
    info: &ckbadger_store::TokenInfo,
    derived: TokenDerivedStats,
) -> Result<TokenResponse, ApiRouteError> {
    let type_hash_type = hash_type_to_str(info.hash_type as i16)
        .ok_or_else(|| {
            ApiError::internal(format!(
                "unknown hash_type {} for token type_hash=0x{}",
                info.hash_type,
                hex::encode(type_hash)
            ))
        })?
        .to_string();
    let maximum_supply = info.max_supply.map(|s| s.to_string());
    Ok(TokenResponse {
        type_script_hash: format!("0x{}", hex::encode(type_hash)),
        type_code_hash: format!("0x{}", hex::encode(&info.type_code_hash)),
        type_hash_type,
        type_args: format!("0x{}", hex::encode(&info.type_args)),
        standard: info.standard.clone(),
        name: info.name.clone(),
        symbol: info.symbol.clone(),
        decimals: info.decimals.map(|d| d as i16),
        description: info.description.clone(),
        icon_url: info.icon_url.clone(),
        published: false,
        famous: false,
        tags: None,
        udt_type: None,
        manager: None,
        email: None,
        operator_website: None,
        total_supply: derived.total_supply.to_string(),
        maximum_supply_status: max_supply_status(
            &info.standard,
            maximum_supply.as_deref(),
            Some(&info.type_args),
        )
        .to_string(),
        maximum_supply,
        holders_count: derived.holders_count,
        transfers_count: derived.transfers_count,
        transfers_24h: derived.transfers_24h,
        cells_count: None,
        same_symbol_tokens: None,
        metadata: None,
        issuer: None,
        total_capacity: derived.owned_capacity.map(|c| c.to_string()),
        total_used_capacity: derived.owned_knowledge.map(|c| c.to_string()),
    })
}

fn is_xudt_standard(standard: &str) -> bool {
    standard.eq_ignore_ascii_case("xudt")
}

fn is_plain_xudt_without_extension(type_args: Option<&[u8]>) -> bool {
    let Some(type_args) = type_args else {
        return false;
    };
    match type_args.len() {
        // Compatibility mode: owner lock hash only (implicit zero flags).
        32 => true,
        // Explicit flags in args: <owner lock script hash(32)> <flags(4)>
        36 => {
            let flags = u32::from_le_bytes(
                type_args[32..36]
                    .try_into()
                    .expect("validated 36-byte xUDT args"),
            );
            // RFC 0052: high bits can replace/disable owner-lock mode too.
            flags == 0
        }
        _ => false,
    }
}

fn max_supply_status(
    standard: &str,
    maximum_supply: Option<&str>,
    type_args: Option<&[u8]>,
) -> &'static str {
    if maximum_supply.is_some() {
        "limited"
    } else if standard.eq_ignore_ascii_case("sudt")
        || (is_xudt_standard(standard) && is_plain_xudt_without_extension(type_args))
    {
        "unlimited"
    } else {
        "unknown"
    }
}

fn parse_token_list_cursor(cursor: &str) -> Result<(i64, &str), ApiRouteError> {
    let (holders_count, token_id) = cursor
        .split_once(':')
        .ok_or_else(|| ApiError::bad_request("Invalid token cursor"))?;
    if token_id.is_empty() {
        return Err(ApiError::bad_request("Invalid token cursor"));
    }
    let holders_count = holders_count
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("Invalid token cursor"))?;
    Ok((holders_count, token_id))
}

fn parse_token_holder_cursor(cursor: &str) -> Result<(TokenBalance, Vec<u8>), ApiRouteError> {
    let (balance, lock_hash_hex) = cursor
        .split_once(':')
        .ok_or_else(|| ApiError::bad_request("Invalid token holders cursor"))?;
    let balance = balance
        .parse::<TokenBalance>()
        .map_err(|_| ApiError::bad_request("Invalid token holders cursor"))?;
    let lock_hash = hex::decode(lock_hash_hex.strip_prefix("0x").unwrap_or(lock_hash_hex))
        .map_err(|_| ApiError::bad_request("Invalid token holders cursor"))?;
    if lock_hash.len() != 32 {
        return Err(ApiError::bad_request("Invalid token holders cursor"));
    }
    Ok((balance, lock_hash))
}

async fn list_tokens(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<TokenResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    if let Some(cached_tokens) = state.load_token_cache() {
        let mut response = serve_tokens_from_cache(cached_tokens, &params, limit)?;
        for row in &mut response.0.data {
            let hash = parse_hash32(&row.type_script_hash, "type_hash")?;
            row.cells_count = Some(
                super::token_history::live_cells_count(&state, &hash)
                    .map_err(|error| ApiError::internal(error.to_string()))?,
            );
        }
        return Ok(response);
    }

    Err(state.asset_cache_unavailable("token cache unavailable; warmup in progress"))
}

/// Serve token list from pre-computed cache.
fn serve_tokens_from_cache(
    cached: Vec<CachedAssetEntry>,
    params: &ListParams,
    limit: usize,
) -> ApiResult<CursorPaginatedResponse<TokenResponse>> {
    let search_lower = params.search.as_ref().map(|s| s.to_lowercase());

    let mut filtered: Vec<_> = cached
        .into_iter()
        .filter(|entry| {
            if let Some(ref standard) = params.standard {
                if &entry.standard != standard {
                    return false;
                }
            }
            if let Some(ref pattern) = search_lower {
                let name_match = entry
                    .name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(pattern))
                    .unwrap_or(false);
                let symbol_match = entry
                    .symbol
                    .as_ref()
                    .map(|s| s.to_lowercase().contains(pattern))
                    .unwrap_or(false);
                if !name_match && !symbol_match {
                    return false;
                }
            }
            true
        })
        .collect();

    // Already sorted by transfers_24h DESC, holders_count DESC from cache
    // But we need to sort by holders_count for the tokens endpoint
    filtered.sort_by(|a, b| {
        b.holders_count
            .cmp(&a.holders_count)
            .then_with(|| a.id.cmp(&b.id))
    });

    let start_idx = if let Some(cursor) = params.cursor.as_deref() {
        let (cursor_holders_count, cursor_token_id) = parse_token_list_cursor(cursor)?;
        filtered
            .iter()
            .position(|e| e.holders_count == cursor_holders_count && e.id == cursor_token_id)
            .map(|idx| idx + 1)
            .ok_or_else(|| ApiError::bad_request("Invalid token cursor"))?
    } else {
        0
    };

    let page: Vec<_> = filtered.iter().skip(start_idx).take(limit + 1).collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last().map(|e| format!("{}:{}", e.holders_count, e.id))
    } else {
        None
    };

    // Build TokenResponse directly from cache — zero DB reads
    let tokens: Vec<TokenResponse> = page
        .into_iter()
        .map(|entry| {
            let required = |value: &Option<String>, field: &str| {
                value.clone().ok_or_else(|| {
                    ApiError::internal(format!("token cache {}: missing {field}", entry.id))
                })
            };
            let type_code_hash = required(&entry.type_code_hash, "type_code_hash")?;
            let type_hash_type = required(&entry.type_hash_type, "type_hash_type")?;
            let type_args = required(&entry.type_args, "type_args")?;
            let total_supply = required(&entry.total_supply, "total_supply")?;
            let decoded_type_args = hex::decode(type_args.strip_prefix("0x").unwrap_or(&type_args))
                .map_err(|error| {
                    ApiError::internal(format!(
                        "token cache {}: invalid type_args: {error}",
                        entry.id
                    ))
                })?;
            Ok(TokenResponse {
                type_script_hash: entry.id.clone(),
                type_code_hash,
                type_hash_type,
                type_args,
                standard: entry.standard.clone(),
                name: entry.name.clone(),
                symbol: entry.symbol.clone(),
                decimals: entry.decimals,
                description: entry.description.clone(),
                icon_url: entry.icon_url.clone(),
                published: false,
                famous: false,
                tags: None,
                udt_type: None,
                manager: None,
                email: None,
                operator_website: None,
                total_supply,
                maximum_supply: entry.maximum_supply.clone(),
                maximum_supply_status: max_supply_status(
                    &entry.standard,
                    entry.maximum_supply.as_deref(),
                    Some(&decoded_type_args),
                )
                .to_string(),
                holders_count: entry.holders_count,
                transfers_count: entry.transfers_count,
                transfers_24h: entry.transfers_24h,
                cells_count: None,
                same_symbol_tokens: None,
                metadata: None,
                issuer: None,
                total_capacity: None,
                total_used_capacity: None,
            })
        })
        .collect::<Result<_, _>>()?;

    ok(CursorPaginatedResponse::without_total(
        tokens,
        limit as i64,
        next_cursor,
    ))
}

async fn get_token(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
) -> ApiResult<TokenResponse> {
    let hash = parse_hash32(&type_hash, "type_hash")?;

    let store = state.store.clone();
    let hash_c = hash.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let info = store.get_token(&hash_c)?;
        match info {
            Some(info) => {
                let (holders_count, total_supply) = store.aggregate_token_holder_stats(&hash_c)?;
                let transfers_count = store.get_token_transfers_count(&hash_c)?;
                let transfers_24h = store
                    .get_token_24h_transfers(&hash_c, chrono::Utc::now().timestamp_millis())?;
                let deltas = store.list_token_daily_deltas(&hash_c)?;
                Ok(Some((
                    info,
                    total_supply,
                    holders_count,
                    transfers_count,
                    transfers_24h,
                    deltas,
                )))
            }
            None => Ok(None),
        }
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    match result {
        Some((info, total_supply, holders_count, transfers_count, transfers_24h, deltas)) => {
            let (owned_capacity, owned_knowledge) = if deltas.is_empty() {
                (None, None)
            } else {
                let (lc, luc) =
                    accumulate_owned_capacity(deltas.into_iter().map(|(_, delta)| {
                        (delta.owned_capacity_delta, delta.owned_knowledge_delta)
                    }))
                    .map_err(|e| {
                        ApiError::internal(format!(
                            "invalid token daily deltas for type_hash=0x{}: {}",
                            hex::encode(&hash),
                            e
                        ))
                    })?;
                (Some(lc), Some(luc))
            };

            let mut response = token_info_to_response(
                &hash,
                &info,
                TokenDerivedStats {
                    total_supply,
                    holders_count,
                    transfers_count,
                    transfers_24h,
                    owned_capacity,
                    owned_knowledge,
                },
            )?;
            response.cells_count = Some(
                super::token_history::live_cells_count(&state, &hash)
                    .map_err(|error| ApiError::internal(error.to_string()))?,
            );
            response.metadata = Some(token_metadata_response(&hash, &info)?);
            // Same-symbol warning: any symbol either token answers to — its
            // stored one or its Unique Cell's (IV-26: a label that renamed a
            // token still collides with the symbol its own cell claims).
            let mut others = Vec::new();
            let symbols = info.claimed_symbols();
            if !symbols.is_empty() {
                for (other_hash, other) in state
                    .store
                    .list_tokens()
                    .map_err(|error| ApiError::internal(error.to_string()))?
                {
                    if other_hash != hash
                        && other
                            .claimed_symbols()
                            .iter()
                            .any(|symbol| symbols.contains(symbol))
                    {
                        let (holders_count, _) = state
                            .store
                            .aggregate_token_holder_stats(&other_hash)
                            .map_err(|error| ApiError::internal(error.to_string()))?;
                        others.push(SameSymbolToken {
                            type_script_hash: format!("0x{}", hex::encode(other_hash)),
                            holders_count,
                        });
                    }
                }
            }
            response.same_symbol_tokens = Some(others);
            // These exact standard layouts name the owner authorized to mint.
            // Compatible/custom scripts and extended xUDT rules prove no such
            // authority merely by having 32 bytes at the front of their args.
            if (info.standard.eq_ignore_ascii_case("sudt") && info.type_args.len() >= 32)
                || (info.standard.eq_ignore_ascii_case("xudt")
                    && is_plain_xudt_without_extension(Some(&info.type_args)))
            {
                let lock: [u8; 32] = info.type_args[..32]
                    .try_into()
                    .expect("validated owner hash width");
                response.issuer = Some(TokenIssuer {
                    lock_hash: format!("0x{}", hex::encode(lock)),
                    address: crate::utils::address::resolve_lock_address(
                        &state.store,
                        &lock,
                        &state.ckb_network,
                    )
                    .map_err(|error| ApiError::internal(error.to_string()))?
                    .map(|resolved| resolved.address),
                    lock_kind: super::activities::participant_lock_kind(&state.store, &lock, None)
                        .map_err(|error| ApiError::internal(error.to_string()))?,
                });
            }
            ok(response)
        }
        None => Err(ApiError::not_found("Token not found")),
    }
}

async fn get_token_holders(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
    Query(params): Query<HolderParams>,
) -> ApiResult<TokenHoldersResponse> {
    let hash = parse_hash32(&type_hash, "type_hash")?;

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor = params
        .cursor
        .as_deref()
        .map(parse_token_holder_cursor)
        .transpose()?;
    let has_cursor = params.cursor.is_some();

    let store = state.store.clone();
    let hash_c = hash.clone();
    let network = state.ckb_network.clone();
    let (_token, holders_count, total_supply, mut page, addresses) =
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let token = store
                .get_token(&hash_c)?
                .ok_or_else(|| anyhow::anyhow!("not_found"))?;
            let (holders_count, total_supply) = store.aggregate_token_holder_stats(&hash_c)?;
            let page = store.list_token_holders_by_balance(&hash_c, limit + 1, cursor)?;
            // Resolve only the rendered rows: the `limit + 1`-th row exists solely to
            // detect `has_more` and is truncated away below.
            let addresses = resolve_lock_addresses(
                &store,
                &network,
                page.iter().take(limit).map(|(lock, _)| lock.as_slice()),
            )?;
            Ok((token, holders_count, total_supply, page, addresses))
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e: anyhow::Error| {
            if e.to_string() == "not_found" {
                ApiError::not_found("Token not found")
            } else {
                ApiError::internal(e.to_string())
            }
        })?;
    if holders_count < 0 {
        return Err(ApiError::internal(format!(
            "invalid token holders_count: type_hash=0x{}, holders_count={}",
            hex::encode(&hash),
            holders_count
        )));
    }
    if !has_cursor && holders_count > 0 && page.is_empty() {
        return Err(ApiError::internal(format!(
            "missing ranked token holder index entries: type_hash=0x{}, holders_count={}",
            hex::encode(&hash),
            holders_count
        )));
    }
    let has_more = page.len() > limit;
    if has_more {
        page.truncate(limit);
    }

    let next_cursor = if has_more {
        page.last()
            .map(|(lock, balance)| format!("{}:{}", balance, hex::encode(lock)))
    } else {
        None
    };

    let holders: Vec<TokenHolderResponse> = page
        .into_iter()
        .map(|(lock_script_hash, balance)| {
            let lock: [u8; 32] = lock_script_hash
                .as_slice()
                .try_into()
                .map_err(|_| ApiError::internal("invalid token holder lock length"))?;
            Ok(TokenHolderResponse {
                lock_kind: super::activities::participant_lock_kind(&state.store, &lock, None)
                    .map_err(|e| ApiError::internal(e.to_string()))?,
                address: addresses.get(&lock_script_hash).cloned(),
                lock_script_hash: format!("0x{}", hex::encode(lock_script_hash)),
                balance: balance.to_string(),
            })
        })
        .collect::<Result<_, crate::response::ApiRouteError>>()?;

    let anchor = state
        .store
        .get_sync_tip_block()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map(|(block_number, header)| TokenAnchor {
            block_number,
            block_hash: format!("0x{}", hex::encode(header.hash)),
        });
    ok(TokenHoldersResponse {
        total_supply: total_supply.to_string(),
        anchor,
        page: CursorPaginatedResponse::new(holders, holders_count, limit as i64, next_cursor),
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenActivityResponse {
    /// Per-token, per-block sequence used by the transfer cursor.
    pub transfer_sequence: i32,
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub actions: Vec<String>,
    pub transfers: Vec<TokenTransferDetail>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTransferDetail {
    pub from_lock_hash: Option<String>,
    pub from_address: Option<String>,
    pub to_lock_hash: String,
    pub to_address: Option<String>,
    pub amount: String,
    pub is_mint: bool,
    pub is_burn: bool,
}

#[derive(Debug, Deserialize)]
pub struct ActivityParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

/// Resolve lock hashes to CKB addresses through the single existing path:
/// `CF_LOCK_SCRIPTS` (`get_lock_script`) + the shared `script_to_address`
/// encoder. Lock hashes with no stored script (or an unencodable script) are
/// simply absent from the map and render as null — never guessed.
fn resolve_lock_addresses<'a>(
    store: &CkbadgerStore,
    network: &str,
    lock_hashes: impl Iterator<Item = &'a [u8]>,
) -> anyhow::Result<HashMap<Vec<u8>, String>> {
    let mut addresses: HashMap<Vec<u8>, String> = HashMap::new();
    for lock_hash in lock_hashes {
        if lock_hash.is_empty() || addresses.contains_key(lock_hash) {
            continue;
        }
        let Some(entry) = store.get_lock_script(lock_hash)? else {
            continue;
        };
        let Ok(address) =
            script_to_address(&entry.code_hash, entry.hash_type, &entry.args, network)
        else {
            continue;
        };
        addresses.insert(lock_hash.to_vec(), address);
    }
    Ok(addresses)
}

async fn get_token_activities(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
    Query(params): Query<ActivityParams>,
) -> ApiResult<CursorPaginatedResponse<TokenActivityResponse>> {
    let hash = parse_hash32(&type_hash, "type_hash")?;

    let limit = params.limit.clamp(1, 100) as usize;

    let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "token activity cursor")?;

    let store = state.store.clone();
    let hash_c = hash.clone();
    let network = state.ckb_network.clone();
    let (results, addresses) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let token_exists = store.get_token(&hash_c)?.is_some();
        if !token_exists {
            return Err(anyhow::anyhow!("not_found"));
        }
        let results = store.list_token_activities(&hash_c, limit + 1, cursor)?;
        let addresses = resolve_lock_addresses(
            &store,
            &network,
            results.iter().flat_map(|(_, _, entry)| {
                entry.transfers.iter().flat_map(|transfer| {
                    transfer
                        .from_lock_hash
                        .as_deref()
                        .into_iter()
                        .chain(std::iter::once(transfer.to_lock_hash.as_slice()))
                })
            }),
        )?;
        Ok((results, addresses))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e: anyhow::Error| {
        if e.to_string() == "not_found" {
            ApiError::not_found("Token not found")
        } else {
            ApiError::internal(e.to_string())
        }
    })?;

    let has_more = results.len() > limit;
    let page: Vec<_> = results.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(block_num, tx_idx, _)| format!("{}:{}", block_num, tx_idx))
    } else {
        None
    };

    let activities: Vec<TokenActivityResponse> = page
        .into_iter()
        .map(|(block_number, transfer_sequence, entry)| -> Result<_, crate::response::ApiRouteError> {
            let (tx_block, tx_index) = state.store.get_tx_location(&entry.tx_hash)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .ok_or_else(|| ApiError::internal(format!("token transfer references missing transaction: token={} block={} sequence={} tx=0x{}", type_hash, block_number, transfer_sequence, hex::encode(&entry.tx_hash))))?;
            if tx_block != block_number {
                return Err(ApiError::internal(format!("token transfer transaction block mismatch: token={} transfer_block={} tx_block={} tx=0x{}", type_hash, block_number, tx_block, hex::encode(&entry.tx_hash))));
            }
            let actions: Vec<String> = entry
                .actions
                .iter()
                .map(|a| match a {
                    ckbadger_store::AssetAction::Mint => "mint".to_string(),
                    ckbadger_store::AssetAction::Transfer => "transfer".to_string(),
                    ckbadger_store::AssetAction::Burn => "burn".to_string(),
                    ckbadger_store::AssetAction::Recycle => "recycle".to_string(),
                    ckbadger_store::AssetAction::Renew => "renew".to_string(),
                    ckbadger_store::AssetAction::Update => "update".to_string(),
                })
                .collect();

            let transfers: Vec<TokenTransferDetail> = entry
                .transfers
                .into_iter()
                .map(|t| TokenTransferDetail {
                    from_lock_hash: t
                        .from_lock_hash
                        .as_ref()
                        .map(|h| format!("0x{}", hex::encode(h))),
                    from_address: t
                        .from_lock_hash
                        .as_ref()
                        .and_then(|h| addresses.get(h.as_slice()).cloned()),
                    to_lock_hash: format!("0x{}", hex::encode(&t.to_lock_hash)),
                    to_address: addresses.get(t.to_lock_hash.as_slice()).cloned(),
                    amount: t.amount.to_string(),
                    is_mint: t.is_mint,
                    is_burn: t.is_burn,
                })
                .collect();

            Ok(TokenActivityResponse {
                transfer_sequence,
                tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
                block_number: entry.block_number,
                tx_index,
                timestamp: entry.timestamp_ms.to_string(),
                actions,
                transfers,
            })
        })
        .collect::<Result<_, _>>()?;

    ok(CursorPaginatedResponse::without_total(
        activities,
        limit as i64,
        next_cursor,
    ))
}

async fn get_token_transfers(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
    Query(params): Query<TransferParams>,
) -> ApiResult<CursorPaginatedResponse<TokenTransferResponse>> {
    let hash = parse_hash32(&type_hash, "type_hash")?;

    let limit = params.limit.clamp(1, 100) as usize;

    let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "token transfer cursor")?;

    let store = state.store.clone();
    let hash_c = hash.clone();
    let network = state.ckb_network.clone();
    let (info, results, addresses) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let info = store
            .get_token(&hash_c)?
            .ok_or_else(|| anyhow::anyhow!("not_found"))?;
        let results = store.list_token_transfers(&hash_c, limit + 1, cursor)?;
        let addresses = resolve_lock_addresses(
            &store,
            &network,
            results.iter().flat_map(|(_, _, record)| {
                record
                    .from_lock_hash
                    .as_deref()
                    .into_iter()
                    .chain(std::iter::once(record.to_lock_hash.as_slice()))
            }),
        )?;
        Ok((info, results, addresses))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e: anyhow::Error| {
        if e.to_string() == "not_found" {
            ApiError::not_found("Token not found")
        } else {
            ApiError::internal(e.to_string())
        }
    })?;

    let has_more = results.len() > limit;
    let page: Vec<_> = results.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(block_num, tx_idx, _)| format!("{}:{}", block_num, tx_idx))
    } else {
        None
    };

    let transfers: Vec<TokenTransferResponse> = page
        .into_iter()
        .map(|(_, _, record)| TokenTransferResponse {
            tx_hash: format!("0x{}", hex::encode(&record.tx_hash)),
            block_number: record.block_number,
            from_lock_hash: record
                .from_lock_hash
                .as_ref()
                .map(|h| format!("0x{}", hex::encode(h))),
            from_address: record
                .from_lock_hash
                .as_ref()
                .and_then(|h| addresses.get(h.as_slice()).cloned()),
            to_lock_hash: format!("0x{}", hex::encode(&record.to_lock_hash)),
            to_address: addresses.get(record.to_lock_hash.as_slice()).cloned(),
            amount: record.amount.to_string(),
            is_mint: record.is_mint,
            is_burn: record.is_burn,
            timestamp: record.timestamp.to_string(),
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        transfers,
        info.transfers_count,
        limit as i64,
        next_cursor,
    ))
}

fn format_yyyymmdd_for_chart(date_yyyymmdd: u32) -> String {
    let date = format!("{date_yyyymmdd:08}");
    format!("{}-{}-{}", &date[0..4], &date[4..6], &date[6..8])
}

async fn get_token_capacity_chart(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
    Query(params): Query<ChartRangeParams>,
) -> ApiResult<StackedAreaChartResponse> {
    let hash = parse_hash32(&type_hash, "type_hash")?;

    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let store = state.store.clone();
    let hash_c = hash.clone();
    let (token, baseline_deltas, deltas) =
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let token = store
                .get_token(&hash_c)?
                .ok_or_else(|| anyhow::anyhow!("not_found"))?;
            let baseline_deltas = if let Some(from) = from_date {
                store.list_token_daily_deltas_in_range(
                    &hash_c,
                    None,
                    Some(from.saturating_sub(1)),
                )?
            } else {
                Vec::new()
            };
            let deltas = store.list_token_daily_deltas_in_range(&hash_c, from_date, to_date)?;
            Ok((token, baseline_deltas, deltas))
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e: anyhow::Error| {
            if e.to_string() == "not_found" {
                ApiError::not_found("Token not found")
            } else {
                ApiError::internal(e.to_string())
            }
        })?;

    let mut cumulative_capacity: i128 = 0;
    let mut cumulative_used: i128 = 0;
    for (_, delta) in baseline_deltas {
        (cumulative_capacity, cumulative_used) = apply_owned_capacity_delta(
            cumulative_capacity,
            cumulative_used,
            delta.owned_capacity_delta,
            delta.owned_knowledge_delta,
            "building token baseline capacity history chart",
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
    }
    let mut daily_deltas: std::collections::BTreeMap<u32, (i128, i128)> =
        std::collections::BTreeMap::new();
    for (date, delta) in deltas {
        let entry = daily_deltas.entry(date).or_insert((0, 0));
        entry.0 = entry
            .0
            .checked_add(delta.owned_capacity_delta)
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "capacity delta overflow while building token capacity history chart: date={}",
                    date
                ))
            })?;
        entry.1 = entry
            .1
            .checked_add(delta.owned_knowledge_delta)
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "used delta overflow while building token capacity history chart: date={}",
                    date
                ))
            })?;
    }

    let chart_bounds = match (from_date, to_date) {
        (Some(from), Some(to)) => Some((from, to)),
        (Some(from), None) => daily_deltas
            .keys()
            .next_back()
            .copied()
            .map(|last| (from, last)),
        (None, Some(to)) => daily_deltas.keys().next().copied().map(|first| (first, to)),
        (None, None) => {
            let first = daily_deltas.keys().next().copied();
            let last = daily_deltas.keys().next_back().copied();
            first.zip(last)
        }
    };

    let dates = if let Some((start, end)) = chart_bounds {
        date_keys_inclusive(start, end).map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        Vec::new()
    };

    let mut data = Vec::with_capacity(dates.len());
    for date in dates {
        let (capacity_delta, used_delta) = daily_deltas.get(&date).copied().unwrap_or((0, 0));
        (cumulative_capacity, cumulative_used) = apply_owned_capacity_delta(
            cumulative_capacity,
            cumulative_used,
            capacity_delta,
            used_delta,
            &format!("building token capacity history chart at date {}", date),
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
        let unused = cumulative_capacity - cumulative_used;

        data.push(StackedAreaDataPoint {
            date: format_yyyymmdd_for_chart(date),
            values: HashMap::from([
                ("used".to_string(), cumulative_used.to_string()),
                ("unused".to_string(), unused.to_string()),
            ]),
        });
    }

    let title = token
        .symbol
        .or(token.name)
        .unwrap_or_else(|| format!("0x{}", hex::encode(&hash)));

    ok(StackedAreaChartResponse {
        data,
        series: vec![
            StackedAreaSeries {
                key: "used".to_string(),
                label: "Used".to_string(),
                color: "#f59e0b".to_string(),
            },
            StackedAreaSeries {
                key: "unused".to_string(),
                label: "Unused".to_string(),
                color: "#00c389".to_string(),
            },
        ],
        title: format!("{title} Capacity History"),
    })
}

#[cfg(test)]
mod tests {
    use super::{is_plain_xudt_without_extension, max_supply_status};
    use crate::warmup::CachedAssetEntry;

    #[test]
    fn compatible_token_does_not_inherit_xudt_issuance_semantics() {
        assert_eq!(
            max_supply_status("xudt_compatible", None, Some(&[0x11; 32])),
            "unknown"
        );
    }

    #[test]
    fn xudt_owner_mode_flags_do_not_prove_plain_lock_issuance() {
        let mut args = vec![0x11; 32];
        args.extend_from_slice(&0x2000_0000u32.to_le_bytes());
        assert!(!is_plain_xudt_without_extension(Some(&args)));
        assert_eq!(max_supply_status("xudt", None, Some(&args)), "unknown");
    }

    #[test]
    fn test_is_plain_xudt_without_extension_for_compatible_and_zero_flags() {
        let compatible = vec![0x11; 32];
        assert!(is_plain_xudt_without_extension(Some(&compatible)));

        let mut explicit_zero_flags = vec![0x22; 32];
        explicit_zero_flags.extend_from_slice(&0u32.to_le_bytes());
        assert!(is_plain_xudt_without_extension(Some(&explicit_zero_flags)));
    }

    #[test]
    fn test_is_plain_xudt_without_extension_rejects_extended_or_invalid_args() {
        let mut with_extension = vec![0x33; 32];
        with_extension.extend_from_slice(&1u32.to_le_bytes());
        assert!(!is_plain_xudt_without_extension(Some(&with_extension)));

        let invalid_len = vec![0x44; 20];
        assert!(!is_plain_xudt_without_extension(Some(&invalid_len)));
        assert!(!is_plain_xudt_without_extension(None));
    }

    #[test]
    fn test_max_supply_status_priority_and_fallbacks() {
        let mut plain_xudt = vec![0x55; 32];
        plain_xudt.extend_from_slice(&0u32.to_le_bytes());
        let mut ext_xudt = vec![0x66; 32];
        ext_xudt.extend_from_slice(&1u32.to_le_bytes());

        assert_eq!(
            max_supply_status("xudt", Some("123"), Some(&ext_xudt)),
            "limited"
        );
        assert_eq!(max_supply_status("sudt", None, None), "unlimited");
        assert_eq!(
            max_supply_status("xudt_compatible", None, Some(&plain_xudt)),
            "unknown"
        );
        assert_eq!(max_supply_status("xudt", None, Some(&ext_xudt)), "unknown");
    }

    #[test]
    fn test_serve_tokens_from_cache_keeps_maximum_supply_and_status() {
        let mut xudt_plain_args = vec![0x11; 32];
        xudt_plain_args.extend_from_slice(&0u32.to_le_bytes());
        let mut xudt_ext_args = vec![0x22; 32];
        xudt_ext_args.extend_from_slice(&1u32.to_le_bytes());

        let cached = vec![
            CachedAssetEntry {
                id: "0x01".to_string(),
                asset_type: "token".to_string(),
                standard: "xudt".to_string(),
                name: Some("Limited".to_string()),
                symbol: Some("CAP".to_string()),
                icon_url: None,
                holders_count: 4,
                transfers_count: 10,
                live_count: None,
                activities_count: None,
                transfers_24h: 1,
                decimals: Some(8),
                total_supply: Some("500".to_string()),
                maximum_supply: Some("1000".to_string()),
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                owned_capacity: Some("1".to_string()),
                owned_knowledge: Some("1".to_string()),
                composition_tier: None,
                onchain_ratio: None,
                onchain_count: None,
                type_code_hash: Some("0xaaa".to_string()),
                type_hash_type: Some("type".to_string()),
                type_args: Some(format!("0x{}", hex::encode(&xudt_plain_args))),
                description: None,
            },
            CachedAssetEntry {
                id: "0x02".to_string(),
                asset_type: "token".to_string(),
                standard: "xudt".to_string(),
                name: Some("Plain".to_string()),
                symbol: Some("PX".to_string()),
                icon_url: None,
                holders_count: 3,
                transfers_count: 10,
                live_count: None,
                activities_count: None,
                transfers_24h: 1,
                decimals: Some(8),
                total_supply: Some("500".to_string()),
                maximum_supply: None,
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                owned_capacity: Some("1".to_string()),
                owned_knowledge: Some("1".to_string()),
                composition_tier: None,
                onchain_ratio: None,
                onchain_count: None,
                type_code_hash: Some("0xbbb".to_string()),
                type_hash_type: Some("type".to_string()),
                type_args: Some(format!("0x{}", hex::encode(&xudt_plain_args))),
                description: None,
            },
            CachedAssetEntry {
                id: "0x03".to_string(),
                asset_type: "token".to_string(),
                standard: "xudt".to_string(),
                name: Some("Extended".to_string()),
                symbol: Some("EX".to_string()),
                icon_url: None,
                holders_count: 2,
                transfers_count: 10,
                live_count: None,
                activities_count: None,
                transfers_24h: 1,
                decimals: Some(8),
                total_supply: Some("500".to_string()),
                maximum_supply: None,
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                owned_capacity: Some("1".to_string()),
                owned_knowledge: Some("1".to_string()),
                composition_tier: None,
                onchain_ratio: None,
                onchain_count: None,
                type_code_hash: Some("0xccc".to_string()),
                type_hash_type: Some("type".to_string()),
                type_args: Some(format!("0x{}", hex::encode(&xudt_ext_args))),
                description: None,
            },
            CachedAssetEntry {
                id: "0x04".to_string(),
                asset_type: "token".to_string(),
                standard: "sudt".to_string(),
                name: Some("sUDT".to_string()),
                symbol: Some("SD".to_string()),
                icon_url: None,
                holders_count: 1,
                transfers_count: 10,
                live_count: None,
                activities_count: None,
                transfers_24h: 1,
                decimals: Some(8),
                total_supply: Some("500".to_string()),
                maximum_supply: None,
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                owned_capacity: Some("1".to_string()),
                owned_knowledge: Some("1".to_string()),
                composition_tier: None,
                onchain_ratio: None,
                onchain_count: None,
                type_code_hash: Some("0xddd".to_string()),
                type_hash_type: Some("type".to_string()),
                type_args: Some("0x1234".to_string()),
                description: None,
            },
        ];

        let params = super::ListParams {
            limit: 20,
            standard: None,
            cursor: None,
            search: None,
        };

        let axum::Json(resp) = super::serve_tokens_from_cache(cached, &params, 20).unwrap();
        let by_symbol: std::collections::HashMap<_, _> = resp
            .data
            .iter()
            .filter_map(|row| row.symbol.as_ref().map(|sym| (sym.as_str(), row)))
            .collect();

        let cap = by_symbol.get("CAP").unwrap();
        assert_eq!(cap.maximum_supply.as_deref(), Some("1000"));
        assert_eq!(cap.maximum_supply_status, "limited");

        let px = by_symbol.get("PX").unwrap();
        assert!(px.maximum_supply.is_none());
        assert_eq!(px.maximum_supply_status, "unlimited");

        let ex = by_symbol.get("EX").unwrap();
        assert!(ex.maximum_supply.is_none());
        assert_eq!(ex.maximum_supply_status, "unknown");

        let sd = by_symbol.get("SD").unwrap();
        assert!(sd.maximum_supply.is_none());
        assert_eq!(sd.maximum_supply_status, "unlimited");
    }

    #[test]
    fn token_cache_missing_required_fields_must_not_serialize_as_empty_values() {
        let valid = CachedAssetEntry {
            id: "0x01".to_string(),
            asset_type: "token".to_string(),
            standard: "sudt".to_string(),
            name: None,
            symbol: None,
            icon_url: None,
            holders_count: 1,
            transfers_count: 0,
            live_count: None,
            activities_count: None,
            transfers_24h: 0,
            decimals: Some(8),
            total_supply: Some("0".to_string()),
            maximum_supply: None,
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
            owned_capacity: None,
            owned_knowledge: None,
            composition_tier: None,
            onchain_ratio: None,
            onchain_count: None,
            type_code_hash: Some("0x11".to_string()),
            type_hash_type: Some("type".to_string()),
            type_args: Some("0x".to_string()),
            description: None,
        };
        let params = super::ListParams {
            limit: 20,
            standard: None,
            cursor: None,
            search: None,
        };
        let mut cases = Vec::new();
        let mut missing = valid.clone();
        missing.type_code_hash = None;
        cases.push(missing);
        let mut missing = valid.clone();
        missing.type_hash_type = None;
        cases.push(missing);
        let mut missing = valid.clone();
        missing.type_args = None;
        cases.push(missing);
        let mut missing = valid.clone();
        missing.total_supply = None;
        cases.push(missing);
        let mut invalid = valid;
        invalid.type_args = Some("0xzz".to_string());
        cases.push(invalid);
        for entry in cases {
            assert!(super::serve_tokens_from_cache(vec![entry], &params, 20).is_err());
        }
    }

    #[test]
    fn test_serve_tokens_from_cache_cursor_preserves_equal_holder_counts() {
        let cached = vec![
            CachedAssetEntry {
                id: "0x01".to_string(),
                asset_type: "token".to_string(),
                standard: "xudt".to_string(),
                name: Some("Alpha".to_string()),
                symbol: Some("A".to_string()),
                icon_url: None,
                holders_count: 10,
                transfers_count: 0,
                live_count: None,
                activities_count: None,
                transfers_24h: 0,
                decimals: Some(8),
                total_supply: Some("1".to_string()),
                maximum_supply: None,
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                owned_capacity: Some("1".to_string()),
                owned_knowledge: Some("1".to_string()),
                composition_tier: None,
                onchain_ratio: None,
                onchain_count: None,
                type_code_hash: Some("0xaaa".to_string()),
                type_hash_type: Some("type".to_string()),
                type_args: Some("0x11".to_string()),
                description: None,
            },
            CachedAssetEntry {
                id: "0x02".to_string(),
                asset_type: "token".to_string(),
                standard: "xudt".to_string(),
                name: Some("Beta".to_string()),
                symbol: Some("B".to_string()),
                icon_url: None,
                holders_count: 10,
                transfers_count: 0,
                live_count: None,
                activities_count: None,
                transfers_24h: 0,
                decimals: Some(8),
                total_supply: Some("1".to_string()),
                maximum_supply: None,
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                owned_capacity: Some("1".to_string()),
                owned_knowledge: Some("1".to_string()),
                composition_tier: None,
                onchain_ratio: None,
                onchain_count: None,
                type_code_hash: Some("0xbbb".to_string()),
                type_hash_type: Some("type".to_string()),
                type_args: Some("0x22".to_string()),
                description: None,
            },
            CachedAssetEntry {
                id: "0x03".to_string(),
                asset_type: "token".to_string(),
                standard: "xudt".to_string(),
                name: Some("Gamma".to_string()),
                symbol: Some("C".to_string()),
                icon_url: None,
                holders_count: 9,
                transfers_count: 0,
                live_count: None,
                activities_count: None,
                transfers_24h: 0,
                decimals: Some(8),
                total_supply: Some("1".to_string()),
                maximum_supply: None,
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                owned_capacity: Some("1".to_string()),
                owned_knowledge: Some("1".to_string()),
                composition_tier: None,
                onchain_ratio: None,
                onchain_count: None,
                type_code_hash: Some("0xccc".to_string()),
                type_hash_type: Some("type".to_string()),
                type_args: Some("0x33".to_string()),
                description: None,
            },
        ];

        let first_params = super::ListParams {
            limit: 1,
            standard: None,
            cursor: None,
            search: None,
        };
        let axum::Json(first_page) =
            super::serve_tokens_from_cache(cached.clone(), &first_params, 1).unwrap();
        assert_eq!(first_page.data.len(), 1);
        assert_eq!(first_page.data[0].type_script_hash, "0x01");
        let next_cursor = first_page.next_cursor.clone().expect("next cursor");

        let second_params = super::ListParams {
            limit: 2,
            standard: None,
            cursor: Some(next_cursor),
            search: None,
        };
        let axum::Json(second_page) =
            super::serve_tokens_from_cache(cached, &second_params, 2).unwrap();

        let hashes: Vec<_> = second_page
            .data
            .iter()
            .map(|row| row.type_script_hash.as_str())
            .collect();
        assert_eq!(hashes, vec!["0x02", "0x03"]);
    }

    #[test]
    fn test_parse_token_holder_cursor_valid() {
        let cursor = format!("100:{}", "aa".repeat(32));
        let (balance, lock_hash) = super::parse_token_holder_cursor(&cursor).unwrap();
        assert_eq!(balance, 100);
        assert_eq!(lock_hash, vec![0xAA; 32]);
    }

    #[test]
    fn test_parse_token_holder_cursor_accepts_balance_above_u128() {
        let cursor = format!(
            "531691198313966349161522824112137830400:{}",
            "aa".repeat(32)
        );
        let (balance, _) = super::parse_token_holder_cursor(&cursor).unwrap();
        assert_eq!(
            balance.to_string(),
            "531691198313966349161522824112137830400"
        );
    }

    #[test]
    fn test_parse_token_holder_cursor_rejects_wrong_length_hash() {
        assert!(super::parse_token_holder_cursor("100:aabbcc").is_err());
    }

    #[test]
    fn test_serve_tokens_from_cache_unknown_decimals_serialize_as_null() {
        // B5: a token with no label and no on-chain info has unknown decimals.
        // The list endpoint must surface null, not a fabricated 0.
        let cached = vec![CachedAssetEntry {
            id: "0x0a".to_string(),
            asset_type: "token".to_string(),
            standard: "xudt".to_string(),
            name: None,
            symbol: None,
            icon_url: None,
            holders_count: 1,
            transfers_count: 1,
            live_count: None,
            activities_count: None,
            transfers_24h: 0,
            decimals: None,
            total_supply: Some("500".to_string()),
            maximum_supply: None,
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
            owned_capacity: None,
            owned_knowledge: None,
            composition_tier: None,
            onchain_ratio: None,
            onchain_count: None,
            type_code_hash: Some("0xaaa".to_string()),
            type_hash_type: Some("type".to_string()),
            type_args: Some("0x11".to_string()),
            description: None,
        }];

        let params = super::ListParams {
            limit: 20,
            standard: None,
            cursor: None,
            search: None,
        };

        let axum::Json(resp) = super::serve_tokens_from_cache(cached, &params, 20).unwrap();
        let json = serde_json::to_value(&resp.data[0]).unwrap();
        assert_eq!(
            json["decimals"],
            serde_json::Value::Null,
            "unknown decimals must serialize as null, got {}",
            json["decimals"]
        );
    }
}
