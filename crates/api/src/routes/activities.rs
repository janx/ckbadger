use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::{
    types::{
        ActivityEntry, AssetAction, AssetChange, LatestActivityItem, ScriptCallEntry, ScriptInfo,
    },
    CkbadgerStore,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use crate::response::{
    default_limit, hash_type_to_str, ok, ApiError, ApiResult, ApiRouteError,
    CursorPaginatedResponse,
};
use crate::utils::address::{address_to_lock_script_hash, compute_script_hash, script_to_address};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/addresses/{addr}/activities", get(get_address_activities))
        .route("/activities/latest", get(get_latest_activities))
}

#[derive(Debug, Deserialize)]
pub struct ActivityParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    filter: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub ckb_delta: String,
    pub used_delta: String,
    pub is_cellbase: bool,
    pub asset_changes: Vec<AssetChangeResponse>,
    pub script_calls: Vec<ScriptCallResponse>,
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum AssetChangeResponse {
    #[serde(rename = "token", rename_all = "camelCase")]
    Token {
        type_script_hash: String,
        delta: String,
        symbol: Option<String>,
        decimals: Option<u8>,
    },
    #[serde(rename = "object", rename_all = "camelCase")]
    Object {
        object_id: String,
        standard: String,
        action: String,
    },
    #[serde(rename = "identity", rename_all = "camelCase")]
    Identity {
        identity_id: String,
        standard: String,
        action: String,
    },
    #[serde(rename = "daoDeposit")]
    DaoDeposit { capacity: String },
    #[serde(rename = "daoWithdrawRequest", rename_all = "camelCase")]
    DaoWithdrawRequest {
        capacity: String,
        deposit_block: i64,
    },
    #[serde(rename = "daoWithdrawComplete")]
    DaoWithdrawComplete {
        capacity: String,
        compensation: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCallResponse {
    pub type_code_hash: String,
    pub type_hash_type: String,
    pub type_args: String,
    pub script_hash: String,
    pub script_name: Option<String>,
    pub protocol_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalActivityResponse {
    pub address: String,
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub ckb_delta: String,
    pub used_delta: String,
    pub is_cellbase: bool,
    pub asset_changes: Vec<AssetChangeResponse>,
    pub script_calls: Vec<ScriptCallResponse>,
    pub peers: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct LatestActivityParams {
    #[serde(default = "default_latest_limit")]
    limit: usize,
}

fn default_latest_limit() -> usize {
    8
}

/// Reverse index: code_hash bytes → protocol name.
/// Built once from `docs/script-name-overrides.json` (same source as `assets.rs`).
static PROTOCOL_INDEX: LazyLock<HashMap<Vec<u8>, String>> = LazyLock::new(|| {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/script-name-overrides.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let doc: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };

    let mut index = HashMap::new();
    if let Some(protocols) = doc.get("protocols").and_then(|v| v.as_object()) {
        for (protocol_name, code_hashes) in protocols {
            if let Some(hashes) = code_hashes.as_array() {
                for hash_val in hashes {
                    if let Some(hex_str) = hash_val.as_str() {
                        let hex = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                        if let Ok(bytes) = hex::decode(hex) {
                            index.insert(bytes, protocol_name.clone());
                        }
                    }
                }
            }
        }
    }
    index
});

fn action_to_string(action: &AssetAction) -> String {
    match action {
        AssetAction::Mint => "mint".to_string(),
        AssetAction::Transfer => "transfer".to_string(),
        AssetAction::Burn => "burn".to_string(),
        AssetAction::Recycle => "recycle".to_string(),
        AssetAction::Renew => "renew".to_string(),
        AssetAction::Update => "update".to_string(),
    }
}

fn convert_asset_change(change: &AssetChange) -> AssetChangeResponse {
    match change {
        AssetChange::Token {
            type_script_hash,
            delta,
            symbol,
            decimals,
        } => AssetChangeResponse::Token {
            type_script_hash: format!("0x{}", hex::encode(type_script_hash)),
            delta: delta.to_string(),
            symbol: symbol.clone(),
            decimals: *decimals,
        },
        AssetChange::Object {
            object_id,
            standard,
            action,
        } => AssetChangeResponse::Object {
            object_id: format!("0x{}", hex::encode(object_id)),
            standard: standard.clone(),
            action: action_to_string(action),
        },
        AssetChange::Identity {
            identity_id,
            standard,
            action,
        } => AssetChangeResponse::Identity {
            identity_id: format!("0x{}", hex::encode(identity_id)),
            standard: standard.clone(),
            action: action_to_string(action),
        },
        AssetChange::DaoDeposit { capacity } => AssetChangeResponse::DaoDeposit {
            capacity: capacity.to_string(),
        },
        AssetChange::DaoWithdrawRequest {
            capacity,
            deposit_block,
        } => AssetChangeResponse::DaoWithdrawRequest {
            capacity: capacity.to_string(),
            deposit_block: *deposit_block,
        },
        AssetChange::DaoWithdrawComplete {
            capacity,
            compensation,
        } => AssetChangeResponse::DaoWithdrawComplete {
            capacity: capacity.to_string(),
            compensation: compensation.to_string(),
        },
    }
}

fn normalized_script_name(info: Option<&ScriptInfo>) -> Option<String> {
    info.and_then(|item| item.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.eq_ignore_ascii_case("unknown"))
        .map(|name| name.to_string())
}

fn resolve_script_info_cached<'a>(
    store: &CkbadgerStore,
    cache: &'a mut HashMap<Vec<u8>, Option<ScriptInfo>>,
    type_code_hash: &[u8],
) -> anyhow::Result<Option<&'a ScriptInfo>> {
    if !cache.contains_key(type_code_hash) {
        cache.insert(
            type_code_hash.to_vec(),
            store.get_script_info(type_code_hash)?,
        );
    }

    Ok(cache.get(type_code_hash).and_then(Option::as_ref))
}

fn convert_script_call(
    store: &CkbadgerStore,
    cache: &mut HashMap<Vec<u8>, Option<ScriptInfo>>,
    call: &ScriptCallEntry,
) -> anyhow::Result<ScriptCallResponse> {
    let hash_type = hash_type_to_str(call.type_hash_type);
    if hash_type == "unknown" {
        return Err(anyhow::anyhow!(
            "unsupported script hash_type {} in activity script_call",
            call.type_hash_type
        ));
    }
    let script_hash = compute_script_hash(
        &call.type_code_hash,
        call.type_hash_type as u8,
        &call.type_args,
    );
    let script_info = resolve_script_info_cached(store, cache, &call.type_code_hash)?;

    Ok(ScriptCallResponse {
        type_code_hash: format!("0x{}", hex::encode(&call.type_code_hash)),
        type_hash_type: hash_type.to_string(),
        type_args: format!("0x{}", hex::encode(&call.type_args)),
        script_hash: format!("0x{}", hex::encode(script_hash)),
        script_name: normalized_script_name(script_info),
        protocol_name: PROTOCOL_INDEX.get(&call.type_code_hash).cloned(),
    })
}

fn convert_script_calls(
    store: &CkbadgerStore,
    cache: &mut HashMap<Vec<u8>, Option<ScriptInfo>>,
    calls: Option<&Vec<ScriptCallEntry>>,
) -> anyhow::Result<Vec<ScriptCallResponse>> {
    calls
        .into_iter()
        .flatten()
        .map(|call| convert_script_call(store, cache, call))
        .collect()
}

pub(crate) fn build_activity_response(
    store: &CkbadgerStore,
    entry: &ActivityEntry,
    script_info_cache: &mut HashMap<Vec<u8>, Option<ScriptInfo>>,
) -> anyhow::Result<ActivityResponse> {
    Ok(ActivityResponse {
        tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
        block_number: entry.block_number,
        tx_index: entry.tx_index,
        timestamp: entry.timestamp.to_string(),
        ckb_delta: entry.ckb_delta.to_string(),
        used_delta: entry.used_delta.to_string(),
        is_cellbase: entry.is_cellbase,
        asset_changes: entry
            .asset_changes
            .iter()
            .map(convert_asset_change)
            .collect(),
        script_calls: convert_script_calls(store, script_info_cache, entry.script_calls.as_ref())?,
        peers: entry
            .peers
            .iter()
            .map(|h| format!("0x{}", hex::encode(h)))
            .collect(),
    })
}

pub(crate) fn build_global_activity_response(
    store: &CkbadgerStore,
    network: &str,
    item: &LatestActivityItem,
    script_info_cache: &mut HashMap<Vec<u8>, Option<ScriptInfo>>,
) -> anyhow::Result<GlobalActivityResponse> {
    let address = if !item.lock_code_hash.is_empty() {
        script_to_address(
            &item.lock_code_hash,
            item.lock_hash_type,
            &item.lock_args,
            network,
        )
        .unwrap_or_else(|_| format!("0x{}", hex::encode(&item.lock_hash)))
    } else {
        format!("0x{}", hex::encode(&item.lock_hash))
    };

    Ok(GlobalActivityResponse {
        address,
        tx_hash: format!("0x{}", hex::encode(&item.entry.tx_hash)),
        block_number: item.entry.block_number,
        tx_index: item.entry.tx_index,
        timestamp: item.entry.timestamp.to_string(),
        ckb_delta: item.entry.ckb_delta.to_string(),
        used_delta: item.entry.used_delta.to_string(),
        is_cellbase: item.entry.is_cellbase,
        asset_changes: item
            .entry
            .asset_changes
            .iter()
            .map(convert_asset_change)
            .collect(),
        script_calls: convert_script_calls(
            store,
            script_info_cache,
            item.entry.script_calls.as_ref(),
        )?,
        peers: item
            .entry
            .peers
            .iter()
            .map(|h| format!("0x{}", hex::encode(h)))
            .collect(),
    })
}

const ACTIVITY_SCAN_CHUNK_SIZE: usize = 128;
type CanonicalActivityLocation = (i64, i32, Vec<u8>);
type CanonicalActivityLocationMap = HashMap<Vec<u8>, CanonicalActivityLocation>;

fn validate_activity_filter(filter: Option<&str>) -> Result<(), ApiRouteError> {
    if let Some(value) = filter {
        if !matches!(
            value,
            "all" | "ckb" | "token" | "nft" | "dao" | "script_call"
        ) {
            return Err(ApiError::bad_request(format!(
                "invalid activity filter '{}'; expected one of: all, ckb, token, nft, dao, script_call",
                value
            )));
        }
    }
    Ok(())
}

fn parse_activity_cursor(value: &str) -> Option<(i64, i32)> {
    let parts: Vec<&str> = value.split(':').collect();
    match parts.as_slice() {
        // Current format: block_num:tx_idx
        [block_num, tx_idx] => Some((block_num.parse::<i64>().ok()?, tx_idx.parse::<i32>().ok()?)),
        _ => None,
    }
}

fn canonical_activity_locations(
    store: &CkbadgerStore,
    rows: &[(i64, i32, ActivityEntry)],
) -> anyhow::Result<CanonicalActivityLocationMap> {
    if rows.is_empty() {
        return Ok(HashMap::new());
    }
    let tx_hashes: Vec<Vec<u8>> = rows
        .iter()
        .map(|(_, _, entry)| entry.tx_hash.clone())
        .collect();
    let tx_batch = store.get_canonical_tx_identities_by_hash_batch(&tx_hashes)?;
    let mut out = HashMap::with_capacity(tx_batch.len());
    for (tx_hash, tx_row_opt) in tx_batch {
        if let Some((block_num, tx_idx, block_hash)) = tx_row_opt {
            out.insert(tx_hash, (block_num, tx_idx, block_hash));
        }
    }
    Ok(out)
}

fn list_canonical_activities_page(
    activity_store: &CkbadgerStore,
    canonical_store: &CkbadgerStore,
    lock_hash: &[u8],
    limit: usize,
    cursor: Option<(i64, i32)>,
    filter: Option<&str>,
) -> anyhow::Result<Vec<(i64, i32, ActivityEntry)>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let scan_limit = ACTIVITY_SCAN_CHUNK_SIZE.max(limit);
    let mut out = Vec::with_capacity(limit);
    let mut scan_cursor = cursor;

    loop {
        let rows = activity_store.list_activities(lock_hash, scan_limit, scan_cursor, filter)?;
        if rows.is_empty() {
            break;
        }
        let rows_len = rows.len();
        let canonical_locations = canonical_activity_locations(canonical_store, &rows)?;
        let mut last_seen = None;
        for (block_num, tx_idx, entry) in rows {
            last_seen = Some((block_num, tx_idx));
            if entry.block_number == block_num
                && entry.tx_index == tx_idx
                && canonical_locations.get(&entry.tx_hash)
                    == Some(&(block_num, tx_idx, entry.block_hash.clone()))
            {
                out.push((block_num, tx_idx, entry));
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
        if rows_len < scan_limit {
            break;
        }
        let Some(last_seen_cursor) = last_seen else {
            break;
        };
        scan_cursor = Some(last_seen_cursor);
    }

    Ok(out)
}

async fn get_address_activities(
    State(state): State<Arc<AppState>>,
    Path(addr): Path<String>,
    Query(params): Query<ActivityParams>,
) -> ApiResult<CursorPaginatedResponse<ActivityResponse>> {
    validate_activity_filter(params.filter.as_deref())?;
    let lock_hash = if addr.starts_with("ckb1") || addr.starts_with("ckt1") {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    let cursor = match params.cursor.as_deref() {
        None | Some("") => None,
        Some(c) => Some(
            parse_activity_cursor(c)
                .ok_or_else(|| ApiError::bad_request("invalid cursor format"))?,
        ),
    };

    let results = list_canonical_activities_page(
        state.store.as_ref(),
        state.store.as_ref(),
        &lock_hash,
        limit + 1,
        cursor,
        params.filter.as_deref(),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = results.len() > limit;
    let page: Vec<_> = results.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(block_num, tx_idx, _)| format!("{}:{}", block_num, tx_idx))
    } else {
        None
    };

    let mut script_info_cache = HashMap::new();
    let activities: Vec<ActivityResponse> = page
        .into_iter()
        .map(|(_, _, entry)| {
            build_activity_response(state.store.as_ref(), &entry, &mut script_info_cache)
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(CursorPaginatedResponse::without_total(
        activities,
        limit as i64,
        next_cursor,
    ))
}

async fn get_latest_activities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LatestActivityParams>,
) -> ApiResult<Vec<GlobalActivityResponse>> {
    let limit = params.limit.clamp(1, 64);
    let items = state
        .store
        .get_latest_activities()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let network = &state.ckb_network;
    let mut script_info_cache = HashMap::new();
    let activities: Vec<GlobalActivityResponse> = items
        .into_iter()
        .take(limit)
        .map(|item| {
            build_global_activity_response(
                state.store.as_ref(),
                network,
                &item,
                &mut script_info_cache,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(activities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::{
        CachedBlockHeader, OwnerActivityDelta, TxActivityBundle, TxIndexEntry,
    };

    fn make_header(hash_byte: u8) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![hash_byte; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        }
    }

    fn make_bundle_with_block_hash(
        tx_hash: &[u8],
        block_hash: &[u8],
        block_number: i64,
        tx_index: i32,
        lock_hash: &[u8],
    ) -> TxActivityBundle {
        TxActivityBundle {
            tx_hash: tx_hash.to_vec(),
            block_hash: block_hash.to_vec(),
            block_number,
            tx_index,
            timestamp: 1_700_000_000 + block_number,
            is_cellbase: false,
            owners: vec![OwnerActivityDelta {
                lock_hash: lock_hash.to_vec(),
                lock_code_hash: vec![0x11; 32],
                lock_hash_type: 1,
                lock_args: vec![0x22; 20],
                ckb_delta: 0,
                used_delta: 0,
                has_type_script: false,
                involved_script_code_hashes: vec![vec![0x33; 32]],
                asset_changes: vec![],
                script_calls: None,
                peers: vec![],
            }],
        }
    }

    #[test]
    fn test_validate_activity_filter_rejects_unknown() {
        let err = validate_activity_filter(Some("tok")).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::BAD_REQUEST);
        assert!(err.1 .0.message.contains("invalid activity filter"));
    }

    #[test]
    fn test_parse_activity_cursor_rejects_non_current_format() {
        assert_eq!(parse_activity_cursor("100:2"), Some((100, 2)));
        assert_eq!(parse_activity_cursor("100:2:7"), None);
        assert_eq!(parse_activity_cursor("100"), None);
    }

    #[test]
    fn test_list_canonical_activities_page_filters_orphaned_entries() {
        let root = tempfile::tempdir().unwrap();
        let domain_path = root.path().join("domain");
        let domain = CkbadgerStore::open_domain(&domain_path).unwrap();

        let lock_hash = [0xAA; 32];
        let stale_tx = vec![0x30; 32];
        let canonical_tx_new = vec![0x20; 32];
        let canonical_tx_old = vec![0x10; 32];

        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
        };

        let mut domain_batch = StoreBatch::new(&domain);
        let stale_bundle =
            make_bundle_with_block_hash(&stale_tx, &[0x60 | 30; 32], 30, 0, &lock_hash);
        let canonical_new_bundle =
            make_bundle_with_block_hash(&canonical_tx_new, &[0x60 | 20; 32], 20, 0, &lock_hash);
        let canonical_old_bundle =
            make_bundle_with_block_hash(&canonical_tx_old, &[0x60 | 10; 32], 10, 0, &lock_hash);
        domain_batch.put_tx_activity_bundle(&stale_bundle);
        domain_batch.put_tx_activity_bundle(&canonical_new_bundle);
        domain_batch.put_tx_activity_bundle(&canonical_old_bundle);
        domain_batch.put_addr_tx(&lock_hash, 30, 0, &stale_tx);
        domain_batch.put_addr_tx(&lock_hash, 20, 0, &canonical_tx_new);
        domain_batch.put_addr_tx(&lock_hash, 10, 0, &canonical_tx_old);
        // Simulate stale/orphan-like mapping without canonical tx_index entry.
        domain_batch.put_tx_hash_map(&stale_tx, 30, 0);
        domain_batch.put_tx_hash_map(&canonical_tx_new, 20, 0);
        domain_batch.put_tx_hash_map(&canonical_tx_old, 10, 0);
        domain_batch.put_tx_index(20, 0, &tx_index);
        domain_batch.put_tx_index(10, 0, &tx_index);
        domain_batch.put_block_header(30, &make_header(0x60 | 30));
        domain_batch.put_block_header(20, &make_header(0x60 | 20));
        domain_batch.put_block_header(10, &make_header(0x60 | 10));
        domain_batch.commit().unwrap();

        let rows =
            list_canonical_activities_page(&domain, &domain, &lock_hash, 3, None, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 20);
        assert_eq!(rows[1].0, 10);
        assert_eq!(rows[0].2.tx_hash, canonical_tx_new);
        assert_eq!(rows[1].2.tx_hash, canonical_tx_old);
    }

    #[test]
    fn test_list_canonical_activities_page_filters_competing_block_hash_history() {
        let root = tempfile::tempdir().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let lock_hash = [0x44; 32];
        let tx_hash = vec![0x77; 32];

        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
        };

        let mut domain_batch = StoreBatch::new(&domain);
        let first_bundle = make_bundle_with_block_hash(&tx_hash, &[0xAA; 32], 20, 0, &lock_hash);
        let second_bundle = make_bundle_with_block_hash(&tx_hash, &[0xBB; 32], 20, 0, &lock_hash);
        domain_batch.put_tx_activity_bundle(&first_bundle);
        domain_batch.put_tx_activity_bundle(&second_bundle);
        domain_batch.put_addr_tx(&lock_hash, 20, 0, &tx_hash);
        domain_batch.put_tx_hash_map(&tx_hash, 20, 0);
        domain_batch.put_tx_index(20, 0, &tx_index);
        domain_batch.put_block_header(20, &make_header(0xBB));
        domain_batch.commit().unwrap();

        let rows =
            list_canonical_activities_page(&domain, &domain, &lock_hash, 10, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].2.tx_hash, tx_hash);
        assert_eq!(rows[0].2.block_hash, vec![0xBB; 32]);
    }
}
