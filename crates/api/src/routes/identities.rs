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
    build_nft_item_activities_response, decode_activity_cursor, decode_item_id,
    decode_nft_item_cursor, list_canonical_nft_collection_activities_page,
    list_identity_items_inner, normalize_activity_action_filter,
    normalize_identity_activity_action_filter, normalize_nft_items_search,
    normalize_nft_items_status, CollectionActivitiesParams, CollectionActivityResponse,
    CollectionHolderResponse, CollectionItemResponse, MnftItemActivitiesParams,
    MnftItemActivityResponse, NftLifecycleStandard, ObjectItemsParams,
};
use crate::cache::InMemoryCache;
use crate::response::{
    default_limit, ok, ApiError, ApiResult, ApiRouteError, CursorPaginatedResponse,
};
use crate::utils::accumulate_owned_capacity;
use crate::AppState;

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
    pub owned_capacity: String,
    pub owned_knowledge: String,
}

async fn get_identity_collection(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
) -> ApiResult<IdentityCollectionDetailResponse> {
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;

    let store = state.store.clone();
    let collection_id_bytes_c = collection_id_bytes.clone();
    let agg = tokio::task::spawn_blocking(move || {
        store.get_identity_collection_aggregate(&collection_id_bytes_c)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
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

    let store2 = state.store.clone();
    let collection_id_bytes_c2 = collection_id_bytes.clone();
    let (owned_capacity, owned_knowledge) = tokio::task::spawn_blocking(move || {
        let daily = store2.list_object_daily_deltas(&collection_id_bytes_c2)?;
        accumulate_owned_capacity(
            daily
                .into_iter()
                .map(|(_, delta)| (delta.owned_capacity_delta, delta.owned_knowledge_delta)),
        )
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(IdentityCollectionDetailResponse {
        collection_id: format!("0x{}", hex::encode(&collection_id_bytes)),
        standard,
        name,
        total_count: agg.total_count,
        live_count: agg.live_count,
        holders_count: agg.holders_count,
        activities_count,
        owned_capacity: owned_capacity.to_string(),
        owned_knowledge: owned_knowledge.to_string(),
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
) -> ApiResult<CursorPaginatedResponse<CollectionHolderResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_identity_holders_cursor)
        .transpose()?;

    let store = state.store.clone();
    let collection_id_bytes_c = collection_id_bytes.clone();
    let agg = tokio::task::spawn_blocking(move || {
        store.get_identity_collection_aggregate(&collection_id_bytes_c)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
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

    let rows: Vec<CollectionHolderResponse> = page
        .into_iter()
        .map(|(lock_hash, count)| CollectionHolderResponse {
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
    Query(params): Query<CollectionActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<CollectionActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let action_filter = normalize_identity_activity_action_filter(params.action.as_deref())?;

    // Validate collection exists
    let store = state.store.clone();
    let collection_id_bytes_c = collection_id_bytes.clone();
    let _agg = tokio::task::spawn_blocking(move || {
        store.get_identity_collection_aggregate(&collection_id_bytes_c)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
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
    let page: Vec<CollectionActivityResponse> = results
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
            CollectionActivityResponse {
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
    Query(params): Query<ObjectItemsParams>,
) -> ApiResult<CursorPaginatedResponse<CollectionItemResponse>> {
    let limit = params.limit.clamp(1, 100);
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;
    let search_lower = normalize_nft_items_search(params.search.as_deref());
    let status_filter = normalize_nft_items_status(params.status.as_deref())?;
    let cursor_bytes = params
        .cursor
        .as_deref()
        .map(decode_nft_item_cursor)
        .transpose()?;

    let store = state.store.clone();
    let collection_id_bytes_c = collection_id_bytes.clone();
    let agg = tokio::task::spawn_blocking(move || {
        store.get_identity_collection_aggregate(&collection_id_bytes_c)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
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
        ..Default::default()
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

// -- Identity item detail endpoints (moved from assets.rs) --

async fn get_dotbit_item_detail(
    State(state): State<Arc<AppState>>,
    Path(identity_id): Path<String>,
) -> ApiResult<CollectionItemResponse> {
    let identity_id_bytes = decode_item_id(&identity_id)?;
    let store = state.store.clone();
    let identity_id_bytes_c = identity_id_bytes.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_identity(&identity_id_bytes_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(".bit item not found"))?;

    if !matches!(
        entry.standard,
        ckbadger_store::types::IdentityStandard::DotBit
    ) {
        return Err(ApiError::bad_request("Item is not a .bit account"));
    }

    let (expired_at, registered_at, status) = match &entry.extra {
        ckbadger_store::types::IdentityExtra::DotBit {
            expired_at,
            registered_at,
            status,
        } => (*expired_at, *registered_at, *status),
        _ => {
            return Err(ApiError::internal(format!(
                "invalid identity entry extra type for .bit account: identity_id=0x{}",
                hex::encode(&identity_id_bytes)
            )))
        }
    };

    let (tx_hash, output_index) = if entry.is_live {
        let outpoint_map = state
            .store
            .get_live_dotbit_outpoints_by_account_ids(
                std::slice::from_ref(&identity_id_bytes),
                &state.append_only_store,
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (tx_hash, output_index) = outpoint_map.get(&identity_id_bytes).ok_or_else(|| {
            ApiError::internal(format!(
                "live dotbit account missing outpoint index: identity_id=0x{}",
                hex::encode(&identity_id_bytes)
            ))
        })?;
        (
            Some(format!("0x{}", hex::encode(tx_hash))),
            Some(*output_index),
        )
    } else {
        (None, None)
    };

    ok(CollectionItemResponse {
        nft_id: format!("0x{}", hex::encode(&identity_id_bytes)),
        name: entry.name,
        standard: entry.standard.asset_standard().to_string(),
        owner_lock_hash: entry
            .owner_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        is_live: entry.is_live,
        created_at_block: entry.created_at_block,
        expired_at,
        registered_at,
        status,
        tx_hash,
        output_index,
    })
}

async fn get_did_ckb_item_detail(
    State(state): State<Arc<AppState>>,
    Path(identity_id): Path<String>,
) -> ApiResult<CollectionItemResponse> {
    let identity_id_bytes = decode_item_id(&identity_id)?;
    let store = state.store.clone();
    let identity_id_bytes_c = identity_id_bytes.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_identity(&identity_id_bytes_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("did:ckb item not found"))?;

    if entry.standard != ckbadger_store::types::IdentityStandard::DidCkb {
        return Err(ApiError::bad_request("Item is not a did:ckb identity"));
    }

    ok(CollectionItemResponse {
        nft_id: format!("0x{}", hex::encode(&identity_id_bytes)),
        name: entry.name,
        standard: "did_ckb".to_string(),
        owner_lock_hash: entry
            .owner_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        is_live: entry.is_live,
        created_at_block: entry.created_at_block,
        expired_at: None,
        registered_at: None,
        status: None,
        tx_hash: None,
        output_index: None,
    })
}

async fn list_dotbit_item_activities(
    State(state): State<Arc<AppState>>,
    Path(identity_id): Path<String>,
    Query(params): Query<MnftItemActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<MnftItemActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let action_filter = normalize_activity_action_filter(params.action.as_deref())?;
    let identity_id_bytes = decode_item_id(&identity_id)?;
    let store = state.store.clone();
    let identity_id_bytes_c = identity_id_bytes.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_identity(&identity_id_bytes_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(".bit item not found"))?;
    if !matches!(
        entry.standard,
        ckbadger_store::types::IdentityStandard::DotBit
    ) {
        return Err(ApiError::bad_request("Item is not a .bit account"));
    }

    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let response = build_nft_item_activities_response(
        &state,
        &identity_id_bytes,
        NftLifecycleStandard::DotBit,
        limit,
        cursor,
        action_filter.as_deref(),
    )?;
    ok(response)
}

async fn list_did_ckb_item_activities(
    State(state): State<Arc<AppState>>,
    Path(identity_id): Path<String>,
    Query(params): Query<MnftItemActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<MnftItemActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let action_filter = normalize_activity_action_filter(params.action.as_deref())?;
    let identity_id_bytes = decode_item_id(&identity_id)?;
    let store = state.store.clone();
    let identity_id_bytes_c = identity_id_bytes.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_identity(&identity_id_bytes_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("did:ckb item not found"))?;
    if entry.standard != ckbadger_store::types::IdentityStandard::DidCkb {
        return Err(ApiError::bad_request("Item is not a did:ckb identity"));
    }

    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let response = build_nft_item_activities_response(
        &state,
        &identity_id_bytes,
        NftLifecycleStandard::DidCkb,
        limit,
        cursor,
        action_filter.as_deref(),
    )?;
    ok(response)
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/assets/identities/dotbit/items/{identity_id}",
            get(get_dotbit_item_detail),
        )
        .route(
            "/assets/identities/dotbit/items/{identity_id}/activities",
            get(list_dotbit_item_activities),
        )
        .route(
            "/assets/identities/did/items/{identity_id}",
            get(get_did_ckb_item_detail),
        )
        .route(
            "/assets/identities/did/items/{identity_id}/activities",
            get(list_did_ckb_item_activities),
        )
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
