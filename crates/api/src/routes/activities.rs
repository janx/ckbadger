use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::{
    types::{
        ActivityEntry, AssetAction, AssetChange, LatestActivityItem, LockCallEntry, ScriptInfo,
        TypeCallEntry,
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
    pub has_type_script: bool,
    pub asset_changes: Vec<AssetChangeResponse>,
    pub type_calls: Vec<TypeCallResponse>,
    pub lock_calls: Vec<LockCallResponse>,
    pub protocol_actions: Vec<ProtocolActionResponse>,
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
pub struct TypeCallResponse {
    pub type_code_hash: String,
    pub type_hash_type: String,
    pub type_args: String,
    pub script_hash: String,
    pub script_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockCallResponse {
    pub lock_code_hash: String,
    pub lock_hash_type: String,
    pub lock_args: String,
    pub script_hash: String,
    pub script_name: Option<String>,
    pub decoded: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolActionResponse {
    pub protocol: String,
    pub action: String,
    pub metadata: serde_json::Value,
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
    pub has_type_script: bool,
    pub asset_changes: Vec<AssetChangeResponse>,
    pub type_calls: Vec<TypeCallResponse>,
    pub lock_calls: Vec<LockCallResponse>,
    pub protocol_actions: Vec<ProtocolActionResponse>,
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
    code_hash: &[u8],
) -> anyhow::Result<Option<&'a ScriptInfo>> {
    if !cache.contains_key(code_hash) {
        cache.insert(code_hash.to_vec(), store.get_script_info(code_hash)?);
    }

    Ok(cache.get(code_hash).and_then(Option::as_ref))
}

fn convert_type_call(
    store: &CkbadgerStore,
    cache: &mut HashMap<Vec<u8>, Option<ScriptInfo>>,
    call: &TypeCallEntry,
) -> anyhow::Result<TypeCallResponse> {
    let hash_type = hash_type_to_str(call.type_hash_type).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported script hash_type {} in activity type_call",
            call.type_hash_type
        )
    })?;
    let script_hash = compute_script_hash(
        &call.type_code_hash,
        call.type_hash_type as u8,
        &call.type_args,
    );
    let script_info = resolve_script_info_cached(store, cache, &call.type_code_hash)?;

    Ok(TypeCallResponse {
        type_code_hash: format!("0x{}", hex::encode(&call.type_code_hash)),
        type_hash_type: hash_type.to_string(),
        type_args: format!("0x{}", hex::encode(&call.type_args)),
        script_hash: format!("0x{}", hex::encode(script_hash)),
        script_name: normalized_script_name(script_info),
    })
}

fn convert_type_calls(
    store: &CkbadgerStore,
    cache: &mut HashMap<Vec<u8>, Option<ScriptInfo>>,
    calls: Option<&Vec<TypeCallEntry>>,
) -> anyhow::Result<Vec<TypeCallResponse>> {
    calls
        .into_iter()
        .flatten()
        .map(|call| convert_type_call(store, cache, call))
        .collect()
}

type ArgsDecoder = fn(&[u8]) -> Option<serde_json::Value>;

/// Parse a 0x-prefixed hex string into bytes. Panics on invalid hex (compile-time constants only).
fn parse_hex_code_hash(hex_str: &str) -> Vec<u8> {
    let hex = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    hex::decode(hex).expect("invalid hex in lock args decoder constant")
}

static LOCK_ARGS_DECODERS: LazyLock<HashMap<Vec<u8>, ArgsDecoder>> = LazyLock::new(|| {
    let mut m: HashMap<Vec<u8>, ArgsDecoder> = HashMap::new();
    // RGB++ lock
    for hex in [
        "0xbc6c568a1a0d0a09f6844dc9d74ddb4343c32143ff25f727c59edf4fb72d6936", // mainnet
        "0x61ca7a4796a4eb19ca4f0d065cb9b10ddcf002f10f7cbb810c706cb6bb5c3248", // testnet
        "0xd07598deec7ce7b5665310386b4abd06a6d48843e953c5cc2112ad0d5a220364", // signet
    ] {
        m.insert(
            parse_hex_code_hash(hex),
            decode_rgbpp_lock_args as ArgsDecoder,
        );
    }
    // BTC time lock
    for hex in [
        "0x70d64497a075bd651e98ac030455ea200637ee325a12ad08aff03f1a117e5a62", // mainnet
        "0x00cdf8fab0f8ac638758ebf5ea5e4052b1d71e8a77b9f43139718621f6849326", // testnet
        "0x80a09eca26d77cea1f5a69471c59481be7404febf40ee90f886c36a948385b55", // signet
    ] {
        m.insert(
            parse_hex_code_hash(hex),
            decode_btc_time_lock_args as ArgsDecoder,
        );
    }
    // Fiber funding lock
    for hex in [
        "0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1", // mainnet
        "0x6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c", // testnet
    ] {
        m.insert(
            parse_hex_code_hash(hex),
            decode_fiber_funding_lock_args as ArgsDecoder,
        );
    }
    // Fiber commitment lock
    for hex in [
        "0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c", // mainnet
        "0x740dee83f87c6f309824d8fd3fbdd3c8380ee6fc9acc90b1a748438afcdf81d8", // testnet
    ] {
        m.insert(
            parse_hex_code_hash(hex),
            decode_fiber_commitment_lock_args as ArgsDecoder,
        );
    }
    // UTXOSwap intent lock
    for hex in [
        "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e", // mainnet
        "0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101", // testnet
    ] {
        m.insert(
            parse_hex_code_hash(hex),
            decode_utxoswap_intent_args as ArgsDecoder,
        );
    }
    m
});

fn decode_rgbpp_lock_args(args: &[u8]) -> Option<serde_json::Value> {
    if args.len() < 36 {
        return None;
    }
    let out_index = u32::from_le_bytes(args[0..4].try_into().ok()?);
    let mut btc_txid = args[4..36].to_vec();
    btc_txid.reverse(); // little-endian to big-endian for display
    Some(serde_json::json!({
        "protocol": "rgbpp",
        "btcTxid": hex::encode(&btc_txid),
        "outIndex": out_index,
    }))
}

fn decode_btc_time_lock_args(args: &[u8]) -> Option<serde_json::Value> {
    // BTC time lock args: btc_txid is always the last 32 bytes (LE).
    // Minimum 36 bytes (matching indexer parser).
    if args.len() < 36 {
        return None;
    }
    let mut btc_txid = args[args.len() - 32..].to_vec();
    btc_txid.reverse();
    Some(serde_json::json!({
        "protocol": "rgbpp",
        "action": "btcTimeLock",
        "btcTxid": hex::encode(&btc_txid),
    }))
}

fn decode_fiber_funding_lock_args(args: &[u8]) -> Option<serde_json::Value> {
    // Funding lock args: pubkey_hash (20 bytes minimum)
    if args.len() < 20 {
        return None;
    }
    Some(serde_json::json!({
        "protocol": "fiber",
        "action": "funding",
        "pubkeyHash": format!("0x{}", hex::encode(&args[0..20])),
    }))
}

fn decode_fiber_commitment_lock_args(args: &[u8]) -> Option<serde_json::Value> {
    // Commitment lock args layout (minimum 57 bytes):
    //   [0..20]   pubkey_hash (20 bytes)
    //   [20..28]  delay_epoch (8 bytes LE)
    //   [28..36]  version (8 bytes BE)
    //   [36..56]  settlement_hash (20 bytes)
    //   [56]      settlement_flag (1 byte)
    if args.len() < 57 {
        return None;
    }
    let pubkey_hash = &args[0..20];
    let delay_epoch = u64::from_le_bytes(args[20..28].try_into().ok()?);
    let version = u64::from_be_bytes(args[28..36].try_into().ok()?);
    let settlement_hash = &args[36..56];
    let settlement_flag = args[56];
    Some(serde_json::json!({
        "protocol": "fiber",
        "action": "commitment",
        "pubkeyHash": format!("0x{}", hex::encode(pubkey_hash)),
        "delayEpoch": delay_epoch,
        "version": version,
        "settlementHash": format!("0x{}", hex::encode(settlement_hash)),
        "settlementFlag": settlement_flag,
    }))
}

fn decode_utxoswap_intent_args(args: &[u8]) -> Option<serde_json::Value> {
    use ckbadger_indexer::parser::utxoswap::parse_intent_args;

    let parsed = parse_intent_args(args)?;

    let mut result = serde_json::json!({
        "protocol": "utxoswap",
        "intentType": parsed.intent_type.display_name(),
        "poolTypeHash": format!("0x{}", hex::encode(parsed.pool_type_hash)),
        "amountIn": parsed.amount_in.to_string(),
        "amountOutMin": parsed.amount_out_min.to_string(),
        "assetInIndex": parsed.asset_in_index,
    });

    if let Some(extra) = &parsed.create_pool_extra {
        result["assetX"] = serde_json::json!(format!("0x{}", hex::encode(extra.asset_x)));
        result["assetY"] = serde_json::json!(format!("0x{}", hex::encode(extra.asset_y)));
        result["amountX"] = serde_json::json!(extra.amount_x.to_string());
        result["amountY"] = serde_json::json!(extra.amount_y.to_string());
        result["totalFeeRate"] = serde_json::json!(extra.total_fee_rate);
    }

    Some(result)
}

fn convert_lock_call(
    store: &CkbadgerStore,
    cache: &mut HashMap<Vec<u8>, Option<ScriptInfo>>,
    call: &LockCallEntry,
) -> anyhow::Result<LockCallResponse> {
    let hash_type = hash_type_to_str(call.lock_hash_type).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported lock hash_type {} in activity lock_call",
            call.lock_hash_type
        )
    })?;
    let script_hash = compute_script_hash(
        &call.lock_code_hash,
        call.lock_hash_type as u8,
        &call.lock_args,
    );
    let script_info = resolve_script_info_cached(store, cache, &call.lock_code_hash)?;

    Ok(LockCallResponse {
        lock_code_hash: format!("0x{}", hex::encode(&call.lock_code_hash)),
        lock_hash_type: hash_type.to_string(),
        lock_args: format!("0x{}", hex::encode(&call.lock_args)),
        script_hash: format!("0x{}", hex::encode(script_hash)),
        script_name: normalized_script_name(script_info),
        decoded: LOCK_ARGS_DECODERS
            .get(call.lock_code_hash.as_slice())
            .and_then(|decoder| decoder(&call.lock_args)),
    })
}

fn convert_lock_calls(
    store: &CkbadgerStore,
    cache: &mut HashMap<Vec<u8>, Option<ScriptInfo>>,
    calls: Option<&Vec<LockCallEntry>>,
) -> anyhow::Result<Vec<LockCallResponse>> {
    calls
        .into_iter()
        .flatten()
        .map(|call| convert_lock_call(store, cache, call))
        .collect()
}

fn convert_protocol_action(
    action: &ckbadger_store::types::ProtocolAction,
) -> anyhow::Result<ProtocolActionResponse> {
    let metadata = action.metadata_value().map_err(|e| {
        anyhow::anyhow!(
            "failed to decode protocol metadata for protocol={} action={}: {}",
            action.protocol,
            action.action,
            e
        )
    })?;

    Ok(ProtocolActionResponse {
        protocol: action.protocol.clone(),
        action: action.action.clone(),
        metadata,
    })
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
        has_type_script: entry.has_type_script,
        asset_changes: entry
            .asset_changes
            .iter()
            .map(convert_asset_change)
            .collect(),
        type_calls: convert_type_calls(store, script_info_cache, entry.type_calls.as_ref())?,
        lock_calls: convert_lock_calls(store, script_info_cache, entry.lock_calls.as_ref())?,
        protocol_actions: entry
            .protocol_actions
            .iter()
            .map(convert_protocol_action)
            .collect::<anyhow::Result<Vec<_>>>()?,
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
        has_type_script: item.entry.has_type_script,
        asset_changes: item
            .entry
            .asset_changes
            .iter()
            .map(convert_asset_change)
            .collect(),
        type_calls: convert_type_calls(store, script_info_cache, item.entry.type_calls.as_ref())?,
        lock_calls: convert_lock_calls(store, script_info_cache, item.entry.lock_calls.as_ref())?,
        protocol_actions: item
            .entry
            .protocol_actions
            .iter()
            .map(convert_protocol_action)
            .collect::<anyhow::Result<Vec<_>>>()?,
        peers: item
            .entry
            .peers
            .iter()
            .map(|h| format!("0x{}", hex::encode(h)))
            .collect(),
    })
}

const ACTIVITY_SCAN_CHUNK_SIZE: usize = 128;

fn validate_activity_filter(filter: Option<&str>) -> Result<(), ApiRouteError> {
    if let Some(value) = filter {
        if !matches!(
            value,
            "all" | "ckb" | "token" | "nft" | "object" | "dao" | "type_call" | "lock_call"
        ) && !value.starts_with("protocol:")
        {
            return Err(ApiError::bad_request(format!(
                "invalid activity filter '{}'; expected one of: all, ckb, token, nft, dao, type_call, lock_call, protocol:<name>",
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

/// Check if an addr_tx entry is canonical using the same logic as
/// the transactions endpoint (`is_canonical_addr_tx` in cells.rs):
/// verify tx_hash location matches (block_num, tx_idx) in TX_HASH_MAP + TX_INDEX.
/// No block_hash comparison — position match is sufficient for canonicity.
fn is_canonical_activity(
    store: &CkbadgerStore,
    block_num: i64,
    tx_idx: i32,
    tx_hash: &[u8],
) -> anyhow::Result<bool> {
    let Some((canonical_block, canonical_tx_idx)) = store.get_tx_location(tx_hash)? else {
        return Ok(false);
    };
    if canonical_block != block_num || canonical_tx_idx != tx_idx {
        return Ok(false);
    }
    Ok(store
        .get_tx_index(canonical_block, canonical_tx_idx)?
        .is_some())
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
        let mut last_seen = None;
        for (block_num, tx_idx, entry) in rows {
            last_seen = Some((block_num, tx_idx));
            if entry.block_number == block_num
                && entry.tx_index == tx_idx
                && is_canonical_activity(canonical_store, block_num, tx_idx, &entry.tx_hash)?
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

    let filter = params.filter.clone();
    let store = state.store.clone();
    let (next_cursor, activities) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let results = list_canonical_activities_page(
            store.as_ref(),
            store.as_ref(),
            &lock_hash,
            limit + 1,
            cursor,
            filter.as_deref(),
        )?;

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
                build_activity_response(store.as_ref(), &entry, &mut script_info_cache)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok((next_cursor, activities))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
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
    let store = state.store.clone();
    let network = state.ckb_network.clone();
    let activities = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let items = store.get_latest_activities()?;

        // Filter to canonical entries only (same logic as address activities):
        // verify tx location matches (block_num, tx_idx) — no block_hash comparison.
        let mut script_info_cache = HashMap::new();
        let activities: Vec<GlobalActivityResponse> = items
            .into_iter()
            .filter(|item| {
                is_canonical_activity(
                    store.as_ref(),
                    item.entry.block_number,
                    item.entry.tx_index,
                    &item.entry.tx_hash,
                )
                .unwrap_or(false)
            })
            .take(limit)
            .map(|item| {
                build_global_activity_response(
                    store.as_ref(),
                    &network,
                    &item,
                    &mut script_info_cache,
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(activities)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
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
                type_calls: None,
                lock_calls: None,
                protocol_actions: vec![],
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
    fn test_validate_activity_filter_accepts_protocol_prefix() {
        assert!(validate_activity_filter(Some("protocol:rgbpp")).is_ok());
        assert!(validate_activity_filter(Some("protocol:fiber")).is_ok());
        assert!(validate_activity_filter(Some("invalid")).is_err());
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

    #[test]
    fn test_decode_rgbpp_lock_args_valid() {
        // out_index=2 (LE), btc_txid=32 bytes
        let mut args = vec![0; 36];
        args[0..4].copy_from_slice(&2u32.to_le_bytes());
        // btc_txid: stored little-endian, reversed for display
        for i in 0..32 {
            args[4 + i] = (32 - i) as u8;
        }
        let result = decode_rgbpp_lock_args(&args).unwrap();
        assert_eq!(result["protocol"], "rgbpp");
        assert_eq!(result["outIndex"], 2);
        // After reverse, should be 01020304...
        let txid = result["btcTxid"].as_str().unwrap();
        assert!(txid.starts_with("0102"));
    }

    #[test]
    fn test_decode_rgbpp_lock_args_too_short() {
        let args = vec![0; 35]; // 1 byte short
        assert!(decode_rgbpp_lock_args(&args).is_none());
    }

    #[test]
    fn test_decode_btc_time_lock_args_valid() {
        // btc_txid is the last 32 bytes (LE, reversed for display)
        let mut args = vec![0; 68];
        for i in 0..32 {
            args[36 + i] = (32 - i) as u8;
        }
        let result = decode_btc_time_lock_args(&args).unwrap();
        assert_eq!(result["protocol"], "rgbpp");
        assert_eq!(result["action"], "btcTimeLock");
        assert!(result.get("after").is_none());
        let txid = result["btcTxid"].as_str().unwrap();
        assert!(txid.starts_with("0102"));
    }

    #[test]
    fn test_decode_btc_time_lock_args_extracts_last_32_bytes() {
        // Verify "last 32 bytes" semantics with variable-length args
        let mut args = vec![0; 100];
        // Put txid in last 32 bytes (not at fixed offset 36..68)
        for i in 0..32 {
            args[68 + i] = (32 - i) as u8;
        }
        let result = decode_btc_time_lock_args(&args).unwrap();
        let txid = result["btcTxid"].as_str().unwrap();
        assert!(txid.starts_with("0102"));
    }

    #[test]
    fn test_decode_btc_time_lock_args_too_short() {
        let args = vec![0; 35]; // 1 byte short of minimum 36
        assert!(decode_btc_time_lock_args(&args).is_none());
    }

    #[test]
    fn test_decode_fiber_funding_lock_args_valid() {
        let mut args = vec![0u8; 20];
        for (i, byte) in args.iter_mut().enumerate() {
            *byte = (i + 1) as u8;
        }
        let result = decode_fiber_funding_lock_args(&args).unwrap();
        assert_eq!(result["protocol"], "fiber");
        assert_eq!(result["action"], "funding");
        let pubkey_hash = result["pubkeyHash"].as_str().unwrap();
        assert_eq!(pubkey_hash, "0x0102030405060708090a0b0c0d0e0f1011121314");
    }

    #[test]
    fn test_decode_fiber_funding_lock_args_longer_than_minimum() {
        // Funding lock args may be longer than 20 bytes; decoder uses first 20
        let mut args = vec![0xFFu8; 40];
        for (i, byte) in args[..20].iter_mut().enumerate() {
            *byte = (i + 1) as u8;
        }
        let result = decode_fiber_funding_lock_args(&args).unwrap();
        assert_eq!(result["protocol"], "fiber");
        let pubkey_hash = result["pubkeyHash"].as_str().unwrap();
        assert_eq!(pubkey_hash, "0x0102030405060708090a0b0c0d0e0f1011121314");
    }

    #[test]
    fn test_decode_fiber_funding_lock_args_too_short() {
        let args = vec![0; 19]; // 1 byte short of minimum 20
        assert!(decode_fiber_funding_lock_args(&args).is_none());
    }

    #[test]
    fn test_decode_fiber_commitment_lock_args_valid() {
        let mut args = vec![0u8; 57];
        // pubkey_hash [0..20]
        for (i, byte) in args[..20].iter_mut().enumerate() {
            *byte = (i + 1) as u8;
        }
        // delay_epoch [20..28] LE = 1000
        args[20..28].copy_from_slice(&1000u64.to_le_bytes());
        // version [28..36] BE = 1
        args[28..36].copy_from_slice(&1u64.to_be_bytes());
        // settlement_hash [36..56]
        for (i, byte) in args[36..56].iter_mut().enumerate() {
            *byte = (0xA0 + i) as u8;
        }
        // settlement_flag [56] = 1
        args[56] = 1;

        let result = decode_fiber_commitment_lock_args(&args).unwrap();
        assert_eq!(result["protocol"], "fiber");
        assert_eq!(result["action"], "commitment");
        assert_eq!(
            result["pubkeyHash"].as_str().unwrap(),
            "0x0102030405060708090a0b0c0d0e0f1011121314"
        );
        assert_eq!(result["delayEpoch"], 1000);
        assert_eq!(result["version"], 1);
        assert_eq!(
            result["settlementHash"].as_str().unwrap(),
            "0xa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3"
        );
        assert_eq!(result["settlementFlag"], 1);
    }

    #[test]
    fn test_decode_fiber_commitment_lock_args_too_short() {
        let args = vec![0; 56]; // 1 byte short of minimum 57
        assert!(decode_fiber_commitment_lock_args(&args).is_none());
    }

    #[test]
    fn test_fiber_locks_have_decoders() {
        // Verify Fiber code hashes are registered in LOCK_ARGS_DECODERS
        let funding_mainnet = parse_hex_code_hash(
            "0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1",
        );
        let commitment_mainnet = parse_hex_code_hash(
            "0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c",
        );
        assert!(LOCK_ARGS_DECODERS.contains_key(&funding_mainnet));
        assert!(LOCK_ARGS_DECODERS.contains_key(&commitment_mainnet));
    }

    #[test]
    fn test_decode_utxoswap_intent_swap() {
        let mut args = vec![0u8; 90];
        args[0..20].fill(0xAA);
        args[20..40].fill(0xBB);
        args[56] = 3; // SwapExactInputForOutput
        args[57] = 1; // asset_in_index
        args[58..74].copy_from_slice(&1_000_000u128.to_le_bytes());
        args[74..90].copy_from_slice(&500_000u128.to_le_bytes());

        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["protocol"], "utxoswap");
        assert_eq!(result["intentType"], "SwapExactInputForOutput");
        assert_eq!(result["assetInIndex"], 1);
        assert_eq!(result["amountIn"], "1000000");
        assert_eq!(result["amountOutMin"], "500000");
        assert!(result.get("assetX").is_none());
    }

    #[test]
    fn test_decode_utxoswap_intent_create_pool() {
        let mut args = vec![0u8; 154];
        args[0..20].fill(0xAA);
        args[20..40].fill(0xBB);
        args[56] = 0; // CreatePool
        args[57] = 30; // total_fee_rate
        args[58..90].fill(0xCC);
        args[90..122].fill(0xDD);
        args[122..138].copy_from_slice(&5_000u128.to_le_bytes());
        args[138..154].copy_from_slice(&10_000u128.to_le_bytes());

        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["protocol"], "utxoswap");
        assert_eq!(result["intentType"], "CreatePool");
        assert_eq!(result["totalFeeRate"], 30);
        assert_eq!(result["amountX"], "5000");
        assert_eq!(result["amountY"], "10000");
        assert!(result["assetX"].as_str().unwrap().starts_with("0x"));
        assert!(result["assetY"].as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn test_decode_utxoswap_intent_too_short() {
        let args = vec![0u8; 89];
        assert!(decode_utxoswap_intent_args(&args).is_none());
    }

    #[test]
    fn test_utxoswap_intent_locks_have_decoders() {
        let mainnet = parse_hex_code_hash(
            "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e",
        );
        let testnet = parse_hex_code_hash(
            "0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101",
        );
        assert!(LOCK_ARGS_DECODERS.contains_key(&mainnet));
        assert!(LOCK_ARGS_DECODERS.contains_key(&testnet));
    }
}
