use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use ckbadger_store::types::{
    identity_alias, identity_sentinel_for, identity_sentinel_standard, LockScriptEntry,
    DOTCELL_SENTINEL_COLLECTION,
};
use ckbadger_store::CkbadgerStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use super::assets::{
    build_nft_item_activities_response, decode_activity_cursor, decode_item_id,
    decode_object_item_cursor, get_live_bit_cell_outpoints_by_identity_ids,
    list_canonical_nft_collection_activities_page, list_identity_items_inner,
    normalize_activity_action_filter, normalize_identity_activity_action_filter,
    normalize_nft_items_search, normalize_nft_items_status, CollectionActivitiesParams,
    CollectionActivityResponse, CollectionHolderResponse, CollectionItemResponse,
    MnftItemActivitiesParams, MnftItemActivityResponse, NftLifecycleStandard, ObjectItemsParams,
};
use crate::cache::InMemoryCache;
use crate::response::{
    default_limit, ok, ApiError, ApiResult, ApiRouteError, CursorPaginatedResponse,
};
use crate::utils::{accumulate_owned_capacity, parse_asset_id_max32};
use crate::AppState;

/// Decode an identity collection ID from a URL path segment.
///
/// Accepts every standard's aliases from the store's identity table and the
/// hex-encoded sentinel IDs. Rejects any collection ID that does not resolve to
/// an identity sentinel.
fn decode_identity_collection_id(
    raw: &str,
) -> Result<Vec<u8>, (axum::http::StatusCode, Json<ApiError>)> {
    if let Some(standard) = identity_alias(raw) {
        return Ok(identity_sentinel_for(standard).to_vec());
    }
    let bytes = parse_asset_id_max32(raw, "identity collection ID")?;
    if identity_sentinel_standard(&bytes).is_none() {
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
        let daily = store2.list_mnft_daily_deltas(&collection_id_bytes_c2)?;
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

    // `.cell` stores its owner as the 20-byte prefix the chain gives, padded
    // into the same fixed-width key. Decoding it by collection is what keeps a
    // padded prefix from being served as a lock hash nobody can look up.
    let is_dotcell = collection_id_bytes == DOTCELL_SENTINEL_COLLECTION;
    let resolver = DotCellPartyResolver {
        store: state.store.as_ref(),
        network: &state.ckb_network,
    };
    let mut rows: Vec<CollectionHolderResponse> = Vec::with_capacity(page.len());
    for (owner_segment, count) in page {
        if is_dotcell {
            let owner20 = ckbadger_store::keys::decode_identity_owner20(owner_segment);
            let party = resolver.resolve(&owner20)?;
            rows.push(CollectionHolderResponse {
                lock_script_hash: party.lock_hash,
                owner_hash_prefix: Some(party.hash_prefix),
                address: party.address,
                item_count: *count,
            });
        } else {
            rows.push(CollectionHolderResponse {
                lock_script_hash: Some(format!("0x{}", hex::encode(owner_segment))),
                owner_hash_prefix: None,
                address: None,
                item_count: *count,
            });
        }
    }

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
        .map(decode_object_item_cursor)
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

    // Convert to MnftCollectionAggregate for the shared inner function
    let obj_agg = ckbadger_store::types::MnftCollectionAggregate {
        name: agg.name,
        standard: match agg.standard {
            ckbadger_store::types::IdentityStandard::DotBit => {
                ckbadger_store::types::ObjectStandard::Spore
            }
            ckbadger_store::types::IdentityStandard::BitCell => {
                ckbadger_store::types::ObjectStandard::Spore
            }
            ckbadger_store::types::IdentityStandard::DidCkb => {
                ckbadger_store::types::ObjectStandard::Spore
            }
            ckbadger_store::types::IdentityStandard::DotCell => {
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

async fn get_bit_cell_item_detail(
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
        .ok_or_else(|| ApiError::not_found(".bit Cell identity not found"))?;

    if entry.standard != ckbadger_store::types::IdentityStandard::BitCell {
        return Err(ApiError::bad_request("Item is not a .bit Cell identity"));
    }
    let expired_at = match &entry.extra {
        ckbadger_store::types::IdentityExtra::BitCell { expired_at, .. } => Some(*expired_at),
        _ => {
            return Err(ApiError::internal(format!(
                ".bit Cell identity has wrong extra variant: identity_id=0x{}",
                hex::encode(&identity_id_bytes)
            )))
        }
    };
    let (tx_hash, output_index) = if entry.is_live {
        let outpoints = get_live_bit_cell_outpoints_by_identity_ids(
            state.store.as_ref(),
            state.append_only_store.as_ref(),
            std::slice::from_ref(&identity_id_bytes),
        )?;
        let (tx_hash, output_index) = outpoints.get(&identity_id_bytes).ok_or_else(|| {
            ApiError::internal(format!(
                "live .bit Cell identity missing outpoint index: identity_id=0x{}",
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
        registered_at: None,
        status: None,
        tx_hash,
        output_index,
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
    // Validated before the existence lookup: request shape is a boundary
    // concern, so whether a malformed cursor is reported must not depend on
    // whether the item happens to exist.
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
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
    // Validated before the existence lookup: request shape is a boundary
    // concern, so whether a malformed cursor is reported must not depend on
    // whether the item happens to exist.
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
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

async fn list_bit_cell_item_activities(
    State(state): State<Arc<AppState>>,
    Path(identity_id): Path<String>,
    Query(params): Query<MnftItemActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<MnftItemActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let action_filter = normalize_activity_action_filter(params.action.as_deref())?;
    let identity_id_bytes = decode_item_id(&identity_id)?;
    // Validated before the existence lookup: request shape is a boundary
    // concern, so whether a malformed cursor is reported must not depend on
    // whether the item happens to exist.
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let store = state.store.clone();
    let identity_id_bytes_c = identity_id_bytes.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_identity(&identity_id_bytes_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(".bit Cell identity not found"))?;
    if entry.standard != ckbadger_store::types::IdentityStandard::BitCell {
        return Err(ApiError::bad_request("Item is not a .bit Cell identity"));
    }

    let response = build_nft_item_activities_response(
        &state,
        &identity_id_bytes,
        NftLifecycleStandard::BitCell,
        limit,
        cursor,
        action_filter.as_deref(),
    )?;
    ok(response)
}

// ── `.cell` (DotCell) names ────────────────────────────────────────────────
//
// A `.cell` name stores its owner and manager as the first 20 bytes of a lock
// script hash. The API reports that prefix verbatim and resolves it through
// `CF_LOCK_SCRIPTS` when exactly one lock is known for it; an unresolved
// prefix is reported as a prefix, never as a fabricated address.

/// One party of a name, as the chain names it plus whatever it resolves to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartyRef {
    pub hash_prefix: String,
    pub lock_hash: Option<String>,
    pub address: Option<String>,
    pub script_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedAddressRef {
    pub address: String,
    pub lock_hash: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DotCellRecordResponse {
    pub key: String,
    pub label: String,
    pub value_hex: String,
    pub value_utf8: Option<String>,
    pub ttl: u32,
    pub decoded_address: Option<DecodedAddressRef>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DotCellNameRef {
    pub identity_id: String,
    pub label: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DotCellOutPointResponse {
    pub tx_hash: String,
    pub index: i16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DotCellSaleResponse {
    pub price_shannons: String,
    pub seller: PartyRef,
    pub offer_out_point: Option<DotCellOutPointResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DotCellItemResponse {
    pub identity_id: String,
    pub label: String,
    pub name: String,
    pub is_live: bool,
    pub created_at_block: i64,
    pub created_at_tx: String,
    pub layout_version: u8,
    pub namespace_args: String,
    pub expired_at: u64,
    pub state: &'static str,
    pub grace_ends_at: u64,
    pub owner: PartyRef,
    pub manager: PartyRef,
    pub sale: Option<DotCellSaleResponse>,
    pub records: Vec<DotCellRecordResponse>,
    pub records_hash: String,
    pub next_id: String,
    pub parent: Option<DotCellNameRef>,
    pub children: Vec<DotCellNameRef>,
    pub live_out_point: Option<DotCellOutPointResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DotCellRingResponse {
    pub namespace_args: String,
    pub root_out_point: DotCellOutPointResponse,
    pub first_id: String,
    pub live_count: i64,
}

/// Accept a 20-byte hex id, a bare label, or `label.cell`.
///
/// A name id is `blake2b(label)[..20]`, so a label needs no scan — but an
/// arbitrary string must not be hashed into a lookup either, or every typo
/// becomes a 404 on a name that could never exist. The grammar (spec §1.2) is
/// `[a-z0-9-]`, at most one dot, at most 40 characters.
fn decode_dotcell_item_ref(raw: &str) -> Result<([u8; 20], Option<String>), ApiRouteError> {
    if let Some(hex_part) = raw.strip_prefix("0x") {
        if hex_part.len() == 40 {
            let bytes = hex::decode(hex_part)
                .map_err(|_| ApiError::bad_request("Invalid .cell name id: not hex"))?;
            return Ok((bytes.try_into().expect("20 bytes"), None));
        }
        return Err(ApiError::bad_request(
            "Invalid .cell name id: expected 20 hex-encoded bytes",
        ));
    }

    let label = raw.strip_suffix(".cell").unwrap_or(raw).to_string();
    if label.is_empty() {
        return Err(ApiError::bad_request(
            "Empty .cell label: the empty name is the ring root, not an identity",
        ));
    }
    if label.chars().count() > 40 {
        return Err(ApiError::bad_request(
            "Invalid .cell label: at most 40 characters",
        ));
    }
    if label.matches('.').count() > 1 {
        return Err(ApiError::bad_request(
            "Invalid .cell label: at most one dot (a sub-name)",
        ));
    }
    if !label
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        return Err(ApiError::bad_request(
            "Invalid .cell label: only a-z, 0-9, - and one dot",
        ));
    }
    Ok((
        ckbadger_store::types::derive_dotcell_id(&label),
        Some(label),
    ))
}

/// The 30-day grace period the contract enforces after expiry (spec §1.2).
const DOTCELL_GRACE_SECONDS: u64 = 2_592_000;

fn dotcell_state(is_live: bool, expired_at: u64, tip_seconds: u64) -> &'static str {
    if !is_live {
        return "recycled";
    }
    if tip_seconds < expired_at {
        "active"
    } else if tip_seconds < expired_at.saturating_add(DOTCELL_GRACE_SECONDS) {
        "grace"
    } else {
        "free"
    }
}

/// The lock a 20-byte owner prefix resolved to: its hash and its script.
struct ResolvedLock {
    lock_hash: Vec<u8>,
    entry: LockScriptEntry,
}

struct DotCellPartyResolver<'a> {
    store: &'a CkbadgerStore,
    network: &'a str,
}

impl DotCellPartyResolver<'_> {
    /// Resolve one 20-byte owner/manager prefix. An ambiguous prefix (two
    /// known locks share it) is an internal error naming both, never a guess.
    fn resolve(&self, prefix: &[u8; 20]) -> Result<PartyRef, ApiRouteError> {
        Ok(self.resolve_with_lock(prefix)?.0)
    }

    /// The same resolution, keeping the lock script it resolved to.
    ///
    /// The sale check needs that script, and re-reading it by hash would be a
    /// second read of the row this one just returned — two reads that can only
    /// ever disagree if something is broken, and a disagreement the caller
    /// would then have to invent a meaning for.
    fn resolve_with_lock(
        &self,
        prefix: &[u8; 20],
    ) -> Result<(PartyRef, Option<ResolvedLock>), ApiRouteError> {
        let hash_prefix = format!("0x{}", hex::encode(prefix));
        let resolved = self
            .store
            .resolve_lock_hash_prefix(prefix)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let Some((lock_hash, entry)) = resolved else {
            return Ok((
                PartyRef {
                    hash_prefix,
                    lock_hash: None,
                    address: None,
                    script_name: None,
                },
                None,
            ));
        };
        let address = ckbadger_common::address::script_to_address(
            &entry.code_hash,
            entry.hash_type,
            &entry.args,
            self.network,
        )
        .ok();
        let script_name = self
            .store
            .get_script_info(&entry.code_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .and_then(|info| info.name);
        Ok((
            PartyRef {
                hash_prefix,
                lock_hash: Some(format!("0x{}", hex::encode(lock_hash))),
                address,
                script_name,
            },
            Some(ResolvedLock {
                lock_hash: lock_hash.to_vec(),
                entry,
            }),
        ))
    }

    /// Resolve a full 32-byte lock hash (a sale's seller).
    fn resolve_full(&self, lock_hash: &[u8; 32]) -> Result<PartyRef, ApiRouteError> {
        let entry = self
            .store
            .get_lock_script(lock_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (address, script_name) = match entry {
            Some(entry) => (
                ckbadger_common::address::script_to_address(
                    &entry.code_hash,
                    entry.hash_type,
                    &entry.args,
                    self.network,
                )
                .ok(),
                self.store
                    .get_script_info(&entry.code_hash)
                    .map_err(|e| ApiError::internal(e.to_string()))?
                    .and_then(|info| info.name),
            ),
            None => (None, None),
        };
        Ok(PartyRef {
            hash_prefix: format!("0x{}", hex::encode(&lock_hash[..20])),
            lock_hash: Some(format!("0x{}", hex::encode(lock_hash))),
            address,
            script_name,
        })
    }
}

fn dotcell_record_response(record: &ckbadger_store::types::DotCellRecord) -> DotCellRecordResponse {
    let value_utf8 = std::str::from_utf8(&record.value).ok().map(str::to_string);
    // Only `address.309` is a CKB address; the other `address.*` keys are BTC
    // and ETH addresses, which this decoder would reject anyway.
    let decoded_address = if record.key == "address.309" {
        value_utf8.as_deref().and_then(|value| {
            crate::utils::address::parse_address_to_script(value)
                .ok()
                .map(|script| DecodedAddressRef {
                    address: value.to_string(),
                    lock_hash: format!(
                        "0x{}",
                        hex::encode(crate::utils::address::compute_script_hash(
                            &script.code_hash,
                            script.hash_type,
                            &script.args,
                        ))
                    ),
                })
        })
    } else {
        None
    };
    DotCellRecordResponse {
        key: record.key.clone(),
        label: record.label.clone(),
        value_hex: format!("0x{}", hex::encode(&record.value)),
        value_utf8,
        ttl: record.ttl,
        decoded_address,
    }
}

fn dotcell_name_ref(
    identity_id: &[u8],
    entry: &ckbadger_store::types::IdentityEntry,
) -> Result<DotCellNameRef, ApiRouteError> {
    let label = match &entry.extra {
        ckbadger_store::types::IdentityExtra::DotCell { label, .. } => label.clone(),
        _ => {
            return Err(ApiError::internal(format!(
                ".cell identity has wrong extra variant: identity_id=0x{}",
                hex::encode(identity_id)
            )))
        }
    };
    Ok(DotCellNameRef {
        identity_id: format!("0x{}", hex::encode(identity_id)),
        name: format!("{label}.cell"),
        label,
    })
}

async fn get_dotcell_item_detail(
    State(state): State<Arc<AppState>>,
    Path(id_or_name): Path<String>,
) -> ApiResult<DotCellItemResponse> {
    let (identity_id, _label) = decode_dotcell_item_ref(&id_or_name)?;
    let store = state.store.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_identity(&identity_id))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(".cell name not found"))?;
    if entry.standard != ckbadger_store::types::IdentityStandard::DotCell {
        return Err(ApiError::bad_request("Item is not a .cell name"));
    }
    let (
        label,
        namespace_args,
        layout_version,
        expired_at,
        owner_hash20,
        manager_hash20,
        next_id,
        records_hash,
        records,
        parent_id,
    ) = match &entry.extra {
        ckbadger_store::types::IdentityExtra::DotCell {
            label,
            namespace_args,
            layout_version,
            expired_at,
            owner_hash20,
            manager_hash20,
            next_id,
            records_hash,
            records,
            parent_id,
        } => (
            label.clone(),
            *namespace_args,
            *layout_version,
            *expired_at,
            *owner_hash20,
            *manager_hash20,
            *next_id,
            *records_hash,
            records.clone(),
            *parent_id,
        ),
        _ => {
            return Err(ApiError::internal(format!(
                ".cell identity has wrong extra variant: identity_id=0x{}",
                hex::encode(identity_id)
            )))
        }
    };

    let store = state.store.as_ref();
    let resolver = DotCellPartyResolver {
        store,
        network: &state.ckb_network,
    };
    let (owner, owner_lock) = resolver.resolve_with_lock(&owner_hash20)?;
    let manager = resolver.resolve(&manager_hash20)?;

    // A name is for sale when its owner prefix IS a Sale Lock instance's hash
    // prefix — a property of the name, never of an offer cell's existence.
    //
    // The three cases are distinguished by what the owner's lock IS, never by
    // a read failing: the prefix resolves to no known lock at all; it resolves
    // to an ordinary lock; or it resolves to a Sale Lock, whose args must then
    // decode or the name's state is unknown and saying "not for sale" would be
    // a guess.
    let sale = match owner_lock {
        None => None,
        Some(ResolvedLock { entry, .. })
            if !ckbadger_indexer::parser::DotCellParser::is_sale_lock(&entry.code_hash) =>
        {
            None
        }
        Some(ResolvedLock { lock_hash, entry }) => {
            let (seller32, price) =
                ckbadger_indexer::parser::DotCellParser::parse_sale_lock_args(&entry.args)
                    .map_err(|e| {
                        ApiError::internal(format!(
                            "listed .cell name has malformed sale lock args: identity_id=0x{}, {e}",
                            hex::encode(identity_id)
                        ))
                    })?;
            let offer_out_point = store
                .list_cells_by_lock(&lock_hash, 1, None, state.append_only_store.as_ref())
                .map_err(|e| ApiError::internal(e.to_string()))?
                .into_iter()
                .next()
                .map(|(tx_hash, index, _)| DotCellOutPointResponse {
                    tx_hash: format!("0x{}", hex::encode(tx_hash)),
                    index,
                });
            Some(DotCellSaleResponse {
                price_shannons: price.to_string(),
                seller: resolver.resolve_full(&seller32)?,
                offer_out_point,
            })
        }
    };

    let parent = match parent_id {
        Some(parent_id) => store
            .get_identity(&parent_id)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map(|parent| dotcell_name_ref(&parent_id, &parent))
            .transpose()?,
        None => None,
    };

    let child_ids = store
        .list_identity_ids_by_collection(&ckbadger_store::keys::pad_id_32(&identity_id), None, 200)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut children = Vec::with_capacity(child_ids.len());
    for child_id in child_ids {
        let child = store
            .get_identity(&child_id)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    ".cell sub-name index points at a missing identity: identity_id=0x{}",
                    hex::encode(&child_id)
                ))
            })?;
        children.push(dotcell_name_ref(&child_id, &child)?);
    }

    let live_out_point = if entry.is_live {
        let outpoints = get_live_bit_cell_outpoints_by_identity_ids(
            store,
            state.append_only_store.as_ref(),
            std::slice::from_ref(&identity_id.to_vec()),
        )?;
        outpoints
            .get(identity_id.as_slice())
            .map(|(tx_hash, index)| DotCellOutPointResponse {
                tx_hash: format!("0x{}", hex::encode(tx_hash)),
                index: *index,
            })
    } else {
        None
    };

    // Expiry state is measured against the chain's own clock.
    let tip_seconds = store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map(|(_, header)| header.timestamp / 1000)
        .ok_or_else(|| {
            ApiError::internal(".cell expiry state needs a chain tip, and none is indexed")
        })?;
    let tip_seconds = u64::try_from(tip_seconds)
        .map_err(|_| ApiError::internal(format!("negative tip block timestamp: {tip_seconds}")))?;

    ok(DotCellItemResponse {
        identity_id: format!("0x{}", hex::encode(identity_id)),
        name: format!("{label}.cell"),
        label,
        is_live: entry.is_live,
        created_at_block: entry.created_at_block,
        created_at_tx: format!("0x{}", hex::encode(&entry.created_at_tx)),
        layout_version,
        namespace_args: format!("0x{}", hex::encode(namespace_args)),
        expired_at,
        state: dotcell_state(entry.is_live, expired_at, tip_seconds),
        grace_ends_at: expired_at.saturating_add(DOTCELL_GRACE_SECONDS),
        owner,
        manager,
        sale,
        records: records.iter().map(dotcell_record_response).collect(),
        records_hash: format!("0x{}", hex::encode(records_hash)),
        next_id: format!("0x{}", hex::encode(next_id)),
        parent,
        children,
        live_out_point,
    })
}

async fn list_dotcell_item_activities(
    State(state): State<Arc<AppState>>,
    Path(id_or_name): Path<String>,
    Query(params): Query<MnftItemActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<MnftItemActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let action_filter = normalize_activity_action_filter(params.action.as_deref())?;
    let (identity_id, _label) = decode_dotcell_item_ref(&id_or_name)?;
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let store = state.store.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_identity(&identity_id))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(".cell name not found"))?;
    if entry.standard != ckbadger_store::types::IdentityStandard::DotCell {
        return Err(ApiError::bad_request("Item is not a .cell name"));
    }

    let response = build_nft_item_activities_response(
        &state,
        &identity_id,
        NftLifecycleStandard::DotCell,
        limit,
        cursor,
        action_filter.as_deref(),
    )?;
    ok(response)
}

async fn get_dotcell_ring(State(state): State<Arc<AppState>>) -> ApiResult<DotCellRingResponse> {
    let store = state.store.clone();
    let rings = tokio::task::spawn_blocking(move || store.list_dotcell_rings())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if rings.len() > 1 {
        return Err(ApiError::internal(format!(
            "a network runs exactly one .cell namespace, found {}",
            rings.len()
        )));
    }
    let (namespace_args, root) = rings
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::not_found(".cell ring root not indexed on this network"))?;
    let live_count = state
        .store
        .get_identity_collection_aggregate(&DOTCELL_SENTINEL_COLLECTION)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map(|agg| agg.live_count)
        .unwrap_or(0);

    ok(DotCellRingResponse {
        namespace_args: format!("0x{}", hex::encode(namespace_args)),
        root_out_point: DotCellOutPointResponse {
            tx_hash: format!("0x{}", hex::encode(&root.root_tx_hash)),
            index: root.root_output_index,
        },
        first_id: format!("0x{}", hex::encode(root.first_id)),
        live_count,
    })
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // Registered before the generic `{collection_id}` routes so `ring` and
        // `items/{id_or_name}` are not read as a collection id.
        .route("/assets/identities/dotcell/ring", get(get_dotcell_ring))
        .route(
            "/assets/identities/dotcell/items/{id_or_name}",
            get(get_dotcell_item_detail),
        )
        .route(
            "/assets/identities/dotcell/items/{id_or_name}/activities",
            get(list_dotcell_item_activities),
        )
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
            "/assets/identities/bit-cell/items/{identity_id}",
            get(get_bit_cell_item_detail),
        )
        .route(
            "/assets/identities/bit-cell/items/{identity_id}/activities",
            get(list_bit_cell_item_activities),
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
    use ckbadger_store::types::{
        BIT_CELL_SENTINEL_COLLECTION, DID_CKB_SENTINEL_COLLECTION, DOTBIT_SENTINEL_COLLECTION,
    };

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

        let bit_cell = decode_identity_collection_id("bit_cell").unwrap();
        assert_eq!(bit_cell, BIT_CELL_SENTINEL_COLLECTION.to_vec());

        let bit_cell_alt = decode_identity_collection_id(".bit-cell").unwrap();
        assert_eq!(bit_cell_alt, BIT_CELL_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_decode_identity_collection_id_case_insensitive() {
        let result = decode_identity_collection_id("DotBit").unwrap();
        assert_eq!(result, DOTBIT_SENTINEL_COLLECTION.to_vec());

        let result = decode_identity_collection_id("DID:CKB").unwrap();
        assert_eq!(result, DID_CKB_SENTINEL_COLLECTION.to_vec());

        let result = decode_identity_collection_id("BIT-CELL").unwrap();
        assert_eq!(result, BIT_CELL_SENTINEL_COLLECTION.to_vec());
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
