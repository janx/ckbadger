use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::{
    types::{
        ItemDelta, LockCallEntry, ParticipantDelta, ScriptInfo, TxActions, TypeCallEntry,
        ITEM_KIND_IDENTITY, ITEM_KIND_OBJECT, ITEM_KIND_TOKEN,
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

/// Resolve a lock_hash to a CKB address using the persistent lock script mapping.
/// Falls back to hex-encoded lock_hash if no mapping exists.
fn resolve_lock_hash_address(
    store: &CkbadgerStore,
    _ao_store: &CkbadgerStore,
    network: &str,
    lock_hash: &[u8],
    cache: &mut HashMap<Vec<u8>, String>,
) -> String {
    if let Some(cached) = cache.get(lock_hash) {
        return cached.clone();
    }
    let address = store
        .get_lock_script(lock_hash)
        .ok()
        .flatten()
        .and_then(|entry| {
            script_to_address(&entry.code_hash, entry.hash_type, &entry.args, network).ok()
        })
        .unwrap_or_else(|| format!("0x{}", hex::encode(lock_hash)));
    cache.insert(lock_hash.to_vec(), address.clone());
    address
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/addresses/{addr}/activities", get(get_address_activities))
        .route("/activities", get(get_global_activities))
        .route("/activities/latest", get(get_latest_activities))
}

#[derive(Debug, Deserialize)]
pub struct ActivityParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    filter: Option<String>,
}

/// Per-address activity response: shows one participant's perspective of a transaction.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    // This participant's Layer 1
    pub ckb_delta: String,
    pub used_delta: String,
    pub is_cellbase: bool,
    // This participant's Layer 2
    pub item_deltas: Vec<ItemDeltaResponse>,
    // TX-level Layer 3
    pub type_calls: Vec<TypeCallResponse>,
    pub lock_calls: Vec<LockCallResponse>,
    pub protocol_actions: Vec<ProtocolActionResponse>,
    // Other participants
    pub participants: Vec<String>,
    pub tags: u16,
}

/// Item delta response — tagged enum keyed on `kind`.
///
/// NOTE: `rename_all` on an internally-tagged enum only renames the tag values,
/// NOT the fields inside each variant. Each variant needs its own `rename_all`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum ItemDeltaResponse {
    #[serde(rename = "token", rename_all = "camelCase")]
    Token {
        type_script_hash: String,
        delta: String,
        symbol: Option<String>,
        decimals: Option<u8>,
    },
    #[serde(rename = "object", rename_all = "camelCase")]
    Object { object_id: String, delta: i8 },
    #[serde(rename = "identity", rename_all = "camelCase")]
    Identity { identity_id: String, delta: i8 },
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

/// Global activity response: shows all participants in a transaction.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub is_cellbase: bool,
    // TX-level Layer 3
    pub protocol_actions: Vec<ProtocolActionResponse>,
    pub type_calls: Vec<TypeCallResponse>,
    pub lock_calls: Vec<LockCallResponse>,
    // All participants with their data
    pub participants: Vec<ParticipantResponse>,
}

/// A single participant's delta within a global activity response.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantResponse {
    pub address: String,
    pub ckb_delta: String,
    pub used_delta: String,
    pub item_deltas: Vec<ItemDeltaResponse>,
    pub tags: u16,
}

#[derive(Debug, Deserialize)]
pub struct LatestActivityParams {
    #[serde(default = "default_latest_limit")]
    limit: usize,
}

fn default_latest_limit() -> usize {
    8
}

/// Convert an `ItemDelta` to `ItemDeltaResponse`, enriching tokens with symbol/decimals.
#[allow(clippy::type_complexity)]
fn convert_item_delta(
    item: &ItemDelta,
    token_cache: &mut HashMap<Vec<u8>, Option<(Option<String>, Option<u8>)>>,
    store: &CkbadgerStore,
) -> ItemDeltaResponse {
    match item.kind {
        ITEM_KIND_TOKEN => {
            let (symbol, decimals) = lookup_token_info(store, token_cache, &item.item_id);
            ItemDeltaResponse::Token {
                type_script_hash: format!("0x{}", hex::encode(&item.item_id)),
                delta: item.delta.to_string(),
                symbol,
                decimals,
            }
        }
        ITEM_KIND_OBJECT => ItemDeltaResponse::Object {
            object_id: format!("0x{}", hex::encode(&item.item_id)),
            delta: item.delta as i8,
        },
        ITEM_KIND_IDENTITY => ItemDeltaResponse::Identity {
            identity_id: format!("0x{}", hex::encode(&item.item_id)),
            delta: item.delta as i8,
        },
        _ => {
            // Unknown kind — treat as token for forward compatibility
            ItemDeltaResponse::Token {
                type_script_hash: format!("0x{}", hex::encode(&item.item_id)),
                delta: item.delta.to_string(),
                symbol: None,
                decimals: None,
            }
        }
    }
}

/// Look up token symbol and decimals from CF_TOKENS, caching results.
#[allow(clippy::type_complexity)]
fn lookup_token_info(
    store: &CkbadgerStore,
    cache: &mut HashMap<Vec<u8>, Option<(Option<String>, Option<u8>)>>,
    type_script_hash: &[u8],
) -> (Option<String>, Option<u8>) {
    if let Some(cached) = cache.get(type_script_hash) {
        return cached
            .as_ref()
            .map(|(s, d)| (s.clone(), *d))
            .unwrap_or((None, None));
    }
    let result = store
        .get_token(type_script_hash)
        .ok()
        .flatten()
        .map(|t| (t.symbol.clone(), t.decimals.map(|d| d as u8)));
    cache.insert(type_script_hash.to_vec(), result.clone());
    result.unwrap_or((None, None))
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
    calls: &[TypeCallEntry],
) -> anyhow::Result<Vec<TypeCallResponse>> {
    calls
        .iter()
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
    // Commitment lock args layout:
    //   Short (36B): pubkey_hash(20) + delay_epoch(8 LE) + version(8 BE)
    //   Full (57B):  short + settlement_hash(20) + settlement_flag(1)
    if args.len() < 36 {
        return None;
    }
    let pubkey_hash = &args[0..20];
    let delay_epoch = u64::from_le_bytes(args[20..28].try_into().ok()?);
    let version = u64::from_be_bytes(args[28..36].try_into().ok()?);

    let mut obj = serde_json::json!({
        "protocol": "fiber",
        "action": "commitment",
        "pubkeyHash": format!("0x{}", hex::encode(pubkey_hash)),
        "delayEpoch": delay_epoch,
        "version": version,
    });

    if args.len() >= 57 {
        let settlement_hash = &args[36..56];
        let settlement_flag = args[56];
        obj["settlementHash"] = serde_json::json!(format!("0x{}", hex::encode(settlement_hash)));
        obj["settlementFlag"] = serde_json::json!(settlement_flag);
    }

    Some(obj)
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
    calls: &[LockCallEntry],
) -> anyhow::Result<Vec<LockCallResponse>> {
    calls
        .iter()
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

/// Build an address-scoped activity response from a TxActions for a specific participant.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn build_activity_response(
    store: &CkbadgerStore,
    ao_store: &CkbadgerStore,
    network: &str,
    actions: &TxActions,
    participant: &ParticipantDelta,
    script_info_cache: &mut HashMap<Vec<u8>, Option<ScriptInfo>>,
    token_cache: &mut HashMap<Vec<u8>, Option<(Option<String>, Option<u8>)>>,
    address_cache: &mut HashMap<Vec<u8>, String>,
) -> anyhow::Result<ActivityResponse> {
    let item_deltas = participant
        .item_deltas
        .iter()
        .map(|item| convert_item_delta(item, token_cache, store))
        .collect();

    let participants = actions
        .participants
        .iter()
        .filter(|p| p.lock_hash != participant.lock_hash)
        .map(|p| resolve_lock_hash_address(store, ao_store, network, &p.lock_hash, address_cache))
        .collect();

    Ok(ActivityResponse {
        tx_hash: format!("0x{}", hex::encode(&actions.tx_hash)),
        block_number: actions.block_number,
        tx_index: actions.tx_index,
        timestamp: actions.timestamp.to_string(),
        ckb_delta: participant.ckb_delta.to_string(),
        used_delta: participant.used_delta.to_string(),
        is_cellbase: actions.is_cellbase,
        item_deltas,
        type_calls: convert_type_calls(store, script_info_cache, &actions.type_calls)?,
        lock_calls: convert_lock_calls(store, script_info_cache, &actions.lock_calls)?,
        protocol_actions: actions
            .protocol_actions
            .iter()
            .map(convert_protocol_action)
            .collect::<anyhow::Result<Vec<_>>>()?,
        participants,
        tags: participant.tags,
    })
}

/// Build a global activity response showing all participants from a TxActions.
#[allow(clippy::type_complexity)]
pub(crate) fn build_global_activity_response(
    store: &CkbadgerStore,
    ao_store: &CkbadgerStore,
    network: &str,
    actions: &TxActions,
    script_info_cache: &mut HashMap<Vec<u8>, Option<ScriptInfo>>,
    address_cache: &mut HashMap<Vec<u8>, String>,
) -> anyhow::Result<GlobalActivityResponse> {
    let mut token_cache: HashMap<Vec<u8>, Option<(Option<String>, Option<u8>)>> = HashMap::new();

    let participants = actions
        .participants
        .iter()
        .map(|p| {
            let address =
                resolve_lock_hash_address(store, ao_store, network, &p.lock_hash, address_cache);
            let item_deltas = p
                .item_deltas
                .iter()
                .map(|item| convert_item_delta(item, &mut token_cache, store))
                .collect();
            ParticipantResponse {
                address,
                ckb_delta: p.ckb_delta.to_string(),
                used_delta: p.used_delta.to_string(),
                item_deltas,
                tags: p.tags,
            }
        })
        .collect();

    Ok(GlobalActivityResponse {
        tx_hash: format!("0x{}", hex::encode(&actions.tx_hash)),
        block_number: actions.block_number,
        tx_index: actions.tx_index,
        timestamp: actions.timestamp.to_string(),
        is_cellbase: actions.is_cellbase,
        protocol_actions: actions
            .protocol_actions
            .iter()
            .map(convert_protocol_action)
            .collect::<anyhow::Result<Vec<_>>>()?,
        type_calls: convert_type_calls(store, script_info_cache, &actions.type_calls)?,
        lock_calls: convert_lock_calls(store, script_info_cache, &actions.lock_calls)?,
        participants,
    })
}

const ACTIVITY_SCAN_CHUNK_SIZE: usize = 128;

fn validate_activity_filter(filter: Option<&str>) -> Result<(), ApiRouteError> {
    if let Some(value) = filter {
        if !matches!(
            value,
            "all"
                | "ckb"
                | "token"
                | "nft"
                | "object"
                | "identity"
                | "dao"
                | "type_call"
                | "lock_call"
        ) && !value.starts_with("protocol:")
        {
            return Err(ApiError::bad_request(format!(
                "invalid activity filter '{}'; expected one of: all, ckb, token, nft, object, identity, dao, type_call, lock_call, protocol:<name>",
                value
            )));
        }
    }
    Ok(())
}

fn validate_global_activity_filter(filter: Option<&str>) -> Result<(), ApiRouteError> {
    if let Some(value) = filter {
        if value.is_empty() {
            return Ok(());
        }
        if !matches!(
            value,
            "all" | "ckb" | "token" | "object" | "identity" | "dao" | "script" | "protocol"
        ) {
            return Err(ApiError::bad_request(format!(
                "invalid global activity filter '{}'; expected one of: all, ckb, token, object, identity, dao, script, protocol",
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
    store: &CkbadgerStore,
    lock_hash: &[u8],
    limit: usize,
    cursor: Option<(i64, i32)>,
    filter: Option<&str>,
) -> anyhow::Result<Vec<TxActions>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let scan_limit = ACTIVITY_SCAN_CHUNK_SIZE.max(limit);
    let mut out = Vec::with_capacity(limit);
    let mut scan_cursor = cursor;

    loop {
        let rows = store.list_activities(lock_hash, scan_limit, scan_cursor, filter)?;
        if rows.is_empty() {
            break;
        }
        let rows_len = rows.len();
        let mut last_seen = None;
        for actions in rows {
            last_seen = Some((actions.block_number, actions.tx_index));
            if is_canonical_activity(
                store,
                actions.block_number,
                actions.tx_index,
                &actions.tx_hash,
            )? {
                out.push(actions);
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

/// Classify a TxActions for global activity filtering based on aggregate participant tags.
fn matches_global_activity_filter(actions: &TxActions, filter: Option<&str>) -> bool {
    use ckbadger_store::types::*;

    fn classify_global_bucket(actions: &TxActions) -> &'static str {
        // DAO operations are emitted as protocol actions with protocol="dao";
        // check before the generic protocol bucket so filter=dao still works.
        if actions.protocol_actions.iter().any(|a| a.protocol == "dao") {
            return "dao";
        }
        if !actions.protocol_actions.is_empty() {
            return "protocol";
        }
        let combined_tags: u16 = actions
            .participants
            .iter()
            .fold(0u16, |acc, p| acc | p.tags);
        if combined_tags & TAG_DAO != 0 {
            return "dao";
        }
        if combined_tags & TAG_TOKEN != 0 {
            return "token";
        }
        if combined_tags & TAG_OBJECT != 0 {
            return "object";
        }
        if combined_tags & TAG_IDENTITY != 0 {
            return "identity";
        }
        if combined_tags & (TAG_TYPE_CALL | TAG_LOCK_CALL) != 0
            || !actions.type_calls.is_empty()
            || !actions.lock_calls.is_empty()
        {
            return "script";
        }
        "ckb"
    }

    match filter {
        None | Some("") | Some("all") => true,
        Some(expected) => classify_global_bucket(actions) == expected,
    }
}

/// List canonical global activities (TX-level), applying filter and canonicity checks.
fn list_canonical_global_activities_page(
    store: &CkbadgerStore,
    limit: usize,
    cursor: Option<(i64, i32)>,
    filter: Option<&str>,
) -> anyhow::Result<Vec<TxActions>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let scan_limit = ACTIVITY_SCAN_CHUNK_SIZE.max(limit);
    let mut out = Vec::with_capacity(limit);
    let mut scan_cursor = cursor;

    loop {
        let rows = store.list_tx_actions_recent(scan_limit, scan_cursor)?;
        if rows.is_empty() {
            break;
        }
        let rows_len = rows.len();
        let mut last_seen = None;
        for actions in rows {
            last_seen = Some((actions.block_number, actions.tx_index));
            if actions.is_cellbase {
                continue;
            }
            if !is_canonical_activity(
                store,
                actions.block_number,
                actions.tx_index,
                &actions.tx_hash,
            )? {
                continue;
            }
            if !matches_global_activity_filter(&actions, filter) {
                continue;
            }
            out.push(actions);
            if out.len() >= limit {
                return Ok(out);
            }
        }
        if rows_len < scan_limit {
            break;
        }
        let Some(next_cursor) = last_seen else {
            break;
        };
        scan_cursor = Some(next_cursor);
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
    let ao_store = state.append_only_store.clone();
    let network = state.ckb_network.clone();
    let lock_hash_clone = lock_hash.clone();
    let (next_cursor, activities) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let results = list_canonical_activities_page(
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
                .map(|actions| format!("{}:{}", actions.block_number, actions.tx_index))
        } else {
            None
        };

        let mut script_info_cache = HashMap::new();
        let mut token_cache = HashMap::new();
        let mut address_cache = HashMap::new();
        let activities: Vec<ActivityResponse> = page
            .iter()
            .filter_map(|actions| {
                let participant = actions
                    .participants
                    .iter()
                    .find(|p| p.lock_hash == lock_hash_clone)?;
                Some(build_activity_response(
                    store.as_ref(),
                    ao_store.as_ref(),
                    &network,
                    actions,
                    participant,
                    &mut script_info_cache,
                    &mut token_cache,
                    &mut address_cache,
                ))
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

async fn get_global_activities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActivityParams>,
) -> ApiResult<CursorPaginatedResponse<GlobalActivityResponse>> {
    validate_global_activity_filter(params.filter.as_deref())?;

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor = match params.cursor.as_deref() {
        None | Some("") => None,
        Some(value) => Some(
            parse_activity_cursor(value)
                .ok_or_else(|| ApiError::bad_request("invalid cursor format"))?,
        ),
    };

    let store = state.store.clone();
    let ao_store = state.append_only_store.clone();
    let network = state.ckb_network.clone();
    let filter = params.filter.clone();
    let (next_cursor, activities) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let results = list_canonical_global_activities_page(
            store.as_ref(),
            limit + 1,
            cursor,
            filter.as_deref(),
        )?;

        let has_more = results.len() > limit;
        let page: Vec<_> = results.into_iter().take(limit).collect();
        let next_cursor = if has_more {
            page.last()
                .map(|actions| format!("{}:{}", actions.block_number, actions.tx_index))
        } else {
            None
        };

        let mut script_info_cache = HashMap::new();
        let mut address_cache = HashMap::new();
        let activities = page
            .iter()
            .map(|actions| {
                build_global_activity_response(
                    store.as_ref(),
                    ao_store.as_ref(),
                    &network,
                    actions,
                    &mut script_info_cache,
                    &mut address_cache,
                )
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
    let ao_store = state.append_only_store.clone();
    let network = state.ckb_network.clone();
    let activities = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let items = store.get_latest_activities()?;

        // Filter to canonical entries only:
        // verify tx location matches (block_num, tx_idx) — no block_hash comparison.
        let mut script_info_cache = HashMap::new();
        let mut address_cache = HashMap::new();
        let activities: Vec<GlobalActivityResponse> = items
            .iter()
            .filter(|actions| {
                is_canonical_activity(
                    store.as_ref(),
                    actions.block_number,
                    actions.tx_index,
                    &actions.tx_hash,
                )
                .unwrap_or(false)
            })
            .take(limit)
            .map(|actions| {
                build_global_activity_response(
                    store.as_ref(),
                    ao_store.as_ref(),
                    &network,
                    actions,
                    &mut script_info_cache,
                    &mut address_cache,
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
    fn test_validate_global_activity_filter_only_accepts_all() {
        assert!(validate_global_activity_filter(None).is_ok());
        assert!(validate_global_activity_filter(Some("")).is_ok());
        assert!(validate_global_activity_filter(Some("all")).is_ok());
        assert!(validate_global_activity_filter(Some("token")).is_ok());
        assert!(validate_global_activity_filter(Some("object")).is_ok());
        assert!(validate_global_activity_filter(Some("identity")).is_ok());
        assert!(validate_global_activity_filter(Some("dao")).is_ok());
        assert!(validate_global_activity_filter(Some("script")).is_ok());
        assert!(validate_global_activity_filter(Some("protocol")).is_ok());
        assert!(validate_global_activity_filter(Some("nft")).is_err());
    }

    #[test]
    fn test_parse_activity_cursor_format() {
        assert_eq!(parse_activity_cursor("100:2"), Some((100, 2)));
        assert_eq!(parse_activity_cursor("100:2:7"), None);
        assert_eq!(parse_activity_cursor("100"), None);
    }

    // Integration tests for activities are in crates/api/tests/api_integration.rs

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
        let args = vec![0; 35]; // 1 byte short of minimum 36
        assert!(decode_fiber_commitment_lock_args(&args).is_none());
    }

    #[test]
    fn test_decode_fiber_commitment_lock_args_short_format() {
        // 36-byte short format: no settlement fields
        let mut args = vec![0u8; 36];
        args[0..20].copy_from_slice(&[0x11; 20]); // pubkey_hash
        args[20..28].copy_from_slice(&42u64.to_le_bytes()); // delay_epoch
        args[28..36].copy_from_slice(&3u64.to_be_bytes()); // version

        let result = decode_fiber_commitment_lock_args(&args).unwrap();
        assert_eq!(result["delayEpoch"], 42);
        assert_eq!(result["version"], 3);
        assert!(result.get("settlementHash").is_none());
        assert!(result.get("settlementFlag").is_none());
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

    #[test]
    fn test_item_delta_response_json_format() {
        let token = ItemDeltaResponse::Token {
            type_script_hash: "0xabcdef1234".to_string(),
            delta: "100".to_string(),
            symbol: None,
            decimals: None,
        };
        let json = serde_json::to_string(&token).unwrap();
        assert!(
            json.contains("\"typeScriptHash\""),
            "missing typeScriptHash: {}",
            json
        );
        assert!(
            json.contains("\"kind\":\"token\""),
            "missing kind: {}",
            json
        );

        let object = ItemDeltaResponse::Object {
            object_id: "0xabcdef".to_string(),
            delta: 1,
        };
        let json = serde_json::to_string(&object).unwrap();
        assert!(json.contains("\"objectId\""), "missing objectId: {}", json);
        assert!(
            json.contains("\"kind\":\"object\""),
            "missing kind: {}",
            json
        );

        let identity = ItemDeltaResponse::Identity {
            identity_id: "0xabcdef".to_string(),
            delta: -1,
        };
        let json = serde_json::to_string(&identity).unwrap();
        assert!(
            json.contains("\"identityId\""),
            "missing identityId: {}",
            json
        );
        assert!(
            json.contains("\"kind\":\"identity\""),
            "missing kind: {}",
            json
        );
    }
}
