use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use ckbadger_store::types::{DID_CKB_SENTINEL_COLLECTION, DOTBIT_SENTINEL_COLLECTION};
use ckbadger_store::CkbadgerStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use super::assets::{
    decode_activity_cursor, decode_nft_item_cursor, list_canonical_nft_collection_activities_page,
    list_identity_items_inner, normalize_identity_activity_action_filter,
    normalize_nft_items_search, normalize_nft_items_status, NftCollectionActivitiesParams,
    NftCollectionActivityResponse, NftCollectionHolderResponse, NftCollectionItemResponse,
    NftItemsParams,
};
use crate::cache::InMemoryCache;
use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::AppState;

type ApiRouteError = (axum::http::StatusCode, Json<ApiError>);

/// Decode an identity collection ID from a URL path segment.
///
/// Accepts human-readable aliases ("dotbit", ".bit", "did:ckb", "did_ckb")
/// and hex-encoded sentinel IDs. Rejects any collection ID that does not
/// resolve to one of the two identity sentinels.
fn decode_identity_collection_id(
    raw: &str,
) -> Result<Vec<u8>, (axum::http::StatusCode, Json<ApiError>)> {
    let normalized = raw.to_ascii_lowercase();
    if normalized == "dotbit" || normalized == ".bit" {
        return Ok(DOTBIT_SENTINEL_COLLECTION.to_vec());
    }
    if normalized == "did:ckb" || normalized == "did_ckb" {
        return Ok(DID_CKB_SENTINEL_COLLECTION.to_vec());
    }
    let bytes = hex::decode(raw.strip_prefix("0x").unwrap_or(raw))
        .map_err(|_| ApiError::bad_request("Invalid identity collection ID"))?;
    if bytes != DOTBIT_SENTINEL_COLLECTION && bytes != DID_CKB_SENTINEL_COLLECTION {
        return Err(ApiError::bad_request(
            "Collection ID is not an identity collection",
        ));
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityCollectionDetailResponse {
    pub collection_id: String,
    pub standard: String,
    pub name: Option<String>,
    pub total_count: i64,
    pub live_count: i64,
    pub holders_count: i64,
    pub activities_count: i64,
}

async fn get_identity_collection(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
) -> ApiResult<IdentityCollectionDetailResponse> {
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;

    let agg = state
        .store
        .get_identity_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Identity collection not found"))?;

    if agg.holders_count < 0 {
        return Err(ApiError::internal(format!(
            "invalid identity collection aggregate holders_count: collection_id=0x{}, holders_count={}",
            hex::encode(&collection_id_bytes),
            agg.holders_count
        )));
    }

    let standard = agg.standard.asset_standard().to_string();
    let name = agg.name;
    let activities_count = agg.activities_count;

    ok(IdentityCollectionDetailResponse {
        collection_id: format!("0x{}", hex::encode(&collection_id_bytes)),
        standard,
        name,
        total_count: agg.total_count,
        live_count: agg.live_count,
        holders_count: agg.holders_count,
        activities_count,
    })
}

// -- Holders endpoint --

const IDENTITY_HOLDER_LIST_CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize)]
pub struct IdentityCollectionHoldersParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

fn collect_identity_holder_counts(
    store: &CkbadgerStore,
    collection_id_bytes: &[u8],
) -> Result<Vec<(Vec<u8>, i64)>, ApiRouteError> {
    store
        .list_identity_owner_counts(collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))
}

fn list_identity_holders_ranked(
    store: &CkbadgerStore,
    collection_id_bytes: &[u8],
) -> Result<Vec<(Vec<u8>, i64)>, ApiRouteError> {
    let mut holders = collect_identity_holder_counts(store, collection_id_bytes)?;
    holders.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(holders)
}

fn list_identity_holders_ranked_cached(
    store: &CkbadgerStore,
    mem_cache: &InMemoryCache,
    collection_id_bytes: &[u8],
) -> Result<Vec<(Vec<u8>, i64)>, ApiRouteError> {
    let cache_key = format!(
        "assets:identity_collection_holders_ranked:0x{}",
        hex::encode(collection_id_bytes)
    );
    if let Some(cached) = mem_cache.get::<Vec<(Vec<u8>, i64)>>(&cache_key) {
        return Ok(cached);
    }

    let holders = list_identity_holders_ranked(store, collection_id_bytes)?;
    mem_cache.set(&cache_key, &holders, IDENTITY_HOLDER_LIST_CACHE_TTL);
    Ok(holders)
}

fn decode_identity_holders_cursor(raw: &str) -> Result<(i64, Vec<u8>), ApiRouteError> {
    let mut parts = raw.split(':');
    let count = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid identity collection holders cursor"))?
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("Invalid identity collection holders cursor"))?;
    let lock_hash_hex = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid identity collection holders cursor"))?;
    if parts.next().is_some() {
        return Err(ApiError::bad_request(
            "Invalid identity collection holders cursor",
        ));
    }
    let lock_hash = hex::decode(lock_hash_hex.strip_prefix("0x").unwrap_or(lock_hash_hex))
        .map_err(|_| ApiError::bad_request("Invalid identity collection holders cursor"))?;
    if lock_hash.len() != 32 {
        return Err(ApiError::bad_request(
            "Invalid identity collection holders cursor",
        ));
    }
    Ok((count, lock_hash))
}

async fn list_identity_collection_holders(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Query(params): Query<IdentityCollectionHoldersParams>,
) -> ApiResult<CursorPaginatedResponse<NftCollectionHolderResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_identity_holders_cursor)
        .transpose()?;

    let agg = state
        .store
        .get_identity_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Identity collection not found"))?;
    if agg.holders_count < 0 {
        return Err(ApiError::internal(format!(
            "invalid identity collection aggregate holders_count: collection_id=0x{}, holders_count={}",
            hex::encode(&collection_id_bytes),
            agg.holders_count
        )));
    }

    let holders = list_identity_holders_ranked_cached(
        state.store.as_ref(),
        &state.mem_cache,
        &collection_id_bytes,
    )?;

    let total = agg.holders_count;
    let start_idx = if let Some((cursor_count, cursor_lock_hash)) = cursor {
        holders
            .iter()
            .position(|(lock_hash, count)| *count == cursor_count && *lock_hash == cursor_lock_hash)
            .map(|idx| idx + 1)
            .ok_or_else(|| ApiError::bad_request("Invalid identity collection holders cursor"))?
    } else {
        0
    };

    let page: Vec<_> = holders.iter().skip(start_idx).take(limit + 1).collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(lock_hash, count)| format!("{}:{}", count, hex::encode(lock_hash)))
    } else {
        None
    };

    let rows: Vec<NftCollectionHolderResponse> = page
        .into_iter()
        .map(|(lock_hash, count)| NftCollectionHolderResponse {
            lock_script_hash: format!("0x{}", hex::encode(lock_hash)),
            address: None,
            item_count: *count,
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        rows,
        total,
        limit as i64,
        next_cursor,
    ))
}

// -- Activities endpoint --

async fn list_identity_collection_activities(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Query(params): Query<NftCollectionActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<NftCollectionActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let action_filter = normalize_identity_activity_action_filter(params.action.as_deref())?;

    // Validate collection exists
    let _agg = state
        .store
        .get_identity_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Identity collection not found"))?;

    // Fetch canonical rows only; skip orphaned history entries.
    let results = list_canonical_nft_collection_activities_page(
        state.store.as_ref(),
        state.store.as_ref(),
        &collection_id_bytes,
        (limit as usize) + 1,
        cursor,
        action_filter.as_deref(),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = results.len() as i64 > limit;
    let page: Vec<NftCollectionActivityResponse> = results
        .into_iter()
        .take(limit as usize)
        .map(|(block_number, tx_index, entry)| {
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
            NftCollectionActivityResponse {
                tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
                block_number,
                tx_index,
                timestamp: entry.timestamp_ms.to_string(),
                actions,
            }
        })
        .collect();

    let next_cursor = if has_more {
        page.last()
            .map(|row| format!("{}:{}", row.block_number, row.tx_index))
    } else {
        None
    };

    ok(CursorPaginatedResponse::without_total(
        page,
        limit,
        next_cursor,
    ))
}

// -- Items endpoint --

async fn list_identity_collection_items(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Query(params): Query<NftItemsParams>,
) -> ApiResult<CursorPaginatedResponse<NftCollectionItemResponse>> {
    let limit = params.limit.clamp(1, 100);
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;
    let search_lower = normalize_nft_items_search(params.search.as_deref());
    let status_filter = normalize_nft_items_status(params.status.as_deref())?;
    let cursor_bytes = params
        .cursor
        .as_deref()
        .map(decode_nft_item_cursor)
        .transpose()?;

    let agg = state
        .store
        .get_identity_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Identity collection not found"))?;

    // Convert to ObjectCollectionAggregate for the shared inner function
    let obj_agg = ckbadger_store::types::ObjectCollectionAggregate {
        name: agg.name,
        standard: match agg.standard {
            ckbadger_store::types::IdentityStandard::DotBit => {
                ckbadger_store::types::ObjectStandard::Spore
            }
            ckbadger_store::types::IdentityStandard::DidCkb => {
                ckbadger_store::types::ObjectStandard::Spore
            }
        },
        total_count: agg.total_count,
        live_count: agg.live_count,
        holders_count: agg.holders_count,
        activities_count: agg.activities_count,
    };

    list_identity_items_inner(
        state.store.as_ref(),
        state.append_only_store.as_ref(),
        &collection_id_bytes,
        limit,
        cursor_bytes,
        search_lower.as_deref(),
        status_filter,
        &obj_agg,
    )
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/assets/identities/{collection_id}",
            get(get_identity_collection),
        )
        .route(
            "/assets/identities/{collection_id}/holders",
            get(list_identity_collection_holders),
        )
        .route(
            "/assets/identities/{collection_id}/activities",
            get(list_identity_collection_activities),
        )
        .route(
            "/assets/identities/{collection_id}/items",
            get(list_identity_collection_items),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_identity_collection_id_aliases() {
        let dotbit = decode_identity_collection_id("dotbit").unwrap();
        assert_eq!(dotbit, DOTBIT_SENTINEL_COLLECTION.to_vec());

        let dotbit_alt = decode_identity_collection_id(".bit").unwrap();
        assert_eq!(dotbit_alt, DOTBIT_SENTINEL_COLLECTION.to_vec());

        let did_ckb = decode_identity_collection_id("did:ckb").unwrap();
        assert_eq!(did_ckb, DID_CKB_SENTINEL_COLLECTION.to_vec());

        let did_ckb_alt = decode_identity_collection_id("did_ckb").unwrap();
        assert_eq!(did_ckb_alt, DID_CKB_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_decode_identity_collection_id_case_insensitive() {
        let result = decode_identity_collection_id("DotBit").unwrap();
        assert_eq!(result, DOTBIT_SENTINEL_COLLECTION.to_vec());

        let result = decode_identity_collection_id("DID:CKB").unwrap();
        assert_eq!(result, DID_CKB_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_decode_identity_collection_id_hex() {
        let hex_id = format!("0x{}", hex::encode(DOTBIT_SENTINEL_COLLECTION));
        let result = decode_identity_collection_id(&hex_id).unwrap();
        assert_eq!(result, DOTBIT_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_decode_identity_collection_id_rejects_non_identity() {
        // Random 32-byte hex that isn't an identity sentinel
        let non_identity = "0x".to_string() + &"aa".repeat(32);
        let result = decode_identity_collection_id(&non_identity);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_identity_collection_id_rejects_invalid_hex() {
        let result = decode_identity_collection_id("not_hex_at_all_zzzz");
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_identity_holders_cursor_valid() {
        let lock_hash_hex = "aa".repeat(32);
        let cursor = format!("42:{}", lock_hash_hex);
        let (count, lock_hash) = decode_identity_holders_cursor(&cursor).unwrap();
        assert_eq!(count, 42);
        assert_eq!(lock_hash.len(), 32);
        assert_eq!(hex::encode(&lock_hash), lock_hash_hex);
    }

    #[test]
    fn test_decode_identity_holders_cursor_with_0x_prefix() {
        let lock_hash_hex = "bb".repeat(32);
        let cursor = format!("10:0x{}", lock_hash_hex);
        let (count, lock_hash) = decode_identity_holders_cursor(&cursor).unwrap();
        assert_eq!(count, 10);
        assert_eq!(hex::encode(&lock_hash), lock_hash_hex);
    }

    #[test]
    fn test_decode_identity_holders_cursor_rejects_extra_parts() {
        let cursor = format!("42:{}:extra", "aa".repeat(32));
        assert!(decode_identity_holders_cursor(&cursor).is_err());
    }

    #[test]
    fn test_decode_identity_holders_cursor_rejects_bad_count() {
        let cursor = format!("notanum:{}", "aa".repeat(32));
        assert!(decode_identity_holders_cursor(&cursor).is_err());
    }

    #[test]
    fn test_decode_identity_holders_cursor_rejects_wrong_length_hash() {
        // Only 16 bytes instead of 32
        let cursor = format!("5:{}", "cc".repeat(16));
        assert!(decode_identity_holders_cursor(&cursor).is_err());
    }

    #[test]
    fn test_decode_identity_holders_cursor_rejects_missing_hash() {
        assert!(decode_identity_holders_cursor("42").is_err());
    }
}
